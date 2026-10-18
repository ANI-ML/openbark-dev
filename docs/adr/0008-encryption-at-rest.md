# ADR-0008: Encryption at rest: encrypted files, a data key in the OS keychain

- Status: accepted
- Date: 2026-10-17
- Deciders: engineering (M1 item 10; brief §3: no plain-text clinical data on disk outside exports)

## Context

Encounters are kept as folders: `session.json`, `audio.wav`, `transcript.jsonl` and `runs/NNN/{note.json, note.md, template.yaml}`. These folders are the record. The product owner asked that recordings stay traceable and re-runnable, and the M1 features (snapshots, edits, addenda, client documents) all build on them. All of it is currently plain text.

The M1 plan proposed an SQLCipher index plus encrypted audio. Since then the folder format has grown and works well. Moving it into a database now would touch every feature, for one benefit: a query index. Listing about 200 encounters by reading their small `session.json` files already takes milliseconds.

## Decision

1. **Encrypt the files and keep the folders.** Every file in an encounter folder is encrypted with **XChaCha20-Poly1305** (RustCrypto `chacha20poly1305` 0.11) under one random 256-bit **data key**.
   - **Small files:** `OBK1` magic, a 24-byte random nonce, then the ciphertext. The file name is bound in as associated data, so files can't be swapped between names.
   - **Audio:** `OBA1` magic, then fixed-size chunks. Each chunk is 64 KiB of audio with its own random nonce and tag, and the chunk index is bound in as associated data, so chunks can't be reordered.
     - The recorder streams audio straight into encrypted chunks, so a recording is never plain on disk, not even briefly.
     - The WAV header's sizes are only known at the end. The first chunk is re-encrypted then, with a fresh nonce; nonces are never reused.
     - Fixed-size chunks allow random access for playback.
2. **The data key lives in the OS keychain** (`keyring` 4.2: Windows Credential Manager, macOS Keychain, Secret Service on Linux desktops), in an entry named "OpenBark" / "data-key". It's created on first use. Fallbacks, in order:
   1. `OPENBARK_DATA_KEY` (base64), for servers and CI.
   2. The keychain.
   3. A key file, `<local data>/OpenBark/data.key`, readable only by the OS account. The Settings screen says so. This is weaker, but it still protects encounter folders that are copied, backed up or synced without the key.
3. **Recovery key.** If the key is lost, the encounters are gone. Settings shows a **recovery key** (the data key in base64) to print or store in the clinic's password manager. `OPENBARK_DATA_KEY` (or `openbark key import`) restores it on a new machine.
4. **Old plain files stay readable.** A file without a magic header is read as plain text. **Encrypt existing encounters** (Settings, or `openbark sessions encrypt`) rewrites them in place, and anything written from now on is encrypted.
5. **Playback without plain files.** Web mode decrypts audio into memory and serves byte ranges from it. The desktop app fetches the audio through a command instead of the file-system asset protocol.
6. **Not encrypted:** templates and standards (not clinical), models, and exports the user makes (PDF, copy).

## Consequences

- Copied, backed-up or synced encounter folders are unreadable without the key. "Show files" still opens the folder, but its files are opaque, so exports go through the app.
- Every read and write of clinical data goes through `os_engine::vault`. A file that reaches disk any other way is a bug, and a test guards the session paths.
- Losing the keychain entry and the recovery key loses the data. The recovery key is shown prominently, and the key file fallback is visible in Settings.
- The key file fallback on a shared OS account is weak. Hub mode (M4) and per-user keys come later, together with sign-in.

## Alternatives considered

| Option | Why not now |
|---|---|
| SQLCipher database for everything | Moves the whole folder format into SQL just as it settled. Audio would still need separate encryption. The index isn't needed at this scale. Kept pinned for later (versions.md). |
| AES-256-GCM | Equally sound, but 96-bit nonces make random nonces riskier over many files. XChaCha20's 192-bit nonce makes random nonces safe. |
| OS disk encryption only (BitLocker, FileVault) | Recommended as well, but it doesn't protect folders copied off the machine, and it isn't always enabled. |
| A passphrase typed at every start | Hurts the busy-clinic flow. It can be added later as an option that wraps the data key. |
