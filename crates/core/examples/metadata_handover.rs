//! Same opt-in v2 bytes through Linux -> macOS -> Windows -> Linux.
//! Artifact handover only: no mount, exFAT medium or caller authorization.
use portaweave_core::{
    Access, CreateAttributes, Credentials, Error as CoreError, FileAccess, Kind, MetadataUpdate,
    ROOT, Timestamp, Volume, WritePosition,
};
use std::{error::Error, path::Path};
type TestResult = Result<(), Box<dyn Error>>;
const ROOT_ATTR: CreateAttributes = CreateAttributes {
    uid: 501,
    gid: 20,
    mode: 0o750,
};
const FILE_ATTR: CreateAttributes = CreateAttributes {
    uid: u32::MAX,
    gid: 65535,
    mode: 0o6751,
};
const OLD: Timestamp = Timestamp {
    seconds: -123,
    nanoseconds: 999_999_999,
};
fn stage_time(stage: u8) -> Timestamp {
    Timestamp {
        seconds: 1_700_000_000 + i64::from(stage),
        nanoseconds: 987_654_321,
    }
}
fn seed(path: &Path) -> TestResult {
    let mut v = Volume::create_with_metadata(path, 1 << 24, ROOT_ATTR)?;
    let created = v.create_node_with_reply(ROOT, "code", Kind::File, Some(FILE_ATTR), Ok)?;
    assert_eq!(created.reply, v.object_info(created.object)?);
    let id = created.object;
    let h = created.handle.ok_or("missing creation pin")?;
    v.write(h, 0, b"stage-0")?;
    v.close_file(h)?;
    v.set_metadata(
        id,
        MetadataUpdate {
            created: Some(OLD),
            accessed: Some(OLD),
            modified: Some(stage_time(0)),
            ..Default::default()
        },
    )?;
    v.hard_link(id, ROOT, "alias")?;
    v.posix(Credentials::new(501, 50, &[])?)?
        .create_file(ROOT, "policy", 0o666, 0o027)?;
    let policy_file = v.posix(Credentials::new(501, 50, &[])?)?.open_file(
        ROOT,
        "policy",
        FileAccess::ReadWrite,
    )?;
    policy_file.write(&mut v, 0, b"policy")?;
    policy_file.close(&mut v)?;
    let atomic = v.create_file_with_attributes(ROOT, "atomic", FILE_ATTR)?;
    let h = v.open_file(atomic)?;
    v.write(h, 0, b"abcdef")?;
    v.truncate_with_metadata(
        h,
        3,
        MetadataUpdate {
            uid: Some(200),
            mode: Some(0o6751),
            modified: Some(stage_time(0)),
            ..Default::default()
        },
    )?;
    v.close_file(h)?;
    let identity = v.volume_identity()?;
    let marker = v.create_file(ROOT, "identity")?;
    let h = v.open_file(marker)?;
    let mut data = identity.uuid.to_vec();
    data.push(0);
    v.write(h, 0, &data)?;
    v.close_file(h)?;
    v.verify()?;
    verify_policy(&mut v)?;
    verify_creation(&mut v)?;
    println!("METADATA_HANDOVER_SEED_OK {:?}", identity.uuid);
    Ok(())
}
fn verify_creation(v: &mut Volume) -> TestResult {
    let created = v.create_node_with_reply(ROOT, "temporary-create", Kind::File, None, Ok)?;
    assert_eq!(created.reply, v.object_info(created.object)?);
    let h = created.handle.ok_or("missing creation pin")?;
    v.write(h, 0, b"pinned")?;
    v.unlink(ROOT, "temporary-create")?;
    assert_eq!(v.read(h, 0, 6)?, b"pinned");
    v.close_file(h)?;
    assert!(matches!(v.stat(created.object), Err(CoreError::NotFound)));
    println!("CREATE_REPLY_HANDOVER_OK (new process-local pin; no native mount)");
    Ok(())
}
fn verify_policy(v: &mut Volume) -> TestResult {
    let before = v.metadata(ROOT)?;
    let mut p = v.posix(Credentials::new(502, 20, &[50])?)?;
    p.check_access(ROOT, "policy", Access::READ)?;
    assert!(matches!(
        p.check_access(ROOT, "policy", Access::WRITE),
        Err(CoreError::PermissionDenied)
    ));
    assert!(matches!(
        p.create_file(ROOT, "forbidden", 0o600, 0),
        Err(CoreError::PermissionDenied)
    ));
    let reader = p.open_file(ROOT, "policy", FileAccess::Read)?;
    assert!(matches!(
        p.set_metadata(
            ROOT,
            "policy",
            MetadataUpdate {
                mode: Some(0o777),
                ..Default::default()
            }
        ),
        Err(CoreError::PermissionDenied)
    ));
    assert!(!p.is_write_protected());
    let lease = reader.acquire_range_view(v, 1, 3)?;
    assert_eq!(reader.read(v, 0, 100)?, b"policy");
    assert!(matches!(
        reader.write(v, 0, b"bad"),
        Err(CoreError::PermissionDenied)
    ));
    reader.close(v)?;
    let writer = v.posix(Credentials::new(501, 50, &[])?)?.open_file(
        ROOT,
        "policy",
        FileAccess::ReadWrite,
    )?;
    writer.write(v, 0, b"POLICY")?;
    assert_eq!(lease.read(v, 1, 3)?, b"oli");
    writer.write(v, 0, b"policy")?;
    let appended = writer.write_request(v, WritePosition::EndOfFile, b"!", false)?;
    assert_eq!(
        (appended.offset, appended.written, appended.info.stat.size),
        (6, 1, 7)
    );
    assert_eq!(appended.info, writer.info(v)?);
    let clipped = writer.write_request(v, WritePosition::Offset(6), b"XYZ", true)?;
    assert_eq!((clipped.written, clipped.info.stat.size), (1, 7));
    assert_eq!(writer.read(v, 0, 10)?, b"policyX");
    let restored = writer.truncate_request(v, 6)?;
    assert_eq!(restored, writer.info(v)?);
    let no_change = writer.write_request(v, WritePosition::EndOfFile, b"blocked", true)?;
    assert_eq!(no_change.written, 0);
    assert_eq!(no_change.info, restored);
    writer.close(v)?;
    assert_eq!(lease.read(v, 1, 3)?, b"oli");
    lease.close(v)?;
    assert_eq!(v.metadata(ROOT)?, before);
    assert!(matches!(
        v.lookup(ROOT, "forbidden"),
        Err(CoreError::NotFound)
    ));
    println!("WRITE_REQUEST_HANDOVER_OK (atomic append/constrained/truncate replies; no OS mount)");
    println!("POSIX_POLICY_HANDOVER_OK (trusted claims, not native authorization)");
    println!("POSIX_DESCRIPTOR_HANDOVER_OK (read grants and current metadata privileges)");
    println!(
        "POSIX_LEASE_HANDOVER_OK (read-authorized immutable range after source close/overwrite)"
    );
    Ok(())
}
fn handover(path: &Path, expected: u8, next: Option<u8>) -> TestResult {
    let mut v = Volume::open(path)?;
    verify_policy(&mut v)?;
    verify_creation(&mut v)?;
    let identity = v.volume_identity()?;
    let marker = v.lookup(ROOT, "identity")?;
    let mh = v.open_file(marker)?;
    let data = v.read(mh, 0, 17)?;
    assert_eq!(&data[..16], &identity.uuid);
    assert_eq!(data[16], expected);
    let id = v.lookup(ROOT, "code")?;
    assert_eq!(id, v.lookup(ROOT, "alias")?);
    let meta = v.metadata(id)?;
    assert_eq!((meta.uid, meta.gid, meta.mode), (u32::MAX, 65535, 0o751));
    assert_eq!(meta.created, OLD);
    assert_eq!(meta.accessed, OLD);
    assert_eq!(meta.modified, stage_time(expected));
    let h = v.open_file(id)?;
    assert_eq!(v.read(h, 0, 100)?, format!("stage-{expected}").as_bytes());
    let atomic = v.lookup(ROOT, "atomic")?;
    let atomic_handle = v.open_file(atomic)?;
    let atomic_meta = v.metadata(atomic)?;
    assert_eq!(
        (atomic_meta.uid, atomic_meta.mode),
        (200 + u32::from(expected), 0o751)
    );
    assert_eq!(atomic_meta.modified, stage_time(expected));
    assert_eq!(
        v.read(atomic_handle, 0, 10)?,
        if expected == 1 {
            b"abc\0\0\0".as_slice()
        } else {
            b"abc".as_slice()
        }
    );
    if let Some(stage) = next {
        let atomic_reply = v.truncate_with_reply(
            atomic_handle,
            if stage == 1 { 6 } else { 3 },
            MetadataUpdate {
                uid: Some(200 + u32::from(stage)),
                mode: Some(0o6751),
                modified: Some(stage_time(stage)),
                ..Default::default()
            },
            Ok,
        )?;
        assert_eq!(atomic_reply, v.object_info(atomic)?);
        v.write(h, 0, format!("stage-{stage}").as_bytes())?;
        let metadata_reply = v.set_metadata_with_reply(
            id,
            MetadataUpdate {
                modified: Some(stage_time(stage)),
                ..Default::default()
            },
            Ok,
        )?;
        assert_eq!(metadata_reply, v.object_info(id)?);
        v.rename(ROOT, "code", ROOT, "CODE", false)?;
        let mut data = identity.uuid.to_vec();
        data.push(stage);
        v.write(mh, 0, &data)?;
    }
    v.close_file(h)?;
    v.close_file(atomic_handle)?;
    v.close_file(mh)?;
    v.verify()?;
    assert_eq!(v.metadata(ROOT)?.uid, 501);
    assert_eq!(v.volume_identity()?, identity);
    println!(
        "METADATA_HANDOVER_OK incoming={expected} outgoing={next:?} UUID={:?}",
        identity.uuid
    );
    println!("ATOMIC_ATTRIBUTES_HANDOVER_OK incoming={expected} outgoing={next:?}");
    println!(
        "ATTRIBUTE_REPLY_HANDOVER_OK incoming={expected} outgoing={next:?} (core replies, not OS dispatch)"
    );
    Ok(())
}
fn main() -> TestResult {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 2 {
        return Err("usage: metadata_handover DISPOSABLE_PATH seed|macos|windows|verify".into());
    }
    let path = Path::new(&args[0]);
    match args[1].to_str() {
        Some("seed") => seed(path),
        Some("macos") => handover(path, 0, Some(1)),
        Some("windows") => handover(path, 1, Some(2)),
        Some("verify") => handover(path, 2, None),
        _ => Err("unknown stage".into()),
    }
}
