use portaweave_native::*;
use std::{
    fs,
    path::PathBuf,
    ptr,
    sync::{
        Arc, Barrier,
        atomic::{AtomicU64, Ordering},
    },
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture {
    path: PathBuf,
    volume: u64,
}
impl Fixture {
    fn new(version: u8) -> Self {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "pw-dir-item-pin-{}-{stamp}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let bytes = path.to_str().unwrap().as_bytes();
        let a = PwCreateAttributes {
            uid: 100,
            gid: 20,
            mode: 0o750,
            reserved: 0,
        };
        let mut volume = 0;
        let rc = unsafe {
            match version {
                1 => pw_volume_create(bytes.as_ptr(), bytes.len(), 1 << 24, &mut volume),
                2 => pw_volume_create_with_metadata(
                    bytes.as_ptr(),
                    bytes.len(),
                    1 << 24,
                    &a,
                    &mut volume,
                ),
                _ => pw_volume_create_with_directory_lifetimes(
                    bytes.as_ptr(),
                    bytes.len(),
                    1 << 24,
                    &a,
                    &mut volume,
                ),
            }
        };
        assert_eq!(rc, 0);
        Self { path, volume }
    }
    fn create(&self, name: &str) -> PwFsKitDirectoryCreated {
        let mut out = sentinel();
        assert_eq!(
            unsafe {
                pw_fskit_directory_create_with_state(
                    self.volume,
                    1,
                    name.as_ptr(),
                    name.len(),
                    ptr::null(),
                    &PwCreateState::default(),
                    &mut out,
                )
            },
            0
        );
        out
    }
    fn healthy(&self) {
        let (mut m, mut p) = (99, 99);
        assert_eq!(unsafe { pw_health(self.volume, &mut m, &mut p) }, 0);
        assert_eq!((m, p), (0, 0));
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            assert_eq!(pw_volume_close(self.volume), 0);
            fs::remove_dir_all(&self.path).unwrap();
        }
    }
}
fn sentinel() -> PwFsKitDirectoryCreated {
    PwFsKitDirectoryCreated {
        object: 77,
        directory: 88,
        attributes: PwFsKitAttributes {
            file_id: 99,
            kind: 7,
            ..Default::default()
        },
    }
}
fn caller() -> PwCredentials {
    PwCredentials {
        uid: 100,
        gid: 20,
        groups: ptr::null(),
        group_count: 0,
        reserved: 0,
    }
}

#[test]
fn directory_item_pins_are_v3_only_and_administrative_not_policy_or_file_tokens() {
    for version in [1, 2] {
        let f = Fixture::new(version);
        let mut out = 99;
        assert_eq!(unsafe { pw_directory_open(f.volume, 1, &mut out) }, -11);
        assert_eq!(out, 99);
        let mut made = sentinel();
        assert_eq!(
            unsafe {
                pw_fskit_directory_create_with_state(
                    f.volume,
                    1,
                    b"absent".as_ptr(),
                    6,
                    ptr::null(),
                    &PwCreateState::default(),
                    &mut made,
                )
            },
            -11
        );
        assert_eq!(made, sentinel());
        assert_eq!(
            unsafe { pw_lookup(f.volume, 1, b"absent".as_ptr(), 6, &mut made.object) },
            -2
        );
        f.healthy();
    }
    let f = Fixture::new(3);
    let foreign = Fixture::new(3);
    let made = f.create("dir");
    let mut file_id = 0;
    let mut file_token = 0;
    assert_eq!(
        unsafe { pw_file_create(f.volume, 1, b"file".as_ptr(), 4, &mut file_id) },
        0
    );
    assert_eq!(
        unsafe { pw_file_open(f.volume, file_id, &mut file_token) },
        0
    );
    assert_eq!(pw_directory_close(f.volume, file_token), -1);
    let mut refused = 99;
    assert_eq!(
        unsafe { pw_directory_open(f.volume, file_id, &mut refused) },
        -4
    );
    assert_eq!(refused, 99);
    assert_eq!(pw_file_close(f.volume, file_token), 0);
    assert_eq!(pw_volume_close(f.volume), -7);
    assert_eq!(pw_directory_close(foreign.volume, made.directory), -1);
    assert_eq!(pw_file_close(f.volume, made.directory), -1);
    assert_eq!(pw_posix_directory_close(f.volume, made.directory), -1);
    let mut info = PwFileInfo {
        object: 99,
        ..Default::default()
    };
    let saved = info;
    assert_eq!(
        unsafe { pw_posix_directory_info(f.volume, made.directory, &mut info) },
        -1
    );
    assert_eq!(info, saved);
    let mut policy = 0;
    assert_eq!(
        unsafe { pw_posix_directory_open(f.volume, made.object, &caller(), &mut policy) },
        0
    );
    assert_eq!(pw_directory_close(f.volume, policy), -1);
    let mut cursor = 0;
    assert_eq!(
        unsafe { pw_dir_begin(f.volume, made.object, &mut cursor) },
        0
    );
    assert_eq!(pw_directory_close(f.volume, cursor), -1);
    assert_eq!(pw_dir_close(f.volume, cursor), 0);
    assert_eq!(unsafe { pw_remove(f.volume, 1, b"dir".as_ptr(), 3, 1) }, 0);
    assert_eq!(pw_directory_close(f.volume, made.directory), 0);
    assert_eq!(pw_directory_close(f.volume, made.directory), -1);
    assert_eq!(
        unsafe { pw_posix_directory_info(f.volume, policy, &mut info) },
        0
    );
    assert_eq!(info.stat.links, 0);
    assert_eq!(pw_posix_directory_close(f.volume, policy), 0);
    let mut stat = PwStat::default();
    assert_eq!(unsafe { pw_stat(f.volume, made.object, &mut stat) }, -2);
    f.healthy();
}

