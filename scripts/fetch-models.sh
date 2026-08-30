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

# LAION aesthetic predictor for ViT-B/32: a single Linear(512, 1). Extract
# the raw f32 tensors from the .pth (a zip of pickled storages) so the
# engine needs no torch: 512 weights then 1 bias, little-endian.
AES_URL="https://github.com/LAION-AI/aesthetic-predictor/raw/main/sa_0_4_vit_b_32_linear.pth"
curl -L --fail --retry 3 -o "$DEST/sa_0_4_vit_b_32_linear.pth" "$AES_URL"
python3 - "$DEST/sa_0_4_vit_b_32_linear.pth" "$DEST/aesthetic_vit_b_32.bin" <<'PY'
import sys, zipfile
src, dst = sys.argv[1], sys.argv[2]
with zipfile.ZipFile(src) as z:
    blobs = [z.read(n) for n in z.namelist() if "/data/" in n]
blobs.sort(key=len, reverse=True)
weight, bias = blobs[0], blobs[1]
assert len(weight) == 2048 and len(bias) == 4, (len(weight), len(bias))
with open(dst, "wb") as f:
    f.write(weight + bias)
print("aesthetic head extracted:", dst)
PY

mkdir -p "$RUNTIME"
cp "$DEST"/vision_model.onnx "$DEST"/text_model.onnx "$DEST"/tokenizer.json \
   "$DEST"/aesthetic_vit_b_32.bin "$RUNTIME/"
echo "installed to $RUNTIME"
