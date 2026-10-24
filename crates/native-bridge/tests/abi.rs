use portaweave_native::*;
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

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
