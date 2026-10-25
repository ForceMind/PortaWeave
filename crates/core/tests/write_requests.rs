use portaweave_core::{
    CreateAttributes, Credentials, Error, FileAccess, MAX_IO, MetadataUpdate, ROOT, Volume,
    WritePosition,
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
        let path = std::env::temp_dir().join(format!(
            "pw-write-request-{}-{stamp}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn volume(&self, quota: u64) -> Volume {
        Volume::create_with_metadata(
            self.0.join("volume"),
            quota,
            CreateAttributes {
                uid: 100,
                gid: 10,
                mode: 0o777,
            },
        )
        .unwrap()
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn caller() -> Credentials {
    Credentials::new(100, 10, &[]).unwrap()
}
fn file(v: &mut Volume) -> portaweave_core::PosixFile {
    let mut p = v.posix(caller()).unwrap();
    p.create_file(ROOT, "file", 0o600, 0).unwrap();
    p.open_file(ROOT, "file", FileAccess::ReadWrite).unwrap()
}

#[test]
fn append_constrained_and_truncate_reply_match_committed_state_and_old_view() {
    let s = Scratch::new();
    let mut v = s.volume(1 << 24);
    let f = file(&mut v);
    let first = f
        .write_request(&mut v, WritePosition::EndOfFile, b"abcdef", false)
        .unwrap();
    assert_eq!(
        (first.offset, first.written, first.info.stat.size),
        (0, 6, 6)
    );
    assert_eq!(first.info, f.info(&v).unwrap());
    let view = f.acquire_range_view(&mut v, 0, 6).unwrap();
    let second = f
        .write_request(&mut v, WritePosition::EndOfFile, b"ghi", false)
        .unwrap();
    assert_eq!(
        (second.offset, second.written, second.info.stat.size),
        (6, 3, 9)
    );
    assert_eq!(second.info.stat.generation, first.info.stat.generation + 1);
    let third = f
        .write_request(&mut v, WritePosition::Offset(7), b"XYZ", true)
        .unwrap();
    assert_eq!((third.written, third.info.stat.size), (2, 9));
    assert_eq!(third.info, f.info(&v).unwrap());
    assert_eq!(f.read(&v, 0, 20).unwrap(), b"abcdefgXY");
    let smaller = f.truncate_request(&mut v, 3).unwrap();
    assert_eq!(smaller, f.info(&v).unwrap());
    assert_eq!(
        (smaller.stat.size, smaller.stat.generation),
        (3, third.info.stat.generation + 1)
    );
    let bigger = f.truncate_request(&mut v, 7).unwrap();
    assert_eq!(bigger, f.info(&v).unwrap());
    assert_eq!(f.read(&v, 0, 20).unwrap(), b"abc\0\0\0\0");
    assert_eq!(view.read(&v, 0, 6).unwrap(), b"abcdef");
    view.close(&mut v).unwrap();
    f.close(&mut v).unwrap();
    drop(v);
    let mut reopened = Volume::open(s.0.join("volume")).unwrap();
    let f = reopened
        .posix(caller())
        .unwrap()
        .open_file(ROOT, "file", FileAccess::Read)
        .unwrap();
    assert_eq!(f.info(&reopened).unwrap(), bigger);
    assert_eq!(f.read(&reopened, 0, 20).unwrap(), b"abc\0\0\0\0");
    f.close(&mut reopened).unwrap();
}

#[test]
fn zero_and_clipped_writes_preserve_generation_times_setid_and_full_quota() {
    let s = Scratch::new();
    let mut v = s.volume(4);
    let f = file(&mut v);
    f.write(&mut v, 0, b"full").unwrap();
    let id = f.stat(&v).unwrap().id;
    v.set_metadata(
        id,
        MetadataUpdate {
            mode: Some(0o6751),
            ..Default::default()
        },
    )
    .unwrap();
    let old = f.info(&v).unwrap();
    for (position, data, constrained) in [
        (WritePosition::EndOfFile, &b"ignored"[..], true),
        (WritePosition::Offset(4), &b"ignored"[..], true),
        (
            WritePosition::Offset(i64::MAX as u64),
            &b"ignored"[..],
            true,
        ),
        (WritePosition::EndOfFile, &b""[..], false),
    ] {
        let result = f
            .write_request(&mut v, position, data, constrained)
            .unwrap();
        assert_eq!(result.written, 0);
        assert_eq!(result.info, old);
    }
    assert!(matches!(
        f.write_request(&mut v, WritePosition::EndOfFile, b"!", false),
        Err(Error::Quota)
    ));
    assert_eq!(f.info(&v).unwrap(), old);
    let changed = f
        .write_request(&mut v, WritePosition::Offset(3), b"OK", true)
        .unwrap();
    assert_eq!(changed.written, 1);
    assert_eq!(changed.info.metadata.mode, 0o751);
    assert_eq!(f.read(&v, 0, 9).unwrap(), b"fulO");
    assert!(!v.is_write_protected());
    f.close(&mut v).unwrap();
}

#[test]
fn current_eof_tracks_other_handles_and_unlinked_inode_without_new_path_authority() {
    let s = Scratch::new();
    let mut v = s.volume(1 << 24);
    let f = file(&mut v);
    let id = f.stat(&v).unwrap().id;
    v.hard_link(id, ROOT, "alias").unwrap();
    let other = v
        .posix(caller())
        .unwrap()
        .open_file(ROOT, "alias", FileAccess::Write)
        .unwrap();
    f.write_request(&mut v, WritePosition::EndOfFile, b"a", false)
        .unwrap();
    other
        .write_request(&mut v, WritePosition::EndOfFile, b"bc", false)
        .unwrap();
    other.truncate_request(&mut v, 1).unwrap();
    v.set_metadata(
        id,
        MetadataUpdate {
            mode: Some(0),
            uid: Some(200),
            ..Default::default()
        },
    )
    .unwrap();
    v.unlink(ROOT, "file").unwrap();
    v.unlink(ROOT, "alias").unwrap();
    let result = f
        .write_request(&mut v, WritePosition::EndOfFile, b"d", false)
        .unwrap();
    assert_eq!(
        (result.offset, result.info.stat.links, result.info.stat.id),
        (1, 0, id)
    );
    assert_eq!(result.info.metadata.uid, 200);
    assert_eq!(f.read(&v, 0, 20).unwrap(), b"ad");
    other.close(&mut v).unwrap();
    f.close(&mut v).unwrap();
    assert!(matches!(v.stat(id), Err(Error::NotFound)));
}

#[test]
fn denied_stale_foreign_oversize_and_overflow_requests_do_not_mutate() {
    let s = Scratch::new();
    let mut v = s.volume(i64::MAX as u64);
    let f = file(&mut v);
    let reader = v
        .posix(caller())
        .unwrap()
        .open_file(ROOT, "file", FileAccess::Read)
        .unwrap();
    let old = f.info(&v).unwrap();
    assert!(matches!(
        reader.write_request(&mut v, WritePosition::EndOfFile, b"", true),
        Err(Error::PermissionDenied)
    ));
    assert!(matches!(
        reader.truncate_request(&mut v, 0),
        Err(Error::PermissionDenied)
    ));
    assert!(matches!(
        f.write_request(&mut v, WritePosition::Offset(u64::MAX), b"", true),
        Err(Error::InvalidRange)
    ));
    assert!(matches!(
        f.write_request(&mut v, WritePosition::Offset(i64::MAX as u64), b"x", false),
        Err(Error::InvalidRange)
    ));
    assert!(matches!(
        f.write_request(&mut v, WritePosition::EndOfFile, &vec![0; MAX_IO + 1], true),
        Err(Error::InvalidRange)
    ));
    assert_eq!(f.info(&v).unwrap(), old);
    assert!(!v.is_write_protected());
    f.truncate_request(&mut v, i64::MAX as u64).unwrap();
    let huge = f.info(&v).unwrap();
    assert!(matches!(
        f.write_request(&mut v, WritePosition::EndOfFile, b"x", false),
        Err(Error::InvalidRange)
    ));
    let constrained = f
        .write_request(
            &mut v,
            WritePosition::Offset(i64::MAX as u64 - 1),
            b"xy",
            true,
        )
        .unwrap();
    assert_eq!(constrained.written, 1);
    assert_eq!(constrained.info.stat.size, i64::MAX as u64);
    assert_eq!(f.read(&v, i64::MAX as u64 - 1, 1).unwrap(), b"x");
    assert_eq!(constrained.info.stat.generation, huge.stat.generation + 1);
    let another = Scratch::new();
    let mut other = another.volume(1024);
    assert!(matches!(
        f.write_request(&mut other, WritePosition::EndOfFile, b"x", false),
        Err(Error::InvalidHandle)
    ));
    reader.close(&mut v).unwrap();
    f.close(&mut v).unwrap();
    assert!(matches!(f.info(&v), Err(Error::InvalidHandle)));
}

#[test]
fn seeded_write_request_model_covers_all_modes_and_shared_handles() {
    let s = Scratch::new();
    let mut v = s.volume(1 << 24);
    let f = file(&mut v);
    let second = v
        .posix(caller())
        .unwrap()
        .open_file(ROOT, "file", FileAccess::ReadWrite)
        .unwrap();
    let mut model = Vec::new();
    let mut seed = 57_u64;
    for step in 0..384 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let append = seed & 1 != 0;
        let constrained = seed & 2 != 0;
        let offset = if append {
            model.len()
        } else {
            ((seed >> 8) % 1024) as usize
        };
        let data = vec![(step % 251) as u8; ((seed >> 20) % 31) as usize];
        let expected = if constrained {
            data.len().min(model.len().saturating_sub(offset))
        } else {
            data.len()
        };
        let file = if step % 2 == 0 { f } else { second };
        let before = file.info(&v).unwrap();
        let result = file
            .write_request(
                &mut v,
                if append {
                    WritePosition::EndOfFile
                } else {
                    WritePosition::Offset(offset as u64)
                },
                &data,
                constrained,
            )
            .unwrap();
        if expected != 0 {
            model.resize(model.len().max(offset + expected), 0);
            model[offset..offset + expected].copy_from_slice(&data[..expected]);
        }
        assert_eq!((result.offset, result.written), (offset as u64, expected));
        assert_eq!(
            result.info.stat.generation,
            before.stat.generation + u64::from(expected != 0)
        );
        if expected == 0 {
            assert_eq!(result.info, before);
        }
        assert_eq!(result.info, f.info(&v).unwrap());
        assert_eq!(f.read(&v, 0, 4096).unwrap(), model);
    }
    f.close(&mut v).unwrap();
    second.close(&mut v).unwrap();
}

#[cfg(feature = "fault-injection")]
#[test]
fn before_commit_write_and_truncate_roll_back_content_metadata_and_generation() {
    use portaweave_core::FaultPoint;
    for truncate in [false, true] {
        let s = Scratch::new();
        let mut v = s.volume(1 << 24);
        let f = file(&mut v);
        f.write(&mut v, 0, b"original").unwrap();
        let old = f.info(&v).unwrap();
        v.inject_once(FaultPoint::BeforeCommit);
        if truncate {
            assert!(matches!(
                f.truncate_request(&mut v, 3),
                Err(Error::Injected)
            ));
        } else {
            assert!(matches!(
                f.write_request(&mut v, WritePosition::EndOfFile, b"new", false),
                Err(Error::Injected)
            ));
        }
        assert_eq!(f.info(&v).unwrap(), old);
        assert_eq!(f.read(&v, 0, 20).unwrap(), b"original");
        f.close(&mut v).unwrap();
        drop(v);
        let mut v = Volume::open(s.0.join("volume")).unwrap();
        let f = v
            .posix(caller())
            .unwrap()
            .open_file(ROOT, "file", FileAccess::Read)
            .unwrap();
        assert_eq!(f.info(&v).unwrap(), old);
        f.close(&mut v).unwrap();
    }
}

#[cfg(feature = "fault-injection")]
#[test]
fn after_commit_returns_complete_reply_and_separate_protection_warning() {
    use portaweave_core::{FaultPoint, MaintenanceStatus};
    for truncate in [false, true] {
        let s = Scratch::new();
        let mut v = s.volume(1 << 24);
        let f = file(&mut v);
        f.write(&mut v, 0, b"original").unwrap();
        let old = f.info(&v).unwrap();
        v.inject_once(FaultPoint::AfterCommit);
        let info = if truncate {
            f.truncate_request(&mut v, 3).unwrap()
        } else {
            f.write_request(&mut v, WritePosition::EndOfFile, b"new", false)
                .unwrap()
                .info
        };
        assert_eq!(info, f.info(&v).unwrap());
        assert_eq!(info.stat.generation, old.stat.generation + 1);
        assert_eq!(info.stat.size, if truncate { 3 } else { 11 });
        assert!(v.is_write_protected());
        assert!(matches!(
            v.maintenance_status(),
            MaintenanceStatus::Failed { .. }
        ));
        assert!(matches!(
            f.write_request(&mut v, WritePosition::EndOfFile, b"x", false),
            Err(Error::WriteProtected)
        ));
        f.close(&mut v).unwrap();
        drop(v);
        let mut v = Volume::open(s.0.join("volume")).unwrap();
        let f = v
            .posix(caller())
            .unwrap()
            .open_file(ROOT, "file", FileAccess::Read)
            .unwrap();
        assert_eq!(f.info(&v).unwrap(), info);
        f.close(&mut v).unwrap();
    }
}
