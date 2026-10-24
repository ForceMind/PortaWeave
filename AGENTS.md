# Engineering rules

This repository begins as a specification, not a completed filesystem. Treat every unimplemented feature as planned and every unexecuted test as unverified.

1. Read the current repository before editing; do not assume a branch, commit, or tool is available. Never overwrite user changes.
2. Read `docs/SPEC.zh-CN.md` and record architecture changes as ADRs. Names and format identifiers are provisional until approved/frozen.
3. Build actual user-mode filesystem mounts. A folder, localhost site, simulated drive card, or mocked UI is not a mount.
4. Reuse SQLite for authoritative metadata. Do not add a second competing transaction authority. Pin reviewed dependencies, including a SQLite build with the WAL-reset fix.
5. Never retire an old segment until replacements are durable, metadata references are committed, and readers cannot need the old data. Crash recovery must be idempotent.
6. Automatic reclamation is required. Do not silently empty user trash, discard unsupported xattrs, or turn manual compaction into the normal workflow.
7. Enforce one owner per container. Never run destructive repair on an active volume. Never repair the sole original as an experiment.
8. Flush failures, ENOSPC, EIO, invalid checksums and short writes are errors, not success. Do not advertise unsupported capabilities.
9. Offline export must not depend on the GUI or filesystem driver. Preserve evidence and operate on copies for uncertain recovery.
10. Test crash boundaries and reclamation races from the first core milestone. Linux tests do not establish macOS/Windows or hardware power-loss correctness.
11. Do not format a physical drive, install privileged components, change the user's Mac, or deploy a site without specific authorization.
12. Keep Chinese and English READMEs, actual test reports, known limitations, and migration notes current. Never fabricate benchmarks, commits, releases or test passes.
13. Use only authorized repository writes. Commit meaningful, reviewed increments; report exact commit IDs only after verified tool success.
14. Continue feasible work within the current execution; do not imply asynchronous or unattended work after the interaction ends.

## Development-volume contract (mandatory, v0.2)

15. Read `docs/COMPATIBILITY.zh-CN.md` and the backend ADR before implementation. Codex/Git/package/build/editor workflows are release gates, not optional future ideas.
16. Preserve real filesystem semantics: stable file identity, links, atomic rename, executable permissions, mapping, shared access, locks, notifications and effective access control. Do not report a fake success or implement hard links as copies.
17. FSKit and WinFsp remain candidates. Document backend gaps (including WinFsp hard links), reproduce them, and change or extend the backend instead of quietly narrowing the product.
18. Test local Codex with its supported sandbox and approved workspace permissions. Never require disabling protections, broad administrator execution or relocating the project to the system drive as a compatibility workaround. Distinguish a policy denial from a filesystem failure.
19. Host-directory probe passes, mocks and passthrough prototypes are not evidence that the real container works. Keep missing-tool and unavailable-platform results separate from PASS; archive reproducible evidence with tool/OS/driver versions.
20. Test actual development databases and package/build trees inside the mounted volume. Native, WSL and cloud environments are separate matrices. Rebuilding OS-specific binaries on a different OS is normal, not proof of storage incompatibility.
21. Do not run agents, change authentication settings, install privileged software, format media or perform destructive power-loss tests without appropriate specific authorization. The initial probes are offline fixtures, not real Codex integration tests.
