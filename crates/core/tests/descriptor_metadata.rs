use portaweave_core::{
    CreateAttributes, Credentials, Error, FileAccess, MetadataUpdate, ROOT, Timestamp, Volume,
};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
const OLD: Timestamp = Timestamp {
    seconds: -123,
    nanoseconds: 987_654_321,
};
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "pw-descriptor-metadata-{}-{stamp}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn volume(&self) -> Volume {
        Volume::create_with_metadata(self.0.join("volume"), 1 << 20, attrs(100, 20, 0o777)).unwrap()
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
fn times() -> MetadataUpdate {
    MetadataUpdate {
        created: Some(OLD),
        accessed: Some(OLD),
        modified: Some(OLD),
        ..Default::default()
    }
}

#[test]
fn read_only_owner_sets_metadata_in_one_reply_without_changing_data_or_parent() {
    let s = Scratch::new();
    let mut v = s.volume();
    let id = v
        .create_file_with_attributes(ROOT, "file", attrs(200, 30, 0o400))
        .unwrap();
    let raw = v.open_file(id).unwrap();
    v.write(raw, 0, b"payload").unwrap();
    v.close_file(raw).unwrap();
    let reader = v
        .posix(caller(200, 30))
        .unwrap()
        .open_file(ROOT, "file", FileAccess::Read)
        .unwrap();
    let before = reader.info(&v).unwrap();
    let parent = v.metadata(ROOT).unwrap();
    let reply = v
        .posix(caller(200, 30))
        .unwrap()
        .set_file_metadata_request(reader, times())
        .unwrap();
    assert_eq!(reply, reader.info(&v).unwrap());
    assert_eq!(reply.stat.id, id);
    assert_eq!(reply.stat.size, before.stat.size);
    assert_eq!(reply.stat.generation, before.stat.generation + 1);
    assert_eq!(
        (
            reply.metadata.created,
            reply.metadata.accessed,
            reply.metadata.modified
        ),
        (OLD, OLD, OLD)
    );
    assert_eq!(
        (reply.metadata.uid, reply.metadata.gid, reply.metadata.mode),
        (200, 30, 0o400)
    );
    assert_eq!(reader.read(&v, 0, 20).unwrap(), b"payload");
    assert!(matches!(
        reader.write(&mut v, 0, b"bad"),
        Err(Error::PermissionDenied)
    ));
    assert_eq!(v.metadata(ROOT).unwrap(), parent);
    let noop = v
        .posix(caller(999, 99))
        .unwrap()
        .set_file_metadata_request(reader, MetadataUpdate::default())
        .unwrap();
    assert_eq!(noop, reply); // A live descriptor can stat without metadata authority.
    reader.close(&mut v).unwrap();
    drop(v);
    let v = Volume::open(s.0.join("volume")).unwrap();
    assert_eq!(v.metadata(id).unwrap(), reply.metadata);
}

#[test]
fn current_owner_authority_survives_unlink_but_old_data_grant_does_not_bypass_chown() {
    let s = Scratch::new();
    let mut v = s.volume();
    let id = v
        .create_file_with_attributes(ROOT, "file", attrs(200, 30, 0o666))
        .unwrap();
    let writer = v
        .posix(caller(200, 30))
        .unwrap()
        .open_file(ROOT, "file", FileAccess::ReadWrite)
        .unwrap();
    writer.write(&mut v, 0, b"old").unwrap();
    let lease = writer.acquire_range_view(&mut v, 0, 3).unwrap();
    v.set_metadata(
        id,
        MetadataUpdate {
            uid: Some(300),
            mode: Some(0),
            ..Default::default()
        },
    )
    .unwrap();
    v.rename(ROOT, "file", ROOT, "renamed", false).unwrap();
    v.unlink(ROOT, "renamed").unwrap();
    v.set_metadata(
        ROOT,
        MetadataUpdate {
            mode: Some(0),
            ..Default::default()
        },
    )
    .unwrap();
    let before = writer.info(&v).unwrap();
    assert!(matches!(
        v.posix(caller(200, 30))
            .unwrap()
            .set_file_metadata_request(writer, times()),
        Err(Error::PermissionDenied)
    ));
    assert_eq!(writer.info(&v).unwrap(), before);
    // Already granted data access still works; metadata uses the NEW owner.
    writer.write(&mut v, 0, b"new").unwrap();
    let reply = v
        .posix(caller(300, 30))
        .unwrap()
        .set_file_metadata_request(writer, times())
        .unwrap();
    assert_eq!(reply.stat.links, 0);
    assert_eq!(reply.metadata.uid, 300);
    assert_eq!(reply.metadata.modified, OLD);
    assert_eq!(lease.read(&v, 0, 3).unwrap(), b"old");
    writer.close(&mut v).unwrap();
    assert!(matches!(v.stat(id), Err(Error::NotFound)));
    assert_eq!(lease.read(&v, 0, 3).unwrap(), b"old");
    lease.close(&mut v).unwrap();
}

