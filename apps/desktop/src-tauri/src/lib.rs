//! OpenBark desktop: Tauri commands over the engine.
//!
//! Every recording becomes a saved session folder (audio, transcript, notes,
//! metadata; see `os_engine::session`). Long-running work runs on blocking
//! threads, keeps the machine awake, and streams progress to the UI:
//! - `transcript-segment`: one [`os_engine::Segment`] as soon as it is recognised
//! - `note-token`: a piece of generated JSON text

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use os_engine::awake::{AwakeGuard, AwakeReason};
use os_engine::session::{self, Session};
use os_engine::{LoadedTemplate, NoteOptions, Segment, TranscribeStats, Transcript};
use os_llm::{LlamaCppBackend, LlamaCppOptions, LlmBackend};
use os_models::ModelStore;
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

struct AppState {
    store: ModelStore,
    sessions_root: PathBuf,
    /// The loaded LLM, kept between notes (loading takes seconds). llama.cpp
    /// allows one backend per process, so switching models replaces it.
    llm: Mutex<Option<(String, Arc<LlamaCppBackend>)>>,
}

#[derive(Serialize)]
struct ModelStatus {
    id: String,
    name: String,
    installed: bool,
}

#[derive(Serialize)]
struct Status {
    product: &'static str,
    version: &'static str,
    models_dir: String,
    sessions_dir: String,
    models: Vec<ModelStatus>,
    llm_backend: String,
}

#[derive(Serialize)]
struct SessionSummary {
    /// On desktop the id is the folder path itself.
    id: String,
    dir: String,
    created_at: String,
    source: String,
    audio_secs: f32,
    runs: usize,
}

#[derive(Serialize)]
struct SessionView {
    id: String,
    dir: String,
    audio_path: String,
    transcript: Option<Transcript>,
    stats: Option<TranscribeStats>,
    /// Markdown of the latest run, if any.
    latest_note: Option<String>,
}

#[derive(Serialize)]
struct NoteResponse {
    markdown: String,
    run_dir: String,
    result: os_engine::NoteResult,
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[tauri::command]
fn status(state: State<'_, AppState>) -> Status {
    let models = os_engine::models::required_status(&state.store)
        .into_iter()
        .map(|(id, name, installed)| ModelStatus { id, name, installed })
        .collect();
    let gpu = os_llm::devices()
        .into_iter()
        .find(|d| d.backend != "CPU")
        .map(|d| format!("{} ({})", d.description, d.backend))
        .unwrap_or_else(|| "CPU".into());
    Status {
        product: os_engine::PRODUCT_NAME,
        version: env!("CARGO_PKG_VERSION"),
        models_dir: state.store.root().display().to_string(),
        sessions_dir: state.sessions_root.display().to_string(),
        models,
        llm_backend: gpu,
    }
}

#[tauri::command]
fn list_sessions(state: State<'_, AppState>) -> Vec<SessionSummary> {
    session::list(&state.sessions_root)
        .into_iter()
        .take(50)
        .map(|s| SessionSummary {
            id: s.dir.display().to_string(),
            dir: s.dir.display().to_string(),
            created_at: s.meta.created_at.format("%Y-%m-%d %H:%M").to_string(),
            source: Path::new(&s.meta.source)
                .file_name()
                .map(|n| n.to_string_lossy().into())
                .unwrap_or_default(),
            audio_secs: s.meta.audio_secs,
            runs: s.meta.runs.len(),
        })
        .collect()
}

#[tauri::command]
fn open_session(dir: String) -> Result<SessionView, String> {
    let s = Session::open(Path::new(&dir)).map_err(err)?;
    let latest_note =
        s.meta.runs.last().and_then(|r| std::fs::read_to_string(s.dir.join(&r.dir).join("note.md")).ok());
    Ok(SessionView {
        id: s.dir.display().to_string(),
        dir: s.dir.display().to_string(),
        audio_path: s.audio_path().display().to_string(),
        transcript: s.load_transcript().ok(),
        stats: s.meta.transcription.as_ref().map(|t| t.stats.clone()),
        latest_note,
    })
}

/// Create a session from a recording file, transcribe it and save the transcript.
#[tauri::command]
async fn transcribe(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    diarize: bool,
) -> Result<SessionView, String> {
    let store = state.store.clone();
    let root = state.sessions_root.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _awake = AwakeGuard::acquire(AwakeReason::Processing);
        let audio = os_asr::Audio::load_wav(Path::new(&path)).map_err(err)?;
        let mut session =
            Session::create_from_file(&root, Path::new(&path), audio.duration_secs()).map_err(err)?;
        let stt = os_engine::models::DEFAULT_STT;
        let transcriber =
            os_engine::models::transcriber(&store, stt, diarize, None, None, None).map_err(err)?;
        let (transcript, stats) = transcriber
            .run(&audio, &mut |seg: &Segment| {
                let _ = app.emit("transcript-segment", seg);
            })
            .map_err(err)?;
        session.save_transcript(&transcript, stt, diarize, &stats).map_err(err)?;
        Ok(SessionView {
            id: session.dir.display().to_string(),
            dir: session.dir.display().to_string(),
            audio_path: session.audio_path().display().to_string(),
            transcript: Some(transcript),
            stats: Some(stats),
            latest_note: None,
        })
    })
    .await
    .map_err(err)?
}

/// Draft a note for a session's transcript and save it as a new run.
#[tauri::command]
async fn generate_note(
    app: AppHandle,
    state: State<'_, AppState>,
    session_dir: String,
    llm: String,
    template: String,
) -> Result<NoteResponse, String> {
    let backend = load_llm(&state, &llm)?;
    tauri::async_runtime::spawn_blocking(move || {
        let _awake = AwakeGuard::acquire(AwakeReason::Processing);
        let mut session = Session::open(Path::new(&session_dir)).map_err(err)?;
        let transcript = session.load_transcript().map_err(err)?;
        let template = LoadedTemplate::resolve(&template).map_err(err)?;
        let result = os_engine::generate_note(
            backend.as_ref(),
            &template,
            &transcript,
            &NoteOptions::default(),
            &mut |piece| {
                let _ = app.emit("note-token", piece);
                true
            },
        )
        .map_err(err)?;
        let run_dir = session.save_run(&llm, &result).map_err(err)?;
        Ok(NoteResponse { markdown: result.markdown.clone(), run_dir: run_dir.display().to_string(), result })
    })
    .await
    .map_err(err)?
}

fn load_llm(state: &State<'_, AppState>, id: &str) -> Result<Arc<LlamaCppBackend>, String> {
    let mut slot = state.llm.lock().map_err(|_| "model lock poisoned".to_string())?;
    if let Some((loaded, backend)) = slot.as_ref() {
        if loaded == id {
            return Ok(backend.clone());
        }
    }
    // Free the previous model before loading another (one backend per process).
    *slot = None;
    let path = os_engine::models::llm_path(&state.store, id).map_err(err)?;
    let backend = Arc::new(LlamaCppBackend::load(LlamaCppOptions::new(path)).map_err(err)?);
    tracing::info!(model = %backend.capabilities().model_name, "llm ready");
    *slot = Some((id.to_string(), backend.clone()));
    Ok(backend)
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(AppState {
            store: ModelStore::default_location(),
            sessions_root: session::default_root(),
            llm: Mutex::new(None),
        })
        .invoke_handler(tauri::generate_handler![
            status,
            list_sessions,
            open_session,
            transcribe,
            generate_note
        ])
        .run(tauri::generate_context!())
        .expect("error while running OpenBark");
}
