use crate::{Error, Result};
use std::fs::{File, TryLockError};

/// Must be dropped AFTER the SQLite connection. Explicit unlock prevents a
/// duplicated/inherited Unix open-file description from retaining a dead
/// owner's lock during another thread's fork/exec window.
pub(crate) struct OwnerLock(File);

impl OwnerLock {
    pub(crate) fn acquire(file: File) -> Result<Self> {
        match file.try_lock() {
            Ok(()) => Ok(Self(file)),
            Err(TryLockError::WouldBlock) => Err(Error::Busy),
            Err(TryLockError::Error(error)) => Err(error.into()),
        }
    }
}

impl Drop for OwnerLock {
    fn drop(&mut self) {
        // Drop cannot return an error. Closing the file remains the fallback;
        // a future explicit unmount API must report release/flush failures.
        let _ = self.0.unlock();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn fixture(label: &str) -> (PathBuf, File) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("pw-owner-{label}-{}-{now}", std::process::id()));
        let file = File::create_new(&path).unwrap();
        (path, file)
    }

    #[test]
    fn a_live_owner_remains_exclusive() {
        let (path, file) = fixture("exclusive");
        let owner = OwnerLock::acquire(file).unwrap();
        let other = File::options().read(true).write(true).open(&path).unwrap();
        assert!(matches!(other.try_lock(), Err(TryLockError::WouldBlock)));
        drop(owner);
        other.try_lock().unwrap();
        other.unlock().unwrap();
        drop(other);
        fs::remove_file(path).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn drop_explicitly_unlocks_despite_a_duplicated_description() {
        let (path, file) = fixture("duplicate");
        let owner = OwnerLock::acquire(file).unwrap();
        let inherited = owner.0.try_clone().unwrap();
        drop(owner);
        let other = File::options().read(true).write(true).open(&path).unwrap();
        other
            .try_lock()
            .expect("a stale duplicate must not retain ownership");
        other.unlock().unwrap();
        drop(other);
        drop(inherited);
        fs::remove_file(path).unwrap();
    }
}
