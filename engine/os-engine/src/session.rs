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
use crate::template::LoadedTemplate;
use crate::transcribe::TranscribeStats;
use crate::transcript::Transcript;
use crate::{EngineError, NoteResult};

pub const SESSION_FILE: &str = "session.json";
pub const AUDIO_FILE: &str = "audio.wav";
pub const TRANSCRIPT_FILE: &str = "transcript.jsonl";
pub const RUN_TEMPLATE_FILE: &str = "template.yaml";

/// Default sessions root: `OPENBARK_SESSIONS_DIR`, else `<local data>/OpenBark/sessions`.
pub fn default_root() -> PathBuf {
    if let Some(dir) = std::env::var_os("OPENBARK_SESSIONS_DIR") {
        return PathBuf::from(dir);
    }
    dirs::data_local_dir().unwrap_or_else(|| PathBuf::from(".")).join("OpenBark").join("sessions")
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
    /// Set when this run redrafted one heading of an earlier run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refinement: Option<Refinement>,
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
}

/// Patient and visit details typed in by the clinic, time-stamped.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PatientInfo {
    /// The animal's name.
    pub name: String,
    /// Family or client surname.
    pub last_name: String,
    pub reason: String,
    /// When these details were first entered, and last changed.
    #[serde(default)]
    pub entered_at: Option<DateTime<Local>>,
    #[serde(default)]
    pub updated_at: Option<DateTime<Local>>,
}

impl PatientInfo {
    pub fn is_empty(&self) -> bool {
        self.name.trim().is_empty() && self.last_name.trim().is_empty() && self.reason.trim().is_empty()
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
        parts.join("; ")
    }
}

/// One consult's folder.
#[derive(Debug, Clone)]
pub struct Session {
    pub dir: PathBuf,
    pub meta: SessionMeta,
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
        fs::copy(source, &audio).map_err(io(&audio))?;
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
        };
        let session = Self { dir, meta };
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
        };
        let session = Self { dir, meta };
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
        let text = fs::read_to_string(&path).map_err(io(&path))?;
        let meta = serde_json::from_str(&text)
            .map_err(|e| EngineError::Session(format!("{}: {e}", path.display())))?;
        Ok(Self { dir: dir.to_path_buf(), meta })
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
        fs::write(&path, transcript.to_jsonl()).map_err(io(&path))?;
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
        let text = fs::read_to_string(&path).map_err(io(&path))?;
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
        fs::write(&json, serde_json::to_string_pretty(result).expect("note result serialises"))
            .map_err(io(&json))?;
        let md = dir.join("note.md");
        fs::write(&md, &result.markdown).map_err(io(&md))?;
        let snapshot = dir.join(RUN_TEMPLATE_FILE);
        fs::write(&snapshot, template.to_yaml()).map_err(io(&snapshot))?;
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

    /// Sign the latest run. Refused if it has numbers absent from the
    /// transcript (DESIGN.md §5: hard-fail content blocks signing).
    pub fn sign_latest(&mut self, signed_by: &str) -> Result<(), EngineError> {
        let run = self
            .meta
            .runs
            .last_mut()
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
        run.signed_by = Some(signed_by.trim().to_string());
        self.save_meta()
    }

    /// The latest run's saved note (`note` from `note.json`).
    pub fn latest_note(&self) -> Result<(RunInfo, serde_json::Value), EngineError> {
        let run = self
            .meta
            .runs
            .last()
            .cloned()
            .ok_or_else(|| EngineError::Session("There is no draft yet. Draft a note first.".into()))?;
        let path = self.dir.join(&run.dir).join("note.json");
        let text = fs::read_to_string(&path).map_err(io(&path))?;
        let saved: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| EngineError::Session(format!("{}: {e}", path.display())))?;
        Ok((run, saved["note"].clone()))
    }

    /// The template a run was written with: its snapshot, or for runs saved
    /// before snapshots existed, the template by id as it is now.
    pub fn run_template(&self, run: &RunInfo) -> Result<LoadedTemplate, EngineError> {
        let snapshot = self.dir.join(&run.dir).join(RUN_TEMPLATE_FILE);
        match fs::read_to_string(&snapshot) {
            Ok(yaml) => LoadedTemplate::from_yaml(&yaml),
            Err(_) => LoadedTemplate::resolve(run.template_id()),
        }
    }

    pub fn is_signed(&self) -> bool {
        self.meta.runs.last().is_some_and(|r| r.signed_at.is_some())
    }

    fn save_meta(&self) -> Result<(), EngineError> {
        let path = self.dir.join(SESSION_FILE);
        let tmp = self.dir.join("session.json.tmp");
        fs::write(&tmp, serde_json::to_string_pretty(&self.meta).expect("meta serialises"))
            .map_err(io(&tmp))?;
        fs::rename(&tmp, &path).map_err(io(&path))
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
fn slug(s: &str) -> String {
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

fn sha256_file(path: &Path) -> Result<String, EngineError> {
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
        assert_eq!(s.run_template(&last).unwrap().label(), "soap@0.1.0");
        assert_eq!(s.latest_note().unwrap().1, serde_json::json!({}));

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

        // A second session from the same file in the same second gets its own folder.
        let other = Session::create_from_file(&root, &src, 12.5).unwrap();
        assert_ne!(other.dir, s.dir);
    }
}
