//! Saved sessions: every recording, transcript and note kept on disk in one
//! folder per consult, so it can be played back, inspected and re-run.
//!
//! ```text
//! <sessions root>/2026-10-17_143205_mochi-recheck/
//!   session.json        what happened: source, audio hash, models, settings, timings, runs
//!   audio.wav           the recording (a copy of the source; live recordings write here)
//!   transcript.jsonl    one segment per line (speaker, start/end, text)
//!   runs/001_gemma-4-e4b-it-q4_0/
//!     note.json         structured note + faithfulness report + generation stats
//!     note.md           the rendered draft
//!     template.yaml     the template (and standards) the draft was written with
//! ```
//!
//! The root defaults to the app's local data directory (never a cloud-synced
//! folder such as Documents/OneDrive/iCloud: these files are clinical data).
//! `OPENBARK_SESSIONS_DIR` overrides it. Files are not yet encrypted at rest;
//! the M1 encrypted store covers that (docs/plans/M0.md, decision 7).

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::note::Refinement;
use crate::template::{Document, LoadedTemplate, Origin};
use crate::transcribe::TranscribeStats;
use crate::transcript::Transcript;
use crate::users::Who;
use crate::{EngineError, NoteResult};

pub const SESSION_FILE: &str = "session.json";
pub const AUDIO_FILE: &str = "audio.wav";
pub const TRANSCRIPT_FILE: &str = "transcript.jsonl";
pub const RUN_TEMPLATE_FILE: &str = "template.yaml";

/// Default sessions root: `OPENBARK_SESSIONS_DIR`, else the visits folder
/// chosen in Settings, else `<local data>/OpenBark/sessions` (ADR-0013).
pub fn default_root() -> PathBuf {
    os_models::locations::resolve(os_models::locations::FolderKind::Visits).0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptionInfo {
    pub stt_model: String,
    pub diarized: bool,
    pub stats: TranscribeStats,
    pub created_at: DateTime<Local>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunInfo {
    /// Folder under `runs/`.
    pub dir: String,
    pub llm: String,
    pub template: String,
    pub created_at: DateTime<Local>,
    pub faithfulness_passed: bool,
    pub total_ms: u64,
    /// Set when a veterinarian signs this draft. Signed runs are never changed;
    /// later corrections become new runs (addenda).
    #[serde(default)]
    pub signed_at: Option<DateTime<Local>>,
    #[serde(default)]
    pub signed_by: Option<String>,
    /// The signer with their role, when the clinic has a team set up.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signer: Option<Who>,
    /// Who drafted, re-prompted or edited this revision (M1 item 11).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub by: Option<Who>,
    /// Which animal this note is about, in a visit with several (M2 item 2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pet: Option<String>,
    /// Set when this run redrafted one heading of an earlier run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refinement: Option<Refinement>,
    /// Set when this run is a veterinarian's hand edit: who, and which heading.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edited: Option<EditSummary>,
    /// The medical record (default) or a document for the client.
    #[serde(default, skip_serializing_if = "Document::is_record")]
    pub document: Document,
    /// Signed additions to a signed note. The note itself never changes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub addenda: Vec<Addendum>,
    /// Which version of the template drafted this run: its layer, owner,
    /// version and fingerprint (ADR-0024 section 10). None for older runs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template_origin: Option<Origin>,
    /// The template's name and icon as they were, so lists needn't open the snapshot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template_icon: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EditSummary {
    pub heading: String,
    pub by: String,
    pub changed: usize,
    pub removed: usize,
}

/// A dated, signed addition to a signed note.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Addendum {
    pub id: String,
    pub text: String,
    pub by: String,
    pub at: DateTime<Local>,
    /// The author with their role, when the clinic has a team set up.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub who: Option<Who>,
}

impl RunInfo {
    /// "soap" from "soap@0.1.0".
    pub fn template_id(&self) -> &str {
        self.template.split('@').next().unwrap_or_default()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMeta {
    pub id: String,
    pub app_version: String,
    pub created_at: DateTime<Local>,
    /// Where the audio came from (a file path, or "live recording").
    pub source: String,
    pub audio_sha256: String,
    pub audio_secs: f32,
    pub transcription: Option<TranscriptionInfo>,
    pub runs: Vec<RunInfo>,
    /// Details the clinic entered (not from the recording).
    #[serde(default)]
    pub patient: Option<PatientInfo>,
    /// Consent to record, confirmed before recording started.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consent: Option<Consent>,
    /// Who started the recording or import.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recorded_by: Option<Who>,
    /// Set when only the recording was deleted (ADR-0022 section 4). The
    /// transcript, drafts, signatures and `audio_sha256` stay.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_deleted: Option<AudioDeleted>,
}

/// When the recording was deleted, and by whom. The rest of the visit stays.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioDeleted {
    pub at: DateTime<Local>,
    pub by: Who,
}

/// The clinic's rule for recording a conversation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsentRule {
    /// Everyone on the recording must agree (the safe default).
    AllParties,
    /// One party to the conversation (the clinician) may consent.
    OneParty,
}

/// How consent was given.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsentMethod {
    /// The client agreed out loud before recording.
    Verbal,
    /// The client signed a form for this visit.
    Written,
    /// A standing consent is on file for this client.
    OnFile,
    /// Only the clinician consented (allowed only under a one-party rule).
    ClinicianOnly,
}

