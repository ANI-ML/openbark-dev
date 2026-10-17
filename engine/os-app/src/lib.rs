//! The OpenBark application layer: everything the UI can ask for, in one
//! place, so the desktop app (Tauri commands) and web mode (HTTP API) are
//! thin transports over identical behaviour.
//!
//! All methods block; callers run them on a worker thread. Progress goes to
//! an [`Events`] sink (Tauri events on desktop, server-sent events on web).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use os_engine::awake::{AwakeGuard, AwakeReason};
use os_engine::live::{LiveOptions, LiveRecorder, LiveSink, LiveSource};
use os_engine::session::{self, Session};
use os_engine::{LoadedTemplate, NoteOptions, Segment, TranscribeStats, Transcript};
use os_llm::{LlamaCppBackend, LlamaCppOptions};
use os_models::hub::{self, Fit, HubModel, HubRepo};
use os_models::{CatalogEntry, ModelStore, Slot, Verify};
use serde::Serialize;

pub use os_asr::capture::AudioBlock;

/// Progress notifications for the UI.
pub trait Events: Send + Sync {
    fn segment(&self, segment: &Segment);
    fn token(&self, piece: &str);
    /// Microphone level 0..1 and elapsed seconds, ~20 times a second.
    fn level(&self, level: f32, seconds: f32);
    fn download(&self, model: &str, done: u64, total: u64);
}

/// A user-facing error message (written per docs/design/DESIGN.md §5 Errors).
/// Deliberately not `Display`, so any displayable error converts into it with `?`.
#[derive(Debug)]
pub struct AppError(pub String);

impl<E: std::fmt::Display> From<E> for AppError {
    fn from(e: E) -> Self {
        AppError(e.to_string())
    }
}

type Result<T> = std::result::Result<T, AppError>;

pub struct App {
    pub store: ModelStore,
    pub sessions_root: PathBuf,
    llm: Mutex<Option<(String, Arc<LlamaCppBackend>)>>,
    recorder: Mutex<Option<LiveRecorder>>,
    hardware: Hardware,
}

#[derive(Debug, Clone, Serialize)]
pub struct Hardware {
    pub gpu: Option<String>,
    pub gpu_memory_bytes: Option<u64>,
    pub ram_bytes: u64,
    pub summary: String,
}

fn probe_hardware() -> Hardware {
    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    let gpu = os_llm::devices().into_iter().find(|d| d.backend != "CPU");
    let ram = sys.total_memory();
    let summary = match &gpu {
        Some(g) => format!("{}, {:.0} GB graphics memory", g.description, g.memory_total_bytes as f64 / 1e9),
        None => format!("{:.0} GB memory, no supported graphics card", ram as f64 / 1e9),
    };
    Hardware {
        gpu: gpu.as_ref().map(|g| g.description.clone()),
        gpu_memory_bytes: gpu.map(|g| g.memory_total_bytes),
        ram_bytes: ram,
        summary,
    }
}

#[derive(Serialize)]
pub struct ModelStatus {
    pub id: String,
    pub name: String,
    pub installed: bool,
}

#[derive(Serialize)]
pub struct Status {
    pub product: &'static str,
    pub version: &'static str,
    pub models_dir: String,
    pub sessions_dir: String,
    pub models: Vec<ModelStatus>,
    pub hardware: Hardware,
    pub recording: bool,
}

#[derive(Serialize)]
pub struct SessionSummary {
    pub id: String,
    pub created_at: String,
    pub source: String,
    pub audio_secs: f32,
    pub runs: usize,
    pub signed: bool,
}

#[derive(Serialize)]
pub struct SessionView {
    pub id: String,
    pub dir: String,
    pub audio_path: String,
    pub created_at: String,
    pub source: String,
    pub transcript: Option<Transcript>,
    pub stats: Option<TranscribeStats>,
    /// The latest note run: `note.json` as written (note, faithfulness, stats…).
    pub latest_run: Option<serde_json::Value>,
    pub latest_markdown: Option<String>,
    pub signed_at: Option<String>,
    pub signed_by: Option<String>,
}

#[derive(Serialize)]
pub struct NoteResponse {
    pub markdown: String,
    pub run: serde_json::Value,
}

#[derive(Serialize)]
pub struct ModelRow {
    pub id: String,
    pub name: String,
    pub publisher: String,
    pub slot: Slot,
    pub size_bytes: u64,
    pub license: String,
    pub installed: bool,
    pub fit: Fit,
    pub builtin: bool,
}

impl App {
    pub fn new(store: ModelStore, sessions_root: PathBuf) -> Self {
        Self {
            store,
            sessions_root,
            llm: Mutex::new(None),
            recorder: Mutex::new(None),
            hardware: probe_hardware(),
        }
    }

