"""Actual synchronous Win32 file API probes on caller-owned new scratch files.

Only workspace_probe supplies the private per-case directory. Child probes
are joined before returning. No driver, mount, ACL change, impersonation or
raw device is involved; both processes still use the same account.
"""
from __future__ import annotations

from contextlib import contextmanager
import ctypes as C
from ctypes import wintypes as W
import json
import os
from pathlib import Path
import subprocess
import sys
import threading
import time

READ, WRITE = 0x80000000, 0x40000000
DELETE = 0x00010000
SHARE_READ, SHARE_ALL = 1, 7
SHARING_VIOLATION, LOCK_VIOLATION = 32, 33
FILE_SUPPORTS_HARD_LINKS = 0x00400000
CHILD_PROTOCOL = "portaweave-win32-probe-v1"
CHILD_MESSAGE_LIMIT = 4096
CHILD_OPERATIONS = frozenset({"read", "sharing-denied", "denied-mask", "write",
                              "exclusive-denied", "shared", "exclusive",
                              "hold", "release", "quit"})


class Overlapped(C.Structure):
    _fields_ = [("Internal", C.c_size_t), ("InternalHigh", C.c_size_t),
                ("Offset", W.DWORD), ("OffsetHigh", W.DWORD), ("hEvent", W.HANDLE)]


class FileIdentity(C.Structure):
    _fields_ = [("volume", C.c_ulonglong), ("identifier", C.c_ubyte * 16)]


class FileStandardInformation(C.Structure):
    _fields_ = [("AllocationSize", C.c_longlong), ("EndOfFile", C.c_longlong),
                ("NumberOfLinks", W.DWORD), ("DeletePending", C.c_ubyte), ("Directory", C.c_ubyte)]


class RenameInformation(C.Structure):
    # FileRenameInfoEx uses the DWORD Flags member of FILE_RENAME_INFO's
    # leading union. RootDirectory alignment differs between x86 and x64.
    _fields_ = [("Flags", W.DWORD), ("RootDirectory", W.HANDLE),
                ("FileNameLength", W.DWORD), ("FileName", W.WCHAR * 1)]


def require(condition: bool, message: str) -> None:
    if not condition:
        raise RuntimeError(message)


def _unique_object(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, f"Duplicate child protocol key: {key}")
        result[key] = value
    return result


def _decode_message(line: bytes) -> dict:
    require(0 < len(line) <= CHILD_MESSAGE_LIMIT and line.endswith(b"\n"),
            "Child protocol ended early or exceeded its line limit")
    try:
        value = json.loads(line.decode("utf-8"), object_pairs_hook=_unique_object)
    except (UnicodeDecodeError, ValueError) as error:
        raise RuntimeError("Invalid child protocol JSON") from error
    require(type(value) is dict, "Child protocol message is not an object")
    return value


