# ADR-0017: Accounts and end-to-end encrypted sync

- Status: accepted (owner decisions 2026-10-18)
- Date: 2026-10-18
- Deciders: product owner (asked on 2026-10-18 that clinicians can sign in on several devices, record on the phone and finish the work on any computer; chose Supabase for the website account portal; made the decisions below on 2026-10-18), engineering
- Relations:
  - Amends ADR-0005. It answers ADR-0005's open questions and replaces its CRDT plan. It keeps ADR-0005's "Audio never syncs".
  - Extends ADR-0008. Vault calls take an explicit key, and there is a keychain store for iOS.
  - Covers the website account portal that `docs/versions.md` lists under `@supabase/supabase-js`.

## Owner decisions (2026-10-18)

Decided by the owner:

| # | Decision |
|---|---|
| 1 | iPhone builds, signing and TestFlight uploads run on GitHub Actions macOS runners. Billing is fixed and Actions works. No owner Mac is in the loop. The S1 audio check also runs as a CI job plus a TestFlight build the owner installs on their iPhone. The owner has a paid Apple Developer account (christopher.pinard@animl.health); whether it is an individual or organisation membership is still to confirm. The signing secrets (distribution certificate `.p12` and its password, and an App Store Connect API key) go into GitHub secrets, added by the owner. See ADR-0018 section 8 |
| 2 | **Sync covers transcripts and notes only.** Audio never leaves the device that recorded it. This keeps ADR-0005's "Audio never syncs"; this ADR does not amend it. On a computer, the evidence link shows the transcript line and its time, but the original audio plays only on the device that recorded it. Record-only phones can't be supported by sync (section 7) |
| 3 | The phone records and transcribes on the device (Parakeet through sherpa-onnx). Drafting stays on computers |
| 4 | Sign-in emails go through SendGrid as Supabase's custom SMTP. ANI.ML already uses SendGrid for ARC. The owner enters the SMTP credentials in the Supabase dashboard, so the key never enters the repo |
| 5 | The Supabase project exists, created by the owner: project ref `nmecovtmijnfnjuadicp`, URL `https://nmecovtmijnfnjuadicp.supabase.co`. Its publishable key is public and was provided. Region: to check. Canada (`ca-central-1`) was recommended. Free plan now, Pro before the pilot |

Accepted by default, unless the owner objects:

| Item | Default |
|---|---|
| Bundle id | `health.animl.openbark` |
| Sign-in | Email codes. No passwords, no social login |
| Second factor | TOTP |
| Recovery | Recovery Kit only. No passphrase envelope and no escrow in M3 |
| Signing on the phone | Not in the pilot |
| Delete everywhere | Veterinarians only |
| Security review | External review before any real clinic data syncs |
| Public wording | "Recordings and notes stay on your devices unless you turn on encrypted sync. Even then, only encrypted copies leave, and our servers can't read them." |
| Milestones | M3 is this work, M4 the Clinic Hub, M5 the browser extension |

Still to do by the owner: confirm the membership type, add the signing secrets to GitHub, create the app record for `health.animl.openbark` in App Store Connect, set up SendGrid SMTP in the Supabase dashboard, and check the project's region.

## Context

The product owner asked: "ensure users can login to multiple devices (phone or computer) and it will save their audio records or transcripts (i.e. record on phone but once processed on device, can see it anywhere they are logged in)." The aim is to record on the phone in the exam room and finish the note later, on a clinic PC or a laptop at home.

Today:
- Every visit is a folder encrypted under one data key per install (ADR-0008). The key is in the OS keychain, and the clinic keeps a recovery key.
- Team members have roles and PINs on one computer (M1 item 11). There are no accounts.
- The only network calls are model downloads. `docs/network.md` says "a host that isn't listed here is a bug", and CI checks that only `os-models` has an HTTP client.

ADR-0005 set the direction: opt-in, end-to-end encrypted, per user and then per clinic, self-hostable. It left four questions open: identity, key recovery, hosting, and the regulatory review. It also said "Audio never syncs". The new request mentions audio, but on 2026-10-18 the owner decided that sync carries transcripts and notes only, so that rule stands.

