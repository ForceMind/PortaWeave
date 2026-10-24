use portaweave_core::{
    CreateAttributes, Error, ExportLinks, FORMAT, METADATA_FORMAT, MetadataUpdate, ROOT, Recovery,
    RecoveryLimits, Timestamp, Volume,
};
use rusqlite::Connection;
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
const ATTR: CreateAttributes = CreateAttributes {
    uid: 501,
    gid: 20,
    mode: 0o750,
};
const OLD: Timestamp = Timestamp {
    seconds: -123,
    nanoseconds: 999_999_999,
};
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let p = std::env::temp_dir().join(format!(
            "pw-metadata-{}-{stamp}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&p).unwrap();
        Self(p)
    }
    fn path(&self) -> PathBuf {
        self.0.join("volume")
    }
    fn create(&self) -> Volume {
        Volume::create_with_metadata(self.path(), 1 << 26, ATTR).unwrap()
    }
    fn files(&self) -> Vec<(PathBuf, Vec<u8>)> {
        fn visit(path: &std::path::Path, out: &mut Vec<(PathBuf, Vec<u8>)>) {
            for entry in fs::read_dir(path).unwrap() {
                let p = entry.unwrap().path();
                if p.is_dir() {
                    visit(&p, out);
                } else {
                    out.push((p.clone(), fs::read(p).unwrap()));
                }
            }
        }
        let mut out = Vec::new();
        visit(&self.path(), &mut out);
        out.sort();
        out
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn old_times(v: &mut Volume, id: portaweave_core::FileId) {
    v.set_metadata(
        id,
        MetadataUpdate {
            created: Some(OLD),
            accessed: Some(OLD),
            modified: Some(OLD),
            ..Default::default()
        },
    )
    .unwrap();
}

#[test]
fn v1_remains_v1_and_metadata_requests_do_not_rewrite_or_invent_fields() {
    let s = Scratch::new();
    let mut v = Volume::create(s.path(), 1 << 20).unwrap();
    let id = v.create_file(ROOT, "v1").unwrap();
    assert!(matches!(
        v.volume_identity(),
        Err(Error::UnsupportedOperation)
    ));
    assert!(matches!(v.metadata(id), Err(Error::UnsupportedOperation)));
    assert!(matches!(
        v.set_metadata(id, MetadataUpdate::default()),
        Err(Error::UnsupportedOperation)
    ));
    assert!(matches!(
        v.create_file_with_attributes(ROOT, "absent", ATTR),
        Err(Error::UnsupportedOperation)
    ));
    assert!(matches!(v.lookup(ROOT, "absent"), Err(Error::NotFound)));
    assert!(!v.is_write_protected());
    drop(v);
    assert_eq!(
        fs::read(s.path().join("format.txt")).unwrap(),
        FORMAT.as_bytes()
    );
    let db = Connection::open(s.path().join("catalog.sqlite")).unwrap();
    assert!(
        !db.prepare("SELECT 1 FROM sqlite_schema WHERE name='object_metadata'")
            .unwrap()
            .exists([])
            .unwrap()
    );
    drop(db);
    let mut v = Volume::open(s.path()).unwrap();
    let h = v.open_file(id).unwrap();
    v.write(h, 0, b"still writable v1").unwrap();
    v.close_file(h).unwrap();
    v.verify().unwrap();
}

#[test]
fn identity_and_explicit_ownership_survive_reopen_and_differ_for_new_volumes() {
    let s = Scratch::new();
    let mut v = s.create();
    let identity = v.volume_identity().unwrap();
    assert_eq!(identity.uuid[6] >> 4, 4);
    assert_eq!(identity.uuid[8] >> 6, 2);
    assert_eq!(identity.capabilities, 1);
    let root = v.metadata(ROOT).unwrap();
    assert_eq!((root.uid, root.gid, root.mode), (501, 20, 0o750));
    let id = v
        .create_file_with_attributes(
            ROOT,
            "tool",
            CreateAttributes {
                uid: u32::MAX,
                gid: u32::MAX,
                mode: 0o6751,
            },
        )
        .unwrap();
    let dir = v.mkdir(ROOT, "private").unwrap();
    let file = v.create_file(dir, "default").unwrap();
    assert_eq!(v.metadata(dir).unwrap().mode, 0o700);
    assert_eq!(v.metadata(file).unwrap().mode, 0o600);
    assert_eq!(v.metadata(file).unwrap().uid, 501);
    let meta = v.metadata(id).unwrap();
    drop(v);
    let v = Volume::open(s.path()).unwrap();
    assert_eq!(v.volume_identity().unwrap(), identity);
    assert_eq!(v.metadata(id).unwrap(), meta);
    v.verify().unwrap();
    drop(v);
    assert_eq!(
        fs::read(s.path().join("format.txt")).unwrap(),
        METADATA_FORMAT.as_bytes()
    );
    let other = Volume::create_with_metadata(s.0.join("second"), 100, ATTR).unwrap();
    assert_ne!(other.volume_identity().unwrap().uuid, identity.uuid);
}

#[test]
fn invalid_attributes_are_rejected_before_creation_or_mutation_without_protection() {
    let s = Scratch::new();
    assert!(matches!(
        Volume::create_with_metadata(
            s.path(),
            100,
            CreateAttributes {
                mode: 0o10000,
                ..ATTR
            }
        ),
        Err(Error::InvalidRange)
    ));
    assert!(!s.path().exists());
    let mut v = s.create();
    let id = v.create_file(ROOT, "file").unwrap();
    let old = v.metadata(id).unwrap();
    let stat = v.stat(id).unwrap();
    for update in [
        MetadataUpdate {
            mode: Some(0o10000),
            ..Default::default()
        },
        MetadataUpdate {
            accessed: Some(Timestamp {
                seconds: 0,
                nanoseconds: 1_000_000_000,
            }),
            ..Default::default()
        },
    ] {
        assert!(matches!(
            v.set_metadata(id, update),
            Err(Error::InvalidRange)
        ));
        assert_eq!(v.metadata(id).unwrap(), old);
        assert_eq!(v.stat(id).unwrap(), stat);
    }
    assert!(matches!(
        v.mkdir_with_attributes(
            ROOT,
            "absent",
            CreateAttributes {
                mode: 0o10000,
                ..ATTR
            }
        ),
        Err(Error::InvalidRange)
    ));
    assert!(matches!(v.lookup(ROOT, "absent"), Err(Error::NotFound)));
    v.set_metadata(id, MetadataUpdate::default()).unwrap();
    assert_eq!(v.metadata(id).unwrap(), old);
    assert!(!v.is_write_protected());
}

#[test]
fn metadata_changes_preserve_nanoseconds_share_alias_identity_and_clear_file_setid() {
    let s = Scratch::new();
    let mut v = s.create();
    let id = v.create_file(ROOT, "file").unwrap();
    old_times(&mut v, id);
    v.set_metadata(
        id,
        MetadataUpdate {
            mode: Some(0o6751),
            ..Default::default()
        },
    )
    .unwrap();
    v.hard_link(id, ROOT, "alias").unwrap();
    let alias = v.lookup(ROOT, "ALIAS").unwrap();
    assert_eq!(alias, id);
    v.set_metadata(
        alias,
        MetadataUpdate {
            uid: Some(502),
            gid: Some(80),
            ..Default::default()
        },
    )
    .unwrap();
    let m = v.metadata(id).unwrap();
    assert_eq!((m.uid, m.gid, m.mode), (502, 80, 0o751));
    assert_eq!(m.created, OLD);
    assert_eq!(m.accessed, OLD);
    assert_eq!(m.modified, OLD);
    assert_ne!(m.changed, OLD);
    let dir = v
        .mkdir_with_attributes(
            ROOT,
            "group",
            CreateAttributes {
                mode: 0o2770,
                ..ATTR
            },
        )
        .unwrap();
    v.set_metadata(
        dir,
        MetadataUpdate {
            gid: Some(80),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(v.metadata(dir).unwrap().mode, 0o2770);
    drop(v);
    let v = Volume::open(s.path()).unwrap();
    assert_eq!(v.metadata(id).unwrap(), m);
    v.verify().unwrap();
}

#[test]
fn content_changes_update_times_and_strip_setid_without_erasing_execute_or_atime() {
    let s = Scratch::new();
    let mut v = s.create();
    let id = v.create_file(ROOT, "tool").unwrap();
    let h = v.open_file(id).unwrap();
    old_times(&mut v, id);
    v.set_metadata(
        id,
        MetadataUpdate {
            mode: Some(0o6751),
            ..Default::default()
        },
    )
    .unwrap();
    v.write(h, 0, b"data").unwrap();
    let m = v.metadata(id).unwrap();
    assert_eq!(m.mode, 0o751);
    assert_ne!(m.modified, OLD);
    assert_eq!(m.created, OLD);
    assert_eq!(m.accessed, OLD);
    let same = m;
    v.write(h, 0, b"").unwrap();
    v.read(h, 0, 100).unwrap();
    assert_eq!(v.metadata(id).unwrap(), same);
    old_times(&mut v, id);
    v.set_metadata(
        id,
        MetadataUpdate {
            mode: Some(0o6751),
            ..Default::default()
        },
    )
    .unwrap();
    v.truncate(h, 1).unwrap();
    let m = v.metadata(id).unwrap();
    assert_eq!(m.mode, 0o751);
    assert_ne!(m.modified, OLD);
    assert_eq!(m.created, OLD);
    assert_eq!(m.accessed, OLD);
    v.close_file(h).unwrap();
    drop(v);
    let v = Volume::open(s.path()).unwrap();
    assert_eq!(v.metadata(id).unwrap(), m);
}

#[test]
fn namespace_changes_touch_parents_and_keep_replaced_open_inode_metadata() {
    let s = Scratch::new();
    let mut v = s.create();
    let p = v.mkdir(ROOT, "parent").unwrap();
    let q = v.mkdir(ROOT, "other").unwrap();
    old_times(&mut v, p);
    let a = v.create_file(p, "file").unwrap();
    assert_ne!(v.metadata(p).unwrap().modified, OLD);
    old_times(&mut v, a);
    let h = v.open_file(a).unwrap();
    v.write(h, 0, b"old inode").unwrap();
    let old = v.metadata(a).unwrap();
    let b = v
        .create_file_with_attributes(
            q,
            "new",
            CreateAttributes {
                mode: 0o711,
                ..ATTR
            },
        )
        .unwrap();
    let bh = v.open_file(b).unwrap();
    v.write(bh, 0, b"new inode").unwrap();
    v.close_file(bh).unwrap();
    old_times(&mut v, p);
    old_times(&mut v, q);
    v.rename(q, "new", p, "file", true).unwrap();
    assert_eq!(v.lookup(p, "file").unwrap(), b);
    assert_eq!(v.read(h, 0, 100).unwrap(), b"old inode");
    let retired = v.metadata(a).unwrap();
    assert_eq!(retired.uid, old.uid);
    assert_eq!(retired.modified, old.modified);
    assert_eq!(retired.created, old.created);
    assert_ne!(v.metadata(p).unwrap().modified, OLD);
    assert_ne!(v.metadata(q).unwrap().modified, OLD);
    assert_eq!(v.metadata(b).unwrap().mode, 0o711);
    v.close_file(h).unwrap();
    assert!(matches!(v.metadata(a), Err(Error::NotFound)));
    old_times(&mut v, p);
    v.rename(p, "file", p, "FILE", false).unwrap();
    assert_ne!(v.metadata(p).unwrap().modified, OLD);
    old_times(&mut v, q);
    v.hard_link(b, q, "link").unwrap();
    assert_ne!(v.metadata(q).unwrap().modified, OLD);
    old_times(&mut v, q);
    v.unlink(q, "link").unwrap();
    assert_ne!(v.metadata(q).unwrap().modified, OLD);
    old_times(&mut v, ROOT);
    v.rmdir(ROOT, "other").unwrap();
    assert_ne!(v.metadata(ROOT).unwrap().modified, OLD);
    v.verify().unwrap();
}

#[cfg(feature = "fault-injection")]
#[test]
fn failed_content_commit_rolls_back_metadata_together_with_bytes() {
    let s = Scratch::new();
    let mut v = s.create();
    let id = v.create_file(ROOT, "tool").unwrap();
    let h = v.open_file(id).unwrap();
    v.write(h, 0, b"old").unwrap();
    old_times(&mut v, id);
    v.set_metadata(
        id,
        MetadataUpdate {
            mode: Some(0o6751),
            ..Default::default()
        },
    )
    .unwrap();
    let old = v.metadata(id).unwrap();
    v.inject_once(portaweave_core::FaultPoint::BeforeCommit);
    assert!(matches!(v.write(h, 0, b"new"), Err(Error::Injected)));
    assert!(v.is_write_protected());
    assert_eq!(v.read(h, 0, 3).unwrap(), b"old");
    assert_eq!(v.metadata(id).unwrap(), old);
    drop(v);
    let v = Volume::open(s.path()).unwrap();
    assert_eq!(v.metadata(id).unwrap(), old);
    v.verify().unwrap();
}

#[test]
fn invalid_v2_identity_or_metadata_is_rejected_before_orphan_gc() {
    for sql in [
        "DELETE FROM object_metadata WHERE object=1",
        "DELETE FROM volume_identity",
        "UPDATE volume_identity SET uuid=zeroblob(16)",
        "PRAGMA ignore_check_constraints=ON; UPDATE object_metadata SET access_nsec=1000000000 WHERE object=1",
        "CREATE TABLE unknown_metadata(value BLOB)",
    ] {
        let s = Scratch::new();
        let mut v = s.create();
        let id = v.create_file(ROOT, "orphan").unwrap();
        let h = v.open_file(id).unwrap();
        v.write(h, 0, b"retain evidence").unwrap();
        v.unlink(ROOT, "orphan").unwrap();
        drop(v);
        let db = Connection::open(s.path().join("catalog.sqlite")).unwrap();
        db.execute_batch(sql).unwrap();
        drop(db);
        let segments: Vec<_> = fs::read_dir(s.path().join("segments"))
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert!(!segments.is_empty());
        assert!(Volume::open(s.path()).is_err());
        let after: Vec<_> = fs::read_dir(s.path().join("segments"))
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(segments, after);
        let db = Connection::open(s.path().join("catalog.sqlite")).unwrap();
        assert!(
            db.prepare("SELECT 1 FROM objects WHERE id=?1")
                .unwrap()
                .exists([id.raw()])
                .unwrap()
        );
        drop(db);
        let recovery =
            Recovery::prepare(s.path(), s.0.join("recovery"), RecoveryLimits::default()).unwrap();
        assert!(recovery.verify().is_err());
    }
}

#[test]
fn recovery_validates_v2_without_modifying_original_and_counts_metadata_rows() {
    let s = Scratch::new();
    let mut v = s.create();
    let id = v.create_file(ROOT, "payload").unwrap();
    let h = v.open_file(id).unwrap();
    v.write(h, 0, b"recovered").unwrap();
    v.close_file(h).unwrap();
    v.hard_link(id, ROOT, "alias").unwrap();
    drop(v);
    let before = s.files();
    let r = Recovery::prepare(s.path(), s.0.join("recovery"), RecoveryLimits::default()).unwrap();
    r.verify().unwrap();
    assert_eq!(s.files(), before);
    let out = s.0.join("export");
    r.export_new(&out, ExportLinks::Preserve).unwrap();
    assert_eq!(fs::read(out.join("files/payload")).unwrap(), b"recovered");
    assert_eq!(s.files(), before);
    drop(r);
    // Existing rows: 2 objects + 2 entries + 1 block + 1 segment = 6;
    // v2 adds 2 metadata rows and 1 identity row, which must count too.
    let r = Recovery::prepare(
        s.path(),
        s.0.join("limited"),
        RecoveryLimits {
            catalog_rows: 8,
            ..RecoveryLimits::default()
        },
    )
    .unwrap();
    assert!(matches!(r.verify(), Err(Error::RecoveryLimit)));
}