/// Consent to record one encounter, confirmed by a member of the team.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Consent {
    pub rule: ConsentRule,
    pub method: ConsentMethod,
    pub confirmed_by: String,
    /// Stamped by the engine when the consent is saved.
    #[serde(default)]
    pub confirmed_at: Option<DateTime<Local>>,
    /// What was read to the client, if a script was used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub script: Option<String>,
}

impl ConsentRule {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().replace('-', "_").as_str() {
            "all_parties" => Some(Self::AllParties),
            "one_party" => Some(Self::OneParty),
            _ => None,
        }
    }
}

impl ConsentMethod {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().replace('-', "_").as_str() {
            "verbal" => Some(Self::Verbal),
            "written" => Some(Self::Written),
            "on_file" => Some(Self::OnFile),
            "clinician_only" => Some(Self::ClinicianOnly),
            _ => None,
        }
    }
}

impl Consent {
    /// Refuse consent that doesn't satisfy its own rule.
    pub fn check(&self) -> Result<(), EngineError> {
        if self.confirmed_by.trim().is_empty() {
            return Err(EngineError::Session("Say who confirmed consent to record.".into()));
        }
        if self.rule == ConsentRule::AllParties && self.method == ConsentMethod::ClinicianOnly {
            return Err(EngineError::Session(
                "Your clinic's rule is that everyone on the recording agrees. Ask the client before recording.".into(),
            ));
        }
        Ok(())
    }
}

/// Patient and visit details typed in by the clinic, time-stamped.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PatientInfo {
    /// The animal's name.
    pub name: String,
    /// Other animals seen in the same visit (multi-pet visits, M2 item 2).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub others: Vec<String>,
    /// Family or client surname.
    pub last_name: String,
    pub reason: String,
    /// The main animal's species as entered ("Cat"); decides species-only
    /// headings (M2 item 4). Never taken from the recording.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub species: String,
    /// The main animal's patient record (ADR-0011), when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub patient_id: Option<String>,
    /// The other animals' patient records, in the order of `others`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub other_ids: Vec<String>,
    /// When these details were first entered, and last changed.
    #[serde(default)]
    pub entered_at: Option<DateTime<Local>>,
    #[serde(default)]
    pub updated_at: Option<DateTime<Local>>,
}

impl PatientInfo {
    /// Every animal in the visit, the main one first.
    pub fn pets(&self) -> Vec<String> {
        std::iter::once(self.name.trim().to_string())
            .chain(self.others.iter().map(|o| o.trim().to_string()))
            .filter(|n| !n.is_empty())
            .collect()
    }

    pub fn is_empty(&self) -> bool {
        self.name.trim().is_empty()
            && self.last_name.trim().is_empty()
            && self.reason.trim().is_empty()
            && self.species.trim().is_empty()
    }

    /// "Maple Okonkwo".
    pub fn display_name(&self) -> String {
        format!("{} {}", self.name.trim(), self.last_name.trim()).trim().to_string()
    }

    /// Context for the note model: clearly marked as entered, not said.
    pub fn prompt_context(&self) -> String {
        let mut parts = Vec::new();
        if !self.name.trim().is_empty() {
            parts.push(format!("patient name {}", self.name.trim()));
        }
        if !self.last_name.trim().is_empty() {
            parts.push(format!("family name {}", self.last_name.trim()));
        }
        if !self.reason.trim().is_empty() {
            parts.push(format!("reason for visit: {}", self.reason.trim()));
        }
        if !self.species.trim().is_empty() {
            parts.push(format!("species: {}", self.species.trim()));
        }
        parts.join("; ")
    }
}

/// One consult's folder.
#[derive(Debug, Clone)]
pub struct Session {
    pub dir: PathBuf,
    pub meta: SessionMeta,
    /// The animal whose notes this handle works on, in a multi-pet visit
    /// (None: the visit's single note). Not saved; set with [`Session::for_pet`].
    pub pet: Option<String>,
}

