//! Web mode: the OpenBark app layer ([`os_app::App`]) behind a small HTTP API,
//! serving the same React UI as the desktop app to a browser on this machine.
//!
//! Binds to 127.0.0.1 only. Exposing it to the clinic network needs the
//! Clinic Hub's authentication and mutual TLS (M4); there is no auth yet, so
//! it must never listen on other interfaces.
//!
//! Other pages in the same browser are kept out ([`local_only`]): a request
//! must name this server as its Host (no DNS rebinding), any Origin must be
//! this server, and every request that changes something must carry the
//! `X-OpenBark` header, which a page on another site can't add without a
//! CORS preflight that this server never grants.
//!
//! API (JSON unless noted):
//! - `GET  /api/status`, `GET /api/devices`
//! - `GET  /api/patients`, `POST /api/patients` `{name, last_name, species}`,
//!   `GET /api/patients/{id}` (with visits), `POST /api/patients/{id}` (change details)
//! - `GET  /api/sessions`, `GET /api/sessions/{id}`, `GET /api/sessions/{id}/audio` (Range)
//! - `POST /api/sessions?diarize=&name=`    body: WAV bytes → new encounter, transcribed
//!   (patient: `patient_name`, `patient_id`, `others` (one per line), `last_name`, `reason`, `species`; consent: `consent_rule`,
//!   `consent_method`, `consent_by`, `consent_script`; the same on `/api/live`,
//!   where consent is required)
//! - `POST /api/sessions/{id}/notes`        `{llm, template, language?}`
//! - `POST /api/notes/cancel`               Stop the running draft (`true` when one was running);
//!   it answers its own request with "Drafting stopped. Nothing was saved."
//! - `POST /api/speed-check`                `{llm}`: time the note model on invented text and
//!   estimate a long visit's note here (ADR-0034); Stop is `/api/notes/cancel`
//! - `GET  /api/diagnostics?note_model=`    Copy diagnostics: the computer, the note model, the
//!   last draft's timings and the end of the log (nothing from any visit)
//! - `POST /api/sessions/{id}/refine`       `{llm, heading, request, save_as_standard, by}`:
//!   redraft one heading of the latest draft
//! - `POST /api/sessions/{id}/edit`         `{heading, value, by}`: the vet's own version of a heading
//! - `POST /api/sessions/{id}/addenda`      `{text, by}`: signed addition to a signed note
//! - `POST /api/sessions/{id}/chart/confirm` `{teeth, confirmed, by, pet?}`: a veterinarian
//!   confirms (or takes back) the drafted tooth chart entries on these teeth; signing
//!   waits for every drafted entry (ADR-0021)
//! - `POST /api/sessions/{id}/defaults` `{heading, labels, confirm, text?, by, pet?}`: confirm
//!   (a veterinarian) or remove normal findings from the template; signing waits for every
//!   one (ADR-0032)
//! - `POST /api/sessions/{id}/pdf`          `{clinic: {name, lines}, page, document?}` → application/pdf
//! - Templates (ADR-0024; reads take `by` in the query, writes in the body; an
//!   admin's PIN travels only in a body, as `approver: {id, pin}`):
//!   `GET /api/templates?by=`, `GET /api/templates/{id}?by=`,
//!   `POST /api/templates/preview` `{template, by}`,
//!   `POST /api/templates` `{template, by, base_version?, base_layer?, hospital?, approver?}` (save),
//!   `GET /api/templates/{id}/compare?by=&with=base|original`,
//!   `POST /api/templates/{id}/reset` `{by}` and `/reset/undo` `{by, undo}`,
//!   `POST /api/templates/{id}/keep` `{by, fingerprint, approver?}`,
//!   `POST /api/templates/{id}/share` `{by, mode: replace|add, name?, approver}`,
//!   `POST /api/templates/{id}/team/reset` `{by, approver}`,
//!   `DELETE /api/templates/{id}?by=` or `POST /api/templates/{id}/delete` `{by, approver?}`,
//!   `POST /api/templates/{id}/standards` `{heading, text, by, scope?, approver?}`,
//!   `DELETE /api/templates/{id}/standards/{sid}?by=` or `POST .../{sid}/remove` `{by, approver?}`,
//!   `POST /api/templates/{id}/standards/{sid}/mute` `{by, muted}`,
//!   `GET /api/sessions/{id}/template?pet=` (the latest draft's template, read-only)
//! - `POST /api/templates/from-example`   `{llm, text}`: a proposed template, not saved
//! - `GET|POST /api/team/settings` (`{by, templates: personal|hospital, approver}`),
//!   `POST /api/team/notice/dismiss` `{by}`, `POST /api/users/{uid}/admin` `{by, admin, approver}`,
//!   `GET /api/users/{uid}/profile?by=` (what a member keeps, before removing them)
//! - `GET  /api/live` (WebSocket)           browser microphone: binary f32 PCM frames
//!   (16 kHz mono); text "stop" ends it; the reply is the saved encounter
//! - `POST /api/record/start|stop`          record from this machine's microphone
//! - `GET  /api/models`, `POST /api/models/{id}/download`, `POST /api/models/remove`
//!   `{id, note_model, forget, by}`, `POST /api/models/import/inspect` `{path, by}`,
//!   `POST /api/models/import` `{path, mode, by}` (ADR-0025)
//! - `GET  /api/mlx`, `POST /api/mlx/install`, `POST /api/mlx/check`, `POST /api/mlx/remove`
//!   `{note_model, by}` (the MLX engine pack); `GET /api/gpu`, `POST /api/gpu/install`,
//!   `POST /api/gpu/remove` `{by}` (body optional)
//! - `GET|POST /api/hf-token` (`{token, by}`; the token is never sent back), `POST /api/hf-token/clear` `{by}`
//! - `GET  /api/hub/search?q=`, `GET /api/hub/repo?id=`, `POST /api/hub/add`, `GET /api/hub/alternatives?base=`
//! - `GET|POST /api/users`, `DELETE /api/users/{uid}?by=`, `POST /api/users/{uid}/pin`, `POST /api/users/{uid}/verify`
//! - Deleting (ADR-0022): `POST /api/sessions/{id}/delete` `{by, reason: {kind, note}, pin?, confirm_signed?}`
//!   → the visit moves to Deleted visits; `POST /api/sessions/{id}/recording/delete` `{by}` → the
//!   visit without its recording; `GET /api/deleted?by=`; `POST /api/deleted/{tid}/restore` `{by}`;
//!   `POST /api/deleted/{tid}/purge` `{by, pin?}` ("Delete now"). A PIN travels only in a body.
//! - Encrypted sync (ADR-0017; off until turned on): `GET /api/sync` (status), `POST /api/sync/server`
//!   `{url, key, by}`, `POST /api/sync/code` `{email, by}`, `POST /api/sync/verify`
//!   `{code, device_name, backfill_days?, by}`, `POST /api/sync/kit/confirm` `{answers: [[n, word]], by}`,
//!   `POST /api/sync/pair` `{words, approve, by}`, `POST /api/sync/pair/new`, `POST /api/sync/recover`
//!   `{words, by}`, `POST /api/sync/now`, `POST /api/sync/devices/{did}/remove` `{by}`, `POST /api/sync/off`
//!   `{by}`, `POST /api/sync/delete-account` `{confirm, by}`, `POST /api/sync/visits/{id}/stop` `{by}`.
//!   The browser never sees a key: this process holds them and makes every sync call.
//! - The clinic hub (ADR-0029; off until turned on): `POST /api/sync/hub/find` (mDNS, a few seconds),
//!   `POST /api/sync/hub/join` `{link, address?, check?, device_name, backfill_days?, existing, by}`,
//!   `GET /api/clinic-hub`, `POST /api/clinic-hub` `{on?, port?, allowed?, quota_mb?, by}`, `POST /api/clinic-hub/code`
//!   `{by}`, `POST /api/clinic-hub/join-self` `{device_name, existing, by}`, `POST /api/clinic-hub/devices/{did}/remove`
//!   `{by}`. The hub's own listener is separate (os-hub): this server stays on 127.0.0.1.
//! - `GET  /api/events`                     server-sent events: segment, token, level, download,
//!   progress, draft (prompt reading, then a writing heartbeat)
//! - Phone calls through the clinic's own Twilio account (ADR-0023; outgoing
//!   only): `GET /api/phone`, `GET /api/phone/bin?caller_id=` (the outgoing
//!   Bin's text), `POST /api/phone/connect` `{account_sid, api_key_sid,
//!   api_key_secret, computer_name, caller_id, outgoing_bin_url, by}` (the
//!   secret travels only in this body and is never sent back),
//!   `POST /api/phone/disconnect` `{by}`, `POST /api/phone/prefs` `{enabled,
//!   record_calls, announcement, by}`, `POST /api/phone/check` `{by}`,
//!   `POST /api/phone/prepare` `{patient_id?, number?, by}`, `GET /api/phone/pending`,
//!   `POST /api/phone/token` (a one-hour Voice token for the phone page),
//!   `GET /api/phone/live?number=&patient_id=&call_sid=&sample_rate=` (WebSocket:
//!   binary interleaved two-channel f32 PCM, clinic then client; text
//!   `{"consent": {"outcome": "agreed"|"declined"|"stayed_on_line", "announced_at": s}}` and
//!   `{"end": {"duration_secs", "call_sid"}}`). `/phone.html` is served with
//!   its own Content-Security-Policy, the only page allowed to reach Twilio.

use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use axum::Router;
use axum::body::Bytes;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{DefaultBodyLimit, Path as UrlPath, Query, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{delete, get, post};
use os_app::{
    App, AppError, Approver, AudioBlock, CallBlock, CallEnd, CallRequest, CallStart, ClinicTerm,
    ConnectRequest, ConsentMethod, ConsentOutcome, ConsentRule, DeleteRequest, Events, Patient, PdfRequest,
    PhonePrefs, RecordingConsent, RecordingSource, RefineRequest, SaveTemplateRequest, ShareMode,
    TemplateMode,
};
use os_engine::Segment;
use os_engine::template::{Scope, Template};
use os_engine::users::Role;
use os_models::ModelStore;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::BroadcastStream;
use tower_http::services::{ServeDir, ServeFile};

pub struct ServerOptions {
    pub port: u16,
    pub store: ModelStore,
    pub sessions_root: PathBuf,
    /// Built web UI (`apps/desktop/dist`).
    pub ui_dir: PathBuf,
}

struct Shared {
    /// Replaced by a fresh one on the new folders when a folder changes (ADR-0013).
    app: RwLock<Arc<App>>,
    events: Arc<Broadcaster>,
}

impl Shared {
    fn app(&self) -> Arc<App> {
        self.app.read().expect("app lock").clone()
    }
}

#[derive(Clone, Serialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
enum ServerEvent {
    Segment(Segment),
    Token(String),
    Level { level: f32, seconds: f32 },
    Download { model: String, done: u64, total: u64 },
    Progress(os_app::Progress),
    Moving { done: u64, total: u64 },
    Mic(os_app::MicHealth),
    Draft(os_app::DraftPhase),
}

struct Broadcaster(broadcast::Sender<ServerEvent>);

impl Events for Broadcaster {
    fn segment(&self, s: &Segment) {
        let _ = self.0.send(ServerEvent::Segment(s.clone()));
    }
    fn token(&self, p: &str) {
        let _ = self.0.send(ServerEvent::Token(p.to_string()));
    }
    fn level(&self, level: f32, seconds: f32) {
        let _ = self.0.send(ServerEvent::Level { level, seconds });
    }
    fn download(&self, model: &str, done: u64, total: u64) {
        let _ = self.0.send(ServerEvent::Download { model: model.to_string(), done, total });
    }
    fn progress(&self, p: &os_app::Progress) {
        let _ = self.0.send(ServerEvent::Progress(p.clone()));
    }
    fn moving(&self, done: u64, total: u64) {
        let _ = self.0.send(ServerEvent::Moving { done, total });
    }
    fn mic(&self, health: &os_app::MicHealth) {
        let _ = self.0.send(ServerEvent::Mic(health.clone()));
    }
    fn draft(&self, phase: &os_app::DraftPhase) {
        let _ = self.0.send(ServerEvent::Draft(*phase));
    }
}

type St = Arc<Shared>;

struct ApiError(StatusCode, String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(serde_json::json!({ "error": self.1 }))).into_response()
    }
}

impl From<AppError> for ApiError {
    fn from(e: AppError) -> Self {
        ApiError(StatusCode::BAD_REQUEST, e.0)
    }
}

/// Run blocking app work off the async runtime.
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, AppError> + Send + 'static,
) -> Result<Json<T>, ApiError> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map(Json)
        .map_err(ApiError::from)
}

pub async fn serve(options: ServerOptions) -> std::io::Result<()> {
    let (tx, _) = broadcast::channel(4096);
    let app = Arc::new(App::new(options.store, options.sessions_root));
    // Deleted visits: recover what a crash left, remove what's due, then hourly.
    app.start_housekeeping();
    // Encrypted sync polls every 30 s once turned on (ADR-0017).
    app.start_sync();
    let state: St = Arc::new(Shared { app: RwLock::new(app), events: Arc::new(Broadcaster(tx)) });
    let shared = state.clone();
    let app = router(state, &options.ui_dir, options.port);

    // Loopback only: there is no authentication yet (see module docs).
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, options.port));
    // A restart (POST /api/restart) starts the new copy before the old one has
    // let go of the port: wait for it, briefly.
    let mut tries = 0;
    let listener = loop {
        match tokio::net::TcpListener::bind(addr).await {
            Ok(l) => break l,
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse && tries < 40 => {
                tries += 1;
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            }
            Err(e) => return Err(e),
        }
    };
    tracing::info!(%addr, "OpenBark web mode");
    tokio::select! {
        served = axum::serve(listener, app) => served,
        () = stop_requested() => {
            eprintln!("OpenBark is stopping.");
            stop(shared.app()).await
        }
    }
}

