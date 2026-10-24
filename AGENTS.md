# Engineering rules

The repository now contains an **experimental persistent Rust core**, host-directory probes and specifications, not a completed or mountable filesystem. Read `docs/IMPLEMENTATION-M1A.zh-CN.md` and `docs/VALIDATION.zh-CN.md` for actual implementation/evidence; pre-implementation status wording in SPEC v0.2 is historical. Never restart the core from scratch or advertise it as production storage.

1. Read the current repository before editing; do not assume a branch, commit, or tool is available. Never overwrite user changes.
2. Read `docs/SPEC.zh-CN.md` as the target design and record changes as ADRs. Format identifiers remain experimental. Unknown formats must not be silently rewritten.
3. Build actual user-mode filesystem mounts. A folder, localhost site, simulated drive card, core API test or mocked UI is not a mount.
4. Reuse SQLite for authoritative metadata. Do not add a second competing transaction authority. Use the committed Cargo.lock and reviewed patched SQLite build; do not silently re-resolve dependencies in CI.
5. Never retire an old segment until replacements are durable, references committed, and readers cannot need it. Recovery must be idempotent.
6. Automatic reclamation is required. Never silently empty trash, discard unsupported xattrs or turn manual compaction into normal usage. Distinguish current serialized GC from a future idle scheduler.
7. Enforce one owner per container. Drop the catalog before the owner guard. Explicitly release the lock so duplicated/inherited descriptors do not retain ownership. Do not replace real exclusivity with stale-file timeouts or arbitrary retries.
8. Flush failures, ENOSPC, EIO, invalid checksums and short writes are errors, not success. A maintenance error after a committed write is not a rollback promise; separate these outcomes before platform integration.
9. Independent readonly diagnosis/export is still pending. The existing `Volume::open()` performs recovery and reclamation, and `verify()` is not a forensic opener. Preserve evidence and work on copies for uncertain recovery.
10. Test crash boundaries and reclamation races from the first milestone. Core tests on hosted OS runners and artifact handover do not establish real exFAT, mounted-volume, Codex or hardware power-loss correctness.
11. Never format physical media, install privileged components, change the user's Mac or deploy a site without specific authorization.
12. Keep Chinese/English READMEs, test reports, limitations and migration notes current. Never fabricate benchmarks, commits, releases or passes. Keep failed and superseded runs traceable.
13. Commit meaningful, reviewed increments to authorized refs; verify exact remote SHAs. Remove one-shot write-capable maintenance workflows before merging; normal CI is read-only.
14. Continue feasible work within the current execution; do not imply unattended work after the interaction ends.

## Mandatory development-volume contract

15. Read `docs/COMPATIBILITY.zh-CN.md` and the backend ADR. Codex/Git/package/build/editor workflows are release gates, not optional future ideas.
16. Preserve real semantics: stable identity, links, atomic rename, executable permissions, mapping, sharing, locks, notifications and effective access control. Never fake success or represent a hard link as a copy.
17. FSKit and WinFsp remain candidates. The core's hard links do not cure WinFsp's documented interface gap. Reproduce and resolve backend deficiencies rather than narrowing the product.
18. Test local Codex with supported sandbox and approved workspace permissions. Never require disabling protections, universal administrator access or moving project contents to the system disk. Distinguish policy denials from filesystem failures.
19. Host probes, mocks, core libraries and passthrough prototypes have different evidence levels. Missing tools/platforms are not PASS. Record OS, driver, engine, tool and policy versions.
20. Test development databases and package/build trees inside the real mounted volume. Native Windows, WSL and cloud tasks are separate matrices. Rebuilding OS-specific binaries is normal.
21. Do not run agents, change authentication, install privileged software or perform destructive removal tests without appropriate authorization. Current fixtures are not real Codex integration tests.
22. Current read leases collect whole-file block references. A 16 MiB per-call I/O cap is not a global memory budget; implement range-bounded indexes and limits before large/untrusted workloads.
23. Treat raw `fork` of a live engine as unsupported: never keep using or drop an inherited owner in the child as though it owns the parent's session. Ordinary application process spawning must be tested without weakening ownership.
