//! Disposable container artifact handover, not a mount or physical exFAT test.
use portaweave_core::{BLOCK_SIZE, ROOT, Volume};
use std::{error::Error, path::Path};

type TestResult = Result<(), Box<dyn Error>>;

fn payload(stage: u8) -> Vec<u8> {
    let mut bytes = vec![stage; BLOCK_SIZE * 4 + 7];
    let label = "跨系统文件内容 / portable bytes".as_bytes();
    bytes[..label.len()].copy_from_slice(label);
    bytes
}

fn seed(path: &Path) -> TestResult {
    let mut volume = Volume::create(path, 16 * 1024 * 1024)?;
    let directory = volume.mkdir(ROOT, "项目")?;
    let file = volume.create_file(directory, "source.bin")?;
    let data = volume.open_file(file)?;
    volume.write(data, 0, &payload(0))?;
    volume.hard_link(file, directory, "source-alias.bin")?;
    volume.close_file(data)?;
    let marker = volume.create_file(ROOT, "stage")?;
    let handle = volume.open_file(marker)?;
    volume.write(handle, 0, &[0])?;
    volume.close_file(handle)?;
    volume.verify()?;
    println!("Created stage 0: {:?}", volume.stats()?);
    Ok(())
}

fn handover(path: &Path, expected: u8, next: Option<u8>) -> TestResult {
    let mut volume = Volume::open(path)?;
    let directory = volume.lookup(ROOT, "项目")?;
    let file = volume.lookup(directory, "source.bin")?;
    let alias = volume.lookup(directory, "source-alias.bin")?;
    assert_eq!(file, alias, "hard-link identity changed between systems");
    assert_eq!(volume.stat(file)?.links, 2);
    let data = volume.open_file(file)?;
    let marker_id = volume.lookup(ROOT, "stage")?;
    let marker = volume.open_file(marker_id)?;
    assert_eq!(volume.read(marker, 0, 1)?, vec![expected]);
    assert_eq!(volume.read(data, 0, BLOCK_SIZE * 5)?, payload(expected));
    if let Some(next) = next {
        let old = volume.acquire_view(data)?;
        volume.write(data, 0, &payload(next))?;
        assert_eq!(volume.read_view(old, 0, BLOCK_SIZE * 5)?, payload(expected));
        volume.release_view(old)?;
        volume.rename(directory, "source.bin", directory, "SOURCE.BIN", false)?;
        let alias_handle = volume.open_file(alias)?;
        assert_eq!(volume.read(alias_handle, 0, BLOCK_SIZE * 5)?, payload(next));
        volume.close_file(alias_handle)?;
        volume.write(marker, 0, &[next])?;
        let temporary = volume.create_file(directory, "temporary.bin")?;
        let temp_handle = volume.open_file(temporary)?;
        volume.write(temp_handle, 0, b"opened during deletion")?;
        volume.unlink(directory, "temporary.bin")?;
        assert_eq!(volume.read(temp_handle, 0, 100)?, b"opened during deletion");
        volume.close_file(temp_handle)?;
    }
    volume.close_file(data)?;
    volume.close_file(marker)?;
    volume.verify()?;
    // One current data segment + one marker segment, with retired versions gone.
    assert_eq!(volume.stats()?.segments, 2);
    println!(
        "Verified incoming stage {expected}, outgoing {:?}: {:?}",
        next,
        volume.stats()?
    );
    Ok(())
}

fn main() -> TestResult {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 2 {
        return Err("usage: interop DISPOSABLE_PATH seed|macos|windows|verify".into());
    }
    let path = Path::new(&args[0]);
    match args[1].to_str() {
        Some("seed") => seed(path),
        Some("macos") => handover(path, 0, Some(1)),
        Some("windows") => handover(path, 1, Some(2)),
        Some("verify") => handover(path, 2, None),
        _ => Err("unknown handover stage".into()),
    }
}
