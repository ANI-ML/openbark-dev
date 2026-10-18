//! `openbark record` and `openbark devices`: live recording from the terminal.

use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::sync::mpsc;

use anyhow::Result;
use os_engine::live::{LiveOptions, LiveRecorder, LiveSink, LiveSource};
use os_engine::session::{self, Session};
use os_engine::{LoadedTemplate, Segment, generate_note};
use os_models::ModelStore;

use crate::{LlmArgs, load_llm, note_options};

#[derive(clap::Args)]
pub struct RecordArgs {
    /// Microphone name (see `openbark devices`; default: system default).
    #[arg(long)]
    pub device: Option<String>,
    /// Play a WAV file through the live path instead of the microphone (testing).
    #[arg(long)]
    pub file: Option<PathBuf>,
    /// With --file: pace it like a real microphone instead of as fast as possible.
    #[arg(long)]
    pub realtime: bool,
    /// Don't label speakers.
    #[arg(long)]
    pub no_speakers: bool,
    /// Live speaker-matching threshold (tuning).
    #[arg(long, hide = true)]
    pub speaker_threshold: Option<f32>,
    /// Only transcribe; don't draft a note after stopping.
    #[arg(long)]
    pub no_note: bool,
    /// Patient (animal) name, saved with the encounter.
    #[arg(long)]
    pub patient: Option<String>,
    /// Client / family last name.
    #[arg(long)]
    pub last_name: Option<String>,
    /// Reason for visit.
    #[arg(long)]
    pub reason: Option<String>,
    /// Print each segment as JSON instead of text.
    #[arg(long)]
    pub jsonl: bool,
    #[command(flatten)]
    pub llm: LlmArgs,
}

struct TerminalSink {
    meter: bool,
    jsonl: bool,
}

impl LiveSink for TerminalSink {
    fn level(&mut self, level: f32, seconds: f32) {
        if self.meter {
            let bars = (level * 30.0).round() as usize;
            let t = seconds as u32;
            eprint!("\r● REC {:02}:{:02} [{:<30}] ", t / 60, t % 60, "█".repeat(bars));
            let _ = std::io::stderr().flush();
        }
    }

    fn segment(&mut self, s: &Segment) {
        if self.meter {
            eprint!("\r{:60}\r", "");
        }
        if self.jsonl {
            println!("{}", serde_json::to_string(s).expect("segment serialises"));
        } else {
            let who = s.speaker.map_or("Speaker ?".to_string(), |n| format!("Speaker {}", n + 1));
            println!("[{}] {who}: {}", s.id, s.text);
        }
    }
}

pub fn devices() -> Result<()> {
    let names = os_asr::capture::input_devices();
    if names.is_empty() {
        anyhow::bail!("no microphones found");
    }
    for (i, n) in names.iter().enumerate() {
        println!("{}{}", n, if i == 0 { "  (default)" } else { "" });
    }
    Ok(())
}

pub fn record(store: &ModelStore, args: RecordArgs) -> Result<()> {
    let source = match &args.file {
        Some(path) => LiveSource::File { path: path.clone(), realtime: args.realtime },
        None => LiveSource::Microphone(args.device.clone()),
    };
    let is_mic = matches!(source, LiveSource::Microphone(_));
    let mut options = LiveOptions::microphone();
    options.patient = crate::sessions::patient_from(&args.patient, &args.last_name, &args.reason);
    options.label_speakers = !args.no_speakers;
    if let Some(t) = args.speaker_threshold {
        options.speaker_threshold = t;
    }
    let meter = std::io::stderr().is_terminal() && (is_mic || args.realtime);
    let sink = Box::new(TerminalSink { meter, jsonl: args.jsonl });
    let recorder = LiveRecorder::start(store, &session::default_root(), source, options, sink)?;
    eprintln!("session: {}", recorder.session_dir.display());

    let result = if is_mic {
        eprintln!("Recording. Press Enter to stop.");
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut line = String::new();
            let _ = std::io::stdin().read_line(&mut line);
            let _ = tx.send(());
        });
        let _ = rx.recv();
        recorder.stop()?
    } else {
        recorder.wait()?
    };
    eprintln!(
        "\nrecorded {:.1} min from {} · {} segments · {} speakers · transcript ready {:.1} s after stop",
        result.stats.audio_secs / 60.0,
        result.device,
        result.stats.segments,
        result.stats.speakers,
        result.finish_ms as f64 / 1000.0
    );
    if args.no_note || result.transcript.is_empty() {
        return Ok(());
    }
    let _awake = os_engine::awake::AwakeGuard::acquire(os_engine::awake::AwakeReason::Processing);
    let mut session = Session::open(&result.session_dir)?;
    let template = LoadedTemplate::resolve(&args.llm.template)?;
    let llm = load_llm(store, &args.llm)?;
    let note = generate_note(&llm, &template, &result.transcript, &note_options(&args.llm), &mut |_| true)?;
    let run = session.save_run(&args.llm.llm, &note, &template)?;
    if !args.jsonl {
        print!("\n{}", note.markdown);
    }
    eprintln!("note saved: {} ({:.1} s)", run.display(), note.stats.total_ms as f64 / 1000.0);
    Ok(())
}