class _ProbeChild:
    """Bounded synchronous request/reply IPC; file handles are never inherited.

    A short-lived I/O thread makes pipe reads AND writes interruptible by the
    parent deadline on Windows. On failure we terminate only our own child,
    reap it, join that thread, then close pipes. No detached worker survives a
    successful return. A generic command argument permits real failure-path
    tests on hosts without Win32; production supplies one fixed local script.
    """

    def __init__(self, command: list[str], *, message_timeout=10.0, session_timeout=30.0):
        require(0 < message_timeout <= 30 and 0 < session_timeout <= 60,
                "Invalid child probe deadline")
        self.command = command
        self.message_timeout, self.session_timeout = message_timeout, session_timeout
        self.process = None
        self.thread = None
        self.sequence = 0
        self.ready = None
        self.closed = False

    def __enter__(self):
        require(self.process is None and not self.closed, "Child probe cannot be reused")
        self.deadline = time.monotonic() + self.session_timeout
        self.process = subprocess.Popen(self.command, stdin=subprocess.PIPE,
                                        stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                                        close_fds=True)
        try:
            self.ready = self._reply(None, "ready")
        except BaseException:
            self._cleanup()
            raise
        return self

    def _remaining(self):
        remaining = min(self.message_timeout, self.deadline - time.monotonic())
        require(remaining > 0, "Child probe session timed out")
        return remaining

    def _transfer(self, request: bytes | None) -> bytes:
        require(not self.closed and self.process is not None, "Child probe is closed")
        require(self.thread is None or not self.thread.is_alive(), "Overlapping child requests")
        result = []

        def exchange():
            try:
                if request is not None:
                    self.process.stdin.write(request)
                    self.process.stdin.flush()
                result.append(self.process.stdout.readline(CHILD_MESSAGE_LIMIT + 1))
            except BaseException as error:
                result.append(error)

        self.thread = threading.Thread(target=exchange, daemon=True,
                                       name=f"PortaWeaveProbeChild-{self.process.pid}")
        self.thread.start()
        self.thread.join(self._remaining())
        require(not self.thread.is_alive(), "Child probe reply timed out")
        require(bool(result), "Child probe reply missing")
        if isinstance(result[0], BaseException):
            raise RuntimeError(f"Child probe pipe failed: {result[0]}") from result[0]
        return result[0]

    def _reply(self, request: bytes | None, operation: str) -> dict:
        message = _decode_message(self._transfer(request))
        common = {"protocol", "sequence", "pid", "operation"}
        require(set(message) in (common | {"result"}, common | {"error"}),
                "Unexpected child reply fields")
        require(message["protocol"] == CHILD_PROTOCOL and
                type(message["sequence"]) is int and message["sequence"] == self.sequence and
                message["operation"] == operation, "Child reply phase mismatch")
        require(type(message["pid"]) is int and message["pid"] == self.process.pid and
                message["pid"] != os.getpid(), "Child reply process identity mismatch")
        if "error" in message:
            require(type(message["error"]) is str, "Invalid child error")
            raise RuntimeError(f"Child {operation} failed: {message['error']}")
        require(type(message["result"]) is dict, "Invalid child result")
        return message["result"]

    def request(self, operation: str) -> dict:
        require(operation in CHILD_OPERATIONS, "Unknown child operation")
        require(self.sequence < 16, "Child probe request budget exceeded")
        self.sequence += 1
        request = {"protocol": CHILD_PROTOCOL, "sequence": self.sequence, "operation": operation}
        return self._reply((json.dumps(request) + "\n").encode("utf-8"), operation)

    def _cleanup(self):
        if self.closed:
            return
        try:
            if self.process is not None:
                if self.process.poll() is None:
                    self.process.kill()
                self.process.wait(timeout=5)
            if self.thread is not None:
                self.thread.join(timeout=5)
                require(not self.thread.is_alive(), "Child probe pipe worker did not stop")
            if self.process is not None:
                try:
                    self.process.stdin.close()
                except BrokenPipeError:
                    pass  # a failed child may have rejected an unflushed request
                self.process.stdout.close()
        finally:
            self.closed = True

    def __exit__(self, kind, error, traceback):
        try:
            if kind is None:
                require(self.request("quit") == {}, "Unexpected child quit result")
                try:
                    code = self.process.wait(timeout=self._remaining())
                except subprocess.TimeoutExpired as failure:
                    raise RuntimeError("Child probe did not exit before its deadline") from failure
                require(code == 0, f"Child probe exited {code}")
                require(self._transfer(None) == b"", "Unexpected output after child quit")
        finally:
            self._cleanup()
        return False


def _identity_reply(result: dict, expected: tuple[int, str]):
    identity = result.get("identity")
    require(set(result) == {"identity"} and type(identity) is list and len(identity) == 2 and
            type(identity[0]) is int and type(identity[1]) is str and
            identity == list(expected), "Child independent file identity mismatch")


@contextmanager
def _windows_child(path: Path, mode: str, identity: tuple[int, str]):
    require(mode in ("sharing", "locking"), "Unknown Windows child mode")
    script = Path(__file__).resolve().with_name("windows_semantics_child.py")
    # Isolated Python selects the helper and implementation by fixed sibling
    # paths, not cwd/PYTHONPATH. The checkout itself may reside on the test volume.
    with _ProbeChild([sys.executable, "-I", "-u", str(script), mode, str(path.parent)]) as child:
        _identity_reply(child.ready, identity)
        yield child