    pub fn status(&self) -> Status {
        let models = os_engine::models::required_status(&self.store)
            .into_iter()
            .map(|(id, name, installed)| ModelStatus { id, name, installed })
            .collect();
        Status {
            product: os_engine::PRODUCT_NAME,
            version: env!("CARGO_PKG_VERSION"),
            models_dir: self.store.root().display().to_string(),
            sessions_dir: self.sessions_root.display().to_string(),
            models,
            hardware: self.hardware.clone(),
            recording: self.recorder.lock().map(|r| r.is_some()).unwrap_or(false),
        }
    }

    // ---- sessions --------------------------------------------------------

    /// Resolve a session id (its folder name) safely under the sessions root.
    pub fn session_dir(&self, id: &str) -> Result<PathBuf> {
        let valid = !id.is_empty()
            && !id.starts_with('.')
            && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
        if !valid {
            return Err(AppError(format!("'{id}' is not a session id")));
        }
        let dir = self.sessions_root.join(id);
        if dir.join(session::SESSION_FILE).is_file() {
            Ok(dir)
        } else {
            Err(AppError(format!("No saved encounter called {id}.")))
        }
    }

    pub fn list_sessions(&self) -> Vec<SessionSummary> {
        session::list(&self.sessions_root)
            .into_iter()
            .take(200)
            .map(|s| SessionSummary {
                id: id_of(&s),
                created_at: s.meta.created_at.format("%Y-%m-%d %H:%M").to_string(),
                source: source_label(&s.meta.source),
                audio_secs: s.meta.audio_secs,
                runs: s.meta.runs.len(),
                signed: s.is_signed(),
            })
            .collect()
    }

    pub fn open_session(&self, id: &str) -> Result<SessionView> {
        let s = Session::open(&self.session_dir(id)?)?;
        Ok(view(&s, None))
    }

    /// Create a session from a WAV file and transcribe it.
    ///
    /// Runs the file through the live pipeline at full speed, so imports and
    /// recordings get the same segmentation and speaker labels (the live
    /// tracker was more accurate than full-recording diarization on short
    /// clips; M1 tuning).
    pub fn import_file(&self, path: &Path, label_speakers: bool, events: &dyn Events) -> Result<SessionView> {
        let _awake = AwakeGuard::acquire(AwakeReason::Processing);
        let (tx, rx) = std::sync::mpsc::channel::<Segment>();
        let mut options = LiveOptions::microphone();
        options.label_speakers = label_speakers;
        let source = LiveSource::File { path: path.to_path_buf(), realtime: false };
        let recorder = LiveRecorder::start(
            &self.store,
            &self.sessions_root,
            source,
            options,
            Box::new(ChannelSink(tx)),
        )?;
        // The worker drops the sink when it finishes, which ends this loop.
        for segment in rx {
            events.segment(&segment);
        }
        let result = recorder.wait()?;
        let s = Session::open(&result.session_dir)?;
        Ok(view(&s, Some(result.transcript)))
    }

    /// Same as [`App::import_file`] for uploaded bytes (web mode).
    pub fn import_bytes(
        &self,
        name: &str,
        wav: &[u8],
        label_speakers: bool,
        events: &dyn Events,
    ) -> Result<SessionView> {
        if wav.len() < 44 || &wav[0..4] != b"RIFF" || &wav[8..12] != b"WAVE" {
            return Err(AppError(
                "That file isn't a WAV recording. Export the recording as WAV and try again.".into(),
            ));
        }
        let tmp = tempfile::tempdir()?;
        let file = Path::new(name).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let path = tmp.path().join(if file.is_empty() { "recording.wav".into() } else { file });
        std::fs::write(&path, wav)?;
        self.import_file(&path, label_speakers, events)
    }

    // ---- live recording --------------------------------------------------

    pub fn input_devices(&self) -> Vec<String> {
        os_asr::capture::input_devices()
    }

    /// Start recording from a microphone on this machine (`Some(device)`/`None`
    /// = default) or from blocks the caller pushes (`stream`).
    pub fn start_recording(
        &self,
        source: RecordingSource,
        label_speakers: bool,
        events: Arc<dyn Events>,
    ) -> Result<String> {
        let mut slot = self.recorder.lock().map_err(|_| AppError("recorder lock poisoned".into()))?;
        if slot.is_some() {
            return Err(AppError("A recording is already running. Stop it first.".into()));
        }
        let source = match source {
            RecordingSource::Microphone(device) => LiveSource::Microphone(device),
            RecordingSource::Stream(blocks) => LiveSource::Stream { label: "recording".into(), blocks },
        };
        let mut options = LiveOptions::microphone();
        options.label_speakers = label_speakers;
        let recorder = LiveRecorder::start(
            &self.store,
            &self.sessions_root,
            source,
            options,
            Box::new(EventsSink(events)),
        )?;
        let id =
            recorder.session_dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        *slot = Some(recorder);
        Ok(id)
    }