/// Ctrl+C, or a polite termination request (SIGTERM, as from `kill` or a
/// service manager).
async fn stop_requested() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        match signal(SignalKind::terminate()) {
            Ok(mut term) => tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                _ = term.recv() => {}
            },
            Err(_) => {
                let _ = tokio::signal::ctrl_c().await;
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

/// Free the note model, then end the process (os_app::App::shutdown): with
/// a model still loaded at exit, llama.cpp's Metal teardown aborts on macOS.
async fn stop(app: Arc<App>) -> ! {
    let _ = tokio::task::spawn_blocking(move || app.shutdown(os_app::SHUTDOWN_WAIT)).await;
    os_app::exit_process(0)
}

/// The whole server: the API, the UI's files, and the guard that keeps other
/// pages out ([`local_only`]).
fn router(state: St, ui_dir: &std::path::Path, port: u16) -> Router {
    let index = ui_dir.join("index.html");
    let api = Router::new()
        .route("/api/status", get(|State(s): State<St>| async move { Json(s.app().status()) }))
        .route("/api/devices", get(|State(s): State<St>| async move { Json(s.app().input_devices()) }))
        .route("/api/patients", get(list_patients).post(create_patient))
        .route("/api/patients/{id}", get(patient).post(update_patient_record))
        .route("/api/sessions", get(list_sessions).post(create_session))
        .route("/api/sessions/{id}", get(open_session))
        .route("/api/sessions/{id}/audio", get(session_audio))
        .route("/api/sessions/{id}/notes", post(create_note))
        .route("/api/notes/cancel", post(|State(s): State<St>| async move { Json(s.app().cancel_draft()) }))
        .route("/api/speed-check", post(speed_check))
        .route("/api/diagnostics", get(diagnostics))
        .route("/api/sessions/{id}/sign", post(sign_note))
        .route("/api/sessions/{id}/chart/confirm", post(confirm_chart))
        .route("/api/sessions/{id}/defaults", post(settle_defaults))
        .route("/api/sessions/{id}/patient", post(update_patient))
        .route("/api/sessions/{id}/refine", post(refine_note))
        .route("/api/sessions/{id}/edit", post(edit_note))
        .route("/api/sessions/{id}/addenda", post(add_addendum))
        .route("/api/sessions/{id}/pdf", post(export_pdf))
        .route("/api/sessions/{id}/delete", post(delete_visit))
        .route("/api/sessions/{id}/recording/delete", post(delete_recording))
        .route("/api/deleted", get(list_deleted))
        .route("/api/deleted/{tid}/restore", post(restore_visit))
        .route("/api/deleted/{tid}/purge", post(purge_deleted))
        .route("/api/templates", get(list_templates).post(save_template))
        .route("/api/templates/preview", post(preview_template))
        .route("/api/templates/from-example", post(template_from_example))
        .route("/api/templates/{id}", get(template).delete(delete_template))
        .route("/api/templates/{id}/delete", post(delete_template_approved))
        .route("/api/templates/{id}/compare", get(compare_template))
        .route("/api/templates/{id}/reset", post(reset_template))
        .route("/api/templates/{id}/reset/undo", post(undo_reset_template))
        .route("/api/templates/{id}/keep", post(keep_template))
        .route("/api/templates/{id}/share", post(share_template))
        .route("/api/templates/{id}/team/reset", post(reset_team_template))
        .route("/api/templates/{id}/standards", post(add_standard))
        .route("/api/templates/{id}/standards/{sid}", delete(remove_standard))
        .route("/api/templates/{id}/standards/{sid}/remove", post(remove_standard_approved))
        .route("/api/templates/{id}/standards/{sid}/mute", post(mute_standard))
        .route("/api/sessions/{id}/template", get(run_template_view))
        .route("/api/team/settings", get(team_settings).post(set_team_settings))
        .route("/api/team/notice/dismiss", post(dismiss_setup_notice))
        .route("/api/vocabulary", get(vocabulary))
        .route("/api/vocabulary/save", post(save_vocabulary_term))
        .route("/api/vocabulary/remove", post(remove_vocabulary_term))
        .route("/api/speech-language", post(set_speech_language))
        .route("/api/spoken-language", post(set_spoken_language))
        .route("/api/users/{uid}/admin", post(set_user_admin))
        .route("/api/users/{uid}/profile", get(profile_summary))
        .route("/api/live", get(live))
        .route("/api/record/prewarm", post(record_prewarm))
        .route("/api/record/release", post(record_release))
        .route("/api/record/start", post(record_start))
        .route("/api/record/stop", post(record_stop))
        .route("/api/models", get(|State(s): State<St>| async move { Json(s.app().models()) }))
        .route("/api/models/{id}/download", post(download_model))
        .route("/api/models/remove", post(remove_model))
        .route("/api/models/import/inspect", post(inspect_import))
        .route("/api/models/import", post(import_model))
        // Off the async runtime: it walks the pack's folder and reads the
        // note-model slot (never waiting on a model load).
        .route(
            "/api/mlx",
            get(|State(s): State<St>| async move { blocking(move || Ok(s.app().mlx_status())).await }),
        )
        .route("/api/mlx/install", post(install_mlx))
        .route("/api/mlx/check", post(check_mlx))
        .route("/api/mlx/remove", post(remove_mlx))
        .route("/api/gpu", get(|State(s): State<St>| async move { Json(s.app().gpu_status()) }))
        .route("/api/gpu/install", post(install_gpu))
        .route("/api/gpu/remove", post(remove_gpu))
        .route("/api/hf-token", get(hf_token_status).post(set_hf_token))
        .route("/api/hf-token/clear", post(clear_hf_token))
        .route("/api/update", get(|State(s): State<St>| async move { Json(s.app().update_status()) }))
        .route("/api/update/check", post(check_for_update))
        .route("/api/restart", post(restart))
        .route("/api/hub/search", get(hub_search))
        .route("/api/hub/repo", get(hub_repo))
        .route("/api/hub/add", post(hub_add))
        .route("/api/hub/alternatives", get(hub_alternatives))
        .route("/api/encryption/encrypt-existing", post(encrypt_existing))
        .route("/api/encryption/recovery-key", get(recovery_key))
        .route("/api/users", get(list_users).post(add_user))
        .route("/api/users/{uid}", delete(remove_user))
        .route("/api/users/{uid}/pin", post(set_user_pin))
        .route("/api/users/{uid}/verify", post(verify_user))
        .route("/api/storage", get(|State(s): State<St>| async move { Json(s.app().storage()) }))
        .route("/api/storage/check", post(check_folder))
        .route("/api/storage/change", post(change_folder))
        .route("/api/folders", get(list_folders))
        .route("/api/sync", get(|State(s): State<St>| async move { Json(s.app().sync_status()) }))
        .route("/api/sync/server", post(sync_server))
        .route("/api/sync/code", post(sync_code))
        .route("/api/sync/verify", post(sync_verify))
        .route("/api/sync/kit/confirm", post(sync_confirm_kit))
        .route("/api/sync/pair", post(sync_pair))
        .route("/api/sync/pair/new", post(sync_new_code))
        .route("/api/sync/recover", post(sync_recover))
        .route("/api/sync/now", post(sync_now))
        .route("/api/sync/devices/{did}/remove", post(sync_remove_device))
        .route("/api/sync/off", post(sync_off))
        .route("/api/sync/delete-account", post(sync_delete_account))
        .route("/api/sync/visits/{id}/stop", post(sync_stop_visit))
        .route("/api/sync/hub/find", post(sync_find_hubs))
        .route("/api/sync/hub/join", post(sync_join_hub))
        .route(
            "/api/clinic-hub",
            get(|State(s): State<St>, Query(q): Query<ByQuery>| async move {
                Json(s.app().clinic_hub_status(q.by.as_deref()))
            })
            .post(clinic_hub_settings),
        )
        .route("/api/sync/pair/confirm", post(sync_confirm_pairing))
        .route("/api/sync/start-over", post(sync_start_over))
        .route("/api/clinic-hub/code", post(clinic_hub_code))
        .route("/api/clinic-hub/join-self", post(clinic_hub_join_self))
        .route("/api/clinic-hub/devices/{did}/remove", post(clinic_hub_remove_device))
        .route("/api/events", get(events_stream))
        .route("/api/phone", get(phone_view))
        .route("/api/phone/bin", get(phone_bin))
        .route("/api/phone/connect", post(phone_connect))
        .route("/api/phone/disconnect", post(phone_disconnect))
        .route("/api/phone/prefs", post(phone_prefs))
        .route("/api/phone/check", post(phone_check))
        .route("/api/phone/prepare", post(phone_prepare))
        .route("/api/phone/pending", get(phone_pending))
        .route("/api/phone/token", post(phone_token))
        .route("/api/phone/live", get(phone_live))
        .layer(DefaultBodyLimit::max(1 << 30))
        .with_state(state);
    api.fallback_service(ServeDir::new(ui_dir).fallback(ServeFile::new(index)))
        .layer(axum::middleware::from_fn_with_state(port, page_policy))
        .layer(axum::middleware::from_fn_with_state(port, local_only))
}

/// Twilio's calling service, as the Voice SDK reaches it (docs/network.md
/// R10): signalling through the roaming edge, then the edge Twilio picked
/// (the SDK reconnects there), and call-quality events, by default and for
/// each home region. The SDK's sounds are local files, so no sound host.
pub const TWILIO_CONNECT: &str = "wss://voice-js.roaming.twilio.com wss://voice-js.ashburn.twilio.com \
wss://voice-js.umatilla.twilio.com wss://voice-js.dublin.twilio.com wss://voice-js.frankfurt.twilio.com \
wss://voice-js.sao-paulo.twilio.com wss://voice-js.sydney.twilio.com wss://voice-js.singapore.twilio.com \
wss://voice-js.tokyo.twilio.com https://eventgw.twilio.com https://eventgw.us1.twilio.com \
https://eventgw.ie1.twilio.com https://eventgw.au1.twilio.com";

/// This server's own WebSockets, on its own port only.
fn own_sockets(port: u16) -> String {
    format!("ws://127.0.0.1:{port} ws://localhost:{port}")
}

/// The phone page's Content-Security-Policy (ADR-0023 section 4): the only
/// page of web mode that may reach Twilio, and only its calling service.
pub fn phone_csp(port: u16) -> String {
    format!(
        "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; \
font-src 'self'; media-src 'self' mediastream: blob:; connect-src 'self' {} {TWILIO_CONNECT}; worker-src 'self'; \
object-src 'none'; base-uri 'none'; frame-ancestors 'none'",
        own_sockets(port)
    )
}

/// Every other page: this computer only. The main page carries the same
/// rule in a `<meta>` tag (apps/desktop/vite.config.ts), for the desktop app.
pub fn page_csp(port: u16) -> String {
    format!(
        "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; \
font-src 'self'; media-src 'self' blob:; connect-src 'self' {}; object-src 'none'; base-uri 'none'; \
frame-ancestors 'none'",
        own_sockets(port)
    )
}

/// Sets each page's Content-Security-Policy: the phone page's own, which
/// allows Twilio, and the local-only one everywhere else.
async fn page_policy(
    State(port): State<u16>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    let phone = request.uri().path() == "/phone.html";
    let mut response = next.run(request).await;
    let html = response
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("text/html"));
    if html {
        let policy = if phone { phone_csp(port) } else { page_csp(port) };
        if let Ok(value) = axum::http::HeaderValue::from_str(&policy) {
            response.headers_mut().insert(axum::http::header::CONTENT_SECURITY_POLICY, value);
        }
    }
    response
}

// ---- phone calls (ADR-0023) ----------------------------------------------------

async fn phone_view(State(s): State<St>) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().phone()).await
}

#[derive(Deserialize)]
struct BinQuery {
    #[serde(default)]
    caller_id: String,
}

async fn phone_bin(State(s): State<St>, Query(q): Query<BinQuery>) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().phone_bin_text(&q.caller_id).map(|text| serde_json::json!({ "text": text })))
        .await
}

/// No `Debug`: it carries the API key secret.
#[derive(Deserialize)]
struct PhoneConnectBody {
    #[serde(flatten)]
    request: ConnectRequest,
    #[serde(default)]
    by: Option<String>,
}

async fn phone_connect(
    State(s): State<St>,
    Json(b): Json<PhoneConnectBody>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().phone_connect(b.request, b.by.as_deref())).await
}

async fn phone_disconnect(State(s): State<St>, Json(b): Json<ByBody>) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().phone_disconnect(b.by.as_deref())).await
}

#[derive(Deserialize)]
struct PrefsBody {
    #[serde(flatten)]
    prefs: PhonePrefs,
    #[serde(default)]
    by: Option<String>,
}

async fn phone_prefs(State(s): State<St>, Json(b): Json<PrefsBody>) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().phone_set_prefs(b.prefs, b.by.as_deref())).await
}

async fn phone_check(State(s): State<St>, Json(b): Json<ByBody>) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().phone_check(b.by.as_deref())).await
}

#[derive(Deserialize)]
struct PrepareBody {
    #[serde(flatten)]
    call: CallRequest,
    #[serde(default)]
    by: Option<String>,
}

async fn phone_prepare(
    State(s): State<St>,
    Json(b): Json<PrepareBody>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().phone_prepare(b.call, b.by.as_deref())).await
}

async fn phone_pending(State(s): State<St>) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().phone_pending()).await
}

/// A POST, so it needs the `X-OpenBark` header like every change (ADR-0023
/// section 10): only this computer's own phone page gets a token.
async fn phone_token(State(s): State<St>) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().phone_token()).await
}

