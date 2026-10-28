//! Pure FSKit laboratory attribute projection. No OS caller authentication.
use super::{
    Fault, OK, PwCreateAttributes, PwMetadata, PwMetadataUpdate, next_token, readable, required,
    run, text, with_session,
};
use portaweave_core::{
    Error, Kind, MetadataUpdate, NodeCreateOptions, ObjectInfo, Result, Timestamp,
};

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PwFsKitAttributes {
    pub file_id: u64, // Core ROOT=1 maps to FSKit rootDirectory=2
    pub size: u64,
    pub kind: u32,
    pub links: u32,          // valid for regular files only
    pub metadata_valid: u32, // 0=v1 missing; 1=v2/v3 stored values
    pub reserved: u32,
    pub metadata: PwMetadata,
}
pub(super) fn project(info: ObjectInfo, expected: i64) -> Result<PwFsKitAttributes> {
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

/// Explicit initial fields. Unset values do not override creation defaults.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PwCreateState {
    pub valid: u64,
    pub size: u64,
    pub created: super::PwTimestamp,
    pub accessed: super::PwTimestamp,
    pub modified: super::PwTimestamp,
}
impl PwCreateState {
    fn core(
        self,
        attributes: Option<portaweave_core::CreateAttributes>,
    ) -> super::FResult<NodeCreateOptions> {
        if self.valid & !15 != 0
            || [self.created, self.accessed, self.modified]
                .iter()
                .any(|t| t.reserved != 0)
        {
            return Err(Fault::invalid());
        }
        let time = |flag, t: super::PwTimestamp| {
            (self.valid & flag != 0).then_some(Timestamp {
                seconds: t.seconds,
                nanoseconds: t.nanoseconds,
            })
        };
        Ok(NodeCreateOptions {
            attributes,
            size: (self.valid & 1 != 0).then_some(self.size),
            created: time(2, self.created),
            accessed: time(4, self.accessed),
            modified: time(8, self.modified),
        })
    }
}

/// Created regular files carry an administrative pin; directories have file=0.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PwFsKitCreated {
    pub object: i64,
    pub file: u64,
    pub attributes: PwFsKitAttributes,
}

/// Separate layout/type: never put a directory pin in PwFsKitCreated.file.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PwFsKitDirectoryCreated {
    pub object: i64,
    pub directory: u64,
    pub attributes: PwFsKitAttributes,
}

/// EXPLICIT v3 create plus an administrative directory pin. Ordinary creation
/// continues admitting no directory pin. No OS caller authorization is implied.
/// # Safety
/// Name/optional attributes/state readable; output writable/aligned/nonaliasing.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_fskit_directory_create_with_state(
    volume: u64,
    parent: i64,
    name: *const u8,
    name_len: usize,
    attributes: *const PwCreateAttributes,
    state: *const PwCreateState,
    out: *mut PwFsKitDirectoryCreated,
) -> i32 {
    run(|| {
        required(out)?;
        let name = unsafe { text(name, name_len)? };
        let attributes = if attributes.is_null() {
            None
        } else {
            readable(attributes, 1)?;
            Some(unsafe { attributes.read() }.core()?)
        };
        readable(state, 1)?;
        let options = unsafe { state.read() }.core(attributes)?;
        let parent = super::file_id(parent)?;
        with_session(volume, |s| {
            s.handle_admission()?;
            let token = next_token()?;
            let created = s
                .volume
                .create_directory_with_pin(parent, name, options, |info| {
                    let id = info.stat.id.raw();
                    project(info, id)
                })
                .map_err(Fault::from)?;
            s.directory_pins.insert(token, created.handle);
            unsafe {
                out.write(PwFsKitDirectoryCreated {
                    object: created.object.raw(),
                    directory: token,
                    attributes: created.reply,
                });
            }
            Ok(OK)
        })
    })
}
/// Reserve admission/tokens before creation; return pin/projected state together.
/// # Safety
/// Name and optional attributes readable/aligned, output writable/nonaliasing.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_fskit_node_create(
    volume: u64,
    parent: i64,
    name: *const u8,
    name_len: usize,
    directory: u8,
    attributes: *const PwCreateAttributes,
    out: *mut PwFsKitCreated,
) -> i32 {
    unsafe {
        pw_fskit_node_create_with_state(
            volume,
            parent,
            name,
            name_len,
            directory,
            attributes,
            &PwCreateState::default(),
            out,
        )
    }
}
/// Atomic initial logical size and v2 times. Directory size is unsupported.
/// # Safety
/// Name/attributes/state readable/aligned, output writable/nonaliasing.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_fskit_node_create_with_state(
    volume: u64,
    parent: i64,
    name: *const u8,
    name_len: usize,
    directory: u8,
    attributes: *const PwCreateAttributes,
    state: *const PwCreateState,
    out: *mut PwFsKitCreated,
) -> i32 {
    run(|| {
        required(out)?;
        if directory > 1 {
            return Err(Fault::invalid());
        }
        let name = unsafe { text(name, name_len)? };
        let attributes = if attributes.is_null() {
            None
        } else {
            Some(unsafe { attributes.read() }.core()?)
        };
        readable(state, 1)?;
        let options = unsafe { state.read() }.core(attributes)?;
        let parent = super::file_id(parent)?;
        with_session(volume, |s| {
            if directory == 0 {
                s.handle_admission()?;
            }
            let token = if directory == 0 { next_token()? } else { 0 };
            let result = s
                .volume
                .create_node_with_options(
                    parent,
                    name,
                    if directory == 0 {
                        Kind::File
                    } else {
                        Kind::Directory
                    },
                    options,
                    |info| {
                        let id = info.stat.id.raw();
                        project(info, id)
                    },
                )
                .map_err(Fault::from)?;
            if let Some(handle) = result.handle {
                s.files.insert(token, handle);
            }
            unsafe {
                out.write(PwFsKitCreated {
                    object: result.object.raw(),
                    file: token,
                    attributes: result.reply,
                })
            };
            Ok(OK)
        })
    })
}
