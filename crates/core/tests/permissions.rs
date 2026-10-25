use portaweave_core::{Access, CreateAttributes, Credentials, Error, MetadataUpdate, ROOT, Volume};
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
        let p = std::env::temp_dir().join(format!(
            "pw-permission-{}-{stamp}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&p).unwrap();
        Self(p)
    }
    fn volume(&self, mode: u16) -> Volume {
        Volume::create_with_metadata(self.0.join("volume"), 1 << 26, attrs(100, 10, mode)).unwrap()
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn attrs(uid: u32, gid: u32, mode: u16) -> CreateAttributes {
    CreateAttributes { uid, gid, mode }
}
fn caller(uid: u32, gid: u32) -> Credentials {
    Credentials::new(uid, gid, &[]).unwrap()
}

#[test]
fn owner_group_and_other_selection_root_execute_and_v1_refusal() {
    let s = Scratch::new();
    let mut v = s.volume(0o777);
    let id = v
        .create_file_with_attributes(ROOT, "file", attrs(200, 20, 0o047))
        .unwrap();
    assert!(matches!(
        v.posix(caller(200, 20))
            .unwrap()
            .check_access(ROOT, "file", Access::READ),
        Err(Error::PermissionDenied)
    ));
    assert!(
        v.posix(caller(300, 20))
            .unwrap()
            .check_access(ROOT, "file", Access::READ)
            .is_ok()
    );
    assert!(matches!(
        v.posix(caller(300, 20))
            .unwrap()
            .check_access(ROOT, "file", Access::WRITE),
        Err(Error::PermissionDenied)
    ));
    assert!(
        v.posix(Credentials::new(300, 50, &[20, 20, 40]).unwrap())
            .unwrap()
            .check_access(ROOT, "file", Access::READ)
            .is_ok()
    );
    assert!(
        v.posix(caller(300, 50))
            .unwrap()
            .check_access(ROOT, "file", Access::READ | Access::WRITE | Access::EXECUTE)
            .is_ok()
    );
    v.set_metadata(
        id,
        MetadataUpdate {
            mode: Some(0),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(
        v.posix(caller(0, 50))
            .unwrap()
            .check_access(ROOT, "file", Access::READ | Access::WRITE)
            .is_ok()
    );
    assert!(matches!(
        v.posix(caller(0, 50))
            .unwrap()
            .check_access(ROOT, "file", Access::EXECUTE),
        Err(Error::PermissionDenied)
    ));
    assert!(
        v.posix(caller(300, 50))
            .unwrap()
            .check_access(ROOT, "file", Access::EXISTS)
            .is_ok()
    );
    assert!(!v.is_write_protected());
    let mut old = Volume::create(s.0.join("v1"), 100).unwrap();
    assert!(matches!(
        old.posix(caller(0, 0)),
        Err(Error::UnsupportedOperation)
    ));
}

#[test]
fn knowledge_of_an_inner_directory_id_cannot_skip_ancestor_search() {
    let s = Scratch::new();
    let mut v = s.volume(0o777);
    let outer = v
        .mkdir_with_attributes(ROOT, "private", attrs(100, 10, 0o700))
        .unwrap();
    let inner = v
        .mkdir_with_attributes(outer, "inner", attrs(200, 20, 0o777))
        .unwrap();
    let file = v
        .create_file_with_attributes(inner, "file", attrs(200, 20, 0o666))
        .unwrap();
    let before = v.metadata(inner).unwrap();
    let mut p = v.posix(caller(200, 20)).unwrap();
    assert!(matches!(
        p.lookup(inner, "file"),
        Err(Error::PermissionDenied)
    ));
    assert!(matches!(
        p.stat(inner, "file"),
        Err(Error::PermissionDenied)
    ));
    assert!(matches!(
        p.check_access(inner, "file", Access::READ),
        Err(Error::PermissionDenied)
    ));
    assert!(matches!(
        p.list_bounded(inner, 100),
        Err(Error::PermissionDenied)
    ));
    assert!(matches!(
        p.create_file(inner, "absent", 0o666, 0),
        Err(Error::PermissionDenied)
    ));
    assert!(!p.is_write_protected());
    assert_eq!(v.metadata(inner).unwrap(), before);
    assert!(matches!(v.lookup(inner, "absent"), Err(Error::NotFound)));
    v.set_metadata(
        outer,
        MetadataUpdate {
            mode: Some(0o711),
            ..Default::default()
        },
    )
    .unwrap();
    let p = v.posix(caller(200, 20)).unwrap();
    assert_eq!(p.lookup(inner, "file").unwrap().id, file);
    assert!(p.list_bounded(inner, 10).is_ok());
    assert!(matches!(
        p.list_bounded(outer, 10),
        Err(Error::PermissionDenied)
    ));
}

#[test]
fn create_uses_effective_ids_umask_and_setgid_directory_inheritance() {
    let s = Scratch::new();
    let mut v = s.volume(0o777);
    let project = v
        .mkdir_with_attributes(ROOT, "group", attrs(100, 20, 0o2777))
        .unwrap();
    let mut p = v.posix(caller(200, 50)).unwrap();
    let file = p.create_file(project, "file", 0o666, 0o027).unwrap();
    let directory = p.mkdir(project, "directory", 0o1777, 0o022).unwrap();
    let ordinary = p.create_file(ROOT, "ordinary", 0o666, 0o022).unwrap();
    let m = v.metadata(file).unwrap();
    assert_eq!((m.uid, m.gid, m.mode), (200, 20, 0o640));
    let m = v.metadata(directory).unwrap();
    assert_eq!((m.uid, m.gid, m.mode), (200, 20, 0o3755));
    let m = v.metadata(ordinary).unwrap();
    assert_eq!((m.uid, m.gid, m.mode), (200, 50, 0o644));
    drop(v);
    let v = Volume::open(s.0.join("volume")).unwrap();
    assert_eq!(v.metadata(directory).unwrap().mode, 0o3755);
    v.verify().unwrap();
}

#[test]
fn namespace_write_requires_parent_write_and_search_not_file_write() {
    let s = Scratch::new();
    let mut v = s.volume(0o777);
    let readonly = v
        .mkdir_with_attributes(ROOT, "read-only", attrs(100, 20, 0o755))
        .unwrap();
    let kept = v
        .create_file_with_attributes(readonly, "owned", attrs(200, 20, 0o600))
        .unwrap();
    let root_before = v.metadata(readonly).unwrap();
    let mut p = v.posix(caller(200, 20)).unwrap();
    assert!(matches!(
        p.unlink(readonly, "owned"),
        Err(Error::PermissionDenied)
    ));
    assert!(matches!(
        p.create_file(readonly, "new", 0o600, 0),
        Err(Error::PermissionDenied)
    ));
    assert_eq!(v.lookup(readonly, "owned").unwrap(), kept);
    assert_eq!(v.metadata(readonly).unwrap(), root_before);
    let deletable = v
        .create_file_with_attributes(ROOT, "deletable", attrs(100, 20, 0o000))
        .unwrap();
    let h = v.open_file(deletable).unwrap();
    v.write(h, 0, b"old opened inode").unwrap();
    v.posix(caller(200, 20))
        .unwrap()
        .unlink(ROOT, "deletable")
        .unwrap();
    assert_eq!(v.read(h, 0, 100).unwrap(), b"old opened inode");
    v.close_file(h).unwrap();
    assert!(!v.is_write_protected());
}

#[test]
fn sticky_unlink_and_rmdir_allow_only_owner_directory_owner_or_explicit_root() {
    let s = Scratch::new();
    let mut v = s.volume(0o1777);
    let a = v
        .create_file_with_attributes(ROOT, "a", attrs(200, 20, 0o666))
        .unwrap();
    v.create_file_with_attributes(ROOT, "b", attrs(300, 20, 0o666))
        .unwrap();
    v.mkdir_with_attributes(ROOT, "directory", attrs(300, 20, 0o777))
        .unwrap();
    let before = v.metadata(ROOT).unwrap();
    let mut p = v.posix(caller(200, 20)).unwrap();
    assert!(matches!(p.unlink(ROOT, "b"), Err(Error::PermissionDenied)));
    assert!(matches!(
        p.rmdir(ROOT, "directory"),
        Err(Error::PermissionDenied)
    ));
    assert!(!p.is_write_protected());
    assert_eq!(v.metadata(ROOT).unwrap(), before);
    assert_eq!(v.lookup(ROOT, "a").unwrap(), a);
    v.posix(caller(200, 20)).unwrap().unlink(ROOT, "a").unwrap();
    v.posix(caller(100, 10)).unwrap().unlink(ROOT, "b").unwrap();
    v.posix(caller(0, 0))
        .unwrap()
        .rmdir(ROOT, "directory")
        .unwrap();
    v.verify().unwrap();
}

#[test]
fn rename_checks_both_sticky_directories_and_denial_keeps_both_inodes() {
    let s = Scratch::new();
    let mut v = s.volume(0o777);
    let from = v
        .mkdir_with_attributes(ROOT, "from", attrs(100, 10, 0o1777))
        .unwrap();
    let to = v
        .mkdir_with_attributes(ROOT, "to", attrs(500, 10, 0o1777))
        .unwrap();
    let a = v
        .create_file_with_attributes(from, "a", attrs(200, 20, 0o600))
        .unwrap();
    let b = v
        .create_file_with_attributes(to, "b", attrs(300, 20, 0o600))
        .unwrap();
    let ah = v.open_file(a).unwrap();
    let bh = v.open_file(b).unwrap();
    v.write(ah, 0, b"source").unwrap();
    v.write(bh, 0, b"target").unwrap();
    let before = (
        v.metadata(a).unwrap(),
        v.metadata(b).unwrap(),
        v.metadata(from).unwrap(),
        v.metadata(to).unwrap(),
    );
    for uid in [200, 300, 100, 500] {
        assert!(matches!(
            v.posix(caller(uid, 20))
                .unwrap()
                .rename(from, "a", to, "b", true),
            Err(Error::PermissionDenied)
        ));
        assert_eq!(v.lookup(from, "a").unwrap(), a);
        assert_eq!(v.lookup(to, "b").unwrap(), b);
        assert_eq!(
            (
                v.metadata(a).unwrap(),
                v.metadata(b).unwrap(),
                v.metadata(from).unwrap(),
                v.metadata(to).unwrap()
            ),
            before
        );
    }
    v.posix(caller(0, 0))
        .unwrap()
        .rename(from, "a", to, "b", true)
        .unwrap();
    assert_eq!(v.lookup(to, "b").unwrap(), a);
    assert_eq!(v.read(bh, 0, 100).unwrap(), b"target");
    v.close_file(ah).unwrap();
    v.close_file(bh).unwrap();
    v.verify().unwrap();
}

#[test]
fn invalid_claims_modes_masks_and_component_names_cannot_mutate() {
    assert!(matches!(
        Credentials::new(1, 1, &[1; 129]),
        Err(Error::ResourceLimit)
    ));
    assert_eq!(
        Credentials::new(1, 2, &[4, 3, 4])
            .unwrap()
            .supplementary_groups(),
        &[3, 4]
    );
    assert!(matches!(Access::from_bits(8), Err(Error::InvalidRange)));
    let s = Scratch::new();
    let mut v = s.volume(0o777);
    let before = v.metadata(ROOT).unwrap();
    let mut p = v.posix(caller(200, 20)).unwrap();
    assert!(matches!(
        p.create_file(ROOT, "mode", 0o4000, 0),
        Err(Error::InvalidRange)
    ));
    assert!(matches!(
        p.mkdir(ROOT, "mode", 0o2000, 0),
        Err(Error::InvalidRange)
    ));
    assert!(matches!(
        p.create_file(ROOT, "mask", 0o666, 0o1000),
        Err(Error::InvalidRange)
    ));
    assert!(matches!(
        p.create_file(ROOT, "../escape", 0o666, 0),
        Err(Error::InvalidName)
    ));
    assert!(p.list_bounded(ROOT, 10).unwrap().is_empty());
    assert!(!p.is_write_protected());
    assert_eq!(v.metadata(ROOT).unwrap(), before);
}

#[test]
fn directory_search_budget_is_explicit_and_refusal_does_not_poison_writes() {
    let s = Scratch::new();
    let mut v = s.volume(0o777);
    let mut parent = ROOT;
    let mut ids = Vec::new();
    for n in 0..256 {
        parent = v.mkdir(parent, &format!("d{n}")).unwrap();
        ids.push(parent);
    }
    let mut p = v.posix(caller(0, 0)).unwrap();
    p.create_file(ids[254], "within", 0o600, 0).unwrap();
    assert!(matches!(
        p.create_file(ids[255], "too-deep", 0o600, 0),
        Err(Error::ResourceLimit)
    ));
    assert!(!p.is_write_protected());
    assert!(matches!(
        v.lookup(ids[255], "too-deep"),
        Err(Error::NotFound)
    ));
}

#[cfg(feature = "fault-injection")]
#[test]
fn successful_authorized_mutation_preserves_maintenance_warning_contract() {
    let s = Scratch::new();
    let mut v = s.volume(0o777);
    v.inject_once(portaweave_core::FaultPoint::AfterCommit);
    let mut p = v.posix(caller(200, 20)).unwrap();
    let id = p.create_file(ROOT, "committed", 0o666, 0o027).unwrap();
    assert!(p.is_write_protected());
    assert!(matches!(
        p.maintenance_status(),
        portaweave_core::MaintenanceStatus::Failed { .. }
    ));
    assert_eq!(v.metadata(id).unwrap().mode, 0o640);
    drop(v);
    let v = Volume::open(s.0.join("volume")).unwrap();
    assert_eq!(v.lookup(ROOT, "committed").unwrap(), id);
}
