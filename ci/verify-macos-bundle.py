"""Validate actual build output; never register extensions or claim a mount."""
import json
import pathlib
import platform
import plistlib
import subprocess
import sys


def dynamic_dependencies(output):
    # otool prints the executable's own path as its first line; that is NOT a
    # load command. Only inspect the indented dependency lines after it.
    lines = output.splitlines()
    if not lines or not lines[0].endswith(":"):
        raise ValueError("Invalid otool dependency report")
    libraries = [line.strip().split(" (", 1)[0] for line in lines[1:] if line.strip()]
    if any("libportaweave_native.dylib" in lib or "/target/" in lib for lib in libraries):
        raise ValueError("Build-tree dynamic dependency would make the bundle nonportable")
    return libraries


def check_bundle(app, arch):
    extension = app / "Contents/Extensions/PortaWeaveFSKit.appex"
    paths = [app, extension]
    expected = ["dev.portaweave.lab", "dev.portaweave.lab.fskit"]
    binaries = []
    for path, identifier in zip(paths, expected):
        with (path / "Contents/Info.plist").open("rb") as stream:
            info = plistlib.load(stream)
        if info.get("CFBundleIdentifier") != identifier:
            raise ValueError(f"Wrong bundle identity in {path}")
        executable = info["CFBundleExecutable"]
        if pathlib.PurePosixPath(executable).name != executable:
            raise ValueError("Executable must be a single file name")
        binary = path / "Contents/MacOS" / executable
        architectures = subprocess.check_output(["lipo", "-archs", str(binary)], text=True).split()
        if architectures != [arch]:
            raise ValueError(f"Unexpected architectures: {architectures}")
        symbols = subprocess.check_output(["nm", "-gU", str(binary)], text=True)
        if " _pw_abi_version" not in symbols:
            raise ValueError("Shared Rust ABI is not linked into the executable")
        libraries = dynamic_dependencies(
            subprocess.check_output(["otool", "-L", str(binary)], text=True)
        )
        signature = subprocess.run(
            ["codesign", "-d", "--verbose=2", str(binary)], capture_output=True, text=True
        )
        if signature.returncode == 0 and "Signature=adhoc" in signature.stderr:
            signing = "linker ad-hoc signature; no developer identity"
        elif signature.returncode != 0 and "not signed at all" in signature.stderr:
            signing = "unsigned"
        else:
            raise ValueError("Unexpected signing identity in laboratory build")
        binaries.append({
            "bundle": identifier, "architectures": architectures,
            "libraries": libraries, "signing": signing,
        })
    attrs = info["EXAppExtensionAttributes"]
    if attrs.get("EXExtensionPointIdentifier") != "com.apple.fskit.fsmodule":
        raise ValueError("Missing FSKit extension point")
    if attrs.get("FSShortName") != "portaweave":
        raise ValueError("Wrong filesystem name")
    if not attrs.get("FSSupportsPathURLs") or not attrs.get("FSRequiresSecurityScopedPathURLResources"):
        raise ValueError("Path resources must require security scope")
    if (app / "Contents/PlugIns/PortaWeaveFSKit.appex").exists():
        raise ValueError("ExtensionKit payload must not be embedded as a Foundation plugin")
    if any((path / "Contents/_CodeSignature").exists() for path in paths):
        raise ValueError("Expected an unsigned CI lab build")
    return {
        "scope": "unsigned Xcode application and ExtensionKit bundle build",
        "os": platform.platform(),
        "binaries": binaries,
        "developer_signed": False,
        "installation_attempted": False,
        "extension_enable_attempted": False,
        "actual_mount": "NOT_RUN",
        "development_volume_certified": False,
    }


if __name__ == "__main__":
    app, report, architecture = sys.argv[1:]
    result = check_bundle(pathlib.Path(app), architecture)
    with pathlib.Path(report).open("x") as stream:
        json.dump(result, stream, indent=2)
        stream.write("\n")
    print("MACOS_APP_EXTENSION_BUILD_OK (unsigned; no installation or mount)")
