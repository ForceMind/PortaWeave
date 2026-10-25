use portaweave_partition_inspector::{Error, Integrity, Scheme, inspect_image};
use std::{
    fs::{self, File},
    io::Write,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Scratch {
    root: PathBuf,
    image: PathBuf,
}
impl Scratch {
    fn write(bytes: &[u8]) -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "pw-partitions-{}-{stamp}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let image = root.join("fixture.img");
        let mut handle = File::create(&image).unwrap();
        handle.write_all(bytes).unwrap();
        handle.sync_all().unwrap();
        Self { root, image }
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}
fn put32(b: &mut [u8], n: usize, value: u32) {
    b[n..n + 4].copy_from_slice(&value.to_le_bytes());
}
fn put64(b: &mut [u8], n: usize, value: u64) {
    b[n..n + 8].copy_from_slice(&value.to_le_bytes());
}
fn crc32(buf: &[u8]) -> u32 {
    let mut result = !0u32;
    for &b in buf {
        result ^= u32::from(b);
        for _ in 0..8 {
            result = if result & 1 != 0 {
                result >> 1 ^ 0xedb8_8320
            } else {
                result >> 1
            };
        }
    }
    !result
}
fn mbr_image() -> Vec<u8> {
    let mut image = vec![0; 512 * 256];
    image[510] = 0x55;
    image[511] = 0xaa;
    image[450] = 0x07;
    put32(&mut image, 454, 32);
    put32(&mut image, 458, 40);
    image[466] = 0x83;
    put32(&mut image, 470, 96);
    put32(&mut image, 474, 25);
    image
}
fn build_header(
    image: &mut [u8],
    lba: usize,
    blocksize: usize,
    positions: (u64, u64),
    usable: (u64, u64),
    array_lba: u64,
    table_crc: u32,
) {
    let (current, other) = positions;
    let (first, last) = usable;
    let h = &mut image[lba * blocksize..(lba + 1) * blocksize];
    h[..8].copy_from_slice(b"EFI PART");
    put32(h, 8, 0x0001_0000);
    put32(h, 12, 92);
    put64(h, 24, current);
    put64(h, 32, other);
    put64(h, 40, first);
    put64(h, 48, last);
    for (idx, b) in h[56..72].iter_mut().enumerate() {
        *b = idx as u8 + 1;
    }
    put64(h, 72, array_lba);
    put32(h, 80, 128);
    put32(h, 84, 128);
    put32(h, 88, table_crc);
    let checksum = crc32(&h[..92]);
    put32(h, 16, checksum);
}
fn gpt_image(blocksize: usize) -> Vec<u8> {
    let sectors = 256usize;
    let entries = 16 * 1024;
    let blocks = entries / blocksize;
    let last = sectors - 1;
    let first_usable = 2 + blocks;
    let last_usable = last - blocks - 1;
    let mut image = vec![0u8; blocksize * sectors];
    image[510] = 0x55;
    image[511] = 0xaa;
    image[450] = 0xee; // protective MBR
    put32(&mut image, 454, 1);
    put32(&mut image, 458, (sectors - 1) as u32);
    let mut arr = vec![0u8; entries];
    arr[0..16].copy_from_slice(&[
        0x28, 0x73, 0x2a, 0xc1, 0x1f, 0xf8, 0xd2, 0x11, 0xba, 0x4b, 0x00, 0xa0, 0xc9, 0x3e, 0xc9,
        0x3b,
    ]);
    arr[16..32].copy_from_slice(&[
        0x1a, 0x2b, 0x3c, 0x4d, 0x5e, 0x6f, 0x70, 0x81, 0x92, 0xa3, 0xb4, 0xc5, 0xd6, 0xe7, 0xf8,
        0x09,
    ]);
    let start = first_usable + 8;
    put64(&mut arr, 32, start as u64);
    put64(&mut arr, 40, (start + 23) as u64);
    for (i, u) in "Data卷".encode_utf16().enumerate() {
        arr[56 + i * 2..58 + i * 2].copy_from_slice(&u.to_le_bytes());
    }
    let crc = crc32(&arr);
    image[2 * blocksize..2 * blocksize + entries].copy_from_slice(&arr);
    let backup_lba = last - blocks;
    image[backup_lba * blocksize..backup_lba * blocksize + entries].copy_from_slice(&arr);
    build_header(
        &mut image,
        1,
        blocksize,
        (1, last as u64),
        (first_usable as u64, last_usable as u64),
        2,
        crc,
    );
    build_header(
        &mut image,
        last,
        blocksize,
        (last as u64, 1),
        (first_usable as u64, last_usable as u64),
        backup_lba as u64,
        crc,
    );
    image
}
#[test]
fn known_crc32_reference_vector() {
    assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
}
#[test]
fn healthy_primary_and_backup_gpt_512_and_4kn_without_mutation() {
    for blocksize in [512usize, 4096] {
        let source = gpt_image(blocksize);
        let scratch = Scratch::write(&source);
        let report = inspect_image(&scratch.image, blocksize as u64).unwrap();
        assert_eq!(report.scheme, Scheme::Gpt);
        assert_eq!(report.integrity, Integrity::Complete);
        assert!(report.warnings.is_empty());
        assert_eq!(report.partitions.len(), 1);
        let p = &report.partitions[0];
        assert_eq!(p.name.as_deref(), Some("Data卷"));
        assert_eq!(p.type_id, "c12a7328-f81f-11d2-ba4b-00a0c93ec93b");
        assert!(p.first_lba < p.last_lba);
        assert_eq!(fs::read(&scratch.image).unwrap(), source);
    }
}
#[test]
fn backup_corruption_is_explicit_degraded_not_repaired() {
    let mut image = gpt_image(512);
    let at = image.len() - 512 + 16;
    image[at] ^= 0x80;
    let scratch = Scratch::write(&image);
    let r = inspect_image(&scratch.image, 512).unwrap();
    assert_eq!(r.integrity, Integrity::Degraded);
    assert_eq!(r.partitions.len(), 1);
    assert!(r.warnings[0].contains("backup GPT invalid"));
    assert_eq!(fs::read(&scratch.image).unwrap(), image);
}
#[test]
fn primary_corruption_recovered_from_backup_only_for_reading() {
    let mut image = gpt_image(512);
    image[512 + 16] ^= 0x80;
    let scratch = Scratch::write(&image);
    let r = inspect_image(&scratch.image, 512).unwrap();
    assert_eq!(r.integrity, Integrity::Degraded);
    assert!(r.warnings[0].contains("using backup"));
    assert_eq!(fs::read(&scratch.image).unwrap(), image);
}
#[test]
fn both_crc_invalid_cannot_be_called_a_healthy_disk() {
    let mut image = gpt_image(512);
    image[512 + 16] ^= 1;
    let backup_crc = image.len() - 512 + 16;
    image[backup_crc] ^= 1;
    let scratch = Scratch::write(&image);
    assert!(matches!(
        inspect_image(&scratch.image, 512),
        Err(Error::Invalid(_))
    ));
}
#[test]
fn table_crc_corruption_both_copies_is_detected() {
    let mut image = gpt_image(512);
    image[2 * 512 + 56] ^= 0x10;
    let last_table = (256 - 1 - 32) * 512;
    image[last_table + 56] ^= 0x10;
    let scratch = Scratch::write(&image);
    assert!(matches!(
        inspect_image(&scratch.image, 512),
        Err(Error::Invalid(_))
    ));
}
#[test]
fn valid_mbr_primary_partitions_with_correct_ranges() {
    let scratch = Scratch::write(&mbr_image());
    let report = inspect_image(&scratch.image, 512).unwrap();
    assert_eq!(report.scheme, Scheme::Mbr);
    assert_eq!(report.integrity, Integrity::Complete);
    assert_eq!(report.partitions.len(), 2);
    assert_eq!(
        (
            report.partitions[0].first_lba,
            report.partitions[0].last_lba
        ),
        (32, 71)
    );
    assert_eq!(report.partitions[1].type_id, "0x83");
}
#[test]
fn overlapping_mbr_entries_are_rejected() {
    let mut img = mbr_image();
    put32(&mut img, 470, 50);
    let scratch = Scratch::write(&img);
    assert!(matches!(
        inspect_image(&scratch.image, 512),
        Err(Error::Invalid(_))
    ));
}
#[test]
fn hybrid_protective_mbr_is_refused_without_guessing() {
    let mut img = gpt_image(512);
    img[466] = 0x07;
    put32(&mut img, 470, 60);
    put32(&mut img, 474, 10);
    let scratch = Scratch::write(&img);
    assert!(matches!(
        inspect_image(&scratch.image, 512),
        Err(Error::Unsupported(_))
    ));
}
#[test]
fn unsupported_extended_ebr_chain_is_announced() {
    let mut img = mbr_image();
    img[450] = 0x0f;
    let scratch = Scratch::write(&img);
    let summary = inspect_image(&scratch.image, 512).unwrap();
    assert!(
        summary
            .warnings
            .iter()
            .any(|s| s.contains("EBR chain not inspected"))
    );
}
#[test]
fn non_aligned_or_bad_sector_size_and_directories_are_rejected() {
    let scratch = Scratch::write(&vec![0; 513]);
    assert!(matches!(
        inspect_image(&scratch.image, 512),
        Err(Error::Invalid(_))
    ));
    assert!(matches!(
        inspect_image(&scratch.image, 1234),
        Err(Error::Unsupported(_))
    ));
    assert!(matches!(
        inspect_image(&scratch.root, 512),
        Err(Error::Unsupported(_))
    ));
}
#[cfg(unix)]
#[test]
fn symlinked_image_is_refused_instead_of_followed() {
    use std::os::unix::fs::symlink;
    let scratch = Scratch::write(&mbr_image());
    let link = scratch.root.join("shortcut.img");
    symlink(&scratch.image, &link).unwrap();
    assert!(matches!(
        inspect_image(&link, 512),
        Err(Error::Unsupported(_))
    ));
}
