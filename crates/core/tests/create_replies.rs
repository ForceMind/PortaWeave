use portaweave_core::{CreateAttributes, Error, Kind, MAX_POSIX_FILES, ROOT, Volume};
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
            "pw-create-reply-{}-{stamp}-{}",
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
                1 << 24,
                CreateAttributes {
                    uid: 100,
                    gid: 20,
                    mode: 0o750,
                },
            )
            .unwrap()
        } else {
            Volume::create(self.0.join("volume"), 1 << 24).unwrap()
        }
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn create_returns_committed_reply_and_file_pin_for_v1_v2_and_directory() {
    for v2 in [false, true] {
        let s = Scratch::new();
        let mut v = s.volume(v2);
        let file = v
            .create_node_with_reply(ROOT, "file", Kind::File, None, Ok)
            .unwrap();
        assert_eq!(file.object, file.reply.stat.id);
        assert_eq!(file.reply, v.object_info(file.object).unwrap());
        assert_eq!(file.reply.metadata.is_some(), v2);
        let h = file.handle.unwrap();
        v.write(h, 0, b"pinned").unwrap();
        v.unlink(ROOT, "file").unwrap();
        assert_eq!(v.read(h, 0, 20).unwrap(), b"pinned");
        v.close_file(h).unwrap();
        assert!(matches!(v.stat(file.object), Err(Error::NotFound)));
        let dir = v
            .create_node_with_reply(ROOT, "dir", Kind::Directory, None, Ok)
            .unwrap();
        assert!(dir.handle.is_none());
        assert_eq!(dir.reply, v.object_info(dir.object).unwrap());
        if v2 {
            let m = dir.reply.metadata.unwrap();
            assert_eq!((m.uid, m.gid, m.mode), (100, 20, 0o700));
        }
        v.verify().unwrap();
    }
}

#[test]
fn creation_projection_refusal_invalid_input_and_conflict_leave_no_entry_or_pin() {
    let s = Scratch::new();
    let mut v = s.volume(true);
    let before = v.object_info(ROOT).unwrap();
    let result: portaweave_core::Result<portaweave_core::CreatedNode<()>> = v
        .create_node_with_reply(ROOT, "refused", Kind::File, None, |info| {
            assert_eq!(info.stat.links, 1);
            Err(Error::UnsupportedOperation)
        });
    assert!(matches!(result, Err(Error::UnsupportedOperation)));
    assert!(matches!(v.lookup(ROOT, "refused"), Err(Error::NotFound)));
    assert_eq!(v.stats().unwrap().open_handles, 0);
    assert_eq!(v.object_info(ROOT).unwrap(), before);
    assert!(!v.is_write_protected());
    assert!(matches!(
        v.create_node_with_reply(ROOT, "../escape", Kind::File, None, Ok),
        Err(Error::InvalidName)
    ));
    let f = v
        .create_node_with_reply(
            ROOT,
            "file",
            Kind::File,
            Some(CreateAttributes {
                uid: 200,
                gid: 30,
                mode: 0o6751,
            }),
            Ok,
        )
        .unwrap();
    let parent = v.object_info(ROOT).unwrap();
    assert!(matches!(
        v.create_node_with_reply(ROOT, "FILE", Kind::File, None, Ok),
        Err(Error::Exists)
    ));
    assert_eq!(v.object_info(ROOT).unwrap(), parent);
    assert_eq!(v.stats().unwrap().open_handles, 1);
    assert_eq!(f.reply.metadata.unwrap().mode, 0o6751);
    v.close_file(f.handle.unwrap()).unwrap();
}

#[test]
fn file_admission_counts_administrative_handles_and_does_not_block_directory_creation() {
    let s = Scratch::new();
    let mut v = s.volume(true);
    let id = v.create_file(ROOT, "source").unwrap();
    let handles: Vec<_> = (0..MAX_POSIX_FILES)
        .map(|_| v.open_file(id).unwrap())
        .collect();
    let before = v.object_info(ROOT).unwrap();
    assert!(matches!(
        v.create_node_with_reply(ROOT, "absent", Kind::File, None, Ok),
        Err(Error::ResourceLimit)
    ));
    assert!(matches!(v.lookup(ROOT, "absent"), Err(Error::NotFound)));
    assert_eq!(v.object_info(ROOT).unwrap(), before);
    assert!(!v.is_write_protected());
    let dir = v
        .create_node_with_reply(ROOT, "directory", Kind::Directory, None, Ok)
        .unwrap();
    assert!(dir.handle.is_none());
    assert_eq!(v.stats().unwrap().open_handles, MAX_POSIX_FILES);
    for h in handles {
        v.close_file(h).unwrap();
    }
    let f = v
        .create_node_with_reply(ROOT, "resumed", Kind::File, None, Ok)
        .unwrap();
    v.close_file(f.handle.unwrap()).unwrap();
}

#[cfg(feature = "fault-injection")]
#[test]
fn before_commit_create_rolls_back_parent_namespace_metadata_and_unpublished_pin() {
    use portaweave_core::FaultPoint;
    for kind in [Kind::File, Kind::Directory] {
        let s = Scratch::new();
        let mut v = s.volume(true);
        let before = v.object_info(ROOT).unwrap();
        v.inject_once(FaultPoint::BeforeCommit);
        assert!(matches!(
            v.create_node_with_reply(ROOT, "absent", kind, None, Ok),
            Err(Error::Injected)
        ));
        assert_eq!(v.object_info(ROOT).unwrap(), before);
        assert!(matches!(v.lookup(ROOT, "absent"), Err(Error::NotFound)));
        assert_eq!(v.stats().unwrap().open_handles, 0);
        drop(v);
        let v = Volume::open(s.0.join("volume")).unwrap();
        assert!(matches!(v.lookup(ROOT, "absent"), Err(Error::NotFound)));
    }
}

#[cfg(feature = "fault-injection")]
#[test]
fn after_commit_maintenance_failure_returns_complete_create_and_closeable_pin() {
    use portaweave_core::{FaultPoint, MaintenanceStatus};
    let s = Scratch::new();
    let mut v = s.volume(true);
    v.inject_once(FaultPoint::AfterCommit);
    let file = v
        .create_node_with_reply(ROOT, "committed", Kind::File, None, Ok)
        .unwrap();
    assert_eq!(file.reply, v.object_info(file.object).unwrap());
    assert_eq!(v.stats().unwrap().open_handles, 1);
    assert!(v.is_write_protected());
    assert!(matches!(
        v.maintenance_status(),
        MaintenanceStatus::Failed { .. }
    ));
    v.close_file(file.handle.unwrap()).unwrap();
    drop(v);
    let mut v = Volume::open(s.0.join("volume")).unwrap();
    assert_eq!(v.lookup(ROOT, "committed").unwrap(), file.object);
    assert_eq!(v.object_info(file.object).unwrap(), file.reply);
    v.set_metadata(file.object, portaweave_core::MetadataUpdate::default())
        .unwrap();
}
