# Reviewing and signing

Video: **Help → Reviewing and signing** in the app.

Review shows the transcript and recording on the left, and the draft note on the right. On a phone, switch between them with **Transcript** and **Note**.

## Check the draft against the recording

- **Rest on any line** of the note: the transcript lines it came from light up. **Click it** to play that moment of the recording.
- Every heading is shown. If nothing was said about it, it says **Not discussed**. OpenBark never fills a gap with "normal".
- **Red callout:** a number in the note isn't anywhere in the transcript. Listen to the recording and correct it. You can't sign until it's resolved.
- **Amber underline:** something needs a second look. For example, a value is in the transcript but not in the line it cites, or the second speech recogniser heard a different number ("10" or "100"). The transcript line shows what the second listen heard.
- **Dotted underline in the transcript:** a likely misheard drug or term, with the likely spelling. This includes a dose written without its leading zero ("hydromorphone.1" is likely 0.1) and "Roman 4" for IV. The transcript itself is never changed.
- **"Check these words":** a statement whose words aren't in the lines it cites. Often the right line is suggested; sometimes the statement was never said. Listen, then fix the line or the citation.

## Ask for changes

Hover a heading and choose **Ask**. Built-in templates suggest requests made for that heading, such as "Chart every tooth that was named, by its Triadan number" under a dental note's chart; choose one or write your own. Say what should change, for example "List every fluid with its volume and rate", then choose **Redraft**. Only that heading is redrafted, from the recording; the rest of the note stays as it was. The request guides the model but isn't evidence: anything not said stays out, and the numbers are checked again.

Tick **Save as a standard** to make the request part of the template. Later drafts with that template then follow it under that heading. With a team, choose **For me** (only your drafts) or **For the hospital** (everyone's drafts, approved with an admin's PIN). Standards are listed, and can be removed, in **Templates**. **Edit this heading in the template** opens the template at that heading.

Above the note, Review says which version of the template drafted it, such as "Drafted with SOAP note, your version 3" or "SOAP note, the hospital's version 2". **Edit template** opens that template; **Show the template** shows exactly what the model was told for this draft. Changing the template later never changes this note.

## Edit it yourself

Choose **Edit** on a heading to type your own version: change, reorder, remove or add lines, medications or measurements. The edit is saved as a new draft, and earlier drafts are kept. Lines you change or add are marked "Edited by" with your name. They're your statements, so a dose you correct doesn't block signing.

## The dental chart

A visit drafted with the **Dental procedure** template, for a dog or a cat, has a **Dental chart**: the teeth seen from the front, in modified Triadan numbers, with the patient's right on your left. Under it, one line per tooth lists what was found, with its AVDC code ("Complicated crown fracture (T/FX/CCF)"), then a legend of the codes used.

- **Only teeth that were mentioned are charted.** A blank tooth wasn't talked about in the recording. It doesn't mean normal, and the legend says so on screen and on paper. A tooth is "Normal" only if someone said it was, and "Missing" only if someone said it was missing.
- **Rest on a tooth, or on a finding in the list,** to light the transcript lines it came from. Click a finding to play that moment, as for any other line. On a phone, tap it.
- **A red callout under a tooth** means its number isn't anywhere in the recording: the draft may have invented it. Correct it before signing; you can't sign until it's resolved.
- **An amber underline** asks for a second look: the tooth was said as a number and as a name that don't agree ("108, upper left fourth premolar": 108 is the upper right), the line it cites doesn't mention the finding, the details don't fit together, or a finding is listed twice. OpenBark charts the number as said and never picks one for you.
- **To edit, click a tooth.** The chart editor opens with that tooth selected and lists its findings, each with the sentence it came from. Choose **Change** or the remove button, or **Add finding** and pick what was found; only the details that fit that finding are offered (a fracture type, a stage, millimetres, an extraction technique, "Planned, not done yet"). **Ctrl** (or **Cmd**) and click selects several teeth, and **Mark missing** or **Mark normal** marks every selected tooth that has no findings yet. With the keyboard: Tab to the chart, move with the arrow keys, Enter selects a tooth and Space adds it to the selection. **Save changes** saves a new draft; what you add or change is marked "Edited by" with your name, and the earlier draft is kept.
- **Dog or cat?** If no species was entered, the chart can't know which teeth the animal has, so it lists the findings without a diagram. The editor asks **Dog or cat?** before it saves, and refuses teeth that animal doesn't have.
- **Deciduous teeth** (500 to 800) show when one was charted, or with **Deciduous teeth** in the editor.
- **Once signed,** the chart is read-only: clicking a tooth only highlights its lines. Corrections go in an addendum.
- The chart is in **Copy note**, in a heading's **Copy** (as words, one line per tooth), and in the PDF, drawn in greys with the same legend.

