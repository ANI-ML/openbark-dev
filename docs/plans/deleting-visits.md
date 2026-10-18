# Plan: deleting visits and recordings

Status: **phase 1 implemented** (steps 0 to 11 and 14 to 16), with [ADR-0022](../adr/0022-deleting-visits.md). Phase 2 (steps 12, 13 and 17) is next. Date: 2026-10-18.

## Goal

A veterinarian or technician can delete a visit, or only its recording, from Review.
- A deleted visit waits in **Deleted visits** for 30 days and can be restored.
- Signed visits need a veterinarian, their PIN, a reason and a second confirmation.
- Every deletion leaves an encrypted, hash-chained entry in the deletion record, with no clinical text.
- Nothing is ever tested against a clinic's real visits.

## Ground rules for testing

- **Never on real data.** The dev box holds real visits, and the product owner's own server runs on port 4317. No test, script or manual check deletes anything there. Data removal is tested only in temporary folders (unit tests) and on the delete demo server (step 16).
- **The `os-app` test traps.** Each one would make a test touch this computer's real files.

  | Trap | What goes wrong | What to do |
  |---|---|---|
  | `App::new` uses `UserStore::default()` | Tests read, and a team test would write, the real `<local data>/OpenBark/users.json` | Every test sets `app.users = UserStore::new(tmp.join("users.json"))` (the field is public) |
  | `App::new` calls `vault::init()` | Without `OPENBARK_DATA_KEY` it loads the real key from the keychain or key file | A shared `test_env()` helper behind `std::sync::Once` sets one throwaway key and a temporary `OPENBARK_LOCATIONS_FILE`, for every test in the crate |
  | Environment variables are process-wide, and tests run in parallel | The existing test says it's "the only test in this crate that touches the environment" | Move its `set_var` calls into `test_env()`, and change that comment |
  | A background purge timer started by `App::new` | It would run inside tests | Start housekeeping explicitly (`App::start_housekeeping`) from the Tauri setup and from `serve`, never from `App::new`. Tests call `purge_due_at(now)` directly |

- **Linux CI.** `os-app` tests currently abort on Linux with a native `std::bad_alloc`. Run the new tests in WSL (`cargo test -p os-app --lib`) as well as on Windows. Don't mark the step done on Windows alone.

## Where each step can be tested

| Tag | Meaning |
|---|---|
| UNIT | `cargo test -p os-engine` / `-p os-models`. Temporary folders, `vault::tests::with_key` |
| APP | `cargo test -p os-app`. Temporary visits folder, `test_env()`, temporary users file |
| SERVER | `cargo test -p os-server`. Router with `tower::ServiceExt::oneshot`, as in `other_pages_are_kept_out` |
| UI | `pnpm --filter desktop test` (vitest, Testing Library) |
| DEMO | Playwright against the delete demo on port 4325 (step 16). Never 4317 |
| WSL | The same cargo tests on Ubuntu-24.04 in WSL |

## What exactly is removed

| Action | Removed from the visits folder | Added or changed | Never touched |
|---|---|---|---|
| Delete visit | The whole visit folder leaves the visits folder. It's renamed to `.openbark/deleted/<trash id>/` and keeps everything in it: `session.json`, `audio.wav`, `transcript.jsonl`, every `runs/NNN_*/` (records and client documents: `note.json`, `note.md`, `template.yaml`), and any `*.tmp` leftovers | An encrypted `deleted.json` inside the moved folder. One `visit_deleted` line in `.openbark/audit.log` | `patients.json` (the patient stays, even with no visits left); templates and standards saved from this visit; `users.json`; `locations.json`; models; exported PDFs; the original imported file; the PIMS |
| Restore | Nothing | The folder is renamed back, with a suffix if the name is taken. `deleted.json` is removed, links are refreshed (`link_visit`), and a `visit_restored` line is added | As above |
| Delete now, or 30 days passed | `.openbark/deleted/<trash id>/` and everything in it | One `visit_purged` line | As above |
| Delete recording | `audio.wav`, and `audio.wav.tmp` if one is left | `session.json` gains `audio_deleted: {at, by}`. One `recording_deleted` line | The transcript, every run, signatures, addenda, consent, `audio_sha256`, `audio_secs`, the source label |

## Steps

Phase 1 is steps 0 to 11, plus 14 to 16. Until step 12 lands, the engine refuses signed visits with "Signed visits can't be deleted yet. Add an addendum to correct the note." Phase 2 is steps 12, 13 and 17.

### Step 0: test harness (APP)

- Add `test_env()` to `os-app`'s test module, and move the existing test onto it.
- Add helpers:
  - `seed_visit(root, patient, others, sign: bool)`. It writes a visit through `Session::create_from_file` with the silent WAV the existing test builds. It also saves a one-segment transcript and a run (`save_run` with a stub `NoteResult`), and signs it with `sign_latest_by` when asked.
  - `team(app)`. It adds a veterinarian (PIN 2468), a technician (1357) and reception (1111) to the temporary users file.
- Tests: the existing `visits_group_under_patients_and_stay_linked_after_a_rename` still passes. A new assertion checks that `app.users` points inside the temp folder.

Done when: `cargo test -p os-app` passes on Windows and in WSL, and no test reads `<local data>/OpenBark`.

### Step 1: "signed" across every animal (UNIT, APP)

