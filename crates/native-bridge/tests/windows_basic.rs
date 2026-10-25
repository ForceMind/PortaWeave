use portaweave_native::*;
use std::{
    fs,
    path::PathBuf,
    ptr,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
const EPOCH: u64 = 116_444_736_000_000_000;
struct Fixture {
    path: PathBuf,
    volume: u64,
    object: i64,
    reader: u64,
    writer: u64,
    admin: u64,
}
fn caller(uid: u32) -> PwCredentials {
    PwCredentials {
        uid,
        gid: 30,
        groups: ptr::null(),
        group_count: 0,
        reserved: 0,
    }
}
fn request() -> PwWindowsBasicInfo {
    PwWindowsBasicInfo {
        file_attributes: u32::MAX,
        reserved: 0,
        creation_time: 0,
        last_access_time: 0,
        last_write_time: 0,
        change_time: 0,
    }
}
impl Fixture {
    fn new() -> Self {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "pw-windows-basic-{}-{stamp}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let p = path.to_str().unwrap();
        let attrs = PwCreateAttributes {
            uid: 200,
            gid: 30,
            mode: 0o777,
            reserved: 0,
        };
        let mut volume = 0;
        let mut object = 0;
        let mut admin = 0;
        let mut reader = 0;
        let mut writer = 0;
        assert_eq!(
            unsafe {
                pw_volume_create_with_metadata(p.as_ptr(), p.len(), 1 << 20, &attrs, &mut volume)
            },
            0
        );
        assert_eq!(
            unsafe {
                pw_node_create_with_metadata(volume, 1, b"file".as_ptr(), 4, 0, &attrs, &mut object)
            },
            0
        );
        assert_eq!(unsafe { pw_file_open(volume, object, &mut admin) }, 0);
        let mut written = 0;
        assert_eq!(
            unsafe { pw_file_write(volume, admin, 0, b"payload".as_ptr(), 7, &mut written) },
            0
        );
        assert_eq!(
            unsafe {
                pw_posix_file_open(volume, 1, b"file".as_ptr(), 4, 4, &caller(200), &mut reader)
            },
            0
        );
        assert_eq!(
            unsafe {
                pw_posix_file_open(volume, 1, b"file".as_ptr(), 4, 2, &caller(300), &mut writer)
            },
            0
        );
        Self {
            path,
            volume,
            object,
            reader,
            writer,
            admin,
        }
    }
    fn info(&self) -> PwFileInfo {
        let mut info = PwFileInfo::default();
        assert_eq!(
            unsafe { pw_posix_file_info(self.volume, self.reader, &mut info) },
            0
        );
        info
    }
    fn set(
        &self,
        file: u64,
        uid: u32,
        req: &PwWindowsBasicInfo,
        out: &mut PwWindowsFileInfo,
    ) -> i32 {
        unsafe { pw_windows_file_set_basic_info(self.volume, file, &caller(uid), req, out) }
    }
    fn content(&self) -> Vec<u8> {
        let mut buf = vec![0; 16];
        let mut read = 0;
        assert_eq!(
            unsafe {
                pw_posix_file_read(
                    self.volume,
                    self.reader,
                    0,
                    buf.as_mut_ptr(),
                    buf.len(),
                    &mut read,
                )
            },
            0
        );
        buf.truncate(read);
        buf
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if self.reader != 0 {
            assert_eq!(pw_posix_file_close(self.volume, self.reader), 0);
        }
        if self.writer != 0 {
            assert_eq!(pw_posix_file_close(self.volume, self.writer), 0);
        }
        if self.admin != 0 {
            assert_eq!(pw_file_close(self.volume, self.admin), 0);
        }
        if self.volume != 0 {
            assert_eq!(pw_volume_close(self.volume), 0);
        }
        fs::remove_dir_all(&self.path).unwrap();
    }
}

#[test]
fn basic_info_updates_exact_times_with_current_owner_and_keeps_open_unlinked_inode() {
    let f = Fixture::new();
    let before = f.info();
    let req = PwWindowsBasicInfo {
        creation_time: 1,
        last_access_time: EPOCH + 7,
        last_write_time: i64::MAX as u64,
        ..request()
    };
    let mut out = PwWindowsFileInfo::default();
    assert_eq!(f.set(f.writer, 300, &req, &mut out), -17);
    assert_eq!(out, PwWindowsFileInfo::default());
    assert_eq!(f.info(), before);
    assert_eq!(f.set(f.reader, 200, &req, &mut out), 0);
    assert_eq!(
        (out.creation_time, out.last_access_time, out.last_write_time),
        (1, EPOCH + 7, i64::MAX as u64)
    );
    assert_eq!(
        (out.index_number, out.file_size, out.allocation_size),
        (f.object as u64, 7, 4096)
    );
    let info = f.info();
    assert_eq!(info.stat.generation, before.stat.generation + 1);
    assert_eq!(
        info.metadata.created,
        PwTimestamp {
            seconds: -11_644_473_600,
            nanoseconds: 100,
            reserved: 0
        }
    );
    assert_eq!(
        info.metadata.accessed,
        PwTimestamp {
            seconds: 0,
            nanoseconds: 700,
            reserved: 0
        }
    );
    let expected = out;
    assert_eq!(f.set(f.writer, 300, &request(), &mut out), 0);
    assert_eq!(out, expected);
    assert_eq!(f.info(), info); // no-op preserves ctime/generation
    assert_eq!(f.content(), b"payload");
    let change_owner = PwMetadataUpdate {
        valid: 1,
        uid: 300,
        ..Default::default()
    };
    assert_eq!(
        unsafe { pw_metadata_set(f.volume, f.object, &change_owner) },
        0
    );
    let after_chown = f.info();
    assert_eq!(f.set(f.reader, 200, &req, &mut out), -17);
    assert_eq!(out, expected);
    assert_eq!(f.info(), after_chown);
    assert_eq!(unsafe { pw_remove(f.volume, 1, b"file".as_ptr(), 4, 0) }, 0);
    let req = PwWindowsBasicInfo {
        last_write_time: EPOCH + 123,
        ..request()
    };
    assert_eq!(f.set(f.writer, 300, &req, &mut out), 0);
    assert_eq!(out.last_write_time, EPOCH + 123);
    assert_eq!(out.index_number, f.object as u64);
    assert_eq!(f.info().stat.links, 0);
    assert_eq!(f.content(), b"payload");
    assert_eq!(pw_volume_close(f.volume), -7);
}

#[test]
fn invalid_unsupported_and_reply_projection_requests_leave_outputs_and_all_state_unchanged() {
    let f = Fixture::new();
    let before = f.info();
    let sentinel = PwWindowsFileInfo {
        file_size: 7,
        allocation_size: 8,
        creation_time: 9,
        last_access_time: 10,
        last_write_time: 11,
        change_time: 12,
        index_number: 13,
    };
    let mut out = sentinel;
    let mut invalid = Vec::new();
    for attributes in [0, 1, 0x80, 0x20, 0xffff_fffe] {
        invalid.push((
            PwWindowsBasicInfo {
                file_attributes: attributes,
                creation_time: EPOCH,
                ..request()
            },
            -11,
        ));
    }
    invalid.push((
        PwWindowsBasicInfo {
            change_time: 1,
            creation_time: EPOCH,
            ..request()
        },
        -11,
    ));
    invalid.push((
        PwWindowsBasicInfo {
            reserved: 1,
            creation_time: EPOCH,
            ..request()
        },
        -1,
    ));
    for ticks in [i64::MAX as u64 + 1, u64::MAX - 1, u64::MAX] {
        invalid.push((
            PwWindowsBasicInfo {
                creation_time: ticks,
                last_write_time: EPOCH,
                ..request()
            },
            -11,
        ));
        invalid.push((
            PwWindowsBasicInfo {
                last_access_time: ticks,
                creation_time: EPOCH,
                ..request()
            },
            -11,
        ));
        invalid.push((
            PwWindowsBasicInfo {
                last_write_time: ticks,
                creation_time: EPOCH,
                ..request()
            },
            -11,
        ));
    }
    for (req, code) in invalid {
        assert_eq!(f.set(f.reader, 200, &req, &mut out), code);
        assert_eq!(out, sentinel);
        assert_eq!(f.info(), before);
    }
    assert_eq!(
        unsafe {
            pw_windows_file_set_basic_info(f.volume, f.reader, ptr::null(), &request(), &mut out)
        },
        -1
    );
    assert_eq!(
        unsafe {
            pw_windows_file_set_basic_info(f.volume, f.reader, &caller(200), ptr::null(), &mut out)
        },
        -1
    );
    assert_eq!(
        unsafe {
            pw_windows_file_set_basic_info(
                f.volume,
                f.reader,
                &caller(200),
                &request(),
                ptr::null_mut(),
            )
        },
        -1
    );
    assert_eq!(out, sentinel);
    assert_eq!(f.info(), before);
    let broken = PwMetadataUpdate {
        valid: 8,
        created: PwTimestamp {
            seconds: i64::MIN,
            ..Default::default()
        },
        ..Default::default()
    };
    assert_eq!(unsafe { pw_metadata_set(f.volume, f.object, &broken) }, 0);
    let before = f.info();
    let req = PwWindowsBasicInfo {
        last_write_time: EPOCH + 1,
        ..request()
    };
    assert_eq!(f.set(f.reader, 200, &req, &mut out), -11);
    assert_eq!(out, sentinel);
    assert_eq!(f.info(), before); // another field's bad projection rolls back proposed mtime
    let req = PwWindowsBasicInfo {
        creation_time: EPOCH + 2,
        ..req
    };
    assert_eq!(f.set(f.reader, 200, &req, &mut out), 0);
    assert_eq!(
        (out.creation_time, out.last_write_time),
        (EPOCH + 2, EPOCH + 1)
    );
    assert_eq!(f.content(), b"payload");
    let mut protected = 99;
    let mut failed = 99;
    assert_eq!(
        unsafe { pw_health(f.volume, &mut protected, &mut failed) },
        0
    );
    assert_eq!((protected, failed), (0, 0));
}

#[test]
fn admin_foreign_and_closed_tokens_cannot_set_basic_info_and_changes_persist_after_reopen() {
    let mut f = Fixture::new();
    let other = Fixture::new();
    let req = PwWindowsBasicInfo {
        creation_time: EPOCH + 345,
        last_access_time: 1,
        last_write_time: EPOCH - 1,
        ..request()
    };
    let mut out = PwWindowsFileInfo::default();
    let before = f.info();
    for token in [f.admin, other.reader, 0, u64::MAX] {
        assert_eq!(f.set(token, 200, &req, &mut out), -1);
        assert_eq!(out, PwWindowsFileInfo::default());
        assert_eq!(f.info(), before);
    }
    assert_eq!(f.set(f.reader, 200, &req, &mut out), 0);
    let expected = f.info();
    let closed = f.reader;
    assert_eq!(pw_posix_file_close(f.volume, f.reader), 0);
    f.reader = 0;
    let sentinel = out;
    assert_eq!(f.set(closed, 200, &req, &mut out), -1);
    assert_eq!(out, sentinel);
    assert_eq!(pw_posix_file_close(f.volume, f.writer), 0);
    f.writer = 0;
    assert_eq!(pw_file_close(f.volume, f.admin), 0);
    f.admin = 0;
    assert_eq!(pw_volume_close(f.volume), 0);
    f.volume = 0;
    let path = f.path.to_str().unwrap();
    assert_eq!(
        unsafe { pw_volume_open(path.as_ptr(), path.len(), &mut f.volume) },
        0
    );
    assert_eq!(
        unsafe {
            pw_posix_file_open(
                f.volume,
                1,
                b"file".as_ptr(),
                4,
                4,
                &caller(200),
                &mut f.reader,
            )
        },
        0
    );
    assert_eq!(f.info(), expected);
    assert_eq!(f.content(), b"payload");
    // Reopening the owner never makes an old process-local token valid again.
    assert_eq!(f.set(closed, 200, &req, &mut out), -1);
}

#[test]
fn portable_descriptor_metadata_api_checks_all_fields_tokens_and_outputs_before_mutation() {
    let f = Fixture::new();
    let update = PwMetadataUpdate {
        valid: 4 | 32,
        mode: 0o6751,
        modified: PwTimestamp {
            seconds: -456,
            nanoseconds: 123_456_789,
            reserved: 0,
        },
        ..Default::default()
    };
    let before = f.info();
    let mut out = before;
    for (token, uid, expected) in [(f.admin, 200, -1), (f.writer, 300, -17), (0, 200, -1)] {
        assert_eq!(
            unsafe {
                pw_posix_file_set_metadata_info(f.volume, token, &caller(uid), &update, &mut out)
            },
            expected
        );
        assert_eq!(out, before);
        assert_eq!(f.info(), before);
    }
    assert_eq!(
        unsafe {
            pw_posix_file_set_metadata_info(
                f.volume,
                f.reader,
                &caller(200),
                &update,
                ptr::null_mut(),
            )
        },
        -1
    );
    assert_eq!(
        unsafe {
            pw_posix_file_set_metadata_info(f.volume, f.reader, &caller(200), ptr::null(), &mut out)
        },
        -1
    );
    assert_eq!(
        unsafe {
            pw_posix_file_set_metadata_info(f.volume, f.reader, ptr::null(), &update, &mut out)
        },
        -1
    );
    let invalid = PwMetadataUpdate {
        valid: 64,
        ..update
    };
    assert_eq!(
        unsafe {
            pw_posix_file_set_metadata_info(f.volume, f.reader, &caller(200), &invalid, &mut out)
        },
        -1
    );
    let invalid = PwMetadataUpdate {
        modified: PwTimestamp {
            nanoseconds: 1_000_000_000,
            ..update.modified
        },
        ..update
    };
    assert_eq!(
        unsafe {
            pw_posix_file_set_metadata_info(f.volume, f.reader, &caller(200), &invalid, &mut out)
        },
        -1
    );
    assert_eq!(f.info(), before);
    assert_eq!(out, before);
    assert_eq!(
        unsafe {
            pw_posix_file_set_metadata_info(f.volume, f.reader, &caller(200), &update, &mut out)
        },
        0
    );
    assert_eq!(out, f.info());
    assert_eq!(out.stat.generation, before.stat.generation + 1);
    assert_eq!(out.metadata.mode, 0o6751); // metadata-only chmod, no content/ownership change
    assert_eq!(out.metadata.modified, update.modified);
    assert_eq!(f.content(), b"payload");
    let expected = out;
    assert_eq!(
        unsafe {
            pw_posix_file_set_metadata_info(
                f.volume,
                f.writer,
                &caller(300),
                &PwMetadataUpdate::default(),
                &mut out,
            )
        },
        0
    );
    assert_eq!(out, expected);
}
