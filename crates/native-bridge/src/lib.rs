//! C ABI for the two future *native* filesystem adapters.
//! An ABI call is not an OS mount: FSKit and WinFsp must still provide
//! their own permission, locking, notification, mmap and flush semantics.
//! Raw callers must respect the ownership and buffer requirements in the header.
#![deny(unsafe_op_in_unsafe_fn)]

use portaweave_core::{
    Access, CreateAttributes, Credentials, Entry, Error, FileAccess, FileId, FileInfo, Handle,
    Kind, MAX_IO, MAX_SUPPLEMENTARY_GROUPS, MaintenanceStatus, MetadataUpdate, PosixFile,
    PosixReadView, ROOT, ReadView, Timestamp, Volume, WritePosition,
};
use std::{
    collections::BTreeMap,
    panic::{AssertUnwindSafe, catch_unwind},
    path::Path,
    slice, str,
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
};

const OK: i32 = 0;
const EOF: i32 = 1;
const INVALID: i32 = -1;
const NOT_FOUND: i32 = -2;
const EXISTS: i32 = -3;
const NOT_DIR: i32 = -4;
const IS_DIR: i32 = -5;
const NOT_EMPTY: i32 = -6;
const BUSY: i32 = -7;
const NO_SPACE: i32 = -8;
const QUOTA: i32 = -9;
const READ_ONLY: i32 = -10;
const UNSUPPORTED: i32 = -11;
const CORRUPT: i32 = -12;
const IO: i32 = -13;
const RESOURCE: i32 = -14;
const INTERNAL: i32 = -15;
const BUFFER_TOO_SMALL: i32 = -16;
const DENIED: i32 = -17;

const MAX_STRING: usize = 32 * 1024;
const MAX_SESSIONS: usize = 16;
const MAX_HANDLES: usize = 4096;
const MAX_CURSORS: usize = 64;
const MAX_DIRECTORY_ENTRIES: usize = 100_000;

#[derive(Clone, Copy, Debug)]
struct Fault(i32);
type FResult<T> = Result<T, Fault>;
impl From<Error> for Fault {
    fn from(e: Error) -> Self {
        Self(match e {
            Error::NotFound => NOT_FOUND,
            Error::Exists => EXISTS,
            Error::NotDirectory => NOT_DIR,
            Error::IsDirectory => IS_DIR,
            Error::NotEmpty => NOT_EMPTY,
            Error::Busy => BUSY,
            Error::Quota => QUOTA,
            Error::HostSpace { .. } => NO_SPACE,
            Error::WriteProtected => READ_ONLY,
            Error::PermissionDenied => DENIED,
            Error::UnsupportedFormat | Error::UnsupportedOperation => UNSUPPORTED,
            Error::Corrupt(_) => CORRUPT,
            Error::ResourceLimit | Error::RecoveryLimit => RESOURCE,
            Error::Io(ref io) => match io.kind() {
                std::io::ErrorKind::NotFound => NOT_FOUND,
                std::io::ErrorKind::AlreadyExists => EXISTS,
                std::io::ErrorKind::StorageFull => NO_SPACE,
                std::io::ErrorKind::PermissionDenied => READ_ONLY,
                _ => IO,
            },
            Error::Sql(_) => IO,
            Error::Cycle | Error::InvalidHandle | Error::InvalidRange | Error::InvalidName => {
                INVALID
            }
            Error::Injected => INTERNAL,
        })
    }
}
impl Fault {
    fn invalid() -> Self {
        Self(INVALID)
    }
    fn busy() -> Self {
        Self(BUSY)
    }
    fn resource() -> Self {
        Self(RESOURCE)
    }
}

#[derive(Default)]
struct Cursor {
    entries: Vec<Entry>,
    position: usize,
}
struct Session {
    volume: Volume,
    files: BTreeMap<u64, Handle>,
    policy_files: BTreeMap<u64, PosixFile>,
    policy_views: BTreeMap<u64, PosixReadView>,
    dirs: BTreeMap<u64, Cursor>,
    views: BTreeMap<u64, ReadView>,
}
static TABLE: OnceLock<Mutex<BTreeMap<u64, Session>>> = OnceLock::new();
static NEXT: AtomicU64 = AtomicU64::new(1);

fn next_token() -> FResult<u64> {
    let mut current = NEXT.load(Ordering::Relaxed);
    loop {
        let next = current.checked_add(1).ok_or_else(Fault::resource)?;
        match NEXT.compare_exchange_weak(current, next, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(previous) => return Ok(previous),
            Err(observed) => current = observed,
        }
    }
}
fn sessions() -> &'static Mutex<BTreeMap<u64, Session>> {
    TABLE.get_or_init(|| Mutex::new(BTreeMap::new()))
}
fn with_table<T>(op: impl FnOnce(&mut BTreeMap<u64, Session>) -> FResult<T>) -> FResult<T> {
    // Refuse service after a panic rather than reusing potentially inconsistent state.
    let mut lock = sessions().lock().map_err(|_| Fault(INTERNAL))?;
    op(&mut lock)
}
fn with_session<T>(id: u64, op: impl FnOnce(&mut Session) -> FResult<T>) -> FResult<T> {
    with_table(|all| {
        let session = all.get_mut(&id).ok_or_else(Fault::invalid)?;
        op(session)
    })
}
fn run(op: impl FnOnce() -> FResult<i32>) -> i32 {
    match catch_unwind(AssertUnwindSafe(op)) {
        Ok(Ok(code)) => code,
        Ok(Err(e)) => e.0,
        Err(_) => INTERNAL,
    }
}

