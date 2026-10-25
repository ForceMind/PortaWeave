# ADR 0007 — Keep descriptor data grants separate from current metadata authority

Date: 2026-10-25. Status: experimental core/C ABI implemented; native identity binding remains closed.

The namespace facade must not return an unrestricted administrative Handle after checking read-only access. Introduce an opaque `PosixFile` with a core handle and immutable read/write grant. `PosixSession::open_file` first checks ancestor search and current mode, then admits a Read, Write or ReadWrite grant. File stat/metadata are available through a valid grant. Data methods validate the owning Volume handle table and requested grant before invoking existing I/O; closing invalidates every copy. Explicit close releases the open inode even under write protection. Token values are lifecycle identifiers, not authentication secrets.

Later mode/owner/ancestor/name changes do not revoke granted data access. A previously opened file remains usable after rename/unlink until its final reference closes, while a new open must pass current checks. Foreign owners, stale tokens and administrative/policy token confusion are rejected. Core policy admission counts all owner handles against 4096; native administrative and policy tables also share a 4096 limit and both block volume close. This is not an exact memory budget, multi-process descriptor protocol or OS mmap.

Metadata changes require current authority independently of a descriptor's data grant. This profile restricts nonempty explicit time/mode/ownership updates to the current owner or explicit uid 0. A nonprivileged owner cannot change uid, and an explicitly requested gid must be a claimed primary/supplementary group even when unchanged. A requested setgid mode is cleared if the nonprivileged owner is not in the resulting group. Any explicit regular-file ownership request clears setuid/setgid, including same-ID requests. No automatic atime or UTIME_NOW/OMIT interface is added. Stored setid does not certify privileged execution.

Mixed size/metadata requires a write grant and current metadata authority before the existing single core transaction. Empty metadata updates permit ordinary granted truncation. Parameter/authority refusal leaves size, bytes, attributes and generation unchanged. Commit failure can still be indeterminate; a successful commit followed by maintenance failure remains success plus its separate warning/protection state.

The C API uses a separate policy-file table. Old administrative I/O/lease APIs cannot accept a policy-file token, and the new policy I/O cannot accept administrative tokens. Data calls use the previously granted capability; metadata calls take current trusted claims. Native adapters must bind both to authenticated OS contexts before exposing them. No unauthenticated IPC, FSKit access/load/mount, WinFsp security/Flush, kernel cache bypass or mounted sandbox claim is enabled.

Primary review references: POSIX open/descriptor access and ownership restrictions. Public indexed primary-source text was consulted; these are explicit profile decisions, not full syscall/errno conformance:

- https://pubs.opengroup.org/onlinepubs/9799919799/functions/open.html
- https://pubs.opengroup.org/onlinepubs/9699919799.2013edition/functions/chown.html

Native Windows ACLs/SIDs, FSKit caller/kernel delegation, protected backing access, symlinks, locks, notifications, range-lease policy and real mounted testing remain separate work.

M1-N (2026-10-26) adds a metadata-only request on an existing live policy file. Current owner/group rules still apply independently of data rights, while a new pathname/ancestor lookup would incorrectly revoke already-open inode access after unlink or directory changes. The same metadata transaction builds FileInfo and runs any pure fallible platform projection before commit. Empty requests preserve generation/ctime. The additive C info API returns that completed result; the Windows bridge uses a separate projection. This is not native WRITE_ATTRIBUTES or SID/ACL authorization and opens no native gate.
