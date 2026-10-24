#!/bin/bash
# Builds an unsigned lab artifact only. Never installs/enables/mounts anything.
set -euo pipefail
cd "$(dirname "$0")/.."
if [ "$(uname -s)" != Darwin ]; then
    echo "BLOCKED: this build requires macOS with Xcode and FSKit SDK" >&2
    exit 3
fi
arch=$(uname -m)
case "$arch" in
    arm64) rust_target=aarch64-apple-darwin ;;
    x86_64) rust_target=x86_64-apple-darwin ;;
    *) echo "BLOCKED: unsupported build architecture $arch" >&2; exit 3 ;;
esac
build_root="$PWD/target/macos-lab-${GITHUB_RUN_ID:-local}-${GITHUB_RUN_ATTEMPT:-1}"
if [ -e "$build_root" ]; then
    echo "Refusing to overwrite existing build evidence: $build_root" >&2
    exit 2
fi
mkdir -p "$build_root"
xcodebuild -version | tee "$build_root/xcode-version.txt"
xcrun --sdk macosx --show-sdk-version | tee "$build_root/sdk-version.txt"
rustup target add "$rust_target"
MACOSX_DEPLOYMENT_TARGET=26.0 cargo build --locked --release \
    -p portaweave-native --target "$rust_target" 2>&1 | tee "$build_root/rust-build.log"
plutil -lint platform/macos/PortaWeave.xcodeproj/project.pbxproj
xcodebuild -project platform/macos/PortaWeave.xcodeproj -scheme PortaWeave \
    -configuration Release -destination 'generic/platform=macOS' \
    -derivedDataPath "$build_root/DerivedData" \
    ARCHS="$arch" ONLY_ACTIVE_ARCH=YES PW_RUST_TARGET="$rust_target" \
    CODE_SIGNING_ALLOWED=NO CODE_SIGNING_REQUIRED=NO \
    build 2>&1 | tee "$build_root/xcode-build.log"
python3 ci/verify-macos-bundle.py \
    "$build_root/DerivedData/Build/Products/Release/PortaWeave.app" \
    "$build_root/bundle-evidence.json" "$arch"
ditto -c -k --keepParent \
    "$build_root/DerivedData/Build/Products/Release/PortaWeave.app" \
    "$build_root/PortaWeave-unsigned-lab.zip"
