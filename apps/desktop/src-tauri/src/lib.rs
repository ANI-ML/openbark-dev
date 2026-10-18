//! OpenBark desktop: Tauri commands over the shared app layer ([`os_app::App`]).
//!
//! Web mode (`openbark serve`) exposes the same operations over HTTP, so the
//! UI behaves identically in both. Events: `transcript-segment`, `note-token`,
//! `audio-level`, `download-progress`.

use std::path::Path;
use std::sync::Arc;

use os_app::{App, AppError, Events, Patient, PdfRequest, RecordingConsent, RecordingSource, RefineRequest};
use os_engine::Segment;
use os_engine::template::{Standard, Template};
use os_engine::template_store::TemplateSummary;
use os_engine::users::{Role, User};
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
async fn open_session(
    app: State<'_, AppState>,
    id: String,
    pet: Option<String>,
) -> Result<os_app::SessionView, String> {
    let app = app.inner().clone();
    work(move || app.open_session(&id, pet.as_deref())).await
}

#[tauri::command]
async fn import_file(
    handle: AppHandle,
    app: State<'_, AppState>,
    path: String,
    speakers: bool,
    patient: Option<Patient>,
    consent: Option<RecordingConsent>,
    by: Option<String>,
) -> Result<os_app::SessionView, String> {
    let app = app.inner().clone();
    work(move || {
        app.import_file(Path::new(&path), speakers, patient, consent, by.as_deref(), &TauriEvents(handle))
    })
    .await
}