/// # Safety
/// Pointer must reference len readable bytes for the duration of this call.
unsafe fn text<'a>(pointer: *const u8, len: usize) -> FResult<&'a str> {
    if pointer.is_null() || len == 0 || len > MAX_STRING {
        return Err(Fault::invalid());
    }
    let bytes = unsafe { slice::from_raw_parts(pointer, len) };
    if bytes.contains(&0) {
        return Err(Fault::invalid());
    }
    str::from_utf8(bytes).map_err(|_| Fault::invalid())
}
fn file_id(raw: i64) -> FResult<FileId> {
    FileId::from_raw(raw).map_err(Fault::from)
}
fn required<T>(p: *mut T) -> FResult<()> {
    if p.is_null() {
        Err(Fault::invalid())
    } else {
        Ok(())
    }
}
fn readable<T>(p: *const T, len: usize) -> FResult<()> {
    if len > 0 && p.is_null() {
        Err(Fault::invalid())
    } else {
        Ok(())
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PwStat {
    pub kind: u32, // 0=file, 1=directory
    pub size: u64,
    pub links: u64,
    pub generation: u64,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PwSpace {
    pub logical_quota_bytes: u64,
    pub logical_used_bytes: u64,
    pub host_available_bytes: u64,
    pub host_data_admission_bytes: u64,
    pub apparent_free_bytes: u64,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PwCheckpoint {
    pub wal_frames: u64,
    pub checkpointed_frames: u64,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PwStorageSync {
    pub segment_files: u64,
    pub wal_frames: u64,
    pub checkpointed_frames: u64,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PwCreateAttributes {
    pub uid: u32,
    pub gid: u32,
    pub mode: u32,
    pub reserved: u32,
}
/// Trusted caller claims, NOT authentication. Never read them from untrusted IPC.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct PwCredentials {
    pub uid: u32,
    pub gid: u32,
    pub groups: *const u32,
    pub group_count: usize,
    pub reserved: u64,
}
/// # Safety
/// Caller supplies a readable/aligned struct and group_count valid u32 values.
unsafe fn credentials(pointer: *const PwCredentials) -> FResult<Credentials> {
    readable(pointer, 1)?;
    let c = unsafe { pointer.read() };
    if c.reserved != 0 {
        return Err(Fault::invalid());
    }
    if c.group_count > MAX_SUPPLEMENTARY_GROUPS {
        return Err(Fault::resource());
    }
    readable(c.groups, c.group_count)?;
    let groups = if c.group_count == 0 {
        &[]
    } else {
        unsafe { slice::from_raw_parts(c.groups, c.group_count) }
    };
    Credentials::new(c.uid, c.gid, groups).map_err(Fault::from)
}

/// Effective-ID mode check for trusted caller claims, not OS access(2).
/// # Safety
/// Name and caller/group buffers are readable/aligned for the call duration.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_posix_check_access(
    volume: u64,
    parent: i64,
    name: *const u8,
    name_len: usize,
    access: u8,
    caller: *const PwCredentials,
) -> i32 {
    run(|| {
        let caller = unsafe { credentials(caller)? };
        let name = unsafe { text(name, name_len)? };
        let access = Access::from_bits(access).map_err(Fault::from)?;
        with_session(volume, |s| {
            s.volume
                .posix(caller)
                .map_err(Fault::from)?
                .check_access(file_id(parent)?, name, access)
                .map_err(Fault::from)?;
            Ok(OK)
        })
    })
}
/// # Safety
/// Name/caller/group buffers readable; output writable/aligned/nonaliasing.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_posix_create(
    volume: u64,
    parent: i64,
    name: *const u8,
    name_len: usize,
    directory: u8,
    mode: u16,
    umask: u16,
    caller: *const PwCredentials,
    out_object: *mut i64,
) -> i32 {
    run(|| {
        required(out_object)?;
        if directory > 1 {
            return Err(Fault::invalid());
        }
        let caller = unsafe { credentials(caller)? };
        let name = unsafe { text(name, name_len)? };
        with_session(volume, |s| {
            let mut policy = s.volume.posix(caller).map_err(Fault::from)?;
            let id = if directory == 1 {
                policy.mkdir(file_id(parent)?, name, mode, umask)
            } else {
                policy.create_file(file_id(parent)?, name, mode, umask)
            }
            .map_err(Fault::from)?;
            unsafe { out_object.write(id.raw()) };
            Ok(OK)
        })
    })
}
/// # Safety
/// Name/caller/group buffers are readable/aligned for the call duration.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_posix_remove(
    volume: u64,
    parent: i64,
    name: *const u8,
    name_len: usize,
    directory: u8,
    caller: *const PwCredentials,
) -> i32 {
    run(|| {
        if directory > 1 {
            return Err(Fault::invalid());
        }
        let caller = unsafe { credentials(caller)? };
        let name = unsafe { text(name, name_len)? };
        with_session(volume, |s| {
            let mut policy = s.volume.posix(caller).map_err(Fault::from)?;
            if directory == 1 {
                policy.rmdir(file_id(parent)?, name)
            } else {
                policy.unlink(file_id(parent)?, name)
            }
            .map_err(Fault::from)?;
            Ok(OK)
        })
    })
}
/// # Safety
/// Names/caller/group buffers are readable/aligned and valid throughout the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_posix_rename(
    volume: u64,
    from_parent: i64,
    from: *const u8,
    from_len: usize,
    to_parent: i64,
    to: *const u8,
    to_len: usize,
    replace: u8,
    caller: *const PwCredentials,
) -> i32 {
    run(|| {
        if replace > 1 {
            return Err(Fault::invalid());
        }
        let caller = unsafe { credentials(caller)? };
        let from = unsafe { text(from, from_len)? };
        let to = unsafe { text(to, to_len)? };
        with_session(volume, |s| {
            s.volume
                .posix(caller)
                .map_err(Fault::from)?
                .rename(
                    file_id(from_parent)?,
                    from,
                    file_id(to_parent)?,
                    to,
                    replace == 1,
                )
                .map_err(Fault::from)?;
            Ok(OK)
        })
    })
}
impl PwCreateAttributes {
    fn core(self) -> FResult<CreateAttributes> {
        if self.reserved != 0 || self.mode > 0o7777 {
            return Err(Fault::invalid());
        }
        Ok(CreateAttributes {
            uid: self.uid,
            gid: self.gid,
            mode: self.mode as u16,
        })
    }
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PwTimestamp {
    pub seconds: i64,
    pub nanoseconds: u32,
    pub reserved: u32,
}
impl From<Timestamp> for PwTimestamp {
    fn from(t: Timestamp) -> Self {
        Self {
            seconds: t.seconds,
            nanoseconds: t.nanoseconds,
            reserved: 0,
        }
    }
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PwMetadata {
    pub uid: u32,
    pub gid: u32,
    pub mode: u32,
    pub reserved: u32,
    pub created: PwTimestamp,
    pub accessed: PwTimestamp,
    pub modified: PwTimestamp,
    pub changed: PwTimestamp,
}
/// Serialized granted-file information. Not a persistent snapshot or OS cache.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PwFileInfo {
    pub object: i64,
    pub stat: PwStat,
    pub metadata: PwMetadata,
}
impl From<FileInfo> for PwFileInfo {
    fn from(info: FileInfo) -> Self {
        let m = info.metadata;
        Self {
            object: info.stat.id.raw(),
            stat: PwStat {
                kind: match info.stat.kind {
                    Kind::File => 0,
                    Kind::Directory => 1,
                },
                size: info.stat.size,
                links: info.stat.links,
                generation: info.stat.generation,
            },
            metadata: PwMetadata {
                uid: m.uid,
                gid: m.gid,
                mode: u32::from(m.mode),
                reserved: 0,
                created: m.created.into(),
                accessed: m.accessed.into(),
                modified: m.modified.into(),
                changed: m.changed.into(),
            },
        }
    }
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PwWriteResult {
    pub written: usize,
    pub offset: u64,
    pub info: PwFileInfo,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PwMetadataUpdate {
    /// Bits 0..5: uid, gid, mode, created, accessed, modified.
    pub valid: u64,
    pub uid: u32,
    pub gid: u32,
    pub mode: u32,
    pub reserved: u32,
    pub created: PwTimestamp,
    pub accessed: PwTimestamp,
    pub modified: PwTimestamp,
}
impl PwMetadataUpdate {
    fn core(self) -> FResult<MetadataUpdate> {
        if self.valid & !63 != 0
            || self.reserved != 0
            || [self.created, self.accessed, self.modified]
                .iter()
                .any(|t| t.reserved != 0)
            || (self.valid & 4 != 0 && self.mode > 0o7777)
        {
            return Err(Fault::invalid());
        }
        let time = |flag, t: PwTimestamp| {
            (self.valid & flag != 0).then_some(Timestamp {
                seconds: t.seconds,
                nanoseconds: t.nanoseconds,
            })
        };
        Ok(MetadataUpdate {
            uid: (self.valid & 1 != 0).then_some(self.uid),
            gid: (self.valid & 2 != 0).then_some(self.gid),
            mode: (self.valid & 4 != 0).then_some(self.mode as u16),
            created: time(8, self.created),
            accessed: time(16, self.accessed),
            modified: time(32, self.modified),
        })
    }
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PwVolumeIdentity {
    pub uuid: [u8; 16],
    pub capabilities: u64,
}
#[unsafe(no_mangle)]
pub extern "C" fn pw_abi_version() -> u32 {
    1
}

/// Admit a bounded immutable read lease from a granted READ file.
/// # Safety
/// Output points to writable/aligned u64 storage for the call duration.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_posix_read_view_begin(
    volume: u64,
    file: u64,
    offset: u64,
    length: u64,
    out_view: *mut u64,
) -> i32 {
    run(|| {
        required(out_view)?;
        with_session(volume, |s| {
            let file = *s.policy_files.get(&file).ok_or_else(Fault::invalid)?;
            let token = next_token()?;
            let view = file
                .acquire_range_view(&mut s.volume, offset, length)
                .map_err(Fault::from)?;
            s.policy_views.insert(token, view);
            unsafe { out_view.write(token) };
            Ok(OK)
        })
    })
}
/// # Safety
/// Buffer writable for capacity bytes, output count writable/nonaliasing.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_posix_read_view_read(
    volume: u64,
    view: u64,
    offset: u64,
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
        with_session(volume, |s| {
            let view = *s.policy_views.get(&view).ok_or_else(Fault::invalid)?;
            let data = view
                .read(&s.volume, offset, capacity)
                .map_err(Fault::from)?;
            if !data.is_empty() {
                unsafe { std::ptr::copy_nonoverlapping(data.as_ptr(), buffer, data.len()) };
            }
            unsafe { out_read.write(data.len()) };
            Ok(OK)
        })
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn pw_posix_read_view_close(volume: u64, view: u64) -> i32 {
    run(|| {
        with_session(volume, |s| {
            s.policy_views
                .get(&view)
                .copied()
                .ok_or_else(Fault::invalid)?
                .close(&mut s.volume)
                .map_err(Fault::from)?;
            s.policy_views.remove(&view);
            Ok(OK)
        })
    })
}

/// Open a separately typed, granted file token using explicit trusted claims.
/// # Safety
/// Name/caller/groups readable, output writable/aligned/nonaliasing.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_posix_file_open(
    volume: u64,
    parent: i64,
    name: *const u8,
    name_len: usize,
    access: u8,
    caller: *const PwCredentials,
    out_file: *mut u64,
) -> i32 {
    run(|| {
        required(out_file)?;
        let access = match access {
            4 => FileAccess::Read,
            2 => FileAccess::Write,
            6 => FileAccess::ReadWrite,
            _ => return Err(Fault::invalid()),
        };
        let caller = unsafe { credentials(caller)? };
        let name = unsafe { text(name, name_len)? };
        with_session(volume, |s| {
            if s.files.len() + s.policy_files.len() >= MAX_HANDLES {
                return Err(Fault::resource());
            }
            let token = next_token()?;
            let file = s
                .volume
                .posix(caller)
                .map_err(Fault::from)?
                .open_file(file_id(parent)?, name, access)
                .map_err(Fault::from)?;
            s.policy_files.insert(token, file);
            unsafe { out_file.write(token) };
            Ok(OK)
        })
    })
}

/// # Safety
/// Buffer writable for capacity bytes, output count writable/nonaliasing.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_posix_file_read(
    volume: u64,
    file: u64,
    offset: u64,
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
        with_session(volume, |s| {
            let file = *s.policy_files.get(&file).ok_or_else(Fault::invalid)?;
            let bytes = file
                .read(&s.volume, offset, capacity)
                .map_err(Fault::from)?;
            if !bytes.is_empty() {
                unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), buffer, bytes.len()) };
            }
            unsafe { out_read.write(bytes.len()) };
            Ok(OK)
        })
    })
}