#[derive(Deserialize)]
struct PhoneLiveQuery {
    number: String,
    #[serde(default)]
    patient_id: Option<String>,
    #[serde(default)]
    call_sid: Option<String>,
    #[serde(default = "rate")]
    sample_rate: u32,
    /// Seconds since the client answered (audio kept by the page meanwhile).
    #[serde(default)]
    since_answer: Option<f32>,
}

/// What the phone page sends as text on the call's WebSocket.
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum CallMessage {
    /// "I've read it": the announcement was read to the client.
    Announced {
        #[serde(default)]
        at: Option<f32>,
    },
    Consent {
        outcome: ConsentOutcome,
        #[serde(default)]
        announced_at: Option<f32>,
    },
    End(CallEnd),
}

/// A recorded call (ADR-0023 section 7): the phone page streams both sides
/// as interleaved two-channel f32 PCM. The visit is saved when the page sends
/// `end`, or when the socket closes (a dropped call keeps what was recorded).
async fn phone_live(State(s): State<St>, Query(q): Query<PhoneLiveQuery>, ws: WebSocketUpgrade) -> Response {
    ws.on_upgrade(move |socket| phone_session(s, q, socket))
}

async fn phone_session(s: St, q: PhoneLiveQuery, mut socket: WebSocket) {
    let (tx, rx) = std::sync::mpsc::channel::<CallBlock>();
    let events: Arc<dyn Events> = s.events.clone();
    let start = CallStart {
        number: q.number,
        patient_id: q.patient_id,
        call_sid: q.call_sid,
        // Checked by the engine: a rate the recorder can't use is refused now.
        sample_rate: Some(q.sample_rate),
        since_answer_secs: q.since_answer,
    };
    let started = {
        let s = s.clone();
        tokio::task::spawn_blocking(move || s.app().start_call(start, rx, events)).await
    };
    let reply = |v: serde_json::Value| Message::Text(v.to_string().into());
    match started {
        Ok(Ok(id)) => {
            let _ = socket.send(reply(serde_json::json!({ "started": id }))).await;
        }
        Ok(Err(e)) => {
            let _ = socket.send(reply(serde_json::json!({ "error": e.0 }))).await;
            return;
        }
        Err(e) => {
            let _ = socket.send(reply(serde_json::json!({ "error": e.to_string() }))).await;
            return;
        }
    }
    let mut tx = Some(tx);
    let mut end = CallEnd::default();
    while let Some(Ok(msg)) = socket.recv().await {
        match msg {
            Message::Binary(bytes) => {
                // After the client declines, the call goes on unrecorded: audio is dropped.
                if let Some(sender) = &tx {
                    let samples: Vec<f32> = bytes
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                        .collect();
                    if sender.send(CallBlock::from_interleaved(&samples, q.sample_rate)).is_err() {
                        tx = None;
                    }
                }
            }
            Message::Text(t) => match serde_json::from_str::<CallMessage>(t.as_str()) {
                Ok(CallMessage::Consent { outcome, announced_at }) => {
                    let s2 = s.clone();
                    let done =
                        tokio::task::spawn_blocking(move || s2.app().call_consent(outcome, announced_at))
                            .await;
                    let msg = match done {
                        Ok(Ok(Some(view))) => {
                            tx = None;
                            serde_json::json!({ "declined": view })
                        }
                        Ok(Ok(None)) => serde_json::json!({ "consent": outcome }),
                        Ok(Err(e)) => serde_json::json!({ "error": e.0 }),
                        Err(e) => serde_json::json!({ "error": e.to_string() }),
                    };
                    let _ = socket.send(reply(msg)).await;
                }
                Ok(CallMessage::Announced { at }) => {
                    let s2 = s.clone();
                    let done = tokio::task::spawn_blocking(move || s2.app().call_announced(at)).await;
                    let msg = match done {
                        Ok(Ok(())) => serde_json::json!({ "announced": true }),
                        Ok(Err(e)) => serde_json::json!({ "error": e.0 }),
                        Err(e) => serde_json::json!({ "error": e.to_string() }),
                    };
                    let _ = socket.send(reply(msg)).await;
                }
                Ok(CallMessage::End(e)) => {
                    end = e;
                    break;
                }
                Err(_) => {
                    let _ = socket
                        .send(reply(
                            serde_json::json!({ "error": "OpenBark didn't understand that message." }),
                        ))
                        .await;
                }
            },
            Message::Close(_) => break,
            _ => {}
        }
    }
    drop(tx); // ends the recording's source
    let result = tokio::task::spawn_blocking(move || s.app().end_call(end)).await;
    let msg = match result {
        Ok(Ok(view)) => serde_json::json!({ "session": view }),
        Ok(Err(e)) => serde_json::json!({ "error": e.0 }),
        Err(e) => serde_json::json!({ "error": e.to_string() }),
    };
    let _ = socket.send(reply(msg)).await;
    let _ = socket.send(Message::Close(None)).await;
}

// ---- deleting (ADR-0022) -------------------------------------------------------

// ---- encrypted sync (ADR-0017) ---------------------------------------------

#[derive(Deserialize)]
struct SyncServerBody {
    #[serde(default)]
    url: String,
    #[serde(default)]
    key: String,
    #[serde(default)]
    by: Option<String>,
}

async fn sync_server(
    State(s): State<St>,
    Json(r): Json<SyncServerBody>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().sync_set_server(&r.url, &r.key, r.by.as_deref())).await
}

#[derive(Deserialize)]
struct SyncCodeBody {
    email: String,
    #[serde(default)]
    by: Option<String>,
}

async fn sync_code(State(s): State<St>, Json(r): Json<SyncCodeBody>) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().sync_send_code(&r.email, r.by.as_deref())).await
}

#[derive(Deserialize)]
struct SyncVerifyBody {
    code: String,
    #[serde(default)]
    device_name: String,
    /// How far back this computer's visits upload; absent for all.
    #[serde(default)]
    backfill_days: Option<u32>,
    #[serde(default)]
    by: Option<String>,
}

async fn sync_verify(
    State(s): State<St>,
    Json(r): Json<SyncVerifyBody>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().sync_verify(&r.code, &r.device_name, r.backfill_days, r.by.as_deref())).await
}

#[derive(Deserialize)]
struct SyncKitBody {
    answers: Vec<(usize, String)>,
    #[serde(default)]
    by: Option<String>,
}

async fn sync_confirm_kit(
    State(s): State<St>,
    Json(r): Json<SyncKitBody>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().sync_confirm_kit(r.answers, r.by.as_deref())).await
}

#[derive(Deserialize)]
struct SyncWordsBody {
    words: String,
    #[serde(default)]
    approve: bool,
    #[serde(default)]
    by: Option<String>,
}

async fn sync_pair(State(s): State<St>, Json(r): Json<SyncWordsBody>) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().sync_pair(&r.words, r.approve, r.by.as_deref())).await
}

async fn sync_new_code(State(s): State<St>) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().sync_new_code()).await
}

async fn sync_recover(
    State(s): State<St>,
    Json(r): Json<SyncWordsBody>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().sync_recover(&r.words, r.by.as_deref())).await
}

async fn sync_now(State(s): State<St>) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().sync_now()).await
}

async fn sync_remove_device(
    State(s): State<St>,
    UrlPath(did): UrlPath<String>,
    Json(r): Json<ByBody>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().sync_remove_device(&did, r.by.as_deref())).await
}

async fn sync_off(State(s): State<St>, Json(r): Json<ByBody>) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().sync_off(r.by.as_deref())).await
}

#[derive(Deserialize)]
struct SyncDeleteBody {
    confirm: String,
    #[serde(default)]
    by: Option<String>,
}

async fn sync_delete_account(
    State(s): State<St>,
    Json(r): Json<SyncDeleteBody>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().sync_delete_account(&r.confirm, r.by.as_deref())).await
}

async fn sync_stop_visit(
    State(s): State<St>,
    UrlPath(id): UrlPath<String>,
    Json(r): Json<ByBody>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().sync_stop_visit(&id, r.by.as_deref())).await
}

// ---- the clinic hub (ADR-0029) ----------------------------------------------

#[derive(Deserialize)]
struct SyncConfirmBody {
    code: String,
    #[serde(default)]
    by: Option<String>,
}

async fn sync_confirm_pairing(
    State(s): State<St>,
    Json(r): Json<SyncConfirmBody>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().sync_confirm_pairing(&r.code, r.by.as_deref())).await
}

async fn sync_start_over(State(s): State<St>, Json(r): Json<ByBody>) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().sync_start_over(r.by.as_deref())).await
}

async fn sync_find_hubs(State(s): State<St>) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().sync_find_hubs()).await
}

#[derive(Deserialize)]
struct SyncJoinHubBody {
    /// The QR code's text, or the 8-character join code.
    link: String,
    #[serde(default)]
    address: String,
    #[serde(default)]
    check: String,
    #[serde(default)]
    device_name: String,
    #[serde(default)]
    backfill_days: Option<u32>,
    /// The person already syncs on another device.
    #[serde(default)]
    existing: bool,
    #[serde(default)]
    by: Option<String>,
}

async fn sync_join_hub(
    State(s): State<St>,
    Json(r): Json<SyncJoinHubBody>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || {
        s.app().sync_join_hub(
            &r.link,
            &r.address,
            &r.check,
            &r.device_name,
            r.backfill_days,
            r.existing,
            r.by.as_deref(),
        )
    })
    .await
}

#[derive(Deserialize)]
struct HubSettingsBody {
    #[serde(default)]
    on: Option<bool>,
    #[serde(default)]
    port: Option<u16>,
    #[serde(default)]
    allowed: Option<Vec<String>>,
    #[serde(default)]
    quota_mb: Option<u64>,
    #[serde(default)]
    by: Option<String>,
}

async fn clinic_hub_settings(
    State(s): State<St>,
    Json(r): Json<HubSettingsBody>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().clinic_hub_settings(r.on, r.port, r.allowed, r.quota_mb, r.by.as_deref())).await
}

async fn clinic_hub_code(State(s): State<St>, Json(r): Json<ByBody>) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().clinic_hub_join_code(r.by.as_deref())).await
}

#[derive(Deserialize)]
struct HubJoinSelfBody {
    #[serde(default)]
    device_name: String,
    #[serde(default)]
    existing: bool,
    #[serde(default)]
    by: Option<String>,
}

async fn clinic_hub_join_self(
    State(s): State<St>,
    Json(r): Json<HubJoinSelfBody>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().clinic_hub_join_self(&r.device_name, r.existing, r.by.as_deref())).await
}

async fn clinic_hub_remove_device(
    State(s): State<St>,
    UrlPath(did): UrlPath<String>,
    Json(r): Json<ByBody>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().clinic_hub_remove_device(&did, r.by.as_deref())).await
}

async fn delete_visit(
    State(s): State<St>,
    UrlPath(id): UrlPath<String>,
    Json(req): Json<DeleteRequest>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().delete_visit(&id, &req)).await
}

async fn delete_recording(
    State(s): State<St>,
    UrlPath(id): UrlPath<String>,
    Json(req): Json<ByBody>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().delete_recording(&id, req.by.as_deref())).await
}

async fn list_deleted(State(s): State<St>, Query(q): Query<ByQuery>) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().list_deleted(q.by.as_deref())).await
}

async fn restore_visit(
    State(s): State<St>,
    UrlPath(tid): UrlPath<String>,
    Json(req): Json<ByBody>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().restore_visit(&tid, req.by.as_deref())).await
}

/// "Delete now". The PIN (for a signed visit) is read from the body only:
/// never from the URL, never logged.
async fn purge_deleted(
    State(s): State<St>,
    UrlPath(tid): UrlPath<String>,
    Json(req): Json<PurgeBody>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || {
        s.app().purge_deleted(&tid, req.by.as_deref(), req.pin.as_deref()).map(|()| serde_json::json!({}))
    })
    .await
}

#[derive(Deserialize)]
struct ByBody {
    #[serde(default)]
    by: Option<String>,
}

/// No `Debug`: it carries a PIN.
#[derive(Deserialize)]
struct PurgeBody {
    #[serde(default)]
    by: Option<String>,
    #[serde(default)]
    pin: Option<String>,
}

// ---- storage (ADR-0013) ------------------------------------------------------

#[derive(Deserialize)]
struct FolderRequest {
    kind: os_app::FolderKind,
    path: String,
    #[serde(default = "switch")]
    mode: os_app::ChangeMode,
}

/// Only this computer's own OpenBark page may use the server (review
/// 2026-10-18): the Host must be this server, an Origin (sent by browsers on
/// cross-site and WebSocket requests) must be this server, and anything other
/// than GET or HEAD needs the `X-OpenBark` header.
async fn local_only(
    State(port): State<u16>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    use axum::http::{Method, header};
    let ours = [format!("127.0.0.1:{port}"), format!("localhost:{port}"), format!("[::1]:{port}")];
    let host =
        request.headers().get(header::HOST).and_then(|v| v.to_str().ok()).unwrap_or("").to_ascii_lowercase();
    if !ours.contains(&host) {
        return (StatusCode::FORBIDDEN, "OpenBark only answers this computer's own page.").into_response();
    }
    if let Some(origin) = request.headers().get(header::ORIGIN) {
        let origin = origin.to_str().unwrap_or("").to_ascii_lowercase();
        if !ours.iter().any(|h| origin == format!("http://{h}")) {
            return (StatusCode::FORBIDDEN, "OpenBark only answers this computer's own page.")
                .into_response();
        }
    }
    // The folder browser reads the disk, so it needs the header even for GET:
    // a cross-site <img> could otherwise make it open a network share.
    let changes =
        !matches!(*request.method(), Method::GET | Method::HEAD) || request.uri().path() == "/api/folders";
    if changes && request.uri().path().starts_with("/api/") && !request.headers().contains_key("x-openbark") {
        return (StatusCode::FORBIDDEN, "Missing the X-OpenBark header.").into_response();
    }
    next.run(request).await
}

