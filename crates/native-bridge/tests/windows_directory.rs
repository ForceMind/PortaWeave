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
}
fn caller(uid: u32) -> PwCredentials {
    PwCredentials {
        uid,
        gid: 20,
        groups: ptr::null(),
        group_count: 0,
        reserved: 0,
    }
}
impl Fixture {
    fn new(v2: bool) -> Self {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "pw-windows-directory-{}-{stamp}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let p = path.to_str().unwrap();
        let mut volume = 0;
        let attrs = PwCreateAttributes {
            uid: 100,
            gid: 20,
            mode: 0o777,
            reserved: 0,
        };
        assert_eq!(
            unsafe {
                if v2 {
                    pw_volume_create_with_metadata(
                        p.as_ptr(),
                        p.len(),
                        1 << 20,
                        &attrs,
                        &mut volume,
                    )
                } else {
                    pw_volume_create(p.as_ptr(), p.len(), 1 << 20, &mut volume)
                }
            },
            0
        );
        Self { path, volume }
    }
    fn create(&self, parent: i64, name: &str, directory: bool) -> i64 {
        let mut object = 0;
        assert_eq!(
            unsafe {
                if directory {
                    pw_directory_create(self.volume, parent, name.as_ptr(), name.len(), &mut object)
                } else {
                    pw_file_create(self.volume, parent, name.as_ptr(), name.len(), &mut object)
                }
            },
            0
        );
        object
    }
    fn begin(&self, directory: i64) -> u64 {
        let mut token = 0;
        assert_eq!(
            unsafe { pw_windows_directory_begin(self.volume, directory, &caller(100), &mut token) },
            0
        );
        token
    }
    fn read(&self, token: u64, offset: u64, capacity: usize) -> Vec<u8> {
        let mut bytes = vec![77; capacity + 2];
        let mut count = 99;
        assert_eq!(
            unsafe {
                pw_windows_directory_read(
                    self.volume,
                    token,
                    offset,
                    bytes.as_mut_ptr().add(1),
                    capacity,
                    &mut count,
                )
            },
            0
        );
        assert_eq!(bytes[0], 77);
        assert!(bytes[count + 1..].iter().all(|b| *b == 77));
        bytes[1..1 + count].to_vec()
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
        if !std::thread::panicking() {
            assert_eq!(pw_volume_close(self.volume), 0);
            fs::remove_dir_all(&self.path).unwrap();
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct Record {
    name: String,
    id: u64,
    size: u64,
    allocation: u64,
    attrs: u32,
    next: u64,
    created: u64,
    accessed: u64,
    modified: u64,
}
fn u64at(b: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(b[offset..offset + 8].try_into().unwrap())
}
fn parse(bytes: &[u8]) -> (Vec<Record>, bool) {
    let mut at = 0;
    let mut records = Vec::new();
    let mut eof = false;
    while at < bytes.len() {
        assert!(bytes.len() - at >= 2);
        let size = u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap()) as usize;
        if size == 0 {
            assert_eq!(at + 2, bytes.len());
            eof = true;
            break;
        }
        assert!(size >= 104 && (size - 104).is_multiple_of(2));
        let padded = size.div_ceil(8) * 8;
        assert!(at + padded <= bytes.len());
        let b = &bytes[at..at + padded];
        assert!(b[2..8].iter().all(|x| *x == 0));
        assert!(b[12..16].iter().all(|x| *x == 0));
        assert!(b[72..80].iter().all(|x| *x == 0));
        assert!(b[88..104].iter().all(|x| *x == 0));
        assert!(b[size..].iter().all(|x| *x == 0));
        let units: Vec<_> = b[104..size]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        assert!(!units.contains(&0));
        records.push(Record {
            name: String::from_utf16(&units).unwrap(),
            id: u64at(b, 64),
            size: u64at(b, 24),
            allocation: u64at(b, 16),
            attrs: u32::from_le_bytes(b[8..12].try_into().unwrap()),
            next: u64at(b, 80),
            created: u64at(b, 32),
            accessed: u64at(b, 40),
            modified: u64at(b, 48),
        });
        at += padded;
    }
    (records, eof)
}

#[test]
fn utf16_wire_layout_padding_identity_times_and_every_pagination_boundary() {
    let f = Fixture::new(true);
    let dir = f.create(1, "directory", true);
    let file = f.create(dir, "file", false);
    let mut handle = 0;
    let mut count = 0;
    assert_eq!(unsafe { pw_file_open(f.volume, file, &mut handle) }, 0);
    assert_eq!(
        unsafe { pw_file_write(f.volume, handle, 0, b"abc".as_ptr(), 3, &mut count) },
        0
    );
    assert_eq!(pw_file_close(f.volume, handle), 0);
    let update = PwMetadataUpdate {
        valid: 8 | 16 | 32,
        created: PwTimestamp {
            seconds: -1,
            nanoseconds: 999_999_999,
            reserved: 0,
        },
        accessed: PwTimestamp {
            seconds: 0,
            nanoseconds: 7,
            reserved: 0,
        },
        modified: PwTimestamp {
            seconds: 0,
            nanoseconds: 123,
            reserved: 0,
        },
        ..Default::default()
    };
    assert_eq!(unsafe { pw_metadata_set(f.volume, file, &update) }, 0);
    assert_eq!(
        unsafe { pw_hard_link(f.volume, file, dir, b"z-alias".as_ptr(), 7) },
        0
    );
    let emoji = f.create(dir, "😀.txt", false);
    let branch = f.create(dir, "branch", true);
    let token = f.begin(dir);
    let all = f.read(token, 0, 4096);
    let (records, eof) = parse(&all);
    assert!(eof);
    assert_eq!(records.len(), 6);
    assert_eq!(
        (&records[0].name, records[0].id, records[0].attrs),
        (&".".to_owned(), dir as u64, 0x10)
    );
    assert_eq!((&records[1].name, records[1].id), (&"..".to_owned(), 1));
    for (n, r) in records.iter().enumerate() {
        assert_eq!(r.next, n as u64 + 1);
    }
    for name in ["file", "z-alias"] {
        let r = records.iter().find(|r| r.name == name).unwrap();
        assert_eq!(
            (r.id, r.size, r.allocation, r.attrs),
            (file as u64, 3, 4096, 0x80)
        );
        assert_eq!(
            (r.created, r.accessed, r.modified),
            (EPOCH - 1, EPOCH, EPOCH + 1)
        );
    }
    assert_eq!(
        records.iter().find(|r| r.name == "😀.txt").unwrap().id,
        emoji as u64
    );
    assert_eq!(
        records.iter().find(|r| r.name == "branch").unwrap().id,
        branch as u64
    );
    assert!(f.read(token, 0, 111).is_empty()); // complete first record needs 112, never partial
    for capacity in [112, 120, 128, 160, 320, 1024] {
        let mut actual = Vec::new();
        let mut offset = 0;
        for _ in 0..20 {
            let mut page = f.read(token, offset, capacity);
            let (mut batch, end) = parse(&page);
            if batch.is_empty() && !end {
                page = f.read(token, offset, 512);
                (batch, _) = parse(&page);
                assert!(!batch.is_empty());
            }
            actual.extend(batch.iter().cloned());
            if let Some(last) = batch.last() {
                offset = last.next;
            }
            if parse(&page).1 {
                break;
            }
        }
        assert_eq!(actual, records);
        assert_eq!(f.read(token, offset, 2), [0, 0]);
    }
    assert_eq!(f.read(token, 0, 4096), all);
    assert!(f.read(token, 6, 1).is_empty());
    let mut metadata = PwMetadata::default();
    assert_eq!(unsafe { pw_metadata_get(f.volume, file, &mut metadata) }, 0);
    assert_eq!(metadata.modified.nanoseconds, 123); // projection never rewrites core values
    assert_eq!(pw_windows_directory_close(f.volume, token), 0);
    f.healthy();
}

#[test]
fn immutable_metadata_survives_namespace_deletion_without_becoming_a_data_or_path_grant() {
    let f = Fixture::new(true);
    let dir = f.create(1, "directory", true);
    let file = f.create(dir, "old", false);
    let token = f.begin(dir);
    let saved = f.read(token, 0, 1024);
    assert_eq!(
        unsafe { pw_remove(f.volume, dir, b"old".as_ptr(), 3, 0) },
        0
    );
    assert_eq!(
        unsafe { pw_remove(f.volume, 1, b"directory".as_ptr(), 9, 1) },
        0
    );
    let mut stat = PwStat::default();
    assert_eq!(unsafe { pw_stat(f.volume, file, &mut stat) }, -2); // no inode pin
    assert_eq!(f.read(token, 0, 1024), saved);
    let mut next = 99;
    assert_eq!(
        unsafe { pw_windows_directory_begin(f.volume, dir, &caller(100), &mut next) },
        -2
    );
    assert_eq!(next, 99);
    let root = f.begin(1);
    assert_eq!(f.read(root, 0, 1024), [0, 0]); // no root dots
    assert_eq!(pw_volume_close(f.volume), -7);
    assert_eq!(pw_windows_directory_close(f.volume, token), 0);
    assert_eq!(pw_windows_directory_close(f.volume, root), 0);
    assert_eq!(pw_windows_directory_close(f.volume, token), -1);
}

#[test]
fn begin_requires_trusted_claims_all_ancestors_read_search_and_explicit_v2() {
    let f = Fixture::new(true);
    let outer = f.create(1, "private", true);
    let mut inner = 0;
    let attrs = PwCreateAttributes {
        uid: 200,
        gid: 20,
        mode: 0o700,
        reserved: 0,
    };
    assert_eq!(
        unsafe {
            pw_node_create_with_metadata(
                f.volume,
                outer,
                b"inner".as_ptr(),
                5,
                1,
                &attrs,
                &mut inner,
            )
        },
        0
    );
    let mut token = 99;
    assert_eq!(
        unsafe { pw_windows_directory_begin(f.volume, inner, &caller(200), &mut token) },
        -17
    );
    assert_eq!(token, 99);
    assert_eq!(
        unsafe {
            pw_metadata_set(
                f.volume,
                outer,
                &PwMetadataUpdate {
                    valid: 4,
                    mode: 0o711,
                    ..Default::default()
                },
            )
        },
        0
    );
    assert_eq!(
        unsafe { pw_windows_directory_begin(f.volume, inner, &caller(200), &mut token) },
        0
    );
    assert_eq!(pw_windows_directory_close(f.volume, token), 0);
    for mode in [0o400, 0o100, 0] {
        assert_eq!(
            unsafe {
                pw_metadata_set(
                    f.volume,
                    inner,
                    &PwMetadataUpdate {
                        valid: 4,
                        mode,
                        ..Default::default()
                    },
                )
            },
            0
        );
        token = 99;
        assert_eq!(
            unsafe { pw_windows_directory_begin(f.volume, inner, &caller(200), &mut token) },
            -17
        );
        assert_eq!(token, 99);
    }
    token = 99;
    assert_eq!(
        unsafe { pw_windows_directory_begin(f.volume, 1, ptr::null(), &mut token) },
        -1
    );
    assert_eq!(token, 99);
    assert_eq!(
        unsafe { pw_windows_directory_begin(f.volume, 1, &caller(100), ptr::null_mut()) },
        -1
    );
    f.healthy();
    let legacy = Fixture::new(false);
    assert_eq!(
        unsafe { pw_windows_directory_begin(legacy.volume, 1, &caller(0), &mut token) },
        -11
    );
    assert_eq!(token, 99);
}

#[test]
fn malformed_reads_foreign_admin_and_closed_tokens_preserve_all_error_outputs() {
    let f = Fixture::new(true);
    let other = Fixture::new(true);
    let token = f.begin(1);
    let foreign = other.begin(1);
    let mut admin = 0;
    assert_eq!(unsafe { pw_dir_begin(f.volume, 1, &mut admin) }, 0);
    let mut buffer = [77; 128];
    let mut count = 99;
    for (volume, cursor, offset) in [
        (f.volume, 0, 0),
        (f.volume, foreign, 0),
        (other.volume, token, 0),
        (f.volume, admin, 0),
        (f.volume, token, 1),
        (f.volume, token, u64::MAX),
    ] {
        assert_eq!(
            unsafe {
                pw_windows_directory_read(
                    volume,
                    cursor,
                    offset,
                    buffer.as_mut_ptr(),
                    128,
                    &mut count,
                )
            },
            -1
        );
        assert_eq!(buffer, [77; 128]);
        assert_eq!(count, 99);
    }
    assert_eq!(
        unsafe { pw_windows_directory_read(f.volume, token, 0, ptr::null_mut(), 1, &mut count) },
        -1
    );
    assert_eq!(
        unsafe {
            pw_windows_directory_read(
                f.volume,
                token,
                0,
                buffer.as_mut_ptr(),
                128,
                ptr::null_mut(),
            )
        },
        -1
    );
    let mut oversized = vec![77; 16 * 1024 * 1024 + 1];
    assert_eq!(
        unsafe {
            pw_windows_directory_read(
                f.volume,
                token,
                0,
                oversized.as_mut_ptr(),
                oversized.len(),
                &mut count,
            )
        },
        -1
    );
    assert!(oversized.iter().all(|b| *b == 77));
    assert_eq!(count, 99);
    assert_eq!(
        unsafe { pw_windows_directory_read(f.volume, token, 0, ptr::null_mut(), 0, &mut count) },
        0
    );
    assert_eq!(count, 0);
    assert_eq!(pw_dir_seek(f.volume, token, 0), -1);
    assert_eq!(pw_windows_directory_close(f.volume, admin), -1);
    assert_eq!(pw_windows_directory_close(f.volume, token), 0);
    count = 99;
    assert_eq!(
        unsafe {
            pw_windows_directory_read(f.volume, token, 0, buffer.as_mut_ptr(), 128, &mut count)
        },
        -1
    );
    assert_eq!(buffer, [77; 128]);
    assert_eq!(count, 99);
    assert_eq!(pw_windows_directory_close(other.volume, foreign), 0);
    assert_eq!(pw_dir_close(f.volume, admin), 0);
}

#[test]
fn administrative_and_windows_snapshots_share_the_real_64_cursor_limit_and_release_admission() {
    let f = Fixture::new(true);
    let mut cursors = Vec::new();
    for index in 0..64 {
        let mut token = 0;
        let windows = index % 2 == 0;
        assert_eq!(
            unsafe {
                if windows {
                    pw_windows_directory_begin(f.volume, 1, &caller(100), &mut token)
                } else {
                    pw_dir_begin(f.volume, 1, &mut token)
                }
            },
            0
        );
        cursors.push((token, windows));
    }
    let mut refused = 99;
    assert_eq!(
        unsafe { pw_windows_directory_begin(f.volume, 1, &caller(100), &mut refused) },
        -14
    );
    assert_eq!(refused, 99);
    assert_eq!(unsafe { pw_dir_begin(f.volume, 1, &mut refused) }, -14);
    assert_eq!(refused, 99);
    assert_eq!(pw_volume_close(f.volume), -7);
    let (released, _) = cursors.pop().unwrap();
    assert_eq!(pw_dir_close(f.volume, released), 0);
    let replacement = f.begin(1);
    assert_ne!(released, replacement);
    for (token, windows) in cursors {
        assert_eq!(
            if windows {
                pw_windows_directory_close(f.volume, token)
            } else {
                pw_dir_close(f.volume, token)
            },
            0
        );
    }
    assert_eq!(pw_windows_directory_close(f.volume, replacement), 0);
    f.healthy();
}

#[test]
fn failed_projection_publishes_no_cursor_or_modified_output_and_can_be_repaired() {
    let f = Fixture::new(true);
    f.create(1, "a", false);
    let b = f.create(1, "b", false);
    let mut update = PwMetadataUpdate {
        valid: 8,
        created: PwTimestamp {
            seconds: i64::MIN,
            nanoseconds: 0,
            reserved: 0,
        },
        ..Default::default()
    };
    assert_eq!(unsafe { pw_metadata_set(f.volume, b, &update) }, 0);
    let mut token = 99;
    assert_eq!(
        unsafe { pw_windows_directory_begin(f.volume, 1, &caller(100), &mut token) },
        -11
    );
    assert_eq!(token, 99);
    f.healthy();
    update.created.seconds = 0;
    assert_eq!(unsafe { pw_metadata_set(f.volume, b, &update) }, 0);
    token = f.begin(1);
    assert_eq!(parse(&f.read(token, 0, 1024)).0.len(), 2);
    assert_eq!(pw_windows_directory_close(f.volume, token), 0);
}

#[test]
fn maximum_legal_name_is_never_partially_emitted_and_eof_needs_its_own_space() {
    let f = Fixture::new(true);
    let name = "a".repeat(255);
    let file = f.create(1, &name, false);
    let token = f.begin(1);
    assert!(f.read(token, 0, 615).is_empty());
    let page = f.read(token, 0, 616);
    let (records, eof) = parse(&page);
    assert!(!eof);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].name, name);
    assert_eq!(records[0].id, file as u64);
    assert_eq!(records[0].next, 1);
    assert_eq!(f.read(token, 1, 2), [0, 0]);
    let combined = f.read(token, 0, 618);
    assert!(parse(&combined).1);
    assert_eq!(parse(&combined).0, records);
    assert_eq!(pw_windows_directory_close(f.volume, token), 0);
}

