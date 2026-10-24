"""One-shot, exact-base source edit; remove with its workflow before merging."""
from pathlib import Path
import subprocess

expected = {
    "crates/core/src/volume.rs": "1ba9fb879f286b32e98fb15b8accc3e894c99702",
    "crates/core/src/segment.rs": "a4d9673c900bd5d77b63fbb08f44b3314935a986",
    "crates/core/src/lib.rs": "ccc680c37e81ea9e66373f9d210d097a66fd6264",
    "crates/core/src/error.rs": "e88e28bcbae323343322ce48ab4124506a7ddcc3",
}
for path, sha in expected.items():
    actual = subprocess.check_output(["git", "hash-object", path], text=True).strip()
    if actual != sha:
        raise SystemExit(f"Refusing changed base: {path}: {actual}")

def replace(path, before, after):
    p = Path(path)
    text = p.read_text(encoding="utf-8")
    if text.count(before) != 1:
        raise SystemExit(f"Expected exactly one patch target in {path}: {before[:80]!r}")
    p.write_text(text.replace(before, after), encoding="utf-8")

replace("Cargo.toml", 'sha2 = "=0.10.9"', 'sha2 = "=0.10.9"\nfs2 = "=0.4.3"')
replace("crates/core/Cargo.toml", 'sha2.workspace = true', 'sha2.workspace = true\nfs2.workspace = true')
replace("crates/core/src/error.rs", '    Quota,', '''    Quota,
    /// Refused BEFORE staging data; does not imply failed I/O or corrupt state.
    HostSpace { available_bytes: u64, required_bytes: u64 },''')
replace("crates/core/src/lib.rs", 'mod segment;\nmod volume;', '''mod segment;
mod segment_io;
mod space;
#[cfg(test)]
mod space_catalog_tests;
mod volume;''')
replace("crates/core/src/lib.rs", 'pub use error::{Error, Result};', '''pub use error::{Error, Result};
pub use space::{SpacePolicy, SpaceStatus};
#[cfg(feature = "fault-injection")]
pub use segment_io::StorageFault;''')
replace("crates/core/src/segment.rs", 'use crate::{BLOCK_SIZE, Error, Result};', 'use crate::{BLOCK_SIZE, Error, Result, segment_io::{IoControl, SegmentFile}};')
replace("crates/core/src/segment.rs", 'pub(crate) fn write(root: &Path, id: i64, blocks: &[(i64, Vec<u8>)]) -> Result<Vec<BlockRef>> {', '''pub(crate) fn encoded_len(block_count: usize) -> Result<u64> {
    if block_count == 0 { return Err(Error::InvalidRange); }
    (block_count as u64).checked_mul(BLOCK_SIZE as u64)
        .and_then(|n| n.checked_add(HEADER))
        .filter(|n| *n <= i64::MAX as u64)
        .ok_or(Error::InvalidRange)
}

pub(crate) fn write(root: &Path, id: i64, blocks: &[(i64, Vec<u8>)], control: &mut IoControl) -> Result<Vec<BlockRef>> {''')
replace("crates/core/src/segment.rs", '    let mut file = File::create_new(path(root, id))?;', '    let mut file = SegmentFile::new(File::create_new(path(root, id))?, control);')
replace("crates/core/src/segment.rs", '    sync_directory(&root.join("segments"))?;\n    Ok(refs)', '    file.sync_directory(&root.join("segments"))?;\n    Ok(refs)')
v = "crates/core/src/volume.rs"
replace(v, '    BLOCK_SIZE, Error, FORMAT, MAX_IO, Result,', '    BLOCK_SIZE, Error, FORMAT, MAX_IO, Result, SpacePolicy, SpaceStatus,\n    space::SpaceGate, segment_io::IoControl,')
replace(v, '    Healthy,\n    Failed { error: String },', '''    Healthy,
    /// No data I/O attempted for compaction; retry on a later operation.
    DeferredSpace { available_bytes: u64, required_bytes: u64 },
    Failed { error: String },''')