#[tauri::command]
async fn start_recording(
    handle: AppHandle,
    app: State<'_, AppState>,
    device: Option<String>,
    speakers: bool,
    patient: Option<Patient>,
    consent: Option<RecordingConsent>,
    by: Option<String>,
) -> Result<String, String> {
    let app = app.inner().clone();
    work(move || {
        app.start_recording(
            RecordingSource::Microphone(device),
            speakers,
            patient,
            consent,
            by.as_deref(),
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
#[allow(clippy::too_many_arguments)]
async fn generate_note(
    handle: AppHandle,
    app: State<'_, AppState>,
    id: String,
    llm: String,
    template: String,
    language: Option<String>,
    pet: Option<String>,
    by: Option<String>,
) -> Result<os_app::NoteResponse, String> {
    let app = app.inner().clone();
    work(move || {
        app.generate_note(
            &id,
            &llm,
            &template,
            language.as_deref(),
            pet.as_deref(),
            by.as_deref(),
            &TauriEvents(handle),
        )
    })
    .await
}

#[tauri::command]
async fn sign_note(
    app: State<'_, AppState>,
    id: String,
    by: String,
    pet: Option<String>,
) -> Result<os_app::SessionView, String> {
    let app = app.inner().clone();
    work(move || app.sign(&id, &by, pet.as_deref())).await
}

#[tauri::command]
async fn refine_note(
    handle: AppHandle,
    app: State<'_, AppState>,
    id: String,
    request: RefineRequest,
) -> Result<os_app::NoteResponse, String> {
    let app = app.inner().clone();
    work(move || app.refine_note(&id, &request, &TauriEvents(handle))).await
}

#[tauri::command]
async fn edit_note(
    app: State<'_, AppState>,
    id: String,
    heading: String,
    value: serde_json::Value,
    by: String,
    pet: Option<String>,
) -> Result<os_app::NoteResponse, String> {
    let app = app.inner().clone();
    work(move || app.edit_note(&id, &heading, value, &by, pet.as_deref())).await
}

#[tauri::command]
async fn add_addendum(
    app: State<'_, AppState>,
    id: String,
    text: String,
    by: String,
    pet: Option<String>,
) -> Result<os_app::SessionView, String> {
    let app = app.inner().clone();
    work(move || app.add_addendum(&id, &text, &by, pet.as_deref())).await
}

/// An encounter's recording, decrypted, for the player (ADR-0008: audio is
/// encrypted on disk, so the file can't be handed to the webview directly).
#[tauri::command]
async fn session_audio(app: State<'_, AppState>, id: String) -> Result<tauri::ipc::Response, String> {
    let app = app.inner().clone();
    work(move || app.session_audio(&id)).await.map(tauri::ipc::Response::new)
}

#[tauri::command]
async fn encrypt_existing(app: State<'_, AppState>) -> Result<usize, String> {
    let app = app.inner().clone();
    work(move || app.encrypt_existing()).await
}

#[tauri::command]
fn recovery_key(app: State<'_, AppState>) -> Result<String, String> {
    app.recovery_key().map_err(|e| e.0)
}

/// Write the note as a PDF to `path` (chosen in a save dialog).
#[tauri::command]
async fn export_pdf(
    app: State<'_, AppState>,
    id: String,
    path: String,
    request: PdfRequest,
) -> Result<(), String> {
    let app = app.inner().clone();
    work(move || {
        let (pdf, _) = app.export_pdf(&id, &request)?;
        std::fs::write(&path, pdf)?;
        Ok(())
    })
    .await
}

/// The suggested file name for a note's PDF.
#[tauri::command]
fn pdf_name(app: State<'_, AppState>, id: String, request: PdfRequest) -> Result<String, String> {
    app.export_pdf(&id, &request).map(|(_, name)| name).map_err(|e| e.0)
}

#[tauri::command]
fn list_users(app: State<'_, AppState>) -> Result<Vec<User>, String> {
    app.list_users().map_err(|e| e.0)
}

#[tauri::command]
fn add_user(
    app: State<'_, AppState>,
    name: String,
    role: Role,
    pin: String,
    by: Option<String>,
) -> Result<User, String> {
    app.add_user(&name, role, &pin, by.as_deref()).map_err(|e| e.0)
}

#[tauri::command]
fn remove_user(app: State<'_, AppState>, uid: String, by: Option<String>) -> Result<(), String> {
    app.remove_user(&uid, by.as_deref()).map_err(|e| e.0)
}

#[tauri::command]
fn set_user_pin(
    app: State<'_, AppState>,
    uid: String,
    pin: String,
    by: Option<String>,
) -> Result<(), String> {
    app.set_user_pin(&uid, &pin, by.as_deref()).map_err(|e| e.0)
}

#[tauri::command]
async fn verify_user(app: State<'_, AppState>, uid: String, pin: String) -> Result<User, String> {
    let app = app.inner().clone();
    work(move || app.verify_user(&uid, &pin)).await
}

#[tauri::command]
fn list_templates(app: State<'_, AppState>) -> Vec<TemplateSummary> {
    app.list_templates()
}

#[tauri::command]
fn template(app: State<'_, AppState>, id: String) -> Result<os_app::TemplateView, String> {
    app.template(&id).map_err(|e| e.0)
}

#[tauri::command]
fn preview_template(app: State<'_, AppState>, template: Template) -> Result<os_app::TemplateView, String> {
    app.preview_template(template).map_err(|e| e.0)
}

#[tauri::command]
fn save_template(app: State<'_, AppState>, template: Template) -> Result<os_app::TemplateView, String> {
    app.save_template(template).map_err(|e| e.0)
}

#[tauri::command]
fn delete_template(app: State<'_, AppState>, id: String) -> Result<(), String> {
    app.delete_template(&id).map_err(|e| e.0)
}

#[tauri::command]
fn add_standard(
    app: State<'_, AppState>,
    id: String,
    heading: String,
    text: String,
    by: Option<String>,
) -> Result<Standard, String> {
    app.add_standard(&id, &heading, &text, by.as_deref()).map_err(|e| e.0)
}

#[tauri::command]
fn remove_standard(
    app: State<'_, AppState>,
    id: String,
    sid: String,
) -> Result<os_app::TemplateView, String> {
    app.remove_standard(&id, &sid).map_err(|e| e.0)
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
            refine_note,
            edit_note,
            add_addendum,
            export_pdf,
            pdf_name,
            session_audio,
            list_users,
            add_user,
            remove_user,
            set_user_pin,
            verify_user,
            encrypt_existing,
            recovery_key,
            list_templates,
            template,
            preview_template,
            save_template,
            delete_template,
            add_standard,
            remove_standard,
            models,
            download_model,
            hub_search,
            hub_repo,
            hub_add
        ])
        .run(tauri::generate_context!())
        .expect("error while running OpenBark");
}
