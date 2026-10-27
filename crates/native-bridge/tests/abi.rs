use portaweave_native::*;
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn policy_leases_reject_write_only_and_administrative_tokens_and_keep_busy_lifetime() {
    let s = Scratch::new();
    let mut volume = 0;
    let mut node = 0;
    let a = PwCreateAttributes {
        uid: 100,
        gid: 20,
        mode: 0o777,
        reserved: 0,
    };
    let c = PwCredentials {
        uid: 200,
        gid: 20,
        groups: std::ptr::null(),
        group_count: 0,
        reserved: 0,
    };
    assert_eq!(
        unsafe {
            pw_volume_create_with_metadata(
                s.path().as_ptr(),
                s.path().len(),
                1 << 20,
                &a,
                &mut volume,
            )
        },
        0
    );
    assert_eq!(
        unsafe { pw_posix_create(volume, 1, b"file".as_ptr(), 4, 0, 0o600, 0, &c, &mut node) },
        0
    );
    let mut file = 0;
    let mut writer = 0;
    let mut raw = 0;
    assert_eq!(
        unsafe { pw_posix_file_open(volume, 1, b"file".as_ptr(), 4, 6, &c, &mut file) },
        0
    );
    assert_eq!(
        unsafe { pw_posix_file_open(volume, 1, b"file".as_ptr(), 4, 2, &c, &mut writer) },
        0
    );
    assert_eq!(unsafe { pw_file_open(volume, node, &mut raw) }, 0);
    let mut count = 99;
    assert_eq!(
        unsafe { pw_posix_file_write(volume, file, 0, b"abcdef".as_ptr(), 6, &mut count) },
        0
    );
    let mut lease = 99;
    assert_eq!(
        unsafe { pw_posix_read_view_begin(volume, writer, 1, 3, &mut lease) },
        -17
    );
    assert_eq!(lease, 99);
    assert_eq!(
        unsafe { pw_posix_read_view_begin(volume, raw, 1, 3, &mut lease) },
        -1
    );
    assert_eq!(lease, 99);
    assert_eq!(
        unsafe { pw_read_view_begin(volume, file, 1, 3, &mut lease) },
        -1
    );
    assert_eq!(lease, 99);
    assert_eq!(
        unsafe { pw_posix_read_view_begin(volume, file, 1, 3, &mut lease) },
        0
    );
    let mut admin = 0;
    assert_eq!(
        unsafe { pw_read_view_begin(volume, raw, 0, 6, &mut admin) },
        0
    );
    let mut buffer = [77; 16];
    count = 99;
    assert_eq!(
        unsafe { pw_posix_read_view_read(volume, admin, 0, buffer.as_mut_ptr(), 3, &mut count) },
        -1
    );
    assert_eq!(count, 99);
    assert_eq!(buffer, [77; 16]);
    assert_eq!(
        unsafe { pw_read_view_read(volume, lease, 1, buffer.as_mut_ptr(), 3, &mut count) },
        -1
    );
    assert_eq!(count, 99);
    assert_eq!(buffer, [77; 16]);
    assert_eq!(
        unsafe { pw_posix_read_view_read(volume, lease, 0, buffer.as_mut_ptr(), 1, &mut count) },
        -1
    );
    assert_eq!(count, 99);
    assert_eq!(buffer, [77; 16]);
    assert_eq!(pw_posix_read_view_close(volume, admin), -1);
    assert_eq!(pw_read_view_close(volume, lease), -1);
    assert_eq!(pw_posix_file_truncate(volume, file, 0), 0);
    assert_eq!(pw_posix_file_close(volume, file), 0);
    assert_eq!(pw_posix_file_close(volume, writer), 0);
    assert_eq!(pw_file_close(volume, raw), 0);
    assert_eq!(
        unsafe { pw_posix_remove(volume, 1, b"file".as_ptr(), 4, 0, &c) },
        0
    );
    assert_eq!(pw_read_view_close(volume, admin), 0);
    assert_eq!(pw_volume_close(volume), -7);
    assert_eq!(
        unsafe { pw_posix_read_view_read(volume, lease, 1, buffer.as_mut_ptr(), 3, &mut count) },
        0
    );
    assert_eq!(&buffer[..count], b"bcd");
    assert_eq!(pw_posix_read_view_close(volume, lease), 0);
    assert_eq!(pw_posix_read_view_close(volume, lease), -1);
    assert_eq!(pw_volume_close(volume), 0);
}

#[test]
fn granted_tokens_do_not_confuse_administrative_handles_or_leak_failed_io_outputs() {
    let s = Scratch::new();
    let mut volume = 0;
    let a = PwCreateAttributes {
        uid: 100,
        gid: 20,
        mode: 0o777,
        reserved: 0,
    };
    let owner = PwCredentials {
        uid: 200,
        gid: 20,
        groups: std::ptr::null(),
        group_count: 0,
        reserved: 0,
    };
    let other = PwCredentials {
        uid: 300,
        gid: 20,
        groups: std::ptr::null(),
        group_count: 0,
        reserved: 0,
    };
    assert_eq!(
        unsafe {
            pw_volume_create_with_metadata(
                s.path().as_ptr(),
                s.path().len(),
                1 << 20,
                &a,
                &mut volume,
            )
        },
        0
    );
    let mut node = 0;
    assert_eq!(
        unsafe {
            pw_posix_create(
                volume,
                1,
                b"file".as_ptr(),
                4,
                0,
                0o600,
                0,
                &owner,
                &mut node,
            )
        },
        0
    );
    let mut denied = 99;
    assert_eq!(
        unsafe { pw_posix_file_open(volume, 1, b"file".as_ptr(), 4, 4, &other, &mut denied) },
        -17
    );
    assert_eq!(denied, 99);
    assert_eq!(
        unsafe { pw_posix_file_open(volume, 1, b"file".as_ptr(), 4, 1, &owner, &mut denied) },
        -1
    );
    assert_eq!(denied, 99);
    let mut reader = 0;
    let mut writer = 0;
    let mut raw = 0;
    assert_eq!(
        unsafe { pw_posix_file_open(volume, 1, b"file".as_ptr(), 4, 4, &owner, &mut reader) },
        0
    );
    assert_eq!(
        unsafe { pw_posix_file_open(volume, 1, b"file".as_ptr(), 4, 6, &owner, &mut writer) },
        0
    );
    assert_eq!(unsafe { pw_file_open(volume, node, &mut raw) }, 0);
    let mut count = 99;
    let mut buffer = [77; 16];
    assert_eq!(
        unsafe { pw_posix_file_write(volume, reader, 0, b"bad".as_ptr(), 3, &mut count) },
        -17
    );
    assert_eq!(count, 99);
    assert_eq!(
        unsafe {
            pw_posix_file_read(
                volume,
                raw,
                0,
                buffer.as_mut_ptr(),
                buffer.len(),
                &mut count,
            )
        },
        -1
    );
    assert_eq!(count, 99);
    assert_eq!(buffer, [77; 16]);
    assert_eq!(
        unsafe {
            pw_file_read(
                volume,
                reader,
                0,
                buffer.as_mut_ptr(),
                buffer.len(),
                &mut count,
            )
        },
        -1
    );
    assert_eq!(count, 99);
    assert_eq!(buffer, [77; 16]);
    assert_eq!(
        unsafe { pw_posix_file_write(volume, writer, 0, b"abcdef".as_ptr(), 6, &mut count) },
        0
    );
    let update = PwMetadataUpdate {
        valid: 4 | 32,
        mode: 0o6751,
        modified: PwTimestamp {
            seconds: -123,
            nanoseconds: 987_654_321,
            reserved: 0,
        },
        ..Default::default()
    };
    assert_eq!(
        unsafe { pw_posix_file_truncate_with_metadata(volume, writer, 3, &other, &update) },
        -17
    );
    assert_eq!(
        unsafe { pw_posix_file_truncate_with_metadata(volume, writer, 3, &owner, &update) },
        0
    );
    assert_eq!(pw_file_close(volume, reader), -1);
    assert_eq!(pw_posix_file_close(volume, raw), -1);
    assert_eq!(pw_volume_close(volume), -7);
    assert_eq!(pw_file_close(volume, raw), 0);
    assert_eq!(
        unsafe {
            pw_posix_file_read(
                volume,
                reader,
                0,
                buffer.as_mut_ptr(),
                buffer.len(),
                &mut count,
            )
        },
        0
    );
    assert_eq!(&buffer[..count], b"abc");
    assert_eq!(pw_posix_file_close(volume, reader), 0);
    assert_eq!(pw_posix_file_close(volume, reader), -1);
    assert_eq!(
        unsafe {
            pw_posix_file_read(
                volume,
                reader,
                0,
                buffer.as_mut_ptr(),
                buffer.len(),
                &mut count,
            )
        },
        -1
    );
    assert_eq!(pw_posix_file_close(volume, writer), 0);
    assert_eq!(pw_volume_close(volume), 0);
}

