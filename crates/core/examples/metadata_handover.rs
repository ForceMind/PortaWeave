//! Same opt-in v2 bytes through Linux -> macOS -> Windows -> Linux.
//! Artifact handover only: no mount, exFAT medium or caller authorization.
use portaweave_core::{CreateAttributes, MetadataUpdate, ROOT, Timestamp, Volume};
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
    let id = v.create_file_with_attributes(ROOT, "code", FILE_ATTR)?;
    let h = v.open_file(id)?;
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
    let identity = v.volume_identity()?;
    let marker = v.create_file(ROOT, "identity")?;
    let h = v.open_file(marker)?;
    let mut data = identity.uuid.to_vec();
    data.push(0);
    v.write(h, 0, &data)?;
    v.close_file(h)?;
    v.verify()?;
    println!("METADATA_HANDOVER_SEED_OK {:?}", identity.uuid);
    Ok(())
}
fn handover(path: &Path, expected: u8, next: Option<u8>) -> TestResult {
    let mut v = Volume::open(path)?;
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
    if let Some(stage) = next {
        v.write(h, 0, format!("stage-{stage}").as_bytes())?;
        v.set_metadata(
            id,
            MetadataUpdate {
                modified: Some(stage_time(stage)),
                ..Default::default()
            },
        )?;
        v.rename(ROOT, "code", ROOT, "CODE", false)?;
        let mut data = identity.uuid.to_vec();
        data.push(stage);
        v.write(mh, 0, &data)?;
    }
    v.close_file(h)?;
    v.close_file(mh)?;
    v.verify()?;
    assert_eq!(v.metadata(ROOT)?.uid, 501);
    assert_eq!(v.volume_identity()?, identity);
    println!(
        "METADATA_HANDOVER_OK incoming={expected} outgoing={next:?} UUID={:?}",
        identity.uuid
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