fn switch() -> os_app::ChangeMode {
    os_app::ChangeMode::Switch
}

async fn check_folder(
    State(s): State<St>,
    Json(r): Json<FolderRequest>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || Ok(s.app().check_folder(r.kind, &r.path, true))).await
}

/// Change a folder, then carry on with a fresh app on the new folders.
async fn change_folder(
    State(s): State<St>,
    Json(r): Json<FolderRequest>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || {
        // The App's own gate makes changes one at a time and refuses while
        // anything is writing; once changed it's retired until replaced here.
        let app = s.app();
        let changed = app.change_folder(r.kind, &r.path, r.mode, true, s.events.as_ref())?;
        let (models, visits) = match r.kind {
            os_app::FolderKind::Models => (PathBuf::from(&changed.path), app.sessions_root.clone()),
            os_app::FolderKind::Visits => (app.store.root().to_path_buf(), PathBuf::from(&changed.path)),
        };
        drop(app);
        let fresh = Arc::new(App::new(ModelStore::new(models), visits));
        // The new folder's own Deleted visits (ADR-0022 section 9).
        fresh.start_housekeeping();
        fresh.start_sync();
        *s.app.write().expect("app lock") = fresh;
        Ok(changed)
    })
    .await
}

#[derive(Deserialize)]
struct FolderQuery {
    #[serde(default)]
    path: String,
}

#[derive(Serialize)]
struct FolderListing {
    /// The folder listed; empty for the starting places.
    path: String,
    parent: Option<String>,
    folders: Vec<FolderEntry>,
}

#[derive(Serialize)]
struct FolderEntry {
    name: String,
    path: String,
}

/// Folders inside `path`, for choosing a folder in the browser (web mode has
/// no native folder picker). With no path: drives and the usual places.
async fn list_folders(Query(q): Query<FolderQuery>) -> Result<impl IntoResponse, ApiError> {
    blocking(move || {
        let entry = |p: PathBuf| FolderEntry {
            name: p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| p.display().to_string()),
            path: p.display().to_string(),
        };
        if q.path.trim().is_empty() {
            let mut folders: Vec<FolderEntry> = Vec::new();
            for dir in [dirs::home_dir(), dirs::document_dir(), dirs::desktop_dir()].into_iter().flatten() {
                folders.push(entry(dir));
            }
            if cfg!(windows) {
                for letter in b'A'..=b'Z' {
                    let root = PathBuf::from(format!("{}:\\", letter as char));
                    if root.exists() {
                        folders.push(FolderEntry {
                            name: root.display().to_string(),
                            path: root.display().to_string(),
                        });
                    }
                }
            } else {
                for root in ["/", "/Volumes", "/media", "/mnt"] {
                    if PathBuf::from(root).is_dir() {
                        folders.push(FolderEntry { name: root.into(), path: root.into() });
                    }
                }
            }
            return Ok(FolderListing { path: String::new(), parent: None, folders });
        }
        let mut typed = q.path.trim().to_string();
        // "D:" alone means the drive's current folder on Windows; mean its root.
        if typed.len() == 2 && typed.ends_with(':') {
            typed.push('\\');
        }
        let dir = PathBuf::from(&typed);
        // A web page mustn't make OpenBark open a network share (it would
        // send this computer's Windows credentials to it).
        if os_models::locations::is_network_path(&dir) {
            return Err(AppError(
                "Network folders can't be browsed in the browser version. Map the share to a drive letter first, or use the desktop app.".into(),
            ));
        }
        let read =
            std::fs::read_dir(&dir).map_err(|e| AppError(format!("Can't open {}: {e}", dir.display())))?;
        let mut folders: Vec<FolderEntry> = read
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .filter(|e| !e.file_name().to_string_lossy().starts_with(['.', '$']))
            .map(|e| entry(e.path()))
            .collect();
        folders.sort_by_key(|f| f.name.to_lowercase());
        Ok(FolderListing {
            path: dir.display().to_string(),
            parent: Some(dir.parent().map(|p| p.display().to_string()).unwrap_or_default()),
            folders,
        })
    })
    .await
}

/// Start a fresh copy of this server (same arguments and settings) and stop
/// this one, so newly installed libraries load (the GPU speech pack). Refused
/// while recording.
async fn restart(State(s): State<St>) -> Result<impl IntoResponse, ApiError> {
    let app = s.app();
    if app.is_recording() {
        return Err(ApiError(StatusCode::CONFLICT, "Stop the recording first, then restart.".into()));
    }
    if app.is_busy() {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "OpenBark is importing, drafting, saving or downloading. Restart when that's finished.".into(),
        ));
    }
    let exe =
        std::env::current_exe().map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    std::process::Command::new(exe).args(std::env::args_os().skip(1)).spawn().map_err(|e| {
        ApiError(StatusCode::INTERNAL_SERVER_ERROR, format!("OpenBark couldn't restart: {e}"))
    })?;
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        stop(app).await
    });
    Ok(Json(serde_json::json!({ "restarting": true })))
}

#[derive(Deserialize)]
struct InstallGpu {
    /// The clinic ticked that it accepts NVIDIA's licence for its libraries.
    #[serde(default)]
    accept_license: bool,
}

async fn install_gpu(State(s): State<St>, Json(r): Json<InstallGpu>) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().install_gpu(r.accept_license, s.events.as_ref())).await
}

/// The body (`{by}`) is optional: older screens sent none.
async fn remove_gpu(State(s): State<St>, body: Bytes) -> Result<impl IntoResponse, ApiError> {
    let by = if body.iter().all(u8::is_ascii_whitespace) {
        None
    } else {
        serde_json::from_slice::<ByBody>(&body)
            .map_err(|e| ApiError(StatusCode::BAD_REQUEST, e.to_string()))?
            .by
    };
    blocking(move || s.app().remove_gpu(by.as_deref())).await
}

// ---- models of your choice and the MLX engine (ADR-0025) -------------------

#[derive(Deserialize)]
struct RemoveModel {
    id: String,
    /// The note model chosen on the requesting device.
    #[serde(default)]
    note_model: Option<String>,
    #[serde(default)]
    forget: bool,
    #[serde(default)]
    by: Option<String>,
}

async fn remove_model(
    State(s): State<St>,
    Json(r): Json<RemoveModel>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().remove_model(&r.id, r.note_model.as_deref(), r.forget, r.by.as_deref())).await
}

#[derive(Deserialize)]
struct InspectImport {
    path: String,
    #[serde(default)]
    by: Option<String>,
}

async fn inspect_import(
    State(s): State<St>,
    Json(r): Json<InspectImport>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().inspect_import(&r.path, r.by.as_deref())).await
}

#[derive(Deserialize)]
struct ImportModel {
    path: String,
    mode: os_app::ImportMode,
    #[serde(default)]
    by: Option<String>,
}

async fn import_model(
    State(s): State<St>,
    Json(r): Json<ImportModel>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || {
        let events = s.events.clone();
        s.app().import_model(&r.path, r.mode, r.by.as_deref(), events.as_ref())
    })
    .await
}

async fn install_mlx(State(s): State<St>) -> Result<impl IntoResponse, ApiError> {
    blocking(move || {
        let events = s.events.clone();
        s.app().install_mlx(events.as_ref())
    })
    .await
}

async fn check_mlx(State(s): State<St>) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().check_mlx()).await
}

#[derive(Deserialize)]
struct RemoveMlx {
    #[serde(default)]
    note_model: Option<String>,
    #[serde(default)]
    by: Option<String>,
}

async fn remove_mlx(State(s): State<St>, Json(r): Json<RemoveMlx>) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().remove_mlx(r.note_model.as_deref(), r.by.as_deref())).await
}

async fn hf_token_status(State(s): State<St>) -> Result<impl IntoResponse, ApiError> {
    blocking(move || Ok(s.app().hf_token_status())).await
}

/// No `Debug`: it carries the token, which is never logged or sent back.
#[derive(Deserialize)]
struct SetToken {
    token: String,
    #[serde(default)]
    by: Option<String>,
}

async fn set_hf_token(State(s): State<St>, Json(r): Json<SetToken>) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().set_hf_token(&r.token, r.by.as_deref())).await
}

async fn clear_hf_token(State(s): State<St>, Json(r): Json<ByBody>) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().clear_hf_token(r.by.as_deref())).await
}

/// The version check in web mode (ADR-0035): this server asks openbark.io,
/// never the browser, whose policy stays "this computer only". The page asks
/// for the automatic check only while its daily check is on; `manual` is
/// Check for updates. Once a day per computer, however many tabs are open.
#[derive(Deserialize)]
struct UpdateCheck {
    #[serde(default)]
    manual: bool,
}

async fn check_for_update(
    State(s): State<St>,
    Json(r): Json<UpdateCheck>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().check_for_update(r.manual)).await
}

#[derive(Deserialize)]
struct Alternatives {
    base: String,
}

async fn hub_alternatives(
    State(s): State<St>,
    Query(q): Query<Alternatives>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().hub_alternatives(&q.base)).await
}

async fn list_patients(State(s): State<St>) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().list_patients()).await
}

async fn patient(State(s): State<St>, UrlPath(id): UrlPath<String>) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().patient(&id)).await
}

#[derive(Deserialize)]
struct PatientDetails {
    name: String,
    #[serde(default)]
    last_name: String,
    #[serde(default)]
    species: String,
    /// The owner's phone number (ADR-0023); empty clears it, absent keeps it.
    #[serde(default)]
    owner_phone: Option<String>,
}

async fn create_patient(
    State(s): State<St>,
    Json(p): Json<PatientDetails>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().create_patient(&p.name, &p.last_name, &p.species)).await
}

async fn update_patient_record(
    State(s): State<St>,
    UrlPath(id): UrlPath<String>,
    Json(p): Json<PatientDetails>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || {
        let record = s.app().update_patient_record(&id, &p.name, &p.last_name, &p.species)?;
        match p.owner_phone {
            Some(phone) => s.app().set_owner_phone(&id, &phone),
            None => Ok(record),
        }
    })
    .await
}

async fn list_sessions(State(s): State<St>) -> impl IntoResponse {
    Json(s.app().list_sessions())
}

#[derive(Deserialize, Default)]
struct PetQuery {
    #[serde(default)]
    pet: Option<String>,
}

async fn open_session(
    State(s): State<St>,
    UrlPath(id): UrlPath<String>,
    Query(q): Query<PetQuery>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().open_session(&id, q.pet.as_deref())).await
}

/// The recording, decrypted in memory (never as a plain file), in byte
/// ranges so the player can seek; only the chunks asked for are decrypted
/// (os_app::App::session_audio_part).
async fn session_audio(
    State(s): State<St>,
    UrlPath(id): UrlPath<String>,
    method: axum::http::Method,
    headers: axum::http::HeaderMap,
) -> Result<Response, ApiError> {
    let range = headers.get(axum::http::header::RANGE).and_then(|v| v.to_str().ok()).map(str::to_owned);
    let head = method == axum::http::Method::HEAD;
    let part = tokio::task::spawn_blocking(move || s.app().session_audio_part(&id, range.as_deref(), head))
        .await
        .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))??;
    let mut response = Response::builder().status(part.status);
    for (name, value) in part.headers() {
        response = response.header(name, value);
    }
    response
        .body(axum::body::Body::from(part.body))
        .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

async fn list_users(State(s): State<St>) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().list_users()).await
}

#[derive(Deserialize)]
struct NewUser {
    name: String,
    role: Role,
    pin: String,
    #[serde(default)]
    by: Option<String>,
}

async fn add_user(State(s): State<St>, Json(u): Json<NewUser>) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().add_user(&u.name, u.role, &u.pin, u.by.as_deref())).await
}

#[derive(Deserialize)]
struct ByQuery {
    #[serde(default)]
    by: Option<String>,
}

async fn remove_user(
    State(s): State<St>,
    UrlPath(uid): UrlPath<String>,
    Query(q): Query<ByQuery>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().remove_user(&uid, q.by.as_deref())).await
}

#[derive(Deserialize)]
struct PinChange {
    pin: String,
    #[serde(default)]
    by: Option<String>,
}

async fn set_user_pin(
    State(s): State<St>,
    UrlPath(uid): UrlPath<String>,
    Json(p): Json<PinChange>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().set_user_pin(&uid, &p.pin, p.by.as_deref())).await
}

#[derive(Deserialize)]
struct PinCheck {
    pin: String,
}

async fn verify_user(
    State(s): State<St>,
    UrlPath(uid): UrlPath<String>,
    Json(p): Json<PinCheck>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().verify_user(&uid, &p.pin)).await
}

async fn encrypt_existing(State(s): State<St>) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().encrypt_existing()).await
}

async fn recovery_key(State(s): State<St>) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().recovery_key()).await
}

/// Patient details passed as query parameters (bodies carry audio).
#[derive(Deserialize, Default)]
struct PatientQuery {
    #[serde(default)]
    patient_name: String,
    #[serde(default)]
    last_name: String,
    #[serde(default)]
    reason: String,
    /// Other animals in the visit, one per line.
    #[serde(default)]
    others: String,
    /// The main animal's species ("Cat").
    #[serde(default)]
    species: String,
    /// The main animal's patient record, when chosen (ADR-0011).
    #[serde(default)]
    patient_id: String,
}

/// Consent passed as query parameters alongside the patient details.
#[derive(Deserialize, Default)]
struct ConsentQuery {
    #[serde(default)]
    consent_rule: String,
    #[serde(default)]
    consent_method: String,
    #[serde(default)]
    consent_by: String,
    #[serde(default)]
    consent_script: String,
}

