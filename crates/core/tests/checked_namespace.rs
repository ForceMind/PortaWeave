use portaweave_core::{CreateAttributes, Error, Kind, ROOT, RenameExpectation, Volume};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "pw-checked-namespace-{}-{stamp}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn volume(&self, v2: bool) -> Volume {
        if v2 {
            Volume::create_with_metadata(
                self.0.join("volume"),
                1 << 20,
                CreateAttributes {
                    uid: 100,
                    gid: 20,
                    mode: 0o700,
                },
            )
            .unwrap()
        } else {
            Volume::create(self.0.join("volume"), 1 << 20).unwrap()
        }
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn remove_does_not_follow_a_replaced_name_and_preserves_old_open_inodes() {
    for v2 in [false, true] {
        let s = Scratch::new();
        let mut v = s.volume(v2);
        let old = v.create_file(ROOT, "slot").unwrap();
        let old_h = v.open_file(old).unwrap();
        v.write(old_h, 0, b"old").unwrap();
        let new = v.create_file(ROOT, "incoming").unwrap();
        let new_h = v.open_file(new).unwrap();
        v.write(new_h, 0, b"new").unwrap();
        v.rename(ROOT, "incoming", ROOT, "slot", true).unwrap();
        let before = (
            v.object_info(ROOT).unwrap(),
            v.object_info(old).unwrap(),
            v.object_info(new).unwrap(),
        );
        assert!(matches!(
            v.remove_checked(ROOT, "SLOT", Kind::File, old),
            Err(Error::StaleEntry)
        ));
        assert_eq!(
            (
                v.object_info(ROOT).unwrap(),
                v.object_info(old).unwrap(),
                v.object_info(new).unwrap()
            ),
            before
        );
        assert_eq!(v.lookup(ROOT, "slot").unwrap(), new);
        assert_eq!(v.read(old_h, 0, 10).unwrap(), b"old");
        assert_eq!(v.read(new_h, 0, 10).unwrap(), b"new");
        assert!(!v.is_write_protected());
        v.remove_checked(ROOT, "SLOT", Kind::File, new).unwrap();
        assert!(matches!(
            v.remove_checked(ROOT, "slot", Kind::File, new),
            Err(Error::StaleEntry)
        ));
        assert_eq!(v.read(new_h, 0, 10).unwrap(), b"new");
        v.close_file(old_h).unwrap();
        v.close_file(new_h).unwrap();
        assert!(matches!(v.stat(old), Err(Error::NotFound)));
        assert!(matches!(v.stat(new), Err(Error::NotFound)));
        v.verify().unwrap();
    }
}

#[test]
fn rename_checks_source_target_and_absence_before_replacing_any_entry() {
    for v2 in [false, true] {
        let s = Scratch::new();
        let mut v = s.volume(v2);
        let a = v.create_file(ROOT, "a").unwrap();
        let b = v.create_file(ROOT, "b").unwrap();
        let c = v.create_file(ROOT, "c").unwrap();
        let hb = v.open_file(b).unwrap();
        v.write(hb, 0, b"old destination").unwrap();
        let before = (
            v.list(ROOT).unwrap(),
            v.object_info(ROOT).unwrap(),
            v.object_info(a).unwrap(),
            v.object_info(b).unwrap(),
        );
        for (from, to, source, target) in [
            ("a", "b", c, Some(b)),
            ("a", "b", a, Some(c)),
            ("a", "b", a, None),
            ("a", "absent", a, Some(b)),
            ("absent", "b", a, Some(b)),
        ] {
            assert!(matches!(
                v.rename_checked(ROOT, from, ROOT, to, RenameExpectation { source, target }),
                Err(Error::StaleEntry)
            ));
            assert_eq!(
                (
                    v.list(ROOT).unwrap(),
                    v.object_info(ROOT).unwrap(),
                    v.object_info(a).unwrap(),
                    v.object_info(b).unwrap()
                ),
                before
            );
        }
        v.rename_checked(
            ROOT,
            "A",
            ROOT,
            "B",
            RenameExpectation {
                source: a,
                target: Some(b),
            },
        )
        .unwrap();
        assert_eq!(v.lookup(ROOT, "b").unwrap(), a);
        assert!(matches!(v.lookup(ROOT, "a"), Err(Error::NotFound)));
        assert_eq!(v.read(hb, 0, 20).unwrap(), b"old destination");
        v.close_file(hb).unwrap();
        v.rename_checked(
            ROOT,
            "b",
            ROOT,
            "fresh",
            RenameExpectation {
                source: a,
                target: None,
            },
        )
        .unwrap();
        assert_eq!(v.lookup(ROOT, "fresh").unwrap(), a);
        v.verify().unwrap();
    }
}

