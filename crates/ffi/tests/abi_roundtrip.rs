use portaweave_ffi::*;
use std::{
    ffi::CString,
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};
static COUNTER: AtomicU64 = AtomicU64::new(0);
struct Lab(PathBuf);
impl Lab {
    fn new() -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        Self(std::env::temp_dir().join(format!(
            "pw-ffi-{}-{stamp}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed),
        )))
    }
    fn c_path(&self) -> CString {
        CString::new(self.0.to_str().unwrap()).unwrap()
    }
}
impl Drop for Lab {
    fn drop(&mut self) {
        if self.0.exists() {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
}

#[test]
fn create_mutate_reopen_and_closed_ids_are_rejected() {
    let lab = Lab::new();
    let mut id = 0;
    unsafe {
        assert_eq!(
            pw_volume_create(lab.c_path().as_ptr(), 128 << 20, &mut id),
            PW_OK
        );
        assert_ne!(id, 0);
        let mut second = 0;
        assert_eq!(pw_volume_open(lab.c_path().as_ptr(), &mut second), PW_BUSY);
        let mut file = 0;
        let name = "工程😀.rs".as_bytes();
        assert_eq!(
            pw_create_file(id, 1, name.as_ptr(), name.len(), &mut file),
            PW_OK
        );
        assert!(file > 1);
        let mut stat = PwFileStat::default();
        assert_eq!(pw_stat(id, file, &mut stat), PW_OK);
        assert_eq!((stat.id, stat.kind, stat.size, stat.links), (file, 0, 0, 1));
        let mut h = 0;
        assert_eq!(pw_open_file(id, file, &mut h), PW_OK);
        let data = b"hello native ABI";
        let mut done = 99;
        assert_eq!(
            pw_write(id, h, 0, data.as_ptr(), data.len(), &mut done),
            PW_OK
        );
        assert_eq!(done, data.len());
        let mut back = [0u8; 64];
        assert_eq!(
            pw_read(id, h, 0, back.as_mut_ptr(), back.len(), &mut done),
            PW_OK
        );
        assert_eq!(done, data.len());
        assert_eq!(&back[..done], data);
        assert_eq!(pw_truncate(id, h, 5), PW_OK);
        assert_eq!(
            pw_read(id, h, 0, back.as_mut_ptr(), back.len(), &mut done),
            PW_OK
        );
        assert_eq!(&back[..done], b"hello");
        assert_eq!(pw_close_file(id, h), PW_OK);
        assert_eq!(
            pw_read(id, h, 0, back.as_mut_ptr(), back.len(), &mut done),
            PW_BAD_HANDLE
        );
        let moved = "moved.rs".as_bytes();
        assert_eq!(
            pw_rename(
                id,
                1,
                name.as_ptr(),
                name.len(),
                1,
                moved.as_ptr(),
                moved.len(),
                0
            ),
            PW_OK
        );
        let mut looked_up = 0;
        assert_eq!(
            pw_lookup(id, 1, moved.as_ptr(), moved.len(), &mut looked_up),
            PW_OK
        );
        assert_eq!(looked_up, file);
        let link = b"alias.rs";
        assert_eq!(pw_hard_link(id, file, 1, link.as_ptr(), link.len()), PW_OK);
        assert_eq!(pw_stat(id, file, &mut stat), PW_OK);
        assert_eq!(stat.links, 2);
        assert_eq!(pw_unlink(id, 1, link.as_ptr(), link.len()), PW_OK);
        let mut health = PwHealth::default();
        assert_eq!(pw_health(id, &mut health), PW_OK);
        assert_eq!(health.write_protected, 0);
        assert_eq!(pw_volume_close(id), PW_OK);
        assert_eq!(pw_volume_close(id), PW_BAD_HANDLE);
        assert_eq!(pw_stat(id, file, &mut stat), PW_BAD_HANDLE);
        let mut resumed = 0;
        assert_eq!(pw_volume_open(lab.c_path().as_ptr(), &mut resumed), PW_OK);
        assert_ne!(resumed, id);
        assert_eq!(
            pw_lookup(resumed, 1, moved.as_ptr(), moved.len(), &mut looked_up),
            PW_OK
        );
        assert_eq!(pw_stat(resumed, looked_up, &mut stat), PW_OK);
        assert_eq!(stat.size, 5);
        assert_eq!(pw_volume_close(resumed), PW_OK);
    }
}
#[test]
fn bad_buffers_and_invalid_names_never_change_content() {
    let lab = Lab::new();
    unsafe {
        assert_eq!(
            pw_volume_create(lab.c_path().as_ptr(), 5 << 20, std::ptr::null_mut()),
            PW_INVALID_ARGUMENT
        );
        assert!(!lab.0.exists());
        let mut id = 0;
        assert_eq!(
            pw_volume_create(lab.c_path().as_ptr(), 5 << 20, &mut id),
            PW_OK
        );
        let mut n = 0;
        assert_eq!(
            pw_lookup(id, 1, std::ptr::null(), 3, &mut n),
            PW_INVALID_ARGUMENT
        );
        assert_eq!(
            pw_create_file(id, 1, b"NUL".as_ptr(), 3, &mut n),
            PW_INVALID_ARGUMENT
        );
        assert_eq!(pw_create_file(id, 1, b"abc".as_ptr(), 3, &mut n), PW_OK);
        let mut h = 0;
        assert_eq!(pw_open_file(id, n, &mut h), PW_OK);
        assert_eq!(
            pw_write(id, h, 0, std::ptr::null(), 1, &mut 0),
            PW_INVALID_ARGUMENT
        );
        assert_eq!(
            pw_read(id, h, 0, std::ptr::null_mut(), 1, &mut 0),
            PW_INVALID_ARGUMENT
        );
        assert_eq!(
            pw_write(id, h, 0, b"abc".as_ptr(), 3, std::ptr::null_mut()),
            PW_INVALID_ARGUMENT
        );
        let mut count = 5;
        assert_eq!(
            pw_read(id, h, 0, std::ptr::null_mut(), 0, &mut count),
            PW_OK
        );
        assert_eq!(count, 0);
        assert_eq!(
            pw_rename(id, 1, b"abc".as_ptr(), 3, 1, b"other".as_ptr(), 5, 2),
            PW_INVALID_ARGUMENT
        );
        let mut stat = PwFileStat::default();
        assert_eq!(pw_stat(id, n, &mut stat), PW_OK);
        assert_eq!(stat.size, 0);
        assert_eq!(pw_close_file(id, h), PW_OK);
        assert_eq!(pw_volume_close(id), PW_OK);
    }
}
