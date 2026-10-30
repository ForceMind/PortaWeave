#!/usr/bin/env python3
"""Offline filesystem smoke probes, not a filesystem or compatibility certification."""
from __future__ import annotations

import argparse
from collections import Counter
from datetime import datetime, timezone
import hashlib
import json
import mmap
import os
from pathlib import Path
import platform
import shutil
import sqlite3
import subprocess
import sys
import tempfile
import time
from typing import Callable


class NotRun(Exception):
    """A prerequisite is missing; never count as PASS."""


class NotApplicable(Exception):
    """A test is specific to another OS, not an unsupported feature waiver."""


def require(condition: bool, message: str) -> None:
    if not condition:
        raise RuntimeError(message)


def command(args: list[str], cwd: Path, env: dict[str, str] | None = None) -> str:
    result = subprocess.run(args, cwd=cwd, env=env, capture_output=True,
                            text=True, encoding="utf-8", errors="replace", timeout=60)
    if result.returncode:
        raise RuntimeError(f"{Path(args[0]).name} exited {result.returncode}: "
                           f"{result.stdout[-1200:]} {result.stderr[-1200:]}")
    return result.stdout


def tool(name: str) -> str:
    found = shutil.which(name)
    if not found:
        raise NotRun(f"Required tool unavailable: {name}")
    return found


def io_roundtrip(p: Path) -> str:
    data = bytes(range(256)) * 1024
    f = p / "data.bin"
    with f.open("w+b") as h:
        h.write(data)
        h.seek(4093)
        h.write(b"changed")
        h.flush()
        os.fsync(h.fileno())
    expected = data[:4093] + b"changed" + data[4100:]
    require(hashlib.sha256(f.read_bytes()).digest() == hashlib.sha256(expected).digest(),
            "Random overwrite changed unexpected bytes")
    with f.open("r+b") as h:
        h.truncate(17)
        h.truncate(8192)
    require(f.read_bytes() == expected[:17] + bytes(8192 - 17), "Truncate/zero-fill mismatch")
    return "Random overwrite, fsync API, shrink and extension zero-fill; not power-loss proof"


def mapped_io(p: Path) -> str:
    f = p / "mapped.bin"
    f.write_bytes(bytes(8192))
    with f.open("r+b") as h:
        with mmap.mmap(h.fileno(), 0) as view:
            view[100:109] = b"mapped-ok"
            view.flush()
        os.fsync(h.fileno())
    require(f.read_bytes()[100:109] == b"mapped-ok", "Mapped bytes not visible after reopen")
    return "Writable mmap, flush API and reopen visibility; executable mapping is a separate gate"


def atomic_replace(p: Path) -> str:
    old, new = p / "document", p / "document.next"
    old.write_bytes(b"old")
    with new.open("wb") as h:
        h.write(b"new")
        h.flush()
        os.fsync(h.fileno())
    # Python's ordinary Windows file open need not grant FILE_SHARE_DELETE.
    # Native Windows open/share/delete tests are tracked separately, not emulated here.
    if os.name == "posix":
        with old.open("rb") as held:
            os.replace(new, old)
            require(held.read() == b"old", "Open handle did not retain old object")
    else:
        os.replace(new, old)
    require(old.read_bytes() == b"new" and not new.exists(), "Replacement mismatch")
    return "Same-directory replacement; POSIX open-object lifetime when applicable"


def hardlink(p: Path) -> str:
    a, b = p / "original", p / "alias"
    a.write_bytes(b"before")
    os.link(a, b)
    require(os.path.samefile(a, b), "Link does not share file identity")
    require(a.stat().st_nlink >= 2, "Incorrect link count")
    b.write_bytes(b"after")
    require(a.read_bytes() == b"after", "Link was implemented as an independent copy")
    a.unlink()
    require(b.read_bytes() == b"after" and b.stat().st_nlink == 1, "Unlink lifetime mismatch")
    return "Shared identity, content, link count and unlink lifetime"


def symlink(p: Path) -> str:
    a, b = p / "target", p / "relative-link"
    a.write_bytes(b"target")
    os.symlink(a.name, b)
    require(b.is_symlink() and os.readlink(b) == a.name, "Not a relative symbolic link")
    require(b.read_bytes() == b"target", "Symlink resolution mismatch")
    a.unlink()
    require(b.is_symlink() and not b.exists(), "Dangling symlink lost its identity")
    return "Relative target resolution and dangling-link preservation"


def names(p: Path) -> str:
    (p / "计划-😀.txt").write_text("内容", encoding="utf-8")
    require((p / "计划-😀.txt").read_text(encoding="utf-8") == "内容", "Unicode roundtrip")
    a, b = p / "Case.txt", p / "case.txt"
    a.write_bytes(b"case")
    os.rename(a, b)
    require("case.txt" in [c.name for c in p.iterdir()], "Case-only rename not reflected")
    return "Unicode roundtrip and case-only rename; normalization collisions need extra tests"


