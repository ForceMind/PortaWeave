use portaweave_core::{ROOT, Volume};
use portaweave_native::*;
use std::{
    fs,
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

static NEXT: AtomicU64 = AtomicU64::new(0);

#[test]
fn actual_idle_worker_continues_cleanup_and_joins_before_owner_release() {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "pw-worker-{}-{stamp}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let path = root.to_str().unwrap();
    let mut core = Volume::create(&root, 1 << 20).unwrap();
    let mut last = ROOT;
    for index in 0..129 {
        let name = format!("abandoned-{index}");
        last = core.create_file(ROOT, &name).unwrap();
        core.open_file(last).unwrap();
        core.unlink(ROOT, &name).unwrap();
    }
    // Deliberate ended-owner fixture, NOT safe unmount or physical power loss.
    drop(core);
    unsafe {
        let mut volume = 0;
        assert_eq!(pw_volume_open(path.as_ptr(), path.len(), &mut volume), 0);
        let mut stat = PwStat::default();
        assert_eq!(pw_stat(volume, last.raw(), &mut stat), 0); // one orphan remains
        let mut status = PwMaintenanceWorkerStatus::default();
        let mut sentinel = PwMaintenanceWorkerStatus {
            running: 99,
            last_status: 99,
            ticks: 99,
            report: PwMaintenanceReport::default(),
        };
        let before = sentinel;
        assert_eq!(
            pw_volume_maintenance_worker_status(u64::MAX, &mut sentinel),
            -1
        );
        assert_eq!(sentinel, before);
        assert_eq!(
            pw_volume_maintenance_worker_status(volume, std::ptr::null_mut()),
            -1
        );
        assert_eq!(pw_volume_maintenance_worker_start(volume, 0, 10), -1);
        assert_eq!(pw_volume_maintenance_worker_start(volume, 50, 0), -1);
        assert_eq!(pw_volume_maintenance_worker_start(volume, 50, 10), 0);
        assert_eq!(pw_volume_maintenance_worker_start(volume, 50, 10), -7);
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            // Repeated monitoring must not postpone the actual quiet timer.
            let mut health = 99;
            let mut protected = 99;
            let mut space = PwSpace::default();
            assert_eq!(pw_health(volume, &mut health, &mut protected), 0);
            assert_eq!((health, protected), (0, 0));
            assert_eq!(pw_volume_space(volume, &mut space), 0);
            assert_eq!(pw_volume_maintenance_worker_status(volume, &mut status), 0);
            if status.ticks > 0 {
                break;
            }
            assert!(Instant::now() < deadline, "idle worker made no progress");
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(status.last_status, 0);
        assert_eq!(status.running, 1);
        assert_eq!(pw_stat(volume, last.raw(), &mut stat), -2);
        let mut object = 0;
        let mut file = 0;
        assert_eq!(
            pw_file_create(volume, 1, b"held".as_ptr(), 4, &mut object),
            0
        );
        assert_eq!(pw_file_open(volume, object, &mut file), 0);
        assert_eq!(pw_volume_close(volume), -7);
        assert_eq!(pw_volume_maintenance_worker_status(volume, &mut status), 0);
        assert_eq!(status.running, 1); // BUSY close does not retire the worker
        assert_eq!(pw_volume_maintenance_worker_stop(volume), 0);
        assert_eq!(pw_volume_maintenance_worker_stop(volume), 0);
        assert_eq!(pw_volume_maintenance_worker_status(volume, &mut status), 0);
        assert_eq!(status.running, 0);
        assert_eq!(pw_volume_maintenance_worker_start(volume, 50, 10), 0);
        assert_eq!(pw_file_close(volume, file), 0);
        assert_eq!(pw_volume_close(volume), 0); // stops/joins while registry is locked
        sentinel = before;
        assert_eq!(
            pw_volume_maintenance_worker_status(volume, &mut sentinel),
            -1
        );
        assert_eq!(sentinel, before);
        assert_eq!(pw_volume_maintenance_worker_stop(volume), -1);
    }
    // Ownership is actually released after join, not merely hidden in a table.
    Volume::open(&root).unwrap().verify().unwrap();
    fs::remove_dir_all(root).unwrap();
}
