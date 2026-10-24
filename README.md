# PortaWeave

[中文](README.zh-CN.md)

> One portable working volume. Two platforms. Automatic space reclamation.

PortaWeave targets an exFAT-backed, dynamically growing directory container mounted as a **real read/write development volume** on macOS and Windows. Ordinary applications, local Codex, Git, editors, package managers and build tools must work directly inside it, without a hidden working copy on the system disk.

## Status — 2026-10-24

**M1-B experimental core safety increment + compatibility probes. Not a mountable filesystem or a production release.** The core stores real immutable block segments with an authoritative SQLite catalog, namespace operations, shared hard-link identity, open-object lifetime, read leases and automatically triggered reclamation. It now also has an independent evidence-copy diagnosis/content-export path and explicit maintenance status. Platform adapters, full metadata/security semantics, idle maintenance scheduling and the desktop client remain unimplemented. Never store the only copy of data in this experimental format.

Read the [M1-A core scope](docs/IMPLEMENTATION-M1A.zh-CN.md), [M1-B safety increment](docs/IMPLEMENTATION-M1B.zh-CN.md) and [validation evidence](docs/VALIDATION.zh-CN.md). A core library running on a hosted macOS/Windows runner does not establish exFAT, mounted-volume or Codex compatibility.

FSKit and WinFsp remain **candidates**, not validated solutions. The core's true hard links do not remove WinFsp's documented hard-link limitation. See the [backend ADR](docs/adr/0001-development-volume-backend-gate.md).

## Run the storage lab

Use the toolchain pinned in `rust-toolchain.toml` and committed dependency lock. The example accepts only a NEW disposable directory; it does not mount or format a drive.

```sh
cargo test --workspace --all-features --locked
cargo test --workspace --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo fmt --all -- --check
cargo run --locked -p portaweave-core --example scratch -- /path/to/new-disposable-lab
```

Automatic reclamation currently runs in serialized operations, not an idle worker. Open handles and read leases delay deletion. Segment-byte statistics exclude the database, WAL and host allocation overhead. Physical power-loss durability and Windows directory flush remain unvalidated.

## Offline original-preserving diagnosis and export

```sh
cargo run --locked -p portaweave-core --bin portaweave-recover -- verify SOURCE NEW_WORKSPACE
cargo run --locked -p portaweave-core --bin portaweave-recover -- export SOURCE NEW_WORKSPACE NEW_OUTPUT
```

The recovery path snapshots evidence without opening the source in SQLite, recovers a separate working catalog, then strictly verifies/exports contents. It is not metadata-complete or a salvage algorithm for a broken catalog. Reading may change host access timestamps; this is not a hardware write blocker. See [scope and budgets](docs/IMPLEMENTATION-M1B.zh-CN.md).

All output directories must be new. Partial work is preserved; never retry into an existing destination. Hard links are preserved by default; `--copy-hardlinks` explicitly produces separate copies with a report. An export is complete only when `COMPLETE` exists and `INCOMPLETE` does not.

Successful mutations report subsequent GC failures through `maintenance_status()`/Stats and latch write protection, rather than misreporting an already committed write as failed. An error from the actual transaction commit can still be indeterminate; never assume all errors imply rollback.

## Engineering documents

- [Product and storage specification](docs/SPEC.zh-CN.md), the design target rather than current feature claims
- [Development-volume compatibility contract](docs/COMPATIBILITY.zh-CN.md)
- [Milestones and release gates](docs/ROADMAP.zh-CN.md)
- [M1-A implementation](docs/IMPLEMENTATION-M1A.zh-CN.md) and [M1-B safety tools](docs/IMPLEMENTATION-M1B.zh-CN.md)
- [Validation and evidence](docs/VALIDATION.zh-CN.md)
- [Primary technical sources](docs/SOURCES.md)
- [Contributor/agent rules](AGENTS.md) and [work handoff](WORK_HANDOFF.zh-CN.md)

## Host-directory probes

Python 3.10+; Git and Node are needed for their corresponding tests. Use a disposable directory, never the raw backing container:

```sh
python tools/workspace_probe.py --root /path/to/scratch --label host-baseline --report /path/to/new-report.json
python -m unittest discover -s tests -v
```

These probes install no drivers, use no network, do not invoke Codex or change its permissions, and are not certification of the actual container. The product still requires one owner per container, safe handover between computers, normal sandbox permissions and real platform-level evidence.

Top-level licensing has not yet been selected by the owner. Third-party licenses remain independent. The name does not imply trademark clearance.
