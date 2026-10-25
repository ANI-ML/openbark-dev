# Encrypted sync between your computers

**Preview.** Use test data only until the independent security review is finished (M3 phase 3).

Recordings and notes stay on your devices unless you turn on encrypted sync. Even then, only encrypted copies leave, and our servers can't read them.

Encrypted sync is **off** until you turn it on. OpenBark works exactly the same without it, with no account.

## What syncs, and what never does

| Syncs (encrypted on your computer first) | Never leaves the device |
|---|---|
| Transcripts | **Recordings.** The audio stays on the device that recorded it |
| Drafts and signed notes, with their templates | Your models, keys and PINs |
| Signatures and addenda | Your storage folder choices |
| Patient details on the visit, and patient records | |
| Tooth chart confirmations | |

On another computer, a synced visit shows its transcript and notes, and says where the audio is: "Recorded on Front desk PC. The audio is on that device." You can draft, review and sign there. Transcribing again needs the computer that holds the recording.

What the sync server can read: your email address, sign-in times, how many devices you have and their platforms, and, for each encrypted item, its kind, its padded size and when it changed. It can't read transcripts, notes, patient or owner names, species, reasons for visits, device names, signatures or consent records.

## Turning it on (the first computer)

1. **Settings → Encrypted sync → Turn on encrypted sync.**
2. Enter your email address and a name for this computer. Choose how far back this computer's visits upload (the last 30 days is recommended).
3. Enter the 6-digit code from the email. There's no password.
4. OpenBark shows your **Recovery Kit**: 24 words and a QR code. Print it, or keep it in your clinic's password manager. The same sheet shows this computer's own **Computer recovery key**.
5. Type the two words OpenBark asks for. Syncing starts.

The two recovery secrets do different things:

| | Restores | Keep it |
|---|---|---|
| **Recovery Kit** (24 words) | Your synced visits, on any new device | Printed, or in a password manager |
| **Computer recovery key** | One computer's own visit folder | The same way |

Nobody else has a copy of the Recovery Kit, including ANI.ML Health. If every device and the Recovery Kit are lost, the synced copies can't be opened again. The visits already on each computer stay readable there.

## Adding another computer

1. On the new computer: **Settings → Encrypted sync → Turn on encrypted sync**, and sign in with the **same** email.
2. It shows **6 words** (and a QR code a phone can scan). They work for 10 minutes.
3. On a computer that already syncs: **Settings → Encrypted sync → Add a device**, and type the 6 words.
4. OpenBark checks that the words match that device, shows its name, and asks you to confirm. Choose **Add device**.

The new computer then receives your synced visits (transcripts and notes, never recordings).

No other computer at hand? On the new computer choose **Use my Recovery Kit instead** and type the 24 words. They stay on that computer and are never sent anywhere.

Why typing the words matters: signing in only proves who you are. The keys come from a device you already use, or from your Recovery Kit, never from the server. The server can lock a device out, but it can never let one in.

## Changes on two computers

Each computer syncs every 30 seconds while OpenBark is open, and keeps working offline; changes upload when it's back online.

- **Patient details and other visit details:** the later change is kept. If both computers changed the same detail, the other value is listed under **Changed on two devices at once**, so nothing is lost.
- **Drafts:** every draft from every computer is kept.
- **Signing is one-way.** If a note was signed on one computer while another made a new draft, or signed a different draft, the first signature is the record. The other draft is shown beside it in Review with "Signed on [computer] at [time]. Add this as an addendum?"
- **Addenda:** every addendum from every computer is kept.
- **The same animal created on two computers** becomes one patient record.

## Removing a computer

**Settings → Encrypted sync → Devices → Remove.** The computer is locked out at once, and the keys change so it can't open anything synced from then on. The next time it connects, it erases its synced copies. Its own visits and recordings stay on it.

The website account page can remove a device too; your next computer to come online then changes the keys.

## Deleting a visit

Deleting a visit on one computer removes only that computer's copy; other computers keep theirs, and it doesn't come back to the computer it was deleted on. Restoring it from Deleted visits syncs it again. **Delete everywhere** comes in a later release.

## Turning sync off, and deleting the account

- **Turn off sync on this computer** signs this computer out. Every visit stays on it. Turning sync on again with the same email needs no new approval.
- **Delete my sync account** deletes every encrypted copy, device and key on the sync server. The visits on each of your computers stay where they are.

## Shared computers

At first, one account syncs per computer. With a team set up, the team member who turns sync on is linked: only visits they recorded, drafted or signed upload. A PIN is not a cryptographic boundary on a shared Windows account; real separation comes with the Clinic Hub or separate Windows accounts.

## Your clinic's own server

Before signing in, **Use my clinic's own server** takes the address and publishable key of your own Supabase project. Set it up with the files in `supabase/` (see `supabase/README.md`).

## Where the keys are kept

The sync keys are kept in this computer's keychain (Windows Credential Manager, macOS Keychain, Secret Service on Linux), or, where there isn't one, in a protected file in the visits folder encrypted with this computer's own key. They never sync.
