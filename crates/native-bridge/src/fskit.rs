//! Pure FSKit laboratory attribute projection. No OS caller authentication.
use super::{Fault, OK, PwMetadata, PwMetadataUpdate, readable, required, run, with_session};
use portaweave_core::{Error, Kind, MetadataUpdate, ObjectInfo, Result};

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PwFsKitAttributes {
    pub file_id: u64, // Core ROOT=1 maps to FSKit rootDirectory=2
    pub size: u64,
    pub kind: u32,
    pub links: u32,          // valid for regular files only
    pub metadata_valid: u32, // 0=v1 missing; 1=v2 stored values
    pub reserved: u32,
    pub metadata: PwMetadata,
}
fn project(info: ObjectInfo, expected: i64) -> Result<PwFsKitAttributes> {
    if info.stat.id.raw() != expected {
        return Err(Error::InvalidHandle);
    }
    let links = if info.stat.kind == Kind::File {
        u32::try_from(info.stat.links).map_err(|_| Error::UnsupportedOperation)?
    } else {
        0
    };
    if let Some(m) = info.metadata {
        for t in [m.created, m.accessed, m.modified, m.changed] {
            if t.nanoseconds >= 1_000_000_000 || isize::try_from(t.seconds).is_err() {
                return Err(Error::UnsupportedOperation);
            }
        }
    }
    Ok(PwFsKitAttributes {
        file_id: info.stat.id.raw() as u64 + 1,
        size: info.stat.size,
        kind: if info.stat.kind == Kind::File { 0 } else { 1 },
        links,
        metadata_valid: u32::from(info.metadata.is_some()),
        reserved: 0,
        metadata: info.metadata.map(Into::into).unwrap_or_default(),
    })
}

/// Trusted lab metadata setter. Captures a fully projected reply before commit.
/// Empty requests observe v1/v2 without inventing metadata or running a mutation.
/// # Safety
/// Update readable/aligned, output writable/aligned/nonaliasing for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_fskit_metadata_set(
    volume: u64,
    object: i64,
    update: *const PwMetadataUpdate,
    out: *mut PwFsKitAttributes,
) -> i32 {
    run(|| {
        required(out)?;
        readable(update, 1)?;
        let update = unsafe { update.read() }.core()?;
        let id = super::file_id(object)?;
        with_session(volume, |s| {
            let result = if update == MetadataUpdate::default() {
                project(s.volume.object_info(id).map_err(Fault::from)?, object)
                    .map_err(Fault::from)?
            } else {
                s.volume
                    .set_metadata_with_reply(id, update, |info| project(info, object))
                    .map_err(Fault::from)?
            };
            unsafe { out.write(result) };
            Ok(OK)
        })
    })
}

/// Mixed size/metadata on an ADMINISTRATIVE file token; does not authorize.
/// The expected object is validated before business commit, even for a mismatch.
/// v1 permits only empty metadata. Unknown fields and missing output reject first.
/// # Safety
/// Update readable/aligned, output writable/aligned/nonaliasing for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_fskit_file_set_attributes(
    volume: u64,
    file: u64,
    object: i64,
    size: u64,
    update: *const PwMetadataUpdate,
    out: *mut PwFsKitAttributes,
) -> i32 {
    run(|| {
        required(out)?;
        readable(update, 1)?;
        let update = unsafe { update.read() }.core()?;
        super::file_id(object)?;
        with_session(volume, |s| {
            let file = *s.files.get(&file).ok_or_else(Fault::invalid)?;
            let result = s
                .volume
                .truncate_with_reply(file, size, update, |info| project(info, object))
                .map_err(Fault::from)?;
            unsafe { out.write(result) };
            Ok(OK)
        })
    })
}