impl Session {
    /// Create a session folder and copy the source recording into it.
    pub fn create_from_file(root: &Path, source: &Path, audio_secs: f32) -> Result<Self, EngineError> {
        let now = Local::now();
        let stem = source.file_stem().map(|s| slug(&s.to_string_lossy())).unwrap_or_default();
        let base = format!(
            "{}_{}",
            now.format("%Y-%m-%d_%H%M%S"),
            if stem.is_empty() { "recording" } else { &stem }
        );
        let dir = unique_dir(root, &base)?;
        let audio = dir.join(AUDIO_FILE);
        if crate::vault::enabled() {
            // Encrypted at rest (ADR-0008): a 16 kHz WAV copy, encrypted.
            let decoded = os_asr::Audio::load(source).map_err(|e| EngineError::Session(e.to_string()))?;
            let sink = crate::vault::AudioWriter::create(&audio).map_err(io(&audio))?;
            let mut writer =
                os_asr::WavWriter::with_sink(Box::new(sink), os_asr::SAMPLE_RATE).map_err(io(&audio))?;
            writer.write(&decoded.samples).map_err(io(&audio))?;
            writer.finish().map_err(io(&audio))?;
        } else if source.extension().is_some_and(|e| e.eq_ignore_ascii_case("wav")) {
            fs::copy(source, &audio).map_err(io(&audio))?;
        } else {
            // Phone recordings (m4a, mp3…): keep a 16 kHz WAV copy so the
            // folder always plays back the same way.
            let decoded = os_asr::Audio::load(source).map_err(|e| EngineError::Session(e.to_string()))?;
            let mut writer = os_asr::WavWriter::create(&audio, os_asr::SAMPLE_RATE).map_err(io(&audio))?;
            writer.write(&decoded.samples).map_err(io(&audio))?;
            writer.finish().map_err(io(&audio))?;
        }
        let meta = SessionMeta {
            id: uuid::Uuid::now_v7().to_string(),
            app_version: env!("CARGO_PKG_VERSION").into(),
            created_at: now,
            source: std::path::absolute(source)
                .unwrap_or_else(|_| source.to_path_buf())
                .display()
                .to_string(),
            audio_sha256: sha256_file(&audio)?,
            audio_secs,
            transcription: None,
            runs: Vec::new(),
            patient: None,
            consent: None,
            recorded_by: None,
            audio_deleted: None,
        };
        let session = Self { dir, meta, pet: None };
        session.save_meta()?;
        Ok(session)
    }

    /// Create a session for a live recording; `audio.wav` is written by the
    /// recorder and completed with [`Session::finish_live_audio`].
    pub fn create_live(root: &Path, label: &str) -> Result<Self, EngineError> {
        let now = Local::now();
        let label = slug(label);
        let base = format!(
            "{}_{}",
            now.format("%Y-%m-%d_%H%M%S"),
            if label.is_empty() { "recording" } else { &label }
        );
        let dir = unique_dir(root, &base)?;
        let meta = SessionMeta {
            id: uuid::Uuid::now_v7().to_string(),
            app_version: env!("CARGO_PKG_VERSION").into(),
            created_at: now,
            source: "live recording".into(),
            audio_sha256: String::new(),
            audio_secs: 0.0,
            transcription: None,
            runs: Vec::new(),
            patient: None,
            consent: None,
            recorded_by: None,
            audio_deleted: None,
        };
        let session = Self { dir, meta, pet: None };
        session.save_meta()?;
        Ok(session)
    }

    /// Record the finished live audio: device, duration and checksum.
    pub fn finish_live_audio(&mut self, device: &str, audio_secs: f32) -> Result<(), EngineError> {
        self.meta.source = match device.strip_prefix("file: ") {
            // An imported file run through the live pipeline keeps its origin.
            Some(path) => {
                std::path::absolute(path).map(|p| p.display().to_string()).unwrap_or_else(|_| path.into())
            }
            None => format!("live recording ({device})"),
        };
        self.meta.audio_secs = audio_secs;
        self.meta.audio_sha256 = sha256_file(&self.audio_path())?;
        self.save_meta()
    }

    pub fn open(dir: &Path) -> Result<Self, EngineError> {
        let path = dir.join(SESSION_FILE);
        let text = crate::vault::read_to_string(&path).map_err(io(&path))?;
        let meta = serde_json::from_str(&text)
            .map_err(|e| EngineError::Session(format!("{}: {e}", path.display())))?;
        Ok(Self { dir: dir.to_path_buf(), meta, pet: None })
    }

    /// Work on one animal's notes (multi-pet visits). `None` or an empty
    /// name means the visit's single note.
    pub fn for_pet(mut self, pet: Option<&str>) -> Self {
        self.pet = pet.map(str::trim).filter(|p| !p.is_empty()).map(String::from);
        self
    }

    /// Whether a run belongs to this handle's animal.
    fn mine(&self, run: &RunInfo) -> bool {
        run.pet == self.pet
    }

    pub fn audio_path(&self) -> PathBuf {
        self.dir.join(AUDIO_FILE)
    }

    pub fn transcript_path(&self) -> PathBuf {
        self.dir.join(TRANSCRIPT_FILE)
    }