What the repo lacks for sync (checked 2026-10-18):
- There is no change log, no tombstones and no CRDT. Only the UUIDv7 session ids exist.
- `session.json` is rewritten in place when a note is signed or an addendum is added.
- Runs are named `runs/NNN`. The names collide when two devices draft offline.
- `patients.json` is a single file.
- The data key belongs to one install, so a second device has no way to get it.
- PINs are hashed with salted SHA-256. That is acceptable inside the encrypted local file, but too weak once anything remote depends on a PIN.

Supabase facts that shape the design (checked live on 2026-10-18):
- **Email codes.** Email one-time codes and magic links are one feature: putting `{{ .Token }}` in the email template sends a 6-digit code. A code needs no deep links in the desktop app, the iPhone app or web mode.
- **Email sending.** Production needs custom SMTP. The built-in sender allows 2 messages an hour, and only to members of the Supabase team.
- **Plans.** The Free plan has 1 GB of storage and a 50 MB file limit, no backups, and pauses after a week without activity. Pro costs from 25 USD a month and includes 100 GB of storage, 250 GB of egress and 7 daily backups.
- **Uploads.** Storage recommends standard uploads up to 6 MB and TUS above that. Without audio, M3 doesn't use Storage (section 11).
- **Access control.** Tables are protected by row-level security (RLS).
- **Region.** Canada (Central), `ca-central-1`, is available. A project's region can't be changed after it is created.
- **Agreements.** The data processing agreement (DPA) is part of the terms of service. A HIPAA business associate agreement (BAA) needs the Team plan, and HIPAA covers human records, not veterinary ones.

Three designs were compared: encrypted copies stored on Supabase, a clinic hub with a relay, and a minimal relay from phone to PC. See Alternatives.

## Decision

### 1. Principles

1. **Off by default.** Everything that works today keeps working with no account. Sync is a separate, clearly labelled setting.
2. **The server holds ciphertext only.** Supabase stores encrypted objects, wrapped keys and the metadata listed in section 8. It never holds a key that opens clinical data.
3. **The cloud can lock a device out but can never let one in.** Signing in proves who you are. The keys come only from a device you already use, or from your Recovery Kit.
4. **The cloud is a convenience copy.** The local copies on clinic computers stay the clinical record. Retention duties never depend on the cloud, and losing the cloud copy never loses the record.
5. **All network traffic goes through Rust.**
   - A new crate, `engine/os-sync`, calls Supabase over plain REST with the existing `reqwest`.
   - The WebView never talks to the network, and its CSP doesn't change.
   - In web mode, the `openbark serve` process does the syncing. The browser never sees a key.
   - No Supabase SDK ships in the app. `@supabase/supabase-js` stays on the website.
6. **Self-hostable.** SQL migrations and Edge Functions live in the repo under `supabase/`, so a clinic can point OpenBark at its own Supabase.
7. **One format for cloud and LAN.** The Clinic Hub (M4) will use the same object and key format over the LAN (ADR-0005 point 5).

### 2. Accounts

| Item | Decision |
|---|---|
| Service | Supabase Auth, in the same project as the website portal: `nmecovtmijnfnjuadicp`. Region to check; `ca-central-1` was recommended |
| Sign-in | Email and a 6-digit code. No passwords |
| Second factor | TOTP, offered to everyone. It becomes required for clinic spaces when they arrive |
| Social logins | None, so App Store guideline 4.8 (offer an equivalent to Sign in with Apple) doesn't apply. If the portal ever adds Google sign-in, the iPhone app must add Sign in with Apple |
| Sessions | Access tokens last 10 minutes. Refresh tokens rotate and are kept in the OS keychain. On iOS the setting is "after first unlock, this device only" |
| Email | Custom SMTP from day one: SendGrid, which ANI.ML already uses for ARC. The owner enters the credentials in the Supabase dashboard. The key is never in the repo |
| Account deletion | In the app and on the portal (guideline 5.1.1(v)) |
| Portal | Shows the account, its devices and sync usage. It can remove a device and delete the account. It holds no keys, can't approve a device and shows no clinical data |

### 3. Devices

**Turning on sync on the first device**
1. Go to Settings > Sync > **Turn on encrypted sync**, then sign in.
2. The device creates the Account Master Key (section 4) and registers itself.
3. It shows the **Recovery Kit**: 24 words and a QR code on a printable page.
4. The user types back two of the words before sync starts.

