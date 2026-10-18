# ADR-0018: The iPhone app

- Status: accepted (owner decisions 2026-10-18). The iOS half of spike S1, run as a CI job plus a TestFlight build on the owner's iPhone, confirms or replaces the framework choice. ADR-0002 (mobile framework, reserved in M0) records that result for both iOS and Android.
- Date: 2026-10-18
- Deciders: product owner ("Start building the iPhone app"; decisions of 2026-10-18 listed in ADR-0017), engineering
- Owner decisions that shape this ADR: builds, signing and TestFlight uploads run on GitHub Actions macOS runners, with no owner Mac; the phone records and transcribes on the device and drafting stays on computers; audio never leaves the device that recorded it; the bundle id is `health.animl.openbark` and there is no signing on the phone in the pilot (both accepted by default).

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
- **S1 gates the work.** S1's iOS audio criteria A1 to A4 must pass on the owner's iPhone, in a TestFlight build made by CI, before feature work starts.
  - If A1 to A3 fail, the fallback is a thin SwiftUI shell for recording and review that calls the same Rust engine through UniFFI (S1 fallback 1).
  - Sync, keys and the engine stay the same either way.
- **Cargo features.** The phone links only what it uses:
  - `os-asr` gains `stt` (sherpa-onnx);
  - `os-engine` gains `inference` (`os-llm`, `os-diar`, note drafting, alignment);
  - `os-app` gains `recorder` and `sync`.

  The S1 build already includes `stt`, because A4 measures live transcription. The first feature build records and transcribes; sync follows. `os-llm` stays off the phone: drafting stays on computers (owner decision, 2026-10-18).
- **Minimum system.** iOS 17.

### 2. What runs where

| Work | iPhone | Computer |
|---|---|---|
| Recording | Always. Encrypted as it records (OBA1) | Yes, as today |
| Live and final transcription | Yes, during recording: Parakeet TDT 0.6B v3 int8 and Silero VAD through sherpa-onnx | Yes, for its own recordings, as today |
| Drafting the note | No. Drafting stays on computers (owner decision, 2026-10-18). Drafting on the phone would need a new decision | Yes, as today (Gemma 4 E4B, 12B or larger), including for visits recorded on a phone |
| Review | Read the transcript, play the audio of visits recorded on this phone, read the note, copy | Full review, Edit and Ask. For a visit recorded on a phone, the evidence link shows the transcript line and its time; the audio plays only on that phone |
| Signing | Not in the pilot (accepted by default) | Yes, as today |
| Templates | Choose one for the visit | Build and edit |
| Model downloads | Through `os-models` (network.md R1), on Wi-Fi by default. Never bundled | As today |

Speech recognition and the LLM are never loaded at the same time. Before loading a model, the app checks `os_proc_available_memory()`.

### 3. Phone modes

M3 has one mode: **record and transcribe.**

- It needs a phone with 6 GB or more (A15 chip or newer) and the speech models (about 0.7 GB). The thresholds are revised once S1 has measured memory use.
- **Before the models are downloaded,** the phone can still record. The visit stays on the phone, marked "Not transcribed yet". Once the models are in place, the phone transcribes it, and only then do the transcript and notes sync.
- **Record-only phones** (too old to run the models) can't be supported. Sync never carries audio, so nothing they record could reach a computer (ADR-0017 section 7). A future decision could let such a phone hand its audio to a computer over the clinic's local network, never through the cloud.
- **Record, transcribe and draft** is not in M3. Drafting stays on computers. The notes from the earlier draft are kept for any future decision: an iPhone 15 Pro or newer with 8 GB, the `com.apple.developer.kernel.increased-memory-limit` entitlement, and possibly a vendored llama.cpp xcframework built with Metal and without OpenMP.

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

## Consequences

- **One codebase.** Most of the phone UI is built and tested on Windows.
- **macOS CI is the bottleneck.** Each iOS build costs macOS minutes, and each check on a real iPhone waits for the owner to install a TestFlight build. Every owner step has a scripted runbook, and builds are batched.
- **Size.** The app is about 60 MB. The speech models are an optional download of about 0.7 GB, on Wi-Fi by default.
- **If S1's audio criteria fail,** the SwiftUI shell costs about 2 more weeks. Sync and the engine are unaffected.
- **Drafting stays on computers.** The phone is useful without it, because it records and transcribes.
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
| Bundling the models in the app | A download of more than 700 MB, which hits cellular download limits and raises licence questions. The models download through `os-models` |
