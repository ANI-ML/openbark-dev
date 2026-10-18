# ADR-0011: Patients, with visits under each patient

- Status: accepted (built)
- Date: 2026-10-18
- Deciders: product owner (asked for "Patients and visits under each patient" in place of a flat list of encounters), engineering

## Context

Until now OpenBark kept a flat list of **encounters**: one row per recording, with the animal's name typed into each one. A doctor looking for "everything we've done for Maple" had to scan by eye, and a typo split one animal into two. The product owner asked for the way a clinic thinks about its work: patients first, then each patient's visits.

Constraints:
- **Local-first and encrypted.** Whatever links visits to patients is clinic data, so it is encrypted at rest like the visits (ADR-0008) and never leaves the computer.
- **OpenBark is not the PIMS.** The practice management system holds the real patient record. OpenBark needs just enough to group visits: the animal's name, the family name and the species. No medical history, no client contact details.
- **Existing visits must keep working**, with no migration step for the user.
- **Multi-pet visits** (one recording covering several animals) must appear under every animal in them.

## Decision

1. **A patient registry**, `PatientStore`, in `<sessions>/patients.json`, encrypted like the visits. A record is an id, the animal's name, the family name, the species, and when it was created and updated.
2. **Visits link to patients by id.** `PatientInfo` gains `patient_id`, and `other_ids` for the other animals in a multi-pet visit. Whenever a visit is saved (recorded, imported or edited), `App::link_visit` links it: to the id it carries if that patient exists, otherwise to a record found by name and family name (`match_key`: case, spacing and accents don't matter), otherwise to a new record.
3. **Older visits link themselves lazily.** They carry only the names typed at the time. The first time patients are listed, each one is matched or added and the id written into the visit. Nothing for the user to do.
4. **A visit keeps what was entered at the time.** Editing a patient's details (a corrected name, a species added later) changes the patient record, not past visits' notes. Changing the name or family name on a visit makes it a different patient; the Recorder clears the link when you do.
5. **The screens:**
   - **Patients** replaces Encounters in the sidebar: one row per animal, most recently seen first, with a filter, and **Recent visits** a tap away for the day's list.
   - **A patient's page** lists every visit, newest first, with its type icon, reason, date, length and status, plus **Edit details** and **New visit**.
   - **Record** starts with **Find a patient**. Picking one fills in the details and links the visit.
   - Opening a multi-pet visit from one animal's page shows that animal's note first.
6. **The word "encounter" leaves the interface.** Screens, notices and the guide say visit. The engine and its files keep the old names (`sessions`, `SessionSummary`), since renaming stored data buys nothing.

API: `list_patients`, `patient(id)`, `create_patient`, `update_patient_record` (Tauri commands; `GET`/`POST /api/patients` and `/api/patients/{id}` in web mode). Imports and live recordings take an optional `patient_id`.

## Consequences

- One animal, one row, even if some visits were recorded before the registry existed.
- Two different animals with the same name and family name merge into one patient. That is rare within a clinic, and choosing the right patient in **Find a patient** avoids it. If it becomes a problem, a later change can add merge and split, or carry the PIMS patient number, which the browser extension sees.
- Deleting a patient isn't offered yet. A record with no visits is harmless, and deleting clinic data needs its own careful flow, the same as for visits. Deleting visits: [ADR-0022](0022-deleting-visits.md).
- The registry is one more encrypted file in the sessions folder. Backups and the recovery key cover it with no extra steps.
