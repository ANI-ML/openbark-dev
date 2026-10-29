# Recording a consult

Video: **Help → Recording a consult** in the app.

1. **Patient** is optional: add it now or after the visit. On a computer, type a few letters of a name in **Patient** to pick an animal you've seen before (its name, family name and species fill in, and the visit is added to its page under **Patients**), and add the **Reason for visit**; **More details** has the species and other animals. On a phone, tap the **Patient** row to open the same form. For a new animal, enter the patient's name and species, the client's last name and the reason for the visit. The species decides headings that only apply to some animals, such as FeLV/FIV status for cats. Everything here can be added or changed later in Review. It's saved with the visit and time-stamped. A new name makes a new patient.
2. Choose the **Template**, for example SOAP note or your clinic's own: a list beside the Record button on a computer, a row under it on a phone.
3. Press the Record button (the large round one in the middle), or the Space bar, to start.
4. Talk as you normally would. Each sentence appears in the live transcript once it's finished, with the speaker labelled. The level meter shows that the microphone is hearing you. On a phone, recording fills the screen: the patient at the top, the time, the level meter and the latest lines of the transcript.
5. Press the button again (on a phone, **Stop and save**) to stop. There's no pause: stop, and record again if you need to. The transcript is finished within seconds. With **Draft the note when I stop** ticked, the note is drafted straight away and opens in Review.

Under the Record button, a line says how consent is handled (**Change** opens the consent settings), and another names the microphone, with its level and **Change**.

While recording and drafting, the computer doesn't go to sleep. The recording is saved as it happens, encrypted, so a power cut keeps everything up to that moment.

## In the browser (web mode)

When you use OpenBark in a browser on this computer, the browser asks for permission to use the microphone the first time. Allow it. If you blocked it, allow the microphone for the page in the browser's address bar, then try again. Open OpenBark at `http://localhost` on this computer: browsers only allow the microphone on a secure page, and the computer's network address isn't one.

## Checking the microphone

The microphone list and **Test microphone** are in **Settings → Recording and consent** (on a phone, **Settings → Microphone and consent**); **Change**, beside the microphone's name under the Record button, opens it. Choose a microphone (or leave **System default**), press **Test microphone** and speak: the bar moves with your voice, and a line underneath says **Hearing you**, or what's wrong. The test stops when you press **Stop test**, start recording, or after a minute.

While you record, OpenBark keeps an eye on the microphone and says so on the Record screen when:

- **it sends only silence** for a few seconds: it's muted, or OpenBark (or the browser) isn't allowed to use it;
- **no sound arrives** at all: OpenBark reopens the microphone by itself;
- **it's unplugged** or a Bluetooth headset drops out: recording moves to the system's default microphone and a message says which one. With no microphone left, recording pauses, nothing silent is added, and it carries on as soon as one is connected. Stop at any time to keep what was recorded;
- **a Bluetooth headset is in its call mode**, which records in narrow phone quality: use the computer's own microphone or a wired one for consults.

With **System default** chosen, recording follows the system: connect AirPods or a headset and make it the default, and recording moves to it. A microphone chosen by name is kept, and taken back if it's unplugged and plugged in again.

Opening Record also loads the speech models in the background (when the computer has memory to spare), so recording starts as soon as you press the button.

## Importing a recording

**Import a recording**, at the top of the Record screen, creates a visit from an audio file. The file goes through the same transcription as a live recording. These work:

- WAV (including ADPCM from dictaphones), AIFF, MP3 and MP2, FLAC;
- phone recordings: M4A, MOV, CAF;
- voice notes and calls: Ogg and Opus (WhatsApp, Telegram), WebM and MKV (browser recordings, Zoom, Teams, Meet);
- video files (MP4, MOV, WebM): OpenBark uses their sound.

In the browser version, a file OpenBark can't read itself is decoded by the browser first, so some other kinds work too. Windows Media (WMA), AMR and Olympus or Philips dictation files (DSS, DS2) don't; the message says so, and you can convert them with the recorder's own software.

To chart a **phone call**, record it with your phone's own call recording, export the audio, then import it and draft with the **Phone call** template. Or call the client from OpenBark itself, through your clinic's own Twilio account: the call is recorded on this computer and becomes a visit ([Phone calls](phone-and-email.md)).

### How long it takes

A progress bar shows each step: reading the file, transcribing (minutes of audio done out of the total), checking numbers with the second recogniser, and saving. Once it has enough to go on, it shows the time left. Stopping a live recording shows the same bar while the last words are transcribed.

On Windows with an NVIDIA graphics card, **Models > Faster transcription on the graphics card** downloads what speech recognition needs to run on the card (about 1.9 GB from NVIDIA and others, plus two larger speech models). On an RTX 4080 an 8.5-minute recording was ready in 43 seconds instead of about 2.5 minutes. Restart OpenBark after the download; the Models screen then says the graphics card is in use.

## Drafting a visit again

On a patient's page, the circular-arrows button beside a visit is **Reprocess**: draft the note again from the same recording, either as the same type of visit (when a draft went wrong) or as another type, such as a recheck drafted as a new consult. Earlier drafts are kept. Signed notes are final: add an addendum in Review instead.

## A client on a phone on speaker

