//! Volatile cooperative range grants on live regular-file handles.
//! These do not intercept data I/O, implement OS locks, or authenticate callers.
use crate::{Error, FileId, Handle, Kind, Result, Volume, volume::token};
use std::ops::Range;

pub const MAX_RANGE_LOCKS: usize = 4096;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LockMode {
    Shared,
    Exclusive,
}
/// One acquisition. Copies identify the SAME grant; unlock/close invalidates all.
#[must_use = "release this grant or close its owning file handle"]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct RangeLock(u64);
pub(crate) struct LockState {
    pub(crate) handle: Handle,
    object: FileId,
    range: Range<u64>,
    mode: LockMode,
}
impl Volume {
    /// Nonblocking [offset, offset+length), including beyond EOF. Zero and
    /// overflow are invalid; no implicit EOF, merging, conversion or wait queue.
    /// Same-handle acquisitions coexist; each returned token releases one grant.
    pub fn lock_file_range(
        &mut self,
        handle: Handle,
        offset: u64,
        length: u64,
        mode: LockMode,
    ) -> Result<RangeLock> {
        let object = self.object(handle)?;
        if self.stat(object)?.kind != Kind::File {
            return Err(Error::IsDirectory);
        }
        let end = offset
            .checked_add(length)
            .filter(|end| *end > offset)
            .ok_or(Error::InvalidRange)?;
        if self.range_locks.len() >= MAX_RANGE_LOCKS {
            return Err(Error::ResourceLimit);
        }
        if self.range_locks.values().any(|old| {
            old.object == object
                && old.handle != handle
                && old.range.start < end
                && offset < old.range.end
                && (old.mode == LockMode::Exclusive || mode == LockMode::Exclusive)
        }) {
            return Err(Error::LockConflict);
        }
        let lock = RangeLock(token()?);
        self.range_locks.insert(
            lock,
            LockState {
                handle,
                object,
                range: offset..end,
                mode,
            },
        );
        Ok(lock)
    }
    /// Only its still-live originating handle can release a token. No I/O/GC;
    /// release works while storage is protected. A failed release changes none.
    pub fn unlock_file_range(&mut self, handle: Handle, lock: RangeLock) -> Result<()> {
        self.object(handle)?;
        let state = self.range_locks.get(&lock).ok_or(Error::InvalidHandle)?;
        if state.handle != handle {
            return Err(Error::InvalidHandle);
        }
        self.range_locks.remove(&lock);
        Ok(())
    }
    pub fn active_range_locks(&self) -> usize {
        self.range_locks.len()
    }
}