- `session.rs`:
  - `pub fn any_signed(&self) -> bool`, true if any run has `signed_at`.
  - `pub fn last_entry_at(&self) -> Option<DateTime<Local>>`, the latest `signed_at` or addendum `at`. Phase 2's clinic rule uses it.
- `os-app`: `list_sessions` uses `any_signed()`. That fixes Recent visits showing a signed multi-pet visit as Draft or Transcribed.
- Tests:
  - UNIT: in a multi-pet visit with only Pickles's note signed, `any_signed()` is true and `is_signed()` without a pet is false.
  - APP: `list_sessions()` reports that visit as signed.

### Step 2: the housekeeping folder and folder detection (UNIT)

- `os-models/src/locations.rs`:
  - `pub const HOUSEKEEPING_DIR: &str = ".openbark";`
  - `contents(FolderKind::Visits, ..)` counts `.openbark` as OpenBark's.
  - A folder that holds only `.openbark` (every visit deleted) is `Contents::OpenBark`.
- `os-engine` re-exports the constant. Path helpers: `deleted_dir(root)`, `audit_path(root)`, `policy_path(root)`.
- `os-app` `StorageView` gains `deleted_bytes` (`dir_size` of `.openbark/deleted`).
- Tests (UNIT):
  - `contents` on a folder with visits plus `.openbark` is `OpenBark`. With `.openbark` alone, `OpenBark`. With `.openbark` plus `notes.docx`, `Other`.
  - `move_tree` carries `.openbark/deleted/<id>/session.json` and `audit.log` along.
  - `session::list` ignores `.openbark` and everything under it.

### Step 3: append-only encrypted lines and the deletion record (UNIT)

- `vault.rs`: `pub fn append_line(path: &Path, plain: &[u8]) -> io::Result<String>`.
  - Encrypted: `OBL1:` followed by base64 of a 24-byte nonce and the ciphertext. The associated data is `OBL1` plus the file name.
  - Plain JSON when the vault is off.
  - One `write_all` of the whole line plus `\n` on a file opened for append, then `sync_data`.
  - Returns the stored line, which the caller hashes.
- `vault.rs`: `pub fn read_lines(path) -> io::Result<Vec<(String, Vec<u8>)>>` returns each stored line with its decrypted bytes.
- New `os-engine/src/audit.rs`:

  ```rust
  pub struct AuditLog { path: PathBuf }
  pub enum EventKind { LogStarted, VisitDeleted, VisitRestored, VisitPurged, RecordingDeleted, PolicyChanged }
  pub enum ReasonKind { Test, Mistake, Duplicate, ConsentWithdrawn, Other }
  pub struct DeleteReason { pub kind: ReasonKind, pub note: String }
  pub enum Purge { ByHand, After30Days }
  pub struct SignedRun { pub animal: usize, pub signer: Who, pub signed_at: DateTime<Local>,
                         pub addenda: usize, pub note_sha256: String }
  pub struct Tombstone { pub visit_id: String, pub trash_id: Option<String>, pub created_at: DateTime<Local>,
                         pub patient_ids: Vec<String>, pub animals: usize, pub recorded_by: Option<Who>,
                         pub source: SourceKind /* Live | File */, pub consent: Option<ConsentMethod>,
                         pub audio_sha256: String, pub audio_secs: f32, pub drafts: usize, pub signed: Vec<SignedRun> }
  pub struct Event { pub id: String, pub seq: u64, pub prev: String, pub at: DateTime<Local>,
                     pub kind: EventKind, pub by: Who, pub team: bool, pub visit: Option<Tombstone>,
                     pub reason: Option<DeleteReason>, pub purge: Option<Purge>,
                     pub recovered: bool, pub app_version: String }
  pub enum ChainReport { Complete { entries: usize }, Forked { entries: usize, at: u64 }, Broken { at: u64 } }
  impl AuditLog {
      pub fn for_sessions(root: &Path) -> Self;
      pub fn append(&self, draft: EventDraft) -> Result<Event, EngineError>; // fills id, seq, prev, at
      pub fn read(&self) -> Result<Vec<Event>, EngineError>;
      pub fn verify(&self) -> Result<ChainReport, EngineError>;
      pub fn contains(&self, event_id: &str) -> bool;
  }
  pub fn tombstone(s: &Session, trash_id: Option<&str>) -> Tombstone;
  ```

  - A process-wide `Mutex` serialises `append`. The first append to a missing file writes `log_started` first.
  - `prev` is the SHA-256 (hex) of the previous stored line. For the first line it's 64 zeros.
  - `tombstone` takes `animal` from the run's position in `PatientInfo::pets()`, never its name. `note_sha256` hashes the decrypted `note.md`.
- Tests (UNIT, under `with_key`):
  - Three appends: lines start with `OBL1:`, `verify` is `Complete { entries: 4 }` (counting `log_started`), and `read` round-trips.
  - Deleting line 2, swapping lines 2 and 3, or editing one character each give `Broken`.
  - Two lines with the same `prev` (simulating two processes) give `Forked`, not `Broken`.
  - The raw file never contains the patient name, the family name, a transcript word or a note word from the seeded visit. The decrypted lines don't contain the patient name either.
  - With the vault off, lines are plain JSON and `verify` still works.
  - A line copied in from another file name fails to decrypt (the associated data binds the name).

### Step 4: the per-visit lock (APP)