#[test]
fn case_and_unicode_same_entry_renames_differ_from_distinct_hard_link_aliases() {
    for v2 in [false, true] {
        let s = Scratch::new();
        let mut v = s.volume(v2);
        let id = v.create_file(ROOT, "café").unwrap();
        v.rename_checked(
            ROOT,
            "cafe\u{301}",
            ROOT,
            "CAFÉ",
            RenameExpectation {
                source: id,
                target: None,
            },
        )
        .unwrap();
        assert_eq!(v.lookup_entry(ROOT, "café").unwrap().name, "CAFÉ");
        v.rename_checked(
            ROOT,
            "café",
            ROOT,
            "cafe\u{301}",
            RenameExpectation {
                source: id,
                target: Some(id),
            },
        )
        .unwrap();
        v.hard_link(id, ROOT, "alias").unwrap();
        let before = (
            v.list(ROOT).unwrap(),
            v.object_info(ROOT).unwrap(),
            v.object_info(id).unwrap(),
        );
        assert!(matches!(
            v.rename_checked(
                ROOT,
                "CAFÉ",
                ROOT,
                "alias",
                RenameExpectation {
                    source: id,
                    target: None
                }
            ),
            Err(Error::StaleEntry)
        ));
        v.rename_checked(
            ROOT,
            "CAFÉ",
            ROOT,
            "alias",
            RenameExpectation {
                source: id,
                target: Some(id),
            },
        )
        .unwrap();
        assert_eq!(
            (
                v.list(ROOT).unwrap(),
                v.object_info(ROOT).unwrap(),
                v.object_info(id).unwrap()
            ),
            before
        );
        assert_eq!(v.stat(id).unwrap().links, 2);
        assert!(!v.is_write_protected());
    }
}

#[test]
fn checked_directories_preserve_type_empty_cycle_and_reopen_rules() {
    let s = Scratch::new();
    let mut v = s.volume(true);
    let a = v.mkdir(ROOT, "a").unwrap();
    let child = v.mkdir(a, "child").unwrap();
    let b = v.mkdir(ROOT, "b").unwrap();
    let file = v.create_file(b, "file").unwrap();
    let before = (
        v.list(ROOT).unwrap(),
        v.object_info(ROOT).unwrap(),
        v.object_info(a).unwrap(),
        v.object_info(b).unwrap(),
    );
    assert!(matches!(
        v.remove_checked(ROOT, "a", Kind::File, a),
        Err(Error::IsDirectory)
    ));
    assert!(matches!(
        v.remove_checked(b, "file", Kind::Directory, file),
        Err(Error::NotDirectory)
    ));
    assert!(matches!(
        v.remove_checked(ROOT, "a", Kind::Directory, b),
        Err(Error::StaleEntry)
    ));
    assert!(matches!(
        v.remove_checked(ROOT, "a", Kind::Directory, a),
        Err(Error::NotEmpty)
    ));
    assert!(matches!(
        v.rename_checked(
            ROOT,
            "a",
            child,
            "cycle",
            RenameExpectation {
                source: a,
                target: None
            }
        ),
        Err(Error::Cycle)
    ));
    assert!(matches!(
        v.rename_checked(
            ROOT,
            "a",
            ROOT,
            "b",
            RenameExpectation {
                source: a,
                target: Some(b)
            }
        ),
        Err(Error::NotEmpty)
    ));
    assert_eq!(
        (
            v.list(ROOT).unwrap(),
            v.object_info(ROOT).unwrap(),
            v.object_info(a).unwrap(),
            v.object_info(b).unwrap()
        ),
        before
    );
    v.remove_checked(b, "file", Kind::File, file).unwrap();
    v.remove_checked(ROOT, "b", Kind::Directory, b).unwrap();
    v.rename_checked(
        a,
        "child",
        ROOT,
        "moved",
        RenameExpectation {
            source: child,
            target: None,
        },
    )
    .unwrap();
    assert_eq!(v.parent_directory(child).unwrap(), ROOT);
    v.remove_checked(ROOT, "a", Kind::Directory, a).unwrap();
    drop(v);
    let v = Volume::open(s.0.join("volume")).unwrap();
    assert_eq!(v.lookup(ROOT, "moved").unwrap(), child);
    assert_eq!(v.parent_directory(child).unwrap(), ROOT);
    v.verify().unwrap();
}

