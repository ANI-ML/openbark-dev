//! OpenBark desktop: Tauri commands over the shared app layer ([`os_app::App`]).
//!
//! Web mode (`openbark serve`) exposes the same operations over HTTP, so the
//! UI behaves identically in both. Events: `transcript-segment`, `note-token`,
//! `audio-level`, `download-progress`.

use std::path::Path;
use std::sync::Arc;

use os_app::{
    App, AppError, Approver, Events, Patient, PdfRequest, ProfileSummary, RecordingConsent, RecordingSource,
    RefineRequest, ResetView, SaveTemplateRequest, ShareMode, TeamSettingsView, TemplateMode,
    TemplateSummary, TemplateView,
};
use os_engine::Segment;
use os_engine::template::{Scope, Standard, Template};
use os_engine::template_diff::TemplateDiff;
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
    fn progress(&self, p: &os_app::Progress) {
        let _ = self.0.emit("processing-progress", p);
    }
    fn moving(&self, done: u64, total: u64) {
        let _ = self.0.emit("move-progress", Moving { done, total });
    }
}

#[derive(Clone, Serialize)]
struct Moving {
    done: u64,
    total: u64,
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
async fn list_patients(app: State<'_, AppState>) -> Result<Vec<os_app::PatientSummary>, String> {
    let app = app.inner().clone();
    work(move || app.list_patients()).await
}

#[tauri::command]
async fn patient(app: State<'_, AppState>, id: String) -> Result<os_app::PatientView, String> {
    let app = app.inner().clone();
    work(move || app.patient(&id)).await
}

#[tauri::command]
fn create_patient(
    app: State<'_, AppState>,
    name: String,
    last_name: String,
    species: String,
) -> Result<os_engine::patients::PatientRecord, String> {
    app.create_patient(&name, &last_name, &species).map_err(|e| e.0)
}

#[tauri::command]
fn update_patient_record(
    app: State<'_, AppState>,
    id: String,
    name: String,
    last_name: String,
    species: String,
) -> Result<os_engine::patients::PatientRecord, String> {
    app.update_patient_record(&id, &name, &last_name, &species).map_err(|e| e.0)
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

// ---- storage (ADR-0013) ------------------------------------------------------

#[tauri::command]
async fn storage(app: State<'_, AppState>) -> Result<os_app::StorageView, String> {
    let app = app.inner().clone();
    work(move || Ok(app.storage())).await
}

#[tauri::command]
async fn check_folder(
    app: State<'_, AppState>,
    kind: os_app::FolderKind,
    path: String,
) -> Result<os_app::FolderCheck, String> {
    let app = app.inner().clone();
    work(move || Ok(app.check_folder(kind, &path, false))).await
}

/// Change a folder; the window then calls `restart_app` so everything opens
/// from the new place.
#[tauri::command]
async fn change_folder(
    handle: AppHandle,
    app: State<'_, AppState>,
    kind: os_app::FolderKind,
    path: String,
    mode: os_app::ChangeMode,
) -> Result<os_app::FolderChanged, String> {
    let app = app.inner().clone();
    work(move || app.change_folder(kind, &path, mode, false, &TauriEvents(handle))).await
}

#[tauri::command]
async fn gpu_status(app: State<'_, AppState>) -> Result<os_app::GpuStatus, String> {
    let app = app.inner().clone();
    work(move || Ok(app.gpu_status())).await
}

#[tauri::command]
async fn install_gpu(
    handle: AppHandle,
    app: State<'_, AppState>,
    accept_license: bool,
) -> Result<os_app::GpuStatus, String> {
    let app = app.inner().clone();
    work(move || app.install_gpu(accept_license, &TauriEvents(handle))).await
}

#[tauri::command]
async fn remove_gpu(app: State<'_, AppState>) -> Result<os_app::GpuStatus, String> {
    let app = app.inner().clone();
    work(move || app.remove_gpu()).await
}

/// Restart, unless a recording or other work is running (it would be lost).
#[tauri::command]
fn restart_app(handle: AppHandle, app: State<'_, AppState>) -> Result<(), String> {
    if app.is_recording() {
        return Err("Stop the recording first, then restart.".into());
    }
    if app.is_busy() {
        return Err(
            "OpenBark is importing, drafting, saving or downloading. Restart when that's finished.".into()
        );
    }
    handle.restart()
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
    dentition: Option<String>,
) -> Result<os_app::NoteResponse, String> {
    let app = app.inner().clone();
    work(move || app.edit_note(&id, &heading, value, &by, pet.as_deref(), dentition.as_deref())).await
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

// ---- deleting (ADR-0022) ------------------------------------------------------

#[tauri::command]
async fn delete_visit(
    app: State<'_, AppState>,
    id: String,
    request: os_app::DeleteRequest,
) -> Result<os_app::DeletedVisit, String> {
    let app = app.inner().clone();
    work(move || app.delete_visit(&id, &request)).await
}

#[tauri::command]
async fn delete_recording(
    app: State<'_, AppState>,
    id: String,
    by: Option<String>,
) -> Result<os_app::SessionView, String> {
    let app = app.inner().clone();
    work(move || app.delete_recording(&id, by.as_deref())).await
}

#[tauri::command]
async fn list_deleted(
    app: State<'_, AppState>,
    by: Option<String>,
) -> Result<Vec<os_app::DeletedSummary>, String> {
    let app = app.inner().clone();
    work(move || app.list_deleted(by.as_deref())).await
}

#[tauri::command]
async fn restore_visit(
    app: State<'_, AppState>,
    tid: String,
    by: Option<String>,
) -> Result<os_app::RestoredVisit, String> {
    let app = app.inner().clone();
    work(move || app.restore_visit(&tid, by.as_deref())).await
}

/// "Delete now". A PIN (signed visits) is only ever an argument, never logged.
#[tauri::command]
async fn purge_deleted(
    app: State<'_, AppState>,
    tid: String,
    by: Option<String>,
    pin: Option<String>,
) -> Result<(), String> {
    let app = app.inner().clone();
    work(move || app.purge_deleted(&tid, by.as_deref(), pin.as_deref())).await
}

// ---- templates (ADR-0024): an admin's PIN is only ever an argument, never logged ----

#[tauri::command]
fn list_templates(app: State<'_, AppState>, by: Option<String>) -> Result<Vec<TemplateSummary>, String> {
    app.list_templates(by.as_deref()).map_err(|e| e.0)
}

#[tauri::command]
fn template(app: State<'_, AppState>, id: String, by: Option<String>) -> Result<TemplateView, String> {
    app.template(&id, by.as_deref()).map_err(|e| e.0)
}

#[tauri::command]
fn preview_template(
    app: State<'_, AppState>,
    template: Template,
    by: Option<String>,
) -> Result<TemplateView, String> {
    app.preview_template(template, by.as_deref()).map_err(|e| e.0)
}

/// A template proposed from an example note; not saved (M2 item 5).
#[tauri::command]
async fn template_from_example(
    app: State<'_, AppState>,
    llm: String,
    text: String,
) -> Result<TemplateView, String> {
    let app = app.inner().clone();
    work(move || app.template_from_example(&llm, &text)).await
}

#[tauri::command]
async fn save_template(
    app: State<'_, AppState>,
    request: SaveTemplateRequest,
) -> Result<TemplateView, String> {
    let app = app.inner().clone();
    work(move || app.save_template(request)).await
}

#[tauri::command]
fn compare_template(
    app: State<'_, AppState>,
    id: String,
    by: Option<String>,
    with: String,
) -> Result<TemplateDiff, String> {
    app.compare_template(&id, by.as_deref(), &with).map_err(|e| e.0)
}

#[tauri::command]
fn reset_template(app: State<'_, AppState>, id: String, by: Option<String>) -> Result<ResetView, String> {
    app.reset_template(&id, by.as_deref()).map_err(|e| e.0)
}

#[tauri::command]
fn undo_reset_template(
    app: State<'_, AppState>,
    id: String,
    by: Option<String>,
    undo: String,
) -> Result<TemplateView, String> {
    app.undo_reset_template(&id, by.as_deref(), &undo).map_err(|e| e.0)
}

#[tauri::command]
async fn keep_template(
    app: State<'_, AppState>,
    id: String,
    by: Option<String>,
    fingerprint: String,
    approver: Option<Approver>,
) -> Result<TemplateView, String> {
    let app = app.inner().clone();
    work(move || app.keep_template(&id, by.as_deref(), &fingerprint, approver.as_ref())).await
}

#[tauri::command]
async fn share_template(
    app: State<'_, AppState>,
    id: String,
    by: Option<String>,
    mode: ShareMode,
    name: Option<String>,
    approver: Option<Approver>,
) -> Result<TemplateView, String> {
    let app = app.inner().clone();
    work(move || app.share_template(&id, by.as_deref(), mode, name.as_deref(), approver.as_ref())).await
}

#[tauri::command]
async fn reset_team_template(
    app: State<'_, AppState>,
    id: String,
    by: Option<String>,
    approver: Option<Approver>,
) -> Result<ResetView, String> {
    let app = app.inner().clone();
    work(move || app.reset_team_template(&id, by.as_deref(), approver.as_ref())).await
}

#[tauri::command]
async fn delete_template(
    app: State<'_, AppState>,
    id: String,
    by: Option<String>,
    approver: Option<Approver>,
) -> Result<(), String> {
    let app = app.inner().clone();
    work(move || app.delete_template(&id, by.as_deref(), approver.as_ref())).await
}

#[tauri::command]
async fn add_standard(
    app: State<'_, AppState>,
    id: String,
    heading: String,
    text: String,
    by: Option<String>,
    scope: Option<Scope>,
    approver: Option<Approver>,
) -> Result<Standard, String> {
    let app = app.inner().clone();
    work(move || app.add_standard(&id, &heading, &text, by.as_deref(), scope, approver.as_ref())).await
}

#[tauri::command]
async fn remove_standard(
    app: State<'_, AppState>,
    id: String,
    sid: String,
    by: Option<String>,
    approver: Option<Approver>,
) -> Result<TemplateView, String> {
    let app = app.inner().clone();
    work(move || app.remove_standard(&id, &sid, by.as_deref(), approver.as_ref())).await
}

#[tauri::command]
fn mute_standard(
    app: State<'_, AppState>,
    id: String,
    sid: String,
    by: Option<String>,
    muted: bool,
) -> Result<TemplateView, String> {
    app.mute_standard(&id, &sid, by.as_deref(), muted).map_err(|e| e.0)
}

#[tauri::command]
fn run_template_view(
    app: State<'_, AppState>,
    id: String,
    pet: Option<String>,
) -> Result<TemplateView, String> {
    app.run_template_view(&id, pet.as_deref()).map_err(|e| e.0)
}

#[tauri::command]
fn team_settings(app: State<'_, AppState>) -> TeamSettingsView {
    app.team_settings()
}

#[tauri::command]
async fn set_team_settings(
    app: State<'_, AppState>,
    templates: TemplateMode,
    by: Option<String>,
    approver: Option<Approver>,
) -> Result<TeamSettingsView, String> {
    let app = app.inner().clone();
    work(move || app.set_template_mode(templates, by.as_deref(), approver.as_ref())).await
}

#[tauri::command]
fn dismiss_setup_notice(app: State<'_, AppState>, by: Option<String>) -> Result<TeamSettingsView, String> {
    app.dismiss_setup_notice(by.as_deref()).map_err(|e| e.0)
}

#[tauri::command]
async fn set_user_admin(
    app: State<'_, AppState>,
    uid: String,
    admin: bool,
    by: Option<String>,
    approver: Option<Approver>,
) -> Result<User, String> {
    let app = app.inner().clone();
    work(move || app.set_user_admin(&uid, admin, by.as_deref(), approver.as_ref())).await
}

#[tauri::command]
fn profile_summary(
    app: State<'_, AppState>,
    uid: String,
    by: Option<String>,
) -> Result<ProfileSummary, String> {
    app.profile_summary(&uid, by.as_deref()).map_err(|e| e.0)
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
    // Deleted visits (ADR-0022): recover what a crash left, remove what's due, then hourly.
    app.start_housekeeping();
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(app)
        .invoke_handler(tauri::generate_handler![
            status,
            input_devices,
            storage,
            check_folder,
            change_folder,
            restart_app,
            gpu_status,
            install_gpu,
            remove_gpu,
            list_patients,
            patient,
            create_patient,
            update_patient_record,
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
            delete_visit,
            delete_recording,
            list_deleted,
            restore_visit,
            purge_deleted,
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
            template_from_example,
            save_template,
            compare_template,
            reset_template,
            undo_reset_template,
            keep_template,
            share_template,
            reset_team_template,
            delete_template,
            add_standard,
            remove_standard,
            mute_standard,
            run_template_view,
            team_settings,
            set_team_settings,
            dismiss_setup_notice,
            set_user_admin,
            profile_summary,
            models,
            download_model,
            hub_search,
            hub_repo,
            hub_add
        ])
        .run(tauri::generate_context!())
        .expect("error while running OpenBark");
}
