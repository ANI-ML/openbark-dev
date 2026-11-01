//! Saved sessions: every recording, transcript and note kept on disk in one
//! folder per consult, so it can be played back, inspected and re-run.
//!
//! ```text
//! <sessions root>/2026-10-17_143205_mochi-recheck/
//!   session.json        what happened: source, audio hash, models, settings, timings, runs
//!   audio.wav           the recording (a copy of the source; live recordings write here)
//!   transcript.jsonl    one segment per line (speaker, start/end, text)
//!   runs/001_a3f9_gemma-4-e4b-it-q4_0/
//!     note.json         structured note + faithfulness report + generation stats
//!     note.md           the rendered draft
//!     template.yaml     the template (and standards) the draft was written with
//! ```
//!
//! Run folders carry the tag of the device that drafted them (`a3f9`,
//! [`crate::device`]) and each run a UUIDv7 `run_id`, so drafts made offline
//! on two devices never collide when they meet (ADR-0017 section 6). Runs
//! from before that (`runs/001_<model>`, no `run_id`) load as they are.
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
    /// UUIDv7, the run's id everywhere (ADR-0017 section 6). `None` for
    /// runs drafted before M3; [`RunInfo::id`] then falls back to `dir`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    /// The tag of the device that drafted it ([`crate::device`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device: Option<String>,
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
    /// The tooth chart finding table's version (`packages/dental/chart.json`)
    /// the run was drafted and checked with, when its template has a chart
    /// (ADR-0021). A later table that changes a word or code must keep this
    /// version's table to draw the run's chart and PDF.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chart_version: Option<u32>,
    /// A veterinarian's confirmations of the drafted tooth chart entries, by
    /// tooth (ADR-0021 decision 10). Signing needs one for every drafted
    /// entry; entries the vet edited or added are their own.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub chart_checks: Vec<ChartCheck>,
    /// Set by encrypted sync (ADR-0017 section 9) on a run that met a
    /// signature made on another device: a draft made offline after the note
    /// was signed elsewhere, or a second signature that came later. The run
    /// is kept and shown beside the record, offered as an addendum; it is
    /// never the visit's record, and nothing is dropped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub set_aside: Option<SetAside>,
}

/// Why a run was set aside when visits from two devices met (ADR-0017
/// section 9), and which signed run is the record.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SetAside {
    pub reason: SetAsideReason,
    /// The record: the run signed first.
    pub record_run: String,
    pub signed_at: DateTime<Local>,
    /// Where the record was signed (a device name from the sync roster), when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signed_on: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SetAsideReason {
    /// A draft made after the note was signed on another device.
    SignedElsewhere,
    /// Signed too, but after the record was signed.
    SignedLater,
}

/// A veterinarian's confirmation that the drafted chart entries on one tooth
/// match the recording (ADR-0021 decision 10): who, when, and exactly which
/// entries, so a changed or redrafted entry is unconfirmed again.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChartCheck {
    pub tooth: String,
    /// The drafted entries confirmed ([`crate::dental::item_key`]).
    pub items: Vec<String>,
    pub by: String,
    /// The confirmer with their role, when the clinic has a team set up.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub who: Option<Who>,
    pub at: DateTime<Local>,
}

/// The teeth of a run's chart with a drafted entry that no veterinarian has
/// confirmed, in Triadan order. `note`: the run's note.
pub fn unconfirmed_teeth(run: &RunInfo, note: &serde_json::Value) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for item in crate::dental::drafted_items(note) {
        let tooth = item["tooth"].as_str().unwrap_or_default();
        let key = crate::dental::item_key(item);
        let confirmed = run.chart_checks.iter().any(|c| c.tooth == tooth && c.items.contains(&key));
        if !confirmed && !out.iter().any(|t| t == tooth) {
            out.push(tooth.to_string());
        }
    }
    out.sort_by_key(|t| crate::dental::triadan_order(t));
    out
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
    /// The run's id: its `run_id`, or for a run from before M3 its folder.
    pub fn id(&self) -> &str {
        self.run_id.as_deref().unwrap_or(&self.dir)
    }

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
    /// For a visit that came from another device through encrypted sync:
    /// the device that recorded it, which alone holds the audio (ADR-0017
    /// section 7). Audio never syncs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_on: Option<AudioOn>,
    /// Set for a phone call made through the clinic's own Twilio account
    /// (ADR-0023 section 8): the other number, when, how long, Twilio's call
    /// SID. Kept in `session.json`, encrypted like the rest of the visit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call: Option<CallInfo>,
}

/// Which way a phone call went.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CallDirection {
    /// The clinic phoned the client (the first release only makes these).
    Outgoing,
    /// The client phoned the clinic (a later release).
    Incoming,
}

/// The source of a recorded phone call.
pub const PHONE_CALL_SOURCE: &str = "Phone call";

/// Speaker label for the client's side of a call (channel 2).
pub const CLIENT_LABEL: &str = "Client";

