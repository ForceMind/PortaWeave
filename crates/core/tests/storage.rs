use portaweave_core::{BLOCK_SIZE, Error, FORMAT, Kind, MAX_IO, ROOT, Volume};
use std::{fs, io::{Read, Seek, SeekFrom, Write}, path::PathBuf, process::Command,
    sync::atomic::{AtomicU64, Ordering}, time::{SystemTime, UNIX_EPOCH}};
#[cfg(feature = "fault-injection")]
use portaweave_core::FaultPoint;

static NEXT: AtomicU64 = AtomicU64::new(1);
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        Self(std::env::temp_dir().join(format!("pw-core-{}-{time}-{n}", std::process::id())))
    }
}
impl Drop for Scratch {
    fn drop(&mut self) { if self.0.exists() { fs::remove_dir_all(&self.0).expect("close all owners before cleanup"); } }
}
fn lab() -> (Scratch, Volume) {
    let scratch = Scratch::new();
    let volume = Volume::create(&scratch.0, 128 * 1024 * 1024).unwrap();
    (scratch, volume)
}
fn contents(volume: &Volume, name: &str) -> Vec<u8> {
    // This helper intentionally only obtains the file ID; callers needing a new
    // handle use mutable access. The query below is used to validate namespace.
    let id = volume.lookup(ROOT, name).unwrap();
    assert_eq!(volume.stat(id).unwrap().kind, Kind::File);
    Vec::new()
}

#[test]
fn persistent_roundtrip_and_stable_identity() {
    let (scratch, mut volume) = lab();
    let id = volume.create_file(ROOT, "项目.txt").unwrap();
    let handle = volume.open_file(id).unwrap();
    volume.write(handle, 0, b"persistent bytes").unwrap();
    volume.close_file(handle).unwrap();
    volume.verify().unwrap();
    drop(volume);
    let mut reopened = Volume::open(&scratch.0).unwrap();
    assert_eq!(reopened.lookup(ROOT, "项目.txt").unwrap(), id);
    let h = reopened.open_file(id).unwrap();
    assert_eq!(reopened.read(h, 0, 100).unwrap(), b"persistent bytes");
    reopened.close_file(h).unwrap();
}

#[test]
fn sparse_cross_block_write_and_eof() {
    let (_scratch, mut v) = lab();
    let id = v.create_file(ROOT, "sparse").unwrap();
    let h = v.open_file(id).unwrap();
    v.write(h, (BLOCK_SIZE - 3) as u64, b"cross boundary").unwrap();
    let bytes = v.read(h, 0, BLOCK_SIZE + 100).unwrap();
    assert!(bytes[..BLOCK_SIZE-3].iter().all(|b| *b == 0));
    assert_eq!(&bytes[BLOCK_SIZE-3..], b"cross boundary");
    assert_eq!(v.read(h, 999_999, 10).unwrap(), Vec::<u8>::new());
    v.verify().unwrap();
}

#[test]
fn shrink_then_extend_never_reveals_old_tail() {
    let (_scratch, mut v) = lab();
    let id = v.create_file(ROOT, "truncate").unwrap();
    let h = v.open_file(id).unwrap();
    v.write(h, 0, &vec![9; BLOCK_SIZE * 3]).unwrap();
    v.truncate(h, 19).unwrap();
    v.truncate(h, (BLOCK_SIZE * 3) as u64).unwrap();
    let out = v.read(h, 0, BLOCK_SIZE * 3).unwrap();
    assert_eq!(&out[..19], &[9; 19]);
    assert!(out[19..].iter().all(|x| *x == 0));
    v.truncate(h, 0).unwrap();
    assert_eq!(v.stats().unwrap().tracked_segment_bytes, 0);
}

#[test]
fn hard_links_share_identity_data_and_link_count() {
    let (_scratch, mut v) = lab();
    let id = v.create_file(ROOT, "one").unwrap();
    let a = v.open_file(id).unwrap();
    v.write(a, 0, b"before").unwrap();
    v.hard_link(id, ROOT, "two").unwrap();
    assert_eq!(v.lookup(ROOT, "two").unwrap(), id);
    assert_eq!(v.stat(id).unwrap().links, 2);
    let b = v.open_file(v.lookup(ROOT, "two").unwrap()).unwrap();
    v.write(b, 0, b"AFTER!").unwrap();
    assert_eq!(v.read(a, 0, 100).unwrap(), b"AFTER!");
    v.unlink(ROOT, "one").unwrap();
    assert_eq!(v.stat(id).unwrap().links, 1);
    v.close_file(a).unwrap();
    v.close_file(b).unwrap();
    assert!(v.stats().unwrap().tracked_segment_bytes > 0);
    v.unlink(ROOT, "two").unwrap();
    assert_eq!(v.stats().unwrap().tracked_segment_bytes, 0);
}