impl ConsentQuery {
    fn into_consent(self) -> Option<RecordingConsent> {
        Some(RecordingConsent {
            rule: ConsentRule::parse(&self.consent_rule)?,
            method: ConsentMethod::parse(&self.consent_method)?,
            confirmed_by: self.consent_by,
            confirmed_at: None,
            script: Some(self.consent_script).filter(|s| !s.trim().is_empty()),
            outcome: None,
            announced_at_secs: None,
        })
    }
}

impl PatientQuery {
    fn into_patient(self) -> Option<Patient> {
        let p = Patient {
            name: self.patient_name,
            others: self.others.lines().map(str::trim).filter(|o| !o.is_empty()).map(String::from).collect(),
            last_name: self.last_name,
            reason: self.reason,
            species: self.species,
            patient_id: Some(self.patient_id).filter(|id| !id.trim().is_empty()),
            ..Default::default()
        };
        (!p.is_empty()).then_some(p)
    }
}

#[derive(Deserialize)]
struct CreateSession {
    #[serde(default)]
    diarize: bool,
    /// A client on a phone on speaker in the room.
    #[serde(default)]
    speakerphone: bool,
    /// Speakers in the room (ADR-0030); absent: estimated.
    #[serde(default)]
    room_speakers: Option<u32>,
    #[serde(default)]
    name: Option<String>,
    #[serde(flatten)]
    patient: PatientQuery,
    #[serde(flatten)]
    consent: ConsentQuery,
    /// The team member (or name) importing it.
    #[serde(default)]
    by: Option<String>,
}

async fn create_session(
    State(s): State<St>,
    Query(q): Query<CreateSession>,
    body: Bytes,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || {
        let events = s.events.clone();
        let name = q.name.clone().unwrap_or_else(|| "recording.wav".into());
        s.app().import_bytes(
            &name,
            &body,
            q.diarize,
            q.speakerphone,
            q.room_speakers,
            q.patient.into_patient(),
            q.consent.into_consent(),
            q.by.as_deref(),
            events.as_ref(),
        )
    })
    .await
}

#[derive(Deserialize)]
struct CreateNote {
    llm: String,
    #[serde(default = "soap")]
    template: String,
    #[serde(default)]
    language: Option<String>,
    #[serde(default)]
    pet: Option<String>,
    #[serde(default)]
    by: Option<String>,
}

fn soap() -> String {
    "soap".into()
}

async fn create_note(
    State(s): State<St>,
    UrlPath(id): UrlPath<String>,
    Json(req): Json<CreateNote>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || {
        let events = s.events.clone();
        s.app().generate_note(
            &id,
            &req.llm,
            &req.template,
            req.language.as_deref(),
            req.pet.as_deref(),
            req.by.as_deref(),
            events.as_ref(),
        )
    })
    .await
}

#[derive(Deserialize)]
struct SpeedCheckRequest {
    llm: String,
}

async fn speed_check(
    State(s): State<St>,
    Json(req): Json<SpeedCheckRequest>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || {
        let events = s.events.clone();
        s.app().speed_check(&req.llm, events.as_ref())
    })
    .await
}

#[derive(Deserialize)]
struct DiagnosticsQuery {
    #[serde(default)]
    note_model: Option<String>,
}

async fn diagnostics(
    State(s): State<St>,
    Query(q): Query<DiagnosticsQuery>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || Ok(s.app().diagnostics(q.note_model.as_deref()))).await
}

#[derive(Deserialize)]
struct Sign {
    by: String,
    #[serde(default)]
    pet: Option<String>,
}

async fn sign_note(
    State(s): State<St>,
    UrlPath(id): UrlPath<String>,
    Json(req): Json<Sign>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().sign(&id, &req.by, req.pet.as_deref())).await
}

#[derive(Deserialize)]
struct ConfirmChart {
    teeth: Vec<String>,
    #[serde(default = "yes")]
    confirmed: bool,
    by: String,
    #[serde(default)]
    pet: Option<String>,
}

async fn confirm_chart(
    State(s): State<St>,
    UrlPath(id): UrlPath<String>,
    Json(req): Json<ConfirmChart>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().confirm_chart(&id, &req.teeth, req.confirmed, &req.by, req.pet.as_deref())).await
}

#[derive(Deserialize)]
struct SettleDefaults {
    heading: String,
    #[serde(default)]
    labels: Vec<String>,
    confirm: bool,
    #[serde(default)]
    text: Option<String>,
    by: String,
    #[serde(default)]
    pet: Option<String>,
}

/// Confirm or remove normal findings from the template (ADR-0032).
async fn settle_defaults(
    State(s): State<St>,
    UrlPath(id): UrlPath<String>,
    Json(req): Json<SettleDefaults>,
) -> Result<impl IntoResponse, ApiError> {
    let request = os_engine::defaults::SettleRequest {
        heading: req.heading,
        labels: req.labels,
        confirm: req.confirm,
        text: req.text,
    };
    blocking(move || s.app().settle_defaults(&id, &request, &req.by, req.pet.as_deref())).await
}

async fn update_patient(
    State(s): State<St>,
    UrlPath(id): UrlPath<String>,
    Json(p): Json<Patient>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().update_patient(&id, p)).await
}

async fn refine_note(
    State(s): State<St>,
    UrlPath(id): UrlPath<String>,
    Json(req): Json<RefineRequest>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || {
        let events = s.events.clone();
        s.app().refine_note(&id, &req, events.as_ref())
    })
    .await
}

#[derive(Deserialize)]
struct EditNote {
    heading: String,
    value: serde_json::Value,
    by: String,
    #[serde(default)]
    pet: Option<String>,
    /// The tooth chart's animal, chosen in the chart editor (ADR-0021).
    #[serde(default)]
    dentition: Option<String>,
}

async fn edit_note(
    State(s): State<St>,
    UrlPath(id): UrlPath<String>,
    Json(req): Json<EditNote>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || {
        s.app().edit_note(&id, &req.heading, req.value, &req.by, req.pet.as_deref(), req.dentition.as_deref())
    })
    .await
}

/// The note as a PDF download.
async fn export_pdf(
    State(s): State<St>,
    UrlPath(id): UrlPath<String>,
    Json(req): Json<PdfRequest>,
) -> Result<Response, ApiError> {
    let (pdf, name) = tokio::task::spawn_blocking(move || s.app().export_pdf(&id, &req))
        .await
        .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))??;
    Ok((
        [
            (axum::http::header::CONTENT_TYPE, "application/pdf".to_string()),
            (axum::http::header::CONTENT_DISPOSITION, format!("attachment; filename=\"{name}\"")),
        ],
        pdf,
    )
        .into_response())
}

#[derive(Deserialize)]
struct NewAddendum {
    text: String,
    by: String,
    #[serde(default)]
    pet: Option<String>,
}

async fn add_addendum(
    State(s): State<St>,
    UrlPath(id): UrlPath<String>,
    Json(req): Json<NewAddendum>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().add_addendum(&id, &req.text, &req.by, req.pet.as_deref())).await
}

async fn list_templates(
    State(s): State<St>,
    Query(q): Query<ByQuery>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().list_templates(q.by.as_deref())).await
}

async fn template(
    State(s): State<St>,
    UrlPath(id): UrlPath<String>,
    Query(q): Query<ByQuery>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().template(&id, q.by.as_deref())).await
}

#[derive(Deserialize)]
struct PreviewBody {
    template: Template,
    #[serde(default)]
    by: Option<String>,
}

async fn preview_template(
    State(s): State<St>,
    Json(req): Json<PreviewBody>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().preview_template(req.template, req.by.as_deref())).await
}

/// `{template, by, ...}`; the bare template the app sent before ADR-0024 is
/// refused in plain words.
async fn save_template(
    State(s): State<St>,
    Json(body): Json<serde_json::Value>,
) -> Result<impl IntoResponse, ApiError> {
    if body.get("template").is_none() {
        return Err(ApiError(StatusCode::BAD_REQUEST, "Send {template, by}.".into()));
    }
    let req: SaveTemplateRequest =
        serde_json::from_value(body).map_err(|e| ApiError(StatusCode::BAD_REQUEST, e.to_string()))?;
    blocking(move || s.app().save_template(req)).await
}

/// `by` and, for the hospital's templates, an admin's approval. No `Debug`:
/// it carries a PIN.
#[derive(Deserialize)]
struct ApprovedBody {
    #[serde(default)]
    by: Option<String>,
    #[serde(default)]
    approver: Option<Approver>,
}

async fn delete_template(
    State(s): State<St>,
    UrlPath(id): UrlPath<String>,
    Query(q): Query<ByQuery>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().delete_template(&id, q.by.as_deref(), None).map(|()| serde_json::json!({})))
        .await
}

async fn delete_template_approved(
    State(s): State<St>,
    UrlPath(id): UrlPath<String>,
    Json(req): Json<ApprovedBody>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || {
        s.app().delete_template(&id, req.by.as_deref(), req.approver.as_ref()).map(|()| serde_json::json!({}))
    })
    .await
}

#[derive(Deserialize)]
struct CompareQuery {
    #[serde(default)]
    by: Option<String>,
    #[serde(default = "original")]
    with: String,
}

fn original() -> String {
    "original".into()
}

async fn compare_template(
    State(s): State<St>,
    UrlPath(id): UrlPath<String>,
    Query(q): Query<CompareQuery>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().compare_template(&id, q.by.as_deref(), &q.with)).await
}

async fn reset_template(
    State(s): State<St>,
    UrlPath(id): UrlPath<String>,
    Json(req): Json<ByBody>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().reset_template(&id, req.by.as_deref())).await
}

#[derive(Deserialize)]
struct UndoBody {
    #[serde(default)]
    by: Option<String>,
    undo: String,
}

async fn undo_reset_template(
    State(s): State<St>,
    UrlPath(id): UrlPath<String>,
    Json(req): Json<UndoBody>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().undo_reset_template(&id, req.by.as_deref(), &req.undo)).await
}

#[derive(Deserialize)]
struct KeepBody {
    #[serde(default)]
    by: Option<String>,
    fingerprint: String,
    #[serde(default)]
    approver: Option<Approver>,
}

async fn keep_template(
    State(s): State<St>,
    UrlPath(id): UrlPath<String>,
    Json(req): Json<KeepBody>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().keep_template(&id, req.by.as_deref(), &req.fingerprint, req.approver.as_ref()))
        .await
}

#[derive(Deserialize)]
struct ShareBody {
    #[serde(default)]
    by: Option<String>,
    mode: ShareMode,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    approver: Option<Approver>,
}

async fn share_template(
    State(s): State<St>,
    UrlPath(id): UrlPath<String>,
    Json(req): Json<ShareBody>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || {
        s.app().share_template(&id, req.by.as_deref(), req.mode, req.name.as_deref(), req.approver.as_ref())
    })
    .await
}

async fn reset_team_template(
    State(s): State<St>,
    UrlPath(id): UrlPath<String>,
    Json(req): Json<ApprovedBody>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().reset_team_template(&id, req.by.as_deref(), req.approver.as_ref())).await
}

async fn run_template_view(
    State(s): State<St>,
    UrlPath(id): UrlPath<String>,
    Query(q): Query<PetQuery>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().run_template_view(&id, q.pet.as_deref())).await
}

async fn team_settings(State(s): State<St>) -> Result<impl IntoResponse, ApiError> {
    blocking(move || Ok(s.app().team_settings())).await
}

#[derive(Deserialize)]
struct TeamSettingsBody {
    #[serde(default)]
    by: Option<String>,
    templates: TemplateMode,
    #[serde(default)]
    approver: Option<Approver>,
}

async fn set_team_settings(
    State(s): State<St>,
    Json(req): Json<TeamSettingsBody>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().set_template_mode(req.templates, req.by.as_deref(), req.approver.as_ref())).await
}

async fn dismiss_setup_notice(
    State(s): State<St>,
    Json(req): Json<ByBody>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().dismiss_setup_notice(req.by.as_deref())).await
}

/// The clinic's words and drug names (ADR-0031).
async fn vocabulary(State(s): State<St>) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().vocabulary()).await
}

#[derive(Deserialize)]
struct VocabularySaveBody {
    term: ClinicTerm,
    #[serde(default)]
    by: Option<String>,
    #[serde(default)]
    approver: Option<Approver>,
}

async fn save_vocabulary_term(
    State(s): State<St>,
    Json(req): Json<VocabularySaveBody>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().save_vocabulary_term(&req.term, req.by.as_deref(), req.approver.as_ref())).await
}

#[derive(Deserialize)]
struct VocabularyRemoveBody {
    term: String,
    #[serde(default)]
    by: Option<String>,
    #[serde(default)]
    approver: Option<Approver>,
}

async fn remove_vocabulary_term(
    State(s): State<St>,
    Json(req): Json<VocabularyRemoveBody>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().remove_vocabulary_term(&req.term, req.by.as_deref(), req.approver.as_ref()))
        .await
}

#[derive(Deserialize)]
struct SpeechLanguageBody {
    language: String,
}

/// The app's language, which the second recogniser is told to expect.
async fn set_speech_language(Json(req): Json<SpeechLanguageBody>) -> StatusCode {
    os_engine::models::set_speech_language(Some(&req.language));
    StatusCode::NO_CONTENT
}

#[derive(Deserialize)]
struct SpokenLanguageBody {
    #[serde(default)]
    language: Option<String>,
}

/// Settings, Recording, Spoken language ("fr"); null is Auto (ADR-0033).
async fn set_spoken_language(Json(req): Json<SpokenLanguageBody>) -> StatusCode {
    os_engine::models::set_spoken_language(req.language.as_deref());
    StatusCode::NO_CONTENT
}

#[derive(Deserialize)]
struct AdminBody {
    #[serde(default)]
    by: Option<String>,
    admin: bool,
    #[serde(default)]
    approver: Option<Approver>,
}

