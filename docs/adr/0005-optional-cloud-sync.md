# ADR-0005: Optional per-user cloud sync, end-to-end encrypted

- Status: **proposed**. Direction accepted by the product owner on 2026-10-17; the design is finalised before the sync milestone.
- Date: 2026-10-17
- Deciders: product owner
- Amended by ADR-0017 (2026-10-18), which answers the open questions below, replaces the CRDT plan, and replaces the key "derived from the user's secret" with random keys passed between the user's devices plus a printed Recovery Kit (no passphrase-derived key). "Audio never syncs" still holds: sync carries transcripts and notes only.

## Context

The master prompt (§8 Non-goals) rules out a cloud-hosted service and accounts, so that clinical data never leaves the clinic. The product owner now wants **per-user cloud sync, eventually**: a vet's notes, templates and settings following them across devices and browsers, including outside the clinic network.

That conflicts with the local-first promise unless the cloud never sees readable clinical data.

## Decision (direction)

1. **Opt-in and off by default.** OpenBark stays fully usable with no account and no network. Sync is a separate, clearly labelled feature, and turning it off loses nothing locally.
2. **End-to-end encrypted.** Data is encrypted on the device with a key derived from the user's secret, which the server never holds. The sync server stores and relays ciphertext only, so it can't read notes, transcripts or patient details. Audio never syncs.
3. **Per user, then per clinic.** A user's own devices first. Sharing within a clinic team comes later, through the Clinic Hub (M4) or shared encrypted spaces.
4. **Self-hostable.** The sync server is open source alongside the app. A clinic or hospital can run its own; a hosted option may exist, but nobody is required to use it.
5. **Same sync engine as the Clinic Hub.** LAN sync (M4) and cloud sync use one replication protocol, so there's one conflict model to test.

## What this changes now (M1 store design)

The encrypted local store built in M1 is designed to sync from day one, even though sync ships later:
- **IDs:** every record gets a globally unique, time-ordered ID (UUIDv7), never an auto-increment integer.
- **Change history:** edits are recorded as an append-only change log with per-device version information. A CRDT (Automerge or Yjs, chosen in M1) is used for note bodies, where two devices may edit the same note.
- **What syncs:** notes, templates, settings and patient references are separate from data that never syncs (audio, model files, device secrets).
- **Deletes:** deletions are tombstones, so they propagate.
- **Sign-off:** signed notes are immutable, and later changes are addenda. Two devices editing at once then can't silently rewrite a signed record.

## Open questions (for the sync milestone)

- Account and identity: email and passkey, clinic SSO, or both.
- Key recovery when a user loses every device (a recovery code; optional escrow held by the clinic admin).
- Hosting: who runs the default sync service, where (Canadian data residency first?), and on what terms.
- Regulatory review of storing encrypted clinical records off-site (Canada, US, UK/EU).
- Updated `docs/network.md` entry, network test, and security whitepaper section.

## Consequences

- The no-cloud non-goal in master prompt §8 is amended: **no cloud service that can read clinical data, and nothing cloud-based required.** Optional E2EE sync is allowed.
- M1's store costs slightly more to build. Retrofitting sync onto a store designed without it would cost far more.