    /// Stop recording; returns the saved session with its full transcript.
    pub fn stop_recording(&self) -> Result<SessionView> {
        let recorder = self
            .recorder
            .lock()
            .map_err(|_| AppError("recorder lock poisoned".into()))?
            .take()
            .ok_or_else(|| AppError("Nothing is recording.".into()))?;
        let result = recorder.stop()?;
        let s = Session::open(&result.session_dir)?;
        Ok(view(&s, Some(result.transcript)))
    }

    // ---- notes -----------------------------------------------------------

    pub fn generate_note(
        &self,
        id: &str,
        llm: &str,
        template: &str,
        events: &dyn Events,
    ) -> Result<NoteResponse> {
        let _awake = AwakeGuard::acquire(AwakeReason::Processing);
        let backend = self.load_llm(llm)?;
        let mut s = Session::open(&self.session_dir(id)?)?;
        let transcript = s.load_transcript()?;
        let template = LoadedTemplate::resolve(template)?;
        let result = os_engine::generate_note(
            backend.as_ref(),
            &template,
            &transcript,
            &NoteOptions::default(),
            &mut |p| {
                events.token(p);
                true
            },
        )?;
        s.save_run(llm, &result)?;
        Ok(NoteResponse { markdown: result.markdown.clone(), run: serde_json::to_value(&result)? })
    }

    /// Mark the latest draft as signed by `by` (refused while numbers are unsupported).
    pub fn sign(&self, id: &str, by: &str) -> Result<SessionView> {
        if by.trim().is_empty() {
            return Err(AppError("Enter the name of the veterinarian signing the note.".into()));
        }
        let mut s = Session::open(&self.session_dir(id)?)?;
        s.sign_latest(by)?;
        Ok(view(&s, None))
    }

    /// A template's sections and fields, for rendering structured notes.
    pub fn template(&self, id: &str) -> Result<serde_json::Value> {
        Ok(serde_json::to_value(&LoadedTemplate::resolve(id)?.template)?)
    }

    fn load_llm(&self, id: &str) -> Result<Arc<LlamaCppBackend>> {
        let mut slot = self.llm.lock().map_err(|_| AppError("model lock poisoned".into()))?;
        if let Some((loaded, backend)) = slot.as_ref()
            && loaded == id
        {
            return Ok(backend.clone());
        }
        *slot = None; // one llama.cpp backend per process: free before loading
        let path = llm_path(&self.store, id)?;
        let backend = Arc::new(LlamaCppBackend::load(LlamaCppOptions::new(path))?);
        *slot = Some((id.to_string(), backend.clone()));
        Ok(backend)
    }

    // ---- models ----------------------------------------------------------

    pub fn models(&self) -> Vec<ModelRow> {
        let builtin: Vec<String> = os_models::Catalog::builtin().models.into_iter().map(|m| m.id).collect();
        hub::full_catalog(&self.store)
            .models
            .into_iter()
            .map(|m| self.row(&m, builtin.contains(&m.id)))
            .collect()
    }

    fn row(&self, m: &CatalogEntry, builtin: bool) -> ModelRow {
        let size = m.total_size();
        ModelRow {
            id: m.id.clone(),
            name: m.display_name.clone(),
            publisher: match &m.source {
                os_models::Source::Huggingface { repo, .. } => {
                    repo.split('/').next().unwrap_or_default().into()
                }
                os_models::Source::Url => "k2-fsa".into(),
            },
            slot: m.slot,
            size_bytes: size,
            license: m.license.clone(),
            installed: self.store.is_installed(m, Verify::Size),
            fit: if m.slot == Slot::Llm {
                hub::fit(size, self.hardware.gpu_memory_bytes, self.hardware.ram_bytes)
            } else {
                Fit::Fits
            },
            builtin,
        }
    }

    pub fn download_model(&self, id: &str, token: Option<&str>, events: &dyn Events) -> Result<()> {
        let _awake = AwakeGuard::acquire(AwakeReason::Processing);
        let catalog = hub::full_catalog(&self.store);
        let entry =
            catalog.get(id).ok_or_else(|| AppError(format!("No model called {id} in the catalog.")))?;
        let total = entry.total_size();
        let mut before = 0u64;
        let mut current = String::new();
        let mut file_done = 0u64;
        os_models::download(&self.store, entry, token, &mut |file: &str, done: u64, _t: u64| {
            if file != current {
                before += file_done;
                current = file.to_string();
            }
            file_done = done;
            events.download(id, before + done, total);
        })?;
        Ok(())
    }

