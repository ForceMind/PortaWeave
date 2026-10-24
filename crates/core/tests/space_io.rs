#[cfg(feature = "fault-injection")]
use portaweave_core::{BLOCK_SIZE, MaintenanceStatus, ROOT, StorageFault};
use portaweave_core::{Error, SpacePolicy, Volume};
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
            "pw-space-{}-{stamp}-{}",
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
fn lab() -> (Scratch, Volume) {
    let scratch = Scratch::new();
    let v = Volume::create(&scratch.0, 128 * 1024 * 1024).unwrap();
    (scratch, v)
}

#[test]
fn host_space_snapshot_and_overflow_checked_policy() {
    let (_s, mut v) = lab();
    let before = v.space_status().unwrap();
    assert_eq!(before.soft_headroom_bytes, 17 * 1024 * 1024);
    assert_eq!(
        before.data_admission_bytes,
        before
            .host_available_bytes
            .saturating_sub(before.soft_headroom_bytes)
    );
    assert!(matches!(
        v.set_space_policy(SpacePolicy {
            host_floor_bytes: u64::MAX,
            metadata_allowance_bytes: 1
        }),
        Err(Error::InvalidRange)
    ));
    assert_eq!(
        v.space_status().unwrap().soft_headroom_bytes,
        before.soft_headroom_bytes
    );
    assert!(!v.is_write_protected());
    v.set_space_policy(SpacePolicy {
        host_floor_bytes: 2,
        metadata_allowance_bytes: 3,
    })
    .unwrap();
    assert_eq!(v.stats().unwrap().space.soft_headroom_bytes, 5);
}

#[cfg(feature = "fault-injection")]
fn seeded(v: &mut Volume) -> (portaweave_core::FileId, portaweave_core::Handle) {
    let id = v.create_file(ROOT, "working.bin").unwrap();
    let h = v.open_file(id).unwrap();
    v.write(h, 0, &vec![11; BLOCK_SIZE * 3]).unwrap();
    (id, h)
}

#[cfg(feature = "fault-injection")]
#[test]
fn preflight_refusal_changes_neither_references_nor_segments_and_can_retry() {
    let (s, mut v) = lab();
    let (id, h) = seeded(&mut v);
    let before = v.stat(id).unwrap();
    let bytes = v.stats().unwrap().tracked_segment_bytes;
    let count = fs::read_dir(s.0.join("segments")).unwrap().count();
    v.set_test_available_space(Some(0));
    assert!(matches!(
        v.write(h, 0, b"changed"),
        Err(Error::HostSpace {
            available_bytes: 0,
            ..
        })
    ));
    assert_eq!(v.stat(id).unwrap(), before);
    assert_eq!(v.stats().unwrap().tracked_segment_bytes, bytes);
    assert_eq!(fs::read_dir(s.0.join("segments")).unwrap().count(), count);
    assert_eq!(v.read(h, 0, 7).unwrap(), vec![11; 7]);
    assert!(!v.is_write_protected());
    v.set_test_available_space(None);
    v.write(h, 0, b"changed").unwrap();
    assert_eq!(v.read(h, 0, 7).unwrap(), b"changed");
    v.verify().unwrap();
}

#[cfg(feature = "fault-injection")]
#[test]
fn admission_counts_full_copy_on_write_blocks_not_user_byte_count() {
    let (_s, mut v) = lab();
    let id = v.create_file(ROOT, "sparse").unwrap();
    let h = v.open_file(id).unwrap();
    let required = 17 * 1024 * 1024 + 24 + BLOCK_SIZE as u64;
    v.set_test_available_space(Some(required - 1));
    match v.write(h, 8 * 1024 * 1024, b"x") {
        Err(Error::HostSpace {
            required_bytes,
            available_bytes,
        }) => {
            assert_eq!(required_bytes, required);
            assert_eq!(available_bytes, required - 1);
        }
        other => panic!("expected precise capacity refusal: {other:?}"),
    }
    v.set_test_available_space(Some(required));
    v.write(h, 8 * 1024 * 1024, b"x").unwrap();
    assert_eq!(v.read(h, 8 * 1024 * 1024, 1).unwrap(), b"x");
}

