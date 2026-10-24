use portaweave_core::{BLOCK_SIZE, Error, ROOT, ReadViewLimits, Volume};
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
            "pw-read-view-{}-{stamp}-{}",
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
fn snapshot_index_admission_is_global_and_does_not_poison_data_writes() {
    let s = Scratch::new();
    let mut v = Volume::create(&s.0, 128 * 1024 * 1024).unwrap();
    let id = v.create_file(ROOT, "snapshot").unwrap();
    let h = v.open_file(id).unwrap();
    v.write(h, 0, &vec![7; BLOCK_SIZE * 3]).unwrap();
    v.set_read_view_limits(ReadViewLimits {
        max_active_views: 2,
        max_indexed_block_refs: 2,
    })
    .unwrap();
    assert!(matches!(v.acquire_view(h), Err(Error::ResourceLimit)));
    assert!(!v.is_write_protected());
    assert_eq!(v.read(h, 0, 4).unwrap(), vec![7; 4]);
    v.truncate(h, (BLOCK_SIZE * 2) as u64).unwrap();
    let view = v.acquire_view(h).unwrap();
    let stats = v.stats().unwrap();
    assert_eq!(stats.read_views, 1);
    assert_eq!(stats.indexed_view_block_refs, 2);
    assert_eq!(stats.read_view_limits.max_indexed_block_refs, 2);
    assert!(matches!(v.acquire_view(h), Err(Error::ResourceLimit)));
    assert!(matches!(
        v.set_read_view_limits(ReadViewLimits {
            max_active_views: 0,
            max_indexed_block_refs: 0,
        }),
        Err(Error::ResourceLimit)
    ));
    v.write(h, 0, b"new-data").unwrap();
    assert_eq!(v.read(h, 0, 8).unwrap(), b"new-data");
    assert_eq!(v.read_view(view, 0, 8).unwrap(), vec![7; 8]);
    v.unlink(ROOT, "snapshot").unwrap();
    v.close_file(h).unwrap();
    assert_eq!(v.read_view(view, 0, 8).unwrap(), vec![7; 8]);
    v.release_view(view).unwrap();
    assert_eq!(v.stats().unwrap().indexed_view_block_refs, 0);
    assert_eq!(v.stats().unwrap().tracked_segment_bytes, 0);
    v.set_read_view_limits(ReadViewLimits {
        max_active_views: 0,
        max_indexed_block_refs: 0,
    })
    .unwrap();
    assert!(!v.is_write_protected());
}

#[test]
fn active_view_count_cap_is_independent_of_index_budget() {
    let s = Scratch::new();
    let mut v = Volume::create(&s.0, 128 * 1024 * 1024).unwrap();
    let id = v.create_file(ROOT, "empty").unwrap();
    let h = v.open_file(id).unwrap();
    v.set_read_view_limits(ReadViewLimits {
        max_active_views: 1,
        max_indexed_block_refs: 100,
    })
    .unwrap();
    let first = v.acquire_view(h).unwrap();
    assert!(matches!(v.acquire_view(h), Err(Error::ResourceLimit)));
    v.release_view(first).unwrap();
    let next = v.acquire_view(h).unwrap();
    v.release_view(next).unwrap();
    assert_eq!(v.stats().unwrap().read_views, 0);
}

#[test]
fn sparse_large_logical_file_reads_only_requested_range() {
    let s = Scratch::new();
    let mut v = Volume::create(&s.0, 1_u64 << 40).unwrap();
    let id = v.create_file(ROOT, "large_sparse").unwrap();
    let h = v.open_file(id).unwrap();
    let offset = (1_u64 << 37) + 123;
    v.write(h, offset, b"payload").unwrap();
    assert_eq!(v.stat(id).unwrap().size, offset + 7);
    assert_eq!(v.read(h, 0, 4096).unwrap(), vec![0; 4096]);
    assert_eq!(v.read(h, offset - 5, 12).unwrap(), b"\0\0\0\0\0payload");
    v.set_read_view_limits(ReadViewLimits {
        max_active_views: 0,
        max_indexed_block_refs: 0,
    })
    .unwrap();
    assert!(matches!(v.acquire_view(h), Err(Error::ResourceLimit)));
    v.write(h, offset + 3, b"XYZ").unwrap();
    assert_eq!(v.read(h, offset, 7).unwrap(), b"payXYZd");
    assert!(v.stats().unwrap().tracked_segment_bytes < (BLOCK_SIZE * 3) as u64);
    v.verify().unwrap();
}

