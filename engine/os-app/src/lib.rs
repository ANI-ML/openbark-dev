//! The OpenBark application layer: everything the UI can ask for, in one
//! place, so the desktop app (Tauri commands) and web mode (HTTP API) are
//! thin transports over identical behaviour.
//!
//! All methods block; callers run them on a worker thread. Progress goes to
//! an [`Events`] sink (Tauri events on desktop, server-sent events on web).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Local};
use os_engine::audit::{self, AuditLog, EventDraft, EventKind, Purge};
pub use os_engine::audit::{DeleteReason, ReasonKind};
use os_engine::awake::{AwakeGuard, AwakeReason};
use os_engine::live::{LiveOptions, LiveRecorder, LiveSink, LiveSource};
pub use os_engine::live::{Progress, Stage};
use os_engine::patients::{PatientRecord, PatientStore};
use os_engine::secrets::{self, TokenSource};
pub use os_engine::session::AudioDeleted;
use os_engine::session::{self, Addendum, ChartCheck, Consent, PatientInfo, Session};
use os_engine::template::{LoadedTemplate, Scope, Template};
use os_engine::trash::{self, DeletedInfo, Recovered, Trash};
use os_engine::users::{Role, User, UserStore, Who};
use os_engine::{NoteOptions, Segment, TemplateStore, TranscribeStats, Transcript};
use os_llm::{
    LlamaCppBackend, LlamaCppOptions, LlmBackend, MlxBackend, MlxDevice, MlxOptions, ModelCheck, PackCheck,
};
use os_models::hub::{self, Access, Fit, HubModel, HubRepo, MlxProblem};
use os_models::local::ImportKind;
pub use os_models::local::{ImportMode, ImportPreview};
use os_models::locations::{self, Contents};
pub use os_models::locations::{FolderKind, Moved, Source as FolderSource};
use os_models::mlx_pack::{self, Support};
pub use os_models::remove::Removal;
use os_models::remove::{self as removal, RemoveRule};
use os_models::{CatalogEntry, Layout, LlmEngine, ModelStore, Slot, Source, Verify};
use serde::Serialize;

pub use os_asr::capture::AudioBlock;
pub use os_engine::template_store::{ProfileSummary, ShareMode, TemplateSummary};
pub use os_engine::users::TemplateMode;
pub use templates::{
    AlwaysApplied, Approver, ResetView, SaveTemplateRequest, TeamSettingsView, TemplateCan, TemplateView,
    TemplatesMode,
};

#[doc(hidden)]
pub mod demo;
mod phone;
mod templates;
pub use os_connect::{ConnectStore, PhonePrefs};
pub use os_engine::live::CallBlock;
pub use os_engine::session::PatientInfo as Patient;
pub use os_engine::session::{CallDirection, CallInfo, ConsentOutcome};
pub use os_engine::session::{Consent as RecordingConsent, ConsentMethod, ConsentRule};
pub use phone::{
    CALL_SAMPLE_RATES, CallEnd, CallRequest, CallStart, CallTarget, ConnectRequest, Connected, Disconnected,
    PhoneToken, PhoneView, SetupCheck, TwilioView, check_call_sample_rate,
};

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
    /// Space Deleted visits take, inside the visits folder (ADR-0022 section 9).
    pub deleted_bytes: u64,
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

/// What is holding a visit (ADR-0022 section 7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Holder {
    Importing,
    Recording,
    Drafting,
    Saving,
    Deleting,
}

impl Holder {
    /// Why `asked` can't have the visit while `self` holds it.
    fn refusal(self, asked: Holder) -> &'static str {
        match (self, asked) {
            (Holder::Deleting, _) => "This visit is being deleted.",
            (Holder::Drafting, Holder::Deleting) => {
                "This visit is being drafted. Delete it when that's finished."
            }
            (Holder::Importing, Holder::Deleting) => {
                "This visit is still being transcribed. Delete it when that's finished."
            }
            (Holder::Recording, Holder::Deleting) => {
                "This visit is still recording. Stop the recording first."
            }
            _ => "This visit is being saved in another window. Try again in a moment.",
        }
    }
}

type VisitLocks = Arc<Mutex<HashMap<String, Holder>>>;

/// One visit held by one piece of work (a per-visit lock, per process).
/// Dropping it frees the visit.
pub struct VisitLock {
    locks: VisitLocks,
    key: String,
}

impl Drop for VisitLock {
    fn drop(&mut self) {
        if let Ok(mut held) = self.locks.lock() {
            held.remove(&self.key);
        }
    }
}

/// Phase 1 of ADR-0022: signed visits are corrected with addenda.
const SIGNED_NOT_YET: &str = "Signed visits can't be deleted yet. Add an addendum to correct the note.";
const NOT_RECORDED: &str = "OpenBark couldn't record the deletion, so the visit was kept.";

pub struct App {
    pub store: ModelStore,
    pub sessions_root: PathBuf,
    pub templates: TemplateStore,
    /// Team members with roles (M1 item 11); empty means single-user.
    pub users: UserStore,
    /// Patients, with their visits linked by id (ADR-0011).
    pub patients: PatientStore,
    /// The loaded note model: llama.cpp or MLX (ADR-0025). A draft holds a
    /// clone of the `Arc` while it runs, so a count above one means busy.
    llm: Mutex<Option<(String, Arc<dyn LlmBackend>)>>,
    /// Every note model handed out (id, MLX or not), weakly: a draft still
    /// running on a model that has since left the slot keeps it busy.
    loaned: Mutex<Vec<Loaned>>,
    /// Model ids with a download or an import running (ADR-0025 §10).
    downloading: Mutex<HashSet<String>>,
    /// The slot holds, or is loading, an MLX model: for status calls that
    /// mustn't wait while a model loads (the slot's lock is held for that).
    mlx_in_slot: std::sync::atomic::AtomicBool,
    /// Whose Hugging Face token was saved in this run (from `whoami`), kept
    /// in memory only.
    hf_user: Mutex<Option<String>>,
    recorder: Mutex<Option<LiveRecorder>>,
    /// The live recording's work ticket, held until it stops.
    recording_work: Mutex<Option<Busy>>,
    /// The live recording's hold on its visit, until it stops.
    recording_lock: Mutex<Option<VisitLock>>,
    /// Visits being imported, recorded, drafted, saved or deleted (ADR-0022).
    visit_locks: VisitLocks,
    gate: Arc<Gate>,
    hardware: Hardware,
    /// The GPU speech pack was in place when this process started, so its
    /// libraries are the ones loaded (ADR-0014).
    gpu_at_start: bool,
    /// The GPU self-test: 0 not run, 1 passed, 2 failed.
    gpu_checked: Arc<std::sync::atomic::AtomicU8>,
    /// Template resets that can still be undone (ADR-0024).
    undo: Mutex<templates::UndoMap>,
    /// The clinic's own Twilio account and this computer's phone settings
    /// (ADR-0023). `OPENBARK_CONNECTIONS_DIR` moves it, away from the keychain.
    pub connect: ConnectStore,
    /// Tests: the loopback fake Twilio's address. Only loopback is accepted.
    pub twilio_base: Option<String>,
    /// The phone call being recorded, if any.
    call: Mutex<Option<phone::CallState>>,
    /// Who the main window asked the phone window to call.
    pending_call: Mutex<Option<phone::Pending>>,
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

/// An MLX engine pack download or set-up is running (one at a time per process).
static MLX_INSTALLING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Passed to the program itself to run the GPU self-test in a child process.
pub const GPU_CHECK_ARG: &str = "--openbark-gpu-check";

/// The most one ranged answer for a recording carries
/// ([`App::session_audio_part`]); players ask again for the rest.
pub const AUDIO_PART_MAX: u64 = 1024 * 1024;

/// Part of a recording, as an HTTP answer: 200 with the whole recording,
/// 206 with bytes `start..=end` of `total`, or 416 when the range asked for
/// lies outside it. Recordings are served as `audio/wav`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioPart {
    pub status: u16,
    pub total: u64,
    pub start: u64,
    pub end: u64,
    pub body: Vec<u8>,
}

impl AudioPart {
    /// The response headers, other than the status: type, length, ranges,
    /// and no caching (decrypted audio must never land in a cache on disk).
    pub fn headers(&self) -> Vec<(&'static str, String)> {
        let mut h = vec![
            ("Content-Type", "audio/wav".to_string()),
            ("Accept-Ranges", "bytes".to_string()),
            ("Cache-Control", "no-store".to_string()),
        ];
        match self.status {
            206 => {
                h.push(("Content-Range", format!("bytes {}-{}/{}", self.start, self.end, self.total)));
                h.push(("Content-Length", (self.end - self.start + 1).to_string()));
            }
            416 => {
                h.push(("Content-Range", format!("bytes */{}", self.total)));
                h.push(("Content-Length", "0".to_string()));
            }
            _ => h.push(("Content-Length", self.total.to_string())),
        }
        h
    }
}

#[derive(Debug, PartialEq, Eq)]
enum RangePlan {
    Whole,
    /// Inclusive, within the recording.
    Part(u64, u64),
    Unsatisfiable,
}

/// What a `Range` header asks of a recording `total` bytes long. Only the
/// first of several ranges is answered (players ask for one); a header that
/// doesn't parse is ignored, as HTTP says.
fn plan_range(header: Option<&str>, total: u64) -> RangePlan {
    let Some(spec) = header.map(str::trim).and_then(|h| h.strip_prefix("bytes=")) else {
        return RangePlan::Whole;
    };
    let first = spec.split(',').next().unwrap_or("").trim();
    let Some((a, b)) = first.split_once('-') else { return RangePlan::Whole };
    let (a, b) = (a.trim(), b.trim());
    let parsed = match (a.is_empty(), b.is_empty()) {
        // bytes=-n: the last n bytes.
        (true, false) => match b.parse::<u64>() {
            Ok(0) => return RangePlan::Unsatisfiable,
            Ok(n) if total > 0 => Some((total.saturating_sub(n), total - 1)),
            Ok(_) => return RangePlan::Unsatisfiable,
            Err(_) => None,
        },
        (false, true) => a.parse::<u64>().ok().map(|s| (s, u64::MAX)),
        (false, false) => match (a.parse::<u64>(), b.parse::<u64>()) {
            (Ok(s), Ok(e)) if s <= e => Some((s, e)),
            _ => None,
        },
        (true, true) => None,
    };
    match parsed {
        None => RangePlan::Whole,
        Some((start, _)) if start >= total => RangePlan::Unsatisfiable,
        Some((start, end)) => RangePlan::Part(start, end.min(total - 1)),
    }
}

/// How long quitting waits for a running draft to stop and the note model
/// to be freed ([`App::shutdown`]). A draft stops at its next token, so this
/// only runs out when a model load is in progress.
pub const SHUTDOWN_WAIT: std::time::Duration = std::time::Duration::from_secs(8);

