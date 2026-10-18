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
pub use os_engine::live::{Progress, Stage};
use os_engine::patients::{PatientRecord, PatientStore};
use os_engine::session::{self, Addendum, Consent, PatientInfo, Session};
use os_engine::template::{Standard, Template};
use os_engine::template_store::TemplateSummary;
use os_engine::users::{Role, User, UserStore, Who};
use os_engine::{LoadedTemplate, NoteOptions, Segment, TemplateStore, TranscribeStats, Transcript};
use os_llm::{LlamaCppBackend, LlamaCppOptions};
use os_models::hub::{self, Fit, HubModel, HubRepo};
use os_models::locations::{self, Contents};
pub use os_models::locations::{FolderKind, Moved, Source as FolderSource};
use os_models::{CatalogEntry, ModelStore, Slot, Verify};
use serde::Serialize;

pub use os_asr::capture::AudioBlock;
pub use os_engine::session::PatientInfo as Patient;
pub use os_engine::session::{Consent as RecordingConsent, ConsentMethod, ConsentRule};

/// Progress notifications for the UI.
pub trait Events: Send + Sync {
    fn segment(&self, segment: &Segment);
    fn token(&self, piece: &str);
    /// Microphone level 0..1 and elapsed seconds, ~20 times a second.
    fn level(&self, level: f32, seconds: f32);
    fn download(&self, model: &str, done: u64, total: u64);
    /// How far an import, or a stopped recording, has got (a progress bar).
    fn progress(&self, _progress: &Progress) {}
    /// Bytes moved so far when a folder changes (ADR-0013).
    fn moving(&self, _done: u64, _total: u64) {}
}

/// Where models or visits are kept (Settings, Storage).
#[derive(Debug, Clone, Serialize)]
pub struct FolderView {
    pub kind: FolderKind,
    pub path: String,
    pub source: FolderSource,
    /// What fixed it, when it can't be changed in the app.
    pub fixed_by: Option<String>,
    pub used_bytes: u64,
    pub free_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StorageView {
    pub models: FolderView,
    pub visits: FolderView,
}

/// What choosing a folder would do, shown before anything changes.
#[derive(Debug, Clone, Serialize)]
pub struct FolderCheck {
    pub kind: FolderKind,
    /// The folder chosen.
    pub chosen: String,
    /// Where OpenBark's files would go: the chosen folder, or an "OpenBark
    /// models" / "OpenBark visits" folder inside it when it holds other things.
    pub target: String,
    /// The target already holds OpenBark data of this kind.
    pub has_data: bool,
    /// Why this folder can't be used; nothing changes while there is one.
    pub problem: Option<FolderProblem>,
    /// Things worth knowing (cloud sync, removable or network drives).
    pub notices: Vec<FolderNotice>,
    /// Size of what's in the current folder.
    pub needed_bytes: u64,
    pub free_bytes: Option<u64>,
    /// Same disk: moving is instant.
    pub same_disk: bool,
    /// Room to move the current folder's contents there.
    pub can_move: bool,
}

/// Why a chosen folder can't be used. Sent as a code the screen translates;
/// [`FolderProblem::message`] is the English, for errors.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum FolderProblem {
    /// Set by an environment variable or the command line.
    Fixed {
        by: String,
    },
    NotAbsolute,
    SameFolder,
    /// Inside the current folder, or around it.
    AroundCurrent,
    /// Overlaps the other kind's folder.
    OverlapsOther,
    /// Inside the app's own data folder, which "delete my data" removes.
    InsideAppData,
    /// A network path typed in the browser version.
    NetworkPath,
    NotWritable,
}

impl FolderProblem {
    pub fn message(&self) -> String {
        match self {
            FolderProblem::Fixed { by } => format!("This folder is set by {by}, so it can't be changed here."),
            FolderProblem::NotAbsolute => r"Choose a full folder path, such as D:\OpenBark.".into(),
            FolderProblem::SameFolder => "That's the folder OpenBark already uses.".into(),
            FolderProblem::AroundCurrent => "Choose a folder that isn't inside the current one, or around it.".into(),
            FolderProblem::OverlapsOther => "Models and visits need separate folders, not one inside the other.".into(),
            FolderProblem::InsideAppData => {
                "Choose a folder outside OpenBark's own data folder: uninstalling with \"delete my data\" would remove it.".into()
            }
            FolderProblem::NetworkPath => {
                "Network folders can't be chosen in the browser version. Map the share to a drive letter first, or use the desktop app.".into()
            }
            FolderProblem::NotWritable => {
                "OpenBark can't write to that folder. Choose another, or ask whoever manages this computer for access.".into()
            }
        }
    }
}

/// Things worth knowing about a chosen folder, as codes the screen translates.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum FolderNotice {
    /// Synced by a cloud service (OneDrive, Dropbox, ...).
    Synced { service: String },
    /// A network share or an external disk.
    NetworkDrive,
    /// A drive that can be unplugged.
    Removable,
}

/// How to change a folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeMode {
    /// Bring everything in the current folder along.
    Move,
    /// Use the new folder as it is; the current one is left where it is.
    Switch,
}