#[cfg(feature = "fault-injection")]
#[test]
fn checked_namespace_before_commit_faults_roll_back_names_metadata_and_pins() {
    use portaweave_core::FaultPoint;
    for operation in 0..3 {
        let s = Scratch::new();
        let mut v = s.volume(true);
        let a = v.create_file(ROOT, "a").unwrap();
        let b = v.create_file(ROOT, "b").unwrap();
        let h = v.open_file(a).unwrap();
        v.write(h, 0, b"old").unwrap();
        let before = (
            v.list(ROOT).unwrap(),
            v.object_info(ROOT).unwrap(),
            v.object_info(a).unwrap(),
            v.object_info(b).unwrap(),
        );
        v.inject_once(FaultPoint::BeforeCommit);
        let result = match operation {
            0 => v.remove_checked(ROOT, "a", Kind::File, a),
            1 => v.rename_checked(
                ROOT,
                "a",
                ROOT,
                "b",
                RenameExpectation {
                    source: a,
                    target: Some(b),
                },
            ),
            _ => v.rename_checked(
                ROOT,
                "a",
                ROOT,
                "A",
                RenameExpectation {
                    source: a,
                    target: None,
                },
            ),
        };
        assert!(matches!(result, Err(Error::Injected)));
        assert_eq!(
            (
                v.list(ROOT).unwrap(),
                v.object_info(ROOT).unwrap(),
                v.object_info(a).unwrap(),
                v.object_info(b).unwrap()
            ),
            before
        );
        assert_eq!(v.read(h, 0, 10).unwrap(), b"old");
        v.close_file(h).unwrap();
        drop(v);
        let v = Volume::open(s.0.join("volume")).unwrap();
        assert_eq!(
            (
                v.list(ROOT).unwrap(),
                v.object_info(ROOT).unwrap(),
                v.object_info(a).unwrap(),
                v.object_info(b).unwrap()
            ),
            before
        );
    }
}

#[cfg(feature = "fault-injection")]
#[test]
fn checked_success_survives_maintenance_failure_and_close_then_reopen() {
    use portaweave_core::{FaultPoint, MaintenanceStatus};
    for remove in [false, true] {
        let s = Scratch::new();
        let mut v = s.volume(true);
        let a = v.create_file(ROOT, "a").unwrap();
        let b = v.create_file(ROOT, "b").unwrap();
        let h = v.open_file(a).unwrap();
        v.write(h, 0, b"kept").unwrap();
        v.inject_once(FaultPoint::AfterCommit);
        if remove {
            v.remove_checked(ROOT, "a", Kind::File, a).unwrap();
        } else {
            v.rename_checked(
                ROOT,
                "a",
                ROOT,
                "b",
                RenameExpectation {
                    source: a,
                    target: Some(b),
                },
            )
            .unwrap();
        }
        assert!(matches!(v.lookup(ROOT, "a"), Err(Error::NotFound)));
        assert_eq!(v.read(h, 0, 10).unwrap(), b"kept");
        assert!(v.is_write_protected());
        assert!(matches!(
            v.maintenance_status(),
            MaintenanceStatus::Failed { .. }
        ));
        v.close_file(h).unwrap();
        drop(v);
        let v = Volume::open(s.0.join("volume")).unwrap();
        if remove {
            assert!(matches!(v.stat(a), Err(Error::NotFound)));
            assert_eq!(v.lookup(ROOT, "b").unwrap(), b);
        } else {
            assert_eq!(v.lookup(ROOT, "b").unwrap(), a);
            assert!(matches!(v.stat(b), Err(Error::NotFound)));
        }
        v.verify().unwrap();
    }
}