#[test]
fn directory_creation_reply_and_typed_pin_are_atomic_and_ordinary_creation_stays_unpinned() {
    let f = Fixture::new(3);
    let mut made = sentinel();
    let attrs = PwCreateAttributes {
        uid: 200,
        gid: 30,
        mode: 0o2700,
        reserved: 0,
    };
    let time = PwTimestamp {
        seconds: -5,
        nanoseconds: 987_654_321,
        reserved: 0,
    };
    let state = PwCreateState {
        valid: 14,
        created: time,
        accessed: time,
        modified: time,
        ..Default::default()
    };
    assert_eq!(
        unsafe {
            pw_fskit_directory_create_with_state(
                f.volume,
                1,
                b"pinned".as_ptr(),
                6,
                &attrs,
                &state,
                &mut made,
            )
        },
        0
    );
    assert!(made.object > 1 && made.directory > 0);
    let a = made.attributes;
    assert_eq!(
        (a.file_id, a.kind, a.size, a.links, a.metadata_valid),
        (made.object as u64 + 1, 1, 0, 0, 1)
    );
    assert_eq!(
        (a.metadata.uid, a.metadata.gid, a.metadata.mode),
        (200, 30, 0o2700)
    );
    assert_eq!(
        (a.metadata.created, a.metadata.accessed, a.metadata.modified),
        (time, time, time)
    );
    let mut other = 0;
    assert_eq!(
        unsafe { pw_directory_open(f.volume, made.object, &mut other) },
        0
    );
    assert_eq!(
        unsafe { pw_remove_checked(f.volume, 1, b"pinned".as_ptr(), 6, 1, made.object) },
        0
    );
    let mut stat = PwStat::default();
    assert_eq!(unsafe { pw_stat(f.volume, made.object, &mut stat) }, 0);
    assert_eq!((stat.kind, stat.links), (1, 0));
    assert_eq!(pw_directory_close(f.volume, made.directory), 0);
    assert_eq!(unsafe { pw_stat(f.volume, made.object, &mut stat) }, 0);
    assert_eq!(pw_directory_close(f.volume, other), 0);
    assert_eq!(unsafe { pw_stat(f.volume, made.object, &mut stat) }, -2);
    let mut legacy = PwFsKitCreated::default();
    assert_eq!(
        unsafe {
            pw_fskit_node_create(
                f.volume,
                1,
                b"ordinary".as_ptr(),
                8,
                1,
                ptr::null(),
                &mut legacy,
            )
        },
        0
    );
    assert_eq!(legacy.file, 0);
    assert_eq!(
        unsafe { pw_remove(f.volume, 1, b"ordinary".as_ptr(), 8, 1) },
        0
    );
    assert_eq!(unsafe { pw_stat(f.volume, legacy.object, &mut stat) }, -2);
    f.healthy();
}