- In `App`: `visit_locks: Mutex<HashMap<String, Holder>>`, keyed by visit id, with `enum Holder { Importing, Recording, Drafting, Saving, Deleting }`. `fn lock_visit(&self, id, holder) -> Result<VisitLock>` returns a guard that frees the entry on drop.
- Taken by:
  - `import_source`, right after `LiveRecorder::start` returns its folder, until the end;
  - `start_recording`, held in `recording_work` beside the ticket;
  - `generate_note` and `refine_note` (Drafting);
  - `edit_note`, `sign`, `add_addendum` and `update_patient` (Saving);
  - every delete action (Deleting).
- The refusal depends on who holds the lock and what was asked:

  | Held by | Asked to delete | Asked to do anything else |
  |---|---|---|
  | Drafting | "This visit is being drafted. Delete it when that's finished." | "This visit is being saved in another window. Try again in a moment." |
  | Importing | "This visit is still being transcribed. Delete it when that's finished." | Same as above |
  | Recording | "This visit is still recording. Stop the recording first." | Same as above |
  | Saving | "This visit is being saved in another window. Try again in a moment." | Same |
  | Deleting | "This visit is being deleted." | "This visit is being deleted." |

- `visits_by_patient` skips a session whose `link_visit` fails with not-found (the folder vanished mid-listing), instead of failing the whole list.
- Tests (APP):
  - With a `Drafting` lock held, `delete_visit` is refused with the drafting message and the folder is untouched.
  - With a recording running into the visit, the delete is refused with the recording message.
  - Two `lock_visit` calls with the same id: the second is refused, and it succeeds after the first guard drops.
  - `list_patients()` succeeds when a visit folder is renamed away between `session::list` and linking. Simulate this with a hook, or by removing the folder after listing in a test-only path.

### Step 5: the Deleted visits store (UNIT)

- New `os-engine/src/trash.rs`:

  ```rust
  pub struct DeletedInfo { pub trash_id: String, pub original_name: String, pub deleted_at: DateTime<Local>,
                           pub by: Who, pub reason: DeleteReason, pub signed: bool,
                           pub purge_after: DateTime<Local>, pub event_id: String }
  pub struct Trash { root: PathBuf }
  impl Trash {
      pub fn for_sessions(root: &Path) -> Self;
      pub fn put(&self, visit_dir: &Path, info: &DeletedInfo) -> Result<PathBuf, EngineError>;   // rename, then deleted.json
      pub fn undo_put(&self, trashed: &Path, original: &Path) -> Result<(), EngineError>;   // the rollback
      pub fn list(&self) -> Vec<(DeletedInfo, Session)>;                     // newest first
      pub fn find_by_original(&self, name: &str) -> Option<DeletedInfo>;
      pub fn restore(&self, trash_id: &str) -> Result<PathBuf, EngineError>; // rename back, remove deleted.json
      pub fn begin_purge(&self, trash_id: &str) -> Result<PathBuf, EngineError>;  // rename to <id>.purging
      pub fn finish_purge(&self, purging: &Path) -> Result<(), EngineError>;      // remove_dir_all
      pub fn due(&self, now: DateTime<Local>) -> Vec<String>;
      pub fn recover(&self, now: DateTime<Local>) -> Vec<Recovered>; // missing deleted.json; leftover .purging
  }
  ```

- The trash id is `meta.id` when it's a valid UUID. Otherwise a new UUIDv7. A suffix is added if the name is taken.
- `fn valid_trash_id(s)` accepts `[0-9a-f-]` and an optional `-N` suffix only.
- Renames and removals retry three times, 200 ms apart, for `PermissionDenied` and Windows sharing violations. Then they fail with "Another program is using a file in this visit. Close any window showing it, then try again."
- Restore uses a free name: the original, then `<original>-1`, `-2` and so on.
- Tests (UNIT, under `with_key`):
  - `put`, then `list`, then `restore` returns the same folder name, `session.json` opens, and the transcript and runs are intact.
  - The trashed folder's name doesn't contain the patient name. No file under `.openbark/deleted` holds the patient name in the clear.
  - Restore when the original name is taken gives `<original>-1`.
  - `begin_purge` then `finish_purge` leaves nothing under `.openbark/deleted`.
  - `recover` gives an item with no `deleted.json` a fresh 30 days, and removes a leftover `.purging` folder.
  - `due(now + 31 days)` lists the item; `due(now + 29 days)` doesn't.
  - Invalid trash ids (`..`, `a/b`, `.openbark`, an empty string) are refused.

### Step 6: delete, restore and remove in the app (APP)

- `os-app`:

  ```rust
  #[derive(Deserialize)] pub struct DeleteRequest { pub by: Option<String>, pub reason: DeleteReason,
      #[serde(default)] pub pin: Option<String>, #[serde(default)] pub confirm_signed: bool }
  // Debug is written by hand and prints the PIN as "***".
  pub struct DeletedVisit { pub trash_id: String, pub pets: Vec<String>, pub signed: bool,
                            pub purge_after: String, pub patient_id: Option<String> }
  pub struct DeletedSummary { pub trash_id: String, pub patient: String, pub pets: Vec<String>,
                              pub created_at: String, pub deleted_at: String, pub by: String,
                              pub reason: DeleteReason, pub signed: bool, pub purge_after: String,
                              pub kept_until: Option<String> /* phase 2 rule */, pub bytes: u64 }
  pub fn delete_visit(&self, id: &str, req: &DeleteRequest) -> Result<DeletedVisit>;
  pub fn list_deleted(&self, by: Option<&str>) -> Result<Vec<DeletedSummary>>;
  pub fn restore_visit(&self, trash_id: &str, by: Option<&str>) -> Result<RestoredVisit /* id, patient_id */>;
  pub fn purge_deleted(&self, trash_id: &str, by: Option<&str>, pin: Option<&str>) -> Result<()>;
  pub fn purge_due_at(&self, now: DateTime<Local>) -> Result<usize>;
  pub fn start_housekeeping(self: &Arc<Self>);   // recover now, purge now, then hourly
  ```