async fn set_user_admin(
    State(s): State<St>,
    UrlPath(uid): UrlPath<String>,
    Json(req): Json<AdminBody>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().set_user_admin(&uid, req.admin, req.by.as_deref(), req.approver.as_ref())).await
}

async fn profile_summary(
    State(s): State<St>,
    UrlPath(uid): UrlPath<String>,
    Query(q): Query<ByQuery>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().profile_summary(&uid, q.by.as_deref())).await
}

#[derive(Deserialize)]
struct FromExample {
    llm: String,
    text: String,
}

async fn template_from_example(
    State(s): State<St>,
    Json(req): Json<FromExample>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().template_from_example(&req.llm, &req.text)).await
}

/// No `Debug`: it may carry an admin's PIN.
#[derive(Deserialize)]
struct NewStandard {
    #[serde(default)]
    heading: String,
    text: String,
    #[serde(default)]
    by: Option<String>,
    #[serde(default)]
    scope: Option<Scope>,
    #[serde(default)]
    approver: Option<Approver>,
}

async fn add_standard(
    State(s): State<St>,
    UrlPath(id): UrlPath<String>,
    Json(req): Json<NewStandard>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || {
        s.app().add_standard(
            &id,
            &req.heading,
            &req.text,
            req.by.as_deref(),
            req.scope,
            req.approver.as_ref(),
        )
    })
    .await
}

async fn remove_standard(
    State(s): State<St>,
    UrlPath((id, sid)): UrlPath<(String, String)>,
    Query(q): Query<ByQuery>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().remove_standard(&id, &sid, q.by.as_deref(), None)).await
}

async fn remove_standard_approved(
    State(s): State<St>,
    UrlPath((id, sid)): UrlPath<(String, String)>,
    Json(req): Json<ApprovedBody>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().remove_standard(&id, &sid, req.by.as_deref(), req.approver.as_ref())).await
}

#[derive(Deserialize)]
struct MuteBody {
    #[serde(default)]
    by: Option<String>,
    muted: bool,
}

async fn mute_standard(
    State(s): State<St>,
    UrlPath((id, sid)): UrlPath<(String, String)>,
    Json(req): Json<MuteBody>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().mute_standard(&id, &sid, req.by.as_deref(), req.muted)).await
}

#[derive(Deserialize)]
struct LiveQuery {
    #[serde(default = "yes")]
    speakers: bool,
    /// A client on a phone on speaker in the room.
    #[serde(default)]
    speakerphone: bool,
    /// Speakers in the room (ADR-0030); absent: estimated.
    #[serde(default)]
    room_speakers: Option<u32>,
    #[serde(default = "rate")]
    sample_rate: u32,
    #[serde(flatten)]
    patient: PatientQuery,
    #[serde(flatten)]
    consent: ConsentQuery,
    #[serde(default)]
    by: Option<String>,
}

fn yes() -> bool {
    true
}
fn rate() -> u32 {
    16_000
}

/// Browser microphone streaming: binary messages are little-endian f32 PCM
/// (mono, `sample_rate` Hz). Text "stop" ends the recording; the server replies
/// with the saved encounter as JSON text, then closes. Text
/// `{"sample_rate": 44100}` changes the rate of the blocks that follow (the
/// browser switched to a microphone running at another rate).
async fn live(State(s): State<St>, Query(q): Query<LiveQuery>, ws: WebSocketUpgrade) -> Response {
    ws.on_upgrade(move |socket| live_session(s, q, socket))
}

/// A rate the recorder can resample from.
fn usable_rate(rate: u32) -> bool {
    (os_app::MIN_SAMPLE_RATE..=os_app::MAX_SAMPLE_RATE).contains(&rate)
}

/// A text message's new sample rate, if it is one.
fn rate_change(text: &str) -> Option<u32> {
    #[derive(Deserialize)]
    struct RateChange {
        sample_rate: u32,
    }
    serde_json::from_str::<RateChange>(text).ok().map(|r| r.sample_rate)
}

#[derive(Deserialize)]
struct Prewarm {
    #[serde(default = "yes")]
    speakers: bool,
    #[serde(default)]
    speakerphone: bool,
}

/// The Record screen opened: load the speech models now, if memory allows.
async fn record_prewarm(State(s): State<St>, Json(req): Json<Prewarm>) -> impl IntoResponse {
    let ready = tokio::task::spawn_blocking(move || s.app().prewarm_speech(req.speakers, req.speakerphone))
        .await
        .unwrap_or(false);
    Json(serde_json::json!({ "loading": ready }))
}

/// The Record screen closed: give the memory back.
async fn record_release(State(s): State<St>) -> impl IntoResponse {
    let _ = tokio::task::spawn_blocking(move || s.app().release_speech()).await;
    Json(serde_json::json!({}))
}

async fn live_session(s: St, mut q: LiveQuery, mut socket: WebSocket) {
    if !usable_rate(q.sample_rate) {
        let error = "This browser reported a microphone rate OpenBark can't use. Reload the page and try again, or use another browser.";
        let _ = socket.send(Message::Text(serde_json::json!({ "error": error }).to_string().into())).await;
        return;
    }
    let patient = std::mem::take(&mut q.patient).into_patient();
    let consent = std::mem::take(&mut q.consent).into_consent();
    let (tx, rx) = std::sync::mpsc::channel::<AudioBlock>();
    let events: Arc<dyn Events> = s.events.clone();
    let started = {
        let s = s.clone();
        tokio::task::spawn_blocking(move || {
            s.app().start_recording(
                RecordingSource::Stream(rx),
                q.speakers,
                q.speakerphone,
                q.room_speakers,
                patient,
                consent,
                q.by.as_deref(),
                events,
            )
        })
        .await
    };
    match started {
        Ok(Ok(id)) => {
            let _ = socket.send(Message::Text(serde_json::json!({ "started": id }).to_string().into())).await;
        }
        Ok(Err(e)) => {
            let _ = socket.send(Message::Text(serde_json::json!({ "error": e.0 }).to_string().into())).await;
            return;
        }
        Err(e) => {
            let _ = socket
                .send(Message::Text(serde_json::json!({ "error": e.to_string() }).to_string().into()))
                .await;
            return;
        }
    }
    while let Some(Ok(msg)) = socket.recv().await {
        match msg {
            Message::Binary(bytes) => {
                let samples: Vec<f32> = bytes
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                    .collect();
                if tx.send(AudioBlock { samples, sample_rate: q.sample_rate }).is_err() {
                    break;
                }
            }
            Message::Text(t) if t.as_str() == "stop" => break,
            Message::Text(t) => match rate_change(t.as_str()) {
                Some(rate) if usable_rate(rate) => {
                    tracing::info!(rate, "browser microphone rate changed");
                    q.sample_rate = rate;
                }
                Some(rate) => tracing::warn!(rate, "ignored an unusable browser microphone rate"),
                None => {}
            },
            Message::Close(_) => break,
            _ => {}
        }
    }
    drop(tx); // ends the stream source
    let result = tokio::task::spawn_blocking(move || s.app().stop_recording()).await;
    let reply = match result {
        Ok(Ok(view)) => serde_json::json!({ "session": view }),
        Ok(Err(e)) => serde_json::json!({ "error": e.0 }),
        Err(e) => serde_json::json!({ "error": e.to_string() }),
    };
    let _ = socket.send(Message::Text(reply.to_string().into())).await;
    let _ = socket.send(Message::Close(None)).await;
}

#[derive(Deserialize)]
struct RecordStart {
    #[serde(default)]
    device: Option<String>,
    #[serde(default = "yes")]
    speakers: bool,
    /// A client on a phone on speaker in the room.
    #[serde(default)]
    speakerphone: bool,
    /// Speakers in the room (ADR-0030); absent: estimated.
    #[serde(default)]
    room_speakers: Option<u32>,
    #[serde(default)]
    patient: Option<Patient>,
    #[serde(default)]
    consent: Option<RecordingConsent>,
    #[serde(default)]
    by: Option<String>,
}

async fn record_start(
    State(s): State<St>,
    Json(req): Json<RecordStart>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || {
        let events: Arc<dyn Events> = s.events.clone();
        s.app().start_recording(
            RecordingSource::Microphone(req.device),
            req.speakers,
            req.speakerphone,
            req.room_speakers,
            req.patient,
            req.consent,
            req.by.as_deref(),
            events,
        )
    })
    .await
}

async fn record_stop(State(s): State<St>) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().stop_recording()).await
}

async fn download_model(
    State(s): State<St>,
    UrlPath(id): UrlPath<String>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || {
        let events = s.events.clone();
        s.app().download_model(&id, None, events.as_ref())
    })
    .await
}

#[derive(Deserialize)]
struct Search {
    #[serde(default)]
    q: String,
}

async fn hub_search(State(s): State<St>, Query(q): Query<Search>) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().hub_search(&q.q)).await
}

#[derive(Deserialize)]
struct RepoQuery {
    id: String,
}

async fn hub_repo(State(s): State<St>, Query(q): Query<RepoQuery>) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().hub_repo(&q.id)).await
}

#[derive(Deserialize)]
struct HubAdd {
    repo: String,
    file: String,
    #[serde(default)]
    accept_license: bool,
}

async fn hub_add(State(s): State<St>, Json(req): Json<HubAdd>) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app().hub_add(&req.repo, &req.file, req.accept_license)).await
}