#[test]
fn posix_abi_bounds_claims_and_denial_leaves_outputs_and_health_unchanged() {
    let s = Scratch::new();
    let mut volume = 0;
    let attrs = PwCreateAttributes {
        uid: 100,
        gid: 20,
        mode: 0o1777,
        reserved: 0,
    };
    assert_eq!(
        unsafe {
            pw_volume_create_with_metadata(
                s.path().as_ptr(),
                s.path().len(),
                1 << 20,
                &attrs,
                &mut volume,
            )
        },
        0
    );
    let mut claims = PwCredentials {
        uid: 200,
        gid: 50,
        groups: std::ptr::null(),
        group_count: 0,
        reserved: 0,
    };
    let mut node = 99;
    assert_eq!(
        unsafe {
            pw_posix_create(
                volume,
                1,
                b"file".as_ptr(),
                4,
                0,
                0o666,
                0o027,
                std::ptr::null(),
                &mut node,
            )
        },
        -1
    );
    assert_eq!(node, 99);
    claims.group_count = 129;
    assert_eq!(
        unsafe {
            pw_posix_create(
                volume,
                1,
                b"file".as_ptr(),
                4,
                0,
                0o666,
                0o027,
                &claims,
                &mut node,
            )
        },
        -14
    );
    assert_eq!(node, 99);
    claims.group_count = 1;
    assert_eq!(
        unsafe {
            pw_posix_create(
                volume,
                1,
                b"file".as_ptr(),
                4,
                0,
                0o666,
                0o027,
                &claims,
                &mut node,
            )
        },
        -1
    );
    claims.group_count = 0;
    claims.reserved = 1;
    assert_eq!(
        unsafe {
            pw_posix_create(
                volume,
                1,
                b"file".as_ptr(),
                4,
                0,
                0o666,
                0o027,
                &claims,
                &mut node,
            )
        },
        -1
    );
    claims.reserved = 0;
    assert_eq!(
        unsafe {
            pw_posix_create(
                volume,
                1,
                b"file".as_ptr(),
                4,
                0,
                0o666,
                0o027,
                &claims,
                &mut node,
            )
        },
        0
    );
    claims.uid = 300;
    assert_eq!(
        unsafe { pw_posix_check_access(volume, 1, b"file".as_ptr(), 4, 8, &claims) },
        -1
    );
    assert_eq!(
        unsafe { pw_posix_remove(volume, 1, b"file".as_ptr(), 4, 0, &claims) },
        -17
    );
    assert_eq!(
        unsafe { pw_posix_check_access(volume, 1, b"file".as_ptr(), 4, 2, &claims) },
        -17
    );
    let mut health = 99;
    let mut protected = 99;
    assert_eq!(unsafe { pw_health(volume, &mut health, &mut protected) }, 0);
    assert_eq!((health, protected), (0, 0));
    let mut found = 0;
    assert_eq!(
        unsafe { pw_lookup(volume, 1, b"file".as_ptr(), 4, &mut found) },
        0
    );
    assert_eq!(found, node);
    assert_eq!(pw_volume_close(volume), 0);
}

#[test]
fn metadata_abi_rejects_unknown_fields_preserves_outputs_and_keeps_v2_identity() {
    let s = Scratch::new();
    let mut volume = 77;
    let mut a = PwCreateAttributes {
        uid: u32::MAX,
        gid: 20,
        mode: 0o751,
        reserved: 1,
    };
    assert_eq!(
        unsafe {
            pw_volume_create_with_metadata(
                s.path().as_ptr(),
                s.path().len(),
                100000,
                &a,
                &mut volume,
            )
        },
        -1
    );
    assert_eq!(volume, 77);
    assert!(!s.0.exists());
    a.reserved = 0;
    assert_eq!(
        unsafe {
            pw_volume_create_with_metadata(
                s.path().as_ptr(),
                s.path().len(),
                100000,
                &a,
                &mut volume,
            )
        },
        0
    );
    let mut identity = PwVolumeIdentity::default();
    assert_eq!(unsafe { pw_volume_identity(volume, &mut identity) }, 0);
    let original = identity;
    let mut node = 99;
    assert_eq!(
        unsafe { pw_node_create_with_metadata(volume, 1, b"tool".as_ptr(), 4, 2, &a, &mut node) },
        -1
    );
    assert_eq!(node, 99);
    assert_eq!(
        unsafe { pw_node_create_with_metadata(volume, 1, b"tool".as_ptr(), 4, 0, &a, &mut node) },
        0
    );
    let mut meta = PwMetadata::default();
    assert_eq!(unsafe { pw_metadata_get(volume, node, &mut meta) }, 0);
    assert_eq!(meta.uid, u32::MAX);
    assert_eq!(meta.mode, 0o751);
    let old = meta;
    for update in [
        PwMetadataUpdate {
            valid: 64,
            ..Default::default()
        },
        PwMetadataUpdate {
            valid: 4,
            mode: 0o10000,
            ..Default::default()
        },
        PwMetadataUpdate {
            valid: 16,
            accessed: PwTimestamp {
                nanoseconds: 1_000_000_000,
                ..Default::default()
            },
            ..Default::default()
        },
        PwMetadataUpdate {
            reserved: 1,
            ..Default::default()
        },
    ] {
        assert_eq!(unsafe { pw_metadata_set(volume, node, &update) }, -1);
        assert_eq!(unsafe { pw_metadata_get(volume, node, &mut meta) }, 0);
        assert_eq!(meta, old);
    }
    assert_eq!(unsafe { pw_metadata_get(volume, -1, &mut meta) }, -1);
    assert_eq!(meta, old);
    let update = PwMetadataUpdate {
        valid: 2 | 16,
        gid: 80,
        accessed: PwTimestamp {
            seconds: -123,
            nanoseconds: 999_999_999,
            reserved: 0,
        },
        ..Default::default()
    };
    assert_eq!(unsafe { pw_metadata_set(volume, node, &update) }, 0);
    assert_eq!(pw_volume_close(volume), 0);
    assert_eq!(
        unsafe { pw_volume_open(s.path().as_ptr(), s.path().len(), &mut volume) },
        0
    );
    assert_eq!(unsafe { pw_volume_identity(volume, &mut identity) }, 0);
    assert_eq!(identity, original);
    assert_eq!(unsafe { pw_metadata_get(volume, node, &mut meta) }, 0);
    assert_eq!(meta.gid, 80);
    assert_eq!(meta.accessed, update.accessed);
    assert_eq!(pw_volume_close(volume), 0);
}
static NEXT: AtomicU64 = AtomicU64::new(1);

