//! Experimental persistent storage core. NOT a mounted filesystem.
//!
//! Serial request processing, immutable block records, SQLite authority, real
//! namespace identity and automatic reclamation. No ACL, executable mapping,
//! watcher, OS adapter, hardware power-loss guarantee or stable format yet.
#![forbid(unsafe_code)]

mod catalog;
mod error;
mod names;
mod namespace;
mod owner;
mod recovery;
mod segment;
mod segment_io;
mod space;
#[cfg(test)]
mod space_catalog_tests;
mod volume;

pub use error::{Error, Result};
pub use recovery::{ExportLinks, ExportReport, Recovery, RecoveryLimits, RecoveryReport};
#[cfg(feature = "fault-injection")]
pub use segment_io::StorageFault;
pub use space::{SpacePolicy, SpaceStatus};
#[cfg(feature = "fault-injection")]
pub use volume::FaultPoint;
pub use volume::{
    CapacitySnapshot, CheckpointStatus, Entry, FileId, Handle, Kind, MaintenanceStatus, ROOT,
    ReadView, ReadViewLimits, Stat, Stats, Volume,
};

pub const BLOCK_SIZE: usize = 64 * 1024;
pub const MAX_IO: usize = 16 * 1024 * 1024;
pub const FORMAT: &str = "experimental-m1a-block-v1";
