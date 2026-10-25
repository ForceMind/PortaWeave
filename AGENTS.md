# Engineering rules

The repository contains an **experimental persistent Rust core and offline safety tools**, host-directory probes and specifications, not a mountable filesystem. Read `docs/IMPLEMENTATION-M1A.zh-CN.md`, `docs/IMPLEMENTATION-M1B.zh-CN.md` and `docs/VALIDATION.zh-CN.md` for actual scope/evidence; pre-implementation status in SPEC v0.2 is historical. Never restart the core or advertise production safety.

1. Read current repository state before editing. Never overwrite user changes or assume a ref/tool is available.
2. SPEC is the target design. Record architecture changes as ADRs. Unknown formats must not be silently rewritten; the current format is experimental.
3. Build actual system mounts. A folder, web page, simulated drive, core API or passthrough demo is not certification.
4. SQLite remains the authoritative transaction layer. Use committed Cargo.lock and reviewed patched dependencies. Do not silently upgrade them in CI.
5. Never retire old segments until replacements are durable, references committed, and readers no longer need them. Recovery must be idempotent.
6. Automatic reclamation is required. Never empty user trash, discard xattrs or require routine manual compaction. Distinguish operation-triggered serial GC from an idle scheduler.
7. One owner per container. Drop the catalog before its lock, explicitly release the lock, and retain inherited-descriptor regression tests. No stale-lock timeout shortcuts.
8. I/O/flush/short-write/space/corruption failures must be visible. Distinguish a failed business operation from failed maintenance after a successful business commit.
9. Use `Recovery::prepare`/`portaweave-recover` for original-preserving diagnosis. Copy supported raw files under exclusive ownership; SQLite may recover only a separate working catalog, not original/evidence. `Volume::open()` still recovers and runs GC. Never use it to inspect suspect originals or overwrite export targets.
10. Test crash boundaries and GC races from the outset. Hosted core tests and artifact handover do not prove exFAT, OS mount, Codex or physical power-loss compatibility.
11. Never format media, install privileged components, change the user's Mac or deploy a site without specific authorization.
12. Keep bilingual READMEs, evidence and limits current. Retain failed/superseded runs. Never fabricate results, versions, commits or releases.
13. Commit reviewed increments to authorized refs and verify exact remote SHAs. Remove one-shot write-capable workflows before merge; normal CI is read-only.
14. Continue feasible work within the current execution; no unattended-work promises.

## Mandatory development-volume contract

15. Read COMPATIBILITY and backend ADRs. Codex/Git/packages/builds/editors are release gates, not optional ideas.
16. Correct identity, links, atomic rename, execution, mapping, sharing, locks, notifications and effective permissions are required. No fake success or copies masquerading as hard links.
17. FSKit/WinFsp remain candidates. Internal hard links do not cure the WinFsp interface gap; resolve or replace deficient backends rather than narrow requirements.
18. Test Codex with normal supported sandbox/workspace approval. Never require disabling protection, universal admin access or relocating the working tree. Distinguish policy denials from storage errors.
19. Separate host probes, core libraries, passthrough and real mounted-volume evidence. Missing tools/platforms are NOT_RUN, never PASS.
20. Put project databases/dependencies/build trees in the actual mount. Native Windows, WSL and cloud are separate matrices; normal OS-specific binary rebuilds are expected.
21. Do not run paid agent tasks, change authentication or conduct destructive media tests without appropriate authorization. Offline fixtures are not Codex tests.
22. Whole-file read leases retain owner-wide admission limits; M1-E range leases select bounded indexes. Neither is an exact heap-byte budget. The 16 MiB per-call cap is not a global bound; parser/GC resource budgets still need work.
23. Raw fork of a live engine is unsupported. Do not use/drop an inherited owner in a child as a new session; test normal process spawning without weakening ownership.
24. Successful business results remain successful if later maintenance fails. Inspect `maintenance_status()`/Stats, surface warning and latched write protection. Actual commit errors can still be indeterminate, not promised rollback.
25. Recovery is not a hardware write blocker or hostile raw-namespace security boundary. Read atime may change. Keep failed workspaces/partial exports; completion requires the positive marker and no incomplete marker. Explicit alias-copy export must report that semantics differ from hard links.

26. M0-H Windows WinFsp lifecycle has opt-in scratch-only `--lab-mount` but it has not been exercised on a genuine installed driver. CI `--self-test` is NOT a mount test. Never certify Finder/Explorer, filesystem-wide fsync, hard links or Codex merely because dispatcher source compiles. Deny access and Flush until safe platform semantics are verified. Do not install privileged WinFsp drivers without specific authorization.

27. M0-J builds an unsigned macOS host App plus embedded ExtensionKit bundle. Full xcodebuild/link/metadata checks are build evidence only; never install/enable it or report a mounted disk from this CI. Keep sandbox entitlements and the fail-closed resource gate until volume operations, permissions, sync and real platform tests exist.

28. M1-F ordinary open validates the exact experimental catalog schema before config queries/WAL configuration/GC. Only CRLF-to-LF in schema SQL is normalized. Recovery uses the same gate on its working copy. Do not bypass it on Windows, silently drop unknown triggers/columns, or claim ordinary open preserves original bytes.

29. M1-G synchronize_storage is a backend Unix host-file/catalog/directory barrier, not native/kernel pending-write drain, parent-namespace creation durability, stronger device-cache flush or safe eject. Non-Unix must return unsupported. Keep WinFsp Flush and FSKit load gates closed until their full platform contracts are verified.

