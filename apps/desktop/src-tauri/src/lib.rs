//! OpenBark desktop: Tauri commands over the shared app layer ([`os_app::App`]).
//!
//! Web mode (`openbark serve`) exposes the same operations over HTTP, so the
//! UI behaves identically in both. Events: `transcript-segment`, `note-token`,
//! `draft-progress` (prompt reading, then a writing heartbeat), `audio-level`,
//! `download-progress`, `phone-call-saved`, `mic-status` (the recording's
//! microphone health) and `mic-test` (the microphone test's level).
//!
//! Phone calls (ADR-0023 section 4) run in a second window, `phone`, the only
//! window allowed to reach Twilio and the only one granted the microphone. It
//! may call only the phone commands ([`PHONE_COMMANDS`]): no visits, patients
//! or settings. The main window can't call those.

use std::path::Path;
use std::sync::Arc;

use os_app::{
    App, AppError, Approver, CallBlock, CallEnd, CallRequest, CallStart, CallTarget, ClinicTerm,
    ConnectRequest, Connected, ConsentOutcome, Disconnected, Events, Patient, PdfRequest, PhonePrefs,
    PhoneToken, PhoneView, ProfileSummary, RecordingConsent, RecordingSource, RefineRequest, ResetView,
    SaveTemplateRequest, SetupCheck, ShareMode, TeamSettingsView, TemplateMode, TemplateSummary,
    TemplateView, VocabularyView,
};
use os_engine::Segment;
use os_engine::template::{Scope, Standard, Template};
use os_engine::template_diff::TemplateDiff;
use os_engine::users::{Role, User};
use os_models::ModelStore;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

#[cfg(feature = "selftest")]
mod selftest;

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
    fn mic(&self, health: &os_app::MicHealth) {
        let _ = self.0.emit("mic-status", health);
    }
    fn mic_test(&self, level: &os_app::MicLevel) {
        let _ = self.0.emit("mic-test", level);
    }
    fn draft(&self, phase: &os_app::DraftPhase) {
        let _ = self.0.emit("draft-progress", phase);
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
    let status = app.status();
    // The iOS check job's smoke test (ADR-0018 section 8): the interface asks
    // for the status as soon as the WebView has loaded it, so a marker in the
    // data folder proves the WebView ran the app. Next to it, the models the
    // engine found (the speech models that come with the iPhone app, ADR-0018
    // section 10), written first so the marker means both are there. Only
    // with OPENBARK_SMOKE=1.
    if std::env::var("OPENBARK_SMOKE").as_deref() == Ok("1") {
        let dir = os_engine::session::default_root();
        let base = dir.parent().unwrap_or(&dir).to_path_buf();
        let models = serde_json::to_string_pretty(&status.models).unwrap_or_default();
        let sync = serde_json::to_string_pretty(&app.sync_status()).unwrap_or_default();
        if let Err(e) = std::fs::create_dir_all(&base)
            .and_then(|_| std::fs::write(base.join("smoke-models.json"), models))
            .and_then(|_| std::fs::write(base.join("smoke-sync.json"), sync))
            .and_then(|_| std::fs::write(base.join("smoke-webview.txt"), env!("CARGO_PKG_VERSION")))
        {
            tracing::warn!(error = %e, "couldn't write the smoke-test marker");
        }
        // The transcription check: the smoke test puts a short synthetic clip
        // in the data folder; transcribe it once with the built-in models and
        // write "ok: <first words>" or "error: <why>" for the script. When it
        // also put a tiny test GGUF there, draft a SOAP note from that
        // transcript and write "ok: <n> sections" or "error: <why>".
        static TRANSCRIBING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        let clip = base.join("smoke-input.wav");
        if clip.is_file() && !TRANSCRIBING.swap(true, std::sync::atomic::Ordering::SeqCst) {
            let app = app.inner().clone();
            let out = base.join("smoke-transcript.txt");
            let gguf = base.join("smoke-llm.gguf");
            let note_out = base.join("smoke-note.txt");
            std::thread::spawn(move || {
                let transcript = app.smoke_transcript(&clip);
                let words: Vec<String> = transcript
                    .as_ref()
                    .map(|t| {
                        t.segments.iter().flat_map(|s| s.text.split_whitespace()).map(String::from).collect()
                    })
                    .unwrap_or_default();
                let line = match &transcript {
                    Ok(_) if !words.is_empty() => {
                        format!("ok: {}", words.iter().take(12).cloned().collect::<Vec<_>>().join(" "))
                    }
                    Ok(_) => "error: the transcript is empty".to_string(),
                    Err(e) => format!("error: {}", e.0),
                };
                // The words go to the file the script reads, never to the log.
                tracing::info!(words = words.len(), ok = transcript.is_ok(), "smoke-test transcription");
                if let Err(e) = std::fs::write(&out, line) {
                    tracing::warn!(error = %e, "couldn't write the smoke-test transcript");
                }
                if let (Ok(t), true) = (&transcript, gguf.is_file()) {
                    // Where the draft is, for the script when it runs long.
                    let progress_file = note_out.with_extension("progress");
                    let mut lines = String::new();
                    let mut progress = |stage: &str| {
                        lines.push_str(stage);
                        lines.push('\n');
                        let _ = std::fs::write(&progress_file, &lines);
                    };
                    progress("loading");
                    let note = match app.smoke_draft(&gguf, t, &mut progress) {
                        Ok(n) => format!("ok: {n} sections"),
                        Err(e) => format!("error: {}", e.0),
                    };
                    tracing::info!(result = %note, "smoke-test draft");
                    if let Err(e) = std::fs::write(&note_out, note) {
                        tracing::warn!(error = %e, "couldn't write the smoke-test note");
                    }
                }
            });
        }
    }
    status
}

