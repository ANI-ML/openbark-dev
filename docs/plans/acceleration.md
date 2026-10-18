# Plan: acceleration on every platform

Status: **proposed**, with [ADR-0019](../adr/0019-acceleration-on-every-platform.md). Date: 2026-10-18.

## Goal

Every user gets the fastest path their computer can run, picked automatically, with a safe fallback. The screen always says where speech and notes run. When something faster exists, OpenBark says what it costs and downloads nothing without a click. A processor-only build can never ship again by accident.

## Where each step can be tested

| Tag | Meaning | What it can prove |
|---|---|---|
| WIN | The Windows dev box: RTX 4080 16 GB, and an i7-12700KF, a hybrid processor with 8 performance and 4 efficiency cores (20 threads, no integrated GPU). It also has a Parsec virtual display adapter | CUDA, Vulkan on NVIDIA, the NVIDIA pack, processor-only builds, hidden-GPU cases (`CUDA_VISIBLE_DEVICES=-1`, `GGML_VK_VISIBLE_DEVICES=` set but empty, which hides every Vulkan device; PowerShell and cmd delete an empty variable instead, so set it from Git Bash or the test harness), SSH sessions, performance-core detection and background throttling on a hybrid processor, leaving out virtual adapters |
| SANDBOX | Windows Sandbox on the dev box (Windows 11 Pro; turn the feature on first). No GPU driver | Whether the program starts without a Vulkan loader or GPU driver, and the "no graphics card" path on a clean Windows |
| WSL | WSL2 Ubuntu on the same box, with CUDA through the Windows driver | Linux builds, the Linux NVIDIA pack, Linux processor paths, missing `libvulkan.so.1`. Not native Linux driver behaviour |
| GH-LINUX | GitHub Actions `ubuntu-22.04`, no GPU | Builds, backend checks, a software Vulkan driver (Mesa lavapipe, package `mesa-vulkan-drivers`) for the Vulkan code path, processor timings on a shared runner. ggml-vulkan skips processor-type Vulkan devices by default, so lavapipe jobs must set `GGML_VK_VISIBLE_DEVICES=0` |
| GH-WIN | GitHub Actions `windows-latest`, no GPU | Builds, CUDA toolkit compile of `ggml-cuda`, backend checks |
| GH-MAC | GitHub Actions `macos-15` arm64, no usable Metal device (believed) | Builds and links Metal, arm64 ONNX Runtime processor timings |
| HW | Hardware we don't have: AMD or Intel GPU, a real Mac, a laptop (battery, iGPU plus dGPU), two discrete GPUs, an Arm PC, native Linux with NVIDIA | Real speed and stability. Only through a volunteer or pilot clinic sharing a saved check result |

Rule for every step: unit tests use fake hardware lists, so the decision logic and the copy are tested for AMD, Intel and Apple even though their speed is not.

## Steps

Order: first stop the silent slow path (steps 1-6), then make each platform faster (steps 7-12), then the longer evaluations (13-14).

### Step 0: baseline numbers (WIN)

- Run `scripts/bench.sh` with three builds of the CLI: processor only, `--features vulkan`, `--features cuda`. Models: Gemma 4 E4B and 12B. Same consults.
- Repeat speech with the pack on and off.
- Record in `docs/perf/acceleration-baseline.md`.
- SANDBOX: install today's Windows installer and record whether it starts without `vulkan-1.dll` (ADR-0019 context). If it doesn't, step 8 is also a fix for "won't start", not only for speed.
- GH-MAC: run `openbark probe` and record whether the runner exposes a Metal device. That decides what the macOS CI checks can claim.
- WIN: time a speech-only import with the window in front and then minimised behind another window, to see whether Windows' background throttling slows it (step 7).

Done when: we have measured times for the product owner's case (processor-only build, no pack) and the best case. No UI copy quotes a number that isn't in this file or in the user's own self-check.

### Step 1: hardware inventory (WIN, WSL, GH-MAC)