    pub fn save_transcript(
        &mut self,
        transcript: &Transcript,
        stt_model: &str,
        diarized: bool,
        stats: &TranscribeStats,
    ) -> Result<(), EngineError> {
        let path = self.transcript_path();
        crate::vault::write(&path, transcript.to_jsonl().as_bytes()).map_err(io(&path))?;
        self.meta.transcription = Some(TranscriptionInfo {
            stt_model: stt_model.into(),
            diarized,
            stats: stats.clone(),
            created_at: Local::now(),
        });
        self.save_meta()
    }

    pub fn load_transcript(&self) -> Result<Transcript, EngineError> {
        let path = self.transcript_path();
        let text = crate::vault::read_to_string(&path).map_err(io(&path))?;
        Transcript::from_jsonl(&text).map_err(|e| EngineError::Session(format!("{}: {e}", path.display())))
    }

    /// Save a generated note as a new numbered run; earlier runs are kept.
    /// The template it was written with is saved beside it, so the run can
    /// be shown and re-run exactly even after the template changes.
    pub fn save_run(
        &mut self,
        llm_id: &str,
        result: &NoteResult,
        template: &LoadedTemplate,
    ) -> Result<PathBuf, EngineError> {
        let name = format!("{:03}_{}", self.meta.runs.len() + 1, slug(llm_id));
        let dir = self.dir.join("runs").join(&name);
        fs::create_dir_all(&dir).map_err(io(&dir))?;
        let json = dir.join("note.json");
        crate::vault::write(
            &json,
            serde_json::to_string_pretty(result).expect("note result serialises").as_bytes(),
        )
        .map_err(io(&json))?;
        let md = dir.join("note.md");
        crate::vault::write(&md, result.markdown.as_bytes()).map_err(io(&md))?;
        let snapshot = dir.join(RUN_TEMPLATE_FILE);
        crate::vault::write(&snapshot, template.to_yaml().as_bytes()).map_err(io(&snapshot))?;
        self.meta.runs.push(RunInfo {
            dir: format!("runs/{name}"),
            llm: llm_id.into(),
            template: result.template.clone(),
            created_at: Local::now(),
            faithfulness_passed: result.faithfulness.passed,
            total_ms: result.stats.total_ms,
            signed_at: None,
            signed_by: None,
            refinement: result.refinement.clone(),
            signer: None,
            by: result.by.clone(),
            pet: self.pet.clone(),
            edited: result.edit.as_ref().map(|e| EditSummary {
                heading: e.heading.clone(),
                by: e.by.clone(),
                changed: e.changed.len(),
                removed: e.removed,
            }),
            document: template.template.document,
            addenda: Vec::new(),
            template_origin: template.template.origin.clone(),
            template_name: Some(template.template.name.clone()),
            template_icon: Some(template.template.icon.clone()),
        });
        self.save_meta()?;
        Ok(dir)
    }

    /// Set or update the clinic-entered patient details. The first entry
    /// stamps `entered_at`; later changes stamp `updated_at`.
    pub fn set_patient(&mut self, mut info: PatientInfo) -> Result<(), EngineError> {
        let now = Local::now();
        info.name = info.name.trim().to_string();
        info.last_name = info.last_name.trim().to_string();
        info.reason = info.reason.trim().to_string();
        info.others = info.others.iter().map(|o| o.trim().to_string()).filter(|o| !o.is_empty()).collect();
        match &self.meta.patient {
            Some(prev) => {
                info.entered_at = prev.entered_at.or(Some(now));
                info.updated_at = Some(now);
            }
            None => {
                info.entered_at = Some(now);
                info.updated_at = None;
            }
        }
        self.meta.patient = if info.is_empty() { None } else { Some(info) };
        self.save_meta()
    }

    /// Record which patient records this visit's animals belong to (ADR-0011).
    /// Not an edit of the details, so their time stamps don't change.
    pub fn link_patients(&mut self, main: Option<String>, others: Vec<String>) -> Result<(), EngineError> {
        let Some(p) = self.meta.patient.as_mut() else { return Ok(()) };
        if p.patient_id == main && p.other_ids == others {
            return Ok(());
        }
        p.patient_id = main;
        p.other_ids = others;
        self.save_meta()
    }

    pub fn set_recorded_by(&mut self, who: Who) -> Result<(), EngineError> {
        self.meta.recorded_by = Some(who);
        self.save_meta()
    }

    /// Save the consent to record, time-stamped now.
    pub fn set_consent(&mut self, mut consent: Consent) -> Result<(), EngineError> {
        consent.check()?;
        consent.confirmed_by = consent.confirmed_by.trim().to_string();
        consent.script = consent.script.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        consent.confirmed_at = Some(Local::now());
        self.meta.consent = Some(consent);
        self.save_meta()
    }

