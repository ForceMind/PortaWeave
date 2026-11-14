"""Internal bounded-protocol child for private Windows host/mount probes.

No file creation, driver access, inherited file handle or account change.
Only workspace_probe's two private fixture files are accepted. Parent-side
deadlines terminate/reap this process on failed I/O or protocol progress.
"""
from contextlib import ExitStack
import json
import os
from pathlib import Path
import sys

# Parent uses isolated Python; select the reviewed implementation by its fixed
# sibling directory, not cwd/PYTHONPATH. That checkout may be on the test volume.
sys.path.insert(0, str(Path(__file__).resolve().parent))
from windows_semantics import (Api, CHILD_MESSAGE_LIMIT, CHILD_PROTOCOL, READ, WRITE,
                               SHARE_ALL, SHARE_READ, SHARING_VIOLATION, LOCK_VIOLATION,
                               C, W, _decode_message, require)


def fixture(mode: str, root: Path) -> Path:
    require(mode in ("sharing", "locking"), "Unknown child fixture")
    root = root.resolve(strict=True)
    case, name = (("windows-share", "sharing.bin") if mode == "sharing" else
                  ("windows-lock", "locks.bin"))
    require(root.is_dir() and root.name == case and
            root.parent.name.startswith("portaweave-probe-"), "Not a private probe directory")
    require(not any(part.lower().endswith(".pwvol") for part in root.parts),
            "Refusing a backing container")
    require(not any((parent / "catalog.sqlite").exists() and
                    any((parent / marker).exists() for marker in ("format.txt", "format.json"))
                    for parent in (root, *root.parents)), "Refusing a raw backing layout")
    path = root / name
    require(not path.is_symlink() and path.is_file(), "Missing regular private fixture")
    return path


class Operations:
    def __init__(self, mode: str, file: Path):
        self.mode, self.file, self.api = mode, file, Api()
        self.held = None

    def close(self):
        if self.held is not None:
            held, self.held = self.held, None
            held.close()

    def identity(self):
        with self.api.opened(self.file, READ) as handle:
            return {"identity": list(self.api.identity(handle))}

    def run(self, operation: str) -> dict:
        api, file = self.api, self.file
        if operation == "release":
            require(self.held is not None, "No held child resource")
            self.close()
            return {}
        require(self.held is None, "Release the child resource before another operation")
        if operation == "quit":
            return {}
        if operation == "hold":
            held = ExitStack()
            try:
                handle = held.enter_context(api.opened(file, READ if self.mode == "sharing" else READ | WRITE,
                                                       SHARE_READ if self.mode == "sharing" else SHARE_ALL))
                if self.mode == "locking":
                    held.enter_context(api.locked(handle))
                result = {"identity": list(api.identity(handle))}
            except BaseException:
                held.close()
                raise
            self.held = held
            return result
        if self.mode == "sharing":
            return self.sharing(operation)
        return self.locking(operation)

    def sharing(self, operation: str) -> dict:
        api, file = self.api, self.file
        require(operation in ("read", "sharing-denied", "denied-mask", "write"),
                "Unknown sharing operation")
        if operation == "sharing-denied":
            api.refused_open(file, WRITE, SHARE_ALL, SHARING_VIOLATION)
            api.expect_failure(api.dll.DeleteFileW(str(file)), SHARING_VIOLATION,
                               "cross-process DeleteFileW without share-delete")
        elif operation == "denied-mask":
            api.refused_open(file, READ, SHARE_READ, SHARING_VIOLATION)
        with api.opened(file, READ | WRITE if operation == "write" else READ) as handle:
            require(api.read(handle) == b"original", "Sharing child observed changed bytes")
            if operation == "write":
                api.write(handle, b"original")
            return {"identity": list(api.identity(handle))}

    def locking(self, operation: str) -> dict:
        api, file = self.api, self.file
        require(operation in ("exclusive-denied", "shared", "exclusive"),
                "Unknown locking operation")
        with api.opened(file) as handle:
            result = {"identity": list(api.identity(handle))}
            if operation == "exclusive-denied":
                api.refused_lock(handle)
                api.refused_lock(handle, exclusive=False)
                api.refused_io(handle)
                with api.locked(handle, offset=4):
                    pass
            elif operation == "shared":
                with api.locked(handle, exclusive=False):
                    require(api.read(handle) == b"original", "Shared child read failed")
                    api.seek(handle)
                    done = W.DWORD()
                    api.expect_failure(api.dll.WriteFile(handle, C.create_string_buffer(b"BAD!"), 4,
                                                         C.byref(done), None), LOCK_VIOLATION,
                                       "cross-process shared-lock WriteFile")
                    with api.opened(file) as other:
                        api.refused_lock(other)
            else:
                with api.locked(handle):
                    require(api.read(handle) == b"original", "Unlocked child observed changed bytes")
                    api.write(handle, b"original")
            return result


def emit(sequence: int, operation: str, *, result=None, error=None):
    message = {"protocol": CHILD_PROTOCOL, "sequence": sequence,
               "pid": os.getpid(), "operation": operation}
    message.update({"error": str(error)[:800]} if error is not None else {"result": result})
    data = (json.dumps(message, ensure_ascii=True) + "\n").encode("utf-8")
    require(len(data) <= CHILD_MESSAGE_LIMIT, "Child reply exceeded limit")
    sys.stdout.buffer.write(data)
    sys.stdout.buffer.flush()


def main(argv: list[str]) -> int:
    sequence, operation, operations = 0, "ready", None
    try:
        require(os.name == "nt", "Windows child requires actual Win32")
        require(len(argv) == 2, "Expected fixed mode and private fixture root")
        operations = Operations(argv[0], fixture(argv[0], Path(argv[1])))
        emit(0, "ready", result=operations.identity())
        for expected in range(1, 17):
            message = _decode_message(sys.stdin.buffer.readline(CHILD_MESSAGE_LIMIT + 1))
            require(set(message) == {"protocol", "sequence", "operation"} and
                    message["protocol"] == CHILD_PROTOCOL and
                    type(message["sequence"]) is int and message["sequence"] == expected and
                    type(message["operation"]) is str, "Invalid child request")
            sequence, operation = expected, message["operation"]
            result = operations.run(operation)
            emit(sequence, operation, result=result)
            if operation == "quit":
                return 0
        raise RuntimeError("Child request budget exceeded")
    except Exception as error:
        emit(sequence, operation, error=f"{type(error).__name__}: {error}")
        return 1
    finally:
        if operations is not None:
            operations.close()


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
