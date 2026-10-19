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

- **The chart is a draft until you confirm it.** Every tooth the draft charted is unconfirmed, and the chart says **Draft chart: check every tooth**. Compare each tooth's findings with the recording (rest on the tooth to light its lines, click a finding to play it), then tick **Checked against the recording** under that tooth. **Sign note** stays off until every drafted tooth is confirmed, and OpenBark refuses to sign it even from elsewhere. A tooth with no flag isn't checked: the check reads words and misses things a person wouldn't, so only your tick confirms a tooth. Untick it to take it back. Entries you edited or added yourself are your own and need no tick. If you change a tooth's entries later, that tooth needs your tick again; the other teeth keep theirs. Drafting again starts a new, unconfirmed chart. Who confirmed each tooth, and when, is kept with the visit and shown under the tooth ("Confirmed by Dr Rivera, 2026-10-19 10:05"). With a team, only a veterinarian can confirm.
- **Only teeth that were mentioned are charted.** A blank tooth wasn't talked about in the recording. It doesn't mean normal, and the legend says so on screen and on paper. A tooth is "Normal" only if someone said it was, and "Missing" only if someone said it was missing.
- **Rest on a tooth, or on a finding in the list,** to light the transcript lines it came from. Click a finding to play that moment, as for any other line. On a phone, tap it.
- **A red callout under a tooth**, and a filled "!" badge on it in the chart, means you can't sign until it's resolved: its number isn't anywhere in the recording (the draft may have invented it), the tooth isn't in this animal's mouth (a 105 in a cat), or no species was entered and only a dog has that tooth, so choose dog or cat first.
- **An amber underline**, and a plain "!" on the tooth, asks for a second look. The tooth was said as a number and as a name that don't agree ("108, upper left fourth premolar", "108, I mean the upper left fourth premolar", or "Next is 108." then "The upper left fourth premolar…": 108 is the upper right); the words for the finding, its grade or its millimetres were said about another tooth in the same line ("309 has grade one mobility and 409 is grade three", "209 looks fine next to the fractured 208"); a grade or measure was said with another finding ("stage three periodontal disease, mobility grade one", "stage two resorption, type one") or wasn't said as a value at all ("loose for three weeks"); the line says the finding wasn't there ("no resorption", "mobility not detected", "neither 108 nor 208 is fractured", "mobility: none", "fracture-free", "Is 108 fractured? No"); it was said as a plan ("should be extracted", "a candidate for crown amputation", "at the next dental") or declined ("owner declined extraction") but charted as done, or done but charted as a plan; it was done at an earlier visit ("extracted at her last dental"); it was said only as possible, to check, or as someone's belief ("possible resorption", "to rule out a root fracture", "if 307 has resorption", "owner thinks"); the line pairs teeth and findings by order ("respectively", "the first … the second"); another line says something else about the tooth ("Actually 108 is fine"); in a visit with several animals, the line is about another animal; the line also has the words for another type ("surgically with a flap" on a closed extraction); two findings on the tooth can't both be right (normal and a fracture, two resorption stages, an extraction and a crown amputation); the details don't fit together or don't fit that kind of tooth; or a finding is listed twice. OpenBark never picks one for you. The summary above the note counts these with the other items to check.
- **A group the recording doesn't make clear names no tooth.** "All the lower incisors" is six teeth; "a pair of lower incisors", "a handful of the premolars", "the majority of", "all but one", "mostly" and "two of them" don't say which teeth, so a tooth charted from them has a red callout until you correct it. Teeth named after "except", "other than" or "apart from" are left out of the group ("all the lower incisors are missing except 401").
- **To edit, click a tooth.** The chart editor opens with that tooth selected and lists its findings, each with the sentence it came from. Choose **Change** or the remove button, or **Add finding** and pick what was found; only the details that fit that finding are offered (a fracture type, a stage, millimetres, an extraction technique, "Planned, not done yet"). **Ctrl** (or **Cmd**) and click selects several teeth (on a touch screen, turn on **Select several** and tap them), and **Mark missing** or **Mark normal** marks every selected tooth that has no findings yet. If a finding can't be added (a probing depth with no millimetres, a persistent deciduous tooth on a permanent number), the reason shows beside the button. With the keyboard: Tab to the chart, move with the arrow keys, Enter selects a tooth and Space adds it to the selection. **Save changes** saves a new draft; what you add or change is marked "Edited by" with your name, and the earlier draft is kept.
- **Dog or cat?** If no species was entered, the chart can't know which teeth the animal has, so it lists the findings without a diagram, and a tooth only a dog has blocks signing until you choose. The editor asks **Dog or cat?** before it saves, and refuses teeth that animal doesn't have. If the species was entered after drafting, the chart offers it ("The patient is recorded as a cat: use a cat's chart"). Draft again and the choice is kept. In a visit with several animals, each one's chart uses that animal's species from its patient record.
- **Deciduous teeth** (500 to 800) show when one was charted, or with **Deciduous teeth** in the editor.
- **Once signed,** the chart is read-only: clicking a tooth only highlights its lines, and each tooth still shows who confirmed it. Confirmations can't be changed either. Corrections go in an addendum.
- The chart is in **Copy note**, in a heading's **Copy** (as words, one line per tooth), and in the PDF, drawn in greys with the same legend.

Tooth names are read in English. In a consultation in another language, a tooth said only by name ("the upper left canine") shows a red callout until you correct it.

## Visits with more than one animal

When you enter the patient before recording, choose **Add another animal** for each other pet seen in the same visit. After the recording, OpenBark drafts one note per animal. Review shows a tab for each, and you edit and sign each animal's note on its own tab. Each note holds only what was said about that animal, or about all of them together, such as advice for the household. Check each one: if a value appears under the wrong animal, correct it before signing.

## Draft again with another template

Pick a template in the top bar and choose **Draft as …** to draft the same consult another way. Every draft is kept with the visit.

## Sign

When the note is right, choose **Sign note**. A dental chart must have every drafted tooth confirmed first (see above). Signing locks it: it can't be edited, re-asked or redrafted afterwards. For anything that comes later, such as a phone update, choose **Add addendum**. Addenda are dated, signed with your name, and appear under the note, in copies and in PDFs.

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
