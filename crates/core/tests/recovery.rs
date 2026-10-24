use portaweave_core::{BLOCK_SIZE, Error, ExportLinks, ROOT, Recovery, RecoveryLimits, Volume};
#[cfg(feature = "fault-injection")]
use portaweave_core::{FaultPoint, MaintenanceStatus};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

static NEXT: AtomicU64 = AtomicU64::new(1);
struct Lab(PathBuf);
impl Lab {
    fn new() -> Self {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("pw-recovery-{}-{time}-{n}", std::process::id()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn at(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
    fn fixture(&self) {
        let mut v = Volume::create(self.at("source"), 10_000_000).unwrap();
        let folder = v.mkdir(ROOT, "项目").unwrap();
        let id = v.create_file(folder, "hello.txt").unwrap();
        let h = v.open_file(id).unwrap();
        v.write(h, 0, b"original contents").unwrap();
        v.hard_link(id, ROOT, "alias.txt").unwrap();
        let empty = v.create_file(ROOT, "empty").unwrap();
        v.open_file(empty).unwrap();
        let sparse = v.create_file(ROOT, "sparse").unwrap();
        let h = v.open_file(sparse).unwrap();
        v.write(h, (BLOCK_SIZE * 3 + 1) as u64, b"z").unwrap();
    }
    fn prepare(&self) -> Recovery {
        Recovery::prepare(
            self.at("source"),
            self.at("workspace"),
            RecoveryLimits::default(),
        )
        .unwrap()
    }
}
impl Drop for Lab {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).expect("close all handles before cleanup");
    }
}
fn hashes(root: &Path) -> BTreeMap<PathBuf, String> {
    let mut result = BTreeMap::new();
    let mut queue = vec![root.to_path_buf()];
    while let Some(path) = queue.pop() {
        for entry in fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                queue.push(entry.path());
            } else {
                result.insert(
                    entry.path().strip_prefix(root).unwrap().to_path_buf(),
                    format!("{:x}", Sha256::digest(fs::read(entry.path()).unwrap())),
                );
            }
        }
    }
    result
}

#[test]
fn diagnosis_and_export_preserve_original_and_evidence_bytes() {
    let lab = Lab::new();
    lab.fixture();
    let before = hashes(&lab.at("source"));
    let recovery = lab.prepare();
    let evidence = hashes(&lab.at("workspace/evidence"));
    assert_eq!(before, evidence);
    let checked = recovery.verify().unwrap();
    assert_eq!(checked.files, 3);
    assert_eq!(checked.directories, 2);
    let report = recovery
        .export_new(lab.at("export"), ExportLinks::Preserve)
        .unwrap();
    assert_eq!(report.files, 4);
    assert_eq!(report.hard_links, 1);
    assert_eq!(report.copied_aliases, 0);
    assert_eq!(
        fs::read(lab.at("export/files/项目/hello.txt")).unwrap(),
        b"original contents"
    );
    assert!(fs::read(lab.at("export/files/empty")).unwrap().is_empty());
    let sparse = fs::read(lab.at("export/files/sparse")).unwrap();
    assert!(sparse[..BLOCK_SIZE * 3 + 1].iter().all(|b| *b == 0));
    assert_eq!(sparse.last(), Some(&b'z'));
    fs::write(lab.at("export/files/alias.txt"), b"linked edit").unwrap();
    assert_eq!(
        fs::read(lab.at("export/files/项目/hello.txt")).unwrap(),
        b"linked edit"
    );
    assert!(lab.at("export/COMPLETE").exists());
    assert!(!lab.at("export/INCOMPLETE").exists());
    drop(recovery);
    assert_eq!(hashes(&lab.at("source")), before);
    assert_eq!(hashes(&lab.at("workspace/evidence")), evidence);
}

#[test]
fn alias_copy_mode_is_explicit_and_reported_not_fake_link() {
    let lab = Lab::new();
    lab.fixture();
    let recovery = lab.prepare();
    let report = recovery
        .export_new(lab.at("export"), ExportLinks::CopyAliases)
        .unwrap();
    assert_eq!(report.hard_links, 0);
    assert_eq!(report.copied_aliases, 1);
    fs::write(lab.at("export/files/alias.txt"), b"independent").unwrap();
    assert_eq!(
        fs::read(lab.at("export/files/项目/hello.txt")).unwrap(),
        b"original contents"
    );
}

