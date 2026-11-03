use super::*;
use std::time::{SystemTime, UNIX_EPOCH};

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        Self(std::env::temp_dir().join(format!("pw-maintenance-{}-{stamp}-{}", std::process::id(), token().unwrap())))
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn attributes() -> CreateAttributes {
    CreateAttributes { uid: 501, gid: 20, mode: 0o700 }
}

// Valid abandoned-owner state, built only through this one owner's connection.
// No second writer, unknown schema, foreign original or fake filesystem mount.
fn orphan_objects(v: &mut Volume, count: usize) -> Vec<FileId> {
    let version = v.catalog_version;
    let tx = v.db.transaction().unwrap();
    let time = Timestamp::now().unwrap();
    let mut ids = Vec::new();
    for index in 0..count {
        let kind = i64::from(version == CatalogVersion::DirectoryV3 && index % 2 == 0);
        tx.execute("INSERT INTO objects(kind) VALUES (?1)", [kind]).unwrap();
        let id = FileId(tx.last_insert_rowid());
        if version.has_metadata() {
            metadata::insert(&tx, id, attributes(), time).unwrap();
        }
        ids.push(id);
    }
    tx.commit().unwrap();
    ids
}

fn count(v: &Volume, sql: &str) -> i64 {
    v.db.query_row(sql, [], |row| row.get(0)).unwrap()
}

#[test]
fn bounded_abandoned_cleanup_continues_without_new_business_mutations() {
    for version in 1..=3 {
        let s = Scratch::new();
        let mut v = match version {
            1 => Volume::create(&s.0, 128 * 1024 * 1024),
            2 => Volume::create_with_metadata(&s.0, 128 * 1024 * 1024, attributes()),
            _ => Volume::create_with_directory_lifetimes(&s.0, 128 * 1024 * 1024, attributes()),
        }.unwrap();
        orphan_objects(&mut v, 300);
        for _ in 0..300 {
            v.stage(&[(0, vec![37; BLOCK_SIZE])]).unwrap();
        }
        v.verify().unwrap();
        drop(v);
        let mut v = Volume::open(&s.0).unwrap();
        // Ordinary reopen now does one bounded step, not an unbounded drain.
        assert_eq!(count(&v, "SELECT COUNT(*) FROM objects WHERE id<>1"), 172);
        assert_eq!(count(&v, "SELECT COUNT(*) FROM segments"), 44);
        let first = v.maintenance_step().unwrap();
        assert_eq!(first.orphan_candidates, MAINTENANCE_CANDIDATES);
        assert_eq!(first.objects_removed, MAINTENANCE_CANDIDATES);
        assert_eq!(first.segments_removed, 44);
        assert!(first.dead_segment_candidates <= 2 * MAINTENANCE_CANDIDATES);
        assert_eq!(first.blocks_copied, 0);
        let next = v.maintenance_step().unwrap();
        assert_eq!(next.objects_removed, 44);
        assert_eq!(count(&v, "SELECT COUNT(*) FROM objects WHERE id<>1"), 0);
        assert_eq!(count(&v, "SELECT COUNT(*) FROM segments"), 0);
        assert_eq!(fs::read_dir(s.0.join("segments")).unwrap().count(), 0);
        assert_eq!(v.maintenance_step().unwrap(), MaintenanceReport::default());
        assert!(!v.is_write_protected());
        v.verify().unwrap();
    }
}

#[test]
fn finite_frontier_revisits_released_low_ids_despite_new_high_ids() {
    let s = Scratch::new();
    let mut v = Volume::create(&s.0, 1 << 20).unwrap();
    let ids = orphan_objects(&mut v, 200);
    let handles: Vec<_> = ids.iter().map(|id| v.open_file(*id).unwrap()).collect();
    let first = v.maintenance_step().unwrap();
    assert_eq!(first.orphan_candidates, 128);
    assert_eq!(first.objects_removed, 0); // Healthy/zero removal is not drained.
    let frontier = v.orphan_sweep.high;
    orphan_objects(&mut v, 200);
    assert_eq!(v.orphan_sweep.high, frontier);
    v.close_file(handles[0]).unwrap(); // finishes only the original high-water range
    assert!(v.stat(ids[0]).is_ok());
    assert!(v.orphan_sweep.high.is_none());
    let next = v.maintenance_step().unwrap();
    assert_eq!(next.objects_removed, 1);
    assert!(matches!(v.stat(ids[0]), Err(Error::NotFound)));
    for handle in handles.into_iter().skip(1) {
        v.close_file(handle).unwrap();
    }
    for _ in 0..8 {
        v.maintenance_step().unwrap();
    }
    assert_eq!(count(&v, "SELECT COUNT(*) FROM objects WHERE id<>1"), 0);
}

#[test]
fn protected_owner_refuses_explicit_maintenance_without_clearing_health() {
    let s = Scratch::new();
    let mut v = Volume::create(&s.0, 1 << 20).unwrap();
    orphan_objects(&mut v, 1);
    v.protected.set(true);
    let health = v.maintenance_status().clone();
    assert!(matches!(v.maintenance_step(), Err(Error::WriteProtected)));
    assert_eq!(*v.maintenance_status(), health);
    assert_eq!(count(&v, "SELECT COUNT(*) FROM objects WHERE id<>1"), 1);
}

#[test]
fn multi_page_dead_segments_keep_old_reader_and_drain_after_release() {
    let s = Scratch::new();
    let mut v = Volume::create(&s.0, 64 * 1024 * 1024).unwrap();
    let id = v.create_file(ROOT, "many-segments").unwrap();
    let h = v.open_file(id).unwrap();
    for index in 0..130 {
        v.write(h, (index * BLOCK_SIZE) as u64, &[37]).unwrap();
    }
    let old: Vec<_> = fs::read_dir(s.0.join("segments")).unwrap().map(|entry| entry.unwrap().path()).collect();
    let lease = v.acquire_view(h).unwrap();
    for index in 0..130 {
        v.write(h, (index * BLOCK_SIZE) as u64, &[42]).unwrap();
    }
    for _ in 0..4 {
        let report = v.maintenance_step().unwrap();
        assert_eq!(report.segments_removed, 0);
        assert!(report.dead_segment_candidates <= 256);
        assert!(old.iter().all(|p| p.exists()));
        assert_eq!(v.read_view(lease, (129 * BLOCK_SIZE) as u64, 1).unwrap(), [37]);
    }
    v.release_view(lease).unwrap();
    for _ in 0..4 {
        v.maintenance_step().unwrap();
    }
    assert!(old.iter().all(|p| !p.exists()));
    assert_eq!(v.stats().unwrap().segments, 130);
    assert_eq!(v.read(h, (129 * BLOCK_SIZE) as u64, 1).unwrap(), [42]);
    v.close_file(h).unwrap();
    v.verify().unwrap();
}
