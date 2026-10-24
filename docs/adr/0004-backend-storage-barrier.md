# ADR 0004 — Separate backend synchronization from native cache/lifecycle completion

Date: 2026-10-25. Status: implemented experimentally; native mount certification pending.

The existing FULL SQLite WAL checkpoint is a catalog operation, not a filesystem flush. Native adapters need an explicit backend barrier before they can complete their own quiesce/cache/unmount protocol.

`synchronize_storage` serially synchronizes every tracked segment (including reader-pinned old segments), the segments directory, a FULL SQLite checkpoint, catalog and optional WAL files, the format marker, then the container directory. It does not materialize the complete segment index, close handles/read leases, run GC, eject media or claim to drain requests still pending in the kernel/adapter. It does not synchronize ancestors above the container or promise initial creation of a new container is durable in its parent's namespace. Format and ownership rules remain unchanged.

On Unix, actual host file/directory sync errors propagate and latch write protection. A busy checkpoint remains BUSY without poisoning writes. Test-only failures at the segment/catalog boundaries preserve readable committed content and must not return success. Default builds contain no fault controls.

On Windows/non-Unix, this API returns UnsupportedOperation before performing a partial barrier. The existing portable directory helper's no-op is never accepted as storage-sync success. WinFsp Flush remains unsupported pending a reviewed implementation and native validation.

This is a backend host-filesystem barrier only. macOS stronger device-cache flush, physical exFAT/USB behavior, kernel cache ordering, FSKit flags, effective permissions and safe unmount are separate unresolved gates. Never certify a mounted development volume, safe unplug or Codex compatibility from a passing host-directory barrier test.