/// A phone call's details (ADR-0023 section 8). The recording has two
/// channels: 1 is the clinic, 2 the client, so speaker labels come from the
/// channel, never from voice embeddings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CallInfo {
    pub direction: CallDirection,
    /// The other side's number, in E.164 ("+15555550142").
    pub number: String,
    /// When the call was answered and recording started.
    pub started_at: DateTime<Local>,
    /// How long the call lasted, from answer to hang-up.
    #[serde(default)]
    pub duration_secs: f32,
    /// Twilio's call SID (`CA...`), so the clinic can match its Twilio logs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call_sid: Option<String>,
    /// The clinic's side (channel 1): "Vet", "Tech", or "Clinic" without a team.
    pub clinic_label: String,
    /// When the recording was deleted, in seconds into the call. Despite the
    /// name, it is set for both deletions: the client declined (the call went
    /// on unrecorded), or the call ended before the announcement was confirmed
    /// (`not_announced`). The visit's consent outcome says which.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declined_at_secs: Option<f32>,
}

impl CallInfo {
    /// Speaker names by channel: the clinic's role, then "Client".
    pub fn speaker_names(&self) -> [String; 2] {
        [self.clinic_label.clone(), CLIENT_LABEL.to_string()]
    }

    /// Context for the note model: who is who on the call.
    pub fn prompt_context(&self) -> String {
        let way = match self.direction {
            CallDirection::Outgoing => "the clinic phoned the client",
            CallDirection::Incoming => "the client phoned the clinic",
        };
        format!(
            "this is a phone call ({way}); Speaker 1 is the clinic ({}), Speaker 2 is the client",
            self.clinic_label
        )
    }
}

/// The device that holds a synced visit's recording.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioOn {
    /// The sync device id.
    pub device_id: String,
    /// Its name in the sync roster ("Anna's iPhone"), when known.
    #[serde(default)]
    pub device_name: String,
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
    /// A phone call's announcement was played from the clinic's own
    /// recording (ADR-0023 section 7). An announcement the user read out is
    /// `Verbal`. Allowed under both rules: the client heard it and could say no.
    Announced,
}

