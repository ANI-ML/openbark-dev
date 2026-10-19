# Network calls

OpenBark is local-first (master prompt §3, rule §7.4). This file lists every network connection the software can make. **A host that isn't listed here is a bug.** CI enforces this in two ways:
- the offline end-to-end test runs the full pipeline with networking disabled;
- the source check confirms that `os-models` is the only engine crate with an HTTP client.

## At run time

| # | When | Host(s) | What's sent | Purpose | Off switch |
|---|---|---|---|---|---|
| R1 | Only when the user chooses **Download** on a model in the model manager, or runs `openbark models pull <id>` | `huggingface.co` (including its `/api/resolve-cache/` redirect); it redirects file downloads to its storage hosts, `cas-bridge.xethub.hf.co`, `us.aws.cdn.hf.co`, `cdn-lfs.hf.co` and `cdn-lfs-us-1.hf.co` | An HTTP GET for a pinned file revision, with a `User-Agent: OpenBark/<version>` header. GGUF files (every part of a split GGUF) and, for MLX models, the allow-listed files of the model folder (ADR-0025 §5). An `Authorization: Bearer` header with the saved Hugging Face token is sent **only** to `huggingface.co` and **only** for a gated repo; the redirects to the storage hosts don't carry it. | Download model weights | Don't download. Models can also be added from this computer (no network). |
| R2 | Same as R1, for catalog entries hosted on GitHub | `github.com`; it redirects to `objects.githubusercontent.com` and `release-assets.githubusercontent.com` | HTTP GET, `User-Agent` header | Download small sherpa-onnx models (VAD, speaker embeddings) | As R1 |

| R3 | Only while the user runs `openbark serve` (web mode) | **Listens** on `127.0.0.1:<port>` (default 4317). Loopback only: unreachable from other machines | The local web UI and its API, serving this computer's own browser | Stop the command. It never binds to other interfaces until the Clinic Hub adds authentication and mutual TLS (M4) |
| R5 | Only when the user chooses **Download** on **Faster transcription on the graphics card** (Windows with an NVIDIA graphics card), after accepting NVIDIA's licence (ADR-0014) | `github.com` (the sherpa-onnx v1.13.8 release, redirecting to `release-assets.githubusercontent.com`) and `files.pythonhosted.org` (NVIDIA's CUDA runtime, cuBLAS and cuDNN packages); the two full-precision models from `huggingface.co` as R1 | HTTP GETs for pinned files (URL, size and SHA-256 fixed in `os-models/src/gpu_pack.rs` and the catalog), `User-Agent: OpenBark/<version>`. Nothing about the clinic | Download the GPU speech pack | Don't choose Download; speech recognition stays on the processor |
| R6 | Only when the user chooses **Download** on the **MLX engine** card (a Mac with Apple silicon and macOS 14 or later; ADR-0025). Also in the developer **test mode** (`OPENBARK_MLX_DEV=1` on Linux x86_64), which only debug builds and builds made with the `mlx-test-mode` feature honour (CI and developers; never the builds clinics get): it downloads the Linux lock's pack (`mlx_pack.linux.lock.json`) from the same hosts | `github.com` (the python-build-standalone 20261009 release, redirecting to `release-assets.githubusercontent.com`) and `files.pythonhosted.org` (the pinned wheels: mlx, mlx-lm, llguidance, transformers and their dependencies) | HTTP GETs for pinned files (URL, size and SHA-256 fixed in `engine/os-models/src/mlx_pack.lock.json`), `User-Agent: OpenBark/<version>`. Nothing about the clinic | Download the MLX engine pack | Don't choose Download; GGUF models keep using the built-in engine. The MLX worker talks to OpenBark only over its standard input and output; its own code opens no socket and listens on nothing. What keeps it off the network: an empty environment with the Hugging Face libraries offline (`HF_HUB_OFFLINE=1` and the like, no proxy variables, no token) and only a local folder to load; on macOS, `sandbox-exec` denying all network access at the OS level; and a best-effort guard in Python (`connect`, `connect_ex`, `sendto`, `sendmsg`, `bind` and `listen` on `socket.socket`, `create_connection` and `getaddrinfo` raise), which doesn't reach Python's low-level `_socket` module or native code in the pack's libraries. Test mode has no OS sandbox |
| R4 | Only when the user opens **Browse Hugging Face** in the model manager and searches or opens a model, or saves a Hugging Face token | `huggingface.co` (`/api/models`, `/api/whoami-v2`, and `/<repo>/resolve/<revision>/` for two small files) | HTTPS GETs with the search words, or the chosen model's id, and a `User-Agent: OpenBark/<version>` header. Searching: one query for GGUF models; on a Mac, a second query for MLX builds. Opening a model: its details, then its file list (`/tree/<revision>?recursive=true`, following at most 20 pages). For an MLX repo, also its `config.json` (at most 1 MiB) and `model.safetensors.index.json` (at most 8 MiB), to check it can run, and one query for GGUF builds of the same base model (`filter=base_model:quantized:<base>&filter=gguf`); for a repo with nothing OpenBark can run (safetensors only, for example), the same query for GGUF builds of that repo, tokenless. The saved token is sent **only** for a gated repo's file list and these two files, never with a search. OpenBark only uses a token saved in it or set for it as `OPENBARK_HF_TOKEN`; the generic `HF_TOKEN` other Hugging Face tools use is never read. Saving a token: one GET to `/api/whoami-v2` with it, to check it and show whose it is. Nothing about patients | Find and add note-writing models | Don't open the browser; models can be pulled by id or added from this computer |

