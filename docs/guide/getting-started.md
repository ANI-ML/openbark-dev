# Getting started

Video: **Help → Getting started** in the app.

## Install

- **Windows:** double-click **OpenBark_<version>_x64-setup.exe** and follow the wizard. OpenBark appears in the Start menu.
- **macOS:** open the **.dmg** and drag **OpenBark** onto **Applications**. The first time, macOS may ask you to allow it in **System Settings > Privacy & Security > Open Anyway**.
- **Linux:** double-click the **.deb** or **.rpm** and choose **Install** in your Software app, or run the **AppImage**.

To uninstall, choose **Uninstall OpenBark**: in the Start menu on Windows, or on the disk image on a Mac. On Linux, remove OpenBark in your Software app. Your encounters and models are kept unless you choose to delete them, and deleting them asks twice. Details, and the install scripts for the browser version: [install/README.md](../../install/README.md).

## The consent notice

The first time OpenBark opens on a computer, it shows a short notice. Recording laws differ by place, and many require everyone in a conversation to agree to be recorded. Your clinic is responsible for telling clients that you record and for getting their consent, as your local rules require. Tick the box and choose **Accept and continue**. The notice won't come back on that computer unless you ask for it again in **Settings → Recording and consent**.

If you want OpenBark to record how each client consented (out loud, a signed form, or consent on file), turn on **Record how the client consented, each time I record** in Settings. The consent is then saved with each encounter and shown in Review.

## Set up OpenBark (once)

OpenBark transcribes and writes notes with models that run on your computer. The first time it opens, after the consent notice, **Set up OpenBark** walks you through them. Decisions: [ADR-0034](../adr/0034-setup-walkthrough.md).

1. **This computer.** Its memory, processor cores, graphics (a dedicated graphics card, integrated graphics or Apple silicon), the free space where models are kept, and the system, with what that means: notes are written on the graphics, or on the processor.
2. **The note model.** OpenBark recommends Gemma 4 E2B or Gemma 4 E4B for this computer and says why in a sentence, for example "16 GB of memory but no graphics card, and 4 processor cores: the smaller model writes notes about 1.6 times as fast on a processor, and finds slightly fewer details (90% of facts against 95% in OpenBark's tests)". Both are shown with their download size and what each does better or worse; you can choose the other one.
3. **Speech models.** The built-in speech models: what each does, its size, and that they run on this computer. On Windows with an NVIDIA graphics card, the optional **GPU speech pack** is offered too, after you accept NVIDIA's licence.
4. **Download.** Smallest first, each model with its own clock, the speed, the time left and the expected finish. The computer stays awake, and an interrupted download resumes when you choose **Try again**. After this, OpenBark works offline.
5. **Speed check** (optional). The note model writes a short practice note from an invented consult for about half a minute, nothing from your visits, and OpenBark says how long a note for a long visit will take on this computer. The estimate is worked out from the speed measured and OpenBark's long test visit (16 minutes of non-stop talking); shorter visits take less. If it's more than five minutes with Gemma 4 E4B, setup suggests Gemma 4 E2B, with the reason. Models and Record show the result afterwards. **Skip the speed check** leaves it out.
6. **Recording tips.** A USB or clip-on microphone close to the people talking, or the iPhone app placed near them; **Speakers in the room** set to 2 for a vet and a client; and review every draft before you sign it.

**Skip setup** closes it; the Record screen then offers **Set up OpenBark** until the models are downloaded. **Run setup again** is in **Models**, in **Help** and in **Settings → About**.

A computer that already had OpenBark before this version sees a one-time card on Record instead: **Check my setup** runs the first two steps and the speed check. It only suggests: the note model changes only if you choose another one. **Not now** hides the card for good.

The **Models** screen lists every model, its size and licence, and whether it fits your computer's memory. **What the model names mean** at the top explains the labels in plain words, and says which model suits your computer:

| In a name | Means |
|---|---|
| 2B, 4B, 12B, 27B | The model's size, in billions of parameters. Bigger is more careful with long or complicated visits, but slower and needs more memory. |
| E2B, E4B | Gemma's efficient sizes: they run like a 2B or 4B model but know a little more. E4B is the everyday choice. |
| it, instruct | Trained to follow instructions. Note writing needs these; skip models marked base or pt. |
| Q4_0, Q4_K_M, Q8_0, 4-bit | How much the file is compressed. 4-bit is about a quarter of the size with a small loss, and is the usual choice. |
| QAT | Trained to be compressed: a 4-bit file with nearly full quality. Prefer it when offered. |
| GGUF, MLX | File formats. OpenBark runs GGUF; MLX is Apple's format (labelled on a Mac). |

As a rule of thumb: older laptops, Gemma 4 E2B (about 3 GB); most laptops, E4B (about 5 GB); a graphics card with 10 GB or more, or a Mac with 24 GB or more, 12B (about 7 GB). Every model name in the app and in the Hugging Face browser shows these labels; rest on one to read its meaning. You can browse Hugging Face for other note-writing models there. On a Mac with Apple silicon and macOS 14 or later, models converted for Apple's MLX framework can run too, once you download the **MLX engine** on the Models screen. You can also add a model file from this computer, and remove models you no longer use. All of this is in [Models](models.md).

## Settings to set up first

- **Name for signing notes:** saved with each note you sign. If your clinic sets up a team, each person signs in with a PIN instead (see [Team and security](team-and-security.md)).
- **Language:** the app's language, and the language notes are written in (see [Languages](../languages.md)).
- **PDFs and copying:** your clinic's name and address for PDFs, the paper size, and how **Copy note** pastes into your PIMS.

## Where visits and models are kept

**Settings → Privacy and storage** shows the two folders OpenBark uses, how much each holds and how much room is left on its drive:
- **Visits**: recordings, transcripts, notes and the patient list. Your clinic's data, encrypted.
- **Models**: the speech and note models. Large, and can be downloaded again.

Choose **Change folder** to put either one somewhere else, such as a bigger second drive, or a drive that's backed up. OpenBark shows the exact folder it will use and the free space before anything changes. Then choose:
- **Move them there:** copied, checked, then removed from the old folder;
- **Start with an empty folder:** what you have now stays where it is.

A folder that already holds other files gets an **OpenBark visits** or **OpenBark models** folder inside it, so nothing is mixed in. OpenBark also tells you if the folder is synced by OneDrive or a similar service, or is on a drive that can be unplugged.

## Where things are

| Screen | What it's for |
|---|---|
| Record | Start a visit, or import a recording (**Import a recording**, at the top). **Patient** is optional: add it now or after the visit |
| Visits | Every visit, in **Needs you** (not signed yet) and **Done** (signed), with a search and Today, This week or Earlier. The number beside Visits counts the ones that need you |
| Patients | Every animal and family, and each animal's visits |
| Templates | Your clinic's note templates |
| Models | The speech and note models |
| Settings | One section at a time: you and your team, recording and consent, languages, phone calls, PDFs, encrypted sync, privacy and storage, deleted visits |
| Help | These walkthroughs, with videos |

On a computer these are in the side menu: Record, Visits, Patients and Templates at the top, Models, Settings and Help at the bottom. On a phone the tabs are **Record**, **Visits**, **Patients** and **Settings**; Templates, Models and Help are rows in Settings, and each opens with a **Settings** back link at the top left.
