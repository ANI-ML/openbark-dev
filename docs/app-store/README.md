# App Store submission pack (iPhone)

Everything App Store Connect asks for, drafted from what the app does today. The owner reviews it and pastes it into App Store Connect when the app record exists (`docs/runbooks/ios.md`). Items marked **Owner** need the owner's decision or account.

Status, 25 October 2026 (sync and the dashboard now run on the clinic's own hub, ADR-0029): the iPhone groundwork builds in CI, the redesign and encrypted sync are in progress, and the TestFlight secrets aren't in GitHub yet. Submit for review only after the checklist at the end is done.

## 1. App information

| Field | Draft | Limit |
|---|---|---|
| Name | OpenBark: Vet Scribe | 30 |
| Subtitle | Visit notes, kept on device | 30 |
| Bundle ID | `health.animl.openbark` | |
| SKU | `openbark-ios` | |
| Primary category | Medical | |
| Secondary category | Productivity | |
| Price | Free | |
| Availability | **Owner:** all countries, or Canada and the US first | |
| Copyright | 2026 ANI.ML Health Inc. | |
| Support URL | https://www.openbark.io/#contact | |
| Marketing URL | https://www.openbark.io | |
| Privacy policy URL | https://www.openbark.io/privacy | |

### Promotional text (170)

Record the visit on your iPhone and get a transcript on the phone itself. Recordings never leave it. Finish the note on your computer, or on the phone.

### Description (4000)

OpenBark listens to a veterinary visit and drafts the clinical record, on your own devices.

Record in the exam room
- One tap to record. OpenBark transcribes on this iPhone, labels who spoke, and keeps the recording here.
- Phone on speaker: record a call with a client on speakerphone and keep both sides apart.

Your data stays yours
- Recordings never leave the iPhone that made them.
- No analytics, no ads, no tracking. OpenBark works fully offline once its speech model is downloaded.
- Optional encrypted sync (coming) moves transcripts and notes between your own devices through a computer in your clinic. Nothing goes to us.

Notes you can trust
- Templates for SOAP notes, rechecks, dental procedures, phone calls and more.
- Every line of a note links back to the moment it was said, so you can check it in seconds.
- Drafts are drafts: nothing is a medical record until a veterinarian reviews and signs it.

Free and open source, built by the ANI.ML Health team.

### Keywords (100)

`veterinary,vet,scribe,SOAP,notes,transcription,dictation,clinic,dental,offline,private,record`

### What's new (first version)

First release of OpenBark for iPhone.

## 2. Age rating

**Owner** to confirm in the questionnaire. Recommended answers: no objectionable content. "Medical/Treatment Information": Infrequent/Mild (veterinary records), which gives 12+. If Apple accepts "None" for veterinary-only content, the rating is 4+.

## 3. App Privacy (the "nutrition label")

Apple asks what data is *collected*: sent off the device in a way the developer or partners can access.

**Data Not Collected**, with or without sync (ADR-0029). Recordings, transcripts and notes stay on the clinic's own devices; model downloads send nothing about the user. Encrypted sync and the clinic dashboard go between the clinic's devices and its own hub computer on its own network, never to ANI.ML Health or a partner, so nothing is "collected" in Apple's sense. Re-check this answer if that ever changes.

## 4. Export compliance

The app uses standard encryption (XChaCha20-Poly1305, X25519, Ed25519, Argon2id, HTTPS) to protect the user's own data. **Owner:** answer "Yes" to "uses encryption" and confirm with counsel that it qualifies for the exemption for standard encryption protecting user data; then set `ITSAppUsesNonExemptEncryption` in `Info.ios.plist` to match, so each upload stops asking.

## 5. Notes for App Review

> OpenBark is a tool for veterinary teams. It records a consultation, transcribes it on the device and drafts a clinical note.
>
> No sign-in is needed. To try it: allow the microphone, choose Record, speak for 20 to 30 seconds (for example, describe a dog's annual check-up), then choose Stop and save. The transcript appears on the phone. The first launch downloads the speech model (about 0.6 GB) from Hugging Face; after that the app works offline.
>
> Writing notes on the phone is optional (Settings, Write notes on this iPhone) and downloads a small model sized for the device.
>
> Recordings never leave the device. If encrypted sync is offered, it runs between the clinic's own devices and a computer in the clinic (the clinic hub) on the clinic's network; there are no OpenBark accounts and no servers of ours. The app asks for Local Network access only when sync is turned on, to find the clinic hub.
>
> Contact: **Owner** to fill in (name, phone, email for the review team).

The app creates no accounts with us (guideline 5.1.1(v) doesn't apply). A person can remove a device from sync, or turn sync off, in Settings.

## 6. Screenshots

Required: 6.9-inch (1320 × 2868) and 6.5-inch (1284 × 2778) iPhone screenshots, up to 10 each. Plan, after the redesign lands:

1. Record, ready (light)
2. Recording, live transcript (dark)
3. Visits, grouped by what needs you
4. Review, a note with its evidence quote
5. Settings, models sized for this iPhone

They come from the Design screens workflow (`.github/workflows/design-screens.yml`) at iPhone sizes, from the synthetic consult under invented patients. Never from clinic data.

## 7. What the owner provides (Apple account)

1. Membership type (Individual or Organization) and the **Team ID**. Organization shows "ANI.ML Health Inc." as the seller; it needs a D-U-N-S number.
2. The App Store Connect app record for `health.animl.openbark` (runbook step).
3. The distribution certificate (`.p12`) and its password, and an App Store Connect API key (issuer ID, key ID, `.p8`).
4. The six `testflight` environment secrets in GitHub: `IOS_CERTIFICATE`, `IOS_CERTIFICATE_PASSWORD`, `APPLE_DEVELOPMENT_TEAM`, `APPLE_API_ISSUER`, `APPLE_API_KEY`, `APPLE_API_KEY_P8` (runbook: `docs/runbooks/ios.md`).
5. App Review contact details, and the answers marked **Owner** above.

Free apps don't need the Paid Apps agreement, tax or banking.

## 8. Before submitting for review

- [ ] TestFlight build installed on a test iPhone: recording survives locking, backgrounding and a phone call (M3 phase 1).
- [ ] Redesigned phone screens merged (ADR-0027).
- [ ] Decide whether the first App Store version includes encrypted sync through the clinic hub (ADR-0029). If yes: the external security review is done, the Local Network permission text is reviewed, and the hub has been tried on a real clinic network.
- [ ] Privacy policy reviewed by the owner and counsel, live at /privacy.
- [ ] Screenshots made and checked.
- [ ] Version and build number set (`apps/desktop/src-tauri/tauri.conf.json`).
