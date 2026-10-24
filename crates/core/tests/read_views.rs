use portaweave_core::{BLOCK_SIZE, Error, ReadViewLimits, ROOT, Volume};
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
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
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
    }).unwrap();
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
    }).unwrap();
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
    }).unwrap();
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
    }).unwrap();
    assert!(matches!(v.acquire_view(h), Err(Error::ResourceLimit)));
    v.write(h, offset + 3, b"XYZ").unwrap();
    assert_eq!(v.read(h, offset, 7).unwrap(), b"payXYZd");
    assert!(v.stats().unwrap().tracked_segment_bytes < (BLOCK_SIZE * 3) as u64);
    v.verify().unwrap();
}