#[test]
fn live_owner_refused_without_creating_workspace() {
    let lab = Lab::new();
    let _owner = Volume::create(lab.at("source"), 100).unwrap();
    assert!(matches!(
        Recovery::prepare(
            lab.at("source"),
            lab.at("workspace"),
            RecoveryLimits::default()
        ),
        Err(Error::Busy)
    ));
    assert!(!lab.at("workspace").exists());
}

#[test]
fn snapshot_is_independent_and_does_not_keep_source_owner() {
    let lab = Lab::new();
    lab.fixture();
    let recovery = lab.prepare();
    let v = Volume::open(lab.at("source")).unwrap();
    drop(v);
    fs::rename(lab.at("source"), lab.at("original-renamed")).unwrap();
    recovery
        .export_new(lab.at("export"), ExportLinks::Preserve)
        .unwrap();
}

#[test]
fn existing_and_nested_destinations_never_overwritten() {
    let lab = Lab::new();
    lab.fixture();
    fs::create_dir(lab.at("exists")).unwrap();
    fs::write(lab.at("exists/keep"), b"untouched").unwrap();
    assert!(
        Recovery::prepare(
            lab.at("source"),
            lab.at("exists"),
            RecoveryLimits::default()
        )
        .is_err()
    );
    assert!(
        Recovery::prepare(
            lab.at("source"),
            lab.at("source/nested"),
            RecoveryLimits::default()
        )
        .is_err()
    );
    let recovery = lab.prepare();
    assert!(
        recovery
            .export_new(lab.at("exists"), ExportLinks::Preserve)
            .is_err()
    );
    assert!(
        recovery
            .export_new(lab.at("workspace/inside"), ExportLinks::Preserve)
            .is_err()
    );
    assert!(
        recovery
            .export_new(lab.at("source/inside"), ExportLinks::Preserve)
            .is_err()
    );
    assert_eq!(fs::read(lab.at("exists/keep")).unwrap(), b"untouched");
}

#[test]
fn snapshot_byte_budget_leaves_incomplete_evidence_and_unchanged_source() {
    let lab = Lab::new();
    lab.fixture();
    let before = hashes(&lab.at("source"));
    let limits = RecoveryLimits {
        snapshot_bytes: 1,
        ..RecoveryLimits::default()
    };
    assert!(matches!(
        Recovery::prepare(lab.at("source"), lab.at("workspace"), limits),
        Err(Error::RecoveryLimit)
    ));
    assert!(lab.at("workspace/SNAPSHOT_INCOMPLETE").exists());
    assert!(!lab.at("workspace/SNAPSHOT_COMPLETE").exists());
    assert_eq!(hashes(&lab.at("source")), before);
}

#[test]
fn logical_export_budget_counts_holes_and_prevents_target_creation() {
    let lab = Lab::new();
    lab.fixture();
    let limits = RecoveryLimits {
        export_bytes: 10,
        ..RecoveryLimits::default()
    };
    let recovery = Recovery::prepare(lab.at("source"), lab.at("workspace"), limits).unwrap();
    assert!(matches!(
        recovery.export_new(lab.at("export"), ExportLinks::Preserve),
        Err(Error::RecoveryLimit)
    ));
    assert!(!lab.at("export").exists());
}

#[test]
fn directory_depth_budget_and_row_budget_are_enforced() {
    let lab = Lab::new();
    lab.fixture();
    let limits = RecoveryLimits {
        depth: 0,
        ..RecoveryLimits::default()
    };
    let recovery = Recovery::prepare(lab.at("source"), lab.at("depth-work"), limits).unwrap();
    assert!(matches!(recovery.verify(), Err(Error::RecoveryLimit)));
    let limits = RecoveryLimits {
        catalog_rows: 8,
        ..RecoveryLimits::default()
    };
    let recovery = Recovery::prepare(lab.at("source"), lab.at("rows-work"), limits).unwrap();
    assert!(matches!(recovery.verify(), Err(Error::RecoveryLimit)));
}