- New module (in `os-app` or a small `os-hw` crate) that lists graphics hardware without llama.cpp: DXGI on Windows, `/sys/class/drm` and `nvidia-smi` on Linux, `sysctl` and the Metal device on macOS. Fields: vendor, name, discrete or integrated, memory, driver version. Also physical cores and, where the OS reports them, performance cores: Windows `GetLogicalProcessorInformationEx` (`EfficiencyClass`), Linux `/sys/devices/cpu_core/cpus` against `cpu_atom`, macOS `hw.perflevel0.physicalcpu`.
- Leave out virtual and software adapters: the DXGI software flag, no dedicated memory, or a vendor id that isn't a GPU maker (Parsec, Microsoft Basic Render Driver, remote-desktop adapters).
- Detect x64-on-Arm emulation (`IsWow64Process2`) for the decision 6 message.
- Keep `device_type` and `memory_free` from llama-cpp-2 in `os_llm::devices()`.
- Prefer a discrete GPU over an integrated one in `probe_hardware`.
- A short-name function: "NVIDIA GeForce RTX 4080" to "RTX 4080", "Apple M3" to "Apple M3 GPU".
- Today `gpu_pack::nvidia_gpu()` runs only on Windows. Move NVIDIA detection here, so Linux (step 11) and the status share one source.

Tests:
- Unit: fake lists (RTX 4080 only; RTX 4080 plus Parsec virtual adapter; Radeon plus Ryzen iGPU; Intel iGPU plus Arc; Apple M1; no GPU; x64 under emulation) give the expected chosen device and short names.
- WIN: real output matches Device Manager, without the Parsec adapter, and reports 8 performance cores. Also run over SSH and from the desktop session, and record any difference (Vulkan and DXGI may not enumerate in a service session).
- SANDBOX: reports no GPU and the right cause.
- WSL and GH-MAC: the probe runs and reports something sensible.

### Step 2: acceleration report (WIN, WSL, GH-LINUX, GH-WIN, GH-MAC)

- `/api/status` and `openbark probe` (whose output is already JSON; there is no `--json` flag) gain: detected GPUs, and backends reported four ways: compiled (Cargo features), bundled (files on disk), registered (loaded by ggml) and devices (what each backend sees). Also the notes device in use, the speech provider and model actually in use (after any fallback), and thread counts. Add the same report to the desktop program behind `--openbark-probe`, like `--openbark-gpu-check`, so CI can check the binary the installer ships.
- Forward the `cuda`, `vulkan`, `metal` features to `os-app` so it can report what was compiled. Stop hard-coding `"sherpa-onnx (cpu)"` in `probe.rs`.
- Log the report once at start-up.
- Replace "no supported graphics card" with the true cause when a GPU was detected but can't be used.

Tests:
- Unit: each mismatch (GPU found and no backend; backend and no device; driver missing) maps to one cause.
- WIN: a processor-only build shows "This copy of OpenBark can't use your RTX 4080". A Vulkan build shows the 4080. `GGML_VK_VISIBLE_DEVICES=` shows the processor with the right reason.

### Step 3: CI and release guard rails (GH-LINUX, GH-WIN, GH-MAC)

