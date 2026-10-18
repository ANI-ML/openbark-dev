# ADR-0019: The fastest working path on every computer, and saying where each stage runs

- Status: proposed
- Date: 2026-10-18
- Deciders: product owner (asked that every user get the speed their hardware allows, on Apple, NVIDIA, AMD or other graphics cards and on every operating system), engineering
- Amends: ADR-0014 decisions 1 (thread counts) and 6 (Linux deferred). Leaves ADR-0009 (MLX only after measurement) standing.
- Plan: [docs/plans/acceleration.md](../plans/acceleration.md)

## Context

### What happened

On the product owner's RTX 4080, a 7-minute recording took 5 minutes to become a note. Two things went wrong, and nothing on screen said so:
1. The `openbark serve` binary had been built without graphics support, so Gemma wrote the note on the processor.
2. The GPU speech pack (ADR-0014) was not installed, so transcription also ran on the processor.

The Models screen even said "no supported graphics card". The card was there; that copy of OpenBark just couldn't use it.

### How a slow build happens today

| Cause | Where in the repo |
|---|---|
| Graphics backends are compile-time Cargo features (`cuda`, `vulkan`, `metal`), off by default. One binary carries one GPU backend or none. | `engine/os-llm`, `engine/os-cli`, `apps/desktop/src-tauri` Cargo.toml |
| No release job builds the `openbark` CLI (and so `openbark serve`). CI builds it without features on Windows and Linux. | `.github/workflows/release.yml`, `ci.yml` |
| `install.ps1` auto mode picks CUDA only if `nvcc` is on PATH, Vulkan only if `VULKAN_SDK` is set, else the processor, and prints one quiet line. It checks for developer tools, not for a graphics card. `install.sh` is similar. | `install/install.ps1` lines 63-73, `install/install.sh` lines 66-76 |
| The hardware probe lists only devices the compiled backends can see, so a processor-only build reports "no supported graphics card". It takes the first GPU it finds, which can be an integrated one. | `engine/os-app/src/lib.rs` `probe_hardware`, `engine/os-llm/src/lib.rs` `devices()` |
| Gemma always tries to put every layer on the GPU (`gpu_layers = u32::MAX`). Nothing checks free graphics memory or what the speech models already use. | `engine/os-llm/src/llama.rs` |
| The processor build of llama.cpp is fixed at AVX2. AVX-512 is never used, and a processor without AVX2 would crash. | `llama-cpp-sys-2` build cache |
| The desktop installers already compile Vulkan (Windows, Linux) or Metal (macOS) in. On Windows and Linux that links the program to the Vulkan loader (`vulkan-1.dll`, `libvulkan.so.1`). A computer without a graphics driver may not have that file, for example some virtual machines and remote desktops, and the program may then fail to start at all. Not yet confirmed (plan step 0). | `.github/workflows/release.yml`, `llama-cpp-sys-2` `build.rs` (`rustc-link-lib=vulkan-1`) |
| The GPU speech self-test result is one word ("ok" or "failed") in `gpu-check.txt` beside the program. It isn't tied to a driver, GPU or app version, so a driver update that breaks CUDA is never re-tested. On macOS and Linux the program folder is read-only, so the file can't live there. | `engine/os-models/src/gpu_pack.rs` `check_file()`, `engine/os-app/src/lib.rs` `verify_gpu` |
| The speech self-test transcribes one second of silence. It proves the libraries load, not that the output is right or faster. | `engine/os-app/src/lib.rs` `gpu_check_main` |
| Windows 11 can move a background program's threads to efficiency cores (EcoQoS) on hybrid Intel processors. Keeping the computer awake (`SetThreadExecutionState`) doesn't stop that, so an import left running behind another window may slow down. Not yet measured. | `engine/os-engine/src/awake.rs` |

### Speech recognition today

| Platform | What runs | Notes |
|---|---|---|
| Windows x64, NVIDIA, pack installed | Full-precision Parakeet and Whisper turbo on CUDA | Measured on the RTX 4080: whole 8.5-minute import 43 s instead of 152 s (ADR-0014) |
| Everything else | Parakeet int8 on the processor | Including Linux with NVIDIA, every AMD and Intel card, and every Mac |