/// End the process, after [`App::shutdown`]: the normal exit when every note
/// model was freed, else one that skips the C++ static destructors so ggml's
/// Metal teardown can't abort (`os_llm::shutdown::exit`).
pub fn exit_process(code: i32) -> ! {
    os_llm::shutdown::exit(code)
}

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
    /// What Remove frees: the pack's libraries plus the two full-precision
    /// models on disk.
    pub installed_bytes: u64,
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
    /// Working alone, each person's own templates, or the hospital's (ADR-0024).
    pub templates: TemplatesMode,
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
    /// The recording is still kept (it can be deleted on its own, ADR-0022).
    pub has_audio: bool,
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
    /// This patient's visits waiting in Deleted visits.
    pub deleted: usize,
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
    /// Any note in the visit is signed, for any animal (ADR-0022 section 2).
    pub any_signed: bool,
    /// The recording is still kept.
    pub has_audio: bool,
    /// When only the recording was deleted, and by whom.
    pub audio_deleted: Option<AudioDeleted>,
    /// The latest draft's tooth chart confirmations, by tooth: who confirmed
    /// the drafted entries against the recording, and when (ADR-0021).
    pub chart_checks: Vec<ChartCheck>,
    /// The latest draft's teeth with a drafted chart entry that no
    /// veterinarian has confirmed yet. Signing waits until it is empty.
    pub unconfirmed_teeth: Vec<String>,
    /// A phone call's details (ADR-0023 section 8): direction, number,
    /// length, Twilio's call SID, and who was on the clinic's side.
    pub call: Option<CallInfo>,
}

/// A visit just moved to Deleted visits.
#[derive(Debug, Clone, Serialize)]
pub struct DeletedVisit {
    pub trash_id: String,
    /// The animals in the visit.
    pub pets: Vec<String>,
    pub signed: bool,
    pub purge_after: String,
    /// The main animal's patient record, to go back to.
    pub patient_id: Option<String>,
}

/// A row of Settings, Deleted visits.
#[derive(Debug, Clone, Serialize)]
pub struct DeletedSummary {
    pub trash_id: String,
    /// "Olive Haddad" (empty when no patient was entered).
    pub patient: String,
    /// The visit's patient records (main animal first).
    pub patient_ids: Vec<String>,
    pub pets: Vec<String>,
    pub created_at: String,
    pub deleted_at: String,
    pub by: String,
    pub reason: DeleteReason,
    pub signed: bool,
    pub purge_after: String,
    /// Phase 2: kept longer by the clinic's retention rule.
    pub kept_until: Option<String>,
    pub bytes: u64,
    /// Described at start after OpenBark closed mid-delete.
    pub recovered: bool,
}

/// A visit brought back from Deleted visits.
#[derive(Debug, Clone, Serialize)]
pub struct RestoredVisit {
    /// Its folder name now (a suffix is added if the original was taken).
    pub id: String,
    pub patient_id: Option<String>,
}

/// Delete a visit (ADR-0022 section 12).
#[derive(Clone, serde::Deserialize)]
pub struct DeleteRequest {
    #[serde(default)]
    pub by: Option<String>,
    pub reason: DeleteReason,
    /// Phase 2: a veterinarian's PIN, for signed visits. Body only, never logged.
    #[serde(default)]
    pub pin: Option<String>,
    /// Phase 2: the acknowledgement that a signed note is kept elsewhere.
    #[serde(default)]
    pub confirm_signed: bool,
}

/// Written by hand so the PIN never reaches a log.
impl std::fmt::Debug for DeleteRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeleteRequest")
            .field("by", &self.by)
            .field("reason", &self.reason)
            .field("pin", &self.pin.as_ref().map(|_| "***"))
            .field("confirm_signed", &self.confirm_signed)
            .finish()
    }
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

#[derive(Serialize)]
pub struct NoteResponse {
    pub markdown: String,
    pub run: serde_json::Value,
    /// Said to the person drafting, such as a template that isn't theirs to use.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notice: Option<String>,
}

/// Where a model came from (ADR-0025 §10).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    Builtin,
    Hub,
    LocalCopy,
    LocalInPlace,
}

/// What Remove does for a row, or why it isn't offered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RowRemove {
    Delete,
    DeleteAndForget,
    ForgetOnly,
    /// A speech model OpenBark needs to transcribe visits.
    Needed,
    /// Removed with the GPU speech pack, not on its own.
    WithGpuPack,
}

#[derive(Debug, Clone, Serialize)]
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
    /// Bytes on disk that are OpenBark's alone, so removing them frees that
    /// much (0 when used in place).
    pub disk_bytes: u64,
    /// Bytes on disk whose data another name also holds (a hard link: a copy
    /// from this computer on the same drive). Removing them frees nothing.
    pub shared_bytes: u64,
    /// A download that stopped part-way.
    pub partial_bytes: u64,
    pub origin: Origin,
    /// Note models only.
    pub engine: Option<LlmEngine>,
    /// Number of files.
    pub parts: usize,
    pub remove: RowRemove,
    /// Used in place, and the file or folder is gone.
    pub missing: bool,
    pub gated: bool,
    /// Used in place: where it is.
    pub path: Option<String>,
}

/// The MLX engine pack's self-test.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckState {
    NotRun,
    Passed,
    Failed,
}

/// The MLX engine card (ADR-0025 §4).
#[derive(Debug, Clone, Serialize)]
pub struct MlxStatus {
    pub support: Support,
    pub macos_version: Option<String>,
    pub pack_installed: bool,
    pub installing: bool,
    pub check: CheckState,
    pub check_message: Option<String>,
    pub metal: bool,
    /// Still to download (0 when installed).
    pub download_bytes: u64,
    pub installed_bytes: u64,
    /// python, mlx, mlx_lm, llguidance, from the lock.
    pub versions: BTreeMap<String, String>,
    /// An MLX model is loaded now.
    pub in_use: bool,
}

/// The Hugging Face token (ADR-0025 §8). Never the token itself.
#[derive(Debug, Clone, Serialize)]
pub struct HfTokenStatus {
    pub saved: bool,
    pub source: Option<TokenSource>,
    /// The name from `whoami` when the token was saved in this run.
    pub user: Option<String>,
}

/// An MLX model folder in a Hub repo, as the sheet shows it.
#[derive(Debug, Clone, Serialize)]
pub struct HubMlxView {
    pub size_bytes: u64,
    pub files: usize,
    pub fit: Fit,
    pub model_type: Option<String>,
    pub quantization: Option<String>,
    pub problem: Option<MlxProblem>,
    /// mlx-lm in the pack can run this `model_type`.
    pub arch_supported: bool,
    pub support: Support,
    pub pack_installed: bool,
}

/// Refused while drafting, refining or proposing a template with the model.
const MODEL_BUSY: &str = "A note is being written with this model. Remove it when that's finished.";
const MLX_FIRST: &str = "Download the MLX engine on the Models screen first.";
const MLX_NEEDS: &str = "MLX models run on a Mac with Apple silicon and macOS 14 or later.";
const MLX_DOWNLOADING: &str = "The MLX engine is already downloading.";
const MLX_CHECK_FAILED: &str = "The MLX engine didn't pass its check on this computer. Choose Try again on the MLX engine card in Models.";

/// Why a model can't be removed on its own, if it can't: the speech models
/// OpenBark needs to transcribe visits, and the GPU speech pack's own
/// recognisers (ADR-0025 §10). The default note model is not protected.
pub fn protected_model(id: &str) -> Option<&'static str> {
    use os_engine::models::{DEFAULT_STT, EMBEDDING, GPU_SECOND_STT, GPU_STT, SEGMENTATION, VAD};
    if [VAD, DEFAULT_STT, SEGMENTATION, EMBEDDING, os_engine::live::SECOND_STT].contains(&id) {
        Some("OpenBark needs this to transcribe visits.")
    } else if [GPU_STT, GPU_SECOND_STT].contains(&id) {
        Some("This comes with the GPU speech pack. Remove the pack instead.")
    } else {
        None
    }
}

fn row_remove(m: &CatalogEntry) -> RowRemove {
    use os_engine::models::{DEFAULT_STT, EMBEDDING, GPU_SECOND_STT, GPU_STT, SEGMENTATION, VAD};
    if [VAD, DEFAULT_STT, SEGMENTATION, EMBEDDING, os_engine::live::SECOND_STT].contains(&m.id.as_str()) {
        return RowRemove::Needed;
    }
    if [GPU_STT, GPU_SECOND_STT].contains(&m.id.as_str()) {
        return RowRemove::WithGpuPack;
    }
    match removal::rule(m) {
        RemoveRule::Delete => RowRemove::Delete,
        RemoveRule::DeleteAndForget => RowRemove::DeleteAndForget,
        RemoveRule::ForgetOnly => RowRemove::ForgetOnly,
    }
}

fn origin_of(m: &CatalogEntry, builtin: bool) -> Origin {
    match &m.source {
        _ if builtin => Origin::Builtin,
        Source::Local { path: Some(_) } => Origin::LocalInPlace,
        Source::Local { path: None } => Origin::LocalCopy,
        _ => Origin::Hub,
    }
}

/// MLX can run here: a Mac with Apple silicon and macOS 14, or test mode.
pub fn mlx_runs_here() -> bool {
    matches!(mlx_pack::support(), Support::Yes | Support::TestMode)
}

/// MLX's device: `OPENBARK_MLX_DEVICE=cpu` forces the CPU (Mac runners
/// without usable Metal); otherwise Metal when available.
pub fn mlx_device() -> MlxDevice {
    if std::env::var("OPENBARK_MLX_DEVICE").is_ok_and(|v| v.trim().eq_ignore_ascii_case("cpu")) {
        MlxDevice::Cpu
    } else {
        MlxDevice::Auto
    }
}

/// On macOS the worker runs under `sandbox-exec` with no network, unless
/// `OPENBARK_MLX_SANDBOX=0` (support only).
pub fn mlx_sandbox() -> bool {
    cfg!(target_os = "macos") && std::env::var("OPENBARK_MLX_SANDBOX").map_or(true, |v| v.trim() != "0")
}

fn pack_key() -> Option<mlx_pack::CheckKey> {
    mlx_pack::check_key(env!("CARGO_PKG_VERSION"), "pack")
}

/// Whether Metal was usable at the last pack check, kept beside it.
fn metal_key() -> Option<mlx_pack::CheckKey> {
    mlx_pack::check_key(env!("CARGO_PKG_VERSION"), "pack:metal")
}

/// Where MLX stands before a model loads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MlxGate {
    /// Not a Mac that runs MLX (and not test mode).
    NotHere,
    /// The engine pack isn't installed.
    NotInstalled,
    /// Installed, with no check result for this app version, lock, macOS
    /// version and chip: after an OpenBark or macOS update, or never run.
    NeedsCheck,
    /// Its check failed on this computer.
    Failed,
    Ready,
}

fn mlx_gate(runs: bool, installed: bool, check: Option<&mlx_pack::CheckRecord>) -> MlxGate {
    match (runs, installed, check) {
        (false, _, _) => MlxGate::NotHere,
        (true, false, _) => MlxGate::NotInstalled,
        (true, true, None) => MlxGate::NeedsCheck,
        (true, true, Some(r)) if r.ok => MlxGate::Ready,
        (true, true, Some(_)) => MlxGate::Failed,
    }
}

/// Make sure the engine can write notes, running its check first when the
/// last result no longer applies (a few seconds, local): an update must
/// never send someone to download an engine they already have. `check` is
/// [`run_mlx_pack_check`] (a stand-in in tests).
fn ensure_mlx(gate: MlxGate, check: impl FnOnce() -> Result<PackCheck>) -> Result<()> {
    match gate {
        MlxGate::Ready => Ok(()),
        MlxGate::NotHere => Err(AppError(MLX_NEEDS.into())),
        MlxGate::NotInstalled => Err(AppError(MLX_FIRST.into())),
        MlxGate::Failed => Err(AppError(MLX_CHECK_FAILED.into())),
        MlxGate::NeedsCheck => {
            tracing::info!("the MLX engine's last check doesn't apply any more; checking it again");
            match check() {
                Ok(c) if c.ok => Ok(()),
                Ok(c) => {
                    tracing::warn!(message = ?c.message, "the MLX engine didn't pass its check");
                    Err(AppError(MLX_CHECK_FAILED.into()))
                }
                Err(e) => {
                    tracing::warn!(error = %e.0, "the MLX engine couldn't check itself");
                    Err(AppError(MLX_CHECK_FAILED.into()))
                }
            }
        }
    }
}

