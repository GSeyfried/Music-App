#!/bin/sh
# Builds the Rust engine (crates/voiceloop-ffi) as a static library for the
# iOS platform Xcode is building, into target/<triple>/<profile>/.
# Called from the VoiceLoopPocket Xcode build phase; also runnable by hand:
#   PLATFORM_NAME=iphonesimulator CONFIGURATION=Debug scripts/build-pocket-engine.sh
set -eu
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
export PATH="$HOME/.cargo/bin:$PATH"
case "${PLATFORM_NAME:-iphonesimulator}" in
  iphonesimulator) TRIPLE=aarch64-apple-ios-sim ;;
  iphoneos) TRIPLE=aarch64-apple-ios ;;
  *) echo "unsupported platform ${PLATFORM_NAME}" >&2; exit 1 ;;
esac
PROFILE_FLAG=""
if [ "${CONFIGURATION:-Debug}" = "Release" ]; then PROFILE_FLAG="--release"; fi
cd "$ROOT"
# Xcode's SDK variables would make cargo link host build scripts against the
# iOS SDK; cargo/cc find the right SDK for the target themselves.
env -u SDKROOT -u IPHONEOS_DEPLOYMENT_TARGET -u LIBRARY_PATH \
  cargo build -p voiceloop-ffi --target "$TRIPLE" $PROFILE_FLAG
