#!/usr/bin/env bash
# A tiny chat model for the shutdown and quit tests (CI only; never shipped):
# SmolLM2-135M-Instruct, Q4_K_M, from unsloth/SmolLM2-135M-Instruct-GGUF
# (Apache-2.0) at a pinned revision, checked by size and SHA-256.
#
# `qwen2.5-0.5b` instead fetches Qwen2.5-0.5B-Instruct, Q4_K_M, from Qwen's own
# Qwen/Qwen2.5-0.5B-Instruct-GGUF (Apache-2.0), also pinned: the iOS smoke
# test's drafting model. SmolLM2-135M loads and generates there, but runs
# past 4,096 tokens inside one string of the SOAP note and never closes it,
# so it can't show that a whole note comes back.
#   scripts/ci/fetch-test-gguf.sh <dest.gguf> [smollm2 | qwen2.5-0.5b]
set -euo pipefail
dest="${1:?usage: fetch-test-gguf.sh <dest.gguf> [smollm2 | qwen2.5-0.5b]}"
case "${2:-smollm2}" in
  smollm2)
    URL=https://huggingface.co/unsloth/SmolLM2-135M-Instruct-GGUF/resolve/9e6855bc4be717fca1ef21360a1db4b29d5c559a/SmolLM2-135M-Instruct-Q4_K_M.gguf
    SIZE=105454144
    SHA256=ed5fa30c487b282ec156c29062f1222e5c20875a944ac98289dbd242e947f747
    ;;
  qwen2.5-0.5b)
    URL=https://huggingface.co/Qwen/Qwen2.5-0.5B-Instruct-GGUF/resolve/9217f5db79a29953eb74d5343926648285ec7e67/qwen2.5-0.5b-instruct-q4_k_m.gguf
    SIZE=491400032
    SHA256=74a4da8c9fdbcd15bd1f6d01d621410d31c6fc00986f5eb687824e7b93d7a9db
    ;;
  *) echo "unknown test model: $2" >&2; exit 2 ;;
esac

sha() { if command -v sha256sum >/dev/null; then sha256sum "$1" | cut -d' ' -f1; else shasum -a 256 "$1" | cut -d' ' -f1; fi; }
size() { wc -c < "$1" | tr -d ' '; }

if [ -f "$dest" ] && [ "$(size "$dest")" = "$SIZE" ] && [ "$(sha "$dest")" = "$SHA256" ]; then
  echo "test model already here: $dest"
  exit 0
fi
mkdir -p "$(dirname "$dest")"
curl -fsSL --retry 3 -o "$dest.part" "$URL"
[ "$(size "$dest.part")" = "$SIZE" ] || { echo "test model: wrong size $(size "$dest.part")"; exit 1; }
[ "$(sha "$dest.part")" = "$SHA256" ] || { echo "test model: SHA-256 mismatch"; exit 1; }
mv "$dest.part" "$dest"
echo "test model: $dest ($SIZE bytes, sha256 $SHA256)"