- `delete_visit` order:
  1. `begin_work()`, then `session_dir(id)`.
  2. Lock the visit, and refuse if recording into it.
  3. `who(by, Need::Notes)`. Single-user mode needs a name: "Add your name in Settings, then delete the visit."
  4. Open the session. If `any_signed()`, refuse in phase 1 (step 12 replaces this).
  5. Build the tombstone and `trash.put`.
  6. `audit.append(visit_deleted)`. On error, `trash.undo_put` and return "OpenBark couldn't record the deletion, so the visit was kept. {detail}".
- `restore_visit`: `Need::Notes`, ticket, `trash.restore`, then `audit.append(visit_restored)`. On error, move it back and report. Then `link_visit`.
- `purge_deleted`: `Need::Notes` for unsigned items. Then:
  1. `begin_purge`;
  2. `audit.append(visit_purged, ByHand)`. On error, rename back and report;
  3. `finish_purge`.
- `purge_due_at`:
  - takes a ticket and does nothing if the gate is changing or the App is retired;
  - `by` is `Who::named("OpenBark")`, with `Purge::After30Days`;
  - skips items the phase 2 rule keeps.
- `start_housekeeping`:
  1. `trash.recover(now)`, recording missing entries with `recovered: true`;
  2. `purge_due_at(now)`;
  3. a thread that repeats step 2 hourly while the App isn't retired. Web mode starts it again on the fresh App after a folder change.
- `session_dir`'s not-found error checks `trash.find_by_original(id)`, and if found says "This visit was deleted. Restore it from Deleted visits in Settings."
- Reception: `who(by, Need::Notes)` already refuses. Its wording for deletion: "{name} can record and enter patient details; deleting visits needs a veterinarian or technician."
- Tests (APP):
  - Technician deletes an unsigned visit: it's gone from `list_sessions` and from both patients' pages (multi-pet), `list_deleted` has it, and `audit.verify()` is complete.
  - `restore_visit` brings back the same id and links, with `patient(id).visits` the same as before.
  - Reception is refused, and the folder is untouched.
  - In single-user mode with no name the delete is refused. With a name, it works.
  - In phase 1 a signed visit is refused with the phase 1 message. That includes a multi-pet visit where only Pickles's note is signed.
  - An audit write failure (make `audit.log` a directory) gives the "couldn't record" message, and the visit is still listed under its original name.
  - `purge_due_at(now + 31 days)` removes it and adds a `visit_purged` line with `After30Days`.
  - After a crash between rename and audit (simulated by calling `trash.put` alone), `start_housekeeping`'s recovery adds a `recovered` entry.
  - Opening the deleted id gives the "This visit was deleted" message.
  - A folder change while a delete holds its ticket is refused, as for drafts.

### Step 7: delete the recording (UNIT, APP)

- `session.rs`:
  - `pub struct AudioDeleted { pub at: DateTime<Local>, pub by: Who }`
  - `SessionMeta.audio_deleted: Option<AudioDeleted>` (`serde(default, skip_serializing_if)`)
  - `pub fn delete_audio(&mut self, who: &Who) -> Result<(), EngineError>`. It removes `audio.wav` and `audio.wav.tmp`, then saves `audio_deleted`.
- `os-app`:
  - `pub fn delete_recording(&self, id: &str, by: Option<&str>) -> Result<SessionView>`.
  - It takes a ticket, locks the visit, refuses if recording into it, and needs `Need::Notes`. If `any_signed()`, it needs `Need::Sign` with the message "Only a veterinarian can delete the recording of a signed visit."
  - It writes the tombstone and appends `recording_deleted`, then deletes.
  - If the visit has no recording: "This visit has no recording."
  - `SessionView` gains `audio_deleted: Option<AudioDeleted>`. `VisitSummary` gains `has_audio: bool`.
  - `session_audio` refuses with "This visit's recording was deleted on {date}."
- `os-cli` `sessions rerun --retranscribe` without audio says: "This visit's recording was deleted, so it can't be transcribed again. Drafting from the transcript still works."
- Tests:
  - UNIT: after `delete_audio`, there's no `audio.wav`, `load_transcript` and `latest_note` work, and `audio_sha256` and `audio_secs` are unchanged.
  - APP:
    - On a signed visit, a veterinarian deletes the recording. `add_addendum` and `export_pdf` still work, and the signature is unchanged.
    - A technician is refused on a signed visit and allowed on an unsigned one.
    - Reception is refused.
    - `session_audio` gives the deleted message.
    - The audit line holds `audio_sha256`.

### Step 8: Tauri commands and web routes (SERVER)