Removing models or engine packs, adding a model from this computer, and saving or removing the Hugging Face token in the keychain make no network calls. (Checking a token when it is saved is the single `whoami-v2` call in R4.)

The install scripts (`install/`) make no calls of their own: building from source uses B1, B2 and B3, and `-Models` / `--models` uses R1 and R2. Uninstalling makes no network calls.

The Windows setup wizard makes one call, and only on a computer without Microsoft Edge WebView2 (Windows 10 and 11 almost always have it): it downloads Microsoft's WebView2 bootstrapper from `go.microsoft.com` (redirecting to `msedge.sf.dl.delivery.mp.microsoft.com`), which installs WebView2 from Microsoft. Nothing about the clinic is sent. The macOS disk image and the Linux packages make no calls.

Nothing else at run time:
- no telemetry, analytics, crash reporting or update checks;
- no licence server;
- no fallback to a cloud model.

Transcription, note generation and everything else run with the network unplugged once the models are on disk.

## At build time (developers and CI only)

| # | Host(s) | Purpose | Notes |
|---|---|---|---|
| B1 | `index.crates.io`, `static.crates.io` | Rust dependencies | Pinned by `Cargo.lock` |
| B2 | `registry.npmjs.org` | JS dependencies | Pinned by `pnpm-lock.yaml` |
| B3 | `github.com/k2-fsa/sherpa-onnx/releases`, plus its redirect hosts | `sherpa-onnx-sys` build script downloads the prebuilt sherpa-onnx library for the target (spike S2) | CI caches the archive and sets `SHERPA_ONNX_ARCHIVE_DIR` so builds don't fetch it each time. The version is pinned to 1.13.8. |
| B4 | `static.rust-lang.org` | Rust toolchain (rustup) | Pinned by `rust-toolchain.toml` |
| B5 | `github.com/tauri-apps/binary-releases`, `github.com/tauri-apps/nsis-tauri-utils` | `tauri build --bundles nsis` downloads NSIS 3.13 and `nsis_tauri_utils.dll` 0.5.3 on first use, and checks their hashes | Cached afterwards |
| B6 | `github.com/create-dmg/create-dmg` (tag archive) | `scripts/macos-dmg.sh` downloads create-dmg 1.3.0 to lay out the macOS disk image | Pinned by tag in the script |
| B7 | `github.com` (redirecting to `release-assets.githubusercontent.com`), `files.pythonhosted.org`, `huggingface.co` and its storage hosts (as R1); for lock regeneration only, `pypi.org` and `api.github.com` | Developer and CI MLX tests (ADR-0025): download the Linux CPU lock's pack (`mlx_pack.linux.lock.json`) and the test model `mlx-community/Qwen3-0.6B-4bit`. The macOS CI job downloads the macOS lock's pack the same way. When the lock is regenerated (`scripts/mlx-pack-lock.py`, a developer's machine), uv resolves against `pypi.org/simple`, and the script reads `pypi.org/pypi/<name>/<version>/json`, the release's `SHA256SUMS` and the asset digests from `api.github.com`. The CI MLX jobs (`scripts/ci/mlx-e2e.sh`) also call Hugging Face's API for `models add-hf` (as R4, tokenless) | Pinned by URL, size and SHA-256 in the lock files, and verified against them on every CI run (no unpacked pack is cached); the test model by revision. The drafting step itself runs with no network (`unshare -rn`) |

## Planned (not built yet)

| Milestone | Host | Purpose | Default |
|---|---|---|---|
| M1 | User-configured URL | Optional remote OpenAI-compatible endpoint (a clinic's own model server, or a hosted model with the user's key) | **Off.** Clearly labelled, never used as a silent fallback |
| M4 | LAN only (mDNS `_openbark._tcp`) | Clinic Hub discovery and mutual-TLS API | Off unless Hub mode is enabled |
| Proposed (ADR-0007) | The clinic's own phone system (PBX) or a SIP trunk the clinic chooses | Call mode: SIP signalling and RTP media for calls placed from OpenBark, recorded and transcribed on this computer. Nothing goes to a transcription service | **Off.** Only once the clinic configures a phone line |
| M3 (ADR-0005, ADR-0017) | `<project>.supabase.co`, Supabase Auth (`/auth/v1`) and REST (`/rest/v1`, with `/functions/v1` for removing a device and deleting the account) only. ANI.ML's hosted project is `nmecovtmijnfnjuadicp.supabase.co`; a clinic can point OpenBark at its own Supabase | Sign-in with an emailed code, then per-user sync of transcripts, notes, patient records, templates and a few settings. **End-to-end encrypted**, so the server sees ciphertext only. **No audio:** recordings never leave the device that recorded them. No Storage uploads. Instant updates over Realtime come later and get their own row when built | **Off.** Requires the user to sign in and turn on encrypted sync |
