use portaweave_core::{Error, ROOT, Recovery, RecoveryLimits, Volume};
use rusqlite::Connection;
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
        let path = std::env::temp_dir().join(format!(
            "pw-catalog-gate-{}-{stamp}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn container(&self) -> PathBuf {
        self.0.join("volume")
    }
    fn populate(&self) -> (i64, i64) {
        let mut v = Volume::create(self.container(), 128 * 1024 * 1024).unwrap();
        let keep = v.create_file(ROOT, "keep").unwrap();
        let h = v.open_file(keep).unwrap();
        v.write(h, 0, b"retained payload").unwrap();
        v.close_file(h).unwrap();
        let orphan = v.create_file(ROOT, "orphan").unwrap();
        let h = v.open_file(orphan).unwrap();
        v.write(h, 0, b"open unlinked payload").unwrap();
        v.unlink(ROOT, "orphan").unwrap();
        // Simulate an owner ending with an open, unlinked file. Normal reopen
        // would collect it, so a premature GC is observable in this fixture.
        drop(v);
        (keep.raw(), orphan.raw())
    }
    fn segment_bytes(&self) -> Vec<(PathBuf, Vec<u8>)> {
        let mut files: Vec<_> = fs::read_dir(self.container().join("segments"))
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (entry.file_name().into(), fs::read(entry.path()).unwrap())
            })
            .collect();
        files.sort();
        files
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn unexpected_schema_is_rejected_before_orphan_collection_or_trigger_execution() {
    for sql in [
        "CREATE TABLE future_feature(value BLOB)",
        "CREATE VIEW unexpected_view AS SELECT * FROM objects",
        "CREATE TRIGGER delete_entries AFTER DELETE ON objects BEGIN DELETE FROM entries; END",
        "DROP INDEX entries_object",
        "ALTER TABLE objects ADD COLUMN unknown_feature INTEGER",
    ] {
        let s = Scratch::new();
        let (keep, orphan) = s.populate();
        let before = s.segment_bytes();
        let db = Connection::open(s.container().join("catalog.sqlite")).unwrap();
        db.execute_batch(sql).unwrap();
        drop(db);
        assert!(matches!(
            Volume::open(s.container()),
            Err(Error::UnsupportedFormat)
        ));
        assert_eq!(before, s.segment_bytes());
        let db = Connection::open(s.container().join("catalog.sqlite")).unwrap();
        let count: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM objects WHERE id IN (?1,?2)",
                [keep, orphan],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);
        let linked: i64 = db
            .query_row("SELECT COUNT(*) FROM entries WHERE object=?1", [keep], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(linked, 1);
        drop(db);
        let recovery = Recovery::prepare(
            s.container(),
            s.0.join("recovery"),
            RecoveryLimits::default(),
        )
        .unwrap();
        assert!(matches!(recovery.verify(), Err(Error::UnsupportedFormat)));
        assert_eq!(before, s.segment_bytes());
    }
}

#[test]
fn recognized_catalog_after_sqlite_vacuum_still_reopens_and_collects_orphans() {
    let s = Scratch::new();
    let (keep, orphan) = s.populate();
    let db = Connection::open(s.container().join("catalog.sqlite")).unwrap();
    db.execute_batch("VACUUM;").unwrap();
    drop(db);
    let mut v = Volume::open(s.container()).unwrap();
    let keep = portaweave_core::FileId::from_raw(keep).unwrap();
    let orphan = portaweave_core::FileId::from_raw(orphan).unwrap();
    assert!(matches!(v.stat(orphan), Err(Error::NotFound)));
    let h = v.open_file(keep).unwrap();
    assert_eq!(v.read(h, 0, 100).unwrap(), b"retained payload");
    v.verify().unwrap();
}