- `apps/desktop/src-tauri/src/lib.rs` gains these commands, registered in `generate_handler!`: `delete_visit`, `delete_recording`, `list_deleted`, `restore_visit`, `purge_deleted`, and later `deletion_record`.
- The Tauri setup calls `start_housekeeping`. So does `serve`, on start and after each folder change.
- `os-server` routes:

  | Route | Handler body | Returns |
  |---|---|---|
  | `POST /api/sessions/{id}/delete` | `DeleteRequest` | `DeletedVisit` |
  | `POST /api/sessions/{id}/recording/delete` | `{by}` | `SessionView` |
  | `GET /api/deleted?by=` | | `[DeletedSummary]` |
  | `POST /api/deleted/{tid}/restore` | `{by}` | `RestoredVisit` |
  | `POST /api/deleted/{tid}/purge` | `{by, pin?}` | `{}` |
  | `GET /api/audit?by=` (step 13) | | `DeletionRecordView` |

- Update the API list in the `os-server` module doc.
- `engine.ts`: `deleteVisit`, `deleteRecording`, `listDeleted`, `restoreVisit`, `purgeDeleted`, `deletionRecord`, each with an `isDesktop ? invoke : api` pair. Add the TypeScript types for `DeleteReason`, `DeletedVisit`, `DeletedSummary` and `AudioDeleted`.
- Tests (SERVER):
  - Each POST route without `X-OpenBark` gives 403. That reuses `local_only`, but test the real router built by a new `fn router(state)` extracted from `serve`.
  - `POST .../delete` with a valid body on a temporary App gives 200, and a GET of the visit then gives 400 with the "was deleted" message.
  - The PIN is read from the body only. `POST /api/deleted/{tid}/purge?pin=2468` on a signed item, with no PIN in the body, is refused.

### Step 9: toasts with an action (UI)

- `Ctx.toast(message: string, action?: { label: string; run: () => void })`.
- `App.tsx` renders an action button inside `.ob-toast`. The 4-second timer pauses while the pointer or focus is inside the toast, and the toast keeps `role="status"`.
- Tests (UI): the toast shows **Undo** and calls `run` once. It closes after 4 s. It stays open while focused.

### Step 10: Review: Delete visit and Delete recording (UI)

- **Delete visit**: a destructive button (`variant="danger"`, compact, Lucide `Trash2`) in `.visit-edit-btn` beside **Edit details**. It shows when `app.can.notes`.
  - Unsigned: it opens a small menu (`role="menu"`, arrow keys move, Escape closes and returns focus) with the reasons. Choosing one calls `deleteVisit`. Then:
    1. a toast "Visit deleted" with **Undo**;
    2. navigate to the patient's page (`view.patient.patient_id`), or to Patients when the visit has no patient;
    3. Undo calls `restoreVisit` and opens the restored visit.
  - Signed in phase 1: disabled, with the phase 1 message as its tooltip.
- **Delete recording**: a compact ghost button under the player, and only when the player is showing. Hidden when `!app.can.notes`. Disabled with a tooltip when the visit is signed and `!app.can.sign`. It opens the recording dialog, and on confirm calls `deleteRecording`, replaces the view, and shows the toast "Recording deleted".
- When `view.audio_deleted` is set, the player's place shows the "Recording deleted" line.
- Engine errors are shown through `t()`, so fixed messages are translated. Messages with a name stay in English, as today.
- **Patient page**: `PatientView` gains `deleted: usize`, the trash items linked to this patient. A muted line under the list shows it.
- Tests (UI):
  - The menu lists the five reasons and the hint in order. It names the other animals in a multi-pet visit. Escape closes it and focus returns to **Delete visit**.
  - Reception (`can.notes` false) sees no button.
  - The recording dialog shows the imported-file line only for imported visits, and the unsigned line only when unsigned.
  - With `audio_deleted` set there's no `<audio>`, and the line reads exactly as in the copy table.

### Step 11: Settings: Deleted visits (UI)

- New `components/DeletedVisits.tsx`, rendered after `<StorageSection />` when `app.can.notes`.
- Rows show the patient (or "No patient"), a "Signed" status dot, the visit date, who deleted it and the reason, and the removal date, with **Restore** (secondary) and **Delete now** (destructive).
- On phones the rows stack, like the visit list.
- The Storage section shows "Deleted visits: {size}" under the visits folder when `deleted_bytes > 0`.
- **Delete now** opens the "Delete this visit now?" dialog.
- Tests (UI): the empty state; a row with every field; Restore calls `restoreVisit` and shows "Visit restored"; Delete now opens the dialog and confirming calls `purgeDeleted`.

### Step 12: signed visits (APP, UI), phase 2

- `delete_visit` when `any_signed()`:
  1. `who(by, Need::Sign)`, with the message "Only a veterinarian can delete a signed visit. {name} is using OpenBark now."
  2. With a team, `pin` is required and checked with `users.verify(by, pin)` in the same call. Without a team, there's no PIN.
  3. `reason.note.trim()` must be at least 10 characters: "Give a reason for deleting this signed visit."
  4. `confirm_signed` must be true: "Confirm that the note is kept elsewhere, or isn't needed."
- `purge_deleted` on a signed item: `Need::Sign`, plus the PIN with a team. `restore_visit` stays `Need::Notes`.
- **The signed dialog** (`components/DeleteSignedDialog.tsx`, using `Dialog`) has two steps in one dialog:
  - Step 1: **Add an addendum instead** (primary; closes the dialog and opens the addendum form), **Continue** (destructive), **Keep visit** (ghost).
  - Step 2: the reason select, the words field, the PIN field (team only, `inputMode="numeric"`, `autocomplete="off"`) and the acknowledgement checkbox. **Delete signed visit** stays disabled until all of them are filled.
  - A wrong PIN shows "That PIN isn't right." under the field and keeps what was typed, except the PIN.
