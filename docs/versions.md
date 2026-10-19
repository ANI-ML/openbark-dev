# Version pins for M0

Status: **proposed**. Checked live on 2026-10-17. Nothing is pinned in code until the M0 plan is approved.

How these were checked: the crates.io API, the npm registry (`dist-tags.latest` plus the publish time), GitHub and Codeberg release APIs, the Hugging Face model API (`/api/models/<id>`, which returns the license tag, gating and the `main` sha), and vendor docs. Each row names its source. "Verify at scaffold" means that I did not confirm the item live and will check it before it goes into a lockfile.

Pinning policy:
- Use exact versions in the pnpm catalog and `Cargo.toml` (`=x.y.z`) for anything native or anything released in the last 30 days.
- Use `~x.y` where we want patch updates.
- Pin models by Hugging Face commit sha, never by `main`.

---

## 1. Toolchain

| Tool | Pin | Source | Notes |
|---|---|---|---|
| Rust | **1.99.1** (stable, 2026-10-15) | static.rust-lang.org/dist/channel-rust-stable.toml; blog.rust-lang.org/releases | Higher than every dependency's minimum Rust version (tauri 1.90, cpal 1.85). Pin it in `rust-toolchain.toml`. **Not installed on the dev box yet.** |
| Node.js | **24.22.0** LTS "Krypton" | nodejs.org/dist/index.json; nodejs/Release schedule.json | The dev box has 24.14.1. Upgrade it, because pnpm 11+ and Expo 58 need ≥24.3 and some pnpm 12 settings need ≥24.18. v24 moves to Maintenance on 2026-10-20 and v26 becomes Active LTS on 2026-10-28. **Plan: move to 26 at M1.** |
| pnpm | **11.28.6** (via the `packageManager` field and corepack) | registry.npmjs.org/pnpm; pnpm v10, v11 and v12 release notes | The dev box has 9.15.4. I'm choosing 11 over 12.14.0 because 12's notes mention a Rust CLI that I couldn't confirm and its strictness changes are new. 11's defaults are good for supply-chain safety: `minimumReleaseAge` of 24 hours, and dependency build scripts denied unless allowed with `allowBuilds`. Revisit 12 at M1. |
| Turborepo (`turbo`) | **2.11.9** | registry.npmjs.org/turbo | Released 2026-10-16. It stays blocked by pnpm's 24-hour release-age rule until 2026-10-17. |
| TypeScript | **~6.0.3** | registry; Microsoft TS 7 announcement | TS 7.0.2 is the native Go compiler and has **no programmatic API**, so typescript-eslint and other tooling break. Stay on 6.0 until TS 7.1. |
| CMake | the copy bundled with VS 2022 Build Tools (already installed) | local check; llama.cpp `docs/build.md` | ggml needs ≥3.14 and the CUDA backend needs ≥3.18. Record the exact version at scaffold. |
| MSVC | VS 2022 Build Tools 17.14 (installed) | local `vswhere` | llama.cpp's docs ask for the C++ workload plus the **Clang** component. Clang is not installed yet. |
| LLVM / libclang | verify at scaffold | — | Needed if `llama-cpp-sys-2` runs bindgen at build time. Install it on the dev box and the CI images. |
| NSIS (via tauri-cli) | **3.13**, plus `nsis_tauri_utils` **0.5.3** | tauri-cli 2.12.3 downloads both on the first `--bundles nsis` build and checks their hashes (build log) | Builds the Windows setup wizard. The template in `apps/desktop/src-tauri/windows/installer.nsi` is tauri-cli 2.12.3's, with one change (the install folder); refresh it when tauri-cli moves. |
| GPU speech pack (ADR-0014) | ONNX Runtime **1.28.2** CUDA build from sherpa-onnx **v1.13.8** (`...cuda-12.x-cudnn-9.x-onnxruntime1.28.2-win-x64-cuda.tar.bz2`, SHA-256 matches GitHub's asset digest); `nvidia-cuda-runtime-cu12` **12.9.79**, `nvidia-cublas-cu12` **12.9.2.10**, `nvidia-cudnn-cu12` **9.27.0.42** (2026-09-29) | GitHub release API; PyPI JSON API | Downloaded on the clinic's request, never shipped. Pinned by URL, size and SHA-256 in `engine/os-models/src/gpu_pack.rs`. Unpacked with `bzip2` 0.5.2, `tar` 0.4.46 and `zip` 2.6.1, the versions already in Cargo.lock (zip 9.0.5 was a day old). |
| MLX engine pack (ADR-0025) | CPython **3.12.15** from python-build-standalone **20261009** (`cpython-3.12.15+20261009-aarch64-apple-darwin-install_only.tar.gz`, 25,165,569 bytes, SHA-256 `83dcb04777c798bd95f3687acf9ae84b09860272ee6bdfb05be8bcf9447f75b8`; PSF-2.0, build scripts MPL-2.0); `mlx`, `mlx-metal` and `mlx-cpu` **0.32.3** (MIT, 2026-09-29), `mlx-lm` **0.32.0** (MIT, 2026-10-01), `llguidance` **1.9.1** (MIT, 2026-09-30), `transformers` **5.19.0** (Apache-2.0, 2026-10-06), `tokenizers` **0.23.3** (Apache-2.0, 2026-10-09), and their dependencies. Their licences, as the packages declare them: MIT, BSD-2-Clause, BSD-3-Clause, Apache-2.0, ISC (shellingham), PSF-2.0 (typing-extensions), MPL-2.0 AND MIT (tqdm), Apache-2.0 AND CNRI-Python (regex), and numpy's BSD-3-Clause AND 0BSD AND MIT AND Zlib AND CC0-1.0. All allow redistribution; MPL-2.0 is file-level copyleft, which a pack downloaded unchanged on request meets. [mlx-pack-pins.md](mlx-pack-pins.md) has each package's licence | PyPI JSON API and the release's `SHA256SUMS` (checked live 2026-10-19: none yanked; newer `mlx` 0.32.4, `mlx-lm` 0.32.2 and `transformers` 5.20.1 exist but fall inside the release-age window) | Downloaded on the clinic's request from the MLX engine card (Apple silicon, macOS 14 or later), never shipped. Pinned by URL, size and SHA-256 in `engine/os-models/src/mlx_pack.lock.json` (and `mlx_pack.linux.lock.json`, the Linux CPU build with `mlx-cpu`, for the developer test mode only). **Release age:** the lock generator only takes releases at least 7 days old (`--exclude-newer`, cut-off recorded in the lock; this lock's is 2026-10-11), longer than pnpm's 24 hours, because this code runs outside Cargo's and pnpm's lockfiles. The full list of 35 wheels, with sizes and hashes, is in [mlx-pack-pins.md](mlx-pack-pins.md), generated by `scripts/mlx-pack-lock.py`. |
| uv (build machine only) | **0.13.1** (MIT OR Apache-2.0, 2026-10-14) | PyPI JSON API (checked 2026-10-19) | Runs `scripts/mlx-pack-lock.py` to resolve the MLX engine pack's wheels. Never shipped or run on a clinic's computer. |
| Vulkan SDK, Linux (glslc only, release builds) | **1.4.363.0** (`vulkansdk-linux-x86_64-1.4.363.0.tar.xz`, 366,564,872 bytes, SHA-256 `197962f5cbf80baf2775a03336a01cee7c8745686c65aaa70d3f751ade4d7e43`) | vulkan.lunarg.com/sdk/latest/linux.txt; LunarG's published checksum, checked live on 2026-10-18 | Ubuntu 22.04 has no glslc package, and ggml's Vulkan backend needs it at build time. The release workflow verifies the checksum, unpacks the SDK and points CMake at it (glslc, current Vulkan headers and the SPIRV-Headers CMake package that ggml's Vulkan backend now requires). |
| create-dmg | **1.3.0** (2026-07-02) | GitHub API, create-dmg/create-dmg releases/latest | Lays out the macOS disk image with **Uninstall OpenBark** beside the app (`scripts/macos-dmg.sh`). Downloaded by tag, not via Homebrew, so the version is pinned. |
| CUDA Toolkit | **12.9** (installed) | llama.cpp `ggml-cuda/CMakeLists.txt` has an explicit ≥12.9 branch | CUDA 13 drops the Maxwell, Pascal and Volta architectures in non-native builds, so stay on 12.x for wider GPU support. |

## 2. Rust crates (engine)

| Crate | Pin | Source | Why / caveat |
|---|---|---|---|
| `tauri` / `tauri-build` / `tauri-cli` | **~2.12** (2.12.4 / 2.7.3 / 2.12.3) | crates.io API | 3.0.0-alpha.5 shipped the same day, so the `~` pin stops us drifting onto 3.x. |
| `llama-cpp-2` + `llama-cpp-sys-2` | **=0.1.160** | crates.io; utilityai/llama-cpp-rs releases | Vendors llama.cpp **v0.6.0 / b11429** (2026-10-05). Gemma 4 support was merged upstream in PR #21309 (April 2026). Features we use: `cuda`, `vulkan`, `metal`, and `mtmd` for audio/vision probing. Open issue to test: **#951, GGUF files over 4 GB failing on MSVC**. That affects Gemma 4 12B (7 GB) and up. |
| `sherpa-onnx` + `sherpa-onnx-sys` | **=1.13.8** (official k2-fsa crates) | crates.io; k2-fsa/sherpa-onnx releases v1.13.8 | ASR (Whisper, Parakeet, Moonshine and other ONNX models), Silero VAD, and diarization (pyannote segmentation plus a speaker-embedding model). All of these are confirmed in `c-api.h`. **The build script downloads prebuilt libraries** unless `SHERPA_ONNX_LIB_DIR` is set, and iOS/Android linking is shared-library only. Bundles **ONNX Runtime 1.28.2**. Link mode (ADR-0001): shared on Windows, Linux, Android and iOS; static on macOS. Since 2026-10-18 Linux x64 uses `sherpa-onnx-v1.13.8-linux-x64-shared-lib.tar.bz2` (9,816,899 bytes, SHA-256 `3892d184be41027e18165e67f549cd4e4cdd8dcd73ac5579e97afd55e14e30b6`, matches GitHub's asset digest) instead of the `-static-lib` archive, and ships `libsherpa-onnx-c-api.so` and `libonnxruntime.so` beside the program. |
| `rusqlite` | **=0.40.2**, features `bundled-sqlcipher-vendored-openssl` | crates.io; libsqlite3-sys 0.38.2 source | SQLCipher **4.14.0** on SQLite 3.51.3. Not needed in M0 except for the schema stub. It is pinned now because of the conflict noted below. |
| `cpal` | **=0.18.2** | crates.io; cpal README | PCM capture on desktop, iOS (CoreAudio) and Android (AAudio, API 26+). It does **not** set up AVAudioSession or background modes; spike S1 covers that. |
| `chacha20poly1305` | **=0.11.0** (aead 0.6) | crates.io (checked 2026-10-17); Apache-2.0 OR MIT | Encryption at rest (ADR-0008): XChaCha20-Poly1305 with random 24-byte nonces. |
| `keyring` | **=4.2.0** (default `v1` feature) | crates.io (checked 2026-10-17); MIT OR Apache-2.0; rust-version 1.88 | Data key in Windows Credential Manager, macOS Keychain or Secret Service. Over SSH on Windows there's no logon session (ERROR_NO_SUCH_LOGON_SESSION), so the key file fallback is used. |
| `getrandom` / `base64` | **=0.4.3** / **=0.23.1** | crates.io (checked 2026-10-17) | Nonces and keys; the recovery key's text form. |
| `symphonia` | **=0.6.1** (default features off; `wav`, `pcm`, `adpcm`, `isomp4`, `aac`, `alac`, `mp1`, `mp2`, `mp3`, `flac`, `ogg`, `vorbis`, `caf`, `mkv`, `aiff`; `adpcm`, `mp1`, `mp2`, `mkv`, `aiff` added 2026-10-18) | crates.io (checked 2026-10-17 with `cargo info`); MPL-2.0; rust-version 1.85 | Decodes phone, dictaphone, browser and call-app files for import (ADR-0007, phase 1). Pure Rust. MPL-2.0 is file-level copyleft: fine as an unmodified dependency of an Apache-2.0 app. It has no Opus decoder; see the next row. |
| `opus-decoder` | **=0.1.1** | crates.io (checked 2026-10-18 with `cargo info` and the crates.io API: released 2026-03-12, about 190k downloads); MIT OR Apache-2.0; rust-version 1.85 | Decodes Opus for import: WebM from browsers and call apps, `.opus` and Ogg Opus voice notes (WhatsApp, Telegram). Pure Rust, `forbid(unsafe_code)`, no C. Claims all 12 RFC 8251 conformance vectors. Chosen over `opus` 0.4.0 (libopus bindings) to keep a single native build. Verified on browser WebM (stereo 48 kHz), Ogg Opus (mono 16 kHz), `.opus` and WebM video. |
| `mdns-sd` | **0.21.5** | crates.io | Pure Rust. Used from M4 (Clinic Hub). Listed here only so the choice is recorded. |
| `hf-hub` | **not used** | crates.io; hf-hub issue #196 | Version 1.0.0 dropped resumable downloads (#196, still open). Multi-GB model downloads need resume, so we write our own downloader: ranged GET, SHA-256 check and disk-space check, against the Hugging Face resolve API. |
| `reqwest`, `tokio`, `serde`, `clap`, `tracing`, `anyhow`/`thiserror`, `sha2` | verify at scaffold | — | Ordinary dependencies. I'll pin the latest stable versions when the workspace is created. |
| `sha1` | **=0.11.0** | crates.io API (checked 2026-10-19: newest stable, released 2026-03-27, not yanked; checksum `aacc4cc499359472b4abe1bf11d0b12e688af9a805fa5e3016f9a386dc2d0214`, matches `Cargo.lock`); MIT OR Apache-2.0 | Git blob SHA-1 (`sha1("blob <len>\0" + bytes)`) to verify small Hub files that aren't in LFS, such as an MLX model's `config.json` (ADR-0025 §5). Brings no new crates: `digest` 0.11.3, `cpufeatures` 0.3.1 and `cfg-if` are already in the tree through `sha2` 0.11.1. The older `sha1` 0.10.7 stays in `Cargo.lock` for axum, tungstenite and zip. |
| `flate2` | **=1.1.10** | crates.io API (checked 2026-10-19: released 2026-08-28, not yanked; checksum `6e634e2e0ebac1ee034020da1ca582e17ffe4e0f5e985823721e168928136dcb`, matches `Cargo.lock`); MIT OR Apache-2.0 | Unpacks the MLX engine pack's Python archive (`.tar.gz`) with `tar` (ADR-0025 §4). Already in `Cargo.lock` through other crates; now a direct dependency of `os-models` at the same version. |
| `windows-sys` (Windows only) | **=0.61.2**, feature `Win32_Storage_FileSystem` | crates.io API (checked 2026-10-19: newest stable, released 2025-10-06, not yanked; checksum `ae137229bcbd6cdf0f7b80a31df61766145077ddf49416a728b02cb3921ff3fc`, matches `Cargo.lock`); MIT OR Apache-2.0; rust-version 1.71 | `GetFileInformationByHandle` for a file's hard-link count, so the space a removal frees is honest when a copy from this computer is a hard link to the original (ADR-0025 §9, §10). Std's own accessor (`number_of_links`) is still unstable. Already in `Cargo.lock` through other crates; now a direct, Windows-only dependency of `os-models` at the same version. |

**Deliberately excluded (decisions to record as an ADR after spike S2):**

| Crate | Why it's excluded |
|---|---|
| `whisper-rs` 0.16.0 / `whisper-rs-sys` 0.15.0 | 1. **It conflicts with `llama-cpp-2` when linked into the same binary.** Both crates statically link their own ggml with different ABIs (`GGML_TYPE_COUNT` is 42 in one and 40 in the other). The link fails with duplicate symbols, and forcing it gave `STATUS_HEAP_CORRUPTION`. Sources: llama-cpp-rs #484 and PR #1091; whisper-rs Codeberg PR #260, still unmerged. 2. It bundles whisper.cpp 1.8.3 (upstream is 1.9.5) and hasn't had a release since March 2026. 3. It moved to Codeberg and its GitHub repo is archived. |
| `ort` 2.0.0-rc.14 | Still a release candidate, and it targets ONNX Runtime 1.31. A second ONNX Runtime alongside sherpa-onnx's 1.28.2 risks clashing `onnxruntime.dll` files. All ONNX work goes through sherpa-onnx until we need otherwise. |
| `sqlx` 0.9 / `tauri-plugin-sql` 2.6 | Cargo can't resolve them alongside our SQLite choice. Each crate in the chain declares `links = "sqlite3"`: sqlx 0.9 needs libsqlite3-sys <0.38, tauri-plugin-sql pulls in sqlx 0.8, and rusqlite 0.40 needs 0.38. sqlx also has no SQLCipher feature. |
| `sherpa-rs` | Deprecated by its author in favour of the official crate. Repo archived. |

## 3. JS / TS packages (UI, PWA, extension)

All versions are from registry.npmjs.org.

| Package | Pin | Caveat |
|---|---|---|
| `react`, `react-dom` | **19.3.1** | — |
| `vite` / `@vitejs/plugin-react` | **8.3.4** / **6.1.2** | plugin-react 6 requires vite 8. |
| `@tauri-apps/api` / `@tauri-apps/cli` | **~2.12** (2.12.4 / 2.12.3) | The `next` tag is 3.x alpha. |
| `vitest` | **5.0.5** | Needs Node ^22.12, ^24 or ≥26. Browser mode is a separate package, `@vitest/browser-playwright`. |
| `@testing-library/react` | **16.3.3** | — |
| `lucide-react` | **1.59.0** (exact; released 2026-10-17, ISC) | Icons, per the brand guide §6 (ADR-0016). Replaces `@phosphor-icons/react` 2.1.10. Checked live on 2026-10-18. |
| Manrope, JetBrains Mono (fonts) | **@fontsource-variable/manrope 5.3.0**, **@fontsource-variable/jetbrains-mono 5.3.0** (OFL-1.1) | Brand type (ADR-0016). Only the latin and latin-ext woff2 files are vendored in `packages/ui/src/fonts`, with the licences in `OFL.txt`. Nothing loads from a CDN. They replace Geist. Checked live on 2026-10-18. |
| `@supabase/supabase-js` | **2.118.0** (MIT, 2026-10-15) | Website account portal only (ADR-0017). Never shipped in the app. Checked live on 2026-10-18. |
| `@playwright/test` | **1.64.1** | — |
| `tailwindcss` | **4.3.3** | Built on top of the design tokens in `packages/ui`. |
| `yaml` / `ajv` / `handlebars` | **^2.9.1** / **8.20.0** / **4.7.10** | Template loading, JSON Schema validation and rendering. yaml 3.0 is still `next`. |
| `@huggingface/transformers` | **4.3.1** | Web spike only. Gemma 4 support (4.0.1+) and the Gemma 4 WebGPU KV-cache fix (4.3.0) are in. |
| `@litert-lm/core` | **0.18.1** | Web spike only. The docs list Gemma 4 E2B/E4B for web, but the v0.15 release notes say only 12B and up is officially supported on web. Treat it as unproven. |
| `wxt` | **0.21.4** (M5; the brief's M3, renumbered by `docs/plans/M3.md`) | Browser extension. Plasmo's last release was 2025-05, so it's out. |
| `yjs` / `@automerge/automerge` | **not used** | ADR-0017 drops the CRDT: runs are immutable and edits create new runs, so neither is needed for sync or the Clinic Hub (M4). Listed versions were 13.6.33 / 3.5.0. |
| `@docusaurus/core` | 3.10.2 (M5) | — |

**Mobile fallback stack (only if spike S1 fails).** Recorded here so the fallback is concrete:

| Package | Status |
|---|---|
| `expo` | **58.0.14** (SDK 58, React Native 0.88). Brand new, and it requires the iOS 27 UIScene changes. SDK 57 (57.0.30) is the conservative choice. |
| `llama.rn` | **0.12.9** stable. npm's `latest` tag points at 0.13.0-rc.7, so pin explicitly. |
| `react-native-sherpa-onnx` | **0.4.4**. A small community package; k2-fsa has no official React Native binding. |
| `cactus-react-native` | **Archived**. Not an option, despite the master prompt suggesting it. |

## 4. Models (M0 subset)

All shas are the Hugging Face `main` commit on 2026-10-17. The full catalog is an M1 deliverable.

| Slot | Repo | Revision | License | Size | M0 role |
|---|---|---|---|---|---|
| LLM, default for the M0 CLI | `google/gemma-4-E4B-it-qat-q4_0-gguf` | 4b4a2c1d584be7264f87aac328a1bc739ce81b6c | Apache-2.0 | 5.15 GB | Default model for the spike |
| LLM, desktop quality candidate | `google/gemma-4-12B-it-qat-q4_0-gguf` | 29d097773436b69ff9feafd636ab4cf873786537 | Apache-2.0 | 6.98 GB | Benchmarked against E4B. Also tests the over-4-GB MSVC load issue. |
| LLM, mobile and web | `google/gemma-4-E2B-it-qat-q4_0-gguf` | 675cff42a74c774d6cb76f76d8eacb49b48c9b93 | Apache-2.0 | 3.35 GB + 0.99 GB mmproj | Spike S1 on device |
| LLM, Android LiteRT | `litert-community/gemma-4-E2B-it-litert-lm` | b3ca0d2f076785a8f4b2219ddbd2bdb99954eae1 | Apache-2.0 | 2.01 GB (gpu) | Spike S1 comparison |
| LLM, web | `onnx-community/gemma-4-E2B-it-ONNX` (q4f16) | 6f21c99d7e18237f8d2ce92aadfdbfeba877f63a | Apache-2.0 | — | Web probe |
| STT, English | `csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8` (upstream `nvidia/parakeet-tdt-0.6b-v3` @ 541d1f99…) | 2bda32ec70b097a55adaa07d9a7173915b43cc78 | CC-BY-4.0, **needs attribution in NOTICE** | 670 MB | Default English STT in the S2 pipeline |
| STT, multilingual | `openai/whisper-large-v3-turbo` @ 41f01f3f… as a sherpa-onnx export | verify at S2 start | MIT | ~0.8B params | Multilingual and French comparison. I haven't yet confirmed the exact sherpa-onnx export name. |
| STT, streaming and mobile | `moonshine-ai/moonshine-streaming-small` | 2c036506f23a09c18df5a50057599ba6d9280999 | MIT (all streaming models, every language) | 123M params | Spike S1. Whether sherpa-onnx can run it needs checking. |
| VAD | Silero VAD **v6.2.3** (model v6.2) | GitHub release | MIT | ~1 MB | Live chunking |
| Diarization: segmentation | `csukuangfj/sherpa-onnx-pyannote-segmentation-3-0` | 9403a6902bb58e3d5ae8c7e77c3422de279db2e0 | MIT, ungated copy (upstream pyannote is gated) | 6 MB / 1.5 MB int8 | S2 pipeline |
| Diarization: speaker embedding | `wespeaker_en_voxceleb_resnet34_LM.onnx` (k2-fsa `speaker-recongition-models` tag) | release asset | CC-BY-4.0 (WeSpeaker docs) | 26.5 MB | English candidate. The sherpa-onnx docs default, 3D-Speaker, was trained on Chinese data. |
| LLM, MLX tests only (developer and CI, ADR-0025) | `mlx-community/Qwen3-0.6B-4bit` (from `Qwen/Qwen3-0.6B`) | 73e3e38d981303bc594367cd910ea6eb48349da8 (checked live 2026-10-19) | Apache-2.0, ungated | 335 MB (9 files) | Real-worker tests of the MLX engine on Linux CPU (`scripts/mlx-test-env.sh`, which checks every file's size and hash). Never shipped or offered by default. |

**Corrections to the master prompt:**
- Gemma 4 has **five** sizes: E2B, E4B, 12B (added 2026-06-03), 26B-A4B and 31B.
- Only **E2B, E4B and 12B accept audio**, at **≤30 seconds per clip**.
- Context is 128K for E2B/E4B and 256K for the rest.
- `google/medgemma-1.5-4b` does not exist. The real repo is `google/medgemma-1.5-4b-it`, under the **HAI-DEF terms** (Google's Health AI Developer Foundations license). It is gated and its restrictions pass down to every user, so it must be a **user-accepted download, never preloaded**.
- `ggml-org/whisper.cpp` does not exist on Hugging Face. The files are in `ggerganov/whisper.cpp`.
- `google/embeddinggemma-2` exists: 740M parameters, Apache-2.0, ungated. Use it. The older `embeddinggemma-300m` is still under the Gemma Terms of Use and needs manual approval.
- Moonshine **streaming** models are MIT in every language. Only the older non-streaming non-English models are non-commercial.
- Qwen3-ASR comes only in 0.6B and 1.7B, and **there is no official diarization variant**.
- The `Pl4tt/OpenScribe` repo named in the prompt is a fork of `sammargolis/OpenScribe` (MIT, an active human-medicine scribe, openscribe.org).
