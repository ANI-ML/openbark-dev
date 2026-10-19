# Troubleshooting

| You see | What it means | What to do |
|---|---|---|
| **Download the models to start** | The speech or note models aren't on this computer yet | Choose **Download everything**. If a download stops, choose **Try again**: it resumes. |
| "Microphone access was blocked" | The browser was told not to use the microphone | Allow the microphone for this page in the browser's address bar, then record again. |
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