/// What the client said after a phone call's announcement (ADR-0023 section 7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsentOutcome {
    /// "Yes, keep recording": express consent, in the recording.
    Agreed,
    /// "No, stop recording": what was recorded was deleted, and the call
    /// went on unrecorded.
    Declined,
    /// Nobody answered "Did the client agree?": the client stayed on the
    /// line after the announcement.
    StayedOnLine,
    /// The announcement was never confirmed as read ("I've read it"), so the
    /// recording was deleted: recording without it isn't allowed (owner
    /// decision 1). The call itself wasn't affected.
    NotAnnounced,
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
    /// Phone calls: what the client said after the announcement.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<ConsentOutcome>,
    /// Phone calls: when the announcement started, in seconds into the
    /// recording (normally 0: it is the first thing recorded).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub announced_at_secs: Option<f32>,
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
            "announced" => Some(Self::Announced),
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
            audio_on: None,
            call: None,
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
            audio_on: None,
            call: None,
        };
        let session = Self { dir, meta, pet: None };
        session.save_meta()?;
        Ok(session)
    }

    /// Record the finished live audio: device, duration and checksum.
    pub fn finish_live_audio(&mut self, device: &str, audio_secs: f32) -> Result<(), EngineError> {
        self.meta.source = match device.strip_prefix("file: ") {
            // A phone call keeps its source.
            _ if self.meta.call.is_some() => PHONE_CALL_SOURCE.to_string(),
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
        let device = self.device();
        let (name, dir) = self.new_run_dir(&device.tag, llm_id)?;
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
        // A veterinarian's chart confirmations carry over to a hand edit or a
        // redraft of one heading, for the drafted entries that didn't change.
        let carried: Vec<ChartCheck> = match self.latest_record() {
            Some(prev)
                if template.template.document.is_record()
                    && (result.edit.is_some() || result.refinement.is_some()) =>
            {
                let drafted: Vec<(String, String)> = crate::dental::drafted_items(&result.note)
                    .into_iter()
                    .map(|i| {
                        (i["tooth"].as_str().unwrap_or_default().to_string(), crate::dental::item_key(i))
                    })
                    .collect();
                prev.chart_checks
                    .iter()
                    .filter_map(|c| {
                        let items: Vec<String> = c
                            .items
                            .iter()
                            .filter(|k| drafted.iter().any(|(t, key)| *t == c.tooth && key == *k))
                            .cloned()
                            .collect();
                        (!items.is_empty()).then(|| ChartCheck { items, ..c.clone() })
                    })
                    .collect()
            }
            _ => Vec::new(),
        };
        self.meta.runs.push(RunInfo {
            dir: format!("runs/{name}"),
            run_id: Some(uuid::Uuid::now_v7().to_string()),
            device: Some(device.tag),
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
            chart_version: template.chart().map(|_| crate::dental::chart().version),
            chart_checks: carried,
            set_aside: None,
        });
        self.save_meta()?;
        Ok(dir)
    }

    /// This device, as known in the visits folder the session is in.
    fn device(&self) -> crate::device::Device {
        crate::device::for_root(self.dir.parent().unwrap_or(&self.dir))
    }

    /// A new, empty run folder: `<number>_<device tag>_<model>`, the number
    /// one past the runs listed, and never an existing folder's name.
    fn new_run_dir(&self, tag: &str, llm_id: &str) -> Result<(String, PathBuf), EngineError> {
        let runs = self.dir.join("runs");
        let mut n = self.meta.runs.len() + 1;
        loop {
            let name = format!("{n:03}_{tag}_{}", slug(llm_id));
            let dir = runs.join(&name);
            if !dir.exists() && !self.meta.runs.iter().any(|r| r.dir == format!("runs/{name}")) {
                fs::create_dir_all(&dir).map_err(io(&dir))?;
                return Ok((name, dir));
            }
            n += 1;
        }
    }

    /// Take in a visit made on another device (ADR-0017 section 6): write
    /// its header (`session.json`) into a new folder under `root`, keeping
    /// its id. Its runs come with their files through
    /// [`Session::merge_runs_from`], so none is listed without them. If a
    /// visit with that id is already here, it is opened instead. Files are
    /// written under this device's data key. The audio isn't there: it stays
    /// on the device that recorded it.
    pub fn adopt(root: &Path, mut meta: SessionMeta) -> Result<Self, EngineError> {
        meta.runs.clear();
        if let Some(existing) = list(root).into_iter().find(|s| s.meta.id == meta.id) {
            return Ok(existing);
        }
        let label = meta
            .patient
            .as_ref()
            .map(|p| p.display_name())
            .filter(|n| !n.trim().is_empty())
            .map(|n| slug(&n))
            .unwrap_or_else(|| "visit".into());
        let base = format!("{}_{label}", meta.created_at.format("%Y-%m-%d_%H%M%S"));
        let dir = unique_dir(root, &base)?;
        let session = Self { dir, meta, pet: None };
        session.save_meta()?;
        Ok(session)
    }

    /// Bring in the runs of the same visit from another device's copy
    /// (`other`): each run this copy doesn't have (by [`RunInfo::id`]) is
    /// copied with its files, decrypted from `other` and written under this
    /// device's data key, keeping its folder name unless that is taken here.
    /// Nothing here is changed or dropped; signed runs stay as they are.
    /// Runs are kept in the order they were drafted. Returns how many came in.
    pub fn merge_runs_from(&mut self, other: &Session) -> Result<usize, EngineError> {
        if other.meta.id != self.meta.id {
            return Err(EngineError::Session("These are copies of two different visits.".into()));
        }
        let mut added = 0;
        for run in &other.meta.runs {
            if self.meta.runs.iter().any(|r| r.id() == run.id()) {
                continue;
            }
            let from = other.dir.join(&run.dir);
            let leaf = run.dir.strip_prefix("runs/").unwrap_or(&run.dir);
            let mut name = leaf.to_string();
            let mut n = 2;
            while self.dir.join("runs").join(&name).exists()
                || self.meta.runs.iter().any(|r| r.dir == format!("runs/{name}"))
            {
                name = format!("{leaf}-{n}");
                n += 1;
            }
            let to = self.dir.join("runs").join(&name);
            copy_run_files(&from, &to)?;
            let mut copy = run.clone();
            copy.dir = format!("runs/{name}");
            self.meta.runs.push(copy);
            added += 1;
        }
        if added > 0 {
            self.meta.runs.sort_by(|a, b| a.created_at.cmp(&b.created_at).then_with(|| a.id().cmp(b.id())));
            self.save_meta()?;
        }
        Ok(added)
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

    /// Save a phone call's details and mark the visit's source "Phone call".
    pub fn set_call(&mut self, call: CallInfo) -> Result<(), EngineError> {
        self.meta.source = PHONE_CALL_SOURCE.to_string();
        self.meta.call = Some(call);
        self.save_meta()
    }

    /// Record what the client said after a call's announcement, keeping when
    /// the consent was first confirmed.
    pub fn set_consent_outcome(
        &mut self,
        outcome: ConsentOutcome,
        announced_at_secs: Option<f32>,
    ) -> Result<(), EngineError> {
        let consent = self
            .meta
            .consent
            .as_mut()
            .ok_or_else(|| EngineError::Session("This visit has no consent to record.".into()))?;
        consent.outcome = Some(outcome);
        if announced_at_secs.is_some() {
            consent.announced_at_secs = announced_at_secs;
        }
        self.save_meta()
    }

    /// A call the client declined (ADR-0023 section 7): remove the audio and
    /// anything transcribed, whatever the recorder had saved, and mark the
    /// recording deleted. The visit itself stays, to say so.
    pub fn forget_recording(&mut self, who: &Who) -> Result<(), EngineError> {
        for path in [self.audio_path(), self.dir.join(format!("{AUDIO_FILE}.tmp")), self.transcript_path()] {
            if path.exists() {
                crate::trash::remove_file_retrying(&path)?;
            }
        }
        self.meta.transcription = None;
        self.meta.audio_secs = 0.0;
        self.meta.audio_sha256 = String::new();
        self.meta.audio_deleted = Some(AudioDeleted { at: Local::now(), by: who.clone() });
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
    /// Refused while a drafted tooth chart entry isn't confirmed by a
    /// veterinarian (ADR-0021 decision 10), and while a normal finding from
    /// the template waits to be confirmed or removed (ADR-0032).
    pub fn sign_latest_by(&mut self, who: &Who) -> Result<(), EngineError> {
        let at = self
            .latest_record_index()
            .ok_or_else(|| EngineError::Session("There is no draft to sign yet.".into()))?;
        let run = &self.meta.runs[at];
        if run.signed_at.is_some() {
            return Err(EngineError::Session("This note is already signed.".into()));
        }
        if !run.faithfulness_passed {
            return Err(EngineError::Session(
                "Something in this draft blocks signing: a number that isn't in the transcript, or a tooth chart that needs dog or cat chosen. Check the flagged items and correct the draft before signing.".into(),
            ));
        }
        // The note itself, so a chart that can't be read never signs unchecked.
        let (_, saved) = self.latest_saved()?;
        let waiting = unconfirmed_teeth(&self.meta.runs[at], &saved["note"]);
        if !waiting.is_empty() {
            return Err(EngineError::Session(format!(
                "The tooth chart is a draft until a veterinarian confirms it. Check each drafted tooth against the recording and confirm it before signing. Not confirmed yet: {}.",
                waiting.join(", ")
            )));
        }
        // Normal findings from the template weren't said: whatever the client
        // (the app, web mode, the API), they never sign unconfirmed.
        let defaults = crate::defaults::pending_in(&saved);
        if !defaults.is_empty() {
            let labels: Vec<&str> = defaults.iter().map(|d| d.label.as_str()).collect();
            return Err(EngineError::Session(format!(
                "This draft has normal findings from the template that weren't said in the recording: {}. Confirm or remove each one before signing.",
                labels.join(", ")
            )));
        }
        let run = &mut self.meta.runs[at];
        run.signed_at = Some(Local::now());
        run.signed_by = Some(who.name.clone());
        run.signer = who.role.is_some().then(|| who.clone());
        self.save_meta()
    }

    /// The latest medical-record run (client documents aside).
    /// A run set aside by sync (a draft or a second signature that met a
    /// signature from another device) is never the record.
    pub fn latest_record(&self) -> Option<&RunInfo> {
        self.meta.runs.iter().rev().find(|r| r.document.is_record() && r.set_aside.is_none() && self.mine(r))
    }

    fn latest_record_index(&self) -> Option<usize> {
        self.meta.runs.iter().rposition(|r| r.document.is_record() && r.set_aside.is_none() && self.mine(r))
    }

    /// The latest draft's tooth chart teeth with a drafted entry that no
    /// veterinarian has confirmed (none when there is no draft or no chart).
    pub fn unconfirmed_teeth(&self) -> Vec<String> {
        match self.latest_note() {
            Ok((run, note)) => unconfirmed_teeth(&run, &note),
            Err(_) => Vec::new(),
        }
    }

    /// Confirm, or take back the confirmation of, the drafted chart entries
    /// on these teeth of the latest draft, as `who`, now (ADR-0021 decision
    /// 10). Refused once the note is signed: a signed chart never changes.
    pub fn confirm_teeth_by(
        &mut self,
        teeth: &[String],
        confirmed: bool,
        who: &Who,
    ) -> Result<(), EngineError> {
        let by = who.name.trim();
        if by.is_empty() {
            return Err(EngineError::Session(
                "Enter the name of the veterinarian confirming the chart.".into(),
            ));
        }
        let (run, note) = self.latest_note()?;
        if run.signed_at.is_some() {
            return Err(EngineError::Session(
                "This note is signed, so its chart can't change. Add an addendum instead.".into(),
            ));
        }
        let at = self
            .latest_record_index()
            .ok_or_else(|| EngineError::Session("There is no draft yet.".into()))?;
        let drafted = crate::dental::drafted_items(&note);
        let now = Local::now();
        for tooth in teeth {
            let items: Vec<String> = drafted
                .iter()
                .filter(|i| i["tooth"].as_str() == Some(tooth.as_str()))
                .map(|i| crate::dental::item_key(i))
                .collect();
            if items.is_empty() {
                return Err(EngineError::Session(format!(
                    "{tooth} has no drafted chart entries to confirm."
                )));
            }
            let checks = &mut self.meta.runs[at].chart_checks;
            checks.retain(|c| c.tooth != *tooth);
            if confirmed {
                checks.push(ChartCheck {
                    tooth: tooth.clone(),
                    items,
                    by: by.to_string(),
                    who: who.role.is_some().then(|| who.clone()),
                    at: now,
                });
            }
        }
        self.save_meta()
    }

    /// Runs of this handle's animal (all of them, in order).
    pub fn my_runs(&self) -> impl Iterator<Item = &RunInfo> {
        self.meta.runs.iter().filter(|r| self.mine(r))
    }

    /// The latest record run's saved note (`note` from `note.json`).
    pub fn latest_note(&self) -> Result<(RunInfo, serde_json::Value), EngineError> {
        let (run, saved) = self.latest_saved()?;
        Ok((run, saved["note"].clone()))
    }

    /// The latest record run's whole `note.json`: the note, its checks, and
    /// the normal findings from the template waiting beside it (ADR-0032).
    pub fn latest_saved(&self) -> Result<(RunInfo, serde_json::Value), EngineError> {
        let run = self
            .latest_record()
            .cloned()
            .ok_or_else(|| EngineError::Session("There is no draft yet. Draft a note first.".into()))?;
        let path = self.dir.join(&run.dir).join("note.json");
        let text = crate::vault::read_to_string(&path).map_err(io(&path))?;
        let saved: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| EngineError::Session(format!("{}: {e}", path.display())))?;
        Ok((run, saved))
    }

    /// The normal findings from the template waiting in the latest draft
    /// (none when there is no draft).
    pub fn pending_defaults(&self) -> Vec<crate::defaults::PendingDefault> {
        self.latest_saved().map(|(_, saved)| crate::defaults::pending_in(&saved)).unwrap_or_default()
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
            .find(|r| r.document.is_record() && r.set_aside.is_none() && r.pet == pet)
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

    /// Save `session.json` after changing `meta` directly (encrypted sync
    /// merging a visit from another device, ADR-0017 section 6).
    pub fn save(&self) -> Result<(), EngineError> {
        self.save_meta()
    }

    fn save_meta(&self) -> Result<(), EngineError> {
        let path = self.dir.join(SESSION_FILE);
        let text = serde_json::to_string_pretty(&self.meta).expect("meta serialises");
        crate::vault::write(&path, text.as_bytes()).map_err(io(&path))
    }
}

/// Copy a run folder's files (one level), decrypted from the source and
/// written under this device's data key.
fn copy_run_files(from: &Path, to: &Path) -> Result<(), EngineError> {
    fs::create_dir_all(to).map_err(io(to))?;
    for entry in fs::read_dir(from).map_err(io(from))? {
        let path = entry.map_err(io(from))?.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name() else { continue };
        let plain = crate::vault::read(&path).map_err(io(&path))?;
        let target = to.join(name);
        crate::vault::write(&target, &plain).map_err(io(&target))?;
    }
    Ok(())
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
            template_defaults: Vec::new(),
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
        let tag = crate::device::for_root(&root).tag;
        assert!(run1.ends_with(format!("001_{tag}_gemma-4-e4b-it-q4-0")), "{}", run1.display());
        // New runs carry a UUIDv7 run id and the device's tag.
        let first = &s.meta.runs[0];
        assert_eq!(
            first.run_id.as_deref().map(|id| uuid::Uuid::parse_str(id).unwrap().get_version_num()),
            Some(7)
        );
        assert_eq!(first.device.as_deref(), Some(tag.as_str()));
        assert_eq!(first.id(), first.run_id.as_deref().unwrap());
        assert_ne!(s.meta.runs[1].id(), first.id());
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
            outcome: None,
            announced_at_secs: None,
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
            outcome: None,
            announced_at_secs: None,
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

    /// ADR-0023 sections 7 and 8: a phone call's details and its announcement.
    #[test]
    fn a_phone_call_keeps_its_details_and_the_clients_answer() {
        let _plain = crate::vault::tests::SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        let mut s = Session::create_live(&tmp.path().join("sessions"), "Bella").unwrap();
        let call = CallInfo {
            direction: CallDirection::Outgoing,
            number: "+15555550142".into(),
            started_at: Local::now(),
            duration_secs: 0.0,
            call_sid: None,
            clinic_label: "Vet".into(),
            declined_at_secs: None,
        };
        s.set_call(call.clone()).unwrap();
        // An announcement played from the clinic's clip is allowed under both rules.
        let announced = Consent {
            rule: ConsentRule::AllParties,
            method: ConsentMethod::Announced,
            confirmed_by: "Dr Lee".into(),
            confirmed_at: None,
            script: Some("This call is recorded.".into()),
            outcome: Some(ConsentOutcome::Agreed),
            announced_at_secs: Some(0.0),
        };
        announced.check().unwrap();
        s.set_consent(announced).unwrap();
        std::fs::write(s.audio_path(), b"RIFF").unwrap();
        s.finish_live_audio("phone call", 252.0).unwrap();
        let back = Session::open(&s.dir).unwrap().meta;
        assert_eq!(back.source, PHONE_CALL_SOURCE, "a call keeps its source after the audio is saved");
        assert_eq!(back.call.as_ref().unwrap(), &call);
        let consent = back.consent.unwrap();
        assert_eq!(consent.outcome, Some(ConsentOutcome::Agreed));
        assert_eq!(consent.announced_at_secs, Some(0.0));
        s.set_consent_outcome(ConsentOutcome::StayedOnLine, None).unwrap();
        let again = Session::open(&s.dir).unwrap().meta.consent.unwrap();
        assert_eq!(again.outcome, Some(ConsentOutcome::StayedOnLine));
        assert_eq!(again.confirmed_at, consent.confirmed_at, "first confirmation time kept");
        assert_eq!(again.announced_at_secs, Some(0.0));
        assert_eq!(ConsentMethod::parse("announced"), Some(ConsentMethod::Announced));
        assert_eq!(call.speaker_names(), ["Vet".to_string(), "Client".to_string()]);
        assert!(call.prompt_context().contains("Speaker 2 is the client"));
        // Visits saved before calls existed still open, with no call.
        let mut old: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(s.dir.join(SESSION_FILE)).unwrap()).unwrap();
        old.as_object_mut().unwrap().remove("call");
        old["consent"].as_object_mut().unwrap().remove("outcome");
        let meta: SessionMeta = serde_json::from_value(old).unwrap();
        assert!(meta.call.is_none() && meta.consent.unwrap().outcome.is_none());
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

    /// A draft with normal findings from the template waiting (ADR-0032).
    fn draft_with_defaults(name: &str) -> (tempfile::TempDir, Session, LoadedTemplate, NoteResult) {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join(format!("{name}.wav"));
        fs::write(&src, b"RIFF....WAVE").unwrap();
        let mut s = Session::create_from_file(&tmp.path().join("sessions"), &src, 3.0).unwrap();
        let t = Transcript {
            segments: vec![Segment {
                id: 0,
                start: 0.0,
                end: 1.0,
                speaker: Some(0),
                text: "Elle se gratte l'oreille gauche".into(),
                hints: Vec::new(),
                check: None,
            }],
        };
        s.save_transcript(&t, "parakeet", false, &Default::default()).unwrap();
        let template = LoadedTemplate::from_yaml(crate::defaults::tests::EXAM).unwrap();
        let mut result = note_result();
        result.template = template.label();
        result.note = serde_json::json!({
            "motif": [{"text": "Se gratte l'oreille gauche", "evidence": [0]}],
            "examen_physique": []
        });
        let all = crate::defaults::all(&template);
        result.template_defaults = crate::defaults::undiscussed(&template, &mut result.note, Some(&t), all);
        s.save_run("gemma", &result, &template).unwrap();
        (tmp, s, template, result)
    }

    /// ADR-0032: the engine never signs while a normal finding from the
    /// template waits; once each is confirmed or removed, it signs.
    #[test]
    fn signing_waits_until_every_template_default_is_confirmed_or_removed() {
        use crate::defaults::{SettleRequest, settle};
        let _plain = crate::vault::tests::SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let (_tmp, mut s, template, result) = draft_with_defaults("Mina exam");
        let labels: Vec<String> = s.pending_defaults().into_iter().map(|d| d.label).collect();
        assert_eq!(labels, ["Yeux", "Cœur", "Tartre"], "Oreilles was said");
        let err = s.sign_latest("Dr Martin").unwrap_err().to_string();
        assert!(err.contains("normal findings from the template"), "{err}");
        assert!(err.contains("Yeux, Cœur, Tartre"), "{err}");
        assert!(!s.is_signed());

        // Confirm one: still refused for the others.
        let transcript = s.load_transcript().unwrap();
        let confirm = SettleRequest {
            heading: "examen_physique".into(),
            labels: vec!["Yeux".into()],
            confirm: true,
            text: None,
        };
        let pending = s.pending_defaults();
        let r = settle(&template, &transcript, &result.note, &pending, &confirm, "Dr Martin", "x").unwrap();
        s.save_run("edited", &r, &template).unwrap();
        let err = s.sign_latest("Dr Martin").unwrap_err().to_string();
        assert!(err.contains("Cœur, Tartre") && !err.contains("Yeux"), "{err}");

        // Confirm the rest: the note signs, with the vet's own lines in it.
        let all = SettleRequest { labels: Vec::new(), ..confirm };
        let (_, note) = s.latest_note().unwrap();
        let pending = s.pending_defaults();
        let r = settle(&template, &transcript, &note, &pending, &all, "Dr Martin", "x").unwrap();
        assert!(r.template_defaults.is_empty());
        s.save_run("edited", &r, &template).unwrap();
        s.sign_latest("Dr Martin").unwrap();
        assert!(s.is_signed());
        let (_, signed) = s.latest_note().unwrap();
        let lines: Vec<&str> =
            signed["examen_physique"].as_array().unwrap().iter().filter_map(|i| i["text"].as_str()).collect();
        assert_eq!(
            lines,
            ["Yeux: Normal, aucun écoulement", "Cœur: Rythme régulier, pas de souffle", "Tartre: Absent"]
        );
        assert!(signed["examen_physique"].as_array().unwrap().iter().all(|i| i["entered_by"] == "Dr Martin"));
    }

    /// ADR-0032: a 0.2.4 device (an iPhone through the clinic hub) reads a
    /// 0.2.5 note through `note` in note.json, the markdown, and the template
    /// snapshot. None of them holds a default as a statement, and the note
    /// still fits the schema a 0.2.4 template makes, so it can be edited there.
    #[test]
    fn an_older_version_reads_a_note_with_defaults_and_shows_none_of_them() {
        let _plain = crate::vault::tests::SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let (_tmp, s, template, _) = draft_with_defaults("Mina compat");
        let (run, saved) = s.latest_saved().unwrap();
        assert_eq!(crate::defaults::pending_in(&saved).len(), 3, "0.2.5 sees them beside the note");
        // What 0.2.4 reads: `note` (Session::latest_note, the app's NoteView,
        // Copy and the PDF) and note.md (the phone's draft).
        let (_, note) = s.latest_note().unwrap();
        let markdown = crate::vault::read_to_string(&s.dir.join(&run.dir).join("note.md")).unwrap();
        for default in ["Normal, aucun écoulement", "Rythme régulier", "Absent"] {
            assert!(!note.to_string().contains(default), "{default} in the note");
            assert!(!markdown.contains(default), "{default} in note.md");
        }
        assert!(note["examen_physique"].as_array().unwrap().is_empty());
        // The 0.2.4 template has no `defaults` (serde ignores the key), so its
        // schema is the 0.2.5 schema: the note fits it, with no `system` tag.
        let yaml = crate::vault::read_to_string(&s.dir.join(&run.dir).join(RUN_TEMPLATE_FILE)).unwrap();
        assert!(yaml.contains("defaults:"), "the snapshot keeps them for 0.2.5");
        let mut old = template.template.clone();
        for sec in &mut old.sections {
            sec.defaults.clear();
        }
        let old = LoadedTemplate::new(old).unwrap();
        assert_eq!(old.schema, template.schema);
        crate::edit::validate_edited(&old, &note).unwrap();
        let empty = Transcript { segments: Vec::new() };
        let shown = crate::render::markdown(&old, &note, &empty, &Default::default());
        assert!(shown.contains("## Examen physique\n\nNot discussed."), "{shown}");
    }

    /// ADR-0021 decision 10: drafted chart entries are unconfirmed until a
    /// veterinarian confirms each tooth; the engine refuses to sign until
    /// then, records who and when, carries confirmations over a hand edit
    /// for the entries that didn't change, and a signed chart never changes.
    #[test]
    fn a_drafted_tooth_chart_signs_only_once_a_vet_confirms_every_tooth() {
        let _plain = crate::vault::tests::SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("Juno dental.wav");
        fs::write(&src, b"RIFF....WAVE").unwrap();
        let mut s = Session::create_from_file(&tmp.path().join("sessions"), &src, 3.0).unwrap();
        let template = LoadedTemplate::builtin("dental").unwrap().for_species(Some("dog"));
        let item = |tooth: &str, finding: &str, ev: u32| {
            serde_json::json!({"tooth": tooth, "text": finding, "finding": finding, "detail": null,
                "grade": null, "mm": null, "planned": false, "evidence": [ev]})
        };
        let mut result = note_result();
        result.template = template.label();
        result.note = serde_json::json!({"dental_chart": [
            item("108", "fracture", 0), item("108", "extraction", 1), item("209", "missing", 2)
        ]});
        s.save_run("gemma", &result, &template).unwrap();
        let vet = Who::named("Dr Rivera");

        // Every drafted tooth is a draft: signing is refused, by the engine.
        assert_eq!(s.unconfirmed_teeth(), ["108", "209"]);
        let err = s.sign_latest("Dr Rivera").unwrap_err().to_string();
        assert!(err.contains("Not confirmed yet: 108, 209"), "{err}");
        assert!(!s.is_signed());

        // One tooth confirmed: still refused for the other.
        s.confirm_teeth_by(&["108".into()], true, &vet).unwrap();
        assert_eq!(s.unconfirmed_teeth(), ["209"]);
        assert!(s.sign_latest("Dr Rivera").unwrap_err().to_string().contains("209"));
        // Who and when, with exactly the entries confirmed, kept with the visit.
        let reopened = Session::open(&s.dir).unwrap();
        let check = &reopened.latest_record().unwrap().chart_checks[0];
        assert_eq!((check.tooth.as_str(), check.by.as_str(), check.items.len()), ("108", "Dr Rivera", 2));
        assert!(check.at <= Local::now());
        // Taken back; a tooth with no drafted entry can't be confirmed.
        s.confirm_teeth_by(&["108".into()], false, &vet).unwrap();
        assert_eq!(s.unconfirmed_teeth(), ["108", "209"]);
        assert!(s.confirm_teeth_by(&["404".into()], true, &vet).is_err());
        assert!(s.confirm_teeth_by(&["108".into()], true, &Who::named("  ")).is_err());

        // A new draft starts unconfirmed, even with the same entries.
        s.confirm_teeth_by(&["108".into(), "209".into()], true, &vet).unwrap();
        assert!(s.unconfirmed_teeth().is_empty());
        s.save_run("gemma", &result, &template).unwrap();
        assert_eq!(s.unconfirmed_teeth(), ["108", "209"], "a fresh draft is checked again");

        // A hand edit: the changed entry is the vet's own; the unchanged
        // entries keep their confirmation.
        s.confirm_teeth_by(&["108".into()], true, &vet).unwrap();
        let mut edited = result.clone();
        edited.note["dental_chart"][2]["finding"] = serde_json::json!("normal");
        edited.note["dental_chart"][2]["entered_by"] = serde_json::json!("Dr Rivera");
        edited.edit = Some(crate::edit::Edit {
            heading: "dental_chart".into(),
            by: "Dr Rivera".into(),
            from_run: None,
            from_run_id: None,
            changed: vec!["/dental_chart/2".into()],
            removed: 1,
            before: serde_json::Value::Null,
            defaults_confirmed: Vec::new(),
            defaults_removed: Vec::new(),
        });
        s.save_run("edited", &edited, &template).unwrap();
        assert!(s.unconfirmed_teeth().is_empty(), "108 carried over, 209 is the vet's own");

        // Signed: the chart is read-only.
        s.sign_latest("Dr Rivera").unwrap();
        let err = s.confirm_teeth_by(&["108".into()], false, &vet).unwrap_err().to_string();
        assert!(err.contains("signed"), "{err}");
        assert_eq!(Session::open(&s.dir).unwrap().latest_record().unwrap().chart_checks.len(), 1);
    }

    /// A note without a tooth chart signs as before.
    #[test]
    fn a_note_without_a_chart_needs_no_confirmation() {
        let _plain = crate::vault::tests::SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("Mochi.wav");
        fs::write(&src, b"RIFF....WAVE").unwrap();
        let mut s = Session::create_from_file(&tmp.path().join("sessions"), &src, 3.0).unwrap();
        s.save_run("gemma", &note_result(), &LoadedTemplate::builtin("soap").unwrap()).unwrap();
        assert!(s.unconfirmed_teeth().is_empty());
        s.sign_latest("Dr Rivera").unwrap();
    }

    // ---- sync groundwork (ADR-0017 sections 6 and 9) ------------------------

    /// A visit saved by 0.1.x: `runs/NNN_<model>` folders, no run ids.
    const OLD_SESSION: &str = r#"{
      "id": "0199a000-0000-7000-8000-0000000000aa",
      "app_version": "0.1.1",
      "created_at": "2026-10-10T09:30:00-04:00",
      "source": "live recording (Microphone)",
      "audio_sha256": "",
      "audio_secs": 61.0,
      "transcription": null,
      "runs": [
        {"dir": "runs/001_gemma-4-e4b-it-q4-0", "llm": "gemma-4-e4b-it-q4_0", "template": "soap@0.1.0",
         "created_at": "2026-10-10T09:31:00-04:00", "faithfulness_passed": true, "total_ms": 1000,
         "signed_at": "2026-10-10T09:40:00-04:00", "signed_by": "Dr Patel"}
      ]
    }"#;

    #[test]
    fn old_visits_load_and_new_runs_get_ids_beside_them() {
        let _plain = crate::vault::tests::SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("sessions");
        let dir = root.join("2026-10-10_093000_maple");
        fs::create_dir_all(dir.join("runs/001_gemma-4-e4b-it-q4-0")).unwrap();
        fs::write(dir.join(SESSION_FILE), OLD_SESSION).unwrap();
        fs::write(dir.join("runs/001_gemma-4-e4b-it-q4-0/note.json"), b"{\"note\":{}}").unwrap();
        fs::write(dir.join("runs/001_gemma-4-e4b-it-q4-0/note.md"), b"# SOAP note\n").unwrap();

        let mut s = Session::open(&dir).unwrap();
        let old = s.meta.runs[0].clone();
        assert_eq!((old.run_id.as_deref(), old.device.as_deref()), (None, None));
        assert_eq!(old.id(), "runs/001_gemma-4-e4b-it-q4-0", "an old run is known by its folder");
        assert!(s.is_signed());
        // An addendum to the old signed note still works, and the file it
        // writes back keeps the old run as it was.
        s.add_addendum("Recheck in 2 weeks.", "Dr Patel").unwrap();
        let text = fs::read_to_string(dir.join(SESSION_FILE)).unwrap();
        assert!(!text.contains("run_id"), "nothing is invented for an old run");
        // A new run beside it gets the next number, the tag and an id.
        let next = s.save_run("gemma", &note_result(), &LoadedTemplate::builtin("soap").unwrap()).unwrap();
        let tag = crate::device::for_root(&root).tag;
        assert!(next.ends_with(format!("002_{tag}_gemma")));
        assert!(Session::open(&dir).unwrap().meta.runs[1].run_id.is_some());
    }

    /// Golden: one visit, drafted offline on two devices, then merged both ways.
    #[test]
    fn runs_from_two_devices_merge_without_colliding() {
        crate::vault::tests::with_key(|| {
            let tmp = tempfile::tempdir().unwrap();
            let (root_a, root_b) = (tmp.path().join("a"), tmp.path().join("b"));
            // Device A records; device B adopts the visit (as sync will).
            let mut a = Session::create_live(&root_a, "Maple").unwrap();
            let soap = LoadedTemplate::builtin("soap").unwrap();
            a.save_run("gemma-4-e4b", &note_result(), &soap).unwrap();
            let mut b = Session::adopt(&root_b, a.meta.clone()).unwrap();
            assert_eq!(b.meta.id, a.meta.id, "the visit keeps its id");
            assert_eq!(
                Session::adopt(&root_b, a.meta.clone()).unwrap().dir,
                b.dir,
                "adopting twice opens it"
            );
            // The first run's files come across with the merge.
            assert_eq!(b.merge_runs_from(&a).unwrap(), 1);

            // Offline, both draft a second run: same number, different devices.
            a.save_run("gemma-4-e4b", &note_result(), &soap).unwrap();
            b.save_run("gemma-4-e4b", &note_result(), &soap).unwrap();
            let (tag_a, tag_b) = (crate::device::for_root(&root_a).tag, crate::device::for_root(&root_b).tag);
            assert_ne!(a.meta.runs[1].dir, b.meta.runs[1].dir, "002_{tag_a}_… and 002_{tag_b}_…");
            b.sign_latest("Dr Okafor").unwrap();

            // They meet.
            assert_eq!(a.merge_runs_from(&b).unwrap(), 1);
            assert_eq!(b.merge_runs_from(&a).unwrap(), 1);
            assert_eq!(a.merge_runs_from(&b).unwrap(), 0, "merging again brings nothing");
            let ids = |s: &Session| s.meta.runs.iter().map(|r| r.id().to_string()).collect::<Vec<_>>();
            assert_eq!(ids(&a), ids(&b), "both hold the same runs, in drafting order");
            assert_eq!(a.meta.runs.len(), 3);
            let tags: Vec<_> = a.meta.runs.iter().map(|r| r.device.clone().unwrap()).collect();
            assert!(tags.contains(&tag_a) && tags.contains(&tag_b));
            // B's signature came across unchanged, and the files open under A's key.
            let signed = a.meta.runs.iter().find(|r| r.signed_by.is_some()).unwrap();
            assert_eq!(signed.signed_by.as_deref(), Some("Dr Okafor"));
            assert_eq!(signed.device.as_deref(), Some(tag_b.as_str()));
            let md = crate::vault::read_to_string(&a.dir.join(&signed.dir).join("note.md")).unwrap();
            assert_eq!(md, "# SOAP note\n");
            assert!(crate::vault::is_encrypted(&a.dir.join(&signed.dir).join("note.json")));
            // Saved: a reopened copy shows the same runs.
            assert_eq!(ids(&Session::open(&a.dir).unwrap()), ids(&a));
            // Copies of two different visits don't merge.
            let other = Session::create_live(&root_a, "Pepper").unwrap();
            assert!(a.merge_runs_from(&other).is_err());
        });
    }
}
