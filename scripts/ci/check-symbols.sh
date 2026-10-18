#!/usr/bin/env bash
# Spike S2 / acceptance test M0-5: the binary must contain exactly one copy of
# ggml (from llama.cpp), no whisper.cpp, and one ONNX Runtime.
# Usage: check-symbols.sh path/to/openbark   (Linux or macOS binary)
set -euo pipefail
bin="${1:?binary path}"
syms=$(nm -g --defined-only "$bin" 2>/dev/null || nm -gU "$bin")

count() { grep -cE "$1" <<<"$syms" || true; }

ggml=$(count '[[:space:]]_?ggml_init$')
whisper=$(count '[[:space:]]_?whisper_init')
ort=$(count '[[:space:]]_?OrtGetApiBase$')

echo "ggml_init definitions: $ggml   whisper_init*: $whisper   OrtGetApiBase: $ort"
fail=0
[[ "$ggml" -eq 1 ]] || { echo "::error::expected exactly one ggml (got $ggml)"; fail=1; }
[[ "$whisper" -eq 0 ]] || { echo "::error::whisper.cpp must not be linked (ADR-0001)"; fail=1; }
# ORT is statically linked on Linux/macOS (one copy expected); on Windows it is a DLL.
[[ "$ort" -le 1 ]] || { echo "::error::more than one ONNX Runtime ($ort)"; fail=1; }
exit $fail