#[test]
fn unlinked_open_object_survives_until_last_close() {
    let (_scratch, mut v) = lab();
    let id = v.create_file(ROOT, "active").unwrap();
    let a = v.open_file(id).unwrap();
    let b = v.open_file(id).unwrap();
    v.write(a, 0, b"still open").unwrap();
    v.unlink(ROOT, "active").unwrap();
    assert!(matches!(v.lookup(ROOT, "active"), Err(Error::NotFound)));
    assert_eq!(v.stat(id).unwrap().links, 0);
    v.close_file(a).unwrap();
    assert_eq!(v.read(b, 0, 100).unwrap(), b"still open");
    v.write(b, 0, b"STILL").unwrap();
    v.close_file(b).unwrap();
    assert_eq!(v.stats().unwrap().tracked_segment_bytes, 0);
    assert!(matches!(v.stat(id), Err(Error::NotFound)));
}

#[test]
fn read_lease_pins_old_segment_after_overwrite_and_unlink() {
    let (_scratch, mut v) = lab();
    let id = v.create_file(ROOT, "leased").unwrap();
    let h = v.open_file(id).unwrap();
    v.write(h, 0, b"old data").unwrap();
    let view = v.acquire_view(h).unwrap();
    v.write(h, 0, b"new data").unwrap();
    assert_eq!(v.read(h, 0, 100).unwrap(), b"new data");
    assert_eq!(v.read_view(view, 0, 100).unwrap(), b"old data");
    v.unlink(ROOT, "leased").unwrap();
    v.close_file(h).unwrap();
    assert!(v.stats().unwrap().tracked_segment_bytes > 0);
    assert_eq!(v.read_view(view, 0, 100).unwrap(), b"old data");
    v.release_view(view).unwrap();
    assert_eq!(v.stats().unwrap().tracked_segment_bytes, 0);
    assert!(matches!(v.read_view(view, 0, 1), Err(Error::InvalidHandle)));
}

#[test]
fn replacement_preserves_destination_open_handle() {
    let (_scratch, mut v) = lab();
    let old = v.create_file(ROOT, "document").unwrap();
    let next = v.create_file(ROOT, "document.next").unwrap();
    let h_old = v.open_file(old).unwrap();
    let h_new = v.open_file(next).unwrap();
    v.write(h_old, 0, b"old version").unwrap();
    v.write(h_new, 0, b"new version").unwrap();
    v.rename(ROOT, "document.next", ROOT, "document", true).unwrap();
    assert_eq!(v.lookup(ROOT, "document").unwrap(), next);
    assert_eq!(v.read(h_old, 0, 100).unwrap(), b"old version");
    assert_eq!(v.read(h_new, 0, 100).unwrap(), b"new version");
    assert_eq!(v.stat(old).unwrap().links, 0);
    v.close_file(h_old).unwrap();
    assert!(matches!(v.stat(old), Err(Error::NotFound)));
    v.verify().unwrap();
}

#[test]
fn no_replace_is_non_destructive_and_same_inode_rename_is_noop() {
    let (_scratch, mut v) = lab();
    let a = v.create_file(ROOT, "a").unwrap();
    let b = v.create_file(ROOT, "b").unwrap();
    assert!(matches!(v.rename(ROOT, "a", ROOT, "b", false), Err(Error::Exists)));
    assert_eq!(v.lookup(ROOT, "a").unwrap(), a);
    assert_eq!(v.lookup(ROOT, "b").unwrap(), b);
    v.hard_link(a, ROOT, "alias").unwrap();
    v.rename(ROOT, "a", ROOT, "alias", true).unwrap();
    assert_eq!(v.lookup(ROOT, "a").unwrap(), a);
    assert_eq!(v.stat(a).unwrap().links, 2);
}

