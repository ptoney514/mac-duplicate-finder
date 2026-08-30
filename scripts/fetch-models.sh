#!/usr/bin/env bash
# Downloads the CLIP ViT-B/32 ONNX models (vision + text encoders with
# projection heads, 512-dim output) and tokenizer into models/ (gitignored),
# then installs a copy into the runtime location the app and CLI share.
set -euo pipefail
cd "$(dirname "$0")/.."

REPO="https://huggingface.co/Xenova/clip-vit-base-patch32/resolve/main"
DEST="models"
RUNTIME="$HOME/Library/Application Support/Culler/models"
mkdir -p "$DEST"

fetch() {
  local url="$1" out="$2"
  echo "fetching $out"
  curl -L --fail --retry 3 -C - -o "$DEST/$out" "$url"
}

fetch "$REPO/onnx/vision_model.onnx" vision_model.onnx
fetch "$REPO/onnx/text_model.onnx" text_model.onnx
fetch "$REPO/tokenizer.json" tokenizer.json

mkdir -p "$RUNTIME"
cp "$DEST"/vision_model.onnx "$DEST"/text_model.onnx "$DEST"/tokenizer.json "$RUNTIME/"
echo "installed to $RUNTIME"