#[tauri::command]
fn input_devices(app: State<'_, AppState>) -> Vec<String> {
    app.input_devices()
}

/// The Record screen opened: load the speech models now, if memory allows.
/// Off the main thread: it takes locks and may free models.
#[tauri::command]
async fn prewarm_speech(
    app: State<'_, AppState>,
    speakers: bool,
    speakerphone: Option<bool>,
) -> Result<bool, String> {
    let app = app.inner().clone();
    work(move || Ok(app.prewarm_speech(speakers, speakerphone.unwrap_or(false)))).await
}

/// The Record screen closed: give the memory back (freeing models takes a moment).
#[tauri::command]
async fn release_speech(app: State<'_, AppState>) -> Result<(), String> {
    let app = app.inner().clone();
    work(move || {
        app.release_speech();
        Ok(())
    })
    .await
}

#[tauri::command]
async fn mic_test_start(
    handle: AppHandle,
    app: State<'_, AppState>,
    device: Option<String>,
) -> Result<os_app::MicTestStarted, String> {
    let app = app.inner().clone();
    work(move || app.start_mic_test(device, Arc::new(TauriEvents(handle)))).await
}

/// Stop the test with this number, or whichever is running.
#[tauri::command]
async fn mic_test_stop(app: State<'_, AppState>, test: Option<u64>) -> Result<(), String> {
    let app = app.inner().clone();
    work(move || {
        match test {
            Some(id) => app.stop_mic_test_number(id),
            None => app.stop_mic_test(),
        }
        Ok(())
    })
    .await
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
    owner_phone: Option<String>,
) -> Result<os_engine::patients::PatientRecord, String> {
    let record = app.update_patient_record(&id, &name, &last_name, &species).map_err(|e| e.0)?;
    match owner_phone {
        Some(phone) => app.set_owner_phone(&id, &phone).map_err(|e| e.0),
        None => Ok(record),
    }
}

// ---- phone calls (ADR-0023) ---------------------------------------------------

/// The only commands the phone window may call. Everything else is refused
/// there, and these are refused in every other window (see `run`).
const PHONE_COMMANDS: &[&str] = &[
    "phone_pending",
    "phone_token",
    "phone_call_start",
    "phone_audio",
    "phone_call_announced",
    "phone_call_consent",
    "phone_call_end",
];

/// Whether a window may call a command: the phone window only the phone
/// commands, every other window everything else.
fn command_allowed(window_label: &str, command: &str) -> bool {
    (window_label == PHONE_WINDOW) == PHONE_COMMANDS.contains(&command)
}

/// The phone window's label.
const PHONE_WINDOW: &str = "phone";

/// The call being recorded: where the phone window's audio goes.
#[derive(Default)]
struct CallAudio {
    tx: std::sync::Mutex<Option<(std::sync::mpsc::Sender<CallBlock>, u32)>>,
}

#[tauri::command]
async fn phone(app: State<'_, AppState>) -> Result<PhoneView, String> {
    let app = app.inner().clone();
    work(move || app.phone()).await
}

#[tauri::command]
fn phone_bin(app: State<'_, AppState>, caller_id: String) -> Result<String, String> {
    app.phone_bin_text(&caller_id).map_err(|e| e.0)
}

/// No `Debug` on the request: it carries the API key secret, which goes to
/// the engine and on to `api.twilio.com` only.
#[tauri::command]
async fn phone_connect(
    app: State<'_, AppState>,
    request: ConnectRequest,
    by: Option<String>,
) -> Result<Connected, String> {
    let app = app.inner().clone();
    work(move || app.phone_connect(request, by.as_deref())).await
}

#[tauri::command]
async fn phone_disconnect(app: State<'_, AppState>, by: Option<String>) -> Result<Disconnected, String> {
    let app = app.inner().clone();
    work(move || app.phone_disconnect(by.as_deref())).await
}

#[tauri::command]
async fn phone_set_prefs(
    app: State<'_, AppState>,
    prefs: PhonePrefs,
    by: Option<String>,
) -> Result<PhoneView, String> {
    let app = app.inner().clone();
    work(move || app.phone_set_prefs(prefs, by.as_deref())).await
}

#[tauri::command]
async fn phone_check(app: State<'_, AppState>, by: Option<String>) -> Result<Vec<SetupCheck>, String> {
    let app = app.inner().clone();
    work(move || app.phone_check(by.as_deref())).await
}

