//! Web mode: the OpenBark app layer ([`os_app::App`]) behind a small HTTP API,
//! serving the same React UI as the desktop app to a browser on this machine.
//!
//! Binds to 127.0.0.1 only. Exposing it to the clinic network needs the
//! Clinic Hub's authentication and mutual TLS (M4); there is no auth yet, so
//! it must never listen on other interfaces.
//!
//! API (JSON unless noted):
//! - `GET  /api/status`, `GET /api/devices`
//! - `GET  /api/sessions`, `GET /api/sessions/{id}`, `GET /api/sessions/{id}/audio` (Range)
//! - `POST /api/sessions?diarize=&name=`    body: WAV bytes → new encounter, transcribed
//! - `POST /api/sessions/{id}/notes`        `{llm, template}`
//! - `POST /api/sessions/{id}/refine`       `{llm, heading, request, save_as_standard, by}`:
//!   redraft one heading of the latest draft
//! - `POST /api/sessions/{id}/edit`         `{heading, value, by}`: the vet's own version of a heading
//! - `POST /api/sessions/{id}/addenda`      `{text, by}`: signed addition to a signed note
//! - `GET  /api/templates`, `POST /api/templates` (save), `POST /api/templates/preview`
//! - `GET|DELETE /api/templates/{id}`
//! - `POST /api/templates/{id}/standards` `{heading, text, by}`, `DELETE /api/templates/{id}/standards/{sid}`
//! - `GET  /api/live` (WebSocket)           browser microphone: binary f32 PCM frames
//!   (16 kHz mono); text "stop" ends it; the reply is the saved encounter
//! - `POST /api/record/start|stop`          record from this machine's microphone
//! - `GET  /api/models`, `POST /api/models/{id}/download`
//! - `GET  /api/hub/search?q=`, `GET /api/hub/repo?id=`, `POST /api/hub/add`
//! - `GET  /api/events`                     server-sent events: segment, token, level, download

use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;
use axum::body::Bytes;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{DefaultBodyLimit, Path as UrlPath, Query, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{delete, get, post};
use os_app::{App, AppError, AudioBlock, Events, Patient, RecordingSource, RefineRequest};
use os_engine::Segment;
use os_engine::template::Template;
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
    app: App,
    events: Arc<Broadcaster>,
}

#[derive(Clone, Serialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
enum ServerEvent {
    Segment(Segment),
    Token(String),
    Level { level: f32, seconds: f32 },
    Download { model: String, done: u64, total: u64 },
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
    let state: St = Arc::new(Shared {
        app: App::new(options.store, options.sessions_root),
        events: Arc::new(Broadcaster(tx)),
    });
    let index = options.ui_dir.join("index.html");
    let api = Router::new()
        .route("/api/status", get(|State(s): State<St>| async move { Json(s.app.status()) }))
        .route("/api/devices", get(|State(s): State<St>| async move { Json(s.app.input_devices()) }))
        .route("/api/sessions", get(list_sessions).post(create_session))
        .route("/api/sessions/{id}", get(open_session))
        .route("/api/sessions/{id}/audio", get(session_audio))
        .route("/api/sessions/{id}/notes", post(create_note))
        .route("/api/sessions/{id}/sign", post(sign_note))
        .route("/api/sessions/{id}/patient", post(update_patient))
        .route("/api/sessions/{id}/refine", post(refine_note))
        .route("/api/sessions/{id}/edit", post(edit_note))
        .route("/api/sessions/{id}/addenda", post(add_addendum))
        .route("/api/templates", get(list_templates).post(save_template))
        .route("/api/templates/preview", post(preview_template))
        .route("/api/templates/{id}", get(template).delete(delete_template))
        .route("/api/templates/{id}/standards", post(add_standard))
        .route("/api/templates/{id}/standards/{sid}", delete(remove_standard))
        .route("/api/live", get(live))
        .route("/api/record/start", post(record_start))
        .route("/api/record/stop", post(record_stop))
        .route("/api/models", get(|State(s): State<St>| async move { Json(s.app.models()) }))
        .route("/api/models/{id}/download", post(download_model))
        .route("/api/hub/search", get(hub_search))
        .route("/api/hub/repo", get(hub_repo))
        .route("/api/hub/add", post(hub_add))
        .route("/api/events", get(events_stream))
        .layer(DefaultBodyLimit::max(1 << 30))
        .with_state(state);
    let app = api.fallback_service(ServeDir::new(&options.ui_dir).fallback(ServeFile::new(index)));