#[derive(Debug, Clone, Serialize)]
pub struct FolderChanged {
    pub kind: FolderKind,
    pub path: String,
    pub moved: Option<Moved>,
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

/// Work that writes visits or models, against a folder change (ADR-0013;
/// review 2026-10-18). Each write holds a [`Busy`] ticket while it runs; a
/// folder change takes `changing` and refuses while any ticket is out, so a
/// move never races a draft, a signature or a download. Once its folders
/// have changed, an App is retired and refuses new work: requests still
/// holding it (web mode swaps in a fresh App) can't write to the old folder.
#[derive(Default)]
struct Gate {
    busy: std::sync::atomic::AtomicUsize,
    changing: std::sync::atomic::AtomicBool,
    retired: std::sync::atomic::AtomicBool,
}

/// A ticket for one piece of writing work; dropping it ends the work.
pub struct Busy(Arc<Gate>);

impl Drop for Busy {
    fn drop(&mut self) {
        self.0.busy.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

pub struct App {
    pub store: ModelStore,
    pub sessions_root: PathBuf,
    pub templates: TemplateStore,
    /// Team members with roles (M1 item 11); empty means single-user.
    pub users: UserStore,
    /// Patients, with their visits linked by id (ADR-0011).
    pub patients: PatientStore,
    llm: Mutex<Option<(String, Arc<LlamaCppBackend>)>>,
    recorder: Mutex<Option<LiveRecorder>>,
    /// The live recording's work ticket, held until it stops.
    recording_work: Mutex<Option<Busy>>,
    gate: Arc<Gate>,
    hardware: Hardware,
    /// The GPU speech pack was in place when this process started, so its
    /// libraries are the ones loaded (ADR-0014).
    gpu_at_start: bool,
    /// The GPU self-test: 0 not run, 1 passed, 2 failed.
    gpu_checked: Arc<std::sync::atomic::AtomicU8>,
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

/// A GPU speech pack download is running (one at a time per process).
static GPU_INSTALLING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Passed to the program itself to run the GPU self-test in a child process.
pub const GPU_CHECK_ARG: &str = "--openbark-gpu-check";

/// The GPU self-test, run in a child process (`<program> --openbark-gpu-check`,
/// with `OPENBARK_MODELS_DIR` set): load both full-precision recognisers on
/// the GPU and transcribe a second of silence. Exits 0 when that works. If
/// ONNX Runtime fails in a way that aborts, only this child goes down.
pub fn gpu_check_main() -> i32 {
    // SAFETY: single-threaded at this point; the child only runs this check.
    unsafe { std::env::set_var("OPENBARK_ASR_PROVIDER", "cuda") };
    let store = ModelStore::default_location();
    let silence = vec![0.0f32; 16_000];
    for id in [os_engine::models::GPU_STT, os_engine::models::GPU_SECOND_STT] {
        let model = match os_engine::models::asr_model(&store, id) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("{id}: {e}");
                return 2;
            }
        };
        match os_asr::OfflineAsr::with_threads(model, 2) {
            Ok(asr) => {
                if let Err(e) = os_asr::AsrBackend::transcribe(&asr, &silence) {
                    eprintln!("{id}: {e}");
                    return 3;
                }
            }
            Err(e) => {
                eprintln!("{id}: {e}");
                return 4;
            }
        }
    }
    0
}

/// Whether the GPU passed its self-test: the saved result, or run the test
/// now in a child process and save it (ADR-0014).
fn verify_gpu(store: &ModelStore) -> bool {
    let Some(file) = os_models::gpu_pack::check_file() else { return false };
    if let Ok(saved) = std::fs::read_to_string(&file) {
        return saved.trim() == "ok";
    }
    let Ok(exe) = std::env::current_exe() else { return false };
    let mut cmd = std::process::Command::new(exe);
    cmd.arg(GPU_CHECK_ARG)
        .env("OPENBARK_MODELS_DIR", store.root())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let ok = match cmd.output() {
        Ok(out) if out.status.success() => true,
        Ok(out) => {
            let err = String::from_utf8_lossy(&out.stderr);
            tracing::warn!(status = %out.status, detail = %err.lines().rev().take(3).collect::<Vec<_>>().join(" | "), "GPU self-test failed; speech recognition stays on the processor");
            false
        }
        Err(e) => {
            tracing::warn!(error = %e, "GPU self-test couldn't start");
            false
        }
    };
    let _ = std::fs::write(
        &file,
        if ok {
            "ok
"
        } else {
            "failed
"
        },
    );
    ok
}

/// Speech recognition on the graphics card (Models screen, ADR-0014).
#[derive(Debug, Clone, Serialize)]
pub struct GpuStatus {
    /// Windows on x64 with an NVIDIA graphics card.
    pub supported: bool,
    pub gpu: Option<String>,
    pub pack_installed: bool,
    /// The full-precision recognisers the GPU runs.
    pub models_installed: bool,
    /// Installed and loaded: speech recognition runs on the GPU.
    pub active: bool,
    /// Downloading or setting up right now.
    pub installing: bool,
    /// Installed since OpenBark started: restart to use it.
    pub restart_needed: bool,
    /// The self-test failed, so the processor is used.
    pub check_failed: bool,
    /// Still to download.
    pub download_bytes: u64,
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
    /// Encryption at rest (ADR-0008): on, where the key is, any warning.
    pub encryption: os_engine::vault::VaultStatus,
    /// Encounters saved before encryption, still in plain files.
    pub plain_encounters: usize,
    /// Whether team members are set up (otherwise single-user).
    pub team: bool,
    /// "macos", "windows" or "linux": the Mac shows MLX-optimized models.
    pub platform: String,
}

/// A visit as listed under a patient.
#[derive(Debug, Clone, Serialize)]
pub struct VisitSummary {
    pub id: String,
    pub created_at: String,
    pub reason: String,
    /// The latest draft's template (the type of visit) and its icon.
    pub template: String,
    /// Its id (empty before the first draft): Reprocess offers it first.
    pub template_id: String,
    pub template_icon: String,
    pub drafted: bool,
    pub signed: bool,
    pub audio_secs: f32,
    /// This animal's name, when the visit covered several.
    pub pet: Option<String>,
    pub source: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PatientSummary {
    pub patient: PatientRecord,
    pub visits: usize,
    pub last_visit: Option<String>,
    pub last_type: String,
    pub last_icon: String,
    pub last_reason: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PatientView {
    pub patient: PatientRecord,
    pub visits: Vec<VisitSummary>,
}

#[derive(Serialize)]
pub struct SessionSummary {
    pub id: String,
    pub created_at: String,
    pub source: String,
    pub audio_secs: f32,
    pub runs: usize,
    pub signed: bool,
    /// "Maple Okonkwo", if the clinic entered it.
    pub patient: String,
    pub reason: String,
    /// Name of the template the latest draft used.
    pub template: String,
}

#[derive(Serialize)]
pub struct SessionView {
    /// The animals in a multi-pet visit (empty for a single animal).
    pub pets: Vec<String>,
    /// Whose notes this view shows, in a multi-pet visit.
    pub pet: Option<String>,
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
    /// Clinic-entered details with their entry and change times.
    pub patient: Option<PatientInfo>,
    /// The template the latest draft was written with (its saved snapshot),
    /// so the note renders under the headings it was drafted to.
    pub latest_template: Option<Template>,
    /// Signed additions to the signed note, oldest first.
    pub addenda: Vec<Addendum>,
    /// Consent to record, as confirmed before recording.
    pub consent: Option<Consent>,
    /// The latest draft of each client document (discharge, summary…).
    pub documents: Vec<DocumentView>,
    /// Who started the recording or import.
    pub recorded_by: Option<Who>,
}

/// A document for the client, drafted from the visit.
#[derive(Serialize)]
pub struct DocumentView {
    pub template_id: String,
    pub name: String,
    pub created_at: String,
    /// `note.json` as written.
    pub run: Option<serde_json::Value>,
    pub markdown: Option<String>,
    /// The template the document was written with.
    pub template: Option<Template>,
}

/// A template for the builder: its headings, and what the model is told.
#[derive(Serialize)]
pub struct TemplateView {
    pub template: Template,
    pub builtin: bool,
    /// The system prompt drafts with this template start from.
    pub prompt: String,
}

impl TemplateView {
    fn of(t: LoadedTemplate) -> Self {
        Self {
            builtin: LoadedTemplate::is_builtin(&t.template.id),
            prompt: os_engine::note::system_prompt(&t),
            template: t.template,
        }
    }
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
        // Encryption at rest: load or create the data key (ADR-0008).
        if let Err(e) = os_engine::vault::init() {
            tracing::error!(error = %e, "encryption couldn't start; encounters will be saved unencrypted");
        }
        let app = Self {
            store,
            patients: PatientStore::for_sessions(&sessions_root),
            sessions_root,
            templates: TemplateStore::default(),
            users: UserStore::default(),
            llm: Mutex::new(None),
            recorder: Mutex::new(None),
            recording_work: Mutex::new(None),
            gate: Arc::new(Gate::default()),
            hardware: probe_hardware(),
            gpu_at_start: os_models::gpu_pack::loaded_at_start(),
            gpu_checked: Arc::new(std::sync::atomic::AtomicU8::new(0)),
        };
        app.start_gpu_check();
        app
    }

    /// With the GPU speech pack loaded, run its self-test in the background
    /// (a few seconds; recognisers use the processor until it passes).
    fn start_gpu_check(&self) {
        let models_ready = self.gpu_models().iter().all(|m| self.store.is_installed(m, Verify::Size));
        if !(self.gpu_at_start && models_ready) {
            return;
        }
        let store = ModelStore::new(self.store.root());
        let checked = self.gpu_checked.clone();
        std::thread::spawn(move || {
            let ok = verify_gpu(&store);
            os_asr::set_gpu_usable(ok);
            checked.store(if ok { 1 } else { 2 }, std::sync::atomic::Ordering::SeqCst);
        });
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
            encryption: os_engine::vault::status(),
            plain_encounters: self.plain_encounters(),
            team: self.users.is_team(),
            platform: platform(),
        }
    }

    /// Who is acting. With a team set up, `by` is a team member's id and
    /// their role must allow `need`; single-user, `by` is just a name.
    fn who(&self, by: Option<&str>, need: Need) -> Result<Option<Who>> {
        let by = by.map(str::trim).filter(|b| !b.is_empty());
        if !self.users.is_team() {
            return Ok(by.map(Who::named));
        }
        let user = by
            .and_then(|id| self.users.get(id))
            .ok_or_else(|| AppError("Choose who you are first.".into()))?;
        let allowed = match need {
            Need::Any => true,
            Need::Notes => user.role.can_write_notes(),
            Need::Sign => user.role.can_sign(),
            Need::Team => user.role.can_manage_team(),
        };
        if !allowed {
            return Err(AppError(match need {
                Need::Notes => format!(
                    "{} can record and enter patient details; drafting and editing notes needs a veterinarian or technician.",
                    user.name
                ),
                Need::Sign => format!("Only a veterinarian can sign. {} is using OpenBark now.", user.name),
                Need::Team => "Only a veterinarian can change the team.".into(),
                Need::Any => unreachable!(),
            }));
        }
        Ok(Some(Who::from(&user)))
    }

    // ---- team (M1 item 11) -----------------------------------------------

    pub fn list_users(&self) -> Result<Vec<User>> {
        Ok(self.users.list()?)
    }

    /// Add a team member. Once a team exists, only a veterinarian can.
    pub fn add_user(&self, name: &str, role: Role, pin: &str, by: Option<&str>) -> Result<User> {
        if self.users.is_team() {
            self.who(by, Need::Team)?;
        }
        Ok(self.users.add(name, role, pin)?)
    }

    pub fn remove_user(&self, id: &str, by: Option<&str>) -> Result<()> {
        self.who(by, Need::Team)?;
        Ok(self.users.remove(id)?)
    }

    /// Change a PIN: your own, or anyone's as a veterinarian.
    pub fn set_user_pin(&self, id: &str, pin: &str, by: Option<&str>) -> Result<()> {
        if by.map(str::trim) != Some(id) {
            self.who(by, Need::Team)?;
        }
        Ok(self.users.set_pin(id, pin)?)
    }

    /// Check a team member's PIN when they start using the app.
    pub fn verify_user(&self, id: &str, pin: &str) -> Result<User> {
        Ok(self.users.verify(id, pin)?)
    }

    /// Encounter folders still holding plain (unencrypted) files.
    fn plain_encounters(&self) -> usize {
        session::list(&self.sessions_root)
            .iter()
            .filter(|s| !os_engine::vault::is_encrypted(&s.dir.join(session::SESSION_FILE)))
            .count()
    }

