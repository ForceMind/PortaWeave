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
            "pw-fskit-dir-{}-{stamp}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let p = path.to_str().unwrap();
        let mut volume = 0;
        let a = PwCreateAttributes {
            uid: 100,
            gid: 20,
            mode: 0o700,
            reserved: 0,
        };
        assert_eq!(
            unsafe {
                match version {
                    1 => pw_volume_create(p.as_ptr(), p.len(), 1 << 24, &mut volume),
                    2 => pw_volume_create_with_metadata(
                        p.as_ptr(),
                        p.len(),
                        1 << 24,
                        &a,
                        &mut volume,
                    ),
                    _ => pw_volume_create_with_directory_lifetimes(
                        p.as_ptr(),
                        p.len(),
                        1 << 24,
                        &a,
                        &mut volume,
                    ),
                }
            },
            0
        );
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
    fn begin(&self, id: i64, dots: u8) -> u64 {
        let mut c = 0;
        assert_eq!(
            unsafe { pw_fskit_directory_begin(self.volume, id, dots, &mut c) },
            0
        );
        c
    }
    fn refresh(&self, c: u64, dots: u8) -> u64 {
        let mut out = 0;
        assert_eq!(
            unsafe { pw_fskit_directory_refresh(self.volume, c, dots, &mut out) },
            0
        );
        out
    }
    fn rows(&self, c: u64) -> Vec<(String, i64, PwFsKitAttributes)> {
        assert_eq!(pw_fskit_directory_seek(self.volume, c, 0), 0);
        let mut rows = Vec::new();
        loop {
            let mut bytes = [77u8; 255];
            let mut len = 99;
            let mut id = 99;
            let mut attributes = sentinel();
            let rc = unsafe {
                pw_fskit_directory_next(
                    self.volume,
                    c,
                    bytes.as_mut_ptr(),
                    bytes.len(),
                    &mut len,
                    &mut id,
                    &mut attributes,
                )
            };
            if rc == 1 {
                assert_eq!(
                    (bytes, len, id, attributes),
                    ([77; 255], 99, 99, sentinel())
                );
                break;
            }
            assert_eq!(rc, 0);
            rows.push((
                String::from_utf8(bytes[..len].to_vec()).unwrap(),
                id,
                attributes,
            ));
        }
        rows
    }
    fn remove(&self, parent: i64, name: &str, dir: u8) {
        assert_eq!(
            unsafe { pw_remove(self.volume, parent, name.as_ptr(), name.len(), dir) },
            0
        );
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
fn sentinel() -> PwFsKitAttributes {
    PwFsKitAttributes {
        file_id: 99,
        size: 98,
        kind: 97,
        links: 96,
        metadata_valid: 95,
        reserved: 94,
        ..Default::default()
    }
}

#[test]
fn v1_v2_v3_rows_are_captured_with_metadata_and_survive_overwrite_unlink_gc_and_restart() {
    for version in [1, 2, 3] {
        let f = Fixture::new(version);
        let id = f.create(1, "中文😀", false);
        let mut h = 0;
        assert_eq!(unsafe { pw_file_open(f.volume, id, &mut h) }, 0);
        let mut n = 0;
        assert_eq!(
            unsafe { pw_file_write(f.volume, h, 0, b"old".as_ptr(), 3, &mut n) },
            0
        );
        assert_eq!(
            unsafe { pw_hard_link(f.volume, id, 1, b"alias".as_ptr(), 5) },
            0
        );
        let c = f.begin(1, 0);
        let old = f.rows(c);
        assert_eq!(old.len(), 2);
        assert_eq!(old[0].0, "alias");
        assert_eq!(old[1].0, "中文😀");
        for row in &old {
            assert_eq!(row.1, id);
            assert_eq!(
                (row.2.file_id, row.2.size, row.2.links, row.2.kind),
                (id as u64 + 1, 3, 2, 0)
            );
            assert_eq!(row.2.metadata_valid, u32::from(version != 1));
            if version == 1 {
                assert_eq!(row.2.metadata, PwMetadata::default());
            } else {
                assert_eq!((row.2.metadata.uid, row.2.metadata.mode), (100, 0o600));
            }
        }
        assert_eq!(
            unsafe { pw_file_write(f.volume, h, 0, b"new content".as_ptr(), 11, &mut n) },
            0
        );
        f.remove(1, "alias", 0);
        f.remove(1, "中文😀", 0);
        assert_eq!(pw_file_close(f.volume, h), 0);
        let mut stat = PwStat::default();
        assert_eq!(unsafe { pw_stat(f.volume, id, &mut stat) }, -2);
        assert_eq!(f.rows(c), old);
        assert_eq!(pw_volume_close(f.volume), -7);
        let fresh = f.refresh(c, 0);
        assert_ne!(fresh, c);
        assert_eq!(pw_fskit_directory_seek(f.volume, c, 0), -1);
        assert!(f.rows(fresh).is_empty());
        assert_eq!(pw_fskit_directory_close(f.volume, fresh), 0);
        f.healthy();
    }
}

#[test]
fn fskit_dots_match_parent_root_and_orphan_and_refresh_never_follows_replaced_names() {
    let f = Fixture::new(3);
    let root = f.begin(1, 1);
    let rows = f.rows(root);
    assert_eq!(
        rows.iter()
            .map(|r| (&*r.0, r.1, r.2.file_id, r.2.kind, r.2.links))
            .collect::<Vec<_>>(),
        [(".", 1, 2, 1, 0), ("..", 1, 2, 1, 0)]
    );
    assert_eq!(pw_fskit_directory_close(f.volume, root), 0);
    let parent = f.create(1, "parent", true);
    let id = f.create(parent, "dir", true);
    let mut pin = 0;
    assert_eq!(unsafe { pw_directory_open(f.volume, id, &mut pin) }, 0);
    let c = f.begin(id, 1);
    let old = f.rows(c);
    assert_eq!(
        (old[0].0.as_str(), old[0].1, old[1].0.as_str(), old[1].1),
        (".", id, "..", parent)
    );
    f.remove(parent, "dir", 1);
    let replacement = f.create(parent, "dir", true);
    assert_ne!(replacement, id);
    let empty = f.refresh(c, 1);
    assert!(f.rows(empty).is_empty());
    assert_eq!(pw_fskit_directory_close(f.volume, empty), 0);
    // A different old copy survives final GC, but a NEW capture of its old ID
    // fails rather than switching to the directory now occupying the name.
    let orphan = f.begin(id, 1);
    assert!(f.rows(orphan).is_empty());
    assert_eq!(pw_directory_close(f.volume, pin), 0);
    let mut out = 99;
    assert_eq!(
        unsafe { pw_fskit_directory_refresh(f.volume, orphan, 1, &mut out) },
        -2
    );
    assert_eq!(out, 99);
    assert!(f.rows(orphan).is_empty());
    assert_eq!(pw_fskit_directory_close(f.volume, orphan), 0);
    f.healthy();
}

#[test]
fn output_validation_small_buffers_end_and_bad_cookies_do_not_consume_rows_or_corrupt_tokens() {
    let f = Fixture::new(2);
    let name = "x".repeat(255);
    let id = f.create(1, &name, false);
    let c = f.begin(1, 0);
    let mut bytes = [77u8; 255];
    let (mut n, mut object) = (99, 99);
    let mut attrs = sentinel();
    assert_eq!(
        unsafe {
            pw_fskit_directory_next(
                f.volume,
                c,
                bytes.as_mut_ptr(),
                254,
                &mut n,
                &mut object,
                &mut attrs,
            )
        },
        -16
    );
    assert_eq!((bytes, n, object, attrs), ([77; 255], 99, 99, sentinel()));
    assert_eq!(
        unsafe {
            pw_fskit_directory_next(
                f.volume,
                c,
                ptr::null_mut(),
                0,
                &mut n,
                &mut object,
                &mut attrs,
            )
        },
        -16
    );
    assert_eq!(
        unsafe {
            pw_fskit_directory_next(
                f.volume,
                c,
                bytes.as_mut_ptr(),
                255,
                &mut n,
                &mut object,
                ptr::null_mut(),
            )
        },
        -1
    );
    assert_eq!(pw_fskit_directory_seek(f.volume, c, 2), -19);
    assert_eq!(pw_fskit_directory_seek(f.volume, c, u64::MAX), -19);
    assert_eq!(
        unsafe {
            pw_fskit_directory_next(
                f.volume,
                c,
                bytes.as_mut_ptr(),
                255,
                &mut n,
                &mut object,
                &mut attrs,
            )
        },
        0
    );
    assert_eq!((n, object, attrs.file_id), (255, id, id as u64 + 1));
    assert_eq!(&bytes, b"x".repeat(255).as_slice());
    assert_eq!(pw_fskit_directory_seek(f.volume, c, 1), 0);
    let saved = (bytes, n, object, attrs);
    assert_eq!(
        unsafe {
            pw_fskit_directory_next(
                f.volume,
                c,
                bytes.as_mut_ptr(),
                255,
                &mut n,
                &mut object,
                &mut attrs,
            )
        },
        1
    );
    assert_eq!((bytes, n, object, attrs), saved);
    let mut legacy = 0;
    assert_eq!(unsafe { pw_dir_begin(f.volume, 1, &mut legacy) }, 0);
    assert_eq!(pw_fskit_directory_seek(f.volume, legacy, 0), -1);
    assert_eq!(pw_dir_close(f.volume, c), -1);
    assert_eq!(pw_windows_directory_close(f.volume, c), -1);
    assert_eq!(pw_dir_close(f.volume, legacy), 0);
    assert_eq!(pw_fskit_directory_close(f.volume, c), 0);
    assert_eq!(pw_fskit_directory_close(f.volume, c), -1);
    let mut refused = 99;
    assert_eq!(
        unsafe { pw_fskit_directory_begin(f.volume, 1, 2, &mut refused) },
        -1
    );
    assert_eq!(refused, 99);
    f.healthy();
}

#[test]
fn typed_snapshots_are_owner_scoped_and_can_replace_their_slot_at_full_mixed_admission() {
    let f = Fixture::new(2);
    let other = Fixture::new(2);
    f.create(1, "a", false);
    let target = f.begin(1, 0);
    let old = f.rows(target);
    let mut tokens = Vec::new();
    let caller = PwCredentials {
        uid: 100,
        gid: 20,
        groups: ptr::null(),
        group_count: 0,
        reserved: 0,
    };
    for i in 0..63 {
        let mut c = 0;
        let rc = unsafe {
            if i % 3 == 0 {
                pw_fskit_directory_begin(f.volume, 1, 0, &mut c)
            } else if i % 3 == 1 {
                pw_dir_begin(f.volume, 1, &mut c)
            } else {
                pw_windows_directory_begin(f.volume, 1, &caller, &mut c)
            }
        };
        assert_eq!(rc, 0);
        tokens.push((i % 3, c));
    }
    let mut refused = 99;
    assert_eq!(
        unsafe { pw_fskit_directory_begin(f.volume, 1, 0, &mut refused) },
        -14
    );
    assert_eq!(refused, 99);
    assert_eq!(pw_fskit_directory_close(other.volume, target), -1);
    assert_eq!(
        unsafe { pw_fskit_directory_refresh(other.volume, target, 0, &mut refused) },
        -1
    );
    assert_eq!(refused, 99);
    assert_eq!(
        unsafe { pw_fskit_directory_refresh(f.volume, target, 2, &mut refused) },
        -1
    );
    assert_eq!(refused, 99);
    assert_eq!(f.rows(target), old);
    f.create(1, "b", false);
    let fresh = f.refresh(target, 0);
    assert_eq!(f.rows(fresh).len(), 2);
    assert_eq!(pw_fskit_directory_seek(f.volume, target, 0), -1);
    for (kind, c) in tokens {
        match kind {
            0 => {
                assert_eq!(f.rows(c), old);
                assert_eq!(pw_fskit_directory_close(f.volume, c), 0);
            }
            1 => assert_eq!(pw_dir_close(f.volume, c), 0),
            _ => assert_eq!(pw_windows_directory_close(f.volume, c), 0),
        }
    }
    assert_eq!(pw_fskit_directory_close(f.volume, fresh), 0);
    f.healthy();
}

#[test]
fn failed_refresh_of_removed_legacy_directory_preserves_captured_parent_and_child_rows() {
    let f = Fixture::new(2);
    let id = f.create(1, "directory", true);
    f.create(id, "child", false);
    let c = f.begin(id, 1);
    let old = f.rows(c);
    f.remove(id, "child", 0);
    f.remove(1, "directory", 1);
    f.create(1, "directory", true);
    let mut out = 99;
    assert_eq!(
        unsafe { pw_fskit_directory_refresh(f.volume, c, 0, &mut out) },
        -2
    );
    assert_eq!(out, 99);
    assert_eq!(f.rows(c), old);
    assert_eq!(pw_fskit_directory_close(f.volume, c), 0);
    f.healthy();
}

#[test]
fn concurrent_readers_against_replacement_return_complete_old_rows_or_unchanged_invalid_outputs() {
    let f = Fixture::new(2);
    let id = f.create(1, "a", false);
    let c = f.begin(1, 0);
    let barrier = Arc::new(Barrier::new(5));
    let threads: Vec<_> = (0..4)
        .map(|_| {
            let barrier = barrier.clone();
            let volume = f.volume;
            std::thread::spawn(move || {
                barrier.wait();
                for _ in 0..60 {
                    let _ = pw_fskit_directory_seek(volume, c, 0);
                    let mut bytes = [77u8; 255];
                    let (mut n, mut object) = (99, 99);
                    let mut attrs = sentinel();
                    let rc = unsafe {
                        pw_fskit_directory_next(
                            volume,
                            c,
                            bytes.as_mut_ptr(),
                            255,
                            &mut n,
                            &mut object,
                            &mut attrs,
                        )
                    };
                    if rc == 0 {
                        assert_eq!(
                            (n, object, attrs.file_id, bytes[0]),
                            (1, id, id as u64 + 1, b'a')
                        );
                    } else {
                        assert!(rc == 1 || rc == -1);
                        assert_eq!((bytes, n, object, attrs), ([77; 255], 99, 99, sentinel()));
                    }
                }
            })
        })
        .collect();
    barrier.wait();
    let fresh = f.refresh(c, 0);
    for t in threads {
        t.join().unwrap();
    }
    assert_eq!(f.rows(fresh).len(), 1);
    assert_eq!(pw_fskit_directory_close(f.volume, fresh), 0);
}
