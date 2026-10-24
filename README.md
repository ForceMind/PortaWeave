# PortaWeave

[中文](README.zh-CN.md)

> One portable working volume. Two platforms. Automatic space reclamation.

PortaWeave targets an exFAT-backed, dynamically growing directory container mounted as a **real read/write development volume** on macOS and Windows. Ordinary applications, local Codex, Git, editors, package managers and build tools must work directly inside it, without a hidden working copy on the system disk.

## Status — 2026-10-24

**M1-A experimental persistent Rust core + compatibility probes. Not a mountable filesystem or a production release.** The core now stores real immutable block segments with an authoritative SQLite catalog, namespace operations, shared hard-link identity, open-object lifetime, read leases and automatically triggered reclamation. Platform adapters, full metadata/security semantics, idle maintenance scheduling and the desktop client remain unimplemented. Never store the only copy of data in this experimental format.

Read the [actual implementation scope and limitations](docs/IMPLEMENTATION-M1A.zh-CN.md) and [validation evidence](docs/VALIDATION.zh-CN.md). A core library running on a hosted macOS/Windows runner does not establish exFAT, mounted-volume or Codex compatibility.

FSKit and WinFsp remain **candidates**, not validated solutions. The core's true hard links do not remove WinFsp's documented hard-link limitation. See the [backend ADR](docs/adr/0001-development-volume-backend-gate.md).

## Run the storage lab

Use the toolchain pinned in `rust-toolchain.toml` and committed dependency lock. The example accepts only a NEW disposable directory; it does not mount or format a drive.

```sh
cargo test --workspace --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo fmt --all -- --check
cargo run --locked -p portaweave-core --example scratch -- /path/to/new-disposable-lab
```

Automatic reclamation currently runs in serialized operations, not an idle worker. Open handles and read leases delay deletion. Segment-byte statistics exclude the database, WAL and host allocation overhead. Physical power-loss durability, Windows directory flush, forensic read-only recovery and a complete exporter are not yet validated or implemented.

## Engineering documents

- [Product and storage specification](docs/SPEC.zh-CN.md), the design target rather than current feature claims
- [Development-volume compatibility contract](docs/COMPATIBILITY.zh-CN.md)
- [Milestones and release gates](docs/ROADMAP.zh-CN.md)
- [Actual M1-A implementation](docs/IMPLEMENTATION-M1A.zh-CN.md)
- [Validation and evidence](docs/VALIDATION.zh-CN.md)
- [Primary technical sources](docs/SOURCES.md)
- [Contributor/agent rules](AGENTS.md) and [work handoff](WORK_HANDOFF.zh-CN.md)

## Host-directory probes

Python 3.10+; Git and Node are needed for their corresponding tests. Use a disposable directory, never the raw backing container:

```sh
python tools/workspace_probe.py --root /path/to/scratch --label host-baseline --report /path/to/new-report.json
python -m unittest discover -s tests -v
```

These probes install no drivers, use no network, do not invoke Codex or change its permissions, and are not a certification of the actual container. The product still requires one owner per container, safe handover between computers, normal sandbox permissions and real platform-level evidence.

Top-level licensing has not yet been selected by the owner. Third-party licenses remain independent. The name does not imply trademark clearance.
