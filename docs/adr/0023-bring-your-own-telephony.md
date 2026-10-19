# ADR-0023: Bring your own telephony: calls and email through the clinic's own Twilio and SendGrid

- Status: accepted (owner decisions 2026-10-18; see section 15). Outgoing calls implemented 2026-10-19 (see "Implementation notes"); not yet tried on a real Twilio account.
- Date: 2026-10-18
- Deciders: product owner (asked on 2026-10-18: "Add the ability to add a SendGrid/Twilio integration for phone support (in app with recording) but unique to the user (we don't do anything and we don't pay for it, they do)"), engineering
- Relations:
  - Extends ADR-0007. Phase 1 (import a phone's own recording) is unchanged. This ADR adds a third source of phone lines next to ADR-0007's PBX extension and SIP trunk: the clinic's own Twilio account, reached through Twilio's Voice JavaScript SDK.
  - Makes one narrow exception to ADR-0017 principle 5 ("the WebView never talks to the network"). Only the separate phone window talks to Twilio, and only for the call itself. Section 4 explains.
  - Uses ADR-0008 for the secrets: the OS keychain, falling back to a file sealed under the local data key.
  - Reuses M1 item 7 (consent), M1 item 9 (the Phone call template, id `callback`, and the client documents) and M1 item 12 (PDF export).

## Context

The owner wants clinics to phone clients from inside OpenBark, record the call, and get a transcript and a note. The clinic must own the phone account and pay for it. ANI.ML Health runs nothing, holds nothing and pays nothing. The owner also named SendGrid, which fits a second need: emailing client documents (discharge instructions, summaries) from the clinic's own email account.

What OpenBark already has:
- **Phone-call import** (ADR-0007 phase 1). Phone recordings in m4a, mp3, CAF and other formats import and transcribe.
- **The Phone call template** (`packages/templates/callback/template.yaml`): reason for call, update from the client, results discussed, advice given, medication changes, plan.
- **A consent model** (`session.rs`): the clinic's rule (`all_parties` or `one_party`), how consent was given, who confirmed it and when, and the script that was read. Under `all_parties`, "only I consent" is refused.
- **A browser audio path.** In web mode, an AudioWorklet taps the microphone and streams PCM to the engine over a WebSocket (`engine.ts`, `LiveSource::Stream` in `live.rs`). A phone call can feed the same path.
- **Client documents and PDFs** written by the engine (`pdf.rs`).

Constraints:
- **Local-first.** Every host is listed in `docs/network.md`. There is no telemetry. Recordings never leave the device unless the clinic chooses.
- **No server of ours.** Nothing can depend on an ANI.ML endpoint, account or key.
- **Rust owns the network.** Today only `os-models` may hold an HTTP client (`scripts/ci/check-network.sh`). ADR-0017 adds `os-sync`.
- **Apache-2.0.** No GPL code in the app.

### Twilio facts (checked 2026-10-18)

| Fact | Source |
|---|---|
| Twilio ships Voice SDKs for JavaScript, iOS, Android and React Native. There is no native desktop SDK, so a desktop app uses the JavaScript SDK in its WebView | https://www.twilio.com/docs/voice/sdks |
| The JavaScript SDK (`@twilio/voice-sdk`) is at 2.18.5 (10 September 2026). Signalling runs over a WebSocket to Twilio, media over WebRTC | https://www.twilio.com/docs/voice/sdks/javascript/changelog |
| The SDK's **AudioProcessor** API hands the app the microphone stream before it is sent to Twilio (since 2.9.0, November 2023) and, with `addProcessor(processor, true)`, the caller's audio before it plays (since 2.17.0, December 2025). That gives OpenBark both sides of the call as separate streams | https://www.twilio.com/docs/voice/sdks/javascript/twilioaudioprocessor and the changelog |
| A device needs an **Access Token**: a JWT signed with HS256 using an API key's secret. It carries the identity, a Voice grant (the TwiML App for outgoing calls, and whether incoming calls are allowed) and an expiry of at most 24 hours. Twilio tells developers to mint tokens on a server so the secret never reaches an untrusted browser | https://www.twilio.com/docs/iam/access-tokens |
| Outgoing calls from the SDK fetch instructions (TwiML) from the **TwiML App**'s Voice URL. Incoming calls to a number fetch TwiML from the number's Voice URL. Both must be URLs Twilio can reach | https://www.twilio.com/docs/usage/api/applications |
| **TwiML Bins** host static TwiML inside the clinic's Twilio account. They support templates such as `{{To}}`. A `{{#Name}}...{{/Name}}` section only tests whether a parameter is present, not its value, so a Bin can't branch on a call's outcome | https://support.twilio.com/hc/en-us/articles/230878368-How-to-use-templates-with-TwiML-Bins |
| `<Dial>` records nothing unless asked: `record` defaults to `do-not-record`. `answerOnBridge="true"` keeps the caller hearing ringing until the far end answers. With no `action`, Twilio moves on to the next verb when the dialled party hangs up. The default timeout is 30 s | https://www.twilio.com/docs/voice/twiml/dial |
| One `<Dial>` can ring up to ten `<Client>` identities at once. The first to answer is connected. `<Parameter>` passes values to the SDK | https://www.twilio.com/docs/voice/twiml/client |
| A clinic can **verify its existing number** and use it as the caller ID for outgoing calls, on trial and paid accounts. Calls back to that number still go to the clinic's usual phone company | https://support.twilio.com/hc/en-us/articles/223180048 |
| A live call can be ended through the REST API (`POST /Calls/{Sid}` with `Status=completed`) | https://www.twilio.com/docs/voice/api/call-resource |
| The SDK's licence is Apache-2.0; its LICENSE file also lists BSD-3-Clause and MIT parts | https://raw.githubusercontent.com/twilio/twilio-voice.js/master/LICENSE.md |
| Twilio tests the SDK on current Chrome, Edge, Firefox and Safari, and says it is "compatible with the latest release of Electron". WebView2 and WKWebView are not mentioned | https://www.twilio.com/docs/voice/sdks/javascript |
| Network: signalling over TLS on TCP 443 to `voice-js.{edge}.twilio.com` (default edge `roaming`, path `/signal`), sound files from `sdk.twilio.com`, call-quality events to `eventgw.twilio.com` (or `eventgw.us1/ie1/au1.twilio.com`). Media is SRTP over UDP to 168.86.128.0/18, ports 10000 to 60000. Codecs are Opus and PCMU | https://www.twilio.com/docs/voice/sdks/network-connectivity-requirements, https://www.twilio.com/docs/voice/sdks/javascript/twiliodevice |
| Call-quality events (call state changes, warnings such as high round-trip time, low MOS, jitter and packet loss) go to Voice Insights in the clinic's account. The public options have no off switch. The source has a `publishEvents` flag, but only in an interface marked "for internal testing" | https://www.twilio.com/docs/voice/voice-insights/api/call/details-sdk-call-quality-events, https://raw.githubusercontent.com/twilio/twilio-voice.js/master/lib/twilio/device.ts |
| `call.getLocalStream()` and `call.getRemoteStream()` also expose both sides, a fallback to the AudioProcessors | https://www.twilio.com/docs/voice/sdks/javascript/twiliocall |
| **Restricted API keys can't create access tokens.** A **Standard** key can, and can use every REST resource except Accounts and Keys. Keys work only in the region where they were made | https://www.twilio.com/docs/iam/api-keys, https://www.twilio.com/docs/iam/api-keys/restricted-api-keys |
| `connect({ params })` sends the params to the TwiML App's Voice URL, 800 bytes at most. Bins have an `{{#e164}}...{{/e164}}` helper that formats a number | https://www.twilio.com/docs/voice/sdks/javascript/twiliodevice, https://www.twilio.com/docs/voice/api/sip-registration |
| On a number, `VoiceUrl` is ignored while `VoiceApplicationSid` is set | https://www.twilio.com/docs/phone-numbers/api/incomingphonenumber-resource |
| Requests to a Bin are signed by Twilio (`X-Twilio-Signature`) | https://www.twilio.com/docs/serverless/twiml-bins/testing/programmatic |
| Functions: 10,000 invocations a month free, then 0.0001 each. Studio: 1,000 executions free, then 0.0025 each. Functions can be deployed through the Serverless API with an API key | https://www.twilio.com/en-us/serverless/pricing, https://www.twilio.com/docs/serverless/api |
| **Trial accounts strip `<Dial><Number>`, `<Dial><Client>`, `<Dial><Sip>`, `<Record>` and `<Start><Recording>` from TwiML.** They allow calls only to and from up to 5 verified numbers, give 75 minutes, cap calls at 10 minutes and expire after 30 days. So a real call from OpenBark can't be tested until the account is upgraded | https://www.twilio.com/docs/usage/trials/try-out-voice, https://www.twilio.com/docs/usage/trials |

Unconfirmed, to check in plan step 1:
- whether the SDK runs in Tauri's WebView2 and WKWebView (Twilio doesn't say);
- whether several devices that register with the same identity all ring;
- whether a Bin works as a TwiML App's Voice URL for outgoing calls (no official example; Twilio's own example is a SIP domain's Voice URL);
- whether there is a REST API for TwiML Bins (none is documented, so this ADR assumes not);
- whether the SDK can be given local sound files, so it never fetches from `sdk.twilio.com`.

