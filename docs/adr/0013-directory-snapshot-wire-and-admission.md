# ADR 0013 — Immutable directory metadata, typed admission and pinned WinFsp wire records

Date: 2026-10-26. Status: experimental core/C/WinFsp directory callback; native mount gates closed.

Follow-up 2026-10-27: [ADR 0014](0014-current-directory-info-and-atomic-replacement.md) adds freshly authorized linked-directory info and explicit atomic snapshot replacement. M0-T's captured data still grants no live inode or kernel refresh; the historical decision below retains its original scope.

Directory pages must not resolve an entry's current attributes independently after earlier names have been captured. A rename/unlink could otherwise change identities, remove an inode or introduce a fallible projection halfway through paging. Capture names, directory/parent and child metadata after trusted READ+SEARCH/all-ancestor checks under one serialized owner borrow. Preflight the entry count and project the entire native result before publishing its token.

This immutable metadata is owned data, not a data lease, live inode pin or live directory grant. It may outlive rename/chmod/unlink/GC; acquiring another snapshot requires current policy again. v1 has no invented metadata. It does not defend against an external writer violating single-owner/raw-backing constraints and does not bind Windows SIDs or OS callers.

Keep Windows and administrative cursor tables typed and volume-scoped, while sharing 64 active cursors and 100000 retained entries per owner, including non-root dot entries. Failed admission publishes nothing. Closing releases metadata without GC. Packing and close use a single native owner mutex, so one read returns complete records or an error with unchanged outputs. Counts are not exact heap/OOM budgets or full parser/GC limits.

The Windows bridge independently emits the pinned WinFsp 2.1 directory wire layout: 104-byte header, UTF-16LE name, 8-byte aligned records, zero padding, two-byte zero EOF, NextOffset ordinal cookies. The actual MSVC callback translation unit asserts SDK sizes/offsets/alignment and parses returned bytes with SDK types. Service and tests share DirectoryMarkerAsNextOffset=1 configuration; markers are 8-byte values, not strings. FSD-owned pattern filtering and actual kernel dispatch remain separate evidence. No DLL/helper source was copied or driver dependency added.

A synthetic directory context holds this snapshot only. Replaying its first page does not refresh it, and stale metadata must not be used as current directory GetFileInfo. Directory Open/GFI/refresh/notifications/share/delete/ACL semantics remain unfinished. FSKit pressure recovery may evict only its own existing LRU name snapshots, with explicit stale verifiers, never foreign Windows snapshots or file pins. All native security/Open/Flush and FSKit mount gates remain closed.

Test permissions and inherited search, exact names/metadata/parent and no live pins; Windows surrogate/max-name boundaries, every pagination size and EOF, invalid output/token/cookie, shared cursor/count limits, concurrent reads/close and projection rejection. Validate C/MSVC/SDK/bundle/artifact levels separately. Only an actual authorized OS mount plus developer workflows and normal Codex sandbox/escape tests can satisfy the development-volume contract.

Primary pinned sources: WinFsp ddca7bd5481857a65ba552f643b8776fd070836f, inc/winfsp/{winfsp.h,fsctl.h}, src/dll/fsop.c and src/sys/dirctl.c. See IMPLEMENTATION-M0T for URLs, results and remaining limits.
