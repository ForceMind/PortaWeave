//! Pure Windows laboratory projection, callable without a driver on every OS.
//! No SID/ACL authentication, reservation, kernel cache or mounted disk claim.
use super::{Fault, MAX_IO, OK, PwCredentials, credentials, readable, required, run, with_session};
use portaweave_core::{Error, FileInfo, MetadataUpdate, Result, Timestamp, WritePosition};

/// 4096-byte logical allocation geometry of the WinFsp lab dispatcher.
/// AllocationSize >= EOF is a logical projection, NOT physical reservation.
const ALLOCATION_UNIT: u64 = 4096;
const UNIX_EPOCH_SECONDS: i128 = 11_644_473_600;
const TICKS_PER_SECOND: i128 = 10_000_000;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PwWindowsFileInfo {
    pub file_size: u64,
    pub allocation_size: u64,
    pub creation_time: u64,
    pub last_access_time: u64,
    pub last_write_time: u64,
    pub change_time: u64,
    pub index_number: u64,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PwWindowsWriteResult {
    pub written: usize,
    pub offset: u64,
    pub info: PwWindowsFileInfo,
}

/// WinFsp SetBasicInfo laboratory input. INVALID_FILE_ATTRIBUTES and zero
/// times mean unchanged. DOS flags and explicit change time are unsupported.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PwWindowsBasicInfo {
    pub file_attributes: u32,
    pub reserved: u32,
    pub creation_time: u64,
    pub last_access_time: u64,
    pub last_write_time: u64,
    pub change_time: u64,
}

fn timestamp(ticks: u64) -> Result<Option<Timestamp>> {
    if ticks == 0 {
        return Ok(None);
    }
    // Includes the -1/-2 per-handle time-control sentinels. There is no
    // automatic time-control state to implement them yet; never fake success.
    if ticks > i64::MAX as u64 {
        return Err(Error::UnsupportedOperation);
    }
    Ok(Some(Timestamp {
        seconds: (ticks / TICKS_PER_SECOND as u64) as i64 - UNIX_EPOCH_SECONDS as i64,
        nanoseconds: (ticks % TICKS_PER_SECOND as u64) as u32 * 100,
    }))
}

impl PwWindowsBasicInfo {
    pub(super) fn update(self) -> super::FResult<MetadataUpdate> {
        if self.reserved != 0 {
            return Err(Fault::invalid());
        }
        if self.file_attributes != u32::MAX || self.change_time != 0 {
            return Err(Error::UnsupportedOperation.into());
        }
        Ok(MetadataUpdate {
            created: timestamp(self.creation_time)?,
            accessed: timestamp(self.last_access_time)?,
            modified: timestamp(self.last_write_time)?,
            ..Default::default()
        })
    }
}

fn file_time(t: Timestamp) -> Result<u64> {
    if t.nanoseconds >= 1_000_000_000 {
        return Err(Error::UnsupportedOperation);
    }
    // Floor to FILETIME's 100 ns granularity; never rewrite stored nanoseconds.
    let ticks = (i128::from(t.seconds) + UNIX_EPOCH_SECONDS) * TICKS_PER_SECOND
        + i128::from(t.nanoseconds / 100);
    if !(0..=i128::from(i64::MAX)).contains(&ticks) {
        return Err(Error::UnsupportedOperation);
    }
    Ok(ticks as u64)
}
pub(super) fn project(info: FileInfo) -> Result<PwWindowsFileInfo> {
    let allocation_size = info.stat.size.div_ceil(ALLOCATION_UNIT) * ALLOCATION_UNIT;
    if allocation_size > i64::MAX as u64 {
        return Err(Error::UnsupportedOperation);
    }
    Ok(PwWindowsFileInfo {
        file_size: info.stat.size,
        allocation_size,
        creation_time: file_time(info.metadata.created)?,
        last_access_time: file_time(info.metadata.accessed)?,
        last_write_time: file_time(info.metadata.modified)?,
        change_time: file_time(info.metadata.changed)?,
        index_number: info.stat.id.raw() as u64,
    })
}

/// Granted policy token only. Projection refuses unrepresentable metadata.
/// # Safety
/// Output writable/aligned/nonaliasing throughout this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_windows_file_info(
    volume: u64,
    file: u64,
    out_info: *mut PwWindowsFileInfo,
) -> i32 {
    run(|| {
        required(out_info)?;
        with_session(volume, |s| {
            let file = *s.policy_files.get(&file).ok_or_else(Fault::invalid)?;
            let info = project(file.info(&s.volume).map_err(Fault::from)?).map_err(Fault::from)?;
            unsafe { out_info.write(info) };
            Ok(OK)
        })
    })
}

