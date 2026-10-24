# PortaWeave

[中文](README.zh-CN.md)

> One portable working volume. Two platforms. Automatic space reclamation.

PortaWeave targets an exFAT-backed, dynamically growing directory container mounted as a **real read/write development volume** on macOS and Windows. Work in it with ordinary applications, local Codex, Git, editors, package managers and build tools. No application-specific import/export API and no hidden copy of the working tree on the system disk.

## Status — 2026-10-24

**Specification + initial compatibility probes. Not a working filesystem or a production release.** The shared Rust engine, platform mounts, automatic GC and desktop application are not implemented. Probe results on an ordinary host directory are not PortaWeave compatibility evidence.

The intended core is Rust + authoritative SQLite metadata + append-only data segments. FSKit and WinFsp are **candidates**, not validated solutions. WinFsp's documented lack of hard links is an explicit backend-selection blocker; do not redefine hard links as independent copies. See the [backend ADR](docs/adr/0001-development-volume-backend-gate.md).

Default automatic reclamation must respect trash, open handles, mappings and committed references. One container has one owner; computers hand it over after safe unmount. Host exFAT damage and failed hardware persistence remain risks. Clients and platform components require installation.

## Engineering documents

- [Product and storage specification](docs/SPEC.zh-CN.md)
- [Development-volume compatibility contract](docs/COMPATIBILITY.zh-CN.md)
- [Milestones and release gates](docs/ROADMAP.zh-CN.md)
- [Validation and evidence](docs/VALIDATION.zh-CN.md)
- [Primary technical sources](docs/SOURCES.md)
- [Contributor/agent rules](AGENTS.md) and [work handoff](WORK_HANDOFF.zh-CN.md)

## Initial probes

Python 3.10+; Git and Node are needed for their corresponding tests. Run only against a disposable test directory:

```sh
python tools/workspace_probe.py --root /path/to/scratch --label host-baseline --report /path/to/report.json
python -m unittest discover -s tests -v
```

The runner creates its own temporary child, does not format a disk, installs no drivers, uses no network and does not invoke Codex or change its permissions. Unsupported or unexecuted applicable tests are not passes. Full real-platform and Codex validation remains mandatory.

Top-level licensing has not yet been selected by the owner. Third-party licenses remain independent. The name does not imply trademark clearance.
