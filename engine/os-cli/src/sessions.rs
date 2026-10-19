//! `openbark run`, `openbark rerun` and `openbark sessions`: saved, traceable
//! consults (see `os_engine::session`).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::Subcommand;
use os_asr::Audio;
use os_engine::awake::{AwakeGuard, AwakeReason};
use os_engine::session::{self, Session};
use os_engine::{LoadedTemplate, generate_note};
use os_models::ModelStore;

use crate::{LlmArgs, TranscribeArgs, build_transcriber, load_llm, note_options};

#[derive(Subcommand)]
pub enum SessionsCommand {
    /// List saved sessions, newest first.
    List,
    /// Print the sessions folder.
    Path,
    /// Encrypt encounters saved before encryption was on (ADR-0008).
    Encrypt,
    /// Run the faithfulness check on an encounter's latest note and print
    /// every flag with the statement it applies to (tuning, audits).
    Check {
        /// The encounter's folder name.
        id: String,
    },
}

#[derive(clap::Args)]
pub struct RunArgs {
    #[command(flatten)]
    pub transcribe: TranscribeArgs,
    #[command(flatten)]
    pub llm: LlmArgs,
    /// Patient (animal) name, saved with the encounter.
    #[arg(long)]
    pub patient: Option<String>,
    /// Client / family last name.
    #[arg(long)]
    pub last_name: Option<String>,
    /// Reason for visit.
    #[arg(long)]
    pub reason: Option<String>,
    /// Sessions folder (default: OPENBARK_SESSIONS_DIR or the app data dir).
    #[arg(long)]
    pub sessions_dir: Option<PathBuf>,
}

#[derive(clap::Args)]
pub struct RerunArgs {
    /// A session folder (as printed by `run` or `sessions list`).
    pub session: PathBuf,
    /// Transcribe the saved audio again instead of reusing transcript.jsonl.
    #[arg(long)]
    pub retranscribe: bool,
    /// Label speakers when re-transcribing.
    #[arg(long)]
    pub diarize: bool,
    #[arg(long, default_value = os_engine::models::DEFAULT_STT)]
    pub stt: String,
    #[command(flatten)]
    pub llm: LlmArgs,
}

/// Patient details from CLI flags, if any were given.
pub fn patient_from(
    name: &Option<String>,
    last_name: &Option<String>,
    reason: &Option<String>,
) -> Option<os_engine::session::PatientInfo> {
    let p = os_engine::session::PatientInfo {
        name: name.clone().unwrap_or_default(),
        last_name: last_name.clone().unwrap_or_default(),
        reason: reason.clone().unwrap_or_default(),
        ..Default::default()
    };
    (!p.is_empty()).then_some(p)
}

fn root(dir: Option<&Path>) -> PathBuf {
    dir.map(Path::to_path_buf).unwrap_or_else(session::default_root)
}

/// Record a session from a file: copy the audio, transcribe, draft, save all.
pub fn run(store: &ModelStore, args: RunArgs) -> Result<()> {
    let _awake = AwakeGuard::acquire(AwakeReason::Processing);
    let audio = Audio::load(&args.transcribe.audio)?;
    let mut session = Session::create_from_file(
        &root(args.sessions_dir.as_deref()),
        &args.transcribe.audio,
        audio.duration_secs(),
    )?;
    if let Some(p) = patient_from(&args.patient, &args.last_name, &args.reason) {
        session.set_patient(p)?;
    }
    eprintln!("session: {}", session.dir.display());

    let transcriber = build_transcriber(store, &args.transcribe)?;
    let (transcript, stats) = transcriber.run(&audio, &mut |_| {})?;
    drop(transcriber);
    session.save_transcript(&transcript, &args.transcribe.stt, args.transcribe.diarize, &stats)?;

    draft(store, &mut session, &transcript, &args.llm)
}

