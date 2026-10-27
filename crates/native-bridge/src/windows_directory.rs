//! Bounded, immutable directory metadata and the pinned WinFsp 2.1 wire format.
//! Explicit current-policy observation/replacement, never a live inode grant.
use super::{
    FResult, Fault, MAX_IO, OK, PwCredentials, PwWindowsFileInfo, credentials, file_id, next_token,
    readable, required, run, windows::project, with_session,
};
use portaweave_core::{DirectorySnapshot, Error, FileInfo, Kind, ROOT};

// Checked against the pinned C header by static assertions in the real MSVC
// callback translation unit. These are WinFsp wire offsets, not Rust repr(C).
const HEADER: usize = 104;
const ALIGNMENT: usize = 8;
const MAX_NAME_UNITS: usize = 255;

struct Record {
    name: Vec<u16>,
    kind: Kind,
    info: PwWindowsFileInfo,
}
impl Record {
    fn new(name: &str, info: FileInfo) -> FResult<Self> {
        let name: Vec<u16> = name.encode_utf16().collect();
        if name.is_empty() || name.len() > MAX_NAME_UNITS || name.contains(&0) {
            return Err(Error::UnsupportedOperation.into());
        }
        let kind = info.stat.kind;
        Ok(Self {
            name,
            kind,
            info: project(info)?,
        })
    }
}
pub(super) struct DirectoryCursor {
    directory: portaweave_core::FileId,
    captured_info: PwWindowsFileInfo,
    entries: Vec<Record>,
}
impl DirectoryCursor {
    pub(super) fn len(&self) -> usize {
        self.entries.len()
    }
    fn from_snapshot(snapshot: DirectorySnapshot) -> FResult<Self> {
        // Validate even root/parent metadata, rather than silently inventing a
        // representable Windows directory when the core cannot project it.
        let directory = snapshot.directory.stat.id;
        let captured_info = project(snapshot.directory.clone())?;
        project(snapshot.parent.clone())?;
        let dots = if snapshot.directory.stat.id == ROOT {
            0
        } else {
            2
        };
        let mut entries = Vec::with_capacity(snapshot.entries.len() + dots);
        if dots != 0 {
            entries.push(Record::new(".", snapshot.directory)?);
            entries.push(Record::new("..", snapshot.parent)?);
        }
        for entry in snapshot.entries {
            entries.push(Record::new(&entry.name, entry.info)?);
        }
        Ok(Self {
            directory,
            captured_info,
            entries,
        })
    }
    fn encode(&self, start: usize, capacity: usize) -> FResult<Vec<u8>> {
        if start > self.entries.len() {
            return Err(Fault::invalid());
        }
        let mut bytes = Vec::new();
        let mut next = start;
        for entry in &self.entries[start..] {
            let size = HEADER + entry.name.len() * 2;
            let padded = size.next_multiple_of(ALIGNMENT);
            if padded > capacity - bytes.len() {
                break;
            }
            let base = bytes.len();
            bytes.resize(base + padded, 0); // Includes every unused/padding byte.
            let record = &mut bytes[base..];
            record[0..2].copy_from_slice(&(size as u16).to_le_bytes());
            record[8..12].copy_from_slice(
                &(if entry.kind == Kind::Directory {
                    0x10_u32
                } else {
                    0x80_u32
                })
                .to_le_bytes(),
            );
            for (offset, value) in [
                (16, entry.info.allocation_size),
                (24, entry.info.file_size),
                (32, entry.info.creation_time),
                (40, entry.info.last_access_time),
                (48, entry.info.last_write_time),
                (56, entry.info.change_time),
                (64, entry.info.index_number),
                (80, next as u64 + 1),
            ] {
                record[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
            }
            // ReparseTag, HardLinks (unimplemented upstream), EaSize stay zero.
            for (index, unit) in entry.name.iter().enumerate() {
                let offset = HEADER + index * 2;
                record[offset..offset + 2].copy_from_slice(&unit.to_le_bytes());
            }
            next += 1;
        }
        if next == self.entries.len() && capacity - bytes.len() >= 2 {
            bytes.extend_from_slice(&[0, 0]); // Unpadded uint16 zero EOF marker.
        }
        Ok(bytes)
    }
}

/// Capture READ+SEARCH-authorized v2 names/attributes before publishing a token.
/// This is a metadata snapshot, not a live directory descriptor or Windows ACL.
/// # Safety
/// Caller/groups readable; output writable/aligned/nonaliasing throughout call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_windows_directory_begin(
    volume: u64,
    directory: i64,
    caller: *const PwCredentials,
    out_cursor: *mut u64,
) -> i32 {
    run(|| {
        required(out_cursor)?;
        let caller = unsafe { credentials(caller)? };
        let directory = file_id(directory)?;
        with_session(volume, |session| {
            let dots = if directory == ROOT { 0 } else { 2 };
            let maximum = session
                .directory_admission()?
                .checked_sub(dots)
                .ok_or_else(Fault::resource)?;
            let snapshot = session
                .volume
                .posix(caller)
                .map_err(Fault::from)?
                .directory_snapshot(directory, maximum)
                .map_err(Fault::from)?;
            let cursor = DirectoryCursor::from_snapshot(snapshot)?;
            let token = next_token()?;
            session.windows_dirs.insert(token, cursor);
            unsafe { out_cursor.write(token) };
            Ok(OK)
        })
    })
}

