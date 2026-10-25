use portaweave_core::{
    CreateAttributes, Credentials, Error, FileAccess, MetadataUpdate, ROOT, ReadViewLimits, Volume,
};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let p = std::env::temp_dir().join(format!(
            "pw-policy-views-{}-{stamp}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&p).unwrap();
        Self(p)
    }
    fn volume(&self) -> (Volume, portaweave_core::FileId) {
        let mut v = Volume::create_with_metadata(
            self.0.join("volume"),
            1 << 26,
            CreateAttributes {
                uid: 100,
                gid: 10,
                mode: 0o777,
            },
        )
        .unwrap();
        let id = v
            .posix(caller())
            .unwrap()
            .create_file(ROOT, "file", 0o600, 0)
            .unwrap();
        let h = v.open_file(id).unwrap();
        v.write(h, 0, b"0123456789").unwrap();
        v.close_file(h).unwrap();
        (v, id)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn caller() -> Credentials {
    Credentials::new(200, 20, &[]).unwrap()
}
fn open(v: &mut Volume, access: FileAccess) -> portaweave_core::PosixFile {
    v.posix(caller())
        .unwrap()
        .open_file(ROOT, "file", access)
        .unwrap()
}

#[test]
fn write_only_grants_cannot_admit_even_empty_read_leases() {
    let s = Scratch::new();
    let (mut v, _) = s.volume();
    let file = open(&mut v, FileAccess::Write);
    let before = v.stats().unwrap();
    assert!(matches!(
        file.acquire_range_view(&mut v, 0, 10),
        Err(Error::PermissionDenied)
    ));
    assert!(matches!(
        file.acquire_range_view(&mut v, 0, 0),
        Err(Error::PermissionDenied)
    ));
    let after = v.stats().unwrap();
    assert_eq!(
        (after.read_views, after.indexed_view_block_refs),
        (before.read_views, before.indexed_view_block_refs)
    );
    assert!(!v.is_write_protected());
    file.close(&mut v).unwrap();
}

#[test]
fn range_lease_retains_original_content_after_chmod_overwrite_truncate_unlink_and_close() {
    let s = Scratch::new();
    let (mut v, id) = s.volume();
    let writer = open(&mut v, FileAccess::ReadWrite);
    let reader = open(&mut v, FileAccess::Read);
    v.set_metadata(
        id,
        MetadataUpdate {
            mode: Some(0),
            ..Default::default()
        },
    )
    .unwrap();
    let lease = reader.acquire_range_view(&mut v, 2, 5).unwrap();
    writer.write(&mut v, 0, b"ABCDEFGHIJ").unwrap();
    writer.truncate(&mut v, 0).unwrap();
    v.unlink(ROOT, "file").unwrap();
    reader.close(&mut v).unwrap();
    writer.close(&mut v).unwrap();
    assert_eq!(v.stats().unwrap().logical_bytes, 0);
    assert!(v.stats().unwrap().tracked_segment_bytes > 0);
    assert_eq!(lease.read(&v, 2, 5).unwrap(), b"23456");
    assert!(matches!(lease.read(&v, 1, 1), Err(Error::InvalidRange)));
    lease.close(&mut v).unwrap();
    assert_eq!(v.stats().unwrap().segments, 0);
    assert!(matches!(lease.read(&v, 2, 1), Err(Error::InvalidHandle)));
}

#[test]
fn absolute_bounds_original_eof_empty_ranges_and_overflow_admission_are_preserved() {
    let s = Scratch::new();
    let (mut v, _) = s.volume();
    let file = open(&mut v, FileAccess::Read);
    let lease = file.acquire_range_view(&mut v, 8, 12).unwrap();
    assert_eq!(lease.read(&v, 8, 12).unwrap(), b"89");
    assert!(lease.read(&v, 19, 1).unwrap().is_empty());
    assert!(lease.read(&v, 20, 0).unwrap().is_empty());
    assert!(matches!(lease.read(&v, 7, 1), Err(Error::InvalidRange)));
    assert!(matches!(lease.read(&v, 20, 1), Err(Error::InvalidRange)));
    let count = v.stats().unwrap().read_views;
    assert!(matches!(
        file.acquire_range_view(&mut v, i64::MAX as u64, 1),
        Err(Error::InvalidRange)
    ));
    assert_eq!(v.stats().unwrap().read_views, count);
    let empty = file.acquire_range_view(&mut v, 5, 0).unwrap();
    assert!(empty.read(&v, 5, 0).unwrap().is_empty());
    assert!(matches!(empty.read(&v, 4, 0), Err(Error::InvalidRange)));
    empty.close(&mut v).unwrap();
    lease.close(&mut v).unwrap();
    file.close(&mut v).unwrap();
    assert!(!v.is_write_protected());
}

#[test]
fn policy_and_administrative_views_share_active_and_index_admission() {
    let s = Scratch::new();
    let (mut v, id) = s.volume();
    let file = open(&mut v, FileAccess::Read);
    let raw = v.open_file(id).unwrap();
    v.set_read_view_limits(ReadViewLimits {
        max_active_views: 3,
        max_indexed_block_refs: 2,
    })
    .unwrap();
    let admin = v.acquire_view(raw).unwrap();
    let lease = file.acquire_range_view(&mut v, 0, 10).unwrap();
    assert_eq!(v.stats().unwrap().indexed_view_block_refs, 2);
    assert!(matches!(
        file.acquire_range_view(&mut v, 0, 10),
        Err(Error::ResourceLimit)
    ));
    lease.close(&mut v).unwrap();
    v.set_read_view_limits(ReadViewLimits {
        max_active_views: 1,
        max_indexed_block_refs: 100,
    })
    .unwrap();
    assert!(matches!(
        file.acquire_range_view(&mut v, 0, 10),
        Err(Error::ResourceLimit)
    ));
    v.release_view(admin).unwrap();
    let replacement = file.acquire_range_view(&mut v, 0, 10).unwrap();
    file.close(&mut v).unwrap();
    v.close_file(raw).unwrap();
    assert_eq!(replacement.read(&v, 0, 10).unwrap(), b"0123456789");
    replacement.close(&mut v).unwrap();
    assert!(!v.is_write_protected());
}

#[test]
fn copied_stale_foreign_and_owner_dropped_view_tokens_cannot_be_reused() {
    let s = Scratch::new();
    let (mut v, _) = s.volume();
    let file = open(&mut v, FileAccess::Read);
    let lease = file.acquire_range_view(&mut v, 0, 10).unwrap();
    file.close(&mut v).unwrap();
    let mut other = Volume::create_with_metadata(
        s.0.join("other"),
        100,
        CreateAttributes {
            uid: 100,
            gid: 10,
            mode: 0o777,
        },
    )
    .unwrap();
    assert!(matches!(
        lease.read(&other, 0, 1),
        Err(Error::InvalidHandle)
    ));
    assert!(matches!(lease.close(&mut other), Err(Error::InvalidHandle)));
    lease.close(&mut v).unwrap();
    assert!(matches!(lease.close(&mut v), Err(Error::InvalidHandle)));
    let file = open(&mut v, FileAccess::Read);
    let next = file.acquire_range_view(&mut v, 0, 10).unwrap();
    assert_ne!(lease, next);
    file.close(&mut v).unwrap();
    drop(v);
    let v = Volume::open(s.0.join("volume")).unwrap();
    assert!(matches!(next.read(&v, 0, 1), Err(Error::InvalidHandle)));
    assert!(!v.is_write_protected());
    assert!(!other.is_write_protected());
}

#[cfg(feature = "fault-injection")]
#[test]
fn protected_owner_can_read_and_release_policy_leases_without_forcing_gc() {
    let s = Scratch::new();
    let (mut v, _) = s.volume();
    let file = open(&mut v, FileAccess::ReadWrite);
    let lease = file.acquire_range_view(&mut v, 0, 10).unwrap();
    v.inject_once(portaweave_core::FaultPoint::AfterCommit);
    file.write(&mut v, 0, b"A").unwrap();
    assert!(v.is_write_protected());
    assert_eq!(lease.read(&v, 0, 10).unwrap(), b"0123456789");
    assert_eq!(file.read(&v, 0, 10).unwrap(), b"A123456789");
    lease.close(&mut v).unwrap();
    file.close(&mut v).unwrap();
    assert_eq!(
        (
            v.stats().unwrap().read_views,
            v.stats().unwrap().indexed_view_block_refs,
            v.stats().unwrap().open_handles
        ),
        (0, 0, 0)
    );
    drop(v);
    let mut v = Volume::open(s.0.join("volume")).unwrap();
    let file = open(&mut v, FileAccess::Read);
    assert_eq!(file.read(&v, 0, 10).unwrap(), b"A123456789");
    file.close(&mut v).unwrap();
}
