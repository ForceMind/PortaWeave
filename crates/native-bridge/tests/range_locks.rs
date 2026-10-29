use portaweave_native::*;
use std::{
    fs, path::PathBuf, ptr,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture {
    path: PathBuf,
    v: u64,
}
fn caller() -> PwCredentials {
    PwCredentials {
        uid: 100,
        gid: 20,
        groups: ptr::null(),
        group_count: 0,
        reserved: 0,
    }
}
impl Fixture {
    fn new(version: u8) -> Self {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "pw-lock-abi-{}-{stamp}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let bytes = path.to_str().unwrap().as_bytes();
        let a = PwCreateAttributes {
            uid: 100,
            gid: 20,
            mode: 0o777,
            reserved: 0,
        };
        let mut v = 0;
        assert_eq!(
            unsafe {
                if version == 2 {
                    pw_volume_create_with_metadata(bytes.as_ptr(), bytes.len(), 1 << 24, &a, &mut v)
                } else {
                    pw_volume_create_with_directory_lifetimes(
                        bytes.as_ptr(),
                        bytes.len(),
                        1 << 24,
                        &a,
                        &mut v,
                    )
                }
            },
            0
        );
        let mut id = 0;
        assert_eq!(
            unsafe { pw_posix_create(v, 1, b"file".as_ptr(), 4, 0, 0o600, 0, &caller(), &mut id) },
            0
        );
        Self { path, v }
    }
    fn open(&self, access: u8) -> u64 {
        let mut file = 0;
        assert_eq!(
            unsafe {
                pw_posix_file_open(self.v, 1, b"file".as_ptr(), 4, access, &caller(), &mut file)
            },
            0
        );
        file
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            assert_eq!(pw_volume_close(self.v), 0);
            fs::remove_dir_all(&self.path).unwrap();
        }
    }
}
#[test]
fn policy_rights_conflicts_wrong_tokens_and_close_release_in_v2_v3() {
    for version in [2, 3] {
        unsafe {
            let f = Fixture::new(version);
            let a = f.open(4);
            let b = f.open(6);
            let mut one = 99;
            let mut out = 99;
            assert_eq!(pw_posix_file_lock_range(f.v, a, 0, 8, 2, &mut out), -17);
            assert_eq!(out, 99);
            assert_eq!(pw_posix_file_lock_range(f.v, a, 0, 8, 1, &mut one), 0);
            assert_eq!(pw_posix_file_lock_range(f.v, b, 7, 1, 2, &mut out), -20);
            assert_eq!(out, 99);
            assert_eq!(pw_posix_file_unlock_range(f.v, b, one), -1);
            assert_eq!(pw_file_close(f.v, one), -1);
            assert_eq!(pw_posix_read_view_close(f.v, one), -1);
            assert_eq!(pw_volume_close(f.v), -7);
            assert_eq!(pw_posix_file_close(f.v, a), 0);
            assert_eq!(pw_posix_file_unlock_range(f.v, a, one), -1);
            assert_eq!(pw_posix_file_lock_range(f.v, b, 7, 1, 2, &mut out), 0);
            assert_ne!(out, one);
            assert_eq!(pw_posix_file_unlock_range(f.v, b, out), 0);
            assert_eq!(pw_posix_file_unlock_range(f.v, b, out), -1);
            assert_eq!(pw_posix_file_close(f.v, b), 0);
        }
    }
}
#[test]
fn refused_outputs_and_foreign_volume_do_not_release_other_locks() {
    unsafe {
        let f = Fixture::new(2);
        let other = Fixture::new(3);
        let a = f.open(6);
        let b = other.open(6);
        let mut lock = 0;
        let mut out = 99;
        assert_eq!(pw_posix_file_lock_range(f.v, a, 2, 3, 2, &mut lock), 0);
        for (off, len, mode) in [(0, 0, 1), (u64::MAX, 1, 1), (0, 1, 0), (0, 1, 3)] {
            assert_eq!(
                pw_posix_file_lock_range(f.v, a, off, len, mode, &mut out),
                -1
            );
            assert_eq!(out, 99);
        }
        assert_eq!(
            pw_posix_file_lock_range(f.v, a, 0, 1, 1, ptr::null_mut()),
            -1
        );
        assert_eq!(pw_posix_file_lock_range(f.v, lock, 0, 1, 1, &mut out), -1);
        assert_eq!(out, 99);
        assert_eq!(pw_posix_file_unlock_range(other.v, b, lock), -1);
        assert_eq!(pw_posix_file_unlock_range(f.v, b, lock), -1);
        let rival = f.open(6);
        assert_eq!(pw_posix_file_lock_range(f.v, rival, 3, 1, 1, &mut out), -20);
        assert_eq!(out, 99);
        assert_eq!(pw_posix_file_unlock_range(f.v, a, lock), 0);
        assert_eq!(pw_posix_file_close(f.v, a), 0);
        assert_eq!(pw_posix_file_close(f.v, rival), 0);
        assert_eq!(pw_posix_file_close(other.v, b), 0);
    }
}
#[test]
fn real_4096_limit_returns_resource_without_changing_output_and_close_frees_it() {
    unsafe {
        let f = Fixture::new(3);
        let a = f.open(6);
        let b = f.open(6);
        let mut out = 0;
        for _ in 0..4096 {
            assert_eq!(pw_posix_file_lock_range(f.v, a, 0, 1, 1, &mut out), 0);
        }
        out = 99;
        assert_eq!(pw_posix_file_lock_range(f.v, b, 2, 1, 1, &mut out), -14);
        assert_eq!(out, 99);
        assert_eq!(pw_posix_file_close(f.v, a), 0);
        assert_eq!(pw_posix_file_lock_range(f.v, b, 0, 1, 2, &mut out), 0);
        assert_ne!(out, 99);
        assert_eq!(pw_posix_file_close(f.v, b), 0);
    }
}
