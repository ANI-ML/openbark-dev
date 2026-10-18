# Changelog

All notable changes to OpenBark, newest first. Generated from `website/content/history.json` by `scripts/website-build.py`; edit that file, not this one. The same history is on the website's Version history page.

## 0.0.1 (pre-alpha, not yet published)

### Since milestone 2: Installers, patients, speed and the website

#### 2026-10-18: Version history, contact and a coffee link

The website gained this Version history page, a way to reach the team, and a link for anyone who wants to support the project.

- A Version history page lists every change that matters to a clinic, newest first, with the commit and files behind each one; GitHub gets the same list as CHANGELOG.md. (website/content/history.json; scripts/website-build.py; CHANGELOG.md)
- A Contact section and footer link reach the ANI.ML Health team at we.care@animl.health. (website/src/index.html (#contact); scripts/website-build.py (CONTACT))
- A Buy us a coffee link on the website, and the Sponsor button on GitHub, go to buymeacoffee.com/animlhealth. (.github/FUNDING.yml; scripts/website-build.py (COFFEE))
- The website deploys to Vercel from the main branch whether the project's root folder is set to website or left blank. (04f7753; vercel.json; website/vercel.json)

#### 2026-10-18: Public website, credits and a new orange look

OpenBark got a fact-checked public website, a rewritten README with credits, a release pipeline still being set up, and a new orange look across app and website.

- A public website was built with a working evidence-link demo, sections on privacy, accuracy, measured results and open models, a downloads area, and no analytics. (7728e07, 87bfaaa; website/src/index.html; website/vercel.json; website/dist/index.html)
- A features page lists 246 features in 17 categories, each linked to where it lives in the code or evaluation, with a filter. (7728e07; website/content/features.json; scripts/website-build.py)
- Every public claim on the site was checked against the project's own files, leading to 54 corrections, such as stating that only Windows has been tested so far. (7728e07; website/content/features.json; website/src/index.html)
- The README was rewritten for clinics, and credits now name Dr. Christopher Pinard and ANI.ML Health Inc., with every default model's attribution listed. (7728e07; README.md; NOTICE)
- A pipeline to publish installers with checksums to a public downloads repository and point the website at them is set up, but no release has been published yet. (being tested) (7728e07; .github/workflows/release.yml; .github/workflows/website-release.yml; website/public/release.json)
- The app and website now use Geist type and ANI.ML Health orange, with accessible contrast, the paw logo in the sidebar and a new orange app icon. (19fdf86; docs/adr/0015-geist-and-animl-orange.md; packages/ui/src/tokens.css)
- Empty Record and Review screens now explain what will appear there and offer the next step, such as Draft note. (19fdf86; apps/desktop/src/screens/Recorder.tsx; apps/desktop/src/screens/Review.tsx)
- The website gained a skip link for keyboard users, a page for missing links, and an evidence demo that stops moving when reduced motion is requested. (19fdf86; scripts/website-build.py; website/dist/404.html; website/dist/site.js)

#### 2026-10-18: Clearer drafting progress and better-grounded notes

Transcription and drafting now show each step as it happens, and notes are written in the template's own heading order, which cut unsupported wording in testing.

- Transcription shows a step-by-step board, from reading the recording to transcribing, checking numbers and saving, with elapsed time, an estimate and screen reader announcements. (b468dc4; apps/desktop/src/components/ProgressBoard.tsx; apps/desktop/src/components/ProcessingBar.tsx)
- Drafting shows each heading of the template ticking off as the model writes it, followed by a check of every number against the transcript. (b468dc4; apps/desktop/src/components/DraftProgress.tsx; engine/os-app/src/lib.rs)
- Redrafts and Reprocess keep the current note visible, dimmed, while the new draft is written, and multi-pet visits show which animal is being drafted. (b468dc4; apps/desktop/src/screens/Review.tsx; apps/desktop/src/components/DraftProgress.tsx)
- Notes are now written in the template's heading order, history and exam before assessment, instead of alphabetically with Assessment first and Subjective last. (b468dc4; engine/os-engine/src/template.rs; docs/eval/heading-order.md)
- On 13 synthetic test consults, wording not supported by the cited transcript lines halved, from 45 to 23, and wrong citations fell from 12 to 3. (b468dc4; docs/eval/heading-order.md)
- In the same test, facts captured stayed level at 141 of 155 against 143 before, and invented numbers stayed at zero in both versions. (b468dc4; docs/eval/heading-order.md)

#### 2026-10-18: Progress bars, storage choice and faster transcription

Clinics can now see how long processing will take, choose which drives hold visits and models, and transcribe faster, with an optional graphics card pack being tested.

- Imports and finished recordings show a single progress bar that says what is happening in words and estimates the time left for the whole job. (4dd018e; apps/desktop/src/components/ProcessingBar.tsx; engine/os-engine/src/live.rs)
- Settings now lets the clinic choose where visits and models are kept, showing the exact folder, free space, and warnings for cloud-synced, removable or network drives. (4dd018e; docs/adr/0013-choose-where-data-is-kept.md; apps/desktop/src/components/ChangeFolderSheet.tsx)
- Moving data to another drive copies and checks each file before removing the original, undoes itself if anything fails, and never overwrites other files. (4dd018e; engine/os-models/src/locations.rs; docs/adr/0013-choose-where-data-is-kept.md)
- Transcription on the processor got faster after tuning: on one 20-thread test machine, an 8.5-minute test consult went from 39 to 28 seconds. (4dd018e; engine/os-asr/src/lib.rs; docs/adr/0014-gpu-speech-recognition.md)
- Help gained a Patients and visits topic, and all seven help videos were re-recorded on the current version of the app. (4dd018e; apps/desktop/src/screens/Help.tsx; apps/desktop/public/help/patients.webm)
- An optional speech pack for Windows with NVIDIA graphics cards installs after the clinic accepts NVIDIA's licence; on one RTX 4080, an 8.5-minute import took 43 seconds, not 152. (being tested) (3888f20; docs/adr/0014-gpu-speech-recognition.md; engine/os-models/src/gpu_pack.rs; apps/desktop/src/components/GpuCard.tsx)
- The graphics card is only used after a separate self-test passes, so a faulty setup falls back to the processor instead of crashing the program. (being tested) (3888f20; docs/adr/0014-gpu-speech-recognition.md; apps/desktop/src-tauri/src/main.rs; engine/os-app/src/lib.rs)
- The graphics card pack can be removed from Models at any time, and uninstalling OpenBark deletes exactly the pack's own files after the program has closed. (being tested) (3888f20; apps/desktop/src/components/GpuCard.tsx; apps/desktop/src-tauri/windows/hooks.nsh)
- A Reprocess button on each visit drafts the note again from the recording, with the same or another visit type; earlier drafts are kept and signed notes stay final. (3888f20; apps/desktop/src/components/ReprocessSheet.tsx; apps/desktop/src/screens/PatientDetail.tsx)
- An automated code review led to 30 fixes, including blocking folder changes while anything is being saved and stricter checks on the browser version's local server. (3888f20; engine/os-app/src/lib.rs; engine/os-server/src/lib.rs)

#### 2026-10-18: Point-and-click installers and patient records

OpenBark gained a standard Windows setup wizard that protects clinic data, and visits are now grouped under each patient instead of in one flat list.

- Windows now installs with a standard setup wizard in five languages, needs no administrator rights, and keeps the program in a separate folder from the clinic's data. (9e11fdd; docs/adr/0012-point-and-click-installers.md; apps/desktop/src-tauri/tauri.windows.conf.json; apps/desktop/src-tauri/windows/installer.nsi)
- Uninstalling keeps recordings and notes unless the clinic ticks a box and confirms a second time, and updates or silent uninstalls never delete data. (9e11fdd; apps/desktop/src-tauri/windows/hooks.nsh; docs/adr/0012-point-and-click-installers.md)
- Packaging for a macOS disk image with its own uninstaller, and for Linux .deb, .rpm and AppImage packages, was set up but has not been built or tested yet. (being tested) (9e11fdd; scripts/macos-dmg.sh; apps/desktop/src-tauri/tauri.linux.conf.json; .github/workflows/release.yml; docs/runbooks/macos.md)
- Patients replaced the flat encounter list, with one row per animal showing species, family name, visit count and last visit, plus a name filter. (bf60de8; docs/adr/0011-patients-and-visits.md; apps/desktop/src/screens/Patients.tsx)
- Each patient has a page listing every visit newest first, with Edit details and a New visit button that opens Record with the patient filled in. (bf60de8; apps/desktop/src/screens/PatientDetail.tsx)
- Record now starts with Find a patient, so a returning animal joins its existing record, and differences in capitals or spacing no longer create a duplicate. (bf60de8; apps/desktop/src/components/PatientPicker.tsx; engine/os-engine/src/patients.rs; docs/adr/0011-patients-and-visits.md)
- Multi-pet visits appear under every animal and open on that animal's note, and visits recorded before this change were linked to patients automatically. (bf60de8; engine/os-app/src/lib.rs; docs/adr/0011-patients-and-visits.md)
- The patient list is encrypted on the computer like the visits, and the interface now says visit instead of encounter in all five languages. (bf60de8; engine/os-engine/src/patients.rs; docs/adr/0011-patients-and-visits.md; packages/i18n/i18n-fr.json)

### Milestone 2: Specialties, help and a new look

#### 2026-10-18: Model guide and wider recording import

Clinic staff got a plain-language guide to AI model names, and importing now accepts WebM, WhatsApp voice notes and video files, with clearer messages when a file cannot be read.

- The Models screen explains model names such as 4B, 12B, Q4 and QAT in plain language, and recommends a size that suits the computer. (82953de; apps/desktop/src/components/ModelGuide.tsx; apps/desktop/src/modelGuide.ts)
- Each model name is broken into labelled parts, with the meaning shown on hover and read out by screen readers. (82953de; apps/desktop/src/screens/Models.tsx)
- Imported recordings can now be WebM and MKV, Opus and Ogg voice notes from WhatsApp or Telegram, AIFF, and dictaphone WAV files. (75dc765; engine/os-asr/src/audio.rs; docs/guide/recording.md)
- Video recordings in MOV and WebM can now be imported alongside MP4, and OpenBark transcribes their sound. (75dc765; engine/os-asr/src/audio.rs; apps/desktop/src/engine.ts; docs/guide/recording.md)
- A file OpenBark cannot read, such as WMA or DSS dictation, now gets a plain message listing the formats that work, and no empty encounter is left behind. (75dc765; engine/os-asr/src/audio.rs; engine/os-app/src/lib.rs; docs/guide/recording.md)
- Groundwork was laid for patient records, an encrypted list linking each animal's visits; the screens for it came in a later update. (being tested) (75dc765; engine/os-engine/src/patients.rs; engine/os-app/src/lib.rs)

#### 2026-10-18: Species headings, example notes and a new look

Templates learned to show headings for certain species, could be built from a clinic's existing note, and the whole app moved to a cleaner design with visit-type tiles.

- A template heading can be shown for certain species only, such as FeLV and FIV status for cats or heartworm testing for dogs. (a06874e; apps/desktop/src/species.ts; engine/os-engine/src/template.rs)
- Species is now chosen in the patient details and never guessed from the recording. (a06874e; apps/desktop/src/components/PatientFields.tsx)
- A clinic can paste a note it already writes and get a proposed template to edit; patient details are never copied and the example is not saved. (a06874e; engine/os-engine/src/from_example.rs; apps/desktop/src/components/ExampleSheet.tsx)
- The app was redesigned after Apple's interface style, with the system font, larger text for the note, grouped panels and smoother sheets and dialogs. (f951cbf; docs/adr/0010-apple-style-refresh.md; packages/ui/src/tokens.css)
- Recording now starts by picking a visit type from icon tiles, such as New consult, Recheck, Annual exam, Phone call, Results or Clinician rounds. (6408787; apps/desktop/src/components/VisitTypePicker.tsx; docs/templates.md)
- Two built-in templates were added, Results and Clinician rounds, and four were renamed New consult, Recheck, Annual exam and Phone call. (6408787; packages/templates/results/template.yaml; packages/templates/rounds/template.yaml)
- A new text box heading writes sections such as surgical reports as paragraphs, and every sentence still links back to the recording. (6408787; engine/os-engine/src/note.rs; engine/os-engine/src/render.rs)
- Each template can be set to concise, standard or thorough detail, without ever allowing anything that was not said. (6408787; engine/os-engine/src/template.rs; docs/templates.md)

#### 2026-10-18: Specialty templates and stronger checks

Fourteen new templates arrived, with a check that flags statements not backed by what was said, safer number reading, multi-pet visits and client documents in other languages.

- Fourteen new built-in templates were added, including dental, surgery and anaesthesia, emergency, dermatology, oncology, exotics, equine, production animal, euthanasia, and two client documents. (299313c; packages/templates/; docs/plans/M2.md)
- Each template heading now offers example requests suited to different kinds of practice, shown in Ask and in the template builder. (299313c; apps/desktop/src/promptExamples.ts; apps/desktop/src/screens/Templates.tsx)
- Each statement in a draft is now checked against the transcript lines it cites, and statements not supported by what was said are flagged for review. (299313c; engine/os-engine/src/align.rs; docs/eval/M2.md)
- Synthetic testing found a tenfold dose error the number check missed, "point one" milligrams per kilo charted as 1; the check now reads it as 0.1 and adds a hint. (299313c; engine/os-engine/src/numbers.rs; docs/eval/M2.md)
- The number check now reads spoken tooth numbers such as "four oh four", times, "point seven" and ordinals such as "fourth premolar" correctly. (299313c; engine/os-engine/src/numbers.rs; docs/eval/M2.md)
- Misheard drug names and routes, such as "a sepromazine" or "Roman 4" for IV, now get hints, and 63 farm, equine, exotics and anaesthesia terms were added. (299313c; engine/os-engine/src/vocab.rs; packages/vocab/veterinary.tsv)
- One recording can now cover several pets, with a separate note for each; in a synthetic two-cat test, no values crossed between notes. (299313c; engine/os-engine/src/note.rs; apps/desktop/src/components/PatientFields.tsx; docs/eval/M2.md)
- On nine synthetic specialty consults, drafts captured 91 of 98 expected facts with no invented numbers; results come from scripted consults, not real clinics. (299313c; docs/eval/M2.md)
- Discharge instructions and client summaries can now be drafted in the client's language; in one synthetic French test, the medication and every number stayed unchanged. (299313c; apps/desktop/src/components/DocumentSheet.tsx; docs/eval/M2.md)
- Install and uninstall scripts were added that keep clinic data unless removal is confirmed; only Windows was fully tested, Linux was tried under WSL, and the Mac is untested. (being tested) (299313c; install/install.ps1; install/install.sh; install/uninstall.ps1)

#### 2026-10-18: A user guide and in-app help

OpenBark gained a written user guide and a Help screen with short walkthrough videos of the real app, available without an internet connection.

- A written user guide now covers getting started, recording, reviewing and signing, templates, client documents, team and security, and troubleshooting. (6e1ed70; docs/guide/README.md)
- A new Help screen in the sidebar has six topics, each with a short captioned video of the real app and the steps written out. (6e1ed70; apps/desktop/src/screens/Help.tsx)
- The help videos are installed with the app, so help works on a computer with no internet connection. (6e1ed70; apps/desktop/public/help/)
- The written help steps are translated into all five app languages: English, French, Spanish, German and Portuguese. (6e1ed70; apps/desktop/src/i18n/messages.ts; packages/i18n/i18n-fr.json)
- Built-in template names are now shown in the app's language, so buttons match the help steps, while a team's own templates keep their names. (6e1ed70; apps/desktop/src/i18n.ts)

### Milestone 1: Ready for a clinic to try

#### 2026-10-18: Consent, encryption and team members

Finished on 18 October: recording consent, encryption of every encounter file on disk, and team members with roles recorded against every change to a note.

- A consent step before recording captured how the client agreed and who confirmed it, and saved this with the encounter. (84f000c; apps/desktop/src/components/ConsentStep.tsx; engine/os-engine/src/session.rs)
- At the product owner's request, consent became a one-time notice on each device, and the step before each recording became an optional Settings choice, off by default. (d51a23c; apps/desktop/src/components/ConsentNotice.tsx; apps/desktop/src/context.ts; docs/plans/M1.md)
- Every encounter file, including audio, transcripts and notes, is now encrypted on disk, with the key kept in the computer's secure keychain where one is available. (5e357a6; docs/adr/0008-encryption-at-rest.md; engine/os-engine/src/vault.rs)
- Settings shows a recovery key for the clinic to store safely, because encounters cannot be opened if the key is lost. (5e357a6; docs/adr/0008-encryption-at-rest.md; apps/desktop/src/screens/Settings.tsx)
- Older encounters can be encrypted in place from Settings; on the Windows test machine, six were converted and no patient names were left readable on disk. (5e357a6; docs/plans/M1.md; engine/os-cli/src/sessions.rs)
- Clinics can add team members as veterinarian, technician or reception, each with their own PIN, and switch user from the sidebar. (9ce6219; engine/os-engine/src/users.rs; apps/desktop/src/components/UserPicker.tsx; apps/desktop/src/components/TeamSettings.tsx)
- Only veterinarians can sign notes and add addenda, and each recording, edit, signature and addendum records who made it and their role. (9ce6219; engine/os-engine/src/users.rs; docs/plans/M1.md)
- PINs show who is working but are not a secure login, and clinics that set up no team keep using the app as before. (9ce6219; docs/plans/M1.md)

#### 2026-10-17: Templates, editing and client documents

Clinics gained their own templates, ways to correct a draft before signing, addenda after signing, client handouts, PDF export and several languages.

- Clinics can build their own note templates from headings, with optional instructions and a live preview, and every saved change keeps a version. (f38ea78; docs/adr/0006-templates-from-headings.md; apps/desktop/src/screens/Templates.tsx)
- Ask on any heading redrafts just that section from the transcript, and the request can be saved as a team standard that later notes follow. (f38ea78; docs/templates.md; apps/desktop/src/components/RefinePanel.tsx)
- Vets can edit any heading by hand, with each edit saved as a new revision and marked as entered by the vet, so a corrected dose no longer blocks signing. (b8e12ec; engine/os-engine/src/edit.rs; apps/desktop/src/components/HeadingEditor.tsx)
- Signed notes are locked against edits and redrafts, and later corrections are added as dated, signed addenda. (b8e12ec; engine/os-engine/src/edit.rs; docs/templates.md)
- Built-in callback note, discharge instruction and client summary templates were added; the client documents follow the vet's note and are being tested. (being tested) (d62fec9; packages/templates/discharge/template.yaml; docs/eval/M1.md)
- Notes and client documents can be exported as a PDF with the clinic letterhead, visit details, signature and addenda, on Letter or A4 paper. (d51a23c; engine/os-engine/src/pdf.rs)
- Notes can be copied as plain text for most practice management systems, as formatted text, or as Markdown, with or without transcript line numbers. (d51a23c; apps/desktop/src/copyFormats.ts)
- The app interface comes in five languages and notes can be written in seven, but the translations still need review by native-speaking veterinary staff. (being tested) (d51a23c; docs/languages.md)

#### 2026-10-17: More accurate notes, with visible checks

A scoring tool for synthetic consults guided changes that raised accuracy, added a second listen for numbers and drug names, and gave every encounter patient details.

- A new evaluation tool scores each draft against the known facts of synthetic consults, counting missed vitals and medications, invented numbers and citation errors. (591ad1b; engine/os-cli/src/eval.rs; docs/eval/M1.md)
- Misheard drug and disease names, such as "marapitin" for maropitant, now get a visible suggestion on the transcript, and the transcript text itself is never changed. (591ad1b; engine/os-engine/src/vocab.rs; packages/vocab/veterinary.tsv)
- Clinics can add their own words to the veterinary vocabulary these suggestions draw on, by editing a word list file on the computer. (591ad1b; engine/os-engine/src/vocab.rs; docs/eval/M1.md)
- Measurements are now written as a number with a separate unit, and every vital in the synthetic tests came out correct with its unit. (591ad1b; docs/eval/M1.md)
- A second speech recogniser, Whisper, listens again to lines with numbers or clinical terms and flags disagreements, such as 100 mL heard as 10 mL. (837f400; engine/os-engine/src/crosscheck.rs; docs/eval/M1.md)
- Patient name, last name and reason for visit can be entered on the Recorder, edited on Review, and appear in Encounters and at the top of copied notes. (837f400; apps/desktop/src/components/PatientFields.tsx; engine/os-engine/src/session.rs)
- Patient details in a draft must match the transcript line they cite, so a guessed species such as "dog" is flagged for review. (837f400; engine/os-engine/src/faithfulness.rs; docs/eval/M1.md)
- On synthetic consults, the share of facts found rose from 33 of 44 to 40 of 44, with no invented numbers; there are no results from real clinics yet. (f7fbc20; docs/eval/M1.md)

#### 2026-10-17: Live recording and a new review screen

OpenBark began recording consults live, gained a redesigned interface built around reviewing the draft against the transcript, and made first-run model setup a single button.

- Consults can now be recorded live from the computer's microphone, with the transcript and speaker labels appearing while the consult is still going. (81d49c6; engine/os-engine/src/live.rs; docs/plans/M1.md)
- On a 9.6-minute synthetic consult played in as a live recording, the Windows test machine saved the draft note about 13 seconds after stop was pressed. (81d49c6; docs/perf/M0.md; docs/plans/M1.md)
- The browser version of OpenBark can also record live from the microphone instead of only taking uploaded files; this is being tested. (being tested) (81d49c6; apps/desktop/src/engine.ts; engine/os-server/src/lib.rs)
- The app was redesigned around a Review screen that links each note line to the transcript it came from, alongside Encounters, Models and Settings screens. (81d49c6; docs/design/DESIGN.md; apps/desktop/src/screens/Review.tsx)
- The Sign note button stays disabled while the draft contains a number that does not appear in the transcript. (81d49c6; apps/desktop/src/screens/Review.tsx)
- Each heading of a draft can be copied on its own with one click, ready to paste into the practice management system. (81d49c6; apps/desktop/src/components/NoteView.tsx)
- A Hugging Face browser lets clinics search for other note models and see whether each fits their computer, and restrictive licences must be accepted before downloading. (81d49c6; engine/os-models/src/hub.rs; apps/desktop/src/screens/Models.tsx)
- On first run, the Recorder lists any missing models with their sizes and downloads them all with one button, picking up again after an interruption. (8f9be0f; apps/desktop/src/components/SetupPanel.tsx; docs/plans/M1.md)
- Recordings from a phone or voice recorder (m4a, mp3, caf, flac, ogg) can be imported and transcribed like any other consult; so far only m4a has been tested. (86d8e1b; engine/os-asr/src/audio.rs; docs/adr/0007-phone-calls.md)

### Milestone 0: The first working pipeline

#### 2026-10-17: First app, test consults and a speed report

A first desktop app and browser view, synthetic test consults, and a speed and safety report from one shared Windows computer.

- A first desktop app showed the transcript beside the draft note, with copy, saved sessions and a light or dark theme; it is an early preview being tested. (being tested) (116ac57; apps/desktop/src/App.tsx, apps/desktop/src-tauri/tauri.conf.json)
- Clicking any line of the transcript played that moment of the recording, so a vet could check what was actually said. (being tested) (116ac57; apps/desktop/src/App.tsx)
- The same screens could open in a web browser on the same computer only, never reachable from other machines on the network. (being tested) (c46c040; engine/os-server/src/lib.rs, engine/os-cli/src/main.rs; b496a2b; docs/network.md)
- A command-line tool turned a recording into a saved SOAP draft in one step and could redraft a saved consult with another model, for developers and testing. (being tested) (c46c040; engine/os-cli/src/main.rs, engine/os-cli/src/sessions.rs)
- We wrote four synthetic consults, including a feline vomiting visit and a multi-pet visit, read aloud by a computer voice, for testing without real patient data. (c46c040; packages/eval/fixtures/scripts/feline-vomiting.yaml, packages/eval/fixtures/scripts/multipet.yaml, tools/fixture-tts/src/main.rs)
- On one shared Windows computer, a 9.6-minute synthetic consult took about 77 seconds from recording to draft with Gemma 4 E4B, short of the 60-second goal; macOS was not measured. (being tested) (b496a2b; docs/perf/M0.md, docs/perf/results/rtx4080-win11-20261017/)
- Testing found the speech model sometimes dropped the word hundred and misheard drug names such as maropitant, so fixes were proposed for the next milestone. (b496a2b; docs/perf/M0.md)
- We published a list of every network connection OpenBark can make: model downloads only, with no telemetry, analytics or cloud fallback. (b496a2b; docs/network.md)
- We wrote setup guides for Windows and for Mac computers; only the Windows guide has been used so far, and the Mac guide is untested. (being tested) (b496a2b; docs/runbooks/windows.md, docs/runbooks/macos.md)
- We proposed optional cloud sync that would be off by default and end-to-end encrypted, so a server could never read notes; it is not built. (b496a2b; docs/adr/0005-optional-cloud-sync.md)

#### 2026-10-17: A plan and an engine that drafts SOAP notes

We set out the first milestone and built the local engine that turns a consult recording into a transcript and a cited SOAP draft.

- We published the first milestone plan, chose the name OpenBark and released the code under the Apache-2.0 open-source licence. (bb8ae01; docs/plans/M0.md, LICENSE, NOTICE)
- A consult recording became a transcript on the computer itself, using the Parakeet speech model, with no cloud service involved. (4b23fd8; engine/os-asr/src/recognizer.rs, engine/os-models/catalog.json, engine/os-engine/src/models.rs)
- The transcript labelled who was speaking, and in synthetic test consults it kept the speakers apart over 99 percent of the time. (4b23fd8; engine/os-diar/src/lib.rs; b496a2b; docs/perf/M0.md)
- Gemma 4 drafted a SOAP note from the transcript using a template that told it to use only what was said in the room. (4b23fd8; packages/templates/soap/template.yaml, packages/templates/soap/schema.json, engine/os-engine/src/note.rs)
- Gemma 4 was offered in three sizes, E2B, E4B and 12B, with E4B as the default for drafting notes. (4b23fd8; engine/os-models/catalog.json, engine/os-engine/src/models.rs)
- Sections that never came up in the consult showed as Not discussed, rather than being filled with assumed normal findings. (4b23fd8; engine/os-engine/src/render.rs, packages/templates/soap/template.yaml)
- A faithfulness check compared every number in the draft, such as vitals and doses, against the transcript and marked any it could not match for review. (4b23fd8; engine/os-engine/src/faithfulness.rs, engine/os-engine/src/numbers.rs)
- Every draft was labelled as not yet reviewed or signed by a veterinarian, with a count of items to check against the recording. (4b23fd8; engine/os-engine/src/render.rs)
- Models downloaded once from fixed, checksum-verified versions, could resume after an interrupted download, and then ran fully offline. (4b23fd8; engine/os-models/src/download.rs, engine/os-models/catalog.json)
- Each consult was saved in its own folder on the computer with audio, transcript and notes, not yet encrypted, and the computer was kept awake during long jobs. (4b23fd8; engine/os-engine/src/session.rs, engine/os-engine/src/awake.rs)

## Coming next

Planned or being tested, not finished. No dates are promised.

- The Mac and Linux installers were built automatically but have not yet been installed and checked on real computers, so they are still being tested. (9e11fdd; README.md (Status: macOS and Linux 'have not yet been run and checked on real machines'); docs/runbooks/macos.md; docs/perf/M0.md 'Pending' (Apple silicon run unchecked); .github/workflows/release.yml)
- Public downloads and the source code have not been published yet; both are planned for the public beta. (7728e07; README.md ('Public downloads will be published with the beta', 'Status: pre-alpha'); .github/workflows/release.yml (no version tag exists, so no release has been built))
- The installers are not signed yet, so Windows and macOS show a security warning the first time OpenBark is opened. (9e11fdd; docs/adr/0012-point-and-click-installers.md Decisions 3 and 6 (macOS ad-hoc signed, code signing not decided); apps/desktop/src-tauri/tauri.conf.json (signingIdentity "-"))
- No iPhone or Android app has been built, and the test of phone recording with the screen locked has been designed but not run. (b496a2b; docs/spikes/S1-mobile-audio-and-inference.md ('Status: designed, not started'); apps/ contains only desktop and fixtures-v1)
- A single visit could not yet be deleted from inside the app, and an option to remove old recordings automatically after a set time was still planned. (b496a2b; docs/plans/M0.md decision 8 (optional auto-delete policy); engine/os-app/src/lib.rs (only a failed import's folder is removed, no delete-visit command); apps/desktop/src/engine.ts)
- A clickable tooth chart for dogs and cats, filled in from the consult and shown in the note and PDF, was planned but not started. (a06874e; docs/plans/M2.md item 7 ('Not started'); no chart component in apps/desktop/src (Triadan appears only as a heading example))
- Summaries of a patient's past records, including scanned PDFs read on the clinic's own computer, were planned but not started. (a06874e; docs/plans/M2.md item 8 ('Not started') and 'Decisions for the product owner' (OCR engine still to choose); no OCR library in the engine)
- OpenBark has only been measured on synthetic test consults on one Windows computer; checking it in real clinics comes before any clinical use. (7728e07; README.md (Status: 'has not yet been validated in real clinics'); docs/perf/M0.md; packages/eval/fixtures/)