- Review's **Delete visit** on a signed visit opens this dialog when `app.can.sign`. Otherwise it's disabled, with the tooltip "Only a veterinarian can delete a signed visit."
- Tests:
  - APP:
    - A technician is refused on a signed visit.
    - A veterinarian with a wrong PIN is refused, and the folder is untouched.
    - A veterinarian without `confirm_signed` is refused. With 5 characters of reason, refused.
    - With everything right, the visit is in Deleted visits and the entry holds `signed` runs with signer, time, addenda count and `note_sha256`, where the hash equals the SHA-256 of the decrypted `note.md`.
    - In a multi-pet visit with one signed animal, the signed path is required.
    - A technician's **Delete now** on a signed item is refused. A veterinarian with the PIN succeeds.
    - Single-user mode: no PIN is needed, but the reason and confirmation are.
  - UI: step order; the button stays disabled until complete; **Add an addendum instead** calls its callback; the PIN field is absent in single-user mode.

### Step 13: the deletion record view and check (APP, UI), phase 2

- `pub fn deletion_record(&self, by: Option<&str>) -> Result<DeletionRecordView>` returns the events, newest first, with patient names looked up from `patients.json` at read time, plus a `ChainReport`. It needs `Need::Sign`.
- In `DeletedVisits.tsx`, veterinarians get a **Deletion record** button. It opens a `Sheet` with the entries and **Check the record**.
- Tests:
  - APP: a technician is refused. Names are resolved. A tampered file reports `Broken`.
  - UI: each event kind renders with its sentence. Each check result shows the right message.

### Step 14: translations (UI)

- Add every string in the copy tables below to `packages/i18n/i18n-{de,es,fr,pt}.json`. The engine messages must be added by hand, because `scripts/i18n-keys.py` only finds `t("...")` calls.
- Run `python scripts/i18n-build.py packages/i18n/i18n-*.json`.
- `i18n.test.ts` checks the placeholders.
- Mark the new strings for review by native-speaking veterinary staff, as `docs/languages.md` asks.
- Done when: the vitest suite passes and a French-language screenshot of each dialog is checked by eye for overflow at 390 px.

### Step 15: guide and docs

- `docs/guide/review.md`: Delete visit, Delete recording, Undo and Deleted visits. Also what deletion does and doesn't remove (the table from ADR-0022 section 8, in plain words), and that the original imported file stays where it was.
- `docs/guide/team-and-security.md`:
  - who can delete what;
  - the PIN for signed visits;
  - single-user mode is honour-based;
  - the deletion record and its limits;
  - examples of retention periods with their sources (Ontario 5 years after the last entry, British Columbia 7, Texas 3, California 3), with "check your licensing body".
- `docs/guide/troubleshooting.md`: "Another program is using a file in this visit."
- The `os-server` module doc's API list (done in step 8). M3 phase 5 gets a pointer to ADR-0022 section 10. ADR-0011's "Deleting a patient isn't offered yet" line gets "Deleting visits: ADR-0022".

### Step 16: end-to-end on the delete demo only (DEMO)

- **The demo** runs on port 4325, from its own copy of the binary and UI:

  ```bash
  DEMO="$PWD/target/delete-demo"          # gitignored
  mkdir -p "$DEMO" && touch "$DEMO/OPENBARK-DEMO"
  export OPENBARK_SESSIONS_DIR="$DEMO/visits" OPENBARK_TEMPLATES_DIR="$DEMO/templates" \
         OPENBARK_USERS_FILE="$DEMO/users.json" OPENBARK_LOCATIONS_FILE="$DEMO/locations.json" \
         OPENBARK_DATA_KEY="<a throwaway key, never this computer's>"
  "$DEMO/openbark.exe" serve --port 4325 --ui-dir "$DEMO/ui" --models-dir "<the models folder>"
  ```

  Models aren't clinic data, and deletion never touches them.
- **New script** `apps/desktop/e2e/deleting.mjs`, run as `OPENBARK_URL=http://localhost:4325 node e2e/deleting.mjs "$DEMO"`. It refuses to start unless all of these hold:
  1. the URL's port isn't 4317;
  2. `<demo>/OPENBARK-DEMO` exists;
  3. `GET /api/storage` reports a visits path inside `<demo>`.

  It only deletes visits it created in the same run, and records their ids.
- **Seed**, through the API: import two fixture WAVs from the eval fixtures (`cargo run -p fixture-tts` regenerates them).
  - Maple Okonkwo, a single animal.
  - Olive Haddad with `others=Pickles`.
  - Draft both with the default model, and sign Maple's note.
  - Copying the 4321 demo's visits (copy, never move, with that demo's throwaway key) is an acceptable shortcut.
- **Single-user flows:**
  1. Delete Olive's unsigned visit from Pickles's page:
     - the menu names Olive;
     - choosing "Recorded by mistake" shows the toast;
     - both patient pages lose the visit and show the deleted line;
     - **Undo** restores the same id.
  2. Delete it again. In Settings > Deleted visits, **Restore**. Delete it again, then **Delete now** and confirm. Then check:
     - no folder under `visits/` contains `olive`;
     - `.openbark/deleted/` is empty;
     - every line of `.openbark/audit.log` starts with `OBL1:`;
     - the file doesn't contain "Olive", "Haddad" or "Pickles".
  3. On Maple's visit, **Delete recording**:
     - the dialog text matches the copy table;
     - after confirming, the player is gone, the line shows, the transcript remains, **Save PDF** still downloads, and **Add addendum** still works.
  4. Phase 2: delete Maple's signed visit through both dialog steps. Check that **Delete signed visit** stays disabled until the form is complete.
