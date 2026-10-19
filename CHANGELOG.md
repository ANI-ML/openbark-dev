# Changelog

All notable changes to OpenBark, newest first. Generated from `website/content/history.json` by `scripts/website-build.py`; edit that file, not this one. The same history is on the website's Version history page.

## 0.1.0 (beta)

### Beta: Ready for clinics to try

#### 2026-10-19: Models of your choice

Bring the model you want, and remove the ones you don't need.

- MLX models from Hugging Face on Macs with Apple silicon and macOS 14 or later, with an optional MLX engine download. Split and gated GGUF models download too, and you can add a model file from this computer. (being tested)
- Remove downloaded models, the MLX engine or the GPU speech pack to free space. The Models screen shows how much each one frees, and the model writing your notes is protected.

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

- An iPhone app that records and transcribes in the exam room, with encrypted sync of transcripts and notes to your computers.
- PIMS integration.
- Signed installers, so Windows and macOS don't warn on first install.
- A dental chart, and summaries of a patient's past records.
- Testing on more Macs, Linux computers and graphics cards.