Prices, trial limits and where Twilio processes data are in Section 11.

### Consent facts

This is a design record, not legal advice. Each clinic must check its own rules.

| Where | What the law says | Source |
|---|---|---|
| Canada, criminal law | Intercepting a private conversation is an offence, unless one party to it consents. The clinic is a party, so its own consent satisfies the Criminal Code | Criminal Code s. 184(1) and (2)(a), https://laws-lois.justice.gc.ca/eng/acts/c-46/section-184.html |
| Canada, privacy law | Updated 8 October 2026. Whoever starts the call, the organisation must tell the customer it is recording and why, "as clearly and narrowly as possible". "In most cases, if the customer proceeds with the call knowing that the conversation is being recorded and why, their consent is implied." Customers must be given a choice, and an alternative if recording isn't integral to the service. The same update treats voiceprints as sensitive biometric information | Office of the Privacy Commissioner of Canada, https://www.priv.gc.ca/en/privacy-topics/surveillance/02_05_d_14/ and https://www.priv.gc.ca/en/opc-news/news-and-announcements/2026/nr-c_261008/ |
| Canada, notice at the start | A company that relied on its privacy policy instead of telling the customer on an outgoing call was found at fault: "Companies must inform customers at the beginning of customer- and company-initiated calls" | PIPEDA-2007-384, https://www.priv.gc.ca/en/opc-actions-and-decisions/investigations/investigations-into-businesses/2007/pipeda-2007-384/ |
| Canada, provinces | Alberta's and British Columbia's PIPA and Quebec's private-sector Act replace PIPEDA inside those provinces. Quebec (as amended by Law 25) wants consent that is "clear, free and informed". The regulator's draft guidance says implied consent must be "manifest". How that applies to recorded calls is unconfirmed | https://www.priv.gc.ca/en/privacy-topics/privacy-laws-in-canada/the-personal-information-protection-and-electronic-documents-act-pipeda/legislation-related-to-pipeda/provincial-legislation-deemed-substantially-similar-to-pipeda/ |
| Canada, veterinary records | The College of Veterinarians of Ontario says vets are responsible under PIPEDA for clients' personal information. Not everything in an animal's record is personal information, but all of it is confidential | https://cvo.org/veterinary-practice/privacy-in-your-practice |
| United States, federal | One-party consent | 18 U.S.C. § 2511(2)(d), https://www.law.cornell.edu/uscode/text/18/2511 |
| United States, states | Mainly all-party: California, Delaware, Florida, Illinois, Maryland, Massachusetts (only secret recording is banned), Michigan (courts disagree), Montana (an exception when people are warned), New Hampshire, Pennsylvania and Washington. All-party for phone calls only: Connecticut (civil liability unless there is a recorded warning or consent) and Nevada. Several of these pages were last updated between 2019 and 2021 | Reporters Committee for Freedom of the Press, https://www.rcfp.org/introduction-to-reporters-recording-guide/ |
| Washington | Consent is "considered obtained whenever one party has announced to all other parties ... in any reasonably effective manner, that such communication ... is about to be recorded", provided "said announcement shall also be recorded" | RCW 9.73.030(3), https://app.leg.wa.gov/RCW/default.aspx?cite=9.73.030 |
| California, notice | "A business that adequately advises all parties to a telephone call, at the outset of the conversation" does not break s. 632. A federal appeals court (unpublished) treated staying on the line after a recording notice as consent | Kearney v. Salomon Smith Barney, 39 Cal. 4th 95 (2006), https://scocal.stanford.edu/opinion/kearney-v-salomon-smith-barney-33656; Maghen v. Quicken Loans (9th Cir. 2017), https://caselaw.findlaw.com/court/us-9th-circuit/1850148.html |
| Calls across borders | Kearney applied California law to calls recorded in Georgia with Californians on the line. The common advice is to assume the stricter law applies | As above, and https://www.rcfp.org/introduction-to-reporters-recording-guide/ |
| United States, prerecorded voices | The TCPA restricts "any call" to a mobile phone made with "an artificial or prerecorded voice" unless the person gave prior express consent, and it isn't limited to marketing. The FCC's 2020 soundboard ruling found that recorded clips played by a live agent still count. Whether one recorded notice inside a live, hand-dialled call does is unconfirmed | 47 U.S.C. § 227(b)(1)(A), https://www.law.cornell.edu/uscode/text/47/227; FCC DA 20-1507 |

