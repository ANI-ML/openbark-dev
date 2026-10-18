//! `openbark`: the OpenBark command line.
//!
//! ```text
//! openbark run consult.wav --diarize          # saved session: audio, transcript, note
//! openbark transcribe consult.wav --diarize | openbark note --template soap
//! ```
//!
//! Only `models pull` touches the network (see docs/network.md).

mod eval;
mod models;
mod probe;
mod record;
mod sessions;
mod templates;

use std::fs;
use std::io::{IsTerminal, Read, Write};
use std::path::PathBuf;
use std::time::Instant;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use os_asr::Audio;
use os_engine::awake::{AwakeGuard, AwakeReason};
use os_engine::{LoadedTemplate, NoteOptions, Transcript, generate_note};
use serde_json::json;

#[derive(Parser)]
#[command(name = "openbark", version, about = "OpenBark: local-first veterinary scribe (pre-alpha)")]
struct Cli {
    /// Model store directory (default: OPENBARK_MODELS_DIR or the platform data dir).
    #[arg(long, global = true)]
    models_dir: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Report hardware and compiled inference backends.
    Probe,
    /// Manage local models.
    #[command(subcommand)]
    Models(models::ModelsCommand),
    /// Transcribe a recording to JSON Lines (one segment per line) on stdout.
    Transcribe(TranscribeArgs),
    /// Generate a draft note from a JSONL transcript (stdin or --input).
    Note(NoteArgs),
    /// Run the whole pipeline in one process and report timings as JSON.
    Bench(BenchArgs),
    /// Transcribe and draft a recording as a saved session (audio + transcript + note).
    Run(sessions::RunArgs),
    /// Draft a new note for a saved session (keeps earlier runs).
    Rerun(sessions::RerunArgs),
    /// Saved sessions.
    #[command(subcommand)]
    Sessions(sessions::SessionsCommand),
    /// Score drafted notes against the synthetic consults' expected facts.
    Eval(eval::EvalArgs),
    /// Record a consult from the microphone with a live transcript (Enter to stop).
    Record(record::RecordArgs),
    /// List microphones.
    Devices,
    /// Note templates: list, show the prompt, check a team template.
    #[command(subcommand)]
    Templates(templates::TemplatesCommand),
    /// Open the OpenBark UI in your browser, served from this machine only (127.0.0.1).
    Serve(ServeArgs),
}

#[derive(clap::Args)]
struct ServeArgs {
    #[arg(long, default_value_t = 4317)]
    port: u16,
    /// Built web UI (default: OPENBARK_UI_DIR, `<exe dir>/ui`, or `apps/desktop/dist`).
    #[arg(long)]
    ui_dir: Option<PathBuf>,
}

#[derive(clap::Args, Clone)]
pub struct TranscribeArgs {
    /// 16-bit PCM mono WAV (any sample rate; resampled to 16 kHz).
    pub audio: PathBuf,
    /// Label speakers with diarization.
    #[arg(long)]
    pub diarize: bool,
    /// Known number of speakers (improves diarization).
    #[arg(long)]
    pub speakers: Option<u32>,
    /// Diarization clustering threshold (higher = fewer speakers).
    #[arg(long, hide = true)]
    pub diar_threshold: Option<f32>,
    /// Diarization window step (fraction of the window).
    #[arg(long, hide = true)]
    pub diar_window_shift: Option<f32>,
    /// Speech-to-text model (catalog id).
    #[arg(long, default_value = os_engine::models::DEFAULT_STT)]
    pub stt: String,
}

#[derive(clap::Args, Clone)]
pub struct LlmArgs {
    /// LLM: catalog id or path to a .gguf file.
    #[arg(long, default_value = os_engine::models::DEFAULT_LLM)]
    pub llm: String,
    /// Layers to offload to GPU (default: all).
    #[arg(long)]
    pub gpu_layers: Option<u32>,
    /// Note template: built-in id or a template directory.
    #[arg(long, default_value = "soap")]
    pub template: String,
    #[arg(long, default_value_t = 4096)]
    pub max_tokens: u32,
}

#[derive(clap::Args)]
struct NoteArgs {
    /// Transcript JSONL file (default: stdin).
    #[arg(long)]
    input: Option<PathBuf>,
    /// Also write the full structured result (note, faithfulness, stats) as JSON.
    #[arg(long)]
    json: Option<PathBuf>,
    /// Stream generated JSON tokens to stderr.
    #[arg(long)]
    stream: bool,
    #[command(flatten)]
    llm: LlmArgs,
}

