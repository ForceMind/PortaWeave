use super::{Fault, OK, next_token, required, run, with_session};
use portaweave_core::LockMode;

/// Trusted policy-file grant, not native fcntl/LockFileEx or caller identity.
/// # Safety
/// Output is writable/aligned for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_posix_file_lock_range(
    volume: u64,
    file: u64,
    offset: u64,
    length: u64,
    mode: u32,
    out_lock: *mut u64,
) -> i32 {
    run(|| {
        required(out_lock)?;
        let mode = match mode {
            1 => LockMode::Shared,
            2 => LockMode::Exclusive,
            _ => return Err(Fault::invalid()),
        };
        with_session(volume, |s| {
            let grant = s
                .policy_files
                .get(&file)
                .copied()
                .ok_or_else(Fault::invalid)?;
            let token = next_token()?; // reserve before core acquisition
            let lock = grant
                .lock_range(&mut s.volume, offset, length, mode)
                .map_err(Fault::from)?;
            s.range_locks.insert(token, (file, lock));
            unsafe {
                out_lock.write(token);
            }
            Ok(OK)
        })
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn pw_posix_file_unlock_range(volume: u64, file: u64, lock: u64) -> i32 {
    run(|| {
        with_session(volume, |s| {
            let grant = s
                .policy_files
                .get(&file)
                .copied()
                .ok_or_else(Fault::invalid)?;
            let (owner, core_lock) = *s.range_locks.get(&lock).ok_or_else(Fault::invalid)?;
            if owner != file {
                return Err(Fault::invalid());
            }
            grant
                .unlock_range(&mut s.volume, core_lock)
                .map_err(Fault::from)?;
            s.range_locks.remove(&lock);
            Ok(OK)
        })
    })
}