class Api:
    def __init__(self):
        require(C.sizeof(W.DWORD) == 4 and C.sizeof(W.WCHAR) == 2 and
                C.sizeof(FileIdentity) == 24 and C.sizeof(FileStandardInformation) == 24 and
                FileStandardInformation.NumberOfLinks.offset == 16 and
                FileStandardInformation.Directory.offset == 21, "Unexpected Win32 wire types")
        require(C.sizeof(Overlapped) == (32 if C.sizeof(C.c_void_p) == 8 else 20) and
                RenameInformation.FileName.offset == (20 if C.sizeof(C.c_void_p) == 8 else 12),
                "Unexpected Win32 pointer/rename alignment")
        self.dll = C.WinDLL("kernel32", use_last_error=True)
        self.invalid = C.c_void_p(-1).value
        signatures = {
            "CreateFileW": (W.HANDLE, [W.LPCWSTR, W.DWORD, W.DWORD, C.c_void_p,
                                      W.DWORD, W.DWORD, W.HANDLE]),
            "CloseHandle": (W.BOOL, [W.HANDLE]),
            "ReadFile": (W.BOOL, [W.HANDLE, C.c_void_p, W.DWORD, C.POINTER(W.DWORD), C.c_void_p]),
            "WriteFile": (W.BOOL, [W.HANDLE, C.c_void_p, W.DWORD, C.POINTER(W.DWORD), C.c_void_p]),
            "SetFilePointerEx": (W.BOOL, [W.HANDLE, C.c_longlong, C.c_void_p, W.DWORD]),
            "LockFileEx": (W.BOOL, [W.HANDLE, W.DWORD, W.DWORD, W.DWORD, W.DWORD, C.POINTER(Overlapped)]),
            "UnlockFileEx": (W.BOOL, [W.HANDLE, W.DWORD, W.DWORD, W.DWORD, C.POINTER(Overlapped)]),
            "DeleteFileW": (W.BOOL, [W.LPCWSTR]),
            "CreateHardLinkW": (W.BOOL, [W.LPCWSTR, W.LPCWSTR, C.c_void_p]),
            "MoveFileExW": (W.BOOL, [W.LPCWSTR, W.LPCWSTR, W.DWORD]),
            "GetFileInformationByHandleEx": (W.BOOL, [W.HANDLE, C.c_int, C.c_void_p, W.DWORD]),
            "SetFileInformationByHandle": (W.BOOL, [W.HANDLE, C.c_int, C.c_void_p, W.DWORD]),
            "GetVolumeInformationByHandleW": (
                W.BOOL, [W.HANDLE, W.LPWSTR, W.DWORD, C.POINTER(W.DWORD), C.POINTER(W.DWORD),
                         C.POINTER(W.DWORD), W.LPWSTR, W.DWORD]),
        }
        for name, (result, arguments) in signatures.items():
            function = getattr(self.dll, name)
            function.restype, function.argtypes = result, arguments

    @staticmethod
    def checked(result):
        if not result:
            raise C.WinError(C.get_last_error())

    @staticmethod
    def expect_failure(result, code: int, operation: str):
        error = C.get_last_error()  # before any other Win32 call
        require(not result, f"{operation} unexpectedly succeeded")
        require(error == code, f"{operation}: expected Win32 {code}, got {error}")

    @contextmanager
    def opened(self, path: Path, access=READ | WRITE, share=SHARE_ALL):
        # OPEN_EXISTING only. File creation is exclusive via seed(). Handles
        # are synchronous and non-inheritable, not async OVERLAPPED I/O.
        handle = self.dll.CreateFileW(str(path), access, share, None, 3, 128, None)
        if handle == self.invalid:
            error = C.get_last_error()
            raise RuntimeError(f"CreateFileW({path.name}): Win32 {error}")
        try:
            yield handle
        finally:
            self.checked(self.dll.CloseHandle(handle))

    def refused_open(self, path: Path, access: int, share: int, code: int | tuple[int, ...]) -> int:
        handle = self.dll.CreateFileW(str(path), access, share, None, 3, 128, None)
        error = C.get_last_error()
        if handle != self.invalid:
            self.checked(self.dll.CloseHandle(handle))
            raise RuntimeError("CreateFileW unexpectedly admitted a forbidden open")
        allowed = (code,) if isinstance(code, int) else code
        require(error in allowed, f"CreateFileW: expected Win32 {code}, got {error}")
        return error

    def replace_posix(self, handle, target: Path):
        encoded = str(target).encode("utf-16-le")
        offset = RenameInformation.FileName.offset
        # Keep an explicit NUL for the Win32 path consumer; FileNameLength
        # counts only the UTF-16 bytes, not this additional terminator.
        data = C.create_string_buffer(max(C.sizeof(RenameInformation), offset + len(encoded) + 2))
        info = RenameInformation.from_buffer(data)
        info.Flags, info.FileNameLength = 3, len(encoded)  # REPLACE_IF_EXISTS | POSIX_SEMANTICS
        C.memmove(C.addressof(data) + offset, encoded, len(encoded))
        self.checked(self.dll.SetFileInformationByHandle(handle, 22, data, C.sizeof(data)))  # FileRenameInfoEx

    def seek(self, handle, offset=0):
        self.checked(self.dll.SetFilePointerEx(handle, offset, None, 0))

    def read(self, handle, length=8) -> bytes:
        self.seek(handle)
        data, done = C.create_string_buffer(length), W.DWORD()
        self.checked(self.dll.ReadFile(handle, data, length, C.byref(done), None))
        return data.raw[:done.value]

    def write(self, handle, data: bytes):
        self.seek(handle)
        done = W.DWORD()
        self.checked(self.dll.WriteFile(handle, C.create_string_buffer(data), len(data), C.byref(done), None))
        require(done.value == len(data), "Short Win32 fixture write")

    def refused_io(self, handle):
        self.seek(handle)
        data, done = C.create_string_buffer(4), W.DWORD()
        self.expect_failure(self.dll.ReadFile(handle, data, 4, C.byref(done), None),
                            LOCK_VIOLATION, "cross-process locked ReadFile")
        self.seek(handle)
        self.expect_failure(self.dll.WriteFile(handle, C.create_string_buffer(b"BAD!"), 4,
                                               C.byref(done), None),
                            LOCK_VIOLATION, "cross-process locked WriteFile")

    def identity(self, handle) -> tuple[int, str]:
        # FileIdInfo=18 returns a full 128-bit ID plus volume serial. A 64-bit
        # BY_HANDLE_FILE_INFORMATION ID is not guaranteed unique on ReFS.
        info = FileIdentity()
        self.checked(self.dll.GetFileInformationByHandleEx(handle, 18, C.byref(info), C.sizeof(info)))
        identifier = bytes(info.identifier)
        require(any(identifier), "File identity is zero")
        return info.volume, identifier.hex()

    def link_count(self, handle) -> int:
        info = FileStandardInformation()
        self.checked(self.dll.GetFileInformationByHandleEx(handle, 1, C.byref(info), C.sizeof(info)))  # FileStandardInfo
        return info.NumberOfLinks

    def filesystem(self, handle) -> tuple[str, int]:
        name, flags = C.create_unicode_buffer(256), W.DWORD()
        self.checked(self.dll.GetVolumeInformationByHandleW(handle, None, 0, None, None,
                                                           C.byref(flags), name, len(name)))
        return name.value, flags.value

    def create_hard_link(self, source: Path, alias: Path):
        self.checked(self.dll.CreateHardLinkW(str(alias), str(source), None))

    @contextmanager
    def locked(self, handle, offset=0, length=4, exclusive=True):
        overlap = Overlapped(Offset=offset)
        # FAIL_IMMEDIATELY is mandatory: a broken backend cannot hang this
        # harness waiting for a grant that this thread itself must release.
        flags = 1 | (2 if exclusive else 0)
        self.checked(self.dll.LockFileEx(handle, flags, 0, length, 0, C.byref(overlap)))
        try:
            yield
        finally:
            self.checked(self.dll.UnlockFileEx(handle, 0, length, 0, C.byref(overlap)))

    def refused_lock(self, handle, exclusive=True):
        overlap = Overlapped()
        flags = 1 | (2 if exclusive else 0)
        admitted = self.dll.LockFileEx(handle, flags, 0, 4, 0, C.byref(overlap))
        error = C.get_last_error()
        if admitted:
            self.checked(self.dll.UnlockFileEx(handle, 0, 4, 0, C.byref(overlap)))
            raise RuntimeError("LockFileEx admitted a conflicting lock")
        require(error == LOCK_VIOLATION, f"LockFileEx: expected Win32 33, got {error}")


