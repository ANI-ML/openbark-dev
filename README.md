<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/brand/logos/openbark-lockup-horizontal-outlined-on-dark.svg">
    <img src="docs/brand/logos/openbark-lockup-horizontal-outlined.svg" width="320" alt="OpenBark">
  </picture>
</p>

<p align="center"><strong>The free, open-source AI scribe for veterinary medicine. It runs on your clinic's own computer.</strong></p>

<p align="center">
  Built by the <a href="https://www.animl.health">ANI.ML Health</a> team.<br>
  Apache-2.0 licensed. Beta. <a href="https://www.openbark.io">openbark.io</a>
</p>

---

OpenBark records the consult (or imports a recording), transcribes it, and drafts the medical record with open models that run locally: on a Windows, Mac or Linux computer in your clinic. Each statement in the draft cites the transcript lines it came from, so you can jump to where it was said. OpenBark flags citations whose wording doesn't match, but a citation can still point to the wrong line, so check before signing. Numbers are checked against the transcript, and nothing is final until a veterinarian signs it. Recordings, transcripts and notes stay on your computer, encrypted.

> **Status: beta.** The desktop app works end to end on Windows (recording, transcription, drafts with evidence, templates, patients, signing, encryption, team roles, installer). It has been measured on synthetic consults on one Windows PC with an NVIDIA RTX 4080. The macOS and Linux versions are built automatically but have not yet been run and checked on real machines. It has not yet been validated in real clinics. Use it alongside your usual records, and review every draft.

## Contents

