# Getting started

Video: **Help → Getting started** in the app.

## Install

- **Windows:** double-click **OpenBark_<version>_x64-setup.exe** and follow the wizard. OpenBark appears in the Start menu.
- **macOS:** open the **.dmg** and drag **OpenBark** onto **Applications**. The first time, macOS may ask you to allow it in **System Settings > Privacy & Security > Open Anyway**.
- **Linux:** double-click the **.deb** or **.rpm** and choose **Install** in your Software app, or run the **AppImage**.

To uninstall, choose **Uninstall OpenBark**: in the Start menu on Windows, or on the disk image on a Mac. On Linux, remove OpenBark in your Software app. Your encounters and models are kept unless you choose to delete them, and deleting them asks twice. Details, and the install scripts for the browser version: [install/README.md](../../install/README.md).

## The consent notice

The first time OpenBark opens on a computer, it shows a short notice. Recording laws differ by place, and many require everyone in a conversation to agree to be recorded. Your clinic is responsible for telling clients that you record and for getting their consent, as your local rules require. Tick the box and choose **Accept and continue**. The notice won't come back on that computer unless you ask for it again in **Settings → Consent to record**.

If you want OpenBark to record how each client consented (out loud, a signed form, or consent on file), turn on **Record how the client consented, each time I record** in Settings. The consent is then saved with each encounter and shown in Review.

## Download the models (once)

OpenBark transcribes and writes notes with models that run on your computer. If they aren't there yet, the **Record** screen shows **Download the models to start** with the total size (about 6 GB). Choose **Download everything**. The smallest models download first, the computer stays awake, and an interrupted download resumes when you choose **Try again**. After this, OpenBark works offline.

The **Models** screen lists every model, its size and licence, and whether it fits your computer's memory. **What the model names mean** at the top explains the labels in plain words, and says which model suits your computer:

| In a name | Means |
|---|---|
| 2B, 4B, 12B, 27B | The model's size, in billions of parameters. Bigger is more careful with long or complicated visits, but slower and needs more memory. |
| E2B, E4B | Gemma's efficient sizes: they run like a 2B or 4B model but know a little more. E4B is the everyday choice. |
| it, instruct | Trained to follow instructions. Note writing needs these; skip models marked base or pt. |
| Q4_0, Q4_K_M, Q8_0, 4-bit | How much the file is compressed. 4-bit is about a quarter of the size with a small loss, and is the usual choice. |
| QAT | Trained to be compressed: a 4-bit file with nearly full quality. Prefer it when offered. |
| GGUF, MLX | File formats. OpenBark runs GGUF; MLX is Apple's format (labelled on a Mac). |

As a rule of thumb: older laptops, Gemma 4 E2B (about 3 GB); most laptops, E4B (about 5 GB); a graphics card with 10 GB or more, or a Mac with 24 GB or more, 12B (about 7 GB). Every model name in the app and in the Hugging Face browser shows these labels; rest on one to read its meaning. You can browse Hugging Face for other note-writing models there. On a Mac, models converted for Apple's MLX framework are marked **MLX optimized** (see [ADR-0009](../adr/0009-mlx-on-mac.md)); OpenBark runs the GGUF builds, on the Mac's graphics chip.

## Settings to set up first

- **Name for signing notes:** saved with each note you sign. If your clinic sets up a team, each person signs in with a PIN instead (see [Team and security](team-and-security.md)).
- **Language:** the app's language, and the language notes are written in (see [Languages](../languages.md)).
- **PDFs and copying:** your clinic's name and address for PDFs, the paper size, and how **Copy note** pastes into your PIMS.

## Where things are

| Screen | What it's for |
|---|---|
| Record | Start a consult, or import a recording |
| Encounters | Every recording, with its patient, template and status (Transcribed, Draft, Signed) |
| Templates | Your clinic's note templates |
| Models | The speech and note models |
| Settings | You, your team, languages, consent, PDFs, encryption |
| Help | These walkthroughs, with videos |
