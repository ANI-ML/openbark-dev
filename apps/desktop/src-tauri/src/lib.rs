//! OpenBark desktop: Tauri commands over the shared app layer ([`os_app::App`]).
//!
//! Web mode (`openbark serve`) exposes the same operations over HTTP, so the
//! UI behaves identically in both. Events: `transcript-segment`, `note-token`,
//! `audio-level`, `download-progress`.

use std::path::Path;
use std::sync::Arc;

use os_app::{App, AppError, Events, Patient, RecordingSource};
use os_engine::Segment;
use os_models::ModelStore;
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

struct TauriEvents(AppHandle);

#[derive(Clone, Serialize)]
struct Level {
    level: f32,
    seconds: f32,
}

#[derive(Clone, Serialize)]
struct Download<'a> {
    model: &'a str,
    done: u64,
    total: u64,
}

impl Events for TauriEvents {
    fn segment(&self, s: &Segment) {
        let _ = self.0.emit("transcript-segment", s);
    }
    fn token(&self, p: &str) {
        let _ = self.0.emit("note-token", p);
    }
    fn level(&self, level: f32, seconds: f32) {
        let _ = self.0.emit("audio-level", Level { level, seconds });
    }
    fn download(&self, model: &str, done: u64, total: u64) {
        let _ = self.0.emit("download-progress", Download { model, done, total });
    }
}

type AppState = Arc<App>;

/// Run blocking app work on a worker thread.
async fn work<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, AppError> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())?.map_err(|e| e.0)
}

#[tauri::command]
fn status(app: State<'_, AppState>) -> os_app::Status {
    app.status()
}

#[tauri::command]
fn input_devices(app: State<'_, AppState>) -> Vec<String> {
    app.input_devices()
}

#[tauri::command]
fn list_sessions(app: State<'_, AppState>) -> Vec<os_app::SessionSummary> {
    app.list_sessions()
}

#[tauri::command]
async fn open_session(app: State<'_, AppState>, id: String) -> Result<os_app::SessionView, String> {
    let app = app.inner().clone();
    work(move || app.open_session(&id)).await
}

#[tauri::command]
async fn import_file(
    handle: AppHandle,
    app: State<'_, AppState>,
    path: String,
    speakers: bool,
    patient: Option<Patient>,
) -> Result<os_app::SessionView, String> {
    let app = app.inner().clone();
    work(move || app.import_file(Path::new(&path), speakers, patient, &TauriEvents(handle))).await
}

#[tauri::command]
async fn start_recording(
    handle: AppHandle,
    app: State<'_, AppState>,
    device: Option<String>,
    speakers: bool,
    patient: Option<Patient>,
) -> Result<String, String> {
    let app = app.inner().clone();
    work(move || {
        app.start_recording(
            RecordingSource::Microphone(device),
            speakers,
            patient,
            Arc::new(TauriEvents(handle)),
        )
    })
    .await
}

#[tauri::command]
async fn update_patient(
    app: State<'_, AppState>,
    id: String,
    patient: Patient,
) -> Result<os_app::SessionView, String> {
    let app = app.inner().clone();
    work(move || app.update_patient(&id, patient)).await
}

#[tauri::command]
async fn stop_recording(app: State<'_, AppState>) -> Result<os_app::SessionView, String> {
    let app = app.inner().clone();
    work(move || app.stop_recording()).await
}

#[tauri::command]
async fn generate_note(
    handle: AppHandle,
    app: State<'_, AppState>,
    id: String,
    llm: String,
    template: String,
) -> Result<os_app::NoteResponse, String> {
    let app = app.inner().clone();
    work(move || app.generate_note(&id, &llm, &template, &TauriEvents(handle))).await
}

#[tauri::command]
async fn sign_note(app: State<'_, AppState>, id: String, by: String) -> Result<os_app::SessionView, String> {
    let app = app.inner().clone();
    work(move || app.sign(&id, &by)).await
}

#[tauri::command]
fn template(app: State<'_, AppState>, id: String) -> Result<serde_json::Value, String> {
    app.template(&id).map_err(|e| e.0)
}

#[tauri::command]
fn models(app: State<'_, AppState>) -> Vec<os_app::ModelRow> {
    app.models()
}

#[tauri::command]
async fn download_model(handle: AppHandle, app: State<'_, AppState>, id: String) -> Result<(), String> {
    let app = app.inner().clone();
    work(move || app.download_model(&id, None, &TauriEvents(handle))).await
}

#[tauri::command]
async fn hub_search(app: State<'_, AppState>, q: String) -> Result<Vec<os_models::hub::HubModel>, String> {
    let app = app.inner().clone();
    work(move || app.hub_search(&q)).await
}

#[tauri::command]
async fn hub_repo(app: State<'_, AppState>, id: String) -> Result<os_app::HubRepoView, String> {
    let app = app.inner().clone();
    work(move || app.hub_repo(&id)).await
}

#[tauri::command]
async fn hub_add(
    app: State<'_, AppState>,
    repo: String,
    file: String,
    accept_license: bool,
) -> Result<os_app::ModelRow, String> {
    let app = app.inner().clone();
    work(move || app.hub_add(&repo, &file, accept_license)).await
}

pub fn run() {
    let app: AppState =
        Arc::new(App::new(ModelStore::default_location(), os_engine::session::default_root()));
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(app)
        .invoke_handler(tauri::generate_handler![
            status,
            input_devices,
            list_sessions,
            open_session,
            import_file,
            start_recording,
            stop_recording,
            generate_note,
            sign_note,
            update_patient,
            template,
            models,
            download_model,
            hub_search,
            hub_repo,
            hub_add
        ])
        .run(tauri::generate_context!())
        .expect("error while running OpenBark");
}
