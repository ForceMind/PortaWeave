# ADR 0021 — Preserve SQLite's locks during backend synchronization

Date: 2026-11-14. Status: implemented; exact-source validation is recorded in the PR.

## Problem

The M1-G backend barrier opened `catalog.sqlite` outside SQLite, called
`File::sync_all`, and closed that temporary descriptor while the owner's
SQLite connection was still live. On POSIX systems, closing **any** descriptor
for an inode releases that process's POSIX record locks on it. SQLite does not
know about a close performed by unrelated code. The storage owner lock is on
another inode and cannot restore the catalog's lost lock.

A new subprocess regression checks SQLite's 510-byte shared-lock range before
and after the barrier. The previous implementation loses the lock while its
connection can still read and write. The process uses a new executable and
independently opened descriptor; it does not use or drop an inherited Volume.

## Decision

Keep SQLite as the sole owner of live catalog/WAL descriptors and their sync
ordering. The existing `PRAGMA wal_checkpoint(FULL)` runs through that same
connection with `synchronous=FULL`, `fullfsync=ON`, and
`checkpoint_fullfsync=ON`. SQLite syncs pending WAL data, backfills the database,
and syncs the database before reporting a completed checkpoint. An already
completed checkpoint may reuse its synced state without issuing another sync.

The backend barrier still streams and syncs tracked segments, syncs their
directory, checks catalog/WAL path types without opening them, performs the
FULL checkpoint, then syncs the format marker and container directory.
It must not independently open/close, duplicate/close, hash, or copy live
SQLite files in the owner's process. Original-preserving recovery remains an
offline operation under exclusive ownership, with SQLite operating only on
its separate working copy.

No unsafe SQLite VFS pointer calls are added to the core. Its
`forbid(unsafe_code)` boundary and the persistent formats remain unchanged.
The Unix test uses the already locked `libc` version as a test-only dependency
for a small, reviewed `fcntl` call in a separate executable process.

## Errors and limits

Busy checkpoints remain BUSY without write protection. SQL, I/O, structure,
and injected failures retain the existing visible error and write-protection
behavior. A failure after work has occurred is not a promise of rollback.
Live handles and read leases keep their data and identity. Windows continues
to reject this Unix backend barrier before any partial operation.

The contract inherits SQLite's VFS synchronization behavior; it does not
promise an extra direct `F_FULLFSYNC` on every invocation. This change does
not certify kernel write draining, safe eject, hostile backing-directory
replacement, physical exFAT/USB durability, or a mounted development volume.

## Primary references

- [SQLite: POSIX locks canceled by an unrelated close](https://sqlite.org/howtocorrupt.html#posix_advisory_locks_canceled_by_a_separate_thread_doing_close)
- [SQLite FULL checkpoint contract](https://sqlite.org/c3ref/wal_checkpoint_v2.html)
- [SQLite WAL checkpoint synchronization](https://sqlite.org/wal.html#performance_considerations)
- [SQLite synchronization pragmas](https://sqlite.org/pragma.html#pragma_fullfsync)

This supersedes ADR 0004 only where it described independent catalog/WAL file
synchronization. Its native-mount and platform durability limits still apply.
