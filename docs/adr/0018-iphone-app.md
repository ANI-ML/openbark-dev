# ADR-0018: The iPhone app

- Status: accepted (decisions of 2026-10-18; amended by the decisions of 2026-10-19 on mobile models, section 10, and by the phone model package of 2026-10-25, end of section 10). The iOS half of spike S1, run as a CI job plus a TestFlight build on a test iPhone, confirms or replaces the framework choice. ADR-0002 (mobile framework, reserved in M0) records that result for both iOS and Android.
- Date: 2026-10-18 (section 10: 2026-10-19; phone model package: 2026-10-25)
- Deciders: product owner ("Start building the iPhone app"; decisions of 2026-10-18 listed in ADR-0017), engineering
- Owner decisions that shape this ADR: builds, signing and TestFlight uploads run on GitHub Actions macOS runners, with no owner Mac; the phone records and transcribes on the device, and since 2026-10-19 it may also draft with a small model (section 10); audio never leaves the device that recorded it; the bundle id is `health.animl.openbark` and there is no signing on the phone in the pilot (both accepted by default).

## Context

The product owner wants clinicians to record on an iPhone, have the visit processed on the phone, and finish it on any computer they're signed in to. Accounts and sync are in ADR-0017. This ADR covers the app.

The master prompt asks for one React UI and one Rust engine on every platform. Spike S1 designed the mobile test (audio criteria A1 to A5, LLM criteria L1 to L3) but hasn't run yet.

What we know (checked 2026-10-18):
- **Tauri.** Tauri 2.12.4 is the current stable release, and the repo already pins it. No upgrade is needed to target iOS.
- **macOS.** iOS builds need full Xcode on macOS, which GitHub's macOS runners provide. Windows can't build, run or sign an iOS app. It can run `cargo check --target aarch64-apple-ios` on crates that are pure Rust.
- **Speech models.** `sherpa-onnx-sys` 1.13.8 supports Tauri on iOS. Its build script downloads the official xcframework and adds it to the Xcode project through `bundle.ios.frameworks`. `os-asr` already picks shared linking for iOS, and k2-fsa publishes a Tauri iOS demo app.
- **Drafting model.** `llama-cpp-2` on iOS is unproven: PR #949, which added iOS support, was closed without merging. Its `openmp` feature must be off for iOS, because Apple's compiler has no OpenMP.
- **Audio capture.** `cpal` 0.18.2 has an iOS backend. It doesn't set up `AVAudioSession` or the background audio mode, so a small Swift plugin is needed.
- **Code that breaks on iOS today:**
  - `keepawake` ends in `compile_error!` on iOS;
  - `keyring` must use the Apple `protected` store on iOS;
  - `lib.rs` has no mobile entry point;
  - the GPU self-test relaunches the app as a child process, which iOS forbids;
  - the dialog plugin has no folder picker on iOS;
  - the opener plugin can only open URLs on iOS.
- **Memory.** The iPhone 16 family has 8 GB of RAM, and the iPhone 17 Pro reportedly has 12 GB.
  - Parakeet TDT 0.6B v3 int8 is about 670 MB on disk. We estimate it peaks at 1 to 1.5 GB while running.
  - Gemma 4 E2B Q4_0 is 3.35 GB. E4B (5.15 GB) isn't realistic on a phone.
- **Background work.** With `UIBackgroundModes` set to `audio`, a recording started in the foreground keeps going while the phone is locked. Once recording stops, the app gets only about 30 s of background time.
- **App Review.** These guidelines apply: 2.5.4 (background modes), 2.5.14 (recording consent and indicator), 1.4.1 (medical apps), 5.1.1(v) (account deletion), 4.8 (login services) and 5.1.3(ii) (health data in iCloud).

Environment:
- The development box runs Windows and there is no Mac on it.
- GitHub Actions works: the billing problem was fixed on 2026-10-18. The repo is private, so macOS minutes count ten times against the allowance.
- No owner Mac is in the loop (owner decision, 2026-10-18). The owner has an iPhone and installs builds from TestFlight, following scripted runbooks.
- The owner has a paid Apple Developer account (christopher.pinard@animl.health). Whether it is an individual or organisation membership is still to confirm.

## Decision

### 1. Technology

- **One project.** The iPhone app is a Tauri 2.12 iOS target of the existing `apps/desktop` project: the same React UI with a phone layout, and the same Rust engine. There is no second UI codebase.
  - The bundle id is `health.animl.openbark`, from `tauri.conf.json`. It can't change after the first upload to App Store Connect.
- **Native code.** It is limited to one Swift Tauri plugin, `os-ios`, which grows out of S1's `os-audio-session` design. It handles:
  - `AVAudioSession`, interruptions and route changes;
  - the idle timer and background tasks;
  - Face ID through LocalAuthentication.

  Background `URLSession` uploads and audio encoding with `AVAudioConverter` were in the earlier draft. They went with audio sync (ADR-0017 section 7).
- **S1 gates the work.** S1's iOS audio criteria A1 to A4 must pass on a test iPhone, in a TestFlight build made by CI, before feature work starts.
  - If A1 to A3 fail, the fallback is a thin SwiftUI shell for recording and review that calls the same Rust engine through UniFFI (S1 fallback 1).
  - Sync, keys and the engine stay the same either way.
