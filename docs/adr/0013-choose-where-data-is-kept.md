# ADR-0013: The clinic chooses where models and visits are kept

- Status: accepted (built)
- Date: 2026-10-18
- Deciders: product owner (asked that users can choose where models are stored versus where notes are stored), engineering

## Context

Everything lived under the app's data folder (`%LOCALAPPDATA%\OpenBark`, `~/Library/Application Support/OpenBark`, `~/.local/share/OpenBark`). That folder is usually on the system drive, which is often the smallest. The models need 6 to 14 GB and can be downloaded again. The visits (recordings, transcripts, notes and the patient list) are the clinic's data and grow every day. Clinics want them in different places: models on a big second drive, visits on a drive that's backed up, or on a clinic share.

Until now the only way to move them was the `OPENBARK_MODELS_DIR` and `OPENBARK_SESSIONS_DIR` environment variables, which aren't something clinic staff can set.

## Decision

1. **Two folders the clinic can place: Models and Visits.** Templates, the team list and the key stay in the app's data folder. They're small, and the key stays apart from the data it protects.
2. **Settings > Storage** shows each folder, how much it holds and how much room is left on its drive, with **Change folder**. The desktop app uses the system folder picker. Web mode has a small folder browser over a loopback-only endpoint (`GET /api/folders`), because browsers can't hand over a folder path.
3. **Nothing changes until the clinic has seen what will happen.** The check (`check_folder`) shows:
   - the exact folder that will be used;
   - the free space on that drive;
   - anything worth knowing: a folder synced by OneDrive, Dropbox, Google Drive or iCloud (visits stay encrypted, so the service keeps copies it can't read), a removable drive, or a network drive.
   It refuses:
   - the folder already in use;
   - one inside or around it;
   - one overlapping the other kind's folder;
   - a folder OpenBark can't write to.
4. **Never mixed into other files.** A chosen folder that already holds other things gets an `OpenBark models` or `OpenBark visits` folder inside it. A folder that already holds OpenBark data of that kind is used as it is.
5. **Move, or switch.**
   - *Move* brings everything along. On the same drive each file is renamed (instant). Across drives each file is copied, checked by size, and only then removed from the old folder, with a progress bar.
   - A move never overwrites a different file: a clash stops it before anything changes. For models, a file with the same name and size already in place is treated as the same model, since the store re-verifies models anyway.
   - If a copy fails, what was copied is removed, renamed files go back, and the old folder is left as it was.
   - *Switch* uses the new folder as it is and leaves the old one where it is.
   - Changing a folder is refused while recording, and the note model is unloaded first, so its memory-mapped file can move on Windows.
6. **Saved in `locations.json`** in the app's data folder (`os_models::locations`). The environment variables and command-line options still win and show as "Set by ..." in Settings, so demos, tests and IT-managed installs stay fixed. `OPENBARK_LOCATIONS_FILE` moves the file itself, for tests.
7. **After a change:** web mode carries on with a fresh engine on the new folders, and the desktop app restarts itself.

## Consequences

- Visits can live on a backed-up drive or share. They stay encrypted at rest (ADR-0008), and the key stays on the computer, so a copied visits folder is unreadable without the key or the recovery key.
- Uninstalling deletes data only in the app's data folder (ADR-0012). Visits or models kept elsewhere stay, and the uninstall wording and README say so.
- A visits folder on a network share is used by one computer at a time; sharing one folder between several computers at once isn't supported (that's the Clinic Hub, M4).
- Tested with `move_tree` unit tests (rename, cross-drive copy, clash, same-size models, overlap) and `e2e/storage.mjs`, which runs only against a throwaway server whose folders are all copies.
