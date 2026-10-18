# Team and security

Video: **Help → Team and security** in the app.

## Team members and roles

Working alone? You don't need a team. Your name in Settings is used for signing.

To keep track of who did what, add your team in **Settings → Team**:

1. The first team member must be a veterinarian. Enter a name and a PIN of 4 to 8 digits, then choose **Add the first veterinarian**.
2. OpenBark now asks **Who's using OpenBark?** Pick your name and enter your PIN.
3. Add the others with their role and PIN.

| Role | Can |
|---|---|
| Veterinarian | Everything: record, draft, ask, edit, sign, add addenda, manage the team |
| Technician | Record, draft, ask, edit, enter patient details |
| Reception | Record, import, enter patient details, read and copy notes |

Each encounter keeps who recorded it, who drafted, asked for or edited each version, and who signed it and added each addendum, with their role.

To switch person, choose your name at the bottom of the sidebar. Change your own PIN in Settings; a veterinarian can reset anyone's PIN by removing and re-adding them.

This keeps an honest record on a shared computer. It isn't a security boundary against someone with access to the computer's account. Clinic-wide accounts and audit come with the Clinic Hub.

## Encryption

Recordings, transcripts and notes are encrypted on the computer. If an encounter folder is copied, backed up or synced elsewhere, it can't be read without the key. **Settings → Encryption** shows where the key is kept:
- **This computer's keychain** (Windows Credential Manager or macOS Keychain). This is the usual case.
- **A file in your user folder**, when the keychain isn't available, such as on a server reached over a remote connection. Visits copied off the computer are still protected, but anyone using your account on that computer can read them. The setting says so.

Visits saved before encryption was available are listed there. Choose **Encrypt them now** to encrypt them.

## The recovery key: keep it safe

Choose **Show the recovery key** once. Print it, or store it in your clinic's password manager. **Without it, encrypted encounters can't be opened on another computer or after reinstalling**, and no one can recover them for you.

## Moving to a new computer

1. Copy the encounters folder (shown in **Settings → Storage**) to the new computer, in the same place.
2. Install OpenBark, then enter the recovery key, either:
   - with `openbark key import <recovery key>`, or
   - by setting `OPENBARK_DATA_KEY` to the key before starting OpenBark.
3. Open OpenBark: your encounters open as before.

## Also recommended

Turn on the operating system's disk encryption (BitLocker or FileVault) and a screen lock. OpenBark's encryption protects the encounter files; disk encryption protects everything else on the computer.
