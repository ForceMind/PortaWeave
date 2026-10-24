# Technical sources

Checked on 2026-10-24. These primary sources support platform and dependency facts; they do not validate PortaWeave's proposed implementation. The design choices, thresholds, milestones and performance goals are proposals.

[S1] Microsoft — File System Functionality Comparison. exFAT lacks sparse files and metadata-only journaling.
https://learn.microsoft.com/en-us/windows/win32/fileio/filesystem-functionality-comparison

[S2] Apple — FSKit; user-space filesystem framework.
https://developer.apple.com/documentation/fskit

[S3] Apple — FSPathURLResource; path-backed resource. Also see the passthrough sample. Exact target-OS availability, permissions and lifecycle remain M0 validation items; the full sample was not downloaded or executed in this research session.
https://developer.apple.com/documentation/fskit/fspathurlresource
https://developer.apple.com/documentation/fskit/building-a-passthrough-file-system

[S4] macFUSE — FUSE Backends. The page consulted was edited 2026-10-19 and lists remaining limitations of the macFUSE FSKit backend. Do not generalize these to every native FSKit implementation.
https://github.com/macfuse/macfuse/wiki/FUSE-Backends

[S5] WinFsp — Design. User-mode filesystem interface, caching and mapping integration, filesystem rather than raw disk implementation.
https://winfsp.dev/doc/WinFsp-Design/

[S6] SQLite — Write-Ahead Logging. Includes WAL requirements, checkpointing and the WAL-reset bug fix information. A patched version is a minimum requirement, not an assurance of whole-product correctness.
https://www.sqlite.org/wal.html

[S7] SQLite — PRAGMA reference. synchronous, fullfsync, auto_vacuum and incremental_vacuum require explicit testing in the selected configuration.
https://www.sqlite.org/pragma.html

[S8] SQLite — Atomic Commit. Hardware, locking and flush assumptions and failure modes.
https://www.sqlite.org/atomiccommit.html

[S9] Microsoft — FlushFileBuffers and File Buffering.
https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-flushfilebuffers
https://learn.microsoft.com/en-us/windows/win32/fileio/file-buffering

[S10] WinFsp — Testing. Includes winfsp-tests, winfstest, FSX, fault and performance testing.
https://winfsp.dev/doc/WinFsp-Testing/

[S11] WinFsp — Licensing and source license.
https://winfsp.dev/src/
https://github.com/winfsp/winfsp/blob/master/License.txt

No filesystem, driver, performance, hardware removal or power-loss experiment was run as part of this planning package. The package contains no vendor code, binaries or font files.

## v0.2 verification — development workspaces

[S12] WinFsp — NTFS Compatibility. Explicitly lists hard links and several specialized controls as unsupported. A selection blocker, not something a Rust catalog can automatically cure.
https://winfsp.dev/doc/NTFS-Compatibility/

[S13] OpenAI — Sandbox and permissions. Local permission boundaries vary by platform and profile; policy-protected Git/config paths require appropriate approval, not disabling isolation.
https://developers.openai.com/codex/sandboxing
https://developers.openai.com/codex/permissions

[S14] OpenAI — Windows sandbox. Verify the actual sandbox implementation, permissions, device prerequisites and identity/session visibility. Do not assume a single Windows implementation forever.
https://developers.openai.com/codex/windows/windows-sandbox

[S15] pnpm — Symlinked node_modules structure. Hard links and symbolic links matter to package workflows.
https://pnpm.io/symlinked-node-modules-structure

[S16] Astral uv — Caching and settings. Linking and legitimate copy fallback must be recorded separately from filesystem hard-link support.
https://docs.astral.sh/uv/concepts/cache/
https://docs.astral.sh/uv/reference/settings/

[S17] OpenAI — Codex cloud environments. Remote repository environments do not automatically expose a locally mounted removable drive.
https://learn.chatgpt.com/docs/environments/cloud-environment

The v0.2 repository adds an initial host-directory probe harness; its executed results belong in VALIDATION. No actual PortaWeave mount, Codex integration, drive removal or power-loss experiment is established by this document. Source statements were checked on 2026-10-24 and must be rechecked when selecting release dependencies.
