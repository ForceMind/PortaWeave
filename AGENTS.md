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