/// Re-generate a note for a saved session (optionally re-transcribing).
pub fn rerun(store: &ModelStore, args: RerunArgs) -> Result<()> {
    let _awake = AwakeGuard::acquire(AwakeReason::Processing);
    let mut session = Session::open(&args.session).context("opening session")?;
    let transcript = if args.retranscribe || !session.transcript_path().is_file() {
        if !session.has_audio() && session.meta.audio_deleted.is_some() {
            bail!(
                "This visit's recording was deleted, so it can't be transcribed again. Drafting from the transcript still works."
            );
        }
        let wav = os_engine::vault::read(&session.audio_path())?;
        let audio = Audio::decode_bytes(wav, "audio.wav")?;
        let transcriber = os_engine::models::transcriber(store, &args.stt, args.diarize, None, None, None)?;
        let (transcript, stats) = transcriber.run(&audio, &mut |_| {})?;
        session.save_transcript(&transcript, &args.stt, args.diarize, &stats)?;
        transcript
    } else {
        session.load_transcript()?
    };
    draft(store, &mut session, &transcript, &args.llm)
}

fn draft(
    store: &ModelStore,
    session: &mut Session,
    transcript: &os_engine::Transcript,
    args: &LlmArgs,
) -> Result<()> {
    let template = LoadedTemplate::resolve(&args.template)?;
    let llm = load_llm(store, args)?;
    let result = generate_note(&llm, &template, transcript, &note_options(args), &mut |_| true)?;
    let run_dir = session.save_run(&args.llm, &result, &template)?;
    print!("{}", result.markdown);
    eprintln!("saved: {}", run_dir.display());
    if !result.faithfulness.passed {
        eprintln!(
            "warning: some numbers in the draft are not in the transcript; check them against the recording"
        );
    }
    Ok(())
}

pub fn command(cmd: SessionsCommand) -> Result<()> {
    let root = session::default_root();
    match cmd {
        SessionsCommand::Path => println!("{}", root.display()),
        SessionsCommand::Check { id } => {
            let s = Session::open(&root.join(&id))?;
            let (run, note) = s.latest_note()?;
            let transcript = s.load_transcript()?;
            let report = match s.run_template(&run) {
                Ok(template) => os_engine::faithfulness::check_template(&note, &transcript, &template),
                Err(_) => os_engine::faithfulness::check(&note, &transcript),
            };
            let text_at = |path: &str| {
                let v = note.pointer(path).cloned().unwrap_or_default();
                v["text"]
                    .as_str()
                    .or(v["drug"].as_str())
                    .or(v["value"].as_str())
                    .unwrap_or_default()
                    .to_string()
            };
            println!(
                "{} ({}): {} statements, {} numbers checked",
                id, run.dir, report.statements_checked, report.numbers_checked
            );
            for u in &report.unsupported_numbers {
                println!("NUMBER  {} {:?} not in transcript: {}", u.path, u.number, text_at(&u.path));
            }
            for st in &report.unsupported_statements {
                println!(
                    "WORDS   {} missing {:?} cited {:?} suggest {:?}: {}",
                    st.path,
                    st.missing,
                    st.cited,
                    st.suggested,
                    text_at(&st.path)
                );
            }
            for c in &report.citation_mismatches {
                println!("CITE    {} {:?} found in {:?}", c.path, c.number, c.found_in);
            }
            for f in &report.tooth_flags {
                let block = if f.kind.blocks_signing() { "BLOCKS SIGNING" } else { "check" };
                println!(
                    "TOOTH   {} {} {:?} ({block}) {:?} {:?}",
                    f.path, f.tooth, f.kind, f.other, f.expected
                );
            }
        }
        SessionsCommand::Encrypt => {
            let mut encounters = 0;
            for s in session::list(&root) {
                let files = os_engine::vault::encrypt_folder(&s.dir)?;
                if files > 0 {
                    encounters += 1;
                    eprintln!("encrypted {} ({files} files)", s.dir.display());
                }
            }
            println!("{encounters} encounter(s) encrypted");
        }
        SessionsCommand::List => {
            let sessions = session::list(&root);
            if sessions.is_empty() {
                bail!("no sessions in {}", root.display());
            }
            for s in sessions {
                let m = &s.meta;
                println!(
                    "{}  {:>5.1} min  {} run(s)  {}",
                    m.created_at.format("%Y-%m-%d %H:%M"),
                    m.audio_secs / 60.0,
                    m.runs.len(),
                    s.dir.display()
                );
            }
        }
    }
    Ok(())
}