#[test]
fn canonical_alias_spelling_directory_parent_and_snapshot_seek_are_volume_scoped() {
    let s = Scratch::new();
    let mut volume = 0;
    assert_eq!(
        unsafe { pw_volume_create(s.path().as_ptr(), s.path().len(), 100_000_000, &mut volume) },
        0
    );
    let mut file = 0_i64;
    let name = b"MiXeD.txt";
    assert_eq!(
        unsafe { pw_file_create(volume, 1, name.as_ptr(), name.len(), &mut file) },
        0
    );
    assert_eq!(
        unsafe { pw_hard_link(volume, file, 1, b"Alias.txt".as_ptr(), 9) },
        0
    );
    let mut buf = [0_u8; 255];
    let mut count = 0;
    let mut found = 77;
    assert_eq!(
        unsafe {
            pw_lookup_entry(
                volume,
                1,
                b"ALIAS.TXT".as_ptr(),
                9,
                buf.as_mut_ptr(),
                1,
                &mut count,
                &mut found,
            )
        },
        -16
    );
    assert_eq!(count, 9);
    assert_eq!(found, 77);
    assert_eq!(
        unsafe {
            pw_lookup_entry(
                volume,
                1,
                b"ALIAS.TXT".as_ptr(),
                9,
                buf.as_mut_ptr(),
                buf.len(),
                &mut count,
                &mut found,
            )
        },
        0
    );
    assert_eq!(&buf[..count], b"Alias.txt");
    assert_eq!(found, file);
    assert_eq!(
        unsafe {
            pw_lookup_entry(
                volume,
                1,
                b"mixed.txt".as_ptr(),
                9,
                buf.as_mut_ptr(),
                buf.len(),
                &mut count,
                &mut found,
            )
        },
        0
    );
    assert_eq!(&buf[..count], b"MiXeD.txt");
    let mut parent = 88;
    assert_eq!(unsafe { pw_parent_directory(volume, 1, &mut parent) }, 0);
    assert_eq!(parent, 1);
    assert_eq!(
        unsafe { pw_parent_directory(volume, file, &mut parent) },
        -4
    );
    assert_eq!(parent, 1);
    let mut cursor = 0;
    assert_eq!(unsafe { pw_dir_begin(volume, 1, &mut cursor) }, 0);
    assert_eq!(pw_dir_seek(volume, cursor, 2), 0);
    assert_eq!(
        unsafe {
            pw_dir_next(
                volume,
                cursor,
                buf.as_mut_ptr(),
                buf.len(),
                &mut count,
                &mut found,
            )
        },
        1
    );
    assert_eq!(pw_dir_seek(volume, cursor, u64::MAX), -1);
    assert_eq!(pw_dir_seek(volume, cursor, 0), 0);
    assert_eq!(
        unsafe {
            pw_dir_next(
                volume,
                cursor,
                buf.as_mut_ptr(),
                buf.len(),
                &mut count,
                &mut found,
            )
        },
        0
    );
    assert_eq!(&buf[..count], b"Alias.txt");
    for _ in 0..2 {
        assert_eq!(pw_dir_seek(volume, cursor, 1), 0);
        assert_eq!(
            unsafe {
                pw_dir_next(
                    volume,
                    cursor,
                    buf.as_mut_ptr(),
                    buf.len(),
                    &mut count,
                    &mut found,
                )
            },
            0
        );
        assert_eq!(&buf[..count], b"MiXeD.txt");
    }
    assert_eq!(pw_dir_seek(0, cursor, 0), -1);
    assert_eq!(pw_dir_close(volume, cursor), 0);
    assert_eq!(pw_dir_seek(volume, cursor, 0), -1);
    assert_eq!(pw_volume_close(volume), 0);
}
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        Self(std::env::temp_dir().join(format!("pw-native-{}-{now}-{n}", std::process::id())))
    }
    fn path(&self) -> &[u8] {
        self.0.to_str().unwrap().as_bytes()
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        if self.0.exists() {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
}
#[test]
fn native_abi_writes_and_reopens_real_core_container() {
    let s = Scratch::new();
    let mut volume = 0_u64;
    assert_eq!(pw_abi_version(), 1);
    assert_eq!(pw_root_id(), 1);
    assert_eq!(
        unsafe { pw_volume_create(s.path().as_ptr(), s.path().len(), 100_000_000, &mut volume) },
        0
    );
    let root = pw_root_id();
    let mut dir = 0_i64;
    assert_eq!(
        unsafe { pw_directory_create(volume, root, b"src".as_ptr(), 3, &mut dir) },
        0
    );
    let name = "数据.rs".as_bytes();
    let mut object = 0_i64;
    assert_eq!(
        unsafe { pw_file_create(volume, dir, name.as_ptr(), name.len(), &mut object) },
        0
    );
    let mut file = 0_u64;
    assert_eq!(unsafe { pw_file_open(volume, object, &mut file) }, 0);
    assert_eq!(pw_volume_close(volume), -7); // no silent drop while file open
    let payload = b"working code";
    let mut written = 0;
    assert_eq!(
        unsafe {
            pw_file_write(
                volume,
                file,
                0,
                payload.as_ptr(),
                payload.len(),
                &mut written,
            )
        },
        0
    );
    assert_eq!(written, payload.len());
    let mut output = [0_u8; 32];
    let mut read = 0;
    assert_eq!(
        unsafe {
            pw_file_read(
                volume,
                file,
                0,
                output.as_mut_ptr(),
                output.len(),
                &mut read,
            )
        },
        0
    );
    assert_eq!(&output[..read], payload);
    assert_eq!(pw_file_truncate(volume, file, 4), 0);
    assert_eq!(
        unsafe {
            pw_file_read(
                volume,
                file,
                0,
                output.as_mut_ptr(),
                output.len(),
                &mut read,
            )
        },
        0
    );
    assert_eq!(&output[..read], b"work");
    let mut space = PwSpace::default();
    assert_eq!(unsafe { pw_volume_space(volume, &mut space) }, 0);
    assert_eq!(space.logical_quota_bytes, 100_000_000);
    assert_eq!(space.logical_used_bytes, 4);
    assert_eq!(
        space.apparent_free_bytes,
        (100_000_000_u64 - 4).min(space.host_data_admission_bytes)
    );
    let mut checkpoint = PwCheckpoint::default();
    assert_eq!(unsafe { pw_catalog_checkpoint(volume, &mut checkpoint) }, 0);
    assert_eq!(checkpoint.wal_frames, checkpoint.checkpointed_frames);
    let mut status = PwStat::default();
    assert_eq!(unsafe { pw_stat(volume, object, &mut status) }, 0);
    assert_eq!(status.kind, 0);
    assert_eq!(status.size, 4);
    assert_eq!(
        unsafe { pw_hard_link(volume, object, dir, b"alias".as_ptr(), 5) },
        0
    );
    assert_eq!(unsafe { pw_stat(volume, object, &mut status) }, 0);
    assert_eq!(status.links, 2);
    assert_eq!(pw_file_close(volume, file), 0);
    assert_eq!(
        unsafe {
            pw_rename(
                volume,
                dir,
                b"alias".as_ptr(),
                5,
                dir,
                b"newalias".as_ptr(),
                8,
                0,
            )
        },
        0
    );
    let mut cursor = 0_u64;
    assert_eq!(unsafe { pw_dir_begin(volume, dir, &mut cursor) }, 0);
    let mut buf = [0_u8; 2];
    let mut required = 0_usize;
    let mut entry_id = 0_i64;
    assert_eq!(
        unsafe {
            pw_dir_next(
                volume,
                cursor,
                buf.as_mut_ptr(),
                buf.len(),
                &mut required,
                &mut entry_id,
            )
        },
        -16
    );
    assert!(required > buf.len());
    let mut names = Vec::new();
    loop {
        let mut big = [0_u8; 1024];
        let step = unsafe {
            pw_dir_next(
                volume,
                cursor,
                big.as_mut_ptr(),
                big.len(),
                &mut required,
                &mut entry_id,
            )
        };
        if step == 1 {
            break;
        }
        assert_eq!(step, 0);
        names.push(String::from_utf8(big[..required].to_vec()).unwrap());
    }
    assert_eq!(
        names,
        vec!["newalias".to_owned(), "数据.rs".to_owned()]
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>()
    );
    assert_eq!(pw_volume_close(volume), -7); // cursor pins lifecycle
    assert_eq!(pw_dir_close(volume, cursor), 0);
    let mut maintenance = 99_u32;
    let mut protected = 99_u32;
    assert_eq!(
        unsafe { pw_health(volume, &mut maintenance, &mut protected) },
        0
    );
    assert_eq!((maintenance, protected), (0, 0));
    assert_eq!(pw_volume_close(volume), 0);
    assert_eq!(
        unsafe { pw_volume_open(s.path().as_ptr(), s.path().len(), &mut volume) },
        0
    );
    let mut recovered = 0;
    assert_eq!(
        unsafe { pw_lookup(volume, dir, name.as_ptr(), name.len(), &mut recovered) },
        0
    );
    assert_eq!(recovered, object);
    assert_eq!(
        unsafe { pw_remove(volume, dir, name.as_ptr(), name.len(), 0) },
        0
    );
    assert_eq!(
        unsafe { pw_remove(volume, dir, b"newalias".as_ptr(), 8, 0) },
        0
    );
    assert_eq!(unsafe { pw_remove(volume, root, b"src".as_ptr(), 3, 1) }, 0);
    assert_eq!(pw_volume_close(volume), 0);
}
#[test]
fn invalid_inputs_do_not_create_files_and_session_tokens_are_not_reused() {
    let s = Scratch::new();
    assert_eq!(
        unsafe {
            pw_volume_create(
                s.path().as_ptr(),
                s.path().len(),
                1000,
                std::ptr::null_mut(),
            )
        },
        -1
    );
    assert!(!s.0.exists());
    let mut volume = 0;
    assert_eq!(
        unsafe { pw_volume_create(s.path().as_ptr(), s.path().len(), 1024, &mut volume) },
        0
    );
    let mut second = 0;
    assert_eq!(
        unsafe { pw_volume_open(s.path().as_ptr(), s.path().len(), &mut second) },
        -7
    );
    let mut file = 0;
    assert_eq!(
        unsafe { pw_file_create(volume, 1, std::ptr::null(), 4, &mut file) },
        -1
    );
    assert_eq!(
        unsafe { pw_file_create(volume, -1, b"bad".as_ptr(), 3, &mut file) },
        -1
    );
    assert_eq!(
        unsafe { pw_file_create(volume, 1, b"ok".as_ptr(), 2, &mut file) },
        0
    );
    let mut handle = 0;
    assert_eq!(unsafe { pw_file_open(volume, file, &mut handle) }, 0);
    assert_eq!(
        unsafe { pw_file_read(volume, handle, 0, std::ptr::null_mut(), 5, &mut 0_usize) },
        -1
    );
    assert_eq!(
        unsafe { pw_file_write(volume, handle, 0, std::ptr::null(), 9, &mut 0_usize) },
        -1
    );
    assert_eq!(pw_file_close(volume, handle), 0);
    assert_eq!(pw_volume_close(volume), 0);
    assert_eq!(pw_volume_close(volume), -1);
    assert_eq!(
        unsafe { pw_volume_open(s.path().as_ptr(), s.path().len(), &mut second) },
        0
    );
    assert_ne!(volume, second);
    assert_eq!(pw_volume_close(second), 0);
}

fn request_volume(s: &Scratch) -> (u64, u64, u64, i64) {
    request_volume_format(s, false)
}
fn request_volume_format(s: &Scratch, directory_v3: bool) -> (u64, u64, u64, i64) {
    let mut volume = 0;
    let mut object = 0;
    let mut writer = 0;
    let mut reader = 0;
    let attrs = PwCreateAttributes {
        uid: 100,
        gid: 20,
        mode: 0o777,
        reserved: 0,
    };
    let c = PwCredentials {
        uid: 100,
        gid: 20,
        groups: std::ptr::null(),
        group_count: 0,
        reserved: 0,
    };
    let create: unsafe extern "C" fn(
        *const u8,
        usize,
        u64,
        *const PwCreateAttributes,
        *mut u64,
    ) -> i32 = if directory_v3 {
        pw_volume_create_with_directory_lifetimes
    } else {
        pw_volume_create_with_metadata
    };
    unsafe {
        assert_eq!(
            create(
                s.path().as_ptr(),
                s.path().len(),
                1 << 24,
                &attrs,
                &mut volume
            ),
            0
        );
        assert_eq!(
            pw_posix_create(volume, 1, b"file".as_ptr(), 4, 0, 0o600, 0, &c, &mut object),
            0
        );
        assert_eq!(
            pw_posix_file_open(volume, 1, b"file".as_ptr(), 4, 6, &c, &mut writer),
            0
        );
        assert_eq!(
            pw_posix_file_open(volume, 1, b"file".as_ptr(), 4, 4, &c, &mut reader),
            0
        );
    }
    (volume, writer, reader, object)
}

#[test]
fn atomic_write_request_abi_has_typed_tokens_preflight_and_unchanged_error_outputs() {
    let s = Scratch::new();
    let (volume, writer, reader, object) = request_volume(&s);
    let mut info = PwFileInfo::default();
    assert_eq!(unsafe { pw_posix_file_info(volume, reader, &mut info) }, 0);
    assert_eq!(info.object, object);
    assert_eq!(info.metadata.uid, 100);
    let old = info;
    let mut result = PwWriteResult {
        written: 55,
        offset: 77,
        info: old,
    };
    let sentinel = result;
    for (file, flags, pointer, length, out, expected) in [
        (reader, 1, b"".as_ptr(), 0, &mut result as *mut _, -17),
        (writer, 4, b"x".as_ptr(), 1, &mut result as *mut _, -1),
        (writer, 0, std::ptr::null(), 1, &mut result as *mut _, -1),
        (writer, 0, b"x".as_ptr(), 1, std::ptr::null_mut(), -1),
        (
            writer,
            3,
            b"x".as_ptr(),
            16 * 1024 * 1024 + 1,
            &mut result as *mut _,
            -1,
        ),
    ] {
        assert_eq!(
            unsafe { pw_posix_file_write_request(volume, file, 0, pointer, length, flags, out) },
            expected
        );
        assert_eq!(result, sentinel);
    }
    assert_eq!(unsafe { pw_posix_file_info(volume, writer, &mut info) }, 0);
    assert_eq!(info, old);
    assert_eq!(
        unsafe { pw_posix_file_truncate_info(volume, reader, 0, &mut info) },
        -17
    );
    assert_eq!(info, old);
    assert_eq!(
        unsafe { pw_posix_file_truncate_info(volume, writer, 10, std::ptr::null_mut()) },
        -1
    );
    let mut raw = 0;
    assert_eq!(unsafe { pw_file_open(volume, object, &mut raw) }, 0);
    assert_eq!(unsafe { pw_posix_file_info(volume, raw, &mut info) }, -1);
    assert_eq!(info, old);
    assert_eq!(
        unsafe { pw_posix_file_write_request(volume, raw, 0, b"x".as_ptr(), 1, 0, &mut result) },
        -1
    );
    assert_eq!(result, sentinel);
    assert_eq!(pw_file_close(volume, raw), 0);
    assert_eq!(
        unsafe {
            pw_posix_file_write_request(
                volume,
                writer,
                u64::MAX,
                b"abcdef".as_ptr(),
                6,
                1,
                &mut result,
            )
        },
        0
    );
    assert_eq!(
        (result.offset, result.written, result.info.stat.size),
        (0, 6, 6)
    );
    assert_eq!(unsafe { pw_posix_file_info(volume, reader, &mut info) }, 0);
    assert_eq!(result.info, info);
    assert_eq!(
        unsafe {
            pw_posix_file_write_request(volume, writer, u64::MAX, b"no".as_ptr(), 2, 3, &mut result)
        },
        0
    );
    assert_eq!((result.offset, result.written), (6, 0));
    assert_eq!(result.info, info);
    assert_eq!(
        unsafe {
            pw_posix_file_write_request(volume, writer, 5, b"XY".as_ptr(), 2, 2, &mut result)
        },
        0
    );
    assert_eq!((result.written, result.info.stat.size), (1, 6));
    assert_eq!(
        unsafe { pw_posix_file_truncate_info(volume, writer, 3, &mut info) },
        0
    );
    assert_eq!(info.stat.size, 3);
    assert_eq!(info.stat.generation, result.info.stat.generation + 1);
    assert_eq!(
        unsafe { pw_posix_file_info(volume, writer, &mut result.info) },
        0
    );
    assert_eq!(result.info, info);
    assert_eq!(
        unsafe {
            pw_posix_file_write_request(volume, writer, 0, std::ptr::null(), 0, 1, &mut result)
        },
        0
    );
    assert_eq!((result.offset, result.written), (3, 0));
    assert_eq!(result.info, info);
    assert_eq!(pw_posix_file_close(volume, writer), 0);
    assert_eq!(unsafe { pw_posix_file_info(volume, writer, &mut info) }, -1);
    assert_eq!(info, result.info);
    assert_eq!(pw_posix_file_close(volume, reader), 0);
    assert_eq!(pw_volume_close(volume), 0);
}

#[test]
fn concurrent_native_append_resolves_eof_inside_the_serialized_owner() {
    let s = Scratch::new();
    let (volume, writer, reader, _) = request_volume(&s);
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(6));
    let threads: Vec<_> = (0_u64..6)
        .map(|worker| {
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                let mut positions = Vec::new();
                for record in 0_u64..16 {
                    let bytes = (worker * 16 + record).to_le_bytes();
                    let mut result = PwWriteResult::default();
                    assert_eq!(
                        unsafe {
                            pw_posix_file_write_request(
                                volume,
                                writer,
                                u64::MAX,
                                bytes.as_ptr(),
                                bytes.len(),
                                1,
                                &mut result,
                            )
                        },
                        0
                    );
                    assert_eq!(result.written, 8);
                    assert_eq!(result.info.stat.size, result.offset + 8);
                    positions.push(result.offset);
                }
                positions
            })
        })
        .collect();
    let mut positions = Vec::new();
    for thread in threads {
        positions.extend(thread.join().unwrap());
    }
    positions.sort_unstable();
    assert_eq!(positions, (0_u64..96).map(|n| n * 8).collect::<Vec<_>>());
    let mut bytes = vec![0_u8; 768];
    let mut count = 0;
    assert_eq!(
        unsafe {
            pw_posix_file_read(
                volume,
                reader,
                0,
                bytes.as_mut_ptr(),
                bytes.len(),
                &mut count,
            )
        },
        0
    );
    assert_eq!(count, 768);
    let mut records: Vec<u64> = bytes
        .as_chunks::<8>()
        .0
        .iter()
        .map(|b| u64::from_le_bytes(*b))
        .collect();
    records.sort_unstable();
    assert_eq!(records, (0..96).collect::<Vec<_>>());
    assert_eq!(pw_posix_file_close(volume, writer), 0);
    assert_eq!(pw_posix_file_close(volume, reader), 0);
    assert_eq!(pw_volume_close(volume), 0);
}