**Adding a device**
1. The new device signs in, generates its own keypairs and registers as *pending*. Its device row is tied to its Supabase session id.
2. It shows a QR code with three things: its device id, a fingerprint of its public keys, and a one-time 128-bit secret. Usually a computer shows the code and the phone scans it.
3. The approving device checks the fingerprint. It wraps the Account Master Key to the new device's public key, with the secret bound in, and adds the new device to the roster.
4. Two fallbacks:
   - Neither device has a camera: both screens show a 6-word safety code, derived from both public keys, for the user to compare.
   - No other device is at hand: the Recovery Kit approves the new device.
5. A pending request expires after 10 minutes.
6. Once approved, the new device pulls the last 90 days of transcripts and notes, roughly 200 KB per visit once padded (section 5 and Consequences). Audio never comes, because it never leaves the device that recorded it.

A pending device can read only its own row and the envelopes addressed to it. The server alone can't approve anything, because it never holds the Account Master Key.

**Removing a device**

This works from any approved device, or from the portal.
1. The `revoke-device` Edge Function marks the device revoked and deletes its auth session, so its refresh token stops working.
2. Every RLS policy calls `sync.device_ok()`, which checks that the request's `session_id` claim belongs to an approved device. A revoked device is refused at once, not when its token expires.
3. The next approved device to come online rotates the keys:
   - It creates a new Account Master Key and a new space key version.
   - It re-wraps the object keys, about 72 bytes each. No object is re-encrypted.
   - It wraps the new Account Master Key for the remaining devices and for the Recovery Kit's public key.
4. If the removed device comes online again, it is told it was revoked:
   - It erases its synced copies and its sync keys.
   - It keeps its own visits, including its recordings (which never left it) and any transcripts and notes that never uploaded, encrypted, so the clinic can recover them.
   - This is best effort, and the screen says so.

**Device lease.** A device that hasn't reached the sync service for 30 days locks the copies it received from other devices until it signs in again. The period can be set from 7 to 90 days. The device's own recordings stay usable. The lease limits what a lost phone can expose.

**Shared clinic computers**
- At first, one account is linked per computer.
- Later in M3, each local team member (`users.rs`) can link their own account, and the PIN picks whose sync keys are used.
- A 4 to 8 digit PIN is not a cryptographic boundary on a shared OS account. The guide says so. Real separation comes with the Clinic Hub (M4), or with separate OS accounts.
- Before a PIN chooses anything synced, PIN hashing moves to Argon2id with a lockout: 5 tries, then 15 minutes.
- PIN hashes never sync.

**Which visits a computer syncs**
- **Single-user mode (no team set up):** every visit from the day sync is turned on. The first time, the user chooses how far back to upload. The recommendation is the last 30 days.
- **With a team:** only visits recorded, drafted or signed by the team member who linked the account. Other people's visits never enter one person's space.

### 4. Keys

All keys are 256-bit and random (from `getrandom`) unless stated.

| Key | Where it lives | What it protects | Notes |
|---|---|---|---|
| Local data key (ADR-0008) | Each device's keychain | That device's visit folders, at rest | Unchanged. It never syncs. The existing recovery key, now called the **Computer recovery key**, still restores it |
| Device keys: X25519 and Ed25519 | The device's keychain. Never exported | X25519 receives wrapped keys. Ed25519 signs object headers and the roster | On iOS the keychain item is "after first unlock, this device only". Recording then works while the phone is locked, and nothing reaches iCloud |
| Account Master Key (AMK) | The server keeps only wrapped copies: one per approved device and one for the Recovery Kit | The space keys | Each copy is a sealed box: an ephemeral X25519 key, then HKDF-SHA256, then XChaCha20-Poly1305. The device id and key version are bound in as associated data |
| Recovery Kit | Printed, or kept in the clinic's password manager. The server keeps only its public key | It derives an X25519 keypair that can open the AMK | Any device can wrap a rotated AMK to the kit's public key without knowing the secret. The printed kit keeps working after a device is removed |
| Space key, versioned | Wrapped under the AMK for a personal space. Later, wrapped to each member's account key for a clinic space | The object keys | M3 has one personal space per account |
| Object key (DEK) | Wrapped under a space key version, inside each object's header | One sync object or one blob | Rotation re-wraps DEKs only. Deleting the wrapped DEKs crypto-shreds any copies left in backups |

