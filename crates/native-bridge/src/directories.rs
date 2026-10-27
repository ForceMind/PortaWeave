//! Explicit v3 directory lifetime primitives. Trusted claims, not OS identity.
use super::{
    Fault, MAX_SESSIONS, OK, PwCreateAttributes, PwCredentials, PwFileInfo, PwMetadataUpdate,
    Session, credentials, file_id, next_token, readable, required, run, text, with_session,
    with_table,
};
use portaweave_core::Volume;
use std::path::Path;

/// Administrative v3 directory reference, NOT a policy grant or OS caller.
/// # Safety
/// Output writable/aligned/nonaliasing throughout the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_directory_open(
    volume: u64,
    directory: i64,
    out_directory: *mut u64,
) -> i32 {
    run(|| {
        required(out_directory)?;
        let directory = file_id(directory)?;
        with_session(volume, |s| {
            s.handle_admission()?;
            let token = next_token()?;
            let handle = s.volume.open_directory(directory).map_err(Fault::from)?;
            s.directory_pins.insert(token, handle);
            unsafe {
                out_directory.write(token);
            }
            Ok(OK)
        })
    })
}

/// Release only an administrative directory token, including under protection.
#[unsafe(no_mangle)]
pub extern "C" fn pw_directory_close(volume: u64, directory: u64) -> i32 {
    run(|| {
        with_session(volume, |s| {
            let handle = *s
                .directory_pins
                .get(&directory)
                .ok_or_else(Fault::invalid)?;
            s.volume.close_directory(handle).map_err(Fault::from)?;
            s.directory_pins.remove(&directory);
            Ok(OK)
        })
    })
}

/// NEW v3 container; never opens/upgrades a v1/v2 path.
/// # Safety
/// Path/attributes readable; output writable/aligned/nonaliasing.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_volume_create_with_directory_lifetimes(
    path: *const u8,
    path_len: usize,
    quota: u64,
    attributes: *const PwCreateAttributes,
    out_volume: *mut u64,
) -> i32 {
    run(|| {
        required(out_volume)?;
        readable(attributes, 1)?;
        let path = unsafe { text(path, path_len)? };
        let attributes = unsafe { attributes.read() }.core()?;
        with_table(|all| {
            if all.len() >= MAX_SESSIONS {
                return Err(Fault::resource());
            }
            let token = next_token()?;
            let volume =
                Volume::create_with_directory_lifetimes(Path::new(path), quota, attributes)
                    .map_err(Fault::from)?;
            all.insert(token, Session::new(volume));
            unsafe {
                out_volume.write(token);
            }
            Ok(OK)
        })
    })
}

/// Admit a READ+SEARCH grant on a currently linked v3 directory.
/// # Safety
/// Caller/groups readable; output writable/aligned/nonaliasing.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_posix_directory_open(
    volume: u64,
    directory: i64,
    caller: *const PwCredentials,
    out_directory: *mut u64,
) -> i32 {
    run(|| {
        required(out_directory)?;
        let caller = unsafe { credentials(caller)? };
        let id = file_id(directory)?;
        with_session(volume, |s| {
            s.handle_admission()?;
            let token = next_token()?;
            let directory = s
                .volume
                .posix(caller)
                .map_err(Fault::from)?
                .open_directory(id)
                .map_err(Fault::from)?;
            s.policy_dirs.insert(token, directory);
            unsafe {
                out_directory.write(token);
            }
            Ok(OK)
        })
    })
}

/// Current attributes of the granted inode, including after final unlink.
/// # Safety
/// Output writable/aligned/nonaliasing.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_posix_directory_info(
    volume: u64,
    directory: u64,
    out_info: *mut PwFileInfo,
) -> i32 {
    run(|| {
        required(out_info)?;
        with_session(volume, |s| {
            let directory = *s.policy_dirs.get(&directory).ok_or_else(Fault::invalid)?;
            let info = directory.info(&s.volume).map_err(Fault::from)?;
            unsafe {
                out_info.write(info.into());
            }
            Ok(OK)
        })
    })
}

/// Fresh CURRENT owner/group authority, independent of the old read grant.
/// # Safety
/// Caller/groups/update readable; output writable/aligned/nonaliasing.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_posix_directory_set_metadata_info(
    volume: u64,
    directory: u64,
    caller: *const PwCredentials,
    update: *const PwMetadataUpdate,
    out_info: *mut PwFileInfo,
) -> i32 {
    run(|| {
        required(out_info)?;
        readable(update, 1)?;
        let caller = unsafe { credentials(caller)? };
        let update = unsafe { update.read() }.core()?;
        with_session(volume, |s| {
            let directory = *s.policy_dirs.get(&directory).ok_or_else(Fault::invalid)?;
            let info = s
                .volume
                .posix(caller)
                .map_err(Fault::from)?
                .set_directory_metadata_with_reply(directory, update, Ok)
                .map_err(Fault::from)?;
            unsafe {
                out_info.write(info.into());
            } // infallible serialization of pre-commit reply
            Ok(OK)
        })
    })
}

/// Release under protection too; successful close is not safe device eject.
#[unsafe(no_mangle)]
pub extern "C" fn pw_posix_directory_close(volume: u64, directory: u64) -> i32 {
    run(|| {
        with_session(volume, |s| {
            let granted = *s.policy_dirs.get(&directory).ok_or_else(Fault::invalid)?;
            granted.close(&mut s.volume).map_err(Fault::from)?;
            s.policy_dirs.remove(&directory);
            Ok(OK)
        })
    })
}
