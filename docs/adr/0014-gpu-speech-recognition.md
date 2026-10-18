# ADR-0014: Speech recognition on NVIDIA graphics cards, as an optional pack

- Status: accepted for Windows (built and tested on an RTX 4080). The product owner should confirm the NVIDIA licence wording before release.
- Date: 2026-10-18
- Deciders: product owner (asked that transcription use the GPU where possible, and that imports be as fast as possible), engineering

## Context

Imports felt slow. Measured on the dev box (20 logical processors, RTX 4080 16 GB) with an 8.5-minute consult as WebM:
- **Reading the file is not the problem.** Decoding WebM cost 0.7 s more than the same audio as WAV. Start-up, models and decoding together took under 3 s.
- **Transcription took about 80 s.** Parakeet int8 was configured with 8 threads per recogniser, but more threads were slower: 2 threads took 56 s, 4 took 34 s, 6 took 28 s, 8 took 39 s and 16 took 92 s. During an import the second recogniser (the number check, Whisper large-v3-turbo) runs alongside, so the two fought over the processor.
- **The number check took another 70 s** after transcription finished, because Whisper turbo on the processor can't keep up.
- **The note model already runs on the GPU** (llama.cpp with Vulkan or CUDA). Speech recognition (sherpa-onnx, on ONNX Runtime) ran only on the processor.

GPU measurements, from sherpa-onnx v1.13.8's CUDA build of ONNX Runtime 1.28.2:

| Model | Processor | GPU |
|---|---|---|
| Parakeet, int8 (8.5 min) | 28 s (6 threads) | 43 s: slower |
| Parakeet, full precision (8.5 min) | 24 s (6 threads) | 14 s |
| Whisper turbo, int8 (5.5 min, every segment) | 94 s (8 threads) | not tried |
| Whisper turbo, full precision (5.5 min) | 166 s (8 threads) | 22 s |
| **Whole import, 8.5 min** | **152 s** | **43 s** |

The int8 models are tuned for processors. The GPU only pays off with the full-precision models (2.5 GB and 3.2 GB).

What the GPU path needs at run time:
- ONNX Runtime's CUDA provider (474 MB);
- NVIDIA's CUDA runtime, cuBLAS and cuBLASLt;
- all of cuDNN 9: Parakeet's LSTM decoder needs `cudnn_adv`, and cuDNN 9.27 added `cudnn_engines_tensor_ir`.

That's about 1.9 GB to download. Most clinics have none of it installed.

## Decision

1. **Processor threads: 6 for the main recogniser, up to 8 for the second** (a third and two fifths of the processor). `OPENBARK_ASR_THREADS` and `OPENBARK_SECOND_THREADS` stay available for tuning.
2. **An optional GPU speech pack** on the Models screen, shown only on Windows x64 with an NVIDIA driver (`nvcuda.dll`; the name comes from `nvidia-smi`).
   - The pack: ONNX Runtime 1.28.2 with CUDA, from the sherpa-onnx v1.13.8 release (the build sherpa-onnx is tested with). NVIDIA's `nvidia-cuda-runtime-cu12` 12.9.79, `nvidia-cublas-cu12` 12.9.2.10 and `nvidia-cudnn-cu12` 9.27.0.42, from NVIDIA's own packages on PyPI.
   - The full-precision Parakeet and Whisper turbo models come with it.
   - Every file is pinned by URL, size and SHA-256 and downloaded like any model (resumable, verified).
   - NVIDIA's libraries are NVIDIA's software under NVIDIA's licence. OpenBark fetches them for the clinic, from NVIDIA, after the clinic ticks that it accepts the licence. OpenBark doesn't ship them.
3. **Installed beside the program,** where ONNX Runtime looks, with a manifest of what was placed (`gpu-pack-files.txt`). The processor-only `onnxruntime.dll`, loaded by the running program, is renamed aside (Windows allows that) and replaced; the GPU is used from the next start. Web mode gets **Restart now** (`POST /api/restart` starts a fresh copy of the server, which waits for the port) and the desktop app restarts itself.
4. **The GPU is never tried inside the program first.** ONNX Runtime can throw a C++ exception that Rust can't catch, which aborts the whole program; the first attempt did exactly that, over a missing cuDNN library. So:
   - after installing, and at each start, a child process (`<program> --openbark-gpu-check`) loads both full-precision models on the GPU and transcribes a second of silence;
   - only if it exits cleanly does the program use the GPU, and the result is cached in `gpu-check.txt`;
   - if the self-test fails, the Models screen says so, the processor stays in use, and **Try again** re-tests (say, after a driver update);
   - a recogniser that still fails to start on the GPU falls back to the processor.
5. **Which model runs where:** the full-precision recognisers are used only when the pack is installed and has passed its self-test. int8 models always stay on the processor. `OPENBARK_ASR_PROVIDER` forces a provider, for testing.
6. **Not now: macOS and Linux.** On Apple silicon the processor path is already quick, and Core ML through ONNX Runtime is unmeasured. Linux CUDA works the same way with `.so` files, but needs its own pins and a test machine.

## Consequences

- With the pack, an 8.5-minute import is ready in about 43 s instead of about 150 s, and the number check keeps pace instead of finishing afterwards.
- The pack takes about 1.9 GB beside the program and 5.7 GB of models. **Remove** takes it away again.
- The full-precision models use about 6 GB of graphics memory while transcribing. On a 16 GB card that's fine next to Gemma E4B; with Gemma 12B the note model loads after transcription finishes.
- The Windows uninstaller removes the pack's files, except on an update.
- New network hosts are listed in docs/network.md (R5).
- Tested end to end in isolation, with every CUDA folder removed from PATH:
  1. a clean program folder;
  2. pack install (106 to 146 s on the dev box's connection);
  3. self-test;
  4. restart;
  5. an 8.5-minute import in 43 s.
  A failing self-test left the server running and said so.