#[test]
fn unicode_case_preservation_and_canonical_collisions() {
    let (_scratch, mut v) = lab();
    let id = v.create_file(ROOT, "Café.txt").unwrap();
    assert_eq!(v.lookup(ROOT, "CAFE\u{301}.TXT").unwrap(), id);
    assert!(matches!(v.create_file(ROOT, "Cafe\u{301}.txt"), Err(Error::Exists)));
    v.rename(ROOT, "café.txt", ROOT, "CAFÉ.TXT", false).unwrap();
    assert_eq!(v.list(ROOT).unwrap()[0].name, "CAFÉ.TXT");
    v.create_file(ROOT, "计划-😀.txt").unwrap();
}

#[test]
fn invalid_names_do_not_poison_or_modify_namespace() {
    let (_scratch, mut v) = lab();
    for name in ["", ".", "..", "a/b", "a\\b", "a:b", "tail.", "tail ", "NUL.txt", "COM1", "LPT²", "a\0b"] {
        assert!(matches!(v.create_file(ROOT, name), Err(Error::InvalidName)), "{name:?}");
    }
    assert!(v.list(ROOT).unwrap().is_empty());
    assert!(!v.is_write_protected());
}

#[test]
fn directory_cycles_type_conflicts_and_nonempty_removal() {
    let (_scratch, mut v) = lab();
    let a = v.mkdir(ROOT, "a").unwrap();
    let b = v.mkdir(a, "b").unwrap();
    assert!(matches!(v.rename(ROOT, "a", b, "cycle", true), Err(Error::Cycle)));
    assert!(matches!(v.rmdir(ROOT, "a"), Err(Error::NotEmpty)));
    assert!(matches!(v.hard_link(a, ROOT, "bad"), Err(Error::IsDirectory)));
    v.create_file(ROOT, "file").unwrap();
    assert!(matches!(v.rename(ROOT, "file", ROOT, "a", true), Err(Error::IsDirectory)));
    assert!(matches!(v.rename(ROOT, "a", ROOT, "file", true), Err(Error::NotDirectory)));
    v.rmdir(a, "b").unwrap();
    v.rmdir(ROOT, "a").unwrap();
    v.verify().unwrap();
}

#[test]
fn directory_move_and_empty_directory_replacement() {
    let (_scratch, mut v) = lab();
    let a = v.mkdir(ROOT, "a").unwrap();
    let empty = v.mkdir(ROOT, "empty").unwrap();
    let child = v.create_file(a, "child").unwrap();
    v.rename(ROOT, "a", ROOT, "empty", true).unwrap();
    assert_eq!(v.lookup(ROOT, "empty").unwrap(), a);
    assert_eq!(v.lookup(a, "child").unwrap(), child);
    assert!(matches!(v.stat(empty), Err(Error::NotFound)));
    v.verify().unwrap();
}

#[test]
fn exclusive_owner_same_process_and_reopen() {
    let (scratch, v) = lab();
    assert!(matches!(Volume::open(&scratch.0), Err(Error::Busy)));
    drop(v);
    Volume::open(&scratch.0).unwrap().verify().unwrap();
}

#[test]
fn quota_counts_objects_not_links_and_retains_open_unlinked_bytes() {
    let scratch = Scratch::new();
    let mut v = Volume::create(&scratch.0, 8).unwrap();
    let a = v.create_file(ROOT, "a").unwrap();
    let h = v.open_file(a).unwrap();
    v.write(h, 0, b"12345678").unwrap();
    v.hard_link(a, ROOT, "alias").unwrap();
    assert_eq!(v.stats().unwrap().logical_bytes, 8);
    let b = v.create_file(ROOT, "b").unwrap();
    let hb = v.open_file(b).unwrap();
    assert!(matches!(v.write(hb, 0, b"x"), Err(Error::Quota)));
    v.unlink(ROOT, "a").unwrap();
    v.unlink(ROOT, "alias").unwrap();
    assert!(matches!(v.write(hb, 0, b"x"), Err(Error::Quota)));
    v.close_file(h).unwrap();
    v.write(hb, 0, b"x").unwrap();
    assert_eq!(v.stats().unwrap().logical_bytes, 1);
}