def seed(path: Path, contents=b"original"):
    with path.open("xb") as file:  # never overwrite even in the private fixture
        file.write(contents)


def hardlink(path: Path) -> str:
    api = Api()
    source, directory = path / "original.bin", path / "links"
    directory.mkdir()
    alias = directory / "alias.bin"
    seed(source)
    with api.opened(source, READ) as original:
        identity = api.identity(original)
        require(api.link_count(original) == 1, "Fresh file did not have exactly one link")
        filesystem, flags = api.filesystem(original)
    advertises_links = bool(flags & FILE_SUPPORTS_HARD_LINKS)
    volume = f"filesystem={filesystem!r}, FILE_SUPPORTS_HARD_LINKS={int(advertises_links)}"
    try:
        api.create_hard_link(source, alias)
    except OSError as error:
        raise RuntimeError(f"CreateHardLinkW failed ({volume}): {error}") from error
    require(advertises_links, f"CreateHardLinkW succeeded without advertised support ({volume})")
    with api.opened(source, READ, SHARE_READ) as original:
        with api.opened(alias, READ) as linked:
            require(api.identity(original) == api.identity(linked) == identity,
                    "Hard link does not share the source file identity")
            require(api.link_count(original) == api.link_count(linked) == 2,
                    "Both names must report exactly two links")
        api.refused_open(alias, WRITE, SHARE_ALL, SHARING_VIOLATION)
    with api.opened(source, READ) as original, api.opened(alias, WRITE) as linked:
        updated, written = b"modified", W.DWORD()
        api.checked(api.dll.WriteFile(linked, C.create_string_buffer(updated), len(updated), C.byref(written), None))
        require(written.value == len(updated) and api.read(original) == updated,
                "Write through the alias was not visible through the original handle")
    api.checked(api.dll.DeleteFileW(str(source)))
    api.refused_open(source, READ, SHARE_ALL, 2)  # ERROR_FILE_NOT_FOUND
    with api.opened(alias, READ) as remaining:
        require(api.identity(remaining) == identity and api.link_count(remaining) == 1 and
                api.read(remaining) == updated, "Removing the source changed the surviving link")
    return f"WIN32_HARDLINK_OK: {volume}; cross-directory ID, exact link counts, shared writes and sharing, surviving alias"