/// Pinned WinFsp 2.1 DIR_INFO bytes, with NextOffset ordinal resume cookies.
/// Repeat/restart on the SAME snapshot is deterministic. A too-small buffer
/// returns success/zero without consuming an entry; EOF is a two-byte zero.
/// # Safety
/// Buffer writable for capacity bytes (NULL only when zero), output writable,
/// aligned and nonaliasing. The snapshot read/packing holds one owner mutex.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_windows_directory_read(
    volume: u64,
    cursor: u64,
    next_offset: u64,
    buffer: *mut u8,
    capacity: usize,
    out_read: *mut usize,
) -> i32 {
    run(|| {
        required(out_read)?;
        readable(buffer, capacity)?;
        if capacity > MAX_IO {
            return Err(Fault::invalid());
        }
        let offset = usize::try_from(next_offset).map_err(|_| Fault::invalid())?;
        with_session(volume, |session| {
            let cursor = session
                .windows_dirs
                .get(&cursor)
                .ok_or_else(Fault::invalid)?;
            let bytes = cursor.encode(offset, capacity)?;
            if !bytes.is_empty() {
                unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), buffer, bytes.len()) };
            }
            unsafe { out_read.write(bytes.len()) };
            Ok(OK)
        })
    })
}

/// Observe CURRENT attributes after fresh trusted READ+SEARCH/ancestor checks.
/// This is not fstat of an open inode; deletion returns NotFound, even though
/// the old snapshot remains readable until explicit replacement/close.
/// # Safety
/// Caller/groups readable; output writable/aligned/nonaliasing throughout call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_windows_directory_info(
    volume: u64,
    cursor: u64,
    caller: *const PwCredentials,
    out_info: *mut PwWindowsFileInfo,
) -> i32 {
    run(|| {
        required(out_info)?;
        let caller = unsafe { credentials(caller)? };
        with_session(volume, |session| {
            let directory = session
                .windows_dirs
                .get(&cursor)
                .ok_or_else(Fault::invalid)?
                .directory;
            let info = session
                .volume
                .posix(caller)
                .map_err(Fault::from)?
                .directory_info(directory)
                .map_err(Fault::from)?;
            let info = project(info)?;
            unsafe { out_info.write(info) };
            Ok(OK)
        })
    })
}

/// Reauthorize and replace ONE existing typed snapshot atomically. Admission
/// credits its old entries/slot, not anyone else's. All conversion and token
/// reservation precede publication; failure preserves the old cursor/outputs.
/// Success retires the old token and starts new ordinal cookies at zero.
/// # Safety
/// Caller/groups readable; outputs writable/aligned/mutually nonaliasing.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_windows_directory_refresh(
    volume: u64,
    cursor: u64,
    caller: *const PwCredentials,
    out_cursor: *mut u64,
    out_info: *mut PwWindowsFileInfo,
) -> i32 {
    run(|| {
        required(out_cursor)?;
        required(out_info)?;
        let caller = unsafe { credentials(caller)? };
        with_session(volume, |session| {
            let old = session
                .windows_dirs
                .get(&cursor)
                .ok_or_else(Fault::invalid)?;
            let directory = old.directory;
            let dots = if directory == ROOT { 0 } else { 2 };
            let maximum = session
                .directory_entry_admission(old.len())?
                .checked_sub(dots)
                .ok_or_else(Fault::resource)?;
            let snapshot = session
                .volume
                .posix(caller)
                .map_err(Fault::from)?
                .directory_snapshot(directory, maximum)
                .map_err(Fault::from)?;
            let new = DirectoryCursor::from_snapshot(snapshot)?;
            let info = new.captured_info;
            let token = next_token()?;
            session.windows_dirs.remove(&cursor);
            session.windows_dirs.insert(token, new);
            unsafe {
                out_info.write(info);
                out_cursor.write(token);
            }
            Ok(OK)
        })
    })
}