Sometimes the client joins by phone: you've called them from the exam room, or they're on speaker while you examine their pet. Their voice reaches the computer's microphone much quieter than yours (often a hundred times weaker in sound energy), squeezed into the narrow sound of a phone line, and with the echo of the room. OpenBark can still pick it up, with some help.

### Better: call from OpenBark

If your clinic has set up [phone calls](phone-and-email.md) (your own Twilio account), call the client from OpenBark instead. The call is recorded on this computer with you and the client on separate channels, so the client's words are transcribed from the phone line itself, not from a loudspeaker across the room, and every line is labelled **Vet** or **Client** without guessing. It's the clearest way to chart a phone conversation.

### Phone on speaker

When the client is on a phone on speaker in the room, turn on **Phone on speaker** on **Record** (in the options beside the Record button, or under it on a phone) before you press Record. It's remembered on this computer until you untick it. The same option applies when you import a recording of such a visit.

With it ticked, OpenBark listens harder for the quieter voice: speech detection hears a copy of the sound with quiet passages lifted, it separates a quiet voice from a louder one that answers it, and each quiet sentence is brought up to a normal level before it's transcribed. Sentences that sound like a phone line go to the second speech recogniser (the one that checks numbers), which hears phone audio better. The recording itself is kept exactly as the microphone heard it. Leave the option off for ordinary consults in the room: lifting quiet sound can also pick up voices from the corridor.

### Setting up

- **Phone volume high.** Turn the phone's call volume up most of the way.
- **Phone between you and the computer.** Lay the phone face up on the desk between you and the computer's microphone, about an arm's length from the computer, not behind a monitor or in your hand while you walk around. The closer the phone is to the microphone, the less room echo comes with the voice.
- **A quiet room.** Close the door; fans, clippers and barking all compete with a quiet voice.
- **No Bluetooth headset or earbuds.** If earbuds are connected to the phone, the client's voice goes into them and the computer never hears it. Don't use a Bluetooth headset as the computer's microphone either: it records everyone in narrow phone quality.
- **On a Mac,** if Control Center shows **Mic Mode** while you record, choose **Standard**. **Voice Isolation** removes every voice except the one nearest the microphone, which is yours.

### What to expect

- The client's sentences appear in the live transcript like yours. On computer-generated test consults (synthetic voices played through a simulated phone line, loudspeaker and room, the phone 15 to 25 dB quieter than the vet), OpenBark heard nearly every client sentence. Real speakerphone recordings haven't been measured yet, so treat these figures as a guide. The client's words are still recognised less accurately than yours, mostly because of the phone line and the echo, so check names, numbers and drug names said by the client against the recording.
- **Speaker labels:** the phone voice usually gets its own speaker number, because a phone line has no high frequencies and a voice in the room does. A short reply such as "Okay" can be labelled as the nearest voice. Labels help you read the transcript; they don't change what the note says.

### When a client's sentence is missing

The recording keeps everything the microphone heard, even when a sentence didn't make it into the transcript.

1. In Review, click the transcript line just before the gap to play that moment of the recording and listen to what the client said.
2. Choose **Edit** on the heading where it belongs and type it in. Your addition is marked "Edited by" with your name.
3. Check the rest of the note before you sign, as always.

## Tips for good transcripts

- Put the computer or microphone between the people talking, not behind a monitor.
- Use a headset, or a microphone close to the person speaking, if you can. A laptop's built-in microphone across the room catches drug names worst, more so with an accent.
- Say numbers clearly: "thirty-nine point two degrees", "one milligram per kilo".
- Name drugs as you would to a colleague. OpenBark suggests likely spellings for misheard drug names, but never changes what was heard.

## Words and drug names

Speech recognition knows everyday English best. A brand name it doesn't know can come out as an ordinary word: a clinic found "Apoquel" written as "apple". OpenBark shows a hint on that word in the transcript (a dotted underline with the likely name) and the note model reads the hint. The transcript itself is never changed.

OpenBark already knows a few hundred terms written for it, a list of common mishearings ("apple" for Apoquel), and about three thousand brand and ingredient names from the public drug registers of the US, the UK, Canada, Australia and the EU (credited in **Settings → About**). Names from the registers, and mishearings of drug names, are hinted only when the talk around them is about medication: a dose, a unit, "give", "start him on", "twice a day", "for the itch", or another drug's name. "She ate an apple" gets no hint.

To add your clinic's own names, open **Settings → Words and drug names**:

1. Type the **word or name** (one or two words) and choose its **kind**: drug, vaccine, product, condition or procedure.
2. If the transcript keeps writing it wrong, add what it writes under **Heard as**, separated by commas (for example `zen really a` for Zenrelia).
3. Choose **Add**. In a team, an admin approves each change with their PIN, because the list is the whole hospital's.

Changes apply from the next recording. The list is kept on this computer in `vocab.tsv` in OpenBark's data folder (on Windows `%LOCALAPPDATA%\OpenBark\vocab.tsv`; `OPENBARK_VOCAB` names another file).

When the second speech recogniser is installed, segments where medication is discussed are heard twice. If it hears a known name where the first one heard something else, that becomes a hint too. It is told to expect the app's language (Settings → Language), so an accent isn't mistaken for another language.