30. M0-K FSVolume callbacks are direct SDK/core contract code. v1 has no effective metadata/permissions or persistent volume UUID. Reject unknown attributes before mutation; keep unary load/activate/mount and access checks closed. FSMutableFileDataBuffer/packer system dispatch is NOT_RUN; direct ABI readback or a packing closure is not OS evidence. Do not mix macOS27 Handler signatures into the fixed macOS26 SDK target.

31. M1-H v2 metadata creation is explicit; default create stays v1 and there is no automatic migration. Metadata APIs are trusted storage operations, not caller authentication, effective POSIX permissions, Windows ACLs or sandbox certification. Never invent missing v1 attributes or advertise stored execute bits as working execution. Preserve same-transaction metadata changes, exact version/schema gates and original-preserving recovery. Content export does not apply source ownership/modes/times to the host and must say so.

32. M0-L initially added FSKit v2 metadata/UUID reads; M0-M adds trusted lab setters, still without OS caller authorization. Keep a separate random session tag for FSItem validation. Only supported fields may be marked valid; v1 missing metadata is unsupported. Direct SDK tests do not justify enabling access/load/mount or claiming executable files and sandbox compatibility.

33. M1-I mixed size/metadata uses one core transaction. Native adapters must not implement mixed setattr with independent truncate/set_metadata commits or consume unsupported fields after a partial change. It is still a trusted primitive, not authorization; keep native gates closed until caller/ACL/cache semantics and real mounts pass. Explicit mtime wins, ctime is core-maintained, and truncation clears file setid even when the requested mode contains it.

34. M0-M v2 FSKit setters consume supported fields only after one successful core operation. Unsupported creation attributes, invalid nanoseconds and mixed unsupported requests must fail before namespace/content mutation. Inherited laboratory uid/gid defaults are not OS caller credentials, umask or authorization. Metadata storage/capability flags do not permit enabling access/load/mount before effective permissions and native tests exist.

35. M1-J PosixSession enforces a mode-only namespace profile for explicit TRUSTED credentials; M1-K/L extend this to granted files/metadata changes/read leases, still not OS identity authentication. Never use daemon identity for every caller, accept uid/group claims from untrusted IPC, bypass ancestor checks using known directory IDs, or equate synthetic-credential tests with ACL/sandbox/mount evidence. Raw Volume/C APIs stay administrative. Symlinks and native identity mapping remain separate work. Keep all native access/load/mount/Flush gates closed; denial is not storage write protection.

36. M1-K data grants persist until close; do not recheck current pathname/mode to invalidate an already granted file. Current metadata authority is separate: a write grant cannot bypass owner/group checks, and mixed changes use one transaction. Keep administrative/policy token tables distinct, count both toward admission/BUSY, reject stale/foreign tokens and allow release under protection. Lifecycle IDs are not unguessable secrets or OS authentication. No native gate is enabled by synthetic descriptor tests.

37. M1-L policy range leases require granted READ even for empty ranges; retain absolute bounds/original EOF/old blocks and share owner-wide admission with administrative leases. Keep C lease tables distinct and include both in BUSY. A source close or later chmod must not revoke an admitted immutable lease. Tokens are process-local, not serializable/persistent authority; no metadata snapshot, heap budget or OS mmap certification is implied.

38. M1-M append/constrained position and length must be resolved inside one serialized owner request. Capture reply information before business commit; do not query afterward and turn known success into failure. Empty/clipped writes preserve object generation/metadata, while existing maintenance behavior remains. ABI output validation must precede mutation; no allocation-reservation, native cache or mount claim follows from core replies.

39. M0-N Windows time/size projection is pure and fallible BEFORE business commit, never after known success. Do not saturate unrepresentable metadata, invent mode-to-ACL mappings, claim rounded logical allocation is physical reservation, or make allocation setters successful no-ops. WinFsp I/O contexts carry policy grants; synthetic fixtures do not authenticate OS callers. Open/security/Flush remain closed and HardLinks remains an upstream interface blocker.

40. M0-O FSKit setattr replies are projected and object/count/time validated before commit. Successful Swift paths must not re-query attributes or throw a conversion after known commit; consume fields only after success and report health separately. These C setters are administrative, reject policy-token confusion/expected-object mismatch, retain explicit v1 metadata absence, and never supply OS caller authentication or enable load/mount. Pure reply projections must not perform external side effects.

## Disk Studio safety boundary (Issue #28)

- Do not reinterpret or silently migrate the existing .pwvol FILE filesystem into a partitionable raw BLOCK disk. Logical subvolumes and GPT/MBR are separate semantics.
- The new partition inspector MUST stay read-only and accept only explicit regular image files. No device paths, no symlinks, no automatic GPT repair; bounded GPT arrays, verified CRC32 and primary/backup correspondence are mandatory.
- A real block-device (.pwdisk) mapping needs independent LBA storage, transactional write ordering, copy-on-write, guest filesystem awareness, per-OS adapter certification and fault injection. A WinFsp/FSKit file mount is NOT a raw block disk.
- All physical disk changes require a separate privileged helper, exact stable device identity, offline exclusive access, explicit user authorization, verified external backup, dry-run preview and independent safety review. Do not format, clone-overwrite, resize or repair any physical disk autonomously; test only disposable images unless specifically authorized.
- Preserve all existing Codex/Git/mount/permissions/flush/GC release gates. Disk Studio is not a reason to deprioritize or falsely close them.