There is **no passphrase envelope**. Anyone who breached the server could attack a passphrase-wrapped AMK offline.

The interface keeps the two recovery secrets apart by name:
- the **Computer recovery key** restores one computer's folder;
- the **Recovery Kit** restores your synced data.

Both print on one sheet.

If every device and the Recovery Kit are lost, the cloud copy can't be recovered. That is by design. Visits already on a computer stay readable with that computer's key.

### 5. Object format

**OBS1, for small objects** (visit headers, events, patient records, settings)
- Layout: magic bytes, a version, a header, a 24-byte nonce, then the XChaCha20-Poly1305 ciphertext.
- The header holds:
  - the object id (UUIDv7), space id, kind and version;
  - the space key version and the wrapped DEK;
  - the device id and a per-device counter.
- The header is the associated data, so the server can't move ciphertext between ids, kinds or spaces.
- The device signs the header with its Ed25519 key. Clients accept signatures only from keys in the **roster**, a device list encrypted and authenticated under the AMK.
- A counter that goes backwards, or a gap that never fills, raises a warning. That catches a server replaying old versions or holding objects back.
- Bodies are padded to 1, 4 or 8 KiB and stored inline in Postgres.

**Blobs** (transcripts and runs; never audio)
- They use the OBA1 layout from `vault.rs`: 64 KiB chunks, each with its own nonce, with the blob id and chunk index bound in.
- Without audio, blobs are small: a transcript is tens of KB. A blob too large for one REST request is split into parts. The part size is set in phase 2 after checking Supabase's request limit live.
- An encrypted manifest lists the parts and their SHA-256 hashes, so missing, reordered or replayed parts are detected.
- Blobs are padded to a whole number of 64 KiB chunks. The earlier draft padded to 512 KiB for audio parts. That would waste database space now that blobs live in Postgres.
- Blobs are stored as ciphertext in Postgres, in the `blobs` table, through REST.

**Plaintext guard.** `os-sync` refuses to send any body that doesn't parse as a valid OBS1 or OBA1 object. A unit test and an end-to-end test enforce this (section 12).

Object paths and names are random ids. Nothing on the server uses the visit folder name, because that name contains the patient's name.

The OBS1 parser is fuzzed before release.

### 6. What syncs

| Data | Syncs? | How | Notes |
|---|---|---|---|
| Visit header: patient link, reason, visit type, consent, who recorded it, chosen template | Yes | One OBS1 object. Fields merge by last writer wins | Built from `session.json` |
| Signatures, addenda, deletions | Yes | Immutable event objects | Signing never edits an earlier object |
| Transcript | Yes | Blob, versioned | Re-transcribing adds a version and keeps the old one |
| Runs (`note.json`, `note.md`, `template.yaml`) | Yes | One immutable blob per run | Runs get a UUIDv7 `run_id`. Folder names gain a device tag (`runs/004_a3f9`) so offline drafts never collide |
| Patient records | Yes, for patients in your synced visits and patients you create | One OBS1 object per record, mapped from `patients.json` | The file on disk stays as it is |
| Audio | **Never** | | It stays on the device that recorded it (ADR-0005). See section 7 |
| User templates and standards | Yes, later in M3 | The whole template. Last writer wins, old versions kept | Built-in templates don't sync |
| Settings | A few: signing name, note language, letterhead | OBS1 | |
| Models, `locations.json`, keys, the GPU pack, PINs, refresh tokens | Never | | |
| The clinic's Twilio and SendGrid credentials and phone settings (`connections.json`, `connections.bin`, keychain entries; ADR-0023), and call recordings | Never | | Each computer connects with its own Twilio key. Call recordings are audio, which never syncs |

The receiving device writes what it merges as ordinary visit folders, through `os_engine::vault` and under its own data key. `Session::adopt` keeps the session id from the other device. Every existing screen keeps working on synced visits.

### 7. Audio stays on the device that recorded it

Owner decision, 2026-10-18: sync carries transcripts and notes only. Audio never leaves the device that recorded it, as ADR-0005 says.

