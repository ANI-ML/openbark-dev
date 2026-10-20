# Troubleshooting

| You see | What it means | What to do |
|---|---|---|
| **Download the models to start** | The speech or note models aren't on this computer yet | Choose **Download everything**. If a download stops, choose **Try again**: it resumes. |
| "Microphone access was blocked" | The browser was told not to use the microphone | Allow the microphone for this page in the browser's address bar, then record again. See [Microphone problems](#microphone-problems). |
| "… is sending silence" while recording | The microphone is muted, or OpenBark isn't allowed to use it | See [Microphone problems](#microphone-problems). |
| "The microphone was disconnected, so recording is paused" | The microphone was unplugged or a Bluetooth headset dropped out, and no other microphone is connected | Plug it back in or connect another: recording carries on by itself. Or press Stop to keep what was recorded. |
| A red callout: "… isn't anywhere in the transcript" | The draft has a number nobody said | Listen to the recording, then correct the line with **Edit**. You can't sign until it's fixed. |
| "The second recogniser heard … instead of …" | Two speech models disagree about a number | Click the line to hear it, and correct it if needed. |
| "Only a veterinarian can sign" | A technician or reception is using OpenBark | Choose your name at the bottom of the sidebar and switch to a veterinarian. |
| "Choose who you are first" | The team is set up and nobody has picked their name | Pick your name and enter your PIN. |
| "That PIN isn't right" | The PIN doesn't match | Try again, or ask a veterinarian to remove and re-add you with a new PIN. |
| "This note is signed, so it can't be changed" | Signed notes are locked | Add an addendum instead. |
| "Another program is using a file in this visit" | Windows won't move or remove a file while another program has it open, such as an Explorer window, a media player or antivirus | Close any window showing the visit's folder or recording, wait a moment, then try again. Nothing was deleted, and the visit is still listed. |
| "Signed visits can't be deleted yet" | The visit has a signed note, for at least one animal | Correct the note with **Add addendum**. To remove only the recording, a veterinarian can choose **Delete recording**. |
| "This visit was deleted" | Another window, or an old link, opened a visit that is now in Deleted visits | Restore it from **Settings → Deleted visits** if you need it. |
| "OpenBark couldn't record the deletion, so the visit was kept" | The deletion record in the visits folder couldn't be written (a full or read-only drive, or a folder permission) | Check the drive has space and that you can write to the visits folder, then try again. |
| "This visit is being drafted. Delete it when that's finished." | A draft of this visit is being written, perhaps in another window | Wait for the draft to finish, then delete. |
| "The clipboard isn't available here" | The browser didn't allow copying | Select the text and copy it with the keyboard, or use the desktop app. |
| Settings, Encryption: "The key is kept in a file …" | This computer's keychain isn't available (often over a remote connection) | It still works. Keep the recovery key somewhere safe. |
| "… can't be decrypted with this computer's key" | The encounter was encrypted with a different key | Import the recovery key that was used before (see [Team and security](team-and-security.md)). |
| "This model writes your notes. Choose another one under In use first." | You tried to remove the model this device uses for notes | Choose another note model under **In use** on the Models screen, then remove it. See [Models](models.md). |
| "A note is being written with this model" | A draft, an Ask or a template from an example is using the model | Wait for it to finish, then remove the model. The draft isn't interrupted. |
| "This model is gated" or "Hugging Face hasn't given your token access yet" | The publisher asks you to request access first | Choose **Request access**, then save a Hugging Face token. See [Models](models.md#gated-models-and-your-hugging-face-token). |
| A model shows **Not found** | It was added from this computer to be used where it is, and the file or folder has moved or its drive is unplugged | Plug the drive back in, or choose **Forget** and add it again from its new place. |
| The MLX engine card says it "didn't pass its check" | The MLX engine can't run on this Mac as it is | Choose **Try again**. GGUF models keep working; choose one under **In use**. |
| Drafting is slow | The note model is sharing the graphics card, or the computer has no supported graphics card | Close games and video tools, or choose a smaller note model in **Models**. |

## Microphone problems

Start with **Test microphone** on **Record** (or in **Settings → Recording**): speak, and the bar should move and say **Hearing you**.

### OpenBark isn't allowed to use the microphone

The test bar stays flat, or the Record screen says the microphone "is sending silence". Computers hand an app that isn't allowed the microphone silence rather than an error.

**On a Mac:**

1. Open **System Settings → Privacy & Security → Microphone**.
2. Turn on **OpenBark** (in the browser version, turn on the browser you use, such as Safari or Chrome).
3. Quit OpenBark (or the browser) and open it again.

If OpenBark isn't in the list, open it, press Record once so macOS asks, and choose **Allow**.

**On Windows 10 and 11:**

1. Open **Settings → Privacy & security → Microphone**.
2. Turn on **Microphone access**, and **Let desktop apps access your microphone** (the desktop app and browsers are desktop apps).
3. In the browser version, also check the browser has the microphone allowed for the OpenBark page: select the icon at the left of the address bar.

**In the browser,** if you chose **Block** when it asked, select the icon at the left of the address bar (Chrome and Edge), or **Permissions** in the address bar (Firefox), or **Safari → Settings for This Website** (Safari), and allow the microphone. If you closed the question without answering, press Record again and choose **Allow**.

### Choosing the right microphone

- Leave **System default** to use whatever the computer uses, or pick one by name in the **Microphone** list. In the browser version the names appear once the microphone is allowed (press **Test microphone** once).
- A microphone picked by name that isn't connected is replaced by the system default, and a message says so.
- On a Mac, the system default is set in **System Settings → Sound → Input**. On Windows, in **Settings → System → Sound → Input**.
- Check the microphone's own mute: a switch on a headset or USB microphone, or a mute key on the keyboard (often F4 or a key with a crossed-out microphone).

### "Another app is using the microphone"

Most apps share the microphone. A few take it for themselves while they run a call (some settings in Teams, Zoom or audio editors use Windows' exclusive mode). Close the other app or end its call, then press Record again. To stop it happening, open **Control Panel → Sound → Recording**, open the microphone's **Properties → Advanced**, and turn off **Allow applications to take exclusive control of this device**.

### Bluetooth headsets and AirPods

A Bluetooth headset has two modes: a high-quality one for listening, and a call mode that turns its microphone on. In call mode the sound is narrow, like a phone line (16 kHz, or 8 kHz on older headsets), and words are missed more often. OpenBark says so on the Record screen when a headset is in call mode.

- For consults, use the computer's own microphone or a wired or USB microphone. Choose it by name in the **Microphone** list so that connecting AirPods doesn't move the recording to them.
- If AirPods connect while you record with **System default** chosen and the computer makes them the default, recording moves to them. Choose the built-in microphone in the list to stop that.
- If a headset drops out mid-recording, recording moves to the default microphone by itself.

### Other sounds go quiet when you record (Windows)

Windows can turn other sounds down while something uses the microphone "for communication". OpenBark records the microphone as it is and doesn't ask for this, but another app can. In **Control Panel → Sound → Communications**, choose **Do nothing**.

### On a Mac: Mic Mode

If Control Center shows **Mic Mode** while you record, choose **Standard**. **Voice Isolation** keeps only the voice nearest the microphone and removes the others, such as the client's.

### Keep the tab open (browser version)

Browsers may slow down a page in a background tab. Recording keeps going there in current browsers, but if the Record screen says "No sound is reaching OpenBark", bring the tab back to the front. On a laptop, plug it in for long consults: the computer is kept awake while recording, but a closed lid still puts it to sleep.

