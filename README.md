# PortaWeave

[中文](README.zh-CN.md)

> One portable working volume. Two platforms. Automatic space reclamation.

PortaWeave targets an exFAT-backed, dynamically growing directory container mounted as a **real read/write development volume** on macOS and Windows. Ordinary applications, local Codex, Git, editors, package managers and build tools must work directly inside it, without a hidden working copy on the system disk.

## Disk Studio (partition management, experimental)

DiskGenius-style partition management is a **separate future product module**, not a reinterpretation of the existing file-based .pwvol format. See [Disk Studio architecture](docs/DISK-STUDIO.zh-CN.md), [ADR 0010](docs/adr/0010-partition-manager-separate-block-plane.md), and [Issue #28](https://github.com/ForceMind/PortaWeave/issues/28).

**M0 implements read-only GPT/MBR inspection of regular RAW/IMG files only** (no physical devices, no symlinks, no writes, no filesystem mount, no partition editing or recovery):

~~~sh
cargo run --locked -p portaweave-partition-inspector -- inspect /path/to/disposable.raw --sector-size 512
cargo test --locked -p portaweave-partition-inspector
~~~

GPT requires valid CRC32 header/entry tables; it reports damaged primary/backup as DEGRADED, does not repair anything, and refuses ambiguous hybrid MBR/GPT. MBR extended EBR logical partitions are not yet enumerated. A virtual partitionable block device (.pwdisk), physical device management and resize/clone/recovery are **planned, not implemented**. None of these claims weakens the standard Codex/Git working-volume release gates.

## Status — 2026-10-27

**M0-V v3 FSKit directory item references, M1-O versioned inode lifetimes, laboratory WinFsp/FSVolume callbacks, unsigned macOS App/Extension build and experimental WinFsp dispatcher. NOT a validated system mount or production release.** The core stores immutable block segments with an authoritative SQLite catalog, namespace operations, shared hard-link identity, open-object lifetime, read leases and operation-triggered reclamation. It also has evidence-copy diagnosis/content export and separate maintenance status. Native adapters are incomplete; effective permissions, full metadata/security semantics, idle scheduling and a usable desktop client remain unimplemented. Never store the only copy in this experimental format.

Read [M1-A](docs/IMPLEMENTATION-M1A.zh-CN.md), [M1-B](docs/IMPLEMENTATION-M1B.zh-CN.md), [M1-C](docs/IMPLEMENTATION-M1C.zh-CN.md) and [validation evidence](docs/VALIDATION.zh-CN.md). Hosted core tests do not establish exFAT, mounted-volume or Codex compatibility. Exact source-version CI results are recorded on the corresponding PR; implementation documentation is not a test-pass claim.

FSKit and WinFsp remain candidates. Internal true hard links do not remove WinFsp's documented interface limitation. See the [backend ADR](docs/adr/0001-development-volume-backend-gate.md).


## M1-O explicit v3 directory lifetimes

A NEW opt-in v3 container retains granted directory inodes across rename, permission changes and last unlink until final close, with one shared 4096 file/directory pin budget. Current metadata authority remains separate from the old read grant. Recovery strictly reports empty orphan directories without inventing paths; normal reopen reclaims abandoned unpinned inodes. Default v1 and explicit v2 are unchanged and never migrated. The actual pinned old reader must refuse v3 without changing source hashes. Direct WinFsp and external-pin SDK contracts do not enable authenticated native Open or system mounts. See [M1-O](docs/IMPLEMENTATION-M1O.zh-CN.md) and [ADR 0015](docs/adr/0015-explicit-v3-directory-inode-lifetimes.md).

## M0-V FSKit directory item references

The v3 laboratory FSVolume now owns a typed inode reference for each cached non-root directory. Atomic directory creation reserves the reference before namespace mutation; lookup reuses it, removed/replaced directories retain their original identity, and reclaim releases it. Closing open modes and releasing a cached item are separate lifetimes. Ordinary directory creation stays unpinned; v1/v2 are unchanged. These are administrative adapter operations, not OS caller authentication, effective permissions or system mounting. See [M0-V](docs/IMPLEMENTATION-M0V.zh-CN.md) and [ADR 0016](docs/adr/0016-fskit-directory-item-references.md).

### Native adapter ABI (experimental)

The new `portaweave-native` library exports a versioned C header in [`include/portaweave_native.h`](include/portaweave_native.h), with actual Rust-core directory and file mutations, enumeration, file I/O, and lifecycle tokens. See [M0-E interface scope](docs/IMPLEMENTATION-M0E.zh-CN.md). **This is not an OS mount:** FSKit/WinFsp callbacks, security, notifications, safe unmount, and complete flush behavior are still missing. Do not treat a successful ABI test as Codex compatibility.

## M0-G WinFsp capacity / durability contract (experimental)

The existing native ABI now exposes logical quota and conservative host available-space snapshots plus an explicit SQLite WAL FULL checkpoint. **A catalog checkpoint is not an OS file flush.** The Windows WinFsp 2.1 callback contract compiles against pinned upstream headers and is exercised with a mock WinFsp object on GitHub Actions, without installing a driver or mounting a disk. The unimplemented access control and Flush callbacks fail closed. See [M0-G](docs/IMPLEMENTATION-M0G.zh-CN.md) and [native adapter readiness](docs/COMPATIBILITY.zh-CN.md).

## M0-H experimental WinFsp dispatcher lifecycle

`platform/windows/winfsp_service.c` implements calls to the real WinFsp filesystem create / mount point / dispatcher start / stop / delete APIs. CI **only** compiles it and runs `--self-test`; no driver or drive letter is installed or mounted. An explicit `--lab-mount R:` entrypoint for Windows uses **only a new disposable %TEMP% container** and requires WinFsp to be already installed; this remains **NOT_RUN**. It denies file opens and returns NOT_SUPPORTED on Flush, intentionally preventing use as a development disk. Read [M0-H experimental scope](docs/IMPLEMENTATION-M0H.zh-CN.md). Do not store user files in it.

## M0-J macOS App and ExtensionKit build

The repository contains a real Xcode host App and embedded FSKit Extension target, statically linked to the shared Rust core. `bash ci/build-macos-lab.sh` builds and inspects an unsigned lab bundle on macOS. It does not install, enable or mount anything; effective permissions, complete native cache/flush semantics, signing and actual mounting remain open. Later FSVolume callback work is described below. See [build scope](docs/IMPLEMENTATION-M0J.zh-CN.md).

## M1-E bounded range leases

`acquire_range_view` retains only block mappings inside a requested interval, with the original EOF and shared owner-wide budgets. Native C begin/read/close functions keep snapshots alive after source handles close; live leases block volume close. This is not OS mmap or mounted-volume compatibility. See [scope and tests](docs/IMPLEMENTATION-M1E.zh-CN.md).

## M1-F catalog schema gate

Normal open and original-preserving recovery now share an experimental schema check. Unexpected tables, columns, indexes, views and triggers are rejected before automatic reclamation; LF/CRLF source checkout differences are normalized. This is not a complete hostile SQLite parser or a read-only ordinary open. See [scope and regression evidence](docs/IMPLEMENTATION-M1F.zh-CN.md).

## FSKit FSVolume callbacks (laboratory)

A real FSVolume subclass now bridges namespace, handles, write/truncate, links, synchronization and resumable enumeration to the shared core on a per-volume serial queue. Direct SDK contract tests are distinct from an OS mount. Missing metadata/permissions are explicitly rejected; unary load, activation and mount remain closed. See [M0-K scope](docs/IMPLEMENTATION-M0K.zh-CN.md).

M0-L bridges v2 persistent UUID and stored owner/mode/timestamps into FSVolume identity, attribute reads and directory enumeration. M0-M adds single-transaction v2 size/metadata setters and explicit owner/mode creation in the laboratory contract. v1 rejects missing metadata; effective permission/access/load/mount gates remain closed. See [M0-L](docs/IMPLEMENTATION-M0L.zh-CN.md) and [M0-M](docs/IMPLEMENTATION-M0M.zh-CN.md) for exact scope and SDK tests.

## Backend storage synchronization

`pw_storage_synchronize` / `Volume::synchronize_storage` performs an explicit Unix host-file, catalog/WAL and container-directory barrier. It preserves live handles and leases; busy/error results are visible. Windows returns unsupported. Native cache drain, stronger device flush and safe eject remain separate gates. See [M1-G](docs/IMPLEMENTATION-M1G.zh-CN.md) and [ADR 0004](docs/adr/0004-backend-storage-barrier.md).

## M1-H explicit metadata format

`Volume::create_with_metadata` / `pw_volume_create_with_metadata` explicitly creates a NEW experimental v2 container with persistent UUID, uid/gid/mode and nanosecond timestamps. Metadata changes share content/namespace transactions, including links and still-open replaced objects. Default creation remains v1; existing containers are never automatically upgraded. These are trusted storage APIs, not effective OS permissions or caller authentication. See [scope, tests and limits](docs/IMPLEMENTATION-M1H.zh-CN.md) and [ADR 0005](docs/adr/0005-opt-in-metadata-format.md).

M1-I adds a single-transaction size/metadata mutation for native adapters, with explicit timestamp precedence and tested commit/maintenance failures. Existing views retain old content. See [M1-I](docs/IMPLEMENTATION-M1I.zh-CN.md); native permission enforcement remains separate work.

## M1-J namespace permission policy

`Volume::posix(Credentials)` and additive C calls enforce owner/group/other checks, ancestor search, sticky deletion/rename, caller ownership, umask and setgid-directory inheritance for selected namespace operations. Credentials are explicit trusted claims; this does not authenticate native callers, govern raw data handles or implement Windows ACLs. Default v1 remains unchanged and native access/load/mount gates stay closed. See [M1-J scope/tests](docs/IMPLEMENTATION-M1J.zh-CN.md) and [ADR 0006](docs/adr/0006-trusted-posix-policy.md).

## M1-K granted files and metadata authority

Opaque policy files retain their read/write grants across chmod/chown/rename/unlink until close. Current owner/group authority is checked separately for metadata changes; mixed size/attributes use one transaction. Policy and administrative C tokens are distinct and share admission/BUSY limits. These trusted-credential tests do not authenticate OS callers or enable mounts. See [M1-K](docs/IMPLEMENTATION-M1K.zh-CN.md) and [ADR 0007](docs/adr/0007-granted-files-and-metadata-policy.md).

## M1-L read-authorized range leases

Granted READ files can admit immutable range leases; WRITE-only files cannot. Leases keep original bounds/EOF/content after source close and mutation, share core admission budgets, and keep volume close BUSY until released. Policy/admin C view tokens stay distinct. This is process-local read capability work, not OS mmap or caller authentication. See [M1-L](docs/IMPLEMENTATION-M1L.zh-CN.md).

All core regular-file opens, including raw administrative Rust opens, now share the 4096-handle admission gate with creation pins and policy grants. The limit is a logical resource cap, not an exact heap budget.

## M0-Q initial creation state

Files can start with a quota-checked sparse logical size; v2 creation can include exact birth/access/modify times. Initial state, name, metadata and the pinned reply share one creation transaction (generation 0), with no follow-up truncate. v1 accepts size while retaining explicit metadata absence; directory size requests reject. No physical reservation or native authorization is claimed. See [M0-Q](docs/IMPLEMENTATION-M0Q.zh-CN.md).

## M0-P atomic FSKit creation

Laboratory create reserves native handle admission/tokens and prepares its Swift item before namespace mutation. The business transaction returns a projected reply plus an already held administrative file pin; success no longer calls a fallible intern/open. Failed admission leaves no name or consumed fields. Mount/identity gates remain closed. See [M0-P](docs/IMPLEMENTATION-M0P.zh-CN.md) and [ADR 0011](docs/adr/0011-created-node-pins-and-replies.md).

## M0-O FSKit committed attribute replies

Laboratory setattr now consumes a pre-commit projected reply for size, mixed and metadata-only changes. It no longer queries the catalog after known success; failed projections roll back before commit, and later maintenance warnings remain separate. v1 size replies explicitly lack metadata. These are administrative callbacks with native identity/load/mount still closed. See [M0-O](docs/IMPLEMENTATION-M0O.zh-CN.md).

## M0-T bounded directory enumeration

WinFsp ReadDirectory consumes a trusted, immutable metadata snapshot and emits pinned SDK directory records with UTF-16 names, ordinal cookies, complete pages and EOF. Windows/admin snapshots share cursor/entry admission; FSKit can reclaim only its own old LRU snapshots under pressure. Native directory Open, kernel refresh, OS identity/ACL and real mounts remain unfinished. See [M0-T](docs/IMPLEMENTATION-M0T.zh-CN.md) and [ADR 0013](docs/adr/0013-directory-snapshot-wire-and-admission.md).

M0-U adds fresh linked-directory attributes after new trusted READ+SEARCH checks, used by the direct WinFsp GetFileInfo contract. Explicit snapshot replacement credits only its own old slot/entries, works at full admission, preserves the old token/outputs on failure and returns a fresh token on success. Removed directories return NotFound; no inode is pinned and no old pathname is reopened. NULL Marker still replays the same snapshot, not automatic kernel refresh. Full open-after-unlink directory lifetime requires a separate format/recovery decision; native security/Open/Flush stay closed. See [M0-U](docs/IMPLEMENTATION-M0U.zh-CN.md) and [ADR 0014](docs/adr/0014-current-directory-info-and-atomic-replacement.md).

## M0-S checked namespace identities

FSKit removal/rename now checks cached source and destination identities within the mutation request and SQL transaction. Stale names return ESTALE before changes, preserving unexpected replacement files. Case-only renames and distinct hard-link aliases retain their separate rules. These are trusted administrative guards, not OS permissions or mount evidence. See [M0-S](docs/IMPLEMENTATION-M0S.zh-CN.md) and [ADR 0012](docs/adr/0012-checked-namespace-identities.md).

## M0-R WinFsp basic-time callback contract

The real SetBasicInfo callback now uses current trusted metadata authority and a pre-commit reply to set creation/access/write times atomically. Unsupported DOS flags, explicit change time and time-control sentinels reject before mutation; missing test credentials deny. Read-only data rights and current metadata authority are separate. No Windows SID/ACL/WRITE_ATTRIBUTES or mount claim is made. See [M0-R](docs/IMPLEMENTATION-M0R.zh-CN.md), [M1-N](docs/IMPLEMENTATION-M1N.zh-CN.md) and [ADR 0009](docs/adr/0009-fallible-windows-reply-projection.md).

## M0-N WinFsp granted-file callback contract

The Windows adapter now implements regular-file information, append/constrained writes and EOF changes using policy grants, plus EOF-aware reads and explicit close status. A fallible Windows time/size projection runs before commit. Open/security/Flush remain closed; allocation requests are unsupported. Windows CI exercises actual pinned callback signatures without installing a driver or mounting. See [M0-N](docs/IMPLEMENTATION-M0N.zh-CN.md) and [ADR 0009](docs/adr/0009-fallible-windows-reply-projection.md).

## M1-M atomic write requests

Granted files resolve append/offset and EOF-constrained writes inside one owner request. Write/truncate replies capture identity, size, generation and metadata before commit, preserving known business success across later maintenance errors. Native ABI tests include real concurrent append callers. This does not enable WinFsp access or certify native caches/flush/mounts. See [M1-M](docs/IMPLEMENTATION-M1M.zh-CN.md) and [ADR 0008](docs/adr/0008-transaction-captured-io-replies.md).

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
