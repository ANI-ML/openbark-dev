//! The deletion record (ADR-0022 section 6): `<visits>/.openbark/audit.log`.
//!
//! Deleting is the first action that leaves nothing behind in the visit
//! itself, so every deletion, restore and removal adds one line here. Each
//! line is one event, encrypted on its own ([`crate::vault::append_line`]),
//! and hash-chained: `prev` is the SHA-256 of the previous line as stored,
//! `seq` counts up from the first line, which is always `log_started`. A
//! missing, reordered or edited line breaks the chain, and [`AuditLog::verify`]
//! finds it.
//!
//! An entry is a tombstone: who acted, when and why, and enough about the
//! visit to recognise it later (its id, patient record ids, the recording's
//! hash, each signed note's hash). It never holds transcript text, note text
//! or patient names.
//!
//! Honest limits: the record is tamper-evident, not tamper-proof. Anyone with
//! the data key could rewrite the whole chain, or delete the file. Two
//! programs writing to one folder at the same moment fork the chain; the
//! check reports that as [`ChainReport::Forked`], not as tampering.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::EngineError;
use crate::session::{ConsentMethod, Session};
use crate::users::Who;

/// What happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    LogStarted,
    VisitDeleted,
    VisitRestored,
    VisitPurged,
    RecordingDeleted,
    /// Phase 2: the clinic's retention rule changed.
    PolicyChanged,
}

/// Why a visit was deleted (the reason menu).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasonKind {
    /// Test or practice recording.
    Test,
    /// Recorded by mistake.
    Mistake,
    /// Duplicate visit.
    Duplicate,
    /// Client withdrew consent.
    ConsentWithdrawn,
    /// Another reason.
    Other,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeleteReason {
    pub kind: ReasonKind,
    /// A few words (required for signed visits in phase 2). No clinical details.
    #[serde(default)]
    pub note: String,
}

/// How a deleted visit was removed for good.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Purge {
    /// "Delete now", from Deleted visits.
    #[serde(rename = "by_hand")]
    ByHand,
    /// Removed by OpenBark after 30 days.
    #[serde(rename = "after_30_days")]
    After30Days,
}

/// A live recording or an imported file (never the file's path, which can
/// contain names).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    Live,
    File,
}

/// A signed note, as the record keeps it once the visit is gone.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SignedRun {
    /// The animal's position in the visit (0: the main animal), never its name.
    pub animal: usize,
    pub signer: Who,
    pub signed_at: DateTime<Local>,
    pub addenda: usize,
    /// SHA-256 of the decrypted `note.md`, so a PIMS copy or PDF can be
    /// matched to what was signed.
    pub note_sha256: String,
}

/// What the record keeps about a visit: enough to recognise it, no clinical text.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tombstone {
    /// The visit's `meta.id` (never the folder name, which holds the patient's name).
    pub visit_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trash_id: Option<String>,
    pub created_at: DateTime<Local>,
    /// Patient record ids (names are looked up when the record is viewed).
    #[serde(default)]
    pub patient_ids: Vec<String>,
    pub animals: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recorded_by: Option<Who>,
    pub source: SourceKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consent: Option<ConsentMethod>,
    pub audio_sha256: String,
    pub audio_secs: f32,
    pub drafts: usize,
    #[serde(default)]
    pub signed: Vec<SignedRun>,
}

/// One line of the record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub id: String,
    pub seq: u64,
    /// SHA-256 (hex) of the previous line as stored; 64 zeros for the first.
    pub prev: String,
    pub at: DateTime<Local>,
    pub kind: EventKind,
    pub by: Who,
    /// Whether OpenBark ran with a team (otherwise single-user, honour-based).
    pub team: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visit: Option<Tombstone>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<DeleteReason>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub purge: Option<Purge>,
    /// Written at start after OpenBark closed unexpectedly mid-action.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub recovered: bool,
    pub app_version: String,
}

/// An event before it's appended: [`AuditLog::append`] fills `seq`, `prev`
/// and `at` (and `id` when not given).
#[derive(Debug, Clone)]
pub struct EventDraft {
    /// Chosen in advance when something else must name the entry first
    /// (`deleted.json` is written before the entry).
    pub id: Option<String>,
    pub kind: EventKind,
    pub by: Who,
    pub team: bool,
    pub visit: Option<Tombstone>,
    pub reason: Option<DeleteReason>,
    pub purge: Option<Purge>,
    pub recovered: bool,
}

