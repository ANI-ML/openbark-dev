# OpenBark user guide

OpenBark records a consultation, transcribes it, and drafts the medical record for a veterinarian to check and sign. Everything runs on your computer. Recordings and notes don't leave it unless you copy or export them.

The same walkthroughs are in the app under **Help**, each with a short video.

| | Guide | What it covers |
|---|---|---|
| 1 | [Getting started](getting-started.md) | The consent notice, downloading the models, your name and languages |
| 2 | [Recording a consult](recording.md) | Patient details, recording, importing a recording (such as a phone call) |
| 3 | [Reviewing and signing](review.md) | Checking the draft against the recording, Ask, Edit, signing, addenda |
| 4 | [Templates](templates.md) | Your clinic's own note headings, and standards |
| 5 | [Client documents, PDF and copying](documents.md) | Discharge instructions, client summaries, PDFs, pasting into your PIMS |
| 6 | [Team and security](team-and-security.md) | Team members and roles, encryption, the recovery key, moving computers |
| 7 | [Models](models.md) | Choosing and downloading models, MLX on a Mac, gated models and tokens, adding and removing models |
| | [Languages](../languages.md) | The app's language, the note's language, and what to expect |
| | [Troubleshooting](troubleshooting.md) | Common messages and what to do |

## Five-minute start

1. Open OpenBark. Read the consent notice and accept it.
2. On **Record**, choose **Download everything** if it's offered. This happens once.
3. In **Settings**, enter your name for signing notes.
4. Back on **Record**, enter the patient, press the red button, and talk as usual. Press it again to stop.
5. In **Review**, check the draft against the recording, fix anything, and choose **Sign note**.

Drafts are never final until a veterinarian signs them.

## For maintainers: the in-app videos

The Help videos (`apps/desktop/public/help/*.webm`, about 4 MB each, no sound) are recordings of the real app, made by `apps/desktop/e2e/help-videos.mjs`. Re-record them when the screens they show change:

1. Start a **demo** server with demo data, never a clinic's own. For example, copy two encounters (one signed, one draft) into a scratch folder:
   `OPENBARK_SESSIONS_DIR=<demo>/sessions OPENBARK_TEMPLATES_DIR=<demo>/templates OPENBARK_USERS_FILE=<demo>/users.json openbark serve --port 4318`
2. `cd apps/desktop && OPENBARK_URL=http://localhost:4318 node e2e/help-videos.mjs` (or name topics: `record review`). The recording topic plays `packages/eval/fixtures/audio/smoke.wav` as the microphone.
3. Check frames with `node e2e/frames.mjs <topic> <seconds…>`.

The captions are in English. The written steps under each video are translated with the rest of the app (`scripts/i18n-keys.py` picks them up from `src/screens/Help.tsx`).