    /// Encrypt every encounter saved before encryption was on. Returns how
    /// many encounters were rewritten.
    pub fn encrypt_existing(&self) -> Result<usize> {
        let _work = self.begin_work()?;
        if !os_engine::vault::enabled() {
            return Err(AppError("Encryption isn't set up on this computer.".into()));
        }
        let recording = self.recorder.lock().ok().and_then(|r| r.as_ref().map(|rec| rec.session_dir.clone()));
        let mut count = 0;
        for s in session::list(&self.sessions_root) {
            if Some(&s.dir) == recording.as_ref() {
                continue;
            }
            if os_engine::vault::encrypt_folder(&s.dir)? > 0 {
                count += 1;
            }
        }
        Ok(count)
    }

    /// The recovery key (the data key in base64), to keep somewhere safe.
    pub fn recovery_key(&self) -> Result<String> {
        os_engine::vault::recovery_key()
            .ok_or_else(|| AppError("Encryption isn't set up on this computer.".into()))
    }

    /// An encounter's recording as WAV bytes (decrypted for playback).
    pub fn session_audio(&self, id: &str) -> Result<Vec<u8>> {
        let dir = self.session_dir(id)?;
        Ok(os_engine::vault::read(&dir.join(session::AUDIO_FILE))?)
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
                patient: s.meta.patient.as_ref().map(|p| p.display_name()).unwrap_or_default(),
                reason: s.meta.patient.as_ref().map(|p| p.reason.clone()).unwrap_or_default(),
                template: s
                    .latest_record()
                    .map(|r| {
                        self.templates
                            .load(r.template_id())
                            .map(|t| t.template.name)
                            .unwrap_or_else(|_| r.template_id().to_string())
                    })
                    .unwrap_or_default(),
            })
            .collect()
    }

    // ---- patients (ADR-0011) -------------------------------------------------

    /// The patient records for a visit's animals (main first), linking the
    /// visit by id the first time it's seen. Visits from before the registry
    /// are matched by animal and family name.
    fn link_visit(&self, s: &mut Session) -> Result<Vec<(String, String)>> {
        let Some(p) = s.meta.patient.clone() else { return Ok(Vec::new()) };
        if p.name.trim().is_empty() {
            return Ok(Vec::new());
        }
        let known = |id: &Option<String>| id.as_ref().filter(|id| self.patients.get(id).is_some()).cloned();
        let main = match known(&p.patient_id) {
            Some(id) => id,
            None => self.patients.find_or_create(&p.name, &p.last_name, &p.species)?.id,
        };
        let mut pairs = vec![(p.name.clone(), main.clone())];
        let mut others = Vec::new();
        for (i, name) in p.others.iter().enumerate() {
            let id = match known(&p.other_ids.get(i).cloned()) {
                Some(id) => id,
                None => self.patients.find_or_create(name, &p.last_name, "")?.id,
            };
            pairs.push((name.clone(), id.clone()));
            others.push(id);
        }
        s.link_patients(Some(main), others)?;
        Ok(pairs)
    }

    fn visit_summary(&self, s: &Session, pet: &str, animals: usize) -> VisitSummary {
        // In a multi-pet visit each animal has its own drafts (filed under its name).
        let per_pet;
        let s = if animals > 1 {
            per_pet = s.clone().for_pet(Some(pet));
            &per_pet
        } else {
            s
        };
        let template_id = s.latest_record().map(|r| r.template_id().to_string()).unwrap_or_default();
        let template = s.latest_record().and_then(|r| self.templates.load(r.template_id()).ok());
        VisitSummary {
            template_id,
            id: id_of(s),
            created_at: s.meta.created_at.format("%Y-%m-%d %H:%M").to_string(),
            reason: s.meta.patient.as_ref().map(|p| p.reason.clone()).unwrap_or_default(),
            template: template.as_ref().map(|t| t.template.name.clone()).unwrap_or_default(),
            template_icon: template.as_ref().map(|t| t.template.icon.clone()).unwrap_or_default(),
            drafted: s.my_runs().next().is_some(),
            signed: s.is_signed(),
            audio_secs: s.meta.audio_secs,
            pet: (animals > 1).then(|| pet.to_string()),
            source: source_label(&s.meta.source),
        }
    }

    /// Every visit, grouped under its patients' record ids.
    fn visits_by_patient(&self) -> Result<std::collections::HashMap<String, Vec<VisitSummary>>> {
        let mut out: std::collections::HashMap<String, Vec<VisitSummary>> = Default::default();
        for mut s in session::list(&self.sessions_root) {
            let pairs = self.link_visit(&mut s)?;
            let animals = pairs.len();
            for (pet, id) in pairs {
                let summary = self.visit_summary(&s, &pet, animals);
                out.entry(id).or_default().push(summary);
            }
        }
        Ok(out)
    }

    /// Patients, most recently seen first.
    pub fn list_patients(&self) -> Result<Vec<PatientSummary>> {
        let _work = self.begin_work()?;
        let mut visits = self.visits_by_patient()?;
        let mut out: Vec<PatientSummary> = self
            .patients
            .list()?
            .into_iter()
            .map(|p| {
                let v = visits.remove(&p.id).unwrap_or_default();
                PatientSummary {
                    visits: v.len(),
                    last_visit: v.first().map(|x| x.created_at.clone()),
                    last_type: v.first().map(|x| x.template.clone()).unwrap_or_default(),
                    last_icon: v.first().map(|x| x.template_icon.clone()).unwrap_or_default(),
                    last_reason: v.first().map(|x| x.reason.clone()).unwrap_or_default(),
                    patient: p,
                }
            })
            .collect();
        out.sort_by(|a, b| {
            b.last_visit
                .cmp(&a.last_visit)
                .then_with(|| a.patient.name.to_lowercase().cmp(&b.patient.name.to_lowercase()))
        });
        Ok(out)
    }

    /// One patient and their visits, newest first.
    pub fn patient(&self, id: &str) -> Result<PatientView> {
        let _work = self.begin_work()?;
        let mut visits = self.visits_by_patient()?;
        let patient = self
            .patients
            .get(id)
            .ok_or_else(|| AppError("That patient isn't in this computer's records.".into()))?;
        Ok(PatientView { visits: visits.remove(id).unwrap_or_default(), patient })
    }

    pub fn create_patient(&self, name: &str, last_name: &str, species: &str) -> Result<PatientRecord> {
        let _work = self.begin_work()?;
        Ok(self.patients.create(name, last_name, species)?)
    }

    pub fn update_patient_record(
        &self,
        id: &str,
        name: &str,
        last_name: &str,
        species: &str,
    ) -> Result<PatientRecord> {
        let _work = self.begin_work()?;
        Ok(self.patients.update(id, name, last_name, species)?)
    }

    /// An encounter, showing one animal's notes in a multi-pet visit
    /// (`pet`), or the visit's single note.
    pub fn open_session(&self, id: &str, pet: Option<&str>) -> Result<SessionView> {
        Ok(view(&self.open(id, pet)?, None))
    }

    fn open(&self, id: &str, pet: Option<&str>) -> Result<Session> {
        Ok(Session::open(&self.session_dir(id)?)?.for_pet(pet))
    }

    /// Note options for this encounter: patient context and, in a
    /// multi-pet visit, which animal the note is about.
    /// The species entered for the animal this handle works on. Other animals
    /// in a multi-pet visit have no species entered, so they get every heading.
    fn species_of(s: &Session) -> Option<String> {
        let p = s.meta.patient.as_ref()?;
        let main = s.pet.as_deref().is_none_or(|pet| pet.trim().eq_ignore_ascii_case(p.name.trim()));
        (main && !p.species.trim().is_empty()).then(|| p.species.trim().to_string())
    }

    fn note_options(&self, s: &Session, language: Option<String>) -> NoteOptions {
        let pets = s.meta.patient.as_ref().map(|p| p.pets()).unwrap_or_default();
        let other_pets = match &s.pet {
            Some(pet) => pets.iter().filter(|p| !p.eq_ignore_ascii_case(pet)).cloned().collect(),
            None => Vec::new(),
        };
        NoteOptions {
            context: s.meta.patient.as_ref().map(|p| p.prompt_context()),
            language,
            pet: s.pet.clone(),
            other_pets,
            ..NoteOptions::default()
        }
    }

    /// Create a session from a recording file (WAV, m4a, mp3…) and transcribe it.
    ///
    /// Runs the file through the live pipeline at full speed, so imports and
    /// recordings get the same segmentation and speaker labels (the live
    /// tracker was more accurate than full-recording diarization on short
    /// clips; M1 tuning).
    pub fn import_file(
        &self,
        path: &Path,
        label_speakers: bool,
        patient: Option<PatientInfo>,
        consent: Option<Consent>,
        by: Option<&str>,
        events: &dyn Events,
    ) -> Result<SessionView> {
        let source = LiveSource::File { path: path.to_path_buf(), realtime: false };
        self.import_source(source, label_speakers, patient, consent, by, events)
    }

