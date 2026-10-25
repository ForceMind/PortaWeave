# ADR 0006 — Separate mode policy from native identity authentication

Date: 2026-10-25. Status: experimental namespace policy implemented; native authentication and mount gates closed.

v2 stores uid/gid/mode but the administrative core APIs do not authorize requests. A native adapter must eventually bind authenticated OS request identities or a verified kernel permission contract to those stored values. Accepting an untrusted uid, using the daemon's identity for every application, or translating Windows SIDs directly into arbitrary POSIX integers would not provide that boundary.

Add a synchronous `Volume::posix(Credentials)` facade for the subset we can verify without a system mount. Credentials are explicit trusted effective IDs and at most 128 supplementary group claims, copied before use. There is no default identity. uid 0 has this profile's explicit privilege; a regular file still needs some execute bit for an execute request. Owner, group and other classes are exclusive, with no fallback to a more permissive class. This is not libc access(2)'s real-ID contract or a complete ACL model.

The component-name API checks search on every directory from the selected parent through logical Core ROOT, including the parent/root. Directory IDs do not bypass ancestors. Search is limited to 256 directories per request; budget refusals do not protect writes. Directory listing additionally requires read; namespace mutations require parent write and search. Sticky deletion/rename requires entry ownership, directory ownership or explicit uid 0 privilege. Both rename parents and any replacement target are checked. This profile does not use POSIX's optional writable-target exception. It checks permissions before semantic no-op/existence handling; exact native errno ordering is not certified.

Creation assigns caller uid, caller gid unless the parent has setgid, and requested permission bits masked by explicit umask. A setgid parent passes its group to both kinds and setgid to child directories. mkdir also accepts sticky. Caller-supplied creation setuid/setgid and other unsupported mode bits are rejected. The existing administrative create defaults are unchanged. ACL default inheritance, symlink traversal, chown/chmod policy, data-handle access and identity mapping are not provided by this increment.

The exclusive Rust owner borrow prevents another legitimate engine request from changing metadata/namespace between preflight and the existing business transaction. Raw backing-file tampering is outside this ownership contract. Business success/maintenance-warning behavior stays unchanged. Permission denial is distinct from storage write protection and does not trigger GC or poison the engine.

C ABI wrappers apply the same policy to trusted claims; they do not authenticate them. Raw APIs remain administrative. Adapters must never accept these claims from untrusted IPC or enable system access from a passing synthetic-credential test. FSKit access/probe/load/activate/mount and WinFsp security/Flush remain closed. Native OS permissions, ACLs, sandbox escape tests, executable loading and actual mounted workflows remain separate gates.

Primary references, reviewed 2026-10-25:

- POSIX.1-2024 general concepts, sections 4.5 and 4.7: https://pubs.opengroup.org/onlinepubs/9799919799/basedefs/V1_chap04.html
- Rename and sticky-directory behavior: https://pubs.opengroup.org/onlinepubs/9799919799/functions/rename.html
- mkdir creation masks and group choices: https://pubs.opengroup.org/onlinepubs/9799919799/functions/mkdir.html

The public HTML open returned HTTP 403 in this environment; indexed primary-source text was retrieved for the implementation review. The profile decisions above are explicit choices, not a claim of complete POSIX conformance.
