//! M0-A experimental C ABI boundary. This is NOT a mounted filesystem.
//!
//! The storage core forbids unsafe code; pointer handling is isolated here.
//! All public exports require valid C pointer regions for their stated lengths.
//! Sessions are opaque monotonic IDs, NOT Rust pointers. Closing a session
//! blocks new operations and waits for its in-flight serialized call.
//! Do not use this API to claim sync, directory enumeration or ACL support.
#![deny(unsafe_op_in_unsafe_fn)]

use portaweave_core::{Error, FileId, Handle, Kind, MaintenanceStatus, MAX_IO, Volume};
use std::{
    collections::BTreeMap,
    ffi::{CStr, c_char},
    panic::{AssertUnwindSafe, catch_unwind},
    path::PathBuf,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
};

pub const PW_OK: i32 = 0;
pub const PW_INVALID_ARGUMENT: i32 = -1;
pub const PW_NOT_FOUND: i32 = -2;
pub const PW_ALREADY_EXISTS: i32 = -3;
pub const PW_BAD_HANDLE: i32 = -4;
pub const PW_NOT_DIRECTORY: i32 = -5;
pub const PW_IS_DIRECTORY: i32 = -6;
pub const PW_NOT_EMPTY: i32 = -7;
pub const PW_NO_SPACE: i32 = -8;
pub const PW_RESOURCE_LIMIT: i32 = -9;
pub const PW_BUSY: i32 = -10;
pub const PW_UNSUPPORTED: i32 = -11;
pub const PW_CORRUPT: i32 = -12;
pub const PW_READ_ONLY: i32 = -13;
pub const PW_IO_ERROR: i32 = -14;
pub const PW_INTERNAL: i32 = -127;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct PwFileStat {
    pub id: u64,
    /// 0=file, 1=directory. Does not represent OS permissions.
    pub kind: u32,
    pub size: u64,
    pub links: u64,
    pub generation: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct PwHealth {
    pub write_protected: u8,
    /// 0=healthy; 1=deferred for space; 2=maintenance failed.
    pub maintenance: u32,
}

struct Session {
    volume: Volume,
    handles: BTreeMap<u64, Handle>,
    next_handle: u64,
}
struct Owner {
    session: Mutex<Session>,
    closing: AtomicBool,
}
#[derive(Default)]
struct Registry {
    next_id: u64,
    active: BTreeMap<u64, Arc<Owner>>,
}
static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();

fn registry() -> &'static Mutex<Registry> {
    REGISTRY.get_or_init(|| Mutex::new(Registry::default()))
}
fn code(err: Error) -> i32 {
    match err {
        Error::NotFound => PW_NOT_FOUND,
        Error::Exists => PW_ALREADY_EXISTS,
        Error::InvalidHandle => PW_BAD_HANDLE,
        Error::NotDirectory => PW_NOT_DIRECTORY,
        Error::IsDirectory => PW_IS_DIRECTORY,
        Error::NotEmpty => PW_NOT_EMPTY,
        Error::Quota | Error::HostSpace { .. } => PW_NO_SPACE,
        Error::ResourceLimit | Error::RecoveryLimit => PW_RESOURCE_LIMIT,
        Error::Busy => PW_BUSY,
        Error::UnsupportedFormat => PW_UNSUPPORTED,
        Error::Corrupt(_) => PW_CORRUPT,
        Error::WriteProtected => PW_READ_ONLY,
        Error::InvalidName | Error::InvalidRange | Error::Cycle => PW_INVALID_ARGUMENT,
        Error::Io(err) => match err.kind() {
            std::io::ErrorKind::NotFound => PW_NOT_FOUND,
            std::io::ErrorKind::AlreadyExists => PW_ALREADY_EXISTS,
            std::io::ErrorKind::StorageFull => PW_NO_SPACE,
            std::io::ErrorKind::PermissionDenied => PW_READ_ONLY,
            _ => PW_IO_ERROR,
        },
        Error::Sql(_) | Error::Injected => PW_IO_ERROR,
    }
}
fn enter<T>(id: u64, action: impl FnOnce(&mut Session) -> portaweave_core::Result<T>) -> Result<T, i32> {
    let session = {
        let g = registry().lock().map_err(|_| PW_INTERNAL)?;
        g.active.get(&id).cloned().ok_or(PW_BAD_HANDLE)?
    };
    let mut guard = session.session.lock().map_err(|_| PW_INTERNAL)?;
    if session.closing.load(Ordering::SeqCst) {
        return Err(PW_BAD_HANDLE);
    }
    action(&mut guard).map_err(code)
}
fn register(volume: Volume) -> Result<u64, i32> {
    let mut g = registry().lock().map_err(|_| PW_INTERNAL)?;
    let id = g.next_id.checked_add(1).ok_or(PW_RESOURCE_LIMIT)?;
    g.next_id = id;
    g.active.insert(
        id,
        Arc::new(Owner {
            session: Mutex::new(Session { volume, handles: BTreeMap::new(), next_handle: 0 }),
            closing: AtomicBool::new(false),
        }),
    );
    Ok(id)
}
fn respond(action: impl FnOnce() -> Result<(), i32>) -> i32 {
    match catch_unwind(AssertUnwindSafe(action)) {
        Ok(Ok(())) => PW_OK,
        Ok(Err(code)) => code,
        Err(_) => PW_INTERNAL,
    }
}
fn object(raw: u64) -> portaweave_core::Result<FileId> {
    FileId::from_raw(raw)
}

