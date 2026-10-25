# ADR 0011 — Admit creation resources before mutation and return a pinned result

Date: 2026-10-25. Status: experimental core/C ABI/FSKit contract; native mount gates closed.

Creating a namespace entry and then calling a fallible open/intern can report failure after the name already committed. A preliminary Swift cache-count check cannot cover the shared administrative/policy native handle budget.

The trusted create request now reserves file admission and monotonically allocated identifiers before mutation. The same SQLite transaction creates the object/name, metadata and parent times and runs a pure reply projection. After a successful commit it publishes the core pin before post-operation maintenance. The C layer holds its owner mutex over admission, mutation and publication; Swift prepares a pending FSItem/cache capacity before the call and binds the returned identity/pin without another query or open. Directories have no regular-file pin.

Projection refusal/BeforeCommit failure publishes neither name nor handle. Reserved tokens may be consumed but are never reused or exposed as valid handles. Known business success survives subsequent maintenance failure; close remains possible under protection. Commit failures can still be indeterminate. Admission bounds logical resources, not exact heap allocation: memory exhaustion/abort is a process failure, not a promised rollback or a solved resource budget.

The new handles are administrative, not authenticated OS capabilities. Do not accept them as policy tokens or serialize them across owners/systems. Default v1, explicit v2 and the exact schema gate stay unchanged. FSKit access/load/mount and WinFsp security/Open/Flush remain closed. Disk Studio's separate read-only block-plane boundary is unaffected.

Tests exercise genuine shared-budget exhaustion, failed output/name/parent preservation, old unlinked file lifetime, before/after-commit faults, SDK pending-item publication and native BUSY/close behavior. SDK/C/artifact success does not certify a mounted development disk or Codex sandbox.