#[test]
fn small_range_of_large_file_survives_truncate_unlink_close_and_compaction() {
    let s = Scratch::new();
    let mut v = Volume::create(&s.0, 128 * 1024 * 1024).unwrap();
    let id = v.create_file(ROOT, "ranges").unwrap();
    let h = v.open_file(id).unwrap();
    v.write(h, 0, &vec![5; BLOCK_SIZE * 8]).unwrap();
    v.set_read_view_limits(ReadViewLimits {
        max_active_views: 4,
        max_indexed_block_refs: 2,
    })
    .unwrap();
    assert!(matches!(v.acquire_view(h), Err(Error::ResourceLimit)));
    // Two physical blocks, but only six permitted bytes in the snapshot.
    let start = BLOCK_SIZE as u64 - 3;
    let a = v.acquire_range_view(h, start, 6).unwrap();
    assert_eq!(v.stats().unwrap().indexed_view_block_refs, 2);
    assert!(matches!(
        v.acquire_range_view(h, 0, 1),
        Err(Error::ResourceLimit)
    ));
    assert!(matches!(
        v.read_view(a, start - 1, 1),
        Err(Error::InvalidRange)
    ));
    assert!(matches!(v.read_view(a, start, 7), Err(Error::InvalidRange)));
    v.write(h, start, b"newest").unwrap();
    v.truncate(h, 0).unwrap();
    v.unlink(ROOT, "ranges").unwrap();
    v.close_file(h).unwrap();
    assert_eq!(v.read_view(a, start, 6).unwrap(), vec![5; 6]);
    assert!(!v.is_write_protected());
    v.release_view(a).unwrap();
    assert_eq!(v.stats().unwrap().indexed_view_block_refs, 0);
    assert_eq!(v.stats().unwrap().tracked_segment_bytes, 0);
    drop(v);
    Volume::open(&s.0).unwrap().verify().unwrap();
}

#[test]
fn sparse_empty_and_beyond_eof_ranges_keep_original_eof_and_share_view_budget() {
    let s = Scratch::new();
    let mut v = Volume::create(&s.0, 1_u64 << 40).unwrap();
    let id = v.create_file(ROOT, "sparse_ranges").unwrap();
    let h = v.open_file(id).unwrap();
    let base = 1_u64 << 38;
    v.truncate(h, base + 10).unwrap();
    v.set_read_view_limits(ReadViewLimits {
        max_active_views: 3,
        max_indexed_block_refs: 0,
    })
    .unwrap();
    let hole = v.acquire_range_view(h, base, 20).unwrap();
    let empty = v.acquire_range_view(h, 0, 0).unwrap();
    let beyond = v.acquire_range_view(h, base + 50, 20).unwrap();
    assert!(matches!(v.acquire_view(h), Err(Error::ResourceLimit)));
    v.write(h, base, b"now allocated").unwrap();
    assert_eq!(v.read_view(hole, base, 20).unwrap(), vec![0; 10]);
    assert!(v.read_view(beyond, base + 50, 20).unwrap().is_empty());
    assert!(v.read_view(empty, 0, 0).unwrap().is_empty());
    assert!(matches!(v.read_view(empty, 0, 1), Err(Error::InvalidRange)));
    for lease in [hole, empty, beyond] {
        v.release_view(lease).unwrap();
    }
    assert_eq!(v.stats().unwrap().read_views, 0);
}

#[test]
fn range_overflow_is_refused_without_a_lease_or_write_protection() {
    let s = Scratch::new();
    let mut v = Volume::create(&s.0, 1024).unwrap();
    let id = v.create_file(ROOT, "bounds").unwrap();
    let h = v.open_file(id).unwrap();
    for (offset, length) in [(u64::MAX, 1), (i64::MAX as u64, 1), (1_u64 << 63, 0)] {
        assert!(matches!(
            v.acquire_range_view(h, offset, length),
            Err(Error::InvalidRange)
        ));
    }
    assert_eq!(v.stats().unwrap().read_views, 0);
    assert!(!v.is_write_protected());
    let view = v.acquire_range_view(h, i64::MAX as u64, 0).unwrap();
    assert!(v.read_view(view, i64::MAX as u64, 0).unwrap().is_empty());
    v.release_view(view).unwrap();
    v.close_file(h).unwrap();
    assert!(matches!(
        v.acquire_range_view(h, 0, 1),
        Err(Error::InvalidHandle)
    ));
}
