#!/usr/bin/env bash
# Source this in Git Bash on Windows before cargo builds:  source scripts/dev-env.sh
# Puts Rust, the VS-bundled CMake and libclang on the environment for this shell.
export PATH="$HOME/.cargo/bin:$PATH"
VSWHERE="/c/Program Files (x86)/Microsoft Visual Studio/Installer/vswhere.exe"
if [ -x "$VSWHERE" ]; then
  VS="$("$VSWHERE" -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath | tr -d '\r')"
  CMAKE_BIN="$(cygpath -u "$VS")/Common7/IDE/CommonExtensions/Microsoft/CMake/CMake/bin"
  [ -x "$CMAKE_BIN/cmake.exe" ] && export PATH="$CMAKE_BIN:$PATH"
fi
if [ -z "${LIBCLANG_PATH:-}" ]; then
  LLVM_BIN="$(ls -d "$LOCALAPPDATA"/Programs/llvm/*/bin 2>/dev/null | head -1)"
  [ -n "$LLVM_BIN" ] && export LIBCLANG_PATH="$LLVM_BIN"
fi
echo "rustc: $(rustc --version 2>/dev/null)  cmake: $(cmake --version 2>/dev/null | head -1)  LIBCLANG_PATH=${LIBCLANG_PATH:-unset}"
# Local CUDA builds: compile kernels for this machine's GPU only. Building all
# 10 default architectures in parallel exhausted memory (cicc 0xC0000374).
# Release/CI builds that ship to other GPUs must unset this.
if command -v nvidia-smi >/dev/null 2>&1 && [ -z "${CUDAARCHS:-}" ]; then
  CAP="$(nvidia-smi --query-gpu=compute_cap --format=csv,noheader 2>/dev/null | head -1 | tr -d '.\r ')"
  [ -n "$CAP" ] && export CUDAARCHS="$CAP" CMAKE_BUILD_PARALLEL_LEVEL="${CMAKE_BUILD_PARALLEL_LEVEL:-8}"
fi
