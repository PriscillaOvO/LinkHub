#!/usr/bin/env bash
#
# build-core-ios.sh — cross-compile the Rust core to a LinkHubCore.xcframework
# that the Xcode app links. macOS + Xcode command-line tools required (the iOS
# SDK and `lipo`/`xcodebuild` ship only on macOS). Run from anywhere.
#
#   ./ios/scripts/build-core-ios.sh [debug|release]   (default: release)
#
# Output: ios/Frameworks/LinkHubCore.xcframework
set -euo pipefail

PROFILE="${1:-release}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
IOS_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
ROOT_DIR="$(cd "$IOS_DIR/.." && pwd)"
CORE_DIR="$ROOT_DIR/core-rs"
INCLUDE_DIR="$IOS_DIR/include"
OUT_DIR="$IOS_DIR/Frameworks"
LIB="liblinkhub_core.a"

if [[ "$(uname)" != "Darwin" ]]; then
  echo "error: iOS builds require macOS (iOS SDK + lipo + xcodebuild)." >&2
  exit 1
fi

DEVICE_TRIPLE="aarch64-apple-ios"
SIM_TRIPLES=("aarch64-apple-ios-sim" "x86_64-apple-ios")

CARGO_PROFILE_FLAG=""
TARGET_SUBDIR="debug"
if [[ "$PROFILE" == "release" ]]; then
  CARGO_PROFILE_FLAG="--release"
  TARGET_SUBDIR="release"
fi

echo "==> installing rust targets"
rustup target add "$DEVICE_TRIPLE" "${SIM_TRIPLES[@]}"

echo "==> building core ($PROFILE) for device + simulator"
for triple in "$DEVICE_TRIPLE" "${SIM_TRIPLES[@]}"; do
  ( cd "$CORE_DIR" && cargo build --lib $CARGO_PROFILE_FLAG --target "$triple" )
done

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

# The two simulator slices (arm64 + x86_64) must be fused with lipo; the device
# slice stays standalone (xcframework cannot hold two same-platform archs).
SIM_FAT="$WORK/sim/$LIB"
mkdir -p "$WORK/sim"
lipo -create \
  "$CORE_DIR/target/aarch64-apple-ios-sim/$TARGET_SUBDIR/$LIB" \
  "$CORE_DIR/target/x86_64-apple-ios/$TARGET_SUBDIR/$LIB" \
  -output "$SIM_FAT"

echo "==> assembling xcframework"
rm -rf "$OUT_DIR/LinkHubCore.xcframework"
mkdir -p "$OUT_DIR"
xcodebuild -create-xcframework \
  -library "$CORE_DIR/target/$DEVICE_TRIPLE/$TARGET_SUBDIR/$LIB" -headers "$INCLUDE_DIR" \
  -library "$SIM_FAT" -headers "$INCLUDE_DIR" \
  -output "$OUT_DIR/LinkHubCore.xcframework"

echo "==> done: $OUT_DIR/LinkHubCore.xcframework"
