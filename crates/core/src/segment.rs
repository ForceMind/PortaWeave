use crate::{BLOCK_SIZE, Error, Result};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

pub(crate) const HEADER: u64 = 24;
const MAGIC: &[u8; 8] = b"PWSEG001";

#[derive(Clone, Debug)]
pub(crate) struct BlockRef {
    pub index: i64,
    pub segment: i64,
    pub offset: i64,
    pub checksum: Vec<u8>,
}

pub(crate) fn path(root: &Path, id: i64) -> PathBuf {
    root.join("segments").join(format!("{id:016x}.seg"))
}

pub(crate) fn regular(path: &Path) -> Result<()> {
    if !fs::symlink_metadata(path)?.file_type().is_file() {
        return Err(Error::Corrupt("expected a regular backing file".into()));
    }
    Ok(())
}

// Windows does not expose a portable directory-fsync through std. This is an
// explicit durability gap: the prototype is NOT an implementation of OS fsync.
// File sync calls below still propagate errors; hardware persistence is unproven.
pub(crate) fn sync_directory(path: &Path) -> Result<()> {
    #[cfg(unix)]
    File::open(path)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

pub(crate) fn write(root: &Path, id: i64, blocks: &[(i64, Vec<u8>)]) -> Result<Vec<BlockRef>> {
    if id <= 0 || blocks.is_empty() || blocks.iter().any(|(_, b)| b.len() != BLOCK_SIZE) {
        return Err(Error::InvalidRange);
    }
    let mut file = File::create_new(path(root, id))?;
    file.write_all(MAGIC)?;
    file.write_all(&id.to_le_bytes())?;
    file.write_all(&(BLOCK_SIZE as u32).to_le_bytes())?;
    file.write_all(&1_u32.to_le_bytes())?;
    let mut refs = Vec::with_capacity(blocks.len());
    for (position, (index, data)) in blocks.iter().enumerate() {
        let offset = HEADER + position as u64 * BLOCK_SIZE as u64;
        file.write_all(data)?;
        refs.push(BlockRef {
            index: *index,
            segment: id,
            offset: i64::try_from(offset).map_err(|_| Error::InvalidRange)?,
            checksum: Sha256::digest(data).to_vec(),
        });
    }
    file.sync_all()?;
    sync_directory(&root.join("segments"))?;
    Ok(refs)
}

pub(crate) fn read(root: &Path, reference: &BlockRef) -> Result<Vec<u8>> {
    let path = path(root, reference.segment);
    regular(&path)?;
    let mut file = File::open(path)?;
    let size = file.metadata()?.len();
    let offset = u64::try_from(reference.offset)
        .map_err(|_| Error::Corrupt("negative block offset".into()))?;
    if reference.segment <= 0
        || offset < HEADER
        || !(offset - HEADER).is_multiple_of(BLOCK_SIZE as u64)
        || offset
            .checked_add(BLOCK_SIZE as u64)
            .is_none_or(|end| end > size)
        || reference.checksum.len() != 32
    {
        return Err(Error::Corrupt("invalid block bounds".into()));
    }
    let mut header = [0_u8; HEADER as usize];
    file.read_exact(&mut header)?;
    if &header[..8] != MAGIC
        || header[8..16] != reference.segment.to_le_bytes()
        || header[16..20] != (BLOCK_SIZE as u32).to_le_bytes()
        || header[20..24] != 1_u32.to_le_bytes()
    {
        return Err(Error::Corrupt("segment header mismatch".into()));
    }
    file.seek(SeekFrom::Start(offset))?;
    let mut data = vec![0; BLOCK_SIZE];
    file.read_exact(&mut data)?;
    if Sha256::digest(&data).as_slice() != reference.checksum.as_slice() {
        return Err(Error::Corrupt("block checksum mismatch".into()));
    }
    Ok(data)
}