def sharing(path: Path) -> str:
    api, file = Api(), path / "sharing.bin"
    seed(file)
    with api.opened(file, READ, SHARE_READ) as first:
        with api.opened(file, READ) as second:
            require(api.read(second) == b"original", "Compatible reader failed")
            require(api.identity(first) == api.identity(second), "Independent opens changed identity")
        api.refused_open(file, WRITE, SHARE_ALL, SHARING_VIOLATION)
        api.expect_failure(api.dll.DeleteFileW(str(file)), SHARING_VIOLATION, "DeleteFileW without share-delete")
    # The NEW sharing mask must also allow EXISTING access, even though the
    # first handle permits every subsequent access category.
    with api.opened(file):
        api.refused_open(file, READ, SHARE_READ, SHARING_VIOLATION)
    with api.opened(file, WRITE):
        pass  # closing the first owner must release the share restriction
    require(file.read_bytes() == b"original", "Sharing refusal changed data")
    with api.opened(file, READ) as observed:
        identity = api.identity(observed)
    with _windows_child(file, "sharing", identity) as child:
        child_pid = child.process.pid
        with api.opened(file, READ, SHARE_READ) as restricted_reader:
            require(api.identity(restricted_reader) == identity,
                    "Parent restrictive read handle changed file identity")
            _identity_reply(child.request("read"), identity)
            _identity_reply(child.request("sharing-denied"), identity)
        # The SAME child independently reopens after the parent releases its
        # restriction. Child success before this point would fail the probe.
        _identity_reply(child.request("write"), identity)
        with api.opened(file, WRITE) as existing_writer:
            require(api.identity(existing_writer) == identity,
                    "Parent existing write handle changed file identity")
            _identity_reply(child.request("denied-mask"), identity)
        _identity_reply(child.request("hold"), identity)
        try:
            api.refused_open(file, WRITE, SHARE_ALL, SHARING_VIOLATION)
            with api.opened(file, READ) as reader:
                require(api.identity(reader) == identity and api.read(reader) == b"original",
                        "Child sharing changed identity or compatible reads")
        finally:
            require(child.request("release") == {}, "Child sharing release failed")
        with api.opened(file) as writer:
            require(api.identity(writer) == identity, "Identity changed after child close")
            api.write(writer, b"original")
        _identity_reply(child.request("read"), identity)
    require(file.read_bytes() == b"original", "Cross-process sharing changed fixture bytes")
    return ("WIN32_SHARE_OK: compatible reads, two-way share checks, delete denial, close/reopen; "
            f"WIN32_CROSS_PROCESS_SHARE_OK parent_pid={os.getpid()} child_pid={child_pid} "
            f"file_identity={identity[0]}:{identity[1]}; independent opens, both directions, "
            "explicit close/retry and joined child; same account, no ACL/mount certification")


