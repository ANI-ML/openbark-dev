#!/usr/bin/env bash
# Set up the real-worker tests for the MLX engine (ADR-0025, "How it is
# tested without a Mac"). Linux x86_64 only (WSL Ubuntu 24.04 or CI).
#
#   1. builds a scratch MLX engine pack from the Linux lock, through
#      OpenBark's own downloader (the ignored test os-models mlx_pack_install);
#   2. downloads the pinned test model mlx-community/Qwen3-0.6B-4bit into a
#      scratch folder with curl, and checks every file's size and hash;
#   3. prints the two exports the tests in engine/os-llm/tests/mlx_worker.rs need.
#
# Usage:  scripts/mlx-test-env.sh [scratch folder]     (default /tmp/openbark-mlx-test)
#         eval "$(scripts/mlx-test-env.sh | grep ^export)"
#
# Network (docs/network.md, B8): files.pythonhosted.org and github.com
# (release-assets.githubusercontent.com) for the pack, huggingface.co and its
# storage hosts for the model. Never point the scratch folder at a real
# OpenBark data folder.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SCRATCH="${1:-/tmp/openbark-mlx-test}"
case "$SCRATCH" in
  /*) ;;
  *) echo "give the scratch folder as a full path" >&2; exit 2 ;;
esac
case "$SCRATCH" in
  *OpenBark*) echo "refusing a folder that looks like a real OpenBark data folder: $SCRATCH" >&2; exit 2 ;;
esac
if [ "$(uname -s)" != "Linux" ] || [ "$(uname -m)" != "x86_64" ]; then
  echo "the Linux test pack runs on Linux x86_64 only" >&2
  exit 2
fi

REPO="mlx-community/Qwen3-0.6B-4bit"
REVISION="73e3e38d981303bc594367cd910ea6eb48349da8"
MODEL_DIR="$SCRATCH/test-model/Qwen3-0.6B-4bit"
export OPENBARK_ENGINES_DIR="$SCRATCH/engines"
export OPENBARK_MODELS_DIR="$SCRATCH/models"
mkdir -p "$OPENBARK_ENGINES_DIR" "$OPENBARK_MODELS_DIR" "$MODEL_DIR"

echo "== MLX engine pack (Linux lock) into $OPENBARK_ENGINES_DIR" >&2
LOG="$SCRATCH/pack-install.log"
(cd "$ROOT" && cargo test -p os-models mlx_pack_install -- --ignored --nocapture) 2>&1 | tee "$LOG" >&2
PYTHON="$(sed -n 's/^OPENBARK_MLX_TEST_PYTHON=//p' "$LOG" | tail -n 1)"
if [ -z "$PYTHON" ] || [ ! -x "$PYTHON" ]; then
  echo "the pack didn't install (see $LOG)" >&2
  exit 1
fi

echo "== test model $REPO@$REVISION into $MODEL_DIR" >&2
# file, size, then sha256 for LFS files or the git blob SHA-1 for the rest.
FILES="
added_tokens.json 707 sha1:b54f9135e44c1e81047e8d05cb027af8bc039eed
config.json 937 sha1:6c974b02334f2f5f68de4cb2796ee4fe80bfc712
merges.txt 1671853 sha1:31349551d90c7606f325fe0f11bbb8bd5fa0d7c7
model.safetensors 335450584 sha256:392e8d466d56100ada00eb82031fb854297fc9e389b7d303eba3af114e87bce2
model.safetensors.index.json 49731 sha1:e743a6a7d2ba4fd21b2256d739117cbf31345297
special_tokens_map.json 613 sha1:ac23c0aaa2434523c494330aeb79c58395378103
tokenizer.json 11422654 sha256:aeb13307a71acd8fe81861d94ad54ab689df773318809eed3cbe794b4492dae4
tokenizer_config.json 9706 sha1:7345216a0785dc7086e8c245b2a9d3896ce2b756
vocab.json 2776833 sha1:4783fe10ac3adce15ac8f358ef5462739852c569
"
check_hash() { # file expected(kind:hex)
  python3 -I -c '
import hashlib, sys
path, want = sys.argv[1], sys.argv[2]
kind, hexd = want.split(":", 1)
data = open(path, "rb").read()
if kind == "sha256":
    got = hashlib.sha256(data).hexdigest()
else:
    got = hashlib.sha1(b"blob %d\0" % len(data) + data).hexdigest()
sys.exit(0 if got == hexd else 1)
' "$1" "$2"
}
echo "$FILES" | while read -r name size hash; do
  [ -n "$name" ] || continue
  dest="$MODEL_DIR/$name"
  if [ -f "$dest" ] && [ "$(stat -c %s "$dest")" = "$size" ] && check_hash "$dest" "$hash"; then
    echo "have $name" >&2
    continue
  fi
  curl -fsSL --retry 3 -A "OpenBark-tests" -o "$dest.part" \
    "https://huggingface.co/$REPO/resolve/$REVISION/$name"
  got="$(stat -c %s "$dest.part")"
  if [ "$got" != "$size" ]; then
    echo "$name: $got bytes, expected $size" >&2
    exit 1
  fi
  if ! check_hash "$dest.part" "$hash"; then
    echo "$name: hash mismatch" >&2
    exit 1
  fi
  mv "$dest.part" "$dest"
  echo "got $name ($size bytes)" >&2
done

echo "export OPENBARK_MLX_TEST_PYTHON=$PYTHON"
echo "export OPENBARK_MLX_TEST_MODEL=$MODEL_DIR"