What follows for OpenBark:
- A clinic can't know where the client is. An area code says nothing reliable about location.
- So every recorded call starts with an announcement in spoken words (not a beep), whatever the clinic's rule, and the announcement is part of the recording.
- The announcement asks a question, so the client can say yes or no. A "yes" in the recording is express consent, which is stronger than implied consent in Quebec and in all-party states.
- Every recorded call offers the alternative the Privacy Commissioner asks for: carry on without recording.
- On outgoing calls the announcement is spoken by the person calling, not played from a recording, because of the TCPA's uncertainty about prerecorded voices.
- Calls are labelled by channel, so OpenBark computes no voice embeddings on calls.
- Whether all this is enough where a client lives is settled in some places (Washington) and argued in others. OpenBark makes the announcement easy, keeps it in the recording and makes stopping easy. The clinic's lawyer or association decides whether that is enough.

## Decision

### 1. Principles

1. **Off by default.** Nothing about phones or email appears until a clinic turns it on in Settings. Everything that works today keeps working.
2. **Clinic-owned accounts only.** The clinic signs up with Twilio (and, if it wants email, SendGrid), pays them directly and pastes its own keys into OpenBark. OpenBark never offers an ANI.ML account, key or number.
3. **ANI.ML runs nothing.** No relay, no token server, no webhook, no proxy. The clinic's computer and the clinic's Twilio and SendGrid accounts are the only parts.
4. **The engine is the token server.** The clinic's API key secret stays in Rust. The phone window only ever holds a short-lived access token.
5. **Record on the computer, never in Twilio.** OpenBark never asks Twilio to record. The call is recorded on the computer, both sides on separate channels, encrypted as it records (ADR-0008).
6. **Announce every recorded call.** The announcement is in the recording.
7. **Every host is listed.** Three new rows in `docs/network.md` (Section 12). Nothing listens for incoming connections.

### 2. What runs where

| Part | Who runs it | What it does | What it holds |
|---|---|---|---|
| OpenBark engine (Rust, new crate `engine/os-connect`) | The clinic, on its computer | Stores the keys. Mints access tokens. Calls the Twilio REST API for setup and to end calls. Records, transcribes and drafts. Builds PDFs and sends email through SendGrid | The keys, in the keychain. Recordings, transcripts and notes, encrypted |
| Phone window (a second WebView, `phone.html`) | The clinic, on its computer | Runs Twilio's Voice SDK: dials, rings, plays the caller's voice, sends the microphone. Taps both sides and streams them to the engine | An access token that expires within an hour. No API key, no patient records |
| Main window | The clinic, on its computer | Everything else, as today | Unchanged. Its CSP still blocks every network host |
| The clinic's Twilio account | Twilio, paid by the clinic | Carries the call between the computer and the phone network. Serves two TwiML Bins. Keeps call logs (numbers, times, durations) | Call logs and quality statistics. No recordings, because none are requested |
| The clinic's SendGrid account (optional) | SendGrid, paid by the clinic | Delivers emails the clinic sends | The messages it delivers, and its activity log |
| ANI.ML Health | Nobody | Nothing | Nothing |

### 3. Setup in the clinic's Twilio account

What the clinic creates by hand (the setup guide in `docs/plans/telephony.md` walks through it):
1. A Twilio account, **upgraded** from trial. A trial account strips `<Dial>` from TwiML, so OpenBark can't place or take a real call on one.
2. A caller ID. Either buy a Twilio number (outgoing and incoming calls), or verify the clinic's existing number (outgoing calls only; call-backs still reach the clinic's usual phones).
3. A **Standard API key**, one per computer, named after the computer, made in the US1 region. One key can then be revoked without touching the others. Restricted keys can't create access tokens, and Main keys can do more than OpenBark needs.
4. Two **TwiML Bins**, pasted from text OpenBark shows with a copy button: *OpenBark outgoing* and *OpenBark incoming*. Twilio has no documented API for Bins, so this step stays manual.
5. **Voice geographic permissions** limited to the countries the clinic calls, normally Canada and the United States. This limits the damage if a key ever leaks.

What OpenBark then does itself, through the REST API with the clinic's key:
- checks the key and reads the account's numbers and verified caller IDs;
- creates a TwiML App named "OpenBark" whose Voice URL is the outgoing Bin;
- points the chosen Twilio number's Voice URL at the incoming Bin, after showing what it will change and asking. It clears the number's `VoiceApplicationSid` if one is set, because Twilio ignores the Voice URL while it is.

The outgoing Bin:

