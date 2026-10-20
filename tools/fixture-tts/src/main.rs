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

mod phone;

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
    /// A speakerphone consult: one speaker heard through a phone on speaker
    /// in the room (see `phone.rs`).
    #[serde(default)]
    phone: Option<phone::PhoneSpec>,
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
    // Voice every line first: a speakerphone mix needs the vet's level.
    let mut voiced: Vec<Vec<f32>> = Vec::new();
    for (speaker, text) in &script.lines {
        let voice = script.speakers.get(speaker).ok_or_else(|| anyhow!("unknown speaker {speaker}"))?;
        let sid = VOICES.iter().position(|v| v == voice).ok_or_else(|| anyhow!("unknown voice {voice}"))?;
        let config = GenerationConfig { sid: sid as i32, speed: args.speed, ..Default::default() };
        let audio = tts
            .generate_with_config::<fn(&[f32], f32) -> bool>(text, &config, None)
            .ok_or_else(|| anyhow!("TTS failed on: {text}"))?;
        resampler.reset();
        voiced.push(resampler.resample(audio.samples(), true));
    }
    // What the room mic hears of each line (the dry line without `phone:`),
    // which may ring on past the line's end (reverb).
    let heard: Vec<Vec<f32>> = match &script.phone {
        None => voiced.clone(),
        Some(spec) => {
            if !script.speakers.contains_key(&spec.speaker) {
                bail!("phone speaker {} isn't one of the script's speakers", spec.speaker);
            }
            let on_phone = |i: usize| script.lines[i].0 == spec.speaker;
            let mut heard: Vec<Vec<f32>> = voiced
                .iter()
                .enumerate()
                .map(|(i, line)| {
                    let seed = spec.seed.wrapping_add(i as u64);
                    if on_phone(i) {
                        // Mostly reverberant: the phone is a metre or more away.
                        phone::in_room(&phone::phone_line(line), -3.0, seed)
                    } else {
                        // The vet is close to the mic: a light touch of the room.
                        phone::in_room(line, 12.0, seed)
                    }
                })
                .collect();
            let level = |want: bool| {
                let lines: Vec<&[f32]> = (0..heard.len())
                    .filter(|i| on_phone(*i) == want)
                    .map(|i| &heard[i][..voiced[i].len()])
                    .collect();
                phone::rms_of(&lines)
            };
            let (room, far) = (level(false), level(true));
            let gain = room * 10f32.powf(spec.level_db / 20.0) / far.max(1e-9);
            for (i, line) in heard.iter_mut().enumerate() {
                if on_phone(i) {
                    line.iter_mut().for_each(|s| *s *= gain);
                }
            }
            heard
        }
    };
    let mut samples: Vec<f32> = Vec::new();
    let mut reference = Vec::new();
    let pause = (args.pause.unwrap_or(script.pause_secs) * OUT_RATE as f32) as usize;
    for (((speaker, text), line), dry) in script.lines.iter().zip(&heard).zip(&voiced) {
        let at = samples.len();
        let start = at as f32 / OUT_RATE as f32;
        let end = (at + dry.len()) as f32 / OUT_RATE as f32;
        reference.push(RefLine { speaker, start, end, text });
        samples.resize(samples.len().max(at + line.len()), 0.0);
        for (s, x) in samples[at..].iter_mut().zip(line) {
            *s += x;
        }
        // The next line starts a pause after this one's dry end.
        samples.resize(samples.len().max(at + dry.len() + pause), 0.0);
        samples.truncate(at + dry.len() + pause.max(line.len().saturating_sub(dry.len())));
    }
    if let Some(spec) = &script.phone {
        let vet: Vec<&[f32]> = voiced
            .iter()
            .zip(&script.lines)
            .filter(|(_, (s, _))| *s != spec.speaker)
            .map(|(l, _)| l.as_slice())
            .collect();
        let noise = phone::room_noise(samples.len(), phone::rms_of(&vet) * 10f32.powf(spec.noise_db / 20.0), spec.seed);
        samples.iter_mut().zip(noise).for_each(|(s, n)| *s += n);
        let peak = samples.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        if peak > 0.98 {
            samples.iter_mut().for_each(|s| *s *= 0.98 / peak);
        }
    }
    let name = format!("{}{}", script.id, args.suffix);
    let wav = out_dir.join(format!("{name}.wav"));
    if !os_asr::sherpa_onnx::write(wav.to_str().context("path")?, &samples, OUT_RATE) {
        bail!("cannot write {}", wav.display());
    }
    std::fs::write(out_dir.join(format!("{name}.ref.json")), serde_json::to_string_pretty(&reference)?)?;
    eprintln!(
        "{}: {:.1} s, {} lines \u{2192} {}",
        script.id,
        samples.len() as f32 / OUT_RATE as f32,
        reference.len(),
        wav.display()
    );
    Ok(())
}