/// The MLX engine pack is installed here and passed its check.
pub fn mlx_ready() -> bool {
    mlx_runs_here()
        && mlx_pack::installed()
        && pack_key().and_then(|k| mlx_pack::check_result(&k)).is_some_and(|r| r.ok)
}

/// Run the MLX pack check in a child worker and record the result (and
/// whether Metal was usable). A check that couldn't run at all is recorded
/// as failed too, and returned as an error.
pub fn run_mlx_pack_check() -> Result<PackCheck> {
    let (Some(python), Some(dir)) = (mlx_pack::python(), mlx_pack::pack_dir()) else {
        return Err(AppError(MLX_NEEDS.into()));
    };
    if !mlx_pack::installed() {
        return Err(AppError(MLX_FIRST.into()));
    }
    let record = |key: Option<mlx_pack::CheckKey>, ok: bool, message: Option<&str>| {
        if let Some(key) = key
            && let Err(e) = mlx_pack::record_check(&key, ok, message)
        {
            tracing::warn!(error = %e, "couldn't save the MLX check result");
        }
    };
    match MlxBackend::pack_check(&python, &dir, mlx_device(), mlx_sandbox()) {
        Ok(check) => {
            record(pack_key(), check.ok, check.message.as_deref());
            record(metal_key(), check.metal, None);
            Ok(check)
        }
        Err(e) => {
            let message = e.to_string();
            record(pack_key(), false, Some(&message));
            record(metal_key(), false, None);
            Err(AppError(format!("The MLX engine couldn't check itself: {message}.")))
        }
    }
}

/// Download (resumable, verified) and unpack the MLX engine pack, then check
/// it. `progress(done, total)` counts bytes downloaded. The check's result
/// is recorded either way; it is returned for the command line.
pub fn install_mlx_pack(store: &ModelStore, progress: &mut dyn FnMut(u64, u64)) -> Result<PackCheck> {
    let entry = mlx_pack::entry().filter(|_| mlx_runs_here()).ok_or_else(|| AppError(MLX_NEEDS.into()))?;
    if !mlx_pack::installed() {
        let total = entry.total_size();
        let (mut before, mut current, mut file_done) = (0u64, String::new(), 0u64);
        os_models::download(store, &entry, None, &mut |file: &str, done: u64, _t: u64| {
            if file != current {
                before += file_done;
                current = file.to_string();
            }
            file_done = done;
            progress(before + done, total);
        })?;
        mlx_pack::unpack(store, |_, _| {})
            .map_err(|e| AppError(format!("The MLX engine couldn't be set up: {e}.")))?;
    }
    run_mlx_pack_check()
}

/// Open a note model by catalog id (built-in, Hub, or added from this
/// computer) or a path to a `.gguf` file: llama.cpp for GGUF, the MLX
/// worker for MLX folders (ADR-0025). Refuses a model that can't be held to
/// a JSON Schema: every draft is constrained (ADR-0006).
pub fn open_llm(store: &ModelStore, spec: &str, gpu_layers: Option<u32>) -> Result<Arc<dyn LlmBackend>> {
    let llama = |path: PathBuf| -> Result<Arc<dyn LlmBackend>> {
        let mut options = LlamaCppOptions::new(path);
        if let Some(layers) = gpu_layers {
            options.gpu_layers = layers;
        }
        Ok(Arc::new(LlamaCppBackend::load(options)?))
    };
    let (name, backend) = if spec.to_ascii_lowercase().ends_with(".gguf") && Path::new(spec).is_file() {
        let name = Path::new(spec).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        (name, llama(PathBuf::from(spec))?)
    } else {
        let catalog = hub::full_catalog(store);
        let m = catalog.get(spec).ok_or_else(|| AppError(format!("No model called {spec}.")))?;
        if let Some(p) = m.in_place()
            && !p.exists()
        {
            return Err(AppError(format!(
                "{} isn't where it was ({}). Add it again from this computer.",
                m.display_name,
                p.display()
            )));
        }
        let downloaded = || {
            if store.is_installed(m, Verify::Size) {
                Ok(())
            } else {
                Err(AppError(format!("{} isn't downloaded yet. Download it in Models.", m.display_name)))
            }
        };
        let backend = match &m.layout {
            Layout::LlmGguf { model } => {
                downloaded()?;
                // A split GGUF: llama.cpp is given part 1 and finds the rest.
                llama(store.file_path(m, model))?
            }
            Layout::LlmMlx { config } => {
                downloaded()?;
                open_mlx(store, m, config)?
            }
            _ => return Err(AppError(format!("{} can't write notes.", m.display_name))),
        };
        (m.display_name.clone(), backend)
    };
    if !backend.capabilities().json_schema_constrained {
        return Err(AppError(format!("{name} didn't pass OpenBark's check, so it can't write notes.")));
    }
    Ok(backend)
}

/// An MLX model, on the pack's worker. The model check runs on its first
/// load on this computer and is remembered in `checks.json`.
fn open_mlx(store: &ModelStore, m: &CatalogEntry, config: &str) -> Result<Arc<dyn LlmBackend>> {
    let installed = mlx_pack::installed();
    let record = pack_key().and_then(|k| mlx_pack::check_result(&k));
    ensure_mlx(mlx_gate(mlx_runs_here(), installed, record.as_ref()), run_mlx_pack_check)?;
    let (Some(python), Some(work_dir)) = (mlx_pack::python(), mlx_pack::pack_dir()) else {
        return Err(AppError(MLX_FIRST.into()));
    };
    let config_path = store.file_path(m, config);
    let folder = config_path.parent().map(Path::to_path_buf).unwrap_or_default();
    let model_dir = std::path::absolute(&folder)?;
    let revision = match &m.source {
        Source::Huggingface { revision, .. } => revision.clone(),
        _ => std::fs::metadata(&config_path)
            .map(|md| {
                let mtime = md
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map_or(0, |d| d.as_secs());
                format!("{}+{mtime}", md.len())
            })
            .unwrap_or_default(),
    };
    let key = mlx_pack::check_key(env!("CARGO_PKG_VERSION"), &format!("{}@{revision}", m.id));
    let passed_before = key.as_ref().and_then(mlx_pack::check_result).is_some_and(|r| r.ok);
    let mut options = MlxOptions::new(python, work_dir, model_dir, m.display_name.clone());
    options.device = mlx_device();
    options.sandbox = mlx_sandbox();
    options.model_check = if passed_before { ModelCheck::AlreadyPassed } else { ModelCheck::Run };
    let backend = MlxBackend::load(options)?;
    if let (Some(key), Some(passed)) = (&key, backend.model_check_ran()) {
        let message = (!passed).then_some("The model didn't pass OpenBark's check.");
        if let Err(e) = mlx_pack::record_check(key, passed, message) {
            tracing::warn!(error = %e, "couldn't save the MLX model check");
        }
    }
    Ok(Arc::new(backend))
}

/// A note model handed out to a draft: its id, whether it runs on MLX, and
/// a weak handle (alive while the draft runs).
type Loaned = (String, bool, std::sync::Weak<dyn LlmBackend>);

/// Drops a running download's id from the set when it ends.
struct Downloading<'a> {
    set: &'a Mutex<HashSet<String>>,
    id: String,
}