```xml
<Response>
  <Dial callerId="+15555550100" answerOnBridge="true" timeLimit="3600">
    <Number>{{#e164}}{{To}}{{/e164}}</Number>
  </Dial>
</Response>
```

The incoming Bin (one `<Client>` per computer, up to ten):

```xml
<Response>
  <Dial timeout="25" answerOnBridge="true">
    <Client>
      <Identity>openbark_front_desk</Identity>
      <Parameter name="parent" value="{{CallSid}}"/>
    </Client>
  </Dial>
  <Say>Sorry, we couldn't take your call. Please try the clinic's main number.</Say>
</Response>
```

Neither Bin has a `record` attribute, so Twilio records nothing. Identities are computer names the clinic picks ("front-desk"), never people's names. Twilio allows only letters, digits and underscores in a client identity, so OpenBark adds the `openbark_` prefix and turns dashes into underscores ("front-desk" becomes `openbark_front_desk`). If step 1 of the plan shows that one shared identity rings every computer, the incoming Bin becomes a single `<Client>` and stops needing edits when a computer is added.

### 4. The phone window and the CSP exception

- The Voice SDK needs network access from the WebView: a WebSocket to Twilio for signalling, and WebRTC for the audio. Rust can't do this for it.
- So the SDK runs in its own window, `phone.html`, with its own Tauri capability: start and stop the call recording, fetch a token, end a call, nothing else. It can't read visits, patients or settings beyond what the call screen shows.
- **The CSP.** Tauri applies one CSP to every window.
  - The global CSP gains the directives Twilio's README gives for SDK 2.18: `connect-src https://eventgw.twilio.com wss://voice-js.roaming.twilio.com https://media.twiliocdn.com https://sdk.twilio.com` and `media-src mediastream: https://media.twiliocdn.com https://sdk.twilio.com`. Source: https://cdn.jsdelivr.net/npm/@twilio/voice-sdk@2.18.1/README.md
  - The main window's `index.html` adds a stricter `<meta>` CSP of its own. Browsers enforce every policy that applies, so the main window still can't reach any host.
  - This trick is checked in plan step 1. If it fails, the fallback is to inject the CSP per window from Rust.
  - CSP's `connect-src` covers the WebSocket. It doesn't govern WebRTC media, which goes only where Twilio's signalling sends it.
- **Sounds.** The SDK fetches ringtones from `sdk.twilio.com` and `media.twiliocdn.com`. OpenBark gives it local sound files instead, if the SDK's `sounds` option allows that (plan step 1). If it does, those two hosts come out of the CSP.
- **Codecs.** OpenBark sets `codecPreferences` to Opus first. The SDK's default puts PCMU first, which sounds like an ordinary phone line even on the computer's side.
- **Microphone permission.** Tauri 2.12 added `on_permission_request` (https://v2.tauri.app/blog/tauri-2.12). OpenBark grants the microphone to the phone window only, and denies it to every other window. On macOS, wry grants WKWebView's media requests unless a handler says otherwise. Older Tauri issues report the prompt coming back at each start (https://github.com/tauri-apps/tauri/issues/8979), so step 2 checks it.
- The SDK is bundled from npm and pinned in `docs/versions.md`. It is never loaded from a CDN.
- In web mode (`openbark serve`), the phone is a page of the local server. The server sends the Twilio hosts in that page's CSP header only.

ADR-0017 principle 5 stays true for everything except the call's own signalling and audio.

### 5. Outgoing calls

1. The user opens a patient or a visit and chooses **Call**, or types a number on the phone screen.
2. If the clinic records calls, the screen confirms that this call will be recorded and announced. The user can switch recording off for this one call.
3. The engine mints an access token. The phone window calls `device.connect({ params: { To: "+1..." } })`.
4. Twilio fetches the outgoing Bin and dials the client from the clinic's caller ID. With `answerOnBridge`, the clinic hears ringing until the client answers.
5. On answer, recording starts, and the screen prompts the user to read the announcement (Section 7).
6. Live transcript on screen. **Hang up** ends the call. The visit is saved and, as with any recording, the draft starts if "Draft when I stop" is on.

### 6. Incoming calls

- When the phone is on, the phone window registers with Twilio. Incoming calls arrive over the SDK's own outgoing WebSocket, so nothing on the computer listens and no port is opened.
- Every OpenBark computer listed in the incoming Bin rings at once. The first to answer gets the call.
- The ringing screen shows the number and, if it matches a client in OpenBark's patient records, the client's and pet's names. Matching happens on the computer.
- Nobody answers within 25 seconds, or OpenBark is closed: Twilio says the short message in the Bin and hangs up. Voicemail is not offered in this ADR, because Twilio would store it in its cloud.
- **When OpenBark hangs up an answered call**, it also ends the caller's side through the REST API (`Status=completed` on the parent call). Otherwise Twilio would move on to the missed-call message after the clinic hangs up.
- OpenBark must be running to take calls. Keeping it in the system tray while the window is closed is part of the plan. A computer that is asleep or off doesn't ring.

### 7. Recording and consent

**Two channels, on the computer.**
- A local AudioProcessor taps the microphone (channel 1, the clinic). A remote AudioProcessor taps the caller (channel 2, the client).
- An AudioWorklet sends both to the engine: Tauri IPC on the desktop, the existing WebSocket in web mode.
- The engine writes a two-channel recording, encrypted in chunks as it records (OBA1), and transcribes each channel.
- Speaker labels come from the channel, so they are exact: **Client** for channel 2, and for channel 1 the role of the team member using OpenBark (**Vet** or **Tech**), or **Clinic** without a team.
- **No voice embeddings on calls.** The live speaker tracker is off for calls, because the channel already says who is speaking. That keeps calls clear of the voiceprint question in the Privacy Commissioner's October 2026 update. Whether the tracker's embeddings in exam-room recordings count as voiceprints is a separate question for the owner, outside this ADR.
- The phone-network side is narrowband audio. Parakeet's accuracy on it is not measured yet. The plan measures it before release (step 6).
- Some reports say Chrome-based browsers give silence when a remote WebRTC stream goes into Web Audio, unless it is also attached to an audio element. The remote AudioProcessor may avoid this, and `call.getRemoteStream()` is the fallback. Step 1 checks it.

**The announcement.**
- It is in spoken words, never a beep, and it asks a question, so the client can answer.
- Default script: "This call is recorded for your pet's medical record. The recording stays at the clinic. Is that all right with you?"
- **Outgoing calls: the caller says it.** When the client answers, the call screen shows the script and the user reads it. A recorded clip isn't used, because the TCPA restricts calls to US mobiles that use a prerecorded voice, and whether one clip inside a live call counts is unsettled.
- **Incoming calls: the clinic chooses** in Settings.
  - **I'll say it myself**, as for outgoing calls.
  - **Play our recording.** Someone at the clinic records it once in Settings, about ten seconds, stored encrypted on the computer. When the clinic answers, the local AudioProcessor mixes it into the microphone stream. Both sides hear it. This answers ADR-0007's open question 3.
  - The TCPA concern doesn't apply here, because the client placed the call. Canada's rules on automatic announcing devices weren't researched; the guide says so.
- Either way, the announcement is the first thing in the recording. That meets Washington's rule that the announcement itself be recorded.
- After the announcement, the call screen asks **Did the client agree?**:
  - **Yes, keep recording** saves express consent;
  - **No, stop recording** deletes what was recorded so far, and the call goes on unrecorded;
  - with no answer, the visit says the client stayed on the line after the announcement.
- The announcement can't be turned off while recording is on. Recording itself can be off for any call (**Don't record this call**). That is the alternative the Privacy Commissioner asks for.

