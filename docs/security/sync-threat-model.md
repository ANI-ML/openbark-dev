# Encrypted sync: threat model and design notes for the security review

Status: **written for the external review (M3 phase 3)**, which happens before any real clinic data syncs. Describes what is built on `feat/m3-sync` (M3 phase 2). The binding design is [ADR-0017](../adr/0017-accounts-and-encrypted-sync.md); this file records how it was built, where it departs from the ADR and why, and what we want the reviewers to look at hardest.

Code: `engine/os-sync` (Rust, all crypto and all network calls), `supabase/migrations/20261020120000_sync.sql` (schema, RLS, functions), `supabase/functions/revoke-device` and `delete-account` (portal), `engine/os-app/src/sync.rs` (app wiring). Tests: `engine/os-sync/tests/supabase.rs` (against a real local Supabase), `engine/os-sync/tests/fuzz.rs`, unit tests in each module, `scripts/sync-e2e.py` (two servers).

## 1. What we protect, and from whom

**Assets:** transcripts, notes, signatures and addenda, visit details (patient and owner names, species, reason, consent), patient records, device names. **Audio is out of scope by construction: it never leaves the device that recorded it.**

**Adversaries considered:**

| # | Adversary | Can | Must not be able to |
|---|---|---|---|
| A1 | The sync server operator, or anyone who breaches the Supabase project (database, logs, backups, Edge Functions, the publishable and service keys) | Read and change every row; drop, delay, replay or reorder rows; add devices to the table; serve any response | Read any clinical content; get a key that opens it; get a device of theirs accepted by the user's devices; make a device seal data under keys they know |
| A2 | Someone with access to the account's email inbox | Sign in to the account (email codes) | Get keys (approval needs a device already in the account, or the Recovery Kit); approve their own device; remove legitimate devices (removal requires an approved device's session, or the portal, which can lock out but not let in) |
| A3 | A removed (revoked) device, or a stolen one after removal | Keep what it already decrypted | Read anything new after rotation; write anything the others accept; reach the server at all (refused by RLS at once) |
| A4 | Another account on the same project | Everything A2 can, for their own account | Read, list or write any row of ours |
| A5 | Network attacker | TLS-level only | (TLS; `native-tls`, no redirects followed) |

Out of scope for phase 2 (documented, not defended): a compromised device that is still approved (it holds the keys); malware on the computer; a shared OS account (a PIN is not a cryptographic boundary, ADR-0017 section 3); traffic analysis beyond what section 6 lists.

## 2. Key hierarchy (as built)

All keys 256-bit from `getrandom`.

| Key | Where | Wrapped/derived | Opens |
|---|---|---|---|
| Local data key (ADR-0008) | OS keychain | unchanged, never syncs | this device's visit folders and sync state file |
| Device X25519 + Ed25519, rebind secret | OS keychain (`sync-<device id>`), or a vault-sealed file where there is no keychain or in tests (`OPENBARK_SYNC_SECRETS_DIR`) | never exported; server stores public keys and SHA-256(rebind secret) | envelopes addressed to it; signs objects and the roster |
| Account Master Key (AMK), versioned | server: one OBE1 envelope per approved device, one to the Recovery Kit | OBE1 = ephemeral X25519 → HKDF-SHA256(ikm = DH [‖ pairing bind], salt = eph_pub ‖ recipient_pub, info = "openbark envelope v1\|amk\|<version>\|<recipient>") → XChaCha20-Poly1305 with the same purpose string as AAD | space keys, roster |
| Recovery Kit | printed: 24 BIP-39 words = 256-bit seed (+ checksum) | X25519 secret = HKDF(seed, "kit x25519 v1"); kit token = HKDF(seed, "kit token v1"), server stores SHA-256(token) | the AMK's kit envelope |
| Space key, versioned + naming key | server `space_keys`: OBW1 under HKDF(AMK, "space-key"), AAD "space-key\|space\|version\|amk_version" | | object DEKs; object ids |
| Object key (DEK), fresh per object version | server `objects.wrapped_dek`: OBW1 under the space key, AAD "dek\|space\|object id\|key_version" | blob part keys = HKDF(DEK, "blob\|<part>") | one object version and its blob parts |
| Roster key | HKDF(AMK, "roster") | | the roster |

Object ids on the server are HMAC-SHA256(naming key, "kind\|local id")[..16] formatted as UUIDv4: stable across devices, unlinkable to visit ids or folder names by the server.