impl EventDraft {
    pub fn new(kind: EventKind, by: Who, team: bool) -> Self {
        Self { id: None, kind, by, team, visit: None, reason: None, purge: None, recovered: false }
    }
}

/// What [`AuditLog::verify`] found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum ChainReport {
    /// Every entry follows the one before it.
    Complete { entries: usize },
    /// Complete, but two copies of OpenBark wrote at the same moment (at
    /// entry `at`), so two entries follow the same one. Harmless.
    Forked { entries: usize, at: u64 },
    /// Line `at` (counting from 1) doesn't follow the one before it, or
    /// can't be read: the record may have been changed outside OpenBark.
    Broken { at: u64 },
}

const ZEROS: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// One append at a time in this process (ADR-0022 section 6).
static APPEND: Mutex<()> = Mutex::new(());

/// A fresh event id (UUIDv7).
pub fn new_id() -> String {
    uuid::Uuid::now_v7().to_string()
}

fn line_hash(stored: &str) -> String {
    hex::encode(Sha256::digest(stored.as_bytes()))
}

/// The deletion record of one visits folder.
#[derive(Debug, Clone)]
pub struct AuditLog {
    path: PathBuf,
}

impl AuditLog {
    pub fn for_sessions(root: &Path) -> Self {
        Self { path: crate::audit_path(root) }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn fail(&self, e: impl std::fmt::Display) -> EngineError {
        EngineError::Session(format!("{}: {e}", self.path.display()))
    }

    /// The non-empty lines as stored (not decrypted).
    fn stored_lines(&self) -> Result<Vec<String>, EngineError> {
        match std::fs::read_to_string(&self.path) {
            Ok(text) => Ok(text
                .lines()
                .map(|l| l.trim_end_matches('\r').to_string())
                .filter(|l| !l.is_empty())
                .collect()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(self.fail(e)),
        }
    }

    fn parse(&self, stored: &str) -> Result<Event, EngineError> {
        let plain = crate::vault::decrypt_line(&self.path, stored).map_err(|e| self.fail(e))?;
        serde_json::from_slice(&plain).map_err(|e| self.fail(e))
    }

    fn write(&self, event: &Event) -> Result<String, EngineError> {
        let plain = serde_json::to_vec(event).expect("events serialise");
        crate::vault::append_line(&self.path, &plain).map_err(|e| self.fail(e))
    }

    /// Add an entry. The first entry in a new record is preceded by
    /// `log_started`. Fails (and nothing is added) if the file can't be
    /// written; the caller then undoes the action.
    ///
    /// An entry whose id was chosen in advance is added once: if the record
    /// already holds it, that entry is returned and nothing is added. A
    /// delete and the start-up recovery can both try to record the same
    /// deletion (housekeeping runs on its own thread), and two entries with
    /// one id would read as a changed record.
    pub fn append(&self, draft: EventDraft) -> Result<Event, EngineError> {
        let _one = APPEND.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| self.fail(e))?;
        }
        let lines = self.stored_lines()?;
        if let Some(id) = &draft.id
            && let Some(done) = lines.iter().filter_map(|l| self.parse(l).ok()).find(|e| e.id == *id)
        {
            return Ok(done);
        }
        let (seq, prev) = match lines.last() {
            Some(last) => {
                // A damaged last line still chains: its hash, and the line count.
                let seq = self.parse(last).map(|e| e.seq).unwrap_or(lines.len() as u64);
                (seq, line_hash(last))
            }
            None => {
                let started = Event {
                    id: new_id(),
                    seq: 1,
                    prev: ZEROS.into(),
                    at: Local::now(),
                    kind: EventKind::LogStarted,
                    by: draft.by.clone(),
                    team: draft.team,
                    visit: None,
                    reason: None,
                    purge: None,
                    recovered: false,
                    app_version: env!("CARGO_PKG_VERSION").into(),
                };
                let stored = self.write(&started)?;
                (1, line_hash(&stored))
            }
        };
        let event = Event {
            id: draft.id.unwrap_or_else(new_id),
            seq: seq + 1,
            prev,
            at: Local::now(),
            kind: draft.kind,
            by: draft.by,
            team: draft.team,
            visit: draft.visit,
            reason: draft.reason,
            purge: draft.purge,
            recovered: draft.recovered,
            app_version: env!("CARGO_PKG_VERSION").into(),
        };
        self.write(&event)?;
        Ok(event)
    }

