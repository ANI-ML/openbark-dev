# Changelog

All notable changes to OpenBark, newest first. Generated from `website/content/history.json` by `scripts/website-build.py`; edit that file, not this one. The same history is on the website's Version history page.

## 0.2.2 (beta)

### Beta: Ready for clinics to try

#### 2026-10-26: Sync between your iPhone and computers, through your clinic hub

Turn one clinic computer into the hub and join your iPhone and other computers to it. A preview for testing with test data, off until you turn it on.

- Encrypted sync and the clinic hub in the desktop installers, as a preview: off until you turn it on, and for test data only until the independent security review is finished. Recordings never sync. (being tested)
- Help shows how to set up sync, step by step, with a short video. The iPhone walkthrough ends with how to join your clinic hub.
- The iPhone app asks for the microphone the first time you record, and says where to turn it back on if it was refused. (being tested)
- The iPhone app fits the screen: clear of the clock, the Dynamic Island and the home bar, and typing in a field no longer zooms the page. (being tested)
- Settings, Encrypted sync says why sync isn't available instead of showing an empty page.

#### 2026-10-26: OpenBark for iPhone, and a clinic hub for sync

The iPhone app records and transcribes in the exam room, with speech built in, and can write notes on iPhones with 8 GB or more. Sync is moving to your clinic's own computer, with nothing sent to us.

- OpenBark for iPhone: record and transcribe in the exam room, offline, with the speech models built in. A short walkthrough the first time you open it. In testing with TestFlight before the App Store. (being tested)
- Notes written on iPhones with 8 GB of memory or more, if you turn it on, with the note model that did best in our testing (Qwen3 4B). Smaller iPhones record and transcribe, and your computer writes the note. (being tested)
- A clinic hub: one computer in your clinic syncs encrypted transcripts and notes with your phones and other computers over your own network. No accounts, no servers of ours. Recordings never sync. A preview, not yet turned on in the installers. (being tested)
- The record of deleted visits stays exact when the hourly clean-up runs during a delete or restore.
- The privacy page now says there are no OpenBark accounts and no servers of ours for sync.

#### 2026-10-25: A new design: Record first, visits in one place

Record is one tap on a calmer screen, visits have their own place grouped by what needs you, notes are easier to check, and Settings is organised into sections.

- The Record screen puts the Record button first. The patient is one optional row, and the template is one choice instead of tiles plus a list.
- Visits have their own place, grouped into Needs you and Done, with times as people say them and one clear status for each visit.
- Reviewing a note: a slimmer header with Copy and Sign, the transcript beside the note, and deleting moved into the More menu.
- Settings is organised into sections, so every setting is one click away.
- Groundwork for the iPhone app: screens laid out for a phone, and models sized for a phone's memory. (being tested)
- A privacy page on the website that lists exactly what OpenBark sends, and when.

#### 2026-10-20: Phone calls, a fuller dental chart, better microphones

Call clients from OpenBark and get the call drafted as a visit, hear a client on a speakerphone, chart more of each dental, and a smoother app and website.

- Phone calls through your clinic's own Twilio account: call out from OpenBark, record after an announcement the client can decline, and get the call drafted as a visit. (being tested)
- The dental chart captures more of what's said, and Species is filled in from the breed.
- A "Phone on speaker" option for recording a client on a speakerphone: OpenBark now hears the far side of the call and labels who said what.
- Microphone checks and recovery: pick and test your microphone, clear messages when it's blocked or busy, and recording carries on if you switch microphones.
- Recording in a browser works again.
- A calmer, more responsive design across the app and the website.
- OpenBark no longer reports a crash when you quit it on a Mac after drafting a note.
- Recordings play again in the desktop app, and long ones start and seek without loading the whole file.

#### 2026-10-19: Your own templates, MLX on Mac, a dental chart

Make templates your own, choose the model that writes your notes, and chart teeth during a dental.

- Change any template, built-ins included, as your own version; a hospital-wide templates setting; admins approve hospital changes with their PIN.
- MLX models on Macs with Apple silicon (macOS 14 or later), as an optional download. (being tested)
- Choose any model from Hugging Face, including split and gated ones, add one from your computer, and remove models to free space.
- A dental chart for dogs and cats, drafted from the conversation, linked to the transcript and included in the note and PDF. You confirm every tooth before signing.
- Help videos re-recorded, in light and dark.
- Installers now name ANI.ML Health Inc. as the publisher.

#### 2026-10-18: Installers, patients and a new look

Everything a clinic needs to install OpenBark and use it day to day.

- Point-and-click installers and uninstallers for Windows, macOS and Linux. Uninstalling keeps your visits unless you choose otherwise. (being tested)
- Patients, with each animal's visits in one place.
- Much faster on NVIDIA graphics cards: transcription and drafting both run on the card, with an optional speed pack for transcription.
- Clear progress for transcription and drafting, and Reprocess to redraft a visit with a different template.
- The new OpenBark brand across the app and the website, with downloads, this version history and a way to contact the team.

### Milestone 2: Specialties and help

#### 2026-10-18: Templates for every kind of visit

Specialty templates, multi-pet visits and built-in help.

- Fourteen specialty templates, including dental, surgery, emergency and equine, plus templates built from your own example note.
- Multi-pet visits, with a separate note for each animal.
- A check that flags statements in the draft that weren't said in the room.
- A user guide and in-app Help with walkthrough videos.
- Import almost any recording, including phone voice notes and video files.

### Milestone 1: Safe to use alongside your records

#### 2026-10-17: Recording, signing and security

The features that make a draft safe to review, sign and keep.

- Live recording, with the transcript and speaker labels appearing as you talk.
- Edit a draft, sign the note, and add signed addenda later.
- A consent step before recording.
- Every visit encrypted on disk, with a recovery key the clinic keeps.
- Team members with roles and PINs, and a record of who changed what.
- PDF export, copy formats, client handouts, and the app in five languages.

### Milestone 0: The first working pipeline

#### 2026-10-17: From recording to a draft note

The core of OpenBark, running entirely on the clinic's own computer.

- Records or imports a consult and transcribes it on the clinic's computer.
- Drafts a SOAP note with Gemma, an open model, where every line links back to what was said.
- Checks every number in the draft against the transcript, and marks anything not discussed as "Not discussed".
- Works offline after a one-time model download.

## Coming next

Planned or being tested, not finished. No dates are promised.

- OpenBark for iPhone on the App Store, after testing on real phones in clinics.
- Encrypted sync through your clinic hub, turned on for everyone after an independent security review.
- A clinic dashboard on your clinic hub: invite your team and see how OpenBark is used, with counts only and never patient data.
- PIMS integration.
- Signed installers, so Windows and macOS don't warn on first install.