    fn import_source(
        &self,
        source: LiveSource,
        label_speakers: bool,
        patient: Option<PatientInfo>,
        consent: Option<Consent>,
        by: Option<&str>,
        events: &dyn Events,
    ) -> Result<SessionView> {
        let _work = self.begin_work()?;
        if let Some(c) = &consent {
            c.check()?;
        }
        let recorded_by = self.who(by, Need::Any)?;
        let _awake = AwakeGuard::acquire(AwakeReason::Processing);
        // The bar starts now: loading the models and decoding take a few seconds.
        events.progress(&Progress::new(Stage::Read, 0.0, 1.0));
        let (tx, rx) = std::sync::mpsc::channel::<Live>();
        let mut options = LiveOptions::microphone();
        options.label_speakers = label_speakers;
        options.patient = patient.filter(|p| !p.is_empty());
        options.consent = consent;
        options.recorded_by = recorded_by;
        let recorder = LiveRecorder::start(
            &self.store,
            &self.sessions_root,
            source,
            options,
            Box::new(ChannelSink(tx)),
        )?;
        let dir = recorder.session_dir.clone();
        // The worker drops the sink when it finishes, which ends this loop.
        for msg in rx {
            match msg {
                Live::Segment(segment) => events.segment(&segment),
                Live::Progress(progress) => events.progress(&progress),
            }
        }
        let result = match recorder.wait() {
            Ok(r) => r,
            Err(e) => {
                // An import that couldn't be read leaves no empty visit behind.
                let _ = std::fs::remove_dir_all(&dir);
                return Err(e.into());
            }
        };
        let mut s = Session::open(&result.session_dir)?;
        self.link_visit(&mut s)?;
        Ok(view(&s, Some(result.transcript)))
    }

    /// Same as [`App::import_file`] for uploaded bytes (web mode). Any format
    /// the engine decodes (see `os_asr::audio::Audio::load`); the file name's
    /// extension is kept as a hint.
    #[allow(clippy::too_many_arguments)]
    pub fn import_bytes(
        &self,
        name: &str,
        wav: &[u8],
        label_speakers: bool,
        patient: Option<PatientInfo>,
        consent: Option<Consent>,
        by: Option<&str>,
        events: &dyn Events,
    ) -> Result<SessionView> {
        if wav.len() < 16 {
            return Err(AppError("That file is empty. Choose the recording again.".into()));
        }
        // Decoded from memory: the upload never touches the disk unencrypted.
        let file = Path::new(name).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let name = if file.is_empty() { "recording.wav".to_string() } else { file };
        let source = LiveSource::Memory { name, bytes: wav.to_vec() };
        self.import_source(source, label_speakers, patient, consent, by, events)
    }

    // ---- live recording --------------------------------------------------

    pub fn input_devices(&self) -> Vec<String> {
        os_asr::capture::input_devices()
    }

    /// Start recording from a microphone on this machine (`Some(device)`/`None`
    /// = default) or from blocks the caller pushes (`stream`). Consent is the
    /// clinic's responsibility (the app shows a one-time notice); when the
    /// per-recording consent step is used, the consent is checked and saved.
    pub fn start_recording(
        &self,
        source: RecordingSource,
        label_speakers: bool,
        patient: Option<PatientInfo>,
        consent: Option<Consent>,
        by: Option<&str>,
        events: Arc<dyn Events>,
    ) -> Result<String> {
        if let Some(c) = &consent {
            c.check()?;
        }
        let recorded_by = self.who(by, Need::Any)?;
        let work = self.begin_work()?;
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
        options.patient = patient.filter(|p| !p.is_empty());
        options.consent = consent;
        options.recorded_by = recorded_by;
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
        *self.recording_work.lock().map_err(|_| AppError("recorder lock poisoned".into()))? = Some(work);
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
        // The recording's ticket ends once its transcript is saved and linked.
        let _work = self.recording_work.lock().map_err(|_| AppError("recorder lock poisoned".into()))?.take();
        let result = recorder.stop()?;
        let mut s = Session::open(&result.session_dir)?;
        self.link_visit(&mut s)?;
        Ok(view(&s, Some(result.transcript)))
    }

    /// Set or change the patient details of a saved encounter (time-stamped).
    pub fn update_patient(&self, id: &str, patient: PatientInfo) -> Result<SessionView> {
        let _work = self.begin_work()?;
        let dir = self.session_dir(id)?;
        let recording_here = self
            .recorder
            .lock()
            .map(|r| r.as_ref().is_some_and(|rec| rec.session_dir == dir))
            .unwrap_or(false);
        if recording_here {
            return Err(AppError("This encounter is still recording. Stop the recording first.".into()));
        }
        let mut s = Session::open(&dir)?;
        let mut patient = patient;
        // A different animal or family: link the visit afresh (ADR-0011).
        if let Some(prev) = &s.meta.patient {
            let key = |name: &str, last: &str| os_engine::patients::match_key(name, last);
            if key(&prev.name, &prev.last_name) != key(&patient.name, &patient.last_name) {
                patient.patient_id = None;
            }
            if prev.others != patient.others {
                patient.other_ids.clear();
            }
        }
        s.set_patient(patient)?;
        self.link_visit(&mut s)?;
        Ok(view(&s, None))
    }

    // ---- notes -----------------------------------------------------------

    /// Draft a note (or a client document) with a template. `language`: write
    /// the note in this language ("fr"…); the template's language when None.
    /// `pet`: the animal this note is about, in a multi-pet visit.
    #[allow(clippy::too_many_arguments)]
    pub fn generate_note(
        &self,
        id: &str,
        llm: &str,
        template: &str,
        language: Option<&str>,
        pet: Option<&str>,
        by: Option<&str>,
        events: &dyn Events,
    ) -> Result<NoteResponse> {
        let _work = self.begin_work()?;
        let who = self.who(by, Need::Notes)?;
        let _awake = AwakeGuard::acquire(AwakeReason::Processing);
        let mut s = self.open(id, pet)?;
        let template = self.templates.load(if template.is_empty() { "soap" } else { template })?;
        // Headings for other species are left out (M2 item 4).
        let template = template.for_species(Self::species_of(&s).as_deref());
        let record = template.template.document.is_record();
        if record && s.is_signed() {
            return Err(AppError(
                "This note is signed, so it can't be redrafted. Add an addendum instead.".into(),
            ));
        }
        // Client documents follow the vet's note (with their corrections).
        let reference = if record {
            None
        } else {
            s.latest_record().and_then(|run| {
                let dir = s.dir.join(&run.dir);
                let saved: serde_json::Value =
                    serde_json::from_str(&os_engine::vault::read_to_string(&dir.join("note.json")).ok()?)
                        .ok()?;
                let markdown = os_engine::vault::read_to_string(&dir.join("note.md")).ok()?;
                Some(os_engine::note::Reference::from_note(&saved["note"], &markdown))
            })
        };
        let backend = self.load_llm_with_progress(llm, events)?;
        let transcript = s.load_transcript()?;
        let options = NoteOptions { reference, ..self.note_options(&s, language.map(String::from)) };
        // The drafting board (Review): reading, then writing from the first word.
        events.progress(&Progress::new(Stage::ReadTranscript, 0.0, transcript.segments.len() as f32));
        let mut first = true;
        let mut result =
            os_engine::generate_note(backend.as_ref(), &template, &transcript, &options, &mut |p| {
                if std::mem::take(&mut first) {
                    events.progress(&Progress::new(Stage::Write, 0.0, 0.0));
                }
                events.token(p);
                true
            })?;
        events.progress(&Progress::new(Stage::Verify, 1.0, 1.0));
        result.by = who;
        result.markdown = with_header(&s, &result.markdown);
        events.progress(&Progress::new(Stage::Save, 0.0, 1.0));
        s.save_run(llm, &result, &template)?;
        Ok(NoteResponse { markdown: result.markdown.clone(), run: serde_json::to_value(&result)? })
    }

