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
use os_engine::session::{self, Addendum, Consent, PatientInfo, Session};
use os_engine::template::{Standard, Template};
use os_engine::template_store::TemplateSummary;
use os_engine::users::{Role, User, UserStore, Who};
use os_engine::{LoadedTemplate, NoteOptions, Segment, TemplateStore, TranscribeStats, Transcript};
use os_llm::{LlamaCppBackend, LlamaCppOptions};
use os_models::hub::{self, Fit, HubModel, HubRepo};
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
    pub templates: TemplateStore,
    /// Team members with roles (M1 item 11); empty means single-user.
    pub users: UserStore,
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
    /// Encryption at rest (ADR-0008): on, where the key is, any warning.
    pub encryption: os_engine::vault::VaultStatus,
    /// Encounters saved before encryption, still in plain files.
    pub plain_encounters: usize,
    /// Whether team members are set up (otherwise single-user).
    pub team: bool,
    /// "macos", "windows" or "linux": the Mac shows MLX-optimized models.
    pub platform: String,
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
        Self {
            store,
            sessions_root,
            templates: TemplateStore::default(),
            users: UserStore::default(),
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
        if let Some(c) = &consent {
            c.check()?;
        }
        let recorded_by = self.who(by, Need::Any)?;
        let _awake = AwakeGuard::acquire(AwakeReason::Processing);
        let (tx, rx) = std::sync::mpsc::channel::<Segment>();
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
        // The worker drops the sink when it finishes, which ends this loop.
        for segment in rx {
            events.segment(&segment);
        }
        let result = recorder.wait()?;
        let s = Session::open(&result.session_dir)?;
        Ok(view(&s, Some(result.transcript)))
    }

    /// Same as [`App::import_file`] for uploaded bytes (web mode). Any format
    /// the engine decodes (WAV, m4a, mp3, CAF, FLAC, Ogg); the file name's
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

    /// Set or change the patient details of a saved encounter (time-stamped).
    pub fn update_patient(&self, id: &str, patient: PatientInfo) -> Result<SessionView> {
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
        s.set_patient(patient)?;
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
        let backend = self.load_llm(llm)?;
        let transcript = s.load_transcript()?;
        let options = NoteOptions { reference, ..self.note_options(&s, language.map(String::from)) };
        let mut result =
            os_engine::generate_note(backend.as_ref(), &template, &transcript, &options, &mut |p| {
                events.token(p);
                true
            })?;
        result.by = who;
        result.markdown = with_header(&s, &result.markdown);
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
        let backend = self.load_llm(&request.llm)?;
        let transcript = s.load_transcript()?;
        let options = self.note_options(&s, request.language.clone());
        let mut result = os_engine::refine_note(
            backend.as_ref(),
            &template,
            &transcript,
            &note,
            &request.heading,
            &request.request,
            &options,
            &mut |p| {
                events.token(p);
                true
            },
        )?;
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
