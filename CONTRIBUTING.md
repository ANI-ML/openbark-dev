# Contributing to OpenBark

Thank you for helping. OpenBark is a free, open-source AI scribe for veterinary medicine that runs on the clinic's own computer. It is built by the [ANI.ML Health](https://www.animl.health) team and licensed under Apache-2.0.

This guide covers the rules every change follows, how to build and test, and how to send a pull request. Please also read the [code of conduct](CODE_OF_CONDUCT.md).

## Three rules that are never relaxed

1. **Never commit real clinical data or secrets.** No real recordings, transcripts, notes, patient or client names, clinic details, or screenshots that show any of them. Use the synthetic consults in `packages/eval/fixtures` and the demo data the end-to-end scripts create. No API keys, tokens, signing certificates, `.env` files or recovery keys either. If you commit one by mistake, tell us straight away at [we.care@animl.health](mailto:we.care@animl.health) rather than only deleting it in a later commit: it stays in the history.
2. **Clinical safety.** A note stays a draft until a veterinarian signs it. The software must never invent vitals, doses or findings. Anything not discussed in the consult is shown as "Not discussed". Changes to drafting are measured before and after (see [Testing](#testing)).
3. **Local-first.** Recording, transcription and drafting run on the clinic's computer. There is no telemetry and no cloud fallback. Every network connection OpenBark can make is listed in [docs/network.md](docs/network.md). A change that adds one must add it there, with an off switch where that makes sense, and CI checks that only the `os-models` crate has an HTTP client.

## How this repository works

The public repository, [ANI-ML/openbark](https://github.com/ANI-ML/openbark), is a published copy of OpenBark's source, refreshed with one commit for each release. Day-to-day development happens in a private repository. Pull requests are welcome all the same: a maintainer reviews yours on the public repository, ports the change to the development repository by hand, and it ships in the next release. The pull request is then closed with a note saying which release includes it, rather than merged. Issues are read and answered on the public repository too.

## Before you start

- **Small fixes** (a typo, a clear bug with a test): open a pull request directly.
- **Anything larger** (a new feature, a new dependency, a change to how drafts are written or stored): open an issue first so we can agree the approach. Architectural decisions are written down as decision records in [docs/adr](docs/adr) (copy `0000-template.md`), and the current work is planned in [docs/plans](docs/plans). Please read the relevant ones before proposing a change to them.
- **Security problems** go through [SECURITY.md](SECURITY.md), never a public issue.

## Setting up

You need:

- **Rust 1.99.1**, pinned in `rust-toolchain.toml` (rustup installs it automatically), with clippy and rustfmt;
- **Node 24** with **pnpm 11** through corepack (`corepack enable`);
- **CMake** and **LLVM/libclang** (set `LIBCLANG_PATH` to LLVM's `bin` folder on Windows);
- on Windows, the **Visual Studio 2022 C++ build tools**;
- on Linux, the Tauri and audio system packages: `libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libclang-dev cmake libasound2-dev` (Ubuntu 24.04 names);
- optionally, the CUDA 12.9 toolkit for an NVIDIA build.

Step-by-step runbooks: [Windows](docs/runbooks/windows.md) and [macOS](docs/runbooks/macos.md). On Windows, `scripts/dev-env.sh` (Git Bash) sets up the paths.

## Building

```bash
pnpm install
cargo build --release -p os-cli                    # the engine and command line; add --features cuda, vulkan or metal
pnpm --filter @openbark/desktop build              # the web UI for the browser version
target/release/openbark serve                      # the browser version, at http://localhost:4317
pnpm desktop                                       # the desktop app in development
cd apps/desktop && pnpm tauri build                # this platform's installer
```

The models (about 6 GB) download from the app's Models screen, or with `openbark models pull <id>` for each id in `openbark models list`. Set `OPENBARK_MODELS_DIR` to share one models folder between builds.

**Layout:** `engine/` (Rust: speech, drafting, encryption, storage, the local web server), `apps/desktop` (Tauri 2 app and React UI, also served in browser mode), `packages/` (UI kit, templates, translations, vocabulary, evaluation fixtures), `install/` (installers and scripts), `website/` (the public site), `docs/` (guide, decisions, plans).

## Testing

Run what CI runs before you open a pull request:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
bash scripts/ci/check-network.sh                   # only os-models may have an HTTP client
pnpm -r run typecheck
pnpm -r run test
```

- **Write the test first** for the engine pipeline, template rendering, the faithfulness checks and connectors.
- **End-to-end scripts** in `apps/desktop/e2e` drive the browser version with Playwright. Run them against a demo server with demo data in a scratch folder, never against a clinic's own data. Point the data folders somewhere neutral, because screenshots and videos can show them, for example:
  `OPENBARK_SESSIONS_DIR=<scratch>/visits OPENBARK_TEMPLATES_DIR=<scratch>/templates OPENBARK_USERS_FILE=<scratch>/users.json openbark serve --port 4318`
- **Drafting changes** (prompts, templates, the faithfulness checks, model choice): run `openbark eval` on the synthetic consults before and after, and put both results in the pull request. The method and earlier results are in [docs/eval](docs/eval). A change that adds an invented number is not accepted.

## Dependencies

Every pinned version, model checksum and model licence is recorded in [docs/versions.md](docs/versions.md), each with its source. If you add or bump a dependency, check it live (current release, licence, age) and update that file in the same pull request. This applies to Dependabot pull requests too.

Some crates are ruled out on purpose, because the binary must contain exactly one ggml and one ONNX Runtime: `whisper-rs`, `ort`, `sqlx`, `tauri-plugin-sql` and `hf-hub`. The reasons are in `docs/spikes/S2-native-linking.md`.

Models that ship preloaded must have a licence that allows redistribution, with attribution in [NOTICE](NOTICE) where required.

## User interface

The brand guide, [docs/brand/DESIGN.md](docs/brand/DESIGN.md), governs how OpenBark looks on every surface: logo, colour, type, icons, components, motion, layout and copy voice ([ADR-0016](docs/adr/0016-brand-guide.md)). [docs/design/DESIGN.md](docs/design/DESIGN.md) still covers interaction behaviour the brand guide doesn't: the evidence link, "Not discussed", unsigned drafts and the accessibility floor.

In short: design tokens only (no raw hex values), Manrope, JetBrains Mono and Lucide bundled locally (never from a CDN), no emoji, flat surfaces with 1 px borders, amber used only for recording and focus, and sentence case. Check light and dark themes and a phone-width window, and include screenshots in the pull request. User-facing text goes through the translation files in `packages/i18n`.

## Commits and pull requests

- Keep commits small, with conventional messages (`fix(review): keep the cursor on the edited line`).
- One topic per pull request. Fill in the pull request template.
- If the change matters to a clinic, add an entry to `website/content/history.json`. `CHANGELOG.md` and the website's Version history page are generated from it by `scripts/website-build.py`; don't edit `CHANGELOG.md` by hand.
- AI-assisted contributions are welcome. You are responsible for every line you submit: read it, test it, and make sure you have the right to contribute it.
- CI must pass, and a maintainer reviews every pull request before it is ported (see [How this repository works](#how-this-repository-works)).

## Licensing of contributions

OpenBark is licensed under the [Apache License 2.0](LICENSE). By submitting a contribution, you agree that it is licensed under the same licence, as section 5 of the Apache License describes, with no additional terms.

Please also sign off each commit to certify the [Developer Certificate of Origin 1.1](https://developercertificate.org/): that you wrote the change, or otherwise have the right to submit it under the project's licence. Add the sign-off with `git commit -s`, which appends a line such as:

```
Signed-off-by: Jane Doe <jane@example.com>
```

Use your real name and an email address you can be reached at.

## Getting help

Ask in an issue, or email the team at ANI.ML Health, [we.care@animl.health](mailto:we.care@animl.health).
