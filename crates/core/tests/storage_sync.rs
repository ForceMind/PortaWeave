use portaweave_core::{Error, ROOT, Volume};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        Self(std::env::temp_dir().join(format!(
            "pw-storage-sync-{}-{stamp}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )))
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        if self.0.exists() {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
}

#[test]
#[cfg(unix)]
fn backend_barrier_keeps_live_handles_and_old_range_leases() {
    let s = Scratch::new();
    let mut v = Volume::create(&s.0, 128 * 1024 * 1024).unwrap();
    let id = v.create_file(ROOT, "data").unwrap();
    let h = v.open_file(id).unwrap();
    v.write(h, 0, b"old bytes").unwrap();
    let lease = v.acquire_range_view(h, 0, 9).unwrap();
    v.write(h, 0, b"new bytes").unwrap();
    let expected = v.stats().unwrap().segments;
    let sync = v.synchronize_storage().unwrap();
    assert_eq!(sync.segment_files, expected as u64);
    assert_eq!(
        sync.checkpoint.wal_frames,
        sync.checkpoint.checkpointed_frames
    );
    assert_eq!(v.read(h, 0, 9).unwrap(), b"new bytes");
    assert_eq!(v.read_view(lease, 0, 9).unwrap(), b"old bytes");
    v.unlink(ROOT, "data").unwrap();
    v.close_file(h).unwrap();
    v.synchronize_storage().unwrap();
    assert_eq!(v.read_view(lease, 0, 9).unwrap(), b"old bytes");
    v.release_view(lease).unwrap();
    assert_eq!(v.synchronize_storage().unwrap().segment_files, 0);
    drop(v);
    Volume::open(&s.0).unwrap().verify().unwrap();
}

#[test]
#[cfg(unix)]
fn busy_checkpoint_is_not_success_or_write_protection() {
    use rusqlite::Connection;
    let s = Scratch::new();
    let mut v = Volume::create(&s.0, 128 * 1024 * 1024).unwrap();
    let id = v.create_file(ROOT, "busy").unwrap();
    let h = v.open_file(id).unwrap();
    let reader = Connection::open(s.0.join("catalog.sqlite")).unwrap();
    reader
        .execute_batch("BEGIN; SELECT * FROM objects;")
        .unwrap();
    v.write(h, 0, b"current bytes").unwrap();
    assert!(matches!(v.synchronize_storage(), Err(Error::Busy)));
    assert!(!v.is_write_protected());
    assert_eq!(v.read(h, 0, 13).unwrap(), b"current bytes");
    reader.execute_batch("ROLLBACK;").unwrap();
    drop(reader);
    v.synchronize_storage().unwrap();
}

#[test]
#[cfg(all(unix, feature = "fault-injection"))]
fn failures_at_both_barrier_boundaries_latch_protection_without_losing_content() {
    use portaweave_core::FaultPoint;
    for fault in [
        FaultPoint::StorageSegmentsSynced,
        FaultPoint::StorageCatalogSynced,
    ] {
        let s = Scratch::new();
        let mut v = Volume::create(&s.0, 128 * 1024 * 1024).unwrap();
        let id = v.create_file(ROOT, "failure").unwrap();
        let h = v.open_file(id).unwrap();
        v.write(h, 0, b"retained").unwrap();
        v.inject_once(fault);
        assert!(matches!(v.synchronize_storage(), Err(Error::Injected)));
        assert!(v.is_write_protected());
        assert_eq!(v.read(h, 0, 8).unwrap(), b"retained");
        assert!(matches!(
            v.synchronize_storage(),
            Err(Error::WriteProtected)
        ));
        v.close_file(h).unwrap();
        drop(v);
        let mut v = Volume::open(&s.0).unwrap();
        let h = v.open_file(id).unwrap();
        assert_eq!(v.read(h, 0, 8).unwrap(), b"retained");
        v.synchronize_storage().unwrap();
    }
}

#[test]
#[cfg(unix)]
fn missing_tracked_segment_returns_an_error_instead_of_success() {
    let s = Scratch::new();
    let mut v = Volume::create(&s.0, 128 * 1024 * 1024).unwrap();
    let id = v.create_file(ROOT, "missing").unwrap();
    let h = v.open_file(id).unwrap();
    v.write(h, 0, b"payload").unwrap();
    let original = fs::read_dir(s.0.join("segments"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let preserved = s.0.join("preserved-segment");
    fs::rename(&original, &preserved).unwrap();
    assert!(matches!(v.synchronize_storage(), Err(Error::Io(_))));
    assert!(v.is_write_protected());
    fs::rename(preserved, original).unwrap();
    v.close_file(h).unwrap();
    drop(v);
    Volume::open(&s.0).unwrap().verify().unwrap();
}

#[test]
#[cfg(not(unix))]
fn unsupported_platform_does_not_report_a_fake_directory_barrier() {
    let s = Scratch::new();
    let mut v = Volume::create(&s.0, 128 * 1024 * 1024).unwrap();
    let id = v.create_file(ROOT, "windows").unwrap();
    let h = v.open_file(id).unwrap();
    assert!(matches!(
        v.synchronize_storage(),
        Err(Error::UnsupportedOperation)
    ));
    assert!(!v.is_write_protected());
    v.write(h, 0, b"still writable").unwrap();
    assert_eq!(v.read(h, 0, 14).unwrap(), b"still writable");
}
