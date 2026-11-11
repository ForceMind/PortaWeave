use portaweave_native::*;
use std::{
    fs,
    path::PathBuf,
    ptr,
    sync::{
        Arc, Barrier,
        atomic::{AtomicU64, Ordering},
    },
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture {
    path: PathBuf,
    volume: u64,
}
impl Fixture {
    fn new(v2: bool) -> Self {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "pw-checked-abi-{}-{stamp}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let p = path.to_str().unwrap();
        let mut volume = 0;
        let a = PwCreateAttributes {
            uid: 100,
            gid: 20,
            mode: 0o700,
            reserved: 0,
        };
        assert_eq!(
            unsafe {
                if v2 {
                    pw_volume_create_with_metadata(p.as_ptr(), p.len(), 1 << 20, &a, &mut volume)
                } else {
                    pw_volume_create(p.as_ptr(), p.len(), 1 << 20, &mut volume)
                }
            },
            0
        );
        Self { path, volume }
    }
    fn create(&self, name: &str, directory: bool) -> i64 {
        let mut id = 0;
        assert_eq!(
            unsafe {
                if directory {
                    pw_directory_create(self.volume, 1, name.as_ptr(), name.len(), &mut id)
                } else {
                    pw_file_create(self.volume, 1, name.as_ptr(), name.len(), &mut id)
                }
            },
            0
        );
        id
    }
    fn lookup(&self, name: &str) -> i64 {
        let mut id = 0;
        assert_eq!(
            unsafe { pw_lookup(self.volume, 1, name.as_ptr(), name.len(), &mut id) },
            0
        );
        id
    }
    fn info(&self, id: i64) -> (PwStat, PwMetadata) {
        let mut stat = PwStat::default();
        let mut metadata = PwMetadata::default();
        assert_eq!(unsafe { pw_stat(self.volume, id, &mut stat) }, 0);
        assert_eq!(
            unsafe { pw_metadata_get(self.volume, id, &mut metadata) },
            0
        );
        (stat, metadata)
    }
    fn healthy(&self) {
        let (mut health, mut protected) = (99, 99);
        assert_eq!(
            unsafe { pw_health(self.volume, &mut health, &mut protected) },
            0
        );
        assert_eq!((health, protected), (0, 0));
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        assert_eq!(pw_volume_close(self.volume), 0);
        fs::remove_dir_all(&self.path).unwrap();
    }
}

#[test]
fn fskit_link_keeps_pinned_file_data_but_cannot_restore_its_last_removed_name() {
    for v2 in [false, true] {
        let f = Fixture::new(v2);
        let file = f.create("source", false);
        let other = f.create("other", false);
        let mut pin = 0;
        assert_eq!(unsafe { pw_file_open(f.volume, file, &mut pin) }, 0);
        let content = b"held contents";
        let mut written = 0;
        assert_eq!(
            unsafe {
                pw_file_write(
                    f.volume,
                    pin,
                    0,
                    content.as_ptr(),
                    content.len(),
                    &mut written,
                )
            },
            0
        );
        assert_eq!(written, content.len());
        let link = |expected, name: &str| unsafe {
            pw_fskit_file_link(f.volume, pin, expected, 1, name.as_ptr(), name.len())
        };
        let remove = |name: &str| {
            assert_eq!(
                unsafe { pw_remove_checked(f.volume, 1, name.as_ptr(), name.len(), 0, file) },
                0
            );
        };
        assert_eq!(link(other, "alias"), -18);
        assert_eq!(link(file, "other"), -3);
        assert_eq!(f.lookup("other"), other);
        assert_eq!(link(file, "alias"), 0);
        assert_eq!(f.lookup("alias"), file);
        remove("source");
        assert_eq!(link(file, "remaining"), 0);
        assert_eq!(f.lookup("remaining"), file);
        remove("alias");
        remove("remaining");
        let mut before = PwStat::default();
        assert_eq!(unsafe { pw_stat(f.volume, file, &mut before) }, 0);
        assert_eq!(before.links, 0);
        assert_eq!(link(file, "revived"), -18);
        let mut absent = 99;
        assert_eq!(
            unsafe { pw_lookup(f.volume, 1, b"revived".as_ptr(), 7, &mut absent) },
            -2
        );
        assert_eq!(absent, 99);
        let mut after = PwStat::default();
        assert_eq!(unsafe { pw_stat(f.volume, file, &mut after) }, 0);
        assert_eq!(after, before);
        let mut data = [0; 13];
        let mut read = 0;
        assert_eq!(
            unsafe { pw_file_read(f.volume, pin, 0, data.as_mut_ptr(), data.len(), &mut read) },
            0
        );
        assert_eq!(&data[..read], content);
        assert_eq!(pw_file_close(f.volume, pin), 0);
        assert_eq!(link(file, "revived"), -1);
        f.healthy();
    }
}

#[test]
fn serialized_c_calls_detect_a_name_replaced_between_lookup_and_mutation_on_another_thread() {
    let f = Fixture::new(true);
    let old = f.create("slot", false);
    let replacement = f.create("incoming", false);
    let (mut old_h, mut new_h) = (0, 0);
    assert_eq!(unsafe { pw_file_open(f.volume, old, &mut old_h) }, 0);
    assert_eq!(
        unsafe { pw_file_open(f.volume, replacement, &mut new_h) },
        0
    );
    let mut count = 0;
    assert_eq!(
        unsafe { pw_file_write(f.volume, old_h, 0, b"old".as_ptr(), 3, &mut count) },
        0
    );
    assert_eq!(
        unsafe { pw_file_write(f.volume, new_h, 0, b"new".as_ptr(), 3, &mut count) },
        0
    );
    assert_eq!(f.lookup("slot"), old); // What a separate preflight observed.
    let barrier = Arc::new(Barrier::new(2));
    let ready = barrier.clone();
    let volume = f.volume;
    let worker = std::thread::spawn(move || {
        assert_eq!(
            unsafe {
                pw_rename(
                    volume,
                    1,
                    b"incoming".as_ptr(),
                    8,
                    1,
                    b"slot".as_ptr(),
                    4,
                    1,
                )
            },
            0
        );
        ready.wait();
    });
    barrier.wait();
    worker.join().unwrap();
    let before = (f.info(1), f.info(old), f.info(replacement));
    assert_eq!(
        unsafe { pw_remove_checked(f.volume, 1, b"slot".as_ptr(), 4, 0, old) },
        -18
    );
    assert_eq!(
        unsafe {
            pw_rename_checked(
                f.volume,
                1,
                b"slot".as_ptr(),
                4,
                1,
                b"moved".as_ptr(),
                5,
                old,
                0,
            )
        },
        -18
    );
    assert_eq!(f.lookup("slot"), replacement);
    assert_eq!((f.info(1), f.info(old), f.info(replacement)), before);
    let mut bytes = [0; 4];
    assert_eq!(
        unsafe { pw_file_read(f.volume, old_h, 0, bytes.as_mut_ptr(), 4, &mut count) },
        0
    );
    assert_eq!(&bytes[..count], b"old");
    assert_eq!(
        unsafe { pw_file_read(f.volume, new_h, 0, bytes.as_mut_ptr(), 4, &mut count) },
        0
    );
    assert_eq!(&bytes[..count], b"new");
    f.healthy();
    assert_eq!(
        unsafe { pw_remove_checked(f.volume, 1, b"SLOT".as_ptr(), 4, 0, replacement) },
        0
    );
    assert_eq!(
        unsafe { pw_remove_checked(f.volume, 1, b"slot".as_ptr(), 4, 0, replacement) },
        -18
    );
    assert_eq!(pw_file_close(f.volume, old_h), 0);
    assert_eq!(pw_file_close(f.volume, new_h), 0);
}

#[test]
fn rename_target_guard_rejects_unexpected_absence_arrival_or_replacement_and_keeps_alias_rules() {
    for v2 in [false, true] {
        let f = Fixture::new(v2);
        let a = f.create("a", false);
        let b = f.create("b", false);
        let c = f.create("c", false);
        assert_eq!(
            unsafe { pw_rename_checked(f.volume, 1, b"a".as_ptr(), 1, 1, b"b".as_ptr(), 1, a, 0) },
            -18
        );
        assert_eq!(
            unsafe { pw_rename_checked(f.volume, 1, b"a".as_ptr(), 1, 1, b"b".as_ptr(), 1, a, c) },
            -18
        );
        assert_eq!(
            unsafe {
                pw_rename_checked(
                    f.volume,
                    1,
                    b"a".as_ptr(),
                    1,
                    1,
                    b"absent".as_ptr(),
                    6,
                    a,
                    b,
                )
            },
            -18
        );
        assert_eq!(f.lookup("a"), a);
        assert_eq!(f.lookup("b"), b);
        assert_eq!(
            unsafe { pw_rename_checked(f.volume, 1, b"a".as_ptr(), 1, 1, b"A".as_ptr(), 1, a, 0) },
            0
        );
        assert_eq!(
            unsafe { pw_hard_link(f.volume, a, 1, b"alias".as_ptr(), 5) },
            0
        );
        assert_eq!(
            unsafe {
                pw_rename_checked(f.volume, 1, b"a".as_ptr(), 1, 1, b"alias".as_ptr(), 5, a, 0)
            },
            -18
        );
        assert_eq!(
            unsafe {
                pw_rename_checked(f.volume, 1, b"a".as_ptr(), 1, 1, b"alias".as_ptr(), 5, a, a)
            },
            0
        );
        assert_eq!(f.lookup("a"), a);
        assert_eq!(f.lookup("alias"), a);
        assert_eq!(
            unsafe { pw_rename_checked(f.volume, 1, b"a".as_ptr(), 1, 1, b"b".as_ptr(), 1, a, b) },
            0
        );
        assert_eq!(f.lookup("b"), a);
        f.healthy();
    }
}

#[test]
fn malformed_checked_arguments_and_type_mismatches_never_mutate_the_expected_entry() {
    let f = Fixture::new(true);
    let file = f.create("file", false);
    let dir = f.create("dir", true);
    let before = (f.info(1), f.info(file), f.info(dir));
    for (parent, name, len, directory, expected) in [
        (1, b"file".as_ptr(), 4, 2, file),
        (1, b"file".as_ptr(), 4, 0, 0),
        (1, b"file".as_ptr(), 4, 0, -1),
        (0, b"file".as_ptr(), 4, 0, file),
        (1, ptr::null(), 4, 0, file),
        (1, b"../file".as_ptr(), 7, 0, file),
    ] {
        assert_eq!(
            unsafe { pw_remove_checked(f.volume, parent, name, len, directory, expected) },
            -1
        );
    }
    assert_eq!(
        unsafe { pw_remove_checked(f.volume, 1, b"dir".as_ptr(), 3, 0, dir) },
        -5
    );
    assert_eq!(
        unsafe { pw_remove_checked(f.volume, 1, b"file".as_ptr(), 4, 1, file) },
        -4
    );
    assert_eq!(
        unsafe {
            pw_rename_checked(
                f.volume,
                1,
                b"file".as_ptr(),
                4,
                1,
                b"new".as_ptr(),
                3,
                file,
                -1,
            )
        },
        -1
    );
    assert_eq!(
        unsafe { pw_rename_checked(f.volume, 1, b"file".as_ptr(), 4, 1, ptr::null(), 3, file, 0) },
        -1
    );
    assert_eq!((f.info(1), f.info(file), f.info(dir)), before);
    f.healthy();
    assert_eq!(
        unsafe { pw_remove_checked(f.volume, 1, b"dir".as_ptr(), 3, 1, dir) },
        0
    );
    assert_eq!(
        unsafe { pw_remove_checked(f.volume, 1, b"file".as_ptr(), 4, 0, file) },
        0
    );
}
