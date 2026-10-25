use portaweave_core::{CreateAttributes, Error, Kind, NodeCreateOptions, ROOT, Timestamp, Volume};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let p = std::env::temp_dir().join(format!(
            "pw-create-state-{}-{n}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&p).unwrap();
        Self(p)
    }
    fn volume(&self, v2: bool) -> Volume {
        if v2 {
            Volume::create_with_metadata(
                self.0.join("v"),
                1 << 20,
                CreateAttributes {
                    uid: 100,
                    gid: 20,
                    mode: 0o750,
                },
            )
            .unwrap()
        } else {
            Volume::create(self.0.join("v"), 1 << 20).unwrap()
        }
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
const OLD: Timestamp = Timestamp {
    seconds: -123,
    nanoseconds: 999_999_999,
};
fn options() -> NodeCreateOptions {
    NodeCreateOptions {
        attributes: Some(CreateAttributes {
            uid: 200,
            gid: 30,
            mode: 0o6751,
        }),
        size: Some(65539),
        created: Some(OLD),
        accessed: Some(OLD),
        modified: Some(OLD),
    }
}

#[test]
fn initial_sparse_size_and_exact_v2_times_commit_as_creation_without_truncate_generation() {
    for v2 in [false, true] {
        let s = Scratch::new();
        let mut v = s.volume(v2);
        let opts = if v2 {
            options()
        } else {
            NodeCreateOptions {
                size: Some(65539),
                ..Default::default()
            }
        };
        let c = v
            .create_node_with_options(ROOT, "file", Kind::File, opts, Ok)
            .unwrap();
        assert_eq!((c.reply.stat.size, c.reply.stat.generation), (65539, 0));
        assert_eq!(c.reply, v.object_info(c.object).unwrap());
        if v2 {
            let m = c.reply.metadata.unwrap();
            assert_eq!((m.uid, m.gid, m.mode), (200, 30, 0o6751));
            assert_eq!((m.created, m.accessed, m.modified), (OLD, OLD, OLD));
            assert_ne!(m.changed, OLD);
        } else {
            assert!(c.reply.metadata.is_none());
        }
        let h = c.handle.unwrap();
        assert_eq!(v.read(h, 0, 65539).unwrap(), vec![0; 65539]);
        assert_eq!(v.stats().unwrap().tracked_segment_bytes, 0);
        v.close_file(h).unwrap();
        drop(v);
        let v = Volume::open(s.0.join("v")).unwrap();
        assert_eq!(v.object_info(c.object).unwrap(), c.reply);
    }
}

#[test]
fn invalid_initial_state_quota_and_projection_refusal_leave_no_namespace_parent_or_pin_change() {
    let s = Scratch::new();
    let mut v = s.volume(true);
    let before = v.object_info(ROOT).unwrap();
    for (kind, opts) in [
        (
            Kind::Directory,
            NodeCreateOptions {
                size: Some(0),
                ..Default::default()
            },
        ),
        (
            Kind::File,
            NodeCreateOptions {
                size: Some(1 << 21),
                ..Default::default()
            },
        ),
        (
            Kind::File,
            NodeCreateOptions {
                size: Some(u64::MAX),
                ..Default::default()
            },
        ),
        (
            Kind::File,
            NodeCreateOptions {
                created: Some(Timestamp {
                    seconds: 1,
                    nanoseconds: 1_000_000_000,
                }),
                ..Default::default()
            },
        ),
    ] {
        assert!(
            v.create_node_with_options(ROOT, "absent", kind, opts, Ok)
                .is_err()
        );
        assert!(matches!(v.lookup(ROOT, "absent"), Err(Error::NotFound)));
        assert_eq!(v.object_info(ROOT).unwrap(), before);
        assert_eq!(v.stats().unwrap().open_handles, 0);
        assert!(!v.is_write_protected());
    }
    let r: portaweave_core::Result<portaweave_core::CreatedNode<()>> =
        v.create_node_with_options(ROOT, "absent", Kind::File, options(), |info| {
            assert_eq!(info.stat.size, 65539);
            Err(Error::UnsupportedOperation)
        });
    assert!(matches!(r, Err(Error::UnsupportedOperation)));
    assert_eq!(v.object_info(ROOT).unwrap(), before);
    assert!(matches!(v.lookup(ROOT, "absent"), Err(Error::NotFound)));
    assert_eq!(v.stats().unwrap().open_handles, 0);
    let legacy = Scratch::new();
    let mut v1 = legacy.volume(false);
    assert!(matches!(
        v1.create_node_with_options(
            ROOT,
            "absent",
            Kind::File,
            NodeCreateOptions {
                created: Some(OLD),
                ..Default::default()
            },
            Ok
        ),
        Err(Error::UnsupportedOperation)
    ));
    assert!(v1.list(ROOT).unwrap().is_empty());
    let d = v
        .create_node_with_options(
            ROOT,
            "dir",
            Kind::Directory,
            NodeCreateOptions {
                created: Some(OLD),
                ..Default::default()
            },
            Ok,
        )
        .unwrap();
    assert!(d.handle.is_none());
    assert_eq!(d.reply.metadata.unwrap().created, OLD);
}

#[cfg(feature = "fault-injection")]
#[test]
fn before_commit_rolls_back_initial_state_and_parent_without_publishing_pin() {
    use portaweave_core::FaultPoint;
    let s = Scratch::new();
    let mut v = s.volume(true);
    let before = v.object_info(ROOT).unwrap();
    v.inject_once(FaultPoint::BeforeCommit);
    assert!(matches!(
        v.create_node_with_options(ROOT, "absent", Kind::File, options(), Ok),
        Err(Error::Injected)
    ));
    assert_eq!(v.object_info(ROOT).unwrap(), before);
    assert_eq!(v.stats().unwrap().open_handles, 0);
    drop(v);
    let v = Volume::open(s.0.join("v")).unwrap();
    assert!(v.list(ROOT).unwrap().is_empty());
}

#[cfg(feature = "fault-injection")]
#[test]
fn after_commit_returns_complete_initial_size_times_and_pin_under_protection() {
    use portaweave_core::{FaultPoint, MaintenanceStatus};
    let s = Scratch::new();
    let mut v = s.volume(true);
    v.inject_once(FaultPoint::AfterCommit);
    let c = v
        .create_node_with_options(ROOT, "committed", Kind::File, options(), Ok)
        .unwrap();
    assert_eq!(c.reply.stat.size, 65539);
    assert_eq!(c.reply.metadata.unwrap().modified, OLD);
    assert!(v.is_write_protected());
    assert!(matches!(
        v.maintenance_status(),
        MaintenanceStatus::Failed { .. }
    ));
    assert_eq!(v.read(c.handle.unwrap(), 65536, 3).unwrap(), [0; 3]);
    v.close_file(c.handle.unwrap()).unwrap();
    drop(v);
    let v = Volume::open(s.0.join("v")).unwrap();
    assert_eq!(v.object_info(c.object).unwrap(), c.reply);
}
