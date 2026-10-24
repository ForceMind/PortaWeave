//! Data-segment I/O seam. Faults are per owner, one staged segment at a time,
//! and compile out of the default build. SQLite and OS faults are NOT simulated
//! by these hooks; tests label those boundaries separately.
use crate::Result;
use std::{fs::File, io::{self, Write}, path::Path};

#[cfg(feature = "fault-injection")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StorageFault {
    /// Each write makes at most this much progress; write_all must retry.
    ShortWrites { max_bytes: usize },
    InterruptedOnce,
    /// Permit this prefix (including header), then return zero progress.
    WriteZeroAfter { bytes: u64 },
    /// Permit this prefix, then report a disk-full I/O error.
    NoSpaceAfter { bytes: u64 },
    IoErrorAfter { bytes: u64 },
    DataSyncFailure,
    DirectorySyncFailure,
}

#[derive(Default)]
pub(crate) struct IoControl {
    #[cfg(feature = "fault-injection")]
    pub next_fault: Option<StorageFault>,
}
pub(crate) struct SegmentFile {
    file: File,
    #[cfg(feature = "fault-injection")]
    fault: Option<StorageFault>,
    #[cfg(feature = "fault-injection")]
    written: u64,
    #[cfg(feature = "fault-injection")]
    interrupted: bool,
}
impl SegmentFile {
    pub fn new(file: File, control: &mut IoControl) -> Self {
        #[cfg(not(feature = "fault-injection"))]
        let _ = control;
        Self {
            file,
            #[cfg(feature = "fault-injection")]
            fault: control.next_fault.take(),
            #[cfg(feature = "fault-injection")]
            written: 0,
            #[cfg(feature = "fault-injection")]
            interrupted: false,
        }
    }
    pub fn sync_all(&self) -> io::Result<()> {
        #[cfg(feature = "fault-injection")]
        if self.fault == Some(StorageFault::DataSyncFailure) {
            return Err(io::Error::other("injected segment data sync failure"));
        }
        self.file.sync_all()
    }
    pub fn sync_directory(&self, path: &Path) -> Result<()> {
        #[cfg(feature = "fault-injection")]
        if self.fault == Some(StorageFault::DirectorySyncFailure) {
            return Err(io::Error::other("injected segment directory sync failure").into());
        }
        crate::segment::sync_directory(path)
    }
    #[cfg(feature = "fault-injection")]
    fn allowed(&mut self, requested: usize) -> io::Result<usize> {
        match self.fault {
            Some(StorageFault::ShortWrites { max_bytes }) => Ok(requested.min(max_bytes)),
            Some(StorageFault::InterruptedOnce) if !self.interrupted => {
                self.interrupted = true;
                Err(io::Error::new(io::ErrorKind::Interrupted, "injected interruption"))
            }
            Some(fault @ (StorageFault::WriteZeroAfter { bytes }
                | StorageFault::NoSpaceAfter { bytes }
                | StorageFault::IoErrorAfter { bytes })) => {
                let remaining = bytes.saturating_sub(self.written);
                if remaining != 0 {
                    return Ok((requested as u64).min(remaining) as usize);
                }
                match fault {
                    StorageFault::WriteZeroAfter { .. } => Ok(0),
                    StorageFault::NoSpaceAfter { .. } => Err(io::Error::new(io::ErrorKind::StorageFull, "injected segment disk full")),
                    _ => Err(io::Error::other("injected segment I/O failure")),
                }
            }
            _ => Ok(requested),
        }
    }
}
impl Write for SegmentFile {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        #[cfg(feature = "fault-injection")]
        let bytes = {
            let allowed = self.allowed(bytes.len())?;
            &bytes[..allowed]
        };
        let count = self.file.write(bytes)?;
        #[cfg(feature = "fault-injection")]
        { self.written += count as u64; }
        Ok(count)
    }
    fn flush(&mut self) -> io::Result<()> { self.file.flush() }
}