What this means:
- **Playback.** On a computer, a synced visit's evidence link shows the transcript line and its time. The original audio plays only on the device that recorded it. The visit screen says so in plain words, for example "Recorded on Anna's iPhone. The audio is on that phone."
- **Reprocessing.** Re-transcribing or re-diarising a visit is possible only on the device that holds the recording. A computer can still draft new runs from the synced transcript.
- **Record-only phones.** Sync can't support them, because nothing a computer could process would reach it. A phone without the speech models yet can still record. Its visits stay on the phone, marked "Not transcribed yet", until the models download and the phone transcribes them. Only then do the transcript and notes sync.
- **The settings screen** says plainly that sync carries transcripts and notes, and that recordings stay on the device.

Not in M3, and each would need a new decision:
- uploading audio, in any form, including a "Sync recordings" option;
- TUS or other resumable large uploads;
- cloud retention and deletion settings for audio, and phone deletion after upload;
- a compressed audio format for sync (AAC or FLAC) and the eval that would choose it;
- cloud storage quotas for audio;
- a record-only phone handing its audio to a computer. If that is ever wanted, it would go over the clinic's local network (the Clinic Hub's job), not through the cloud.

### 8. What the server can and cannot read

| The server can read | The server cannot read |
|---|---|
| Account email, sign-in times, IP addresses and user agents (Supabase logs) | Transcripts and notes. Audio never reaches it |
| How many devices there are, their platforms, public keys and last-seen times | Patient and owner names, species, the reason for the visit |
| Space membership (later: which accounts share a clinic space) | Which patient a visit belongs to |
| For each object: its kind, padded size, and creation and update times | Templates, team names, device names, signatures and addenda |
| From those: roughly how many visits a day, working hours, and a loose hint of consult length from padded transcript sizes | Consent records |

A breached or malicious server could still try three things:
- **Withhold, delay or delete data, or replay old versions.** Clients detect gaps and rollbacks through the signed counters and the manifests, and the local copies remain.
- **Register a fake device.** It would get no key, because approval happens on a device the user already has.
- **Alter the website portal.** The portal holds no keys, and the Recovery Kit is never typed there.

Public wording never says the server stores "no data". It describes exactly what this section lists.

### 9. Merging and conflicts

Merging happens on the device. It is deterministic and never destroys local data.

- **Order.** Each change carries a hybrid logical clock value plus the device id, so changes order correctly even when a phone's clock is wrong.
  - The server's `seq` is only the pull cursor.
  - The `push` function takes a lock for each space, so `seq` order is commit order and a late commit is never skipped.
- **Visit header and patient fields.** Last writer wins, field by field. Both values go into a local conflict log that Review can show.
- **Runs.** Both devices' runs are kept. They show newest first, with who made each one and on which device.
- **Signing is one-way.** A signed run never changes.
  - If two devices sign different runs offline, the earlier signature is the record. The other run shows beside it and is offered as an addendum.
  - A draft made offline from a run that was signed elsewhere is kept as a separate draft, with the message "Signed on [device] at [time]. Add this as an addendum?" Nothing is dropped silently.
- **Addenda.** Every addendum from every device is kept.
- **Duplicate patients.** Patients created offline with the same `match_key` merge into the record with the lowest UUIDv7. The other id is kept as an alias, so links to it still work.
- **No processing jobs in M3.** Every visit is transcribed on the device that recorded it, because audio never syncs (section 7). Drafting a synced visit on a computer creates a new run, which merges like any other.

There is no CRDT. Runs are immutable and edits already create new runs (M1 item 6), so two devices never edit the same text. This replaces ADR-0005's plan to use Automerge or Yjs.

### 10. Deletion

| Action | Effect |
|---|---|
| Delete from this device | Removes this device's synced copies only |
| Stop syncing this visit | Writes a tombstone and removes the cloud copy within 24 hours. Every local copy stays |
| Delete everywhere | Arrives with ADR-0011's careful delete flow: veterinarians only, a reason is required, and the audit log records it. Every device removes its copy on its next pull. Deleting a signed visit needs a second confirmation |
| Delete my account | Removes every row, stored object and key envelope. Local data is untouched. Without the envelopes, any ciphertext left in Supabase's 7 daily backups is unreadable |

A `purge` function, run by `pg_cron`, removes tombstoned objects and blobs. A local, append-only **audit log** records device approvals, removals, key rotations and deletions.