/// Append/offset/constrained write; the Windows reply is validated BEFORE commit.
/// # Safety
/// Data readable for length bytes, output writable/aligned/nonaliasing.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_windows_file_write(
    volume: u64,
    file: u64,
    offset: u64,
    data: *const u8,
    length: usize,
    flags: u32,
    out_result: *mut PwWindowsWriteResult,
) -> i32 {
    run(|| {
        required(out_result)?;
        readable(data, length)?;
        if length > MAX_IO || flags & !3 != 0 {
            return Err(Fault::invalid());
        }
        let position = if flags & 1 != 0 {
            WritePosition::EndOfFile
        } else {
            WritePosition::Offset(offset)
        };
        with_session(volume, |s| {
            let file = *s.policy_files.get(&file).ok_or_else(Fault::invalid)?;
            let data = if length == 0 {
                &[]
            } else {
                unsafe { std::slice::from_raw_parts(data, length) }
            };
            let result = file
                .write_request_with_reply(&mut s.volume, position, data, flags & 2 != 0, |result| {
                    Ok(PwWindowsWriteResult {
                        written: result.written,
                        offset: result.offset,
                        info: project(result.info)?,
                    })
                })
                .map_err(Fault::from)?;
            unsafe { out_result.write(result) };
            Ok(OK)
        })
    })
}

/// Logical EOF only; allocation/preallocation requests are not implemented.
/// # Safety
/// Output writable/aligned/nonaliasing throughout this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_windows_file_set_size(
    volume: u64,
    file: u64,
    size: u64,
    out_info: *mut PwWindowsFileInfo,
) -> i32 {
    run(|| {
        required(out_info)?;
        with_session(volume, |s| {
            let file = *s.policy_files.get(&file).ok_or_else(Fault::invalid)?;
            let info = file
                .truncate_request_with_reply(&mut s.volume, size, project)
                .map_err(Fault::from)?;
            unsafe { out_info.write(info) };
            Ok(OK)
        })
    })
}

/// Policy file plus CURRENT trusted caller. Not Windows SID/ACL authorization.
/// The core authorizes all requested metadata and projects the Windows reply
/// before one commit. Success must be followed by a separate health query.
/// # Safety
/// Caller/groups and input readable, output writable/aligned/nonaliasing.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_windows_file_set_basic_info(
    volume: u64,
    file: u64,
    caller: *const PwCredentials,
    request: *const PwWindowsBasicInfo,
    out_info: *mut PwWindowsFileInfo,
) -> i32 {
    run(|| {
        required(out_info)?;
        readable(request, 1)?;
        let caller = unsafe { credentials(caller)? };
        let update = unsafe { request.read() }.update()?;
        with_session(volume, |s| {
            let file = *s.policy_files.get(&file).ok_or_else(Fault::invalid)?;
            let info = s
                .volume
                .posix(caller)
                .map_err(Fault::from)?
                .set_file_metadata_with_reply(file, update, project)
                .map_err(Fault::from)?;
            unsafe { out_info.write(info) };
            Ok(OK)
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn filetime_projection_preserves_signed_epoch_and_checks_range_without_overflow() {
        let at = |seconds, nanoseconds| {
            file_time(Timestamp {
                seconds,
                nanoseconds,
            })
        };
        assert_eq!(at(-11_644_473_600, 0).unwrap(), 0);
        assert_eq!(at(-11_644_473_600, 99).unwrap(), 0);
        assert_eq!(at(-11_644_473_600, 100).unwrap(), 1);
        assert_eq!(at(0, 0).unwrap(), 116_444_736_000_000_000);
        assert_eq!(at(-1, 999_999_999).unwrap(), 116_444_735_999_999_999);
        let seconds = (i64::MAX / 10_000_000) - 11_644_473_600;
        let nanos = (i64::MAX % 10_000_000) as u32 * 100;
        assert_eq!(at(seconds, nanos).unwrap(), i64::MAX as u64);
        for (s, n) in [
            (-11_644_473_601, 999_999_999),
            (seconds, nanos + 100),
            (i64::MIN, 0),
            (i64::MAX, 0),
            (0, 1_000_000_000),
        ] {
            assert!(matches!(at(s, n), Err(Error::UnsupportedOperation)));
        }
    }

    #[test]
    fn basic_times_round_trip_exact_ticks_and_refuse_controls() {
        assert_eq!(timestamp(0).unwrap(), None);
        for ticks in [
            1,
            99,
            100,
            9_999_999,
            10_000_000,
            116_444_736_000_000_007,
            i64::MAX as u64,
        ] {
            let value = timestamp(ticks).unwrap().unwrap();
            assert_eq!(file_time(value).unwrap(), ticks);
            assert_eq!(value.nanoseconds % 100, 0);
        }
        for ticks in [i64::MAX as u64 + 1, u64::MAX - 1, u64::MAX] {
            assert!(matches!(timestamp(ticks), Err(Error::UnsupportedOperation)));
        }
    }
}