- **Cargo features.** The phone links only what it uses:
  - `os-asr` gains `stt` (sherpa-onnx);
  - `os-engine` gains `inference` (`os-llm`, `os-diar`, note drafting, alignment);
  - `os-app` gains `recorder` and `sync`.

  The S1 build already includes `stt`, because A4 measures live transcription. The first feature build records and transcribes; sync follows. Drafting on the phone is opt-in (section 10): builds with `inference` link `llama-cpp-2` with Metal and without OpenMP once it's proven on iOS; until then the phone build leaves `os-llm`'s llama.cpp out (its `llama` feature), and every crate builds without it.
  - Built in M3 phase 0: `os-asr` has `stt` and `capture`, `os-diar` has `stt`, `os-llm` has `llama`, `os-engine` has `stt`, `inference` and `recorder`, and `os-app` has `recorder`, `stt`, `inference` and `sync` (empty until `os-sync` arrives in phase 2). Desktop members take every default, so desktop builds resolve exactly as before. The Tauri app links `os-app` with `recorder` only on phones; its `stt` and `inference` features add the rest.
- **Minimum system.** iOS 17.

### 2. What runs where

| Work | iPhone | Computer |
|---|---|---|
| Recording | Always. Encrypted as it records (OBA1) | Yes, as today |
| Live and final transcription | Yes, during recording: Parakeet TDT 0.6B v3 int8 and Silero VAD through sherpa-onnx | Yes, for its own recordings, as today |
| Drafting the note | Optional, with a small model that fits the phone's memory tier, never loaded while speech recognition is (decision of 2026-10-19, section 10). Off until the person turns it on | Yes, as today (Gemma 4 E4B, 12B or larger), including for visits recorded on a phone |
| Review | Read the transcript, play the audio of visits recorded on this phone, read the note, copy | Full review, Edit and Ask. For a visit recorded on a phone, the evidence link shows the transcript line and its time; the audio plays only on that phone |
| Signing | Not in the pilot (accepted by default) | Yes, as today |
| Templates | Choose one for the visit | Build and edit |
| Model downloads | Through `os-models` (network.md R1), on Wi-Fi by default. The four speech models come inside the app (amended 2026-10-25, phone model package in section 10); the drafting model is a download | As today |

Speech recognition and the LLM are never loaded at the same time. Before loading a model, the app checks `os_proc_available_memory()`.

### 3. Phone modes

M3 has one mode: **record and transcribe.**

- It needs a phone with 6 GB or more (A15 chip or newer) and the speech models (about 0.7 GB), which come inside the app since 2026-10-25 (phone model package, section 10). The thresholds are revised once S1 has measured memory use.
- **Before the models are downloaded,** the phone can still record. The visit stays on the phone, marked "Not transcribed yet". Once the models are in place, the phone transcribes it, and only then do the transcript and notes sync.
- **Record-only phones** (too old to run the models) can't be supported. Sync never carries audio, so nothing they record could reach a computer (ADR-0017 section 7). A future decision could let such a phone hand its audio to a computer over the clinic's local network, never through the cloud.
- **Record, transcribe and draft:** allowed since 2026-10-19 (section 10), opt-in, with the tier's small model. It needs the `com.apple.developer.kernel.increased-memory-limit` entitlement and `llama-cpp-2` built with Metal and without OpenMP (or a vendored llama.cpp xcframework if the crate can't build for iOS).

### 4. Background recording

- **Background mode.** `Info.plist` sets `UIBackgroundModes` to `audio`. Recording must start in the foreground, as iOS requires, and then continues while the phone is locked, with the system's recording indicator showing.
- **Audio session.**
  - Category `.record`, or `.playAndRecord` where playback is needed; spoken-audio mode; Bluetooth input allowed.
  - Interruptions such as calls and Siri are logged in `session.json`, and recording resumes afterwards.
  - Route changes, such as AirPods or a lapel microphone, are handled.
- **Capture thread.** Capture runs on a Rust thread that writes encrypted chunks. It doesn't depend on Tauri commands while the app is in the background, which matters because tauri#12523 reports odd command behaviour there.
- **Screen.** The idle timer is off while the recorder screen is open.
- **After Stop.**
  - The app has only about 30 s of background time (`beginBackgroundTask`). Because transcription runs during recording, little is left to do.
  - Anything left resumes when the app returns to the foreground, and the audio is safe on disk meanwhile.
  - Where it is available, `BGContinuedProcessingTask` shows progress. This is believed to be iOS 26 and is checked live before use.
- **Uploads.** Only transcripts and notes upload, and they are small. They go in the time left after Stop, or when the app next opens. No background `URLSession` is needed. The recording itself never uploads.
- **Consent.** The phone uses the same notice and the same optional consent step as the desktop. Guideline 2.5.14 asks for a clear indication while recording.

### 5. Storage and keys on the phone