    /// Re-prompt one heading of the latest draft ("plan/medications", or a
    /// whole section such as "plan"). The redraft is saved as a new run; the
    /// rest of the note is unchanged. With `save_as_standard`, the request is
    /// also saved against that heading of the template, so later drafts
    /// follow it without being asked.
    pub fn refine_note(
        &self,
        id: &str,
        request: &RefineRequest,
        events: &dyn Events,
    ) -> Result<NoteResponse> {
        let _work = self.begin_work()?;
        let who = self.who(request.by.as_deref(), Need::Notes)?;
        let mut s = self.open(id, request.pet.as_deref())?;
        if s.is_signed() {
            return Err(AppError(
                "This note is signed, so it can't be changed. Add an addendum instead.".into(),
            ));
        }
        let (run, note) = s.latest_note()?;
        let template = s.run_template(&run)?;
        let title = template.heading_title(&request.heading).ok_or_else(|| {
            AppError(format!("{} has no heading called {}.", template.template.name, request.heading))
        })?;
        if request.request.trim().is_empty() {
            return Err(AppError(format!("Say what should change under {title}.")));
        }
        if request.save_as_standard {
            // Fail before drafting if the standard can't be saved.
            self.templates.load(&template.template.id).map_err(|_| {
                AppError(format!(
                    "{} is no longer in your templates, so the request can't be saved as a standard.",
                    template.template.name
                ))
            })?;
        }
        let _awake = AwakeGuard::acquire(AwakeReason::Processing);
        let backend = self.load_llm_with_progress(&request.llm, events)?;
        let transcript = s.load_transcript()?;
        let options = self.note_options(&s, request.language.clone());
        events.progress(&Progress::new(Stage::ReadTranscript, 0.0, transcript.segments.len() as f32));
        let mut first = true;
        let mut result = os_engine::refine_note(
            backend.as_ref(),
            &template,
            &transcript,
            &note,
            &request.heading,
            &request.request,
            &options,
            &mut |p| {
                if std::mem::take(&mut first) {
                    events.progress(&Progress::new(Stage::Write, 0.0, 0.0));
                }
                events.token(p);
                true
            },
        )?;
        events.progress(&Progress::new(Stage::Verify, 1.0, 1.0));
        if request.save_as_standard {
            self.templates.add_standard(
                &template.template.id,
                &request.heading,
                &request.request,
                who.as_ref().map(|w| w.name.as_str()),
            )?;
        }
        if let Some(r) = result.refinement.as_mut() {
            r.from_run = Some(run.dir.clone());
            r.saved_as_standard = request.save_as_standard;
        }
        result.by = who;
        result.markdown = with_header(&s, &result.markdown);
        s.save_run(&request.llm, &result, &template)?;
        Ok(NoteResponse { markdown: result.markdown.clone(), run: serde_json::to_value(&result)? })
    }

    /// Save a veterinarian's own version of one heading as a new draft.
    /// Changed items are marked as entered by them; the earlier draft is kept.
    pub fn edit_note(
        &self,
        id: &str,
        heading: &str,
        value: serde_json::Value,
        by: &str,
        pet: Option<&str>,
    ) -> Result<NoteResponse> {
        let _work = self.begin_work()?;
        let who = self
            .who(Some(by), Need::Notes)?
            .ok_or_else(|| AppError("Add your name in Settings, then save your edits.".into()))?;
        let mut s = self.open(id, pet)?;
        if s.is_signed() {
            return Err(AppError(
                "This note is signed, so it can't be changed. Add an addendum instead.".into(),
            ));
        }
        let (run, note) = s.latest_note()?;
        let template = s.run_template(&run)?;
        let transcript = s.load_transcript()?;
        let mut result =
            os_engine::edit::apply_edit(&template, &transcript, &note, heading, value, &who.name, &run.llm)?;
        result.by = Some(who);
        if let Some(e) = result.edit.as_mut() {
            e.from_run = Some(run.dir.clone());
        }
        result.markdown = with_header(&s, &result.markdown);
        s.save_run("edited", &result, &template)?;
        Ok(NoteResponse { markdown: result.markdown.clone(), run: serde_json::to_value(&result)? })
    }

    /// Add a signed addendum to a signed note.
    pub fn add_addendum(&self, id: &str, text: &str, by: &str, pet: Option<&str>) -> Result<SessionView> {
        let _work = self.begin_work()?;
        let who = self
            .who(Some(by), Need::Sign)?
            .ok_or_else(|| AppError("Add your name in Settings, then sign the addendum.".into()))?;
        let mut s = self.open(id, pet)?;
        s.add_addendum_by(text, &who)?;
        Ok(view(&s, None))
    }

