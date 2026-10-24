//! Conservative host-space admission. This is NOT an OS reservation or an
//! emergency reserve file: other applications can consume space after sampling.
use crate::{Error, Result};
use std::path::Path;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SpacePolicy {
    /// Soft floor left for the host. No bytes are allocated by this setting.
    pub host_floor_bytes: u64,
    /// Conservative allowance, not an upper bound on SQLite/WAL growth.
    pub metadata_allowance_bytes: u64,
}
impl Default for SpacePolicy {
    fn default() -> Self {
        Self { host_floor_bytes: 16 * 1024 * 1024, metadata_allowance_bytes: 1024 * 1024 }
    }
}
impl SpacePolicy {
    pub(crate) fn headroom(self) -> Result<u64> {
        self.host_floor_bytes.checked_add(self.metadata_allowance_bytes).ok_or(Error::InvalidRange)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SpaceStatus {
    /// A host filesystem snapshot, not guaranteed capacity for this container.
    pub host_available_bytes: u64,
    pub soft_headroom_bytes: u64,
    /// Ignores reclaimable garbage; never counts it as already available.
    pub data_admission_bytes: u64,
}

#[derive(Default)]
pub(crate) struct SpaceGate {
    pub policy: SpacePolicy,
    #[cfg(feature = "fault-injection")]
    pub available_ceiling: Option<u64>,
}
impl SpaceGate {
    pub fn status(&self, root: &Path) -> Result<SpaceStatus> {
        let available = fs2::available_space(root)?;
        #[cfg(feature = "fault-injection")]
        let available = self.available_ceiling.map_or(available, |ceiling| available.min(ceiling));
        let headroom = self.policy.headroom()?;
        Ok(SpaceStatus {
            host_available_bytes: available,
            soft_headroom_bytes: headroom,
            data_admission_bytes: available.saturating_sub(headroom),
        })
    }
    pub fn admit(&self, root: &Path, segment_bytes: u64) -> Result<()> {
        let status = self.status(root)?;
        let required = segment_bytes.checked_add(status.soft_headroom_bytes).ok_or(Error::InvalidRange)?;
        if status.host_available_bytes < required {
            return Err(Error::HostSpace { available_bytes: status.host_available_bytes, required_bytes: required });
        }
        Ok(())
    }
}