Other findings:
- **The GPU card only appears for Windows plus NVIDIA.** Mac, Linux, AMD and Intel users are told nothing about where speech runs.
- **Silent fallbacks.** sherpa-onnx logs "Fallback to cpu" and still creates the recogniser when a provider is missing. The app then believes it is on the GPU. If the recogniser does fail on the GPU, it retries on the processor with the full-precision model, which is the slowest combination.
- **Start-up race.** Until the GPU self-test passes, a recording uses the processor for the whole visit.
- **Core ML is a no-op on our Mac build.** sherpa-onnx compiles Core ML out of its static macOS library (`SHERPA_ONNX_DISABLE_COREML`). `OPENBARK_ASR_PROVIDER=coreml` logs "coreml" and runs on the processor.
- **The Mac library is probably slower than it needs to be.** sherpa-onnx makes its arm64 library by thinning a universal2 build. OpenWhispr measured the arm64-only ONNX Runtime about 2.9 times faster for Parakeet on an Apple M5 (230 ms against 665 ms for an 11 s clip, identical output). That was the shared library; ours is static and not yet measured.
- **Linux CUDA can't work as linked.** Linux uses sherpa-onnx's static, processor-only ONNX Runtime, so the CUDA provider check in `cuda_pack_installed()` never takes effect there. (Since 2026-10-18 Linux links it shared, per the ADR-0001 amendment. The ONNX Runtime it ships is still processor-only.)
- **Thread counts were tuned on one 20-thread machine.** A third of logical processors, clamped to 2-6. An 8-core M1 or a 4-core laptop gets 2 threads, which ADR-0014's own table shows is about twice as slow as 6. llama.cpp on the processor uses its default of 4 threads on any machine.
- **The number check (Whisper turbo) is the most expensive stage** wherever there is no GPU speech path.

### What the options are, per vendor

| Option | Platforms | Evidence | Verdict |
|---|---|---|---|
| CUDA through ONNX Runtime (current pack) | Windows, Linux, NVIDIA | 3.5 times faster whole import on the RTX 4080. int8 is slower on CUDA (missing DynamicQuantizeLinear kernel), so full precision only | Keep. Add Linux |
| Core ML through ONNX Runtime | macOS | Preview status; no LSTM or int8 ops in its supported-operator lists. Field reports for Parakeet: the TDT export fails to load under Core ML (external-data weights), and the CTC int8 export ran about 1.8 times slower than the processor on an M1 (CrispASR); a report on an M5 says Core ML doesn't support Parakeet's nodes and shapes | Reject |
| DirectML through ONNX Runtime | Windows, any DX12 GPU | Sustained engineering only, packages frozen at ORT 1.24.4, no sherpa-onnx prebuilt, recompiles per input length, no Parakeet measurement anywhere | Not now |
| WebGPU plugin through ONNX Runtime | All | Needs a sherpa-onnx patch for the new registration API; unmeasured for Parakeet | Not now |
| ROCm or MIGraphX | Linux, AMD | ROCm EP removed in ORT 1.23. MIGraphX is Linux only, needs ROCm installed, and ROCm supports only a short list of Radeon cards (RX 7700 to 7900 and RX 9060 to 9070 series). sherpa-onnx has no MIGraphX provider and no prebuilt | Reject |
| ggml speech engine (transcribe.cpp, MIT) | Metal, Vulkan, CUDA | Parakeet v3 Q8_0: M4 Max 182-215 times real time on Metal against 35-38 on the processor; Ryzen 4750U iGPU 24-26 on Vulkan against 12-15 | Evaluate (spike), adopt only on measurement |
| FluidAudio (Neural Engine) | macOS | About 190 times real time on M4 Pro (its README); Swift, Apple platforms only, a second stack | Not now |
| MLX for speech | macOS | No usable path from Rust or sherpa-onnx today | Reject for now |

