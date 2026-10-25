//! Trusted administrative namespace requests with cached-identity guards.
//! These are not policy grants or OS authentication.
use super::{Fault, OK, file_id, run, text, with_session};
use portaweave_core::{Kind, RenameExpectation};

/// Remove only the expected live namespace entry. A missing/replaced entry is
/// STALE; validation, identity check and mutation share the serialized request.
/// # Safety
/// Name readable UTF-8 for name_len bytes throughout the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_remove_checked(
    volume: u64,
    parent: i64,
    name: *const u8,
    name_len: usize,
    directory: u8,
    expected_object: i64,
) -> i32 {
    run(|| {
        let name = unsafe { text(name, name_len)? };
        if directory > 1 {
            return Err(Fault::invalid());
        }
        let parent = file_id(parent)?;
        let expected = file_id(expected_object)?;
        with_session(volume, |s| {
            s.volume
                .remove_checked(
                    parent,
                    name,
                    if directory == 1 {
                        Kind::Directory
                    } else {
                        Kind::File
                    },
                    expected,
                )
                .map_err(Fault::from)?;
            Ok(OK)
        })
    })
}

/// Rename only the expected source and destination. target=0 requires absence,
/// except same-entry case changes; positive target requires that exact inode.
/// # Safety
/// Both names readable UTF-8 for their lengths throughout the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_rename_checked(
    volume: u64,
    from_parent: i64,
    from: *const u8,
    from_len: usize,
    to_parent: i64,
    to: *const u8,
    to_len: usize,
    expected_source: i64,
    expected_target: i64,
) -> i32 {
    run(|| {
        let from = unsafe { text(from, from_len)? };
        let to = unsafe { text(to, to_len)? };
        let from_parent = file_id(from_parent)?;
        let to_parent = file_id(to_parent)?;
        let expected = RenameExpectation {
            source: file_id(expected_source)?,
            target: if expected_target == 0 {
                None
            } else {
                Some(file_id(expected_target)?)
            },
        };
        with_session(volume, |s| {
            s.volume
                .rename_checked(from_parent, from, to_parent, to, expected)
                .map_err(Fault::from)?;
            Ok(OK)
        })
    })
}
