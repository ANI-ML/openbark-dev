#!/usr/bin/env bash
# A tiny chat model for the shutdown and quit tests (CI only; never shipped):
# SmolLM2-135M-Instruct, Q4_K_M, from unsloth/SmolLM2-135M-Instruct-GGUF
# (Apache-2.0) at a pinned revision, checked by size and SHA-256.
#   scripts/ci/fetch-test-gguf.sh <dest.gguf>
set -euo pipefail
dest="${1:?usage: fetch-test-gguf.sh <dest.gguf>}"
REV=9e6855bc4be717fca1ef21360a1db4b29d5c559a
FILE=SmolLM2-135M-Instruct-Q4_K_M.gguf
SIZE=105454144
SHA256=ed5fa30c487b282ec156c29062f1222e5c20875a944ac98289dbd242e947f747

sha() { if command -v sha256sum >/dev/null; then sha256sum "$1" | cut -d' ' -f1; else shasum -a 256 "$1" | cut -d' ' -f1; fi; }
size() { wc -c < "$1" | tr -d ' '; }

if [ -f "$dest" ] && [ "$(size "$dest")" = "$SIZE" ] && [ "$(sha "$dest")" = "$SHA256" ]; then
  echo "test model already here: $dest"
  exit 0
fi
mkdir -p "$(dirname "$dest")"
curl -fsSL --retry 3 -o "$dest.part" "https://huggingface.co/unsloth/SmolLM2-135M-Instruct-GGUF/resolve/$REV/$FILE"
[ "$(size "$dest.part")" = "$SIZE" ] || { echo "test model: wrong size $(size "$dest.part")"; exit 1; }
[ "$(sha "$dest.part")" = "$SHA256" ] || { echo "test model: SHA-256 mismatch"; exit 1; }
mv "$dest.part" "$dest"
echo "test model: $dest ($SIZE bytes, sha256 $SHA256)"