/// # Safety
/// Data readable for length bytes, output count writable/nonaliasing.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_posix_file_write(
    volume: u64,
    file: u64,
    offset: u64,
    data: *const u8,
    length: usize,
    out_written: *mut usize,
) -> i32 {
    run(|| {
        required(out_written)?;
        readable(data, length)?;
        if length > MAX_IO {
            return Err(Fault::invalid());
        }
        with_session(volume, |s| {
            let file = *s.policy_files.get(&file).ok_or_else(Fault::invalid)?;
            let data = if length == 0 {
                &[]
            } else {
                unsafe { slice::from_raw_parts(data, length) }
            };
            let count = file
                .write(&mut s.volume, offset, data)
                .map_err(Fault::from)?;
            unsafe { out_written.write(count) };
            Ok(OK)
        })
    })
}
/// # Safety
/// Output must be writable/aligned/nonaliasing for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_posix_file_info(
    volume: u64,
    file: u64,
    out_info: *mut PwFileInfo,
) -> i32 {
    run(|| {
        required(out_info)?;
        with_session(volume, |s| {
            let file = *s.policy_files.get(&file).ok_or_else(Fault::invalid)?;
            let info = file.info(&s.volume).map_err(Fault::from)?;
            unsafe { out_info.write(info.into()) };
            Ok(OK)
        })
    })
}
/// Atomic offset/append/constrained write with transaction-captured information.
/// # Safety
/// Data readable for length bytes, result writable/aligned/nonaliasing.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_posix_file_write_request(
    volume: u64,
    file: u64,
    offset: u64,
    data: *const u8,
    length: usize,
    flags: u32,
    out_result: *mut PwWriteResult,
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
                unsafe { slice::from_raw_parts(data, length) }
            };
            let result = file
                .write_request(&mut s.volume, position, data, flags & 2 != 0)
                .map_err(Fault::from)?;
            unsafe {
                out_result.write(PwWriteResult {
                    written: result.written,
                    offset: result.offset,
                    info: result.info.into(),
                })
            };
            Ok(OK)
        })
    })
}
/// Capture new file information before truncate commits, not in a second query.
/// # Safety
/// Output must be writable/aligned/nonaliasing for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_posix_file_truncate_info(
    volume: u64,
    file: u64,
    size: u64,
    out_info: *mut PwFileInfo,
) -> i32 {
    run(|| {
        required(out_info)?;
        with_session(volume, |s| {
            let file = *s.policy_files.get(&file).ok_or_else(Fault::invalid)?;
            let info = file
                .truncate_request(&mut s.volume, size)
                .map_err(Fault::from)?;
            unsafe { out_info.write(info.into()) };
            Ok(OK)
        })
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn pw_posix_file_truncate(volume: u64, file: u64, size: u64) -> i32 {
    run(|| {
        with_session(volume, |s| {
            s.policy_files
                .get(&file)
                .copied()
                .ok_or_else(Fault::invalid)?
                .truncate(&mut s.volume, size)
                .map_err(Fault::from)?;
            Ok(OK)
        })
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn pw_posix_file_close(volume: u64, file: u64) -> i32 {
    run(|| {
        with_session(volume, |s| {
            s.policy_files
                .get(&file)
                .copied()
                .ok_or_else(Fault::invalid)?
                .close(&mut s.volume)
                .map_err(Fault::from)?;
            s.policy_files.remove(&file);
            Ok(OK)
        })
    })
}
/// # Safety
/// Name/caller/groups/update are readable/aligned and valid throughout the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_posix_metadata_set(
    volume: u64,
    parent: i64,
    name: *const u8,
    name_len: usize,
    caller: *const PwCredentials,
    update: *const PwMetadataUpdate,
) -> i32 {
    run(|| {
        let caller = unsafe { credentials(caller)? };
        let name = unsafe { text(name, name_len)? };
        readable(update, 1)?;
        let update = unsafe { update.read() }.core()?;
        with_session(volume, |s| {
            s.volume
                .posix(caller)
                .map_err(Fault::from)?
                .set_metadata(file_id(parent)?, name, update)
                .map_err(Fault::from)?;
            Ok(OK)
        })
    })
}
/// # Safety
/// Caller/groups/update are readable/aligned and valid throughout the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_posix_file_truncate_with_metadata(
    volume: u64,
    file: u64,
    size: u64,
    caller: *const PwCredentials,
    update: *const PwMetadataUpdate,
) -> i32 {
    run(|| {
        let caller = unsafe { credentials(caller)? };
        readable(update, 1)?;
        let update = unsafe { update.read() }.core()?;
        with_session(volume, |s| {
            let file = *s.policy_files.get(&file).ok_or_else(Fault::invalid)?;
            s.volume
                .posix(caller)
                .map_err(Fault::from)?
                .truncate_with_metadata(file, size, update)
                .map_err(Fault::from)?;
            Ok(OK)
        })
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn pw_root_id() -> i64 {
    ROOT.raw()
}

/// # Safety
/// path points to path_len valid UTF-8 bytes. out_volume is writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_volume_create(
    path: *const u8,
    path_len: usize,
    quota: u64,
    out_volume: *mut u64,
) -> i32 {
    run(|| {
        required(out_volume)?;
        let path = unsafe { text(path, path_len)? };
        with_table(|all| {
            if all.len() >= MAX_SESSIONS {
                return Err(Fault::resource());
            }
            let token = next_token()?;
            let volume = Volume::create(Path::new(path), quota).map_err(Fault::from)?;
            all.insert(
                token,
                Session {
                    volume,
                    files: BTreeMap::new(),
                    policy_files: BTreeMap::new(),
                    policy_views: BTreeMap::new(),
                    dirs: BTreeMap::new(),
                    views: BTreeMap::new(),
                },
            );
            unsafe {
                out_volume.write(token);
            }
            Ok(OK)
        })
    })
}

/// Explicit opt-in v2 creation; no upgrade or OS identity inference.
/// # Safety
/// Path is valid UTF-8 bytes; attributes is readable; output writable/nonaliasing.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_volume_create_with_metadata(
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
            let volume = Volume::create_with_metadata(Path::new(path), quota, attributes)
                .map_err(Fault::from)?;
            all.insert(
                token,
                Session {
                    volume,
                    files: BTreeMap::new(),
                    policy_files: BTreeMap::new(),
                    policy_views: BTreeMap::new(),
                    dirs: BTreeMap::new(),
                    views: BTreeMap::new(),
                },
            );
            unsafe { out_volume.write(token) };
            Ok(OK)
        })
    })
}

/// # Safety
/// Output points to a writable, aligned PwVolumeIdentity.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_volume_identity(
    volume: u64,
    out_identity: *mut PwVolumeIdentity,
) -> i32 {
    run(|| {
        required(out_identity)?;
        with_session(volume, |s| {
            let i = s.volume.volume_identity().map_err(Fault::from)?;
            unsafe {
                out_identity.write(PwVolumeIdentity {
                    uuid: i.uuid,
                    capabilities: i.capabilities,
                })
            };
            Ok(OK)
        })
    })
}