- **Team flows** (the script adds the three members through `POST /api/users`):
  - Reception: no **Delete visit**, no Deleted visits section, and `POST .../delete` gives 400 with the reception message.
  - Technician: deletes an unsigned visit. **Delete visit** on a signed visit is disabled with its tooltip.
  - Veterinarian:
    - PIN 0000 shows "That PIN isn't right." and the visit stays;
    - PIN 2468 deletes it;
    - **Delete now** on it asks for the PIN;
    - **Deletion record** lists the entries;
    - **Check the record** says complete.
- **Guard check:** `fetch` of `POST /api/sessions/{id}/delete` without `X-OpenBark` gives 403.
- **Layout:** screenshots of the menu, each dialog and Deleted visits in desktop light, desktop dark and mobile light (390 x 844). At each one, `document.scrollingElement.scrollWidth <= innerWidth`. Keyboard: Tab to **Delete visit**, Enter opens the menu, arrows and Escape work, and focus returns.
- **Brand checks** before closing: tokens only, Lucide icons, no emoji, the destructive style from brand guide section 7.2, one primary button per view, visible focus rings, and contrast of at least 4.5:1 on every new text.
- **Done when:** the script reports no problems in all three variants, and the screenshots are reviewed.

### Step 17: optional clinic rule, "Keep signed visits for at least N years" (APP, UI), phase 2

- `.openbark/policy.json` (encrypted): `{keep_signed_years: Option<u8>, set_by: Who, set_at}`.
- `App::set_retention(years, by, pin)` needs `Need::Team` plus the PIN with a team, and appends `policy_changed` with the old and new values.
- `delete_visit` and `purge_deleted` refuse a signed visit while `last_entry_at + years > now`: "Your clinic keeps signed visits for {years} years. This one can be deleted from {date}."
- `purge_due_at` skips such items. `DeletedSummary.kept_until` shows why.
- Settings, Deleted visits (veterinarians): a select labelled "Keep signed visits for at least", with Off, 3, 5, 7 and 10 years. Helper: "Signed visits can't be deleted until this long after their last signature or addendum. Check your licensing body's rule."
- Tests (APP): with 5 years set, deleting a signed visit from last month is refused, and an unsigned one isn't affected. A technician can't change the rule. Changing it adds a `policy_changed` entry.

### Not in this plan (M3 phase 5)

- "Delete from this computer", "Delete everywhere" and restore events follow ADR-0022 section 10 and are built with sync.
- Checklist for that phase:
  - events are keyed on `meta.id`;
  - a suppression record lives in `.openbark/`;
  - received deletions go to Deleted visits for 30 days;
  - the Supabase backup caveat on wrapped keys is settled first.

## Copy

Every string goes through `t()`. Labels have no full stop; sentences do. `{pets}` is joined with `Intl.ListFormat` in the app language ("Olive and Pickles").

### Review

| Where | Copy |
|---|---|
| Button (hidden for reception) | Delete visit |
| Tooltip, technician on a signed visit | Only a veterinarian can delete a signed visit. |
| Tooltip, phase 1 on a signed visit | Signed visits can't be deleted yet. Add an addendum to correct the note. |
| Menu title | Why delete this visit? |
| Menu items | Test or practice recording / Recorded by mistake / Duplicate visit / Client withdrew consent / Another reason |
| Menu hint | Filed under the wrong patient? Use Edit details instead. |
| Menu footer | It moves to Deleted visits for 30 days. |
| Menu, multi-pet | This visit also has notes for {pets}. They're deleted with it. |
| Toast | Visit deleted |
| Toast action | Undo |
| Toast after Undo or Restore | Visit restored |
| Recording button | Delete recording |
| Recording deleted line | Recording deleted {date} by {name}. The transcript and notes are kept. |
| Toast | Recording deleted |

### The recording dialog

| Part | Copy |
|---|---|
| Title | Delete this recording? The note stays. |
| Body | The transcript, drafts and signed notes are kept. You won't be able to play this visit back or transcribe it again. |
| Unsigned visits only | The note isn't signed yet. Listen back to anything you want to check first. |
| Multi-pet visits only | The recording covers every animal in this visit. |
| Imported visits only | The file you imported, {file}, is still where it was. Delete it there too if you need to. |
| Buttons | Delete recording (destructive), Keep recording (ghost) |

### The signed visit dialog (phase 2)

| Part | Copy |
|---|---|
| Step 1 title | Delete a signed visit? |
| Step 1 body | Signed notes are part of the medical record. Your licensing body may require you to keep them for years. To correct a signed note, add an addendum instead. |
| Step 1, second paragraph | Copies already in your practice software, or saved as PDF, aren't affected. |
| Step 1, multi-pet | This visit also has notes for {pets}. They're deleted with it. |
| Step 1 buttons | Add an addendum instead (primary), Continue (destructive), Keep visit (ghost) |
| Step 2 title | Delete the signed visit for {patient}, {date}? |
| Step 2 title, no patient | Delete the signed visit from {date}? |
| Reason label | Reason |
| Reason placeholder option | Choose a reason |
| Words label | In a few words |
| Words helper | Saved in the deletion record. Leave out clinical details. |
| PIN label (team only) | Your PIN |
| Checkbox | This note is kept in our practice software, or we don't need to keep it. |
| Footnote | It stays in Deleted visits for 30 days. Only a veterinarian can remove it sooner. |
| Step 2 buttons | Delete signed visit (destructive), Keep visit (ghost) |

