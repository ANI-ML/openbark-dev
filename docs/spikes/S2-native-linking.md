# Spike S2: linking llama.cpp and speech libraries on every target

Status: **designed, not started**. Time-box: **6 working days**.

Outputs:
- `docs/adr/0001-inference-libraries.md`
- the real `engine/` workspace. Unlike S1, this spike turns into the M0 engine rather than being thrown away.
- a CI matrix that stays green.

## The question

Can one Rust binary contain all of the following, build reproducibly on every target, and still meet the latency budget?
- LLM inference: llama.cpp, through `llama-cpp-2`
- speech-to-text
- voice activity detection (VAD)
- speaker diarization

## What we already know, and what it changes

The master prompt assumed `llama.cpp + whisper.cpp + sherpa-onnx`. Research on 2026-10-17 found:

1. **llama.cpp and whisper.cpp can't currently be linked into one binary through the maintained crates.** `llama-cpp-sys-2` and `whisper-rs-sys` each statically build their own copy of ggml, and the two copies have different ABIs (`GGML_TYPE_COUNT` is 42 in one and 40 in the other). The result:
   - duplicate-symbol errors at link time;
   - `STATUS_HEAP_CORRUPTION` at runtime when the link is forced.

   The fix on the whisper side, Codeberg PR #260 ("System-ggml"), is unmerged. `whisper-rs` hasn't had a release since March 2026, and it bundles whisper.cpp 1.8.3 while upstream is at 1.9.5.
2. **sherpa-onnx now ships official Rust crates** (`sherpa-onnx` 1.13.8). They cover:
   - ASR models, including Whisper, Parakeet TDT and Moonshine;
   - Silero VAD;
   - pyannote segmentation plus speaker embeddings for diarization.

   It uses ONNX Runtime, not ggml, so **it doesn't collide with llama.cpp.**
3. **Two ONNX Runtimes would collide in turn.** Adding the `ort` crate (ORT 1.31) next to sherpa-onnx's bundled ORT 1.28.2 risks this.

**Proposed architecture to test:** `llama-cpp-2` handles every LLM. sherpa-onnx handles every audio model: STT, VAD and diarization. The binary has one ggml and one ONNX Runtime.

whisper.cpp **isn't dropped as a model**. Whisper still runs, as an ONNX export through sherpa-onnx. What's dropped is the whisper.cpp **library**, unless configuration B below shows it's worth the extra build complexity.

## Configurations

| ID | What's linked | Purpose | Required? |
|---|---|---|---|
| **A** | `llama-cpp-2 =0.1.160` + `sherpa-onnx =1.13.8` | Proposed default | **Yes**, on every target |
| **B** | A + `whisper-rs-sys` patched with PR #260, linked against llama-cpp-sys-2's ggml (`DEP_LLAMA_GGML_CMAKE_DIR`, from llama-cpp-rs PR #1091), with both pinned to the same ggml revision | Find out whether whisper.cpp on Metal or CoreML is fast enough to justify carrying a patched fork | macOS arm64 only, and only if A's STT on Mac misses budget |
| **C** | A + an audio LLM through llama.cpp's multimodal path (the `mtmd` feature): `ggml-org/Qwen3-ASR-0.6B-GGUF`, and Gemma 4 E4B audio input fed in ≤30 s chunks | A single-ggml route to ASR, and the "audio-native" path from master prompt §4.4 | Probe only, on Windows CUDA and macOS. Gemma 4 audio support in llama.cpp is **not confirmed**. |

## Targets

| Target | Runner | Backend | Must run end-to-end in M0? |
|---|---|---|---|
| Windows x64 + CUDA 12.9 | **this dev box** (RTX 4080 16 GB, i7-12700KF, 64 GB RAM); CI builds without running | llama.cpp CUDA; sherpa CPU or CUDA | **Yes**, and this is the performance reference |
| Windows x64 Vulkan | CI build; run on this box with `--backend vulkan` | llama.cpp Vulkan | Yes (smoke test) |
| macOS arm64 | GitHub `macos-15` (Apple Silicon) | Metal | **Yes** |
| macOS x86_64 | cross-compiled on the arm64 runner | CPU / Metal | Build only |
| Linux x64 | `ubuntu-24.04` | CPU (CUDA build-only via the toolkit container) | Yes on CPU, inside a **network-less namespace** (offline test) |
| Android arm64 | CI with NDK, then a physical device from S1 | Vulkan / CPU | Build in M0; run during S1 |
| iOS arm64 | `macos-15` + Xcode | Metal | Build in M0; run during S1 |