def locking(path: Path) -> str:
    api, file = Api(), path / "locks.bin"
    seed(file)
    with api.opened(file) as first, api.opened(file) as second:
        with api.locked(first):
            api.refused_lock(second, exclusive=False)
            api.seek(second)
            data, done = C.create_string_buffer(4), W.DWORD()
            api.expect_failure(api.dll.ReadFile(second, data, 4, C.byref(done), None), LOCK_VIOLATION, "locked ReadFile")
            api.seek(second)
            api.expect_failure(api.dll.WriteFile(second, C.create_string_buffer(b"BAD!"), 4,
                                                 C.byref(done), None), LOCK_VIOLATION, "locked WriteFile")
            require(api.read(first) == b"original", "Lock-owning handle lost read access")
            with api.locked(second, offset=4):
                pass  # adjacent half-open ranges do not conflict
        with api.locked(first, exclusive=False), api.locked(second, exclusive=False):
            require(api.read(second) == b"original", "Shared lock blocked reading")
            api.seek(first)
            api.expect_failure(api.dll.WriteFile(first, C.create_string_buffer(b"BAD!"), 4,
                                                 C.byref(done), None), LOCK_VIOLATION, "shared-lock owner's WriteFile")
            with api.opened(file) as third:
                api.refused_lock(third)
        with api.locked(second):
            pass  # explicit unlock permits the previously conflicting request
    require(file.read_bytes() == b"original", "Rejected locked I/O modified contents")
    with api.opened(file) as parent:
        identity = api.identity(parent)
        with _windows_child(file, "locking", identity) as child:
            child_pid = child.process.pid
            with api.locked(parent):
                _identity_reply(child.request("exclusive-denied"), identity)
            with api.locked(parent, exclusive=False):
                _identity_reply(child.request("shared"), identity)
            _identity_reply(child.request("exclusive"), identity)
            _identity_reply(child.request("hold"), identity)
            try:
                api.refused_lock(parent)
                api.refused_io(parent)
                with api.locked(parent, offset=4):
                    pass
            finally:
                require(child.request("release") == {}, "Child lock release failed")
            with api.locked(parent):
                require(api.identity(parent) == identity and api.read(parent) == b"original",
                        "Child unlock lost file identity or bytes")
                api.write(parent, b"original")
            _identity_reply(child.request("exclusive"), identity)
        # Successful exit includes an explicit release, zero exit code, EOF,
        # and joined pipe thread. This is not abnormal-exit/power-loss testing.
        with api.locked(parent):
            require(api.read(parent) == b"original", "Joined child left a lock or changed bytes")
    require(file.read_bytes() == b"original", "Cross-process rejected I/O modified contents")
    return ("WIN32_LOCK_OK: shared/exclusive/adjacent grants, enforced read/write refusal, unlock/retry; "
            f"WIN32_CROSS_PROCESS_LOCK_OK parent_pid={os.getpid()} child_pid={child_pid} "
            f"file_identity={identity[0]}:{identity[1]}; independent opens, both directions, "
            "explicit unlock/retry and joined child; same account, no mmap/ACL/mount certification")