#[test]
fn windows_projection_io_rolls_back_unrepresentable_replies_and_keeps_exact_core_metadata() {
    let s = Scratch::new();
    let (volume, writer, reader, object) = request_volume(&s);
    let mut stored = PwFileInfo::default();
    let mut info = PwWindowsFileInfo::default();
    let mut update = PwMetadataUpdate {
        valid: 8 | 16 | 32,
        created: PwTimestamp {
            seconds: -1,
            nanoseconds: 999_999_999,
            reserved: 0,
        },
        accessed: PwTimestamp {
            seconds: 0,
            nanoseconds: 123_456_789,
            reserved: 0,
        },
        modified: PwTimestamp {
            seconds: 0,
            nanoseconds: 99,
            reserved: 0,
        },
        ..Default::default()
    };
    assert_eq!(unsafe { pw_metadata_set(volume, object, &update) }, 0);
    assert_eq!(
        unsafe { pw_windows_file_info(volume, reader, &mut info) },
        0
    );
    assert_eq!(info.index_number, object as u64);
    assert_eq!(info.file_size, 0);
    assert_eq!(info.allocation_size, 0);
    assert_eq!(info.creation_time, 116_444_735_999_999_999);
    assert_eq!(info.last_access_time, 116_444_736_001_234_567);
    assert_eq!(info.last_write_time, 116_444_736_000_000_000);
    assert_eq!(
        unsafe { pw_posix_file_info(volume, reader, &mut stored) },
        0
    );
    assert_eq!(stored.metadata.created, update.created);
    assert_eq!(stored.metadata.accessed, update.accessed);
    let mut result = PwWindowsWriteResult::default();
    assert_eq!(
        unsafe {
            pw_windows_file_write(volume, writer, u64::MAX, b"abc".as_ptr(), 3, 1, &mut result)
        },
        0
    );
    assert_eq!(
        (
            result.written,
            result.offset,
            result.info.file_size,
            result.info.allocation_size
        ),
        (3, 0, 3, 4096)
    );
    assert_eq!(
        unsafe { pw_windows_file_info(volume, writer, &mut info) },
        0
    );
    assert_eq!(info, result.info);
    assert_eq!(
        unsafe { pw_windows_file_write(volume, writer, 2, b"XYZ".as_ptr(), 3, 2, &mut result) },
        0
    );
    assert_eq!((result.written, result.info.file_size), (1, 3));
    assert_eq!(
        unsafe { pw_windows_file_set_size(volume, writer, 4097, &mut info) },
        0
    );
    assert_eq!((info.file_size, info.allocation_size), (4097, 8192));
    assert_eq!(
        unsafe { pw_windows_file_set_size(volume, writer, 3, &mut info) },
        0
    );
    update.valid = 8;
    update.created.seconds = -11_644_473_601;
    assert_eq!(unsafe { pw_metadata_set(volume, object, &update) }, 0);
    assert_eq!(
        unsafe { pw_posix_file_info(volume, reader, &mut stored) },
        0
    );
    let old = stored;
    let sentinel = result;
    let old_info = info;
    assert_eq!(
        unsafe { pw_windows_file_info(volume, reader, &mut info) },
        -11
    );
    assert_eq!(info, old_info);
    assert_eq!(
        unsafe {
            pw_windows_file_write(volume, writer, u64::MAX, b"bad".as_ptr(), 3, 1, &mut result)
        },
        -11
    );
    assert_eq!(result, sentinel);
    assert_eq!(
        unsafe { pw_windows_file_set_size(volume, writer, 0, &mut info) },
        -11
    );
    assert_eq!(info, old_info);
    assert_eq!(
        unsafe { pw_posix_file_info(volume, writer, &mut stored) },
        0
    );
    assert_eq!(stored, old);
    let mut bytes = [0; 10];
    let mut count = 99;
    assert_eq!(
        unsafe { pw_posix_file_read(volume, reader, 0, bytes.as_mut_ptr(), 10, &mut count) },
        0
    );
    assert_eq!(&bytes[..count], b"abX");
    let (mut maintenance, mut protected) = (9, 9);
    assert_eq!(
        unsafe { pw_health(volume, &mut maintenance, &mut protected) },
        0
    );
    assert_eq!((maintenance, protected), (0, 0));
    update.created.seconds = 0;
    update.created.nanoseconds = 0;
    assert_eq!(unsafe { pw_metadata_set(volume, object, &update) }, 0);
    assert_eq!(
        unsafe { pw_windows_file_set_size(volume, writer, 4, &mut info) },
        0
    );
    assert_eq!(pw_posix_file_close(volume, writer), 0);
    assert_eq!(pw_posix_file_close(volume, reader), 0);
    assert_eq!(pw_volume_close(volume), 0);
}

