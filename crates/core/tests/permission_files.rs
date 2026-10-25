use portaweave_core::{
    CreateAttributes, Credentials, Error, FileAccess, MAX_POSIX_FILES, MetadataUpdate, ROOT,
    Timestamp, Volume,
};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
const OLD: Timestamp = Timestamp {
    seconds: -123,
    nanoseconds: 999_999_999,
};
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let p = std::env::temp_dir().join(format!(
            "pw-permission-files-{}-{stamp}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&p).unwrap();
        Self(p)
    }
    fn volume(&self) -> Volume {
        Volume::create_with_metadata(
            self.0.join("volume"),
            1 << 26,
            CreateAttributes {
                uid: 100,
                gid: 10,
                mode: 0o777,
            },
        )
        .unwrap()
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn caller(uid: u32, gid: u32) -> Credentials {
    Credentials::new(uid, gid, &[]).unwrap()
}
fn create(v: &mut Volume, mode: u16) -> portaweave_core::FileId {
    v.create_file_with_attributes(
        ROOT,
        "file",
        CreateAttributes {
            uid: 200,
            gid: 20,
            mode,
        },
    )
    .unwrap()
}

#[test]
fn open_time_grants_enforce_read_write_and_truncate_without_side_effects() {
    let s = Scratch::new();
    let mut v = s.volume();
    let id = create(&mut v, 0o666);
    let reader = v
        .posix(caller(300, 20))
        .unwrap()
        .open_file(ROOT, "file", FileAccess::Read)
        .unwrap();
    let writer = v
        .posix(caller(300, 20))
        .unwrap()
        .open_file(ROOT, "file", FileAccess::Write)
        .unwrap();
    writer.write(&mut v, 0, b"abcdef").unwrap();
    let before = (v.stat(id).unwrap(), v.metadata(id).unwrap());
    assert_eq!(reader.read(&v, 0, 100).unwrap(), b"abcdef");
    assert!(matches!(
        reader.write(&mut v, 0, b"bad"),
        Err(Error::PermissionDenied)
    ));
    assert!(matches!(
        reader.truncate(&mut v, 0),
        Err(Error::PermissionDenied)
    ));
    assert!(matches!(
        writer.read(&v, 0, 100),
        Err(Error::PermissionDenied)
    ));
    assert_eq!((v.stat(id).unwrap(), v.metadata(id).unwrap()), before);
    assert!(!v.is_write_protected());
    assert_eq!(writer.stat(&v).unwrap().size, 6);
    reader.close(&mut v).unwrap();
    writer.close(&mut v).unwrap();
}

#[test]
fn open_grants_survive_owner_mode_ancestor_and_name_changes_until_last_close() {
    let s = Scratch::new();
    let mut v = s.volume();
    let id = create(&mut v, 0o660);
    let writer = v
        .posix(caller(200, 20))
        .unwrap()
        .open_file(ROOT, "file", FileAccess::ReadWrite)
        .unwrap();
    let reader = v
        .posix(caller(300, 20))
        .unwrap()
        .open_file(ROOT, "file", FileAccess::Read)
        .unwrap();
    writer.write(&mut v, 0, b"old").unwrap();
    v.set_metadata(
        id,
        MetadataUpdate {
            uid: Some(400),
            mode: Some(0),
            ..Default::default()
        },
    )
    .unwrap();
    v.set_metadata(
        ROOT,
        MetadataUpdate {
            mode: Some(0),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(matches!(
        v.posix(caller(200, 20))
            .unwrap()
            .open_file(ROOT, "file", FileAccess::Read),
        Err(Error::PermissionDenied)
    ));
    v.rename(ROOT, "file", ROOT, "changed", false).unwrap();
    v.unlink(ROOT, "changed").unwrap();
    writer.write(&mut v, 0, b"new").unwrap();
    assert_eq!(reader.read(&v, 0, 100).unwrap(), b"new");
    writer.close(&mut v).unwrap();
    assert_eq!(reader.read(&v, 0, 100).unwrap(), b"new");
    reader.close(&mut v).unwrap();
    assert!(matches!(v.stat(id), Err(Error::NotFound)));
    assert!(matches!(reader.read(&v, 0, 10), Err(Error::InvalidHandle)));
}

#[test]
fn copied_closed_and_foreign_volume_capabilities_cannot_reopen_or_cross_owners() {
    let s = Scratch::new();
    let mut v = s.volume();
    create(&mut v, 0o600);
    let file = v
        .posix(caller(200, 20))
        .unwrap()
        .open_file(ROOT, "file", FileAccess::ReadWrite)
        .unwrap();
    let mut other = Volume::create_with_metadata(
        s.0.join("other"),
        100,
        CreateAttributes {
            uid: 200,
            gid: 20,
            mode: 0o777,
        },
    )
    .unwrap();
    assert!(matches!(
        file.read(&other, 0, 10),
        Err(Error::InvalidHandle)
    ));
    assert!(matches!(file.close(&mut other), Err(Error::InvalidHandle)));
    file.close(&mut v).unwrap();
    assert!(matches!(file.close(&mut v), Err(Error::InvalidHandle)));
    let replacement = v
        .posix(caller(200, 20))
        .unwrap()
        .open_file(ROOT, "file", FileAccess::ReadWrite)
        .unwrap();
    assert_ne!(file, replacement);
    assert!(matches!(
        file.write(&mut v, 0, b"bad"),
        Err(Error::InvalidHandle)
    ));
    replacement.close(&mut v).unwrap();
    assert!(!v.is_write_protected());
    assert!(!other.is_write_protected());
}

#[test]
fn metadata_privileges_are_current_owner_privileges_not_descriptor_write_grants() {
    let s = Scratch::new();
    let mut v = s.volume();
    let id = create(&mut v, 0o666);
    let file = v
        .posix(caller(300, 20))
        .unwrap()
        .open_file(ROOT, "file", FileAccess::ReadWrite)
        .unwrap();
    file.write(&mut v, 0, b"abcdef").unwrap();
    let before = (v.stat(id).unwrap(), v.metadata(id).unwrap());
    let mut p = v.posix(caller(300, 20)).unwrap();
    assert!(matches!(
        p.set_metadata(
            ROOT,
            "file",
            MetadataUpdate {
                mode: Some(0o777),
                ..Default::default()
            }
        ),
        Err(Error::PermissionDenied)
    ));
    assert!(matches!(
        p.set_metadata(
            ROOT,
            "file",
            MetadataUpdate {
                modified: Some(OLD),
                ..Default::default()
            }
        ),
        Err(Error::PermissionDenied)
    ));
    assert!(matches!(
        p.truncate_with_metadata(
            file,
            0,
            MetadataUpdate {
                mode: Some(0o777),
                ..Default::default()
            }
        ),
        Err(Error::PermissionDenied)
    ));
    assert_eq!((v.stat(id).unwrap(), v.metadata(id).unwrap()), before);
    v.posix(caller(300, 20))
        .unwrap()
        .truncate_with_metadata(file, 3, MetadataUpdate::default())
        .unwrap();
    assert_eq!(file.read(&v, 0, 100).unwrap(), b"abc");
    file.close(&mut v).unwrap();
}

#[test]
fn ownership_and_chmod_restrictions_clear_file_setid_and_reject_foreign_groups() {
    let s = Scratch::new();
    let mut v = s.volume();
    let id = create(&mut v, 0o6751);
    let before = v.metadata(id).unwrap();
    let mut owner = v.posix(caller(200, 50)).unwrap();
    assert!(matches!(
        owner.set_metadata(
            ROOT,
            "file",
            MetadataUpdate {
                uid: Some(300),
                ..Default::default()
            }
        ),
        Err(Error::PermissionDenied)
    ));
    assert!(matches!(
        owner.set_metadata(
            ROOT,
            "file",
            MetadataUpdate {
                gid: Some(20),
                ..Default::default()
            }
        ),
        Err(Error::PermissionDenied)
    ));
    assert_eq!(owner.metadata(ROOT, "file").unwrap(), before);
    owner
        .set_metadata(
            ROOT,
            "file",
            MetadataUpdate {
                mode: Some(0o6751),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(owner.metadata(ROOT, "file").unwrap().mode, 0o4751);
    owner
        .set_metadata(
            ROOT,
            "file",
            MetadataUpdate {
                uid: Some(200),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(owner.metadata(ROOT, "file").unwrap().mode, 0o751);
    v.posix(Credentials::new(200, 50, &[20, 80]).unwrap())
        .unwrap()
        .set_metadata(
            ROOT,
            "file",
            MetadataUpdate {
                gid: Some(80),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(v.metadata(id).unwrap().gid, 80);
    v.posix(caller(0, 0))
        .unwrap()
        .set_metadata(
            ROOT,
            "file",
            MetadataUpdate {
                uid: Some(300),
                gid: Some(90),
                mode: Some(0o6751),
                ..Default::default()
            },
        )
        .unwrap();
    let m = v.metadata(id).unwrap();
    assert_eq!((m.uid, m.gid, m.mode), (300, 90, 0o751));
    assert!(!v.is_write_protected());
}

#[test]
fn mixed_descriptor_attributes_require_both_grants_and_commit_once() {
    let s = Scratch::new();
    let mut v = s.volume();
    let id = create(&mut v, 0o600);
    let reader = v
        .posix(caller(200, 20))
        .unwrap()
        .open_file(ROOT, "file", FileAccess::Read)
        .unwrap();
    let file = v
        .posix(caller(200, 20))
        .unwrap()
        .open_file(ROOT, "file", FileAccess::ReadWrite)
        .unwrap();
    file.write(&mut v, 0, b"abcdef").unwrap();
    let before = (v.stat(id).unwrap(), v.metadata(id).unwrap());
    let update = MetadataUpdate {
        mode: Some(0o6751),
        modified: Some(OLD),
        ..Default::default()
    };
    assert!(matches!(
        v.posix(caller(200, 20))
            .unwrap()
            .truncate_with_metadata(reader, 1, update),
        Err(Error::PermissionDenied)
    ));
    assert!(matches!(
        v.posix(caller(200, 20)).unwrap().truncate_with_metadata(
            file,
            1,
            MetadataUpdate {
                modified: Some(Timestamp {
                    seconds: 0,
                    nanoseconds: 1_000_000_000
                }),
                ..Default::default()
            }
        ),
        Err(Error::InvalidRange)
    ));
    assert_eq!((v.stat(id).unwrap(), v.metadata(id).unwrap()), before);
    v.posix(caller(200, 20))
        .unwrap()
        .truncate_with_metadata(file, 3, update)
        .unwrap();
    assert_eq!(v.stat(id).unwrap().generation, before.0.generation + 1);
    assert_eq!(v.metadata(id).unwrap().modified, OLD);
    assert_eq!(v.metadata(id).unwrap().mode, 0o751);
    assert_eq!(reader.read(&v, 0, 100).unwrap(), b"abc");
    reader.close(&mut v).unwrap();
    file.close(&mut v).unwrap();
}

#[test]
fn policy_open_admission_counts_administrative_handles_and_old_grants_expire_after_owner_drop() {
    let s = Scratch::new();
    let mut v = s.volume();
    let id = create(&mut v, 0o600);
    let _raw = v.open_file(id).unwrap();
    let mut files = Vec::new();
    let mut p = v.posix(caller(200, 20)).unwrap();
    for _ in 1..MAX_POSIX_FILES {
        files.push(p.open_file(ROOT, "file", FileAccess::Read).unwrap());
    }
    assert!(matches!(
        p.open_file(ROOT, "file", FileAccess::Read),
        Err(Error::ResourceLimit)
    ));
    // Raw administrative opens cannot bypass a budget filled by policy grants.
    assert!(matches!(v.open_file(id), Err(Error::ResourceLimit)));
    let old = files.pop().unwrap();
    old.close(&mut v).unwrap();
    let replacement = v
        .posix(caller(200, 20))
        .unwrap()
        .open_file(ROOT, "file", FileAccess::Read)
        .unwrap();
    assert_ne!(old, replacement);
    drop(v);
    let v = Volume::open(s.0.join("volume")).unwrap();
    assert!(matches!(
        replacement.read(&v, 0, 1),
        Err(Error::InvalidHandle)
    ));
    assert!(!v.is_write_protected());
}

#[cfg(feature = "fault-injection")]
#[test]
fn authorized_mixed_precommit_fault_rolls_back_and_grants_can_close_under_protection() {
    let s = Scratch::new();
    let mut v = s.volume();
    let id = create(&mut v, 0o600);
    let file = v
        .posix(caller(200, 20))
        .unwrap()
        .open_file(ROOT, "file", FileAccess::ReadWrite)
        .unwrap();
    file.write(&mut v, 0, b"abcdef").unwrap();
    let before = (v.stat(id).unwrap(), v.metadata(id).unwrap());
    v.inject_once(portaweave_core::FaultPoint::BeforeCommit);
    assert!(matches!(
        v.posix(caller(200, 20)).unwrap().truncate_with_metadata(
            file,
            3,
            MetadataUpdate {
                mode: Some(0o711),
                modified: Some(OLD),
                ..Default::default()
            }
        ),
        Err(Error::Injected)
    ));
    assert_eq!((v.stat(id).unwrap(), v.metadata(id).unwrap()), before);
    assert_eq!(file.read(&v, 0, 100).unwrap(), b"abcdef");
    file.close(&mut v).unwrap();
    assert_eq!(v.stats().unwrap().open_handles, 0);
    drop(v);
    let v = Volume::open(s.0.join("volume")).unwrap();
    assert_eq!((v.stat(id).unwrap(), v.metadata(id).unwrap()), before);
}

#[cfg(feature = "fault-injection")]
#[test]
fn authorized_committed_metadata_survives_later_maintenance_failure() {
    let s = Scratch::new();
    let mut v = s.volume();
    let id = create(&mut v, 0o600);
    let file = v
        .posix(caller(200, 20))
        .unwrap()
        .open_file(ROOT, "file", FileAccess::ReadWrite)
        .unwrap();
    file.write(&mut v, 0, b"abcdef").unwrap();
    v.inject_once(portaweave_core::FaultPoint::AfterCommit);
    v.posix(caller(200, 20))
        .unwrap()
        .truncate_with_metadata(
            file,
            3,
            MetadataUpdate {
                modified: Some(OLD),
                ..Default::default()
            },
        )
        .unwrap();
    assert!(v.is_write_protected());
    assert!(matches!(
        v.maintenance_status(),
        portaweave_core::MaintenanceStatus::Failed { .. }
    ));
    assert_eq!(file.read(&v, 0, 100).unwrap(), b"abc");
    file.close(&mut v).unwrap();
    drop(v);
    let v = Volume::open(s.0.join("volume")).unwrap();
    assert_eq!(v.stat(id).unwrap().size, 3);
    assert_eq!(v.metadata(id).unwrap().modified, OLD);
}
