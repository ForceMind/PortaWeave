use portaweave_core::{
    CreateAttributes, CreatedDirectory, Error, Kind, MetadataUpdate, NodeCreateOptions, ROOT,
    Timestamp, Volume,
};
#[cfg(feature = "fault-injection")]
use portaweave_core::{Recovery, RecoveryLimits};
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
    mode: 0o750,
};
impl Scratch {
    fn new() -> Self {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let p = std::env::temp_dir().join(format!(
            "pw-dir-create-pin-{}-{stamp}-{}",
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
        Volume::create_with_directory_lifetimes(self.path(), 1 << 20, ATTR).unwrap()
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn explicit_directory_create_publishes_its_pin_and_precommit_initial_state() {
    let s = Scratch::new();
    let mut v = s.volume();
    let old = Timestamp {
        seconds: -12,
        nanoseconds: 123_456_789,
    };
    let made = v
        .create_directory_with_pin(
            ROOT,
            "created",
            NodeCreateOptions {
                attributes: Some(CreateAttributes {
                    uid: 200,
                    gid: 30,
                    mode: 0o2700,
                }),
                created: Some(old),
                accessed: Some(old),
                modified: Some(old),
                ..Default::default()
            },
            Ok,
        )
        .unwrap();
    assert_eq!(made.reply, v.object_info(made.object).unwrap());
    assert_eq!(
        (
            made.reply.stat.kind,
            made.reply.stat.size,
            made.reply.stat.generation
        ),
        (Kind::Directory, 0, 0)
    );
    let meta = made.reply.metadata.unwrap();
    assert_eq!((meta.uid, meta.gid, meta.mode), (200, 30, 0o2700));
    assert_eq!(
        (meta.created, meta.accessed, meta.modified),
        (old, old, old)
    );
    assert_eq!(v.stats().unwrap().open_handles, 1);
    let second = v.open_directory(made.object).unwrap();
    v.rmdir(ROOT, "created").unwrap();
    assert_eq!(v.stat(made.object).unwrap().links, 0);
    v.set_metadata(
        made.object,
        MetadataUpdate {
            mode: Some(0),
            ..Default::default()
        },
    )
    .unwrap();
    v.close_directory(made.handle).unwrap();
    assert_eq!(v.stat(made.object).unwrap().links, 0);
    v.close_directory(second).unwrap();
    assert!(matches!(v.stat(made.object), Err(Error::NotFound)));
    assert!(matches!(
        v.close_directory(made.handle),
        Err(Error::InvalidHandle)
    ));
    let default = v
        .create_node_with_reply(ROOT, "ordinary", Kind::Directory, None, Ok)
        .unwrap();
    assert!(default.handle.is_none());
    assert_eq!(v.stats().unwrap().open_handles, 0);
    v.verify().unwrap();
}

#[test]
fn legacy_formats_and_refused_projections_publish_no_directory_or_pin() {
    for v2 in [false, true] {
        let s = Scratch::new();
        let mut v = if v2 {
            Volume::create_with_metadata(s.path(), 100, ATTR).unwrap()
        } else {
            Volume::create(s.path(), 100).unwrap()
        };
        let marker = fs::read(s.path().join("format.txt")).unwrap();
        assert!(matches!(
            v.create_directory_with_pin(ROOT, "unsupported", Default::default(), Ok),
            Err(Error::UnsupportedOperation)
        ));
        assert!(v.list(ROOT).unwrap().is_empty());
        assert_eq!(v.stats().unwrap().open_handles, 0);
        assert_eq!(fs::read(s.path().join("format.txt")).unwrap(), marker);
        assert!(!v.is_write_protected());
    }
    let s = Scratch::new();
    let mut v = s.volume();
    let before = v.object_info(ROOT).unwrap();
    let result: portaweave_core::Result<CreatedDirectory<()>> =
        v.create_directory_with_pin(ROOT, "refused", Default::default(), |info| {
            assert_eq!((info.stat.kind, info.stat.links), (Kind::Directory, 1));
            Err(Error::UnsupportedOperation)
        });
    assert!(matches!(result, Err(Error::UnsupportedOperation)));
    for options in [
        NodeCreateOptions {
            size: Some(0),
            ..Default::default()
        },
        NodeCreateOptions {
            modified: Some(Timestamp {
                seconds: 0,
                nanoseconds: 1_000_000_000,
            }),
            ..Default::default()
        },
    ] {
        assert!(
            v.create_directory_with_pin(ROOT, "refused", options, Ok)
                .is_err()
        );
    }
    assert!(matches!(
        v.create_directory_with_pin(ROOT, "../escape", Default::default(), Ok),
        Err(Error::InvalidName)
    ));
    assert_eq!(v.object_info(ROOT).unwrap(), before);
    assert!(v.list(ROOT).unwrap().is_empty());
    assert_eq!(v.stats().unwrap().open_handles, 0);
    assert!(!v.is_write_protected());
    let made = v
        .create_directory_with_pin(ROOT, "created", Default::default(), Ok)
        .unwrap();
    let parent = v.object_info(ROOT).unwrap();
    assert!(matches!(
        v.create_directory_with_pin(ROOT, "CREATED", Default::default(), Ok),
        Err(Error::Exists)
    ));
    assert_eq!(v.object_info(ROOT).unwrap(), parent);
    assert_eq!(v.stats().unwrap().open_handles, 1);
    v.close_directory(made.handle).unwrap();
    v.verify().unwrap();
}

#[cfg(feature = "fault-injection")]
#[test]
fn directory_create_commit_faults_keep_reply_pin_and_business_health_separate() {
    use portaweave_core::{FaultPoint, MaintenanceStatus};
    for before in [true, false] {
        let s = Scratch::new();
        let mut v = s.volume();
        let parent = v.object_info(ROOT).unwrap();
        v.inject_once(if before {
            FaultPoint::BeforeCommit
        } else {
            FaultPoint::AfterCommit
        });
        let result = v.create_directory_with_pin(ROOT, "dir", Default::default(), Ok);
        if before {
            assert!(matches!(result, Err(Error::Injected)));
            assert_eq!(v.object_info(ROOT).unwrap(), parent);
            assert_eq!(v.stats().unwrap().open_handles, 0);
        } else {
            let created = result.unwrap();
            assert_eq!(created.reply, v.object_info(created.object).unwrap());
            assert_eq!(v.stats().unwrap().open_handles, 1);
            assert!(matches!(
                v.maintenance_status(),
                MaintenanceStatus::Failed { .. }
            ));
            v.close_directory(created.handle).unwrap();
            assert_eq!(v.stats().unwrap().open_handles, 0);
        }
        assert!(v.is_write_protected());
        drop(v);
        let r =
            Recovery::prepare(s.path(), s.0.join("recovery"), RecoveryLimits::default()).unwrap();
        assert_eq!(r.verify().unwrap().directories, if before { 1 } else { 2 });
        drop(r);
        let v = Volume::open(s.path()).unwrap();
        assert_eq!(v.lookup(ROOT, "dir").is_ok(), !before);
        v.verify().unwrap();
    }
}

#[cfg(feature = "fault-injection")]
#[test]
#[ignore = "parent-only disposable fixture and actual process exit"]
fn directory_create_crash_child() {
    use portaweave_core::FaultPoint;
    let path = std::env::var_os("PW_DIR_CREATE_CRASH_ROOT").expect("parent fixture required");
    let before = std::env::var("PW_DIR_CREATE_CRASH_BEFORE").unwrap() == "1";
    let mut v = Volume::open(path).unwrap();
    v.inject_process_exit_once(if before {
        FaultPoint::BeforeCommit
    } else {
        FaultPoint::AfterCommit
    });
    let _ = v.create_directory_with_pin(ROOT, "dir", Default::default(), Ok);
    panic!("process exit did not fire");
}

#[cfg(feature = "fault-injection")]
#[test]
fn actual_process_exit_before_and_after_pinned_directory_create_recovers_valid_names() {
    for before in [true, false] {
        let s = Scratch::new();
        drop(s.volume());
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "directory_create_crash_child",
                "--nocapture",
            ])
            .env("PW_DIR_CREATE_CRASH_ROOT", s.path())
            .env("PW_DIR_CREATE_CRASH_BEFORE", if before { "1" } else { "0" })
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(86));
        let r =
            Recovery::prepare(s.path(), s.0.join("recovery"), RecoveryLimits::default()).unwrap();
        let report = r.verify().unwrap();
        assert_eq!(report.directories, if before { 1 } else { 2 });
        assert_eq!(report.orphan_directories, 0);
        drop(r);
        let v = Volume::open(s.path()).unwrap();
        assert_eq!(v.lookup(ROOT, "dir").is_ok(), !before);
        v.verify().unwrap();
    }
}