#[test]
fn windows_io_rejects_read_only_admin_stale_and_invalid_requests_before_mutation() {
    let s = Scratch::new();
    let (volume, writer, reader, object) = request_volume(&s);
    let mut info = PwWindowsFileInfo::default();
    let mut result = PwWindowsWriteResult::default();
    assert_eq!(
        unsafe { pw_windows_file_info(volume, reader, &mut info) },
        0
    );
    let old = info;
    let sentinel = result;
    assert_eq!(
        unsafe { pw_windows_file_write(volume, reader, 0, std::ptr::null(), 0, 1, &mut result) },
        -17
    );
    assert_eq!(
        unsafe { pw_windows_file_set_size(volume, reader, 0, &mut info) },
        -17
    );
    assert_eq!(
        unsafe {
            pw_windows_file_write(volume, writer, 0, b"x".as_ptr(), 1, 0, std::ptr::null_mut())
        },
        -1
    );
    assert_eq!(
        unsafe { pw_windows_file_write(volume, writer, 0, std::ptr::null(), 1, 0, &mut result) },
        -1
    );
    assert_eq!(
        unsafe { pw_windows_file_write(volume, writer, 0, b"x".as_ptr(), 1, 4, &mut result) },
        -1
    );
    assert_eq!(
        unsafe { pw_windows_file_set_size(volume, writer, 10, std::ptr::null_mut()) },
        -1
    );
    assert_eq!(result, sentinel);
    assert_eq!(info, old);
    let mut raw = 0;
    assert_eq!(unsafe { pw_file_open(volume, object, &mut raw) }, 0);
    assert_eq!(unsafe { pw_windows_file_info(volume, raw, &mut info) }, -1);
    assert_eq!(
        unsafe { pw_windows_file_write(volume, raw, 0, b"x".as_ptr(), 1, 0, &mut result) },
        -1
    );
    assert_eq!(
        unsafe { pw_windows_file_set_size(volume, raw, 1, &mut info) },
        -1
    );
    assert_eq!(pw_file_close(volume, raw), 0);
    assert_eq!(
        unsafe { pw_windows_file_info(volume, writer, &mut info) },
        0
    );
    assert_eq!(info, old);
    assert_eq!(pw_posix_file_close(volume, writer), 0);
    assert_eq!(
        unsafe { pw_windows_file_info(volume, writer, &mut info) },
        -1
    );
    assert_eq!(info, old);
    assert_eq!(pw_posix_file_close(volume, reader), 0);
    assert_eq!(pw_volume_close(volume), 0);
}