#[test]
fn descriptor_metadata_reuses_group_and_setid_policy_and_refuses_invalid_or_foreign_files() {
    let s = Scratch::new();
    let other = Scratch::new();
    let mut v = s.volume();
    let mut foreign = other.volume();
    let id = v
        .create_file_with_attributes(ROOT, "file", attrs(200, 30, 0o666))
        .unwrap();
    let file = v
        .posix(caller(200, 30))
        .unwrap()
        .open_file(ROOT, "file", FileAccess::Read)
        .unwrap();
    let before = file.info(&v).unwrap();
    for update in [
        MetadataUpdate {
            uid: Some(201),
            ..Default::default()
        },
        MetadataUpdate {
            gid: Some(99),
            ..Default::default()
        },
    ] {
        assert!(matches!(
            v.posix(caller(200, 30))
                .unwrap()
                .set_file_metadata_request(file, update),
            Err(Error::PermissionDenied)
        ));
        assert_eq!(file.info(&v).unwrap(), before);
    }
    assert!(matches!(
        v.posix(caller(200, 30)).unwrap().set_file_metadata_request(
            file,
            MetadataUpdate {
                accessed: Some(Timestamp {
                    seconds: 0,
                    nanoseconds: 1_000_000_000
                }),
                ..Default::default()
            }
        ),
        Err(Error::InvalidRange)
    ));
    assert!(matches!(
        foreign
            .posix(caller(0, 0))
            .unwrap()
            .set_file_metadata_request(file, times()),
        Err(Error::InvalidHandle)
    ));
    // A caller who owns the file but does not belong to its group loses SGID.
    let reply = v
        .posix(caller(200, 99))
        .unwrap()
        .set_file_metadata_request(
            file,
            MetadataUpdate {
                mode: Some(0o6751),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(reply.metadata.mode, 0o4751);
    // Explicit ownership requests clear BOTH file setid bits, even same-ID root requests.
    let reply = v
        .posix(caller(0, 0))
        .unwrap()
        .set_file_metadata_request(
            file,
            MetadataUpdate {
                uid: Some(200),
                mode: Some(0o6751),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(reply.metadata.mode, 0o751);
    file.close(&mut v).unwrap();
    assert!(matches!(
        v.posix(caller(0, 0))
            .unwrap()
            .set_file_metadata_request(file, times()),
        Err(Error::InvalidHandle)
    ));
    assert_eq!(v.metadata(id).unwrap(), reply.metadata);
    assert!(!v.is_write_protected());
}

#[test]
fn platform_reply_refusal_rolls_back_all_metadata_and_generation() {
    let s = Scratch::new();
    let mut v = s.volume();
    v.create_file_with_attributes(ROOT, "file", attrs(200, 30, 0o600))
        .unwrap();
    let file = v
        .posix(caller(200, 30))
        .unwrap()
        .open_file(ROOT, "file", FileAccess::ReadWrite)
        .unwrap();
    file.write(&mut v, 0, b"unchanged").unwrap();
    let before = file.info(&v).unwrap();
    let result: Result<(), _> = v
        .posix(caller(200, 30))
        .unwrap()
        .set_file_metadata_with_reply(file, times(), |proposed| {
            assert_eq!(proposed.stat.generation, before.stat.generation + 1);
            assert_eq!(proposed.metadata.modified, OLD);
            Err(Error::UnsupportedOperation)
        });
    assert!(matches!(result, Err(Error::UnsupportedOperation)));
    assert_eq!(file.info(&v).unwrap(), before);
    assert_eq!(file.read(&v, 0, 20).unwrap(), b"unchanged");
    assert!(!v.is_write_protected());
    file.close(&mut v).unwrap();
}

#[cfg(feature = "fault-injection")]
#[test]
fn before_commit_rolls_back_and_after_commit_returns_known_reply_with_health_warning() {
    use portaweave_core::{FaultPoint, MaintenanceStatus};
    let s = Scratch::new();
    let mut v = s.volume();
    let id = v
        .create_file_with_attributes(ROOT, "file", attrs(200, 30, 0o600))
        .unwrap();
    let file = v
        .posix(caller(200, 30))
        .unwrap()
        .open_file(ROOT, "file", FileAccess::Read)
        .unwrap();
    let before = file.info(&v).unwrap();
    v.inject_once(FaultPoint::BeforeCommit);
    assert!(matches!(
        v.posix(caller(200, 30))
            .unwrap()
            .set_file_metadata_request(file, times()),
        Err(Error::Injected)
    ));
    assert_eq!(file.info(&v).unwrap(), before);
    file.close(&mut v).unwrap(); // close under protection
    drop(v);
    let mut v = Volume::open(s.0.join("volume")).unwrap();
    let file = v
        .posix(caller(200, 30))
        .unwrap()
        .open_file(ROOT, "file", FileAccess::Read)
        .unwrap();
    v.inject_once(FaultPoint::AfterCommit);
    let reply = v
        .posix(caller(200, 30))
        .unwrap()
        .set_file_metadata_request(file, times())
        .unwrap();
    assert_eq!(reply, file.info(&v).unwrap());
    assert_eq!(reply.metadata.modified, OLD);
    assert_eq!(reply.stat.generation, before.stat.generation + 1);
    assert!(v.is_write_protected());
    assert!(matches!(
        v.maintenance_status(),
        MaintenanceStatus::Failed { .. }
    ));
    file.close(&mut v).unwrap();
    drop(v);
    let v = Volume::open(s.0.join("volume")).unwrap();
    assert_eq!(v.metadata(id).unwrap(), reply.metadata);
    assert_eq!(v.stat(id).unwrap(), reply.stat);
}