#[test]
fn corrupt_block_is_detected_without_repair_or_export() {
    let lab = Lab::new();
    lab.fixture();
    let path = fs::read_dir(lab.at("source/segments"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let mut bytes = fs::read(&path).unwrap();
    bytes[24] ^= 1;
    fs::write(&path, bytes).unwrap();
    let before = hashes(&lab.at("source"));
    let recovery = lab.prepare();
    assert!(matches!(recovery.verify(), Err(Error::Corrupt(_))));
    assert!(
        recovery
            .export_new(lab.at("export"), ExportLinks::Preserve)
            .is_err()
    );
    assert!(!lab.at("export").exists());
    drop(recovery);
    assert_eq!(hashes(&lab.at("source")), before);
}

#[test]
fn invalid_directory_name_is_rejected_before_path_construction() {
    let lab = Lab::new();
    lab.fixture();
    let db = rusqlite::Connection::open(lab.at("source/catalog.sqlite")).unwrap();
    db.execute("UPDATE entries SET display_name='../outside',name_key='../outside' WHERE display_name='alias.txt'", []).unwrap();
    drop(db);
    let before = hashes(&lab.at("source"));
    let recovery = lab.prepare();
    assert!(
        recovery
            .export_new(lab.at("export"), ExportLinks::Preserve)
            .is_err()
    );
    assert_eq!(hashes(&lab.at("source")), before);
    assert!(!lab.at("outside").exists());
}

#[test]
fn diagnostic_does_not_collect_unnamed_orphan_objects() {
    let lab = Lab::new();
    let mut v = Volume::create(lab.at("source"), 100).unwrap();
    let id = v.create_file(ROOT, "orphan").unwrap();
    let h = v.open_file(id).unwrap();
    v.write(h, 0, b"not discarded").unwrap();
    v.unlink(ROOT, "orphan").unwrap();
    drop(v);
    let before = hashes(&lab.at("source"));
    let recovery = lab.prepare();
    assert_eq!(recovery.verify().unwrap().orphan_files, 1);
    assert_eq!(
        recovery
            .export_new(lab.at("export"), ExportLinks::Preserve)
            .unwrap()
            .files,
        0
    );
    assert_eq!(hashes(&lab.at("source")), before);
    assert_eq!(hashes(&lab.at("workspace/evidence")), before);
}

#[cfg(unix)]
#[test]
fn source_symlink_and_backing_symlink_are_refused() {
    use std::os::unix::fs::symlink;
    let lab = Lab::new();
    lab.fixture();
    symlink(lab.at("source"), lab.at("alias")).unwrap();
    assert!(
        Recovery::prepare(
            lab.at("alias"),
            lab.at("workspace"),
            RecoveryLimits::default()
        )
        .is_err()
    );
    fs::write(lab.at("outside"), b"not a segment").unwrap();
    symlink(lab.at("outside"), lab.at("source/segments/escape.seg")).unwrap();
    assert!(
        Recovery::prepare(
            lab.at("source"),
            lab.at("workspace"),
            RecoveryLimits::default()
        )
        .is_err()
    );
    assert_eq!(fs::read(lab.at("outside")).unwrap(), b"not a segment");
}

#[test]
fn cli_verifies_and_exports_without_mounting() {
    let lab = Lab::new();
    lab.fixture();
    let status = Command::new(env!("CARGO_BIN_EXE_portaweave-recover"))
        .arg("export")
        .arg(lab.at("source"))
        .arg(lab.at("workspace"))
        .arg(lab.at("export"))
        .status()
        .unwrap();
    assert!(status.success());
    assert!(lab.at("export/COMPLETE").exists());
    let status = Command::new(env!("CARGO_BIN_EXE_portaweave-recover"))
        .arg("verify")
        .arg(lab.at("source"))
        .arg(lab.at("workspace"))
        .status()
        .unwrap();
    assert!(!status.success());
}

#[cfg(feature = "fault-injection")]
#[test]
fn committed_write_and_namespace_result_are_not_lost_on_maintenance_failure() {
    let lab = Lab::new();
    let mut v = Volume::create(lab.at("source"), 100).unwrap();
    let id = v.create_file(ROOT, "data").unwrap();
    let h = v.open_file(id).unwrap();
    v.inject_once(FaultPoint::AfterCommit);
    assert_eq!(v.write(h, 0, b"committed").unwrap(), 9);
    assert!(matches!(
        v.maintenance_status(),
        MaintenanceStatus::Failed { .. }
    ));
    assert!(matches!(
        v.stats().unwrap().maintenance,
        MaintenanceStatus::Failed { .. }
    ));
    assert_eq!(v.read(h, 0, 100).unwrap(), b"committed");
    assert!(matches!(
        v.write(h, 0, b"retry"),
        Err(Error::WriteProtected)
    ));
    v.close_file(h).unwrap();
    drop(v);
    let mut v = Volume::open(lab.at("source")).unwrap();
    assert_eq!(v.maintenance_status(), &MaintenanceStatus::Healthy);
    v.inject_once(FaultPoint::AfterCommit);
    let new_id = v.create_file(ROOT, "created").unwrap();
    assert_eq!(v.lookup(ROOT, "created").unwrap(), new_id);
    assert!(v.is_write_protected());
}

#[cfg(feature = "fault-injection")]
#[test]
fn before_commit_error_stays_an_error_and_preserves_old_content() {
    let lab = Lab::new();
    let mut v = Volume::create(lab.at("source"), 100).unwrap();
    let id = v.create_file(ROOT, "data").unwrap();
    let h = v.open_file(id).unwrap();
    v.write(h, 0, b"OLD").unwrap();
    v.inject_once(FaultPoint::BeforeCommit);
    assert!(matches!(v.write(h, 0, b"NEW"), Err(Error::Injected)));
    assert_eq!(v.read(h, 0, 100).unwrap(), b"OLD");
    assert_eq!(v.maintenance_status(), &MaintenanceStatus::Healthy);
    assert!(v.is_write_protected());
}

#[cfg(feature = "fault-injection")]
#[test]
fn crashed_wal_is_recovered_on_copy_without_checkpointing_original() {
    for point in ["before", "after"] {
        let lab = Lab::new();
        let mut v = Volume::create(lab.at("source"), 100).unwrap();
        let id = v.create_file(ROOT, "data").unwrap();
        let h = v.open_file(id).unwrap();
        v.write(h, 0, b"OLD").unwrap();
        drop(v);
        let status = Command::new(std::env::current_exe().unwrap())
            .args(["--ignored", "--exact", "crash_helper", "--nocapture"])
            .env("PW_RECOVERY_SOURCE", lab.at("source"))
            .env("PW_RECOVERY_POINT", point)
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(86));
        assert!(lab.at("source/catalog.sqlite-wal").exists());
        let before = hashes(&lab.at("source"));
        let recovery = lab.prepare();
        recovery
            .export_new(lab.at("export"), ExportLinks::Preserve)
            .unwrap();
        assert_eq!(
            fs::read(lab.at("export/files/data")).unwrap(),
            if point == "after" { b"NEW" } else { b"OLD" }
        );
        drop(recovery);
        assert_eq!(hashes(&lab.at("source")), before);
        assert_eq!(hashes(&lab.at("workspace/evidence")), before);
    }
}

#[cfg(feature = "fault-injection")]
#[test]
#[ignore = "invoked only in a child using a disposable fixture"]
fn crash_helper() {
    let source = std::env::var_os("PW_RECOVERY_SOURCE").unwrap();
    let mut v = Volume::open(PathBuf::from(source)).unwrap();
    let h = v.open_file(v.lookup(ROOT, "data").unwrap()).unwrap();
    let point = if std::env::var("PW_RECOVERY_POINT").unwrap() == "after" {
        FaultPoint::AfterCommit
    } else {
        FaultPoint::BeforeCommit
    };
    v.inject_process_exit_once(point);
    let _ = v.write(h, 0, b"NEW");
    panic!("child did not terminate");
}