    /// The note (or a client document) as a PDF, with the clinic letterhead.
    /// Returns the bytes and a suggested file name.
    pub fn export_pdf(&self, id: &str, request: &PdfRequest) -> Result<(Vec<u8>, String)> {
        let s = self.open(id, request.pet.as_deref())?;
        let run = match &request.document {
            Some(doc) => s.my_runs().filter(|r| r.template_id() == doc).last(),
            None => s.latest_record(),
        }
        .ok_or_else(|| AppError("There's no draft to export yet. Draft the note first.".into()))?;
        let template = s.run_template(run)?;
        let path = s.dir.join(&run.dir).join("note.json");
        let saved: serde_json::Value = serde_json::from_str(&os_engine::vault::read_to_string(&path)?)?;
        let labels = os_engine::pdf::Labels::for_language(request.language.as_deref().unwrap_or("en"));
        let mut details = Vec::new();
        if let Some(p) = &s.meta.patient {
            if !p.display_name().is_empty() {
                details.push(format!("{}: {}", labels.patient, p.display_name()));
            }
            if !p.reason.is_empty() {
                details.push(format!("{}: {}", labels.reason, p.reason));
            }
        }
        details.push(format!("{}: {}", labels.recorded, s.meta.created_at.format("%Y-%m-%d %H:%M")));
        let record = run.document.is_record();
        let header = os_engine::pdf::PdfHeader {
            clinic: request.clinic.clone(),
            details,
            signed: run.signed_at.filter(|_| record).map(|at| {
                labels.signed(
                    &run.signed_by.clone().unwrap_or_default(),
                    &at.format("%Y-%m-%d %H:%M").to_string(),
                )
            }),
            addenda: if record {
                run.addenda
                    .iter()
                    .map(|a| {
                        let heading =
                            format!("{}, {}, {}", labels.addendum, a.at.format("%Y-%m-%d %H:%M"), a.by);
                        (heading, a.text.clone())
                    })
                    .collect()
            } else {
                Vec::new()
            },
            page: request.page,
            labels,
        };
        let pdf = os_engine::pdf::note_pdf(&template, &saved["note"], &header);
        let who = s.meta.patient.as_ref().map(|p| p.display_name()).filter(|n| !n.is_empty());
        let stem = format!(
            "{} {} {}",
            s.meta.created_at.format("%Y-%m-%d"),
            who.unwrap_or_default(),
            template.template.name
        );
        let name: String = stem
            .split_whitespace()
            .collect::<Vec<_>>()
            .join("-")
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '-')
            .collect();
        Ok((pdf, format!("{name}.pdf")))
    }

    /// Mark the latest draft as signed by `by` (refused while numbers are unsupported).
    pub fn sign(&self, id: &str, by: &str, pet: Option<&str>) -> Result<SessionView> {
        let _work = self.begin_work()?;
        let who = self
            .who(Some(by), Need::Sign)?
            .ok_or_else(|| AppError("Enter the name of the veterinarian signing the note.".into()))?;
        let mut s = self.open(id, pet)?;
        s.sign_latest_by(&who)?;
        Ok(view(&s, None))
    }

    // ---- templates -------------------------------------------------------

    pub fn list_templates(&self) -> Vec<TemplateSummary> {
        self.templates.list()
    }

    /// A template's headings, standards and the prompt it produces.
    pub fn template(&self, id: &str) -> Result<TemplateView> {
        Ok(TemplateView::of(self.templates.load(id)?))
    }

    /// Check an unsaved template and show what the model would be told.
    pub fn preview_template(&self, template: Template) -> Result<TemplateView> {
        let mut t = LoadedTemplate::new(template)?;
        if let Ok(saved) = self.templates.load(&t.template.id) {
            t.template.standards = saved.template.standards;
        }
        Ok(TemplateView::of(t))
    }

    /// Propose a template from a note the clinic already writes (M2 item 5).
    /// Nothing is saved: the proposal opens in the builder for editing.
    pub fn template_from_example(&self, llm: &str, example: &str) -> Result<TemplateView> {
        let _awake = AwakeGuard::acquire(AwakeReason::Processing);
        let backend = self.load_llm(llm)?;
        let t = os_engine::from_example::template_from_example(backend.as_ref(), example)?;
        let prompt = os_engine::note::system_prompt(&t);
        // A new template: its id comes from the name the team saves it under.
        let template = Template { id: String::new(), ..t.template };
        Ok(TemplateView { template, builtin: false, prompt })
    }

    /// Save a team template (new, or a change that bumps its version).
    pub fn save_template(&self, template: Template) -> Result<TemplateView> {
        Ok(TemplateView::of(self.templates.save(template)?))
    }

    pub fn delete_template(&self, id: &str) -> Result<()> {
        Ok(self.templates.delete(id)?)
    }

    /// Save a request as a standard for a heading ("" = the whole note).
    pub fn add_standard(&self, id: &str, heading: &str, text: &str, by: Option<&str>) -> Result<Standard> {
        Ok(self.templates.add_standard(id, heading, text, by)?)
    }

    pub fn remove_standard(&self, id: &str, standard_id: &str) -> Result<TemplateView> {
        self.templates.remove_standard(id, standard_id)?;
        self.template(id)
    }

    /// [`App::load_llm`], telling the drafting board when the model has to be
    /// loaded first (it stays loaded for the next draft).
    fn load_llm_with_progress(&self, id: &str, events: &dyn Events) -> Result<Arc<LlamaCppBackend>> {
        let loaded = self.llm.lock().map(|slot| slot.as_ref().is_some_and(|(l, _)| l == id)).unwrap_or(false);
        if !loaded {
            events.progress(&Progress::new(Stage::LoadModel, 0.0, 1.0));
        }
        self.load_llm(id)
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

    // ---- storage (ADR-0013) ----------------------------------------------

    fn folder(&self, kind: FolderKind) -> PathBuf {
        match kind {
            FolderKind::Models => self.store.root().to_path_buf(),
            FolderKind::Visits => self.sessions_root.clone(),
        }
    }

    fn folder_view(&self, kind: FolderKind) -> FolderView {
        let path = self.folder(kind);
        let (resolved, mut source) = locations::resolve(kind);
        let mut fixed_by = (source == FolderSource::Fixed).then(|| locations::env_var(kind).to_string());
        // Started with a command-line folder (openbark serve --models-dir ...).
        if resolved != path {
            source = FolderSource::Fixed;
            fixed_by.get_or_insert_with(|| "the command line".to_string());
        }
        FolderView {
            kind,
            path: locations::plain(&path).display().to_string(),
            source,
            fixed_by,
            used_bytes: locations::dir_size(&path),
            free_bytes: locations::free_space(&path),
        }
    }

    pub fn storage(&self) -> StorageView {
        StorageView {
            models: self.folder_view(FolderKind::Models),
            visits: self.folder_view(FolderKind::Visits),
        }
    }

    /// Look at a folder the clinic chose, without changing anything (except a
    /// quick write test, cleaned up after).
    /// `from_browser`: web mode, where network paths are refused (a web page
    /// mustn't be able to make OpenBark open a network share).
    pub fn check_folder(&self, kind: FolderKind, chosen: &str, from_browser: bool) -> FolderCheck {
        let chosen_path = PathBuf::from(chosen.trim());
        let current = self.folder(kind);
        let other = self.folder(match kind {
            FolderKind::Models => FolderKind::Visits,
            FolderKind::Visits => FolderKind::Models,
        });
        let ids: Vec<String> = hub::full_catalog(&self.store).models.into_iter().map(|m| m.id).collect();
        let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
        let target = match locations::contents(kind, &chosen_path, &ids) {
            Contents::Other => chosen_path.join(kind.subfolder()),
            _ => chosen_path.clone(),
        };
        let has_data = locations::contents(kind, &target, &ids) == Contents::OpenBark;
        let needed_bytes = locations::dir_size(&current);
        let free_bytes = locations::free_space(&target);
        let same_disk = matches!(
            (locations::disk_of(&target), locations::disk_of(&current)),
            (Some((a, _)), Some((b, _))) if a == b
        );
        let view = self.folder_view(kind);
        let app_data = locations::app_data_dir();
        let problem = if let Some(by) = &view.fixed_by {
            Some(FolderProblem::Fixed { by: by.clone() })
        } else if !chosen_path.is_absolute() {
            Some(FolderProblem::NotAbsolute)
        } else if from_browser && locations::is_network_path(&chosen_path) {
            Some(FolderProblem::NetworkPath)
        } else if locations::overlaps(&target, &current)
            && locations::plain(&target) == locations::plain(&current)
        {
            Some(FolderProblem::SameFolder)
        } else if locations::overlaps(&target, &current) {
            Some(FolderProblem::AroundCurrent)
        } else if locations::overlaps(&target, &other) {
            Some(FolderProblem::OverlapsOther)
        } else if locations::overlaps(&target, &app_data)
            && locations::plain(&target) != locations::plain(&locations::default_path(kind))
        {
            Some(FolderProblem::InsideAppData)
        } else if writable(&target).is_err() {
            Some(FolderProblem::NotWritable)
        } else {
            None
        };
        let mut notices = Vec::new();
        if let Some(service) = locations::synced_by(&target) {
            notices.push(FolderNotice::Synced { service: service.to_string() });
        }
        let s = target.to_string_lossy();
        if locations::is_network_path(&target)
            || s.starts_with("/Volumes/")
            || s.starts_with("/mnt/")
            || s.starts_with("/media/")
        {
            notices.push(FolderNotice::NetworkDrive);
        } else if let Some((_, true)) = locations::disk_of(&target) {
            notices.push(FolderNotice::Removable);
        }
        let can_move = same_disk || free_bytes.is_none_or(|free| free > needed_bytes + (512 << 20));
        FolderCheck {
            kind,
            chosen: locations::plain(&chosen_path).display().to_string(),
            target: locations::plain(&target).display().to_string(),
            has_data,
            problem,
            notices,
            needed_bytes,
            free_bytes,
            same_disk,
            can_move,
        }
    }

    /// Use a new folder for models or visits, moving what's there when asked.
    /// Saved in `locations.json`; the caller then starts a fresh [`App`] on the
    /// new folders (web mode) or restarts (desktop).
    pub fn change_folder(
        &self,
        kind: FolderKind,
        chosen: &str,
        mode: ChangeMode,
        from_browser: bool,
        events: &dyn Events,
    ) -> Result<FolderChanged> {
        use std::sync::atomic::Ordering::SeqCst;
        let check = self.check_folder(kind, chosen, from_browser);
        if let Some(problem) = check.problem {
            return Err(AppError(problem.message()));
        }
        // One change at a time, and none while anything is writing.
        if self.gate.retired.load(SeqCst) {
            return Err(AppError(
                "OpenBark has just changed where it keeps its files. Try again in a moment.".into(),
            ));
        }
        if self.gate.changing.compare_exchange(false, true, SeqCst, SeqCst).is_err() {
            return Err(AppError("Another folder change is already under way.".into()));
        }
        struct EndChange<'a>(&'a Gate);
        impl Drop for EndChange<'_> {
            fn drop(&mut self) {
                self.0.changing.store(false, std::sync::atomic::Ordering::SeqCst);
            }
        }
        let _changing = EndChange(&self.gate);
        // Look again now that this change holds the gate: one that finished in
        // between has retired this App.
        if self.gate.retired.load(SeqCst) {
            return Err(AppError(
                "OpenBark has just changed where it keeps its files. Try again in a moment.".into(),
            ));
        }
        if self.is_recording() {
            return Err(AppError("Stop the recording first, then change the folder.".into()));
        }
        if self.is_busy() {
            return Err(AppError(
                "OpenBark is importing, drafting, saving or downloading. Change the folder when that's finished.".into(),
            ));
        }
        let target = PathBuf::from(&check.target);
        let moved = match mode {
            ChangeMode::Move => {
                if !check.can_move {
                    return Err(AppError(format!(
                        "There isn't room on that drive to move {} there. Free some space, or choose another folder.",
                        os_models::locations::plain(&self.folder(kind)).display()
                    )));
                }
                // The note model is memory-mapped while loaded; free it so its file can move.
                if kind == FolderKind::Models {
                    *self.llm.lock().map_err(|_| AppError("model lock poisoned".into()))? = None;
                }
                let from = self.folder(kind);
                let moved =
                    locations::move_tree(&from, &target, kind == FolderKind::Models, |done, total| {
                        events.moving(done, total)
                    })
                    .map_err(|e| AppError(format!("Nothing was moved: {e}.")))?;
                Some(moved)
            }
            ChangeMode::Switch => {
                std::fs::create_dir_all(&target)?;
                None
            }
        };
        let mut saved = locations::load();
        saved.set(kind, Some(target.clone()));
        if let Err(e) = locations::save(&saved) {
            // The files moved but the choice couldn't be saved: put them back,
            // so OpenBark and its files agree.
            if moved.is_some() {
                let from = self.folder(kind);
                let back = locations::move_tree(&target, &from, kind == FolderKind::Models, |_, _| {});
                return Err(AppError(match back {
                    Ok(_) => format!("The folder couldn't be changed, so everything was moved back: {e}."),
                    Err(b) => format!(
                        "The choice couldn't be saved ({e}) and moving back failed ({b}). Your files are in {}.",
                        locations::plain(&target).display()
                    ),
                }));
            }
            return Err(AppError(format!("The folder couldn't be changed: {e}.")));
        }
        // This App's folders are now wrong: refuse new work until it's replaced.
        self.gate.retired.store(true, SeqCst);
        Ok(FolderChanged { kind, path: check.target, moved })
    }

    // ---- models ----------------------------------------------------------

    pub fn models(&self) -> Vec<ModelRow> {
        let builtin: Vec<String> = os_models::Catalog::builtin().models.into_iter().map(|m| m.id).collect();
        hub::full_catalog(&self.store)
            .models
            .into_iter()
            // The GPU recognisers come with the GPU speech pack, not one by one.
            .filter(|m| m.id != os_engine::models::GPU_STT && m.id != os_engine::models::GPU_SECOND_STT)
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
        let _work = self.begin_work()?;
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

    // ---- GPU speech pack (ADR-0014) ---------------------------------------

    fn gpu_models(&self) -> Vec<CatalogEntry> {
        let catalog = os_models::Catalog::builtin();
        [os_engine::models::GPU_STT, os_engine::models::GPU_SECOND_STT]
            .iter()
            .filter_map(|id| catalog.get(id).cloned())
            .collect()
    }

    /// Start a piece of writing work, or refuse while the folders are changing.
    pub fn begin_work(&self) -> Result<Busy> {
        use std::sync::atomic::Ordering::SeqCst;
        // Count first, then look: a change that starts after this sees the count.
        self.gate.busy.fetch_add(1, SeqCst);
        let ticket = Busy(self.gate.clone());
        if self.gate.retired.load(SeqCst) {
            return Err(AppError(
                "OpenBark has just changed where it keeps its files. Try again in a moment.".into(),
            ));
        }
        if self.gate.changing.load(SeqCst) {
            return Err(AppError(
                "OpenBark is changing where it keeps its files. Try again when that's finished.".into(),
            ));
        }
        Ok(ticket)
    }

    /// Importing, drafting, saving, downloading or recording right now.
    pub fn is_busy(&self) -> bool {
        self.gate.busy.load(std::sync::atomic::Ordering::SeqCst) > 0
    }

    /// A live recording is running.
    pub fn is_recording(&self) -> bool {
        self.recorder.lock().map(|r| r.is_some()).unwrap_or(true)
    }

    pub fn gpu_status(&self) -> GpuStatus {
        let gpu = os_models::gpu_pack::nvidia_gpu();
        let nvidia = gpu.is_some();
        let pack_installed = os_models::gpu_pack::installed();
        let checked = self.gpu_checked.load(std::sync::atomic::Ordering::SeqCst);
        let models = self.gpu_models();
        let models_installed = models.iter().all(|m| self.store.is_installed(m, Verify::Size));
        let missing = |m: &CatalogEntry| {
            m.files.iter().filter(|f| !self.store.file_ok(m, f, Verify::Size)).map(|f| f.size).sum::<u64>()
        };
        let mut download_bytes: u64 = models.iter().map(missing).sum();
        if !pack_installed {
            download_bytes += os_models::gpu_pack::entry().total_size();
        }
        GpuStatus {
            supported: os_models::gpu_pack::supported() && nvidia,
            gpu,
            pack_installed,
            models_installed,
            active: self.gpu_at_start && pack_installed && models_installed && os_asr::gpu_usable(),
            installing: GPU_INSTALLING.load(std::sync::atomic::Ordering::SeqCst),
            restart_needed: pack_installed && models_installed && !self.gpu_at_start && checked != 2,
            check_failed: checked == 2,
            download_bytes,
        }
    }

    /// Download and set up the GPU speech pack and the full-precision
    /// recognisers. Progress goes out as one download, `gpu-speech-pack`.
    pub fn install_gpu(&self, accept_license: bool, events: &dyn Events) -> Result<GpuStatus> {
        use std::sync::atomic::Ordering::SeqCst;
        if !accept_license {
            return Err(AppError("Accept NVIDIA's licence for its libraries first.".into()));
        }
        if !self.gpu_status().supported {
            return Err(AppError(
                "Speech recognition on the GPU needs Windows and an NVIDIA graphics card.".into(),
            ));
        }
        // One install at a time, across tabs and windows.
        if GPU_INSTALLING.compare_exchange(false, true, SeqCst, SeqCst).is_err() {
            return Err(AppError("The GPU speech pack is already downloading.".into()));
        }
        struct Done;
        impl Drop for Done {
            fn drop(&mut self) {
                GPU_INSTALLING.store(false, std::sync::atomic::Ordering::SeqCst);
            }
        }
        let _done = Done;
        let _work = self.begin_work()?;
        let _awake = AwakeGuard::acquire(AwakeReason::Processing);
        let mut entries = vec![os_models::gpu_pack::entry()];
        entries.extend(self.gpu_models());
        let total: u64 = entries.iter().map(|e| e.total_size()).sum();
        let mut before = 0u64;
        for entry in &entries {
            if entry.id == os_models::gpu_pack::PACK_ID && os_models::gpu_pack::installed() {
                before += entry.total_size();
                continue;
            }
            let mut current = String::new();
            let mut done_before_file = 0u64;
            let mut file_done = 0u64;
            os_models::download(&self.store, entry, None, &mut |file: &str, done: u64, _t: u64| {
                if file != current {
                    done_before_file += file_done;
                    current = file.to_string();
                }
                file_done = done;
                events.download(os_models::gpu_pack::PACK_ID, before + done_before_file + done, total);
            })?;
            if entry.id == os_models::gpu_pack::PACK_ID {
                os_models::gpu_pack::unpack(&self.store, |_, _| {})
                    .map_err(|e| AppError(format!("The GPU speech pack couldn't be set up: {e}.")))?;
            }
            before += entry.total_size();
        }
        // Test it now, in a child process that loads the new libraries; this
        // process uses them from its next start. A new attempt tests afresh.
        if let Some(f) = os_models::gpu_pack::check_file() {
            let _ = std::fs::remove_file(f);
        }
        let ok = verify_gpu(&self.store);
        self.gpu_checked.store(if ok { 1 } else { 2 }, SeqCst);
        // Already running on the pack's libraries (a reinstall or a retry):
        // use the GPU from now on. Otherwise from the next start.
        if self.gpu_at_start {
            os_asr::set_gpu_usable(ok);
        }
        Ok(self.gpu_status())
    }

    /// Take the GPU speech pack and the full-precision recognisers away again.
    pub fn remove_gpu(&self) -> Result<GpuStatus> {
        let _work = self.begin_work()?;
        os_asr::set_gpu_usable(false);
        self.gpu_checked.store(0, std::sync::atomic::Ordering::SeqCst);
        os_models::gpu_pack::remove()
            .map_err(|e| AppError(format!("The GPU speech pack couldn't be removed: {e}.")))?;
        for m in self.gpu_models() {
            let _ = std::fs::remove_dir_all(self.store.model_dir(&m));
        }
        Ok(self.gpu_status())
    }

    pub fn hub_search(&self, query: &str) -> Result<Vec<HubModel>> {
        Ok(hub::search(query, 30, None, platform() == "macos")?)
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
        let _work = self.begin_work()?;
        let r = hub::repo(repo, None)?;
        let entry = hub::add_to_catalog(&self.store, &r, file, accept_license)?;
        Ok(self.row(&entry, false))
    }
}

