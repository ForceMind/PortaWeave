use portaweave_core::{
    CreateAttributes, Credentials, Error, MetadataUpdate, ROOT, Timestamp, Volume,
};
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
            "pw-directory-snapshot-{}-{stamp}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn volume(&self) -> Volume {
        Volume::create_with_metadata(self.0.join("volume"), 1 << 20, attrs(100, 0o777)).unwrap()
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn attrs(uid: u32, mode: u16) -> CreateAttributes {
    CreateAttributes { uid, gid: 20, mode }
}
fn caller(uid: u32) -> Credentials {
    Credentials::new(uid, 20, &[]).unwrap()
}

#[test]
fn snapshot_checks_read_search_and_every_ancestor_without_requiring_child_data_access() {
    let s = Scratch::new();
    let mut v = s.volume();
    let outer = v
        .mkdir_with_attributes(ROOT, "private", attrs(100, 0o700))
        .unwrap();
    let inner = v
        .mkdir_with_attributes(outer, "inner", attrs(200, 0o700))
        .unwrap();
    let child = v
        .create_file_with_attributes(inner, "no-data-access", attrs(300, 0))
        .unwrap();
    let before = v.object_info(inner).unwrap();
    assert!(matches!(
        v.posix(caller(200)).unwrap().directory_snapshot(inner, 10),
        Err(Error::PermissionDenied)
    ));
    v.set_metadata(
        outer,
        MetadataUpdate {
            mode: Some(0o711),
            ..Default::default()
        },
    )
    .unwrap();
    let snapshot = v
        .posix(caller(200))
        .unwrap()
        .directory_snapshot(inner, 10)
        .unwrap();
    assert_eq!(snapshot.directory.stat.id, inner);
    assert_eq!(snapshot.parent.stat.id, outer);
    assert_eq!(snapshot.entries.len(), 1);
    assert_eq!(snapshot.entries[0].info.stat.id, child);
    assert_eq!(snapshot.entries[0].info.metadata.mode, 0);
    assert_eq!(v.object_info(inner).unwrap(), before);
    for mode in [0o400, 0o100, 0] {
        v.set_metadata(
            inner,
            MetadataUpdate {
                mode: Some(mode),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(matches!(
            v.posix(caller(200)).unwrap().directory_snapshot(inner, 10),
            Err(Error::PermissionDenied)
        ));
    }
    assert!(!v.is_write_protected());
    assert!(
        v.posix(caller(0))
            .unwrap()
            .directory_snapshot(inner, 10)
            .is_ok()
    ); // explicit profile privilege
}

#[test]
fn names_aliases_attributes_and_parent_are_immutable_without_pinning_data_or_inodes() {
    let s = Scratch::new();
    let mut v = s.volume();
    let dir = v.mkdir(ROOT, "directory").unwrap();
    let other = v.mkdir(ROOT, "other").unwrap();
    let file = v.create_file(dir, "cafe\u{301}-😀").unwrap();
    let h = v.open_file(file).unwrap();
    v.write(h, 0, b"old bytes").unwrap();
    v.close_file(h).unwrap();
    let time = Timestamp {
        seconds: -123,
        nanoseconds: 987_654_321,
    };
    v.set_metadata(
        file,
        MetadataUpdate {
            mode: Some(0o400),
            modified: Some(time),
            ..Default::default()
        },
    )
    .unwrap();
    v.hard_link(file, dir, "alias").unwrap();
    let snapshot = v
        .posix(caller(100))
        .unwrap()
        .directory_snapshot(dir, 2)
        .unwrap();
    let saved = snapshot.clone();
    assert_eq!(snapshot.entries.len(), 2);
    for entry in &snapshot.entries {
        assert_eq!(entry.info.stat.id, file);
        assert_eq!(entry.info.stat.links, 2);
        assert_eq!(entry.info.stat.size, 9);
        assert_eq!(entry.info.metadata.modified, time);
    }
    assert_eq!(v.stats().unwrap().open_handles, 0);
    assert_eq!(v.stats().unwrap().read_views, 0);
    v.rename(ROOT, "directory", other, "moved", false).unwrap();
    v.unlink(dir, "alias").unwrap();
    v.unlink(dir, "café-😀").unwrap();
    assert!(matches!(v.stat(file), Err(Error::NotFound))); // metadata snapshot does not keep an inode alive
    v.create_file(dir, "arrived").unwrap();
    let current = v
        .posix(caller(100))
        .unwrap()
        .directory_snapshot(dir, 2)
        .unwrap();
    assert_eq!(current.parent.stat.id, other);
    assert_eq!(current.entries[0].name, "arrived");
    assert_eq!(snapshot, saved);
    assert_eq!(snapshot.parent.stat.id, ROOT);
    drop(v);
    assert_eq!(snapshot, saved); // plain owned data outlives engine ownership
}

#[test]
fn empty_bounded_and_legacy_cases_preserve_metadata_and_health() {
    let s = Scratch::new();
    let mut v = s.volume();
    let empty = v
        .posix(caller(100))
        .unwrap()
        .directory_snapshot(ROOT, 0)
        .unwrap();
    assert!(empty.entries.is_empty());
    assert_eq!(empty.directory, empty.parent);
    v.create_file(ROOT, "a").unwrap();
    v.create_file(ROOT, "b").unwrap();
    let before = v.object_info(ROOT).unwrap();
    assert!(matches!(
        v.posix(caller(100)).unwrap().directory_snapshot(ROOT, 1),
        Err(Error::ResourceLimit)
    ));
    assert_eq!(v.object_info(ROOT).unwrap(), before);
    assert!(!v.is_write_protected());
    assert_eq!(
        v.posix(caller(100))
            .unwrap()
            .directory_snapshot(ROOT, 2)
            .unwrap()
            .entries
            .len(),
        2
    );
    let mut legacy = Volume::create(s.0.join("v1"), 100).unwrap();
    assert!(matches!(
        legacy.posix(caller(0)),
        Err(Error::UnsupportedOperation)
    ));
}

#[cfg(feature = "fault-injection")]
#[test]
fn protected_engine_can_observe_an_authorized_snapshot_without_running_maintenance() {
    use portaweave_core::FaultPoint;
    let s = Scratch::new();
    let mut v = s.volume();
    let file = v.create_file(ROOT, "file").unwrap();
    let saved = v
        .posix(caller(100))
        .unwrap()
        .directory_snapshot(ROOT, 1)
        .unwrap();
    v.inject_once(FaultPoint::BeforeCommit);
    assert!(matches!(
        v.set_metadata(
            file,
            MetadataUpdate {
                mode: Some(0),
                ..Default::default()
            }
        ),
        Err(Error::Injected)
    ));
    assert!(v.is_write_protected());
    assert_eq!(
        v.posix(caller(100))
            .unwrap()
            .directory_snapshot(ROOT, 1)
            .unwrap(),
        saved
    );
    assert!(v.is_write_protected());
}