- **Visit folders.** Visits live in `Application Support/OpenBark/sessions`, in the same folder format and with the same vault encryption as the desktop. `session.rs`, `patients.rs` and their tests apply unchanged.
- **Backups and file protection.**
  - The folders are excluded from backup (`isExcludedFromBackup`).
  - Files use "complete until first user authentication". Full "complete" protection would block writing while the phone is locked.
- **Keys.** They use `keyring` with the Apple `protected` store, set to "after first unlock, this device only". There is no key-file fallback on iOS.
- **App lock.** Face ID or the passcode opens the app, on by default. The team PIN picker isn't used at first, because a phone belongs to one person.
- **iCloud.** Nothing is stored in iCloud (guideline 5.1.3(ii)).
- **Space.** The app checks free space before recording. An hour of PCM is 115 MB.
- **Recordings stay on the phone** until the user deletes them, because they never upload. Settings shows the space they use. Automatic clean-up of old recordings on the phone is not in M3 and would need a new decision.

### 6. Screens

The screens follow `docs/design/DESIGN.md` and the brand guide (ADR-0016). They are built and tested first in a desktop browser at phone width, on Windows.

- **Record:** Find a patient, kind of visit, reason, template, the record button, a level meter and a timer.
- **Today:** the day's visits, each with its status: Not transcribed yet, Transcribing, On this phone only, Synced, Draft ready, Signed.
- **Visit:** the transcript, playback of this phone's recording, the note (read-only) and Copy.
- **Patients.**
- **Settings:**
  - Account;
  - Devices: Scan to add a device, Remove;
  - Recovery Kit;
  - Sync, with the plain statement "Sync carries transcripts and notes. Recordings stay on this iPhone.";
  - Models, with Wi-Fi only for downloads;
  - Storage used by recordings;
  - Delete my account.

Hidden on the phone:
- the folder choice in Settings > Storage (ADR-0013), because iOS has no folder picker;
- Show files, because the opener can only open URLs;
- the GPU speech pack (ADR-0014);
- the template builder.

### 7. Code changes before any iOS build passes

| Change | Why |
|---|---|
| Move `keepawake` under a dependency limited to Windows, macOS and Linux | It hits `compile_error!` on iOS |
| Use `keyring` with the Apple `protected` store on iOS, and give `vault.rs` an iOS branch | iOS can only use the protected store. The vault's keychain code is compiled for desktop systems only |
| Turn off the `openmp` feature of `llama-cpp-2` for iOS | Apple's compiler has no OpenMP. While drafting stays on computers, the phone doesn't link `llama-cpp-2` at all; this keeps a build with `inference` from failing by accident |
| Add `#[cfg_attr(mobile, tauri::mobile_entry_point)]` to `run` in `lib.rs` | Tauri mobile requires it |
| Make the GPU self-test desktop-only | `main.rs` relaunches the app as a child process, which iOS forbids |
| Hide the folder picker and Show files on mobile (`engine.ts`) | Not supported by the plugins on iOS |
| In `tauri.conf.json`: add an iOS bundle section, an asset protocol scope for the iOS data folder, and limit window sizes to desktop | `minWidth` 720 and the current scope are desktop-only |
| In `Info.plist`: add `UIBackgroundModes` `audio`, `NSCameraUsageDescription` (QR scanning), `NSFaceIDUsageDescription` and `ITSAppUsesNonExemptEncryption`, and reword `NSMicrophoneUsageDescription` | The current microphone text, "Audio stays on this device", stays true, because audio never syncs. It is reworded to say what the app does: "OpenBark records your consultations to transcribe them on this iPhone. Recordings stay on this iPhone." |
| Run `tauri ios init` in a manually started GitHub Actions macOS job, and commit the `src-tauri/gen/apple` it produces | It doesn't exist yet, and there is no Mac to run it on |

A Windows script, `scripts/ios-check.sh`, runs `cargo check --target aarch64-apple-ios` on the recorder and sync crates without default features. It catches most mistakes before a macOS CI run. sherpa-onnx still needs the Apple SDK, so only a macOS runner can check it.

### 8. Building and signing on GitHub Actions macOS runners

Owner decision, 2026-10-18: every iOS build, signature and TestFlight upload runs on GitHub Actions macOS runners. Billing is fixed and Actions works. No owner Mac is in the loop.

| Job | Runs | Does |
|---|---|---|
| iOS init | Started by hand, once | Runs `tauri ios init` and offers `src-tauri/gen/apple` as a build artifact to commit |
| iOS check | On pull requests that touch the engine, the app or `gen/apple` | Builds for the simulator, including sherpa-onnx, and runs a smoke test that the WebView loads. No signing |
| S1 audio check | Started by hand | Builds the S1 test app, runs the simulator checks, signs it and uploads it to TestFlight. The owner installs it and runs the device checks (M3 phase 1) |
| TestFlight | On a tag, or started by hand | Builds, signs and uploads the app |

**Cost.** macOS minutes cost 0.062 USD each and count ten times against the allowance on a private repo. So the signed jobs run on tags or by hand, never on every push, and the Rust build and sherpa-onnx archive are cached.

**Signing.**
- The owner adds the secrets to GitHub. They are never in the repo:
  - the Apple distribution certificate as a `.p12` file, and its password;
  - an App Store Connect API key: the `.p8` file, its key id and the issuer id.