#[test]
fn directory_create_bad_outputs_state_and_conflicts_leave_namespace_parent_and_tokens_unchanged() {
    let f = Fixture::new(3);
    let mut parent = PwMetadata::default();
    assert_eq!(unsafe { pw_metadata_get(f.volume, 1, &mut parent) }, 0);
    for (state, expected) in [
        PwCreateState {
            valid: 1,
            size: 0,
            ..Default::default()
        },
        PwCreateState {
            valid: 16,
            ..Default::default()
        },
        PwCreateState {
            valid: 8,
            modified: PwTimestamp {
                seconds: 0,
                nanoseconds: 1_000_000_000,
                reserved: 0,
            },
            ..Default::default()
        },
        PwCreateState {
            valid: 2,
            created: PwTimestamp {
                seconds: 0,
                nanoseconds: 0,
                reserved: 1,
            },
            ..Default::default()
        },
    ]
    .into_iter()
    .zip([-11, -1, -1, -1])
    {
        let mut out = sentinel();
        let rc = unsafe {
            pw_fskit_directory_create_with_state(
                f.volume,
                1,
                b"absent".as_ptr(),
                6,
                ptr::null(),
                &state,
                &mut out,
            )
        };
        assert_eq!(rc, expected);
        assert_eq!(out, sentinel());
    }
    assert_eq!(
        unsafe {
            pw_fskit_directory_create_with_state(
                f.volume,
                1,
                b"absent".as_ptr(),
                6,
                ptr::null(),
                &PwCreateState::default(),
                ptr::null_mut(),
            )
        },
        -1
    );
    let mut id = 99;
    assert_eq!(
        unsafe { pw_lookup(f.volume, 1, b"absent".as_ptr(), 6, &mut id) },
        -2
    );
    assert_eq!(id, 99);
    let mut after = PwMetadata::default();
    assert_eq!(unsafe { pw_metadata_get(f.volume, 1, &mut after) }, 0);
    assert_eq!(parent, after);
    let made = f.create("created");
    let mut out = sentinel();
    assert_eq!(
        unsafe {
            pw_fskit_directory_create_with_state(
                f.volume,
                1,
                b"CREATED".as_ptr(),
                7,
                ptr::null(),
                &PwCreateState::default(),
                &mut out,
            )
        },
        -3
    );
    assert_eq!(out, sentinel());
    let mut refused = 99;
    assert_eq!(unsafe { pw_directory_open(f.volume, 0, &mut refused) }, -1);
    assert_eq!(refused, 99);
    assert_eq!(pw_directory_close(f.volume, made.directory), 0);
    f.healthy();
}

#[test]
fn concurrent_directory_creators_publish_one_complete_reply_and_no_losing_pin() {
    let f = Fixture::new(3);
    let barrier = Arc::new(Barrier::new(5));
    let threads: Vec<_> = (0..4)
        .map(|_| {
            let barrier = barrier.clone();
            let volume = f.volume;
            std::thread::spawn(move || {
                let mut out = sentinel();
                barrier.wait();
                let rc = unsafe {
                    pw_fskit_directory_create_with_state(
                        volume,
                        1,
                        b"race".as_ptr(),
                        4,
                        ptr::null(),
                        &PwCreateState::default(),
                        &mut out,
                    )
                };
                (rc, out)
            })
        })
        .collect();
    barrier.wait();
    let mut winner = None;
    for t in threads {
        let (rc, out) = t.join().unwrap();
        if rc == 0 {
            assert!(winner.replace(out).is_none());
        } else {
            assert_eq!(rc, -3);
            assert_eq!(out, sentinel());
        }
    }
    let made = winner.unwrap();
    assert_eq!(made.attributes.file_id, made.object as u64 + 1);
    assert_eq!(
        (made.attributes.kind, made.attributes.metadata_valid),
        (1, 1)
    );
    assert_eq!(pw_volume_close(f.volume), -7);
    assert_eq!(pw_directory_close(f.volume, made.directory), 0);
    f.healthy();
}
