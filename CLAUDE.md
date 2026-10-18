# OpenBark: working notes for Claude Code

OpenBark is a local-first, open-source (Apache-2.0) veterinary AI scribe. The product brief is [OPENSCRIBE_CLAUDE_CODE_PROMPT.md](OPENSCRIBE_CLAUDE_CODE_PROMPT.md). It still uses the old working name "OpenScribe"; the product is now **OpenBark**.

## Before doing anything

1. Read the current milestone plan in `docs/plans/` (M0 right now). Only work on what it covers, and get product-owner confirmation before large changes.
2. `docs/versions.md` holds every pinned version, model sha and license, each with its source. Don't add or bump a dependency without checking it live and updating that file.
3. Architectural decisions go in `docs/adr/` (copy `0000-template.md`).
4. **Any UI work:** read `docs/design/DESIGN.md` first; it overrides component-library defaults. Run its §12 pre-flight checklist before closing a UI task. In short: tokens only (no raw hex), Geist and Phosphor bundled locally, no emoji, no middle-dot metadata, one primary button per view, hairlines rather than shadows, motion only to answer actions (the recording ring is the one exception).

## Decisions already made (don't relitigate)

- **One ggml and one ONNX Runtime per binary.** LLMs run through `llama-cpp-2`. All audio models (STT, VAD, diarization) run through the official `sherpa-onnx` crates.
- **Don't add these:**
  - `whisper-rs`: duplicate ggml, which corrupted the heap when forced to link.
  - `ort`: a second ONNX Runtime.
  - `sqlx` / `tauri-plugin-sql`: they conflict with rusqlite over `libsqlite3-sys`.
  - `hf-hub`: no resumable downloads.

  Reasons are in `docs/spikes/S2-native-linking.md`.
- **Encrypted store:** `rusqlite` with `bundled-sqlcipher-vendored-openssl`.
- **The product name lives in one constant.** It may still change after a trademark search.
- **MedGemma** is a user-accepted download (HAI-DEF terms), never preloaded. Preloaded models must have licenses that allow redistribution, with attribution in `NOTICE` where required (Parakeet: CC-BY-4.0).

## Rules

- Local-first. Every network call is listed in `docs/network.md`, with an off switch where one makes sense. There is no telemetry.
- Clinical safety. A note stays a draft until a DVM signs it. Never invent vitals, doses or findings. Anything not discussed renders as "Not discussed".
- Write tests first for the engine pipeline, template rendering, the faithfulness pass and connectors. No milestone closes with failing CI.
- Small conventional commits.

## Environment

- Main dev box: Windows 11, RTX 4080 16 GB, CUDA 12.9, VS 2022 Build Tools. It is the CUDA performance reference.
- The product owner tests macOS and iOS on their own Mac and iPhone by pulling from GitHub. Anything Mac-side needs a written, scripted runbook.
- Repo: `ANI-ML/openbark` (private).