/// # Safety
/// Output points to a writable, aligned PwMetadata.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_metadata_get(
    volume: u64,
    object: i64,
    out_metadata: *mut PwMetadata,
) -> i32 {
    run(|| {
        required(out_metadata)?;
        with_session(volume, |s| {
            let m = s.volume.metadata(file_id(object)?).map_err(Fault::from)?;
            unsafe {
                out_metadata.write(PwMetadata {
                    uid: m.uid,
                    gid: m.gid,
                    mode: m.mode as u32,
                    reserved: 0,
                    created: m.created.into(),
                    accessed: m.accessed.into(),
                    modified: m.modified.into(),
                    changed: m.changed.into(),
                })
            };
            Ok(OK)
        })
    })
}

/// Trusted core metadata mutation. Does not authorize/authenticate OS callers.
/// # Safety
/// Update points to a readable, aligned PwMetadataUpdate.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_metadata_set(
    volume: u64,
    object: i64,
    update: *const PwMetadataUpdate,
) -> i32 {
    run(|| {
        readable(update, 1)?;
        let update = unsafe { update.read() }.core()?;
        with_session(volume, |s| {
            s.volume
                .set_metadata(file_id(object)?, update)
                .map_err(Fault::from)?;
            Ok(OK)
        })
    })
}

