//! Render a synthetic consult script (packages/eval/fixtures/scripts/*.yaml)
//! to a 16 kHz mono WAV plus a ground-truth `.ref.json` with each line's
//! speaker, text and timing (for WER and diarization scoring).
//!
//! ```text
//! cargo run -p fixture-tts -- --kokoro <dir> packages/eval/fixtures/scripts/*.yaml
//! ```
//! Kokoro-82M (Apache-2.0): k2-fsa sherpa-onnx release `tts-models`,
//! `kokoro-int8-en-v0_19.tar.bz2`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use clap::Parser;
use os_asr::sherpa_onnx::{
    GenerationConfig, LinearResampler, OfflineTts, OfflineTtsConfig, OfflineTtsKokoroModelConfig,
    OfflineTtsModelConfig,
};
use serde::{Deserialize, Serialize};

const OUT_RATE: i32 = 16_000;

/// Speaker ids of kokoro-en-v0_19.
const VOICES: &[&str] = &[
    "af",
    "af_bella",
    "af_nicole",
    "af_sarah",
    "af_sky",
    "am_adam",
    "am_michael",
    "bf_emma",
    "bf_isabella",
    "bm_george",
    "bm_lewis",
];

#[derive(Parser)]
struct Args {
    /// Directory of the extracted kokoro-int8-en-v0_19 model.
    #[arg(long)]
    kokoro: PathBuf,
    /// Output directory (default: packages/eval/fixtures/audio).
    #[arg(long, default_value = "packages/eval/fixtures/audio")]
    out: PathBuf,
    /// Speaking speed (Kokoro runs ~250 words/min at 1.0; people talk slower).
    #[arg(long, default_value_t = 1.0)]
    speed: f32,
    /// Override the script's pause between turns (seconds), e.g. to model exam silences.
    #[arg(long)]
    pause: Option<f32>,
    /// Appended to the output file names, e.g. "-paced".
    #[arg(long, default_value = "")]
    suffix: String,
    scripts: Vec<PathBuf>,
}

#[derive(Deserialize)]
struct Script {
    id: String,
    speakers: BTreeMap<String, String>,
    #[serde(default = "default_pause")]
    pause_secs: f32,
    lines: Vec<(String, String)>,
}

fn default_pause() -> f32 {
    0.6
}

#[derive(Serialize)]
struct RefLine<'a> {
    speaker: &'a str,
    start: f32,
    end: f32,
    text: &'a str,
}

fn main() -> Result<()> {
    let args = Args::parse();
    if args.scripts.is_empty() {
        bail!("give at least one script");
    }
    let model = |f: &str| args.kokoro.join(f).to_string_lossy().into_owned();
    let config = OfflineTtsConfig {
        model: OfflineTtsModelConfig {
            kokoro: OfflineTtsKokoroModelConfig {
                model: Some(model("model.int8.onnx")),
                voices: Some(model("voices.bin")),
                tokens: Some(model("tokens.txt")),
                data_dir: Some(model("espeak-ng-data")),
                length_scale: 1.0,
                ..Default::default()
            },
            num_threads: std::thread::available_parallelism().map(|n| n.get() as i32).unwrap_or(4),
            provider: Some("cpu".into()),
            ..Default::default()
        },
        max_num_sentences: 1,
        ..Default::default()
    };
    let tts = OfflineTts::create(&config)
        .ok_or_else(|| anyhow!("cannot load Kokoro from {}", args.kokoro.display()))?;
    let resampler = LinearResampler::create(tts.sample_rate(), OUT_RATE).context("resampler")?;
    std::fs::create_dir_all(&args.out)?;
    for path in &args.scripts {
        render(&tts, &resampler, path, &args)?;
    }
    Ok(())
}

fn render(tts: &OfflineTts, resampler: &LinearResampler, path: &Path, args: &Args) -> Result<()> {
    let out_dir = args.out.as_path();
    let script: Script = serde_yaml_ng::from_str(&std::fs::read_to_string(path)?)
        .with_context(|| format!("parsing {}", path.display()))?;
    let mut samples: Vec<f32> = Vec::new();
    let mut reference = Vec::new();
    let pause = vec![0.0f32; (args.pause.unwrap_or(script.pause_secs) * OUT_RATE as f32) as usize];
    for (speaker, text) in &script.lines {
        let voice = script.speakers.get(speaker).ok_or_else(|| anyhow!("unknown speaker {speaker}"))?;
        let sid = VOICES.iter().position(|v| v == voice).ok_or_else(|| anyhow!("unknown voice {voice}"))?;
        let config = GenerationConfig { sid: sid as i32, speed: args.speed, ..Default::default() };
        let audio = tts
            .generate_with_config::<fn(&[f32], f32) -> bool>(text, &config, None)
            .ok_or_else(|| anyhow!("TTS failed on: {text}"))?;
        resampler.reset();
        let line = resampler.resample(audio.samples(), true);
        let start = samples.len() as f32 / OUT_RATE as f32;
        samples.extend_from_slice(&line);
        let end = samples.len() as f32 / OUT_RATE as f32;
        reference.push(RefLine { speaker, start, end, text });
        samples.extend_from_slice(&pause);
    }
    let name = format!("{}{}", script.id, args.suffix);
    let wav = out_dir.join(format!("{name}.wav"));
    if !os_asr::sherpa_onnx::write(wav.to_str().context("path")?, &samples, OUT_RATE) {
        bail!("cannot write {}", wav.display());
    }
    std::fs::write(out_dir.join(format!("{name}.ref.json")), serde_json::to_string_pretty(&reference)?)?;
    eprintln!(
        "{}: {:.1} s, {} lines → {}",
        script.id,
        samples.len() as f32 / OUT_RATE as f32,
        reference.len(),
        wav.display()
    );
    Ok(())
}
