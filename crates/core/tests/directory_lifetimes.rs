use portaweave_core::{
    CreateAttributes, Credentials, DIRECTORY_FORMAT, Error, ExportLinks, MetadataUpdate, ROOT,
    Recovery, RecoveryLimits, Timestamp, Volume,
};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Scratch(PathBuf);
fn attrs(uid: u32, mode: u16) -> CreateAttributes {
    CreateAttributes { uid, gid: 20, mode }
}
fn caller(uid: u32) -> Credentials {
    Credentials::new(uid, 20, &[]).unwrap()
}
impl Scratch {
    fn new() -> Self {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let p = std::env::temp_dir().join(format!(
            "pw-directory-lifetime-{}-{stamp}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&p).unwrap();
        Self(p)
    }
    fn path(&self) -> PathBuf {
        self.0.join("volume")
    }
    fn volume(&self) -> Volume {
        Volume::create_with_directory_lifetimes(self.path(), 1 << 24, attrs(100, 0o777)).unwrap()
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn hashes(root: &Path) -> BTreeMap<PathBuf, [u8; 32]> {
    fn walk(root: &Path, p: &Path, out: &mut BTreeMap<PathBuf, [u8; 32]>) {
        for e in fs::read_dir(p).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                walk(root, &p, out);
            } else {
                out.insert(
                    p.strip_prefix(root).unwrap().to_owned(),
                    Sha256::digest(fs::read(p).unwrap()).into(),
                );
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}

#[test]
fn v3_is_explicit_and_legacy_creation_and_directory_gates_do_not_migrate() {
    let s = Scratch::new();
    for v2 in [false, true] {
        let p = s.0.join(if v2 { "v2" } else { "v1" });
        let mut v = if v2 {
            Volume::create_with_metadata(&p, 100, attrs(100, 0o700)).unwrap()
        } else {
            Volume::create(&p, 100).unwrap()
        };
        let before = fs::read(p.join("format.txt")).unwrap();
        assert!(!v.supports_directory_lifetimes());
        assert!(matches!(
            v.open_directory(ROOT),
            Err(Error::UnsupportedOperation)
        ));
        if v2 {
            assert_eq!(v.volume_identity().unwrap().capabilities, 1);
            assert!(matches!(
                v.posix(caller(100)).unwrap().open_directory(ROOT),
                Err(Error::UnsupportedOperation)
            ));
        }
        assert!(Volume::create_with_directory_lifetimes(&p, 100, attrs(100, 0o700)).is_err());
        assert_eq!(fs::read(p.join("format.txt")).unwrap(), before);
    }
    let v = s.volume();
    assert!(v.supports_directory_lifetimes());
    assert_eq!(v.volume_identity().unwrap().capabilities, 3);
    assert_eq!(
        fs::read(s.path().join("format.txt")).unwrap(),
        DIRECTORY_FORMAT.as_bytes()
    );
    v.verify().unwrap();
    drop(v);
    let db = rusqlite::Connection::open(s.path().join("catalog.sqlite")).unwrap();
    assert_eq!(
        db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        3
    );
    drop(db);
    let v = Volume::open(s.path()).unwrap();
    v.verify().unwrap();
}

#[test]
fn granted_directory_survives_revocation_move_unlink_and_never_repopulates_an_orphan() {
    let s = Scratch::new();
    let mut v = s.volume();
    let dir = v
        .mkdir_with_attributes(ROOT, "selected", attrs(100, 0o700))
        .unwrap();
    let hidden = v
        .mkdir_with_attributes(ROOT, "hidden", attrs(200, 0o700))
        .unwrap();
    let file = v.create_file(dir, "child").unwrap();
    let other = v.create_file(ROOT, "other").unwrap();
    let grant = v.posix(caller(100)).unwrap().open_directory(dir).unwrap();
    let old = grant.snapshot(&v, 1).unwrap();
    assert_eq!(old.parent.as_ref().unwrap().stat.id, ROOT);
    assert_eq!(old.entries[0].info.stat.id, file);
    assert!(matches!(grant.snapshot(&v, 0), Err(Error::ResourceLimit)));
    v.rename(ROOT, "selected", hidden, "moved", false).unwrap();
    v.set_metadata(
        dir,
        MetadataUpdate {
            uid: Some(200),
            mode: Some(0),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(matches!(
        v.posix(caller(100)).unwrap().open_directory(dir),
        Err(Error::PermissionDenied)
    ));
    assert_eq!(grant.info(&v).unwrap().metadata.uid, 200);
    assert_eq!(grant.info(&v).unwrap().metadata.mode, 0);
    assert_eq!(
        grant.snapshot(&v, 1).unwrap().parent.unwrap().stat.id,
        hidden
    );
    v.unlink(dir, "child").unwrap();
    v.rmdir(hidden, "moved").unwrap();
    assert_eq!(grant.info(&v).unwrap().stat.links, 0);
    let empty = grant.snapshot(&v, 0).unwrap();
    assert!(empty.parent.is_none() && empty.entries.is_empty());
    assert!(matches!(
        v.posix(caller(200)).unwrap().open_directory(dir),
        Err(Error::NotFound)
    ));
    assert!(matches!(
        v.posix(caller(200)).unwrap().directory_info(dir),
        Err(Error::NotFound)
    ));
    assert!(matches!(
        v.create_file(dir, "forbidden"),
        Err(Error::NotFound)
    ));
    assert!(matches!(v.mkdir(dir, "forbidden"), Err(Error::NotFound)));
    assert!(matches!(
        v.hard_link(other, dir, "forbidden"),
        Err(Error::NotFound)
    ));
    assert!(matches!(
        v.rename(ROOT, "other", dir, "forbidden", false),
        Err(Error::NotFound)
    ));
    assert_eq!(v.lookup(ROOT, "other").unwrap(), other);
    assert!(!v.is_write_protected());
    v.verify().unwrap();
    let copied = grant;
    grant.close(&mut v).unwrap();
    assert!(matches!(copied.info(&v), Err(Error::InvalidHandle)));
    assert!(matches!(v.stat(dir), Err(Error::NotFound)));
    assert_eq!(old.directory.stat.id, dir);
    assert_eq!(old.entries[0].info.stat.id, file);
    let replacement = v.mkdir(ROOT, "selected").unwrap();
    assert_ne!(replacement, dir);
    assert_eq!(v.stats().unwrap().open_handles, 0);
    v.verify().unwrap();
}

#[test]
fn directory_replacement_preserves_each_target_pin_until_its_last_close() {
    let s = Scratch::new();
    let mut v = s.volume();
    let source = v.mkdir(ROOT, "source").unwrap();
    let target = v.mkdir(ROOT, "target").unwrap();
    let granted = v
        .posix(caller(100))
        .unwrap()
        .open_directory(target)
        .unwrap();
    let administrative = v.open_directory(target).unwrap();
    let source_grant = v
        .posix(caller(100))
        .unwrap()
        .open_directory(source)
        .unwrap();
    v.rename(ROOT, "source", ROOT, "target", true).unwrap();
    assert_eq!(v.lookup(ROOT, "target").unwrap(), source);
    assert_eq!(granted.info(&v).unwrap().stat.id, target);
    assert_eq!(granted.info(&v).unwrap().stat.links, 0);
    assert!(granted.snapshot(&v, 0).unwrap().parent.is_none());
    granted.close(&mut v).unwrap();
    assert_eq!(v.stat(target).unwrap().links, 0);
    v.close_directory(administrative).unwrap();
    assert!(matches!(v.stat(target), Err(Error::NotFound)));
    assert_eq!(source_grant.info(&v).unwrap().stat.links, 1);
    source_grant.close(&mut v).unwrap();
    v.verify().unwrap();
}

#[test]
fn directory_metadata_uses_current_owner_and_precommit_projection_not_old_read_authority() {
    let s = Scratch::new();
    let mut v = s.volume();
    let id = v.mkdir(ROOT, "dir").unwrap();
    let grant = v.posix(caller(100)).unwrap().open_directory(id).unwrap();
    v.rmdir(ROOT, "dir").unwrap();
    v.set_metadata(
        id,
        MetadataUpdate {
            uid: Some(200),
            mode: Some(0),
            ..Default::default()
        },
    )
    .unwrap();
    let old = grant.info(&v).unwrap();
    let update = MetadataUpdate {
        mode: Some(0o700),
        modified: Some(Timestamp {
            seconds: -12,
            nanoseconds: 987_654_321,
        }),
        ..Default::default()
    };
    assert!(matches!(
        v.posix(caller(100))
            .unwrap()
            .set_directory_metadata_with_reply(grant, update, Ok),
        Err(Error::PermissionDenied)
    ));
    let refused: portaweave_core::Result<()> = v
        .posix(caller(200))
        .unwrap()
        .set_directory_metadata_with_reply(grant, update, |_| Err(Error::UnsupportedOperation));
    assert!(matches!(refused, Err(Error::UnsupportedOperation)));
    assert_eq!(grant.info(&v).unwrap(), old);
    assert!(!v.is_write_protected());
    let info = v
        .posix(caller(200))
        .unwrap()
        .set_directory_metadata_with_reply(grant, update, Ok)
        .unwrap();
    assert_eq!(info, grant.info(&v).unwrap());
    assert_eq!(info.stat.links, 0);
    assert_eq!(info.stat.generation, old.stat.generation + 1);
    assert_eq!(info.metadata.modified, update.modified.unwrap());
    grant.close(&mut v).unwrap();
    assert!(matches!(v.stat(id), Err(Error::NotFound)));
}

#[test]
fn directory_grants_are_volume_scoped_and_close_invalidates_all_copies() {
    let s = Scratch::new();
    let mut v = s.volume();
    let mut foreign =
        Volume::create_with_directory_lifetimes(s.0.join("foreign"), 100, attrs(100, 0o700))
            .unwrap();
    let grant = v.posix(caller(100)).unwrap().open_directory(ROOT).unwrap();
    assert!(matches!(grant.info(&foreign), Err(Error::InvalidHandle)));
    assert!(matches!(
        grant.close(&mut foreign),
        Err(Error::InvalidHandle)
    ));
    let copy = grant;
    grant.close(&mut v).unwrap();
    assert!(matches!(copy.info(&v), Err(Error::InvalidHandle)));
    let fresh = v.posix(caller(100)).unwrap().open_directory(ROOT).unwrap();
    assert_ne!(fresh, copy);
    fresh.close(&mut v).unwrap();
    drop(v);
    let v = Volume::open(s.path()).unwrap();
    assert!(matches!(copy.info(&v), Err(Error::InvalidHandle)));
}

#[test]
fn abandoned_empty_directory_is_strictly_reported_not_exported_and_reclaimed_only_by_normal_open() {
    let s = Scratch::new();
    let mut v = s.volume();
    let id = v.mkdir(ROOT, "removed").unwrap();
    let grant = v.posix(caller(100)).unwrap().open_directory(id).unwrap();
    let file = v.create_file(ROOT, "data").unwrap();
    let h = v.open_file(file).unwrap();
    v.write(h, 0, b"preserved").unwrap();
    v.close_file(h).unwrap();
    v.hard_link(file, ROOT, "alias").unwrap();
    v.rmdir(ROOT, "removed").unwrap();
    assert_eq!(grant.info(&v).unwrap().stat.links, 0);
    drop(v); // deliberately abandoned fixture, not safe unmount
    let before = hashes(&s.path());
    let r = Recovery::prepare(s.path(), s.0.join("recovery"), RecoveryLimits::default()).unwrap();
    let report = r.verify().unwrap();
    assert_eq!((report.orphan_directories, report.orphan_files), (1, 0));
    let export = r
        .export_new(s.0.join("export"), ExportLinks::Preserve)
        .unwrap();
    assert_eq!(export.hard_links, 1);
    assert_eq!(
        fs::read(s.0.join("export/files/data")).unwrap(),
        b"preserved"
    );
    assert!(!s.0.join("export/files/removed").exists());
    assert!(
        fs::read_to_string(s.0.join("export/report.txt"))
            .unwrap()
            .contains("unnamed_orphan_directories_not_exported=1")
    );
    assert_eq!(hashes(&s.path()), before);
    drop(r);
    let v = Volume::open(s.path()).unwrap();
    assert!(matches!(v.stat(id), Err(Error::NotFound)));
    assert!(matches!(grant.info(&v), Err(Error::InvalidHandle)));
    v.verify().unwrap();
    drop(v);
    Volume::open(s.path()).unwrap().verify().unwrap();
}

#[test]
fn legacy_orphan_directories_and_v3_nonempty_or_data_bearing_orphans_are_still_corrupt() {
    for version in 1..=3 {
        for with_data in [false, true] {
            if version < 3 && with_data {
                continue;
            }
            let s = Scratch::new();
            let mut v = match version {
                1 => Volume::create(s.path(), 1 << 24).unwrap(),
                2 => Volume::create_with_metadata(s.path(), 1 << 24, attrs(100, 0o777)).unwrap(),
                _ => s.volume(),
            };
            let dir = v.mkdir(ROOT, "dir").unwrap();
            let file = if version == 3 && !with_data {
                v.create_file(dir, "child").unwrap()
            } else {
                v.create_file(ROOT, "data").unwrap()
            };
            if with_data {
                let h = v.open_file(file).unwrap();
                v.write(h, 0, b"data").unwrap();
                v.close_file(h).unwrap();
            }
            drop(v);
            let db = rusqlite::Connection::open(s.path().join("catalog.sqlite")).unwrap();
            db.execute("DELETE FROM entries WHERE object=?1", [dir.raw()])
                .unwrap();
            if with_data {
                db.execute(
                    "UPDATE blocks SET object=?1 WHERE object=?2",
                    [dir.raw(), file.raw()],
                )
                .unwrap();
            }
            drop(db);
            let before = hashes(&s.path());
            let r = Recovery::prepare(s.path(), s.0.join("recovery"), RecoveryLimits::default())
                .unwrap();
            assert!(
                matches!(r.verify(), Err(Error::Corrupt(_))),
                "v{version}/data={with_data}"
            );
            assert_eq!(hashes(&s.path()), before);
            drop(r);
            assert!(matches!(Volume::open(s.path()), Err(Error::Corrupt(_))));
            assert!(s.path().join("catalog.sqlite").is_file());
        }
    }
}

#[test]
fn v3_marker_version_and_required_capability_mismatches_are_rejected() {
    for change in ["marker", "version", "capabilities"] {
        let s = Scratch::new();
        let v = s.volume();
        drop(v);
        if change == "marker" {
            fs::write(
                s.path().join("format.txt"),
                portaweave_core::METADATA_FORMAT,
            )
            .unwrap();
        } else {
            let db = rusqlite::Connection::open(s.path().join("catalog.sqlite")).unwrap();
            if change == "version" {
                db.execute_batch("PRAGMA user_version=2").unwrap();
            } else {
                db.execute_batch(
                    "PRAGMA ignore_check_constraints=ON;UPDATE volume_identity SET capabilities=1",
                )
                .unwrap();
            }
        }
        assert!(
            matches!(
                Volume::open(s.path()),
                Err(Error::UnsupportedFormat | Error::Corrupt(_))
            ),
            "{change}"
        );
    }
}

#[cfg(feature = "fault-injection")]
#[test]
fn directory_rmdir_fault_boundaries_preserve_business_results_and_release_under_protection() {
    use portaweave_core::{FaultPoint, MaintenanceStatus};
    for before_commit in [true, false] {
        let s = Scratch::new();
        let mut v = s.volume();
        let id = v.mkdir(ROOT, "dir").unwrap();
        let grant = v.posix(caller(100)).unwrap().open_directory(id).unwrap();
        let old = grant.info(&v).unwrap();
        v.inject_once(if before_commit {
            FaultPoint::BeforeCommit
        } else {
            FaultPoint::AfterCommit
        });
        let result = v.rmdir(ROOT, "dir");
        if before_commit {
            assert!(matches!(result, Err(Error::Injected)));
            assert_eq!(grant.info(&v).unwrap(), old);
        } else {
            result.unwrap();
            assert_eq!(grant.info(&v).unwrap().stat.links, 0);
            assert!(matches!(
                v.maintenance_status(),
                MaintenanceStatus::Failed { .. }
            ));
        }
        assert!(v.is_write_protected());
        grant.close(&mut v).unwrap();
        drop(v);
        let r =
            Recovery::prepare(s.path(), s.0.join("recovery"), RecoveryLimits::default()).unwrap();
        assert_eq!(
            r.verify().unwrap().orphan_directories,
            u64::from(!before_commit)
        );
        drop(r);
        let v = Volume::open(s.path()).unwrap();
        assert_eq!(v.lookup(ROOT, "dir").is_ok(), before_commit);
        v.verify().unwrap();
    }
}

#[cfg(feature = "fault-injection")]
#[test]
#[ignore = "only spawned by the parent with a disposable v3 fixture"]
fn directory_crash_child() {
    use portaweave_core::FaultPoint;
    let path = std::env::var_os("PW_DIR_CRASH_ROOT").expect("parent fixture required");
    let before = std::env::var("PW_DIR_CRASH_BEFORE").unwrap() == "1";
    let mut v = Volume::open(path).unwrap();
    let id = v.lookup(ROOT, "dir").unwrap();
    let _pin = v.open_directory(id).unwrap();
    v.inject_process_exit_once(if before {
        FaultPoint::BeforeCommit
    } else {
        FaultPoint::AfterCommit
    });
    let _ = v.rmdir(ROOT, "dir");
    panic!("process-exit fault did not terminate");
}

#[cfg(feature = "fault-injection")]
#[test]
fn real_process_exit_around_unlink_leaves_only_valid_versioned_recovery_states() {
    for before in [true, false] {
        let s = Scratch::new();
        let mut v = s.volume();
        let id = v.mkdir(ROOT, "dir").unwrap();
        drop(v);
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "directory_crash_child",
                "--nocapture",
            ])
            .env("PW_DIR_CRASH_ROOT", s.path())
            .env("PW_DIR_CRASH_BEFORE", if before { "1" } else { "0" })
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(86));
        let original = hashes(&s.path());
        let r =
            Recovery::prepare(s.path(), s.0.join("recovery"), RecoveryLimits::default()).unwrap();
        assert_eq!(r.verify().unwrap().orphan_directories, u64::from(!before));
        assert_eq!(hashes(&s.path()), original);
        drop(r);
        let v = Volume::open(s.path()).unwrap();
        assert_eq!(v.stat(id).is_ok(), before);
        v.verify().unwrap();
        drop(v);
        Volume::open(s.path()).unwrap().verify().unwrap();
    }
}