### 11. Supabase layout

- **Projects.**
  - The owner created the project `nmecovtmijnfnjuadicp` on 2026-10-18. It is on the Free plan and is used for development now.
  - Its region is to check. `ca-central-1` was recommended for clinical data, and a region can't be changed later.
  - It moves to the Pro plan before the pilot. If its region isn't `ca-central-1`, the pilot uses a new project in `ca-central-1` instead, and this one stays for development.
- **Tables.** Schema `sync`, with RLS on every table: `devices`, `key_envelopes`, `spaces`, `space_members`, `space_keys`, `objects` and `blobs`. RLS on `blobs` checks space membership and calls `device_ok()`.
- **Storage.** Not used in M3. Without audio, every object and blob fits in Postgres rows, so there is no bucket, no signed upload URL and no TUS client.
- **Postgres functions.**
  - `push` takes the space lock, assigns `seq`, and checks `base_version` for objects that can change. On a conflict it returns the current row.
  - `pull` returns rows above a cursor.
- **Edge Functions.** `revoke-device`, `delete-account` and `purge`. None of them sees plaintext.
- **Updates.** Until Realtime is added later in M3, desktops poll every 30 s while open. Realtime then sends row-change notices for `objects`, and those notices carry only ciphertext rows.
- **Uploads.** `os-sync` sends everything through REST, from Rust, on every platform. On iOS, transcripts and notes are small enough to upload in the time left after Stop, or when the app next opens. No background `URLSession` is needed.
- **Email.** Custom SMTP through SendGrid, set up by the owner in the Supabase dashboard. The email template sends a 6-digit code (`{{ .Token }}`).

### 12. Changes to docs/network.md

The source-check rule becomes: "`os-models` and `os-sync` are the only engine crates with an HTTP client."

New run-time rows:

| # | When | Host(s) | What's sent | Purpose | Off switch |
|---|---|---|---|---|---|
| R6 | Only after the user signs in under Settings > Sync | `<project>.supabase.co/auth/v1` (`ca-central-1`) | Email address, the 6-digit code, TOTP codes, the refresh token | Sign-in | Don't sign in, or Sign out |
| R7 | Only while signed in with sync on | `<project>.supabase.co/rest/v1` and `/functions/v1` | Device public keys, wrapped keys, OBS1 ciphertext, OBA1 ciphertext parts of transcripts and notes, padded sizes, cursors. Never audio | Sync transcripts, notes, records and keys | Turn off sync |
| R8 | As R7, later in M3 | `wss://<project>.supabase.co/realtime/v1` | The access token. It receives row-change notices that carry only ciphertext rows | Visits appear within seconds | Turn off sync |

The earlier draft had a Storage row for recordings. It went with audio sync. Until R6 and R7 are built, the planned row in `docs/network.md` describes them: Supabase Auth and REST only, no audio. R8 is added when Realtime is built.

Tests:
- The offline end-to-end test still passes with sync off.
- A new end-to-end test runs against a local Supabase (`supabase start` in Docker) and records every request body. It fails if any body is not OBS1 or OBA1 ciphertext, if any body contains a fixture patient name, or if any request is made to Storage. A test also checks that the outbox refuses audio files. It never runs on real data.
- RLS tests check that one account can't list, read or write another account's rows or objects, and that a revoked device is refused.

### 13. Engine changes

- **New crate `engine/os-sync`:**
  - `keys.rs`: the keys in section 4;
  - `seal.rs`: OBS1, reusing OBA1;
  - `map.rs`: visit folders to sync objects;
  - `merge.rs`;
  - `outbox.rs`: a persistent, encrypted upload queue;
  - `supabase.rs`: the REST calls;
  - `state.rs`.
- **`os_engine::vault`:** seal and open with an explicit key and associated data; `AudioWriter::with_key`; a keychain store for iOS (`keyring` with the Apple `protected` store).
- **`session.rs`:** `Session::adopt`; `RunInfo` gains `run_id`; `Edit.from_run` and refinements refer to run ids; run folder names carry a device tag.
- **`patients.rs`:** `merge()`, with aliases.
- **`users.rs`:** Argon2id PINs with a lockout.
- **New dependencies:** `x25519-dalek`, `ed25519-dalek`, `hkdf`, `sha2` and `argon2`, then `tokio-tungstenite` later for Realtime. Each is checked live and pinned in `docs/versions.md` before it is added.