def exclusive_create(p: Path) -> str:
    (p / "held.lock").write_bytes(b"owner")
    code = """from pathlib import Path
try:
    Path('held.lock').open('xb').close()
except FileExistsError:
    print('EXCLUSIVE_OK')
else:
    raise SystemExit('Exclusive create unexpectedly succeeded')
"""
    require("EXCLUSIVE_OK" in command([sys.executable, "-c", code], p), "Exclusive create failed")
    require((p / "held.lock").read_bytes() == b"owner", "Lock contents changed")
    return "Second process cannot exclusively create an existing file"


def sqlite_wal(p: Path) -> str:
    db = p / "application.sqlite"
    connection = sqlite3.connect(db, timeout=0.2)
    try:
        require(connection.execute("PRAGMA journal_mode=WAL").fetchone()[0] == "wal", "No WAL")
        connection.execute("PRAGMA synchronous=FULL")
        connection.execute("CREATE TABLE sample(value INTEGER)")
        connection.commit()
        connection.execute("BEGIN IMMEDIATE")
        child = """import sqlite3
c=sqlite3.connect('application.sqlite', timeout=0.1)
try:
    c.execute('INSERT INTO sample VALUES (7)')
    c.commit()
except sqlite3.OperationalError as e:
    if 'locked' not in str(e).lower(): raise
    print('BUSY_OK')
else:
    raise SystemExit('Concurrent writer bypassed database lock')
finally:
    c.close()
"""
        require("BUSY_OK" in command([sys.executable, "-c", child], p), "Writer lock not enforced")
        connection.rollback()
        command([sys.executable, "-c", "import sqlite3; c=sqlite3.connect('application.sqlite'); "
                 "c.execute('INSERT INTO sample VALUES (7)'); c.commit(); c.close()"], p)
        require(connection.execute("SELECT value FROM sample").fetchall() == [(7,)], "Commit visibility")
        require(connection.execute("PRAGMA integrity_check").fetchone()[0] == "ok", "Integrity check")
        require(connection.execute("PRAGMA wal_checkpoint(TRUNCATE)").fetchone()[0] == 0, "Checkpoint busy")
    finally:
        connection.close()
    return f"Application WAL and cross-process writer locking; Python SQLite {sqlite3.sqlite_version}"


def git_worktree(p: Path) -> str:
    executable = tool("git")
    hooks = p / "empty-hooks"
    hooks.mkdir()
    env = {k: v for k, v in os.environ.items() if not k.startswith("GIT_")}
    env.update(GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull,
               GIT_TERMINAL_PROMPT="0")
    base = [executable, "-c", "user.name=PortaWeave Probe", "-c", "user.email=probe@example.invalid",
            "-c", "commit.gpgsign=false", "-c", "core.autocrlf=false",
            "-c", f"core.hooksPath={hooks}", "-c", f"init.templateDir={hooks}"]
    repo = p / "repo"
    repo.mkdir()
    def git(*args: str) -> str:
        return command(base + list(args), repo, env)
    git("init", "-b", "main")
    (repo / "source.txt").write_bytes(b"initial\n")
    git("add", "source.txt")
    git("commit", "-m", "fixture")
    linked = p / "linked"
    git("worktree", "add", "-b", "probe", str(linked))
    (linked / "source.txt").write_bytes(b"changed\n")
    git("-C", str(linked), "add", "source.txt")
    git("-C", str(linked), "commit", "-m", "worktree fixture")
    require((repo / "source.txt").read_bytes() == b"initial\n", "Worktree isolation failed")
    git("worktree", "remove", str(linked))
    git("gc", "--prune=now")
    git("fsck", "--full", "--strict")
    require(not git("status", "--porcelain").strip(), "Fixture repository not clean")
    return "Local Git commit/worktree/gc/fsck without network, user config or hooks"


def node_watch(p: Path) -> str:
    node = tool("node")
    script = p / "fixture.cjs"
    script.write_text("""const fs = require('node:fs');
fs.writeFileSync('source.cjs', 'module.exports = 2;');
let eventSeen = false;
const watcher = fs.watch('.', (event, filename) => {
  if (filename == null || String(filename) === 'source.cjs') eventSeen = true;
});
watcher.on('error', e => { console.error(e); process.exit(1); });
setTimeout(() => {
  fs.writeFileSync('source.next', 'module.exports = 3;');
  fs.renameSync('source.next', 'source.cjs');
}, 100);
setTimeout(() => {
  watcher.close();
  const value = require('./source.cjs');
  fs.mkdirSync('dist');
  fs.writeFileSync('dist/output.cjs', 'module.exports = ' + (value * 2) + ';');
  if (!eventSeen || require('./dist/output.cjs') !== 6) process.exit(1);
  console.log('NODE_WATCH_OK');
}, 1500);
""", encoding="utf-8")
    require("NODE_WATCH_OK" in command([node, str(script)], p), "Node fixture failed")
    return "Node module execution and directory event after replacement; not npm/pnpm certification"


