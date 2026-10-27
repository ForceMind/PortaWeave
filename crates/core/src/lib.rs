//! Experimental persistent storage core. NOT a mounted filesystem.
//!
//! Serial request processing, immutable block records, SQLite authority, real
//! namespace identity and automatic reclamation. No ACL, executable mapping,
//! watcher, OS adapter, hardware power-loss guarantee or stable format yet.
#![forbid(unsafe_code)]

mod catalog;
mod error;
mod metadata;
mod names;
mod namespace;
mod owner;
mod permissions;
mod recovery;
mod segment;
mod segment_io;
mod space;
#[cfg(test)]
mod space_catalog_tests;
mod volume;

pub use error::{Error, Result};
pub use metadata::{CreateAttributes, Metadata, MetadataUpdate, Timestamp, VolumeIdentity};
pub use namespace::{CreatedDirectory, CreatedNode, NodeCreateOptions, RenameExpectation};
pub use permissions::{
    Access, Credentials, DirectoryEntryInfo, DirectorySnapshot, FileAccess, FileInfo,
    FileWriteResult, MAX_POSIX_FILES, MAX_SUPPLEMENTARY_GROUPS, OpenDirectorySnapshot,
    PosixDirectory, PosixFile, PosixReadView, PosixSession,
};
pub use recovery::{ExportLinks, ExportReport, Recovery, RecoveryLimits, RecoveryReport};
#[cfg(feature = "fault-injection")]
pub use segment_io::StorageFault;
pub use space::{SpacePolicy, SpaceStatus};
#[cfg(feature = "fault-injection")]
pub use volume::FaultPoint;
pub use volume::{
    CapacitySnapshot, CheckpointStatus, DirectoryHandle, Entry, FileId, Handle, Kind,
    MaintenanceStatus, ObjectInfo, ROOT, ReadView, ReadViewLimits, Stat, Stats, StorageSyncStatus,
    Volume, WritePosition,
};

pub const BLOCK_SIZE: usize = 64 * 1024;
pub const MAX_IO: usize = 16 * 1024 * 1024;
pub const FORMAT: &str = "experimental-m1a-block-v1";
/// Explicit opt-in experimental format. Existing v1 containers are never upgraded.
pub const METADATA_FORMAT: &str = "experimental-m1h-metadata-v2";
/// Explicit opt-in v3. Adds referenced, empty unlinked directories; no migration.
pub const DIRECTORY_FORMAT: &str = "experimental-m1o-directory-v3";