- `release.yml` builds and attaches the `openbark` CLI per platform with the same features as the installer, with the web UI beside it in the layout `install.ps1 -From` already expects (`openbark.exe` and `ui\`).
- Every release and CI build job runs `openbark probe`, and in release jobs the desktop program's `--openbark-probe`. It fails if the expected backend is not compiled (before step 8) or not bundled and registered (after step 8): `vulkan` on Windows and Linux, `metal` on macOS. It never requires a device, because runners have no GPU.
- `ci.yml` builds the CLI with the platform's GPU feature, not without. That means installing the Vulkan SDK in the Windows CI job (`choco install vulkan-sdk`, `VULKAN_SDK` set, as `release.yml` does) and `libvulkan-dev glslc` in the Linux job.
- `install.ps1` and `install.sh`: auto mode detects a card, not `nvcc` or `VULKAN_SDK`. Windows uses `Get-CimInstance Win32_VideoController` (PowerShell can't call DXGI directly) or `nvidia-smi`; Linux uses `/sys/class/drm` or `nvidia-smi`. If a discrete or Apple GPU is present and the GPU build can't be done, stop with a clear message that names the release page for the prebuilt CLI. An integrated GPU alone builds for the processor and says so. The scripts don't download anything themselves (`docs/network.md`). `-Gpu cpu` still forces the processor.
- Fix README lines 130-136 (missing serve line, stray instruction).

Tests:
- A deliberate processor-only build in a test job makes the check fail (then revert).
- WIN: run `install.ps1` with the Vulkan SDK hidden from the environment and confirm it stops instead of building quietly for the processor.
- Unit (script level): the card detection treats the Parsec adapter as no card.

### Step 4: show where each stage runs (WIN, GH-LINUX for unit tests)

- Notes chip: `<model> · <device>` on Models and Review. Speech chip on Models, the recording screen and imports.
- Progress lines: "Transcribing on RTX 4080", "Writing note on the processor".
- Models screen: a "Where things run" card with one row per stage, backend and threads.
- `session.json` records device, provider, model and time per stage. Review shows "Transcribed in 14 s on RTX 4080. Note written in 6 s on RTX 4080."
- Strings in `i18n/messages.ts`, with a translation for every shipped locale (German, Spanish, French, Portuguese), as `GpuCard` has today. Light and dark themes. Phone width.
- Remove any path that could label speech or notes "MLX".

Tests:
- Component tests with fake status for NVIDIA, AMD, Intel, Apple, processor only, partial offload.
- WIN: real visits with the pack on and off show the right chips.

### Step 5: honest speech self-test and fallback (WIN, WSL)

- Ship the self-check clip here, not in step 12, because this check needs it. It is a 30-second synthetic consult from the Kokoro-voiced eval fixtures, about 1 MB, with its script.
- The child-process check (`--openbark-gpu-check`) transcribes the clip, not silence. It fails on empty output, on a word error rate against the clip's script above a threshold set from the processor's result, on "Fallback to cpu" in stderr, and when the GPU is slower than the processor. It doesn't require the processor's exact words, because full precision and int8 legitimately differ. Timing starts after one warm-up pass, so library start-up and PTX compilation aren't counted.
- Move the result from `gpu-check.txt` beside the program to the data folder, keyed by app version, GPU, driver version and pack contents. A changed key re-runs the check. The data folder is needed for the Linux pack anyway, because the program folder there is read-only.
- If a recogniser fails on the GPU at run time, rebuild it with the int8 model on the processor, mark the GPU unusable for this session, and show it. Today `OfflineAsr::with_threads` retries on the processor with the same full-precision model, so the fallback has to come back to `os-engine` to swap the model.
- Start-up race: a recording that starts before the check finishes switches its next segments to the GPU once the check passes, or the import waits up to a few seconds for the check.
- Detect the pack dropped by an app update and offer **Reinstall**. This is a download with its size shown, because the archives are deleted after unpacking.
- Reject `OPENBARK_ASR_PROVIDER=coreml` and any provider the build can't honour, with a clear log line.

Tests:
- WIN: remove one cuDNN file from a test copy of the pack; the check fails with a named reason and the app uses int8 on the processor.
- WIN: force the slower-than-processor rule with the int8 model on CUDA; the check rejects it.
- WIN: change the recorded driver version in the cached result; the next start re-runs the check.
- Unit: the stderr parser catches sherpa-onnx's fallback messages; the word-error comparison passes the processor's and the GPU's real transcripts of the clip and fails an empty or shuffled one.

### Step 6: faster-option offers (WIN, GH-LINUX for unit tests)

- Generalise `GpuCard.tsx` into one card that covers every platform and every situation in ADR-0019 decision 6. It shows on Mac, Linux, AMD and Intel too, even when the only message is "speech runs on the processor".
- Show the NVIDIA offer in first-run setup and once after a slow visit, not only on Models. Size and licence on the button. Download only on click, licence tick unchanged.
- Fix the hosts in the copy. Today the card says NVIDIA's libraries "come from NVIDIA", but they download from PyPI (`files.pythonhosted.org`, R5). Name GitHub, the Python Package Index (NVIDIA's own packages) and Hugging Face. The size always comes from `download_bytes`, never a fixed figure.
- Rename "GPU speech pack" to "NVIDIA GPU pack" in copy once step 9 adds notes to it.
- Speed claims: the measured 43 s against 2.5 minutes only for NVIDIA, labelled as measured on an RTX 4080. Otherwise the user's own self-check numbers, or no number.

Tests:
- Component tests for each situation with fake status.
- WIN: with the pack removed, the offer appears in setup, on Models and after a visit; dismissing it sticks.
- Check `docs/network.md` still matches every download button.

### Step 7: processor threads per machine (WIN, WSL, GH-MAC, GH-LINUX)

- Thread counts from physical cores, and performance cores on Apple silicon (`hw.perflevel0.physicalcpu`) and hybrid Intel.
- One budget across speech, number check and speaker separation when they run together.
- llama.cpp threads set from physical performance cores when notes run on the processor. Add `OPENBARK_LLM_THREADS`.
- While work runs, opt out of Windows background power throttling (`SetProcessInformation`, `ProcessPowerThrottling`, execution speed), next to the existing `AwakeGuard` in `os-engine/src/awake.rs`. On macOS, check that the guard's IOKit assertion keeps App Nap off during a background import. If it doesn't, add `NSProcessInfo.beginActivity`.

Tests:
- Unit: fake core counts (20 logical x86 with 8P+4E, 4 cores 8 threads, M1 4P+4E, M3 Pro 6P+6E) give the expected split and never oversubscribe.
- WIN: re-run ADR-0014's thread sweep; the new rule must not be slower than today's 6 threads. The dev box is hybrid (8P+4E), so this also tests the performance-core rule.
- WIN: repeat step 0's minimised-window import with the throttling opt-out; it must be no slower than with the window in front.
- WSL with 4 and 8 processors (`.wslconfig`), GH-MAC and GH-LINUX: time Parakeet at the old and new counts. Record in `docs/perf`.

### Step 8: llama.cpp dynamic backends and memory fitting (WIN, WSL, GH-LINUX, GH-WIN, GH-MAC)

- Turn on `dynamic-backends` in `os-llm` for Windows and Linux only. macOS keeps Metal compiled in (ADR-0019 decision 7). `dynamic-backends` turns on `dynamic-link`, so `llama`, `ggml` and `ggml-base` become shared libraries that must ship too.
- Before `LlamaBackend::init`: call `load_backends_from_path(<backends>/cpu)`, a folder holding only the `ggml-cpu-*` variants, so ggml picks the best one. Then load each GPU backend file on its own with `llama_cpp_sys_2::ggml_backend_load(path)`, only if its cached child-process check passed and its recorded llama.cpp commit matches the program's. Never point `load_backends_from_path` at a folder that holds GPU backends: it loads all of them in the main process.
- `<backends>` is the Tauri resource folder for the desktop app and the program's folder for the CLI archive. Declare the libraries as `bundle.resources` in `tauri.conf.json` (none are declared today) and check where each format puts them: NSIS, AppImage, `.deb`, `.rpm`. On Linux, give the libraries an `$ORIGIN` run path.
- Bundle `llama`, `ggml`, `ggml-base`, all `ggml-cpu-*` variants and `ggml-vulkan` in the Windows and Linux installers and CLI archives.
- Change `scripts/ci/check-symbols.sh` to count `ggml_init` across the program and its bundled libraries. After this step the program file alone has none, and the check as written fails.
- Replace `gpu_layers = u32::MAX` with `fit_params`. Use the chosen device's free memory, with the speech models' measured peak share as that device's margin when the NVIDIA pack is active. Fit for the largest context OpenBark requests: `generate` creates the context per request, so the KV cache isn't allocated at load. Pin exactly one device with `with_devices` and split mode `None`, so CUDA and Vulkan never split the model across the same card.
- Child-process notes check (`--openbark-llm-check`): load the chosen model on the candidate device, generate a few tokens through the JSON grammar, check the output. Cache per backend, device, driver, app version, llama.cpp commit and model. Fallback: CUDA, then Vulkan or Metal, then processor.
- Guard against degenerate output (runs of `<unused>` tokens) in normal generation.

Tests:
- WIN: Vulkan build loads the 4080. Delete `ggml-vulkan.dll` from a test folder; notes fall back to the processor and the screen says why. Gemma 12B with the speech models loaded fits by partial offload and says so. Watch dedicated memory in Task Manager while it runs: shared GPU memory must not grow (the driver's system-memory fallback).
- SANDBOX: the installer starts and drafts on the processor, with no Vulkan loader present.
- GH-LINUX: in a container without `libvulkan.so.1`, the app starts and drafts on the processor. With lavapipe and `GGML_VK_VISIBLE_DEVICES=0`, the Vulkan device is found and the self-test runs (speed not judged).
- GH-WIN, GH-LINUX: the bundles contain every backend file, the desktop program's `--openbark-probe` lists them as registered, and `check-symbols.sh` finds one ggml.
- GH-MAC: the Metal build is unchanged and `openbark probe` lists `metal` as compiled and registered.
- Check that the right processor variant loads on the dev box (AVX2; the i7-12700KF has no AVX-512) and on the CI runners.
- A `ggml-cuda` file with a different recorded commit is not loaded, and the app says why.

### Step 9: CUDA for notes in the NVIDIA pack, Windows (GH-WIN, WIN)

- Build `ggml-cuda.dll` in CI from the same llama.cpp commit, with the CUDA 12.9 toolkit to match the pack's cudart 12.9 and cuBLAS 12.9. Architectures 75, 80, 86, 89, 120 plus PTX; measure the size with 61 and 70 added and decide. Add a build-time row to `docs/network.md` for the toolkit download in CI.
- Publish it on OpenBark's public releases repo, one file per llama.cpp commit. Pin URL, size, SHA-256 and the commit in `gpu_pack.rs`, and record the commit in the pack's manifest so step 8 can refuse a stale file. Measure its size and put the true total in the offer.
- An app update that changes the llama.cpp commit leaves the old file unused and shows the decision 6 offer for the new one. The update fetches nothing. The same offer covers clinics that installed the pack before this step: the extra file is offered with its size, never fetched on its own.
- Add `ggml-cuda` (MIT, built by OpenBark) to `gpu_pack::entry()`'s licence line and to `NOTICE`.
- Update `docs/network.md` R5 and ADR-0014's pack description.

Tests:
- WIN: with the pack, notes run on CUDA ("RTX 4080 (CUDA)"); without it, on Vulkan. Compare with step 0's numbers.
- WIN: a corrupted `ggml-cuda.dll` fails the notes check and Vulkan is used.
- WIN: a pack manifest without the CUDA notes file shows the offer, and starting the app makes no network calls.

### Step 10: faster Mac speech on the processor (GH-MAC)

- In CI, build sherpa-onnx v1.13.8 static against an arm64-only static ONNX Runtime 1.28.2. That archive is published: `onnxruntime-osx-arm64-static_lib-1.28.2.zip` (about 20 MB) on `github.com/csukuangfj/onnxruntime-libs` release v1.28.2, the same file sherpa-onnx's own `cmake/onnxruntime-osx-arm64-static.cmake` pins. Building ONNX Runtime from source is only the fallback if it disappears. That cmake file also defines `SHERPA_ONNX_DISABLE_COREML`, so this build still has no Core ML. Point `SHERPA_ONNX_LIB_DIR` (honoured by `sherpa-onnx-sys` 1.13.8) at the result for the macOS build. Add every source it downloads to the build-time table in `docs/network.md` (B3 or a new row).
- Time Parakeet int8 on the runner with the current library and the arm64-only one. Same transcript required.

Done when: adopted only if it is faster on the runner with identical output. If it isn't, record the numbers and keep the current library.

What we can't prove: speed on M1 to M5. The runner is one virtual machine. The user's own self-check shows their number.

### Step 11: NVIDIA GPU pack for Linux (WSL, GH-LINUX)

- Link sherpa-onnx `shared` on Linux, so a CUDA build of ONNX Runtime can be loaded. Done on 2026-10-18 (ADR-0001 amendment). It also fixed a start-up crash.
- Pin sherpa-onnx's `cuda-12.x-cudnn-9.x-onnxruntime1.28.2-linux-x64-gpu` archive and NVIDIA's manylinux runtime, cuBLAS and cuDNN wheels.
- Install into the user's data folder, not beside the program (`.deb` and `.rpm` installs are root-owned, and the AppImage is read-only). Load from there. "Load from there" needs a mechanism, because the program is linked to `libsherpa-onnx-c-api.so` and `libonnxruntime.so` and the loader resolves them before `main` runs. When the pack is present and its check passed, OpenBark re-runs itself once at start-up with `LD_LIBRARY_PATH` putting the pack folder first. ONNX Runtime then finds its CUDA provider beside the CUDA build of `libonnxruntime.so`. The child-process checks use the same environment. The CUDA notes backend, if added for Linux, carries an `$ORIGIN` run path to find the pack's cudart.
- `gpu_pack.rs` today is Windows-shaped: `.dll` names, files beside the program, and the `onnxruntime.dll` size check. Give it a per-platform file list and install folder rather than branching inside each function.
- Detect NVIDIA on Linux through `/proc/driver/nvidia` or `libcuda.so.1` and `nvidia-smi`. Widen `gpu_pack::supported()`.
- Measure the true download size before the offer goes live. Update `docs/network.md` R5.

Tests:
- WSL: install, self-test, an import of the 8.5-minute consult, remove. Compare with Windows.
- GH-LINUX: the pack's file list and hashes verify; without a GPU the offer doesn't appear and nothing breaks.

What we can't prove: native Linux NVIDIA driver behaviour. Release notes say "tested in WSL2 only" until a native machine confirms.

### Step 12: the self-check (WIN, WSL, GH-MAC, GH-LINUX)

- Reuse the clip shipped in step 5.
- Run once after setup, in the background, in a child process. Rerun only when the key changes (app version, GPU, driver, packs, models) or on **Check again**. Pause if a recording starts.
- Measure each speech and notes candidate and two or three thread counts. Store `acceleration.json` in the data folder. Feed steps 2, 5, 6, 7 and 8 from it.
- Show "This computer: a 10-minute visit takes about N s to transcribe and N s to write up".
- **Save check result** shows the contents, then writes the file where the user chooses. Nothing is sent. The file has no computer name, user name, folder paths or clinic data (ADR-0019 decision 10).
- Candidates that aren't installed (no pack, no CUDA notes file) are listed as "not installed". The check never downloads them.

Tests:
- WIN: first run creates the file; a second start does not rerun; removing the pack reruns; the estimate is within 25% of a real 10-minute import.
- WSL, GH-MAC, GH-LINUX: the check finishes on the processor and writes a valid file.
- Offline test: the check makes no network calls.
- Unit: the saved file contains none of the computer name, user name or data-folder path.

### Step 13: spike, one engine for GPU speech on Metal and Vulkan (WIN, GH-MAC, GH-LINUX, then HW)

- Try transcribe.cpp (MIT) for Parakeet v3 and Whisper turbo behind the recogniser trait. Two weeks at most.
- Measure on WIN through Vulkan and CUDA against the current paths. Check word error against the eval fixtures. Check correctness with lavapipe in GH-LINUX and on GH-MAC.
- Work out the ggml clash with llama.cpp (loadable backends or one shared pin).
- Write the result as `docs/spikes/S3-gpu-speech-engine.md`.

Adopt only by a new ADR, and only if it beats the processor on WIN through Vulkan and on at least one real Mac or AMD machine (HW). Until then, AMD, Intel and Mac speech stays on the processor and the UI says so.

### Step 14: battery and power notice (WIN, later)

- Detect on-battery and low-power mode (Windows `GetSystemPowerStatus`, macOS IOKit, Linux `/sys/class/power_supply`).
- One line under the chips: "On battery: your computer may slow the graphics card and processor."

Test: WIN only proves the "plugged in" path. Laptops: HW.

## What we can't verify, and how we stay honest

| Combination | What we can do | What we say |
|---|---|---|
| AMD or Intel GPU, notes (Vulkan) | CI build, lavapipe code path, self-test with fallback | Support table: "built and checked in CI, not measured". No speed figures in the UI except the user's own |
| AMD or Intel GPU, speech | Nothing to accelerate yet | "Speech runs on the processor. OpenBark can't use Radeon cards for speech yet." |
| Apple silicon, notes (Metal) | Build and link on GH-MAC | "Built for Apple silicon GPUs. Speed measured on your Mac by the self-check" |
| Apple silicon, speech | Processor timings on GH-MAC | Chip says "processor". Never "MLX", "Metal" or "Neural Engine" unless that is what runs |
| Hybrid processors (performance and efficiency cores) | WIN (i7-12700KF) for Intel desktop; unit tests for Apple and laptop chips | Thread counts from the self-check |
| Laptops with iGPU plus dGPU | Unit tests with fake device lists | Self-test decides. Copy names the card actually used |
| Two discrete GPUs | Unit tests only. Notes are pinned to one card | Copy names the card used. Splitting across cards isn't offered until someone can measure it |
| Windows on Arm (x64 under emulation) | Unit test of the emulation check | "Running under emulation, much slower". No native build yet |
| Native Linux with NVIDIA | WSL2 only | Release notes: "tested in WSL2" |
| Intel Macs | Nothing | Website and README: not supported by the installers; build from source for the processor |

Also:
- The support table goes in `docs/guide` and on the website, with a "how we know" column: measured on our hardware, built in CI only, or untested.
- We ask pilot clinics with AMD, Intel or Mac machines to use **Save check result** and send the file if they are willing. Each file we receive moves a row in the table. Nothing is collected automatically.

## Order and rough size

| Step | Size | Depends on |
|---|---|---|
| 0 Baseline | 1 day | None |
| 1 Hardware inventory | 2-3 days | None |
| 2 Acceleration report | 2 days | 1 |
| 3 CI and installer guard rails | 2 days | 2 |
| 4 Chips and "Where things run" | 3 days | 2 |
| 5 Speech self-test and fallback (ships the clip) | 4 days | 2 |
| 6 Faster-option offers | 2 days | 4, 5 |
| 7 Thread tuning | 2 days | 1 |
| 8 Dynamic backends and memory fitting | 1 week | 2 |
| 9 CUDA notes in the NVIDIA pack | 3 days | 6, 8 |
| 10 Mac arm64 ONNX Runtime | 2 days | None |
| 11 Linux NVIDIA pack | 1 week | 5 |
| 12 Self-check | 1 week | 5, 7, 8 |
| 13 GPU speech engine spike | 2 weeks | 12 |
| 14 Battery notice | 1 day | 4 |

Steps 1 to 6 alone would have shown the product owner, on the first visit, that notes and speech were on the processor and why. They come first.

## Risks

| Risk | Mitigation |
|---|---|
| Dynamic backend files go missing from a bundle | Step 3's probe check fails the release |
| A GPU driver crashes the program | Every GPU path is tried first in a child process; the result is cached and retried only on change |
| The self-check takes too long on slow machines | Time limit per candidate; skip GPU candidates that failed before; show expected time |
| A Vulkan driver gives wrong output silently | Notes self-test checks JSON validity; degenerate-output guard in normal generation |
| GitHub's macOS runner differs from real Macs | Treat runner numbers as a lower bound for "does it work", not as user-facing speed |
| CUDA build in CI is slow or flaky | Cache the toolkit; build `ggml-cuda` only on tags and by hand |
| A `ggml-cuda` file from an older release crashes a newer app | Recorded llama.cpp commit; the main process never loads a mismatched file; checked in a child process first |
| Graphics memory over-committed on Windows NVIDIA; the driver silently uses system RAM | Budget from measured peaks with a margin; step 8 test watches shared GPU memory |
| A background import is throttled onto efficiency cores | Power-throttling opt-out while working (step 7) |