/// **Call** in the main window: remembers who to call, then opens (or
/// brings forward) the phone window. Async: creating a window from a
/// synchronous command deadlocks on Windows.
#[tauri::command]
async fn phone_open(
    handle: AppHandle,
    app: State<'_, AppState>,
    call: CallRequest,
    by: Option<String>,
) -> Result<CallTarget, String> {
    if cfg!(target_os = "linux") {
        return Err(
            "Calls work in web mode on Linux. Run `openbark serve`, then open OpenBark in Chrome or Firefox."
                .into(),
        );
    }
    let app = app.inner().clone();
    let target = work(move || app.phone_prepare(call, by.as_deref())).await?;
    if let Some(window) = handle.get_webview_window(PHONE_WINDOW) {
        // A call is already on screen: show it, don't replace it.
        let _ = window.set_focus();
        let _ = window.emit("phone-target", &target);
        return Ok(target);
    }
    tauri::WebviewWindowBuilder::new(&handle, PHONE_WINDOW, tauri::WebviewUrl::App("phone.html".into()))
        .title("OpenBark phone")
        .inner_size(420.0, 720.0)
        .min_inner_size(360.0, 560.0)
        .build()
        .map_err(|e| e.to_string())?;
    Ok(target)
}

#[tauri::command]
fn phone_pending(app: State<'_, AppState>) -> Result<Option<CallTarget>, String> {
    app.phone_pending().map_err(|e| e.0)
}

/// A one-hour access token for the phone window's `Device`. The API key
/// secret never leaves the engine.
#[tauri::command]
fn phone_token(app: State<'_, AppState>) -> Result<PhoneToken, String> {
    app.phone_token().map_err(|e| e.0)
}

/// The client answered: start recording both sides.
#[tauri::command]
async fn phone_call_start(
    handle: AppHandle,
    app: State<'_, AppState>,
    audio: State<'_, CallAudio>,
    mut start: CallStart,
    sample_rate: u32,
) -> Result<String, String> {
    // A rate the recorder can't use is refused before anything starts.
    os_app::check_call_sample_rate(sample_rate).map_err(|e| e.0)?;
    start.sample_rate = Some(sample_rate);
    let (tx, rx) = std::sync::mpsc::channel();
    let app = app.inner().clone();
    let events: Arc<dyn Events> = Arc::new(TauriEvents(handle));
    let id = work(move || app.start_call(start, rx, events)).await?;
    *audio.tx.lock().map_err(|_| "call lock poisoned".to_string())? = Some((tx, sample_rate));
    Ok(id)
}

/// Interleaved two-channel f32 PCM (clinic, client), little-endian, as the
/// raw body of the call.
#[tauri::command]
fn phone_audio(request: tauri::ipc::Request<'_>, audio: State<'_, CallAudio>) -> Result<(), String> {
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else {
        return Err("expected raw audio".into());
    };
    let mut slot = audio.tx.lock().map_err(|_| "call lock poisoned".to_string())?;
    if let Some((tx, rate)) = slot.as_ref() {
        let samples: Vec<f32> =
            bytes.as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect();
        if tx.send(CallBlock::from_interleaved(&samples, *rate)).is_err() {
            *slot = None; // the recording stopped (declined): the call goes on unrecorded
        }
    }
    Ok(())
}

/// "I've read it": the announcement was read; until then nothing is kept.
#[tauri::command]
async fn phone_call_announced(app: State<'_, AppState>, at: Option<f32>) -> Result<(), String> {
    let app = app.inner().clone();
    work(move || app.call_announced(at)).await
}

/// The phone window closed or crashed: stop feeding the recorder and end the
/// call in the engine, so the next call isn't refused. A recording that was
/// never announced is deleted there.
fn phone_window_gone(app: &App, audio: &CallAudio) -> Option<os_app::SessionView> {
    if let Ok(mut tx) = audio.tx.lock() {
        *tx = None;
    }
    match app.phone_window_closed() {
        Ok(view) => view,
        Err(e) => {
            tracing::warn!(error = %e.0, "couldn't end the call after the phone window closed");
            None
        }
    }
}

#[tauri::command]
async fn phone_call_consent(
    app: State<'_, AppState>,
    audio: State<'_, CallAudio>,
    outcome: ConsentOutcome,
    announced_at: Option<f32>,
) -> Result<Option<os_app::SessionView>, String> {
    let app = app.inner().clone();
    let declined = work(move || app.call_consent(outcome, announced_at)).await?;
    if declined.is_some() {
        *audio.tx.lock().map_err(|_| "call lock poisoned".to_string())? = None;
    }
    Ok(declined)
}

