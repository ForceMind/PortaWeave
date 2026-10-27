use portaweave_native::*;
use std::{
    fs,
    path::PathBuf,
    ptr,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
fn caller(uid: u32) -> PwCredentials {
    PwCredentials {
        uid,
        gid: 20,
        groups: ptr::null(),
        group_count: 0,
        reserved: 0,
    }
}
fn sentinel() -> PwFileInfo {
    PwFileInfo {
        object: 99,
        stat: PwStat {
            kind: 9,
            size: 98,
            links: 97,
            generation: 96,
        },
        metadata: PwMetadata {
            uid: 95,
            ..Default::default()
        },
    }
}
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
            "pw-directory-abi-{}-{stamp}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let p = path.to_str().unwrap();
        let mut volume = 0;
        let a = PwCreateAttributes {
            uid: 100,
            gid: 20,
            mode: 0o777,
            reserved: 0,
        };
        let status = unsafe {
            match version {
                1 => pw_volume_create(p.as_ptr(), p.len(), 1 << 24, &mut volume),
                2 => pw_volume_create_with_metadata(p.as_ptr(), p.len(), 1 << 24, &a, &mut volume),
                _ => pw_volume_create_with_directory_lifetimes(
                    p.as_ptr(),
                    p.len(),
                    1 << 24,
                    &a,
                    &mut volume,
                ),
            }
        };
        assert_eq!(status, 0);
        Self { path, volume }
    }
    fn create(&self, parent: i64, name: &str, dir: bool) -> i64 {
        let mut id = 0;
        assert_eq!(
            unsafe {
                if dir {
                    pw_directory_create(self.volume, parent, name.as_ptr(), name.len(), &mut id)
                } else {
                    pw_file_create(self.volume, parent, name.as_ptr(), name.len(), &mut id)
                }
            },
            0
        );
        id
    }
    fn open(&self, id: i64) -> u64 {
        let mut h = 0;
        assert_eq!(
            unsafe { pw_posix_directory_open(self.volume, id, &caller(100), &mut h) },
            0
        );
        h
    }
    fn info(&self, h: u64) -> PwFileInfo {
        let mut info = sentinel();
        assert_eq!(
            unsafe { pw_posix_directory_info(self.volume, h, &mut info) },
            0
        );
        info
    }
    fn snapshot(&self, h: u64) -> u64 {
        let mut c = 0;
        assert_eq!(
            unsafe { pw_windows_directory_begin_granted(self.volume, h, &mut c) },
            0
        );
        c
    }
    fn read_snapshot(&self, c: u64) -> Vec<u8> {
        let mut bytes = vec![77; 1024];
        let mut n = 99;
        assert_eq!(
            unsafe {
                pw_windows_directory_read(
                    self.volume,
                    c,
                    0,
                    bytes.as_mut_ptr(),
                    bytes.len(),
                    &mut n,
                )
            },
            0
        );
        bytes.truncate(n);
        bytes
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
fn metadata(f: &Fixture, id: i64, update: PwMetadataUpdate) {
    assert_eq!(unsafe { pw_metadata_set(f.volume, id, &update) }, 0);
}

#[test]
fn new_v3_creation_reports_required_capability_and_rejects_legacy_directory_grants_without_migration()
 {
    for version in [1, 2, 3] {
        let f = Fixture::new(version);
        let before = fs::read(f.path.join("format.txt")).unwrap();
        let mut h = 99;
        let status = unsafe { pw_posix_directory_open(f.volume, 1, &caller(100), &mut h) };
        if version < 3 {
            assert_eq!(status, -11);
            assert_eq!(h, 99);
        } else {
            assert_eq!(status, 0);
            assert_eq!(pw_posix_directory_close(f.volume, h), 0);
            let mut identity = PwVolumeIdentity {
                uuid: [0; 16],
                capabilities: 0,
            };
            assert_eq!(unsafe { pw_volume_identity(f.volume, &mut identity) }, 0);
            assert_eq!(identity.capabilities, 3);
        }
        let p = f.path.to_str().unwrap();
        let mut volume = 99;
        let a = PwCreateAttributes {
            uid: 100,
            gid: 20,
            mode: 0o700,
            reserved: 0,
        };
        assert_ne!(
            unsafe {
                pw_volume_create_with_directory_lifetimes(
                    p.as_ptr(),
                    p.len(),
                    1 << 20,
                    &a,
                    &mut volume,
                )
            },
            0
        );
        assert_eq!(volume, 99);
        assert_eq!(fs::read(f.path.join("format.txt")).unwrap(), before);
    }
}

#[test]
fn granted_inode_info_and_empty_snapshot_survive_revocation_and_last_namespace_unlink() {
    let f = Fixture::new(3);
    let id = f.create(1, "dir", true);
    let first = f.open(id);
    let last = f.open(id);
    let old = f.snapshot(first);
    assert_eq!(f.read_snapshot(old).len(), 226);
    metadata(
        &f,
        id,
        PwMetadataUpdate {
            valid: 4,
            mode: 0,
            ..Default::default()
        },
    );
    let mut denied = 99;
    assert_eq!(
        unsafe { pw_posix_directory_open(f.volume, id, &caller(100), &mut denied) },
        -17
    );
    assert_eq!(denied, 99);
    assert_eq!(unsafe { pw_remove(f.volume, 1, b"dir".as_ptr(), 3, 1) }, 0);
    let info = f.info(first);
    assert_eq!(
        (
            info.object,
            info.stat.kind,
            info.stat.links,
            info.metadata.mode
        ),
        (id, 1, 0, 0)
    );
    let empty = f.snapshot(first);
    assert_eq!(f.read_snapshot(empty), [0, 0]);
    let mut current = PwWindowsFileInfo::default();
    assert_eq!(
        unsafe { pw_windows_directory_info(f.volume, old, &caller(100), &mut current) },
        -2
    );
    let mut object = 99;
    assert_eq!(
        unsafe { pw_file_create(f.volume, id, b"forbidden".as_ptr(), 9, &mut object) },
        -2
    );
    assert_eq!(object, 99);
    assert_eq!(pw_volume_close(f.volume), -7);
    assert_eq!(pw_posix_directory_close(f.volume, first), 0);
    assert_eq!(f.info(last).object, id);
    assert_eq!(pw_posix_directory_close(f.volume, last), 0);
    let mut stat = PwStat::default();
    assert_eq!(unsafe { pw_stat(f.volume, id, &mut stat) }, -2);
    assert_eq!(f.read_snapshot(old).len(), 226);
    assert_eq!(f.read_snapshot(empty), [0, 0]); // metadata copies hold no inode
    assert_eq!(pw_windows_directory_close(f.volume, old), 0);
    assert_eq!(pw_windows_directory_close(f.volume, empty), 0);
    f.healthy();
}

#[test]
fn directory_descriptor_metadata_rechecks_current_owner_and_preserves_failed_outputs() {
    let f = Fixture::new(3);
    let id = f.create(1, "dir", true);
    let h = f.open(id);
    assert_eq!(unsafe { pw_remove(f.volume, 1, b"dir".as_ptr(), 3, 1) }, 0);
    metadata(
        &f,
        id,
        PwMetadataUpdate {
            valid: 1 | 4,
            uid: 200,
            mode: 0,
            ..Default::default()
        },
    );
    let before = f.info(h);
    let mut info = sentinel();
    let update = PwMetadataUpdate {
        valid: 4 | 32,
        mode: 0o700,
        modified: PwTimestamp {
            seconds: -12,
            nanoseconds: 987_654_321,
            reserved: 0,
        },
        ..Default::default()
    };
    assert_eq!(
        unsafe {
            pw_posix_directory_set_metadata_info(f.volume, h, &caller(100), &update, &mut info)
        },
        -17
    );
    assert_eq!(info, sentinel());
    assert_eq!(
        unsafe {
            pw_posix_directory_set_metadata_info(f.volume, h, ptr::null(), &update, &mut info)
        },
        -1
    );
    assert_eq!(info, sentinel());
    let mut invalid = update;
    invalid.modified.nanoseconds = 1_000_000_000;
    assert_eq!(
        unsafe {
            pw_posix_directory_set_metadata_info(f.volume, h, &caller(200), &invalid, &mut info)
        },
        -1
    );
    assert_eq!(info, sentinel());
    assert_eq!(f.info(h), before);
    assert_eq!(
        unsafe {
            pw_posix_directory_set_metadata_info(f.volume, h, &caller(200), &update, &mut info)
        },
        0
    );
    assert_eq!(info, f.info(h));
    assert_eq!(info.stat.generation, before.stat.generation + 1);
    assert_eq!(info.metadata.modified, update.modified);
    assert_eq!(pw_posix_directory_close(f.volume, h), 0);
    f.healthy();
}

#[test]
fn directory_tokens_are_typed_volume_scoped_and_stale_with_unchanged_outputs() {
    let f = Fixture::new(3);
    let foreign = Fixture::new(3);
    let id = f.create(1, "dir", true);
    let h = f.open(id);
    let file = f.create(1, "file", false);
    let mut admin = 0;
    let mut policy = 0;
    let mut cursor = 0;
    assert_eq!(unsafe { pw_file_open(f.volume, file, &mut admin) }, 0);
    assert_eq!(
        unsafe {
            pw_posix_file_open(
                f.volume,
                1,
                b"file".as_ptr(),
                4,
                4,
                &caller(100),
                &mut policy,
            )
        },
        0
    );
    assert_eq!(unsafe { pw_dir_begin(f.volume, 1, &mut cursor) }, 0);
    let mut info = sentinel();
    let mut bytes = [77; 8];
    let mut n = 99;
    let mut fresh = 99;
    for token in [admin, policy, cursor, 0, u64::MAX] {
        assert_eq!(
            unsafe { pw_posix_directory_info(f.volume, token, &mut info) },
            -1
        );
        assert_eq!(
            unsafe { pw_windows_directory_begin_granted(f.volume, token, &mut fresh) },
            -1
        );
        assert_eq!(pw_posix_directory_close(f.volume, token), -1);
        assert_eq!((info, fresh), (sentinel(), 99));
    }
    assert_eq!(
        unsafe { pw_posix_directory_info(foreign.volume, h, &mut info) },
        -1
    );
    assert_eq!(
        unsafe { pw_posix_directory_info(f.volume, h, ptr::null_mut()) },
        -1
    );
    assert_eq!(unsafe { pw_posix_file_info(f.volume, h, &mut info) }, -1);
    assert_eq!(
        unsafe { pw_file_read(f.volume, h, 0, bytes.as_mut_ptr(), bytes.len(), &mut n) },
        -1
    );
    assert_eq!((bytes, n), ([77; 8], 99));
    assert_eq!(info, sentinel());
    assert_eq!(pw_file_close(f.volume, h), -1);
    assert_eq!(pw_posix_file_close(f.volume, h), -1);
    assert_eq!(pw_posix_directory_close(f.volume, h), 0);
    assert_eq!(
        unsafe { pw_posix_directory_info(f.volume, h, &mut info) },
        -1
    );
    assert_eq!(info, sentinel());
    assert_eq!(pw_dir_close(f.volume, cursor), 0);
    assert_eq!(pw_file_close(f.volume, admin), 0);
    assert_eq!(pw_posix_file_close(f.volume, policy), 0);
    f.healthy();
}

#[test]
fn granted_snapshot_projects_all_children_before_admission_without_reauthorizing_the_old_read_grant()
 {
    let f = Fixture::new(3);
    let dir = f.create(1, "dir", true);
    let child = f.create(dir, "child", false);
    let h = f.open(dir);
    metadata(
        &f,
        dir,
        PwMetadataUpdate {
            valid: 4,
            mode: 0,
            ..Default::default()
        },
    );
    let old = f.snapshot(h);
    let old_bytes = f.read_snapshot(old);
    metadata(
        &f,
        child,
        PwMetadataUpdate {
            valid: 8,
            created: PwTimestamp {
                seconds: -11_644_473_601,
                nanoseconds: 0,
                reserved: 0,
            },
            ..Default::default()
        },
    );
    let mut denied = 99;
    assert_eq!(
        unsafe { pw_windows_directory_begin_granted(f.volume, h, &mut denied) },
        -11
    );
    assert_eq!(denied, 99);
    assert_eq!(f.read_snapshot(old), old_bytes);
    assert_eq!(f.info(h).object, dir);
    assert_eq!(pw_posix_directory_close(f.volume, h), 0); // linked object stays, old cursor is independent
    assert_eq!(f.read_snapshot(old), old_bytes);
    assert_eq!(pw_windows_directory_close(f.volume, old), 0);
    f.healthy();
}

#[test]
fn granted_snapshots_share_the_real_cursor_budget_and_never_evict_file_or_directory_pins() {
    let f = Fixture::new(3);
    let dir = f.create(1, "empty", true);
    let h = f.open(dir);
    let mut snapshots = Vec::new();
    for _ in 0..64 {
        snapshots.push(f.snapshot(h));
    }
    let mut refused = 99;
    assert_eq!(
        unsafe { pw_windows_directory_begin_granted(f.volume, h, &mut refused) },
        -14
    );
    assert_eq!(refused, 99);
    assert_eq!(f.info(h).object, dir);
    assert_eq!(pw_volume_close(f.volume), -7);
    assert_eq!(
        pw_windows_directory_close(f.volume, snapshots.pop().unwrap()),
        0
    );
    let new = f.snapshot(h);
    snapshots.push(new);
    for c in snapshots {
        assert_eq!(f.read_snapshot(c).len(), 226);
        assert_eq!(pw_windows_directory_close(f.volume, c), 0);
    }
    assert_eq!(pw_volume_close(f.volume), -7);
    assert_eq!(pw_posix_directory_close(f.volume, h), 0);
    f.healthy();
}

#[test]
fn four_real_info_readers_racing_last_close_never_observe_partially_reclaimed_metadata() {
    use std::sync::{Arc, Barrier};
    let f = Fixture::new(3);
    let dir = f.create(1, "dir", true);
    let h = f.open(dir);
    assert_eq!(unsafe { pw_remove(f.volume, 1, b"dir".as_ptr(), 3, 1) }, 0);
    let expected = f.info(h);
    let ready = Arc::new(Barrier::new(5));
    let mut workers = Vec::new();
    for _ in 0..4 {
        let barrier = Arc::clone(&ready);
        let volume = f.volume;
        workers.push(std::thread::spawn(move || {
            let mut info = sentinel();
            assert_eq!(unsafe { pw_posix_directory_info(volume, h, &mut info) }, 0);
            assert_eq!(info, expected);
            barrier.wait();
            for _ in 0..64 {
                info = sentinel();
                match unsafe { pw_posix_directory_info(volume, h, &mut info) } {
                    0 => assert_eq!(info, expected),
                    -1 => assert_eq!(info, sentinel()),
                    other => panic!("unexpected status {other}"),
                }
            }
        }));
    }
    ready.wait();
    assert_eq!(pw_posix_directory_close(f.volume, h), 0);
    for w in workers {
        w.join().unwrap();
    }
    let mut info = sentinel();
    assert_eq!(
        unsafe { pw_posix_directory_info(f.volume, h, &mut info) },
        -1
    );
    assert_eq!(info, sentinel());
    f.healthy();
}

#[test]
fn windows_directory_inode_projection_and_basic_times_remain_precommit_and_owner_checked_after_unlink()
 {
    let f = Fixture::new(3);
    let dir = f.create(1, "dir", true);
    let h = f.open(dir);
    assert_eq!(unsafe { pw_remove(f.volume, 1, b"dir".as_ptr(), 3, 1) }, 0);
    let mut info = PwWindowsFileInfo::default();
    assert_eq!(
        unsafe { pw_windows_directory_handle_info(f.volume, h, &mut info) },
        0
    );
    assert_eq!(
        (info.index_number, info.file_size, info.allocation_size),
        (dir as u64, 0, 0)
    );
    let basic = PwWindowsBasicInfo {
        file_attributes: u32::MAX,
        reserved: 0,
        creation_time: 1,
        last_access_time: 116_444_736_000_000_007,
        last_write_time: 116_444_735_999_999_999,
        change_time: 0,
    };
    let sentinel = info;
    assert_eq!(
        unsafe {
            pw_windows_directory_handle_set_basic_info(f.volume, h, &caller(200), &basic, &mut info)
        },
        -17
    );
    assert_eq!(info, sentinel);
    assert_eq!(
        unsafe {
            pw_windows_directory_handle_set_basic_info(f.volume, h, ptr::null(), &basic, &mut info)
        },
        -1
    );
    assert_eq!(info, sentinel);
    assert_eq!(
        unsafe {
            pw_windows_directory_handle_set_basic_info(
                f.volume,
                h,
                &caller(100),
                &basic,
                ptr::null_mut(),
            )
        },
        -1
    );
    assert_eq!(
        unsafe {
            pw_windows_directory_handle_set_basic_info(f.volume, h, &caller(100), &basic, &mut info)
        },
        0
    );
    assert_eq!(info.creation_time, 1);
    assert_eq!(info.last_access_time, basic.last_access_time);
    assert_eq!(info.last_write_time, basic.last_write_time);
    let saved = info;
    metadata(
        &f,
        dir,
        PwMetadataUpdate {
            valid: 8,
            created: PwTimestamp {
                seconds: -11_644_473_601,
                nanoseconds: 0,
                reserved: 0,
            },
            ..Default::default()
        },
    );
    let before = f.info(h);
    let change = PwWindowsBasicInfo {
        creation_time: 0,
        last_write_time: 1,
        ..basic
    };
    assert_eq!(
        unsafe { pw_windows_directory_handle_info(f.volume, h, &mut info) },
        -11
    );
    assert_eq!(info, saved);
    assert_eq!(
        unsafe {
            pw_windows_directory_handle_set_basic_info(
                f.volume,
                h,
                &caller(100),
                &change,
                &mut info,
            )
        },
        -11
    );
    assert_eq!(info, saved);
    assert_eq!(f.info(h), before);
    f.healthy();
    assert_eq!(pw_posix_directory_close(f.volume, h), 0);
    assert_eq!(
        unsafe { pw_windows_directory_handle_info(f.volume, h, &mut info) },
        -1
    );
    assert_eq!(info, saved);
}