#[cfg(feature = "fault-injection")]
#[test]
fn partial_truncate_requires_new_tail_but_zero_truncate_can_reclaim() {
    let (_s, mut v) = lab();
    let (id, h) = seeded(&mut v);
    let before = v.stat(id).unwrap();
    v.set_test_available_space(Some(0));
    assert!(matches!(v.truncate(h, 17), Err(Error::HostSpace { .. })));
    assert_eq!(v.stat(id).unwrap(), before);
    assert_eq!(v.read(h, 0, 32).unwrap(), vec![11; 32]);
    assert!(!v.is_write_protected());
    v.truncate(h, 0).unwrap();
    assert_eq!(v.stats().unwrap().tracked_segment_bytes, 0);
}

#[cfg(feature = "fault-injection")]
#[test]
fn deleting_unreferenced_segments_does_not_require_compaction_headroom() {
    let (_s, mut v) = lab();
    let (_, h) = seeded(&mut v);
    v.close_file(h).unwrap();
    v.set_test_available_space(Some(0));
    v.unlink(ROOT, "working.bin").unwrap();
    assert_eq!(v.stats().unwrap().tracked_segment_bytes, 0);
    assert_eq!(*v.maintenance_status(), MaintenanceStatus::Healthy);
    assert!(!v.is_write_protected());
}

#[cfg(feature = "fault-injection")]
#[test]
fn compaction_defers_without_poison_then_retries_after_space_returns() {
    let (_s, mut v) = lab();
    let (_, h) = seeded(&mut v);
    let view = v.acquire_view(h).unwrap();
    v.write(h, 0, &vec![22; BLOCK_SIZE * 2]).unwrap();
    let before = v.stats().unwrap().tracked_segment_bytes;
    v.set_test_available_space(Some(0));
    v.release_view(view).unwrap();
    assert!(matches!(
        v.maintenance_status(),
        MaintenanceStatus::DeferredSpace {
            available_bytes: 0,
            ..
        }
    ));
    assert!(!v.is_write_protected());
    assert_eq!(v.stats().unwrap().tracked_segment_bytes, before);
    assert_eq!(
        v.read(h, (BLOCK_SIZE * 2) as u64, 16).unwrap(),
        vec![11; 16]
    );
    v.set_test_available_space(None);
    v.close_file(h).unwrap();
    assert_eq!(*v.maintenance_status(), MaintenanceStatus::Healthy);
    assert!(v.stats().unwrap().tracked_segment_bytes < before);
    v.verify().unwrap();
}

#[cfg(feature = "fault-injection")]
#[test]
fn interrupted_and_partial_progress_writes_are_retried_not_misreported() {
    for fault in [
        StorageFault::ShortWrites { max_bytes: 4093 },
        StorageFault::InterruptedOnce,
    ] {
        let (_s, mut v) = lab();
        let (_, h) = seeded(&mut v);
        v.inject_storage_fault_once(fault).unwrap();
        let next = vec![33; BLOCK_SIZE * 3];
        assert_eq!(v.write(h, 0, &next).unwrap(), next.len());
        assert_eq!(v.read(h, 0, next.len()).unwrap(), next);
        assert!(!v.is_write_protected());
        v.verify().unwrap();
    }
}

