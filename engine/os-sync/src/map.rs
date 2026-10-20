//! Visit folders as sync objects (ADR-0017 section 6), and back.
//!
//! | On disk | Object | Changes |
//! |---|---|---|
//! | `session.json` header fields (patient details, consent, who recorded it, the audio's facts, the audio's deletion) | `visit`, one per visit | last writer wins, field by field, by clock |
//! | `transcript.jsonl` and its transcription facts | `transcript` and a blob | last writer wins |
//! | each run folder (`note.json`, `note.md`, `template.yaml`) | `run` and a blob, immutable | never; runs from both devices are kept |
//! | a signature, an addendum | `event`, immutable | never |
//! | a run's tooth chart confirmations | `checks` | last writer wins |
//! | a patient record (`patients.json`) | `patient` | the registry's merge (ADR-0017 section 9) |
//! | stopping a visit's sync | `tombstone` | never |
//! | `audio.wav` | **nothing** | audio never syncs (ADR-0017 section 7) |
//!
//! Object ids are derived under the naming key from the kind and the local
//! id (`crate::keys::object_id`), never from a folder name, which carries
//! the patient's name.

use std::collections::BTreeMap;
use std::path::Path;

use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use os_engine::patients::PatientRecord;
use os_engine::session::{Addendum, ChartCheck, RunInfo, SessionMeta, TranscriptionInfo};
use os_engine::users::Who;

use crate::SyncError;
use crate::guard;
use crate::keys;
use crate::state::FieldSync;

/// The visit header's fields that sync, each merged on its own.
pub const VISIT_FIELDS: [&str; 5] = ["patient", "consent", "recorded_by", "audio", "audio_deleted"];

/// Every object's plaintext: its clock timestamp and its data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Body {
    pub hlc: String,
    pub data: Value,
}