/// Hang up: saves the visit and tells the main window, which opens it.
#[tauri::command]
async fn phone_call_end(
    handle: AppHandle,
    app: State<'_, AppState>,
    audio: State<'_, CallAudio>,
    end: CallEnd,
) -> Result<Option<os_app::SessionView>, String> {
    // Ends the recording's source before stopping it.
    *audio.tx.lock().map_err(|_| "call lock poisoned".to_string())? = None;
    let app = app.inner().clone();
    let view = work(move || app.end_call(end)).await?;
    if let Some(v) = &view {
        let _ = handle.emit_to("main", "phone-call-saved", serde_json::json!({ "id": v.id }));
    }
    Ok(view)
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
async fn remove_gpu(app: State<'_, AppState>, by: Option<String>) -> Result<os_app::GpuStatus, String> {
    let app = app.inner().clone();
    work(move || app.remove_gpu(by.as_deref())).await
}

// ---- models of your choice and the MLX engine (ADR-0025) -------------------

#[tauri::command]
async fn remove_model(
    app: State<'_, AppState>,
    id: String,
    note_model: Option<String>,
    forget: bool,
    by: Option<String>,
) -> Result<os_app::Removal, String> {
    let app = app.inner().clone();
    work(move || app.remove_model(&id, note_model.as_deref(), forget, by.as_deref())).await
}

#[tauri::command]
async fn inspect_import(
    app: State<'_, AppState>,
    path: String,
    by: Option<String>,
) -> Result<os_app::ImportPreview, String> {
    let app = app.inner().clone();
    work(move || app.inspect_import(&path, by.as_deref())).await
}

#[tauri::command]
async fn import_model(
    handle: AppHandle,
    app: State<'_, AppState>,
    path: String,
    mode: os_app::ImportMode,
    by: Option<String>,
) -> Result<os_app::ModelRow, String> {
    let app = app.inner().clone();
    work(move || app.import_model(&path, mode, by.as_deref(), &TauriEvents(handle))).await
}

#[tauri::command]
async fn mlx_status(app: State<'_, AppState>) -> Result<os_app::MlxStatus, String> {
    let app = app.inner().clone();
    work(move || Ok(app.mlx_status())).await
}

#[tauri::command]
async fn install_mlx(handle: AppHandle, app: State<'_, AppState>) -> Result<os_app::MlxStatus, String> {
    let app = app.inner().clone();
    work(move || app.install_mlx(&TauriEvents(handle))).await
}

#[tauri::command]
async fn check_mlx(app: State<'_, AppState>) -> Result<os_app::MlxStatus, String> {
    let app = app.inner().clone();
    work(move || app.check_mlx()).await
}

#[tauri::command]
async fn remove_mlx(
    app: State<'_, AppState>,
    note_model: Option<String>,
    by: Option<String>,
) -> Result<os_app::Removal, String> {
    let app = app.inner().clone();
    work(move || app.remove_mlx(note_model.as_deref(), by.as_deref())).await
}

#[tauri::command]
async fn hf_token_status(app: State<'_, AppState>) -> Result<os_app::HfTokenStatus, String> {
    let app = app.inner().clone();
    work(move || Ok(app.hf_token_status())).await
}

/// The token is checked and saved; it is never returned or logged.
#[tauri::command]
async fn set_hf_token(
    app: State<'_, AppState>,
    token: String,
    by: Option<String>,
) -> Result<os_app::HfTokenStatus, String> {
    let app = app.inner().clone();
    work(move || app.set_hf_token(&token, by.as_deref())).await
}

#[tauri::command]
async fn clear_hf_token(
    app: State<'_, AppState>,
    by: Option<String>,
) -> Result<os_app::HfTokenStatus, String> {
    let app = app.inner().clone();
    work(move || app.clear_hf_token(by.as_deref())).await
}

#[tauri::command]
async fn hub_alternatives(
    app: State<'_, AppState>,
    base: String,
) -> Result<Vec<os_models::hub::HubModel>, String> {
    let app = app.inner().clone();
    work(move || app.hub_alternatives(&base)).await
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
#[allow(clippy::too_many_arguments)]
async fn import_file(
    handle: AppHandle,
    app: State<'_, AppState>,
    path: String,
    speakers: bool,
    speakerphone: Option<bool>,
    room_speakers: Option<u32>,
    patient: Option<Patient>,
    consent: Option<RecordingConsent>,
    by: Option<String>,
) -> Result<os_app::SessionView, String> {
    let app = app.inner().clone();
    work(move || {
        app.import_file(
            Path::new(&path),
            speakers,
            speakerphone.unwrap_or(false),
            room_speakers,
            patient,
            consent,
            by.as_deref(),
            &TauriEvents(handle),
        )
    })
    .await
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn start_recording(
    handle: AppHandle,
    app: State<'_, AppState>,
    device: Option<String>,
    speakers: bool,
    speakerphone: Option<bool>,
    room_speakers: Option<u32>,
    patient: Option<Patient>,
    consent: Option<RecordingConsent>,
    by: Option<String>,
) -> Result<String, String> {
    let app = app.inner().clone();
    work(move || {
        app.start_recording(
            RecordingSource::Microphone(device),
            speakers,
            speakerphone.unwrap_or(false),
            room_speakers,
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

/// The drafting board's Stop button: the running draft ends at its next
/// prompt batch or token, and nothing is saved.
#[tauri::command]
fn cancel_draft(app: State<'_, AppState>) -> bool {
    app.cancel_draft()
}

/// The speed check (setup, ADR-0034): time the note model on invented text.
/// Stop is `cancel_draft`.
#[tauri::command]
async fn speed_check(
    handle: AppHandle,
    app: State<'_, AppState>,
    llm: String,
) -> Result<os_app::SpeedCheck, String> {
    let app = app.inner().clone();
    work(move || app.speed_check(&llm, &TauriEvents(handle))).await
}

/// Copy diagnostics (Settings, About): the computer, the note model, the
/// last draft's timings and the end of the log. Nothing from any visit.
#[tauri::command]
async fn diagnostics(
    app: State<'_, AppState>,
    note_model: Option<String>,
) -> Result<os_app::Diagnostics, String> {
    let app = app.inner().clone();
    work(move || Ok(app.diagnostics(note_model.as_deref()))).await
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

/// A veterinarian confirms (or takes back) the drafted tooth chart entries
/// on these teeth (ADR-0021).
#[tauri::command]
async fn confirm_chart(
    app: State<'_, AppState>,
    id: String,
    teeth: Vec<String>,
    confirmed: bool,
    by: String,
    pet: Option<String>,
) -> Result<os_app::SessionView, String> {
    let app = app.inner().clone();
    work(move || app.confirm_chart(&id, &teeth, confirmed, &by, pet.as_deref())).await
}

/// Confirm or remove normal findings from the template waiting in the
/// latest draft (ADR-0032).
#[tauri::command]
async fn settle_defaults(
    app: State<'_, AppState>,
    id: String,
    request: os_engine::defaults::SettleRequest,
    by: String,
    pet: Option<String>,
) -> Result<os_app::NoteResponse, String> {
    let app = app.inner().clone();
    work(move || app.settle_defaults(&id, &request, &by, pet.as_deref())).await
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

/// The scheme the player loads recordings from: `obaudio://localhost/<visit
/// id>`, or `http://obaudio.localhost/<visit id>` on Windows (engine.ts
/// builds it with `convertFileSrc`). It answers byte ranges, decrypting only
/// what is asked. WebKit's player (macOS, Linux) needs ranges over a custom
/// scheme, and a long recording never has to sit whole in memory.
const AUDIO_SCHEME: &str = "obaudio";

fn audio_protocol(
    ctx: tauri::UriSchemeContext<'_, tauri::Wry>,
    request: tauri::http::Request<Vec<u8>>,
    responder: tauri::UriSchemeResponder,
) {
    if !audio_allowed(ctx.webview_label()) {
        let refused = tauri::http::Response::builder()
            .status(403)
            .header("Cache-Control", "no-store")
            .body(Vec::new())
            .unwrap_or_else(|_| tauri::http::Response::new(Vec::new()));
        responder.respond(refused);
        return;
    }
    let app = ctx.app_handle().state::<AppState>().inner().clone();
    tauri::async_runtime::spawn_blocking(move || responder.respond(audio_response(&app, &request)));
}

/// Whether a window may load recordings through [`AUDIO_SCHEME`]: every window
/// but the phone window, which reaches only the phone commands (ADR-0023
/// section 4) and so never a visit's recording.
fn audio_allowed(window_label: &str) -> bool {
    window_label != PHONE_WINDOW
}

fn audio_response(app: &App, request: &tauri::http::Request<Vec<u8>>) -> tauri::http::Response<Vec<u8>> {
    // Visit ids are letters, digits, '-', '_' and '.', so nothing is encoded;
    // session_dir refuses anything else.
    let id = request.uri().path().trim_start_matches('/');
    let range = request.headers().get("range").and_then(|v| v.to_str().ok());
    let head = request.method() == tauri::http::Method::HEAD;
    let mut response = tauri::http::Response::builder();
    // The page's origin (tauri://localhost, http://tauri.localhost) differs
    // from the scheme's.
    if let Some(origin) = request.headers().get("origin") {
        response = response
            .header("Access-Control-Allow-Origin", origin.clone())
            .header("Access-Control-Expose-Headers", "Content-Range, Content-Length, Accept-Ranges");
    }
    let built = match app.session_audio_part(id, range, head) {
        Ok(part) => {
            response = response.status(part.status);
            for (name, value) in part.headers() {
                response = response.header(name, value);
            }
            response.body(part.body)
        }
        Err(e) => response
            .status(404)
            .header("Content-Type", "text/plain; charset=utf-8")
            .header("Cache-Control", "no-store")
            .body(e.0.into_bytes()),
    };
    built.unwrap_or_else(|_| tauri::http::Response::new(Vec::new()))
}

/// An encounter's recording, decrypted, for the player (ADR-0008: audio is
/// encrypted on disk, so the file can't be handed to the webview directly).
/// The player uses [`AUDIO_SCHEME`]; this whole-file copy is its fallback.
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

/// The clinic's words and drug names (ADR-0031).
#[tauri::command]
fn vocabulary(app: State<'_, AppState>) -> Result<VocabularyView, String> {
    app.vocabulary().map_err(|e| e.0)
}

#[tauri::command]
async fn save_vocabulary_term(
    app: State<'_, AppState>,
    term: ClinicTerm,
    by: Option<String>,
    approver: Option<Approver>,
) -> Result<VocabularyView, String> {
    let app = app.inner().clone();
    work(move || app.save_vocabulary_term(&term, by.as_deref(), approver.as_ref())).await
}

#[tauri::command]
async fn remove_vocabulary_term(
    app: State<'_, AppState>,
    term: String,
    by: Option<String>,
    approver: Option<Approver>,
) -> Result<VocabularyView, String> {
    let app = app.inner().clone();
    work(move || app.remove_vocabulary_term(&term, by.as_deref(), approver.as_ref())).await
}

/// The app's language, which the second recogniser is told to expect.
#[tauri::command]
fn set_speech_language(language: String) {
    os_engine::models::set_speech_language(Some(&language));
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

// ---- encrypted sync (ADR-0017): Rust makes every call; the WebView sees no key.

#[tauri::command]
fn sync_status(app: State<'_, AppState>) -> serde_json::Value {
    app.sync_status()
}

#[tauri::command]
async fn sync_set_server(
    app: State<'_, AppState>,
    url: String,
    key: String,
    by: Option<String>,
) -> Result<serde_json::Value, String> {
    let app = app.inner().clone();
    work(move || app.sync_set_server(&url, &key, by.as_deref())).await
}

#[tauri::command]
async fn sync_send_code(
    app: State<'_, AppState>,
    email: String,
    by: Option<String>,
) -> Result<serde_json::Value, String> {
    let app = app.inner().clone();
    work(move || app.sync_send_code(&email, by.as_deref())).await
}

#[tauri::command]
async fn sync_verify(
    app: State<'_, AppState>,
    code: String,
    device_name: String,
    backfill_days: Option<u32>,
    by: Option<String>,
) -> Result<serde_json::Value, String> {
    let app = app.inner().clone();
    work(move || app.sync_verify(&code, &device_name, backfill_days, by.as_deref())).await
}

#[tauri::command]
async fn sync_confirm_kit(
    app: State<'_, AppState>,
    answers: Vec<(usize, String)>,
    by: Option<String>,
) -> Result<serde_json::Value, String> {
    let app = app.inner().clone();
    work(move || app.sync_confirm_kit(answers, by.as_deref())).await
}

#[tauri::command]
async fn sync_pair(
    app: State<'_, AppState>,
    words: String,
    approve: bool,
    by: Option<String>,
) -> Result<serde_json::Value, String> {
    let app = app.inner().clone();
    work(move || app.sync_pair(&words, approve, by.as_deref())).await
}

#[tauri::command]
async fn sync_new_code(app: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let app = app.inner().clone();
    work(move || app.sync_new_code()).await
}

#[tauri::command]
async fn sync_recover(
    app: State<'_, AppState>,
    words: String,
    by: Option<String>,
) -> Result<serde_json::Value, String> {
    let app = app.inner().clone();
    work(move || app.sync_recover(&words, by.as_deref())).await
}

#[tauri::command]
async fn sync_now(app: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let app = app.inner().clone();
    work(move || app.sync_now()).await
}

#[tauri::command]
async fn sync_remove_device(
    app: State<'_, AppState>,
    device_id: String,
    by: Option<String>,
) -> Result<serde_json::Value, String> {
    let app = app.inner().clone();
    work(move || app.sync_remove_device(&device_id, by.as_deref())).await
}

#[tauri::command]
async fn sync_off(app: State<'_, AppState>, by: Option<String>) -> Result<serde_json::Value, String> {
    let app = app.inner().clone();
    work(move || app.sync_off(by.as_deref())).await
}

#[tauri::command]
async fn sync_delete_account(
    app: State<'_, AppState>,
    confirm: String,
    by: Option<String>,
) -> Result<serde_json::Value, String> {
    let app = app.inner().clone();
    work(move || app.sync_delete_account(&confirm, by.as_deref())).await
}

#[tauri::command]
async fn sync_stop_visit(
    app: State<'_, AppState>,
    id: String,
    by: Option<String>,
) -> Result<serde_json::Value, String> {
    let app = app.inner().clone();
    work(move || app.sync_stop_visit(&id, by.as_deref())).await
}

// ---- the clinic hub (ADR-0029) ----------------------------------------------

#[tauri::command]
async fn sync_confirm_pairing(
    app: State<'_, AppState>,
    code: String,
    by: Option<String>,
) -> Result<serde_json::Value, String> {
    let app = app.inner().clone();
    work(move || app.sync_confirm_pairing(&code, by.as_deref())).await
}

#[tauri::command]
async fn sync_start_over(app: State<'_, AppState>, by: Option<String>) -> Result<serde_json::Value, String> {
    let app = app.inner().clone();
    work(move || app.sync_start_over(by.as_deref())).await
}

#[tauri::command]
async fn sync_find_hubs(app: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let app = app.inner().clone();
    work(move || app.sync_find_hubs()).await
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn sync_join_hub(
    app: State<'_, AppState>,
    link: String,
    address: String,
    check: String,
    device_name: String,
    backfill_days: Option<u32>,
    existing: bool,
    by: Option<String>,
) -> Result<serde_json::Value, String> {
    let app = app.inner().clone();
    work(move || {
        app.sync_join_hub(&link, &address, &check, &device_name, backfill_days, existing, by.as_deref())
    })
    .await
}

#[tauri::command]
fn clinic_hub_status(app: State<'_, AppState>, by: Option<String>) -> serde_json::Value {
    app.clinic_hub_status(by.as_deref())
}

#[tauri::command]
async fn clinic_hub_settings(
    app: State<'_, AppState>,
    on: Option<bool>,
    port: Option<u16>,
    allowed: Option<Vec<String>>,
    quota_mb: Option<u64>,
    by: Option<String>,
) -> Result<serde_json::Value, String> {
    let app = app.inner().clone();
    work(move || app.clinic_hub_settings(on, port, allowed, quota_mb, by.as_deref())).await
}

#[tauri::command]
async fn clinic_hub_code(app: State<'_, AppState>, by: Option<String>) -> Result<serde_json::Value, String> {
    let app = app.inner().clone();
    work(move || app.clinic_hub_join_code(by.as_deref())).await
}

#[tauri::command]
async fn clinic_hub_join_self(
    app: State<'_, AppState>,
    device_name: String,
    existing: bool,
    by: Option<String>,
) -> Result<serde_json::Value, String> {
    let app = app.inner().clone();
    work(move || app.clinic_hub_join_self(&device_name, existing, by.as_deref())).await
}

#[tauri::command]
async fn clinic_hub_remove_device(
    app: State<'_, AppState>,
    device_id: String,
    by: Option<String>,
) -> Result<serde_json::Value, String> {
    let app = app.inner().clone();
    work(move || app.clinic_hub_remove_device(&device_id, by.as_deref())).await
}

/// Pins the command handler to the desktop runtime, so it can be wrapped.
fn for_wry<F: Fn(tauri::ipc::Invoke<tauri::Wry>) -> bool>(f: F) -> F {
    f
}

/// The app's entry point. Desktop builds call it from `main.rs`; on iOS and
/// Android Tauri calls it as the mobile entry point (ADR-0018 section 7).
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // The log file and panics in it (`<data>/OpenBark/logs`): release builds
    // have no console. Counts and timings only, never what was said.
    os_app::logs::init();
    let app: AppState =
        Arc::new(App::new(ModelStore::default_location(), os_engine::session::default_root()));
    // Deleted visits (ADR-0022): recover what a crash left, remove what's due, then hourly.
    app.start_housekeeping();
    // Encrypted sync polls every 30 s once turned on (ADR-0017).
    app.start_sync();
    let handler = for_wry(tauri::generate_handler![
        status,
        input_devices,
        prewarm_speech,
        release_speech,
        mic_test_start,
        mic_test_stop,
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
        cancel_draft,
        speed_check,
        diagnostics,
        sign_note,
        confirm_chart,
        settle_defaults,
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
        sync_status,
        sync_set_server,
        sync_send_code,
        sync_verify,
        sync_confirm_kit,
        sync_pair,
        sync_new_code,
        sync_recover,
        sync_now,
        sync_remove_device,
        sync_off,
        sync_delete_account,
        sync_stop_visit,
        sync_find_hubs,
        sync_join_hub,
        sync_confirm_pairing,
        sync_start_over,
        clinic_hub_status,
        clinic_hub_settings,
        clinic_hub_code,
        clinic_hub_join_self,
        clinic_hub_remove_device,
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
        vocabulary,
        save_vocabulary_term,
        remove_vocabulary_term,
        set_speech_language,
        set_user_admin,
        profile_summary,
        models,
        download_model,
        remove_model,
        inspect_import,
        import_model,
        mlx_status,
        install_mlx,
        check_mlx,
        remove_mlx,
        hf_token_status,
        set_hf_token,
        clear_hf_token,
        hub_search,
        hub_repo,
        hub_add,
        hub_alternatives,
        phone,
        phone_bin,
        phone_connect,
        phone_disconnect,
        phone_set_prefs,
        phone_check,
        phone_open,
        phone_pending,
        phone_token,
        phone_call_start,
        phone_audio,
        phone_call_announced,
        phone_call_consent,
        phone_call_end
    ]);
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(app)
        .manage(CallAudio::default())
        .register_asynchronous_uri_scheme_protocol(AUDIO_SCHEME, audio_protocol)
        // The microphone goes to the phone window only (ADR-0023 section 4);
        // the main window records through the engine, never the WebView.
        .on_permission_request(|webview, kind| {
            use tauri::webview::{PermissionKind, PermissionResponse};
            match kind {
                PermissionKind::Microphone if webview.label() == PHONE_WINDOW => PermissionResponse::Allow,
                PermissionKind::Microphone | PermissionKind::Camera => PermissionResponse::Deny,
                _ => PermissionResponse::Default,
            }
        })
        // The phone window may call the phone commands and nothing else; the
        // other windows can't call the commands meant for it.
        .invoke_handler(move |invoke| {
            if !command_allowed(invoke.message.webview().label(), invoke.message.command()) {
                invoke.resolver.reject("This window can't do that.");
                return true;
            }
            handler(invoke)
        })
        // The phone window closed mid-call (closed, or its WebView crashed):
        // end the call in the engine.
        .on_window_event(|window, event| {
            if window.label() == PHONE_WINDOW && matches!(event, tauri::WindowEvent::Destroyed) {
                let handle = window.app_handle().clone();
                std::thread::spawn(move || {
                    let app = handle.state::<AppState>().inner().clone();
                    let audio = handle.state::<CallAudio>();
                    if let Some(view) = phone_window_gone(&app, &audio) {
                        let _ =
                            handle.emit_to("main", "phone-call-saved", serde_json::json!({ "id": view.id }));
                    }
                });
            }
        });
    #[cfg(feature = "selftest")]
    let builder = builder.on_page_load(|webview, payload| {
        if payload.event() == tauri::webview::PageLoadEvent::Finished {
            selftest::page_loaded(webview);
        }
    });
    builder.build(tauri::generate_context!()).expect("error while running OpenBark").run(|handle, event| {
        if let tauri::RunEvent::Exit = event {
            quitting(handle);
        }
    });
}

/// The app is about to exit: Cmd-Q or the Quit menu item on macOS (AppKit
/// calls `exit()` right after this returns), the last window closing, or a
/// restart. Free the note model first: with a model still on the GPU when
/// the C++ static destructors run, ggml's Metal teardown aborts and macOS
/// reports a crash (os_llm::shutdown). If it can't be freed in time (a model
/// load is still running), end the process without that teardown.
fn quitting(handle: &AppHandle) {
    #[cfg(feature = "selftest")]
    if selftest::skip_shutdown() {
        eprintln!("selftest: quitting without freeing the note model");
        return;
    }
    let app = handle.state::<AppState>().inner().clone();
    let freed = app.shutdown(os_app::SHUTDOWN_WAIT);
    #[cfg(feature = "selftest")]
    eprintln!("selftest: note models freed before exit: {freed}");
    if !freed {
        os_app::exit_process(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ADR-0023 section 4: the phone window may call only the phone commands,
    /// and no other window may call them.
    #[test]
    fn the_phone_window_calls_only_the_phone_commands() {
        for c in PHONE_COMMANDS {
            assert!(command_allowed("phone", c), "{c}");
            assert!(!command_allowed("main", c), "{c} from the main window");
        }
        for c in ["open_session", "list_patients", "phone_connect", "phone_open", "export_pdf", "status"] {
            assert!(!command_allowed("phone", c), "{c} from the phone window");
            assert!(command_allowed("main", c), "{c}");
        }
    }

    /// The phone window can't load recordings through the audio scheme either.
    #[test]
    fn the_phone_window_cant_play_recordings() {
        assert!(audio_allowed("main"));
        assert!(!audio_allowed(PHONE_WINDOW));
    }

    /// Finding 4 of the 2026-10-19 review: the phone window must hear
    /// `phone-target` (a Call pressed on another patient), so its capability
    /// allows listening for events. It grants nothing that reaches visits,
    /// files or the shell, and the main window's capability doesn't name it.
    #[test]
    fn the_phone_windows_capability_allows_events_and_nothing_more() {
        let phone: serde_json::Value =
            serde_json::from_str(include_str!("../capabilities/phone.json")).unwrap();
        assert_eq!(phone["windows"], serde_json::json!(["phone"]));
        let perms: Vec<&str> =
            phone["permissions"].as_array().unwrap().iter().map(|p| p.as_str().unwrap()).collect();
        for needed in ["core:event:allow-listen", "core:event:allow-unlisten", "core:window:allow-close"] {
            assert!(perms.contains(&needed), "{needed} in {perms:?}");
        }
        assert!(
            perms.iter().all(|p| p.starts_with("core:event:") || p.starts_with("core:window:")),
            "{perms:?}"
        );
        let main: serde_json::Value =
            serde_json::from_str(include_str!("../capabilities/default.json")).unwrap();
        assert_eq!(main["windows"], serde_json::json!(["main"]));
    }

    /// Finding 5: a closed phone window stops the audio and ends the call;
    /// with no call it does nothing.
    #[test]
    fn a_closed_phone_window_stops_the_audio() {
        let tmp = std::env::temp_dir().join(format!("openbark-desktop-test-{}", std::process::id()));
        // SAFETY: this test alone sets these, before the App exists.
        unsafe {
            std::env::set_var("OPENBARK_DATA_KEY", "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=");
            std::env::set_var("OPENBARK_CONNECTIONS_DIR", tmp.join("connections"));
            std::env::set_var("OPENBARK_USERS_FILE", tmp.join("users.json"));
            std::env::set_var("OPENBARK_TEMPLATES_DIR", tmp.join("templates"));
            std::env::set_var("OPENBARK_LOCATIONS_FILE", tmp.join("locations.json"));
            std::env::set_var("OPENBARK_KEY_FILE", tmp.join("data.key"));
            std::env::set_var("OPENBARK_ENGINES_DIR", tmp.join("engines"));
        }
        let app = App::new(ModelStore::new(tmp.join("models")), tmp.join("sessions"));
        let audio = CallAudio::default();
        let (tx, _rx) = std::sync::mpsc::channel();
        *audio.tx.lock().unwrap() = Some((tx, 48_000));
        assert!(phone_window_gone(&app, &audio).is_none(), "no call: nothing saved");
        assert!(audio.tx.lock().unwrap().is_none(), "the audio channel is closed");
        assert!(!app.call_active());
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