| Option for drafting (llama.cpp) | Evidence | Verdict |
|---|---|---|
| Vulkan | Runs on NVIDIA, AMD and Intel with the normal driver. On llama.cpp's public scoreboards (Llama 2 7B Q4_0, RTX 4080 Super and RTX 4090), generation matches CUDA and prompt processing is about 13-27% behind. Those figures use NVIDIA's coopmat2 Vulkan extension, which needs a recent driver; without it prompt processing falls much further behind | Default on Windows and Linux |
| CUDA | Fastest on NVIDIA. Needs cudart and cuBLAS, which the GPU speech pack already installs | Optional, inside the NVIDIA pack |
| Metal | Already shipped. M5 tensor units are turned on automatically | Default on Apple silicon |
| ROCm (HIP) | About 240 MB per platform (upstream release archives). Upstream's Windows build targets RDNA1 to RDNA4 (gfx1010 to gfx1201). Open Gemma 4 bug on HIP (`<unused49>` loops, gfx1151 on Windows) and other open HIP wrong-output reports | Reject for now |
| SYCL (Intel) | Recent corruption bugs; Vulkan is close enough on Arc | Reject for now |
| MLX | Third-party comparisons mostly favour mlx-lm for small dense models (for example Qwen3-4B 4-bit on an M4 Max: 159 against 118 tokens per second for llama.cpp, Groundy via Starmorph). We found no primary-source Gemma 4 comparison, and none of these runs used constrained JSON output | ADR-0009 stands: measure first |

llama.cpp can load its backends at run time (`GGML_BACKEND_DL`). The pinned `llama-cpp-sys-2` 0.1.160 has this as the `dynamic-backends` feature. It also builds every processor variant (on x86-64, from a plain x86-64 baseline through SSE4.2, AVX2 and AVX-512 to AMX) and picks the best one at start-up (`GGML_CPU_ALL_VARIANTS`). Upstream llama.cpp ships its Windows, Linux and Android releases this way; its macOS releases keep Metal compiled in. A backend file that is missing, or whose libraries are missing (no NVIDIA driver, no Vulkan loader), is skipped, not fatal. A backend that crashes while it starts (a broken driver) still takes the whole program down, and `ggml_backend_load_all_from_path` loads every backend it finds in the folder. So OpenBark must not point it at a folder that holds untested GPU backends (decision 2).

Bundled backend files must come from the same llama.cpp commit as the program. ggml checks an API version when loading, but not the full binary interface. A `ggml-cuda` file left over from an older OpenBark can crash a newer one.

### Constraints

- Local-first. Every network call is listed in `docs/network.md`. Downloads only on the user's request or with clear consent.
- No telemetry. Any measurement stays on the computer unless the user exports it.
- Honest copy. Don't say "accelerated" unless it is. Don't quote speed-ups we haven't measured.
- Brand guide §7.6: always show where inference runs.
- Test hardware: one Windows box with an RTX 4080 and a hybrid Intel i7-12700KF (8 performance cores, 4 efficiency cores, 20 threads, no integrated GPU). It also shows a virtual display adapter (Parsec). Plus WSL2 on the same box, Windows Sandbox (a Windows 11 Pro feature, still to be turned on; it has no GPU driver), and GitHub Actions (macOS arm64 and Ubuntu, no usable GPU). No AMD, Intel or Apple GPU, no laptop.

## Decision

### 1. Know the hardware separately from the build

OpenBark lists the computer's graphics hardware itself, independent of what was compiled in:
- Windows: DXGI adapters (vendor id, name, dedicated memory, driver version).
- Linux: `/sys/class/drm/card*/device` (vendor, VRAM where the driver reports it), plus `nvidia-smi` when present.
- macOS: chip name, performance and efficiency core counts (`sysctl`), and the Metal device.

Virtual and software adapters (Microsoft Basic Render Driver, remote-desktop and streaming adapters such as Parsec) are left out. The rule is a software flag, no dedicated memory, or a vendor id that isn't a GPU maker.

It then compares that list with the devices llama.cpp and the speech engine can actually use. Every mismatch has a named cause and a fix (section 6). Discrete GPUs are preferred over integrated ones, matching llama.cpp's own rule.

It also notes when the x64 program is running under emulation on an Arm PC (Windows `IsWow64Process2`). Everything is much slower there, and the user is told so (decision 6).