    /// Every entry, oldest first.
    pub fn read(&self) -> Result<Vec<Event>, EngineError> {
        self.stored_lines()?.iter().map(|l| self.parse(l)).collect()
    }

    /// Every entry that can be read, oldest first, leaving out lines that
    /// can't (a line torn by a crash; [`AuditLog::verify`] still reports
    /// it). Fails when the record has lines but none can be read, such as
    /// another computer's key: nothing should be added to it blindly.
    pub fn read_readable(&self) -> Result<Vec<Event>, EngineError> {
        let lines = self.stored_lines()?;
        let events: Vec<Event> = lines.iter().filter_map(|l| self.parse(l).ok()).collect();
        match lines.first() {
            Some(first) if events.is_empty() => {
                Err(self.parse(first).err().unwrap_or_else(|| self.fail("unreadable")))
            }
            _ => Ok(events),
        }
    }

    /// Check the chain ("Check the record").
    pub fn verify(&self) -> Result<ChainReport, EngineError> {
        let lines = self.stored_lines()?;
        let mut hashes: Vec<String> = Vec::with_capacity(lines.len());
        let mut seqs: Vec<u64> = Vec::with_capacity(lines.len());
        let mut ids = std::collections::HashSet::new();
        let mut fork: Option<u64> = None;
        for (i, stored) in lines.iter().enumerate() {
            let at = i as u64 + 1;
            let Ok(event) = self.parse(stored) else { return Ok(ChainReport::Broken { at }) };
            if !ids.insert(event.id.clone()) {
                return Ok(ChainReport::Broken { at });
            }
            if i == 0 {
                if event.kind != EventKind::LogStarted || event.prev != ZEROS || event.seq != 1 {
                    return Ok(ChainReport::Broken { at });
                }
            } else if event.prev == hashes[i - 1] && event.seq == seqs[i - 1] + 1 {
                // Follows the line before it.
            } else if let Some(j) = hashes[..i - 1].iter().rposition(|h| *h == event.prev)
                && event.seq == seqs[j] + 1
            {
                // Follows an earlier line: another copy wrote in between.
                fork.get_or_insert(event.seq);
            } else {
                return Ok(ChainReport::Broken { at });
            }
            hashes.push(line_hash(stored));
            seqs.push(event.seq);
        }
        let entries = lines.len();
        Ok(match fork {
            Some(at) => ChainReport::Forked { entries, at },
            None => ChainReport::Complete { entries },
        })
    }

    /// Whether the record holds an entry with this id.
    pub fn contains(&self, event_id: &str) -> bool {
        self.stored_lines()
            .map(|lines| lines.iter().filter_map(|l| self.parse(l).ok()).any(|e| e.id == event_id))
            .unwrap_or(false)
    }
}

