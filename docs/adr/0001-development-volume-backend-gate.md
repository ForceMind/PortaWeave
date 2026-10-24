# ADR 0001 — Development-volume semantics precede backend choice

Date: 2026-10-24. Status: **requirement accepted; backend selection conditional**.

## Context

The owner explicitly requires normal system/application operations and local Codex development inside the volume. The initial proposal excluded broad developer workloads and deferred complex link support. That scope is superseded by SPEC v0.2 and the compatibility contract.

Primary-source verification identifies a concrete gap: WinFsp's NTFS compatibility page lists hard links as unsupported. Its mmap, notifications and security facilities do not remove that gap. A real disk-like appearance is not semantic compatibility. FSKit likewise requires executable mapping, permission, event and sandbox testing, not assumption.

## Decision

Keep the portable Rust storage core, one authoritative SQLite catalog and append-only segments as the proposed storage design. Store stable object IDs independently of paths and design real hard-link identity and open-reference lifetime into the core.

Treat FSKit and WinFsp as candidates. Before freezing them, run FS-01..14 and the Codex/security probes. FS-07 on the documented WinFsp path is BLOCKED until reproduced and resolved. Review an upstream contribution, a maintained extension or an alternative native backend with licensing/signing/security costs. Do not substitute copies for links or quietly remove the requirement.

Developer databases, executable artifacts, package trees, Git worktrees, watchers and normal sandbox permissions are first-version validation gates. Special-purpose boot/paging/raw-block/formatting operations are separate and may return explicit unsupported errors.

## Prohibited shortcuts

No read-only product presented as a work disk. No hidden system-drive working copy. No faked ACL success, discarded xattrs, fake fsync, or unlink of reader-pinned segments. No requirement to disable Codex sandboxing, give all processes administrator rights, or make the volume world writable. A documented tool fallback is evidence for that mode only, never evidence for a missing syscall.

## Consequences

M0 is broader than a simple mount demo. UI follows capability proof. Native Windows, macOS and WSL have distinct evidence. Cloud tasks are remote, not a local-mount test. Native-host probe results validate the harness only. Full compatibility remains unproven and may require changing backend architecture; the requirement must not be diluted to preserve an early design.

## Sources

See SOURCES S2–S5 and S12–S17. In particular:
https://winfsp.dev/doc/NTFS-Compatibility/
https://developers.openai.com/codex/sandboxing
https://developers.openai.com/codex/windows/windows-sandbox