### 2. Pick the fastest working path automatically, with a safe fallback

For each stage (speech, number check, notes), OpenBark builds a list of candidates from the hardware and the installed backends, fastest first. A candidate is used only after a **self-test in a child process** passes, because a GPU library can abort the whole program in a way Rust can't catch. The self-test checks three things:
1. it runs without crashing;
2. the output is right. For speech, the transcript of a shipped clip with a known script must be close to that script, within a word-error threshold set from the processor's own result on the same clip. An exact match with the processor isn't possible, because the GPU runs the full-precision model and the processor runs int8. For notes, the model must produce valid JSON through the grammar;
3. it is faster than the next candidate down, timed after a warm-up run. The first run includes library start-up and, on cards newer than the compiled code, PTX compilation, so it isn't counted. A GPU that is slower than the processor is not used.

The self-test also fails if sherpa-onnx prints "Fallback to cpu". Results are kept in the data folder, never beside the program, keyed by app version, GPU, driver version, backend and model. If a candidate fails at run time, OpenBark drops to the next one, with the matching model (int8 on the processor, never full precision), and says so.

The main process loads a GPU backend only after that backend has passed its check in a child process. llama.cpp's processor variants live in their own folder, which is loaded whole. Each GPU backend file is loaded on its own by path (`ggml_backend_load`), and only if its cached check passed. A backend from a different llama.cpp commit than the program is never loaded.

Fallback order:

| Stage | Order |
|---|---|
| Speech and number check | CUDA (full precision) > processor (int8) |
| Notes | CUDA > Vulkan or Metal > processor |

