//! `openbark record` and `openbark devices`: live recording from the terminal.

use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::sync::mpsc;

use anyhow::Result;
use os_diar::{ClusterOptions, SplitOptions};
use os_engine::live::{LiveOptions, LiveRecorder, LiveSink, LiveSource};
use os_engine::session::{self, Consent, ConsentMethod, ConsentRule, Session};
use os_engine::speakers::SpeakerTuning;
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
    /// How consent to record was given, saved with the encounter: verbal,
    /// written, on-file, or clinician-only (one-party rule only). Optional.
    #[arg(long)]
    pub consent: Option<String>,
    /// Who confirmed consent (default: the signed-in user).
    #[arg(long)]
    pub consent_by: Option<String>,
    /// The clinic's consent rule: all-parties (default) or one-party.
    #[arg(long, default_value = "all-parties")]
    pub consent_rule: String,
    /// Print each segment as JSON instead of text.
    #[arg(long)]
    pub jsonl: bool,
    /// A client on a phone on speaker in the room: pick up the quieter voice.
    #[arg(long)]
    pub speakerphone: bool,
    /// Write the saved transcript here as JSON Lines once it's complete
    /// (`--jsonl` prints segments as they're heard, before the final pass).
    #[arg(long, hide = true)]
    pub transcript_out: Option<PathBuf>,
    /// Speakers in the room: the number of people talking (default: estimated
    /// when recording stops).
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..=8))]
    pub speakers: Option<u32>,
    #[command(flatten)]
    pub tuning: TuningArgs,
    #[command(flatten)]
    pub llm: LlmArgs,
}

/// Speaker labelling settings, for measuring them (speaker labels eval,
/// ADR-0030). Hidden: the defaults are what the app uses.
#[derive(clap::Args)]
pub struct TuningArgs {
    /// Labels as before ADR-0030 (one per utterance, as shown live); the
    /// options below then change that.
    #[arg(long, hide = true)]
    pub legacy_speakers: bool,
    /// Voice detection: the pause that ends an utterance, seconds.
    #[arg(long, hide = true)]
    pub vad_min_silence: Option<f32>,
    /// Voice detection: the longest utterance, seconds.
    #[arg(long, hide = true)]
    pub vad_max_speech: Option<f32>,
    /// Cut an utterance where the voices either side of a word are less
    /// alike than this; 0 never cuts.
    #[arg(long, hide = true)]
    pub split_threshold: Option<f32>,
    #[arg(long, hide = true)]
    pub split_window: Option<f32>,
    #[arg(long, hide = true)]
    pub split_hop: Option<f32>,
    /// Final labels: voices at least this alike are one speaker; 0 keeps the
    /// labels shown while recording.
    #[arg(long, hide = true)]
    pub cluster_threshold: Option<f32>,
    #[arg(long, hide = true)]
    pub cluster_prior: Option<f32>,
    #[arg(long, hide = true)]
    pub cluster_min_speech: Option<f32>,
}

impl TuningArgs {
    pub fn tuning(&self) -> SpeakerTuning {
        let mut t = if self.legacy_speakers { SpeakerTuning::legacy() } else { SpeakerTuning::default() };
        if let Some(s) = self.vad_min_silence {
            t.min_silence_secs = s;
        }
        if let Some(s) = self.vad_max_speech {
            t.max_speech_secs = s;
        }
        if let Some(x) = self.split_threshold {
            t.split = (x > 0.0).then(|| SplitOptions { threshold: x, ..t.split.unwrap_or_default() });
        }
        if let Some(split) = &mut t.split {
            split.window_secs = self.split_window.unwrap_or(split.window_secs);
            split.hop_secs = self.split_hop.unwrap_or(split.hop_secs);
        }
        if let Some(x) = self.cluster_threshold {
            t.recluster =
                (x > 0.0).then(|| ClusterOptions { threshold: x, ..t.recluster.unwrap_or_default() });
        }
        if let Some(cluster) = &mut t.recluster {
            cluster.prior = self.cluster_prior.unwrap_or(cluster.prior);
            cluster.min_speaker_secs = self.cluster_min_speech.unwrap_or(cluster.min_speaker_secs);
        }
        t
    }
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

    fn mic(&mut self, h: &os_engine::mic_health::MicHealth) {
        use os_engine::mic_health::{MicQuality, MicState};
        if self.meter {
            eprint!("\r{:60}\r", "");
        }
        let note = match h.state {
            MicState::Ok => match (&h.switched_from, h.quality) {
                (Some(from), _) => Some(format!("now recording from {} (was {from})", h.device)),
                (None, MicQuality::Narrowband) => Some(format!("{} sends telephone-quality audio", h.device)),
                (None, MicQuality::HandsFree) => {
                    Some(format!("{} is in Bluetooth hands-free mode", h.device))
                }
                (None, MicQuality::Good) => None,
            },
            MicState::Silent => {
                Some("no sound from the microphone: is it muted, or blocked in privacy settings?".into())
            }
            MicState::Stalled => Some("no audio is arriving from the microphone".into()),
            MicState::Lost => Some("microphone disconnected: paused until one is back".into()),
        };
        if let Some(note) = note {
            eprintln!("microphone: {note}");
        }
    }
}

fn consent_from(args: &RecordArgs) -> Result<Option<Consent>> {
    let Some(method) = &args.consent else { return Ok(None) };
    let consent = Consent {
        rule: ConsentRule::parse(&args.consent_rule)
            .ok_or_else(|| anyhow::anyhow!("--consent-rule is all-parties or one-party"))?,
        method: ConsentMethod::parse(method)
            .ok_or_else(|| anyhow::anyhow!("--consent is verbal, written, on-file or clinician-only"))?,
        confirmed_by: args
            .consent_by
            .clone()
            .or_else(|| std::env::var("USERNAME").ok())
            .or_else(|| std::env::var("USER").ok())
            .unwrap_or_default(),
        confirmed_at: None,
        script: None,
        outcome: None,
        announced_at_secs: None,
    };
    consent.check()?;
    Ok(Some(consent))
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
    options.consent = consent_from(&args)?;
    if is_mic && options.consent.is_none() {
        eprintln!(
            "Reminder: get the client's consent before recording (record it with --consent verbal|written|on-file)."
        );
    }
    options.label_speakers = !args.no_speakers;
    options.speakerphone = args.speakerphone;
    options.room_speakers = args.speakers;
    options.tuning = args.tuning.tuning();
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
    if let Some(path) = &args.transcript_out {
        std::fs::write(path, result.transcript.to_jsonl())?;
    }
    if args.no_note || result.transcript.is_empty() {
        return Ok(());
    }
    let _awake = os_engine::awake::AwakeGuard::acquire(os_engine::awake::AwakeReason::Processing);
    let mut session = Session::open(&result.session_dir)?;
    let template = LoadedTemplate::resolve(&args.llm.template)?;
    let llm = load_llm(store, &args.llm)?;
    let note = generate_note(&*llm, &template, &result.transcript, &note_options(&args.llm), &mut |_| true)?;
    let run = session.save_run(&args.llm.llm, &note, &template)?;
    if !args.jsonl {
        print!("\n{}", note.markdown);
    }
    eprintln!("note saved: {} ({:.1} s)", run.display(), note.stats.total_ms as f64 / 1000.0);
    Ok(())
}