/// The operating system, as the UI should treat it. `OPENBARK_PLATFORM`
/// overrides it for demos and screenshots of another platform's screens.
fn platform() -> String {
    std::env::var("OPENBARK_PLATFORM")
        .ok()
        .filter(|p| matches!(p.as_str(), "macos" | "windows" | "linux"))
        .unwrap_or_else(|| std::env::consts::OS.to_string())
}

/// What an action needs from the person doing it (M1 item 11).
#[derive(Clone, Copy)]
enum Need {
    Any,
    Notes,
    Sign,
    Team,
}

/// What to put in a PDF export.
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct PdfRequest {
    #[serde(default)]
    pub clinic: os_engine::pdf::Clinic,
    #[serde(default)]
    pub page: os_engine::pdf::PageSize,
    /// A client document's template id; the medical record when absent.
    #[serde(default)]
    pub document: Option<String>,
    /// Language for the PDF's own words ("Patient", "Page 1 of 2").
    #[serde(default)]
    pub language: Option<String>,
    /// The animal, in a multi-pet visit.
    #[serde(default)]
    pub pet: Option<String>,
}

/// A re-prompt of one heading of the latest draft.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct RefineRequest {
    pub llm: String,
    /// "plan/medications", or a whole section: "plan".
    pub heading: String,
    pub request: String,
    #[serde(default)]
    pub save_as_standard: bool,
    /// Language for the redraft ("fr"…); the template's when None.
    #[serde(default)]
    pub language: Option<String>,
    /// Who asked (for the standard's record).
    #[serde(default)]
    pub by: Option<String>,
    /// The animal, in a multi-pet visit.
    #[serde(default)]
    pub pet: Option<String>,
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

/// What an import's worker reports to the thread that started it.
enum Live {
    Segment(Segment),
    Progress(Progress),
}

/// Sends live segments and progress to the thread that started an import.
struct ChannelSink(std::sync::mpsc::Sender<Live>);