/// # Safety
/// Name/attributes are readable, output writable; all pointers valid/nonaliasing.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_node_create_with_metadata(
    volume: u64,
    parent: i64,
    name: *const u8,
    name_len: usize,
    directory: u8,
    attributes: *const PwCreateAttributes,
    out_object: *mut i64,
) -> i32 {
    run(|| {
        required(out_object)?;
        readable(attributes, 1)?;
        if directory > 1 {
            return Err(Fault::invalid());
        }
        let name = unsafe { text(name, name_len)? };
        let a = unsafe { attributes.read() }.core()?;
        with_session(volume, |s| {
            let parent = file_id(parent)?;
            let object = if directory == 1 {
                s.volume.mkdir_with_attributes(parent, name, a)
            } else {
                s.volume.create_file_with_attributes(parent, name, a)
            }
            .map_err(Fault::from)?;
            unsafe { out_object.write(object.raw()) };
            Ok(OK)
        })
    })
}
/// # Safety
/// path points to path_len valid UTF-8 bytes. out_volume is writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_volume_open(
    path: *const u8,
    path_len: usize,
    out_volume: *mut u64,
) -> i32 {
    run(|| {
        required(out_volume)?;
        let path = unsafe { text(path, path_len)? };
        with_table(|all| {
            if all.len() >= MAX_SESSIONS {
                return Err(Fault::resource());
            }
            let token = next_token()?;
            let volume = Volume::open(Path::new(path)).map_err(Fault::from)?;
            all.insert(
                token,
                Session {
                    volume,
                    files: BTreeMap::new(),
                    policy_files: BTreeMap::new(),
                    policy_views: BTreeMap::new(),
                    dirs: BTreeMap::new(),
                    views: BTreeMap::new(),
                },
            );
            unsafe {
                out_volume.write(token);
            }
            Ok(OK)
        })
    })
}
/// Releases engine ownership. Not an operating-system unmount or a durability certification.
#[unsafe(no_mangle)]
pub extern "C" fn pw_volume_close(volume: u64) -> i32 {
    run(|| {
        with_table(|all| {
            let session = all.get(&volume).ok_or_else(Fault::invalid)?;
            if !session.files.is_empty()
                || !session.policy_files.is_empty()
                || !session.policy_views.is_empty()
                || !session.dirs.is_empty()
                || !session.views.is_empty()
            {
                return Err(Fault::busy());
            }
            all.remove(&volume);
            Ok(OK)
        })
    })
}
/// # Safety
/// out_stat must be valid writable memory for a PwStat.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_stat(volume: u64, object: i64, out_stat: *mut PwStat) -> i32 {
    run(|| {
        required(out_stat)?;
        with_session(volume, |s| {
            let stat = s.volume.stat(file_id(object)?).map_err(Fault::from)?;
            unsafe {
                out_stat.write(PwStat {
                    kind: if stat.kind == Kind::File { 0 } else { 1 },
                    size: stat.size,
                    links: stat.links,
                    generation: stat.generation,
                });
            }
            Ok(OK)
        })
    })
}
/// # Safety
/// name is valid UTF-8 of name_len bytes; out_object is writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_lookup(
    volume: u64,
    parent: i64,
    name: *const u8,
    name_len: usize,
    out_object: *mut i64,
) -> i32 {
    run(|| {
        required(out_object)?;
        let name = unsafe { text(name, name_len)? };
        with_session(volume, |s| {
            let id = s
                .volume
                .lookup(file_id(parent)?, name)
                .map_err(Fault::from)?;
            unsafe {
                out_object.write(id.raw());
            }
            Ok(OK)
        })
    })
}
/// # Safety
/// name is readable UTF-8; outputs are writable and non-aliasing. On a small
/// buffer only out_name_len changes. No NUL terminator is written.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_lookup_entry(
    volume: u64,
    parent: i64,
    name: *const u8,
    name_len: usize,
    name_buffer: *mut u8,
    capacity: usize,
    out_name_len: *mut usize,
    out_object: *mut i64,
) -> i32 {
    run(|| {
        required(out_name_len)?;
        required(out_object)?;
        readable(name_buffer, capacity)?;
        let name = unsafe { text(name, name_len)? };
        with_session(volume, |s| {
            let entry = s
                .volume
                .lookup_entry(file_id(parent)?, name)
                .map_err(Fault::from)?;
            let bytes = entry.name.as_bytes();
            if bytes.len() > capacity {
                unsafe {
                    out_name_len.write(bytes.len());
                }
                return Ok(BUFFER_TOO_SMALL);
            }
            unsafe {
                std::ptr::copy_nonoverlapping(bytes.as_ptr(), name_buffer, bytes.len());
                out_name_len.write(bytes.len());
                out_object.write(entry.id.raw());
            }
            Ok(OK)
        })
    })
}
/// # Safety
/// out_parent points to writable i64 storage.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_parent_directory(
    volume: u64,
    object: i64,
    out_parent: *mut i64,
) -> i32 {
    run(|| {
        required(out_parent)?;
        with_session(volume, |s| {
            let parent = s
                .volume
                .parent_directory(file_id(object)?)
                .map_err(Fault::from)?;
            unsafe {
                out_parent.write(parent.raw());
            }
            Ok(OK)
        })
    })
}
fn create_node(
    volume: u64,
    parent: i64,
    name: &str,
    directory: bool,
    out: *mut i64,
) -> FResult<i32> {
    required(out)?;
    with_session(volume, |s| {
        let parent = file_id(parent)?;
        let id = if directory {
            s.volume.mkdir(parent, name)
        } else {
            s.volume.create_file(parent, name)
        }
        .map_err(Fault::from)?;
        unsafe {
            out.write(id.raw());
        }
        Ok(OK)
    })
}
/// # Safety
/// name is valid UTF-8 of name_len bytes; out_object is writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_file_create(
    volume: u64,
    parent: i64,
    name: *const u8,
    name_len: usize,
    out_object: *mut i64,
) -> i32 {
    run(|| {
        create_node(
            volume,
            parent,
            unsafe { text(name, name_len)? },
            false,
            out_object,
        )
    })
}
/// # Safety
/// name is valid UTF-8 of name_len bytes; out_object is writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_directory_create(
    volume: u64,
    parent: i64,
    name: *const u8,
    name_len: usize,
    out_object: *mut i64,
) -> i32 {
    run(|| {
        create_node(
            volume,
            parent,
            unsafe { text(name, name_len)? },
            true,
            out_object,
        )
    })
}
/// # Safety
/// out_file must be valid writable memory for a u64.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_file_open(volume: u64, object: i64, out_file: *mut u64) -> i32 {
    run(|| {
        required(out_file)?;
        with_session(volume, |s| {
            if s.files.len() + s.policy_files.len() >= MAX_HANDLES {
                return Err(Fault::resource());
            }
            let token = next_token()?;
            let h = s.volume.open_file(file_id(object)?).map_err(Fault::from)?;
            s.files.insert(token, h);
            unsafe {
                out_file.write(token);
            }
            Ok(OK)
        })
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn pw_file_close(volume: u64, file: u64) -> i32 {
    run(|| {
        with_session(volume, |s| {
            let h = *s.files.get(&file).ok_or_else(Fault::invalid)?;
            s.volume.close_file(h).map_err(Fault::from)?;
            s.files.remove(&file);
            Ok(OK)
        })
    })
}
/// # Safety
/// out_view must point to writable u64 storage. Offsets are absolute.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_read_view_begin(
    volume: u64,
    file: u64,
    offset: u64,
    length: u64,
    out_view: *mut u64,
) -> i32 {
    run(|| {
        required(out_view)?;
        with_session(volume, |s| {
            let handle = *s.files.get(&file).ok_or_else(Fault::invalid)?;
            let token = next_token()?;
            let view = s
                .volume
                .acquire_range_view(handle, offset, length)
                .map_err(Fault::from)?;
            s.views.insert(token, view);
            unsafe {
                out_view.write(token);
            }
            Ok(OK)
        })
    })
}
/// # Safety
/// buffer is writable for capacity bytes and out_read points to writable usize.
/// On error neither output is changed. A zero capacity permits a null buffer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_read_view_read(
    volume: u64,
    view: u64,
    offset: u64,
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
        with_session(volume, |s| {
            let view = *s.views.get(&view).ok_or_else(Fault::invalid)?;
            let bytes = s
                .volume
                .read_view(view, offset, capacity)
                .map_err(Fault::from)?;
            if !bytes.is_empty() {
                unsafe {
                    std::ptr::copy_nonoverlapping(bytes.as_ptr(), buffer, bytes.len());
                }
            }
            unsafe {
                out_read.write(bytes.len());
            }
            Ok(OK)
        })
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn pw_read_view_close(volume: u64, view: u64) -> i32 {
    run(|| {
        with_session(volume, |s| {
            let lease = *s.views.get(&view).ok_or_else(Fault::invalid)?;
            s.volume.release_view(lease).map_err(Fault::from)?;
            s.views.remove(&view);
            Ok(OK)
        })
    })
}
/// # Safety
/// If capacity > 0, buffer points to capacity writable bytes, out_read to a writable usize.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_file_read(
    volume: u64,
    file: u64,
    offset: u64,
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
        with_session(volume, |s| {
            let h = *s.files.get(&file).ok_or_else(Fault::invalid)?;
            let bytes = s.volume.read(h, offset, capacity).map_err(Fault::from)?;
            if !bytes.is_empty() {
                unsafe {
                    std::ptr::copy_nonoverlapping(bytes.as_ptr(), buffer, bytes.len());
                }
            }
            unsafe {
                out_read.write(bytes.len());
            }
            Ok(OK)
        })
    })
}
/// # Safety
/// If length > 0, data points to length readable bytes; out_written is writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_file_write(
    volume: u64,
    file: u64,
    offset: u64,
    data: *const u8,
    length: usize,
    out_written: *mut usize,
) -> i32 {
    run(|| {
        required(out_written)?;
        readable(data, length)?;
        if length > MAX_IO {
            return Err(Fault::invalid());
        }
        with_session(volume, |s| {
            let h = *s.files.get(&file).ok_or_else(Fault::invalid)?;
            let empty: &[u8] = &[];
            let bytes = if length == 0 {
                empty
            } else {
                unsafe { slice::from_raw_parts(data, length) }
            };
            let n = s.volume.write(h, offset, bytes).map_err(Fault::from)?;
            unsafe {
                out_written.write(n);
            }
            Ok(OK)
        })
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn pw_file_truncate(volume: u64, file: u64, size: u64) -> i32 {
    run(|| {
        with_session(volume, |s| {
            let h = *s.files.get(&file).ok_or_else(Fault::invalid)?;
            s.volume.truncate(h, size).map_err(Fault::from)?;
            Ok(OK)
        })
    })
}
/// Atomically commit v2 size/blocks and metadata; no caller authorization.
/// # Safety
/// Update points to a readable, aligned PwMetadataUpdate for the call duration.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_file_truncate_with_metadata(
    volume: u64,
    file: u64,
    size: u64,
    update: *const PwMetadataUpdate,
) -> i32 {
    run(|| {
        readable(update, 1)?;
        let update = unsafe { update.read() }.core()?;
        with_session(volume, |s| {
            let h = *s.files.get(&file).ok_or_else(Fault::invalid)?;
            s.volume
                .truncate_with_metadata(h, size, update)
                .map_err(Fault::from)?;
            Ok(OK)
        })
    })
}
/// # Safety
/// name is valid UTF-8 of name_len bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_remove(
    volume: u64,
    parent: i64,
    name: *const u8,
    name_len: usize,
    directory: u8,
) -> i32 {
    run(|| {
        let name = unsafe { text(name, name_len)? };
        if directory > 1 {
            return Err(Fault::invalid());
        }
        with_session(volume, |s| {
            let parent = file_id(parent)?;
            if directory == 1 {
                s.volume.rmdir(parent, name)
            } else {
                s.volume.unlink(parent, name)
            }
            .map_err(Fault::from)?;
            Ok(OK)
        })
    })
}
/// # Safety
/// from_name and to_name point to the respective valid UTF-8 lengths.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_rename(
    volume: u64,
    from_parent: i64,
    from_name: *const u8,
    from_len: usize,
    to_parent: i64,
    to_name: *const u8,
    to_len: usize,
    replace: u8,
) -> i32 {
    run(|| {
        let from = unsafe { text(from_name, from_len)? };
        let to = unsafe { text(to_name, to_len)? };
        if replace > 1 {
            return Err(Fault::invalid());
        }
        with_session(volume, |s| {
            s.volume
                .rename(
                    file_id(from_parent)?,
                    from,
                    file_id(to_parent)?,
                    to,
                    replace == 1,
                )
                .map_err(Fault::from)?;
            Ok(OK)
        })
    })
}
/// # Safety
/// name points to name_len valid UTF-8 bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_hard_link(
    volume: u64,
    object: i64,
    parent: i64,
    name: *const u8,
    name_len: usize,
) -> i32 {
    run(|| {
        let name = unsafe { text(name, name_len)? };
        with_session(volume, |s| {
            s.volume
                .hard_link(file_id(object)?, file_id(parent)?, name)
                .map_err(Fault::from)?;
            Ok(OK)
        })
    })
}
/// # Safety
/// out_cursor points to writable u64 memory.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_dir_begin(volume: u64, parent: i64, out_cursor: *mut u64) -> i32 {
    run(|| {
        required(out_cursor)?;
        with_session(volume, |s| {
            if s.dirs.len() >= MAX_CURSORS {
                return Err(Fault::resource());
            }
            let entries = s
                .volume
                .list_bounded(file_id(parent)?, MAX_DIRECTORY_ENTRIES)
                .map_err(Fault::from)?;
            let token = next_token()?;
            s.dirs.insert(
                token,
                Cursor {
                    entries,
                    position: 0,
                },
            );
            unsafe {
                out_cursor.write(token);
            }
            Ok(OK)
        })
    })
}
/// # Safety
/// out_name_len and out_object must be writable. If capacity is sufficient,
/// name_buf must reference capacity writable bytes. On BUFFER_TOO_SMALL,
/// out_name_len receives the required byte length and the cursor does not advance.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_dir_next(
    volume: u64,
    cursor: u64,
    name_buf: *mut u8,
    capacity: usize,
    out_name_len: *mut usize,
    out_object: *mut i64,
) -> i32 {
    run(|| {
        required(out_name_len)?;
        required(out_object)?;
        readable(name_buf, capacity)?;
        with_session(volume, |s| {
            let cursor = s.dirs.get_mut(&cursor).ok_or_else(Fault::invalid)?;
            let Some(entry) = cursor.entries.get(cursor.position) else {
                return Ok(EOF);
            };
            let bytes = entry.name.as_bytes();
            unsafe {
                out_name_len.write(bytes.len());
            }
            if bytes.len() > capacity {
                return Ok(BUFFER_TOO_SMALL);
            }
            if !bytes.is_empty() {
                unsafe {
                    std::ptr::copy_nonoverlapping(bytes.as_ptr(), name_buf, bytes.len());
                }
            }
            unsafe {
                out_object.write(entry.id.raw());
            }
            cursor.position += 1;
            Ok(OK)
        })
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn pw_dir_close(volume: u64, cursor: u64) -> i32 {
    run(|| {
        with_session(volume, |s| {
            s.dirs.remove(&cursor).ok_or_else(Fault::invalid)?;
            Ok(OK)
        })
    })
}
/// Seek inside the SAME bounded directory snapshot. Invalid positions do not
/// advance it. The caller must invalidate native OS verifiers on mutations.
#[unsafe(no_mangle)]
pub extern "C" fn pw_dir_seek(volume: u64, cursor: u64, position: u64) -> i32 {
    run(|| {
        let position = usize::try_from(position).map_err(|_| Fault::invalid())?;
        with_session(volume, |s| {
            let cursor = s.dirs.get_mut(&cursor).ok_or_else(Fault::invalid)?;
            if position > cursor.entries.len() {
                return Err(Fault::invalid());
            }
            cursor.position = position;
            Ok(OK)
        })
    })
}
/// # Safety
/// out_space points to a valid, writable, aligned PwSpace.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_volume_space(volume: u64, out_space: *mut PwSpace) -> i32 {
    run(|| {
        required(out_space)?;
        with_session(volume, |s| {
            let info = s.volume.capacity().map_err(Fault::from)?;
            unsafe {
                out_space.write(PwSpace {
                    logical_quota_bytes: info.logical_quota_bytes,
                    logical_used_bytes: info.logical_used_bytes,
                    host_available_bytes: info.host_available_bytes,
                    host_data_admission_bytes: info.host_data_admission_bytes,
                    apparent_free_bytes: info.apparent_free_bytes,
                });
            }
            Ok(OK)
        })
    })
}
/// # Safety
/// out_result points to valid, writable PwCheckpoint. Catalog only, not fsync.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_catalog_checkpoint(volume: u64, out_result: *mut PwCheckpoint) -> i32 {
    run(|| {
        required(out_result)?;
        with_session(volume, |s| {
            let result = s.volume.checkpoint_catalog().map_err(Fault::from)?;
            unsafe {
                out_result.write(PwCheckpoint {
                    wal_frames: result.wal_frames,
                    checkpointed_frames: result.checkpointed_frames,
                });
            }
            Ok(OK)
        })
    })
}
/// # Safety
/// out_result points to writable PwStorageSync. Output is unchanged on failure.
/// This is a backend host-file barrier, NOT OS cache drain or safe device eject.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_storage_synchronize(
    volume: u64,
    out_result: *mut PwStorageSync,
) -> i32 {
    run(|| {
        required(out_result)?;
        with_session(volume, |s| {
            let result = s.volume.synchronize_storage().map_err(Fault::from)?;
            unsafe {
                out_result.write(PwStorageSync {
                    segment_files: result.segment_files,
                    wal_frames: result.checkpoint.wal_frames,
                    checkpointed_frames: result.checkpoint.checkpointed_frames,
                });
            }
            Ok(OK)
        })
    })
}
/// # Safety
/// out_maintenance and out_protected point to writable u32 values.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_health(
    volume: u64,
    out_maintenance: *mut u32,
    out_protected: *mut u32,
) -> i32 {
    run(|| {
        required(out_maintenance)?;
        required(out_protected)?;
        with_session(volume, |s| {
            let status = match s.volume.maintenance_status() {
                MaintenanceStatus::Healthy => 0,
                MaintenanceStatus::DeferredSpace { .. } => 1,
                MaintenanceStatus::Failed { .. } => 2,
            };
            unsafe {
                out_maintenance.write(status);
                out_protected.write(u32::from(s.volume.is_write_protected()));
            }
            Ok(OK)
        })
    })
}