/// A value with the clock timestamp of its last change.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Stamped {
    pub v: Value,
    pub t: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisitBody {
    pub session_id: String,
    pub created_at: DateTime<Local>,
    pub app_version: String,
    /// The sync device that recorded the visit and alone holds its audio.
    pub audio_on: String,
    pub fields: BTreeMap<String, Stamped>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunBody {
    pub session_id: String,
    /// The run as drafted: no signature, addenda, chart checks or sync marks
    /// (those travel as their own objects).
    pub run: RunInfo,
    pub files: Vec<String>,
    /// SHA-256 of each blob part, in order.
    pub parts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptBody {
    pub session_id: String,
    pub info: Option<TranscriptionInfo>,
    pub hash: String,
    pub parts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SignEvent {
    pub at: DateTime<Local>,
    pub by: String,
    #[serde(default)]
    pub signer: Option<Who>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventBody {
    pub session_id: String,
    pub run_id: String,
    #[serde(default)]
    pub sign: Option<SignEvent>,
    #[serde(default)]
    pub addendum: Option<Addendum>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChecksBody {
    pub session_id: String,
    pub run_id: String,
    pub checks: Vec<ChartCheck>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatientBody {
    pub record: PatientRecord,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TombstoneBody {
    pub session_id: String,
    pub objects: Vec<String>,
}

/// What to record in the state once the server has an item.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Commit {
    Header { visit: String, fields: BTreeMap<String, FieldSync>, hash: String },
    Transcript { visit: String, hash: String, hlc: String },
    Run { visit: String, run_id: String },
    Sign { visit: String, run_id: String },
    Addendum { visit: String, id: String },
    Checks { visit: String, run_id: String, hash: String, hlc: String },
    Patient { id: String, hash: String, hlc: String },
    Tombstone { visit: String },
    Delete,
}

/// A stable hash of a JSON value (its serialised text).
pub fn hash_value(v: &Value) -> String {
    hex::encode(keys::sha256(&[v.to_string().as_bytes()]))
}

pub fn hash_bytes(b: &[u8]) -> String {
    hex::encode(keys::sha256(&[b]))
}

/// The header fields of a visit, as JSON.
pub fn field_values(meta: &SessionMeta) -> BTreeMap<String, Value> {
    let mut out = BTreeMap::new();
    out.insert("patient".into(), serde_json::to_value(&meta.patient).expect("serialises"));
    out.insert("consent".into(), serde_json::to_value(&meta.consent).expect("serialises"));
    out.insert("recorded_by".into(), serde_json::to_value(&meta.recorded_by).expect("serialises"));
    out.insert(
        "audio".into(),
        serde_json::json!({"source": meta.source, "secs": meta.audio_secs, "sha256": meta.audio_sha256}),
    );
    out.insert("audio_deleted".into(), serde_json::to_value(&meta.audio_deleted).expect("serialises"));
    out
}

/// Put one synced field's value into a visit header.
pub fn apply_field(meta: &mut SessionMeta, field: &str, v: &Value) -> Result<(), SyncError> {
    let bad = |e: serde_json::Error| SyncError::Format(format!("visit field {field}: {e}"));
    match field {
        "patient" => meta.patient = serde_json::from_value(v.clone()).map_err(bad)?,
        "consent" => meta.consent = serde_json::from_value(v.clone()).map_err(bad)?,
        "recorded_by" => meta.recorded_by = serde_json::from_value(v.clone()).map_err(bad)?,
        "audio_deleted" => meta.audio_deleted = serde_json::from_value(v.clone()).map_err(bad)?,
        "audio" => {
            meta.source = v["source"].as_str().unwrap_or_default().to_string();
            meta.audio_secs = v["secs"].as_f64().unwrap_or_default() as f32;
            meta.audio_sha256 = v["sha256"].as_str().unwrap_or_default().to_string();
        }
        _ => {}
    }
    Ok(())
}

/// When a field was last changed on this device, if the value says
/// (patient details, consent, deleting the recording).
pub fn edit_time(meta: &SessionMeta, field: &str) -> Option<DateTime<Local>> {
    match field {
        "patient" => meta.patient.as_ref().and_then(|p| p.updated_at.or(p.entered_at)),
        "consent" => meta.consent.as_ref().and_then(|c| c.confirmed_at),
        "audio_deleted" => meta.audio_deleted.as_ref().map(|a| a.at),
        _ => None,
    }
}

/// A run as it syncs: what was drafted, without the parts that travel on
/// their own (signature, addenda, chart checks) or are worked out on each
/// device (set aside).
pub fn strip_run(run: &RunInfo) -> RunInfo {
    let mut r = run.clone();
    r.signed_at = None;
    r.signed_by = None;
    r.signer = None;
    r.addenda.clear();
    r.chart_checks.clear();
    r.set_aside = None;
    r
}

/// A run folder's files (one level), decrypted, as one blob's plaintext.
/// Refuses anything that looks like a recording.
pub fn run_files(dir: &Path) -> Result<(Vec<String>, Vec<u8>), SyncError> {
    let mut files: BTreeMap<String, String> = BTreeMap::new();
    let entries = std::fs::read_dir(dir).map_err(|e| SyncError::Io(format!("{}: {e}", dir.display())))?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()).map(String::from) else { continue };
        if name.ends_with(".tmp") {
            continue;
        }
        let bytes = os_engine::vault::read(&path).map_err(|e| SyncError::Io(format!("{}: {e}", path.display())))?;
        guard::refuse_audio_file(&name, &bytes)?;
        let text = match String::from_utf8(bytes) {
            Ok(t) => t,
            Err(e) => format!("base64:{}", keys::b64(e.as_bytes())),
        };
        files.insert(name, text);
    }
    let names = files.keys().cloned().collect();
    Ok((names, serde_json::to_vec(&files).expect("serialises")))
}

/// A run blob back into its files.
pub fn run_files_from(blob: &[u8]) -> Result<Vec<(String, Vec<u8>)>, SyncError> {
    let files: BTreeMap<String, String> =
        serde_json::from_slice(blob).map_err(|_| SyncError::Format("damaged run files".into()))?;
    let mut out = Vec::new();
    for (name, text) in files {
        let safe = !name.is_empty()
            && name.len() <= 64
            && !name.starts_with('.')
            && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
        if !safe {
            return Err(SyncError::Format(format!("a run file has an unsafe name: {name}")));
        }
        let bytes = match text.strip_prefix("base64:") {
            Some(b) => keys::unb64(b)?,
            None => text.into_bytes(),
        };
        guard::refuse_audio_file(&name, &bytes)?;
        out.push((name, bytes));
    }
    Ok(out)
}

/// A safe run folder name from another device: `<leaf>` under `runs/`.
pub fn safe_run_leaf(dir: &str) -> Option<String> {
    let leaf = dir.strip_prefix("runs/").unwrap_or(dir);
    let ok = !leaf.is_empty()
        && leaf.len() <= 120
        && !leaf.starts_with('.')
        && leaf.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    ok.then(|| leaf.to_string())
}

/// Whether a visit's patient record ids include `id`.
pub fn visit_patients(meta: &SessionMeta) -> Vec<String> {
    meta.patient
        .as_ref()
        .map(|p| p.patient_id.iter().chain(p.other_ids.iter()).cloned().collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_files_round_trip_and_refuse_audio() {
        os_engine::vault::disable();
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("note.json"), b"{\"note\":{}}").unwrap();
        std::fs::write(dir.path().join("note.md"), "## Plan\n".as_bytes()).unwrap();
        let (names, blob) = run_files(dir.path()).unwrap();
        assert_eq!(names, ["note.json", "note.md"]);
        let back = run_files_from(&blob).unwrap();
        assert_eq!(back[0], ("note.json".to_string(), b"{\"note\":{}}".to_vec()));
        std::fs::write(dir.path().join("audio.wav"), b"RIFF....WAVE").unwrap();
        assert!(run_files(dir.path()).is_err(), "a recording in a run folder stops the upload");
        let evil = serde_json::to_vec(&BTreeMap::from([("../session.json".to_string(), "x".to_string())])).unwrap();
        assert!(run_files_from(&evil).is_err());
        assert_eq!(safe_run_leaf("runs/002_a3f9_gemma"), Some("002_a3f9_gemma".into()));
        assert_eq!(safe_run_leaf("runs/../x"), None);
    }
}