- [What it does](#what-it-does)
- [Install](#install)
- [Getting started](#getting-started)
- [Models and hardware](#models-and-hardware)
- [Privacy](#privacy)
- [How accuracy is checked](#how-accuracy-is-checked)
- [Languages](#languages)
- [Build from source](#build-from-source)
- [Documentation](#documentation)
- [How it was built](#how-it-was-built)
- [Version history](#version-history)
- [Contact and support](#contact-and-support)
- [License and credits](#license-and-credits)

## What it does

**Record or import**
- One button to record, or the Space bar; a live transcript with speaker labels as you talk.
- Import a recording from a phone, dictaphone or call app: WAV, MP3, M4A/AAC, FLAC, OGG/Opus, WebM, AIFF and more.
- A loading board shows each step (reading, transcribing minutes of audio, checking numbers, saving) with the time left.
- Audio is saved, encrypted, every couple of seconds as you record, so a crash or power cut keeps almost everything recorded up to that moment.

**Draft the note**
- 20 built-in templates for the kind of visit: New consult, Recheck, Annual exam, Phone call, Results, Clinician rounds, SOAP, Dental, Surgery and anaesthesia, Emergency and triage, Dermatology, Oncology, Equine, Production animal, Exotics, Euthanasia, plus client documents (discharge instructions, client summary, estimate explanation, euthanasia aftercare).
- The drafting board shows the note being written heading by heading, including when you draft again or reprocess with another template.
- Headings that depend on species (FeLV/FIV status for cats, for example) appear only for that species. Multi-pet visits get one note per animal.
- **Ask** redrafts one heading the way you want it ("one line per follow-up, with when and why"); save the request as a standard so later drafts follow it.
- Edit any heading yourself; your lines are marked as yours. Sign to lock the note; add dated, signed addenda after.
- Build your own templates from headings, or paste an example note and let OpenBark propose the template.

**Patients and visits**
- Patients lists every animal, with each one's visits, type and status (transcribed, draft, signed).
- Find a patient when you start recording; **Reprocess** any visit to draft it again with the same or a different template. Earlier drafts are always kept.

**For the client and your PIMS**
- Discharge instructions and client summaries in plain language, written from your corrected note, in the client's language.
- Copy the note into your practice management system as plain text, formatted text or Markdown; save a PDF with your clinic's letterhead.

**Team**
- Team members with roles: veterinarians sign, technicians draft and edit, reception records. Every revision records who made it.

See every feature, with where it lives in the code, on the website's features page (`website/`, published separately).

## Install

Point-and-click installers for each platform. On Windows no administrator rights are needed. On Linux, installing the .deb or .rpm asks for an administrator password (the AppImage does not). On macOS, removing the app from the main Applications folder may ask for one. Uninstalling keeps your visits unless you choose otherwise.

| | Install | Uninstall |
|---|---|---|
| **Windows** 10/11 | Run **OpenBark_&lt;version&gt;_x64-setup.exe** and follow the wizard | Start menu > OpenBark > **Uninstall OpenBark**, or Settings > Apps |
| **macOS** 13+ (Apple silicon) | Open the **.dmg** and drag OpenBark onto Applications | Double-click **Uninstall OpenBark** on the disk image, or drag the app to the Trash |
| **Linux** | Open the **.deb** or **.rpm** in your Software app, or run the **AppImage** | Remove it in your Software app |

The first time you open it, OpenBark offers to download the models (about 6 GB, once). After that it works offline.

Full details, including the macOS "Open Anyway" step for this unsigned build, the uninstaller's choices and the advanced install scripts for the browser version: **[install/README.md](install/README.md)**.

Installers are built by [.github/workflows/release.yml](.github/workflows/release.yml) from a version tag. Public downloads will be published with the beta.

## Getting started

1. Read and accept the one-time consent notice. Your clinic is responsible for getting each client's consent to record.
2. Download the models when asked (Record screen). "Models ready" means you're set.
3. In **Settings**, add your name for signing, choose the app language and the language notes are written in, and add your clinic's letterhead for PDFs.
4. On **Record**, find or enter the patient, pick the kind of visit, and press the red button or Space.
5. Press it again to stop. The draft opens in **Review**: rest on any line to see where it was said, fix what needs fixing, then **Sign note**.

The in-app **Help** has short videos for each of these. The written guide is in [docs/guide](docs/guide/README.md).

## Models and hardware

| Job | Model | License |
|---|---|---|
| Writes the note | Gemma 4 (E4B for most computers, 12B with 10 GB+ of graphics memory or a 24 GB Mac, E2B for small machines) via llama.cpp | Apache-2.0 |
| Transcribes | NVIDIA Parakeet TDT 0.6B v3 (English and 24 other European languages) via sherpa-onnx | CC-BY-4.0 |
| Re-listens to numbers and drug names | OpenAI Whisper large-v3-turbo | MIT |
| Speech detection, speaker labels | Silero VAD, pyannote segmentation 3.0, WeSpeaker ResNet34 | MIT, MIT, CC-BY-4.0 |

- **Recommended:** 16 GB of memory. A graphics card (NVIDIA, AMD or Intel through Vulkan, or Apple silicon through Metal) makes drafting much faster.
- **Windows with NVIDIA:** the optional GPU speech pack (Models screen) moves transcription onto the card too. On an RTX 4080, an 8.5-minute recording was ready in 43 seconds instead of about 2.5 minutes.
- The **Models** screen explains what names like E4B, 12B and Q4 mean, says which model suits your computer, and lets you browse Hugging Face for others (on a Mac it labels MLX-optimized builds).

## Privacy

- Recording, transcription and drafting all run on your computer. No telemetry, no analytics, no cloud fallback. OpenBark asks once whether to check for new versions; only if you say yes does it read the latest version number from openbark.io once a day, and nothing about you, your clinic or your patients is sent.
- Every network connection OpenBark can make is listed in [docs/network.md](docs/network.md): model downloads you start (from Hugging Face and GitHub), the optional GPU speech pack if you choose it (from GitHub and NVIDIA's packages on PyPI), a Hugging Face search you open, and the version check if you turn it on (from www.openbark.io). On Windows, the setup wizard downloads Microsoft WebView2 if the computer doesn't have it. Nothing about patients ever leaves the computer.
- Recordings, transcripts, notes and the patient list are encrypted at rest (XChaCha20-Poly1305). The key is kept in the system keychain, or in a file in your user folder if the keychain isn't available (Settings says so). Save the recovery key (Settings > Encryption > Show the recovery key) and keep it safe. Without it, visits can't be opened on another computer, after reinstalling, or if this computer's key is lost.
- Choose where visits and models are kept (Settings > Storage), such as a backed-up drive.
- The browser version listens on 127.0.0.1 only and refuses requests from other sites.

## How accuracy is checked

- **The evidence link:** every statement cites the transcript lines it came from; rest on a line to see them.
- **Numbers:** a number in the note that isn't in the transcript blocks signing until you correct or retype that line yourself. An optional second speech model (Whisper, about 1 GB, downloaded from the Models screen) listens again to lines with numbers or words that look like drug or condition names, and flags numbers it hears differently. Without it, this check is off.
- **Only what was said:** templates keep the core rules (no invented findings, differentials only if voiced). Drafts are written in the template's order, so the history and exam come before the assessment.
- **Measured:** an evaluation suite scores drafts on synthetic consults with known facts. So far, on 13 scripted consults voiced by text-to-speech and drafted with Gemma 4 E4B: 0 invented numbers, and 141 of 155 expected facts captured (91%), including 89 of 98 across 9 specialty consults ([docs/eval](docs/eval)). Other note models have not been evaluated.

## Languages

- **Consults:** English and 24 other European languages are recognised (Parakeet). The veterinary vocabulary hints and the reading of numbers said in words work in English only, and only English consults have been evaluated.
- **Notes:** English, French, Spanish, German, Portuguese, Italian and Dutch. Client documents can be written in the client's language.
- **The app:** English, French, Spanish, German and Portuguese.

## Build from source

Requirements: Rust 1.99 (pinned in `rust-toolchain.toml`), Node 24 with pnpm 11 (via corepack), CMake, and LLVM/libclang. Windows also needs the Visual Studio 2022 C++ build tools.

```bash
pnpm install
cargo build --release -p os-cli                 # the engine and command line (add --features cuda, vulkan or metal)
Add `pnpm --filter @openbark/desktop build` before the serve line, with the comment: build the web UI for the browser version.
pnpm desktop                                    # the desktop app in development
cd apps/desktop && pnpm tauri build             # this platform's installer
```

Step-by-step runbooks: [Windows](docs/runbooks/windows.md), [macOS](docs/runbooks/macos.md). Tests: `cargo test`, `pnpm test`, and the end-to-end scripts in `apps/desktop/e2e` (run against a demo server, never a clinic's data).

**Layout:** `engine/` (Rust: speech, drafting, encryption, storage, the web server), `apps/desktop` (Tauri 2 app and React UI, also served in browser mode), `packages/` (UI kit, templates, translations, evaluation fixtures), `install/` (installers and scripts), `website/` (the public site), `docs/` (guide, decisions, plans).

## Documentation

- **User guide:** [docs/guide](docs/guide/README.md)
- **Install and uninstall:** [install/README.md](install/README.md)
- **Decisions** (why things are the way they are): [docs/adr](docs/adr), from inference libraries to encryption, patients, installers, storage and GPU speech
- **Network calls:** [docs/network.md](docs/network.md). **Pinned versions and model licenses:** [docs/versions.md](docs/versions.md)
- **Plans:** [M2](docs/plans/M2.md) (earlier [M1](docs/plans/M1.md), [M0](docs/plans/M0.md)). **Evaluation:** [docs/eval](docs/eval)

## How it was built

OpenBark is built by the [ANI.ML Health](https://www.animl.health) team. Every architectural decision is written down as a decision record, every dependency is checked and pinned, changes to drafting are measured before and after, and larger changes go through a multi-stage review before they're merged.

## Version history

Every change that matters to a clinic, newest first, with the commit and files behind it: see [CHANGELOG.md](CHANGELOG.md), also on the website's Version history page.

## Contact and support

Questions, feedback, a bug to report, or a clinic that would like to try OpenBark: email the team at ANI.ML Health, [we.care@animl.health](mailto:we.care@animl.health).

OpenBark is free. If it gives you back some evenings, you can [buy us a coffee](https://buymeacoffee.com/animlhealth).

## License and credits

Built by the **[ANI.ML Health](https://www.animl.health)** team.

Apache-2.0: see [LICENSE](LICENSE) and [NOTICE](NOTICE). Models are downloaded separately and each is under its own license, which the app shows before download. "Gemma" is a trademark of Google LLC; OpenBark is not affiliated with or endorsed by Google, NVIDIA or OpenAI.

OpenBark drafts notes for a veterinarian to review. It does not make clinical decisions, and a draft is not a medical record until a veterinarian has reviewed and signed it.