def delete_pending(path: Path) -> str:
    api, file = Api(), path / "delete.bin"
    seed(file)
    with api.opened(file, READ | WRITE | DELETE) as held:
        identity = api.identity(held)
        # Explicit legacy disposition, not DeleteFileW's OS-dependent
        # POSIX-delete-then-fallback implementation. BOOLEAN is ONE byte.
        disposition = C.c_ubyte(1)
        api.checked(api.dll.SetFileInformationByHandle(held, 4, C.byref(disposition), C.sizeof(disposition)))
        api.refused_open(file, READ, SHARE_ALL, 5)  # documented ERROR_ACCESS_DENIED
        require(api.read(held) == b"original" and api.identity(held) == identity,
                "Delete-pending handle lost content/identity")
    api.refused_open(file, READ, SHARE_ALL, 2)  # final close: ERROR_FILE_NOT_FOUND
    # Current DeleteFileW prefers POSIX unlink on supported filesystems; older
    # implementations/fallbacks retain a delete-pending name. Record which
    # happened, while always requiring old data/identity and final removal.
    unlink = path / "unlink.bin"
    seed(unlink)
    with api.opened(unlink) as held:
        identity = api.identity(held)
        api.checked(api.dll.DeleteFileW(str(unlink)))
        visibility = api.refused_open(unlink, READ, SHARE_ALL, (2, 5))
        require(api.read(held) == b"original" and api.identity(held) == identity,
                "DeleteFileW invalidated the held file")
    api.refused_open(unlink, READ, SHARE_ALL, 2)
    return f"WIN32_DELETE_PENDING_OK: explicit legacy disposition and DeleteFileW retained handles; DeleteFileW new-open Win32={visibility} (2=unlinked,5=pending)"


def replace_open(path: Path) -> str:
    api, source, target = Api(), path / "source.bin", path / "target.bin"
    seed(source, b"new-data")
    seed(target, b"old-data")
    moved = path / "moved.bin"
    with api.opened(source, READ | WRITE | DELETE) as fresh, api.opened(target) as old:
        fresh_id, old_id = api.identity(fresh), api.identity(old)
        require(fresh_id != old_id, "Two new files have the same identity")
        # Legacy MoveFileEx can rename the open source to an absent name.
        # Replacing an OPEN target needs the explicit modern POSIX contract.
        api.checked(api.dll.MoveFileExW(str(source), str(moved), 0))
        require(api.identity(fresh) == fresh_id, "MoveFileExW invalidated the source")
        api.replace_posix(fresh, target)
        with api.opened(target, READ) as current:
            current_id, contents = api.identity(current), api.read(current)
            require(current_id == fresh_id,
                    f"Replacement identity {current_id} differs from source {fresh_id}; old={old_id}, "
                    f"data={contents!r}, fixture names={sorted(p.name for p in path.iterdir())}")
            require(contents == b"new-data", f"Replacement has wrong fixture data: {contents!r}")
        require(api.identity(old) == old_id and api.read(old) == b"old-data", "Replacement invalidated the old target handle")
        require(api.identity(fresh) == fresh_id and api.read(fresh) == b"new-data", "Rename invalidated the source handle")
        api.refused_open(source, READ, SHARE_ALL, 2)
        api.refused_open(moved, READ, SHARE_ALL, 2)
    require(target.read_bytes() == b"new-data", "Replacement did not survive close/reopen")
    return "WIN32_REPLACE_OPEN_OK: MoveFileExW open-source rename, FileRenameInfoEx POSIX replacement, old target/source/new name identities retained"