/// # Safety
/// `ptr` must point to a valid, NUL-terminated UTF-8 C string.
unsafe fn path(ptr: *const c_char) -> Result<PathBuf, i32> {
    if ptr.is_null() { return Err(PW_INVALID_ARGUMENT); }
    // SAFETY: caller supplies valid NUL-terminated C string.
    let bytes = unsafe { CStr::from_ptr(ptr) };
    let text = bytes.to_str().map_err(|_| PW_INVALID_ARGUMENT)?;
    if text.is_empty() { return Err(PW_INVALID_ARGUMENT); }
    Ok(PathBuf::from(text))
}
/// # Safety
/// For nonzero `len`, `ptr` must be readable for that many bytes.
unsafe fn name<'a>(ptr: *const u8, len: usize) -> Result<&'a str, i32> {
    if len == 0 || len > 255 || ptr.is_null() { return Err(PW_INVALID_ARGUMENT); }
    // SAFETY: documented valid readable buffer; no ownership transfer.
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    std::str::from_utf8(bytes).map_err(|_| PW_INVALID_ARGUMENT)
}
/// # Safety
/// `out` must point to one writable instance of `T`.
unsafe fn output<T>(out: *mut T, value: T) -> Result<(), i32> {
    if out.is_null() { return Err(PW_INVALID_ARGUMENT); }
    // SAFETY: caller owns one writable instance of T, not aliased to input buffers.
    unsafe { out.write(value) };
    Ok(())
}

