# ADR-0022: Deleting visits and recordings

- Status: accepted (owner asked to implement, 2026-10-18); phase 1 implemented
- Date: 2026-10-18
- Deciders: product owner (asked: "Implement ... deleting visits."), engineering
- Relations:
  - Fulfils ADR-0011, which said deleting clinic data "needs its own careful flow".
  - Builds on ADR-0008 (encryption at rest) and ADR-0013 (where data is kept).
  - Gives ADR-0017 section 10 ("Delete everywhere") the local flow and audit log it relies on.
  - Copy follows the brand guide (ADR-0016, `docs/brand/DESIGN.md` section 8). Interaction follows `docs/design/DESIGN.md` section 5.
- Plan: [docs/plans/deleting-visits.md](../plans/deleting-visits.md)

## Context

OpenBark can't delete anything a clinic records today. A test recording, a visit filed under the wrong animal, a duplicate import, or a client who withdraws consent all stay for good. The only way out is to delete folders by hand. That leaves no record, can break the patient list, and fails easily on Windows.

Facts from the code that shape the design:

| Fact | Where | Why it matters |
|---|---|---|
| A visit is one folder: `session.json`, `audio.wav`, `transcript.jsonl`, `runs/NNN_<model>/{note.json, note.md, template.yaml}` | `os-engine/src/session.rs` | One folder move removes or restores a whole visit |
| The folder name is the API id, and it contains the patient's name (`2026-10-17_143205_maple-recheck`). `meta.id` is a UUIDv7 | `session.rs`, `os-app` `session_dir` | Deleted items and audit entries must use `meta.id`, never the folder name |
| Every file is encrypted under one install-wide data key. Associated data binds only the file name (small files) or chunk index (audio), not the folder | `os-engine/src/vault.rs`, ADR-0008 | A visit folder can be moved without re-encrypting. There is no per-visit key |
| Visits point to patients. `patients.json` doesn't list visits | `os-engine/src/patients.rs` | Deleting a visit needs no change to the patient registry |
| Signed runs never change. Corrections are addenda. In a multi-pet visit each run carries `pet` | `session.rs` `RunInfo`, `add_addendum_by` | "Signed" must be checked across every animal |
| `Session::is_signed()` only checks the handle's own animal. For a multi-pet visit opened without a pet it returns false | `session.rs` | A delete guard can't use it. Recent visits already mislabels signed multi-pet visits as drafts |
| Roles are veterinarian, technician and reception. The PIN is checked once, at "Who's using OpenBark?". Later calls trust `by` | `os-engine/src/users.rs`, `os-app` `who()` | Deleting a signed visit should check the PIN again in the same call |
| The work gate (`begin_work`, `Busy`) only protects folder changes. There is no per-visit lock | `os-app/src/lib.rs` | A delete during a draft would let `save_run` recreate the folder without its audio or transcript |
| `locations::contents` treats anything in the visits folder other than visit folders and `patients.json` as "other files" | `os-models/src/locations.rs` | A new trash folder or log file there would make an existing visits folder look foreign (ADR-0013) |
| After import, only playback and the CLI's `--retranscribe` read `audio.wav`. Drafting, signing, addenda, PDFs and evidence links use the transcript | `os-app` `session_audio`, `os-cli/src/sessions.rs` | Deleting only the recording keeps every other feature working |
| There is no audit log. Who did what is stored on each revision | `users.rs` module doc, ADR-0017 section 10 | Deletion is the first action that leaves nothing behind, so it needs a separate record |

Rules outside the code:

- **Retention.** Regulators require veterinary records to be kept for years, and to be corrected without hiding the original.
  - Ontario (CVO): keep records five years after the last entry. Changes keep the original content and record who changed it and when.
  - British Columbia (CVBC): seven years from the last visit.
  - Texas (22 TAC 573.52): three years after the last visit.
  - California (16 CCR 2032.3): three years after the last visit. This comes from secondary sources; the official text wasn't checked.
  - UK (RCVS, chapter 13): "as long as is necessary". Amendments must not remove or obscure the original.
  - Every one of them requires records to be destroyed in a way that protects confidentiality.
