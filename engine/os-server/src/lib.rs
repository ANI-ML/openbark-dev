//! Local web mode: the OpenBark engine behind a small HTTP API, serving the
//! same React UI as the desktop app to any browser on this machine.
//!
//! Binds to 127.0.0.1 only. Exposing it to the clinic network needs the
//! Clinic Hub's authentication and mutual TLS (M4); until then there is no
//! auth, so it must never listen on other interfaces.
//!
//! API (JSON unless noted):
//! - `GET  /api/status`
//! - `GET  /api/sessions`                    newest first
//! - `GET  /api/sessions/{id}`               transcript + latest note
//! - `GET  /api/sessions/{id}/audio`         the WAV (supports Range, for seeking)
//! - `POST /api/sessions?diarize=true&name=` body: WAV bytes → new session, transcribed
//! - `POST /api/sessions/{id}/notes`         `{llm, template}` → new run
//! - `GET  /api/events`                      server-sent events: `segment`, `token`

use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path as UrlPath, Query, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{get, post};
use os_engine::awake::{AwakeGuard, AwakeReason};
use os_engine::session::{self, Session};
use os_engine::{LoadedTemplate, NoteOptions, Segment, TranscribeStats, Transcript};
use os_llm::{LlamaCppBackend, LlamaCppOptions};
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

struct AppState {
    store: ModelStore,
    sessions_root: PathBuf,
    llm: Mutex<Option<(String, Arc<LlamaCppBackend>)>>,
    events: broadcast::Sender<ServerEvent>,
}

#[derive(Clone, Serialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
enum ServerEvent {
    Segment(Segment),
    Token(String),
}

type Shared = Arc<AppState>;

struct ApiError(StatusCode, String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(serde_json::json!({ "error": self.1 }))).into_response()
    }
}

fn internal(e: impl std::fmt::Display) -> ApiError {
    ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

pub async fn serve(options: ServerOptions) -> std::io::Result<()> {
    let (events, _) = broadcast::channel(1024);
    let state: Shared = Arc::new(AppState {
        store: options.store,
        sessions_root: options.sessions_root,
        llm: Mutex::new(None),
        events,
    });
    let index = options.ui_dir.join("index.html");
    let app = Router::new()
        .route("/api/status", get(status))
        .route("/api/sessions", get(list_sessions).post(create_session))
        .route("/api/sessions/{id}", get(open_session))
        .route("/api/sessions/{id}/audio", get(session_audio))
        .route("/api/sessions/{id}/notes", post(create_note))
        .route("/api/events", get(events_stream))
        .layer(DefaultBodyLimit::max(1 << 30))
        .with_state(state)
        .fallback_service(ServeDir::new(&options.ui_dir).fallback(ServeFile::new(index)));

    // Loopback only: no authentication exists yet (see module docs).
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, options.port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, "OpenBark web mode");
    axum::serve(listener, app).await
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
    mode: &'static str,
    models_dir: String,
    sessions_dir: String,
    models: Vec<ModelStatus>,
    llm_backend: String,
}

async fn status(State(state): State<Shared>) -> Json<Status> {
    let models = os_engine::models::required_status(&state.store)
        .into_iter()
        .map(|(id, name, installed)| ModelStatus { id, name, installed })
        .collect();
    let gpu = os_llm::devices()
        .into_iter()
        .find(|d| d.backend != "CPU")
        .map(|d| format!("{} ({})", d.description, d.backend))
        .unwrap_or_else(|| "CPU".into());
    Json(Status {
        product: os_engine::PRODUCT_NAME,
        version: env!("CARGO_PKG_VERSION"),
        mode: "web",
        models_dir: state.store.root().display().to_string(),
        sessions_dir: state.sessions_root.display().to_string(),
        models,
        llm_backend: gpu,
    })
}

#[derive(Serialize)]
struct SessionSummary {
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
    audio_url: String,
    transcript: Option<Transcript>,
    stats: Option<TranscribeStats>,
    latest_note: Option<String>,
}