/// # Safety
/// `path_utf8` is a valid NUL-terminated UTF-8 path; `out_session` is writable.
/// The target path must not exist. Creation may leave an inspectable incomplete
/// directory on any failure; callers must never use a valuable existing path.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_volume_create(
    path_utf8: *const c_char, quota: u64, out_session: *mut u64,
) -> i32 {
    respond(|| {
        if out_session.is_null() { return Err(PW_INVALID_ARGUMENT); }
        // SAFETY: validated by the documented C caller contract.
        let p = unsafe { path(path_utf8)? };
        let id = register(Volume::create(p, quota).map_err(code)?)?;
        unsafe { output(out_session, id) }
    })
}
/// # Safety
/// `path_utf8` is a valid NUL-terminated UTF-8 path; `out_session` is writable.
/// Opening an experimental container may perform recovery and maintenance.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_volume_open(
    path_utf8: *const c_char, out_session: *mut u64,
) -> i32 {
    respond(|| {
        if out_session.is_null() { return Err(PW_INVALID_ARGUMENT); }
        let p = unsafe { path(path_utf8)? };
        let id = register(Volume::open(p).map_err(code)?)?;
        unsafe { output(out_session, id) }
    })
}
/// # Safety
/// The caller must stop issuing operations using this session ID. Concurrent
/// operations already inside the engine are serialized and allowed to finish.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_volume_close(session: u64) -> i32 {
    respond(|| {
        let owner = {
            let mut g = registry().lock().map_err(|_| PW_INTERNAL)?;
            let owner = g.active.remove(&session).ok_or(PW_BAD_HANDLE)?;
            owner.closing.store(true, Ordering::SeqCst);
            owner
        };
        // Wait for a currently executing callback to complete before dropping
        // the SQLite connection and owner lock. Stale callbacks fail admission.
        let guard = owner.session.lock().map_err(|_| PW_INTERNAL)?;
        drop(guard);
        Ok(())
    })
}
/// # Safety
/// `out_health` points to one writable PwHealth.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_health(session: u64, out_health: *mut PwHealth) -> i32 {
    respond(|| {
        if out_health.is_null() { return Err(PW_INVALID_ARGUMENT); }
        let health = enter(session, |s| {
            let maintenance = match s.volume.maintenance_status() {
                MaintenanceStatus::Healthy => 0,
                MaintenanceStatus::DeferredSpace { .. } => 1,
                MaintenanceStatus::Failed { .. } => 2,
            };
            Ok(PwHealth { write_protected: u8::from(s.volume.is_write_protected()), maintenance })
        })?;
        unsafe { output(out_health, health) }
    })
}
/// # Safety
/// `out_stat` points to one writable PwFileStat.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_stat(session: u64, object_id: u64, out_stat: *mut PwFileStat) -> i32 {
    respond(|| {
        if out_stat.is_null() { return Err(PW_INVALID_ARGUMENT); }
        let result = enter(session, |s| {
            let stat = s.volume.stat(object(object_id)?)?;
            Ok(PwFileStat {
                id: stat.id.raw(),
                kind: if stat.kind == Kind::File { 0 } else { 1 },
                size: stat.size,
                links: stat.links,
                generation: stat.generation,
            })
        })?;
        unsafe { output(out_stat, result) }
    })
}
/// # Safety
/// `name_utf8` has exactly `name_len` readable bytes; `out_id` is writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_lookup(
    session: u64, parent: u64, name_utf8: *const u8, name_len: usize, out_id: *mut u64,
) -> i32 {
    respond(|| {
        if out_id.is_null() { return Err(PW_INVALID_ARGUMENT); }
        let name = unsafe { name(name_utf8, name_len)? };
        let id = enter(session, |s| s.volume.lookup(object(parent)?, name))?;
        unsafe { output(out_id, id.raw()) }
    })
}
/// # Safety
/// `name_utf8` has exactly `name_len` readable bytes; `out_id` is writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_create_file(
    session: u64, parent: u64, name_utf8: *const u8, name_len: usize, out_id: *mut u64,
) -> i32 {
    respond(|| {
        if out_id.is_null() { return Err(PW_INVALID_ARGUMENT); }
        let name = unsafe { name(name_utf8, name_len)? };
        let id = enter(session, |s| s.volume.create_file(object(parent)?, name))?;
        unsafe { output(out_id, id.raw()) }
    })
}
/// # Safety
/// `name_utf8` has exactly `name_len` readable bytes; `out_id` is writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_mkdir(
    session: u64, parent: u64, name_utf8: *const u8, name_len: usize, out_id: *mut u64,
) -> i32 {
    respond(|| {
        if out_id.is_null() { return Err(PW_INVALID_ARGUMENT); }
        let name = unsafe { name(name_utf8, name_len)? };
        let id = enter(session, |s| s.volume.mkdir(object(parent)?, name))?;
        unsafe { output(out_id, id.raw()) }
    })
}
/// # Safety
/// `out_handle` points to one writable u64.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_open_file(
    session: u64, object_id: u64, out_handle: *mut u64,
) -> i32 {
    respond(|| {
        if out_handle.is_null() { return Err(PW_INVALID_ARGUMENT); }
        let id = enter(session, |s| {
            let next = s.next_handle.checked_add(1).ok_or(Error::ResourceLimit)?;
            let raw = s.volume.open_file(object(object_id)?)?;
            s.next_handle = next;
            s.handles.insert(next, raw);
            Ok(next)
        })?;
        unsafe { output(out_handle, id) }
    })
}
/// # Safety
/// `session` and `handle` are process-local numeric IDs. Unknown IDs return
/// PW_BAD_HANDLE without dereferencing a pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_close_file(session: u64, handle: u64) -> i32 {
    respond(|| {
        enter(session, |s| {
            let raw = s.handles.remove(&handle).ok_or(Error::InvalidHandle)?;
            s.volume.close_file(raw)
        })
    })
}
/// # Safety
/// `out_bytes` is writable; if `capacity` is nonzero, `buffer` must point
/// to `capacity` writable bytes. Buffers must not alias another active pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_read(
    session: u64, handle: u64, offset: u64,
    buffer: *mut u8, capacity: usize, out_bytes: *mut usize,
) -> i32 {
    respond(|| {
        if out_bytes.is_null() || (capacity > 0 && buffer.is_null()) || capacity > MAX_IO {
            return Err(PW_INVALID_ARGUMENT);
        }
        let data = enter(session, |s| {
            let raw = *s.handles.get(&handle).ok_or(Error::InvalidHandle)?;
            s.volume.read(raw, offset, capacity)
        })?;
        if !data.is_empty() {
            // SAFETY: caller provides the region; output may be shorter than capacity.
            unsafe { std::ptr::copy_nonoverlapping(data.as_ptr(), buffer, data.len()) };
        }
        unsafe { output(out_bytes, data.len()) }
    })
}
/// # Safety
/// `out_bytes` is writable; if `len` is nonzero, `buffer` must point to
/// `len` readable bytes, valid throughout the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_write(
    session: u64, handle: u64, offset: u64,
    buffer: *const u8, len: usize, out_bytes: *mut usize,
) -> i32 {
    respond(|| {
        if out_bytes.is_null() || (len > 0 && buffer.is_null()) || len > MAX_IO {
            return Err(PW_INVALID_ARGUMENT);
        }
        let input: &[u8] = if len == 0 { &[] } else {
            // SAFETY: caller provides the region and we do not retain it.
            unsafe { std::slice::from_raw_parts(buffer, len) }
        };
        let written = enter(session, |s| {
            let raw = *s.handles.get(&handle).ok_or(Error::InvalidHandle)?;
            s.volume.write(raw, offset, input)
        })?;
        unsafe { output(out_bytes, written) }
    })
}
/// # Safety
/// Uses numeric IDs only; the caller must not use the handle after close.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_truncate(session: u64, handle: u64, size: u64) -> i32 {
    respond(|| {
        enter(session, |s| {
            let raw = *s.handles.get(&handle).ok_or(Error::InvalidHandle)?;
            s.volume.truncate(raw, size)
        })
    })
}
/// # Safety
/// `name_utf8` is readable for `name_len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_unlink(
    session: u64, parent: u64, name_utf8: *const u8, name_len: usize,
) -> i32 {
    respond(|| {
        let name = unsafe { name(name_utf8, name_len)? };
        enter(session, |s| s.volume.unlink(object(parent)?, name))
    })
}
/// # Safety
/// `name_utf8` is readable for `name_len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_rmdir(
    session: u64, parent: u64, name_utf8: *const u8, name_len: usize,
) -> i32 {
    respond(|| {
        let name = unsafe { name(name_utf8, name_len)? };
        enter(session, |s| s.volume.rmdir(object(parent)?, name))
    })
}
/// # Safety
/// Both name buffers must be readable for their specified byte lengths.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_rename(
    session: u64, from_parent: u64, from_utf8: *const u8, from_len: usize,
    to_parent: u64, to_utf8: *const u8, to_len: usize, replace_existing: u8,
) -> i32 {
    respond(|| {
        if replace_existing > 1 { return Err(PW_INVALID_ARGUMENT); }
        let from = unsafe { name(from_utf8, from_len)? };
        let to = unsafe { name(to_utf8, to_len)? };
        enter(session, |s| {
            s.volume.rename(object(from_parent)?, from, object(to_parent)?, to, replace_existing == 1)
        })
    })
}
/// # Safety
/// `name_utf8` is readable for `name_len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_hard_link(
    session: u64, target_object: u64, parent: u64, name_utf8: *const u8, name_len: usize,
) -> i32 {
    respond(|| {
        let name = unsafe { name(name_utf8, name_len)? };
        enter(session, |s| {
            s.volume.hard_link(object(target_object)?, object(parent)?, name)
        })
    })
}