    // Loopback only: there is no authentication yet (see module docs).
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, options.port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, "OpenBark web mode");
    axum::serve(listener, app).await
}

async fn list_sessions(State(s): State<St>) -> impl IntoResponse {
    Json(s.app.list_sessions())
}

async fn open_session(
    State(s): State<St>,
    UrlPath(id): UrlPath<String>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app.open_session(&id)).await
}

async fn session_audio(
    State(s): State<St>,
    UrlPath(id): UrlPath<String>,
    request: axum::extract::Request,
) -> Result<Response, ApiError> {
    use tower::ServiceExt;
    let dir = s.app.session_dir(&id)?;
    let response = ServeFile::new(dir.join(os_engine::session::AUDIO_FILE))
        .oneshot(request)
        .await
        .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(response.into_response())
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
}

impl PatientQuery {
    fn into_patient(self) -> Option<Patient> {
        let p = Patient {
            name: self.patient_name,
            last_name: self.last_name,
            reason: self.reason,
            ..Default::default()
        };
        (!p.is_empty()).then_some(p)
    }
}

#[derive(Deserialize)]
struct CreateSession {
    #[serde(default)]
    diarize: bool,
    #[serde(default)]
    name: Option<String>,
    #[serde(flatten)]
    patient: PatientQuery,
}

async fn create_session(
    State(s): State<St>,
    Query(q): Query<CreateSession>,
    body: Bytes,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || {
        let events = s.events.clone();
        let name = q.name.clone().unwrap_or_else(|| "recording.wav".into());
        s.app.import_bytes(&name, &body, q.diarize, q.patient.into_patient(), events.as_ref())
    })
    .await
}

#[derive(Deserialize)]
struct CreateNote {
    llm: String,
    #[serde(default = "soap")]
    template: String,
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
        s.app.generate_note(&id, &req.llm, &req.template, events.as_ref())
    })
    .await
}

#[derive(Deserialize)]
struct Sign {
    by: String,
}

async fn sign_note(
    State(s): State<St>,
    UrlPath(id): UrlPath<String>,
    Json(req): Json<Sign>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app.sign(&id, &req.by)).await
}

async fn update_patient(
    State(s): State<St>,
    UrlPath(id): UrlPath<String>,
    Json(p): Json<Patient>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app.update_patient(&id, p)).await
}

async fn refine_note(
    State(s): State<St>,
    UrlPath(id): UrlPath<String>,
    Json(req): Json<RefineRequest>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || {
        let events = s.events.clone();
        s.app.refine_note(&id, &req, events.as_ref())
    })
    .await
}

#[derive(Deserialize)]
struct EditNote {
    heading: String,
    value: serde_json::Value,
    by: String,
}

async fn edit_note(
    State(s): State<St>,
    UrlPath(id): UrlPath<String>,
    Json(req): Json<EditNote>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app.edit_note(&id, &req.heading, req.value, &req.by)).await
}

#[derive(Deserialize)]
struct NewAddendum {
    text: String,
    by: String,
}

async fn add_addendum(
    State(s): State<St>,
    UrlPath(id): UrlPath<String>,
    Json(req): Json<NewAddendum>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app.add_addendum(&id, &req.text, &req.by)).await
}

async fn list_templates(State(s): State<St>) -> Result<impl IntoResponse, ApiError> {
    blocking(move || Ok(s.app.list_templates())).await
}

async fn template(State(s): State<St>, UrlPath(id): UrlPath<String>) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app.template(&id)).await
}

async fn preview_template(
    State(s): State<St>,
    Json(t): Json<Template>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app.preview_template(t)).await
}

