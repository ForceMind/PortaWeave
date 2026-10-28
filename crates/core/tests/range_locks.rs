use portaweave_core::{
    CreateAttributes, Credentials, Error, FileAccess,
    LockMode::{Exclusive, Shared},
    MAX_RANGE_LOCKS, MetadataUpdate, ROOT, Volume,
};
use std::{
    fs,
    path::PathBuf,
    sync::{
        Arc, Barrier, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        Self(std::env::temp_dir().join(format!(
            "pw-range-{}-{stamp}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )))
    }
    fn volume(&self, version: u8) -> Volume {
        let a = CreateAttributes {
            uid: 100,
            gid: 20,
            mode: 0o777,
        };
        match version {
            1 => Volume::create(&self.0, 1 << 24),
            2 => Volume::create_with_metadata(&self.0, 1 << 24, a),
            _ => Volume::create_with_directory_lifetimes(&self.0, 1 << 24, a),
        }
        .unwrap()
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
#[test]
fn conflicts_follow_inode_and_handle_not_name_in_all_formats() {
    for version in [1, 2, 3] {
        let s = Scratch::new();
        let mut v = s.volume(version);
        let id = v.create_file(ROOT, "file").unwrap();
        v.hard_link(id, ROOT, "alias").unwrap();
        let a = v.open_file(id).unwrap();
        let b = v.open_file(v.lookup(ROOT, "alias").unwrap()).unwrap();
        let before = v.stat(id).unwrap();
        let one = v.lock_file_range(a, 10, 10, Shared).unwrap();
        let two = v.lock_file_range(b, 19, 2, Shared).unwrap();
        assert!(matches!(
            v.lock_file_range(a, 19, 1, Exclusive),
            Err(Error::LockConflict)
        ));
        let own = v.lock_file_range(a, 10, 9, Exclusive).unwrap();
        let edge = v.lock_file_range(b, 0, 10, Exclusive).unwrap(); // touching is not overlapping
        let far = v.lock_file_range(b, u64::MAX - 1, 1, Exclusive).unwrap();
        assert_eq!(v.stat(id).unwrap(), before);
        assert!(matches!(
            v.unlock_file_range(b, one),
            Err(Error::InvalidHandle)
        ));
        v.rename(ROOT, "file", ROOT, "moved", false).unwrap();
        v.unlink(ROOT, "alias").unwrap();
        v.unlink(ROOT, "moved").unwrap();
        assert!(matches!(
            v.lock_file_range(b, 10, 1, Shared),
            Err(Error::LockConflict)
        ));
        // Advisory grants deliberately do not intercept ordinary data requests.
        v.write(b, 10, b"uncooperative").unwrap();
        v.unlock_file_range(a, own).unwrap();
        v.unlock_file_range(a, one).unwrap();
        assert!(matches!(
            v.unlock_file_range(a, one),
            Err(Error::InvalidHandle)
        ));
        for lock in [two, edge, far] {
            v.unlock_file_range(b, lock).unwrap();
        }
        v.close_file(a).unwrap();
        v.close_file(b).unwrap();
        assert!(matches!(v.stat(id), Err(Error::NotFound)));
        assert_eq!(v.active_range_locks(), 0);
    }
}
#[test]
fn grants_share_raw_budget_and_release_with_only_the_originating_close() {
    let s = Scratch::new();
    let mut v = s.volume(2);
    let c = Credentials::new(100, 20, &[]).unwrap();
    v.posix(c.clone())
        .unwrap()
        .create_file(ROOT, "file", 0o600, 0)
        .unwrap();
    let id = v.lookup(ROOT, "file").unwrap();
    let a = v.open_file(id).unwrap();
    let policy = v
        .posix(c.clone())
        .unwrap()
        .open_file(ROOT, "file", FileAccess::ReadWrite)
        .unwrap();
    for _ in 0..MAX_RANGE_LOCKS - 1 {
        let _lock = v.lock_file_range(a, 0, 1, Shared).unwrap();
    }
    let last = policy.lock_range(&mut v, 0, 1, Shared).unwrap();
    assert_eq!(v.active_range_locks(), MAX_RANGE_LOCKS);
    assert!(matches!(
        policy.lock_range(&mut v, 2, 1, Shared),
        Err(Error::ResourceLimit)
    ));
    v.close_file(a).unwrap();
    assert_eq!(v.active_range_locks(), 1);
    let extra = policy.lock_range(&mut v, 0, 1, Exclusive).unwrap();
    policy.unlock_range(&mut v, last).unwrap();
    policy.unlock_range(&mut v, extra).unwrap();
    let _left = policy.lock_range(&mut v, 0, 1, Shared).unwrap();
    policy.close(&mut v).unwrap();
    assert_eq!(v.active_range_locks(), 0);
}
#[test]
fn policy_uses_open_rights_and_invalid_ranges_do_not_mutate_health() {
    let s = Scratch::new();
    let mut v = s.volume(3);
    let c = Credentials::new(100, 20, &[]).unwrap();
    v.posix(c.clone())
        .unwrap()
        .create_file(ROOT, "file", 0o600, 0)
        .unwrap();
    let r = v
        .posix(c.clone())
        .unwrap()
        .open_file(ROOT, "file", FileAccess::Read)
        .unwrap();
    let w = v
        .posix(c.clone())
        .unwrap()
        .open_file(ROOT, "file", FileAccess::Write)
        .unwrap();
    assert!(matches!(
        r.lock_range(&mut v, 0, 1, Exclusive),
        Err(Error::PermissionDenied)
    ));
    assert!(matches!(
        w.lock_range(&mut v, 0, 1, Shared),
        Err(Error::PermissionDenied)
    ));
    let before = r.info(&v).unwrap();
    for (offset, length) in [(0, 0), (u64::MAX, 1), (1, u64::MAX)] {
        assert!(matches!(
            r.lock_range(&mut v, offset, length, Shared),
            Err(Error::InvalidRange)
        ));
    }
    assert_eq!(r.info(&v).unwrap(), before);
    assert_eq!(v.active_range_locks(), 0);
    v.set_metadata(
        before.stat.id,
        MetadataUpdate {
            mode: Some(0),
            ..Default::default()
        },
    )
    .unwrap();
    let lock = r.lock_range(&mut v, 0, 5, Shared).unwrap();
    assert!(matches!(
        w.lock_range(&mut v, 0, 1, Exclusive),
        Err(Error::LockConflict)
    ));
    r.close(&mut v).unwrap();
    assert!(matches!(
        r.unlock_range(&mut v, lock),
        Err(Error::InvalidHandle)
    ));
    let _lock = w.lock_range(&mut v, 0, 5, Exclusive).unwrap();
    w.close(&mut v).unwrap();
    assert!(!v.is_write_protected());
}
#[test]
fn foreign_and_reopened_owners_cannot_use_old_lock_tokens() {
    let a = Scratch::new();
    let b = Scratch::new();
    let mut v = a.volume(1);
    let mut other = b.volume(1);
    let id = v.create_file(ROOT, "file").unwrap();
    let h = v.open_file(id).unwrap();
    let lock = v.lock_file_range(h, 0, 1, Exclusive).unwrap();
    let oid = other.create_file(ROOT, "file").unwrap();
    let oh = other.open_file(oid).unwrap();
    assert!(matches!(
        other.unlock_file_range(oh, lock),
        Err(Error::InvalidHandle)
    ));
    drop(v);
    let mut v = Volume::open(&a.0).unwrap();
    let fresh = v.open_file(id).unwrap();
    assert_eq!(v.active_range_locks(), 0);
    assert!(matches!(
        v.unlock_file_range(fresh, lock),
        Err(Error::InvalidHandle)
    ));
    let _lock = v.lock_file_range(fresh, 0, 1, Exclusive).unwrap();
    v.close_file(fresh).unwrap();
    other.close_file(oh).unwrap();
}
#[test]
fn serialized_competing_threads_have_exactly_one_exclusive_winner() {
    let s = Scratch::new();
    let mut v = s.volume(1);
    let id = v.create_file(ROOT, "file").unwrap();
    let handles: Vec<_> = (0..8).map(|_| v.open_file(id).unwrap()).collect();
    let owner = Arc::new(Mutex::new(v));
    let start = Arc::new(Barrier::new(8));
    let finish = Arc::new(Barrier::new(8));
    let threads: Vec<_> = handles
        .into_iter()
        .map(|h| {
            let o = owner.clone();
            let s = start.clone();
            let f = finish.clone();
            std::thread::spawn(move || {
                s.wait();
                let result = o.lock().unwrap().lock_file_range(h, 0, 8, Exclusive);
                f.wait();
                let won = match result {
                    Ok(_) => true,
                    Err(Error::LockConflict) => false,
                    other => panic!("unexpected {other:?}"),
                };
                o.lock().unwrap().close_file(h).unwrap();
                won
            })
        })
        .collect();
    assert_eq!(
        threads
            .into_iter()
            .map(|t| usize::from(t.join().unwrap()))
            .sum::<usize>(),
        1
    );
    assert_eq!(owner.lock().unwrap().active_range_locks(), 0);
}
#[cfg(feature = "fault-injection")]
#[test]
fn release_and_close_work_under_storage_protection() {
    use portaweave_core::FaultPoint;
    let s = Scratch::new();
    let mut v = s.volume(1);
    let id = v.create_file(ROOT, "file").unwrap();
    let h = v.open_file(id).unwrap();
    let first = v.lock_file_range(h, 0, 1, Exclusive).unwrap();
    let _second = v.lock_file_range(h, 0, 1, Shared).unwrap();
    v.inject_once(FaultPoint::AfterCommit);
    v.write(h, 0, b"x").unwrap();
    assert!(v.is_write_protected());
    v.unlock_file_range(h, first).unwrap();
    v.close_file(h).unwrap();
    assert_eq!(v.active_range_locks(), 0);
}
