#!/usr/bin/env bash
# Builds culler-core for aarch64-apple-darwin and x86_64-apple-darwin,
# generates the UniFFI Swift bindings, and packages a universal static
# XCFramework for the Xcode project.
#
# Outputs:
#   apple/CullerCore.xcframework                     (gitignored build product)
#   apple/Culler/Culler/Engine/Generated/*.swift     (committed, per PRD §12)
set -euo pipefail
cd "$(dirname "$0")/.."

# Cargo lives in a keg-only Homebrew rustup on this machine.
export PATH="/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:$PATH"

# Match the app's deployment target so C objects (sqlite3, blake3) don't
# trigger "built for newer macOS" linker warnings in Xcode.
export MACOSX_DEPLOYMENT_TARGET=14.0

# arm64-only since milestone 4: onnxruntime ships no macOS x86_64 prebuilts
# (ADR-0005). Re-add x86_64-apple-darwin here if Intel support returns.
TARGETS=(aarch64-apple-darwin)
STAGE=target/xcframework
rm -rf "$STAGE"
mkdir -p "$STAGE/headers"

for target in "${TARGETS[@]}"; do
  echo "building culler-core for $target"
  cargo build --release -p culler-core --target "$target"
done

echo "generating Swift bindings"
cargo run -q -p uniffi-bindgen -- generate \
  --library target/aarch64-apple-darwin/release/libculler_core.dylib \
  --language swift --out-dir "$STAGE/bindings"

cp "$STAGE/bindings/culler_coreFFI.h" "$STAGE/headers/"
cp "$STAGE/bindings/culler_coreFFI.modulemap" "$STAGE/headers/module.modulemap"

echo "creating static library"
lipo -create \
  target/aarch64-apple-darwin/release/libculler_core.a \
  -output "$STAGE/libculler_core.a"

rm -rf apple/CullerCore.xcframework
xcodebuild -create-xcframework \
  -library "$STAGE/libculler_core.a" -headers "$STAGE/headers" \
  -output apple/CullerCore.xcframework >/dev/null

mkdir -p apple/Culler/Culler/Engine/Generated
cp "$STAGE/bindings/culler_core.swift" apple/Culler/Culler/Engine/Generated/

echo "done: apple/CullerCore.xcframework"