## Steps

1. **Workspace.** Create `engine/os-engine`, `os-llm`, `os-asr`, `os-diar` and `os-models` with configuration A only. Each crate wraps its library behind a trait (`LlmBackend`, `AsrBackend`, `Diarizer`) that also reports what the backend can do, so the UI never knows which library served a request (master prompt §4.2).
2. **Link check.** After building, list the binary's symbols (`dumpbin /symbols` on Windows, `nm` elsewhere) and assert that exactly one copy of the `ggml_*` symbols is present. Do the same check for ONNX Runtime by looking for `onnxruntime` in each OS's dependency listing. This check becomes a CI test.
3. **The over-4-GB GGUF test.** On MSVC, load Gemma 4 12B QAT Q4_0 (6.98 GB). This tests llama-cpp-rs issue #951. If it fails:
   - try `dynamic-link` against an official llama.cpp v0.6.0 DLL;
   - otherwise patch the crate, or find an upstream fix.
4. **CLI.** The full M0 milestone command is:

   ```
   openscribe transcribe consult.wav --stt parakeet --diarize
     | openscribe note --template soap --llm gemma-4-e4b
   ```

   - `transcribe` emits JSONL: segments with speaker, start and end times, and text.
   - `note` emits the SOAP note as JSON (validated against `packages/templates/soap.schema.json`) plus rendered Markdown.
   - Structured output comes from llama.cpp's grammar or JSON-schema constraint, so we never parse free text.
5. **Model fetch.** `openscribe models pull <catalog-id>` is a minimal ranged-GET downloader. It does SHA-256 verification against the pinned sha in `docs/versions.md`, and it is the only network code path in the CLI.
6. **Build-time network.** `sherpa-onnx-sys` downloads its prebuilt libraries during the build. CI fetches them once, checks the checksum, caches them, and passes `SHERPA_ONNX_LIB_DIR`. Both the build-time and run-time hosts get listed in `docs/network.md`.
7. **Benchmark.** Use the three synthetic consults: 5, 10 and 20 minutes, two or three speakers, rendered locally (see the M0 plan, task 7). Record:
   - STT: real-time factor (processing time ÷ audio length) and word error rate, overall and on a veterinary term list;
   - diarization error rate;
   - LLM: time to first token, tokens/s and total note time;
   - peak RSS / VRAM.

   Run E4B and 12B on the Windows CUDA box and on M-series Mac CI. CI runners have no GPU, so Mac CI numbers are indicative only. The 60-second budget is judged on Apple Silicon hardware we own or borrow.
8. **Configurations B and C** run on days 5–6, and only if A is green.

## Pass / fail

| # | Criterion | Pass |
|---|---|---|
| L1 | Configuration A builds from a clean checkout on all 7 targets in CI | Green |
| L2 | One ggml and one ONNX Runtime in every binary | The symbol test passes |
| L3 | Gemma 4 12B (6.98 GB GGUF) loads on Windows MSVC | Loads and generates |
| E1 | Full pipeline on Windows CUDA: 10-minute consult to SOAP JSON | Valid against the schema; **≤60 s wall clock** with E4B and with 12B |
| E2 | Same pipeline on macOS arm64 | Valid; time recorded (judged against 60 s on M-series hardware) |
| E3 | Full pipeline offline: Linux run with no network namespace, after models are fetched | Passes |
| E4 | No invented vitals or doses in any of the three synthetic notes | Zero, checked by script against a list of values present in the gold transcript |

## Decision rule

- **A passes:** ADR-0001 adopts it. whisper.cpp stays out of the engine, and Whisper models are served through sherpa-onnx.
- **A passes and B shows whisper.cpp ≥2× faster on Mac at equal WER:** record B as a *future* macOS-only optimisation that needs PR #260 upstreamed first. We don't carry a fork in M0.
- **C works well, meaning Qwen3-ASR or Gemma audio gets WER within 10% of Parakeet at acceptable speed:** keep it as an experimental pipeline flag (master prompt §4.4). It doesn't replace the default.
- **A fails on a platform:** fix it inside the time-box if possible. Otherwise isolate the failing library in a separate process (`os-asr-worker` over stdio) on that platform, and record that in the ADR.