async fn events_stream(
    State(s): State<St>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, std::convert::Infallible>>> {
    let stream = BroadcastStream::new(s.events.0.subscribe())
        .filter_map(|e| e.ok().map(|ev| Ok(Event::default().json_data(&ev).unwrap_or_default())));
    Sse::new(stream).keep_alive(KeepAlive::default())
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    use super::*;

    /// Only this computer's own page gets through (review 2026-10-18).
    #[tokio::test]
    async fn other_pages_are_kept_out() {
        let app = Router::new()
            .route("/api/thing", get(|| async { "ok" }).post(|| async { "ok" }))
            .layer(axum::middleware::from_fn_with_state(4317u16, local_only));
        let send = |method: &str, host: &str, origin: Option<&str>, header: bool| {
            let mut r = Request::builder().method(method).uri("/api/thing").header("host", host);
            if let Some(o) = origin {
                r = r.header("origin", o);
            }
            if header {
                r = r.header("x-openbark", "1");
            }
            let app = app.clone();
            async move { app.oneshot(r.body(Body::empty()).unwrap()).await.unwrap().status() }
        };
        assert_eq!(send("GET", "127.0.0.1:4317", None, false).await, StatusCode::OK);
        assert_eq!(send("GET", "localhost:4317", Some("http://localhost:4317"), false).await, StatusCode::OK);
        assert_eq!(send("POST", "127.0.0.1:4317", Some("http://127.0.0.1:4317"), true).await, StatusCode::OK);
        // DNS rebinding: the attacker's name as Host.
        assert_eq!(send("GET", "evil.example:4317", None, false).await, StatusCode::FORBIDDEN);
        // Another site's page.
        assert_eq!(
            send("GET", "127.0.0.1:4317", Some("https://evil.example"), false).await,
            StatusCode::FORBIDDEN
        );
        // A simple cross-site POST can't add the header.
        assert_eq!(send("POST", "127.0.0.1:4317", None, false).await, StatusCode::FORBIDDEN);
    }

    // ---- deleting (ADR-0022), on the real router and a temporary App ----------

    use std::sync::OnceLock;

    use os_engine::faithfulness::FaithfulnessReport;
    use os_engine::session::{PatientInfo, Session};
    use os_engine::transcript::Transcript;
    use os_engine::users::UserStore;

    const PORT: u16 = 4399;

    /// A throwaway key and temporary team, template and folder files, set
    /// once for every test in this crate (never this computer's own).
    fn test_env() -> &'static std::path::Path {
        static DIR: OnceLock<PathBuf> = OnceLock::new();
        DIR.get_or_init(|| {
            let dir = std::env::temp_dir().join(format!("openbark-os-server-tests-{}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            // SAFETY: runs once, before any App exists; every test calls test_env() first.
            unsafe {
                std::env::set_var("OPENBARK_DATA_KEY", "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=");
                std::env::set_var("OPENBARK_LOCATIONS_FILE", dir.join("locations.json"));
                std::env::set_var("OPENBARK_USERS_FILE", dir.join("users.json"));
                std::env::set_var("OPENBARK_TEMPLATES_DIR", dir.join("templates"));
                std::env::set_var("OPENBARK_KEY_FILE", dir.join("data.key"));
                std::env::set_var("OPENBARK_ENGINES_DIR", dir.join("engines"));
                // Twilio settings and secrets (ADR-0023): a scratch folder, no keychain.
                std::env::set_var("OPENBARK_CONNECTIONS_DIR", dir.join("connections"));
            }
            dir
        })
    }

    struct Server {
        router: Router,
        app: Arc<App>,
        _tmp: tempfile::TempDir,
    }

    fn server() -> Server {
        test_env();
        let tmp = tempfile::tempdir().unwrap();
        let mut app = App::new(ModelStore::new(tmp.path().join("models")), tmp.path().join("sessions"));
        app.users = UserStore::new(tmp.path().join("users.json"));
        let app = Arc::new(app);
        let (tx, _) = broadcast::channel(16);
        let state: St = Arc::new(Shared { app: RwLock::new(app.clone()), events: Arc::new(Broadcaster(tx)) });
        Server { router: router(state, &tmp.path().join("ui"), PORT), app, _tmp: tmp }
    }

    /// A visit with a transcript and a draft, signed when asked.
    fn visit(app: &App, name: &str, sign: bool) -> Session {
        let mut s = Session::create_live(&app.sessions_root, name).unwrap();
        s.set_patient(PatientInfo { name: name.into(), last_name: "Okonkwo".into(), ..Default::default() })
            .unwrap();
        s.save_transcript(&Transcript { segments: Vec::new() }, "parakeet", false, &Default::default())
            .unwrap();
        let result = os_engine::NoteResult {
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
        };
        s.save_run("gemma", &result, &os_engine::LoadedTemplate::builtin("soap").unwrap()).unwrap();
        if sign {
            s.sign_latest("Dr Rivera").unwrap();
        }
        s
    }

    fn id(s: &Session) -> String {
        s.dir.file_name().unwrap().to_string_lossy().into_owned()
    }

    async fn call(
        router: &Router,
        method: &str,
        uri: &str,
        body: Option<serde_json::Value>,
        header: bool,
    ) -> (StatusCode, serde_json::Value) {
        let mut r = Request::builder().method(method).uri(uri).header("host", format!("127.0.0.1:{PORT}"));
        if header {
            r = r.header("x-openbark", "1");
        }
        let body = match body {
            Some(b) => {
                r = r.header("content-type", "application/json");
                Body::from(b.to_string())
            }
            None => Body::empty(),
        };
        let res = router.clone().oneshot(r.body(body).unwrap()).await.unwrap();
        let status = res.status();
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
        (status, serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null))
    }

    /// Phone calls (ADR-0023): the secret goes in and never comes back, a
    /// token goes only to a request with the `X-OpenBark` header, and only the
    /// phone page may reach Twilio.
    #[tokio::test]
    async fn the_phone_routes_keep_the_secret_in_and_twilio_to_the_phone_page() {
        use os_connect::mock::fake::{FakeTwilio, sid};
        test_env();
        let tmp = tempfile::tempdir().unwrap();
        let ui = tmp.path().join("ui");
        std::fs::create_dir_all(&ui).unwrap();
        std::fs::write(ui.join("index.html"), "<!doctype html><title>main</title>").unwrap();
        std::fs::write(ui.join("phone.html"), "<!doctype html><title>phone</title>").unwrap();
        let fake_key = concat!("fake-", "value-for-", "server-tests-77");
        let fake = FakeTwilio::start(&sid("AC", 1), &sid("SK", 2), fake_key, &["+15555550100"]);
        let mut app = App::new(ModelStore::new(tmp.path().join("models")), tmp.path().join("sessions"));
        app.users = UserStore::new(tmp.path().join("users.json"));
        app.connect = os_app::ConnectStore::with_keychain(
            tmp.path().join("connections"),
            Box::new(os_connect::NoKeychain),
        );
        app.twilio_base = Some(fake.server.base.clone());
        let app = Arc::new(app);
        let (tx, _) = broadcast::channel(16);
        let state: St = Arc::new(Shared { app: RwLock::new(app.clone()), events: Arc::new(Broadcaster(tx)) });
        let router = router(state, &ui, PORT);

        let connect = serde_json::json!({
            "account_sid": sid("AC", 1), "api_key_sid": sid("SK", 2), "api_key_secret": fake_key,
            "computer_name": "front-desk", "caller_id": "+15555550100",
            "outgoing_bin_url": format!("https://handler.twilio.com/twiml/{}", sid("EH", 3)),
        });
        let (status, _) = call(&router, "POST", "/api/phone/connect", Some(connect.clone()), false).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "another page can't connect an account");
        let (status, body) = call(&router, "POST", "/api/phone/connect", Some(connect), true).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert!(!body.to_string().contains(fake_key), "the secret is never sent back");
        assert_eq!(body["phone"]["secret"]["hint"], "****s-77");
        let (_, view) = call(&router, "GET", "/api/phone", None, false).await;
        assert!(!view.to_string().contains(fake_key));
        assert_eq!(view["connected"], true);

        let (status, body) = call(&router, "POST", "/api/phone/token", None, true).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "the phone is still off");
        assert!(body["error"].as_str().unwrap().contains("The phone is off"));
        let on = serde_json::json!({ "enabled": true, "record_calls": true, "announcement": "This call is recorded. Is that all right?" });
        let (status, _) = call(&router, "POST", "/api/phone/prefs", Some(on), true).await;
        assert_eq!(status, StatusCode::OK);
        let (status, _) = call(&router, "POST", "/api/phone/token", None, false).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "no token without the X-OpenBark header");
        let (status, body) = call(&router, "POST", "/api/phone/token", None, true).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["identity"], "openbark_front_desk");
        assert!(!body.to_string().contains(fake_key));
        let (_, bin) =
            call(&router, "GET", "/api/phone/bin?caller_id=%2B1%20555%20555%200100", None, false).await;
        assert!(bin["text"].as_str().unwrap().contains(r#"callerId="+15555550100""#));
        let (_, target) = call(
            &router,
            "POST",
            "/api/phone/prepare",
            Some(serde_json::json!({ "number": "555-555-0142" })),
            true,
        )
        .await;
        assert_eq!(target["number"], "+15555550142");
        let (_, pending) = call(&router, "GET", "/api/phone/pending", None, false).await;
        assert_eq!(pending["number"], "+15555550142");

        // Each page's policy: Twilio only from the phone page.
        let page = |path: &'static str| {
            let router = router.clone();
            async move {
                let r = Request::builder()
                    .uri(path)
                    .header("host", format!("127.0.0.1:{PORT}"))
                    .body(Body::empty());
                let res = router.oneshot(r.unwrap()).await.unwrap();
                res.headers()
                    .get("content-security-policy")
                    .map(|v| v.to_str().unwrap().to_string())
                    .unwrap_or_default()
            }
        };
        let phone = page("/phone.html").await;
        assert!(
            phone.contains("wss://voice-js.roaming.twilio.com")
                && phone.contains("https://eventgw.twilio.com"),
            "{phone}"
        );
        assert!(!phone.contains("sdk.twilio.com") && !phone.contains("twiliocdn"), "sounds are local files");
        assert!(phone.contains("wss://voice-js.ashburn.twilio.com"), "the edge the SDK reconnects to");
        assert!(
            phone.contains(&format!("ws://127.0.0.1:{PORT}")) && !phone.contains(":*"),
            "own port only: {phone}"
        );
        for path in ["/", "/index.html", "/patients"] {
            let main = page(path).await;
            assert!(main.starts_with("default-src 'self'"), "{path}: {main}");
            assert!(
                !main.contains("twilio") && !main.contains("https:") && !main.contains("wss:"),
                "{path}: {main}"
            );
            assert!(!main.contains(":*"), "{path}: only this server's own port: {main}");
        }
        // The pending call's patient phone route.
        let bella = app.create_patient("Bella", "Patel", "canine").unwrap();
        let uri = format!("/api/patients/{}", bella.id);
        let (status, rec) = call(&router, "POST", &uri, Some(serde_json::json!({ "name": "Bella", "last_name": "Patel", "species": "canine", "owner_phone": "(555) 555-0142" })), true).await;
        assert_eq!(status, StatusCode::OK, "{rec}");
        assert_eq!(rec["owner_phone"], "+15555550142");
        let (_, rec) = call(
            &router,
            "POST",
            &uri,
            Some(serde_json::json!({ "name": "Bella", "last_name": "Patel", "species": "canine" })),
            true,
        )
        .await;
        assert_eq!(rec["owner_phone"], "+15555550142", "absent keeps it");
        let (status, _) =
            call(&router, "POST", "/api/phone/disconnect", Some(serde_json::json!({})), true).await;
        assert_eq!(status, StatusCode::OK);
        let (_, view) = call(&router, "GET", "/api/phone", None, false).await;
        assert_eq!(view["connected"], false);
    }

    /// The player's requests for a recording, as WebKit and Chromium send them.
    #[tokio::test]
    async fn recordings_play_in_ranges() {
        let srv = server();
        let visits = os_app::demo::audio_visits(&srv.app.sessions_root).unwrap();
        let v = &visits[0];
        let whole = srv.app.session_audio(&v.id).unwrap();
        let total = whole.len();
        let get = |range: Option<String>, method: &'static str| {
            let mut r = Request::builder()
                .method(method)
                .uri(format!("/api/sessions/{}/audio", v.id))
                .header("host", format!("127.0.0.1:{PORT}"));
            if let Some(range) = range {
                r = r.header("range", range);
            }
            let router = srv.router.clone();
            async move {
                let res = router.oneshot(r.body(Body::empty()).unwrap()).await.unwrap();
                let status = res.status();
                let h = |n: &str| res.headers().get(n).map(|v| v.to_str().unwrap().to_string());
                let heads = (h("content-type"), h("content-range"), h("accept-ranges"), h("cache-control"));
                let body = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
                (status, heads, body)
            }
        };
        let (status, (ty, cr, ar, cc), body) = get(Some("bytes=0-1".into()), "GET").await;
        assert_eq!(status, StatusCode::PARTIAL_CONTENT);
        assert_eq!(ty.as_deref(), Some("audio/wav"));
        assert_eq!(cr, Some(format!("bytes 0-1/{total}")));
        assert_eq!(ar.as_deref(), Some("bytes"));
        assert_eq!(cc.as_deref(), Some("no-store"));
        assert_eq!(&body[..], &whole[..2]);
        let (status, (_, cr, ..), body) = get(Some("bytes=0-".into()), "GET").await;
        assert_eq!(status, StatusCode::PARTIAL_CONTENT);
        let max = os_app::AUDIO_PART_MAX as usize;
        assert_eq!(cr, Some(format!("bytes 0-{}/{total}", max - 1)));
        assert_eq!(&body[..], &whole[..max]);
        let (status, _, body) = get(Some(format!("bytes={}-", total - 10)), "GET").await;
        assert_eq!(status, StatusCode::PARTIAL_CONTENT);
        assert_eq!(&body[..], &whole[total - 10..]);
        let (status, (_, cr, ..), _) = get(Some(format!("bytes={total}-")), "GET").await;
        assert_eq!(status, StatusCode::RANGE_NOT_SATISFIABLE);
        assert_eq!(cr, Some(format!("bytes */{total}")));
        let (status, _, body) = get(None, "GET").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(&body[..], &whole[..]);
        let (status, _, body) = get(None, "HEAD").await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.is_empty());
    }

    #[tokio::test]
    async fn every_delete_route_needs_the_openbark_header() {
        let srv = server();
        let s = visit(&srv.app, "Maple", false);
        let body = serde_json::json!({ "by": "Sam", "reason": { "kind": "mistake" } });
        for uri in [
            format!("/api/sessions/{}/delete", id(&s)),
            format!("/api/sessions/{}/recording/delete", id(&s)),
            "/api/deleted/0199a7f0-1c2d-7e3f-8a9b-0c1d2e3f4a5b/restore".to_string(),
            "/api/deleted/0199a7f0-1c2d-7e3f-8a9b-0c1d2e3f4a5b/purge".to_string(),
        ] {
            let (status, _) = call(&srv.router, "POST", &uri, Some(body.clone()), false).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{uri}");
        }
        assert!(s.dir.join("session.json").is_file(), "nothing was deleted");
    }

    #[tokio::test]
    async fn a_deleted_visit_moves_to_deleted_visits_and_says_so_when_opened() {
        let srv = server();
        let s = visit(&srv.app, "Maple", false);
        let uri = format!("/api/sessions/{}/delete", id(&s));
        let body = serde_json::json!({ "by": "Sam", "reason": { "kind": "test", "note": "practice run" } });
        let (status, deleted) = call(&srv.router, "POST", &uri, Some(body), true).await;
        assert_eq!(status, StatusCode::OK, "{deleted}");
        assert_eq!(deleted["trash_id"], serde_json::json!(s.meta.id));
        let (status, err) = call(&srv.router, "GET", &format!("/api/sessions/{}", id(&s)), None, false).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(err["error"], "This visit was deleted. Restore it from Deleted visits in Settings.");
        let (status, listed) = call(&srv.router, "GET", "/api/deleted?by=Sam", None, false).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(listed[0]["reason"], serde_json::json!({ "kind": "test", "note": "practice run" }));
        let tid = deleted["trash_id"].as_str().unwrap();
        let (status, restored) = call(
            &srv.router,
            "POST",
            &format!("/api/deleted/{tid}/restore"),
            Some(serde_json::json!({ "by": "Sam" })),
            true,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{restored}");
        assert_eq!(restored["id"], serde_json::json!(id(&s)));
        // Signed visits are refused in phase 1.
        let signed = visit(&srv.app, "Olive", true);
        let body = serde_json::json!({ "by": "Sam", "reason": { "kind": "duplicate" } });
        let (status, err) =
            call(&srv.router, "POST", &format!("/api/sessions/{}/delete", id(&signed)), Some(body), true)
                .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(err["error"], "Signed visits can't be deleted yet. Add an addendum to correct the note.");
        // The recording route answers in the same way.
        let (status, err) = call(
            &srv.router,
            "POST",
            &format!("/api/sessions/{}/recording/delete", id(&s)),
            Some(serde_json::json!({ "by": "Sam" })),
            true,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(err["error"], "This visit has no recording.");
    }

    #[tokio::test]
    async fn the_pin_is_read_from_the_body_only() {
        let srv = server();
        let dvm = srv.app.users.add("Dr Rivera", os_engine::users::Role::Dvm, "2468").unwrap();
        let s = visit(&srv.app, "Maple", true);
        // A signed visit in Deleted visits (phase 1 can't put one there itself).
        let trash = os_engine::trash::Trash::for_sessions(&srv.app.sessions_root);
        let now = chrono::Local::now();
        let info = os_engine::trash::DeletedInfo {
            trash_id: trash.new_id(&s.meta.id),
            original_name: id(&s),
            deleted_at: now,
            by: os_engine::users::Who::named("Dr Rivera"),
            reason: os_engine::audit::DeleteReason {
                kind: os_engine::audit::ReasonKind::Duplicate,
                note: String::new(),
            },
            signed: true,
            purge_after: os_engine::trash::purge_after(now),
            event_id: os_engine::audit::new_id(),
            recovered: false,
        };
        trash.put(&s.dir, &info).unwrap();
        let uri = format!("/api/deleted/{}/purge?pin=2468", info.trash_id);
        let (status, err) =
            call(&srv.router, "POST", &uri, Some(serde_json::json!({ "by": dvm.id })), true).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(err["error"], "That PIN isn't right.");
        assert_eq!(trash.list().len(), 1, "still in Deleted visits");
        let uri = format!("/api/deleted/{}/purge", info.trash_id);
        let body = serde_json::json!({ "by": dvm.id, "pin": "2468" });
        let (status, _) = call(&srv.router, "POST", &uri, Some(body), true).await;
        assert_eq!(status, StatusCode::OK);
        assert!(trash.list().is_empty());
    }

    /// Review: no way round the guards through the web API. A GET never
    /// deletes, a request without a reason is refused, and with a team the
    /// role is checked whoever's id is missing or wrong.
    #[tokio::test]
    async fn the_web_api_cant_be_used_to_slip_past_the_rules() {
        let srv = server();
        let s = visit(&srv.app, "Maple", false);
        let uri = format!("/api/sessions/{}/delete", id(&s));
        let still_there = || assert!(s.dir.join("session.json").is_file(), "the visit was deleted");
        for method in ["GET", "HEAD"] {
            let (status, _) = call(&srv.router, method, &uri, None, true).await;
            assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED, "{method}");
        }
        still_there();
        // No reason, or one that isn't on the list.
        for body in [
            serde_json::json!({ "by": "Sam" }),
            serde_json::json!({ "by": "Sam", "reason": { "kind": "because" } }),
            serde_json::json!({ "by": "Sam", "reason": "mistake" }),
        ] {
            let (status, _) = call(&srv.router, "POST", &uri, Some(body.clone()), true).await;
            assert!(status.is_client_error(), "{body}: {status}");
        }
        still_there();
        // Path tricks in the ids.
        for bad in ["..", ".openbark", "%2e%2e", "a%2fb", "a%5cb"] {
            let body = serde_json::json!({ "by": "Sam", "reason": { "kind": "mistake" } });
            let (status, _) =
                call(&srv.router, "POST", &format!("/api/sessions/{bad}/delete"), Some(body), true).await;
            assert!(status.is_client_error(), "{bad}: {status}");
            let (status, _) = call(
                &srv.router,
                "POST",
                &format!("/api/deleted/{bad}/purge"),
                Some(serde_json::json!({ "by": "Sam" })),
                true,
            )
            .await;
            assert!(status.is_client_error(), "{bad}: {status}");
        }
        still_there();
        // With a team: reception, nobody, or a stranger's id are all refused.
        srv.app.users.add("Dr Rivera", os_engine::users::Role::Dvm, "2468").unwrap();
        let desk = srv.app.users.add("Lee", os_engine::users::Role::Reception, "1111").unwrap();
        for by in [serde_json::json!(desk.id), serde_json::Value::Null, serde_json::json!("Sam")] {
            let body = serde_json::json!({ "by": by, "reason": { "kind": "mistake" } });
            let (status, err) = call(&srv.router, "POST", &uri, Some(body), true).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{by}: {err}");
            let (status, _) = call(
                &srv.router,
                "GET",
                &format!("/api/deleted?by={}", by.as_str().unwrap_or("")),
                None,
                false,
            )
            .await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{by} saw Deleted visits");
        }
        still_there();
    }

    // ---- templates (ADR-0024), on the real router ---------------------------------

    /// A server whose App has its own templates folder and team file.
    fn template_server() -> Server {
        test_env();
        let tmp = tempfile::tempdir().unwrap();
        let mut app = App::new(ModelStore::new(tmp.path().join("models")), tmp.path().join("sessions"));
        app.users = UserStore::new(tmp.path().join("users.json"));
        app.templates = os_engine::TemplateStore::new(tmp.path().join("templates"));
        let app = Arc::new(app);
        let (tx, _) = broadcast::channel(16);
        let state: St = Arc::new(Shared { app: RwLock::new(app.clone()), events: Arc::new(Broadcaster(tx)) });
        Server { router: router(state, &tmp.path().join("ui"), PORT), app, _tmp: tmp }
    }

    fn soap_json(plan: &str) -> serde_json::Value {
        let mut t = os_engine::LoadedTemplate::builtin("soap").unwrap().template;
        t.sections.iter_mut().find(|s| s.id == "plan").unwrap().instruction = Some(plan.into());
        serde_json::to_value(t).unwrap()
    }

    #[tokio::test]
    async fn every_template_write_needs_the_openbark_header() {
        let srv = template_server();
        let body = serde_json::json!({ "by": "x" });
        let posts = [
            "/api/templates",
            "/api/templates/preview",
            "/api/templates/soap/delete",
            "/api/templates/soap/reset",
            "/api/templates/soap/reset/undo",
            "/api/templates/soap/keep",
            "/api/templates/soap/share",
            "/api/templates/soap/team/reset",
            "/api/templates/soap/standards",
            "/api/templates/soap/standards/s1/remove",
            "/api/templates/soap/standards/s1/mute",
            "/api/team/settings",
            "/api/team/notice/dismiss",
            "/api/users/u1/admin",
        ];
        for uri in posts {
            let (status, _) = call(&srv.router, "POST", uri, Some(body.clone()), false).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{uri}");
        }
        for uri in ["/api/templates/soap?by=x", "/api/templates/soap/standards/s1?by=x"] {
            let (status, _) = call(&srv.router, "DELETE", uri, None, false).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{uri}");
        }
    }

    #[tokio::test]
    async fn templates_resolve_for_the_person_asking_and_pins_come_from_the_body_only() {
        let srv = template_server();
        let dvm = srv.app.users.add("Dr Rivera", os_engine::users::Role::Dvm, "2468").unwrap();
        let rvt = srv.app.users.add("Sam", os_engine::users::Role::Rvt, "1357").unwrap();
        // The body the app sent before ADR-0024 is refused in plain words.
        let (status, err) = call(&srv.router, "POST", "/api/templates", Some(soap_json("x")), true).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(err["error"], "Send {template, by}.");
        // The technician saves their version.
        let body = serde_json::json!({ "template": soap_json("Tech plan."), "by": rvt.id });
        let (status, saved) = call(&srv.router, "POST", "/api/templates", Some(body), true).await;
        assert_eq!(status, StatusCode::OK, "{saved}");
        assert_eq!(saved["layer"], "personal");
        let layer_for = |list: &serde_json::Value| {
            list.as_array().unwrap().iter().find(|t| t["id"] == "soap").unwrap()["layer"].clone()
        };
        let (_, theirs) =
            call(&srv.router, "GET", &format!("/api/templates?by={}", rvt.id), None, false).await;
        assert_eq!(layer_for(&theirs), "personal");
        let (_, vets) = call(&srv.router, "GET", &format!("/api/templates?by={}", dvm.id), None, false).await;
        assert_eq!(layer_for(&vets), "builtin");
        let (_, view) =
            call(&srv.router, "GET", &format!("/api/templates/soap?by={}", dvm.id), None, false).await;
        assert!(!view["prompt"].as_str().unwrap().contains("Tech plan."));
        // Sharing with the admin's PIN in the URL and none in the body is refused.
        let uri = format!("/api/templates/soap/share?pin=2468&approver.id={}&approver.pin=2468", dvm.id);
        let body = serde_json::json!({ "by": rvt.id, "mode": "replace" });
        let (status, err) = call(&srv.router, "POST", &uri, Some(body), true).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(
            err["error"],
            "Changing the hospital's templates needs an admin. Ask one to approve with their PIN."
        );
        // In the body, it shares.
        let body = serde_json::json!({ "by": rvt.id, "mode": "replace", "approver": { "id": dvm.id, "pin": "2468" } });
        let (status, shared) = call(&srv.router, "POST", "/api/templates/soap/share", Some(body), true).await;
        assert_eq!(status, StatusCode::OK, "{shared}");
        assert_eq!(shared["layer"], "team");
        assert_eq!(shared["origin"]["approved_by"]["name"], "Dr Rivera");
    }

    // ---- models of your choice (ADR-0025), in a temporary models folder ------

    /// A Hub model in the temporary store's user catalog, with its file written.
    fn hub_model(app: &App) -> (String, std::path::PathBuf) {
        let entry: os_models::CatalogEntry = serde_json::from_value(serde_json::json!({
            "id": "hf:someone/tiny-GGUF/tiny-Q4_K_M",
            "display_name": "someone/tiny-GGUF (tiny-Q4_K_M)",
            "slot": "llm",
            "license": "apache-2.0",
            "source": { "kind": "huggingface", "repo": "someone/tiny-GGUF", "revision": "0123456789abcdef0123456789abcdef01234567" },
            "files": [{ "path": "tiny-Q4_K_M.gguf", "size": 12, "sha256": "0".repeat(64) }],
            "layout": { "type": "llm_gguf", "model": "tiny-Q4_K_M.gguf" },
        }))
        .unwrap();
        os_models::hub::save_user_entry(&app.store, entry.clone()).unwrap();
        let file = app.store.file_path(&entry, "tiny-Q4_K_M.gguf");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, b"GGUF\x03\0\0\0abcd").unwrap();
        (entry.id, file)
    }

    #[tokio::test]
    async fn model_routes_follow_the_contract_and_its_guards() {
        let srv = server();
        let (id, file) = hub_model(&srv.app);
        let body = serde_json::json!({ "id": id, "note_model": null, "forget": false, "by": null });
        // Other pages can't remove anything.
        let (status, _) = call(&srv.router, "POST", "/api/models/remove", Some(body.clone()), false).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert!(file.is_file());
        // The note model, and a speech model OpenBark needs, are refused.
        let refused = serde_json::json!({ "id": id, "note_model": id, "forget": false, "by": null });
        let (status, err) = call(&srv.router, "POST", "/api/models/remove", Some(refused), true).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(err["error"], "This model writes your notes. Choose another one under In use first.");
        let needed = serde_json::json!({ "id": os_engine::models::VAD, "note_model": null, "forget": false, "by": null });
        let (status, err) = call(&srv.router, "POST", "/api/models/remove", Some(needed), true).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(err["error"], "OpenBark needs this to transcribe visits.");
        assert!(file.is_file());
        // Removed: the file goes, the row stays with Download.
        let (status, removal) = call(&srv.router, "POST", "/api/models/remove", Some(body), true).await;
        assert_eq!(status, StatusCode::OK, "{removal}");
        assert_eq!(removal["freed_bytes"], 12);
        assert_eq!(removal["forgotten"], false);
        assert!(!file.exists());
        let (_, rows) = call(&srv.router, "GET", "/api/models", None, false).await;
        let row = rows.as_array().unwrap().iter().find(|r| r["id"] == serde_json::json!(id)).unwrap().clone();
        assert_eq!(row["installed"], false);
        assert_eq!(row["origin"], "hub");
        assert_eq!(row["remove"], "delete");
        // The GPU pack's remove takes no body, or `{by}`.
        let (status, gpu) = call(&srv.router, "POST", "/api/gpu/remove", None, true).await;
        assert_eq!(status, StatusCode::OK, "{gpu}");
        assert!(gpu.get("installed_bytes").is_some());
        let (status, _) =
            call(&srv.router, "POST", "/api/gpu/remove", Some(serde_json::json!({ "by": null })), true).await;
        assert_eq!(status, StatusCode::OK);
        // The MLX card's status, and its removal refusal for an MLX note model.
        let (status, mlx) = call(&srv.router, "GET", "/api/mlx", None, false).await;
        assert_eq!(status, StatusCode::OK);
        assert!(mlx.get("support").is_some() && mlx.get("in_use") == Some(&serde_json::json!(false)));
        // An import path that isn't absolute is refused before anything happens.
        let (status, err) = call(
            &srv.router,
            "POST",
            "/api/models/import/inspect",
            Some(serde_json::json!({ "path": "models/tiny.gguf" })),
            true,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{err}");
    }

    /// Looking at a path on the computer running OpenBark (inspect) reveals
    /// files and sizes there, so with a team only a veterinarian may, as for
    /// adding the model.
    #[tokio::test]
    async fn inspecting_an_import_needs_a_veterinarian_with_a_team() {
        let srv = server();
        let file = srv._tmp.path().join("Downloads").join("Tiny-Q4_K_M.gguf");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        let mut bytes = b"GGUF".to_vec();
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(&[1u8; 32]);
        std::fs::write(&file, &bytes).unwrap();
        let path = file.to_string_lossy().into_owned();
        let dvm = srv.app.users.add("Dr Rivera", os_engine::users::Role::Dvm, "2468").unwrap();
        let desk = srv.app.users.add("Lee", os_engine::users::Role::Reception, "1111").unwrap();
        for (by, why) in [
            (serde_json::Value::Null, "Choose who you are first."),
            (serde_json::json!(desk.id), "Only a veterinarian can change the models."),
        ] {
            let body = serde_json::json!({ "path": path, "by": by });
            let (status, err) =
                call(&srv.router, "POST", "/api/models/import/inspect", Some(body), true).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{err}");
            assert_eq!(err["error"], why);
            assert!(err.get("files").is_none(), "nothing about the path is told");
        }
        let body = serde_json::json!({ "path": path, "by": dvm.id });
        let (status, preview) =
            call(&srv.router, "POST", "/api/models/import/inspect", Some(body), true).await;
        assert_eq!(status, StatusCode::OK, "{preview}");
        assert_eq!(preview["kind"], "gguf");
        assert_eq!(preview["total_bytes"], bytes.len());
    }
}