async fn save_template(State(s): State<St>, Json(t): Json<Template>) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app.save_template(t)).await
}

async fn delete_template(
    State(s): State<St>,
    UrlPath(id): UrlPath<String>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app.delete_template(&id)).await
}

#[derive(Deserialize)]
struct NewStandard {
    #[serde(default)]
    heading: String,
    text: String,
    #[serde(default)]
    by: Option<String>,
}

async fn add_standard(
    State(s): State<St>,
    UrlPath(id): UrlPath<String>,
    Json(req): Json<NewStandard>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app.add_standard(&id, &req.heading, &req.text, req.by.as_deref())).await
}

async fn remove_standard(
    State(s): State<St>,
    UrlPath((id, sid)): UrlPath<(String, String)>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app.remove_standard(&id, &sid)).await
}

#[derive(Deserialize)]
struct LiveQuery {
    #[serde(default = "yes")]
    speakers: bool,
    #[serde(default = "rate")]
    sample_rate: u32,
    #[serde(flatten)]
    patient: PatientQuery,
}

fn yes() -> bool {
    true
}
fn rate() -> u32 {
    16_000
}

/// Browser microphone streaming: binary messages are little-endian f32 PCM
/// (mono, `sample_rate` Hz). Text "stop" ends the recording; the server replies
/// with the saved encounter as JSON text, then closes.
async fn live(State(s): State<St>, Query(q): Query<LiveQuery>, ws: WebSocketUpgrade) -> Response {
    ws.on_upgrade(move |socket| live_session(s, q, socket))
}

async fn live_session(s: St, mut q: LiveQuery, mut socket: WebSocket) {
    let patient = std::mem::take(&mut q.patient).into_patient();
    let (tx, rx) = std::sync::mpsc::channel::<AudioBlock>();
    let events: Arc<dyn Events> = s.events.clone();
    let started = {
        let s = s.clone();
        tokio::task::spawn_blocking(move || {
            s.app.start_recording(RecordingSource::Stream(rx), q.speakers, patient, events)
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
            Message::Close(_) => break,
            _ => {}
        }
    }
    drop(tx); // ends the stream source
    let result = tokio::task::spawn_blocking(move || s.app.stop_recording()).await;
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
    #[serde(default)]
    patient: Option<Patient>,
}

async fn record_start(
    State(s): State<St>,
    Json(req): Json<RecordStart>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || {
        let events: Arc<dyn Events> = s.events.clone();
        s.app.start_recording(RecordingSource::Microphone(req.device), req.speakers, req.patient, events)
    })
    .await
}

async fn record_stop(State(s): State<St>) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app.stop_recording()).await
}

async fn download_model(
    State(s): State<St>,
    UrlPath(id): UrlPath<String>,
) -> Result<impl IntoResponse, ApiError> {
    blocking(move || {
        let events = s.events.clone();
        s.app.download_model(&id, None, events.as_ref())
    })
    .await
}

#[derive(Deserialize)]
struct Search {
    #[serde(default)]
    q: String,
}

async fn hub_search(State(s): State<St>, Query(q): Query<Search>) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app.hub_search(&q.q)).await
}

#[derive(Deserialize)]
struct RepoQuery {
    id: String,
}

async fn hub_repo(State(s): State<St>, Query(q): Query<RepoQuery>) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app.hub_repo(&q.id)).await
}

#[derive(Deserialize)]
struct HubAdd {
    repo: String,
    file: String,
    #[serde(default)]
    accept_license: bool,
}

async fn hub_add(State(s): State<St>, Json(req): Json<HubAdd>) -> Result<impl IntoResponse, ApiError> {
    blocking(move || s.app.hub_add(&req.repo, &req.file, req.accept_license)).await
}

async fn events_stream(
    State(s): State<St>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, std::convert::Infallible>>> {
    let stream = BroadcastStream::new(s.events.0.subscribe())
        .filter_map(|e| e.ok().map(|ev| Ok(Event::default().json_data(&ev).unwrap_or_default())));
    Sse::new(stream).keep_alive(KeepAlive::default())
}
