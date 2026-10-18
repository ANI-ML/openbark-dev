# ADR-0001: Inference libraries: llama.cpp for LLMs, sherpa-onnx for all audio models

- Status: **accepted** (configuration A of spike S2; performance numbers in `docs/perf/M0.md`)
- Date: 2026-10-17
- Deciders: product owner (approved the change from the master prompt on 2026-10-17); engineering

## Context

The master prompt assumed three native libraries in one Rust engine: llama.cpp for LLMs, whisper.cpp for speech-to-text, and sherpa-onnx for diarization. Research and spike S2 found:

1. **llama.cpp and whisper.cpp can't share a binary through the maintained crates.**
   - `llama-cpp-sys-2` and `whisper-rs-sys` each statically link their own copy of ggml, and the two copies have different ABIs (`GGML_TYPE_COUNT` is 42 in one and 40 in the other).
   - The link fails with duplicate symbols, and forcing it caused heap corruption (llama-cpp-rs #484 and PR #1091).
   - The fix on the whisper side (Codeberg PR #260) is unmerged. whisper-rs hasn't had a release since March 2026, and it bundles whisper.cpp 1.8.3 while upstream is at 1.9.5.
2. **sherpa-onnx ships official Rust crates** (1.13.8). They cover offline ASR (Whisper, Parakeet TDT, Moonshine, Qwen3-ASR and more), Silero VAD, speaker diarization and TTS, all on ONNX Runtime. ONNX Runtime has no ggml, so it doesn't collide with llama.cpp.
3. **Measured on Windows:** sherpa-onnx's prebuilt *static* Windows libraries use the static C runtime (`/MT`), while llama.cpp builds with the dynamic one (`/MD`). The link fails with 70+ `LNK2038 RuntimeLibrary mismatch` errors (`target/logs/build-cpu-static.log` during the spike). **Shared mode** (sherpa-onnx DLLs next to the executable) links and runs.

## Decision

- **LLMs** run through `llama-cpp-2 =0.1.160` (llama.cpp v0.6.0): GGUF models on CUDA, Metal, Vulkan or CPU.
- **Every audio model** runs through `sherpa-onnx =1.13.8`: speech-to-text (Parakeet TDT v3 is the default; Whisper is served as an ONNX export), Silero VAD, and pyannote segmentation plus speaker-embedding diarization.
- **One ggml and one ONNX Runtime per binary.** CI checks this with `scripts/ci/check-symbols.sh`.
- **The link mode is decided once,** in `engine/os-asr/Cargo.toml`, and every other crate reaches sherpa-onnx through `os_asr::sherpa_onnx`:
  - **shared** on Windows (the CRT mismatch), Android and iOS (sherpa only ships shared libraries for mobile);
  - **static** on macOS and Linux.
- **Not used:**
  - `whisper-rs`: the duplicate-ggml problem above;
  - `ort`: a second ONNX Runtime;
  - `sqlx` / `tauri-plugin-sql`: they conflict over `libsqlite3-sys`;
  - `hf-hub`: no resumable downloads.

## Consequences

**Benefits:**
- One inference stack per modality.
- No forks of upstream crates.
- sherpa-onnx's model zoo (Moonshine, Qwen3-ASR, MedASR, SenseVoice) is available without new native dependencies.
- llama.cpp's audio path (`mtmd`) stays open for the "audio-native" experiment.

**Costs:**
- On Windows we ship `sherpa-onnx-c-api.dll`, `sherpa-onnx-cxx-api.dll`, `onnxruntime.dll` and `onnxruntime_providers_shared.dll` next to the executable. The installer must include them (M5).
- The sherpa-onnx prebuilt libraries are CPU builds. GPU speech-to-text would mean building sherpa-onnx with CUDA or CoreML ourselves. We'll only consider that if speech-to-text turns out to be the bottleneck, and on the reference box it isn't.
- whisper.cpp's Metal and CoreML acceleration on Macs isn't available. S2 configuration B can revisit this if Parakeet on the CPU misses the budget on M-series machines.
- `sherpa-onnx-sys` downloads its prebuilt archive at build time. That download is listed in `docs/network.md` as B3, and CI caches it.

## Alternatives considered

| Option | Why not |
|---|---|
| Patch `whisper-rs-sys` to link llama.cpp's ggml (PR #260) | We'd maintain a fork of a quiet crate and keep two upstream ggml revisions in lockstep. Revisit only if configuration B shows a large Mac speed-up. |
| Run whisper.cpp in a separate process | It works, but adds IPC and packaging cost for no gain over sherpa-onnx. |
| Build one ggml ourselves (`system-ggml`) | Still needs the unmerged whisper-rs-sys patch. |
| Build sherpa-onnx from source with `/MD` | Possible later. Shared mode solves the problem today with no build cost. |