impl LiveSink for ChannelSink {
    fn level(&mut self, _level: f32, _seconds: f32) {}
    fn segment(&mut self, segment: &Segment) {
        let _ = self.0.send(Live::Segment(segment.clone()));
    }
    fn progress(&mut self, progress: &Progress) {
        let _ = self.0.send(Live::Progress(progress.clone()));
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
    fn progress(&mut self, progress: &Progress) {
        self.0.progress(progress);
    }
}

/// Make `dir` (and its parents) and write a test file there. What the test
/// created is removed again.
fn writable(dir: &Path) -> std::result::Result<(), String> {
    let created: Vec<PathBuf> = dir.ancestors().take_while(|p| !p.exists()).map(Path::to_path_buf).collect();
    let probe = dir.join(".openbark-write-test");
    let ok = std::fs::create_dir_all(dir).and_then(|_| std::fs::write(&probe, b"ok")).is_ok();
    let _ = std::fs::remove_file(&probe);
    for p in &created {
        let _ = std::fs::remove_dir(p);
    }
    if ok {
        Ok(())
    } else {
        Err("OpenBark can't write to that folder. Choose another, or ask whoever manages this computer for access.".into())
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

/// Visit header for copied notes: patient, reason and when it was recorded.
fn with_header(s: &Session, markdown: &str) -> String {
    let mut header = String::new();
    if let Some(p) = &s.meta.patient {
        if !p.display_name().is_empty() {
            header.push_str(&format!(
                "Patient: {}
",
                p.display_name()
            ));
        }
        if !p.reason.is_empty() {
            header.push_str(&format!(
                "Reason for visit: {}
",
                p.reason
            ));
        }
    }
    header.push_str(&format!(
        "Recorded: {}

",
        s.meta.created_at.format("%Y-%m-%d %H:%M")
    ));
    format!("{header}{markdown}")
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
    let record = s.latest_record();
    let latest = record.map(|r| s.dir.join(&r.dir));
    let latest_template = record.and_then(|r| s.run_template(r).ok()).map(|t| t.template);
    let addenda = record.map(|r| r.addenda.clone()).unwrap_or_default();
    // The latest run of each client document, in the order first drafted.
    let mut documents: Vec<DocumentView> = Vec::new();
    for run in s.my_runs().filter(|r| !r.document.is_record()) {
        let dir = s.dir.join(&run.dir);
        let template = s.run_template(run).ok().map(|t| t.template);
        let doc = DocumentView {
            template_id: run.template_id().to_string(),
            name: template.as_ref().map(|t| t.name.clone()).unwrap_or_else(|| run.template_id().to_string()),
            created_at: run.created_at.format("%Y-%m-%d %H:%M").to_string(),
            run: os_engine::vault::read_to_string(&dir.join("note.json"))
                .ok()
                .and_then(|t| serde_json::from_str(&t).ok()),
            markdown: os_engine::vault::read_to_string(&dir.join("note.md")).ok(),
            template,
        };
        match documents.iter_mut().find(|d| d.template_id == doc.template_id) {
            Some(slot) => *slot = doc,
            None => documents.push(doc),
        }
    }
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
            .and_then(|d| os_engine::vault::read_to_string(&d.join("note.json")).ok())
            .and_then(|t| serde_json::from_str(&t).ok()),
        latest_markdown: latest
            .and_then(|d| os_engine::vault::read_to_string(&d.join("note.md")).ok())
            .map(|md| with_addenda(md, &addenda)),
        signed_at: record.and_then(|r| r.signed_at).map(|t| t.format("%Y-%m-%d %H:%M").to_string()),
        signed_by: record.and_then(|r| r.signed_by.clone()),
        patient: s.meta.patient.clone(),
        latest_template,
        addenda,
        consent: s.meta.consent.clone(),
        documents,
        recorded_by: s.meta.recorded_by.clone(),
        pets: s.meta.patient.as_ref().map(|p| p.pets()).filter(|p| p.len() > 1).unwrap_or_default(),
        pet: s.pet.clone(),
    }
}

/// The signed note followed by its addenda, for copying out.
fn with_addenda(mut markdown: String, addenda: &[Addendum]) -> String {
    for a in addenda {
        markdown.push_str(&format!(
            "\n## Addendum, {}, {}\n\n{}\n",
            a.at.format("%Y-%m-%d %H:%M"),
            a.by,
            a.text.trim()
        ));
    }
    markdown
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visits_group_under_patients_and_stay_linked_after_a_rename() {
        // A throwaway key, so the test never touches this computer's own.
        // SAFETY: the only test in this crate that touches the environment.
        let tmp = tempfile::tempdir().unwrap();
        unsafe {
            std::env::set_var("OPENBARK_DATA_KEY", "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=");
            // Folder changes below save their choice here, never in the real locations.json.
            std::env::set_var("OPENBARK_LOCATIONS_FILE", tmp.path().join("locations.json"));
        }
        let root = tmp.path().join("sessions");
        // Chosen folders, as Settings would save them (so they aren't "fixed").
        let mut chosen = os_models::locations::Locations::default();
        chosen.set(FolderKind::Visits, Some(root.clone()));
        chosen.set(FolderKind::Models, Some(tmp.path().join("models")));
        os_models::locations::save(&chosen).unwrap();
        let app = App::new(ModelStore::new(tmp.path().join("models")), root.clone());
        // A real (silent) 16 kHz mono WAV, a tenth of a second long.
        let audio = tmp.path().join("visit.wav");
        let data = vec![0u8; 3200];
        let mut wav = Vec::new();
        wav.extend(b"RIFF");
        wav.extend((36 + data.len() as u32).to_le_bytes());
        wav.extend(b"WAVEfmt ");
        wav.extend(16u32.to_le_bytes());
        wav.extend(1u16.to_le_bytes());
        wav.extend(1u16.to_le_bytes());
        wav.extend(16_000u32.to_le_bytes());
        wav.extend(32_000u32.to_le_bytes());
        wav.extend(2u16.to_le_bytes());
        wav.extend(16u16.to_le_bytes());
        wav.extend(b"data");
        wav.extend((data.len() as u32).to_le_bytes());
        wav.extend(data);
        std::fs::write(&audio, wav).unwrap();
        let visit = |p: Patient| {
            let mut s = Session::create_from_file(&root, &audio, 1.0).unwrap();
            s.set_patient(p).unwrap();
        };
        let named = |name: &str, last: &str| Patient {
            name: name.into(),
            last_name: last.into(),
            ..Default::default()
        };
        visit(Patient { species: "canine".into(), ..named("Maple", "Okonkwo") });
        visit(named("maple ", "OKONKWO"));
        visit(Patient { others: vec!["Pickles".into()], ..named("Olive", "Haddad") });

        let list = app.list_patients().unwrap();
        let find =
            |name: &str| list.iter().find(|p| p.patient.name.eq_ignore_ascii_case(name)).unwrap().clone();
        assert_eq!(list.len(), 3, "{list:?}");
        assert_eq!(find("Maple").visits, 2, "the same animal, spelt two ways");
        assert_eq!(find("Maple").patient.species, "canine");
        assert_eq!(find("Pickles").visits, 1, "each animal in a multi-pet visit has its own patient");

        // Visits are now linked by id: renaming the patient keeps them.
        let maple = find("Maple").patient;
        app.update_patient_record(&maple.id, "Maple", "Okonkwo-Reid", "canine").unwrap();
        assert_eq!(app.patient(&maple.id).unwrap().visits.len(), 2);
        assert_eq!(app.list_patients().unwrap().len(), 3, "no duplicate made for the old spelling");
        let pickles = app.patient(&find("Pickles").patient.id).unwrap();
        assert_eq!(pickles.visits[0].pet.as_deref(), Some("Pickles"));

        // The work gate (ADR-0013 review): no folder change while something
        // is writing; after a change, this App takes no more work.
        struct Quiet;
        impl Events for Quiet {
            fn segment(&self, _: &Segment) {}
            fn token(&self, _: &str) {}
            fn level(&self, _: f32, _: f32) {}
            fn download(&self, _: &str, _: u64, _: u64) {}
        }
        let elsewhere = tmp.path().join("elsewhere");
        let elsewhere = elsewhere.to_str().unwrap();
        let drafting = app.begin_work().unwrap();
        let busy = app.change_folder(FolderKind::Visits, elsewhere, ChangeMode::Switch, false, &Quiet);
        let refusal = busy.err().map(|e| e.0).unwrap_or_default();
        assert!(refusal.contains("importing"), "refused while a draft is running: {refusal}");
        drop(drafting);
        app.change_folder(FolderKind::Visits, elsewhere, ChangeMode::Switch, false, &Quiet).unwrap();
        assert!(app.begin_work().is_err(), "a retired App refuses new work");
        assert!(app.create_patient("Late", "Writer", "").is_err(), "no writes to the old folder");
        assert!(app.change_folder(FolderKind::Visits, elsewhere, ChangeMode::Switch, false, &Quiet).is_err());
        assert!(
            std::fs::read_to_string(tmp.path().join("locations.json")).unwrap().contains("elsewhere"),
            "the choice was saved"
        );
    }
}
