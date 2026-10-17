# OpenScribe — Claude Code Master Prompt

> Paste this as the opening message of a new Claude Code session (or drop it in as `CLAUDE.md` at the repo root). It is written to be read by Claude Code as a build brief: mission, competitive bar, hard constraints, architecture, phased milestones, and working rules.

---

## 0. One-paragraph mission

Build **OpenScribe**: an open-source, local-first veterinary AI scribe that runs entirely on the user's own hardware (laptop, clinic server, or phone), lets anyone load open models from Hugging Face through a GUI (Gemma 4 preloaded), ships with a veterinary template library and template builder, and writes notes back into practice information management systems (PIMS). The goal is not parity with the paid veterinary scribes — it is to beat them on the things they structurally cannot do: zero per-seat cost, no clinical audio or records leaving the building, user-owned models, and an open connector SDK. It is "the Ollama/LM Studio of veterinary documentation": a polished consumer-grade app wrapped around open inference.

Positioning in one line: **the paid scribes rent you intelligence; OpenScribe lets you own it.**

---

## 1. Who this is for

- **Primary:** veterinarians in general practice, ER, and specialty (including oncology, dental, exotics, equine, large animal) who want ambient or dictated notes without a subscription or cloud exposure.
- **Secondary:** RVTs/technicians and reception staff who co-document (multi-role "teams" workflows), relief/locum vets who move between PIMS, and academic/teaching hospitals with strict data governance.
- **Tertiary:** developers and researchers who want to swap in their own fine-tuned veterinary models (e.g., a future open veterinary SLM/NER stack) and build connectors.

The reference user is a busy clinician on a mid-range MacBook or a Windows laptop with an RTX-class GPU, with an iPhone in the exam room, in a clinic running one of ezyVet / Instinct / Shepherd / Cornerstone / AVImark / Digitail / Provet / IDEXX Neo / Covetrus Pulse.

---

## 2. The competitive bar (what we must match, then beat)

The market has consolidated fast in 2026: Scribenote went free (default templates, unlimited notes), ScribbleVet was acquired by Instinct Science (Jan 2026) and is being embedded into Instinct EMR, Digitail ships a PIMS-native scribe (Tails AI) plus a standalone app (Tails VIP), and a dozen others compete on templates and PIMS breadth. Published pricing sits roughly $40–$200 per vet per month, with some flat-rate clinic plans ($119–$169/mo). Every one of them is cloud-only.

### 2.1 Feature matrix — what the paid scribes offer today

| Vendor | Pricing (public, Oct 2026) | Notable features | PIMS story |
|---|---|---|---|
| **Scribenote** | Free (default templates, unlimited, Teams Mode); Pro ~$79–99/DVM/mo; Enterprise (SSO, dashboards) | Adaptive custom templates, client summaries, multilingual mode, AI writing assistant, human support on Pro | Cornerstone, AVImark, ezyVet named; copy/paste elsewhere |
| **ScribbleVet** (Instinct Science) | Essential $40/mo for 150 SOAPs (+$20 per 100); Personal $120–200/mo; Practice tier | Unlimited recordings, custom templates, automated client emails, vet-tuned models, medical record summarizer, **visual dental charts**, multi-language, **multi-pet appointments**, 1-click PIMS transfer; Plumb's drug content now in-workflow; explicit consent-law handling | ezyVet, Pulse, Vetspire, Instinct, Shepherd, DaySmart, Rhapsody + Chrome extension for web PIMS |
| **VetRec** | $99/vet/mo annual, $150 monthly; Enterprise | 30+ specialty templates (GP, ER, Exotics, Dental charts), Template Builder, AI patient summaries from prior records, **phone-call integration** for client comms logging, multi-language client comms; SOC 2 Type II | Markets the longest list (~12+ direct integrations) |
| **CoVet** | ~$46–99/mo | 95+ templates, AI template builder ("CoCo"), multi-patient recording, visual dental charting, client summaries + **branded PDFs**, phone documentation, task assignment / multi-role collaboration, clinical decision support via LifeLearn Sofie | 11 named integrations incl. Avimark, Cornerstone, Impromed, Instinct, Vetspire; Chrome extension |
| **HappyDoc** | $119/mo flat, unlimited users | Claims 99.8% accuracy trained on 1.5M+ vet records; flat clinic pricing | Two-way AVImark and Cornerstone (server-based PIMS) |
| **Talkatoo** | $40–50 dictation tier; Ultimate ~$116–126 | Dictation-first (no forced SOAP structure), 14k+ users since 2019, newer ambient mode | Dictation into any field |
| **VetGeni** | $50/mo | Drug database (739+ drugs), Wiley clinical references, education tools, pet-parent portal, free no-account demo tools | Connector catalog |
| **Digitail Tails AI / Tails VIP** | Bundled in Digitail PIMS; Tails VIP standalone app | Ambient SOAP written straight into the record (PIMS-native); Tails VIP for locums/legacy PIMS | Native inside Digitail; VIP is copy-out |
| **Otto AI Scribe** | $49/mo (2 users); $169/mo team unlimited | Cheapest team-wide; PIMS write-back only on contact-priced bundle | Bundle for write-back |
| **PetDesk Scribe** | Bundled in PetDesk | Part of client-comms ecosystem | Syncs via PetDesk's PIMS extension |
| **VetVisitAI** | $50/user/mo | 29 note formats, unlimited | Copy-paste to any PIMS |
| **VetSkribe** | ~$95–145/mo | Hybrid AI + human review | — |
| **General scribes** (Heidi free tier, Freed, DeepCura, Scribeberry) | $0–129/mo | Not vet-native; species templates bolted on | Browser push to EHRs |

