# ADR 0002 — Release the owner lock explicitly, after the catalog

Date: 2026-10-24. Status: implemented in the experimental core; platform unmount contract still pending.

## Observation

The core integration suite passed on three OS runners, but a subsequent parallel Linux run failed when immediately reopening a just-dropped Volume (`Busy`, run 40074413948). This was not dismissed by rerunning the job.

An independently controlled Linux file-descriptor experiment reproduced the relevant mechanism: lock a file, duplicate its descriptor, close the original, then a fresh descriptor cannot acquire the lock until explicit unlock or final duplicate close. A concurrently spawned child may temporarily inherit descriptors before exec even when they are close-on-exec. The CI trace does not provide a complete syscall trace of that transient window, so this mechanism is the supported diagnosis rather than a fabricated trace.

Primary references:
- https://doc.rust-lang.org/std/fs/struct.File.html#method.try_lock
- https://doc.rust-lang.org/std/fs/struct.File.html#method.unlock
- https://www.man7.org/linux/man-pages/man2/flock.2.html

## Decision

Use an `OwnerLock` guard immediately after successful acquisition. Its destructor explicitly unlocks before closing the file. The SQLite connection is declared before the owner guard in `Volume`, so catalog teardown completes before ownership release. Error paths after acquisition also own the guard.

Do not add stale-file timeouts, ignore an active owner, remove the lock file, or make arbitrary retries pass the test. A live owner must continue excluding other open attempts.

Tests cover normal owner exclusion/release and, on Unix, keeping a duplicated description alive after the owner is dropped. Repeated parallel integration tests remain in Linux CI. The first corrected Linux run also completed ten full repetitions before the correction was committed.

## Boundaries

A destructor cannot report an unlock failure; file closure remains the fallback. A future explicit unmount API must separately report flush/release errors and must not advertise safe eject solely because Drop ran.

Raw fork-and-continue with an inherited live engine is unsupported. A child must not use or drop the inherited owner as if it independently owned the session: explicit unlock through the inherited description could release the parent's lock. Ordinary subprocess spawning and a single dedicated engine owner are the intended architecture.

This change neither implements FSKit/Windows mounts nor proves hardware persistence. It addresses ownership lifetime without changing the storage format or weakening the development-volume requirements.