Tooth names are read in English. In a consultation in another language, a tooth said only by name ("the upper left canine") shows a red callout until you correct it.

## Visits with more than one animal

When you enter the patient before recording, choose **Add another animal** for each other pet seen in the same visit. After the recording, OpenBark drafts one note per animal. Review shows a tab for each, and you edit and sign each animal's note on its own tab. Each note holds only what was said about that animal, or about all of them together, such as advice for the household. Check each one: if a value appears under the wrong animal, correct it before signing.

## Draft again with another template

Pick a template in the top bar and choose **Draft as …** to draft the same consult another way. Every draft is kept with the visit.

## Sign

When the note is right, choose **Sign note**. Signing locks it: it can't be edited, re-asked or redrafted afterwards. For anything that comes later, such as a phone update, choose **Add addendum**. Addenda are dated, signed with your name, and appear under the note, in copies and in PDFs.

If your clinic has a team set up, only veterinarians can sign and add addenda.

## Delete a visit

A test recording, a visit recorded by mistake, a duplicate import, or a client who withdraws consent: open the visit in Review and choose **Delete visit**, beside **Edit details**. Pick the reason from the short menu. The visit is deleted at once and a message offers **Undo**.

- The visit goes to **Deleted visits** (Settings) for 30 days. Restore it from there any time in those 30 days.
- In a visit with more than one animal, the whole visit is deleted, with every animal's notes. The menu names the other animals first.
- Filed under the wrong patient? That's a correction, not a deletion: use **Edit details** instead.
- **Signed visits can't be deleted yet.** To correct a signed note, add an addendum.
- Veterinarians and technicians can delete visits. Reception can't, and doesn't see Deleted visits. Working alone, you need your name in Settings, as for signing.

The patient stays in your patient list, even with no visits left. Their page says when a deleted visit is waiting in Deleted visits.

## Deleted visits

**Settings → Deleted visits** lists every deleted visit: the patient, the visit date, who deleted it and why, and the day it will be removed for good.

- **Restore** puts the visit back, with its recording, transcript and every draft, and links it to its patients again.
- **Delete now** removes it for good, after you confirm. It can't be undone.
- After 30 days, OpenBark removes a deleted visit for good on its own.

**Settings → Storage** shows how much space Deleted visits take.

## Delete only the recording

After the note is checked and signed, you may not need the recording any more. Choose **Delete recording**, under the player, and confirm: "Delete this recording? The note stays."

- Only the recording goes, at once. The transcript, every draft, signed notes and addenda stay. Evidence links still light up the transcript line; they just can't play it.
- You can't play the visit back or transcribe it again afterwards. Drafting again works, because drafts are written from the transcript.
- On a signed visit, only a veterinarian can delete the recording.
- Review then says "Recording deleted" with the date and who did it.

## What deleting removes, and what it doesn't

OpenBark only deletes its own files. When a visit is deleted for good (Delete now, or after 30 days):

| Where | What happens |
|---|---|
| OpenBark's screens | The visit is gone |
| This computer's disk | OpenBark's files are removed. Encrypted fragments can stay in free disk space until something overwrites them. They can't be read without the key |
| Backups of this computer (File History, shadow copies, Time Machine, your own backups) | Encrypted copies stay until those backups expire. The recovery key can open them |
| A visits folder kept in a cloud-synced folder | An encrypted copy can stay in that service's recycle bin for a few weeks |
| Your practice software, saved PDFs, copied text | Not touched. OpenBark never controlled them |
| The file you imported, if the visit came from a file | Still where it was. Delete it there too if you need to |
| Visits saved before encryption was turned on | Plain-text fragments may stay on the disk |
| The deletion record | Keeps a dated entry of who deleted what and why, with no transcript or note text (see [Team and security](team-and-security.md)) |
