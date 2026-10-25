# ADR 0008 — Resolve writes and capture their replies before commit

Date: 2026-10-25. Status: implemented experimentally; native dispatch/mount certification pending.

Native write contracts include append and constrained writes that cannot extend EOF. Querying size through one ABI call and writing through another admits interleaving, even when both individual calls are serialized. Likewise, querying attributes after a successful write can turn a known committed business operation into an apparent failure if that second query fails.

Resolve append position and constrained length within the same exclusive owner request. The existing block staging and SQLite transaction remain authoritative. Select reply information from that business transaction before commit, and retain the value across subsequent maintenance. Truncate uses the same pre-commit reply capture. There is no second transaction, journal, dependency, format change or v1 migration.

Zero/fully clipped writes preserve content, generation and timestamps; maintenance follows the existing empty-write contract. Validate the original request length before clipping and the resolved address range before I/O. Immutable policy grants, stale/foreign handle rejection, old read leases and logical quota rules remain unchanged. Native tokens remain process-local and are not caller authentication.

The additive C API holds its owner mutex across EOF resolution, data mutation and reply capture. Concurrent append tests must demonstrate distinct contiguous offsets and complete records, not merely a final file length. Model/fault tests compare the returned identity/attributes/generation with committed state and reopen results. A commit error can still be indeterminate; a later maintenance error is separate from business success.

These reply structures describe committed core state, not Windows allocation reservation, kernel cache coherency, native mmap, flush or safe unmount. Windows callbacks still need a faithful attribute/time projection, authenticated Open/ACL/shared access and real driver tests before any access gate opens.

Primary native contract reviewed from the pinned WinFsp v2.1 header (`ddca7bd5481857a65ba552f643b8776fd070836f`):
https://github.com/winfsp/winfsp/blob/ddca7bd5481857a65ba552f643b8776fd070836f/inc/winfsp/winfsp.h
https://winfsp.dev/doc/WinFsp-API-winfsp.h/