## Consequences

- **Brand promise.** The public wording, accepted by default, is: "Recordings and notes stay on your devices unless you turn on encrypted sync. Even then, only encrypted copies leave, and our servers can't read them." It stays true, and recordings in fact never leave at all. The sync settings screen and the guide say plainly that sync carries transcripts and notes only. Four texts change before sync ships: the README, the website, the bundle `longDescription` ("never leave this computer") and the iPhone microphone text.
- **Audio on other devices.** A computer can't play a phone's recording, and can't re-transcribe it. The evidence link still shows the transcript line and its time. This is the honest cost of keeping audio off the server.
- **Legal.** ANI.ML Health will run a hosted service that stores encrypted personal data, and under GDPR encrypted personal data is still personal data. A hosted pilot needs four things first: a privacy policy, terms for the sync service, a DPA template for clinics, and Supabase's DPA (which is part of its terms). A HIPAA BAA isn't needed for veterinary data.
- **Security review.** An external review of the threat model, the key hierarchy, the OBS1 format and the RLS policies happens **before any real clinic data syncs**, not after.
- **Cost.**
  - The Free plan now. Supabase Pro, about 25 USD a month, before the pilot.
  - SendGrid is already in use at ANI.ML for ARC.
  - Without audio, a visit takes roughly 200 KB in the database once padded, so storage grows slowly. Database disk use is checked against the Pro plan's allowance once real usage is known.
- **Two recovery secrets.** Users now have two. One printable sheet holds both, with plain names.
- **Metadata.** Some still leaks: visit volume, working hours, approximate consult length, emails and IP addresses. `docs/network.md` and the security whitepaper say so.
- **Lock-in.** Dependence on Supabase is limited: the app uses plain REST, the migrations are in the repo, the server can be self-hosted, and the data is ordinary Postgres rows.
- **Housekeeping.** CLAUDE.md still describes the encrypted store as "rusqlite with bundled-sqlcipher", which has been out of date since ADR-0008. The same change corrects it.

## Alternatives considered

| Option | Why not |
|---|---|
| Clinic hub with a relay. Clinical data lives only on clinic devices; phones and laptops sync with the clinic computer, and off site through an encrypted relay (iroh over QUIC) | It has the strongest privacy story. But seeing a visit at home needs the clinic computer to be on and reachable: small clinics switch PCs off at night, and relief and mobile vets have no hub. It also adds an unverified transport (iroh's TLS backend may clash with the native-tls choice) and a relay that ANI.ML must run. Its best ideas are kept: the cloud can lock a device out but not let one in, device leases, Argon2id PINs and the audit log. LAN sync stays the Clinic Hub's job in M4, on this same object format |
| Minimal relay. The phone records only, the PC transcribes and drafts, and one account key protects everything | The quickest to build, but it has five problems. Nothing is readable anywhere until one drafting computer is on. Revocation waits for the token to expire. There are no device signatures and no rollback detection. The recovery key is the account key itself, so it stops working after a device is removed. And it uploads raw PCM. Three of its ideas are kept: PC-to-PC sync comes first and is built on Windows, cargo features gate what the phone links, and all networking stays in Rust |
| Supabase SDK in the WebView | It puts tokens, and possibly keys, in JavaScript. It widens the CSP and adds a second HTTP client to the app. Rust handles the calls with plain REST instead |
| A web client that decrypts in the browser | We would serve the code that holds the keys, which weakens end-to-end encryption. Working from home means installing OpenBark |
| A key derived from a passphrase and stored on the server | A breached server could brute-force weak passphrases offline |
| Audio sync as a separate opt-in (the earlier draft of this ADR) | The owner decided on 2026-10-18 that sync carries transcripts and notes only. Audio is the most sensitive data, and keeping it off the server keeps ADR-0005's rule. Bringing it back would need a new decision |
| iCloud or CloudKit | Apple-only, and guideline 5.1.3(ii) bars personal health information in iCloud. Windows and Linux computers couldn't join |
| A CRDT for note bodies (ADR-0005's plan) | Not needed. Runs are immutable and edits create new runs. Showing "edited on two devices" is clinically safer than a silent merge |