**The consent record.** `ConsentMethod` gains `announced` (played from the clinic's recording). The existing `verbal` covers an announcement the user read. Both are allowed under both rules. The consent saved with the visit holds:
- the method;
- whether the client agreed, declined, or stayed on the line without answering;
- who was on the call for the clinic;
- the announcement's text;
- the time it started, as an offset into the recording.

Review shows "Announcement at 0:00. Client agreed." with a play button.

### 8. From call to visit

A recorded call becomes an ordinary visit:

| Field | Value |
|---|---|
| Source | "Phone call", with the direction ("Outgoing" or "Incoming") |
| Patient | The patient the call started from, or chosen during or after the call |
| Template | **Phone call** (`callback`), preselected. The user can change it |
| Consent | As Section 7 |
| Call details | The other number, start time, duration and Twilio's call SID, so the clinic can match its Twilio logs. All stored encrypted in `session.json` |
| Audio | Two channels, encrypted |

- The transcript, draft, review, sign, addenda, PDF and copy all work as for any visit.
- With sync on (ADR-0017), the visit header and transcript sync like any other visit. The recording stays on the computer that made it.
- Patient records gain two optional owner fields: **phone** and **email**. They are typed by the clinic, stored encrypted in `patients.json`, and used for Call, for matching incoming numbers, and for Email. The clinic's practice management system stays the source of truth.
- Calls that weren't recorded save nothing but an optional line in the patient's history: "Call, outgoing, 4 min, not recorded".

### 9. Emailing documents through the clinic's SendGrid

- Optional, and separate from phone calls. A clinic can use either one alone.
- Setup:
  - a SendGrid API key with **Custom** permissions and only **Mail Send** allowed (older SendGrid pages call this "Restricted Access"; the scope is `mail.send`);
  - a verified sender: Single Sender Verification, or Domain Authentication for better delivery;
  - the From name and address, and an optional Reply-To.
  - Sources: https://www.twilio.com/docs/sendgrid/ui/account-and-settings/api-keys and https://www.twilio.com/docs/sendgrid/ui/sending-email/sender-verification
- The request: `personalizations` with the client's address, `from`, `subject`, `content`, and the PDF as a base64 `attachments` entry. Success is `202 Accepted`. The whole message must be under 30 MB. Source: https://www.twilio.com/docs/sendgrid/api-reference/mail-send/mail-send
- EU sending (`api.eu.sendgrid.com`) needs a Pro plan and an EU subuser. Source: https://www.twilio.com/docs/sendgrid/data-residency/faq
- SendGrid says it doesn't keep message bodies after delivery. It keeps event history for 30 days. Sources: https://support.sendgrid.com/hc/en-us/articles/18961023703963-How-to-Find-the-Body-or-Contents-of-Emails and https://www.twilio.com/docs/sendgrid/api-reference/email-logs
- Where it appears: on a client document (discharge instructions, client summary) and on a signed note's PDF, as **Email to client**.
- Flow:
  1. Choose **Email to client**. The owner's email from the patient record is filled in, and can be changed.
  2. Check the preview: subject, a short message and the PDF attached.
  3. Choose **Send**.
- The engine builds the PDF (`pdf.rs`) and calls `POST https://api.sendgrid.com/v3/mail/send` with the clinic's key, or `api.eu.sendgrid.com` for an EU account.
- **Tracking is off on every message.** Click tracking (which rewrites every link) and open tracking are set to `false` in every request, so links aren't rewritten and no tracking pixel is added. SendGrid's docs don't say outright that the request overrides the account setting, so the setup guide also asks the clinic to switch tracking off under Settings > Tracking. Source: https://www.twilio.com/docs/sendgrid/ui/account-and-settings/tracking
- The visit records the send: to whom, when, which document, and SendGrid's message id. A failed send says why and is never retried silently.
- Unsigned notes can't be emailed. Client documents follow the existing rule: available after signing.
- Email is not private like the record. The settings screen says so plainly.
- Text messages are out of scope. US texting needs A2P 10DLC registration, which is a separate decision.

### 10. Credentials

| Item | Secret? | Where it is kept |
|---|---|---|
| Twilio Account SID (`AC...`) | No | Engine settings |
| Twilio API key SID (`SK...`) | No | Engine settings |
| Twilio API key secret | **Yes** | OS keychain, entry "OpenBark" / "twilio-api-key-secret" |
| TwiML App SID (`AP...`), caller ID, Bin URLs, this computer's identity | No | Engine settings |
| SendGrid API key | **Yes** | OS keychain, entry "OpenBark" / "sendgrid-api-key" |
| From name and address, Reply-To, region | No | Engine settings |
| The announcement recording | Clinical-adjacent | Encrypted under the data key, like a visit |

Rules:
- **Where secrets go.** The Twilio secret goes only to `api.twilio.com`, and is used inside the engine to sign tokens. The SendGrid key goes only to the SendGrid API host. Tests enforce both.
- **Where secrets never go:**
  - the WebView or the browser;
  - logs (a redaction test checks this);
  - exports, PDFs or support bundles;
  - sync: ADR-0017's "Never" row gains these credentials.
- **Without a keychain** (an SSH session, or Linux without Secret Service), secrets are sealed under the local data key in `connections.bin` (OBK1), following ADR-0008's fallbacks. Settings says which is in use.
- **After saving**, a secret is shown only as its last four characters.
- **Remove** deletes the keychain entries and says: "Also delete this key in your Twilio console, so it can't be used anywhere else."
- **Tokens.** The engine mints them with `exp` one hour ahead and refreshes them when the SDK warns of expiry. Identity is this computer's identity. The grant names only the OpenBark TwiML App.
- **Who can change settings.** Only veterinarians can connect or disconnect accounts. Anyone can place and answer calls. On a shared OS account this is not a security boundary (M1 item 11). Real separation arrives with the Clinic Hub (M4).
- **Web mode.** `POST /api/phone/token` returns a token only to this computer's own page: the `local_only` guard and the `X-OpenBark` header, as for every other route.

Why minting tokens locally is acceptable: Twilio's advice to mint on a server exists to keep the secret out of strangers' browsers. Here the computer belongs to the account owner, and the secret is already stored on it. The engine plays the server's part. The risk is a compromised clinic computer, the same risk as a compromised server. The defences:
- a standard key per computer, revocable on its own;
- geographic permissions;
- usage alerts in the Twilio console, which the setup guide recommends.

### 11. Costs

The clinic pays Twilio and SendGrid directly, on their accounts. ANI.ML pays nothing and earns nothing from it.

Twilio's US and Canada pages show the same prices, "current as of August 2026". They print "$" with no currency code; Twilio bills in US dollars.

| Item | Paid to | Price on 2026-10-18 |
|---|---|---|
| Local number | Twilio | 1.15 a month (toll-free 2.15). Nothing, if the clinic only verifies its existing number for outgoing calls |
| Outgoing call | Twilio | 0.0140 a minute for the phone-network leg, plus 0.0040 a minute for the SDK leg: about 0.018 a minute |
| Incoming call to a local number | Twilio | 0.0085 a minute, plus 0.0040 for the SDK leg: about 0.0125 a minute |
| Recording in Twilio | Twilio | Nothing, because OpenBark never asks Twilio to record (Twilio charges 0.0025 a minute plus storage when it does) |
| Voice Insights advanced features | Twilio | Not used. Twilio lists them at 0.0024 a minute; OpenBark doesn't turn them on |
| Email | SendGrid | The permanent free plan was retired from 27 May 2025. New accounts get a 60-day trial of 100 emails a day. The cheapest paid plan, Essentials, starts at 19.95 a month |

Example: a clinic making 300 five-minute callback calls a month pays Twilio about 27 for the minutes plus 1.15 for a number.

Sources:
- https://www.twilio.com/en-us/voice/pricing/ca and https://www.twilio.com/en-us/voice/pricing/us
- https://www.twilio.com/en-us/changelog/sendgrid-free-plan
- https://www.twilio.com/en-us/products/email-api/pricing

**Trial accounts** are useful for testing the setup, not for real calls:
- 75 voice minutes;
- 10 minutes per call;
- calls only to and from verified numbers (up to 5), within the sign-up country;
- `<Dial>` with a number, a client or SIP is stripped from TwiML, so a call from or to OpenBark can't complete;
- the account expires after 30 days.

Upgrading means adding a payment method and a starting balance; Twilio's page gives no minimum.

Sources: https://www.twilio.com/docs/usage/trials/try-out-voice and https://www.twilio.com/docs/usage/tutorials/how-to-use-your-free-trial-account

The app never shows a price as fact. The setup screen links to the price pages.

**Where Twilio processes data.**
- Twilio's regions are US1 (the default), IE1 and AU1. There is no Canadian region.
- Its edge locations, which route the call audio, include none in Canada either. The nearest are Ashburn and Umatilla.
- So a Canadian clinic's call logs are kept in the United States, and the call audio passes through US servers while the call lasts. The recording itself is made and kept on the clinic's computer.
- Twilio's data protection addendum is part of its terms. Twilio acts as the clinic's processor.
- Sources: https://www.twilio.com/docs/global-infrastructure/understanding-twilio-regions, https://www.twilio.com/docs/global-infrastructure/edge-locations, https://www.twilio.com/en-us/legal/data-protection-addendum
- The guide says this plainly, so a Canadian clinic can decide.

### 12. Changes to docs/network.md

When this ships, three run-time rows are added. Their numbers are assigned then; R6 to R8 are reserved by ADR-0017. Until then, one planned row describes them.

| # | When | Host(s) | What's sent | Purpose | Off switch |
|---|---|---|---|---|---|
| R9 | Only when someone connects or checks Twilio in Settings, and when OpenBark ends the caller's side of an answered incoming call | `api.twilio.com` | HTTPS with the clinic's API key SID and secret (basic auth). Reads the account, numbers and caller IDs. Creates the TwiML App, sets one number's Voice URL, ends a call. Nothing about patients | Set up the clinic's own Twilio account, and end calls cleanly | Don't connect Twilio, or **Disconnect** |
| R10 | Only while the phone is on, and only from the phone window | `wss://voice-js.roaming.twilio.com` (signalling, TCP 443); `https://eventgw.twilio.com` (call-quality events); media as SRTP over UDP to 168.86.128.0/18, ports 10000 to 60000. Also `sdk.twilio.com` and `media.twiliocdn.com` (ringtones), unless step 1 shows the SDK can use local sounds | A one-hour access token. The number dialled. The call's audio, as on any phone call. Call-quality events (call state, round-trip time, jitter, packet loss; no audio), which go to the clinic's own Twilio account (Voice Insights). The SDK's public options can't turn them off | Make and take calls through the clinic's own Twilio account | Turn the phone off. Recording and transcription stay on the computer |
| R11 | Only when someone presses **Send** on an email, or **Send a test email** | `api.sendgrid.com`, or `api.eu.sendgrid.com` for an EU account | HTTPS with the clinic's SendGrid key. The From, To and Reply-To addresses, subject, message and the PDF the user chose. Click and open tracking set to off | Email a client document from the clinic's own SendGrid account | Don't connect SendGrid |

- Nothing listens. Incoming calls arrive over R10's outgoing WebSocket.
- A clinic firewall must allow UDP from the computer to 168.86.128.0/18 on ports 10000 to 60000. Twilio's relay for networks that block UDP (its Network Traversal Service) is billed separately and isn't used. Source: https://www.twilio.com/docs/voice/sdks/network-connectivity-requirements
- OpenBark doesn't pass the SDK's internal-testing `publishEvents` option, because it isn't a public option and could change without notice.
- The source check becomes: "`os-models`, `os-sync` and `os-connect` are the only engine crates with an HTTP client."
- A new check: the main window's bundle contains no Twilio code and its CSP allows no remote host.

### 13. What ANI.ML never sees

| ANI.ML never sees or holds | Who does |
|---|---|
| The clinic's Twilio or SendGrid keys | The clinic's computer only |
| Call audio | The computer, and Twilio while it carries the call (like any phone company) |
| Recordings, transcripts and notes | The computer. With sync on, the encrypted copies of transcripts and notes in ADR-0017 |
| Phone numbers, call times and durations | The computer (encrypted), and the clinic's Twilio call logs |
| Emails, addresses and attachments | The computer, the clinic's SendGrid account and the client's email provider |
| Bills and usage | The clinic, Twilio and SendGrid |

### 14. Platforms

| Platform | Plan |
|---|---|
| Windows (WebView2) | First target. WebView2 follows Edge, and Microsoft lists no WebRTC gap (https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/browser-features). Tested on the dev box |
| macOS (WKWebView) | Likely. wry implements WKWebView's media-capture delegate, and `Info.plist` already has the microphone text. A hardened, signed build also needs the `com.apple.security.device.audio-input` entitlement. Needs a real Mac to test, because CI runners can't place calls |
| Linux (WebKitGTK) | **Desktop phone not offered.** WebKitGTK's `enable-webrtc` setting is off by default, wry doesn't set it, and the Ubuntu 24.04 and Debian packages appear to be built without WebRTC. One project found `RTCPeerConnection` missing even with the settings on. Sources: https://webkitgtk.org/reference/webkit2gtk/stable/property.Settings.enable-webrtc.html, https://github.com/tauri-apps/wry/issues/85. Linux clinics use web mode in Chrome or Firefox, which Twilio supports |
| Web mode | Same phone page, in Chrome, Edge, Firefox or Safari on the same computer. `127.0.0.1` counts as a secure origin, so the microphone works |
| iPhone | Not in this ADR. It would use Twilio's iOS SDK with CallKit and needs its own decision (ADR-0018) |

Twilio supports browsers and Electron, not WebViews. If Twilio declines to help with a WebView problem, web mode is the supported fallback on every platform.

### 15. Open questions for the owner

**Owner decisions (2026-10-18):**

| # | Question | Decision |
|---|---|---|
| 1 | Announcement policy | The vet reads a short script on screen, then the app asks "Did the client agree?". Recording without that step isn't possible, and "carry on unrecorded" is always available. No recorded clip on outgoing calls. |
| 2 | Incoming calls in the first release | No. **Outgoing calls only** first. Incoming calls come in a later release. |
| 3 | TwiML Bins or an automatic Function deploy | **Bins.** OpenBark generates the snippets and the clinic pastes them. No code is deployed into the clinic's account. |
| 4 | Email first or calls first | **Calls first**, email after. |
| 5 | Upgraded development account | Open. The owner creates a Twilio account when testing starts. |
| 6 | Voiceprint privacy review | Open, outside this ADR. |

The original questions follow for the record.

1. Is the announcement policy right? This ADR has the caller read it on outgoing calls (because of the TCPA), lets the clinic play a recording on incoming calls, and won't record without an announcement. A clinic that wants a recorded clip on outgoing calls should get its own legal advice first.
2. Is incoming calling needed in the first release, or is outgoing enough? Outgoing alone needs no number and no incoming Bin. It can even use the clinic's existing number as caller ID.
3. Should the TwiML Bins step be replaced by an automatic deploy of a Twilio Function into the clinic's account? Functions are free up to 10,000 calls a month, so cost isn't the issue. It would save the clinic about five minutes and allow a proper missed-call fallback, but OpenBark would then ship code that runs in the clinic's cloud and must keep it working.
4. Should the email feature wait until after calls ship, or come first? It is smaller and needs no WebView change.
5. The owner's own Twilio account must be **upgraded** (a payment method and a small balance) before any real call can be tested. Is that acceptable for development?
6. Exam-room recordings use voice embeddings to label speakers. The Privacy Commissioner now treats voiceprints as sensitive biometric information. Should that get its own review? It is outside this ADR, because calls don't use embeddings.

## Consequences

- **Clinics get calling and email inside OpenBark** at their own cost, on their own accounts, with ANI.ML out of the loop.
- **The recording is local and exact.** Two channels give exact speaker labels, which a speakerphone recording never could.
- **New network surface.** Three rows, one of them from a WebView. The phone window is isolated by its CSP and capability, and the main window stays offline. That isolation must be tested on every platform.
- **Twilio sees the call.** It carries the audio, as any phone company does, and keeps call logs and quality statistics in the clinic's account. The guide and the settings screen say so. The public wording "recordings never leave the device" stays true, because the recording is made on the computer.
- **Telemetry wording.** "No telemetry" stays true for OpenBark, which sends nothing to ANI.ML. Twilio's SDK sends call-quality events to the clinic's own Twilio account while the phone is on, and there is no public way to stop it. `docs/network.md` and the settings screen say so.
- **No WebView support from Twilio.** Twilio supports browsers and Electron. A WebView-only bug may get no help from Twilio, and web mode is then the fallback.
- **Linux desktop has no phone.** Linux clinics use web mode for calls.
- **Testing costs a little.** Real calls need an upgraded Twilio account, so development needs a small balance and a 1.15-a-month number.
- **Legal uncertainty stays with the clinic.** OpenBark announces, records the announcement and makes stopping easy. It doesn't decide whether that is consent where the client lives. The guide says this plainly and points to the clinic's association or lawyer.
- **Setup friction.** A Twilio account, a key, two pasted Bins and geographic permissions take about 20 minutes. The guide and the in-app checks must be very clear.
- **Phone-band transcription is unmeasured.** The release waits for an eval on narrowband fixtures.
- **Support.** Twilio problems (account suspended, number not verified, trial limits) surface in OpenBark. Errors must name the cause and say what to do in the Twilio console.
- **Dependencies.** `@twilio/voice-sdk` 2.18.x (Apache-2.0, with BSD-3-Clause and MIT parts, which go into `NOTICE` if required) and either `jsonwebtoken` 11 (its `Header` has `cty`) or a short HMAC-SHA256 signer on `hmac` and `sha2`. Each is checked live and pinned in `docs/versions.md`.

## Alternatives considered

| Option | Why not (for now) |
|---|---|
| A token server or relay run by ANI.ML | Against the owner's request: ANI.ML would hold or proxy credentials, and could pay for usage by mistake. Also a server to secure and keep running |
| Twilio-side recording (`record-from-answer-dual`) | Puts clinical audio in Twilio's cloud, costs extra, and would need downloading and deleting. Local recording gives the same two channels with nothing stored outside the clinic |
| A SIP softphone (baresip, BSD-3-Clause) registered to a Twilio SIP Domain | Keeps audio in Rust and the WebView offline, works on Linux, and fits ADR-0007 phase 2. Twilio supports it: up to 10 registrations per address, TLS on 5061 and SRTP if the domain is set to secure. But Twilio's SIP side offers only PCMU and PCMA, so the clinic's own side is narrowband too. It also has NAT trouble that the SDK avoids, needs a sidecar process, and still needs hosted TwiML. It stays the route for clinics with a PBX (ADR-0007), and the fallback if the WebView route fails. Source: https://www.twilio.com/docs/voice/api/sip-registration |
| A native WebRTC stack in Rust (`webrtc` crate, 0.21) | Pre-1.0, and Twilio's signalling protocol isn't public, so OpenBark would be reimplementing Twilio's SDK |
| Electron for the phone window | It is the only desktop runtime Twilio names, but it would add a second browser engine to the app. Web mode already gives Linux a supported browser |
| Running the Voice SDK in the main window | Simpler, but the window that holds every visit would gain network access. A separate window keeps clinical data in a window with no network |
| Twilio Functions deployed automatically into the clinic's account (Serverless API) | Fully automatic setup, and free up to 10,000 invocations a month. But OpenBark would ship and maintain code running in the clinic's cloud. Kept as open question 3 |
| Twilio Studio flows created through the Studio API | Automatic and able to branch on call outcome. 1,000 executions a month free, then 0.0025 each. Harder for a clinic to read than two short Bins |
| Media Streams to transcribe in real time | Needs a public WebSocket server to receive the audio. There is none, by design |
| Emailing through the clinic's own mail server (SMTP) | Possible later. SendGrid was the owner's choice, and its REST API keeps the HTTP client in Rust with no SMTP stack. SMTP would be a separate row and decision |
| Calling from the iPhone app | Needs Twilio's iOS SDK, CallKit and push notifications through Apple. Separate decision, after ADR-0018's first release |

## Implementation notes (2026-10-19, outgoing calls)

Built on the branch `feat/telephony`, tested on Windows against a local fake Twilio account only (`os-connect`'s `mock` feature). What differs from, or settles, the text above:

- **Sounds.** The SDK's `sounds` option replaces every default URL (checked in `@twilio/voice-sdk` 2.18.5's `device.js`), so OpenBark passes local files for all 15 sounds and `sdk.twilio.com` and `media.twiliocdn.com` are left out of the phone page's CSP (plan step 1, question 6, settled from the source; still to confirm on a real call).
- **Event gateway.** After registering, the SDK switches to `eventgw.<home region>.twilio.com`, so the phone page's CSP lists `eventgw.twilio.com` and the `us1`, `ie1` and `au1` gateways.
- **The main window's CSP** is a `<meta>` tag added at build time only (`apps/desktop/vite.config.ts`), so the dev server still runs. Web mode also sends it as a header (`os-server` `PAGE_CSP`), and `/phone.html` gets its own header (`PHONE_CSP`).
- **Window isolation on the desktop.** Tauri allows app commands from every window, so `src-tauri/src/lib.rs` wraps the command handler: the phone window may call only the phone commands, and no other window may call them. The phone window's capability grants only `core:window:allow-close`. The microphone goes to the phone window only (`on_permission_request`).
- **Recording starts at the answer, and the engine takes a few seconds** to load its speech models. The phone window keeps the audio from the answer until the engine is ready and sends it first, so the announcement is in the recording; a "yes" or "no" given before then is applied once it is.
- **Declined calls** keep the visit (`session.json` only: who, when, the number, the consent outcome and when the client declined) and nothing else: no audio, no transcript.
- **Unrecorded calls** save nothing (the optional history line of section 8 isn't built).
- **Speaker labels** come from the channel: "Vet" (veterinarian), "Tech" (technician) or "Clinic" (reception, or no team), and "Client". The note model is told which speaker is which, as context entered by the clinic.
- **Patient phone numbers**: the owner's phone only (stored in E.164, encrypted). The email field arrives with SendGrid.
- **Not yet confirmed** (plan steps 1 and 2): the SDK in WebView2 and WKWebView, the remote AudioProcessor delivering sound rather than silence, a Bin as a TwiML App's Voice URL for outgoing calls, and a real call end to end.