#[test]
fn fskit_admin_setters_capture_attributes_and_reject_policy_tokens_or_wrong_objects() {
    let s = Scratch::new();
    let (volume, writer, reader, object) = request_volume(&s);
    let mut raw = 0;
    assert_eq!(unsafe { pw_file_open(volume, object, &mut raw) }, 0);
    let mut count = 0;
    assert_eq!(
        unsafe { pw_file_write(volume, raw, 0, b"abcdef".as_ptr(), 6, &mut count) },
        0
    );
    let mut before = PwFileInfo::default();
    assert_eq!(
        unsafe { pw_posix_file_info(volume, reader, &mut before) },
        0
    );
    let mut update = PwMetadataUpdate {
        valid: 4 | 32,
        mode: 0o6751,
        modified: PwTimestamp {
            seconds: -123,
            nanoseconds: 999_999_999,
            reserved: 0,
        },
        ..Default::default()
    };
    let mut reply = PwFsKitAttributes {
        size: 77,
        ..Default::default()
    };
    let sentinel = reply;
    assert_eq!(
        unsafe { pw_fskit_file_set_attributes(volume, writer, object, 0, &update, &mut reply) },
        -1
    );
    assert_eq!(
        unsafe { pw_fskit_file_set_attributes(volume, raw, object + 1, 0, &update, &mut reply) },
        -1
    );
    assert_eq!(reply, sentinel);
    let mut after = PwFileInfo::default();
    assert_eq!(unsafe { pw_posix_file_info(volume, reader, &mut after) }, 0);
    assert_eq!(after, before);
    assert_eq!(
        unsafe {
            pw_fskit_file_set_attributes(volume, raw, object, 0, &update, std::ptr::null_mut())
        },
        -1
    );
    assert_eq!(
        unsafe {
            pw_fskit_file_set_attributes(volume, raw, object, 0, std::ptr::null(), &mut reply)
        },
        -1
    );
    assert_eq!(reply, sentinel);
    assert_eq!(
        unsafe { pw_fskit_file_set_attributes(volume, raw, object, 3, &update, &mut reply) },
        0
    );
    assert_eq!(
        (
            reply.file_id,
            reply.kind,
            reply.size,
            reply.links,
            reply.metadata_valid,
            reply.reserved
        ),
        (object as u64 + 1, 0, 3, 1, 1, 0)
    );
    assert_eq!(reply.metadata.mode, 0o751);
    assert_eq!(reply.metadata.modified, update.modified);
    assert_eq!(unsafe { pw_posix_file_info(volume, reader, &mut after) }, 0);
    assert_eq!(after.stat.generation, before.stat.generation + 1);
    assert_eq!(reply.metadata, after.metadata);
    update = PwMetadataUpdate::default();
    let old_reply = reply;
    assert_eq!(
        unsafe { pw_fskit_metadata_set(volume, object, &update, &mut reply) },
        0
    );
    assert_eq!(reply, old_reply);
    assert_eq!(
        unsafe { pw_posix_file_info(volume, reader, &mut before) },
        0
    );
    assert_eq!(before, after);
    update.valid = 4;
    update.mode = 0o710;
    assert_eq!(
        unsafe { pw_fskit_metadata_set(volume, 1, &update, &mut reply) },
        0
    );
    assert_eq!(
        (reply.file_id, reply.kind, reply.links, reply.metadata.mode),
        (2, 1, 0, 0o710)
    );
    assert_eq!(pw_file_close(volume, raw), 0);
    assert_eq!(
        unsafe { pw_fskit_file_set_attributes(volume, raw, object, 0, &update, &mut reply) },
        -1
    );
    assert_eq!(pw_posix_file_close(volume, reader), 0);
    assert_eq!(pw_posix_file_close(volume, writer), 0);
    assert_eq!(pw_volume_close(volume), 0);
}

