use std::{fmt, io};

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    Sql(rusqlite::Error),
    NotFound,
    Exists,
    NotDirectory,
    IsDirectory,
    NotEmpty,
    InvalidName,
    InvalidHandle,
    /// A checked namespace request no longer names the expected inode/target.
    StaleEntry,
    InvalidRange,
    /// Policy refusal, distinct from storage write protection or an I/O error.
    PermissionDenied,
    Quota,
    /// Refused BEFORE staging data; does not imply failed I/O or corrupt state.
    HostSpace {
        available_bytes: u64,
        required_bytes: u64,
    },
    RecoveryLimit,
    ResourceLimit,
    Busy,
    /// Nonblocking cooperative range admission conflict, not storage failure.
    LockConflict,
    Cycle,
    UnsupportedFormat,
    UnsupportedOperation,
    Corrupt(String),
    WriteProtected,
    Injected,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "I/O: {e}"),
            Self::Sql(e) => write!(f, "SQLite: {e}"),
            Self::Corrupt(e) => write!(f, "corrupt data: {e}"),
            other => write!(f, "{other:?}"),
        }
    }
}
impl std::error::Error for Error {}
impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}
impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Self::Sql(e)
    }
}
pub type Result<T> = std::result::Result<T, Error>;