impl Drop for Downloading<'_> {
    fn drop(&mut self) {
        if let Ok(mut set) = self.set.lock() {
            set.remove(&self.id);
        }
    }
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
            loaned: Mutex::new(Vec::new()),
            downloading: Mutex::new(HashSet::new()),
            mlx_in_slot: std::sync::atomic::AtomicBool::new(false),
            hf_user: Mutex::new(None),
            recorder: Mutex::new(None),
            recording_work: Mutex::new(None),
            recording_lock: Mutex::new(None),
            visit_locks: Arc::new(Mutex::new(HashMap::new())),
            gate: Arc::new(Gate::default()),
            hardware: probe_hardware(),
            gpu_at_start: os_models::gpu_pack::loaded_at_start(),
            gpu_checked: Arc::new(std::sync::atomic::AtomicU8::new(0)),
            undo: Mutex::new(HashMap::new()),
            connect: ConnectStore::default(),
            twilio_base: None,
            call: Mutex::new(None),
            pending_call: Mutex::new(None),
        };
        app.start_gpu_check();
        // Folders of older MLX engine packs and unfinished unpacks (ADR-0025 §4).
        static MLX_TIDY: std::sync::Once = std::sync::Once::new();
        MLX_TIDY.call_once(mlx_pack::tidy);
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
            templates: self.templates_mode(),
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
            Need::Notes | Need::Delete => user.role.can_write_notes(),
            Need::Sign | Need::ConfirmChart | Need::DeleteSigned | Need::DeleteSignedRecording => {
                user.role.can_sign()
            }
            Need::Team | Need::Models | Need::Phone => user.role.can_manage_team(),
        };
        if !allowed {
            return Err(AppError(match need {
                Need::Notes => format!(
                    "{} can record and enter patient details; drafting and editing notes needs a veterinarian or technician.",
                    user.name
                ),
                Need::Delete => format!(
                    "{} can record and enter patient details; deleting visits needs a veterinarian or technician.",
                    user.name
                ),
                Need::Sign => format!("Only a veterinarian can sign. {} is using OpenBark now.", user.name),
                Need::ConfirmChart => {
                    format!(
                        "Only a veterinarian can confirm the tooth chart. {} is using OpenBark now.",
                        user.name
                    )
                }
                Need::DeleteSigned => {
                    format!(
                        "Only a veterinarian can delete a signed visit. {} is using OpenBark now.",
                        user.name
                    )
                }
                Need::DeleteSignedRecording => {
                    "Only a veterinarian can delete the recording of a signed visit.".into()
                }
                Need::Team => "Only a veterinarian can change the team.".into(),
                Need::Models => "Only a veterinarian can change the models.".into(),
                Need::Phone => "Only a veterinarian can connect Twilio or change the phone settings.".into(),
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
        let was_team = self.users.is_team();
        if was_team {
            self.who(by, Need::Team)?;
        }
        let user = self.users.add(name, role, pin)?;
        self.note_team_setup(was_team);
        Ok(user)
    }

    /// Remove a team member, with their profile: their own template versions
    /// and standards (ADR-0024 D7). Notes keep the version that drafted them.
    pub fn remove_user(&self, id: &str, by: Option<&str>) -> Result<()> {
        self.who(by, Need::Team)?;
        self.users.remove(id)?;
        self.templates.remove_profile(&self.profiles_root(), id)?;
        Ok(())
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
        Ok(os_engine::vault::read(&self.audio_file(id)?)?)
    }

    /// Part of a visit's recording for a media player, by an HTTP `Range`
    /// header (`bytes=a-b`, `bytes=a-`, `bytes=-n`). Only the chunks asked
    /// for are decrypted, and an open-ended range is answered with at most
    /// [`AUDIO_PART_MAX`] bytes, so a long recording never sits whole in
    /// memory. WebKit's player (macOS, Linux) won't play over a custom
    /// scheme without ranges. With `head`, the body is left empty.
    pub fn session_audio_part(&self, id: &str, range: Option<&str>, head: bool) -> Result<AudioPart> {
        let mut reader = os_engine::vault::AudioReader::open(&self.audio_file(id)?)?;
        let total = reader.len();
        let (status, start, end) = match plan_range(range, total) {
            RangePlan::Whole => (200, 0, total.saturating_sub(1)),
            RangePlan::Part(start, end) => (206, start, end.min(start + AUDIO_PART_MAX - 1)),
            RangePlan::Unsatisfiable => {
                return Ok(AudioPart { status: 416, total, start: 0, end: 0, body: Vec::new() });
            }
        };
        let body = if head || total == 0 { Vec::new() } else { reader.read_range(start, end)? };
        Ok(AudioPart { status, total, start, end, body })
    }

    /// The recording's file, or why there's none.
    fn audio_file(&self, id: &str) -> Result<PathBuf> {
        let dir = self.session_dir(id)?;
        let audio = dir.join(session::AUDIO_FILE);
        if !audio.is_file()
            && let Some(gone) = Session::open(&dir)?.meta.audio_deleted
        {
            return Err(AppError(format!(
                "This visit's recording was deleted on {}.",
                gone.at.format("%Y-%m-%d")
            )));
        }
        Ok(audio)
    }

    // ---- sessions --------------------------------------------------------

    /// Resolve a session id (its folder name) safely under the sessions root.
    /// No trailing dot: Windows drops it, so "<id>." would open the same
    /// folder as "<id>" under another name and slip past that visit's lock
    /// (review of ADR-0022). OpenBark never names a visit folder that way.
    pub fn session_dir(&self, id: &str) -> Result<PathBuf> {
        let valid = !id.is_empty()
            && !id.starts_with('.')
            && !id.ends_with('.')
            && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
        if !valid {
            return Err(AppError(format!("'{id}' is not a session id")));
        }
        let dir = self.sessions_root.join(id);
        if dir.join(session::SESSION_FILE).is_file() {
            Ok(dir)
        } else if self.trash().find_by_original(id).is_some() {
            // Another window, or an old link (ADR-0022 section 9).
            Err(AppError("This visit was deleted. Restore it from Deleted visits in Settings.".into()))
        } else {
            Err(AppError(format!("No saved encounter called {id}.")))
        }
    }

    fn trash(&self) -> Trash {
        Trash::for_sessions(&self.sessions_root)
    }

    fn audit(&self) -> AuditLog {
        AuditLog::for_sessions(&self.sessions_root)
    }

    /// Hold a visit (or another key) for one piece of work, or say why not.
    fn lock_visit(&self, id: &str, holder: Holder) -> Result<VisitLock> {
        let key = id.to_ascii_lowercase();
        let mut held = self.visit_locks.lock().map_err(|_| AppError("visit lock poisoned".into()))?;
        if let Some(by) = held.get(&key) {
            return Err(AppError(by.refusal(holder).into()));
        }
        held.insert(key.clone(), holder);
        Ok(VisitLock { locks: self.visit_locks.clone(), key })
    }

    /// A live recording is writing into this folder.
    fn recording_into(&self, dir: &Path) -> bool {
        self.recorder.lock().map(|r| r.as_ref().is_some_and(|rec| rec.session_dir == dir)).unwrap_or(false)
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
                // Any animal's signed note makes the visit signed (ADR-0022 section 2).
                signed: s.any_signed(),
                patient: s.meta.patient.as_ref().map(|p| p.display_name()).unwrap_or_default(),
                reason: s.meta.patient.as_ref().map(|p| p.reason.clone()).unwrap_or_default(),
                template: s
                    .latest_record()
                    .map(|r| {
                        r.template_name.clone().unwrap_or_else(|| {
                            self.templates
                                .load(r.template_id())
                                .map(|t| t.template.name)
                                .unwrap_or_else(|_| r.template_id().to_string())
                        })
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
        let latest = s.latest_record();
        let template_id = latest.map(|r| r.template_id().to_string()).unwrap_or_default();
        // The name and icon saved with the run; today's lookup for older runs.
        let saved = latest
            .and_then(|r| Some((r.template_name.clone()?, r.template_icon.clone().unwrap_or_default())));
        let (template, template_icon) = match saved {
            Some(pair) => pair,
            None => latest
                .and_then(|r| self.templates.load(r.template_id()).ok())
                .map(|t| (t.template.name, t.template.icon))
                .unwrap_or_default(),
        };
        VisitSummary {
            template_id,
            id: id_of(s),
            created_at: s.meta.created_at.format("%Y-%m-%d %H:%M").to_string(),
            reason: s.meta.patient.as_ref().map(|p| p.reason.clone()).unwrap_or_default(),
            template,
            template_icon,
            drafted: s.my_runs().next().is_some(),
            signed: s.is_signed(),
            audio_secs: s.meta.audio_secs,
            pet: (animals > 1).then(|| pet.to_string()),
            source: source_label(&s.meta.source),
            has_audio: s.has_audio(),
        }
    }

    /// Every visit, grouped under its patients' record ids.
    fn visits_by_patient(&self) -> Result<HashMap<String, Vec<VisitSummary>>> {
        self.visits_by_patient_from(session::list(&self.sessions_root))
    }

    fn visits_by_patient_from(&self, sessions: Vec<Session>) -> Result<HashMap<String, Vec<VisitSummary>>> {
        let mut out: HashMap<String, Vec<VisitSummary>> = Default::default();
        for mut s in sessions {
            let pairs = match self.link_visit(&mut s) {
                Ok(pairs) => pairs,
                // Deleted (or moved) while the list was being made: leave it out
                // rather than failing the whole list (ADR-0022 section 7).
                Err(_) if !s.dir.join(session::SESSION_FILE).is_file() => continue,
                Err(e) => return Err(e),
            };
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
        let deleted = self
            .trash()
            .list()
            .iter()
            .filter(|(_, s)| {
                s.meta.patient.as_ref().is_some_and(|p| {
                    p.patient_id.as_deref() == Some(id) || p.other_ids.iter().any(|o| o == id)
                })
            })
            .count();
        Ok(PatientView { visits: visits.remove(id).unwrap_or_default(), patient, deleted })
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
    /// The species entered for the animal this handle works on: the visit's
    /// for the main animal, and another animal's from its patient record in a
    /// multi-pet visit. None when it wasn't entered (every heading applies).
    fn species_of(&self, s: &Session) -> Option<String> {
        let p = s.meta.patient.as_ref()?;
        let main = s.pet.as_deref().is_none_or(|pet| pet.trim().eq_ignore_ascii_case(p.name.trim()));
        let species = if main {
            p.species.clone()
        } else {
            let pet = s.pet.as_deref().unwrap_or_default().trim();
            let at = p.others.iter().position(|o| o.trim().eq_ignore_ascii_case(pet))?;
            self.patients.get(p.other_ids.get(at)?)?.species
        };
        let species = species.trim();
        (!species.is_empty()).then(|| species.to_string())
    }

    /// A redraft of a chart whose animal still isn't entered keeps the dog or
    /// cat the vet chose on the last draft (ADR-0021 decision 4).
    fn with_last_dentition(s: &Session, template: LoadedTemplate) -> LoadedTemplate {
        let Some((heading, None)) = template.chart() else { return template };
        let chosen = s
            .latest_record()
            .and_then(|run| s.run_template(run).ok())
            .and_then(|t| t.chart())
            .and_then(|(_, d)| d);
        match chosen {
            Some(d) => template.with_dentition(&heading, d).unwrap_or(template),
            None => template,
        }
    }

    fn note_options(&self, s: &Session, language: Option<String>) -> NoteOptions {
        let pets = s.meta.patient.as_ref().map(|p| p.pets()).unwrap_or_default();
        let other_pets = match &s.pet {
            Some(pet) => pets.iter().filter(|p| !p.eq_ignore_ascii_case(pet)).cloned().collect(),
            None => Vec::new(),
        };
        // A phone call says who is who (ADR-0023 section 7): entered, not said.
        let context = [
            s.meta.patient.as_ref().map(|p| p.prompt_context()),
            s.meta.call.as_ref().map(|c| c.prompt_context()),
        ]
        .into_iter()
        .flatten()
        .filter(|c| !c.is_empty())
        .collect::<Vec<_>>()
        .join("; ");
        NoteOptions {
            context: Some(context).filter(|c| !c.is_empty()),
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
    /// clips; M1 tuning). `speakerphone`: a client on a phone on speaker
    /// (see `LiveOptions::speakerphone`).
    #[allow(clippy::too_many_arguments)]
    pub fn import_file(
        &self,
        path: &Path,
        label_speakers: bool,
        speakerphone: bool,
        patient: Option<PatientInfo>,
        consent: Option<Consent>,
        by: Option<&str>,
        events: &dyn Events,
    ) -> Result<SessionView> {
        let source = LiveSource::File { path: path.to_path_buf(), realtime: false };
        self.import_source(source, label_speakers, speakerphone, patient, consent, by, events)
    }

    #[allow(clippy::too_many_arguments)]
    fn import_source(
        &self,
        source: LiveSource,
        label_speakers: bool,
        speakerphone: bool,
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
        options.speakerphone = speakerphone;
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
        // Held while the new folder fills (a delete waits for the transcript).
        let _lock = self.lock_visit(&folder_name(&dir), Holder::Importing).ok();
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
        speakerphone: bool,
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
        self.import_source(source, label_speakers, speakerphone, patient, consent, by, events)
    }

    // ---- live recording --------------------------------------------------

    pub fn input_devices(&self) -> Vec<String> {
        os_asr::capture::input_devices()
    }

    /// Start recording from a microphone on this machine (`Some(device)`/`None`
    /// = default) or from blocks the caller pushes (`stream`). Consent is the
    /// clinic's responsibility (the app shows a one-time notice); when the
    /// per-recording consent step is used, the consent is checked and saved.
    /// `speakerphone`: a client on a phone on speaker in the room (see
    /// `LiveOptions::speakerphone`).
    #[allow(clippy::too_many_arguments)]
    pub fn start_recording(
        &self,
        source: RecordingSource,
        label_speakers: bool,
        speakerphone: bool,
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
        options.speakerphone = speakerphone;
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
        let id = folder_name(&recorder.session_dir);
        *slot = Some(recorder);
        *self.recording_work.lock().map_err(|_| AppError("recorder lock poisoned".into()))? = Some(work);
        *self.recording_lock.lock().map_err(|_| AppError("recorder lock poisoned".into()))? =
            self.lock_visit(&id, Holder::Recording).ok();
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
        // The recording's ticket and hold end once its transcript is saved and linked.
        let _work = self.recording_work.lock().map_err(|_| AppError("recorder lock poisoned".into()))?.take();
        let _lock = self.recording_lock.lock().map_err(|_| AppError("recorder lock poisoned".into()))?.take();
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
        let _lock = self.lock_visit(id, Holder::Saving)?;
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
        // One draft of a visit at a time, and no delete meanwhile (ADR-0022 section 7).
        let _lock = self.lock_visit(id, Holder::Drafting)?;
        let _awake = AwakeGuard::acquire(AwakeReason::Processing);
        let mut s = self.open(id, pet)?;
        // Their own version, then the hospital's, then the built-in (ADR-0024).
        let wanted = if template.is_empty() { templates::DEFAULT_TEMPLATE } else { template };
        let (template, notice) = self.drafting_template(wanted, who.as_ref())?;
        // Headings for other species are left out (M2 item 4).
        let mut template =
            Self::with_last_dentition(&s, template.for_species(self.species_of(&s).as_deref()));
        // The run records exactly what drafted it.
        let exact = os_engine::template::fingerprint(&template.template);
        if let Some(origin) = template.template.origin.as_mut() {
            origin.fingerprint = exact;
        }
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
        Ok(NoteResponse { markdown: result.markdown.clone(), run: serde_json::to_value(&result)?, notice })
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
        let _lock = self.lock_visit(id, Holder::Drafting)?;
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
        // Fail before drafting if the standard can't be saved: the scope, the
        // admin's PIN for a standard for everyone, and the heading.
        let scope = if request.save_as_standard {
            Some(self.check_standard_request(
                &template.template.id,
                &request.heading,
                request.by.as_deref(),
                request.standard_scope,
                request.approver.as_ref(),
            )?)
        } else {
            None
        };
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
        if let Some(scope) = scope {
            self.save_standard_after_refine(
                &template.template.id,
                &request.heading,
                &request.request,
                request.by.as_deref(),
                scope,
            )?;
        }
        if let Some(r) = result.refinement.as_mut() {
            r.from_run = Some(run.dir.clone());
            r.saved_as_standard = request.save_as_standard;
        }
        result.by = who;
        result.markdown = with_header(&s, &result.markdown);
        s.save_run(&request.llm, &result, &template)?;
        Ok(NoteResponse {
            markdown: result.markdown.clone(),
            run: serde_json::to_value(&result)?,
            notice: None,
        })
    }

    /// Save a veterinarian's own version of one heading as a new draft.
    /// Changed items are marked as entered by them; the earlier draft is kept.
    ///
    /// `dentition` ("canine" or "feline"): the animal the vet chose in the
    /// chart editor when the chart had none (ADR-0021 decision 4). It is
    /// written into the new run's template snapshot, so later edits, re-prompts
    /// and the PDF draw the same mouth.
    #[allow(clippy::too_many_arguments)]
    pub fn edit_note(
        &self,
        id: &str,
        heading: &str,
        value: serde_json::Value,
        by: &str,
        pet: Option<&str>,
        dentition: Option<&str>,
    ) -> Result<NoteResponse> {
        let _work = self.begin_work()?;
        let who = self
            .who(Some(by), Need::Notes)?
            .ok_or_else(|| AppError("Add your name in Settings, then save your edits.".into()))?;
        let _lock = self.lock_visit(id, Holder::Saving)?;
        let mut s = self.open(id, pet)?;
        if s.is_signed() {
            return Err(AppError(
                "This note is signed, so it can't be changed. Add an addendum instead.".into(),
            ));
        }
        let (run, note) = s.latest_note()?;
        let mut template = s.run_template(&run)?;
        if let Some(chosen) = dentition.map(str::trim).filter(|d| !d.is_empty()) {
            template = Self::with_chosen_dentition(&template, heading, chosen, &value)?;
        }
        let transcript = s.load_transcript()?;
        let mut result =
            os_engine::edit::apply_edit(&template, &transcript, &note, heading, value, &who.name, &run.llm)?;
        result.by = Some(who);
        if let Some(e) = result.edit.as_mut() {
            e.from_run = Some(run.dir.clone());
        }
        result.markdown = with_header(&s, &result.markdown);
        s.save_run("edited", &result, &template)?;
        Ok(NoteResponse {
            markdown: result.markdown.clone(),
            run: serde_json::to_value(&result)?,
            notice: None,
        })
    }

    /// The run's template with the chart's dentition the vet chose. Refused
    /// when the heading isn't the chart, when it already has another
    /// dentition, or when a charted tooth isn't in the chosen animal's mouth.
    fn with_chosen_dentition(
        template: &LoadedTemplate,
        heading: &str,
        chosen: &str,
        value: &serde_json::Value,
    ) -> Result<LoadedTemplate> {
        use os_engine::dental::{Dentition, tooth_misfit};
        let d = os_engine::template::species_key(chosen)
            .and_then(Dentition::from_species_key)
            .ok_or_else(|| AppError(format!("A tooth chart is for a dog or a cat, not \"{chosen}\".")))?;
        match template.chart() {
            Some((path, _)) if path == heading => {}
            _ => return Err(AppError(format!("'{heading}' isn't a tooth chart."))),
        }
        let refused: Vec<String> = value
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|i| i["tooth"].as_str())
            .filter(|t| tooth_misfit(t, Some(d)).is_some())
            .map(String::from)
            .collect();
        if !refused.is_empty() {
            return Err(AppError(format!(
                "{} aren't in a {}'s mouth. Remove them or choose another animal.",
                refused.join(", "),
                d.animal()
            )));
        }
        Ok(template.with_dentition(heading, d)?)
    }

    /// Add a signed addendum to a signed note.
    pub fn add_addendum(&self, id: &str, text: &str, by: &str, pet: Option<&str>) -> Result<SessionView> {
        let _work = self.begin_work()?;
        let who = self
            .who(Some(by), Need::Sign)?
            .ok_or_else(|| AppError("Add your name in Settings, then sign the addendum.".into()))?;
        let _lock = self.lock_visit(id, Holder::Saving)?;
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

    /// Confirm, or take back, a veterinarian's check of the drafted tooth
    /// chart entries on these teeth of the latest draft (ADR-0021 decision
    /// 10). Signing waits until every drafted entry is confirmed; a signed
    /// chart can't change.
    pub fn confirm_chart(
        &self,
        id: &str,
        teeth: &[String],
        confirmed: bool,
        by: &str,
        pet: Option<&str>,
    ) -> Result<SessionView> {
        let _work = self.begin_work()?;
        let who = self
            .who(Some(by), Need::ConfirmChart)?
            .ok_or_else(|| AppError("Add your name in Settings, then confirm the chart.".into()))?;
        let _lock = self.lock_visit(id, Holder::Saving)?;
        let mut s = self.open(id, pet)?;
        s.confirm_teeth_by(teeth, confirmed, &who)?;
        Ok(view(&s, None))
    }

    /// Mark the latest draft as signed by `by` (refused while numbers are
    /// unsupported or a drafted chart entry isn't confirmed).
    pub fn sign(&self, id: &str, by: &str, pet: Option<&str>) -> Result<SessionView> {
        let _work = self.begin_work()?;
        let who = self
            .who(Some(by), Need::Sign)?
            .ok_or_else(|| AppError("Enter the name of the veterinarian signing the note.".into()))?;
        let _lock = self.lock_visit(id, Holder::Saving)?;
        let mut s = self.open(id, pet)?;
        s.sign_latest_by(&who)?;
        Ok(view(&s, None))
    }

    // ---- deleting (ADR-0022) ---------------------------------------------

    /// Move a visit to Deleted visits for 30 days. A veterinarian or
    /// technician (single-user: the name in Settings) gives a reason. The
    /// steps are: rename into `.openbark/deleted/`, write `deleted.json`, then
    /// add the deletion record entry; if the entry can't be written, the
    /// rename is undone. Signed visits are refused in phase 1.
    pub fn delete_visit(&self, id: &str, req: &DeleteRequest) -> Result<DeletedVisit> {
        let _work = self.begin_work()?;
        let dir = self.session_dir(id)?;
        let _lock = self.lock_visit(id, Holder::Deleting)?;
        if self.recording_into(&dir) {
            return Err(AppError(Holder::Recording.refusal(Holder::Deleting).into()));
        }
        let who = self
            .who(req.by.as_deref(), Need::Delete)?
            .ok_or_else(|| AppError("Add your name in Settings, then delete the visit.".into()))?;
        let s = Session::open(&dir)?;
        if s.any_signed() {
            return Err(AppError(SIGNED_NOT_YET.into()));
        }
        let trash = self.trash();
        let trash_id = trash.new_id(&s.meta.id);
        let now = Local::now();
        let reason = DeleteReason { kind: req.reason.kind, note: clean_note(&req.reason.note) };
        let info = DeletedInfo {
            trash_id: trash_id.clone(),
            original_name: id.to_string(),
            deleted_at: now,
            by: who.clone(),
            reason: reason.clone(),
            signed: false,
            purge_after: trash::purge_after(now),
            event_id: audit::new_id(),
            recovered: false,
        };
        let tombstone = audit::tombstone(&s, Some(&trash_id));
        let trashed = trash.put(&dir, &info)?;
        let mut entry = EventDraft::new(EventKind::VisitDeleted, who, self.users.is_team());
        entry.id = Some(info.event_id.clone());
        entry.visit = Some(tombstone);
        entry.reason = Some(reason);
        if let Err(e) = self.audit().append(entry) {
            if let Err(back) = trash.undo_put(&trashed, &dir) {
                tracing::error!(error = %back, "a deleted visit couldn't be put back after the record failed");
            }
            return Err(AppError(format!("{NOT_RECORDED} {e}")));
        }
        let patient = s.meta.patient.as_ref();
        Ok(DeletedVisit {
            trash_id,
            pets: patient.map(|p| p.pets()).unwrap_or_default(),
            signed: false,
            purge_after: stamp(info.purge_after),
            patient_id: patient.and_then(|p| p.patient_id.clone()),
        })
    }

    /// Deleted visits, newest first. Reception can't see them.
    pub fn list_deleted(&self, by: Option<&str>) -> Result<Vec<DeletedSummary>> {
        self.who(by, Need::Delete)?;
        Ok(self
            .trash()
            .list()
            .into_iter()
            .map(|(info, s)| {
                let p = s.meta.patient.as_ref();
                DeletedSummary {
                    bytes: os_models::locations::dir_size(&s.dir),
                    patient: p.map(|p| p.display_name()).unwrap_or_default(),
                    patient_ids: p
                        .map(|p| p.patient_id.iter().chain(&p.other_ids).cloned().collect())
                        .unwrap_or_default(),
                    pets: p.map(|p| p.pets()).unwrap_or_default(),
                    created_at: stamp(s.meta.created_at),
                    deleted_at: stamp(info.deleted_at),
                    by: info.by.name,
                    reason: info.reason,
                    signed: info.signed,
                    purge_after: stamp(info.purge_after),
                    kept_until: None,
                    recovered: info.recovered,
                    trash_id: info.trash_id,
                }
            })
            .collect())
    }

    /// Bring a visit back from Deleted visits, under its original folder name
    /// (with a suffix if that's taken), and link it to its patients again.
    pub fn restore_visit(&self, trash_id: &str, by: Option<&str>) -> Result<RestoredVisit> {
        let _work = self.begin_work()?;
        let who = self
            .who(by, Need::Delete)?
            .ok_or_else(|| AppError("Add your name in Settings, then restore the visit.".into()))?;
        let _lock = self.lock_visit(&format!("deleted:{trash_id}"), Holder::Deleting)?;
        let trash = self.trash();
        let (info, s) = trash.get(trash_id)?;
        let tombstone = audit::tombstone(&s, Some(trash_id));
        let restored = trash.restore(trash_id)?;
        let mut entry = EventDraft::new(EventKind::VisitRestored, who, self.users.is_team());
        entry.visit = Some(tombstone);
        if let Err(e) = self.audit().append(entry) {
            // Back to Deleted visits, as it was.
            if let Err(back) = trash.put(&restored, &info) {
                tracing::error!(error = %back, "a restored visit couldn't go back to Deleted visits");
            }
            return Err(AppError(format!(
                "OpenBark couldn't record the restore, so the visit stays in Deleted visits. {e}"
            )));
        }
        let mut s = Session::open(&restored)?;
        if let Err(e) = self.link_visit(&mut s) {
            tracing::warn!(error = %e.0, "a restored visit couldn't be linked to its patients yet");
        }
        Ok(RestoredVisit {
            id: id_of(&s),
            patient_id: s.meta.patient.as_ref().and_then(|p| p.patient_id.clone()),
        })
    }

    /// "Delete now": remove a deleted visit for good. Rename it to
    /// `.purging`, record the removal, then remove the folder. A signed item
    /// needs a veterinarian, with their PIN when there's a team.
    pub fn purge_deleted(&self, trash_id: &str, by: Option<&str>, pin: Option<&str>) -> Result<()> {
        let _work = self.begin_work()?;
        let _lock = self.lock_visit(&format!("deleted:{trash_id}"), Holder::Deleting)?;
        let trash = self.trash();
        let (info, s) = trash.get(trash_id)?;
        // What the folder holds, not only what deleted.json says: removal
        // can't be undone, and another copy of OpenBark on this folder (the
        // per-visit lock is per process) could have signed it meanwhile.
        let who = if info.signed || s.any_signed() {
            let who = self
                .who(by, Need::DeleteSigned)?
                .ok_or_else(|| AppError("Add your name in Settings, then delete the visit.".into()))?;
            if self.users.is_team() {
                // Checked here, in the same call (ADR-0022 section 2).
                self.users.verify(who.id.as_deref().unwrap_or_default(), pin.unwrap_or_default())?;
            }
            who
        } else {
            self.who(by, Need::Delete)?
                .ok_or_else(|| AppError("Add your name in Settings, then delete the visit.".into()))?
        };
        let tombstone = audit::tombstone(&s, Some(trash_id));
        drop(s);
        let purging = trash.begin_purge(trash_id)?;
        let mut entry = EventDraft::new(EventKind::VisitPurged, who, self.users.is_team());
        entry.visit = Some(tombstone);
        entry.reason = Some(info.reason);
        entry.purge = Some(Purge::ByHand);
        if let Err(e) = self.audit().append(entry) {
            if let Err(back) = trash.undo_purge(&purging) {
                tracing::error!(error = %back, "a visit being removed couldn't go back to Deleted visits");
            }
            return Err(AppError(format!("{NOT_RECORDED} {e}")));
        }
        // Recorded; if a file is in use the folder goes at the next start.
        trash.finish_purge(&purging)?;
        Ok(())
    }

    /// Remove every deleted visit whose 30 days are up (by `now`). Does
    /// nothing while the folders are changing. Returns how many were removed.
    pub fn purge_due_at(&self, now: DateTime<Local>) -> Result<usize> {
        let Ok(_work) = self.begin_work() else { return Ok(0) };
        let trash = self.trash();
        let audit = self.audit();
        let team = self.users.is_team();
        let mut removed = 0;
        for trash_id in trash.due(now) {
            // Phase 2: items the clinic's retention rule keeps are skipped here.
            let Ok(_lock) = self.lock_visit(&format!("deleted:{trash_id}"), Holder::Deleting) else {
                continue;
            };
            let Ok((info, s)) = trash.get(&trash_id) else { continue };
            let tombstone = audit::tombstone(&s, Some(&trash_id));
            drop(s);
            let purging = match trash.begin_purge(&trash_id) {
                Ok(p) => p,
                Err(e) => {
                    tracing::warn!(error = %e, "a deleted visit couldn't be removed yet");
                    continue;
                }
            };
            let mut entry =
                EventDraft::new(EventKind::VisitPurged, Who::named(os_engine::PRODUCT_NAME), team);
            entry.visit = Some(tombstone);
            entry.reason = Some(info.reason);
            entry.purge = Some(Purge::After30Days);
            if let Err(e) = audit.append(entry) {
                tracing::warn!(error = %e, "the removal couldn't be recorded; the visit stays in Deleted visits");
                let _ = trash.undo_purge(&purging);
                continue;
            }
            if let Err(e) = trash.finish_purge(&purging) {
                tracing::warn!(error = %e, "a removed visit's folder goes at the next start");
            }
            removed += 1;
        }
        Ok(removed)
    }

    /// Put right what a crash left in Deleted visits (ADR-0022 section 3):
    /// describe items without `deleted.json`, finish part-done removals, and
    /// add any missing deletion record entries, marked as recovered. Returns
    /// how many entries were added.
    pub fn recover_deleted(&self, now: DateTime<Local>) -> usize {
        let Ok(_work) = self.begin_work() else { return 0 };
        let trash = self.trash();
        let audit = self.audit();
        let team = self.users.is_team();
        let system = Who::named(os_engine::PRODUCT_NAME);
        // A record that can't be read isn't added to blindly. A line torn by
        // the crash being recovered from is left out, not a reason to wait.
        let events = match audit.read_readable() {
            Ok(events) => events,
            Err(e) => {
                tracing::warn!(error = %e, "the deletion record can't be read; recovery waits");
                return 0;
            }
        };
        let mut added = 0;
        for recovered in trash.recover(now) {
            let Recovered::Purged { trash_id, info, session } = recovered else { continue };
            let recorded = events.iter().any(|e| {
                e.kind == EventKind::VisitPurged
                    && e.visit.as_ref().and_then(|v| v.trash_id.as_deref()) == Some(trash_id.as_str())
            });
            if recorded {
                continue;
            }
            let mut entry = EventDraft::new(EventKind::VisitPurged, system.clone(), team);
            entry.visit = session.as_ref().map(|s| audit::tombstone(s, Some(&trash_id)));
            entry.purge = Some(match &info {
                Some(i) if i.purge_after > now => Purge::ByHand,
                _ => Purge::After30Days,
            });
            entry.reason = info.map(|i| i.reason);
            entry.recovered = true;
            if audit.append(entry).is_ok() {
                added += 1;
            }
        }
        // Deleted visits whose entry is missing (including any just described).
        for (info, s) in trash.list() {
            if events.iter().any(|e| e.id == info.event_id) {
                continue;
            }
            let mut entry = EventDraft::new(EventKind::VisitDeleted, info.by.clone(), team);
            entry.id = Some(info.event_id.clone());
            entry.visit = Some(audit::tombstone(&s, Some(&info.trash_id)));
            entry.reason = Some(info.reason.clone());
            entry.recovered = true;
            if audit.append(entry).is_ok() {
                added += 1;
            }
        }
        added
    }

    /// Recovery, then removal of what's due: at start, and hourly.
    pub fn housekeeping_now(&self, now: DateTime<Local>) {
        let recovered = self.recover_deleted(now);
        let removed = self.purge_due_at(now).unwrap_or(0);
        if recovered + removed > 0 {
            tracing::info!(recovered, removed, "Deleted visits housekeeping");
        }
    }

    /// Start housekeeping for Deleted visits: once now, then hourly while this
    /// App is in use. Called by the desktop app's setup and by `serve` (on
    /// start and for each fresh App after a folder change), never by
    /// [`App::new`], so tests never run it in the background.
    pub fn start_housekeeping(self: &Arc<Self>) {
        let app = Arc::downgrade(self);
        let spawned = std::thread::Builder::new().name("openbark-housekeeping".into()).spawn(move || {
            loop {
                {
                    let Some(app) = app.upgrade() else { return };
                    if app.gate.retired.load(std::sync::atomic::Ordering::SeqCst) {
                        return;
                    }
                    app.housekeeping_now(Local::now());
                }
                for _ in 0..60 {
                    std::thread::sleep(std::time::Duration::from_secs(60));
                    if app.strong_count() == 0 {
                        return;
                    }
                }
            }
        });
        if let Err(e) = spawned {
            tracing::warn!(error = %e, "Deleted visits housekeeping couldn't start");
        }
    }

    /// Delete only the recording ("Delete this recording? The note stays.").
    /// The transcript, drafts, signed notes and addenda are kept. On a signed
    /// visit only a veterinarian can. Recorded first, then removed.
    pub fn delete_recording(&self, id: &str, by: Option<&str>) -> Result<SessionView> {
        let _work = self.begin_work()?;
        let dir = self.session_dir(id)?;
        let _lock = self.lock_visit(id, Holder::Deleting)?;
        if self.recording_into(&dir) {
            return Err(AppError(Holder::Recording.refusal(Holder::Deleting).into()));
        }
        let mut s = Session::open(&dir)?;
        let need = if s.any_signed() { Need::DeleteSignedRecording } else { Need::Delete };
        let who = self
            .who(by, need)?
            .ok_or_else(|| AppError("Add your name in Settings, then delete the recording.".into()))?;
        if !s.has_audio() {
            return Err(AppError("This visit has no recording.".into()));
        }
        let mut entry = EventDraft::new(EventKind::RecordingDeleted, who.clone(), self.users.is_team());
        entry.visit = Some(audit::tombstone(&s, None));
        self.audit().append(entry).map_err(|e| {
            AppError(format!("OpenBark couldn't record the deletion, so the recording was kept. {e}"))
        })?;
        s.delete_audio(&who)?;
        Ok(view(&s, None))
    }

    // ---- templates -------------------------------------------------------

    /// Load a note model into the slot, as a draft would, without drafting:
    /// the quit checks (the desktop self-test, os-cli's exit_order example)
    /// need a model on the GPU at exit, and GitHub's Mac runners draft on
    /// their paravirtual GPU too slowly to wait for.
    #[doc(hidden)]
    pub fn preload_llm(&self, llm: &str) -> Result<()> {
        self.load_llm(llm).map(drop)
    }

    /// Propose a template from a note the clinic already writes (M2 item 5).
    /// Nothing is saved: the proposal opens in the builder for editing.
    pub fn template_from_example(&self, llm: &str, example: &str) -> Result<TemplateView> {
        let _awake = AwakeGuard::acquire(AwakeReason::Processing);
        let backend = self.load_llm(llm)?;
        let mut t = os_engine::from_example::template_from_example(backend.as_ref(), example)?;
        // A new template: its id comes from the name it's saved under.
        t.template.id = String::new();
        Ok(self.proposal_view(t))
    }

    /// [`App::load_llm`], telling the drafting board when the model has to be
    /// loaded first (it stays loaded for the next draft).
    fn load_llm_with_progress(&self, id: &str, events: &dyn Events) -> Result<Arc<dyn LlmBackend>> {
        let loaded = self.llm.lock().map(|slot| slot.as_ref().is_some_and(|(l, _)| l == id)).unwrap_or(false);
        if !loaded {
            events.progress(&Progress::new(Stage::LoadModel, 0.0, 1.0));
        }
        self.load_llm(id)
    }

    /// The note model, loaded by its layout: llama.cpp for GGUF, the MLX
    /// worker for MLX folders. It stays loaded for the next draft.
    fn load_llm(&self, id: &str) -> Result<Arc<dyn LlmBackend>> {
        let mut slot = self.llm.lock().map_err(|_| AppError("model lock poisoned".into()))?;
        if os_llm::shutdown::stopping() {
            return Err(AppError("OpenBark is closing.".into()));
        }
        if let Some((loaded, backend)) = slot.as_ref()
            && loaded == id
        {
            return Ok(backend.clone());
        }
        *slot = None; // one model per process: free before loading
        // While it loads (minutes, for MLX), status calls read this instead
        // of waiting for the slot.
        let loading_mlx =
            hub::full_catalog(&self.store).get(id).is_some_and(|m| m.llm_engine() == Some(LlmEngine::Mlx));
        self.mlx_in_slot.store(loading_mlx, std::sync::atomic::Ordering::SeqCst);
        let backend = match open_llm(&self.store, id, None) {
            Ok(b) => b,
            Err(e) => {
                self.mlx_in_slot.store(false, std::sync::atomic::Ordering::SeqCst);
                return Err(e);
            }
        };
        let mlx = backend.capabilities().backend == "mlx";
        if let Ok(mut loaned) = self.loaned.lock() {
            loaned.retain(|(_, _, w)| w.strong_count() > 0);
            loaned.push((id.to_string(), mlx, Arc::downgrade(&backend)));
        }
        *slot = Some((id.to_string(), backend.clone()));
        self.mlx_in_slot.store(mlx, std::sync::atomic::Ordering::SeqCst);
        Ok(backend)
    }

    /// Before the process exits: stop running drafts and free every note
    /// model, so nothing of llama.cpp is alive when the C++ static
    /// destructors run (ggml's Metal teardown aborts otherwise; see
    /// `os_llm::shutdown`). Waits up to `timeout`; true when every model was
    /// freed. Call [`os_llm::shutdown::exit`] afterwards (or return from
    /// `main`). The app refuses to load models from here on.
    pub fn shutdown(&self, timeout: std::time::Duration) -> bool {
        os_llm::shutdown::begin();
        let deadline = std::time::Instant::now() + timeout;
        let released = self.release_llms(deadline);
        // A handle's count reaches zero just before the model behind it is
        // freed (Arc drops the value after the count), so wait for that too.
        let freed = released
            && os_llm::shutdown::wait_released(deadline.saturating_duration_since(std::time::Instant::now()));
        if !freed {
            tracing::warn!(
                live = os_llm::shutdown::live_models(),
                "a note model was still in use when OpenBark closed"
            );
        }
        freed
    }

    /// Empty the note-model slot and wait for drafts still holding a model
    /// to let go, until `deadline`. True when no handed-out model is alive.
    /// The slot's lock is tried rather than waited on: a model load holds it.
    fn release_llms(&self, deadline: std::time::Instant) -> bool {
        let pause = std::time::Duration::from_millis(10);
        loop {
            match self.llm.try_lock() {
                Ok(mut slot) => {
                    *slot = None;
                    break;
                }
                Err(std::sync::TryLockError::Poisoned(p)) => {
                    *p.into_inner() = None;
                    break;
                }
                Err(std::sync::TryLockError::WouldBlock) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(pause)
                }
                Err(std::sync::TryLockError::WouldBlock) => return false,
            }
        }
        self.mlx_in_slot.store(false, std::sync::atomic::Ordering::SeqCst);
        while self.loaned_out(|_, _| true) {
            if std::time::Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(pause);
        }
        true
    }

    /// Something outside the slot still holds a matching model: a draft that
    /// started before the slot changed. Call after dropping the slot's copy.
    fn loaned_out(&self, matches: impl Fn(&str, bool) -> bool) -> bool {
        self.loaned
            .lock()
            .map(|l| l.iter().any(|(id, mlx, w)| matches(id, *mlx) && w.strong_count() > 0))
            .unwrap_or(false)
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
            deleted_bytes: self.trash().bytes(),
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
                    self.mlx_in_slot.store(false, SeqCst);
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
        let in_place = m.in_place();
        ModelRow {
            id: m.id.clone(),
            name: m.display_name.clone(),
            publisher: match &m.source {
                Source::Huggingface { repo, .. } => repo.split('/').next().unwrap_or_default().into(),
                Source::Url => "k2-fsa".into(),
                Source::Local { .. } => "This computer".into(),
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
            disk_bytes: self.store.disk_usage(m),
            shared_bytes: self.store.shared_bytes(m),
            partial_bytes: self.store.partial_bytes(m),
            origin: origin_of(m, builtin),
            engine: m.llm_engine(),
            parts: m.files.len(),
            remove: row_remove(m),
            missing: in_place.is_some_and(|p| !p.exists()),
            gated: m.gated,
            path: in_place.map(|p| p.display().to_string()),
        }
    }

    /// Mark a model as downloading (or importing) until the guard drops;
    /// a second download of the same model at once is refused.
    fn start_download(&self, id: &str) -> Result<Downloading<'_>> {
        let mut set = self.downloading.lock().map_err(|_| AppError("download list poisoned".into()))?;
        if !set.insert(id.to_string()) {
            return Err(AppError("This model is still downloading.".into()));
        }
        Ok(Downloading { set: &self.downloading, id: id.to_string() })
    }

    /// Download a model. Without `token`, a gated repo's model uses the
    /// saved Hugging Face token; nothing else ever sends one (ADR-0025 §8).
    pub fn download_model(&self, id: &str, token: Option<&str>, events: &dyn Events) -> Result<()> {
        let _work = self.begin_work()?;
        let _awake = AwakeGuard::acquire(AwakeReason::Processing);
        let catalog = hub::full_catalog(&self.store);
        let entry =
            catalog.get(id).ok_or_else(|| AppError(format!("No model called {id} in the catalog.")))?;
        // Gigabytes that could never write a note here.
        if entry.llm_engine() == Some(LlmEngine::Mlx) && !mlx_runs_here() {
            return Err(AppError(MLX_NEEDS.into()));
        }
        let _downloading = self.start_download(id)?;
        let saved = match token {
            Some(_) => None,
            None if entry.gated => secrets::hf_token().map(|(t, _)| t),
            None => None,
        };
        let token = token.or(saved.as_deref());
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

    /// Remove a model (ADR-0025 §10), after the guards in the plan's
    /// contract 10, in order. A running draft is never cancelled: a busy
    /// model is refused, an idle loaded one is unloaded first.
    pub fn remove_model(
        &self,
        id: &str,
        note_model: Option<&str>,
        forget: bool,
        by: Option<&str>,
    ) -> Result<Removal> {
        self.who(by, Need::Models)?;
        let _work = self.begin_work()?;
        let catalog = hub::full_catalog(&self.store);
        let entry =
            catalog.get(id).ok_or_else(|| AppError(format!("No model called {id} in the catalog.")))?;
        if let Some(why) = protected_model(id) {
            return Err(AppError(why.into()));
        }
        // Both held to the end: nothing loads this model, and no download of
        // it starts, mid-removal. The slot first (a model load holds it for
        // minutes), then the download list, which is never held while waiting
        // for the slot, so downloads can start and finish during a load.
        let mut slot = self.llm.lock().map_err(|_| AppError("model lock poisoned".into()))?;
        let downloading = self.downloading.lock().map_err(|_| AppError("download list poisoned".into()))?;
        if downloading.contains(id) {
            return Err(AppError("This model is still downloading.".into()));
        }
        if note_model.map(str::trim) == Some(id) {
            return Err(AppError(
                "This model writes your notes. Choose another one under In use first.".into(),
            ));
        }
        if let Some((loaded, backend)) = slot.as_ref()
            && loaded == id
        {
            if Arc::strong_count(backend) > 1 {
                return Err(AppError(MODEL_BUSY.into()));
            }
            *slot = None;
            self.mlx_in_slot.store(false, std::sync::atomic::Ordering::SeqCst);
        }
        if self.loaned_out(|loaned, _| loaned == id) {
            return Err(AppError(MODEL_BUSY.into()));
        }
        let builtin = os_models::Catalog::builtin().get(id).is_some();
        let removed = removal::remove(&self.store, entry, builtin, forget)?;
        drop(downloading);
        drop(slot);
        tracing::info!(
            id,
            freed = removed.freed_bytes,
            shared = removed.shared_bytes,
            forgotten = removed.forgotten,
            "model removed"
        );
        Ok(removed)
    }

    /// Look at a model file or MLX folder on this computer, changing nothing.
    /// It reveals what is at a path on the computer running OpenBark (files,
    /// sizes, free space), so with a team it needs a veterinarian, as adding does.
    pub fn inspect_import(&self, path: &str, by: Option<&str>) -> Result<ImportPreview> {
        self.who(by, Need::Models)?;
        let preview = os_models::local::inspect(&self.store, Path::new(path.trim()))?;
        if preview.kind == ImportKind::Mlx && !mlx_runs_here() {
            return Err(AppError(MLX_NEEDS.into()));
        }
        Ok(preview)
    }

    /// Add a model from this computer: copied into the store, or used where
    /// it is (ADR-0025 §9). Progress goes out as downloads of the new id.
    pub fn import_model(
        &self,
        path: &str,
        mode: ImportMode,
        by: Option<&str>,
        events: &dyn Events,
    ) -> Result<ModelRow> {
        self.who(by, Need::Models)?;
        let _work = self.begin_work()?;
        let _awake = AwakeGuard::acquire(AwakeReason::Processing);
        let preview = self.inspect_import(path, by)?;
        let _downloading = self.start_download(&preview.id)?;
        let id = preview.id.clone();
        let total = preview.total_bytes;
        let mut before = 0u64;
        let mut current = String::new();
        let mut file_done = 0u64;
        let entry = os_models::local::import(
            &self.store,
            Path::new(path.trim()),
            mode,
            &mut |file: &str, done: u64, _t: u64| {
                if file != current {
                    before += file_done;
                    current = file.to_string();
                }
                file_done = done;
                events.download(&id, (before + done).min(total), total);
            },
        )?;
        Ok(self.row(&entry, false))
    }

    // ---- MLX engine pack (ADR-0025 §4) ------------------------------------

    pub fn mlx_status(&self) -> MlxStatus {
        let support = mlx_pack::support();
        let pack_installed = mlx_pack::installed();
        let record = pack_key().and_then(|k| mlx_pack::check_result(&k));
        let check = match &record {
            _ if !pack_installed => CheckState::NotRun,
            None => CheckState::NotRun,
            Some(r) if r.ok => CheckState::Passed,
            Some(_) => CheckState::Failed,
        };
        let metal =
            pack_installed && metal_key().and_then(|k| mlx_pack::check_result(&k)).is_some_and(|r| r.ok);
        let download_bytes = match mlx_pack::entry() {
            Some(e) if !pack_installed => {
                e.files.iter().filter(|f| !self.store.file_ok(&e, f, Verify::Size)).map(|f| f.size).sum()
            }
            _ => 0,
        };
        let mut versions = BTreeMap::new();
        if let Some(lock) = mlx_pack::lock() {
            versions.insert("python".to_string(), lock.python.version.clone());
            for (name, key) in [("mlx", "mlx"), ("mlx-lm", "mlx_lm"), ("llguidance", "llguidance")] {
                if let Some(w) = lock.wheels.iter().find(|w| w.name.eq_ignore_ascii_case(name)) {
                    versions.insert(key.to_string(), w.version.clone());
                }
            }
        }
        // Never wait for the slot: a model load holds its lock for minutes.
        let in_use = match self.llm.try_lock() {
            Ok(slot) => slot.as_ref().is_some_and(|(_, b)| b.capabilities().backend == "mlx"),
            Err(_) => self.mlx_in_slot.load(std::sync::atomic::Ordering::SeqCst),
        };
        MlxStatus {
            support,
            macos_version: mlx_pack::macos_version(),
            pack_installed,
            installing: MLX_INSTALLING.load(std::sync::atomic::Ordering::SeqCst),
            check,
            check_message: if check == CheckState::Failed { record.and_then(|r| r.message) } else { None },
            metal,
            download_bytes,
            installed_bytes: if pack_installed { mlx_pack::disk_bytes() } else { 0 },
            versions,
            in_use,
        }
    }

    /// Download, unpack and check the MLX engine pack. Progress goes out as
    /// one download, `mlx-engine-pack`. A failed check shows on the card.
    pub fn install_mlx(&self, events: &dyn Events) -> Result<MlxStatus> {
        use std::sync::atomic::Ordering::SeqCst;
        if !mlx_runs_here() {
            return Err(AppError(MLX_NEEDS.into()));
        }
        // One install at a time, across tabs and windows.
        if MLX_INSTALLING.compare_exchange(false, true, SeqCst, SeqCst).is_err() {
            return Err(AppError(MLX_DOWNLOADING.into()));
        }
        struct Done;
        impl Drop for Done {
            fn drop(&mut self) {
                MLX_INSTALLING.store(false, std::sync::atomic::Ordering::SeqCst);
            }
        }
        let _done = Done;
        let _work = self.begin_work()?;
        let _awake = AwakeGuard::acquire(AwakeReason::Processing);
        let result =
            install_mlx_pack(&self.store, &mut |done, total| events.download(mlx_pack::PACK_ID, done, total));
        match result {
            Ok(_) => {}
            // The check ran and failed, or couldn't run: recorded, and the card says so.
            Err(_) if mlx_pack::installed() => {}
            Err(e) => return Err(e),
        }
        Ok(self.mlx_status())
    }

    /// Run the pack check again ("Try again").
    pub fn check_mlx(&self) -> Result<MlxStatus> {
        if !mlx_runs_here() {
            return Err(AppError(MLX_NEEDS.into()));
        }
        if MLX_INSTALLING.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(AppError(MLX_DOWNLOADING.into()));
        }
        if !mlx_pack::installed() {
            return Err(AppError(MLX_FIRST.into()));
        }
        // A failure is recorded and shown on the card, not raised.
        let _ = run_mlx_pack_check();
        Ok(self.mlx_status())
    }

    /// Remove the MLX engine pack. MLX models stay on disk.
    pub fn remove_mlx(&self, note_model: Option<&str>, by: Option<&str>) -> Result<Removal> {
        self.who(by, Need::Models)?;
        let _work = self.begin_work()?;
        if let Some(note) = note_model.map(str::trim).filter(|n| !n.is_empty())
            && hub::full_catalog(&self.store)
                .get(note)
                .is_some_and(|m| m.llm_engine() == Some(LlmEngine::Mlx))
        {
            return Err(AppError(
                "Your note model uses the MLX engine. Choose another one under In use first.".into(),
            ));
        }
        if MLX_INSTALLING.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(AppError(MLX_DOWNLOADING.into()));
        }
        const MLX_BUSY: &str =
            "A note is being written with an MLX model. Remove the engine when that's finished.";
        let mut slot = self.llm.lock().map_err(|_| AppError("model lock poisoned".into()))?;
        if let Some((_, backend)) = slot.as_ref()
            && backend.capabilities().backend == "mlx"
        {
            if Arc::strong_count(backend) > 1 {
                return Err(AppError(MLX_BUSY.into()));
            }
            *slot = None;
            self.mlx_in_slot.store(false, std::sync::atomic::Ordering::SeqCst);
        }
        if self.loaned_out(|_, mlx| mlx) {
            return Err(AppError(MLX_BUSY.into()));
        }
        let freed = mlx_pack::remove(&self.store)
            .map_err(|e| AppError(format!("The MLX engine couldn't be removed: {e}.")))?;
        drop(slot);
        tracing::info!(freed, "MLX engine removed");
        Ok(Removal { id: mlx_pack::PACK_ID.into(), freed_bytes: freed, ..Removal::default() })
    }

    // ---- Hugging Face token (ADR-0025 §8) ---------------------------------

    pub fn hf_token_status(&self) -> HfTokenStatus {
        let source = secrets::hf_token().map(|(_, source)| source);
        let user = source.and_then(|_| self.hf_user.lock().ok().and_then(|u| u.clone()));
        HfTokenStatus { saved: source.is_some(), source, user }
    }

    /// Check a token with Hugging Face (`whoami-v2`, once), then save it.
    pub fn set_hf_token(&self, token: &str, by: Option<&str>) -> Result<HfTokenStatus> {
        self.who(by, Need::Models)?;
        let token = token.trim();
        if !secrets::plausible_token(token) {
            return Err(AppError(hub::HubError::BadToken.to_string()));
        }
        let name = hub::whoami(token)?;
        secrets::set_hf_token(token)
            .map_err(|e| AppError(format!("OpenBark couldn't save the token: {e}.")))?;
        if let Ok(mut user) = self.hf_user.lock() {
            *user = Some(name);
        }
        Ok(self.hf_token_status())
    }

    pub fn clear_hf_token(&self, by: Option<&str>) -> Result<HfTokenStatus> {
        self.who(by, Need::Models)?;
        secrets::clear_hf_token()
            .map_err(|e| AppError(format!("OpenBark couldn't remove the token: {e}.")))?;
        if let Ok(mut user) = self.hf_user.lock() {
            *user = None;
        }
        Ok(self.hf_token_status())
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
        // What Remove frees: the libraries it deletes (the processor-only
        // ONNX Runtime stays) and the full-precision models.
        let library_bytes: u64 = match os_models::gpu_pack::program_dir() {
            Some(dir) if pack_installed => os_models::gpu_pack::libraries()
                .iter()
                .filter(|n| *n != "onnxruntime.dll" && *n != "onnxruntime_providers_shared.dll")
                .filter_map(|n| std::fs::metadata(dir.join(n)).ok())
                .map(|m| m.len())
                .sum(),
            _ => 0,
        };
        let installed_bytes = library_bytes + models.iter().map(|m| self.store.disk_usage(m)).sum::<u64>();
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
            installed_bytes,
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
    pub fn remove_gpu(&self, by: Option<&str>) -> Result<GpuStatus> {
        self.who(by, Need::Models)?;
        let _work = self.begin_work()?;
        os_asr::set_gpu_usable(false);
        self.gpu_checked.store(0, std::sync::atomic::Ordering::SeqCst);
        os_models::gpu_pack::remove()
            .map_err(|e| AppError(format!("The GPU speech pack couldn't be removed: {e}.")))?;
        // Only the models' listed files, never a whole folder (ADR-0025 §10).
        for m in self.gpu_models() {
            if let Err(e) = removal::remove_files(&self.store, &m) {
                tracing::warn!(id = %m.id, error = %e, "couldn't remove a GPU recogniser");
            }
        }
        Ok(self.gpu_status())
    }

    pub fn hub_search(&self, query: &str) -> Result<Vec<HubModel>> {
        Ok(hub::search(query, 30, None, platform() == "macos")?)
    }

    /// A repo's details. Metadata and public files without a token; the
    /// saved token only for a gated repo whose files are hidden (ADR-0025 §8).
    fn fetch_repo(&self, repo: &str) -> Result<HubRepo> {
        let r = hub::repo(repo, None)?;
        if r.gated_kind.is_some()
            && r.access == Access::NeedsToken
            && let Some((token, _)) = secrets::hf_token()
        {
            return Ok(hub::repo(repo, Some(&token))?);
        }
        Ok(r)
    }

    pub fn hub_repo(&self, repo: &str) -> Result<HubRepoView> {
        let r = self.fetch_repo(repo)?;
        let fit = |size| hub::fit(size, self.hardware.gpu_memory_bytes, self.hardware.ram_bytes);
        let files = r
            .files
            .iter()
            .map(|f| HubFileView {
                path: f.path.clone(),
                size_bytes: f.size,
                fit: fit(f.size),
                parts: f.parts.len(),
            })
            .collect();
        let mlx = r.mlx.as_ref().map(|m| HubMlxView {
            size_bytes: m.size,
            files: m.files.len(),
            fit: fit(m.size),
            model_type: m.model_type.clone(),
            quantization: m.quantization.clone(),
            problem: m.problem.clone(),
            arch_supported: m.model_type.as_deref().is_some_and(mlx_pack::architecture_supported),
            support: mlx_pack::support(),
            pack_installed: mlx_pack::installed(),
        });
        Ok(HubRepoView { files, mlx, repo: r })
    }

    /// Add a Hub file (or `@mlx`, the whole MLX folder) to the catalog;
    /// download it with [`App::download_model`].
    pub fn hub_add(&self, repo: &str, file: &str, accept_license: bool) -> Result<ModelRow> {
        // An MLX folder can't write a note here: don't offer gigabytes of it.
        if file == hub::MLX_CHOICE && !mlx_runs_here() {
            return Err(AppError(MLX_NEEDS.into()));
        }
        let _work = self.begin_work()?;
        let r = self.fetch_repo(repo)?;
        let entry = hub::add_to_catalog(&self.store, &r, file, accept_license)?;
        Ok(self.row(&entry, false))
    }

    /// GGUF builds of an MLX repo's base model (tokenless).
    pub fn hub_alternatives(&self, base_model: &str) -> Result<Vec<HubModel>> {
        Ok(hub::gguf_alternatives(base_model, 5)?)
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
    /// Confirm drafted tooth chart entries against the recording (veterinarian).
    ConfirmChart,
    Team,
    /// Remove or add models and engine packs, or change the Hugging Face
    /// token, in web mode with a team (veterinarian; ADR-0025 §8-10).
    Models,
    /// Delete, restore or remove an unsigned visit, or see Deleted visits
    /// (veterinarian or technician; ADR-0022 section 1).
    Delete,
    /// Remove a signed visit from Deleted visits (veterinarian).
    DeleteSigned,
    /// Delete the recording of a signed visit (veterinarian).
    DeleteSignedRecording,
    /// Connect or disconnect the clinic's Twilio account, or change the
    /// phone settings (veterinarian; ADR-0023 section 10).
    Phone,
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
#[derive(Clone, serde::Deserialize)]
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
    /// Save the standard for the person asking, or for everyone (ADR-0024 section 8).
    #[serde(default)]
    pub standard_scope: Option<Scope>,
    /// For everyone, in a team: an admin's PIN. Body only, never logged.
    #[serde(default)]
    pub approver: Option<Approver>,
}

/// Written by hand so an approver's PIN never reaches a log.
impl std::fmt::Debug for RefineRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RefineRequest")
            .field("heading", &self.heading)
            .field("save_as_standard", &self.save_as_standard)
            .field("by", &self.by)
            .field("standard_scope", &self.standard_scope)
            .field("approver", &self.approver)
            .finish()
    }
}

pub enum RecordingSource {
    Microphone(Option<String>),
    Stream(std::sync::mpsc::Receiver<AudioBlock>),
}

#[derive(Debug, Clone, Serialize)]
pub struct HubFileView {
    pub path: String,
    pub size_bytes: u64,
    pub fit: Fit,
    /// Files to download: more than one for a split GGUF.
    pub parts: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct HubRepoView {
    pub repo: HubRepo,
    pub files: Vec<HubFileView>,
    pub mlx: Option<HubMlxView>,
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

/// A deletion reason's words, as kept in the record: trimmed, on one line,
/// and short (no clinical details belong there).
fn clean_note(note: &str) -> String {
    note.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(500).collect()
}

fn id_of(s: &Session) -> String {
    folder_name(&s.dir)
}

fn folder_name(dir: &Path) -> String {
    dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

fn stamp(at: DateTime<Local>) -> String {
    at.format("%Y-%m-%d %H:%M").to_string()
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
    let latest_run: Option<serde_json::Value> = latest
        .as_ref()
        .and_then(|d| os_engine::vault::read_to_string(&d.join("note.json")).ok())
        .and_then(|t| serde_json::from_str(&t).ok());
    let unconfirmed_teeth = match (record, &latest_run) {
        (Some(r), Some(saved)) => session::unconfirmed_teeth(r, &saved["note"]),
        _ => Vec::new(),
    };
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
        latest_run,
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
        any_signed: s.any_signed(),
        has_audio: s.has_audio(),
        audio_deleted: s.meta.audio_deleted.clone(),
        chart_checks: record.map(|r| r.chart_checks.clone()).unwrap_or_default(),
        unconfirmed_teeth,
        call: s.meta.call.clone(),
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
mod tests;