def python_venv(p: Path) -> str:
    envdir = p / ".venv"
    command([sys.executable, "-m", "venv", "--without-pip", "--copies", str(envdir)], p)
    interpreter = envdir / ("Scripts/python.exe" if os.name == "nt" else "bin/python")
    (p / "module_fixture.py").write_text("value = 19\n", encoding="utf-8")
    output = command([str(interpreter), "-c", "import module_fixture; print(module_fixture.value)"], p)
    require(output.strip() == "19", "Venv interpreter/import failed")
    return "Copied venv interpreter executed inside test tree; no package download"


def executable(p: Path) -> str:
    if os.name != "posix":
        raise NotApplicable("POSIX executable-bit/shebang test; Windows executable checks remain separate")
    script = p / "run.sh"
    script.write_bytes(b"#!/bin/sh\nprintf 'EXEC_OK\\n'\n")
    script.chmod(0o700)
    require(script.stat().st_mode & 0o100 != 0, "Executable bit not stored")
    require(command([str(script)], p).strip() == "EXEC_OK", "Direct execution failed")
    return "Direct executable-bit/shebang execution, not native compiler/dynamic-loader coverage"


def windows_native(p: Path, operation: str) -> str:
    if os.name != "nt":
        raise NotApplicable("Actual Win32 API probe; Windows runner required")
    if __package__:
        from . import windows_semantics
    else:
        import windows_semantics
    return getattr(windows_semantics, operation)(p)


CASES: dict[str, Callable[[Path], str]] = {
    "io": io_roundtrip, "mmap": mapped_io, "replace": atomic_replace,
    "hardlink": hardlink, "symlink": symlink, "names": names,
    "exclusive": exclusive_create, "sqlite": sqlite_wal, "git": git_worktree,
    "node": node_watch, "venv": python_venv, "exec": executable,
    "windows-share": lambda p: windows_native(p, "sharing"),
    "windows-lock": lambda p: windows_native(p, "locking"),
    "windows-delete": lambda p: windows_native(p, "delete_pending"),
    "windows-replace": lambda p: windows_native(p, "replace_open"),
}


def probe(root: Path, selected: list[str], label: str) -> dict:
    root = root.expanduser().resolve(strict=True)
    if not root.is_dir() or any(part.lower().endswith(".pwvol") for part in root.parts):
        raise ValueError("Use a disposable directory in the mounted volume, never the backing .pwvol")
    # Containers may be renamed without the conventional suffix. Inspect only
    # the backing layout's presence, never open its catalog or recover it.
    if any((parent / "format.json").exists() and (parent / "catalog.sqlite").exists()
           for parent in (root, *root.parents)):
        raise ValueError("Refusing a raw container layout or its descendants")
    if not selected or len(set(selected)) != len(selected) or any(n not in CASES for n in selected):
        raise ValueError("Select nonempty, distinct, known case names")
    results = []
    with tempfile.TemporaryDirectory(prefix="portaweave-probe-", dir=root) as temp:
        for name in selected:
            path = Path(temp) / name
            path.mkdir()
            start = time.monotonic()
            status = "PASS"
            try:
                detail = CASES[name](path)
            except NotRun as error:
                status, detail = "NOT_RUN", str(error)
            except NotApplicable as error:
                status, detail = "NOT_APPLICABLE", str(error)
            except Exception as error:
                status, detail = "FAIL", f"{type(error).__name__}: {error}"
            results.append({"case": name, "status": status, "detail": detail,
                            "duration_seconds": round(time.monotonic() - start, 3)})
    counts = dict(Counter(row["status"] for row in results))
    return {
        "schema_version": 1, "timestamp_utc": datetime.now(timezone.utc).isoformat(),
        "scope": "filesystem-api-smoke-probe", "label_unverified": label,
        "root": str(root), "platform": platform.platform(), "python": platform.python_version(),
        "sqlite": sqlite3.sqlite_version, "mount_identity_verified": False,
        "product_compatibility_certified": False, "selected_cases": selected,
        "not_selected": [name for name in CASES if name not in selected],
        "counts": counts, "results": results,
        "applicable_selected_cases_passed": not (counts.get("FAIL") or counts.get("NOT_RUN")),
        "not_covered": ["real PortaWeave mount identification", "Codex integration/sandbox enforcement",
                        "Windows ACLs and mounted-volume share/lock semantics", "native compilation/dynamic loading", "package installation",
                        "GC", "power loss", "real exFAT", "macOS/Windows cross-system handover"],
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", required=True, type=Path)
    parser.add_argument("--label", default="unverified-target")
    parser.add_argument("--report", type=Path)
    parser.add_argument("--cases", default=",".join(CASES))
    options = parser.parse_args(argv)
    try:
        report = probe(options.root, options.cases.split(","), options.label)
        text = json.dumps(report, ensure_ascii=False, indent=2) + "\n"
        if options.report:
            # Exclusive create: never overwrite a user's existing report or other file.
            with options.report.open("x", encoding="utf-8") as handle:
                handle.write(text)
        print(text)
        if report["counts"].get("FAIL"):
            return 1
        return 2 if report["counts"].get("NOT_RUN") else 0
    except (OSError, ValueError) as error:
        print(f"Probe setup/report error: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
