# Getting started

Video: **Help → Getting started** in the app.

## Install

- **Windows:** double-click **Install OpenBark.cmd**. OpenBark appears in the Start menu and on the desktop.
- **macOS:** double-click **Install OpenBark.command**. OpenBark appears in your Applications folder.
- **Linux:** run `./install/install.sh`. OpenBark appears in your app menu.

OpenBark opens in your web browser, and only this computer can reach it. To uninstall, use **Uninstall OpenBark** (Start menu on Windows, or the same folder on macOS and Linux). Your encounters and models are kept unless you type DELETE when asked. Details: [install/README.md](../../install/README.md).

## The consent notice

The first time OpenBark opens on a computer, it shows a short notice. Recording laws differ by place, and many require everyone in a conversation to agree to be recorded. Your clinic is responsible for telling clients that you record and for getting their consent, as your local rules require. Tick the box and choose **Accept and continue**. The notice won't come back on that computer unless you ask for it again in **Settings → Consent to record**.

If you want OpenBark to record how each client consented (out loud, a signed form, or consent on file), turn on **Record how the client consented, each time I record** in Settings. The consent is then saved with each encounter and shown in Review.

## Download the models (once)

OpenBark transcribes and writes notes with models that run on your computer. If they aren't there yet, the **Record** screen shows **Download the models to start** with the total size (about 6 GB). Choose **Download everything**. The smallest models download first, the computer stays awake, and an interrupted download resumes when you choose **Try again**. After this, OpenBark works offline.

The **Models** screen lists every model, its size and licence, and whether it fits your computer's memory. You can browse Hugging Face for other note-writing models there. On a Mac, models converted for Apple's MLX framework are marked **MLX optimized** (see [ADR-0009](../adr/0009-mlx-on-mac.md)); OpenBark runs the GGUF builds, on the Mac's graphics chip.

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
