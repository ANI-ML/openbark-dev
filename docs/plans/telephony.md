# Plan: calls and email through the clinic's own Twilio and SendGrid

Status: **in progress**, with [ADR-0023](../adr/0023-bring-your-own-telephony.md). Date: 2026-10-18.

Progress (2026-10-19, branch `feat/telephony`): outgoing calls are built and tested on Windows against a local fake Twilio account (steps 3 to 5, 7, 8, 10 for the owner's phone, 12 for Twilio, 13). Waiting on the owner's Twilio account: step 1 (the spike questions), step 2 (platforms), step 6 (phone-band accuracy), and every TRIAL and UPGRADED check. Incoming calls (step 9) and email (steps 11 and 12 for SendGrid) come later, as decided. The clinic guide is [docs/guide/phone-and-email.md](../guide/phone-and-email.md); the end-to-end test is `apps/desktop/e2e/phone-calls.mjs`.

## Goal

A clinic connects its own Twilio account and, if it wants, its own SendGrid account. From inside OpenBark it can then:
- phone a client;
- take a client's call;
- record the call on the computer, with an announcement at the start;
- get a transcript and a Phone call note;
- email a client document from its own address.

ANI.ML Health runs nothing, holds no keys and pays nothing. Everything is off until the clinic turns it on.

## Where each step can be tested

| Tag | Meaning | What it can prove |
|---|---|---|
| WIN | The Windows dev box, with no Twilio account | Engine code, token minting against Twilio's own helper library, the two-channel recorder fed from files, the UI, CSP isolation, redaction tests, the SendGrid request body against a local mock |
| TRIAL | A Twilio **trial** account, created by the owner. Twilio strips `<Dial>` (to a number, a client or SIP), `<Record>` and `<Start><Recording>` from TwiML on trials. Other limits: 75 voice minutes, 10 minutes a call, only up to 5 verified numbers, expires after 30 days | That the SDK runs in WebView2: microphone permission, a token minted by the engine, registration, and a call from OpenBark to a TwiML App whose Bin only uses `<Say>` and `<Play>`. That proves the WebRTC path and both audio taps (the remote side carries Twilio's voice). The REST setup (TwiML App, number settings). **Not** a call to a phone |
| UPGRADED | The owner's own account, upgraded (payment method and a small balance) with one local number (1.15 a month). Calls cost about 0.018 a minute | Real outgoing calls to the owner's mobile, incoming calls from it, the announcement heard on a real phone, two-channel recording of a real call, a call over 10 minutes, a Canadian number |
| CLINIC | The pilot clinic's own paid account and number | Real clients, real phone systems, caller ID display, the setup guide followed by a clinic |
| SG | A SendGrid account (60-day trial, 100 emails a day) with a verified single sender, created by the owner | Real sends, attachments, tracking off, error messages |
| MAC | A real Mac. CI runners have no microphone and can't place calls | WKWebView microphone and WebRTC, the remote-stream tap, the permission prompt across restarts |
| CI | GitHub Actions, as today | Unit tests, the network source check, the CSP check, the offline end-to-end test. No real calls, ever |

Linux needs no tag: the research for ADR-0023 found that distribution WebKitGTK has no WebRTC, so the Linux desktop app hides the phone and Linux clinics use web mode. Web mode is tested on Windows in Chrome and Firefox (WIN, UPGRADED).

Rules for every step:
- CI never calls Twilio or SendGrid. Tests use recorded fixtures and local mocks.
- Trial minutes are scarce, and upgraded minutes cost money. Each TRIAL and UPGRADED check is written down first and run once.
- Tests never use real client data. Test calls go to the owner's own mobile.

## Steps

Order: prove the risky parts first (steps 1 to 2), then the engine (3 to 6), then calls end to end (7 to 10), then email (11 to 12), then the release work (13 to 14). Email (steps 11 and 12) needs nothing from steps 1 to 10 and can move first if the owner prefers (ADR-0023 open question 4).

### Step 1: spike in a throwaway branch (WIN, TRIAL, then UPGRADED)

Build the smallest thing that places a call from Tauri's WebView2:
- a `phone.html` window loading the bundled `@twilio/voice-sdk` 2.18.x;
- a token minted by a Rust test command from the account's Standard API key;
- the microphone granted to the phone window through Tauri 2.12's `on_permission_request`;
- a hand-made TwiML App and Bins.

**1a, on the trial account.** The Bin plays a sentence with `<Say>`, then `<Play>`s a long WAV, because trials strip `<Dial>`. Questions:
1. Does `getUserMedia` work in the phone window? With the grant persisted (WebView2 saves it in the profile by default), does any prompt appear after a restart?
2. Does `device.connect()` reach the TwiML App, and does the `<Say>` sentence play?
3. Do the local and remote AudioProcessors deliver both streams to an AudioWorklet? Is the remote stream silent in Web Audio (a reported Chrome issue)? If it is, does `call.getRemoteStream()` attached to a muted audio element fix it?
4. Does the PCM reach Rust through IPC without gaps over a 9-minute `<Play>`?
5. **CSP isolation:** with Twilio's hosts in Tauri's global CSP and a stricter `<meta>` CSP in `index.html`, can the main window still reach no host? Test it with a `fetch` and a `WebSocket` to a Twilio host from the main window's console. Both must fail.
6. Which hosts did the SDK actually use (WebView2 developer tools)? Can the `sounds` option point at bundled files, so `sdk.twilio.com` and `media.twiliocdn.com` are never contacted?
7. What does the SDK send to `eventgw.twilio.com`? Record the kind of data.
8. Does the engine's token match one made by `twilio-node` for the same inputs, and does Twilio accept it?
9. Does the REST setup work: create the TwiML App, read numbers and verified caller IDs?

**1b, after the owner upgrades the account and buys one number.**
10. Does a call reach the owner's mobile through the outgoing Bin, with `{{#e164}}{{To}}{{/e164}}` taking the number from `device.connect({ params: { To } })`?
11. With two windows registered under the same identity, do both ring on an incoming call?
12. Does ending the parent call through REST stop the missed-call message after the clinic hangs up?

Done when: the answers are written down, and ADR-0023 is updated wherever an answer differs. If question 3, 4 or 5 fails, stop and go back to the owner with the fallbacks: web mode only, or the SIP route (ADR-0023, Alternatives).

### Step 2: platform checks (MAC, WIN)

- MAC: the same spike build, signed with the hardened runtime and the `com.apple.security.device.audio-input` entitlement. Microphone permission in WKWebView, whether the prompt comes back after a restart (Tauri issue 8979), a call on the upgraded account, and the remote AudioProcessor producing sound rather than silence.
- WIN: web mode in Chrome, Edge and Firefox on `http://127.0.0.1:4317`, with the same call.
- Linux: the desktop build checks for `RTCPeerConnection` at start and hides the phone when it is missing, with a pointer to web mode. If a future WebKitGTK ships WebRTC, the phone appears without a code change.

Done when: ADR-0023's platform table says "works" or "web mode only" for each platform, with evidence.

### Step 3: `engine/os-connect` and credentials (WIN)

- New crate `engine/os-connect`, with `reqwest` from the workspace. Add it to `allowed` in `scripts/ci/check-network.sh`.
- `secrets.rs`: keychain entries "OpenBark" / "twilio-api-key-secret" and "OpenBark" / "sendgrid-api-key" through `keyring` 4.2. Without a keychain, seal them under the data key in `connections.bin` (OBK1). Settings shows which is in use.
- `settings.rs`: the non-secret fields (ADR-0023 Section 10), saved by the engine, not in the browser's localStorage.
- `token.rs`: mint the Voice access token. HS256 with the API key secret. Header `typ: JWT`, `cty: twilio-fpa;v=1`. Claims: `jti` (key SID, a dash and the time), `iss` (the key SID), `sub` (the account SID), `exp` one hour ahead. Grants: `identity`, `voice.outgoing.application_sid`, `voice.incoming.allow`. Use `jsonwebtoken` 11 (its `Header` has `cty`; pick one crypto backend) or a 40-line signer on `hmac` and `sha2`. Check it live and pin it in `docs/versions.md`.
- Redaction: `Debug` and `Display` on secret types print `****` plus the last four characters.

Tests (written first):
- A token minted from a fixed key, secret and clock matches the token Twilio's official helper library produces for the same inputs. Generate the fixture once with `twilio-node` or `twilio-python` and commit it.
- No secret appears in logs, in errors, or in any IPC or HTTP response. The test sets a secret and scans every captured output for it.
- Remove deletes the keychain entries and the sealed file.

### Step 4: Twilio setup through REST (WIN with a mock, then TRIAL and UPGRADED)

- `twilio.rs` calls, all against `https://api.twilio.com/2010-04-01/Accounts/{AccountSid}/` with basic auth (key SID and secret):
  - check the account;
  - list incoming numbers and verified caller IDs;
  - create or update the "OpenBark" TwiML App with the outgoing Bin URL;
  - set one number's Voice URL to the incoming Bin, only after the user confirms, clearing `VoiceApplicationSid` (Twilio ignores the Voice URL while it is set);
  - end a call (`Status=completed`).
- Generate the two Bins' TwiML from the settings: caller ID, the computers' identities, the missed-call sentence.
- Map Twilio errors to plain causes, for example: wrong key, trial account calling an unverified number, number not in the account, geographic permission off.

Tests:
- WIN: a local mock server replays recorded Twilio responses, including errors. Each error maps to its message.
- TRIAL: run setup once on the trial account. The TwiML App appears in the console with the right Voice URL.
- UPGRADED: point the owner's number at the incoming Bin, then check its settings in the console.

### Step 5: two-channel recording (WIN)

- `live.rs`: a new `LiveSource::Call { clinic, client }` with two receivers. The worker resamples both to 16 kHz and writes a two-channel encrypted recording (OBA1). Voice detection and recognition run per channel, and segments merge by time.
- Speaker labels from the channel: Client for channel 2. Channel 1 takes the team member's role (Vet or Tech), or Clinic without a team. The speaker tracker (voice embeddings) is off for calls.
- `session.rs`: `source` gains "Phone call". New `call` metadata: direction, the other number, start time, duration, call SID. `ConsentMethod::Announced`, allowed under both rules. The consent gains an `outcome`: agreed, declined or stayed on the line, plus the announcement's offset in the recording.
- Playback and the evidence link handle two channels: play both mixed, and jump to the line.
- Desktop: a Tauri IPC channel carries PCM from the phone window. Web mode: the existing `/api/live` WebSocket gains a `channels=2` form with interleaved blocks.

Tests (no microphone, no Twilio):
- A two-channel fixture, made with `fixture-tts`, with the clinic on one side and the client on the other. Speakers are labelled exactly, and segment times stay within 200 ms of the script.
- Frame count matches the clock over a 10-minute fake call fed in real time.
- The recording on disk is encrypted from the first chunk. The existing session-path guard covers it.
- Old one-channel visits open as before.
- No speaker embedding is computed for a call (a test checks the tracker is never built).

### Step 6: phone-band accuracy (WIN)

- Make phone-band versions of the eval fixtures: 8 kHz, G.711 µ-law, band-passed to 300 to 3400 Hz, on the client channel.
- Run `openbark eval` with the Phone call template. Compare facts found, invented numbers and number-check flags with the clean fixtures.

Done when: the numbers are in `docs/eval/telephony.md`. If accuracy drops badly, consider a telephone-tuned model as its own decision. Release (step 14) waits for this.

### Step 7: the phone window (WIN, then TRIAL)

- `phone.html` and its React entry, bundled by Vite as a second page.
- A Tauri capability for the `phone` window only: `phone_token`, `phone_call_started`, `phone_audio`, `phone_call_ended`, `phone_end_parent`. No visit, patient or settings commands.
- CSP: the global CSP gains the Twilio hosts from step 1. `index.html` gets the stricter `<meta>` CSP.
- The SDK: `Device` with the minted token, `tokenWillExpire` triggering a fresh token, and the two AudioProcessors.
- The announcement:
  - outgoing calls: the script on screen when the client answers, with **I've read it**;
  - incoming calls with "Play our recording": the clinic's clip mixed into the microphone stream when the clinic answers, and played locally so the user hears it too;
  - then **Did the client agree?** with **Yes, keep recording** and **No, stop recording**.
- `codecPreferences` set to Opus first. Local ringtones, if step 1 showed the `sounds` option accepts them.
- The microphone granted to the phone window only, through `on_permission_request`. Every other window is denied.
- System tray: closing the main window keeps the phone running while the phone is on. Quitting from the tray stops it.
- Linux desktop: the phone is hidden when `RTCPeerConnection` is missing, with the line "Calls work in web mode on Linux."

Tests:
- CI: a check that the main window's built bundle contains no `@twilio` code, and that its `<meta>` CSP allows no remote host.
- WIN: Playwright drives the phone window with the SDK replaced by a fake that plays two fixture streams. The visit appears with both channels, and the consent outcome matches the button pressed.
- TRIAL: the `<Say>`/`<Play>` call from step 1 through the real phone window. The visit appears with Twilio's voice on channel 2 and the dev box microphone on channel 1.
- UPGRADED: one real outgoing call to the owner's mobile, about 3 minutes. The read announcement is at 0:00 in the recording, and the transcript labels are right.

### Step 8: outgoing calls end to end (UPGRADED)

- **Call** on the patient screen and on a visit, plus the dial pad.
- The pre-call line ("This call will be recorded and announced") with **Don't record this call**.
- The in-call screen: timer, mute, hold (local mute of both directions), keypad for phone menus, live transcript, **Stop and delete the recording**, **Hang up**.
- After the call: the visit opens with the Phone call template, and drafting starts if "Draft when I stop" is on.

Tests (UPGRADED, about 25 minutes in total, roughly 0.50 of calls):
- A normal call, hung up by the clinic. Then one hung up by the mobile.
- Client declines: **No, stop recording** leaves no audio on disk, and the visit says "Client declined recording".
- Network pulled mid-call: the visit keeps what was recorded, and the screen says the call dropped.
- A 15-minute call: no lost frames.

### Step 9: incoming calls end to end (UPGRADED, then CLINIC)

- Registration while the phone is on. The ringing screen with the caller's number, and the client and pet if the number matches a patient record.
- **Answer** and **Decline**. Answering records and announces as Settings says: the clinic's recording, or the script to read.
- On hang-up, end the parent call through REST, so the caller never hears the missed-call message.
- Two computers: the first to answer gets the call, and the other stops ringing.

Tests:
- UPGRADED: a call from the owner's mobile with the clip, then with the script; a call with OpenBark closed (the missed-call message plays); a call with the dev box and a second computer (or web mode in a second browser) both on.
- CLINIC: calls from clients' own phones during the pilot.

### Step 10: patient phone and email fields (WIN)

- `patients.rs`: optional owner phone (stored in E.164 form) and email, encrypted like the rest of `patients.json`. Shown and edited on the patient screen.
- Incoming-number matching runs on the computer.

Tests: the fields round-trip encrypted. Matching handles `+1 (555) 555-0100`, `555-555-0100` and `15555550100`. ADR-0017 sync maps the new fields once sync exists.

### Step 11: SendGrid email (WIN with a mock, then SG)

- `sendgrid.rs`: `POST /v3/mail/send` with the PDF attached, `tracking_settings.click_tracking.enable = false` (and `enable_text = false`) and `open_tracking.enable = false` on every request. The region picks `api.sendgrid.com` or `api.eu.sendgrid.com`.
- **Email to client** on client documents and on signed notes' PDFs. A preview with To, subject and message, then **Send**.
- The visit records each send: to whom, when, which document, and the `X-Message-Id`.
- **Send a test email** in Settings, sent to the From address.

Tests:
- WIN: a mock records the request body. Tracking is off, the attachment decodes to the PDF the user saw, nothing else is attached, and the key appears only in the `Authorization` header.
- WIN: unsigned notes offer no Email button.
- SG: a real send to the owner's address, with link text checked as unrewritten. Also a wrong key, an unverified sender, and an attachment over the limit, each giving its message.

### Step 12: Settings and the setup checks (WIN, TRIAL, UPGRADED, SG)

- Settings > Phone and email, with the screens and copy below.
- **Check setup** runs every check and says exactly what is wrong and where to fix it in the Twilio or SendGrid console.
- Only veterinarians can connect or disconnect. Everyone can call.

Tests: each check's failure, driven by the mocks. One full setup on a fresh trial account, then upgraded, and on SG, by following the clinic guide word for word, timed. The target is under 20 minutes. Check setup on a trial account must say that real calls need an upgrade.

### Step 13: documents (WIN)

- `docs/network.md`: R9, R10 and R11 (ADR-0023 Section 12), and the source check sentence.
- `docs/guide/phone-and-email.md`: the clinic setup guide below.
- `docs/guide/recording.md`: a pointer from the phone-import section to the new guide.
- `docs/versions.md`: `@twilio/voice-sdk` (version, licence and source checked live) and the JWT crate if one is used.
- ADR-0017's "never syncs" row gains the Twilio and SendGrid credentials.
- i18n: every new string in French, Spanish, German and Portuguese, marked for review by native speakers.

### Step 14: pilot (CLINIC, SG)

- One clinic, its own accounts, outgoing calls first. Incoming after two weeks if the owner wants it.
- Watch: call quality, transcript quality on real calls, how often clients object, setup time, and any Twilio error the app explained badly.
- Release only after step 6's numbers are acceptable and step 2's platform table is filled in.

## Clinic setup guide (outline)

To become `docs/guide/phone-and-email.md`. Plain steps with screenshots. Each step says what it costs.

**Before you start**
- You pay Twilio and SendGrid directly. ANI.ML Health never sees your keys, your calls or your bills.
- Check your local rules on recording calls. OpenBark announces every recorded call and asks the client, but your association or lawyer decides whether that is enough where your clients live.
- Twilio has no Canadian data centre. Call logs are kept in the United States, and call audio passes through US servers while the call lasts. Recordings are made and kept on your computer.
- Your firewall must let this computer send UDP to Twilio (168.86.128.0/18, ports 10000 to 60000). Most clinic networks already do. Ask your IT provider if calls connect but you hear nothing.
- On Linux, make calls in web mode (`openbark serve`, then Chrome or Firefox). The Linux desktop app can't make calls.

**Part 1: Twilio (about 15 minutes)**
1. Create a Twilio account at twilio.com. **Upgrade it** (add a payment method and a starting balance). A trial account can't connect calls through OpenBark.
2. Choose your caller ID:
   - **Buy a number** (Phone Numbers > Buy a number), to make and take calls. About 1.15 a month.
   - **Or verify your clinic's number** (Phone Numbers > Verified Caller IDs), to make calls that show your usual number. Clients who call back reach your usual phones.
3. Limit where calls can go: Voice > Settings > Geo permissions. Turn on only Canada and the United States, or the countries you call.
4. Set a usage alert: Billing > Usage triggers. For example, an email when you pass 50 in a month.
5. Create an API key for this computer: Settings > Account settings > API keys & auth tokens > Create API key, in the **United States (US1)** region. Type **Standard** (a Restricted key can't be used for calls). Name it after the computer, for example "OpenBark front desk". Copy the **SID** and the **secret** now: Twilio shows the secret only once.
6. Create two TwiML Bins (TwiML Bins > Create): open OpenBark's Settings > Phone and email > **Show the Twilio text**, copy each one into its own Bin, save, and copy each Bin's URL back into OpenBark.
7. In OpenBark, paste your Account SID (on the Twilio console's home page), the key SID and the secret, and choose **Connect**. OpenBark creates its TwiML App and asks before it changes your number.

**Part 2: SendGrid (optional, about 10 minutes, plus DNS time for a domain)**
1. Create a SendGrid account. The trial allows 100 emails a day for 60 days. Then choose a plan.
2. Verify the address you send from: Settings > Sender Authentication. Single Sender is quickest. Domain Authentication delivers better and needs three DNS records.
3. Turn off click and open tracking: Settings > Tracking.
4. Create an API key: Settings > API Keys > Create API Key. Choose **Custom** (called **Restricted Access** in older screens) and allow **Mail Send** only. Copy it now: SendGrid shows it only once.
5. In OpenBark, paste the key, enter the From name and address, and choose **Send a test email**.

**Part 3: other computers**
- Repeat Part 1 step 5 for each computer, so each has its own key. Then add the computer's name to the incoming Bin (OpenBark shows the updated text).

**If something goes wrong**
- A table of OpenBark's error messages, what each means, and where to fix it in the Twilio or SendGrid console.

**Turning it off**
- Settings > Phone and email > **Disconnect**. Then delete the key in the Twilio or SendGrid console as well.

## UI flows and copy

Brand voice (`docs/brand/DESIGN.md` §8): plain, short, sentence case, no "smart" or "AI". Lucide icons: `phone`, `phone-incoming`, `phone-off`, `mic-off`, `mail`. Buttons follow §7.2: one primary (ink) per view. The amber accent stays reserved for recording: the recording dot on the call screen is amber, and the Call button is not.

### Settings > Phone and email (nothing connected)

| Element | Copy |
|---|---|
| Section title | Phone and email |
| Intro | Call clients and email documents from OpenBark, through your own Twilio and SendGrid accounts. You pay them directly. ANI.ML Health never sees your keys, calls or emails. |
| Twilio card title | Phone calls (Twilio) |
| Twilio card body | Make and take calls here. Recorded calls are recorded on this computer and become visits. |
| Twilio button | Connect Twilio |
| SendGrid card title | Email (SendGrid) |
| SendGrid card body | Email discharge instructions and summaries from your clinic's address. |
| SendGrid button | Connect SendGrid |
| Link | How to set up Twilio and SendGrid |
| Veterinarian-only note (others) | Only a veterinarian can connect accounts. |

### Connect Twilio (sheet)

| Element | Copy |
|---|---|
| Title | Connect Twilio |
| Field | Account SID |
| Helper | On your Twilio console's home page. Starts with AC. |
| Field | API key SID |
| Helper | Starts with SK. Use a Standard key made for this computer. |
| Field | API key secret |
| Helper | Kept in this computer's keychain. It's only sent to Twilio. |
| Field | This computer's name |
| Helper | Used to ring this computer. Use a place, not a person: "front-desk". |
| Field | Outgoing Bin URL |
| Field | Incoming Bin URL (optional) |
| Helper | Leave empty to make calls only. |
| Link | Show the Twilio text |
| Primary button | Connect |
| Secondary button | Cancel |
| Checking | Checking your Twilio account… |
| Number confirm | OpenBark will send calls to +1 555 555 0100 to this computer. Twilio will stop sending them to where they go now. |
| Number confirm buttons | Send calls here / Not now |
| Connected | Connected to Twilio. Calls show +1 555 555 0100. |

### Show the Twilio text (sheet)

| Element | Copy |
|---|---|
| Title | Text for your TwiML Bins |
| Body | Create two TwiML Bins in your Twilio console and paste one of these into each. Twilio has no way for OpenBark to do this for you. |
| Labels | Outgoing calls / Incoming calls |
| Button (each) | Copy |
| Note | Neither text asks Twilio to record. Recording happens on this computer. |

### Settings > Phone and email (Twilio connected)

| Element | Copy |
|---|---|
| Status chip | Twilio · connected |
| Rows | Caller ID: +1 555 555 0100 / This computer: front-desk / Key: SK…4f2a |
| Toggle | Turn on the phone on this computer |
| Toggle helper | While it's on, this computer can take calls, and Twilio's calling service is connected. |
| Toggle | Record calls |
| Toggle helper | Recorded calls start with an announcement and become visits. You can choose not to record a call. |
| Announcement script label | Announcement |
| Default script | This call is recorded for your pet's medical record. The recording stays at the clinic. Is that all right with you? |
| Outgoing note | On calls you make, you read the announcement yourself. It's shown on screen when the client answers. |
| Incoming options label | On calls you take |
| Incoming options | Play our recording / I'll say it myself |
| Record announcement button | Record the announcement |
| Recording the announcement | Recording · 00:07 |
| Announcement saved | Saved. Play it back to check it. |
| Rule note | We announce every recorded call, whatever your clinic's consent rule, because a client may be somewhere that needs everyone's agreement. |
| Data note | Twilio carries your calls, like any phone company, and keeps call logs (numbers, times, durations) in your account. Twilio has no Canadian data centre, so those logs are kept in the United States. It doesn't keep the audio, because OpenBark never asks it to record. |
| Quality note | While the phone is on, Twilio's calling software sends call-quality figures (no audio) to your Twilio account. |
| Trial account warning | This is a Twilio trial account. Trials can't connect calls through OpenBark. Upgrade it in your Twilio console. |
| Buttons | Check setup / Disconnect |
| Disconnect confirm | Disconnect Twilio? This removes the key from this computer. Also delete it in your Twilio console, so it can't be used anywhere else. |

### Placing a call

| Element | Copy |
|---|---|
| Button on patient and visit | Call |
| No number on file | No phone number for this client. Add one, or type it on the dial pad. |
| Pre-call line | This call will be recorded and announced. |
| Pre-call option | Don't record this call |
| Calling | Calling Patel · +1 555 555 0142… |
| Ringing (from Twilio) | Ringing… |
| Connected, recording | Recording · 02:14 |
| Connected, not recording | On call · 02:14 · Not recorded |
| Read prompt (outgoing, and incoming when set) | Read this to the client now: |
| Read confirm | I've read it |
| Announcement playing (incoming, clip) | Playing the announcement… |
| Agreement question | Did the client agree? |
| Agreement buttons | Yes, keep recording / No, stop recording |
| Agreement, no answer yet | Keep recording for now. Answer when you know. |
| Buttons | Mute / Hold / Keypad / Hang up |
| Stop at any time | Stop and delete the recording |
| Stop confirm | Delete the recording so far? The call goes on, unrecorded. |
| After stopping | Not recorded. The client declined. |
| Busy | The line is busy. |
| No answer | No answer. |
| Ended | Call ended · 4:12. Writing note… |
| Ended, not recorded | Call ended · 4:12. Not recorded. |

### Incoming call

| Element | Copy |
|---|---|
| Ringing title (matched) | Patel is calling · Bella |
| Ringing title (unknown) | +1 555 555 0142 is calling |
| Buttons | Answer / Decline |
| Taken elsewhere | Answered on another computer. |
| Missed | Missed call from +1 555 555 0142 at 10:42. |

### Errors (calls)

| Cause | Copy |
|---|---|
| Wrong key or secret | Twilio didn't accept this key. Check the SID and secret, or make a new key in your Twilio console. |
| Trial account | Your Twilio account is a trial, and trials can't connect calls through OpenBark. Upgrade it in your Twilio console. |
| Sound but no voice (UDP blocked) | The call connected but no sound is getting through. Your network may be blocking Twilio's call audio. Ask your IT provider to allow UDP to Twilio (see the setup guide). |
| Linux desktop | Calls work in web mode on Linux. Run `openbark serve`, then open OpenBark in Chrome or Firefox. |
| Geographic permission off | Twilio blocked a call to this country. Turn it on in your Twilio console under Voice > Settings > Geo permissions. |
| Microphone blocked | OpenBark can't use the microphone. Allow it in your computer's privacy settings, then try again. |
| No internet | Can't reach Twilio. Check this computer's internet connection. |
| Call dropped | The call dropped. What was recorded is saved. |
| Account suspended or out of funds | Twilio refused the call: your account needs attention. Check billing in your Twilio console. |

### Review: a recorded call

| Element | Copy |
|---|---|
| Visit source line | Phone call · Outgoing · 4 min 12 s · +1 555 555 0142 |
| Consent line (read, agreed) | Announcement read by Dr Lee at 0:02. Client agreed. |
| Consent line (clip, agreed) | Announcement played at 0:00. Client agreed. |
| Consent line (no answer recorded) | Announcement read by Dr Lee at 0:02. Client stayed on the line. |
| Consent line (declined) | Client declined recording at 0:09. The recording was deleted. |
| Speaker labels | Vet / Tech / Clinic, and Client |
| Template default | Phone call |

### Connect SendGrid (sheet)

| Element | Copy |
|---|---|
| Title | Connect SendGrid |
| Field | API key |
| Helper | A key with Mail Send only. Kept in this computer's keychain, and only sent to SendGrid. |
| Field | From name |
| Example | Riverside Animal Clinic |
| Field | From address |
| Helper | Must be verified in SendGrid. |
| Field | Reply-to address (optional) |
| Field | Region |
| Options | Global / EU |
| Primary button | Connect |
| After connect | Connected. Send a test email to check it works. |
| Test button | Send a test email |
| Test sent | Test email sent to clinic@example.com. |
| Privacy note | Email isn't private like the record. SendGrid and the client's email provider handle the message and its attachment. OpenBark turns off SendGrid's link and open tracking on every email. |

### Emailing a document

| Element | Copy |
|---|---|
| Button on client documents and signed notes | Email to client |
| Unsigned note | Sign the note before emailing it. |
| Sheet title | Email to client |
| Field | To |
| Field | Subject |
| Default subject | Bella's discharge instructions from Riverside Animal Clinic |
| Field | Message |
| Default message | Hello, here are Bella's discharge instructions from today's visit. Call us at +1 555 555 0100 if you have any questions. |
| Attachment row | Discharge instructions · Bella · PDF |
| Primary button | Send |
| Sending | Sending… |
| Sent | Sent to a.patel@example.com. |
| Logged in visit | Emailed discharge instructions to a.patel@example.com at 10:42. |
| No email on file | No email address for this client. Add one to send. |

### Errors (email)

| Cause | Copy |
|---|---|
| Wrong key | SendGrid didn't accept this key. Check it, or make a new one with Mail Send allowed. |
| Sender not verified | SendGrid hasn't verified clinic@example.com. Verify it under Sender Authentication in SendGrid. |
| Too large | This email is too large for SendGrid. It must be under 30 MB. |
| Trial ended or daily limit reached | SendGrid refused the email: your account has reached its limit. Check your SendGrid plan. |
| No internet | Can't reach SendGrid. Check this computer's internet connection. The email wasn't sent. |

## What the owner must do

| # | Task | Needed by |
|---|---|---|
| 1 | Answer ADR-0023's six open questions | Before step 7 |
| 2 | Create a Twilio trial account, verify your mobile number, and create a Standard API key in US1 named "OpenBark dev box". Put the SID and secret on the dev box in `OPENBARK_TWILIO_*` variables for the spike, never in the repo | Step 1a |
| 3 | **Upgrade that account** (payment method and a small starting balance) and buy one local number, 1.15 a month. Real calls can't be tested on a trial. Expect a few dollars of calls across all the tests | Step 1b |
| 4 | Be on your mobile for the UPGRADED checks (about 45 minutes in total, spread over steps 1, 7, 8 and 9) | Steps 1 to 9 |
| 5 | Lend or borrow a Mac for an hour | Step 2 |
| 6 | Create a SendGrid trial account and verify a single sender | Step 11 |
| 7 | Find a pilot clinic willing to use its own paid Twilio account, and ask it to check its recording rules with its association or lawyer | Step 14 |
| 8 | Decide the public wording about Twilio carrying the audio, Twilio's US data location, and Twilio's call-quality events, for the website and README | Before release |

## What a clinic must do

- Create, upgrade and pay for its own Twilio account, and its own SendGrid account if it wants email.
- Buy a number, or verify its existing one for outgoing calls only.
- Create one Standard API key per computer, two TwiML Bins, a usage alert, and geographic permissions.
- Paste the keys and Bin URLs into OpenBark.
- Make sure its firewall lets call audio through (UDP to Twilio).
- Decide, with its association or lawyer, whether the announcement and the "Did the client agree?" step meet its recording rules.
- Keep OpenBark running (in the tray) on the computers that should ring.
