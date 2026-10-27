//! Explicit v3 artifact handover, not an exFAT/mount/power-loss test.
//! Each outgoing owner deliberately abandons ONE pinned empty orphan. The next
//! process preserves the source before ordinary open reclaims that orphan.
use portaweave_core::{
    CreateAttributes, Credentials, Error as CoreError, ExportLinks, FileId, MetadataUpdate,
    NodeCreateOptions, ROOT, Recovery, RecoveryLimits, Timestamp, Volume,
};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, error::Error, fs, path::Path};
type TestResult = Result<(), Box<dyn Error>>;
const ATTR: CreateAttributes = CreateAttributes {
    uid: 501,
    gid: 20,
    mode: 0o777,
};
fn caller(uid: u32) -> portaweave_core::Result<Credentials> {
    Credentials::new(uid, 20, &[])
}
fn put(v: &mut Volume, name: &str, bytes: &[u8]) -> TestResult {
    let id = match v.lookup(ROOT, name) {
        Ok(id) => id,
        Err(CoreError::NotFound) => v.create_file(ROOT, name)?,
        Err(e) => return Err(e.into()),
    };
    let h = v.open_file(id)?;
    v.write(h, 0, bytes)?;
    v.truncate(h, bytes.len() as u64)?;
    v.close_file(h)?;
    Ok(())
}
fn read(v: &mut Volume, name: &str) -> Result<Vec<u8>, Box<dyn Error>> {
    let id = v.lookup(ROOT, name)?;
    let h = v.open_file(id)?;
    let data = v.read(h, 0, 100)?;
    v.close_file(h)?;
    Ok(data)
}
fn hashes(root: &Path) -> Result<BTreeMap<std::path::PathBuf, [u8; 32]>, Box<dyn Error>> {
    fn walk(
        root: &Path,
        path: &Path,
        out: &mut BTreeMap<std::path::PathBuf, [u8; 32]>,
    ) -> TestResult {
        for entry in fs::read_dir(path)? {
            let path = entry?.path();
            if path.is_dir() {
                walk(root, &path, out)?;
            } else {
                out.insert(
                    path.strip_prefix(root)?.to_owned(),
                    Sha256::digest(fs::read(path)?).into(),
                );
            }
        }
        Ok(())
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out)?;
    Ok(out)
}
fn preserve_before_open(path: &Path, expected: u8) -> TestResult {
    let before = hashes(path)?;
    let label = path
        .file_name()
        .ok_or("fixture needs a file name")?
        .to_string_lossy();
    let workspace = path.with_file_name(format!("{label}-recovery-{expected}"));
    let export = path.with_file_name(format!("{label}-export-{expected}"));
    let recovery = Recovery::prepare(path, workspace, RecoveryLimits::default())?;
    let report = recovery.verify()?;
    assert_eq!((report.orphan_directories, report.orphan_files), (1, 0));
    assert_eq!(
        recovery
            .export_new(&export, ExportLinks::Preserve)?
            .hard_links,
        1
    );
    assert_eq!(
        fs::read(export.join("files/payload"))?,
        format!("stage-{expected}").as_bytes()
    );
    assert!(!export.join("files/abandoned").exists());
    assert!(
        fs::read_to_string(export.join("report.txt"))?
            .contains("unnamed_orphan_directories_not_exported=1")
    );
    assert_eq!(hashes(path)?, before);
    println!(
        "V3_DIRECTORY_RECOVERY_ORIGINAL_SHA256_OK incoming={expected} (abandoned-owner fixture; not power loss)"
    );
    Ok(()) // source owner is released BEFORE ordinary open
}
fn exercise(v: &mut Volume, stage: u8) -> TestResult {
    let created = v.create_directory_with_pin(
        ROOT,
        "created-with-pin",
        NodeCreateOptions {
            attributes: Some(ATTR),
            modified: Some(Timestamp {
                seconds: i64::from(stage),
                nanoseconds: 456_789,
            }),
            ..Default::default()
        },
        Ok,
    )?;
    assert_eq!(created.reply, v.object_info(created.object)?);
    assert_eq!(
        created
            .reply
            .metadata
            .ok_or("missing initial directory metadata")?
            .modified,
        Timestamp {
            seconds: i64::from(stage),
            nanoseconds: 456_789
        }
    );
    assert_eq!(v.stats()?.open_handles, 1);
    v.rmdir(ROOT, "created-with-pin")?;
    assert_eq!(v.stat(created.object)?.links, 0);
    v.close_directory(created.handle)?;
    assert!(matches!(v.stat(created.object), Err(CoreError::NotFound)));
    println!(
        "DIRECTORY_CREATE_PIN_HANDOVER_OK stage={stage} (atomic v3 initial state/reference; no native mount)"
    );
    let holder = v.mkdir_with_attributes(ROOT, "holder", ATTR)?;
    let directory = v.mkdir_with_attributes(ROOT, "active", ATTR)?;
    let child = v.create_file(directory, "child")?;
    let grant = v.posix(caller(501)?)?.open_directory(directory)?;
    let captured = grant.snapshot(v, 1)?;
    assert_eq!(captured.entries[0].info.stat.id, child);
    v.rename(ROOT, "active", holder, "moved", false)?;
    v.set_metadata(
        holder,
        MetadataUpdate {
            mode: Some(0),
            ..Default::default()
        },
    )?;
    assert!(matches!(
        v.posix(caller(501)?)?.open_directory(directory),
        Err(CoreError::PermissionDenied)
    ));
    assert_eq!(
        grant
            .snapshot(v, 1)?
            .parent
            .ok_or("missing linked parent")?
            .stat
            .id,
        holder
    );
    v.unlink(directory, "child")?;
    v.rmdir(holder, "moved")?;
    assert_eq!(grant.info(v)?.stat.links, 0);
    let empty = grant.snapshot(v, 0)?;
    assert!(empty.parent.is_none() && empty.entries.is_empty());
    assert!(matches!(
        v.create_file(directory, "no-new-children"),
        Err(CoreError::NotFound)
    ));
    assert!(matches!(
        v.posix(caller(501)?)?.directory_info(directory),
        Err(CoreError::NotFound)
    ));
    v.set_metadata(
        directory,
        MetadataUpdate {
            uid: Some(502),
            ..Default::default()
        },
    )?;
    let update = MetadataUpdate {
        mode: Some(0),
        modified: Some(Timestamp {
            seconds: i64::from(stage),
            nanoseconds: 123_456_789,
        }),
        ..Default::default()
    };
    assert!(matches!(
        v.posix(caller(501)?)?
            .set_directory_metadata_with_reply(grant, update, Ok),
        Err(CoreError::PermissionDenied)
    ));
    let reply = v
        .posix(caller(502)?)?
        .set_directory_metadata_with_reply(grant, update, Ok)?;
    assert_eq!(reply, grant.info(v)?);
    v.verify()?;
    grant.close(v)?;
    assert!(matches!(v.stat(directory), Err(CoreError::NotFound)));
    assert_eq!(captured.entries[0].info.stat.id, child);
    v.rmdir(ROOT, "holder")?;

    let source = v.mkdir(ROOT, "source")?;
    let target = v.mkdir_with_attributes(ROOT, "target", ATTR)?;
    let pin = v.posix(caller(501)?)?.open_directory(target)?;
    let second = v.open_directory(target)?;
    v.rename(ROOT, "source", ROOT, "target", true)?;
    assert_eq!(v.lookup(ROOT, "target")?, source);
    assert_eq!(pin.info(v)?.stat.id, target);
    assert_eq!(pin.info(v)?.stat.links, 0);
    pin.close(v)?;
    assert_eq!(v.stat(target)?.links, 0);
    v.close_directory(second)?;
    assert!(matches!(v.stat(target), Err(CoreError::NotFound)));
    v.rmdir(ROOT, "target")?;
    assert_eq!(v.stats()?.open_handles, 0);
    assert!(!v.is_write_protected());
    v.verify()?;
    println!("V3_DIRECTORY_LIFETIME_HANDOVER_OK stage={stage} (trusted core grants; no OS mount)");
    Ok(())
}
fn abandon(v: &mut Volume) -> TestResult {
    let id = v.mkdir(ROOT, "abandoned")?;
    let pin = v.open_directory(id)?;
    v.rmdir(ROOT, "abandoned")?;
    put(v, "orphan-id", &id.raw().to_le_bytes())?;
    assert_eq!(v.stats()?.open_handles, 1);
    v.verify()?;
    // Intentionally NOT closed. Dropping this owner leaves a valid empty orphan;
    // this is neither a safe-unmount path nor a simulated hardware failure.
    let _abandoned_owner_pin = pin;
    Ok(())
}
fn seed(path: &Path) -> TestResult {
    let mut v = Volume::create_with_directory_lifetimes(path, 1 << 24, ATTR)?;
    assert_eq!(v.volume_identity()?.capabilities, 3);
    let mut marker = v.volume_identity()?.uuid.to_vec();
    marker.push(0);
    put(&mut v, "identity", &marker)?;
    put(&mut v, "payload", b"stage-0")?;
    let payload = v.lookup(ROOT, "payload")?;
    v.hard_link(payload, ROOT, "alias")?;
    exercise(&mut v, 0)?;
    abandon(&mut v)?;
    println!("V3_DIRECTORY_HANDOVER_SEED_OK (explicit v3; abandoned-owner fixture)");
    Ok(())
}
fn handover(path: &Path, expected: u8, next: Option<u8>) -> TestResult {
    preserve_before_open(path, expected)?;
    let mut v = Volume::open(path)?;
    let old_id = FileId::from_raw(i64::from_le_bytes(
        read(&mut v, "orphan-id")?
            .try_into()
            .map_err(|_| "invalid orphan marker")?,
    ))?;
    assert!(matches!(v.stat(old_id), Err(CoreError::NotFound)));
    let identity = v.volume_identity()?;
    assert_eq!(identity.capabilities, 3);
    let marker = read(&mut v, "identity")?;
    assert_eq!(&marker[..16], &identity.uuid);
    assert_eq!(marker[16], expected);
    let payload = v.lookup(ROOT, "payload")?;
    assert_eq!(v.lookup(ROOT, "alias")?, payload);
    assert_eq!(v.stat(payload)?.links, 2);
    assert_eq!(
        read(&mut v, "payload")?,
        format!("stage-{expected}").as_bytes()
    );
    exercise(&mut v, next.unwrap_or(expected))?;
    if let Some(stage) = next {
        put(&mut v, "payload", format!("stage-{stage}").as_bytes())?;
        let mut marker = identity.uuid.to_vec();
        marker.push(stage);
        put(&mut v, "identity", &marker)?;
        abandon(&mut v)?;
    }
    v.verify()?;
    println!(
        "V3_DIRECTORY_HANDOVER_OK incoming={expected} outgoing={next:?} UUID={:?}",
        identity.uuid
    );
    Ok(())
}
fn main() -> TestResult {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 2 {
        return Err("usage: directory_handover DISPOSABLE_PATH seed|macos|windows|verify".into());
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
