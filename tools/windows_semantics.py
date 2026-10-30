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
DELETE = 0x00010000
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


class RenameInformation(C.Structure):
    # FileRenameInfoEx uses the DWORD Flags member of FILE_RENAME_INFO's
    # leading union. RootDirectory alignment differs between x86 and x64.
    _fields_ = [("Flags", W.DWORD), ("RootDirectory", W.HANDLE),
                ("FileNameLength", W.DWORD), ("FileName", W.WCHAR * 1)]


def require(condition: bool, message: str) -> None:
    if not condition:
        raise RuntimeError(message)


class Api:
    def __init__(self):
        require(C.sizeof(W.DWORD) == 4 and C.sizeof(W.WCHAR) == 2 and
                C.sizeof(FileInformation) == 52, "Unexpected Win32 wire types")
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
            "MoveFileExW": (W.BOOL, [W.LPCWSTR, W.LPCWSTR, W.DWORD]),
            "GetFileInformationByHandle": (W.BOOL, [W.HANDLE, C.POINTER(FileInformation)]),
            "SetFileInformationByHandle": (W.BOOL, [W.HANDLE, C.c_int, C.c_void_p, W.DWORD]),
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
        data = C.create_string_buffer(max(C.sizeof(RenameInformation), offset + len(encoded)))
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
            require(api.identity(current) == fresh_id and api.read(current) == b"new-data", "Replacement lost source identity/data")
        require(api.identity(old) == old_id and api.read(old) == b"old-data", "Replacement invalidated the old target handle")
        require(api.identity(fresh) == fresh_id and api.read(fresh) == b"new-data", "Rename invalidated the source handle")
        api.refused_open(source, READ, SHARE_ALL, 2)
        api.refused_open(moved, READ, SHARE_ALL, 2)
    require(target.read_bytes() == b"new-data", "Replacement did not survive close/reopen")
    return "WIN32_REPLACE_OPEN_OK: MoveFileExW open-source rename, FileRenameInfoEx POSIX replacement, old target/source/new name identities retained"
