use portaweave_native::*;
use std::{fs, mem, sync::atomic::{AtomicU64, Ordering}, time::{SystemTime, UNIX_EPOCH}};

static NEXT: AtomicU64 = AtomicU64::new(0);

#[test]
fn administrative_step_preserves_pins_failed_outputs_and_owner_lifetime() {
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let root = std::env::temp_dir().join(format!("pw-native-maintenance-{}-{stamp}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    fs::create_dir(&root).unwrap();
    for version in 1..=3 {
        let path = root.join(format!("v{version}"));
        let path = path.to_str().unwrap();
        let attrs = PwCreateAttributes { uid: 501, gid: 20, mode: 0o700, reserved: 0 };
        let mut volume = 0;
        let mut object = 0;
        let mut file = 0;
        let mut view = 0;
        let mut count = 99;
        unsafe {
            let status = match version {
                1 => pw_volume_create(path.as_ptr(), path.len(), 1 << 20, &mut volume),
                2 => pw_volume_create_with_metadata(path.as_ptr(), path.len(), 1 << 20, &attrs, &mut volume),
                _ => pw_volume_create_with_directory_lifetimes(path.as_ptr(), path.len(), 1 << 20, &attrs, &mut volume),
            };
            assert_eq!(status, 0);
            let mut report = PwMaintenanceReport { orphan_candidates: 99, objects_removed: 99, dead_segment_candidates: 99, segments_removed: 99, compaction_candidates: 99, blocks_copied: 99 };
            let sentinel = report;
            assert_eq!(pw_volume_maintenance_step(u64::MAX, &mut report), -1);
            assert_eq!(report, sentinel);
            assert_eq!(pw_volume_maintenance_step(volume, std::ptr::null_mut()), -1);
            assert_eq!(mem::size_of::<PwMaintenanceReport>(), 24);
            assert_eq!(pw_file_create(volume, 1, b"data".as_ptr(), 4, &mut object), 0);
            assert_eq!(pw_file_open(volume, object, &mut file), 0);
            assert_eq!(pw_file_write(volume, file, 0, b"old".as_ptr(), 3, &mut count), 0);
            assert_eq!(pw_read_view_begin(volume, file, 0, 3, &mut view), 0);
            assert_eq!(pw_file_write(volume, file, 0, b"new".as_ptr(), 3, &mut count), 0);
            assert_eq!(pw_volume_maintenance_step(volume, &mut report), 0);
            assert_eq!(report.segments_removed, 0);
            assert_eq!(pw_file_close(volume, file), 0);
            assert_eq!(pw_volume_close(volume), -7);
            assert_eq!(pw_volume_maintenance_step(volume, &mut report), 0);
            let mut bytes = [0; 3];
            assert_eq!(pw_read_view_read(volume, view, 0, bytes.as_mut_ptr(), bytes.len(), &mut count), 0);
            assert_eq!(bytes, *b"old");
            assert_eq!(pw_read_view_close(volume, view), 0);
            assert_eq!(pw_volume_maintenance_step(volume, &mut report), 0);
            assert!(report.orphan_candidates <= 128 && report.dead_segment_candidates <= 256 && report.compaction_candidates <= 128 && report.blocks_copied <= 256);
            let mut health = 99;
            let mut protected = 99;
            assert_eq!(pw_health(volume, &mut health, &mut protected), 0);
            assert_eq!((health, protected), (0, 0));
            assert_eq!(pw_volume_close(volume), 0);
            report = sentinel;
            assert_eq!(pw_volume_maintenance_step(volume, &mut report), -1);
            assert_eq!(report, sentinel);
        }
    }
    fs::remove_dir_all(root).unwrap();
}