#[test]
fn concurrent_read_and_close_returns_whole_snapshots_or_unchanged_invalid_outputs() {
    use std::sync::{Arc, Barrier};
    let f = Fixture::new(true);
    f.create(1, "entry", false);
    let token = f.begin(1);
    let expected = f.read(token, 0, 512);
    let barrier = Arc::new(Barrier::new(5));
    let mut workers = Vec::new();
    for _ in 0..4 {
        let start = barrier.clone();
        let bytes = expected.clone();
        let volume = f.volume;
        workers.push(std::thread::spawn(move || {
            let mut buffer = [77; 512];
            let mut count = 99;
            assert_eq!(
                unsafe {
                    pw_windows_directory_read(
                        volume,
                        token,
                        0,
                        buffer.as_mut_ptr(),
                        512,
                        &mut count,
                    )
                },
                0
            );
            assert_eq!(&buffer[..count], bytes.as_slice());
            start.wait();
            for _ in 0..100 {
                buffer.fill(77);
                count = 99;
                match unsafe {
                    pw_windows_directory_read(
                        volume,
                        token,
                        0,
                        buffer.as_mut_ptr(),
                        512,
                        &mut count,
                    )
                } {
                    0 => {
                        assert_eq!(count, bytes.len());
                        assert_eq!(&buffer[..count], bytes.as_slice());
                        assert!(buffer[count..].iter().all(|b| *b == 77));
                    }
                    -1 => {
                        assert_eq!(buffer, [77; 512]);
                        assert_eq!(count, 99);
                    }
                    status => panic!("unexpected read status {status}"),
                }
            }
        }));
    }
    barrier.wait();
    assert_eq!(pw_windows_directory_close(f.volume, token), 0);
    for worker in workers {
        worker.join().unwrap();
    }
    f.healthy();
}
