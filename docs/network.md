# Network calls

OpenBark is local-first (master prompt §3, rule §7.4). This file lists every network connection the software can make. **A host that isn't listed here is a bug.** CI enforces this in two ways:
- the offline end-to-end test runs the full pipeline with networking disabled;
- the source check confirms that `os-models` is the only engine crate with an HTTP client.

## At run time

| # | When | Host(s) | What's sent | Purpose | Off switch |
|---|---|---|---|---|---|
| R1 | Only when the user runs `openbark models pull <id>` (later, the Download button in the model manager) | `huggingface.co`; it redirects file downloads to its storage hosts, `cas-bridge.xethub.hf.co`, `cdn-lfs.hf.co` and `cdn-lfs-us-1.hf.co` | An HTTP GET for a pinned file revision, with a `User-Agent: OpenBark/<version>` header. Also an `Authorization: Bearer` header, sent **only** to `huggingface.co` and **only** if the user supplied a token for a gated repo. | Download model weights | Don't pull. Models can also be copied in manually; drag-and-drop arrives in M1. |
| R2 | Same as R1, for catalog entries hosted on GitHub | `github.com`; it redirects to `objects.githubusercontent.com` and `release-assets.githubusercontent.com` | HTTP GET, `User-Agent` header | Download small sherpa-onnx models (VAD, speaker embeddings) | As R1 |

| R3 | Only while the user runs `openbark serve` (web mode) | **Listens** on `127.0.0.1:<port>` (default 4317). Loopback only: unreachable from other machines | The local web UI and its API, serving this computer's own browser | Stop the command. It never binds to other interfaces until the Clinic Hub adds authentication and mutual TLS (M4) |
| R5 | Only when the user chooses **Download** on **Faster transcription on the graphics card** (Windows with an NVIDIA graphics card), after accepting NVIDIA's licence (ADR-0014) | `github.com` (the sherpa-onnx v1.13.8 release, redirecting to `release-assets.githubusercontent.com`) and `files.pythonhosted.org` (NVIDIA's CUDA runtime, cuBLAS and cuDNN packages); the two full-precision models from `huggingface.co` as R1 | HTTP GETs for pinned files (URL, size and SHA-256 fixed in `os-models/src/gpu_pack.rs` and the catalog), `User-Agent: OpenBark/<version>`. Nothing about the clinic | Download the GPU speech pack | Don't choose Download; speech recognition stays on the processor |
| R4 | Only when the user opens **Browse Hugging Face** in the model manager and searches or opens a model | `huggingface.co` (`/api/models`) | HTTPS GETs with the search words, or the chosen model's id, and a `User-Agent: OpenBark/<version>` header. One query for GGUF models; on a Mac, a second query for MLX builds (to label them "MLX optimized"). No token, nothing about patients | Find and add note-writing models | Don't open the browser; models can be pulled by id or copied in |

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

## Planned (not built yet)

| Milestone | Host | Purpose | Default |
|---|---|---|---|
| M1 | User-configured URL | Optional remote OpenAI-compatible endpoint (a clinic's own model server, or a hosted model with the user's key) | **Off.** Clearly labelled, never used as a silent fallback |
| M4 | LAN only (mDNS `_openbark._tcp`) | Clinic Hub discovery and mutual-TLS API | Off unless Hub mode is enabled |
| Proposed (ADR-0007) | The clinic's own phone system (PBX) or a SIP trunk the clinic chooses | Call mode: SIP signalling and RTP media for calls placed from OpenBark, recorded and transcribed on this computer. Nothing goes to a transcription service | **Off.** Only once the clinic configures a phone line |
| Later (ADR-0005) | User-chosen sync server, either self-hosted or a hosted option | Per-user cloud sync of notes, templates and settings. **End-to-end encrypted**, so the server sees ciphertext only. Audio never syncs. | **Off.** Requires the user to sign in and opt in |