#[derive(clap::Args)]
struct BenchArgs {
    #[command(flatten)]
    transcribe: TranscribeArgs,
    #[command(flatten)]
    llm: LlmArgs,
    /// Write the note Markdown here.
    #[arg(long)]
    markdown: Option<PathBuf>,
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("OPENBARK_LOG").unwrap_or_else(|_| "warn".into()),
        )
        .init();
    let cli = Cli::parse();
    let store = cli
        .models_dir
        .map(os_models::ModelStore::new)
        .unwrap_or_else(os_models::ModelStore::default_location);

    match cli.command {
        Command::Probe => probe::run(),
        Command::Models(cmd) => models::run(cmd, &store),
        Command::Transcribe(args) => {
            let _awake = AwakeGuard::acquire(AwakeReason::Processing);
            let transcriber = build_transcriber(&store, &args)?;
            let audio = Audio::load_wav(&args.audio)?;
            // Without diarization, stream segments as they are recognised; with
            // it, speakers are only known at the end, so print the final transcript.
            let live = !args.diarize;
            let mut stdout = std::io::stdout().lock();
            let (transcript, stats) = transcriber.run(&audio, &mut |seg| {
                if live {
                    let _ = writeln!(stdout, "{}", serde_json::to_string(seg).expect("segment serialises"));
                    let _ = stdout.flush();
                }
            })?;
            if !live {
                write!(stdout, "{}", transcript.to_jsonl())?;
            }
            eprintln!("{}", serde_json::to_string(&json!({ "transcribe": stats }))?);
            Ok(())
        }
        Command::Note(args) => {
            let _awake = AwakeGuard::acquire(AwakeReason::Processing);
            let input = match &args.input {
                Some(path) => {
                    fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?
                }
                None => {
                    if std::io::stdin().is_terminal() {
                        bail!(
                            "pipe a transcript in, e.g. `openbark transcribe a.wav | openbark note`, or use --input"
                        );
                    }
                    let mut s = String::new();
                    std::io::stdin().read_to_string(&mut s)?;
                    s
                }
            };
            let transcript = Transcript::from_jsonl(&input).context("parsing transcript JSONL")?;
            let template = LoadedTemplate::resolve(&args.llm.template)?;
            let llm = load_llm(&store, &args.llm)?;
            let stream = args.stream;
            let result =
                generate_note(&llm, &template, &transcript, &note_options(&args.llm), &mut |piece| {
                    if stream {
                        eprint!("{piece}");
                    }
                    true
                })?;
            if stream {
                eprintln!();
            }
            if let Some(path) = &args.json {
                fs::write(path, serde_json::to_string_pretty(&result)?)?;
            }
            print!("{}", result.markdown);
            eprintln!(
                "{}",
                serde_json::to_string(
                    &json!({ "note": result.stats, "faithfulness_passed": result.faithfulness.passed })
                )?
            );
            Ok(())
        }
        Command::Bench(args) => bench(&store, args),
        Command::Run(args) => sessions::run(&store, args),
        Command::Rerun(args) => sessions::rerun(&store, args),
        Command::Sessions(cmd) => sessions::command(cmd),
        Command::Serve(args) => serve(store, args),
        Command::Record(args) => record::record(&store, args),
        Command::Devices => record::devices(),
        Command::Templates(cmd) => templates::run(cmd),
        Command::Eval(args) => eval::run(&store, args),
    }
}

pub(crate) fn note_options(args: &LlmArgs) -> NoteOptions {
    NoteOptions { max_tokens: args.max_tokens, ..Default::default() }
}

pub(crate) fn build_transcriber(
    store: &os_models::ModelStore,
    args: &TranscribeArgs,
) -> Result<os_engine::Transcriber> {
    Ok(os_engine::models::transcriber(
        store,
        &args.stt,
        args.diarize,
        args.speakers,
        args.diar_threshold,
        args.diar_window_shift,
    )?)
}

pub(crate) fn load_llm(store: &os_models::ModelStore, args: &LlmArgs) -> Result<os_llm::LlamaCppBackend> {
    let path = os_engine::models::llm_path(store, &args.llm)?;
    let mut options = os_llm::LlamaCppOptions::new(path);
    if let Some(layers) = args.gpu_layers {
        options.gpu_layers = layers;
    }
    Ok(os_llm::LlamaCppBackend::load(options)?)
}

fn bench(store: &os_models::ModelStore, args: BenchArgs) -> Result<()> {
    let _awake = AwakeGuard::acquire(AwakeReason::Processing);
    let memory = probe::PeakMemory::start();
    let started = Instant::now();

    let transcriber = build_transcriber(store, &args.transcribe)?;
    let audio = Audio::load_wav(&args.transcribe.audio)?;
    let (transcript, transcribe_stats) = transcriber.run(&audio, &mut |_| {})?;
    drop(transcriber);

    let t = Instant::now();
    let template = LoadedTemplate::resolve(&args.llm.template)?;
    let llm = load_llm(store, &args.llm)?;
    let llm_load_ms = t.elapsed().as_millis() as u64;
    let result = generate_note(&llm, &template, &transcript, &note_options(&args.llm), &mut |_| true)?;
    let wall_ms = started.elapsed().as_millis() as u64;

    if let Some(path) = &args.markdown {
        fs::write(path, &result.markdown)?;
    }
    let report = json!({
        "audio": args.transcribe.audio,
        "audio_secs": transcribe_stats.audio_secs,
        "stt": args.transcribe.stt,
        "llm": result.model,
        "template": result.template,
        "transcribe": transcribe_stats,
        "llm_load_ms": llm_load_ms,
        "note": result.stats,
        "faithfulness": result.faithfulness,
        "wall_ms": wall_ms,
        "peak_rss_mb": memory.stop() / (1024 * 1024),
        "devices": os_llm::devices(),
    });
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

fn serve(store: os_models::ModelStore, args: ServeArgs) -> Result<()> {
    let ui_dir = args
        .ui_dir
        .or_else(|| std::env::var_os("OPENBARK_UI_DIR").map(PathBuf::from))
        .or_else(|| {
            std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join("ui"))).filter(|d| d.is_dir())
        })
        .unwrap_or_else(|| PathBuf::from("apps/desktop/dist"));
    if !ui_dir.join("index.html").is_file() {
        bail!(
            "web UI not found in {}. Build it with `pnpm --filter @openbark/desktop build`, or pass --ui-dir",
            ui_dir.display()
        );
    }
    let sessions_root = os_engine::session::default_root();
    eprintln!("OpenBark web mode: http://localhost:{}  (this machine only)", args.port);
    eprintln!("  sessions: {}", sessions_root.display());
    eprintln!("  Ctrl+C to stop");
    tokio::runtime::Runtime::new()?.block_on(os_server::serve(os_server::ServerOptions {
        port: args.port,
        store,
        sessions_root,
        ui_dir,
    }))?;
    Ok(())
}
