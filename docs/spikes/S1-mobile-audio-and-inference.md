# Spike S1: background audio and on-device inference inside Tauri 2 mobile

Status: **designed, not started**. Time-box: **5 working days** (2 for Android, 2 for iOS, 1 for write-up).
Output: `docs/adr/0002-mobile-framework.md`, plus a throwaway app in `spikes/s1-tauri-mobile/` (deleted or archived after the ADR).

## The question

Can a Tauri 2 app on iOS and Android do both of these?
1. Capture continuous 16 kHz mono PCM for a full consult, including with the **screen locked or the app in the background**.
2. Run local speech-to-text and Gemma 4 E2B on the phone's GPU well enough to be useful, without the OS killing it.

If audio works but inference doesn't, Tauri stays and phones offload inference to the Clinic Hub. That trade-off is allowed by master prompt §2.3(4). If audio fails, the mobile shell changes.

## What we already know (2026-10-17 research)

- **Tauri has no official microphone plugin.** The community plugins (`tauri-plugin-audio-recorder` 0.1.2, `tauri-plugin-mic-recorder`) record **M4A files** on mobile and don't stream PCM, so they can't drive a live transcript.
- **cpal 0.18.2** supports iOS (CoreAudio) and Android (AAudio, API 26+) from the Rust core. It does not set up AVAudioSession or background modes.
- **Tauri doesn't document iOS background audio at all.** Issue tauri-apps/tauri#12523 shows that someone got it working with AVFAudio plus the `audio` background mode, but reports that `tauri::command`s behave oddly while backgrounded.
- **Android has known Tauri lifecycle bugs around foreground services.** tauri#15671 is still open: the WebView comes back blank on relaunch while a foreground service keeps the process alive.
- **Nobody has publicly shipped `llama-cpp-2` in a Tauri mobile build.** Its iOS PR (#949) was closed unmerged. The Android NDK fixes (PR #1140) did land.
- **sherpa-onnx ships an official Tauri 2 hello-world** as an APK and an IPA (release tag `tauri`). Its mobile linking is shared-library only, and its build script copies the libraries into Tauri's jniLibs folder and the iOS project.

## How we'll test it

Build a minimal Tauri 2.12 app with these four parts.

### 1. Audio capture in Rust

- cpal input stream at 16 kHz mono (resample if the device refuses).
- Feeds a lock-free ring buffer, which a worker thread drains to:
  - a WAV file on disk, so we can count dropped frames;
  - sherpa-onnx Silero VAD, then a sherpa-onnx **streaming** English model. Moonshine streaming small if sherpa-onnx supports it; otherwise a streaming Zipformer. The final model is picked on day 1.
- Partial transcripts go to the WebView through Tauri events.

### 2. Native session plugin

A small Tauri plugin named `os-audio-session`, the only native code in the spike.

- **iOS (Swift):**
  - `AVAudioSession` with category `.playAndRecord` (or `.record`), mode `.measurement` or `.spokenAudio`.
  - `UIBackgroundModes: [audio]` in Info.plist.
  - Handle interruptions (phone calls, Siri) and route changes (Bluetooth lapel mic).
- **Android (Kotlin):**
  - A foreground service with `foregroundServiceType="microphone"` and an ongoing notification.
  - Permissions: `FOREGROUND_SERVICE_MICROPHONE`, `RECORD_AUDIO` and `POST_NOTIFICATIONS`.
  - A wake lock held only while recording.

**Keep awake (product requirement, 2026-10-17).** The phone must not sleep or suspend from the start of a recording until processing is complete.
- **iOS:** the audio background mode keeps capture alive. While the recorder screen is visible, set `UIApplication.shared.isIdleTimerDisabled = true`. Wrap post-recording processing in `beginBackgroundTask` and resume it when the app returns to the foreground.
- **Android:** the foreground service (`microphone` type, then `dataSync` or `shortService` for processing), a `PARTIAL_WAKE_LOCK` held until the note is done, and `FLAG_KEEP_SCREEN_ON` while the recorder is visible.
- **Pass criterion A5:** a 15-minute recording plus note generation completes with the screen locked for the whole run, and the device never reaches doze or app suspension. Verified with `adb shell dumpsys deviceidle` and the iOS console.

### 3. LLM

- `llama-cpp-2 =0.1.160` with Gemma 4 E2B QAT Q4_0 GGUF (3.35 GB).
  - iOS: `metal` feature.
  - Android: try `vulkan`, then OpenCL (Adreno), then CPU.
- Android only: compare against LiteRT-LM running `gemma-4-E2B-it-gpu.litertlm` (2.01 GB), called from Kotlin. This tells us whether the Android path should be LiteRT rather than llama.cpp.
- Workload: a fixed 10-minute synthetic consult transcript (about 2.5k tokens) plus the SOAP prompt, producing about 500 output tokens.

### 4. Probes

The app logs these metrics to a JSON file.

| Metric | What it measures |
|---|---|
| frames captured / expected | Dropped audio |
| peak RSS | Memory pressure (iOS kills apps that use too much) |
| CPU and GPU temperature, if available | Thermal throttling |
| battery Δ over 15 minutes | Power cost |
| time to first token and tokens/s | LLM speed |
| real-time factor (RTF) | Speech-to-text speed: processing time ÷ audio length |

## Devices

| Platform | Minimum test device | Notes |
|---|---|---|
| iOS | iPhone with A16 or newer on the current iOS | **Needs a Mac with Xcode. This dev box is Windows, so this is a blocker to resolve.** |
| Android | Pixel 8-class or Galaxy S23-class (Snapdragon 8 Gen 2 / Tensor G3), Android 14+ | Possible from this Windows box once the Android SDK and NDK are installed. |
| Android, low end | one mid-range device with ~6 GB RAM | Shows how far the "Phone — battery saver" profile has to drop. |

## Test script (repeat 3× per device)

1. Start recording. Play a 15-minute two-speaker consult WAV from a laptop speaker 1 m away.
2. At 0:30, lock the screen. At 7:00, unlock, switch to another app for 60 seconds, then come back. At 10:00, simulate an interruption: an incoming call on iOS, an alarm on Android.
3. Stop at 15:00. Generate the SOAP note on the device.
4. Background the app for 5 minutes, then reopen it. Check that the WebView recovers (this is tauri#15671).

## Pass / fail

| # | Criterion | Pass |
|---|---|---|
| A1 | Capture survives the lock/unlock, background and interruption steps | ≤0.5% frames lost; the interruption is logged and capture resumes automatically |
| A2 | The process is never killed in 15 minutes | 3 of 3 runs on each device |
| A3 | The UI recovers after being backgrounded | No blank WebView in 3 of 3 runs |
| A4 | Live transcript during capture | RTF ≤ 0.5 on the reference device |
| L1 | E2B note generation finishes | 3 of 3 runs, no OOM kill |
| L2 | Speed of E2B note generation | ≤90 s from "Generate" to a complete note on the reference device |
| L3 | Thermals | No throttling-induced >50% tokens/s drop within one note |
| B1 | Build reproducibility | APK built in CI, IPA built with `xcodebuild` from a clean checkout using a scripted setup |

## Decision rule

| Outcome | Decision |
|---|---|
| A1–A3 and L1–L2 pass on both platforms | **Stay on Tauri 2 mobile**, with on-device inference available. |
| A passes, L fails on a platform | **Stay on Tauri.** That platform defaults to Clinic Hub offload, and on-device inference becomes an "experimental" profile. On Android, try LiteRT-LM before giving up. |
| A fails and can't be fixed within the time-box | **Change the mobile shell.** Take the candidates below to the product owner, ranked. |
| B1 fails only because of tooling | Fix it within M0. It isn't a framework decision. |

**Fallback candidates** if the shell changes, in order of preference. All three keep **one Rust engine**, which master prompt §3 requires:

1. **Native Swift and Kotlin shells calling the Rust engine through UniFFI.** Best background-audio control and the same engine everywhere, but the UI has to be written twice. This could be limited to a deliberately thin "recorder plus review" mobile UI.
2. **Expo / React Native UI calling the Rust engine through a native module** (UniFFI-generated bindings). It reuses React skills, though not the DOM components. `uniffi-bindgen-react-native` needs checking first.
3. **Expo with `llama.rn` 0.12.9 and `react-native-sherpa-onnx` 0.4.4.** This is what the master prompt suggests, but it **replaces** the Rust engine on mobile with a second inference stack. Cactus React Native is archived, so it isn't an option. Ranked last because it doubles the inference surface we have to test.

## Risks in the spike itself

- **There is no Mac on hand.** The iOS half can't start without one. Options: borrow or rent a Mac (or a cloud Mac), or run Android first and do iOS second.
- **Expo SDK 58 / iOS 27 churn** affects fallback 3 only.
- **App Store review of background-audio apps.** The `audio` background mode is meant for apps that play or record audio in the background, which a recording scribe is. The S1 write-up records the review-guideline wording we're relying on (master prompt §7.7 asks us to flag store-submission trade-offs).