#[cfg(feature = "fault-injection")]
#[test]
fn staged_io_failures_never_publish_incomplete_data_and_reopen_cleans_orphans() {
    for fault in [
        StorageFault::WriteZeroAfter { bytes: 27 },
        StorageFault::NoSpaceAfter {
            bytes: 24 + BLOCK_SIZE as u64 + 3,
        },
        StorageFault::IoErrorAfter {
            bytes: 24 + BLOCK_SIZE as u64 + 3,
        },
        StorageFault::DataSyncFailure,
        StorageFault::DirectorySyncFailure,
    ] {
        let (s, mut v) = lab();
        let (id, h) = seeded(&mut v);
        let before = v.stat(id).unwrap();
        let view = v.acquire_view(h).unwrap();
        v.inject_storage_fault_once(fault).unwrap();
        let error = v.write(h, 0, &vec![44; BLOCK_SIZE * 3]).unwrap_err();
        let Error::Io(error) = error else {
            panic!("not an I/O failure: {error:?}");
        };
        match fault {
            StorageFault::WriteZeroAfter { .. } => {
                assert_eq!(error.kind(), std::io::ErrorKind::WriteZero)
            }
            StorageFault::NoSpaceAfter { .. } => {
                assert_eq!(error.kind(), std::io::ErrorKind::StorageFull)
            }
            _ => assert_eq!(error.kind(), std::io::ErrorKind::Other),
        }
        assert!(v.is_write_protected());
        assert_eq!(v.stat(id).unwrap(), before);
        assert_eq!(
            v.read(h, 0, BLOCK_SIZE * 3).unwrap(),
            vec![11; BLOCK_SIZE * 3]
        );
        assert_eq!(v.read_view(view, 0, 100).unwrap(), vec![11; 100]);
        assert!(matches!(
            v.write(h, 0, b"retry"),
            Err(Error::WriteProtected)
        ));
        v.close_file(h).unwrap();
        v.release_view(view).unwrap();
        drop(v);
        let mut reopened = Volume::open(&s.0).unwrap();
        reopened.verify().unwrap();
        assert!(!reopened.is_write_protected());
        let h = reopened.open_file(id).unwrap();
        assert_eq!(
            reopened.read(h, 0, BLOCK_SIZE * 3).unwrap(),
            vec![11; BLOCK_SIZE * 3]
        );
        assert_eq!(reopened.stats().unwrap().segments, 1);
    }
}

#[cfg(feature = "fault-injection")]
#[test]
fn compaction_io_failure_preserves_business_success_and_current_bytes() {
    for fault in [
        StorageFault::NoSpaceAfter { bytes: 31 },
        StorageFault::DataSyncFailure,
    ] {
        let (s, mut v) = lab();
        let (id, h) = seeded(&mut v);
        let view = v.acquire_view(h).unwrap();
        v.write(h, 0, &vec![55; BLOCK_SIZE * 2]).unwrap();
        v.inject_storage_fault_once(fault).unwrap();
        v.release_view(view).unwrap();
        assert!(matches!(
            v.maintenance_status(),
            MaintenanceStatus::Failed { .. }
        ));
        assert!(v.is_write_protected());
        assert!(matches!(v.read_view(view, 0, 1), Err(Error::InvalidHandle)));
        assert_eq!(
            v.read(h, 0, BLOCK_SIZE * 2).unwrap(),
            vec![55; BLOCK_SIZE * 2]
        );
        assert_eq!(
            v.read(h, (BLOCK_SIZE * 2) as u64, BLOCK_SIZE).unwrap(),
            vec![11; BLOCK_SIZE]
        );
        v.close_file(h).unwrap();
        drop(v);
        let mut reopened = Volume::open(&s.0).unwrap();
        let h = reopened.open_file(id).unwrap();
        assert_eq!(
            reopened.read(h, 0, BLOCK_SIZE * 2).unwrap(),
            vec![55; BLOCK_SIZE * 2]
        );
        assert_eq!(
            reopened
                .read(h, (BLOCK_SIZE * 2) as u64, BLOCK_SIZE)
                .unwrap(),
            vec![11; BLOCK_SIZE]
        );
        reopened.verify().unwrap();
    }
}

#[cfg(feature = "fault-injection")]
#[test]
fn faults_are_owner_local_and_zero_short_write_limit_is_rejected() {
    let (_a, mut a) = lab();
    let (_b, mut b) = lab();
    let (_, ha) = seeded(&mut a);
    let (_, hb) = seeded(&mut b);
    assert!(matches!(
        a.inject_storage_fault_once(StorageFault::ShortWrites { max_bytes: 0 }),
        Err(Error::InvalidRange)
    ));
    a.inject_storage_fault_once(StorageFault::DataSyncFailure)
        .unwrap();
    b.write(hb, 0, b"other owner").unwrap();
    assert!(!b.is_write_protected());
    assert!(matches!(a.write(ha, 0, b"this owner"), Err(Error::Io(_))));
    assert_eq!(b.read(hb, 0, 11).unwrap(), b"other owner");
}
