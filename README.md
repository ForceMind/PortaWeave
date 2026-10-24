# PortaWeave

[中文](README.zh-CN.md)

> One portable working volume. Two platforms. Automatic space reclamation.

PortaWeave targets an exFAT-backed, dynamically growing directory container mounted as a **real read/write development volume** on macOS and Windows. Ordinary applications, local Codex, Git, editors, package managers and build tools must work directly inside it, without a hidden working copy on the system disk.

## Status — 2026-10-24

**M0-E experimental native C ABI plus M1-D bounded-read core + compatibility probes. Not a mountable filesystem or a production release.** The core stores immutable block segments with an authoritative SQLite catalog, namespace operations, shared hard-link identity, open-object lifetime, read leases and operation-triggered reclamation. It also has evidence-copy diagnosis/content export and separate maintenance status. M1-C adds host-space admission and data-segment I/O failure testing. Platform adapters, full metadata/security semantics, idle scheduling and the desktop client remain unimplemented. Never store the only copy in this experimental format.

Read [M1-A](docs/IMPLEMENTATION-M1A.zh-CN.md), [M1-B](docs/IMPLEMENTATION-M1B.zh-CN.md), [M1-C](docs/IMPLEMENTATION-M1C.zh-CN.md) and [validation evidence](docs/VALIDATION.zh-CN.md). Hosted core tests do not establish exFAT, mounted-volume or Codex compatibility. Exact source-version CI results are recorded on the corresponding PR; implementation documentation is not a test-pass claim.

FSKit and WinFsp remain candidates. Internal true hard links do not remove WinFsp's documented interface limitation. See the [backend ADR](docs/adr/0001-development-volume-backend-gate.md).


### Native adapter ABI (experimental)

The new `portaweave-native` library exports a versioned C header in [`include/portaweave_native.h`](include/portaweave_native.h), with actual Rust-core directory and file mutations, enumeration, file I/O, and lifecycle tokens. See [M0-E interface scope](docs/IMPLEMENTATION-M0E.zh-CN.md). **This is not an OS mount:** FSKit/WinFsp callbacks, security, notifications, safe unmount, and complete flush behavior are still missing. Do not treat a successful ABI test as Codex compatibility.

## M0-G WinFsp capacity / durability contract (experimental)

The existing native ABI now exposes logical quota and conservative host available-space snapshots plus an explicit SQLite WAL FULL checkpoint. **A catalog checkpoint is not an OS file flush.** The Windows WinFsp 2.1 callback contract compiles against pinned upstream headers and is exercised with a mock WinFsp object on GitHub Actions, without installing a driver or mounting a disk. The unimplemented access control and Flush callbacks fail closed. See [M0-G](docs/IMPLEMENTATION-M0G.zh-CN.md) and [native adapter readiness](docs/COMPATIBILITY.zh-CN.md).

## Run the storage lab

Use `rust-toolchain.toml` and the committed dependency lock. The example accepts only a NEW disposable directory; it does not mount or format a drive.

```sh
cargo test --workspace --all-features --locked
cargo test --workspace --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo fmt --all -- --check
cargo run --locked -p portaweave-core --example scratch -- /path/to/new-disposable-lab
```

Automatic reclamation currently runs in serialized operations, not an idle worker. Handles/read leases delay deletion. Segment-byte statistics exclude the database, WAL and host allocation overhead. Physical power loss and Windows directory flush remain unvalidated.

### Host-space and maintenance states

`space_status()` and `Stats.space` expose a host free-space snapshot and soft admission budget. Default headroom is 16 MiB plus a 1 MiB metadata allowance. This is **not physically allocated emergency storage or a reservation**. Other applications, host allocation granularity and SQLite growth can still exhaust the disk after a check succeeds.

A pre-stage `Error::HostSpace` preserves current references without poisoning the owner. Low-space compaction reports `DeferredSpace`, retains the old segment and retries on a later maintenance-triggering operation. Actual I/O/SQLite failures still latch protection. Failure after a successful business commit is reported separately as `Failed`; an actual commit error may remain indeterminate. Do not assume every error means rollback. Fault-injection hooks compile out of default builds and must not be enabled in a user-facing client.

## Original-preserving diagnosis and export

```sh
cargo run --locked -p portaweave-core --bin portaweave-recover -- verify SOURCE NEW_WORKSPACE
cargo run --locked -p portaweave-core --bin portaweave-recover -- export SOURCE NEW_WORKSPACE NEW_OUTPUT
```

Recovery copies evidence without opening the source in SQLite, recovers a separate working catalog, then strictly verifies/exports contents. It is not metadata-complete or a broken-catalog salvage algorithm. Reading may change access timestamps; this is not hardware write protection. See [scope and budgets](docs/IMPLEMENTATION-M1B.zh-CN.md).

All destinations must be new; partial work is preserved. Hard links are preserved by default; `--copy-hardlinks` explicitly produces separate copies and reports the difference. Completion requires `COMPLETE` and no `INCOMPLETE`. Export is a recovery tool, not a substitute for the required real working-volume interface.

## Engineering documents

- [Product/storage specification](docs/SPEC.zh-CN.md), the target rather than current feature claims
- [Development-volume contract](docs/COMPATIBILITY.zh-CN.md)
- [Roadmap](docs/ROADMAP.zh-CN.md) and [validation](docs/VALIDATION.zh-CN.md)
- [M1-A core](docs/IMPLEMENTATION-M1A.zh-CN.md), [M1-B recovery](docs/IMPLEMENTATION-M1B.zh-CN.md), [M1-C capacity/I/O](docs/IMPLEMENTATION-M1C.zh-CN.md)
- [Primary sources](docs/SOURCES.md), [agent rules](AGENTS.md), [handoff](WORK_HANDOFF.zh-CN.md)

## Host-directory probes

Python 3.10+ and the corresponding installed Git/Node tools. Use a disposable directory, never the raw container:

```sh
python tools/workspace_probe.py --root /path/to/scratch --label host-baseline --report /path/to/new-report.json
python -m unittest discover -s tests -v
```

Probes install no drivers, use no network, do not invoke Codex or alter permissions, and are not mounted-volume certification. One owner, safe computer handover and normal sandbox permissions remain mandatory.

Top-level licensing has not yet been selected by the owner. Third-party licenses remain independent. The name does not imply trademark clearance.
