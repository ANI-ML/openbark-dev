# ADR-0007: Phone calls: import now, a built-in call mode next

- Status: proposed (phase 1 shipped; phase 2 needs the product owner's answers below)
- Date: 2026-10-17
- Deciders: product owner, engineering

## Context

The product owner asked for "a built in call out mode so the phone call feature allows outbound and recorded scripts for calls to scribe", preferably public and free. Competitors offer phone documentation (VetRec, CoVet), but always through their cloud. Clinics phone owners every day with results, rechecks and post-op checks, and those calls belong in the record.

Constraints that shape the options:
- **There is no free route to ordinary phone numbers.** Reaching the public phone network always goes through a carrier, which charges per minute or per seat.
- **Phones won't let a third-party app record cellular calls.**
  - iOS has no API for it. iOS 18.1 and later can record calls in the Phone app itself, with an announcement to both sides, and save the audio to Notes.
  - Android has blocked third-party call recording since about 2022 (Play policy on the accessibility workaround). Some manufacturers' dialers record natively.
- **Local-first** (CLAUDE.md, docs/network.md): no clinical audio goes to a cloud service that we choose. Any provider is one the clinic chooses, and every network call is listed.
- **Licences:** the app is Apache-2.0. GPL libraries (for example PJSIP without a commercial licence, or Linphone) can't be bundled.
- **Consent:** many places require everyone on a call to agree to recording. The consent step (M1 item 7) already records consent per encounter.

## Decision

### Phase 1: import calls recorded on the phone (shipped)

- The engine now decodes the formats phones produce, in addition to WAV:
  - m4a (AAC and ALAC), mp3, CAF, FLAC and Ogg Vorbis;
  - decoded with symphonia 0.6.1 (MPL-2.0, pure Rust);
  - a real iPhone-style AAC m4a gives the same transcript as the WAV.
- **Callback note** is a built-in template for the medical record.
- The flow: the vet records the call with the phone's own call recording (which announces itself), exports the audio, then imports it into OpenBark. The consent step applies.
- Cost: free. Downside: manual, a few taps per call.

### Phase 2: a built-in call mode (proposed)

OpenBark becomes one more line on the clinic's phone system, a SIP endpoint, and places and receives calls itself. The audio passes through OpenBark, so:
- it records the call on this computer;
- the clinic side and the owner side are separate channels, so speaker labels are exact;
- the same live pipeline gives a live transcript, the second recogniser and hints;
- a **recorded consent announcement** plays when the call connects ("This call is recorded for your pet's medical record"), and is saved as the encounter's consent;
- **call scripts**: a template-driven checklist on screen during the call (results call, post-op check, recheck reminder), and afterwards the Callback note drafts to the same headings.

Where the phone line comes from, cheapest first:
1. **The clinic's existing internet phone system (PBX).** OpenBark registers as an extension, so there's often no new cost beyond a seat. Most cloud and on-site PBXs support SIP extensions.
2. **A SIP trunk the clinic signs up for**, if it has no PBX: per-minute pricing, typically around a cent a minute in North America (check current pricing).
3. **Self-hosted open-source PBX** (Asterisk, GPL-2.0; FreeSWITCH, MPL-1.1) on the Clinic Hub (M4). The software is free but still needs a trunk to reach real numbers. It runs as a separate program, so its licence doesn't affect ours.

Implementation candidates, to confirm in a spike:
- **Desktop:** baresip + libre (BSD-3-Clause), embedded or as a sidecar process. Media is RTP; recording taps the decoded audio before it reaches the speaker.
- **Web mode:** SIP over WebSocket from the browser with SIP.js or JsSIP (both MIT), to a PBX that supports WebRTC.
- **Mobile:** VoIP calls through CallKit (iOS) and ConnectionService (Android). These calls are the app's own audio, so they can be recorded. Needs the S1 mobile spike.

## Consequences

- Phase 1 already removes the "WAV only" wall for any recorder, not just phones.
- Phase 2 adds a network surface: SIP signalling and RTP media to one endpoint the clinic configures. It goes into docs/network.md as a new row. There's no telemetry, and audio is never sent to a transcription cloud. Encryption: SIP over TLS and SRTP where the provider supports them.
- Testing calls needs a test PBX or trunk. CI uses a local Asterisk container and recorded RTP; no real numbers.
- Calls bring new edge cases: hold music, transfers, voicemail, and a far end that drops out. The transcript must mark hold and transfer gaps.

## Open questions for the product owner

1. Which phone system does the pilot clinic use (for example Weave, RingCentral, 8x8, on-site PBX or none)? That decides whether OpenBark can be an extension.
2. Outbound only to start, or inbound calls too?
3. Should the consent announcement be the clinic's own recorded voice, or text-to-speech?
4. Which call scripts first? I suggest a results call, a post-op check and a recheck reminder.

## Alternatives considered

| Option | Why not (for now) |
|---|---|
| Cloud call platforms' recording and transcription (Twilio, Telnyx and similar) | Puts clinical audio in a third-party cloud; per-minute costs; against local-first. Their SIP trunking could still serve as option 2 above. |
| Recording a speakerphone with the laptop microphone | Works today and is free, but it's one mixed channel, the quality is poor, and it records the room. Fine as a fallback. |
| PJSIP | Excellent, but GPL-2.0 unless commercially licensed. Not bundleable in an Apache-2.0 app. |
| Recording cellular calls from our mobile app | Not allowed by iOS, and blocked by Android policy. |
