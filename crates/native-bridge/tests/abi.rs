use portaweave_native::*;
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};
static NEXT: AtomicU64 = AtomicU64::new(1);
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        Self(std::env::temp_dir().join(format!("pw-native-{}-{now}-{n}", std::process::id())))
    }
    fn path(&self) -> &[u8] { self.0.to_str().unwrap().as_bytes() }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        if self.0.exists() { fs::remove_dir_all(&self.0).unwrap(); }
    }
}
#[test]
fn native_abi_writes_and_reopens_real_core_container() {
    let s = Scratch::new();
    let mut volume = 0_u64;
    assert_eq!(pw_abi_version(), 1);
    assert_eq!(pw_root_id(), 1);
    assert_eq!(unsafe { pw_volume_create(s.path().as_ptr(), s.path().len(), 100_000_000, &mut volume) }, 0);
    let root = pw_root_id();
    let mut dir = 0_i64;
    assert_eq!(unsafe { pw_directory_create(volume, root, b"src".as_ptr(), 3, &mut dir) }, 0);
    let name = "数据.rs".as_bytes();
    let mut object = 0_i64;
    assert_eq!(unsafe { pw_file_create(volume, dir, name.as_ptr(), name.len(), &mut object) }, 0);
    let mut file = 0_u64;
    assert_eq!(unsafe { pw_file_open(volume, object, &mut file) }, 0);
    assert_eq!(pw_volume_close(volume), -7); // no silent drop while file open
    let payload = b"working code";
    let mut written = 0;
    assert_eq!(unsafe { pw_file_write(volume, file, 0, payload.as_ptr(), payload.len(), &mut written) }, 0);
    assert_eq!(written, payload.len());
    let mut output = [0_u8; 32];
    let mut read = 0;
    assert_eq!(unsafe { pw_file_read(volume, file, 0, output.as_mut_ptr(), output.len(), &mut read) }, 0);
    assert_eq!(&output[..read], payload);
    assert_eq!(pw_file_truncate(volume, file, 4), 0);
    assert_eq!(unsafe { pw_file_read(volume, file, 0, output.as_mut_ptr(), output.len(), &mut read) }, 0);
    assert_eq!(&output[..read], b"work");
    let mut status = PwStat::default();
    assert_eq!(unsafe { pw_stat(volume, object, &mut status) }, 0);
    assert_eq!(status.kind, 0);
    assert_eq!(status.size, 4);
    assert_eq!(unsafe { pw_hard_link(volume, object, dir, b"alias".as_ptr(), 5) }, 0);
    assert_eq!(unsafe { pw_stat(volume, object, &mut status) }, 0);
    assert_eq!(status.links, 2);
    assert_eq!(pw_file_close(volume, file), 0);
    assert_eq!(unsafe { pw_rename(volume, dir, b"alias".as_ptr(), 5, dir, b"newalias".as_ptr(), 8, 0) }, 0);
    let mut cursor = 0_u64;
    assert_eq!(unsafe { pw_dir_begin(volume, dir, &mut cursor) }, 0);
    let mut buf = [0_u8; 2];
    let mut required = 0_usize;
    let mut entry_id = 0_i64;
    assert_eq!(unsafe { pw_dir_next(volume, cursor, buf.as_mut_ptr(), buf.len(), &mut required, &mut entry_id) }, -16);
    assert!(required > buf.len());
    let mut names = Vec::new();
    loop {
        let mut big = [0_u8; 1024];
        let step = unsafe { pw_dir_next(volume, cursor, big.as_mut_ptr(), big.len(), &mut required, &mut entry_id) };
        if step == 1 { break; }
        assert_eq!(step, 0);
        names.push(String::from_utf8(big[..required].to_vec()).unwrap());
    }
    assert_eq!(names, vec!["newalias".to_owned(), "数据.rs".to_owned()].into_iter().collect::<std::collections::BTreeSet<_>>().into_iter().collect::<Vec<_>>());
    assert_eq!(pw_volume_close(volume), -7); // cursor pins lifecycle
    assert_eq!(pw_dir_close(volume, cursor), 0);
    let mut maintenance = 99_u32;
    let mut protected = 99_u32;
    assert_eq!(unsafe { pw_health(volume, &mut maintenance, &mut protected) }, 0);
    assert_eq!((maintenance, protected), (0, 0));
    assert_eq!(pw_volume_close(volume), 0);
    assert_eq!(unsafe { pw_volume_open(s.path().as_ptr(), s.path().len(), &mut volume) }, 0);
    let mut recovered = 0;
    assert_eq!(unsafe { pw_lookup(volume, dir, name.as_ptr(), name.len(), &mut recovered) }, 0);
    assert_eq!(recovered, object);
    assert_eq!(unsafe { pw_remove(volume, dir, name.as_ptr(), name.len(), 0) }, 0);
    assert_eq!(unsafe { pw_remove(volume, dir, b"newalias".as_ptr(), 8, 0) }, 0);
    assert_eq!(unsafe { pw_remove(volume, root, b"src".as_ptr(), 3, 1) }, 0);
    assert_eq!(pw_volume_close(volume), 0);
}
#[test]
fn invalid_inputs_do_not_create_files_and_session_tokens_are_not_reused() {
    let s = Scratch::new();
    assert_eq!(unsafe { pw_volume_create(s.path().as_ptr(), s.path().len(), 1000, std::ptr::null_mut()) }, -1);
    assert!(!s.0.exists());
    let mut volume = 0;
    assert_eq!(unsafe { pw_volume_create(s.path().as_ptr(), s.path().len(), 1024, &mut volume) }, 0);
    let mut second = 0;
    assert_eq!(unsafe { pw_volume_open(s.path().as_ptr(), s.path().len(), &mut second) }, -7);
    let mut file = 0;
    assert_eq!(unsafe { pw_file_create(volume, 1, std::ptr::null(), 4, &mut file) }, -1);
    assert_eq!(unsafe { pw_file_create(volume, -1, b"bad".as_ptr(), 3, &mut file) }, -1);
    assert_eq!(unsafe { pw_file_create(volume, 1, b"ok".as_ptr(), 2, &mut file) }, 0);
    let mut handle = 0;
    assert_eq!(unsafe { pw_file_open(volume, file, &mut handle) }, 0);
    assert_eq!(unsafe { pw_file_read(volume, handle, 0, std::ptr::null_mut(), 5, &mut 0_usize) }, -1);
    assert_eq!(unsafe { pw_file_write(volume, handle, 0, std::ptr::null(), 9, &mut 0_usize) }, -1);
    assert_eq!(pw_file_close(volume, handle), 0);
    assert_eq!(pw_volume_close(volume), 0);
    assert_eq!(pw_volume_close(volume), -1);
    assert_eq!(unsafe { pw_volume_open(s.path().as_ptr(), s.path().len(), &mut second) }, 0);
    assert_ne!(volume, second);
    assert_eq!(pw_volume_close(second), 0);
}
