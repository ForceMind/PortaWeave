//! Experimental persistent storage core. NOT a mounted filesystem.
//!
//! Serial request processing, immutable block records, SQLite authority, real
//! namespace identity and automatic reclamation. No ACL, executable mapping,
//! watcher, OS adapter, hardware power-loss guarantee or stable format yet.
#![forbid(unsafe_code)]

mod error;
mod names;
mod namespace;
mod owner;
mod segment;
mod volume;

pub use error::{Error, Result};
#[cfg(feature = "fault-injection")]
pub use volume::FaultPoint;
pub use volume::{Entry, FileId, Handle, Kind, ROOT, ReadView, Stat, Stats, Volume};

pub const BLOCK_SIZE: usize = 64 * 1024;
pub const MAX_IO: usize = 16 * 1024 * 1024;
pub const FORMAT: &str = "experimental-m1a-block-v1";