**Rotation** after a removal: a new AMK and space key version; envelopes for every device that is both approved on the server and active in the signed roster (with the roster's public keys) and for the kit public key **from the signed roster**, never from the server's column; every live object's DEK re-wrapped; all in one `sync.rotate` call that refuses if the device set or the object set changed meanwhile. Objects are not re-encrypted (ADR-0017). Blobs need no re-wrap (their keys derive from the DEK).

## 3. Formats

- **OBS1** (objects): `OBS1 | 0x01 | u16 header length | header JSON | nonce 24 | ciphertext | Ed25519 signature 64`. Header {v, id, space, kind, ver, dev, ctr}, `deny_unknown_fields`, ≤ 1 KiB, ids restricted to `[A-Za-z0-9-]`. AAD = everything up to the end of the header. Body = u32 length ‖ JSON {hlc, data} ‖ zero padding so the ciphertext is exactly 1, 4 or 8 KiB or a multiple of 8 KiB up to 64 KiB; padding must be zero on open. **The signature covers every byte before it** (header and ciphertext), not just the header as ADR-0017 worded it: otherwise any holder of the DEK could pair a valid signed header with other ciphertext.
- **OBB1** (blob parts): the OBA1 chunk layout (64 KiB chunks, per-chunk nonce, AAD = magic ‖ header ‖ chunk index ‖ last flag) **under its own magic**. Departure from ADR-0017 ("blobs use OBA1"): OBA1 is what an encrypted recording looks like on disk, so a guard that accepted OBA1 would accept a recording. The guard and the database refuse OBA1. Parts ≤ 16 chunks (1 MiB plaintext); ≤ 64 parts. The parent object's encrypted manifest lists each part's SHA-256; parts are checked for hash, header (object, space, version, index, count) and signature.
- **OBE1** (envelopes): `OBE1 | 0x01 | eph_pub 32 | nonce 24 | ct 48`; low-order points refused (all-zero shared secret).
- **OBW1** (wrapped keys): `OBW1 | nonce 24 | ct`, purpose bound as AAD.

Parsers are fuzzed (`tests/fuzz.rs`: random input and mutations of valid objects; 2,000,000 cases run clean on 2026-10-20; no panic, no mutated object opens).

## 4. Pairing (departure from ADR-0017: typed, not compared)

ADR-0017 has the new device show a 6-word safety code that the user *compares* on both screens. Comparing protects the approving device from a server that swaps the new device's keys, but not the **new** device from a server that hands it an envelope of a fake AMK the server chose (A1 could then read everything the new device uploads). We therefore have the user **type** the 6 words on the approving device:

1. The new device makes its keys and a 66-bit code (6 BIP-39 words). Pairing key K = Argon2id(code, salt = "openbark pair v1\|" ‖ device id; 19 MiB, t=2, p=1).
2. It registers as pending (10 minutes) with HMAC(K, device id ‖ X25519 pub ‖ Ed25519 pub) and its name sealed under HKDF(K).
3. The approving device, given the typed words, finds the pending row whose HMAC verifies (so the keys are the new device's), shows the name, and wraps the AMK to the X25519 key **with HKDF(K, "pair bind v1") mixed into the envelope key**.
4. The new device opens the envelope only with K: proof the approver saw its screen.

The QR code carries the same words plus the full key fingerprint, for phones (phase 4). Online guessing of the code is bounded by the 10-minute window; offline guessing of 66 bits against Argon2id within that window is infeasible. **Reviewers: please check this argument, the parameters, and whether 6 words is enough.**

Recovery Kit approval: the pending device opens the kit envelope itself, wraps the AMK to its own key, and calls `approve_with_kit(kit token)`; the server compares SHA-256(token) with the stored verifier. The token is derived on a separate label, so revealing it to the server says nothing about the kit's X25519 key. A pending device may read the kit's envelope (RLS) because only the kit opens it.

## 5. Server-side controls (defence in depth; not relied on for confidentiality)

- RLS on every `sync` table; every policy requires `sync.device_ok()`: the JWT's `session_id` belongs to an **approved** device of `auth.uid()`. A revoked device is refused at once even with an unexpired access token (tested).
- No INSERT/UPDATE/DELETE grants to `authenticated`; writes only through `security definer` functions with `search_path = ''` that check the caller. Reads through RLS (pull, blobs and keys use `security invoker` functions so RLS decides).
- Column grants hide `session_id` and `rebind_verifier`.
- Check constraints refuse any body that doesn't start with the expected magic (OBS1, OBB1, OBE1, OBW1), with size limits.
- `push` takes the space row lock, so `seq` is commit order (no skipped late commits); optimistic `base_version` for mutable kinds; immutable kinds (`run`, `event`) can't be overwritten; committed blob parts can't change.
- Revocation deletes the device's envelopes and its `auth.sessions` row (refresh stops), and sets `rotate_needed`. **Check on the hosted project that the `postgres` role may delete from `auth.sessions`** (it can locally); if not, the code logs a warning and RLS still refuses the device.
- A removed device id never rejoins: a removed device that asks again does so under a new id with new keys and needs a new approval.

## 6. What the server learns (ADR-0017 section 8, as built)

Email, sign-in times, IPs and user agents (Supabase logs); device count, platforms, public keys, last-seen times; for each object its kind (`visit`, `run`, `event`, `checks`, `transcript`, `patient`, `tombstone`), version, padded size, creation and update times, and the space key version; blob part counts. From these: visit volume per day, working hours, a loose hint of consult length (transcript blob sizes in 64 KiB steps), how many drafts and signatures. **Not** names, transcript or note text, species, reasons, device names, consent, which patient a visit is for. The integration test `the_server_sees_only_ciphertext` asserts on the raw rows of every table.

## 7. Client-side controls

- **Plaintext guard** (`guard.rs`), in the one place requests are built: every binary field must parse as the format its name promises; any other string must be ≤ 64 characters without spaces; field names such as `audio` or `name` are refused; sign-in bodies allow only their fields. Tests record every request body and assert no fixture name, no transcript words, no audio marker, no Storage or Realtime path.
- **Audio:** the mapping never opens `audio.wav`; `refuse_audio_file` rejects audio by name and by magic (RIFF, OBA1, fLaC, OggS, ID3, MP3 frame sync, ftyp, caff, FORM) before sealing and again after opening a blob.
- **Every pulled object** is checked before anything is written: OBS1 structure; header id, kind, space and version equal to the row (no moving or replaying under another name or version); signer in the signed roster (a removed device's objects only up to its last counter); version not lower than the last one seen (rollback); DEK unwrap and AEAD.
- Merges never destroy local data: last writer wins per header field by hybrid logical clock with both values in a local conflict log; runs are immutable and kept from both devices; signing is one-way and the first signature is the record; later drafts are set aside, not dropped (`merge.rs`, tested).
- Writes into visit folders go through the vault under the local data key, holding the app's per-visit lock.

## 8. Known gaps (phase 3 and later)

1. **Gap detection** (ADR-0017: "a gap that never fills raises a warning"): not built. Mutable objects overwrite their rows, so per-device counter gaps are normal; detecting withheld objects needs a signed per-device head or log. Rollback per object *is* detected.
2. **TOTP** second factor: not built (Supabase supports it; the UI and Rust calls are missing).
3. **Device lease** (lock synced copies after 30 days offline): phase 5.
4. **Recovery Kit rotation** (a new kit if the old one is lost while devices remain): not built.
5. **Patient registry writes** from sync and from the app aren't serialised by a lock (the vault's atomic replace prevents corruption; a concurrent edit could be lost and re-synced).
6. **Pending-device and pairing abuse:** at most 5 live pending devices per account; no rate limit on `check_pairing` attempts beyond Argon2id cost on the approving device.
7. **The refresh token** sits in the keychain beside the device keys; the access token only in memory.
8. **Metadata:** see section 6. Padding hides exact sizes only to bucket granularity.
9. **Supabase Auth settings on the hosted project** (10-minute access tokens, refresh rotation, OTP length and expiry, rate limits, SMTP) are dashboard settings; `supabase/README.md` lists them.
10. **Fuzzing** is a deterministic mutation fuzzer in the test suite, not coverage-guided (`cargo fuzz` needs nightly); worth a coverage-guided run during review.
11. Hosted project region and the Pro plan (backups) are owner tasks before the pilot.

## 8a. The clinic hub (ADR-0029, phase A)

ADR-0029 replaces the hosted Supabase project with a hub on the clinic's own computer (`engine/os-hub`); the Supabase transport stays until phase D. Everything above about keys, formats, sealing, the roster, merging and the plaintext guard is unchanged: the hub stores the same ciphertext the project stored and can't open it. What is new, and in the review's scope:

- **The listener** (docs/network.md L1): TLS 1.3 on the local network, only while hub mode is on, only for the allowed client ranges (others closed before the handshake), with body, connection and time limits, rate limits per address and per account, and a storage quota per account.
- **Identity**: no email, passwords or tokens. Every request is signed with the device's Ed25519 key (time, nonce, method, path, body hash); the hub checks it against the key it registered. Creating an account and registering any device need a single-use hub join code shown on the hub; ten wrong codes lock a client address out for an hour.
- **Pinning**: devices pin the hub certificate's SHA-256 (from the QR code, or after comparing a 50-bit check typed from the hub's screen) and refuse any other certificate.
- **New devices wait in a lobby**, filed under an Argon2id tag of their 6 words, until a device of the person's account (or the Recovery Kit) approves them.
- **The hub computer's own accounts** are readable by whoever controls that computer, as on any of the account's devices.

The SQL-to-Rust mapping of every function's checks is in [hub-authorization.md](hub-authorization.md).

## 9. Questions for the reviewers

1. The typed 6-word pairing (section 4): sufficient? Should the approving device also show a code for the new device to confirm?
2. Signatures over header and ciphertext; DEK re-wrap on rotation without re-encryption: acceptable given the revoked device keeps what it already read?
3. RLS policies and the `security definer` functions: any path for A2 or A4 to read or write another account's rows, or for a pending device to self-approve?
4. The plaintext guard's rules: anything that could carry plaintext through a structural field?
5. Recovery Kit derivations and the server-side kit verifier.
6. The clinic hub (section 8a, [hub-authorization.md](hub-authorization.md)): any call whose hub check is weaker than its SQL original? Is the lobby (new devices unassigned until approved, found by an Argon2id tag of their words) sound? Is the manual join's 50-bit certificate check enough on a clinic network?
