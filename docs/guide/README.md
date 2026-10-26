# OpenBark user guide

OpenBark records a consultation, transcribes it, and drafts the medical record for a veterinarian to check and sign. Everything runs on your computer. Recordings and notes stay on your devices unless you turn on encrypted sync. Even then, only encrypted copies leave, and our servers can't read them.

The same walkthroughs are in the app under **Help** (on a phone, **Settings → Guides and videos**), each with a short video.

| | Guide | What it covers |
|---|---|---|
| 1 | [Getting started](getting-started.md) | The consent notice, downloading the models, your name and languages |
| 2 | [Recording a consult](recording.md) | Patient details, recording, importing a recording (such as a phone call), a client on a phone on speaker |
| 3 | [Reviewing and signing](review.md) | Checking the draft against the recording, Ask, Edit, signing, addenda |
| 4 | [Templates](templates.md) | Your clinic's own note headings, and standards |
| 5 | [Client documents, PDF and copying](documents.md) | Discharge instructions, client summaries, PDFs, pasting into your PIMS |
| 6 | [Team and security](team-and-security.md) | Team members and roles, encryption, the recovery key, moving computers |
| 7 | [Models](models.md) | Choosing and downloading models, MLX on a Mac, gated models and tokens, adding and removing models |
| 8 | [Phone calls](phone-and-email.md) | Calling clients through your clinic's own Twilio account, the announcement, and calls as visits |
| 9 | [Encrypted sync](sync.md) | Preview: set up the clinic hub and join your iPhone and computers; transcripts and notes travel encrypted, recordings never leave the device that made them |
| | [Languages](../languages.md) | The app's language, the note's language, and what to expect |
| | [Troubleshooting](troubleshooting.md) | Common messages and what to do |

## Five-minute start

1. Open OpenBark. Read the consent notice and accept it.
2. On **Record**, choose **Download everything** if it's offered. This happens once.
3. In **Settings**, enter your name for signing notes.
4. Back on **Record**, add the patient if you like (it's optional), press the large round Record button, and talk as usual. Press it again (on a phone, **Stop and save**) to stop.
5. In **Review**, check the draft against the recording, fix anything, and choose **Sign note** (on a computer: during the pilot the iPhone doesn't sign). Visits still to check and sign wait in **Visits**, under **Needs you**.

Drafts are never final until a veterinarian signs them.

## For maintainers: the in-app videos

The Help videos are recordings of the real app, made by `apps/desktop/e2e/help-videos.mjs`: `apps/desktop/public/help/<topic>.webm` in the light theme and `<topic>-dark.webm` in the dark theme (about 3 to 15 MB each, no sound). Help plays the one that matches the theme the app is showing and falls back to the light one. Re-record both when the screens they show change.

Record against a **demo** server with synthetic visits only, never a clinic's own data, and with neutral folders, because Settings, Storage shows the folder paths on screen. The script refuses to start if the server's folders show your user name, `AppData` or a temp folder, and while it records it fails any topic whose page shows them.

1. Make the demo folder (Windows example; `cmd` for the junction):
   - `C:\OpenBark-demo\visits`, `C:\OpenBark-demo\templates`, and an empty team: no `users.json` yet (the team video adds the first member).
   - `C:\OpenBark-demo\models`: a junction to the models you've already downloaded, so nothing downloads again and their real location isn't shown: `mklink /J C:\OpenBark-demo\models %LOCALAPPDATA%\OpenBark\models`. The recording only reads it.
   - `C:\OpenBark-demo\key`: a throwaway key, 32 random bytes in base64 (PowerShell: `[Convert]::ToBase64String((1..32 | % { Get-Random -Max 256 }) -as [byte[]])`). Never use your own key.
   - `C:\OpenBark-demo\locations.json`: `{ "visits": "C:\\OpenBark-demo\\visits", "models": "C:\\OpenBark-demo\\models" }`. Choosing the two folders this way, rather than with `OPENBARK_SESSIONS_DIR` and `OPENBARK_MODELS_DIR`, keeps **Change folder** available in the Storage frames, as a clinic sees it.
2. Set the environment for every `openbark` command below: `OPENBARK_LOCATIONS_FILE=C:/OpenBark-demo/locations.json`, `OPENBARK_TEMPLATES_DIR=C:/OpenBark-demo/templates`, `OPENBARK_USERS_FILE=C:/OpenBark-demo/users.json`, `OPENBARK_DATA_KEY=<the contents of C:/OpenBark-demo/key>` and `OPENBARK_VOCAB=C:/OpenBark-demo/vocab.tsv` (a file that needn't exist, so your own word list isn't read).
3. Seed a few visits from the synthetic consults in `packages/eval/fixtures/audio` (the WAVs come from `packages/eval`'s generator and aren't committed), for example:
   `openbark run C:/OpenBark-demo/fixtures/feline-vomiting.wav --diarize --speakers 2 --template sick-visit --patient Mochi --last-name Tanaka --reason "Vomiting and weight loss" --sessions-dir C:/OpenBark-demo/visits --models-dir C:/OpenBark-demo/models`
   The videos use Ziggy Chen (exotics), Jasper Moreau (equine), Pepper Okonkwo (dermatology), Biscuit Alvarez (canine-wellness) and Mochi Tanaka (feline-vomiting). Then, in the app, give each patient a species (Patients, Edit details) and sign Pepper's dermatology consult, so the Patients video shows a dog with two visits, one signed. Leave Mochi's consult unsigned: the Review video signs it. Copy `visits` somewhere outside the demo folder now: it's the starting point for each pass.
4. Build the UI (`cd apps/desktop && pnpm build`) and start the server: `openbark serve --port 4318 --ui-dir apps/desktop/dist`. Run it from `C:\OpenBark-demo`.
5. Record the light pass: `cd apps/desktop && OPENBARK_URL=http://localhost:4318 HELP_THEME=light node e2e/help-videos.mjs`. You can name topics (`record review`), but they build on each other in order: Record adds Pepper's recheck, which Patients finds by typing "Pepp"; Review redrafts, edits and signs Mochi's consult, which Documents uses; and Team adds the first member, after which every page asks who's using OpenBark. A failed topic is kept in `target/help-video-failed/` and never replaces the video in `public/help`. The recording topic plays a WAV as the microphone (`FAKE_MIC`, default `packages/eval/fixtures/audio/smoke.wav`); give it a copy of `smoke.wav` with a few seconds of silence at the end, because the browser loops the file.
6. Stop the server, put the demo back (restore `visits` from your copy, empty `templates`, delete `users.json`), start it again, and record the dark pass with `HELP_THEME=dark`.
7. Check frames with `node e2e/frames.mjs <topic> <seconds…>` (for example `record-dark 12 30`).

The **sync** video (Help, Set up encrypted sync) needs two servers built with `OPENBARK_SYNC_PREVIEW`: the clinic computer that becomes the hub (`OPENBARK_URL`) and a second computer that joins it (`OPENBARK_SYNC_URL`), both fresh for each theme. It isn't part of a run without `OPENBARK_SYNC_URL`. The **Help videos** workflow (`.github/workflows/help-videos.yml`; run it from the Actions tab or push a commit containing `[help-videos]`) records it in both themes on a runner with neutral demo folders and uploads the videos and frames to check as the `help-videos` artifact; commit the two `.webm` files to `apps/desktop/public/help/`.

The captions are in English. The written steps under each video are translated with the rest of the app (`scripts/i18n-keys.py` picks them up from `src/screens/Help.tsx`).