### Settings: Deleted visits

| Where | Copy |
|---|---|
| Section title | Deleted visits |
| Intro | Deleted visits are kept here for 30 days, then removed for good. |
| Empty state | No deleted visits. |
| Row, no patient | No patient |
| Row lines | Visit of {date} / Deleted {date} by {name} / Removed for good on {date} |
| Row line, phase 2 rule | Kept until {date} by your clinic's rule |
| Buttons | Restore (secondary), Delete now (destructive) |
| Storage line | Deleted visits: {size} |
| Toast | Visit deleted for good |
| Delete now title | Delete this visit now? |
| Delete now body | OpenBark removes its files now, and it can't be restored. Backups of this computer may still hold encrypted copies, which the recovery key can open. |
| Delete now, signed | This visit has a signed note. Only a veterinarian can remove it before {date}. |
| Delete now buttons | Delete now (destructive), Keep visit (ghost) |
| Patient page, one | 1 deleted visit is kept for 30 days. Restore it from Deleted visits in Settings. |
| Patient page, several | {n} deleted visits are kept for 30 days. Restore them from Deleted visits in Settings. |

### The deletion record (phase 2)

| Where | Copy |
|---|---|
| Button and sheet title | Deletion record |
| Intro | Every deletion and restore, with who did it and why. No notes or transcripts are kept here. |
| Empty | Nothing has been deleted yet. |
| Visit deleted | {name} deleted the visit for {patient} from {date}. |
| Visit restored | {name} restored the visit for {patient} from {date}. |
| Removed by hand | {name} removed the visit for {patient} from {date} for good. |
| Removed after 30 days | The visit for {patient} from {date} was removed for good after 30 days. |
| Recording deleted | {name} deleted the recording of the visit for {patient} from {date}. |
| Recovered entry | Recorded after OpenBark closed unexpectedly. |
| Rule changed | {name} set signed visits to be kept for {years} years. / {name} turned off the rule for keeping signed visits. |
| Check button | Check the record |
| Check, complete | The record is complete: {n} entries, none missing or changed. |
| Check, forked | The record is complete: {n} entries. Two copies of OpenBark wrote to it at the same time, which is harmless. |
| Check, broken | Entry {n} doesn't match the one before it. The record may have been changed outside OpenBark. |

### Engine messages

| When | Copy |
|---|---|
| Reception | {name} can record and enter patient details; deleting visits needs a veterinarian or technician. |
| Technician, signed visit | Only a veterinarian can delete a signed visit. {name} is using OpenBark now. |
| Technician, signed recording | Only a veterinarian can delete the recording of a signed visit. |
| Single-user, no name | Add your name in Settings, then delete the visit. |
| Wrong PIN | That PIN isn't right. |
| Signed, no reason | Give a reason for deleting this signed visit. |
| Signed, no acknowledgement | Confirm that the note is kept elsewhere, or isn't needed. |
| Phase 1, signed | Signed visits can't be deleted yet. Add an addendum to correct the note. |
| Lock messages | As in step 4 |
| Record couldn't be written | OpenBark couldn't record the deletion, so the visit was kept. {detail} |
| File in use | Another program is using a file in this visit. Close any window showing it, then try again. |
| Opening a deleted visit | This visit was deleted. Restore it from Deleted visits in Settings. |
| Stale trash id | That deleted visit is no longer there. It may have been restored or removed for good. |
| No recording | This visit has no recording. |
| Playback after deletion | This visit's recording was deleted on {date}. |
| CLI retranscribe | This visit's recording was deleted, so it can't be transcribed again. Drafting from the transcript still works. |
| Phase 2 rule | Your clinic keeps signed visits for {years} years. This one can be deleted from {date}. |

## Order and rough size

| Steps | What | Size |
|---|---|---|
| 0 to 2 | Harness, `any_signed`, housekeeping folder | 1 day |
| 3 | `append_line` and the deletion record | 1.5 days |
| 4 to 6 | Lock, Deleted visits store, app calls | 3 days |
| 7 | Delete recording | 1 day |
| 8 and 9 | API, toast action | 1 day |
| 10 and 11 | Review and Settings UI | 2 days |
| 14 to 16 | Translations, guide, end-to-end | 2 days |
| 12, 13, 17 | Phase 2: signed visits, record view, clinic rule | 3 days |

## Risks

| Risk | Mitigation |
|---|---|
| A test deletes or rewrites real clinic data | Step 0's harness. The e2e script's three checks before any action. Port 4317 is never a target |
| Windows refuses a rename while a file is open | Retries, a plain message, and the visit stays visible until the move succeeds |
| The chain forks when the desktop app and `openbark serve` share a folder | The check reports "two copies wrote at once" as harmless. The guide advises running one of them per folder |
| Wrong PINs can be guessed on a shared computer | A PIN is a convenience, not authentication (`users.rs`). A follow-up can add a short pause after five wrong PINs, in one place used by both the picker and deletion |
| People expect "deleted" to mean gone from backups | The Delete now dialog and the guide say plainly what remains |
| Undo after navigating away fails because the restored name is taken | Restore picks a free name and opens whatever id it returns |
| The new os-app tests hit the Linux `bad_alloc` | Run them in WSL. If the native crash reproduces, keep the deletion tests independent of model loading so they can be run on their own |