    pub fn hub_search(&self, query: &str) -> Result<Vec<HubModel>> {
        Ok(hub::search(query, 30, None)?)
    }

    pub fn hub_repo(&self, repo: &str) -> Result<HubRepoView> {
        let r = hub::repo(repo, None)?;
        let files = r
            .files
            .iter()
            .map(|f| HubFileView {
                path: f.path.clone(),
                size_bytes: f.size,
                fit: hub::fit(f.size, self.hardware.gpu_memory_bytes, self.hardware.ram_bytes),
            })
            .collect();
        Ok(HubRepoView { files, repo: r })
    }

    /// Add a Hub file to the catalog; download it with [`App::download_model`].
    pub fn hub_add(&self, repo: &str, file: &str, accept_license: bool) -> Result<ModelRow> {
        let r = hub::repo(repo, None)?;
        let entry = hub::add_to_catalog(&self.store, &r, file, accept_license)?;
        Ok(self.row(&entry, false))
    }
}

pub enum RecordingSource {
    Microphone(Option<String>),
    Stream(std::sync::mpsc::Receiver<AudioBlock>),
}

#[derive(Serialize)]
pub struct HubFileView {
    pub path: String,
    pub size_bytes: u64,
    pub fit: Fit,
}

#[derive(Serialize)]
pub struct HubRepoView {
    pub repo: HubRepo,
    pub files: Vec<HubFileView>,
}

/// Sends live segments to the thread that started an import.
struct ChannelSink(std::sync::mpsc::Sender<Segment>);

impl LiveSink for ChannelSink {
    fn level(&mut self, _level: f32, _seconds: f32) {}
    fn segment(&mut self, segment: &Segment) {
        let _ = self.0.send(segment.clone());
    }
}

struct EventsSink(Arc<dyn Events>);

impl LiveSink for EventsSink {
    fn level(&mut self, level: f32, seconds: f32) {
        self.0.level(level, seconds);
    }
    fn segment(&mut self, segment: &Segment) {
        self.0.segment(segment);
    }
}

/// A catalog id (built-in or user-added) or a path to a `.gguf` file.
fn llm_path(store: &ModelStore, spec: &str) -> Result<PathBuf> {
    if spec.to_ascii_lowercase().ends_with(".gguf") && Path::new(spec).is_file() {
        return Ok(PathBuf::from(spec));
    }
    let catalog = hub::full_catalog(store);
    let m = catalog.get(spec).ok_or_else(|| AppError(format!("No model called {spec}.")))?;
    if !store.is_installed(m, Verify::Size) {
        return Err(AppError(format!("{} isn't downloaded yet. Download it in Models.", m.display_name)));
    }
    match &m.layout {
        os_models::Layout::LlmGguf { model } => Ok(store.file_path(m, model)),
        _ => Err(AppError(format!("{} can't write notes.", m.display_name))),
    }
}

fn id_of(s: &Session) -> String {
    s.dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

fn source_label(source: &str) -> String {
    if source.starts_with("live recording") {
        "Recorded".into()
    } else {
        Path::new(source)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| source.into())
    }
}

fn view(s: &Session, transcript: Option<Transcript>) -> SessionView {
    let latest = s.meta.runs.last().map(|r| s.dir.join(&r.dir));
    SessionView {
        id: id_of(s),
        dir: s.dir.display().to_string(),
        audio_path: s.audio_path().display().to_string(),
        created_at: s.meta.created_at.format("%Y-%m-%d %H:%M").to_string(),
        source: source_label(&s.meta.source),
        transcript: transcript.or_else(|| s.load_transcript().ok()),
        stats: s.meta.transcription.as_ref().map(|t| t.stats.clone()),
        latest_run: latest
            .as_ref()
            .and_then(|d| std::fs::read_to_string(d.join("note.json")).ok())
            .and_then(|t| serde_json::from_str(&t).ok()),
        latest_markdown: latest.and_then(|d| std::fs::read_to_string(d.join("note.md")).ok()),
        signed_at: s
            .meta
            .runs
            .last()
            .and_then(|r| r.signed_at)
            .map(|t| t.format("%Y-%m-%d %H:%M").to_string()),
        signed_by: s.meta.runs.last().and_then(|r| r.signed_by.clone()),
    }
}