- The job imports the certificate into a temporary keychain on the runner and deletes it at the end. The API key lets the build fetch the App Store provisioning profile.
- The build command is `tauri ios build --export-method app-store-connect`. The upload uses `xcrun altool --upload-app` with `--apiKey` and `--apiIssuer`.
- The exact secret names follow Tauri's iOS signing guide, checked live before the workflow is written.
- Without a Mac, the owner makes the certificate request and the `.p12` with OpenSSL on Windows. `docs/runbooks/ios.md` gives the steps, and how to create the app record for `health.animl.openbark` in App Store Connect.

**Fallbacks.** Xcode Cloud, Codemagic or a rented Mac are used only if GitHub's runners can't build the app.

**Distribution.**
- The pilot uses TestFlight internal testing: up to 100 testers, with no review.
- External testers need Beta App Review, and TestFlight builds expire after 90 days.
- The public App Store comes after the external security review.

Windows handles everything else: all the Rust except the Apple SDK build scripts, all the UI, sync, Supabase and the tests. M3 puts that work first.

### 9. App Store

- **Works without an account.** The app is local-first and fully usable with no account, which is what guideline 5.1.1 expects. App Review can record and transcribe with no backend.
- **Review notes.** They explain sync and include a demo account with fictional data and its Recovery Kit, so a reviewer can try sync without a second device.
- **Background audio.** It is justified as continuous recording of a consultation (2.5.4), with the recording indicator and consent (2.5.14).
- **Medical framing (1.4.1).** A draft is not a record until a veterinarian signs it. The app is for animal care.
- **Account deletion.** In the app (5.1.1(v)).
- **Privacy label.**
  - Contact info (email), used for app functionality and linked to the user, applies only when signed in.
  - Audio never leaves the phone, so it isn't collected.
  - Before submitting, we check against Apple's definitions whether end-to-end encrypted transcripts and notes count as "collected". If in doubt, declare user content, not used for tracking.
- **Export compliance.**
  - The app uses standard algorithms: XChaCha20-Poly1305, X25519, Ed25519, HKDF and TLS.
  - Answer App Store Connect's questions and set `ITSAppUsesNonExemptEncryption` to match.
  - Before external release, check whether a BIS annual self-classification report and France's declaration apply.
- **Nothing extra.** Version 1 has no analytics, no crash reporting SDK and no push notifications. A later "Draft ready" notification carries no content.
- **Seller name.** An individual membership shows the person's name. An organisation membership shows ANI.ML Health Inc., and needs a D-U-N-S number. Which one the owner's account is remains to confirm, before the first public release.

### 10. Mobile models (decisions of 2026-10-19)

The owner decided on 2026-10-19:

> 1. "In the iPhone app it preferentially loads mobile small parameter versions that can be used on an iPhone."
> 2. "The Hugging Face library available on mobile should only be mobile friendly models."