/// What the record keeps about a visit. `animal` is each signed run's
/// position in [`crate::session::PatientInfo::pets`], never its name; the note
/// hash is of the decrypted `note.md`.
pub fn tombstone(s: &Session, trash_id: Option<&str>) -> Tombstone {
    let patient = s.meta.patient.as_ref();
    let pets = patient.map(|p| p.pets()).unwrap_or_default();
    let animal_of = |pet: &Option<String>| {
        pet.as_deref().and_then(|p| pets.iter().position(|x| x.eq_ignore_ascii_case(p.trim()))).unwrap_or(0)
    };
    let signed = s
        .meta
        .runs
        .iter()
        .filter_map(|r| {
            let signed_at = r.signed_at?;
            let note = s.dir.join(&r.dir).join("note.md");
            let note_sha256 =
                crate::vault::read(&note).map(|b| hex::encode(Sha256::digest(&b))).unwrap_or_default();
            Some(SignedRun {
                animal: animal_of(&r.pet),
                signer: r
                    .signer
                    .clone()
                    .unwrap_or_else(|| Who::named(r.signed_by.as_deref().unwrap_or_default())),
                signed_at,
                addenda: r.addenda.len(),
                note_sha256,
            })
        })
        .collect();
    let patient_ids = patient
        .map(|p| p.patient_id.iter().chain(p.other_ids.iter()).filter(|id| !id.is_empty()).cloned().collect())
        .unwrap_or_default();
    Tombstone {
        visit_id: s.meta.id.clone(),
        trash_id: trash_id.map(String::from),
        created_at: s.meta.created_at,
        patient_ids,
        animals: pets.len().max(1),
        recorded_by: s.meta.recorded_by.clone(),
        source: if s.meta.source.starts_with("live recording") { SourceKind::Live } else { SourceKind::File },
        consent: s.meta.consent.as_ref().map(|c| c.method),
        audio_sha256: s.meta.audio_sha256.clone(),
        audio_secs: s.meta.audio_secs,
        drafts: s.meta.runs.len(),
        signed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::faithfulness::FaithfulnessReport;
    use crate::session::PatientInfo;
    use crate::template::LoadedTemplate;
    use crate::transcript::{Segment, Transcript};
    use crate::{NoteResult, vault};

    /// A visit for Olive Haddad with Pickles, transcribed, drafted and signed.
    fn seeded(root: &Path) -> Session {
        let mut s = Session::create_live(root, "olive").unwrap();
        s.set_patient(PatientInfo {
            name: "Olive".into(),
            others: vec!["Pickles".into()],
            last_name: "Haddad".into(),
            reason: "Itching".into(),
            patient_id: Some("p-olive".into()),
            other_ids: vec!["p-pickles".into()],
            ..Default::default()
        })
        .unwrap();
        let t = Transcript {
            segments: vec![Segment {
                id: 0,
                start: 0.0,
                end: 1.0,
                speaker: Some(0),
                text: "Scratching behind the ears since Tuesday".into(),
                hints: Vec::new(),
                check: None,
            }],
        };
        s.save_transcript(&t, "parakeet", false, &Default::default()).unwrap();
        let result = NoteResult {
            status: "draft",
            template: "soap@0.1.0".into(),
            model: "test".into(),
            note: serde_json::json!({}),
            faithfulness: FaithfulnessReport { passed: true, ..Default::default() },
            stats: Default::default(),
            refinement: None,
            edit: None,
            by: None,
            template_defaults: Vec::new(),
            markdown: "# SOAP note\n- Otitis externa suspected\n".into(),
        };
        let mut pickles = s.clone().for_pet(Some("Pickles"));
        pickles.save_run("gemma", &result, &LoadedTemplate::builtin("soap").unwrap()).unwrap();
        pickles.sign_latest("Dr Rivera").unwrap();
        Session::open(&pickles.dir).unwrap()
    }

    fn draft(kind: EventKind) -> EventDraft {
        EventDraft::new(kind, Who::named("Sam"), false)
    }

    #[test]
    fn appends_chain_and_read_back() {
        vault::tests::with_key(|| {
            let tmp = tempfile::tempdir().unwrap();
            let root = tmp.path().join("visits");
            let s = seeded(&root);
            let log = AuditLog::for_sessions(&root);
            assert_eq!(log.verify().unwrap(), ChainReport::Complete { entries: 0 });
            let mut d = draft(EventKind::VisitDeleted);
            d.visit = Some(tombstone(&s, Some("t-1")));
            d.reason = Some(DeleteReason { kind: ReasonKind::Mistake, note: String::new() });
            let first = log.append(d).unwrap();
            log.append(draft(EventKind::VisitRestored)).unwrap();
            let mut purged = draft(EventKind::VisitPurged);
            purged.purge = Some(Purge::After30Days);
            log.append(purged).unwrap();
            let raw = std::fs::read_to_string(log.path()).unwrap();
            assert!(raw.lines().all(|l| l.starts_with("OBL1:")), "{raw}");
            assert_eq!(log.verify().unwrap(), ChainReport::Complete { entries: 4 });
            let events = log.read().unwrap();
            assert_eq!(events[0].kind, EventKind::LogStarted);
            assert_eq!(events[1], first);
            assert_eq!(events[1].seq, 2);
            assert_eq!(events[3].purge, Some(Purge::After30Days));
            assert!(log.contains(&first.id) && !log.contains("nope"));
            // The signed run: the animal's position, never its name; the note's hash.
            let signed = &events[1].visit.as_ref().unwrap().signed;
            assert_eq!(signed.len(), 1);
            assert_eq!(signed[0].animal, 1, "Pickles is the second animal");
            assert_eq!(signed[0].signer.name, "Dr Rivera");
            let md = vault::read(&s.dir.join(&s.meta.runs[0].dir).join("note.md")).unwrap();
            assert_eq!(signed[0].note_sha256, hex::encode(Sha256::digest(&md)));
            let tomb = events[1].visit.as_ref().unwrap();
            assert_eq!(tomb.patient_ids, ["p-olive", "p-pickles"]);
            assert_eq!(tomb.animals, 2);
            // No names, no transcript, no note text: not on disk, not even decrypted.
            for word in ["Olive", "Haddad", "Pickles", "Scratching", "Otitis", "Itching"] {
                assert!(!raw.contains(word), "{word} on disk");
            }
            let plain: String = vault::read_lines(log.path())
                .unwrap()
                .iter()
                .map(|(_, p)| String::from_utf8_lossy(p).into_owned())
                .collect();
            for word in ["Olive", "Haddad", "Pickles", "Scratching", "Otitis"] {
                assert!(!plain.contains(word), "{word} in the decrypted record");
            }
        });
    }

    fn four_lines(log: &AuditLog) -> Vec<String> {
        for kind in [EventKind::VisitDeleted, EventKind::VisitRestored, EventKind::RecordingDeleted] {
            log.append(draft(kind)).unwrap();
        }
        log.stored_lines().unwrap()
    }

    #[test]
    fn a_missing_swapped_or_edited_line_breaks_the_chain() {
        vault::tests::with_key(|| {
            let tmp = tempfile::tempdir().unwrap();
            let log = AuditLog::for_sessions(tmp.path());
            let lines = four_lines(&log);
            assert_eq!(lines.len(), 4);
            let write = |ls: &[&String]| {
                let text: String = ls.iter().map(|l| format!("{l}\n")).collect();
                std::fs::write(log.path(), text).unwrap();
            };
            write(&[&lines[0], &lines[2], &lines[3]]);
            assert_eq!(log.verify().unwrap(), ChainReport::Broken { at: 2 }, "line 2 deleted");
            write(&[&lines[0], &lines[2], &lines[1], &lines[3]]);
            assert_eq!(log.verify().unwrap(), ChainReport::Broken { at: 2 }, "lines 2 and 3 swapped");
            let mut edited = lines[1].clone();
            let mid = edited.len() / 2;
            let c = if &edited[mid..mid + 1] == "A" { "B" } else { "A" };
            edited.replace_range(mid..mid + 1, c);
            write(&[&lines[0], &edited, &lines[2], &lines[3]]);
            assert_eq!(log.verify().unwrap(), ChainReport::Broken { at: 2 }, "one character edited");
            write(&[&lines[1], &lines[2], &lines[3]]);
            assert_eq!(log.verify().unwrap(), ChainReport::Broken { at: 1 }, "log_started removed");
            write(&[&lines[0], &lines[1], &lines[1], &lines[2]]);
            assert_eq!(log.verify().unwrap(), ChainReport::Broken { at: 3 }, "a line duplicated");
            write(&[&lines[0], &lines[1], &lines[2], &lines[3]]);
            assert_eq!(log.verify().unwrap(), ChainReport::Complete { entries: 4 });
            // The same lines under another file name don't decrypt.
            let other = AuditLog { path: tmp.path().join("other.log") };
            std::fs::copy(log.path(), &other.path).unwrap();
            assert_eq!(other.verify().unwrap(), ChainReport::Broken { at: 1 });
        });
    }

    #[test]
    fn two_writers_at_once_fork_rather_than_break() {
        vault::tests::with_key(|| {
            let tmp = tempfile::tempdir().unwrap();
            let log = AuditLog::for_sessions(tmp.path());
            log.append(draft(EventKind::VisitDeleted)).unwrap();
            let before = std::fs::read_to_string(log.path()).unwrap();
            // Two programs read the same last line, then each appends.
            log.append(draft(EventKind::VisitRestored)).unwrap();
            let a = log.stored_lines().unwrap().pop().unwrap();
            std::fs::write(log.path(), &before).unwrap();
            log.append(draft(EventKind::RecordingDeleted)).unwrap();
            let b = log.stored_lines().unwrap().pop().unwrap();
            std::fs::write(log.path(), format!("{before}{a}\n{b}\n")).unwrap();
            assert_eq!(log.verify().unwrap(), ChainReport::Forked { entries: 4, at: 3 });
            // Later entries follow the last line, and the record stays complete.
            log.append(draft(EventKind::VisitPurged)).unwrap();
            assert_eq!(log.verify().unwrap(), ChainReport::Forked { entries: 5, at: 3 });
        });
    }

    /// A deletion is recorded once, even if the delete and the start-up
    /// recovery both try (they run on different threads).
    #[test]
    fn an_entry_with_a_chosen_id_is_added_once() {
        vault::tests::with_key(|| {
            let tmp = tempfile::tempdir().unwrap();
            let log = AuditLog::for_sessions(tmp.path());
            let mut d = draft(EventKind::VisitDeleted);
            d.id = Some(new_id());
            let first = log.append(d.clone()).unwrap();
            let mut again = d.clone();
            again.recovered = true;
            assert_eq!(log.append(again).unwrap(), first, "the entry already there is returned");
            log.append(draft(EventKind::VisitRestored)).unwrap();
            assert_eq!(log.read().unwrap().len(), 3);
            assert_eq!(log.verify().unwrap(), ChainReport::Complete { entries: 3 });
        });
    }

    /// Recovery reads past a line torn by a crash, but not a record it can't
    /// read at all.
    #[test]
    fn readable_entries_skip_a_torn_line_but_not_a_foreign_record() {
        vault::tests::with_key(|| {
            let tmp = tempfile::tempdir().unwrap();
            let log = AuditLog::for_sessions(tmp.path());
            log.append(draft(EventKind::VisitDeleted)).unwrap();
            let mut f = std::fs::OpenOptions::new().append(true).open(log.path()).unwrap();
            std::io::Write::write_all(&mut f, b"OBL1:AAAA\n\0\0\0\0").unwrap();
            drop(f);
            assert!(log.read().is_err());
            assert_eq!(log.read_readable().unwrap().len(), 2);
            log.append(draft(EventKind::VisitRestored)).unwrap();
            assert_eq!(log.read_readable().unwrap().len(), 3);
            assert!(matches!(log.verify().unwrap(), ChainReport::Broken { at: 3 }));
            let foreign = AuditLog { path: tmp.path().join("other.log") };
            std::fs::copy(log.path(), &foreign.path).unwrap();
            assert!(foreign.read_readable().is_err(), "none of it decrypts under this name");
            assert!(AuditLog { path: tmp.path().join("none.log") }.read_readable().unwrap().is_empty());
        });
    }

    #[test]
    fn with_the_vault_off_lines_are_plain_json_and_still_chain() {
        let _plain = vault::tests::SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        vault::disable();
        let tmp = tempfile::tempdir().unwrap();
        let log = AuditLog::for_sessions(tmp.path());
        let lines = four_lines(&log);
        assert!(lines.iter().all(|l| l.starts_with('{')), "{lines:?}");
        assert_eq!(log.verify().unwrap(), ChainReport::Complete { entries: 4 });
        let text = format!("{}\n{}\n{}\n", lines[0], lines[1], lines[3]);
        std::fs::write(log.path(), text).unwrap();
        assert_eq!(log.verify().unwrap(), ChainReport::Broken { at: 3 });
    }

    #[test]
    fn an_unwritable_record_is_an_error() {
        let _plain = vault::tests::SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(crate::audit_path(tmp.path())).unwrap(); // a folder where the file should be
        assert!(AuditLog::for_sessions(tmp.path()).append(draft(EventKind::VisitDeleted)).is_err());
    }
}
