use super::*;
use std::time::{SystemTime, UNIX_EPOCH};

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        Self(std::env::temp_dir().join(format!(
            "pw-compaction-{}-{stamp}-{}",
            std::process::id(),
            token().unwrap()
        )))
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

// Build a valid large segment through the INTERNAL writer, not the public
// MAX_IO-limited write API. No second catalog connection or foreign writer.
// Current public writes make smaller segments; this checks that compaction's
// own admission does not rely on that fact or reject a compatible larger one.
fn seeded(version: u8) -> (Scratch, Volume, Handle, i64) {
    let scratch = Scratch::new();
    let attrs = CreateAttributes {
        uid: 501,
        gid: 20,
        mode: 0o700,
    };
    let mut v = match version {
        1 => Volume::create(&scratch.0, 128 * 1024 * 1024),
        2 => Volume::create_with_metadata(&scratch.0, 128 * 1024 * 1024, attrs),
        _ => Volume::create_with_directory_lifetimes(&scratch.0, 128 * 1024 * 1024, attrs),
    }
    .unwrap();
    let id = v.create_file(ROOT, "retained").unwrap();
    let live = MAX_IO / BLOCK_SIZE + 1;
    let payload: Vec<_> = (0..live * 2)
        .map(|index| (index as i64, vec![37; BLOCK_SIZE]))
        .collect();
    let refs = v.stage(&payload).unwrap();
    let segment = refs[0].segment;
    let tx = v.db.transaction().unwrap();
    Volume::bind(&tx, id, &refs[..live]).unwrap();
    tx.execute(
        "UPDATE objects SET size=?1 WHERE id=?2",
        params![(live * BLOCK_SIZE) as i64, id.0],
    )
    .unwrap();
    tx.commit().unwrap();
    let h = v.open_file(id).unwrap();
    v.verify().unwrap();
    (scratch, v, h, segment)
}

fn refs_left(v: &Volume, segment: i64) -> i64 {
    v.db
        .query_row(
            "SELECT COUNT(*) FROM blocks WHERE segment=?1",
            [segment],
            |row| row.get(0),
        )
        .unwrap()
}

#[test]
fn compaction_caps_copy_and_preserves_partial_source_until_final_commit() {
    for version in 1..=3 {
        let (_s, mut v, h, source) = seeded(version);
        let before = v.object_info(v.object(h).unwrap()).unwrap();
        let lease = v.acquire_view(h).unwrap();
        // An admitted old reader still suppresses the entire source candidate.
        v.mutate(|_| Ok(())).unwrap();
        assert_eq!(refs_left(&v, source), (MAX_IO / BLOCK_SIZE + 1) as i64);
        assert_eq!(v.read_view(lease, MAX_IO as u64, 1).unwrap(), [37]);
        // Releasing the reader triggers exactly one bounded compaction.
        v.release_view(lease).unwrap();
        assert_eq!(refs_left(&v, source), 1);
        assert!(segment::path(&v.root, source).exists());
        let copied: i64 = v
            .db
            .query_row(
                "SELECT SUM(bytes) FROM segments WHERE id<>?1",
                [source],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(copied as u64, MAX_IO as u64 + segment::HEADER);
        assert_eq!(v.read(h, 0, 1).unwrap(), [37]);
        assert_eq!(
            v.read(h, MAX_IO as u64, BLOCK_SIZE).unwrap(),
            vec![37; BLOCK_SIZE]
        );
        assert_eq!(v.object_info(before.stat.id).unwrap(), before);
        v.verify().unwrap();
        // A later owner operation resumes the remaining ref without modifying
        // logical identity, size, timestamps or generation.
        v.mutate(|_| Ok(())).unwrap();
        assert_eq!(refs_left(&v, source), 0);
        assert!(!segment::path(&v.root, source).exists());
        assert_eq!(v.object_info(before.stat.id).unwrap(), before);
        assert!(!v.is_write_protected());
        v.close_file(h).unwrap();
    }
}

#[cfg(feature = "fault-injection")]
#[test]
fn interrupted_chunk_keeps_old_references_and_reopen_resumes_safely() {
    let (s, mut v, h, source) = seeded(3);
    v.inject_once(FaultPoint::BeforeCompactionCommit);
    // The successful foreground action remains successful; maintenance failure
    // is separately latched. The new staged segment has no committed refs.
    v.mutate(|_| Ok(())).unwrap();
    assert!(matches!(
        v.maintenance_status(),
        MaintenanceStatus::Failed { .. }
    ));
    assert!(v.is_write_protected());
    assert_eq!(refs_left(&v, source), (MAX_IO / BLOCK_SIZE + 1) as i64);
    assert_eq!(v.read(h, MAX_IO as u64, 1).unwrap(), [37]);
    v.close_file(h).unwrap();
    drop(v);
    let mut v = Volume::open(&s.0).unwrap();
    assert_eq!(refs_left(&v, source), 1);
    assert!(segment::path(&v.root, source).exists());
    let id = v.lookup(ROOT, "retained").unwrap();
    let h = v.open_file(id).unwrap();
    assert_eq!(v.read(h, MAX_IO as u64, 1).unwrap(), [37]);
    v.mutate(|_| Ok(())).unwrap();
    assert!(!segment::path(&v.root, source).exists());
    v.verify().unwrap();
    v.close_file(h).unwrap();
}