The graphics memory budget for notes uses llama.cpp's `fit_params` on the device's free memory, minus what the speech models will hold (passed as that device's margin). The speech share is the peak measured during the self-check, not the model files' size, because ONNX Runtime's CUDA memory pool grows beyond the weights. The fit is for the largest context OpenBark asks for: the context is created per request, so memory must be left for the longest consult, not only for loading. Notes use exactly one GPU (`with_devices`, no split). Otherwise llama.cpp could split the model across two backends that see the same card, CUDA and Vulkan. This matters on Windows NVIDIA cards: when graphics memory runs out, the driver's default "system memory fallback" moves memory into normal RAM with no error, just a large slowdown. So the budget, not the driver, must keep the total under the card's memory. If the whole model doesn't fit, OpenBark offloads what fits and says "partly on RTX 3060, partly on the processor". Integrated GPUs are used for notes only if they pass the self-test, beat the processor, and have enough shared memory for the model. The memory floor starts at 8 GB and is tuned once we have real results.

### 3. Transcription, per platform

| OS | GPU | Out of the box | Faster option, with consent | Fallback | How we know |
|---|---|---|---|---|---|
| Windows x64 | NVIDIA | Parakeet int8 on the processor, threads tuned per machine | **NVIDIA GPU pack** (today's speech pack, renamed): CUDA with full-precision Parakeet and Whisper. About 1.9 GB of libraries plus 5.7 GB of models. NVIDIA's licence for the CUDA libraries; models CC-BY-4.0 (Parakeet) and MIT (Whisper) | Processor, int8 | Measured on the RTX 4080 (ADR-0014) |
| Windows x64 | AMD | Processor | None yet. A ggml Vulkan engine is evaluated (decision 9) | Processor | Processor path only. No AMD hardware |
| Windows x64 | Intel Arc or integrated | Processor | None yet, as AMD | Processor | Processor path only. No Intel GPU |
| Windows x64 | None | Processor | None | None needed | Dev box with the GPU hidden; Windows Sandbox (no driver) |
| Linux x64 | NVIDIA | Processor | **NVIDIA GPU pack for Linux**: sherpa-onnx's CUDA build (393 MB) plus NVIDIA's runtime libraries. Total size to be measured before the offer goes live. Installed in the user's data folder, not beside the program. Same licences as Windows | Processor, int8 | WSL2 on the RTX 4080. Native Linux with an NVIDIA card: not tested |
| Linux x64 | AMD or Intel | Processor | None yet, as Windows AMD | Processor | Processor path only |
| macOS | Apple silicon | Processor, with an arm64-only ONNX Runtime and threads from the performance cores. No download | None in this ADR. Metal through a ggml engine is evaluated (decision 9) | Processor | Speed on the macOS runner's processor. No real Mac |
| macOS | Intel | Processor (build from source) | None | None needed | Not tested |
| Windows on Arm | Any | Not supported. The x64 installer may run under Windows emulation, untested. If it does, OpenBark says it is running under emulation and will be slow (decision 6) | None | None | Not tested. Emulation detection: unit test only |

sherpa-onnx and llama.cpp both publish Windows arm64 builds, so a native Arm build is possible later. It is out of scope here, because we have no Arm PC to test on.

Voice activity detection and speaker separation stay on the processor everywhere. They are small, and their cost is shown per visit.

`coreml` is removed as an accepted value of `OPENBARK_ASR_PROVIDER`, because our build can't honour it. A provider the build can't honour is rejected with a clear log line instead of silently running on the processor.

### 4. Notes (drafting), per platform

| OS | GPU | Out of the box | Faster option, with consent | Fallback | How we know |
|---|---|---|---|---|---|
| Windows x64 | NVIDIA | Vulkan, in the installer | CUDA, as one more file in the NVIDIA GPU pack. It reuses the CUDA runtime the pack already installs. About 100-250 MB extra, MIT code built by us; size to be measured. Cards older than the compiled architectures (decision 7) fail the self-test and stay on Vulkan | Vulkan, then processor | Dev box: CUDA and Vulkan |
| Windows x64 | AMD | Vulkan | None (no ROCm) | Processor | Build in CI, Vulkan code path with a software driver. Speed on AMD not tested |
| Windows x64 | Intel Arc | Vulkan | None (no SYCL) | Processor | As AMD |
| Windows x64 | Integrated only | Processor. Vulkan on the iGPU only if the self-test passes, it beats the processor, and the model fits in shared memory | None | Processor | Not tested |
| Windows x64 | None | Processor, best variant for the chip (AVX2 or AVX-512), threads from physical cores. E2B or E4B suggested with an honest time estimate | None | None needed | Dev box with the GPU hidden; Windows Sandbox (no driver, no Vulkan loader) |
| Linux x64 | NVIDIA | Vulkan (NVIDIA driver) | CUDA in the Linux NVIDIA pack, after the speech part works | Vulkan, then processor | WSL2 for CUDA; Vulkan on native Linux not tested |
| Linux x64 | AMD | Vulkan (Mesa RADV) | None | Processor | CI with a software Vulkan driver |
| Linux x64 | Intel | Vulkan (Mesa ANV) | None | Processor | As AMD |
| Linux x64 | None, or no Vulkan library | Processor | None | None needed | Ubuntu CI without `libvulkan.so.1` |
| macOS | Apple silicon | Metal (M5 tensor units automatic), compiled in as today | None. MLX only if ADR-0009's measurement shows it clearly faster | Processor | Builds and links on the macOS runner. Metal speed needs a real Mac |
| macOS | Intel | Processor (build from source) | None | None needed | Not tested |

One installer carries all the backends for its platform (decision 7). The user never has to pick a build.

### 5. Show where each stage runs (brand guide §7.6)

Chips use the brand pill. The device name replaces "local"; it is local by definition. When OpenBark is served from another computer (the Clinic Hub, later), the host name comes first.

| Where | Chip or line | Examples |
|---|---|---|
| Notes model chip (Models, Review) | `<model> · <device>` | "gemma-4-e4b · RTX 4080", "gemma-4-e4b · Apple M3 GPU", "gemma-4-e4b · processor" |
| Speech chip (Models, recording, import) | `<model> · <device>` | "parakeet · RTX 4080", "parakeet · processor" |
| Progress line while working | Plain sentence | "Transcribing on RTX 4080", "Writing note on Apple M3 GPU", "Writing note on the processor" |
| Partial offload | Plain sentence | "Writing note partly on RTX 3060, partly on the processor" |
| Models screen, "Where things run" | One row per stage, with backend and threads | "Notes: RTX 4080 (CUDA)", "Speech: processor, 8 threads", "Number check: processor, added 40 s to the last visit" |

Device names are short: "RTX 4080", "Radeon RX 7800 XT", "Arc B580", "Apple M3 GPU". The backend (CUDA, Vulkan, Metal) goes in the details row, not the chip. The words "MLX" and "Neural Engine" are never used for something that doesn't run on MLX or the Neural Engine. Today, nothing does.

Each visit records which device and model ran each stage, and how long each stage took, in `session.json`. Review shows "Transcribed in 14 s on RTX 4080. Note written in 6 s on RTX 4080."

### 6. Tell the user when something faster exists, and what it costs

When the hardware could do better than what is running, OpenBark says why and what to do. It shows these on the Models screen, in first-run setup, and once after a visit that was slow for this computer. Each can be dismissed.

| Situation | Message (draft) | Action |
|---|---|---|
| GPU present, build can't use it | "This copy of OpenBark can't use your RTX 4080, so notes are written on the processor." | Link to the right installer |
| NVIDIA card, pack not installed | "Your RTX 4080 can do speech recognition with the NVIDIA GPU pack. Measured on an RTX 4080: an 8.5-minute consult was ready in 43 seconds instead of 2.5 minutes. It downloads {size} from GitHub, the Python Package Index (NVIDIA's own packages) and Hugging Face. NVIDIA's libraries are under NVIDIA's licence." `{size}` is the server's `download_bytes`, never a fixed number | **Download** after the licence tick (unchanged consent) |
| Pack installed, not restarted | "Restart OpenBark to start using the graphics card." | **Restart now** |
| Self-test failed | "Your graphics card couldn't run speech recognition, so the processor is used. This can follow a driver update." | **Try again** |
| Pack removed by an app update | "The update replaced a file the GPU pack needs." | **Reinstall**. This is a new download, and the button shows its size: `gpu_pack::unpack` deletes the archives after unpacking. Copying from the program folder isn't possible either, because the update overwrote the files there |
| Pack installed before CUDA notes existed, or its CUDA notes file is from an older OpenBark | "Notes can also be written with CUDA on your RTX 4080. This adds {size} to the NVIDIA GPU pack." | **Download**. Nothing is fetched by the update itself. Until the user clicks, notes use Vulkan |
| x64 OpenBark running under emulation on an Arm PC | "OpenBark is running under Windows' x64 emulation on this computer, so transcription and notes will be much slower than on an x64 PC." | None. Honest only |
| GPU found, driver missing | "Your Radeon RX 7800 XT was found, but its graphics driver isn't available to OpenBark. Installing or updating the driver lets notes be written on it." | Link to driver help |
| AMD, Intel or Apple GPU and speech on the processor | "Speech runs on the processor. OpenBark can't use Radeon cards for speech yet. Notes are written on your Radeon." | None. Honest only |
| On battery (later) | "On battery: your computer may slow the graphics card and processor." | None |

The 43-second figure (about 3.5 times faster for a whole import) is quoted only for NVIDIA, where it was measured, and is labelled as measured on an RTX 4080. The message names the download hosts truthfully: NVIDIA's libraries come from PyPI (`files.pythonhosted.org`), not from an NVIDIA server (`docs/network.md` R5). Once the self-check has run, estimates come from this computer's own numbers.

No message starts a download. Every download needs a click, shows its size and licence, and has a row in `docs/network.md`.

### 7. Release builds carry the right backends, and CI proves it

- The Windows and Linux installers are built with `dynamic-backends`: all processor variants plus Vulkan, as separate libraries. Today's program needs the Vulkan loader just to start. After this change it starts without one and drafts on the processor. The libraries are declared as Tauri bundle resources. OpenBark finds them through the app's resource folder (desktop) or beside the program (CLI archive), so they land in the right place in the NSIS install, AppImage, `.deb` and `.rpm`. On Linux the backends carry an `$ORIGIN` run path. The Vulkan loader is a recommended package, not a required one, where the package format allows that.
- macOS keeps Metal compiled in, as today. Every Mac the installer supports (Apple silicon) has Metal. Loadable backends would add dylib signing, notarisation and run-path work to the app bundle for no gain. The cost is one fixed processor build, and that matters little because notes run on Metal.
- `ggml-cuda` is built in CI from the same llama.cpp commit, with the CUDA 12.9 toolkit to match the cudart and cuBLAS 12.9 the pack installs. Architectures: 7.5 to 12.0 plus PTX. The file size decides whether to add 6.1 and 7.0 (GTX 10-series, Titan V). Cards that aren't covered fail the self-test and use Vulkan. It is not in the installer, because it needs NVIDIA's runtime. It is published on OpenBark's public releases repo and fetched as part of the NVIDIA GPU pack. It is pinned by URL, size and SHA-256, and recorded with its llama.cpp commit so a mismatched file is never loaded (decision 2).
- The release workflow also builds the `openbark` CLI (for `openbark serve`) per platform, with the same backends, and attaches it to the release. Web-mode users stop building from source.
- Every release and CI job runs `openbark probe` (its output is already JSON) on the CLI. It also runs the desktop program with a new `--openbark-probe` argument, because the installer ships the desktop program, not the CLI. The job fails if an expected backend is not bundled and registered. It never requires a GPU device, because CI runners have none. A processor-only build can't ship again.
- `scripts/ci/check-symbols.sh` (one ggml, ADR-0001) is changed to count across the program and its bundled libraries, since loadable backends move ggml out of the program file.
- `install.ps1` and `install.sh` detect a graphics card, not a developer kit. They stop and say so if a discrete or Apple GPU is present and the GPU backend can't be built, unless the user passes `-Gpu cpu`. An integrated GPU alone doesn't stop the build, because notes on an iGPU are only used after a self-test anyway (decision 4). The scripts only point to the prebuilt CLI; they don't download it. `docs/network.md` says the install scripts make no network calls of their own.

### 8. Tune processor threads per machine

- Count physical cores, and on Apple silicon and hybrid Intel chips, performance cores only.
- Share one budget across the stages that run together (speech, number check, speaker separation), so a 4-core laptop isn't asked for 11 threads.
- When notes run on the processor, give llama.cpp the physical performance cores instead of its fixed default of 4.
- The self-check (decision 10) tries two or three counts around the estimate and keeps the fastest.
- `OPENBARK_ASR_THREADS` and `OPENBARK_SECOND_THREADS` stay as overrides. Add `OPENBARK_LLM_THREADS`.
- While transcribing or drafting, OpenBark opts out of Windows' background power throttling (`SetProcessInformation` with `ProcessPowerThrottling`, execution speed). Otherwise an import behind another window can be moved to efficiency cores. The opt-out ends when the work ends, alongside the existing keep-awake guard. On macOS the keep-awake guard's IOKit power assertion is expected to keep App Nap away. That is checked, not assumed.

### 9. Evaluate one engine for GPU speech on Metal, Vulkan and AMD

A time-boxed spike tries transcribe.cpp (MIT, ggml) for Parakeet v3 and Whisper turbo:
- correctness and speed on the RTX 4080 through Vulkan and CUDA;
- correctness in CI with a software Vulkan driver, and on the macOS runner;
- ggml version clash with llama.cpp (ADR-0001 avoided two copies), handled with loadable backends or a shared pin.

It is adopted, by a new ADR, only if it beats the processor on the dev box through Vulkan and on at least one real Mac or AMD machine. Until then AMD, Intel and Mac speech stays on the processor, and the UI says so. New model downloads it needs (GGUF builds of Parakeet and Whisper) would get their own network row.

### 10. A self-check that runs once and stores the result

- **When:** once, in the background, after first-run setup finishes. Again only when the app version, GPU, driver, packs or chosen models change, or when the user clicks **Check again**. It pauses if a recording starts.
- **What:** a 30-second synthetic consult clip (Kokoro voice, from our eval fixtures, about 1 MB, shipped with the app) through each speech candidate, and a short fixed prompt through the chosen notes model on each notes candidate. About a minute on a GPU machine; it says how long it expects.
- **Result:** stored in `acceleration.json` in the data folder: hardware, candidates, pass or fail with reason, speed (real-time factor, tokens per second), chosen path and thread counts. It drives decisions 2, 6 and 8.
- **Shown** on the Models screen as "This computer: a 10-minute visit takes about 50 s to transcribe and 15 s to write up".
- **Never sent anywhere.** **Save check result** writes the file to a place the user picks, so a clinic can choose to share it with us. This is how we will learn about AMD, Intel and Mac machines without telemetry. The file holds only hardware, driver and OS versions, OpenBark's version, model ids and timings. It holds no computer name, user name, folder paths, clinic or patient data, and no transcript text (the self-check clip is synthetic anyway). The app shows the contents before saving.
- **Network:** none. The clip ships with the app, and the check only uses models and backends already on disk. A candidate that would need a download is listed as "not installed", never fetched.

## Consequences

- The product owner's case can't happen silently: a processor-only build or a missing pack is named on screen, and CI refuses to ship a build without its GPU backend.
- AMD and Intel users get notes on their GPU through Vulkan from the installer, and are told plainly that speech runs on the processor.
- Mac users get a faster processor path for speech at no download cost, if the arm64 library measures faster, and honest labels. "MLX" stays a Hugging Face label only.
- Linux users with NVIDIA can get the same speech speed-up as Windows users.
- Windows and Linux installers grow by the Vulkan and processor-variant libraries (tens of MB). Their program no longer needs the Vulkan loader to start. The NVIDIA pack grows by the CUDA backend file.
- More moving parts: dynamic backend loading, a second self-test, a Linux pack, and per-machine thread tuning. Each has a fallback to today's behaviour.
- We still can't measure AMD, Intel or Apple GPUs ourselves. Paths we haven't measured are protected by the self-test and fallback, and the UI quotes no speeds for them, only the user's own measured numbers. The support table in the docs marks each combination as measured, built in CI only, or untested.
- `docs/network.md` changes: R5 covers the CUDA notes backend (from OpenBark's public releases repo on `github.com`, already a listed host) and the Linux pack. No new run-time hosts unless decision 9 is adopted. Build-time rows are added for the CUDA toolkit download in CI and for the arm64-only ONNX Runtime used on macOS (plan step 10), if adopted.
- `NOTICE` and the pack's licence line gain `ggml-cuda` (MIT, built by OpenBark from llama.cpp).

## Alternatives considered

| Option | Why not |
|---|---|
| Ship one installer per GPU vendor | Users would have to know their hardware and pick. The product owner's problem was exactly a wrong build. Dynamic backends make one installer enough |
| Loadable backends on macOS too | Every supported Mac has Metal, so nothing would be chosen at run time. It would add dylib signing, notarisation and run-path work to the app bundle |
| Point `load_backends_from_path` at the whole program folder | It would load every GPU backend in the main process, including an untested or stale `ggml-cuda`, and a crash there takes the app down. Untested GPU backends are loaded one by one, only after a child-process check |
| Put CUDA in the main installer | Needs NVIDIA's runtime libraries, which we can't ship under our licence and which are useless without an NVIDIA card. The pack already fetches them with consent |
| Download the faster option automatically when a GPU is found | Breaks the consent rule. Offers stay click-to-download |
| Core ML on the Mac (shared sherpa-onnx library) | Evidence says it is slower or broken for Parakeet. Not worth changing the link mode for |
| DirectML for AMD and Intel on Windows | Frozen at ONNX Runtime 1.24, needs a custom sherpa-onnx build, recompiles per input length, no measurement anywhere, and we can't test it |
| ROCm or HIP for AMD | Removed from ONNX Runtime; HIP for llama.cpp is large (about 240 MB) and has an open Gemma 4 bug. Vulkan is close or faster for generation in most reports (one open report shows a large Vulkan slowdown on an RX 9070 XT, so the self-test still decides) |
| MLX runtime for notes now | No evidence it beats llama.cpp Metal for Gemma 4. ADR-0009's measure-first rule stands |
| Anonymous speed reports sent home | Telemetry. The user can save and share the check result if they choose |
| Keep the fixed thread rule and let users tune with environment variables | Clinic staff won't. The self-check does it for them |
