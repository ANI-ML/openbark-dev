# Team and security

Video: **Help → Team and security** in the app.

## Team members and roles

Working alone? You don't need a team. Your name in Settings is used for signing.

To keep track of who did what, add your team in **Settings → You and your team**:

1. The first team member must be a veterinarian. Enter a name and a PIN of 4 to 8 digits, then choose **Add the first veterinarian**.
2. OpenBark now asks **Who's using OpenBark?** Pick your name and enter your PIN.
3. Add the others with their role and PIN.

| Role | Can |
|---|---|
| Veterinarian | Everything: record, draft, ask, edit, sign, add addenda, delete and restore visits, delete recordings, manage the team, change templates (as their own version), connect or disconnect Twilio and change the phone settings, make calls |
| Technician | Record, draft, ask, edit, enter patient details, delete and restore unsigned visits, delete the recording of an unsigned visit, change templates (as their own version), make calls |
| Reception | Record, import, enter patient details, read and copy notes, read templates, make calls |

### Admins and templates

**Admin** is a flag on a team member, whatever their role. The person who sets up the team is the first admin. An admin can make someone else an admin, or remove the flag, with their PIN; there is always at least one admin.

Only an admin changes what the whole team drafts with, and **each change needs an admin's PIN**:

| Action | Working alone | With a team |
|---|---|---|
| Change a template | Yes: it changes your templates | Veterinarians and technicians: saved as their own version |
| Change the hospital's templates, reset or delete one | Yes | An admin, with their PIN |
| Share your version with the hospital | Not needed | An admin approves with their PIN |
| Save a standard for yourself | Not needed | Veterinarians and technicians |
| Save or remove a standard for the hospital | Yes | An admin's PIN |
| Templates: each person's own, or shared by the hospital | Not needed | An admin, with their PIN |

**Settings → You and your team → Templates** chooses whether each person keeps their own versions (the default; the hospital's templates are the defaults) or everyone drafts with the hospital's templates. Switching to the hospital's keeps everyone's versions on disk, and they come back when you switch back. The PIN is sent only inside the request and never written to a log.

Each encounter keeps who recorded it, who drafted, asked for or edited each version, and who signed it and added each addendum, with their role.

To switch person, choose your name at the bottom of the sidebar. Change your own PIN in Settings; a veterinarian can **Reset PIN** for anyone in **Settings → You and your team**. Don't remove and re-add someone to reset their PIN: removing a team member also removes their own template versions and standards (the confirmation says how many). Notes keep the version that drafted them.

This keeps an honest record on a shared computer. It isn't a security boundary against someone with access to the computer's account. Clinic-wide accounts and audit come with the Clinic Hub.

## Deleting visits

How to delete a visit or a recording, and what deleting removes, is in [Reviewing and signing](review.md#delete-a-visit).

| Action | Who can, with a team | Can it be undone? |
|---|---|---|
| Delete an unsigned visit | Veterinarian or technician | Yes: it waits in Deleted visits for 30 days |
| Delete a signed visit | Nobody yet. Correct a signed note with an addendum | |
| Delete the recording of an unsigned visit | Veterinarian or technician | No |
| Delete the recording of a signed visit | Veterinarian | No |
| Restore a deleted visit, or Delete now | Veterinarian or technician | Restore: yes. Delete now: no |

Reception can't delete, restore or see Deleted visits. Working alone, deleting needs your name in Settings. Like signing, that's on your honour: OpenBark can't tell who is at the keyboard.

### Signed notes are part of the medical record

OpenBark isn't your practice software, and the note you copied into it is your legal record. Still, keep signed notes in OpenBark for as long as your licensing body asks you to keep records. Some examples, to show how much they differ:

| Where | How long | Source |
|---|---|---|
| Ontario (CVO) | 5 years after the last entry | CVO Professional Practice Standard: Medical Records |
| British Columbia (CVBC) | 7 years from the last visit | CVBC medical record retention |
| Texas | 3 years after the last visit | 22 TAC 573.52 |
| California | 3 years after the last visit | 16 CCR 2032.3 (from the Veterinary Medical Board's FAQ) |
| United Kingdom (RCVS) | As long as necessary | RCVS Supporting Guidance, chapter 13 |

These are examples, not advice. Check your own licensing body's rule. Every one of them expects corrections to keep the original and to say who changed it and when, which is what addenda do.

### The deletion record

Every deletion, restore and removal for good adds an entry to the deletion record, kept in the visits folder (`.openbark/audit.log`). An entry says what happened, when, who did it with their role, and why. It identifies the visit by its id, its patient records, the recording's fingerprint and, for a signed note, the note's fingerprint, so a copy in your practice software or a saved PDF can later be matched to what was signed. It holds no patient names, no transcript and no note text. Entries are kept for good, even after the visit is gone.

If an entry can't be written, the deletion doesn't happen and OpenBark says so: "OpenBark couldn't record the deletion, so the visit was kept."

Its limits, honestly:
- The record is encrypted, and each entry is linked to the one before it, so a missing, reordered or edited entry shows up. It can't stop someone with the recovery key from rewriting the whole record, or from deleting the file.
- Keep each visits folder in use by one copy of OpenBark at a time: the desktop app or `openbark serve`, not both. Two copies writing at the same moment can't damage visits, but they can each add an entry at once, and work on one visit isn't co-ordinated between them.
- If OpenBark closes unexpectedly in the middle of a deletion, it finishes the job the next time it starts, and marks the entry as recorded afterwards.

## Encryption

Recordings, transcripts and notes are encrypted on the computer. If an encounter folder is copied, backed up or synced elsewhere, it can't be read without the key. **Settings → Privacy and storage** shows where the key is kept:
- **This computer's keychain** (Windows Credential Manager or macOS Keychain). This is the usual case.
- **A file in your user folder**, when the keychain isn't available, such as on a server reached over a remote connection. Visits copied off the computer are still protected, but anyone using your account on that computer can read them. The setting says so.

Visits saved before encryption was available are listed there. Choose **Encrypt them now** to encrypt them.

## The recovery key: keep it safe

Choose **Show the recovery key** once. Print it, or store it in your clinic's password manager. **Without it, encrypted encounters can't be opened on another computer or after reinstalling**, and no one can recover them for you.

## Moving to a new computer

1. Copy the encounters folder (shown in **Settings → Privacy and storage**) to the new computer, in the same place.
2. Install OpenBark, then enter the recovery key, either:
   - with `openbark key import <recovery key>`, or
   - by setting `OPENBARK_DATA_KEY` to the key before starting OpenBark.
3. Open OpenBark: your encounters open as before.

## Also recommended

Turn on the operating system's disk encryption (BitLocker or FileVault) and a screen lock. OpenBark's encryption protects the encounter files; disk encryption protects everything else on the computer.