    /// Sign the latest run. Refused if it has numbers absent from the
    /// transcript (DESIGN.md §5: hard-fail content blocks signing).
    pub fn sign_latest(&mut self, signed_by: &str) -> Result<(), EngineError> {
        self.sign_latest_by(&Who::named(signed_by))
    }

    /// Sign as a team member (their role is kept with the signature).
    pub fn sign_latest_by(&mut self, who: &Who) -> Result<(), EngineError> {
        let pet = self.pet.clone();
        let run = self
            .meta
            .runs
            .iter_mut()
            .rev()
            .find(|r| r.document.is_record() && r.pet == pet)
            .ok_or_else(|| EngineError::Session("There is no draft to sign yet.".into()))?;
        if run.signed_at.is_some() {
            return Err(EngineError::Session("This note is already signed.".into()));
        }
        if !run.faithfulness_passed {
            return Err(EngineError::Session(
                "Some numbers in this draft aren't in the transcript. Check them against the recording and correct the draft before signing.".into(),
            ));
        }
        run.signed_at = Some(Local::now());
        run.signed_by = Some(who.name.clone());
        run.signer = who.role.is_some().then(|| who.clone());
        self.save_meta()
    }

    /// The latest medical-record run (client documents aside).
    pub fn latest_record(&self) -> Option<&RunInfo> {
        self.meta.runs.iter().rev().find(|r| r.document.is_record() && self.mine(r))
    }

    /// Runs of this handle's animal (all of them, in order).
    pub fn my_runs(&self) -> impl Iterator<Item = &RunInfo> {
        self.meta.runs.iter().filter(|r| self.mine(r))
    }

    /// The latest record run's saved note (`note` from `note.json`).
    pub fn latest_note(&self) -> Result<(RunInfo, serde_json::Value), EngineError> {
        let run = self
            .latest_record()
            .cloned()
            .ok_or_else(|| EngineError::Session("There is no draft yet. Draft a note first.".into()))?;
        let path = self.dir.join(&run.dir).join("note.json");
        let text = crate::vault::read_to_string(&path).map_err(io(&path))?;
        let saved: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| EngineError::Session(format!("{}: {e}", path.display())))?;
        Ok((run, saved["note"].clone()))
    }

    /// The template a run was written with: its snapshot, or for runs saved
    /// before snapshots existed, the template by id as it is now.
    pub fn run_template(&self, run: &RunInfo) -> Result<LoadedTemplate, EngineError> {
        let snapshot = self.dir.join(&run.dir).join(RUN_TEMPLATE_FILE);
        match crate::vault::read_to_string(&snapshot) {
            Ok(yaml) => LoadedTemplate::from_yaml(&yaml),
            Err(_) => LoadedTemplate::resolve(run.template_id()),
        }
    }

    /// Add a signed addendum to the signed note.
    pub fn add_addendum(&mut self, text: &str, by: &str) -> Result<Addendum, EngineError> {
        self.add_addendum_by(text, &Who::named(by))
    }

    /// Add an addendum as a team member.
    pub fn add_addendum_by(&mut self, text: &str, who: &Who) -> Result<Addendum, EngineError> {
        let (text, by) = (text.trim(), who.name.trim());
        if by.is_empty() {
            return Err(EngineError::Session(
                "Enter the name of the veterinarian signing the addendum.".into(),
            ));
        }
        if text.is_empty() {
            return Err(EngineError::Session("Write the addendum first.".into()));
        }
        let pet = self.pet.clone();
        let run = self
            .meta
            .runs
            .iter_mut()
            .rev()
            .find(|r| r.document.is_record() && r.pet == pet)
            .filter(|r| r.signed_at.is_some())
            .ok_or_else(|| {
                EngineError::Session("Addenda are for signed notes. Edit the draft instead.".into())
            })?;
        let addendum = Addendum {
            id: uuid::Uuid::now_v7().to_string(),
            text: text.into(),
            by: by.into(),
            at: Local::now(),
            who: who.role.is_some().then(|| who.clone()),
        };
        run.addenda.push(addendum.clone());
        self.save_meta()?;
        Ok(addendum)
    }

    /// Whether the medical record is signed (client documents don't count).
    /// Only this handle's animal: for the whole visit, see [`Session::any_signed`].
    pub fn is_signed(&self) -> bool {
        self.latest_record().is_some_and(|r| r.signed_at.is_some())
    }

    /// Whether any note in the visit is signed, for any animal (ADR-0022
    /// section 2). A signed note for one animal makes the whole visit signed.
    pub fn any_signed(&self) -> bool {
        self.meta.runs.iter().any(|r| r.signed_at.is_some())
    }

    /// The latest signature or addendum in the visit, for any animal: when
    /// the record last changed (the phase 2 retention rule counts from here).
    pub fn last_entry_at(&self) -> Option<DateTime<Local>> {
        self.meta
            .runs
            .iter()
            .flat_map(|r| r.signed_at.into_iter().chain(r.addenda.iter().map(|a| a.at)))
            .max()
    }

    /// Whether the recording is still in the folder.
    pub fn has_audio(&self) -> bool {
        self.audio_path().is_file()
    }

    /// Delete only the recording (ADR-0022 section 4): `audio.wav` and any
    /// leftover `audio.wav.tmp`. The transcript, every run, signatures,
    /// addenda, consent, `audio_sha256`, `audio_secs` and the source stay, so
    /// the length still shows and a later copy can still be matched.
    pub fn delete_audio(&mut self, who: &Who) -> Result<(), EngineError> {
        let audio = self.audio_path();
        if !audio.is_file() {
            return Err(EngineError::Session("This visit has no recording.".into()));
        }
        crate::trash::remove_file_retrying(&audio)?;
        let tmp = self.dir.join(format!("{AUDIO_FILE}.tmp"));
        if tmp.exists() {
            crate::trash::remove_file_retrying(&tmp)?;
        }
        self.meta.audio_deleted = Some(AudioDeleted { at: Local::now(), by: who.clone() });
        self.save_meta()
    }

    fn save_meta(&self) -> Result<(), EngineError> {
        let path = self.dir.join(SESSION_FILE);
        let text = serde_json::to_string_pretty(&self.meta).expect("meta serialises");
        crate::vault::write(&path, text.as_bytes()).map_err(io(&path))
    }
}