- **OpenBark is not the practice management system (PIMS).** Clinics copy the signed note into the PIMS, which holds the legal record. OpenBark can't know which jurisdiction a clinic is in.
- **Scribe audio.** No veterinary regulator guidance on AI-scribe audio was found. Human-medicine guidance in Ontario leans towards deleting audio once a clinician has checked and stored the note. This supports the brand guide's example: "Delete this recording? The note stays."
- **Secure deletion.** Deleted files can survive on SSDs, copy-on-write file systems, shadow copies, backups and cloud recycle bins. NIST SP 800-88 Rev. 2 (September 2025) names cryptographic erase as the purge method for encrypted media. Overwriting isn't reliable on SSDs.

## Decision

### 1. Three actions, one table of rules

| Action | What it removes | Who, with a team | Single-user | How it's confirmed | Can it be undone? |
|---|---|---|---|---|---|
| Delete an unsigned visit | The whole visit, moved to **Deleted visits** | Veterinarian or technician | The name in Settings | No dialog. The person picks a reason from a short menu, and a toast offers Undo | Yes, for 30 days |
| Delete a signed visit (any animal's note signed) | The whole visit, moved to **Deleted visits** | Veterinarian only, with their PIN re-entered | The name in Settings, plus a written reason | A two-step dialog. Step 1 points to addenda. Step 2 asks for a reason, the PIN and an acknowledgement | Yes, for 30 days |
| Delete the recording | `audio.wav` only. The transcript, drafts, signed notes and addenda stay | Veterinarian or technician on an unsigned visit; veterinarian only on a signed one | The name in Settings | A dialog: "Delete this recording? The note stays." | No |
| Restore a deleted visit | Nothing | Veterinarian or technician | The name in Settings | None; a toast | Not applicable |
| Delete now, from Deleted visits | The files, for good | Veterinarian or technician if unsigned; veterinarian with PIN if signed | The name in Settings | A dialog | No |
| Removed after 30 days | The files, for good | OpenBark itself | Same | None | No |

Rules that apply to every row:
- Reception can't delete, restore or see Deleted visits.
- Single-user mode needs a name in Settings, as editing and signing do. Like signing, it is honour-based, and the guide says so.
- Every action writes an entry to the **deletion record** (section 6). An action that can't be recorded doesn't happen.
- Nothing outside OpenBark's own folders is ever deleted. That includes the original file someone imported, PDFs, the PIMS and the clipboard.

### 2. Signed notes are medical records

- **Corrections stay addenda.** The signed dialog's first step says so and offers **Add an addendum instead** as the first choice.
- **Signed visits can be deleted, with guards.**
  - Only a veterinarian can do it, with the PIN re-entered and checked inside the same engine call.
  - A reason is required: a reason from the list plus a few written words.
  - The person ticks an acknowledgement that the note is kept elsewhere or isn't needed. The engine refuses the request without it, so no client can skip the second step.
  - Refusing outright was rejected. OpenBark isn't the record of truth, there are real reasons to delete (wrong recording, duplicate, consent withdrawn), and a refusal pushes people to delete folders by hand, with no record at all.
- **"Signed" means any signed run, for any animal.** A new `Session::any_signed()` checks `meta.runs.iter().any(|r| r.signed_at.is_some())`. It also fixes the Recent visits label.
- **An optional clinic rule, in phase 2.** Settings gains "Keep signed visits for at least N years" (off, 3, 5, 7 or 10). While it's on, a signed visit can't be deleted, or removed early from Deleted visits, until N years after its last signature or addendum. A veterinarian sets it with their PIN, and changes are recorded. The interface shows no jurisdiction's number; the guide lists examples with sources.

### 3. Deleted visits: an in-app trash, not the operating system's

- **Layout.** OpenBark keeps its own housekeeping in one reserved folder inside the visits folder: `<visits>/.openbark/`.
  - Deleted visits go to `.openbark/deleted/<trash id>/`. The trash id is the visit's `meta.id`, with a suffix if that name is taken.
  - The deletion record is `.openbark/audit.log`.
  - The phase 2 clinic rule is `.openbark/policy.json`.
  - It sits on the same volume as the visits, so a move is one atomic rename. The patient's name leaves the folder name at once.
- **Deleting is a rename.** The visit folder is renamed into `.openbark/deleted/`. An encrypted `deleted.json` is written beside `session.json`, holding:
  - the original folder name;
  - when it was deleted, by whom and why;
  - whether it was signed;
  - `purge_after`, 30 days later;
  - the id of the deletion record entry.
- **Order and rollback.** The steps are: rename, write `deleted.json`, then add the deletion record entry. If the entry can't be written, the rename is undone and the person sees "OpenBark couldn't record the deletion, so the visit was kept."
- **Recovery at start.** On start, OpenBark checks Deleted visits:
  - An item without `deleted.json` (a crash mid-delete) gets one with a fresh 30 days.
  - An item whose entry is missing from the deletion record gets one, marked as recovered.
- **Restore** renames the folder back. If the original name is taken it adds a suffix, then removes `deleted.json`, records the restore and re-links the patients (`link_visit`).
- **Removal for good** happens hourly while OpenBark runs, and once at start. It also happens when someone chooses **Delete now**. The steps are:
  1. rename the item to `<trash id>.purging`;
  2. record the removal;
  3. remove the folder.

  A `.purging` folder left by a crash is removed at the next start, and its entry is added then if it's missing. Removal holds a work ticket and skips while a folder change is under way.
- **Files in use.** On Windows, a rename or removal can fail while another program holds a file open (antivirus, an Explorer window). OpenBark retries briefly, then says "Another program is using a file in this visit. Close any window showing it, then try again." A half-deleted visit is never left visible.
- **Why 30 days.** It's long enough to catch a mistake made before a weekend or holiday. It's short enough that Deleted visits doesn't become a second archive. It matches what clinic staff know from email and file recycle bins.

### 4. Deleting only the recording

- Removes `audio.wav` and any leftover `audio.wav.tmp`, at once, after the dialog.
- `session.json` gains `audio_deleted: {at, by}`. `audio_sha256`, `audio_secs` and the source label stay, so the length still shows and a later copy can still be matched to this visit.
- Review replaces the player with "Recording deleted {date} by {name}. The transcript and notes are kept." Evidence links still highlight the transcript line; they just don't play.
- Playback (`session_audio`) and the CLI's `--retranscribe` refuse with a plain message. Reprocess already drafts from the transcript, so it keeps working.
- On an imported visit the dialog adds: "The file you imported, {file}, is still where it was. Delete it there too if you need to."
- ADR-0017 section 7 needs the same "no audio here" state for synced visits ("The audio is on that phone"). Both are built once.
- Deleting recordings automatically after signing is not part of this ADR. It needs its own decision, as ADR-0018 and M3 already say for phones.

### 5. Multi-pet visits

- One folder and one recording cover every animal, so v1 deletes whole visits only.
- The reason menu and the dialogs name the other animals: "This visit also has notes for Pickles. They're deleted with it."
- A signed note for any animal makes the whole visit signed.
- Removing one animal from a visit ("This visit isn't about Pickles") is a correction, not a deletion. It means unlinking the animal and marking its runs as entered in error, and it needs its own decision.

### 6. The deletion record (audit log)

- **One file:** `<visits>/.openbark/audit.log`. It moves with the visits (ADR-0013), and backups cover it.
- **One event per line.** Each line is encrypted on its own through a new `vault::append_line`: `OBL1:` then base64 of the nonce and ciphertext. The magic and file name are the associated data. Lines are written with a single append and then synced to disk. With encryption off (tests, or if the vault failed to start), lines are plain JSON.
- **Hash-chained.** Each event carries `seq` and `prev`, the SHA-256 of the previous line as stored. The first line is a `log_started` event. A missing, reordered or edited line breaks the chain, and **Check the record** finds it.
- **Events in v1:** `log_started`, `visit_deleted`, `visit_restored`, `visit_purged` (by hand or after 30 days), `recording_deleted`. Phase 2 adds `policy_changed`. Sync adds its own events later (section 10).
- **What an entry holds** (a tombstone):
  - the event id (UUIDv7) and time;
  - who acted, with their role, and whether OpenBark ran single-user or with a team;
  - the reason;
  - the app version;
  - the visit's `meta.id`, the trash id, when the visit was created, and who recorded it;
  - the patient record ids (not names), and how many animals the visit covered;
  - whether it was a live recording or an import (never the file path, which can contain names);
  - the consent method;
  - `audio_sha256` and `audio_secs`;
  - for each signed run: the animal's position in the visit, the signer, when it was signed, how many addenda it had, and the SHA-256 of its `note.md`.

  It holds no transcript text, no note text and no patient names. Names are looked up from `patients.json` when a veterinarian views the record, and patient records are never deleted.
- **Kept forever.** Entries are never removed, including after the visit is gone. A note hash lets a clinic show later that a PIMS copy or PDF matches what was signed.
- **Honest limits.** The record is tamper-evident, not tamper-proof. Anyone with the data key could rewrite the whole chain, or delete the file. If the desktop app and `openbark serve` write to one folder at the same moment, the chain can fork. The check reports that as "two copies wrote at once", not as tampering.
- **Who sees it.** Veterinarians, in Settings under Deleted visits. Single-user mode shows it to the one user.

### 7. The work gate and a per-visit lock

- Delete, restore, removal and recording deletion all take a `begin_work()` ticket, so none of them races a folder change.
- `App` gains a per-visit lock: a set of visit ids, each with what holds it. It is held by:
  - import, while its new folder exists;
  - a live recording;
  - draft, refine, edit, sign, addendum and patient-detail changes;
  - every delete action.
- A delete refuses in plain words: "This visit is being drafted. Delete it when that's finished." or "This visit is still recording. Stop the recording first."
- Two drafts of one visit from two windows are refused too. Today they can both take the same run number.
- Listing skips a visit that disappears mid-listing, instead of failing the whole patient list.
- The lock is per process. Two programs on one folder can still race. That is rare and is written down in the guide.

### 8. What deletion achieves, stated honestly

There are no per-visit keys and no overwrite pass in v1.

| Where a copy can remain | After "Delete now", or 30 days in Deleted visits | Why |
|---|---|---|
| OpenBark's screens and API | Gone | The files are removed |
| Free space on this computer's disk | Encrypted fragments may remain until overwritten | SSD wear levelling and copy-on-write file systems. They can't be read without the data key |
| Backups: File History, shadow copies, Time Machine, the clinic's own | Encrypted copies remain until those backups expire | Outside OpenBark. The recovery key can open them |
| A cloud-synced visits folder (allowed by ADR-0013, with a notice) | An encrypted copy in that service's recycle bin, for weeks | Outside OpenBark |
| The PIMS, saved PDFs, pasted text, the original imported file | Unaffected | OpenBark never controlled them |
| Visits saved before encryption | Plain-text fragments may remain on the disk | Encrypting them afterwards doesn't remove the old fragments |
| The deletion record | The entry stays, with no clinical text | By design |

- The interface says: "OpenBark removes its files now, and it can't be restored. Backups of this computer may still hold encrypted copies, which the recovery key can open."
- The guide explains the table.
- **Crypto-shredding is deferred.** Per-visit keys stored in the visit folder wouldn't help, because the leftover fragments include the wrapped key, and the data key unwraps it. Real shredding needs keys held outside the folder, plus rotating the data key. Rotation would invalidate the printed recovery key (ADR-0008 section 3). This is revisited with ADR-0017's per-object keys.

### 9. Folder detection and storage

- `locations::contents` treats `.openbark` as OpenBark's own, with tests. Otherwise a visits folder holding deleted visits would look foreign when a clinic switches to it on a new computer.
- Moving the visits folder carries `.openbark` along, which is correct.
- **Switch to a folder** uses that folder's own Deleted visits and record.
- Settings > Storage shows the space Deleted visits use separately.
- `session_dir` already refuses ids that start with a dot. `session::list` only reads folders that hold a `session.json` directly, so deleted visits never appear as visits.
- Opening a visit that was just deleted (another window, an old link) says "This visit was deleted. Restore it from Deleted visits in Settings."

### 10. Future sync (ADR-0017, M3 phase 5)

| Local action (this ADR) | With encrypted sync on |
|---|---|
| Delete visit | A synced visit asks where. **Delete from this computer** is local only, and it writes a suppression record in `.openbark/` so the next pull doesn't download the visit again. **Delete everywhere** is for veterinarians only, uses this same dialog and reason, and needs the second confirmation for signed visits |
| Delete everywhere, received from another device | The receiving device moves its copy to its own Deleted visits for the same 30 days, and appends the event to its own deletion record with the sending device's id |
| Restore | Sends a restore event while the 30 days last. After the cloud copy is purged, the restoring device uploads its copy again |
| Delete now, removal after 30 days | Local only. The cloud copy goes through ADR-0017's tombstone and `purge` function |
| Delete recording | Local only, since audio never syncs. `audio_deleted` syncs in the visit header, so other devices show "Recording deleted" |
| The deletion record | Every event is keyed on `meta.id`, never the folder name. The `visit_deleted` and `visit_restored` entries become ADR-0017's immutable deletion event objects |

- The iPhone app (ADR-0018) uses the same `os-app` rules.
- The Clinic Hub (M4) replaces PIN-based identity in `by`.
- **A caveat for ADR-0017.** "Deleting the wrapped DEKs crypto-shreds any copies left in backups" holds only if the wrapped keys inside Supabase's seven daily backups can't be unwrapped. That needs a space-key rotation, or waiting for the backups to expire. The sync phase has to settle this before "Delete everywhere" ships.

### 11. Interface

- **Review, visit details.** **Delete visit**, a destructive button beside **Edit details**. It shows whether or not a draft exists. Reception doesn't see it. A technician who opens a signed visit sees it disabled, with the reason in its tooltip.
- **Review, transcript pane.** **Delete recording**, under the player.
- **Settings, Deleted visits.** A new section after Storage. Each row has the patient, the visit date, who deleted it and why, and when it will be removed for good, with **Restore** and **Delete now**. Veterinarians also see **Deletion record** and **Check the record**.
- **Patient page.** A line under the visit list when that patient has deleted visits: "1 deleted visit is kept for 30 days. Restore it from Deleted visits in Settings."
- **No delete icon on list rows** (patient page, Recent visits). Rows already carry Reprocess. Deleting from Review means the person has seen the visit first.
- **Toasts** gain an optional action, used for Undo. The timer pauses while the pointer or keyboard focus is on the toast.
- **Look and wording.** Lucide `Trash2` for the delete buttons and the destructive button style from the brand guide (section 7.2). Every string goes through `t()` and the four translation files. The plan lists the exact wording.

### 12. API

| Tauri command | Web route (`X-OpenBark` required on every POST) | Body or query |
|---|---|---|
| `delete_visit` | `POST /api/sessions/{id}/delete` | `{by, reason: {kind, note}, pin?, confirm_signed?}` |
| `delete_recording` | `POST /api/sessions/{id}/recording/delete` | `{by}` |
| `list_deleted` | `GET /api/deleted?by=` | |
| `restore_visit` | `POST /api/deleted/{tid}/restore` | `{by}` |
| `purge_deleted` | `POST /api/deleted/{tid}/purge` | `{by, pin?}` |
| `deletion_record` | `GET /api/audit?by=` | Veterinarians only. Includes the chain check |

- The PIN travels only in a request body, never in a URL, and is never logged.
- Trash ids are validated like session ids, and only UUID characters are accepted.

### 13. Phases

| Phase | Contents |
|---|---|
| 1 | The housekeeping folder and the `contents` fix; `any_signed` and the Recent visits fix; `vault::append_line` and the deletion record; the per-visit lock; Deleted visits with Undo, restore and removal; deleting unsigned visits; deleting recordings; the Settings section; translations; guide. Signed visits are refused in phase 1 with "Signed visits can't be deleted yet. Add an addendum to correct the note." |
| 2 | Deleting signed visits (veterinarian, PIN, reason, two-step dialog); the deletion record view and check; the optional "Keep signed visits for at least N years" rule |
| 3 | M3 phase 5: "Delete from this computer", "Delete everywhere" and restore events, mapped as in section 10 |

## Consequences

- Clinics can remove test recordings, mistakes and duplicates themselves, and get them back for 30 days.
- Signed records are harder to lose than drafts. Corrections still go through addenda, and every deletion leaves a dated, attributed entry with the signed note's hash.
- Deleting a recording becomes the normal privacy step after signing, without touching the record.
- The visits folder gains one reserved folder, `.openbark`. Folder detection, moving and storage sizes must all know about it, and tests guard each one.
- The deletion record is the first piece of ADR-0017's audit log. Sync events reuse its format.
- Deleted visits take disk space for 30 days. Settings shows how much.
- The per-visit lock also fixes two existing races: two drafts at once, and a draft recreating a deleted folder.
- Wording about what deletion achieves has to stay honest everywhere: the interface, the guide and the website. OpenBark never claims deleted data is unrecoverable from backups.
- PIN checks are still a convenience on a shared computer, not authentication. Anyone with the OS account can read the files. The Clinic Hub (M4) brings real sign-in.
- Docs to update when this is built: `docs/guide/review.md`, `docs/guide/team-and-security.md`, the API list in the `os-server` module doc, and M3 phase 5, which gets a pointer here. No new dependency, so `docs/versions.md` doesn't change. No network call, so `docs/network.md` doesn't change.

## Alternatives considered

| Option | Why not |
|---|---|
| Delete at once after a confirmation, with no Deleted visits | Mistakes can't be undone, and this is clinical data. The design guide prefers Undo to "are you sure" for anything that can be reversed |
| The operating system's recycle bin (the `trash` crate) | A new dependency. Network and USB drives have no recycle bin. OpenBark couldn't track, list, remove or record what's in it. A restore from Explorer would skip the record. The folder name, with the patient's name, would sit in the bin |
| A "deleted" flag in `session.json` that hides the visit | Every listing would need a filter, and one missed filter shows the visit again. The folder name with the patient's name stays in the visits folder |
| Never allow deleting signed visits | OpenBark isn't the record of truth, and there are real reasons to delete. A refusal pushes clinics to delete folders by hand, with no record. The phase 2 clinic rule lets a clinic choose this for itself |
| A dialog for every visit delete | Against the design guide (dialogs only for irreversible actions), and it trains people to click through. The reason menu plus Undo is quicker and still records why |
| Per-visit keys and crypto-shredding now | The leftover fragments include the wrapped key, which the data key opens. Real shredding needs keys held elsewhere and a data-key rotation that breaks the printed recovery key. Revisit with ADR-0017's per-object keys |
| Overwrite files before deleting | Not reliable on SSDs or copy-on-write file systems, slow for long recordings, and it would promise more than it delivers |
| Keep the deletion record in each visit, or in `patients.json` | A record inside the visit disappears with it. `patients.json` is rewritten whole, so it can't be appended to safely |
| A plain-text deletion record | It would put staff names and reasons on disk unencrypted |
| Delete one animal from a multi-pet visit | That's a correction. It needs its own design: unlink the animal and mark its runs as entered in error |
| Delete recordings automatically after signing | A policy decision with clinical trade-offs. It gets its own ADR when a clinic asks |
| An `openbark sessions delete` command for administrators | It would skip roles and the interface's warnings. It can come later, built on the same `os-app` calls |

## Sources

- CVO, Professional Practice Standard: Medical Records: https://www.cvo.org/standards/medical-records-professional-practice-standard
- CVBC, medical record retention: https://cvbc.ca/practice-facilities-homepage/medical-record-retention/
- Texas, 22 TAC 573.52: https://law.cornell.edu/regulations/texas/22-Tex-Admin-Code-SS-573-52
- California VMB FAQ (secondary for 16 CCR 2032.3): https://www.vmb.ca.gov/consumers/faqs.shtml
- AVMA summary of record retention laws (2008): https://www.avma.org/javma-news/2008-02-01/report-summarizes-laws-how-long-keep-patient-records
- RCVS Supporting Guidance, chapter 13: https://www.rcvs.org.uk/veterinary-professionals/conduct-and-guidance/supporting-guidance/13-clinical-and-client-records
- OMA, AI scribe guidance: https://oma.org/siteassets/oma/media/pagetree/pps/running/operations/cybersecurity/ai-scribe-guidance-tips.pdf
- NIST SP 800-88 Rev. 2: https://csrc.nist.gov/pubs/sp/800/88/r2/final