#[test]
fn create_never_overwrites_existing_directory() {
    let scratch = Scratch::new();
    fs::create_dir(&scratch.0).unwrap();
    fs::write(scratch.0.join("keep"), b"untouched").unwrap();
    assert!(Volume::create(&scratch.0, 100).is_err());
    assert_eq!(fs::read(scratch.0.join("keep")).unwrap(), b"untouched");
    assert_eq!(fs::read_dir(&scratch.0).unwrap().count(), 1);
}

#[test]
fn unknown_format_refused_without_reformatting() {
    let (scratch, v) = lab();
    drop(v);
    let db = fs::read(scratch.0.join("catalog.sqlite")).unwrap();
    fs::write(scratch.0.join("format.txt"), b"future-format").unwrap();
    assert!(matches!(Volume::open(&scratch.0), Err(Error::UnsupportedFormat)));
    assert_eq!(fs::read(scratch.0.join("catalog.sqlite")).unwrap(), db);
}

#[test]
fn checksum_error_protects_writes_and_preserves_evidence() {
    let (scratch, mut v) = lab();
    let id = v.create_file(ROOT, "corrupt-me").unwrap();
    let h = v.open_file(id).unwrap();
    v.write(h, 0, b"important bytes").unwrap();
    let path = fs::read_dir(scratch.0.join("segments")).unwrap().next().unwrap().unwrap().path();
    let mut raw = fs::OpenOptions::new().read(true).write(true).open(&path).unwrap();
    raw.seek(SeekFrom::Start(24)).unwrap();
    let mut byte = [0]; raw.read_exact(&mut byte).unwrap();
    raw.seek(SeekFrom::Start(24)).unwrap(); raw.write_all(&[byte[0] ^ 1]).unwrap(); raw.sync_all().unwrap();
    drop(raw);
    assert!(matches!(v.read(h, 0, 100), Err(Error::Corrupt(_))));
    assert!(v.is_write_protected());
    assert!(matches!(v.write(h, 0, b"must fail"), Err(Error::WriteProtected)));
    drop(v);
    assert!(Volume::open(&scratch.0).is_err());
    assert!(path.exists());
}

#[test]
fn invalid_range_and_empty_write_are_non_destructive() {
    let (_scratch, mut v) = lab();
    let id = v.create_file(ROOT, "range").unwrap();
    let h = v.open_file(id).unwrap();
    assert!(matches!(v.write(h, i64::MAX as u64, b"x"), Err(Error::InvalidRange)));
    assert!(matches!(v.read(h, 0, MAX_IO + 1), Err(Error::InvalidRange)));
    v.write(h, 10_000, b"").unwrap();
    assert_eq!(v.stat(id).unwrap().size, 0);
    assert_eq!(v.stats().unwrap().tracked_segment_bytes, 0);
    assert!(!v.is_write_protected());
}

#[test]
fn mixed_segment_reclamation_is_automatic() {
    let (_scratch, mut v) = lab();
    let id = v.create_file(ROOT, "mixed").unwrap();
    let h = v.open_file(id).unwrap();
    v.write(h, 0, &vec![1; BLOCK_SIZE * 4]).unwrap();
    v.write(h, 0, &vec![2; BLOCK_SIZE * 3]).unwrap();
    // A manual compact/gc call was NOT made. One surviving old block is moved
    // into a new segment; the 4-block old segment has been retired.
    assert_eq!(v.stats().unwrap().tracked_segment_bytes, (BLOCK_SIZE * 4 + 48) as u64);
    let data = v.read(h, 0, BLOCK_SIZE * 4).unwrap();
    assert!(data[..BLOCK_SIZE*3].iter().all(|b| *b == 2));
    assert!(data[BLOCK_SIZE*3..].iter().all(|b| *b == 1));
    v.verify().unwrap();
}

#[test]
fn mixed_compaction_waits_for_read_lease() {
    let (_scratch, mut v) = lab();
    let id = v.create_file(ROOT, "mixed").unwrap();
    let h = v.open_file(id).unwrap();
    v.write(h, 0, &vec![1; BLOCK_SIZE * 4]).unwrap();
    let view = v.acquire_view(h).unwrap();
    v.write(h, 0, &vec![2; BLOCK_SIZE * 3]).unwrap();
    assert_eq!(v.stats().unwrap().tracked_segment_bytes, (BLOCK_SIZE * 7 + 48) as u64);
    assert!(v.read_view(view, 0, BLOCK_SIZE * 4).unwrap().iter().all(|b| *b == 1));
    v.release_view(view).unwrap();
    assert_eq!(v.stats().unwrap().tracked_segment_bytes, (BLOCK_SIZE * 4 + 48) as u64);
}