/// Sessions under `root`, newest first.
pub fn list(root: &Path) -> Vec<Session> {
    let mut sessions: Vec<Session> = fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().join(SESSION_FILE).is_file())
        .filter_map(|e| Session::open(&e.path()).ok())
        .collect();
    sessions.sort_by_key(|s| std::cmp::Reverse(s.meta.created_at));
    sessions
}

fn unique_dir(root: &Path, base: &str) -> Result<PathBuf, EngineError> {
    fs::create_dir_all(root).map_err(io(root))?;
    for n in 0..1000 {
        let name = if n == 0 { base.to_string() } else { format!("{base}-{n}") };
        let dir = root.join(name);
        if fs::create_dir(&dir).is_ok() {
            return Ok(dir);
        }
    }
    Err(EngineError::Session(format!("cannot create a session folder in {}", root.display())))
}

/// File-name-safe, lowercase, ASCII: "Mochi Recheck (2)" → "mochi-recheck-2".
pub(crate) fn slug(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').chars().take(48).collect()
}

/// SHA-256 of a file's content (decrypted, so it identifies the recording
/// itself whether or not it's encrypted at rest).
fn sha256_file(path: &Path) -> Result<String, EngineError> {
    if crate::vault::is_encrypted(path) {
        let plain = crate::vault::read(path).map_err(io(path))?;
        return Ok(hex::encode(Sha256::digest(&plain)));
    }
    let mut file = fs::File::open(path).map_err(io(path))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = file.read(&mut buf).map_err(io(path))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn io(path: &Path) -> impl Fn(std::io::Error) -> EngineError + '_ {
    move |e| EngineError::Session(format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::faithfulness::FaithfulnessReport;
    use crate::transcript::Segment;

    fn note_result() -> NoteResult {
        NoteResult {
            status: "draft",
            template: "soap@0.1.0".into(),
            model: "test".into(),
            note: serde_json::json!({}),
            faithfulness: FaithfulnessReport { passed: true, ..Default::default() },
            stats: Default::default(),
            refinement: None,
            edit: None,
            by: None,
            markdown: "# SOAP note\n".into(),
        }
    }

    #[test]
    fn slugs_are_safe() {
        assert_eq!(slug("Mochi Recheck (2)"), "mochi-recheck-2");
        assert_eq!(slug("  ...  "), "");
        assert_eq!(slug("gemma-4-e4b-it-q4_0"), "gemma-4-e4b-it-q4-0");
    }

    #[test]
    fn session_round_trip_with_runs() {
        // Plain files: hold the vault lock so no other test turns encryption on meanwhile.
        let _plain = crate::vault::tests::SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("Mochi recheck.wav");
        fs::write(&src, b"RIFF....WAVE").unwrap();
        let root = tmp.path().join("sessions");

        let mut s = Session::create_from_file(&root, &src, 12.5).unwrap();
        assert!(s.dir.file_name().unwrap().to_string_lossy().ends_with("_mochi-recheck"));
        assert_eq!(fs::read(s.audio_path()).unwrap(), b"RIFF....WAVE");
        assert_eq!(s.meta.audio_sha256.len(), 64);

        let t = Transcript {
            segments: vec![Segment {
                id: 0,
                start: 0.0,
                end: 1.0,
                speaker: Some(0),
                text: "Hi".into(),
                hints: Vec::new(),
                check: None,
            }],
        };
        s.save_transcript(&t, "parakeet", true, &Default::default()).unwrap();
        let soap = LoadedTemplate::builtin("soap").unwrap();
        let run1 = s.save_run("gemma-4-e4b-it-q4_0", &note_result(), &soap).unwrap();
        let run2 = s.save_run("gemma-4-12b-it-q4_0", &note_result(), &soap).unwrap();
        assert!(run1.ends_with("001_gemma-4-e4b-it-q4-0"));
        assert!(run2.join("note.md").is_file());
        let last = s.meta.runs.last().unwrap().clone();
        assert_eq!(last.template_id(), "soap");
        assert_eq!(s.run_template(&last).unwrap().label(), "soap@0.1.1");
        assert_eq!(s.latest_note().unwrap().1, serde_json::json!({}));

        // Addenda only after signing; the signed run itself is untouched.
        assert!(s.add_addendum("Owner called: eating again.", "Dr Patel").is_err());
        s.sign_latest("Dr Patel").unwrap();
        let a = s.add_addendum(" Owner called: eating again. ", "Dr Patel").unwrap();
        assert_eq!(a.text, "Owner called: eating again.");
        let reopened = Session::open(&s.dir).unwrap();
        assert_eq!(reopened.meta.runs.last().unwrap().addenda, vec![a]);
        assert!(reopened.is_signed());

        let reopened = Session::open(&s.dir).unwrap();
        assert_eq!(reopened.meta.runs.len(), 2);
        assert_eq!(reopened.load_transcript().unwrap(), t);
        assert_eq!(list(&root).len(), 1);

        // Patient details are time-stamped on entry and on change.
        s.set_patient(PatientInfo {
            name: " Maple ".into(),
            last_name: "Okonkwo".into(),
            reason: "Limping".into(),
            ..Default::default()
        })
        .unwrap();
        let first = Session::open(&s.dir).unwrap().meta.patient.unwrap();
        assert_eq!(first.display_name(), "Maple Okonkwo");
        assert!(first.entered_at.is_some() && first.updated_at.is_none());
        s.set_patient(PatientInfo { reason: "Limping, right hind".into(), ..first.clone() }).unwrap();
        let second = Session::open(&s.dir).unwrap().meta.patient.unwrap();
        assert_eq!(second.entered_at, first.entered_at);
        assert!(second.updated_at.is_some());

        // Consent: checked against its rule, stamped, saved.
        let mut consent = Consent {
            rule: ConsentRule::AllParties,
            method: ConsentMethod::ClinicianOnly,
            confirmed_by: "Dr Patel".into(),
            confirmed_at: None,
            script: Some(" I'd like to record our conversation. ".into()),
        };
        assert!(s.set_consent(consent.clone()).is_err(), "all-party rule needs the client");
        consent.method = ConsentMethod::Verbal;
        s.set_consent(consent).unwrap();
        let saved = Session::open(&s.dir).unwrap().meta.consent.unwrap();
        assert!(saved.confirmed_at.is_some());
        assert_eq!(saved.script.as_deref(), Some("I'd like to record our conversation."));
        let one_party = Consent {
            rule: ConsentRule::OneParty,
            method: ConsentMethod::ClinicianOnly,
            confirmed_by: "Dr Patel".into(),
            confirmed_at: None,
            script: None,
        };
        assert!(one_party.check().is_ok());
        assert!(Consent { confirmed_by: " ".into(), ..one_party }.check().is_err());

        // A second session from the same file in the same second gets its own folder.
        let other = Session::create_from_file(&root, &src, 12.5).unwrap();
        assert_ne!(other.dir, s.dir);
    }

    /// ADR-0022 section 2: one animal's signed note makes the whole visit signed.
    #[test]
    fn any_signed_looks_at_every_animal() {
        let _plain = crate::vault::tests::SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        let mut s = Session::create_live(&tmp.path().join("sessions"), "Olive").unwrap();
        s.set_patient(PatientInfo {
            name: "Olive".into(),
            others: vec!["Pickles".into()],
            last_name: "Haddad".into(),
            ..Default::default()
        })
        .unwrap();
        assert!(!s.any_signed() && s.last_entry_at().is_none());
        let soap = LoadedTemplate::builtin("soap").unwrap();
        let mut olive = s.clone().for_pet(Some("Olive"));
        olive.save_run("gemma", &note_result(), &soap).unwrap();
        let mut pickles = Session::open(&s.dir).unwrap().for_pet(Some("Pickles"));
        pickles.save_run("gemma", &note_result(), &soap).unwrap();
        pickles.sign_latest("Dr Rivera").unwrap();
        let whole = Session::open(&s.dir).unwrap();
        assert!(whole.any_signed(), "Pickles's note is signed");
        assert!(!whole.is_signed(), "without a pet, is_signed only sees the visit's own note");
        assert!(!whole.clone().for_pet(Some("Olive")).is_signed());
        let signed_at = whole.meta.runs[1].signed_at;
        assert_eq!(whole.last_entry_at(), signed_at);
        let a = pickles.add_addendum("Owner called.", "Dr Rivera").unwrap();
        assert_eq!(Session::open(&s.dir).unwrap().last_entry_at(), Some(a.at));
    }

    /// ADR-0022 section 4: deleting the recording keeps everything else.
    #[test]
    fn deleting_the_recording_keeps_the_rest() {
        let _plain = crate::vault::tests::SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("Maple.wav");
        fs::write(&src, b"RIFF....WAVE").unwrap();
        let mut s = Session::create_from_file(&tmp.path().join("sessions"), &src, 12.5).unwrap();
        let t = Transcript {
            segments: vec![Segment {
                id: 0,
                start: 0.0,
                end: 1.0,
                speaker: Some(0),
                text: "Hi".into(),
                hints: Vec::new(),
                check: None,
            }],
        };
        s.save_transcript(&t, "parakeet", false, &Default::default()).unwrap();
        s.save_run("gemma", &note_result(), &LoadedTemplate::builtin("soap").unwrap()).unwrap();
        fs::write(s.dir.join("audio.wav.tmp"), b"left over").unwrap();
        let (sha, secs) = (s.meta.audio_sha256.clone(), s.meta.audio_secs);
        assert!(s.has_audio());
        s.delete_audio(&Who::named("Sam")).unwrap();
        assert!(!s.audio_path().exists() && !s.dir.join("audio.wav.tmp").exists());
        let reopened = Session::open(&s.dir).unwrap();
        assert!(!reopened.has_audio());
        assert_eq!(reopened.meta.audio_deleted.as_ref().unwrap().by.name, "Sam");
        assert_eq!((reopened.meta.audio_sha256.clone(), reopened.meta.audio_secs), (sha, secs));
        assert_eq!(reopened.load_transcript().unwrap(), t);
        assert!(reopened.latest_note().is_ok());
        let again = s.delete_audio(&Who::named("Sam")).unwrap_err().to_string();
        assert_eq!(again, "This visit has no recording.");
    }

    #[test]
    fn listing_ignores_the_housekeeping_folder() {
        let _plain = crate::vault::tests::SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("sessions");
        let s = Session::create_live(&root, "Maple").unwrap();
        let deleted = crate::deleted_dir(&root).join("0199a7f0-1c2d-7e3f-8a9b-0c1d2e3f4a5b");
        fs::create_dir_all(&deleted).unwrap();
        fs::copy(s.dir.join(SESSION_FILE), deleted.join(SESSION_FILE)).unwrap();
        fs::write(crate::audit_path(&root), b"").unwrap();
        let listed = list(&root);
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].dir, s.dir);
    }

    #[test]
    fn encrypted_sessions_round_trip_with_nothing_plain_on_disk() {
        crate::vault::tests::with_key(|| {
            let tmp = tempfile::tempdir().unwrap();
            let root = tmp.path().join("sessions");
            let mut s = Session::create_live(&root, "Mochi").unwrap();
            s.set_patient(PatientInfo {
                name: "Mochi".into(),
                reason: "Limping".into(),
                ..Default::default()
            })
            .unwrap();
            let t = Transcript {
                segments: vec![Segment {
                    id: 0,
                    start: 0.0,
                    end: 1.0,
                    speaker: Some(0),
                    text: "Temperature 39.4".into(),
                    hints: Vec::new(),
                    check: None,
                }],
            };
            s.save_transcript(&t, "parakeet", true, &Default::default()).unwrap();
            let mut result = note_result();
            result.markdown = "# SOAP note
- Temperature 39.4
"
            .into();
            s.save_run("gemma", &result, &LoadedTemplate::builtin("soap").unwrap()).unwrap();

            let reopened = Session::open(&s.dir).unwrap();
            assert_eq!(reopened.meta.patient.as_ref().unwrap().name, "Mochi");
            assert_eq!(reopened.load_transcript().unwrap(), t);
            assert_eq!(reopened.latest_note().unwrap().1, serde_json::json!({}));
            // No file in the folder holds the patient name or the temperature in the clear.
            fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
                for e in fs::read_dir(dir).unwrap().flatten() {
                    if e.path().is_dir() { walk(&e.path(), out) } else { out.push(e.path()) }
                }
            }
            let mut files = Vec::new();
            walk(&s.dir, &mut files);
            assert!(files.len() >= 5, "{files:?}");
            for f in files {
                let raw = fs::read(&f).unwrap();
                assert!(crate::vault::is_encrypted(&f), "{} is plain", f.display());
                assert!(!raw.windows(5).any(|w| w == b"Mochi" || w == b"39.4 "), "{} leaks", f.display());
            }
        });
    }
}