fn session_id(s: &Session) -> String {
    s.dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

fn view(s: &Session, transcript: Option<Transcript>) -> SessionView {
    let id = session_id(s);
    let latest_note =
        s.meta.runs.last().and_then(|r| std::fs::read_to_string(s.dir.join(&r.dir).join("note.md")).ok());
    SessionView {
        audio_url: format!("/api/sessions/{id}/audio"),
        id,
        dir: s.dir.display().to_string(),
        transcript: transcript.or_else(|| s.load_transcript().ok()),
        stats: s.meta.transcription.as_ref().map(|t| t.stats.clone()),
        latest_note,
    }
}

/// Resolve a session id (a folder name) safely under the sessions root.
fn session_dir(state: &AppState, id: &str) -> Result<PathBuf, ApiError> {
    let valid = !id.is_empty()
        && !id.starts_with('.')
        && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    if !valid {
        return Err(ApiError(StatusCode::BAD_REQUEST, "invalid session id".into()));
    }
    let dir = state.sessions_root.join(id);
    if dir.join(session::SESSION_FILE).is_file() {
        Ok(dir)
    } else {
        Err(ApiError(StatusCode::NOT_FOUND, format!("no session {id}")))
    }
}

async fn list_sessions(State(state): State<Shared>) -> Json<Vec<SessionSummary>> {
    let list = session::list(&state.sessions_root)
        .into_iter()
        .take(50)
        .map(|s| SessionSummary {
            id: session_id(&s),
            dir: s.dir.display().to_string(),
            created_at: s.meta.created_at.format("%Y-%m-%d %H:%M").to_string(),
            source: Path::new(&s.meta.source)
                .file_name()
                .map(|n| n.to_string_lossy().into())
                .unwrap_or_default(),
            audio_secs: s.meta.audio_secs,
            runs: s.meta.runs.len(),
        })
        .collect();
    Json(list)
}

async fn open_session(
    State(state): State<Shared>,
    UrlPath(id): UrlPath<String>,
) -> Result<Json<SessionView>, ApiError> {
    let dir = session_dir(&state, &id)?;
    let s = Session::open(&dir).map_err(internal)?;
    Ok(Json(view(&s, None)))
}

async fn session_audio(
    State(state): State<Shared>,
    UrlPath(id): UrlPath<String>,
    request: axum::extract::Request,
) -> Result<Response, ApiError> {
    use tower::ServiceExt;
    let dir = session_dir(&state, &id)?;
    let response = ServeFile::new(dir.join(session::AUDIO_FILE)).oneshot(request).await.map_err(internal)?;
    Ok(response.into_response())
}

#[derive(Deserialize)]
struct CreateSession {
    #[serde(default)]
    diarize: bool,
    /// Original file name, used for the session folder name.
    #[serde(default)]
    name: Option<String>,
}

async fn create_session(
    State(state): State<Shared>,
    Query(q): Query<CreateSession>,
    body: Bytes,
) -> Result<Json<SessionView>, ApiError> {
    if body.len() < 44 || &body[0..4] != b"RIFF" || &body[8..12] != b"WAVE" {
        return Err(ApiError(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "upload a WAV file (M0 supports WAV only)".into(),
        ));
    }
    let state2 = state.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<SessionView, ApiError> {
        let _awake = AwakeGuard::acquire(AwakeReason::Processing);
        let name = q.name.unwrap_or_else(|| "upload.wav".into());
        let stem = Path::new(&name).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let tmp = tempfile::tempdir().map_err(internal)?;
        let upload = tmp.path().join(if stem.is_empty() { "upload.wav".to_string() } else { stem });
        std::fs::write(&upload, &body).map_err(internal)?;
        let audio = os_asr::Audio::load_wav(&upload).map_err(internal)?;
        let mut s = Session::create_from_file(&state2.sessions_root, &upload, audio.duration_secs())
            .map_err(internal)?;
        let stt = os_engine::models::DEFAULT_STT;
        let transcriber = os_engine::models::transcriber(&state2.store, stt, q.diarize, None, None, None)
            .map_err(internal)?;
        let events = state2.events.clone();
        let (transcript, stats) = transcriber
            .run(&audio, &mut |seg| {
                let _ = events.send(ServerEvent::Segment(seg.clone()));
            })
            .map_err(internal)?;
        s.save_transcript(&transcript, stt, q.diarize, &stats).map_err(internal)?;
        Ok(view(&s, Some(transcript)))
    })
    .await
    .map_err(internal)??;
    Ok(Json(result))
}

#[derive(Deserialize)]
struct CreateNote {
    llm: String,
    #[serde(default = "default_template")]
    template: String,
}

fn default_template() -> String {
    "soap".into()
}

#[derive(Serialize)]
struct NoteResponse {
    markdown: String,
    run_dir: String,
    result: os_engine::NoteResult,
}

async fn create_note(
    State(state): State<Shared>,
    UrlPath(id): UrlPath<String>,
    Json(req): Json<CreateNote>,
) -> Result<Json<NoteResponse>, ApiError> {
    let dir = session_dir(&state, &id)?;
    let state2 = state.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<NoteResponse, ApiError> {
        let _awake = AwakeGuard::acquire(AwakeReason::Processing);
        let backend = load_llm(&state2, &req.llm)?;
        let mut s = Session::open(&dir).map_err(internal)?;
        let transcript = s.load_transcript().map_err(internal)?;
        let template = LoadedTemplate::resolve(&req.template).map_err(internal)?;
        let events = state2.events.clone();
        let result = os_engine::generate_note(
            backend.as_ref(),
            &template,
            &transcript,
            &NoteOptions::default(),
            &mut |p| {
                let _ = events.send(ServerEvent::Token(p.to_string()));
                true
            },
        )
        .map_err(internal)?;
        let run_dir = s.save_run(&req.llm, &result).map_err(internal)?;
        Ok(NoteResponse { markdown: result.markdown.clone(), run_dir: run_dir.display().to_string(), result })
    })
    .await
    .map_err(internal)??;
    Ok(Json(result))
}

fn load_llm(state: &AppState, id: &str) -> Result<Arc<LlamaCppBackend>, ApiError> {
    let mut slot = state.llm.lock().map_err(|_| internal("model lock poisoned"))?;
    if let Some((loaded, backend)) = slot.as_ref() {
        if loaded == id {
            return Ok(backend.clone());
        }
    }
    *slot = None;
    let path = os_engine::models::llm_path(&state.store, id).map_err(internal)?;
    let backend = Arc::new(LlamaCppBackend::load(LlamaCppOptions::new(path)).map_err(internal)?);
    *slot = Some((id.to_string(), backend.clone()));
    Ok(backend)
}

async fn events_stream(
    State(state): State<Shared>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, std::convert::Infallible>>> {
    let stream = BroadcastStream::new(state.events.subscribe()).filter_map(|e| {
        e.ok().map(|event| Ok(Event::default().json_data(&event).unwrap_or_else(|_| Event::default())))
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}