#[test]
fn fskit_v1_size_reply_does_not_invent_metadata_or_accept_nonempty_updates() {
    let s = Scratch::new();
    let mut volume = 0;
    let mut object = 0;
    let mut raw = 0;
    assert_eq!(
        unsafe { pw_volume_create(s.path().as_ptr(), s.path().len(), 1024, &mut volume) },
        0
    );
    assert_eq!(
        unsafe { pw_file_create(volume, 1, b"file".as_ptr(), 4, &mut object) },
        0
    );
    assert_eq!(unsafe { pw_file_open(volume, object, &mut raw) }, 0);
    let mut update = PwMetadataUpdate::default();
    let mut reply = PwFsKitAttributes::default();
    assert_eq!(
        unsafe { pw_fskit_file_set_attributes(volume, raw, object, 10, &update, &mut reply) },
        0
    );
    assert_eq!(
        (reply.size, reply.file_id, reply.metadata_valid),
        (10, object as u64 + 1, 0)
    );
    assert_eq!(reply.metadata, PwMetadata::default());
    let old = reply;
    update.valid = 4;
    update.mode = 0o777;
    assert_eq!(
        unsafe { pw_fskit_file_set_attributes(volume, raw, object, 0, &update, &mut reply) },
        -11
    );
    assert_eq!(reply, old);
    assert_eq!(
        unsafe { pw_fskit_metadata_set(volume, object, &update, &mut reply) },
        -11
    );
    assert_eq!(reply, old);
    update = PwMetadataUpdate::default();
    assert_eq!(
        unsafe { pw_fskit_metadata_set(volume, object, &update, &mut reply) },
        0
    );
    assert_eq!(reply, old);
    assert_eq!(pw_file_close(volume, raw), 0);
    assert_eq!(pw_volume_close(volume), 0);
}

#[test]
fn atomic_fskit_create_publishes_one_admin_pin_and_keeps_failed_outputs_unchanged() {
    for v2 in [false, true] {
        let s = Scratch::new();
        let mut volume = 0;
        let a = PwCreateAttributes {
            uid: 100,
            gid: 20,
            mode: 0o750,
            reserved: 0,
        };
        let rc = if v2 {
            unsafe {
                pw_volume_create_with_metadata(
                    s.path().as_ptr(),
                    s.path().len(),
                    1 << 20,
                    &a,
                    &mut volume,
                )
            }
        } else {
            unsafe { pw_volume_create(s.path().as_ptr(), s.path().len(), 1 << 20, &mut volume) }
        };
        assert_eq!(rc, 0);
        let mut out = PwFsKitCreated {
            object: 77,
            file: 88,
            ..Default::default()
        };
        let sentinel = out;
        assert_eq!(
            unsafe {
                pw_fskit_node_create(
                    volume,
                    1,
                    b"null".as_ptr(),
                    4,
                    0,
                    std::ptr::null(),
                    std::ptr::null_mut(),
                )
            },
            -1
        );
        let mut found = 99;
        assert_eq!(
            unsafe { pw_lookup(volume, 1, b"null".as_ptr(), 4, &mut found) },
            -2
        );
        assert_eq!(found, 99);
        assert_eq!(
            unsafe {
                pw_fskit_node_create(volume, 1, b"bad".as_ptr(), 3, 2, std::ptr::null(), &mut out)
            },
            -1
        );
        assert_eq!(out, sentinel);
        assert_eq!(
            unsafe {
                pw_fskit_node_create(
                    volume,
                    1,
                    b"../escape".as_ptr(),
                    9,
                    0,
                    std::ptr::null(),
                    &mut out,
                )
            },
            -1
        );
        assert_eq!(out, sentinel);
        assert_eq!(
            unsafe {
                pw_fskit_node_create(
                    volume,
                    1,
                    b"file".as_ptr(),
                    4,
                    0,
                    std::ptr::null(),
                    &mut out,
                )
            },
            0
        );
        assert!(out.file > 0);
        assert_eq!(out.attributes.file_id, out.object as u64 + 1);
        assert_eq!(
            (
                out.attributes.kind,
                out.attributes.links,
                out.attributes.metadata_valid
            ),
            (0, 1, u32::from(v2))
        );
        let file = out;
        let mut parent_before = PwMetadata::default();
        if v2 {
            assert_eq!(unsafe { pw_metadata_get(volume, 1, &mut parent_before) }, 0);
            assert_eq!(out.attributes.metadata.mode, 0o600);
        }
        assert_eq!(
            unsafe {
                pw_fskit_node_create(
                    volume,
                    1,
                    b"FILE".as_ptr(),
                    4,
                    0,
                    std::ptr::null(),
                    &mut out,
                )
            },
            -3
        );
        assert_eq!(out, file);
        if v2 {
            let mut after = PwMetadata::default();
            assert_eq!(unsafe { pw_metadata_get(volume, 1, &mut after) }, 0);
            assert_eq!(after, parent_before);
        }
        assert_eq!(pw_volume_close(volume), -7);
        let mut count = 99;
        assert_eq!(
            unsafe { pw_file_write(volume, file.file, 0, b"pin".as_ptr(), 3, &mut count) },
            0
        );
        assert_eq!(unsafe { pw_remove(volume, 1, b"file".as_ptr(), 4, 0) }, 0);
        let mut bytes = [0; 8];
        assert_eq!(
            unsafe { pw_posix_file_read(volume, file.file, 0, bytes.as_mut_ptr(), 8, &mut count) },
            -1
        );
        assert_eq!(
            unsafe { pw_file_read(volume, file.file, 0, bytes.as_mut_ptr(), 8, &mut count) },
            0
        );
        assert_eq!(&bytes[..count], b"pin");
        assert_eq!(pw_file_close(volume, file.file), 0);
        assert_eq!(
            unsafe {
                pw_fskit_node_create(volume, 1, b"dir".as_ptr(), 3, 1, std::ptr::null(), &mut out)
            },
            0
        );
        assert_eq!((out.file, out.attributes.kind), (0, 1));
        assert_eq!(pw_volume_close(volume), 0);
    }
}