| # | Decision (2026-10-19) | What changes |
|---|---|---|
| 1 | **On iOS the default models are the small, mobile ones.** Speech recognition: Parakeet TDT 0.6B v3 int8 through sherpa-onnx, or a smaller sherpa-onnx model if Parakeet doesn't fit. Drafting: a small GGUF on llama.cpp with Metal | This **amends "drafting stays on computers"** (decision 3 of 2026-10-18, sections 1 to 3 and Consequences). The phone **may** draft on the device with a small model when it can run one, and can still hand drafting to a computer through sync later |
| 2 | **On iOS the Hugging Face browser and the catalog show only mobile-friendly models:** GGUF only (no MLX), single-file or split GGUFs within a memory budget, a parameter cap, and quantisations suited to phones | The phone profile below. MLX is never shown on a phone: OpenBark's MLX engine is a Python pack (ADR-0025), which can't run on iOS (and App Review doesn't allow downloading code) |

**Drafting on the phone is opt-in, not on by default** (engineering's call, asked for by the owner). Why:
- `llama-cpp-2` on iOS is unproven (its iOS pull request, #949, was closed unmerged) and S1's drafting criteria L1 to L3 haven't run on a phone. Recording and transcription must never be put at risk by it.
- Speech recognition and the drafting model are never loaded together (section 2). Drafting on the phone means unloading Parakeet first, so it is something the person chooses after the visit, not a step that runs by itself.
- The model is a 1.1 to 3.4 GB download, on Wi-Fi; battery use and heat in the exam room are real costs.
- Notes from 2B-class models haven't been measured against the eval fixtures yet. Gemma 4 E4B, the computers' default, is the quality reference.

So the phone needs only the speech models to start (onboarding lists voice detection, Parakeet and the two speaker models; no drafting model). Since 2026-10-25 those four come inside the app, so nothing is downloaded before the first recording (phone model package, below). The Models screen offers the tier's default first. Drafting on the phone becomes on by default only by a new product decision, after the first TestFlight build with `inference` passes S1's L1 to L3 on a test iPhone and the eval shows the tier default meets the bar.

**Memory tiers.** iOS gives each app a memory limit (jetsam) that Apple doesn't publish. The `com.apple.developer.kernel.increased-memory-limit` entitlement raises it on some devices, and `os_proc_available_memory()` reports what's left (advisory; it can change while the app runs). llama.cpp maps GGUF files, so most weight pages don't count against the limit: on an iPhone Air, Llama 3.2 3B peaked at 696 MB of `phys_footprint` under llama.cpp against 2,508 MB under MLX. The KV cache, Metal buffers, the WebView and the app do count. The budgets below are for the GGUF files on disk, set under the headroom measured without the entitlement, so a phone without it still loads the default.

| Tier | iPhones (iOS 17 or later) | Measured headroom for one app | Drafting budget (all GGUF parts) | Parameter cap | Default drafting model | Expected speed (llama.cpp Metal, Q4) |
|---|---|---|---|---|---|---|
| Under 6 GB | XR (3 GB); XS, 11, 12, 13 (4 GB) | not useful | none | none | none. OpenBark doesn't run on these phones (section 3) | |
| 6 GB | 12 Pro, 13 Pro, 14, 14 Plus, 14 Pro, 15, 15 Plus | 13 Pro: 3,125 MB at launch without the entitlement, 4,200 MB with it; a 1.7B Q8_0 used about 330 MB of it once loaded | **1.3 GB** | **2B** | None since 2026-10-25: no model within the budget met the bar, so these phones don't draft (Qwen3 1.7B Q4_K_M, 1.11 GB, until then) | A16: about 54 tokens/s generating for 1.1B Q4_0, 23 for 2.7B; 1.7B in between |
| 8 GB | 15 Pro, 15 Pro Max, the 16 family and 16e, 17, 17e | 15 Pro Max: about 3.3 GB without the entitlement, about 6 GB with it (one report) | **3.5 GB** | **4.5B** (effective, so Gemma's E2B counts as 2B) | **Qwen3 4B Instruct 2507 Q4_K_M** (2.50 GB) since 2026-10-25 (phone model package, below); Gemma 4 E2B QAT Q4_0 (3.35 GB) until then, still offered | A17 Pro and A18 Pro: 57 to 70 tokens/s for 1.1B, 23 to 25 for 2.7B; Llama 3.2 3B Q4_K_M 18 (15 Pro) to 22 (16 Pro) |
| 12 GB | Air, 17 Pro, 17 Pro Max, 18 Pro | not published. iPhone Air reports a Metal working set of 8,192 MB; an 8B Q4_K ran at about 9 tokens/s and a 13B didn't load | **5.3 GB** | **8.5B** | **Qwen3 4B Instruct 2507 Q4_K_M** since 2026-10-25; Gemma 4 E2B and E4B QAT Q4_0 (5.15 GB) can be chosen | A19 Pro: 87 tokens/s for 1.1B, 36 for 2.7B; Gemma 3 4B about 17, Llama 3.2 3B about 25 |

- **Superseded on 2026-10-25** (phone model package, below): the eval found Gemma 4 E2B writing two wrong numbers (a tenfold acepromazine dose and a wrong weight), so Qwen3 4B Instruct 2507 is now the default on 8 and 12 GB. The reasoning kept for the record: **why E2B on 8 GB despite its 3.35 GB:** 1.93 GB of the file is Gemma 4's per-layer embedding table (Q6_K). llama.cpp keeps input-layer tensors on the CPU, mapped from the file, so they never enter the Metal working set; the Q4_0 transformer weights are about 1.05 GB. A developer shipping llama.cpp with Gemma 4 E2B (Q3_K_S) on 6 GB and larger phones needed both memory entitlements and a 1,024-token context; 4 GB phones were unusable. So the 6 GB tier gets Qwen3 1.7B instead, and E2B waits for the 8 GB tier.
- **Why not E4B by default on 12 GB** (still holds on 2026-10-25, see `docs/eval/phone-models.md`): E2B is faster, uses less battery, and is the same family as the computers' default, so prompts and schemas behave the same. E4B stays a choice; LiteRT-LM loaded it 36 times out of 36 on an iPhone 17 Pro with either entitlement (peak about 4.7 GB) and failed without them.
- **Speech:** Parakeet TDT 0.6B v3 int8 (670 MB) on every tier. ONNX Runtime doesn't map its weights, so plan for about the file size plus working memory (a CoreML build of Parakeet v2 lists about 800 MB peak). That fits every supported tier's measured headroom, so no smaller model is built now. If phase 1 measures otherwise, the fallback is `sherpa-onnx-nemo-parakeet_tdt_transducer_110m-en-36000-int8` (k2-fsa release `asr-models`, 108,035,095 bytes, SHA-256 `f628312e9fdf8686374cb01a69425c41732529d540860311f16f37cbc32cfe9b`, CC-BY-4.0, the same `nemo_transducer` path; it ships as a `.tar.bz2`, so it would need an unpacked mirror or bzip2 on iOS).
- **The entitlement** `com.apple.developer.kernel.increased-memory-limit` goes into the app's entitlements with the first build that turns on `inference`; the Account Holder enables "Increased Memory Limit" on the app id (docs/runbooks/ios.md). `com.apple.developer.kernel.extended-virtual-addressing` is not needed while llama.cpp maps files of a few GB.

**Quantisations offered on a phone:** 3 to 5-bit builds: Q3_K_M, Q3_K_L, IQ4_XS, IQ4_NL, Q4_0, Q4_1, Q4_K_S, Q4_K_M, Q5_0, Q5_K_S, Q5_K_M and Unsloth's dynamic "XL" versions of those. Q8_0 only for models of 1B parameters or fewer (Qwen3 0.6B ships only Q8_0). Never 16 or 32-bit, Q6_K, or 2-bit and IQ1 to IQ3_XXS builds. A file whose quantisation can't be read from its name isn't offered.

**Parameter counts** come from the Hub's GGUF summary when a repo is opened on a phone (`expand[]=gguf`, its `total`), else from the name ("1.7B", "270M"; Gemma's "E2B" counts as 2B). Search results are filtered by the name only; a name without a size stays in the list and its files are checked when it's opened.

**What a phone shows** (built in M3 phase 0, `os-models::platform`):
- The catalog lists only entries marked `mobile` (the drafting models above and the speech models). An entry over this phone's tier shows "Too large for this iPhone (8 GB): 5.2 GB. Models up to 3.5 GB fit." and can't be downloaded. Models made only for small phones (Qwen3 1.7B) aren't listed on computers.
- The Hugging Face browser searches GGUF only, drops repos whose names are over the cap, never shows MLX, and lists a repo's files that don't fit under "Not offered on this iPhone" with the reason. A model added on the phone is marked `mobile`.
- Adding a model from files, the GPU speech pack and the MLX engine card aren't offered.
- `OPENBARK_PLATFORM=ios` turns this profile on in a computer build, for demos and tests; `OPENBARK_DEVICE_RAM_GB` stands in for the phone's memory. Desktop behaviour and requests are unchanged.

**Pinned catalog entries** (sizes and SHA-256 from the Hugging Face API, checked live on 2026-10-19):

| Entry | Repo @ revision | File | Bytes | SHA-256 | Licence | Tiers |
|---|---|---|---|---|---|---|
| `qwen3-1.7b-q4_k_m` (new) | `unsloth/Qwen3-1.7B-GGUF` @ `d7f544eead698dbd1f15126ef60b45a1e1933222` | `Qwen3-1.7B-Q4_K_M.gguf` | 1,107,409,472 | `b139949c5bd74937ad8ed8c8cf3d9ffb1e99c866c823204dc42c0d91fa181897` | Apache-2.0, not gated; 1.72B parameters | 6, 8, 12 GB (computers don't list it) |
| `gemma-4-e2b-it-q4_0` | `google/gemma-4-E2B-it-qat-q4_0-gguf` @ `675cff42a74c774d6cb76f76d8eacb49b48c9b93` | `gemma-4-E2B_q4_0-it.gguf` | 3,349,516,256 | `fa401b55b07ee70a54c6dae3903c783a6e65064312529ea57175cb5f8dec6634` | Apache-2.0, not gated | 8, 12 GB |
| `gemma-4-e4b-it-q4_0` | `google/gemma-4-E4B-it-qat-q4_0-gguf` @ `4b4a2c1d584be7264f87aac328a1bc739ce81b6c` | `gemma-4-E4B_q4_0-it.gguf` | 5,154,941,280 | `676c35070db6dbe52f93e9c864ee0fba4eddea94b9c875d9cb10daff453fbaee` | Apache-2.0, not gated | 12 GB |
| `parakeet-tdt-0.6b-v3-int8`, `silero-vad`, `pyannote-segmentation-3.0`, `wespeaker-en-resnet34-lm` | as in the catalog | | | | as in the catalog | every tier |

Considered and not pinned (the Hub browser still offers them when they fit): Qwen3 4B Instruct 2507 Q4_K_M (`unsloth/Qwen3-4B-Instruct-2507-GGUF`, 2,497,281,120 bytes, Apache-2.0) for 8 GB phones; Phi-4-mini-instruct Q4_K_M (2,491,874,272 bytes, MIT, 3.84B); SmolLM3 3B Q4_K_M (1,915,305,312 bytes, Apache-2.0); Llama 3.2 1B and 3B Instruct (807,694,464 and 2,019,377,696 bytes at Q4_K_M) under the Llama 3.2 Community License, which isn't permissive, so the browser asks for it to be accepted; Qwen3 0.6B (Q8_0 only from Qwen, 639,446,688 bytes), too weak for notes. Qwen3 is a thinking model; drafting constrains its output to the template's schema from the first token, and that needs checking in the eval before Qwen3 1.7B's notes are trusted.

### Phone model package (2026-10-25)

The owner asked on 2026-10-25: "For the iPhone - make sure a package of phone-friendly models is loaded (ie nemotron or others)." and later: "If you don't think nemotron was best and you have other ideas that's fine too." This ships in 0.2.1 with the first TestFlight build.

- **Speech models inside the app.** `silero-vad`, `parakeet-tdt-0.6b-v3-int8`, `pyannote-segmentation-3.0` and `wespeaker-en-resnet34-lm` (703,646,089 bytes together) are `bundle > resources` in `tauri.ios.conf.json`. The iOS build jobs fetch them with `scripts/ci/ios-models.py`, which checks every file's size and SHA-256 against `catalog.json`. In the app they sit in `<app>/assets/models`, which `ModelStore` searches first, read-only: a model found there is never downloaded again, can't be removed, and shows as "Built in" with its credit on the Models screen. Desktop builds are unchanged. The iOS check job's smoke test checks that the engine finds all four in the app with nothing downloaded.
- **Transcription on the phone works** (fixed 2026-10-25). The phone builds (iOS check and TestFlight) turn on `stt`. The first try failed at link because the Xcode project never linked sherpa-onnx's framework (every sherpa-onnx symbol was undefined, not only the punctuation ones first noticed). The iOS library is the official k2-fsa v1.13.8 xcframework, the version of the `sherpa-onnx` crate and the archive its build script downloads; `scripts/ci/ios-sherpa.py` fetches it by pinned SHA-256 (docs/versions.md), and `project.yml` links and embeds it (XcodeGen regenerates the project at build time, `scripts/ci/ios-prepare.sh`). The iOS check's smoke test transcribes the committed synthetic smoke clip with the built-in models on the simulator and fails on an empty transcript. Desktop sherpa-onnx is unchanged.
- **Drafting stays opt-in** (owner decision of 2026-10-19, confirmed 2026-10-25): Write notes on this iPhone is off until the person turns it on, on every tier, and isn't offered on 6 GB iPhones.
- **The drafting model on demand.** Turning on Write notes on this iPhone offers the tier's default with its size ("Wi-Fi is recommended") and downloads it with the usual progress. The other phone models that fit stay in Models.
- **A first-run walkthrough** (owner request, 2026-10-25: "Make sure there is a user walk through on iPhone"). Four short screens, skippable from each, following Apple's onboarding guidance: what OpenBark does on the phone and that recordings stay on it; recording and consent, with the consent text shared with the desktop notice and the microphone asked for only there, just in time; notes on this iPhone (speech models built in, the drafting model offered); and where encrypted sync with the clinic computer is set up later. Then Record. Shown once on a first run, and again from Settings, Help, Show the walkthrough. Computers keep the consent notice.
- **Nemotron 3 Nano 4B** (`nemotron-3-nano-4b-q4_k_m`, NVIDIA's own GGUF, 2,837,072,864 bytes, 3.97B parameters, a hybrid Mamba-2 and Transformer model) is in the catalog for phones of 8 GB and up, under the NVIDIA Nemotron Open Model License (a copy ships with the app; NOTICE carries NVIDIA's statement). The pinned llama.cpp registers `nemotron_h`; CI drafts a note with it on Linux.
- **Tier defaults from the eval.** `docs/eval/phone-models.md` compares the candidates on every fixture from its reference transcript (`openbark eval --from-ref`); the defaults and their reasons are recorded there and in the table below.

| Tier | Default before | Default from 2026-10-25 | Why (`docs/eval/phone-models.md`, synthetic consults from their reference transcripts, Linux CPU) |
|---|---|---|---|
| 6 GB | Qwen3 1.7B Q4_K_M | **None: no drafting on 6 GB iPhones** (owner decision, 2026-10-25) | No candidate within 1.3 GB and 2B passed: Qwen3 1.7B, Granite 4.0 1B and Gemma 3 1B all wrote numbers that weren't said (doses among them), and the last two also ran past the token limit on some consults. So a 6 GB iPhone records and transcribes, and Write notes on this iPhone is replaced by "Writing notes on the phone needs an iPhone with 8 GB of memory or more. Record here, and draft the note on your computer." |
| 8 GB | Gemma 4 E2B QAT Q4_0 | Qwen3 4B Instruct 2507 Q4_K_M | The only phone candidate with no invented number once three known gaps of the number check on reference transcripts are set aside (each listed there), and the most facts found (94%) and the best tooth charts. Apache-2.0, 2,497,281,120 bytes, 4.02B parameters. Gemma 4 E2B wrote a tenfold dose and a wrong weight; Nemotron 3 Nano 4B, Phi-4-mini, SmolLM3 and Granite 4.0 H Micro also invented numbers |
| 12 GB | Gemma 4 E2B QAT Q4_0 | Qwen3 4B Instruct 2507 Q4_K_M | As for 8 GB. Gemma 4 E4B, the computers' default and the eval's reference, scored a point higher, but its 5.15 GB file leaves the least margin under the 5.3 GB budget and needed the memory entitlements under LiteRT-LM; it stays selectable |

- **Memory to measure on TestFlight.** llama.cpp maps the weights, but the KV cache counts against the app's limit: for Qwen3 4B (36 layers, 8 KV heads of 128) it is about 144 KiB per token at 16-bit, so about 0.9 to 1.3 GB for the 6,000 to 9,000-token contexts the fixtures need (prompt plus up to 4,096 new tokens). That sits inside the 3.3 GB measured on an 8 GB phone without the entitlement, with the WebView and Metal buffers on top. The first TestFlight build with `inference` checks `phys_footprint` against this; if it's too tight, the KV cache can be quantised or the context capped.
- **Nemotron 3 Nano 4B** loads and drafts on the pinned llama.cpp and stays in the catalog for 8 and 12 GB phones, not as a default.

**To revisit:** after phase 1 (Parakeet's real peak memory on a test iPhone) and after the first TestFlight build with `inference` (llama.cpp's `phys_footprint`, tokens per second and heat for each tier default). The budgets move with those measurements.

**Sources** (checked live on 2026-10-19):
- Apple, increased memory limit entitlement: https://developer.apple.com/documentation/bundleresources/entitlements/com.apple.developer.kernel.increased-memory-limit
- Apple, extended virtual addressing: https://developer.apple.com/documentation/bundleresources/entitlements/com.apple.developer.kernel.extended-virtual-addressing
- Apple, `os_proc_available_memory`: https://developer.apple.com/documentation/os/os_proc_available_memory
- 6 GB headroom with and without the entitlement (iPhone 13 Pro, PocketPal AI): https://github.com/a-ghorbani/pocketpal-ai/pull/651
- 8 GB headroom (iPhone 15 Pro Max): https://github.com/SideStore/SideStore/issues/1616
- Entitlements and Gemma 4 E4B on iPhone 17 Pro: https://zenn.dev/mtfum/articles/ios_memory_entitlements?locale=en
- iPhone Air Metal working set and 8B/13B results: https://developer.apple.com/forums/thread/805161
- llama.cpp against MLX memory on iPhone Air: https://ananasdda.github.io/pocket-ai-lab/benchmarks/
- llama.cpp on iPhones (llama.swiftui bench, A16 to A19 Pro): https://github.com/ggml-org/llama.cpp/discussions/4508
- Llama 3.2 3B on iPhone 15 Pro and 16 Pro: https://pocketllm.app/blog/llama-3-2-iphone-benchmarks/
- Gemma 4 E2B with llama.cpp in a shipped iOS app: https://reddit.sentinel-team.org/posts/1sxzvkt/snapshots/2026-05-02T03%3A06%3A21.60641Z
- llama.cpp Gemma 4 per-layer embeddings: https://github.com/ggml-org/llama.cpp/issues/22243 and `src/llama-model-loader.h`
- iPhone memory by model: https://en.wikipedia.org/wiki/IPhone_17_Pro, https://en.wikipedia.org/wiki/IPhone_Air, https://en.wikipedia.org/wiki/IPhone_16e, https://en.wikipedia.org/wiki/IPhone_17, https://www.macrumors.com/2023/09/12/iphone-15-pro-models-have-8gb-ram/
- Model files: the Hugging Face API for each repo above (`/api/models/<repo>` and `/tree/main`); sherpa-onnx models: https://api.github.com/repos/k2-fsa/sherpa-onnx/releases/tags/asr-models

## Consequences

- **One codebase.** Most of the phone UI is built and tested on Windows.
- **macOS CI is the bottleneck.** Each iOS build costs macOS minutes, and each check on a real iPhone waits for the owner to install a TestFlight build. Every owner step has a scripted runbook, and builds are batched.
- **Size.** The app is about 60 MB without models. Since 2026-10-25 the four speech models come inside it: 703,646,089 bytes installed, and about 0.52 GB more to download (zipped as an IPA stores them, measured by the iOS check). sherpa-onnx's framework adds about 26 MB on a device (its arm64 slice, ONNX Runtime included). The drafting model stays a download (phone model package, section 10).
- **If S1's audio criteria fail,** the SwiftUI shell costs about 2 more weeks. Sync and the engine are unaffected.
- **Drafting on the phone is optional** (section 10). The phone is useful without it, because it records and transcribes; computers draft as before, including visits recorded on a phone.
- **Audio stays on the phone that recorded it.** A computer shows the transcript line and its time, but can't play the phone's recording.
- **Android** can follow on the same engine, recorded in ADR-0002.

## Alternatives considered

| Option | Why not |
|---|---|
| A native SwiftUI app from the start | A second UI to build and maintain. It is kept as the fallback if S1 fails |
| React Native or Flutter | A second UI language plus a bridge to Rust. It contradicts the master prompt |
| A web app in Safari | No background recording, no practical on-device models, and keys handled in the browser |
| A phone that only records | The owner asked for processing on the device, and sherpa-onnx officially supports Tauri on iOS. Audio never syncs, so a record-only phone would strand its recordings. It is not a mode in M3 |
| The owner's Mac, Xcode Cloud or Codemagic as the main build path | The owner decided on 2026-10-18 that builds run on GitHub Actions macOS runners, with no Mac in the loop. The others stay as fallbacks |
| LiteRT-LM for drafting on the phone | Faster and proven on iOS, but it is a second LLM stack, which breaks the "one ggml" rule |
| Bundling the models in the app | Rejected on 2026-10-18 for size and licences. **Reversed for the speech models on 2026-10-25** at the owner's request (phone model package, section 10): their licences (MIT, CC-BY-4.0) allow it with the credits in NOTICE and on the Models screen, and recording and transcription then work offline right after install. The App Store asks before a download this size on cellular, which is acceptable for a clinic app. Drafting models stay downloads |
