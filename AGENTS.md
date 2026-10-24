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
22. Whole-file read-lease indexes remain a memory risk. The 16 MiB per-call cap is not a global bound; range indexes and parser/resource budgets need further work.
23. Raw fork of a live engine is unsupported. Do not use/drop an inherited owner in a child as a new session; test normal process spawning without weakening ownership.
24. Successful business results remain successful if later maintenance fails. Inspect `maintenance_status()`/Stats, surface warning and latched write protection. Actual commit errors can still be indeterminate, not promised rollback.
25. Recovery is not a hardware write blocker or hostile raw-namespace security boundary. Read atime may change. Keep failed workspaces/partial exports; completion requires the positive marker and no incomplete marker. Explicit alias-copy export must report that semantics differ from hard links.

26. M0-H Windows WinFsp lifecycle has opt-in scratch-only `--lab-mount` but it has not been exercised on a genuine installed driver. CI `--self-test` is NOT a mount test. Never certify Finder/Explorer, filesystem-wide fsync, hard links or Codex merely because dispatcher source compiles. Deny access and Flush until safe platform semantics are verified. Do not install privileged WinFsp drivers without specific authorization.
