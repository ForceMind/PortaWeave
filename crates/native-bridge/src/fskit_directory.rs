//! Frozen administrative FSKit names AND projected attributes, not live grants.
use super::{
    BAD_COOKIE, BUFFER_TOO_SMALL, EOF, FResult, Fault, OK, PwFsKitAttributes, Session, file_id,
    fskit::project, next_token, readable, required, run, with_session,
};
use portaweave_core::{FileId, ROOT};

struct Record {
    name: String,
    object: i64,
    attributes: PwFsKitAttributes,
}
pub(super) struct DirectoryCursor {
    directory: FileId,
    entries: Vec<Record>,
    position: usize,
}
impl DirectoryCursor {
    pub(super) fn len(&self) -> usize {
        self.entries.len()
    }
    fn capture(
        session: &Session,
        directory: FileId,
        dots: bool,
        remaining: usize,
    ) -> FResult<Self> {
        let stat = session.volume.stat(directory).map_err(Fault::from)?;
        let dot_count = if dots && (directory == ROOT || stat.links != 0) {
            2
        } else {
            0
        };
        let maximum = remaining
            .checked_sub(dot_count)
            .ok_or_else(Fault::resource)?;
        let snapshot = session
            .volume
            .directory_object_snapshot(directory, maximum)
            .map_err(Fault::from)?;
        let directory_info = project(snapshot.directory, directory.raw()).map_err(Fault::from)?;
        let parent = snapshot
            .parent
            .map(|p| {
                let id = p.stat.id.raw();
                project(p, id).map(|info| (id, info))
            })
            .transpose()
            .map_err(Fault::from)?;
        let mut entries = Vec::with_capacity(snapshot.entries.len() + dot_count);
        if dots && let Some((parent_id, parent_info)) = parent {
            entries.push(Record {
                name: ".".into(),
                object: directory.raw(),
                attributes: directory_info,
            });
            entries.push(Record {
                name: "..".into(),
                object: parent_id,
                attributes: parent_info,
            });
        }
        for entry in snapshot.entries {
            let id = entry.info.stat.id.raw();
            entries.push(Record {
                name: entry.name,
                object: id,
                attributes: project(entry.info, id).map_err(Fault::from)?,
            });
        }
        Ok(Self {
            directory,
            entries,
            position: 0,
        })
    }
}
fn dot_flag(value: u8) -> FResult<bool> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(Fault::invalid()),
    }
}

/// Snapshot metadata only; optional dots include root/root, never an orphan.
/// # Safety
/// Output writable/aligned/nonaliasing for the call duration.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_fskit_directory_begin(
    volume: u64,
    directory: i64,
    include_dots: u8,
    out_cursor: *mut u64,
) -> i32 {
    run(|| {
        required(out_cursor)?;
        let dots = dot_flag(include_dots)?;
        let directory = file_id(directory)?;
        with_session(volume, |s| {
            let remaining = s.directory_admission()?;
            let prepared = DirectoryCursor::capture(s, directory, dots, remaining)?;
            let token = next_token()?;
            s.fskit_dirs.insert(token, prepared);
            unsafe { out_cursor.write(token) };
            Ok(OK)
        })
    })
}
/// Replace the SAME directory's snapshot, crediting only its old entries/slot.
/// Prepare/project/reserve before retirement. Success invalidates the old token
/// and all its cookies; failure preserves old data, position and outputs.
/// # Safety
/// Output writable/aligned/nonaliasing for the call duration.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_fskit_directory_refresh(
    volume: u64,
    cursor: u64,
    include_dots: u8,
    out_cursor: *mut u64,
) -> i32 {
    run(|| {
        required(out_cursor)?;
        let dots = dot_flag(include_dots)?;
        with_session(volume, |s| {
            let old = s.fskit_dirs.get(&cursor).ok_or_else(Fault::invalid)?;
            let prepared = DirectoryCursor::capture(
                s,
                old.directory,
                dots,
                s.directory_entry_admission(old.len())?,
            )?;
            let token = next_token()?;
            s.fskit_dirs.remove(&cursor);
            s.fskit_dirs.insert(token, prepared);
            unsafe { out_cursor.write(token) };
            Ok(OK)
        })
    })
}
/// One captured row, no catalog lookup. Every rejected output, including
/// BUFFER_TOO_SMALL/END, stays unchanged and the position does not advance.
/// # Safety
/// Buffers/outputs writable, aligned and mutually nonaliasing during the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_fskit_directory_next(
    volume: u64,
    cursor: u64,
    name: *mut u8,
    capacity: usize,
    out_len: *mut usize,
    out_object: *mut i64,
    out_attributes: *mut PwFsKitAttributes,
) -> i32 {
    run(|| {
        required(out_len)?;
        required(out_object)?;
        required(out_attributes)?;
        readable(name, capacity)?;
        with_session(volume, |s| {
            let cursor = s.fskit_dirs.get_mut(&cursor).ok_or_else(Fault::invalid)?;
            let Some(row) = cursor.entries.get(cursor.position) else {
                return Ok(EOF);
            };
            if row.name.len() > capacity {
                return Err(Fault(BUFFER_TOO_SMALL));
            }
            unsafe {
                if !row.name.is_empty() {
                    std::ptr::copy_nonoverlapping(row.name.as_ptr(), name, row.name.len());
                }
                out_len.write(row.name.len());
                out_object.write(row.object);
                out_attributes.write(row.attributes);
            }
            cursor.position += 1;
            Ok(OK)
        })
    })
}
/// Seek/replay the SAME snapshot. A bad ordinal is BAD_COOKIE, independently
/// of INVALID (wrong/foreign/stale token). EOF=len is a valid resume cookie.
#[unsafe(no_mangle)]
pub extern "C" fn pw_fskit_directory_seek(volume: u64, cursor: u64, position: u64) -> i32 {
    run(|| {
        with_session(volume, |s| {
            let cursor = s.fskit_dirs.get_mut(&cursor).ok_or_else(Fault::invalid)?;
            let position = usize::try_from(position).map_err(|_| Fault(BAD_COOKIE))?;
            if position > cursor.len() {
                return Err(Fault(BAD_COOKIE));
            }
            cursor.position = position;
            Ok(OK)
        })
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn pw_fskit_directory_close(volume: u64, cursor: u64) -> i32 {
    run(|| {
        with_session(volume, |s| {
            s.fskit_dirs.remove(&cursor).ok_or_else(Fault::invalid)?;
            Ok(OK)
        })
    })
}
