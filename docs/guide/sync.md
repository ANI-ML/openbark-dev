# Encrypted sync through your clinic hub

**Preview.** Use test data only until the independent security review is finished.

Encrypted sync carries transcripts, notes and patient details between your iPhone and your computers, through one computer at your clinic: the **clinic hub** (ADR-0029). Nothing goes to ANI.ML Health: there are no OpenBark accounts or emails, and we run no servers for sync. **Recordings never sync.** The audio stays on the device that recorded it.

Encrypted sync is **off** until you turn it on. OpenBark works exactly the same without it.

In the app: **Help → Set up encrypted sync** has a short video of the steps below.

## What syncs, and what never does

| Syncs (encrypted on your device first) | Never leaves the device |
|---|---|
| Transcripts | **Recordings.** The audio stays on the device that recorded it |
| Drafts and signed notes, with their templates | Your models, keys and PINs |
| Signatures and addenda | Your storage folder choices |
| Patient details on the visit, and patient records | |
| Tooth chart confirmations | |

On another device, a synced visit shows its transcript and notes, and says where the audio is: "Recorded on Dr Rivera's iPhone. The audio is on that device." You can draft, review and sign there. Transcribing again needs the device that holds the recording.

The hub keeps encrypted copies it can't read, except for the accounts of people who use the hub computer themselves.

## 1. Make one computer the clinic hub

Use a staff-only computer with full-disk encryption, and leave it on: devices sync only while it's on and they're on the clinic network.

1. **Settings → Clinic hub**, and turn on **Make this computer the clinic hub**. Nothing listens on the network until you do. With a team set up, a veterinarian who manages it does this.
2. The screen shows **Listening on port …** and this computer's address.
3. Choose **New join code**. The hub shows a QR code, the **join code**, its **address** and a **certificate check**. A code works once, for 30 minutes; making a new code stops the old one.

Only private network addresses may connect (**Hub settings → Allowed addresses**); within them, the join code is the only thing that stops a device from joining, so show it only to your own staff.

## 2. Join from your first device

On the iPhone or another computer, on the **same clinic network** (the clinic's Wi-Fi):

1. **Settings → Encrypted sync → Join your clinic hub.**
2. Type the **join code**. (Or paste the hub's link, from its QR code.)
3. Type the hub's **address**, or choose **Find on this network** and pick the hub. A hub found on the network is only a hint: the join code and the certificate check decide which hub the device trusts.
4. Type the **certificate check** shown beside the code.
5. Choose **This is my first device**, give the device a name (only your devices can read it), choose how far back its visits upload (the last 30 days is recommended), then **Join the hub**.
6. OpenBark shows your **Recovery Kit**: 24 words and a QR code. Print it, or keep it in your clinic's password manager. On a computer, the same sheet shows its own **Computer recovery key**.
7. Choose **I've kept it safe**, type the two words OpenBark asks for, then **Check and start syncing**.

| | Restores | Keep it |
|---|---|---|
| **Recovery Kit** (24 words) | Your synced visits, on any new device | Printed, or in a password manager |
| **Computer recovery key** | One computer's own visit folder | The same way |

Nobody else has a copy of the Recovery Kit, including ANI.ML Health. If every device and the Recovery Kit are lost, the synced copies can't be opened again. The visits already on each device stay readable there.

## 3. Add your next device

1. On the hub, choose **New join code**.
2. On the new device: **Settings → Encrypted sync → Join your clinic hub**, type the code, address and check, and choose **I already use OpenBark on another device**. It shows **6 words** (and a QR code). They work for 10 minutes.
3. On a device that already syncs: **Settings → Encrypted sync → Add a device**, type the 6 words, check the name and platform, and choose **Add device**.
4. It shows a code. Type it on the new device and choose **Confirm and start syncing**. Until then the new device syncs nothing.

No other device at hand? On the new device choose **Use my Recovery Kit instead** and type the 24 words. They stay on that device and are never sent anywhere.

**The hub computer itself** can sync like any other device: **Settings → Clinic hub → Use this computer with its hub**.

## Changes on two devices

Each device syncs every 30 seconds while OpenBark is open and it can reach the hub, and keeps working offline; changes upload when it's back.

- **Patient details and other visit details:** the later change is kept. If both devices changed the same detail, the other value is listed under **Changed on two devices at once**, so nothing is lost.
- **Drafts:** every draft from every device is kept.
- **Signing is one-way.** If a note was signed on one device while another made a new draft, or signed a different draft, the first signature is the record. The other draft is shown beside it in Review with "Signed on [device] at [time]. Add this as an addendum?"
- **Addenda:** every addendum from every device is kept.
- **The same animal created on two devices** becomes one patient record.

## Removing a device

**Settings → Encrypted sync → Devices → Remove**, or on the hub, **Settings → Clinic hub → Connected devices → Remove**. The device is locked out at once, and the keys change so it can't open anything synced from then on. The next time it connects, it erases its synced copies. Its own visits and recordings stay on it.

## Deleting a visit

Deleting a visit on one device removes only that device's copy; other devices keep theirs, and it doesn't come back to the device it was deleted on. Restoring it from Deleted visits syncs it again.

## Turning sync off, and deleting the account

- **Turn off sync on this computer** (or **this iPhone**) stops syncing there. Every visit stays on it.
- **Delete my sync account** deletes every encrypted copy, device and key on the hub. The visits on each of your devices stay where they are.

## If a device can't find or reach the hub

- Both must be on the clinic network, and the hub computer must be on with OpenBark open.
- **Find on this network** finds nothing on some networks (guest Wi-Fi, networks that block device discovery). Type the address shown on the hub instead.
- A computer's firewall may ask to allow OpenBark the first time hub mode is on: allow it on private networks.
- Remote access (for example Tailscale's 100.64.0.0/10) is off by default; add it under **Hub settings → Allowed addresses** only if the clinic uses it.

## Where the keys are kept

The sync keys are kept in the computer's keychain (Windows Credential Manager, macOS Keychain, Secret Service on Linux), or, where there isn't one, in a protected file in the visits folder encrypted with this computer's own key. On the iPhone they're in its protected storage. They never sync.