/// Release is allowed under engine protection and never performs data GC.
#[unsafe(no_mangle)]
pub extern "C" fn pw_windows_directory_close(volume: u64, cursor: u64) -> i32 {
    run(|| {
        with_session(volume, |session| {
            session
                .windows_dirs
                .remove(&cursor)
                .ok_or_else(Fault::invalid)?;
            Ok(OK)
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        PwCreateAttributes, pw_dir_begin, pw_dir_close, pw_directory_create, pw_file_create,
        pw_remove, pw_volume_close, pw_volume_create_with_metadata,
    };
    use std::ptr;

    #[test]
    fn shared_entry_admission_counts_both_cursor_tables_and_synthetic_dots() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "pw-directory-budget-{}-{stamp}",
            std::process::id()
        ));
        let p = path.to_str().unwrap();
        let mut volume = 0;
        let attrs = PwCreateAttributes {
            uid: 100,
            gid: 20,
            mode: 0o777,
            reserved: 0,
        };
        let caller = PwCredentials {
            uid: 100,
            gid: 20,
            groups: ptr::null(),
            group_count: 0,
            reserved: 0,
        };
        assert_eq!(
            unsafe {
                pw_volume_create_with_metadata(p.as_ptr(), p.len(), 1 << 20, &attrs, &mut volume)
            },
            0
        );
        with_session(volume, |s| {
            s.directory_entry_ceiling = Some(3);
            Ok(())
        })
        .unwrap(); // Test-only lower ceiling, never a production bypass.
        let mut object = 0;
        assert_eq!(
            unsafe { pw_file_create(volume, 1, b"a".as_ptr(), 1, &mut object) },
            0
        );
        assert_eq!(
            unsafe { pw_file_create(volume, 1, b"b".as_ptr(), 1, &mut object) },
            0
        );
        let (mut windows, mut admin) = (0, 99);
        assert_eq!(
            unsafe { pw_windows_directory_begin(volume, 1, &caller, &mut windows) },
            0
        ); // 2 retained entries
        assert_eq!(
            unsafe { pw_dir_begin(volume, 1, &mut admin) },
            crate::RESOURCE
        );
        assert_eq!(admin, 99);
        let mut refused = 99;
        assert_eq!(
            unsafe { pw_windows_directory_begin(volume, 1, &caller, &mut refused) },
            crate::RESOURCE
        );
        assert_eq!(refused, 99);
        let mut empty = 0;
        assert_eq!(
            unsafe { pw_directory_create(volume, 1, b"empty".as_ptr(), 5, &mut empty) },
            0
        );
        assert_eq!(unsafe { pw_dir_begin(volume, empty, &mut admin) }, 0); // empty admin snapshot costs 0 entries
        let mut info = PwWindowsFileInfo::default();
        let old = windows;
        assert_eq!(
            unsafe { pw_windows_directory_refresh(volume, old, &caller, &mut windows, &mut info) },
            0
        );
        assert_ne!(old, windows); // replacement credits 2, now retains exactly 3
        assert_eq!(pw_windows_directory_close(volume, old), crate::INVALID);
        let mut extra = 0;
        assert_eq!(
            unsafe { pw_file_create(volume, 1, b"extra".as_ptr(), 5, &mut extra) },
            0
        );
        let saved_info = info;
        let mut rejected = 99;
        assert_eq!(
            unsafe {
                pw_windows_directory_refresh(volume, windows, &caller, &mut rejected, &mut info)
            },
            crate::RESOURCE
        );
        assert_eq!((rejected, info), (99, saved_info));
        with_session(volume, |s| {
            assert_eq!(s.windows_dirs.get(&windows).unwrap().len(), 3);
            Ok(())
        })
        .unwrap();
        assert_eq!(unsafe { pw_remove(volume, 1, b"extra".as_ptr(), 5, 0) }, 0);
        assert_eq!(
            unsafe { pw_windows_directory_begin(volume, empty, &caller, &mut refused) },
            crate::RESOURCE
        ); // dots cost 2
        assert_eq!(pw_windows_directory_close(volume, windows), 0);
        let mut root = 0;
        assert_eq!(unsafe { pw_dir_begin(volume, 1, &mut root) }, 0); // now 3 real entries
        assert_eq!(
            unsafe { pw_windows_directory_begin(volume, empty, &caller, &mut refused) },
            crate::RESOURCE
        );
        assert_eq!(pw_dir_close(volume, root), 0);
        assert_eq!(
            unsafe { pw_windows_directory_begin(volume, empty, &caller, &mut windows) },
            0
        ); // dots admitted after release
        assert_eq!(
            unsafe { pw_dir_begin(volume, 1, &mut refused) },
            crate::RESOURCE
        );
        assert_eq!(pw_windows_directory_close(volume, windows), 0);
        assert_eq!(pw_dir_close(volume, admin), 0);
        assert_eq!(pw_volume_close(volume), 0);
        std::fs::remove_dir_all(path).unwrap();
    }
}
