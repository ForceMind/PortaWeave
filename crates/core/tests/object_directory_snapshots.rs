use portaweave_core::{CreateAttributes, Error, Kind, MetadataUpdate, ROOT, Volume};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Scratch(PathBuf);
const ATTR: CreateAttributes = CreateAttributes {
    uid: 100,
    gid: 20,
    mode: 0o700,
};
impl Scratch {
    fn new() -> Self {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let p = std::env::temp_dir().join(format!(
            "pw-object-dir-snapshot-{}-{stamp}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&p).unwrap();
        Self(p)
    }
    fn volume(&self, version: u8) -> Volume {
        match version {
            1 => Volume::create(self.0.join("volume"), 1 << 24).unwrap(),
            2 => Volume::create_with_metadata(self.0.join("volume"), 1 << 24, ATTR).unwrap(),
            _ => Volume::create_with_directory_lifetimes(self.0.join("volume"), 1 << 24, ATTR)
                .unwrap(),
        }
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn v1_v2_v3_capture_names_identity_metadata_together_without_live_file_or_directory_pins() {
    for version in [1, 2, 3] {
        let s = Scratch::new();
        let mut v = s.volume(version);
        let file = v.create_file(ROOT, "文件").unwrap();
        let h = v.open_file(file).unwrap();
        v.write(h, 0, b"old").unwrap();
        v.hard_link(file, ROOT, "alias").unwrap();
        let dir = v.mkdir(ROOT, "directory").unwrap();
        let before = v.object_info(ROOT).unwrap();
        let snapshot = v.directory_object_snapshot(ROOT, 3).unwrap();
        assert_eq!(snapshot.directory, before);
        assert_eq!(snapshot.parent, Some(before.clone()));
        assert_eq!(snapshot.entries.len(), 3);
        assert_eq!(v.stats().unwrap().open_handles, 1);
        for entry in &snapshot.entries {
            assert_eq!(entry.info, v.object_info(entry.info.stat.id).unwrap());
            assert_eq!(entry.info.metadata.is_some(), version != 1);
        }
        let old = snapshot
            .entries
            .iter()
            .find(|e| e.name == "文件")
            .unwrap()
            .info
            .clone();
        assert_eq!(
            (old.stat.kind, old.stat.size, old.stat.links),
            (Kind::File, 3, 2)
        );
        v.write(h, 0, b"new content").unwrap();
        if version != 1 {
            v.set_metadata(
                file,
                MetadataUpdate {
                    mode: Some(0),
                    ..Default::default()
                },
            )
            .unwrap();
        }
        v.unlink(ROOT, "alias").unwrap();
        v.unlink(ROOT, "文件").unwrap();
        v.close_file(h).unwrap();
        v.rmdir(ROOT, "directory").unwrap();
        assert!(matches!(v.stat(file), Err(Error::NotFound)));
        assert!(matches!(v.stat(dir), Err(Error::NotFound)));
        assert_eq!(
            snapshot
                .entries
                .iter()
                .find(|e| e.name == "文件")
                .unwrap()
                .info,
            old
        );
        assert_eq!(
            snapshot
                .entries
                .iter()
                .filter(|e| e.info.stat.id == file)
                .count(),
            2
        );
        assert_eq!(v.stats().unwrap().open_handles, 0);
        assert!(
            v.directory_object_snapshot(ROOT, 0)
                .unwrap()
                .entries
                .is_empty()
        );
        assert!(!v.is_write_protected());
        v.verify().unwrap();
    }
}

#[test]
fn bounded_capture_tracks_current_parent_and_empty_orphan_has_no_parent_or_path_authority() {
    let s = Scratch::new();
    let mut v = s.volume(3);
    let holder = v.mkdir(ROOT, "holder").unwrap();
    let directory = v.mkdir(ROOT, "directory").unwrap();
    let pin = v.open_directory(directory).unwrap();
    let before = v.directory_object_snapshot(directory, 0).unwrap();
    assert_eq!(before.parent.unwrap().stat.id, ROOT);
    v.rename(ROOT, "directory", holder, "moved", false).unwrap();
    let moved = v.directory_object_snapshot(directory, 0).unwrap();
    assert_eq!(moved.parent.unwrap().stat.id, holder);
    let file = v.create_file(directory, "file").unwrap();
    let parent = v.object_info(directory).unwrap();
    assert!(matches!(
        v.directory_object_snapshot(directory, 0),
        Err(Error::ResourceLimit)
    ));
    assert!(matches!(
        v.directory_object_snapshot(file, 0),
        Err(Error::NotDirectory)
    ));
    assert_eq!(v.object_info(directory).unwrap(), parent);
    assert!(!v.is_write_protected());
    v.unlink(directory, "file").unwrap();
    v.rmdir(holder, "moved").unwrap();
    let orphan = v.directory_object_snapshot(directory, 0).unwrap();
    assert_eq!(orphan.directory.stat.links, 0);
    assert!(orphan.parent.is_none() && orphan.entries.is_empty());
    assert!(matches!(
        v.create_file(directory, "forbidden"),
        Err(Error::NotFound)
    ));
    v.close_directory(pin).unwrap();
    assert!(matches!(
        v.directory_object_snapshot(directory, 0),
        Err(Error::NotFound)
    ));
    assert_eq!(before.directory.stat.id, directory);
    assert_eq!(orphan.directory.stat.id, directory);
    v.verify().unwrap();
}

#[cfg(feature = "fault-injection")]
#[test]
fn administrative_metadata_capture_is_read_only_under_latched_protection() {
    use portaweave_core::{FaultPoint, MaintenanceStatus};
    let s = Scratch::new();
    let mut v = s.volume(2);
    v.create_file(ROOT, "present").unwrap();
    let before = v.directory_object_snapshot(ROOT, 1).unwrap();
    v.inject_once(FaultPoint::BeforeCommit);
    assert!(matches!(
        v.create_file(ROOT, "refused"),
        Err(Error::Injected)
    ));
    assert!(v.is_write_protected());
    assert_eq!(v.maintenance_status(), &MaintenanceStatus::Healthy);
    assert_eq!(v.directory_object_snapshot(ROOT, 1).unwrap(), before);
    assert!(v.is_write_protected());
    v.verify().unwrap();
}