#[test]
fn fskit_create_file_admission_includes_policy_and_admin_handles_before_namespace_change() {
    let s = Scratch::new();
    let (volume, writer, reader, object) = request_volume_format(&s, true);
    let owner = PwCredentials {
        uid: 100,
        gid: 20,
        groups: std::ptr::null(),
        group_count: 0,
        reserved: 0,
    };
    let mut directory_pin = 0;
    assert_eq!(
        unsafe { pw_posix_directory_open(volume, 1, &owner, &mut directory_pin) },
        0
    );
    let mut handles = Vec::new();
    for _ in 0..4093 {
        let mut h = 0;
        assert_eq!(unsafe { pw_file_open(volume, object, &mut h) }, 0);
        handles.push(h);
    }
    let mut before = PwMetadata::default();
    assert_eq!(unsafe { pw_metadata_get(volume, 1, &mut before) }, 0);
    let mut out = PwFsKitCreated {
        object: 77,
        file: 88,
        ..Default::default()
    };
    let sentinel = out;
    assert_eq!(
        unsafe {
            pw_fskit_node_create(
                volume,
                1,
                b"absent".as_ptr(),
                6,
                0,
                std::ptr::null(),
                &mut out,
            )
        },
        -14
    );
    assert_eq!(out, sentinel);
    let mut denied = 99;
    assert_eq!(
        unsafe { pw_posix_directory_open(volume, 1, &owner, &mut denied) },
        -14
    );
    assert_eq!(denied, 99);
    let mut found = 99;
    assert_eq!(
        unsafe { pw_lookup(volume, 1, b"absent".as_ptr(), 6, &mut found) },
        -2
    );
    let mut after = PwMetadata::default();
    assert_eq!(unsafe { pw_metadata_get(volume, 1, &mut after) }, 0);
    assert_eq!(before, after);
    assert_eq!(
        unsafe {
            pw_fskit_node_create(
                volume,
                1,
                b"directory".as_ptr(),
                9,
                1,
                std::ptr::null(),
                &mut out,
            )
        },
        0
    );
    assert_eq!(out.file, 0);
    let old_directory = directory_pin;
    assert_eq!(pw_posix_directory_close(volume, directory_pin), 0);
    assert_eq!(
        unsafe { pw_posix_directory_open(volume, 1, &owner, &mut directory_pin) },
        0
    );
    assert_ne!(old_directory, directory_pin);
    for h in handles {
        assert_eq!(pw_file_close(volume, h), 0);
    }
    assert_eq!(
        unsafe {
            pw_fskit_node_create(
                volume,
                1,
                b"resumed".as_ptr(),
                7,
                0,
                std::ptr::null(),
                &mut out,
            )
        },
        0
    );
    assert_eq!(pw_file_close(volume, out.file), 0);
    assert_eq!(pw_posix_file_close(volume, writer), 0);
    assert_eq!(pw_posix_file_close(volume, reader), 0);
    assert_eq!(pw_posix_directory_close(volume, directory_pin), 0);
    assert_eq!(pw_volume_close(volume), 0);
}

#[test]
fn initial_creation_state_abi_validates_before_mutation_and_preserves_v1_v2_semantics() {
    for v2 in [false, true] {
        let s = Scratch::new();
        let mut volume = 0;
        let root = PwCreateAttributes {
            uid: 100,
            gid: 20,
            mode: 0o750,
            reserved: 0,
        };
        let rc = if v2 {
            unsafe {
                pw_volume_create_with_metadata(
                    s.path().as_ptr(),
                    s.path().len(),
                    1 << 20,
                    &root,
                    &mut volume,
                )
            }
        } else {
            unsafe { pw_volume_create(s.path().as_ptr(), s.path().len(), 1 << 20, &mut volume) }
        };
        assert_eq!(rc, 0);
        let mut out = PwFsKitCreated {
            object: 77,
            file: 88,
            ..Default::default()
        };
        let sentinel = out;
        let t = PwTimestamp {
            seconds: -123,
            nanoseconds: 999_999_999,
            reserved: 0,
        };
        let mut state = PwCreateState {
            valid: if v2 { 15 } else { 1 },
            size: 7,
            created: t,
            accessed: t,
            modified: t,
        };
        assert_eq!(
            unsafe {
                pw_fskit_node_create_with_state(
                    volume,
                    1,
                    b"absent".as_ptr(),
                    6,
                    0,
                    std::ptr::null(),
                    std::ptr::null(),
                    &mut out,
                )
            },
            -1
        );
        assert_eq!(out, sentinel);
        let mut bad = state;
        bad.valid = 16;
        assert_eq!(
            unsafe {
                pw_fskit_node_create_with_state(
                    volume,
                    1,
                    b"absent".as_ptr(),
                    6,
                    0,
                    std::ptr::null(),
                    &bad,
                    &mut out,
                )
            },
            -1
        );
        assert_eq!(out, sentinel);
        bad = state;
        bad.created.reserved = 1;
        assert_eq!(
            unsafe {
                pw_fskit_node_create_with_state(
                    volume,
                    1,
                    b"absent".as_ptr(),
                    6,
                    0,
                    std::ptr::null(),
                    &bad,
                    &mut out,
                )
            },
            -1
        );
        assert_eq!(out, sentinel);
        bad = PwCreateState {
            valid: 1,
            size: 1 << 21,
            ..Default::default()
        };
        assert_eq!(
            unsafe {
                pw_fskit_node_create_with_state(
                    volume,
                    1,
                    b"absent".as_ptr(),
                    6,
                    0,
                    std::ptr::null(),
                    &bad,
                    &mut out,
                )
            },
            -9
        );
        assert_eq!(out, sentinel);
        bad = PwCreateState {
            valid: 1,
            ..Default::default()
        };
        assert_eq!(
            unsafe {
                pw_fskit_node_create_with_state(
                    volume,
                    1,
                    b"absent".as_ptr(),
                    6,
                    1,
                    std::ptr::null(),
                    &bad,
                    &mut out,
                )
            },
            -11
        );
        assert_eq!(out, sentinel);
        let mut found = 99;
        assert_eq!(
            unsafe { pw_lookup(volume, 1, b"absent".as_ptr(), 6, &mut found) },
            -2
        );
        assert_eq!(found, 99);
        if !v2 {
            bad = PwCreateState {
                valid: 2,
                created: t,
                ..Default::default()
            };
            assert_eq!(
                unsafe {
                    pw_fskit_node_create_with_state(
                        volume,
                        1,
                        b"absent".as_ptr(),
                        6,
                        0,
                        std::ptr::null(),
                        &bad,
                        &mut out,
                    )
                },
                -11
            );
            assert_eq!(out, sentinel);
        } else {
            bad = state;
            bad.created.nanoseconds = 1_000_000_000;
            assert_eq!(
                unsafe {
                    pw_fskit_node_create_with_state(
                        volume,
                        1,
                        b"absent".as_ptr(),
                        6,
                        0,
                        std::ptr::null(),
                        &bad,
                        &mut out,
                    )
                },
                -1
            );
            assert_eq!(out, sentinel);
        }
        assert_eq!(
            unsafe {
                pw_fskit_node_create_with_state(
                    volume,
                    1,
                    b"file".as_ptr(),
                    4,
                    0,
                    std::ptr::null(),
                    &state,
                    &mut out,
                )
            },
            0
        );
        assert_eq!(
            (out.attributes.size, out.attributes.metadata_valid),
            (7, u32::from(v2))
        );
        if v2 {
            assert_eq!(
                (
                    out.attributes.metadata.created,
                    out.attributes.metadata.accessed,
                    out.attributes.metadata.modified
                ),
                (t, t, t)
            );
        }
        let mut stat = PwStat::default();
        assert_eq!(unsafe { pw_stat(volume, out.object, &mut stat) }, 0);
        assert_eq!(stat.generation, 0);
        let mut data = [9; 8];
        let mut count = 99;
        assert_eq!(
            unsafe { pw_file_read(volume, out.file, 0, data.as_mut_ptr(), 8, &mut count) },
            0
        );
        assert_eq!(count, 7);
        assert_eq!(&data[..7], &[0; 7]);
        assert_eq!(pw_file_close(volume, out.file), 0);
        if v2 {
            state.valid = 2;
            assert_eq!(
                unsafe {
                    pw_fskit_node_create_with_state(
                        volume,
                        1,
                        b"dir".as_ptr(),
                        3,
                        1,
                        std::ptr::null(),
                        &state,
                        &mut out,
                    )
                },
                0
            );
            assert_eq!(out.file, 0);
            assert_eq!(out.attributes.metadata.created, t);
        }
        assert_eq!(pw_volume_close(volume), 0);
    }
}
