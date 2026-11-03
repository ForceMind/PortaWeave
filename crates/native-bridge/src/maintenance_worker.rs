//! Opt-in native owner worker. Never clones/opens Volume or its catalog.
use super::*;
use std::{
    sync::{TryLockError, mpsc},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PwMaintenanceWorkerStatus {
    pub running: u32,
    pub last_status: i32,
    pub ticks: u64,
    /// Last SUCCESSFUL report; an error can have committed earlier cleanup.
    pub report: PwMaintenanceReport,
}

struct Worker {
    stop: mpsc::Sender<()>,
    join: JoinHandle<()>,
}

pub(super) struct State {
    worker: Option<Worker>,
    activity: Instant,
    next_due: Option<Instant>,
    ticks: u64,
    last_status: i32,
    report: PwMaintenanceReport,
}
impl State {
    pub(super) fn new() -> Self {
        Self {
            worker: None,
            activity: Instant::now(),
            next_due: None,
            ticks: 0,
            last_status: OK,
            report: PwMaintenanceReport::default(),
        }
    }
    pub(super) fn note_activity(&mut self) {
        self.activity = Instant::now();
        self.next_due = None;
    }
    fn due(&self, now: Instant, idle: Duration) -> bool {
        now.saturating_duration_since(self.activity) >= idle
            && self.next_due.is_none_or(|next| now >= next)
    }
    fn complete(
        &mut self,
        result: portaweave_core::Result<portaweave_core::MaintenanceReport>,
        idle: Duration,
        interval: Duration,
    ) -> bool {
        // Only preflight HostSpace is retryable. A real StorageFull I/O error
        // can share the C NO_SPACE code but must stop after latching protection.
        let retry = matches!(&result, Ok(_) | Err(Error::HostSpace { .. }));
        self.ticks = self.ticks.saturating_add(1);
        let mut progress = false;
        match result {
            Ok(r) => {
                progress =
                    r.objects_removed != 0 || r.segments_removed != 0 || r.blocks_copied != 0;
                self.report = PwMaintenanceReport {
                    orphan_candidates: r.orphan_candidates,
                    objects_removed: r.objects_removed,
                    dead_segment_candidates: r.dead_segment_candidates,
                    segments_removed: r.segments_removed,
                    compaction_candidates: r.compaction_candidates,
                    blocks_copied: r.blocks_copied,
                };
                self.last_status = OK;
            }
            Err(error) => self.last_status = Fault::from(error).0,
        }
        // Quiet/pinned/deferred work backs off rather than busy-polling SQL.
        let delay = if progress { interval } else { interval.max(idle) };
        self.next_due = Instant::now().checked_add(delay);
        retry
    }
    fn status(&self) -> PwMaintenanceWorkerStatus {
        PwMaintenanceWorkerStatus {
            running: u32::from(self.worker.as_ref().is_some_and(|w| !w.join.is_finished())),
            last_status: self.last_status,
            ticks: self.ticks,
            report: self.report,
        }
    }
    pub(super) fn stop(&mut self) -> FResult<()> {
        if let Some(worker) = self.worker.take() {
            let _ = worker.stop.send(());
            // Safe while holding TABLE: the worker only TRY-locks it, and
            // the stop channel wakes recv_timeout immediately, not at expiry.
            if worker.join.join().is_err() {
                self.last_status = INTERNAL;
                return Err(Fault(INTERNAL));
            }
        }
        Ok(())
    }
}
impl Drop for State {
    fn drop(&mut self) {
        // Explicit stop/volume_close can report a join failure. Drop cannot.
        let _ = self.stop();
    }
}

fn worker_loop(volume: u64, idle: Duration, interval: Duration, stop: mpsc::Receiver<()>) {
    while let Err(mpsc::RecvTimeoutError::Timeout) = stop.recv_timeout(interval) {
        let mut table = match sessions().try_lock() {
            Ok(table) => table,
            Err(TryLockError::WouldBlock) => continue,
            Err(TryLockError::Poisoned(_)) => break,
        };
        let Some(session) = table.get_mut(&volume) else {
            break;
        };
        if !session.maintenance.due(Instant::now(), idle) {
            continue;
        }
        let result = session.volume.maintenance_step();
        if !session.maintenance.complete(result, idle, interval) {
            break;
        }
    }
}

/// Opt-in administrative worker, not OS authentication or a mounted filesystem.
#[unsafe(no_mangle)]
pub extern "C" fn pw_volume_maintenance_worker_start(
    volume: u64,
    idle_ms: u32,
    interval_ms: u32,
) -> i32 {
    run(|| {
        if !(10..=600_000).contains(&idle_ms) || !(10..=60_000).contains(&interval_ms) {
            return Err(Fault::invalid());
        }
        with_session_untracked(volume, |s| {
            if s.maintenance.worker.is_some() {
                return Err(Fault::busy());
            }
            if s.volume.is_write_protected() {
                return Err(Fault(READ_ONLY));
            }
            let idle = Duration::from_millis(u64::from(idle_ms));
            let interval = Duration::from_millis(u64::from(interval_ms));
            let (sender, receiver) = mpsc::channel();
            // Spawn failure occurs before publishing worker/control state.
            let join = thread::Builder::new()
                .name(format!("pw-maint-{volume}"))
                .spawn(move || worker_loop(volume, idle, interval, receiver))
                .map_err(|_| Fault(IO))?;
            s.maintenance = State {
                worker: Some(Worker { stop: sender, join }),
                activity: Instant::now(),
                next_due: None,
                ticks: 0,
                last_status: OK,
                report: PwMaintenanceReport::default(),
            };
            Ok(OK)
        })
    })
}

/// Idempotent stop/join. Busy volume close does not stop a live worker.
#[unsafe(no_mangle)]
pub extern "C" fn pw_volume_maintenance_worker_stop(volume: u64) -> i32 {
    run(|| {
        with_session_untracked(volume, |s| {
            s.maintenance.stop()?;
            Ok(OK)
        })
    })
}

/// # Safety
/// out_status is writable/aligned/non-aliasing; unchanged on failure. Polling
/// does not count as foreground activity. No empty-backlog/safe-eject claim.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pw_volume_maintenance_worker_status(
    volume: u64,
    out_status: *mut PwMaintenanceWorkerStatus,
) -> i32 {
    run(|| {
        required(out_status)?;
        with_session_untracked(volume, |s| {
            unsafe {
                out_status.write(s.maintenance.status());
            }
            Ok(OK)
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monotonic_idle_and_backoff_do_not_treat_zero_work_as_drained() {
        let mut state = State::new();
        let base = state.activity;
        let idle = Duration::from_secs(30);
        let interval = Duration::from_secs(1);
        assert!(!state.due(base + Duration::from_secs(29), idle));
        assert!(state.due(base + idle, idle));
        state.next_due = Some(base + Duration::from_secs(60));
        assert!(!state.due(base + idle, idle));
        assert!(state.due(base + Duration::from_secs(60), idle));
        let before = Instant::now();
        assert!(state.complete(
            Ok(portaweave_core::MaintenanceReport::default()),
            idle,
            interval
        ));
        assert_eq!(state.status().ticks, 1);
        assert!(state.next_due.unwrap() >= before + idle);
        state.note_activity();
        assert!(state.next_due.is_none());
        assert!(!state.due(state.activity, idle));
        assert!(state.complete(
            Err(Error::HostSpace {
                available_bytes: 0,
                required_bytes: 1,
            }),
            idle,
            interval
        ));
        assert_eq!(state.last_status, NO_SPACE);
        assert!(!state.complete(Err(Error::WriteProtected), idle, interval));
        assert_eq!(state.last_status, READ_ONLY);
        assert!(!state.complete(
            Err(Error::Io(std::io::Error::from(
                std::io::ErrorKind::StorageFull
            ))),
            idle,
            interval
        ));
        assert_eq!(state.last_status, NO_SPACE);
    }

    #[test]
    fn failed_join_is_reported_without_releasing_the_core_owner() {
        let volume = next_token().unwrap();
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "pw-join-{}-{stamp}-{volume}",
            std::process::id()
        ));
        let core = Volume::create(&root, 1 << 20).unwrap();
        let (stop, _receiver) = mpsc::channel();
        // Controlled controller-thread failure OUTSIDE the registry/core.
        // A panic while holding TABLE would instead poison/refuse that registry.
        let join: JoinHandle<()> = thread::spawn(|| panic!("controlled join failure"));
        with_table(|all| {
            let mut session = Session::new(core);
            session.maintenance.worker = Some(Worker { stop, join });
            assert!(all.insert(volume, session).is_none());
            Ok(())
        })
        .unwrap();
        assert_eq!(pw_volume_close(volume), INTERNAL);
        assert!(matches!(Volume::open(&root), Err(Error::Busy)));
        let status = with_session_untracked(volume, |s| Ok(s.maintenance.status())).unwrap();
        assert_eq!((status.running, status.last_status), (0, INTERNAL));
        assert_eq!(pw_volume_close(volume), OK);
        Volume::open(&root).unwrap().verify().unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
}
