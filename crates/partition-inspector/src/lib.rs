//! Read-only, bounded GPT/MBR inspection for REGULAR-FILE images ONLY.
//! No writes, driver/device IO, partition repair or filesystem mount.
use std::{
    collections::BTreeSet,
    fmt,
    fs::{self, File},
    io::{Read, Seek, SeekFrom},
    path::Path,
};
const MAX_TABLE_BYTES: u64 = 4 * 1024 * 1024;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scheme {
    Mbr,
    Gpt,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Integrity {
    Complete,
    Degraded,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Partition {
    pub index: usize,
    pub first_lba: u64,
    pub last_lba: u64,
    pub type_id: String,
    pub unique_id: Option<String>,
    pub name: Option<String>,
    pub bootable: bool,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inspection {
    pub scheme: Scheme,
    pub integrity: Integrity,
    pub sector_size: u64,
    pub disk_bytes: u64,
    pub partitions: Vec<Partition>,
    pub warnings: Vec<String>,
}
#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Invalid(String),
    Unsupported(String),
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "I/O: {e}"),
            Self::Invalid(s) => write!(f, "invalid image: {s}"),
            Self::Unsupported(s) => write!(f, "unsupported: {s}"),
        }
    }
}
impl std::error::Error for Error {}
impl From<std::io::Error> for Error {
    fn from(v: std::io::Error) -> Self {
        Self::Io(v)
    }
}
pub type Result<T> = std::result::Result<T, Error>;
fn invalid(s: impl Into<String>) -> Error {
    Error::Invalid(s.into())
}
fn get16(b: &[u8], n: usize) -> u16 {
    u16::from_le_bytes([b[n], b[n + 1]])
}
fn get32(b: &[u8], n: usize) -> u32 {
    u32::from_le_bytes(b[n..n + 4].try_into().expect("validated byte extent"))
}
fn get64(b: &[u8], n: usize) -> u64 {
    u64::from_le_bytes(b[n..n + 8].try_into().expect("validated byte extent"))
}
fn add(a: u64, b: u64) -> Result<u64> {
    a.checked_add(b)
        .ok_or_else(|| invalid("LBA addition overflow"))
}
fn mul(a: u64, b: u64) -> Result<u64> {
    a.checked_mul(b)
        .ok_or_else(|| invalid("byte offset overflow"))
}
fn block(file: &mut File, offset: u64, len: usize) -> Result<Vec<u8>> {
    file.seek(SeekFrom::Start(offset))?;
    let mut out = vec![0; len];
    file.read_exact(&mut out)?;
    Ok(out)
}
fn crc32(buf: &[u8]) -> u32 {
    let mut crc = !0_u32;
    for b in buf {
        crc ^= u32::from(*b);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                crc >> 1 ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}
fn guid(b: &[u8]) -> String {
    format!(
        "{:08x}-{:04x}-{:04x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        get32(b, 0),
        get16(b, 4),
        get16(b, 6),
        b[8],
        b[9],
        b[10],
        b[11],
        b[12],
        b[13],
        b[14],
        b[15]
    )
}
fn no_overlap(parts: &mut [Partition]) -> Result<()> {
    parts.sort_by_key(|p| p.first_lba);
    for pair in parts.windows(2) {
        if pair[0].last_lba >= pair[1].first_lba {
            return Err(invalid(format!(
                "overlapping partitions {} and {}",
                pair[0].index, pair[1].index
            )));
        }
    }
    Ok(())
}
#[derive(Debug)]
struct Gpt {
    first: u64,
    last: u64,
    disk_id: [u8; 16],
    count: u32,
    entry_size: u32,
    table_crc: u32,
    entries: Vec<Partition>,
}
fn gpt_copy(file: &mut File, sectors: u64, sector_size: u64, primary: bool) -> Result<Gpt> {
    if sectors < 8 {
        return Err(invalid("disk too small for GPT"));
    }
    let final_lba = sectors - 1;
    let here = if primary { 1 } else { final_lba };
    let opposite = if primary { final_lba } else { 1 };
    let header = block(file, mul(here, sector_size)?, sector_size as usize)?;
    if &header[..8] != b"EFI PART" {
        return Err(invalid("missing GPT header signature"));
    }
    if get32(&header, 8) != 0x0001_0000 {
        return Err(Error::Unsupported("unsupported GPT header revision".into()));
    }
    let header_len = get32(&header, 12) as usize;
    if !(92..=sector_size as usize).contains(&header_len) || get32(&header, 20) != 0 {
        return Err(invalid("invalid GPT header size/reserved field"));
    }
    let mut check = header[..header_len].to_vec();
    check[16..20].fill(0);
    if crc32(&check) != get32(&header, 16) {
        return Err(invalid("GPT header CRC mismatch"));
    }
    if get64(&header, 24) != here || get64(&header, 32) != opposite {
        return Err(invalid("GPT primary/backup LBA mismatch"));
    }
    let first = get64(&header, 40);
    let last = get64(&header, 48);
    if first < 2 || last >= final_lba || first > last {
        return Err(invalid("invalid GPT usable LBA range"));
    }
    let count = get32(&header, 80);
    let entry_size = get32(&header, 84);
    if count == 0
        || entry_size < 128
        || !entry_size.is_multiple_of(128)
        || !(entry_size / 128).is_power_of_two()
    {
        return Err(invalid("invalid GPT entry geometry"));
    }
    let bytes = mul(u64::from(count), u64::from(entry_size))?;
    if bytes > MAX_TABLE_BYTES {
        return Err(Error::Unsupported(
            "GPT entry array exceeds 4 MiB read limit".into(),
        ));
    }
    if bytes < 16 * 1024 {
        return Err(invalid("GPT table below UEFI minimum 16 KiB"));
    }
    let table_lba = get64(&header, 72);
    let end = add(table_lba, bytes.div_ceil(sector_size))?;
    if (primary && (table_lba < 2 || end > first))
        || (!primary && (table_lba <= last || end > final_lba))
    {
        return Err(invalid("GPT table overlaps usable space/header"));
    }
    let array = block(file, mul(table_lba, sector_size)?, bytes as usize)?;
    let table_crc = get32(&header, 88);
    if crc32(&array) != table_crc {
        return Err(invalid("GPT partition array CRC mismatch"));
    }
    let mut disk_id = [0; 16];
    disk_id.copy_from_slice(&header[56..72]);
    if disk_id == [0; 16] {
        return Err(invalid("zero GPT disk GUID"));
    }
    let mut entries = Vec::new();
    let mut seen = BTreeSet::new();
    for i in 0..count as usize {
        let start = i * entry_size as usize;
        let item = &array[start..start + entry_size as usize];
        if item[0..16].iter().all(|b| *b == 0) {
            if item.iter().any(|b| *b != 0) {
                return Err(invalid("unused GPT entry has nonzero fields"));
            }
            continue;
        }
        let mut uuid = [0; 16];
        uuid.copy_from_slice(&item[16..32]);
        if uuid == [0; 16] || !seen.insert(uuid) {
            return Err(invalid("zero/duplicate GPT partition GUID"));
        }
        let a = get64(item, 32);
        let b = get64(item, 40);
        if b < a || a < first || b > last {
            return Err(invalid("GPT partition outside usable LBA range"));
        }
        let units: Vec<u16> = item[56..128]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|x| u16::from_le_bytes([x[0], x[1]]))
            .take_while(|u| *u != 0)
            .collect();
        let name = String::from_utf16(&units).map_err(|_| invalid("invalid GPT UTF16 name"))?;
        entries.push(Partition {
            index: i + 1,
            first_lba: a,
            last_lba: b,
            type_id: guid(&item[..16]),
            unique_id: Some(guid(&uuid)),
            name: if name.is_empty() { None } else { Some(name) },
            bootable: false,
        });
    }
    no_overlap(&mut entries)?;
    Ok(Gpt {
        first,
        last,
        disk_id,
        count,
        entry_size,
        table_crc,
        entries,
    })
}
fn inspect_gpt(file: &mut File, size: u64, sector: u64) -> Result<Inspection> {
    let a = gpt_copy(file, size / sector, sector, true);
    let b = gpt_copy(file, size / sector, sector, false);
    let (integrity, partitions, warnings) = match (a, b) {
        (Ok(a), Ok(b)) => {
            if a.disk_id != b.disk_id
                || a.first != b.first
                || a.last != b.last
                || a.count != b.count
                || a.entry_size != b.entry_size
                || a.table_crc != b.table_crc
                || a.entries != b.entries
            {
                return Err(invalid("primary and backup GPT disagree; no repair"));
            }
            (Integrity::Complete, a.entries, vec![])
        }
        (Ok(a), Err(e)) => (
            Integrity::Degraded,
            a.entries,
            vec![format!("backup GPT invalid ({e}); source unchanged")],
        ),
        (Err(e), Ok(b)) => (
            Integrity::Degraded,
            b.entries,
            vec![format!(
                "primary GPT invalid ({e}); using backup for read-only inspection"
            )],
        ),
        (Err(a), Err(b)) => {
            return Err(invalid(format!(
                "both GPT copies invalid: primary={a}; backup={b}"
            )));
        }
    };
    Ok(Inspection {
        scheme: Scheme::Gpt,
        integrity,
        sector_size: sector,
        disk_bytes: size,
        partitions,
        warnings,
    })
}
fn inspect_mbr(file: &mut File, mbr: &[u8], size: u64, sector: u64) -> Result<Inspection> {
    let mut parts = Vec::new();
    let mut warnings = Vec::new();
    let mut protective = 0;
    for i in 0..4 {
        let a = 446 + i * 16;
        let x = &mbr[a..a + 16];
        let ty = x[4];
        if x[0] != 0 && x[0] != 0x80 {
            return Err(invalid("invalid MBR boot flag"));
        }
        let first = u64::from(get32(x, 8));
        let count = u64::from(get32(x, 12));
        if ty == 0 {
            if first != 0 || count != 0 {
                return Err(invalid("unused MBR entry contains partition bounds"));
            }
            continue;
        }
        if first == 0 || count == 0 || add(first, count)? > size / sector {
            return Err(invalid("MBR partition outside disk or zero length"));
        }
        if ty == 0xee {
            protective += 1;
        }
        if matches!(ty, 0x05 | 0x0f | 0x85) {
            warnings.push("extended MBR region detected: EBR chain not inspected".into());
        }
        parts.push(Partition {
            index: i + 1,
            first_lba: first,
            last_lba: first + count - 1,
            type_id: format!("0x{ty:02x}"),
            unique_id: None,
            name: None,
            bootable: x[0] == 0x80,
        });
    }
    if protective > 0 {
        if protective != 1 || parts.len() != 1 || parts[0].first_lba != 1 {
            return Err(Error::Unsupported(
                "hybrid MBR/GPT layouts not supported".into(),
            ));
        }
        return inspect_gpt(file, size, sector);
    }
    no_overlap(&mut parts)?;
    Ok(Inspection {
        scheme: Scheme::Mbr,
        integrity: Integrity::Complete,
        disk_bytes: size,
        sector_size: sector,
        partitions: parts,
        warnings,
    })
}
/// Inspect an existing regular-file RAW/IMG disk image. Never writes bytes.
///
/// Rejects block devices, directories, symlinks and non-sector-aligned files.
/// Sector size must be specified exactly (512 or 4096), not guessed.
pub fn inspect_image(path: impl AsRef<Path>, sector_size: u64) -> Result<Inspection> {
    if !matches!(sector_size, 512 | 4096) {
        return Err(Error::Unsupported("sector size must be 512 or 4096".into()));
    }
    let path = path.as_ref();
    let info = fs::symlink_metadata(path)?;
    if !info.file_type().is_file() {
        return Err(Error::Unsupported(
            "only non-symlink regular image files supported".into(),
        ));
    }
    let size = info.len();
    if size < sector_size || size % sector_size != 0 {
        return Err(invalid("unusable image length / not sector aligned"));
    }
    let mut file = File::open(path)?;
    if !file.metadata()?.is_file() || file.metadata()?.len() != size {
        return Err(invalid("image changed while opening"));
    }
    let mbr = block(&mut file, 0, sector_size as usize)?;
    if mbr[510] != 0x55 || mbr[511] != 0xaa {
        return Err(invalid("missing MBR signature 55AA"));
    }
    inspect_mbr(&mut file, &mbr, size, sector_size)
}
