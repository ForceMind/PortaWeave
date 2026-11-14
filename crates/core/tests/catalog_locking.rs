#![cfg(unix)]

use portaweave_core::{CreateAttributes, ROOT, Volume};
use std::{
    fs::{self, File, OpenOptions},
    io,
    os::fd::AsRawFd,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
const CHILD_TEST: &str = "catalog_lock_probe_child";
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "pw-catalog-lock-{}-{stamp}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn volume_path(&self) -> PathBuf {
        self.0.join("volume")
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

enum Format {
    V1,
    V2,
    V3,
}
fn create_volume(path: &Path, format: Format) -> Volume {
    let attributes = CreateAttributes {
        uid: 100,
        gid: 20,
        mode: 0o700,
    };
    let quota = 128 * 1024 * 1024;
    match format {
        Format::V1 => Volume::create(path, quota),
        Format::V2 => Volume::create_with_metadata(path, quota, attributes),
        Format::V3 => Volume::create_with_directory_lifetimes(path, quota, attributes),
    }
    .unwrap()
}

struct ProbeChild(Option<Child>);
impl Drop for ProbeChild {
    fn drop(&mut self) {
        if let Some(child) = self.0.as_mut() {
            // A timeout or parent assertion must never leave a live probe or zombie.
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn expect_catalog_lock(path: &Path, expected: &str, stage: &str) {
    let parent = std::process::id();
    // exec a fresh test process. It opens only the catalog file, never a Volume
    // or another SQLite connection. The parent must not open/close a second
    // descriptor for this inode: POSIX record locks are process-scoped.
    let child = Command::new(std::env::current_exe().unwrap())
        .args(["--ignored", "--exact", CHILD_TEST, "--nocapture"])
        .env("PW_CATALOG_LOCK_PATH", path)
        .env("PW_CATALOG_LOCK_PARENT", parent.to_string())
        .env("PW_CATALOG_LOCK_EXPECTED", expected)
        .env("RUST_BACKTRACE", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let child_id = child.id();
    let mut child = ProbeChild(Some(child));
    let started = Instant::now();
    loop {
        if child.0.as_mut().unwrap().try_wait().unwrap().is_some() {
            let output = child.0.take().unwrap().wait_with_output().unwrap();
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(
                output.status.success(),
                "catalog lock probe failed at {stage}: {}\n{stdout}\n{stderr}",
                output.status
            );
            let marker = format!("PW_CATALOG_LOCK_{expected} parent={parent} child={child_id}");
            assert!(
                stdout.contains(&marker),
                "missing probe result at {stage}: {stdout}"
            );
            return;
        }
        assert!(
            started.elapsed() < PROBE_TIMEOUT,
            "catalog lock probe exceeded {PROBE_TIMEOUT:?} at {stage}"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

fn set_lock(file: &File, lock: &mut libc::flock) -> io::Result<()> {
    // SAFETY: the descriptor is a live owned File; `lock` points to an
    // initialized platform libc::flock for the duration of this nonblocking
    // F_SETLK call. No pointer is retained by the kernel.
    let result = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETLK, lock) };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[test]
#[ignore = "invoked by parent tests in a fresh exec process"]
fn catalog_lock_probe_child() {
    let parent: u32 = std::env::var("PW_CATALOG_LOCK_PARENT")
        .expect("parent-only probe")
        .parse()
        .unwrap();
    let child = std::process::id();
    assert_ne!(child, parent, "probe must be a separate process");
    // SAFETY: getppid takes no arguments and has no memory preconditions.
    let actual_parent = unsafe { libc::getppid() };
    assert_eq!(u32::try_from(actual_parent).unwrap(), parent);

    let path = std::env::var_os("PW_CATALOG_LOCK_PATH").unwrap();
    let expected = std::env::var("PW_CATALOG_LOCK_EXPECTED").unwrap();
    assert!(matches!(expected.as_str(), "HELD" | "AVAILABLE"));
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .unwrap();
    // SAFETY: libc::flock contains only integer fields. All-zero is a valid
    // initial representation on the Unix targets; required fields are set below.
    let mut lock: libc::flock = unsafe { std::mem::zeroed() };
    lock.l_type = libc::F_WRLCK as _;
    lock.l_whence = libc::SEEK_SET as _;
    // SQLite's Unix VFS SHARED_FIRST / SHARED_SIZE. A writer covering the whole
    // shared range conflicts even on VFS variants locking only one shared byte.
    lock.l_start = 0x4000_0002;
    lock.l_len = 510;
    let observed = match set_lock(&file, &mut lock) {
        Ok(()) => {
            // Never retain a successful probe lock while reporting/asserting.
            lock.l_type = libc::F_UNLCK as _;
            set_lock(&file, &mut lock).expect("release successful probe lock");
            "AVAILABLE"
        }
        Err(error)
            if matches!(
                error.raw_os_error(),
                Some(libc::EACCES) | Some(libc::EAGAIN)
            ) =>
        {
            "HELD"
        }
        Err(error) => panic!("F_SETLK failed for a reason other than lock contention: {error}"),
    };
    assert_eq!(
        observed, expected,
        "unexpected SQLite catalog SHARED range state"
    );
    println!("PW_CATALOG_LOCK_{observed} parent={parent} child={child}");
}

fn barrier_preserves_catalog_locks(format: Format) {
    let scratch = Scratch::new();
    let path = scratch.volume_path();
    let catalog = path.join("catalog.sqlite");
    let mut volume = create_volume(&path, format);
    let file = volume.create_file(ROOT, "data").unwrap();
    let handle = volume.open_file(file).unwrap();
    volume.write(handle, 0, b"old bytes").unwrap();
    let lease = volume.acquire_range_view(handle, 0, 9).unwrap();
    volume.write(handle, 0, b"new bytes").unwrap();

    expect_catalog_lock(&catalog, "HELD", "before barrier");
    let first = volume.synchronize_storage().unwrap();
    expect_catalog_lock(&catalog, "HELD", "after barrier");
    assert_eq!(
        first.checkpoint.wal_frames,
        first.checkpoint.checkpointed_frames
    );
    assert_eq!(volume.read(handle, 0, 9).unwrap(), b"new bytes");
    assert_eq!(volume.read_view(lease, 0, 9).unwrap(), b"old bytes");

    // With no intervening write, the repeated checkpoint has no new frames.
    let second = volume.synchronize_storage().unwrap();
    expect_catalog_lock(
        &catalog,
        "HELD",
        "after repeated barrier without new frames",
    );
    assert_eq!(first, second);
    volume.write(handle, 0, b"newer now").unwrap();
    assert_eq!(volume.read(handle, 0, 9).unwrap(), b"newer now");
    assert_eq!(volume.read_view(lease, 0, 9).unwrap(), b"old bytes");
    volume.close_file(handle).unwrap();
    volume.release_view(lease).unwrap();
    drop(volume);

    // Positive control: this host permits the exact same lock once SQLite closes.
    expect_catalog_lock(&catalog, "AVAILABLE", "after owner release");
}

#[test]
fn v1_barrier_preserves_catalog_locks() {
    barrier_preserves_catalog_locks(Format::V1);
}
#[test]
fn v2_barrier_preserves_catalog_locks() {
    barrier_preserves_catalog_locks(Format::V2);
}
#[test]
fn v3_barrier_preserves_catalog_locks() {
    barrier_preserves_catalog_locks(Format::V3);
}

#[cfg(feature = "fault-injection")]
fn catalog_sync_failure_preserves_locks(format: Format) {
    use portaweave_core::{Error, FaultPoint};

    let scratch = Scratch::new();
    let path = scratch.volume_path();
    let catalog = path.join("catalog.sqlite");
    let mut volume = create_volume(&path, format);
    let file = volume.create_file(ROOT, "retained").unwrap();
    let handle = volume.open_file(file).unwrap();
    volume.write(handle, 0, b"old bytes").unwrap();
    let lease = volume.acquire_range_view(handle, 0, 9).unwrap();
    volume.write(handle, 0, b"new bytes").unwrap();

    expect_catalog_lock(&catalog, "HELD", "before injected catalog sync failure");
    volume.inject_once(FaultPoint::StorageCatalogSynced);
    assert!(matches!(volume.synchronize_storage(), Err(Error::Injected)));
    assert!(volume.is_write_protected());
    expect_catalog_lock(&catalog, "HELD", "after injected catalog sync failure");
    assert_eq!(volume.read(handle, 0, 9).unwrap(), b"new bytes");
    assert_eq!(volume.read_view(lease, 0, 9).unwrap(), b"old bytes");
    assert!(matches!(
        volume.write(handle, 0, b"denied"),
        Err(Error::WriteProtected)
    ));
    assert!(matches!(
        volume.synchronize_storage(),
        Err(Error::WriteProtected)
    ));
    volume.close_file(handle).unwrap();
    volume.release_view(lease).unwrap();
    drop(volume);
    expect_catalog_lock(&catalog, "AVAILABLE", "after protected owner release");
}

#[test]
#[cfg(feature = "fault-injection")]
fn v1_catalog_sync_failure_preserves_locks() {
    catalog_sync_failure_preserves_locks(Format::V1);
}
#[test]
#[cfg(feature = "fault-injection")]
fn v2_catalog_sync_failure_preserves_locks() {
    catalog_sync_failure_preserves_locks(Format::V2);
}
#[test]
#[cfg(feature = "fault-injection")]
fn v3_catalog_sync_failure_preserves_locks() {
    catalog_sync_failure_preserves_locks(Format::V3);
}