#[test]
fn stale_and_cross_volume_handles_are_rejected() {
    let (scratch, mut v) = lab();
    let id = v.create_file(ROOT, "handle").unwrap();
    let h = v.open_file(id).unwrap();
    drop(v);
    let mut again = Volume::open(&scratch.0).unwrap();
    let fresh = again.open_file(id).unwrap();
    assert_ne!(h, fresh);
    assert!(matches!(again.read(h, 0, 1), Err(Error::InvalidHandle)));
    let (_other, mut other) = lab();
    let other_id = other.create_file(ROOT, "other").unwrap();
    other.open_file(other_id).unwrap();
    assert!(matches!(other.write(fresh, 0, b"x"), Err(Error::InvalidHandle)));
    again.close_file(fresh).unwrap();
    assert!(matches!(again.close_file(fresh), Err(Error::InvalidHandle)));
}

#[test]
fn terabyte_logical_hole_does_not_allocate_a_terabyte() {
    let scratch = Scratch::new();
    let size = 1_u64 << 40;
    let mut v = Volume::create(&scratch.0, size * 2).unwrap();
    let id = v.create_file(ROOT, "huge-logical").unwrap();
    let h = v.open_file(id).unwrap();
    v.truncate(h, size).unwrap();
    assert_eq!(v.stats().unwrap().tracked_segment_bytes, 0);
    v.write(h, size - 1, b"z").unwrap();
    assert_eq!(v.read(h, size - 10, 100).unwrap(), [vec![0; 9], b"z".to_vec()].concat());
    assert_eq!(v.stats().unwrap().tracked_segment_bytes, (BLOCK_SIZE + 24) as u64);
    v.truncate(h, 0).unwrap();
    assert_eq!(v.stats().unwrap().tracked_segment_bytes, 0);
}

#[test]
fn model_based_write_truncate_alias_and_reopen() {
    let (scratch, mut v) = lab();
    let id = v.create_file(ROOT, "model").unwrap();
    v.hard_link(id, ROOT, "alias").unwrap();
    let mut h = v.open_file(id).unwrap();
    let mut model = Vec::new();
    let mut seed = 0x91ab_u64;
    for step in 0..240 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let position = (seed % 4096) as usize;
        if step % 4 == 0 {
            v.truncate(h, position as u64).unwrap();
            model.resize(position, 0);
        } else {
            let data = vec![(seed >> 32) as u8; (seed % 79 + 1) as usize];
            v.write(h, position as u64, &data).unwrap();
            if model.len() < position + data.len() { model.resize(position + data.len(), 0); }
            model[position..position + data.len()].copy_from_slice(&data);
        }
        assert_eq!(v.read(h, 0, 8192).unwrap(), model, "step {step}");
        assert_eq!(v.lookup(ROOT, "alias").unwrap(), id);
        if step % 40 == 39 {
            v.close_file(h).unwrap();
            drop(v);
            v = Volume::open(&scratch.0).unwrap();
            h = v.open_file(id).unwrap();
            assert_eq!(v.read(h, 0, 8192).unwrap(), model);
        }
    }
    v.verify().unwrap();
}

#[test]
fn orphan_from_ended_session_is_reclaimed_on_reopen() {
    let (scratch, mut v) = lab();
    let id = v.create_file(ROOT, "unlinked").unwrap();
    let h = v.open_file(id).unwrap();
    v.write(h, 0, b"orphan").unwrap();
    v.unlink(ROOT, "unlinked").unwrap();
    drop(v); // no close_file; session ends, the old handle cannot be reused
    let reopened = Volume::open(&scratch.0).unwrap();
    assert_eq!(reopened.stats().unwrap().tracked_segment_bytes, 0);
    assert_eq!(reopened.stats().unwrap().logical_bytes, 0);
}