replace(v, '    pub maintenance: MaintenanceStatus,', '    pub maintenance: MaintenanceStatus,\n    pub space: SpaceStatus,')
replace(v, '    maintenance: MaintenanceStatus,\n    #[cfg(feature = "fault-injection")]', '    maintenance: MaintenanceStatus,\n    space: SpaceGate,\n    io_control: IoControl,\n    #[cfg(feature = "fault-injection")]')
replace(v, '            maintenance: MaintenanceStatus::Healthy,', '            maintenance: MaintenanceStatus::Healthy,\n            space: SpaceGate::default(),\n            io_control: IoControl::default(),')
replace(v, '    pub fn is_write_protected(&self) -> bool {', '''    /// A soft per-owner policy, not persistent allocation or an OS reservation.
    pub fn set_space_policy(&mut self, policy: SpacePolicy) -> Result<()> {
        policy.headroom()?;
        if self.protected.get() { return Err(Error::WriteProtected); }
        self.space.policy = policy;
        Ok(())
    }
    pub fn space_status(&self) -> Result<SpaceStatus> {
        self.observe(self.space.status(&self.root))
    }
    /// Test-only ceiling; never invents more capacity than the real host has.
    #[cfg(feature = "fault-injection")]
    pub fn set_test_available_space(&mut self, ceiling: Option<u64>) {
        self.space.available_ceiling = ceiling;
    }
    #[cfg(feature = "fault-injection")]
    pub fn inject_storage_fault_once(&mut self, fault: crate::StorageFault) -> Result<()> {
        if self.protected.get() { return Err(Error::WriteProtected); }
        if matches!(fault, crate::StorageFault::ShortWrites { max_bytes: 0 }) {
            return Err(Error::InvalidRange);
        }
        self.io_control.next_fault = Some(fault);
        Ok(())
    }
    pub fn is_write_protected(&self) -> bool {''')
replace(v, '''        if let Err(error) = self.hit(2).and_then(|_| self.maintain()) {
            self.protected.set(true);
            self.maintenance = MaintenanceStatus::Failed {
                error: error.to_string(),
            };
        }''', '''        match self.hit(2).and_then(|_| self.maintain()) {
            Ok(()) => self.maintenance = MaintenanceStatus::Healthy,
            Err(Error::HostSpace { available_bytes, required_bytes }) => {
                self.maintenance = MaintenanceStatus::DeferredSpace { available_bytes, required_bytes };
            }
            Err(error) => {
                self.protected.set(true);
                self.maintenance = MaintenanceStatus::Failed { error: error.to_string() };
            }
        }''')
replace(v, '''    fn stage(&mut self, blocks: &[(i64, Vec<u8>)]) -> Result<Vec<BlockRef>> {
        // Reserve''', '''    fn stage(&mut self, blocks: &[(i64, Vec<u8>)]) -> Result<Vec<BlockRef>> {
        let bytes = segment::encoded_len(blocks.len())?;
        // Checked before even allocating a catalog identity. This is only a
        // conservative admission snapshot; real I/O errors must still propagate.
        self.space.admit(&self.root, bytes)?;
        // Reserve''')
replace(v, '''        let refs = segment::write(&self.root, id, blocks)?;
        let bytes = segment::HEADER + blocks.len() as u64 * BLOCK_SIZE as u64;''', '''        let refs = segment::write(&self.root, id, blocks, &mut self.io_control)?;''')
replace(v, '                maintenance: self.maintenance.clone(),', '                maintenance: self.maintenance.clone(),\n                space: self.space_status()?,')
replace("crates/core/tests/space_io.rs", '    let h = v.open_file(v.create_file(ROOT, "sparse").unwrap()).unwrap();', '    let id = v.create_file(ROOT, "sparse").unwrap();\n    let h = v.open_file(id).unwrap();')
replace(".github/workflows/rust-core.yml", '--test storage --test recovery -- --test-threads=8', '--test storage --test recovery --test space_io -- --test-threads=8')
print("Applied exact-base M1-C edits; new modules/tests remain explicitly experimental.")
