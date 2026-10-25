use portaweave_core::{CreateAttributes, Error, MetadataUpdate, ROOT, Timestamp, Volume};
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
            "pw-attribute-reply-{}-{stamp}-{}",
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
const EXPLICIT: Timestamp = Timestamp {
    seconds: -123,
    nanoseconds: 999_999_999,
};

#[test]
fn object_replies_preserve_v1_absence_and_v2_atomic_metadata_and_root_identity() {
    for v2 in [false, true] {
        let s = Scratch::new();
        let mut v = s.volume(v2);
        let id = v.create_file(ROOT, "file").unwrap();
        let h = v.open_file(id).unwrap();
        v.write(h, 0, b"original").unwrap();
        let before = v.object_info(id).unwrap();
        let update = MetadataUpdate {
            mode: Some(0o6751),
            modified: Some(EXPLICIT),
            ..Default::default()
        };
        if !v2 {
            assert!(matches!(
                v.truncate_with_reply(h, 0, update, Ok),
                Err(Error::UnsupportedOperation)
            ));
            assert_eq!(v.object_info(id).unwrap(), before);
        }
        let reply = v
            .truncate_with_reply(
                h,
                3,
                if v2 {
                    update
                } else {
                    MetadataUpdate::default()
                },
                Ok,
            )
            .unwrap();
        assert_eq!(reply, v.object_info(id).unwrap());
        assert_eq!(reply.stat.generation, before.stat.generation + 1);
        assert_eq!(v.read(h, 0, 20).unwrap(), b"ori");
        if v2 {
            let m = reply.metadata.unwrap();
            assert_eq!(m.mode, 0o751);
            assert_eq!(m.modified, EXPLICIT);
            let root = v
                .set_metadata_with_reply(
                    ROOT,
                    MetadataUpdate {
                        mode: Some(0o710),
                        ..Default::default()
                    },
                    Ok,
                )
                .unwrap();
            assert_eq!(root, v.object_info(ROOT).unwrap());
            assert_eq!(root.stat.id, ROOT);
            let no_change = v
                .set_metadata_with_reply(ROOT, MetadataUpdate::default(), Ok)
                .unwrap();
            assert_eq!(root, no_change);
        } else {
            assert!(reply.metadata.is_none());
            assert!(v.object_info(ROOT).unwrap().metadata.is_none());
        }
        v.close_file(h).unwrap();
        drop(v);
        let v = Volume::open(s.0.join("volume")).unwrap();
        assert_eq!(v.object_info(id).unwrap(), reply);
    }
}

#[test]
fn rejected_attribute_projection_preserves_content_metadata_generation_and_old_view() {
    let s = Scratch::new();
    let mut v = s.volume(true);
    let id = v.create_file(ROOT, "file").unwrap();
    let h = v.open_file(id).unwrap();
    v.write(h, 0, b"original").unwrap();
    let before = v.object_info(id).unwrap();
    let view = v.acquire_range_view(h, 0, 8).unwrap();
    let update = MetadataUpdate {
        mode: Some(0o777),
        modified: Some(EXPLICIT),
        ..Default::default()
    };
    let result: portaweave_core::Result<()> = v.set_metadata_with_reply(id, update, |info| {
        assert_eq!(info.metadata.unwrap().mode, 0o777);
        Err(Error::UnsupportedOperation)
    });
    assert!(matches!(result, Err(Error::UnsupportedOperation)));
    let result: portaweave_core::Result<()> = v.truncate_with_reply(h, 3, update, |info| {
        assert_eq!(info.stat.size, 3);
        Err(Error::InvalidHandle)
    });
    assert!(matches!(result, Err(Error::InvalidHandle)));
    assert_eq!(v.object_info(id).unwrap(), before);
    assert_eq!(v.read(h, 0, 20).unwrap(), b"original");
    assert_eq!(v.read_view(view, 0, 8).unwrap(), b"original");
    assert!(!v.is_write_protected());
    v.release_view(view).unwrap();
    v.close_file(h).unwrap();
}

#[cfg(feature = "fault-injection")]
#[test]
fn attribute_reply_before_commit_fault_rolls_back_size_times_mode_and_generation() {
    use portaweave_core::FaultPoint;
    for mixed in [false, true] {
        let s = Scratch::new();
        let mut v = s.volume(true);
        let id = v.create_file(ROOT, "file").unwrap();
        let h = v.open_file(id).unwrap();
        v.write(h, 0, b"original").unwrap();
        let old = v.object_info(id).unwrap();
        let update = MetadataUpdate {
            mode: Some(0o777),
            modified: Some(EXPLICIT),
            ..Default::default()
        };
        v.inject_once(FaultPoint::BeforeCommit);
        let result = if mixed {
            v.truncate_with_reply(h, 3, update, Ok)
        } else {
            v.set_metadata_with_reply(id, update, Ok)
        };
        assert!(matches!(result, Err(Error::Injected)));
        assert_eq!(v.object_info(id).unwrap(), old);
        assert_eq!(v.read(h, 0, 20).unwrap(), b"original");
        v.close_file(h).unwrap();
        drop(v);
        let v = Volume::open(s.0.join("volume")).unwrap();
        assert_eq!(v.object_info(id).unwrap(), old);
    }
}

#[cfg(feature = "fault-injection")]
#[test]
fn committed_attribute_reply_survives_maintenance_failure_and_reopen() {
    use portaweave_core::{FaultPoint, MaintenanceStatus};
    for mixed in [false, true] {
        let s = Scratch::new();
        let mut v = s.volume(true);
        let id = v.create_file(ROOT, "file").unwrap();
        let h = v.open_file(id).unwrap();
        v.write(h, 0, b"original").unwrap();
        let old = v.object_info(id).unwrap();
        let update = MetadataUpdate {
            uid: Some(200),
            mode: Some(0o6751),
            modified: Some(EXPLICIT),
            ..Default::default()
        };
        v.inject_once(FaultPoint::AfterCommit);
        let reply = if mixed {
            v.truncate_with_reply(h, 3, update, Ok).unwrap()
        } else {
            v.set_metadata_with_reply(id, update, Ok).unwrap()
        };
        assert_eq!(reply, v.object_info(id).unwrap());
        assert_eq!(reply.stat.generation, old.stat.generation + 1);
        assert_eq!(reply.metadata.unwrap().mode, 0o751);
        assert_eq!(reply.metadata.unwrap().modified, EXPLICIT);
        assert!(v.is_write_protected());
        assert!(matches!(
            v.maintenance_status(),
            MaintenanceStatus::Failed { .. }
        ));
        v.close_file(h).unwrap();
        drop(v);
        let v = Volume::open(s.0.join("volume")).unwrap();
        assert_eq!(v.object_info(id).unwrap(), reply);
    }
}
