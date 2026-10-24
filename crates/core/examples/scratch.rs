//! Run only with a NEW, disposable path: cargo run -p portaweave-core --example scratch -- /tmp/new-lab
use portaweave_core::{ROOT, Volume};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args_os().nth(1).ok_or("usage: scratch NEW_DISPOSABLE_PATH")?;
    eprintln!("EXPERIMENTAL STORAGE LAB: not mounted, not for the only copy of any data");
    let mut volume = Volume::create(&path, 64 * 1024 * 1024)?;
    let file = volume.create_file(ROOT, "hello.txt")?;
    let handle = volume.open_file(file)?;
    volume.write(handle, 0, b"PortaWeave persistent core\n")?;
    volume.hard_link(file, ROOT, "alias.txt")?;
    volume.unlink(ROOT, "hello.txt")?;
    println!("{}", String::from_utf8(volume.read(handle, 0, 4096)?)?);
    println!("Before final unlink: {:?}", volume.stats()?);
    volume.unlink(ROOT, "alias.txt")?;
    println!("Unlinked but open: {:?}", volume.stats()?);
    volume.close_file(handle)?;
    println!("Last handle closed, automatic reclamation: {:?}", volume.stats()?);
    volume.verify()?;
    drop(volume);
    Volume::open(&path)?.verify()?;
    Ok(())
}
