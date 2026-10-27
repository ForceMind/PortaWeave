"""Run the actual pre-v3 cdylib against a disposable v3 container, read-only.

The caller builds the pinned checkout into a separate target directory and
creates the NEW v3 fixture. This check never opens a source with the new reader.
"""
import ctypes
import hashlib
from pathlib import Path
import subprocess
import sys

BASELINE = "a578785591976e9d71658dc5b93bf1c234c9db7f"


def main():
    if len(sys.argv) != 4:
        raise SystemExit("usage: old_reader_guard.py OLD_CHECKOUT OLD_DEBUG_LIBDIR V3_FIXTURE")
    checkout, libdir, source = map(Path, sys.argv[1:])
    actual = subprocess.check_output(
        ["git", "-C", str(checkout), "rev-parse", "HEAD"], text=True
    ).strip()
    assert actual == BASELINE, f"unexpected old-reader source: {actual}"
    assert (source / "format.txt").read_text().strip() == "experimental-m1o-directory-v3"

    def hashes():
        return {str(p.relative_to(source)): hashlib.sha256(p.read_bytes()).hexdigest()
                for p in source.rglob("*") if p.is_file()}

    before = hashes()
    name = ("portaweave_native.dll" if sys.platform == "win32" else
            "libportaweave_native.dylib" if sys.platform == "darwin" else
            "libportaweave_native.so")
    library = ctypes.CDLL(str((libdir / name).resolve()))
    library.pw_volume_open.argtypes = [ctypes.c_void_p, ctypes.c_size_t,
                                      ctypes.POINTER(ctypes.c_uint64)]
    library.pw_volume_open.restype = ctypes.c_int32
    path = str(source.resolve()).encode("utf-8")
    output = ctypes.c_uint64(99)
    for _ in range(2):
        status = library.pw_volume_open(path, len(path), ctypes.byref(output))
        assert status == -11, f"old reader did not refuse v3: {status}"
        assert output.value == 99, "old reader modified a failed output"
        assert hashes() == before, "old reader changed original container bytes"
    print(f"ACTUAL_V2_READER_REJECTS_V3_OK old_source={actual} platform={sys.platform} "
          "status=UNSUPPORTED output_unchanged=true original_SHA256_unchanged=true")


if __name__ == "__main__":
    main()