#[test]
fn format_and_bundled_sqlite_gate() {
    assert!(rusqlite::version_number() >= 3_051_003);
    println!("bundled SQLite: {} ({})", rusqlite::version(), rusqlite::version_number());
    let (scratch, v) = lab();
    assert_eq!(fs::read_to_string(scratch.0.join("format.txt")).unwrap(), FORMAT);
    v.verify().unwrap();
}

#[test]
fn cross_process_owner_lock() {
    let (scratch, _v) = lab();
    let status = Command::new(std::env::current_exe().unwrap())
        .args(["--ignored", "--exact", "child_helper", "--nocapture"])
        .env("PW_TEST_ROOT", &scratch.0).env("PW_TEST_ACTION", "lock").status().unwrap();
    assert!(status.success());
}

#[test]
#[ignore = "spawned only by parent tests with a disposable fixture"]
fn child_helper() {
    let path = std::env::var_os("PW_TEST_ROOT").expect("parent-owned fixture required");
    let action = std::env::var("PW_TEST_ACTION").unwrap();
    if action == "lock" {
        assert!(matches!(Volume::open(path), Err(Error::Busy)));
        return;
    }
    #[cfg(feature = "fault-injection")]
    {
        let mut v = Volume::open(path).unwrap();
        let h = v.open_file(v.lookup(ROOT, "crash").unwrap()).unwrap();
        let point = match action.as_str() { "data" => FaultPoint::AfterDataSync, "before" => FaultPoint::BeforeCommit,
            "after" => FaultPoint::AfterCommit, _ => panic!("bad action") };
        v.inject_once(point);
        assert!(matches!(v.write(h, 0, b"NEW-BYTES"), Err(Error::Injected)));
        // Exit without running Rust destructors or SQLite close/checkpoint.
        std::process::exit(86);
    }
    #[cfg(not(feature = "fault-injection"))]
    panic!("fault-injection feature required");
}

#[cfg(feature = "fault-injection")]
#[test]
fn process_exit_at_data_and_commit_boundaries_recovers_expected_version() {
    for action in ["data", "before", "after"] {
        let (scratch, mut v) = lab();
        let id = v.create_file(ROOT, "crash").unwrap();
        let h = v.open_file(id).unwrap();
        v.write(h, 0, b"OLD-BYTES").unwrap();
        drop(v);
        let status = Command::new(std::env::current_exe().unwrap())
            .args(["--ignored", "--exact", "child_helper", "--nocapture"])
            .env("PW_TEST_ROOT", &scratch.0).env("PW_TEST_ACTION", action).status().unwrap();
        assert_eq!(status.code(), Some(86), "{action}");
        let mut reopened = Volume::open(&scratch.0).unwrap();
        let h = reopened.open_file(id).unwrap();
        let expected: &[u8] = if action == "after" { b"NEW-BYTES" } else { b"OLD-BYTES" };
        assert_eq!(reopened.read(h, 0, 100).unwrap(), expected, "{action}");
        assert_eq!(reopened.stats().unwrap().segments, 1);
        reopened.verify().unwrap();
    }
}

#[cfg(feature = "fault-injection")]
#[test]
fn failed_compaction_keeps_committed_data_and_retries_after_reopen() {
    let (scratch, mut v) = lab();
    let id = v.create_file(ROOT, "compact-crash").unwrap();
    let h = v.open_file(id).unwrap();
    v.write(h, 0, &vec![1; BLOCK_SIZE*4]).unwrap();
    v.inject_once(FaultPoint::BeforeCompactionCommit);
    // User write commits, maintenance fails. Err is not a rollback promise.
    assert!(matches!(v.write(h, 0, &vec![2; BLOCK_SIZE*3]), Err(Error::Injected)));
    assert!(v.is_write_protected());
    let expected = [vec![2; BLOCK_SIZE*3], vec![1; BLOCK_SIZE]].concat();
    assert_eq!(v.read(h, 0, BLOCK_SIZE*4).unwrap(), expected);
    drop(v);
    let mut reopened = Volume::open(&scratch.0).unwrap();
    let h = reopened.open_file(id).unwrap();
    assert_eq!(reopened.read(h, 0, BLOCK_SIZE*4).unwrap(), expected);
    assert_eq!(reopened.stats().unwrap().tracked_segment_bytes, (BLOCK_SIZE*4+48) as u64);
    reopened.verify().unwrap();
}
