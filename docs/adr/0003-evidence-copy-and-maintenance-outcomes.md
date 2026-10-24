# ADR 0003 — Preserve original evidence and separate maintenance outcomes

Date: 2026-10-24. Status: implemented experimentally in PR #4; platform evidence is recorded separately.

`Volume::open()` remains an owner-controlled writable recovery path. It must not be reused for original-preserving diagnosis. `Recovery::prepare` copies supported raw files under the same exclusive owner lock using read-only source descriptors, hashes the snapshot, then releases the source. SQLite opens a second working catalog, never the original or evidence catalog. WAL stays with the database; the working SHM is rebuilt. No original GC, repair or in-place mutation is allowed.

Snapshot/output targets must be new and outside the source/workspace. An incomplete marker survives failure; completion requires the positive marker and absence of the incomplete marker. Diagnostic parsing and export have explicit budgets; unknown/nonnormal source entries or invalid paths are refused. Strict content export preserves real host hard links by default or explicitly reports independent aliases when requested. It is not ACL/xattr/time-complete backup or damaged-catalog salvage.

Successful business mutation results must survive a later GC error. `MaintenanceStatus::Failed` retains the warning and latches write protection; later mutations fail, while reads/handle closure remain available. An error from the business action or the actual SQLite commit is still returned and may be indeterminate. Do not claim all errors mean rollback.

This is not protection against a hostile process concurrently rewriting raw directories, a hardware write blocker, or a physical durability guarantee. Read access timestamps may change. The current format and serial maintenance behavior stay experimental; M0 and the remaining M1-B gates are not waived.
