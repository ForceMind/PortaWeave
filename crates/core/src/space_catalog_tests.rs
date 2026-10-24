//! A real SQLite SQLITE_FULL result using its page-count limit, not a physical
//! full disk or a VFS power-loss simulation.
use crate::{Error, ROOT, Volume};
use std::{
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn sqlite_full_is_visible_write_protects_and_preserves_committed_content() {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("pw-catalog-full-{}-{stamp}", std::process::id()));
    let mut v = Volume::create(&root, 128 * 1024 * 1024).unwrap();
    let id = v.create_file(ROOT, "committed").unwrap();
    let h = v.open_file(id).unwrap();
    v.write(h, 0, b"keep committed content").unwrap();
    let pages: i64 =
        v.db.query_row("PRAGMA page_count", [], |r| r.get(0))
            .unwrap();
    v.db.pragma_update(None, "max_page_count", pages).unwrap();
    let error = (0..2048)
        .find_map(|i| {
            let name = format!("fixture-{i:04}-{}", "z".repeat(160));
            v.create_file(ROOT, &name).err()
        })
        .expect("the real SQLite page limit must be reached");
    match error {
        Error::Sql(rusqlite::Error::SqliteFailure(code, _)) => {
            assert_eq!(code.extended_code & 0xff, rusqlite::ffi::SQLITE_FULL)
        }
        other => panic!("expected real SQLITE_FULL, got {other:?}"),
    }
    assert!(v.is_write_protected());
    assert_eq!(v.read(h, 0, 100).unwrap(), b"keep committed content");
    assert!(matches!(
        v.write(h, 0, b"retry"),
        Err(Error::WriteProtected)
    ));
    v.close_file(h).unwrap();
    drop(v);
    let mut v = Volume::open(&root).unwrap();
    v.verify().unwrap();
    let h = v.open_file(id).unwrap();
    assert_eq!(v.read(h, 0, 100).unwrap(), b"keep committed content");
    v.close_file(h).unwrap();
    drop(v);
    fs::remove_dir_all(root).unwrap();
}
