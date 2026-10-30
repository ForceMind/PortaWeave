"""Actual synchronous Win32 file API probes on caller-owned new scratch files.

Only workspace_probe supplies the private per-case directory. No driver,
mount, ACL change, process impersonation, raw device or background work.
"""
from __future__ import annotations

from contextlib import contextmanager
import ctypes as C
from ctypes import wintypes as W
from pathlib import Path

READ, WRITE = 0x80000000, 0x40000000
SHARE_READ, SHARE_ALL = 1, 7
SHARING_VIOLATION, LOCK_VIOLATION = 32, 33


class Overlapped(C.Structure):
    _fields_ = [("Internal", C.c_size_t), ("InternalHigh", C.c_size_t),
                ("Offset", W.DWORD), ("OffsetHigh", W.DWORD), ("hEvent", W.HANDLE)]


class FileInformation(C.Structure):
    _fields_ = [("attributes", W.DWORD), ("created", W.FILETIME),
                ("accessed", W.FILETIME), ("written", W.FILETIME),
                ("volume", W.DWORD), ("sizeHigh", W.DWORD), ("sizeLow", W.DWORD),
                ("links", W.DWORD), ("indexHigh", W.DWORD), ("indexLow", W.DWORD)]


def require(condition: bool, message: str) -> None:
    if not condition:
        raise RuntimeError(message)


class Api:
    def __init__(self):
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
            "MoveFileExW": (W.BOOL, [W.LPCWSTR, W.LPCWSTR, W.DWORD]),
            "GetFileInformationByHandle": (W.BOOL, [W.HANDLE, C.POINTER(FileInformation)]),
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
            raise C.WinError(C.get_last_error())
        try:
            yield handle
        finally:
            self.checked(self.dll.CloseHandle(handle))

    def refused_open(self, path: Path, access: int, share: int, code: int):
        handle = self.dll.CreateFileW(str(path), access, share, None, 3, 128, None)
        error = C.get_last_error()
        if handle != self.invalid:
            self.checked(self.dll.CloseHandle(handle))
            raise RuntimeError("CreateFileW unexpectedly admitted a forbidden open")
        require(error == code, f"CreateFileW: expected Win32 {code}, got {error}")

    def seek(self, handle, offset=0):
        self.checked(self.dll.SetFilePointerEx(handle, offset, None, 0))

    def read(self, handle, length=8) -> bytes:
        self.seek(handle)
        data, done = C.create_string_buffer(length), W.DWORD()
        self.checked(self.dll.ReadFile(handle, data, length, C.byref(done), None))
        return data.raw[:done.value]

    def identity(self, handle) -> tuple[int, int, int]:
        info = FileInformation()
        self.checked(self.dll.GetFileInformationByHandle(handle, C.byref(info)))
        require(bool(info.indexHigh or info.indexLow), "File identity is zero")
        return info.volume, info.indexHigh, info.indexLow

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
    return "WIN32_SHARE_OK: compatible reads, two-way share checks, delete denial, close/reopen"


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
    return "WIN32_LOCK_OK: shared/exclusive/adjacent grants, enforced read/write refusal, unlock/retry; two handles, one process"


def delete_pending(path: Path) -> str:
    api, file = Api(), path / "delete.bin"
    seed(file)
    with api.opened(file) as held:
        identity = api.identity(held)
        api.checked(api.dll.DeleteFileW(str(file)))
        api.refused_open(file, READ, SHARE_ALL, 5)  # documented ERROR_ACCESS_DENIED
        require(api.read(held) == b"original" and api.identity(held) == identity,
                "Delete-pending handle lost content/identity")
    api.refused_open(file, READ, SHARE_ALL, 2)  # final close: ERROR_FILE_NOT_FOUND
    return "WIN32_DELETE_PENDING_OK: share-delete, new-open denial, retained handle, final-close removal"


def replace_open(path: Path) -> str:
    api, source, target = Api(), path / "source.bin", path / "target.bin"
    seed(source, b"new-data")
    seed(target, b"old-data")
    with api.opened(source) as fresh, api.opened(target) as old:
        fresh_id, old_id = api.identity(fresh), api.identity(old)
        require(fresh_id != old_id, "Two new files have the same identity")
        api.checked(api.dll.MoveFileExW(str(source), str(target), 1))  # REPLACE_EXISTING; never delayed/reboot/copy
        with api.opened(target, READ) as current:
            require(api.identity(current) == fresh_id and api.read(current) == b"new-data", "Replacement lost source identity/data")
        require(api.identity(old) == old_id and api.read(old) == b"old-data", "Replacement invalidated the old target handle")
        require(api.identity(fresh) == fresh_id and api.read(fresh) == b"new-data", "Rename invalidated the source handle")
        api.refused_open(source, READ, SHARE_ALL, 2)
    require(target.read_bytes() == b"new-data", "Replacement did not survive close/reopen")
    return "WIN32_REPLACE_OPEN_OK: share-delete on both files, old target/source/new name identities retained"