### 2.2 Parity checklist (table stakes — all must ship by v1.0)

- Ambient recording of vet–client conversation **and** dictation mode (Talkatoo-style free-form) **and** hybrid (dictate corrections over an ambient draft).
- SOAP generation with structured sections, plus discharge instructions, client summary (plain-language, multilingual), referral letter, callback/phone-call note, medical record summarizer for prior PDFs.
- Specialty template library: GP wellness, sick visit, ER/triage, dental with tooth chart, surgery/anesthesia, oncology (staging, chemo visit, recheck), dermatology, exotics/avian/reptile, equine, bovine/large animal, euthanasia/QoL, recheck/progress note.
- Template builder (no-code) + import/export + an "AI template from example note" generator (CoCo/Adaptive-Templates equivalent).
- Multi-pet appointments (one recording → N patients' notes).
- Multi-language transcription and translated client summaries.
- Teams mode: tech starts the recording, DVM reviews/signs, reception sends the client summary.
- Visual dental chart that the LLM populates from the transcript (dental findings → tooth-by-tooth chart).
- Phone-call documentation (record a call, generate a callback note).
- Branded PDF export (clinic logo, letterhead) for client handouts and referrals.
- 1-click transfer into PIMS: at minimum clipboard-formatted, browser-extension field injection for web PIMS, and API write-back where a partner API exists.
- Consent handling: per-jurisdiction recording-consent prompts (one-party vs two-party), consent captured in the record, optional spoken consent detection.

### 2.3 Where OpenScribe beats them (the differentiators — design everything around these)

1. **Fully local inference.** Audio, transcript, note, and patient data never leave the device/clinic. No vendor BAA, no cloud retention, no "we train on your data." This is the headline.
2. **Zero marginal cost.** No per-seat, no per-note caps (ScribbleVet's 150-SOAP tier, Freed's 40-note tiers). Unlimited users per clinic. Free forever, Apache-2.0.
3. **Bring-your-own-model.** GUI model manager that pulls any compatible model from Hugging Face (Gemma 4 family preloaded), with hardware detection and quality/speed recommendations. Swap STT, LLM, diarization, and embedding models independently. Pluggable endpoints (Ollama, llama.cpp server, LM Studio, vLLM, any OpenAI-compatible URL) for clinics that already run a model server.
4. **Clinic Hub mode.** One clinic computer (or an existing hospital server partition) runs the OpenScribe engine; phones and thin laptops on the LAN discover it (mDNS) and offload inference. Phones can still run fully on-device (Gemma 4 E2B/E4B + Moonshine/Whisper) when off-network.
5. **Open connector SDK.** PIMS connectors are plugins with a documented interface; anyone can add Vetspire, Provet, Neo, Pulse, Hippo, Vetstoria-adjacent, or a homegrown EMR. Includes HL7v2/FHIR-flavoured exports for integration-friendly systems.
6. **Faithfulness-first generation.** Every sentence in the note is traceable to transcript evidence (click a claim → hear the audio span). Unstated items are rendered as "not discussed" rather than hallucinated. This is a trust feature no vendor exposes.
7. **Transparent evaluation.** Ships with a benchmark harness (vet-vocabulary WER, note faithfulness, template adherence) so users can compare models on their own hardware and vendors' "99.8% accuracy" marketing becomes testable.
8. **Research-ready structured output.** Optional structured extraction alongside the narrative note (problem list, diagnoses with controlled vocabulary, medications with dose/route/frequency, diagnostics ordered) emitted as JSON — useful for registries, trials, and QA.

---

## 3. Non-negotiable constraints

- **Local-first, offline-capable.** The default install must work with the network cable unplugged after models are downloaded. No telemetry. No account required. Any optional cloud (BYO API key for a hosted model) is off by default, clearly labelled, and never silently used as a fallback.
- **License:** Apache-2.0 for the app. Only bundle/preload models whose licenses permit commercial redistribution (Gemma 4: Apache-2.0; Whisper: MIT; Parakeet: CC-BY-4.0 with attribution; Moonshine English: MIT — non-English Moonshine variants are non-commercial, so surface them as user-fetched, not preloaded).
- **Platforms:** Web (PWA with WebGPU), Desktop (macOS Apple Silicon + Intel, Windows x64 with CUDA/DirectML/Vulkan, Linux), Mobile (iOS, Android). One UI codebase; one Rust engine; platform-specific inference backends.
- **Security:** Encrypted at rest (SQLCipher or equivalent), OS keychain for secrets, per-clinic multi-user with roles (DVM, RVT, reception, admin), audit log of note edits and exports, configurable audio retention (default: delete audio after note is signed), full export and delete.
- **Clinical safety:** The note is always a draft until a DVM signs. Never auto-send to a client or write to a PIMS without an explicit confirm. Never invent vitals, doses, or findings.
- **Accessibility and speed:** First draft of a 10-minute consult must appear in under 60 seconds on an M-series Mac or a 12 GB-VRAM GPU with the default model set; streaming transcript visible live.

---

## 4. Architecture

### 4.1 Monorepo layout (pnpm + Turborepo; Rust workspace inside)

```
openscribe/
├── apps/
│   ├── desktop/          # Tauri 2 shell (macOS/Windows/Linux) — React UI + Rust engine
│   ├── mobile/           # Tauri 2 mobile targets (iOS/Android); fall back to Expo RN if Tauri mobile blocks on audio/GPU
│   ├── web/              # PWA build of the same React UI; inference via WebGPU (Transformers.js / LiteRT-LM JS) or a Clinic Hub
│   ├── hub/              # Clinic Hub: headless engine + REST/WS API, Docker image, mDNS advertisement
│   └── browser-extension/# Chrome/Edge/Firefox extension: field injection into web PIMS + "send to OpenScribe" from PIMS pages
├── packages/
│   ├── ui/               # Shared React components (design system, recorder, review editor, dental chart, template builder)
│   ├── core/             # TS domain logic: encounters, patients, templates, note schema, PIMS connector interface
│   ├── templates/        # Built-in template library (YAML) + JSON Schemas
│   ├── connectors/       # PIMS connectors (one package per PIMS) implementing the SDK
│   ├── eval/             # Benchmark harness, test transcripts, scoring
│   └── sdk/              # Public TS + Rust SDKs for connectors and model adapters
├── engine/               # Rust workspace
│   ├── os-engine/        # Orchestrator: pipelines, job queue, model registry, hardware probe
│   ├── os-asr/           # STT adapters: whisper.cpp, sherpa-onnx (Parakeet, Moonshine, Qwen3-ASR), streaming VAD
│   ├── os-llm/           # LLM adapters: llama.cpp (GGUF), LiteRT-LM (.litertlm), ONNX Runtime GenAI, remote OpenAI-compatible
│   ├── os-diar/          # Speaker diarization: sherpa-onnx pyannote-segmentation-3.0 + WeSpeaker embeddings
│   ├── os-models/        # Hugging Face Hub client: search, download w/ resume + checksum, format detection, cache
│   └── os-store/         # Encrypted SQLite, migrations, audit log
├── docs/                 # Docusaurus site: user guide, connector SDK, model guide, security whitepaper
└── scripts/
```

**Decision rule for Claude Code:** Tauri 2 (desktop + mobile) with a Rust engine is the default. In Milestone 0, spike the two riskiest assumptions — (a) background audio capture and GPU inference inside Tauri on iOS/Android, (b) llama.cpp + sherpa-onnx linking on all targets. If (a) fails on mobile, switch `apps/mobile` to Expo/React Native with a native module (Cactus SDK or llama.rn + sherpa-onnx RN bindings) and keep the engine API contract identical. Document the decision in `docs/adr/`.

### 4.2 Inference backends by platform

| Platform | LLM | STT | Diarization |
|---|---|---|---|
| macOS (Apple Silicon) | llama.cpp Metal (GGUF, Q4_K_M default); optional MLX adapter later | whisper.cpp CoreML/Metal; Parakeet via sherpa-onnx | sherpa-onnx CPU |
| Windows / Linux (NVIDIA) | llama.cpp CUDA; optional vLLM/Ollama endpoint | whisper.cpp CUDA; Parakeet TDT via sherpa-onnx CUDA | sherpa-onnx |
| Windows (AMD/Intel) | llama.cpp Vulkan | whisper.cpp Vulkan / CPU | sherpa-onnx |
| iOS | llama.cpp Metal (E2B/E4B) or LiteRT-LM; or Clinic Hub | whisper.cpp CoreML small/turbo or Moonshine | sherpa-onnx or skip (2-speaker heuristic) |
| Android | LiteRT-LM (gemma-4-E2B/E4B-it .litertlm, GPU) or llama.cpp; AICore where available | Moonshine / whisper.cpp | litert-community Speaker-Diarization-LiteRT or sherpa-onnx |
| Web (PWA) | LiteRT-LM JS (`@litert-lm/core`) with Gemma 4 E2B/E4B, or Transformers.js ONNX (`onnx-community/gemma-4-E2B-it-ONNX`) on WebGPU; else Clinic Hub | Transformers.js whisper-turbo / Moonshine ONNX on WebGPU/WASM | sherpa-onnx WASM speaker-diarization build |

Design the engine API so the UI never knows which backend served a request. Every backend reports capabilities (streaming, max context, audio-native, structured output) and the orchestrator picks a pipeline.

### 4.3 Model management (the "load from Hugging Face" GUI)

- **Preloaded catalog (curated, pinned revisions, license shown):**
  - *LLM (Gemma 4, Apache-2.0):* `google/gemma-4-E2B-it` (phones, browsers), `google/gemma-4-E4B-it` (phones/thin laptops, default mobile), `google/gemma-4-12B-it` (default desktop — fits 16 GB unified/VRAM, native audio input), `google/gemma-4-26B-A4B-it` (MoE, 16 GB VRAM with offload), `google/gemma-4-31B-it` (clinic hub / workstation). Offer GGUF (community quants and QAT tags), `.litertlm` (litert-community), and ONNX (onnx-community) variants per target.
  - *Medical-adjacent:* `google/medgemma-1.5-4b` as an optional download (human-medical; label clearly).
  - *STT:* `openai/whisper-large-v3-turbo` (default, multilingual), `nvidia/parakeet-tdt-0.6b-v3` (English/European accuracy pick), Moonshine v2 small/medium English (streaming/mobile), `QwenLM/Qwen3-ASR` 0.6B/1.7B (Apache, built-in diarization variant).
  - *Diarization:* pyannote segmentation-3.0 + WeSpeaker ResNet34 (via sherpa-onnx builds).
  - *Embeddings:* `google/embeddinggemma-2` (740M) for prior-record RAG and template retrieval.
- **"Any model" flow:** search HF Hub (name/tag/task), show size, quant, license, context length, and a hardware-fit badge (Fits / Tight / Won't fit) computed from the hardware probe (RAM, VRAM, GPU API, CPU features). Download with resume, SHA-256 verification, disk-space check, and a background queue. Accept HF access tokens for gated repos. Also accept drag-and-drop local GGUF/ONNX/.litertlm files.
- **Model slots:** STT, LLM (note generation), LLM (fast — for live summaries/edits), Diarization, Embeddings. Each slot can point to a local model or a remote OpenAI-compatible endpoint.
- **Profiles:** "Phone — battery saver," "Laptop — balanced," "Clinic Hub — max quality," plus custom. One-click benchmark on-device (tokens/s, RTF, memory) stored in the model card.
- **Reserved slot:** a "Veterinary specialist model" slot so a future open veterinary SLM / veterinary NER model can be dropped in as the preferred generator or as a post-processor (entity normalization), without code changes.

### 4.4 The scribe pipeline

```
Audio in (mic / file / phone call / Bluetooth lapel mic)
  → VAD + chunker (Silero VAD) → streaming STT (live transcript in UI)
  → diarization (speaker turns; user labels DVM / client / tech once; voice-print memory optional, local only)
  → veterinary vocabulary pass (bias list + post-correction dictionary: drug names, breeds, anatomical terms, abbreviations; user-extensible)
  → note generation: template → JSON Schema → constrained/structured generation → render to template sections
  → faithfulness pass: each generated sentence is aligned to transcript spans; unsupported sentences flagged; absent sections rendered "Not discussed"
  → review editor (side-by-side transcript ↔ note, click-to-audio, dictate corrections, accept/reject per section)
  → sign → outputs (PIMS transfer, PDF, client summary, structured JSON) → retention policy runs
```

- Support **audio-native generation** as an experimental path on Gemma 4 E4B/12B (audio → note without separate STT) behind a flag; evaluate against the STT pipeline in the eval harness.
- Streaming everywhere: tokens stream into the note; the user can start editing before generation finishes.
- Long consults: chunk + rolling summary; Gemma 4's 128K/256K context should make most consults single-pass — measure.
- Prior-record context: user drags in prior notes/PDFs (local OCR via Tesseract where rasterized); EmbeddingGemma retrieval feeds relevant history into the prompt; "Medical record summarizer" is a first-class template.

### 4.5 Templates

- Templates are YAML with a JSON Schema for the structured core and a Markdown/Handlebars render layer. Fields carry `required | optional | if_discussed`, species/breed conditionals, and PIMS target hints (which PIMS field this section maps to).
- Builder UI: drag sections, set instructions per section, choose tone/length, preview against a sample transcript, "generate template from this example note," versioning, share as a file or via a git-backed community gallery (`openscribe-templates` repo; app can pull by URL).
- Ship the specialty library in §2.2 plus client-facing outputs (discharge, summary, estimate explanation, euthanasia aftercare) in English, French, Spanish at minimum, with translation generated locally.
- Dental chart: an SVG tooth chart component (canine and feline Triadan numbering) with a schema the LLM fills (missing, fractured, mobility, periodontal stage, extraction, FORL/TR); editable by click.

### 4.6 PIMS integration (open connector SDK)

Connector interface (TS and Rust): `discoverPatients(query)`, `getPatient(id)`, `getUpcomingAppointments()`, `writeNote(patientId, encounterId, sections[])`, `attachPDF()`, `capabilities()`. Every connector declares auth type, write-back support, and a field map from template sections to PIMS fields.

Tiers to implement, in order:
1. **Universal:** formatted clipboard (plain, RTF, Markdown) with per-PIMS formatting presets; global hotkey "paste next section"; PDF/DOCX export.
2. **Browser extension (web PIMS):** detect the PIMS page, map fields, inject sections, pull patient context (name, species, breed, weight, age) into OpenScribe. Targets: ezyVet, Instinct, Shepherd, Digitail, Provet Cloud, IDEXX Neo, Covetrus Pulse, Vetspire, Rhapsody, DaySmart.
3. **Desktop PIMS (server-based):** AVImark, Cornerstone, ImproMed — keystroke/clipboard automation with window detection; document the "HappyDoc-style" two-way path as a future native module.
4. **API connectors (partner APIs, user supplies credentials):** ezyVet (OAuth2 client-credentials, 12-hour tokens, per-database partner UUID — implement token caching and rate-limit pacing), Instinct Partner API (docs.instinctvet.com; access via integrations@instinct.vet), Shepherd, Digitail, Provet Cloud, IDEXX Neo, Covetrus Pulse. Build against sandbox/trial hosts where available; stub where access is gated and leave a clear "request partner access" guide in docs.
5. **Standards exports:** HL7v2 MDM/ORU-style message export and a FHIR R4 `DocumentReference` + `Composition` bundle for integration-friendly systems and research pipelines.

### 4.7 Clinic Hub

- `apps/hub` runs the engine headless with a REST + WebSocket API (OpenAI-compatible `/v1/audio/transcriptions` and `/v1/chat/completions` plus OpenScribe-native `/v1/encounters`). Docker image with CUDA, ROCm, and CPU variants; one-line installer for macOS/Windows.
- mDNS/Bonjour advertisement; clients pair by QR code; mutual TLS with locally generated certs; per-user tokens.
- Shared clinic store (patients, templates, users, audit) with device sync; conflict-free note edits (CRDT or last-writer-wins with version history — pick CRDT if cheap via Yjs/Automerge).
- Admin dashboard: model status, GPU utilization, queue, time-saved metrics (computed locally; no telemetry).

### 4.8 Evaluation harness (`packages/eval`)

- Synthetic + consented real veterinary consult transcripts with gold notes (start with synthetic generated by the user's team; include a contribution guide).
- Metrics: WER overall and on a veterinary term list; note faithfulness (claim-level support), omission rate, template adherence, hallucinated vitals/doses (hard fail), latency, memory.
- CLI: `openscribe eval --stt parakeet --llm gemma-4-12b --templates soap,discharge` → Markdown/JSON report. Surface the same in the GUI "Compare models on my hardware."

---

## 5. UX requirements (make it feel like a product, not a demo)

- Onboarding ≤ 3 screens: hardware probe → recommended model set → one-click download. Show time-to-first-note estimate.
- Recorder: big single button, live waveform, live transcript, patient picker (from PIMS context or manual), template picker, multi-pet toggle, pause/resume, "mark moment" bookmarks.
- Review: two-pane (transcript | note) with evidence highlighting, section accept/reject, voice corrections, keyboard-driven.
- Light/dark, keyboard shortcuts, large touch targets on mobile, works one-handed.
- Design language: calm, clinical, warm; no SaaS clutter. Use the `frontend-design` skill if available; otherwise produce a small token-based design system in `packages/ui`.
- Copy is in plain veterinary English; never expose model jargon on the primary path (advanced settings hold the knobs).

---

## 6. Milestones (ship something usable at each)

**M0 — Spikes & skeleton (week 1–2).** Monorepo, Tauri 2 shell on macOS+Windows, Rust engine with llama.cpp (Gemma 4 E4B GGUF) and whisper.cpp (turbo) end-to-end CLI: `openscribe transcribe file.wav | openscribe note --template soap`. Mobile and web spikes decided and recorded as ADRs. CI builds on all desktop targets.

**M1 — Desktop MVP.** Model manager (preloaded catalog + HF search/download), recorder, streaming transcript, SOAP + discharge + client summary templates, review editor, clipboard/PDF export, encrypted local store, consent prompt. Internal dogfood at one clinic.

**M2 — Templates & faithfulness.** Full specialty library, template builder, "template from example," evidence alignment, "Not discussed" rendering, multi-pet, multilingual summaries, dental chart v1, medical record summarizer with local OCR.

**M3 — PIMS.** Browser extension (ezyVet, Instinct, Shepherd, Digitail first), connector SDK docs, ezyVet API connector against trial host, desktop PIMS clipboard presets, HL7/FHIR export.

**M4 — Clinic Hub + mobile.** Hub Docker + installers, device pairing, iOS/Android clients (on-device E2B/E4B + Hub offload), teams mode and roles, audit log, admin dashboard.

**M5 — Web PWA + eval + 1.0.** WebGPU path, eval harness in GUI, docs site, security whitepaper, template gallery, signed installers, App Store / Play Store builds, launch.

Each milestone: demo script, test coverage report, CHANGELOG, and a short "what a vet can do now" note in `docs/`.

---

## 7. How to work (rules for Claude Code in this repo)

1. **Plan before code.** For each milestone, write `docs/plans/M<n>.md` (scope, tasks, risks, acceptance tests) and get confirmation before large changes. Record every architectural decision as an ADR.
2. **Test first where it matters.** Engine pipeline, connectors, template rendering, and the faithfulness pass have unit + golden tests. UI has component tests; critical flows have Playwright e2e. No milestone closes with failing CI.
3. **Never fabricate.** Verify model IDs, licenses, API endpoints, and package versions against live sources (Hugging Face, vendor docs, crates.io/npm) before pinning. If a PIMS API is gated, say so and stub behind a clear interface.
4. **Local-first discipline.** Any network call must be enumerated in `docs/network.md` with purpose and an off switch. CI has a test that fails if an unlisted host is contacted.
5. **Small, reviewable commits**, conventional-commit messages, PR template with screenshots for UI.
6. **Performance budget:** track time-to-first-token, RTF for STT, and peak memory per profile in CI on at least one macOS and one CUDA runner.
7. **Ask when it matters:** licensing ambiguity, anything that touches patient data retention, PIMS write-back semantics, and store-submission trade-offs. Otherwise make the call, note it, move on.
8. **Documentation is a deliverable:** user guide, connector SDK guide, model guide ("which Gemma for my hardware"), security whitepaper, and a comparison page that states plainly what OpenScribe does and does not do versus the paid scribes.

---

## 8. Non-goals (for now)

- No cloud-hosted OpenScribe service. No accounts. No marketplace payments.
- No autonomous actions in the PIMS beyond writing the note the DVM confirmed.
- No clinical decision support, drug databases, or dosing calculators in 1.0 (leave extension points; Plumb's-style content is licensed and out of scope).
- No training/fine-tuning inside the app (export de-identified data for external training is fine, opt-in).

---

## 9. Open decisions for the product owner (answer before M0 closes)

1. **Name.** "OpenScribe" is already used by an MIT-licensed open-source *human* medical scribe on GitHub (`Pl4tt/OpenScribe`) and by at least one other project. Decide: keep the name (and differentiate as "OpenScribe Vet"), or pick another (e.g., OpenVetScribe, Scribe.vet-style). Check trademarks before 1.0.
2. **Mobile framework** if Tauri mobile fails the M0 spike: Expo/React Native vs native Swift/Kotlin shells.
3. **Default desktop model:** Gemma 4 12B Q4 (quality) vs E4B (speed) when hardware fits both.
4. **Consent defaults** by region (Canada, US one-party vs two-party states, UK/EU).
5. **Template gallery governance:** who reviews community templates; license for contributed templates (CC-BY-4.0 suggested).
6. **Hosting the model catalog metadata:** in-repo JSON (fully offline) vs a tiny static JSON endpoint for updates (one more network call to list).

---

## 10. Reference links (verify before relying on them)

- Gemma 4 release and HF checkpoints: https://huggingface.co/blog/gemma4 ; https://developers.googleblog.com/bring-state-of-the-art-agentic-skills-to-the-edge-with-gemma-4/ ; Gemma 4 12B: https://blog.google/innovation-and-ai/technology/developers-tools/introducing-gemma-4-12b/
- LiteRT-LM JS (browser Gemma 4): https://developers.google.com/edge/litert-lm/js ; LiteRT models: https://huggingface.co/litert-community
- Transformers.js Gemma 4 WebGPU: https://huggingface.co/spaces/shootstuff/Gemma-4-WebGPU ; ONNX: `onnx-community/gemma-4-E2B-it-ONNX`
- EmbeddingGemma 2: https://blog.google/innovation-and-ai/technology/developers-tools/embeddinggemma-2/
- STT landscape 2026: https://www.assemblyai.com/blog/top-open-source-stt-options-for-voice-applications ; https://typilot.com/blog/best-local-speech-to-text-model-2026
- sherpa-onnx diarization: https://k2-fsa.github.io/sherpa/onnx/speaker-diarization/index.html ; WASM build: https://huggingface.co/Transcripta/sherpa-onnx-speaker-diarization-wasm ; Android LiteRT: https://huggingface.co/litert-community/Speaker-Diarization-LiteRT
- ezyVet API: https://developers.ezyvet.com/ ; Instinct integrations & Partner API: https://help.instinct.vet/en/articles/15452896-instinct-emr-integration-partners
- Instinct × ScribbleVet acquisition: https://instinct.vet/news/instinct-science-scribblevet-acquisition-2026/
- Competitor comparisons (vendor-written, cross-check): https://tailwerks.com/best-veterinary-ai-scribe/ ; https://www.vetsoftwarehub.com/article/veterinary-ai-scribe-pricing-comparison-2026 ; https://clunic.com/providers/veterinary ; https://digitail.com/blog/the-ultimate-guide-to-the-best-ai-scribes-for-veterinary-clinics/ ; https://co.vet/post/covet-vs-scribblevet/ ; https://scribblevet.com/conference-signup
- Existing OSS medical scribes to study (not vet): https://github.com/Pl4tt/OpenScribe ; https://github.com/BirgerMoell/open-medical-scribe

---

**Start now with M0.** First reply: the M0 plan in `docs/plans/M0.md`, the two spike designs, and a list of versions you intend to pin (with the sources you checked).
