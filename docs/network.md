# Network calls

OpenBark is local-first (master prompt §3, rule §7.4). This file lists every network connection the software can make. **A host that isn't listed here is a bug.** CI enforces this in two ways:
- the offline end-to-end test runs the full pipeline with networking disabled;
- the source check (`scripts/ci/check-network.sh`) confirms that `os-models` and `os-connect` are the only engine crates with an HTTP client. `os-connect` holds the planned Twilio calls (R9 below, ADR-0023); nothing in the app calls it yet.

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

## The website (not the app)

The public website, www.openbark.io, is separate from the software above. It has an account portal (`/portal`, ADR-0017) where a visitor can sign in with an emailed code, keep a short profile (name, clinic, role, country, update emails), find the downloads and delete the account. **The website makes these calls, in the visitor's browser. The OpenBark app makes none of them:** it ships no Supabase SDK and no portal code, and nothing in it talks to Supabase until encrypted sync is built and turned on (the M3 row under Planned).

| # | When | Host(s) | What's sent | Purpose |
|---|---|---|---|---|
| W1 | Opening the portal page | `cdn.jsdelivr.net` | An HTTPS GET for one file, `@supabase/supabase-js@2.118.0/dist/umd/supabase.js`, pinned to that version and checked with Subresource Integrity (sha384). No referrer | Load the sign-in library. The site's Content-Security-Policy allows only this exact file from jsDelivr |
| W2 | Only when the visitor acts: asks for a code, enters it, saves the profile, signs out or deletes the account; and while signed in, loading the profile and refreshing the session | `nmecovtmijnfnjuadicp.supabase.co`: `/auth/v1` (sign-in), `/rest/v1/profiles` (the profile, one row per account, protected by row level security), `/functions/v1/delete-account` | The email address and the code; the profile fields; the account's own access token. **Never clinical data**: no recordings, transcripts, notes or anything about patients | Sign-in, the profile, deleting the account |

- Opening the portal signed out sends nothing to Supabase. The website end-to-end test checks this, and answers every Supabase call in its own mocked sign-in, so tests never reach the live project.
- The sign-in session is kept in the browser's localStorage (`sb-nmecovtmijnfnjuadicp-auth-token`), as supabase-js does by default. The site sets no cookies.
- No realtime connection (`wss:`) is opened; the CSP doesn't allow one.
- Every page also fetches `release.json` from the site itself and the Manrope and JetBrains Mono fonts from Google Fonts.

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
| Proposed (ADR-0023) | The clinic's own Twilio account: `api.twilio.com` and the Voice SDK's hosts (R9 and R10 below) | Calls to clients from OpenBark, recorded and transcribed on this computer | **Off.** Only once the clinic connects its own Twilio account |
| M3 (ADR-0005, ADR-0017) | `<project>.supabase.co`, Supabase Auth (`/auth/v1`) and REST (`/rest/v1`, with `/functions/v1` for removing a device and deleting the account) only. ANI.ML's hosted project is `nmecovtmijnfnjuadicp.supabase.co`; a clinic can point OpenBark at its own Supabase | Sign-in with an emailed code, then per-user sync of transcripts, notes, patient records, templates and a few settings. **End-to-end encrypted**, so the server sees ciphertext only. **No audio:** recordings never leave the device that recorded them. No Storage uploads. Instant updates over Realtime come later and get their own row when built | **Off.** Requires the user to sign in and turn on encrypted sync |

### Planned: calls through the clinic's own Twilio account (ADR-0023)

**Planned, off by default, clinic-owned account.** Nothing below happens until a clinic connects its own Twilio account in Settings. The clinic pays Twilio directly. ANI.ML runs no relay or token server and holds no keys. The engine code for R9 is in `engine/os-connect`, but nothing in the app or the CLI calls it yet. R6 to R8 are reserved by ADR-0017. R11 (SendGrid email) gets its row when email is built, after calls (ADR-0023 section 15, decision 4).

| # | When | Host(s) | What's sent | Purpose | Off switch |
|---|---|---|---|---|---|
| R9 | Planned, off by default, clinic-owned account. Only when someone connects or checks Twilio in Settings, and, once incoming calls ship, when OpenBark ends the caller's side of an answered incoming call | `api.twilio.com` (HTTPS) | HTTPS with the clinic's API key SID and secret (basic auth). The secret goes to no other host, and redirects aren't followed. Lists the account's TwiML Apps, numbers and verified caller IDs. Creates or updates the "OpenBark" TwiML App, sets one number's Voice URL after the clinic confirms, ends a call. `User-Agent: OpenBark/<version>`. Nothing about patients | Set up the clinic's own Twilio account, and end calls cleanly | Don't connect Twilio, or **Disconnect** |
| R10 | Planned, off by default, clinic-owned account. Only while the phone is on, and only from the phone window | `wss://voice-js.roaming.twilio.com` (signalling, TCP 443); `https://eventgw.twilio.com` (call-quality events); media as SRTP over UDP to 168.86.128.0/18, ports 10000 to 60000. Also `sdk.twilio.com` and `media.twiliocdn.com` (ringtones), unless plan step 1 shows the SDK can use local sounds | An access token minted on this computer, valid for one hour. The number dialled. The call's audio, as on any phone call. Call-quality events (call state, round-trip time, jitter, packet loss; no audio), which go to Voice Insights in the clinic's own Twilio account. The SDK's public options can't turn them off | Make calls (and, in a later release, take them) through the clinic's own Twilio account | Turn the phone off. Recording and transcription stay on the computer |

- Nothing listens. Incoming calls, when they ship, arrive over R10's outgoing WebSocket.
- A clinic firewall must allow UDP from the computer to 168.86.128.0/18 on ports 10000 to 60000. Twilio's relay for networks that block UDP is billed separately and isn't used.
- Twilio carries the call like any phone company and keeps call logs (numbers, times, durations) in the clinic's account. It records nothing, because OpenBark never asks it to: the recording is made on this computer.
