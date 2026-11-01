//! Render a synthetic consult script (packages/eval/fixtures/scripts/*.yaml)
//! to a 16 kHz mono WAV plus a ground-truth `.ref.json` with each line's
//! speaker, text and timing (for WER and diarization scoring).
//!
//! ```text
//! cargo run -p fixture-tts -- --kokoro <dir> packages/eval/fixtures/scripts/*.yaml
//! cargo run -p fixture-tts -- --piper <dir> --kokoro <dir> packages/eval/fixtures/fr/scripts/*.yaml
//! ```
//! Voices, all from the k2-fsa sherpa-onnx release `tts-models`:
//! - Kokoro-82M (Apache-2.0), `kokoro-int8-en-v0_19.tar.bz2`: English voices,
//!   named as before (`af_sarah`).
//! - Piper (VITS) voices, `vits-piper-<name>.tar.bz2`, named `piper:<name>`,
//!   with `#<n>` for one speaker of a voice that has several
//!   (`piper:fr_FR-upmc-medium#1`). `--piper` is the folder they are
//!   extracted into, each in its own `vits-piper-<name>` folder. The French
//!   voices and their licences are listed in docs/eval/french.md. Piper reads
//!   through espeak-ng, so an English word in a French line is said with
//!   French phonetics, much as a French speaker might say a drug name.
//!
//! A line can name its language, `[client, "Is that the Apoquel?", en]`; the
//! script's `voices_by_language` then voices it (an English voice for the
//! English lines of a French consult). Untagged lines use `speakers`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use clap::Parser;
use os_asr::sherpa_onnx::{
    GenerationConfig, LinearResampler, OfflineTts, OfflineTtsConfig, OfflineTtsKokoroModelConfig,
    OfflineTtsModelConfig, OfflineTtsVitsModelConfig,
};
use serde::{Deserialize, Serialize};

mod phone;

const OUT_RATE: i32 = 16_000;

/// The `--room` reverb and noise are the same on every run.
const ROOM_SEED: u64 = 41;

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

/// A voice: a Kokoro speaker (`af_sarah`) or a Piper voice
/// (`piper:fr_FR-siwis-medium`, `piper:fr_FR-upmc-medium#1`).
#[derive(Debug, Clone, PartialEq, Eq)]
enum Voice {
    Kokoro { sid: i32 },
    Piper { model: String, sid: i32 },
}

fn parse_voice_spec(s: &str) -> Result<Voice, String> {
    if let Some(rest) = s.strip_prefix("piper:") {
        let (model, sid) = match rest.split_once('#') {
            Some((m, n)) => (m, n.parse::<i32>().map_err(|_| format!("bad speaker number in {s}"))?),
            None => (rest, 0),
        };
        let ok = !model.is_empty() && model.chars().all(|c| c.is_ascii_alphanumeric() || "_-".contains(c));
        if !ok || sid < 0 {
            return Err(format!("bad Piper voice {s}: expected piper:<name> or piper:<name>#<n>"));
        }
        return Ok(Voice::Piper { model: model.to_string(), sid });
    }
    match VOICES.iter().position(|v| *v == s) {
        Some(sid) => Ok(Voice::Kokoro { sid: sid as i32 }),
        None => Err(format!("unknown voice {s}: one of {}, or piper:<name>", VOICES.join(", "))),
    }
}

#[derive(Parser)]
struct Args {
    /// Directory of the extracted kokoro-int8-en-v0_19 model (needed when a
    /// script uses a Kokoro voice).
    #[arg(long)]
    kokoro: Option<PathBuf>,
    /// Folder holding extracted Piper voices, each in its `vits-piper-<name>`
    /// folder (needed when a script uses a `piper:` voice).
    #[arg(long)]
    piper: Option<PathBuf>,
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
    /// Keep each voiced line here and reuse it (Kokoro is the slow part).
    #[arg(long)]
    cache: Option<PathBuf>,
    /// Speakerphone scripts: render without the phone (everyone in the room,
    /// dry), as a control.
    #[arg(long)]
    no_phone: bool,
    /// Speakerphone scripts: the phone's direct-to-reverberant ratio in dB
    /// (default -3: mostly room echo; higher is a phone nearer the mic).
    #[arg(long, default_value_t = -3.0)]
    phone_drr: f32,
    /// Speakerphone scripts: override the phone voice's level (dB re the vet).
    #[arg(long)]
    phone_level: Option<f32>,
    /// Everyone in a room heard by one computer microphone (a laptop's own):
    /// each line gets the room's reverb at this direct-to-reverberant ratio
    /// (dB; 0 is as much echo as voice), and light background noise runs
    /// under the mix. Scripts with `phone:` keep their own room.
    #[arg(long)]
    room: Option<f32>,
    /// With --room: the background noise relative to the voices, in dB.
    #[arg(long, default_value_t = -45.0)]
    room_noise: f32,
    /// Voice a speaker with another voice, `role=voice` (for example
    /// `client=bf_isabella`, two similar voices). Repeatable.
    #[arg(long = "voice", value_parser = parse_voice)]
    voices: Vec<(String, String)>,
    scripts: Vec<PathBuf>,
}

#[derive(Deserialize)]
struct Script {
    id: String,
    speakers: BTreeMap<String, String>,
    /// Voices for lines tagged with a language, by language then speaker
    /// (`en: {dvm: af_sarah}`). A tagged line whose language or speaker
    /// isn't here uses `speakers`.
    #[serde(default)]
    voices_by_language: BTreeMap<String, BTreeMap<String, String>>,
    #[serde(default = "default_pause")]
    pause_secs: f32,
    lines: Vec<Line>,
    /// A speakerphone consult: one speaker heard through a phone on speaker
    /// in the room (see `phone.rs`).
    #[serde(default)]
    phone: Option<phone::PhoneSpec>,
}

/// One line of a script: `[speaker, text]`, or `[speaker, text, language]`.
#[derive(Deserialize)]
#[serde(try_from = "Vec<String>")]
struct Line {
    speaker: String,
    text: String,
    lang: Option<String>,
}

impl TryFrom<Vec<String>> for Line {
    type Error = String;

    fn try_from(v: Vec<String>) -> Result<Self, String> {
        let mut it = v.into_iter();
        match (it.next(), it.next(), it.next(), it.next()) {
            (Some(speaker), Some(text), lang, None) => Ok(Line { speaker, text, lang }),
            _ => Err("a line is [speaker, text] or [speaker, text, language]".into()),
        }
    }
}

impl Script {
    /// The voice for a line: its language's voice for the speaker, if the
    /// script gives one, else the speaker's own.
    fn voice_for(&self, line: &Line) -> Result<&str> {
        let tagged = line.lang.as_ref().and_then(|l| self.voices_by_language.get(l)?.get(&line.speaker));
        tagged
            .or_else(|| self.speakers.get(&line.speaker))
            .map(String::as_str)
            .ok_or_else(|| anyhow!("unknown speaker {}", line.speaker))
    }
}

fn default_pause() -> f32 {
    0.6
}

/// `role=voice`, a Kokoro or Piper voice.
fn parse_voice(s: &str) -> Result<(String, String), String> {
    let (role, voice) = s.split_once('=').ok_or("expected role=voice, for example client=bf_isabella")?;
    parse_voice_spec(voice)?;
    Ok((role.trim().to_string(), voice.to_string()))
}

#[derive(Serialize)]
struct RefLine<'a> {
    speaker: &'a str,
    start: f32,
    end: f32,
    text: &'a str,
    /// The line's language, when the script tags it.
    #[serde(skip_serializing_if = "Option::is_none")]
    lang: Option<&'a str>,
}

/// A loaded TTS model and the resampler from its rate to 16 kHz (none when
/// it already speaks at 16 kHz).
struct Engine {
    tts: OfflineTts,
    resampler: Option<LinearResampler>,
}

impl Engine {
    fn new(config: &OfflineTtsConfig, what: &str) -> Result<Self> {
        let tts = OfflineTts::create(config).ok_or_else(|| anyhow!("cannot load {what}"))?;
        let rate = tts.sample_rate();
        let resampler = if rate == OUT_RATE {
            None
        } else {
            Some(LinearResampler::create(rate, OUT_RATE).context("resampler")?)
        };
        Ok(Self { tts, resampler })
    }

    /// One line of speech at 16 kHz.
    fn say(&self, text: &str, sid: i32, speed: f32) -> Result<Vec<f32>> {
        let config = GenerationConfig { sid, speed, ..Default::default() };
        let audio = self
            .tts
            .generate_with_config::<fn(&[f32], f32) -> bool>(text, &config, None)
            .ok_or_else(|| anyhow!("TTS failed on: {text}"))?;
        Ok(match &self.resampler {
            Some(r) => {
                r.reset();
                r.resample(audio.samples(), true)
            }
            None => audio.samples().to_vec(),
        })
    }
}

fn threads() -> i32 {
    std::thread::available_parallelism().map(|n| n.get() as i32).unwrap_or(4)
}

/// The TTS models, each loaded the first time a line needs it.
struct Voices<'a> {
    args: &'a Args,
    kokoro: Option<Engine>,
    piper: BTreeMap<String, Engine>,
}

impl<'a> Voices<'a> {
    fn new(args: &'a Args) -> Self {
        Self { args, kokoro: None, piper: BTreeMap::new() }
    }

    fn engine(&mut self, voice: &Voice) -> Result<(&Engine, i32)> {
        match voice {
            Voice::Kokoro { sid } => {
                if self.kokoro.is_none() {
                    self.kokoro = Some(load_kokoro(self.args)?);
                }
                Ok((self.kokoro.as_ref().context("Kokoro")?, *sid))
            }
            Voice::Piper { model, sid } => {
                if !self.piper.contains_key(model) {
                    let engine = load_piper(self.args, model)?;
                    self.piper.insert(model.clone(), engine);
                }
                Ok((&self.piper[model], *sid))
            }
        }
    }
}

fn load_kokoro(args: &Args) -> Result<Engine> {
    let dir = args.kokoro.as_ref().context("this script uses a Kokoro voice: give --kokoro <dir>")?;
    let model = |f: &str| dir.join(f).to_string_lossy().into_owned();
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
            num_threads: threads(),
            provider: Some("cpu".into()),
            ..Default::default()
        },
        max_num_sentences: 1,
        ..Default::default()
    };
    Engine::new(&config, &format!("Kokoro from {}", dir.display()))
}

fn load_piper(args: &Args, name: &str) -> Result<Engine> {
    let root = args.piper.as_ref().context("this script uses a Piper voice: give --piper <dir>")?;
    let dir = root.join(format!("vits-piper-{name}"));
    let file = |f: &str| dir.join(f).to_string_lossy().into_owned();
    let config = OfflineTtsConfig {
        model: OfflineTtsModelConfig {
            vits: OfflineTtsVitsModelConfig {
                model: Some(file(&format!("{name}.onnx"))),
                tokens: Some(file("tokens.txt")),
                data_dir: Some(file("espeak-ng-data")),
                ..Default::default()
            },
            num_threads: threads(),
            provider: Some("cpu".into()),
            ..Default::default()
        },
        max_num_sentences: 1,
        ..Default::default()
    };
    Engine::new(&config, &format!("Piper voice {name} from {}", dir.display()))
}

fn main() -> Result<()> {
    let args = Args::parse();
    if args.scripts.is_empty() {
        bail!("give at least one script");
    }
    let mut voices = Voices::new(&args);
    std::fs::create_dir_all(&args.out)?;
    for path in &args.scripts {
        render(&mut voices, path, &args)?;
    }
    Ok(())
}

fn render(voices: &mut Voices, path: &Path, args: &Args) -> Result<()> {
    let out_dir = args.out.as_path();
    let mut script: Script = serde_yaml_ng::from_str(&std::fs::read_to_string(path)?)
        .with_context(|| format!("parsing {}", path.display()))?;
    for (role, voice) in &args.voices {
        if let Some(v) = script.speakers.get_mut(role) {
            *v = voice.clone();
        }
    }
    // Voice every line first: a speakerphone mix needs the vet's level.
    let mut voiced: Vec<Vec<f32>> = Vec::new();
    for line in &script.lines {
        let text = &line.text;
        let voice = script.voice_for(line)?;
        let spec = parse_voice_spec(voice).map_err(|e| anyhow!(e))?;
        let cached = args.cache.as_ref().map(|dir| {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            std::hash::Hash::hash(&(voice, text, args.speed.to_bits()), &mut h);
            dir.join(format!("{:016x}.wav", std::hash::Hasher::finish(&h)))
        });
        if let Some(wave) = cached.as_ref().and_then(|p| os_asr::sherpa_onnx::Wave::read(p.to_str()?)) {
            voiced.push(wave.samples().to_vec());
            continue;
        }
        let (engine, sid) = voices.engine(&spec)?;
        let audio = engine.say(text, sid, args.speed)?;
        if let Some(path) = &cached {
            std::fs::create_dir_all(path.parent().context("cache dir")?)?;
            os_asr::sherpa_onnx::write(path.to_str().context("path")?, &audio, OUT_RATE);
        }
        voiced.push(audio);
    }
    // What the room mic hears of each line (the dry line without `phone:`),
    // which may ring on past the line's end (reverb).
    let phone = if args.no_phone { None } else { script.phone.clone() };
    let heard: Vec<Vec<f32>> = match (&phone, args.room) {
        // Everyone at a similar distance from the computer's microphone.
        (None, Some(drr)) => voiced
            .iter()
            .enumerate()
            .map(|(i, line)| phone::in_room(line, drr, ROOM_SEED + i as u64))
            .collect(),
        (None, None) => voiced.clone(),
        (Some(spec), _) => {
            let level_db = args.phone_level.unwrap_or(spec.level_db);
            if !script.speakers.contains_key(&spec.speaker) {
                bail!("phone speaker {} isn't one of the script's speakers", spec.speaker);
            }
            let on_phone = |i: usize| script.lines[i].speaker == spec.speaker;
            let mut heard: Vec<Vec<f32>> = voiced
                .iter()
                .enumerate()
                .map(|(i, line)| {
                    let seed = spec.seed.wrapping_add(i as u64);
                    if on_phone(i) {
                        // Mostly reverberant: the phone is a metre or more away.
                        phone::in_room(&phone::phone_line(line), args.phone_drr, seed)
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
            let gain = room * 10f32.powf(level_db / 20.0) / far.max(1e-9);
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
    // --room: the next line can start while this one's echo still rings (a
    // short pause stays short). The speakerphone mix keeps its own rule.
    let ring_on = phone.is_none() && args.room.is_some();
    let mut next_at = 0usize;
    for ((script_line, line), dry) in script.lines.iter().zip(&heard).zip(&voiced) {
        let at = if ring_on { next_at } else { samples.len() };
        let start = at as f32 / OUT_RATE as f32;
        let end = (at + dry.len()) as f32 / OUT_RATE as f32;
        reference.push(RefLine {
            speaker: &script_line.speaker,
            start,
            end,
            text: &script_line.text,
            lang: script_line.lang.as_deref(),
        });
        samples.resize(samples.len().max(at + line.len()), 0.0);
        for (s, x) in samples[at..].iter_mut().zip(line) {
            *s += x;
        }
        // The next line starts a pause after this one's dry end.
        next_at = at + dry.len() + pause;
        samples.resize(samples.len().max(next_at), 0.0);
        if !ring_on {
            samples.truncate(at + dry.len() + pause.max(line.len().saturating_sub(dry.len())));
        }
    }
    if phone.is_none() && args.room.is_some() {
        let all: Vec<&[f32]> = voiced.iter().map(Vec::as_slice).collect();
        let level = phone::rms_of(&all) * 10f32.powf(args.room_noise / 20.0);
        let noise = phone::room_noise(samples.len(), level, ROOM_SEED);
        samples.iter_mut().zip(noise).for_each(|(s, n)| *s += n);
        let peak = samples.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        if peak > 0.98 {
            samples.iter_mut().for_each(|s| *s *= 0.98 / peak);
        }
    }
    if let Some(spec) = &phone {
        let vet: Vec<&[f32]> = voiced
            .iter()
            .zip(&script.lines)
            .filter(|(_, l)| l.speaker != spec.speaker)
            .map(|(l, _)| l.as_slice())
            .collect();
        let noise = phone::room_noise(
            samples.len(),
            phone::rms_of(&vet) * 10f32.powf(spec.noise_db / 20.0),
            spec.seed,
        );
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voices_are_kokoro_names_or_piper_specs() {
        assert_eq!(parse_voice_spec("af_sarah"), Ok(Voice::Kokoro { sid: 3 }));
        assert_eq!(
            parse_voice_spec("piper:fr_FR-siwis-medium"),
            Ok(Voice::Piper { model: "fr_FR-siwis-medium".into(), sid: 0 })
        );
        assert_eq!(
            parse_voice_spec("piper:fr_FR-upmc-medium#1"),
            Ok(Voice::Piper { model: "fr_FR-upmc-medium".into(), sid: 1 })
        );
        assert!(parse_voice_spec("ff_siwis").is_err());
        assert!(parse_voice_spec("piper:").is_err());
        assert!(parse_voice_spec("piper:../x").is_err());
        assert!(parse_voice_spec("piper:fr_FR-upmc-medium#x").is_err());
    }

    #[test]
    fn tagged_lines_take_their_language_voice() {
        let script: Script = serde_yaml_ng::from_str(
            "id: x
speakers: {dvm: 'piper:fr_FR-siwis-medium', client: 'piper:fr_FR-gilles-low'}
voices_by_language:
  en: {dvm: af_sarah}
lines:
  - [dvm, \"Bonjour.\"]
  - [dvm, \"Is that the Apoquel?\", en]
  - [client, \"Yes it is.\", en]
",
        )
        .unwrap();
        let voices: Vec<&str> = script.lines.iter().map(|l| script.voice_for(l).unwrap()).collect();
        assert_eq!(voices, ["piper:fr_FR-siwis-medium", "af_sarah", "piper:fr_FR-gilles-low"]);
        assert_eq!(script.lines[1].lang.as_deref(), Some("en"));
        let bad: Result<Script, _> = serde_yaml_ng::from_str("id: x\nspeakers: {}\nlines: [[dvm]]\n");
        assert!(bad.is_err());
    }

    /// Every script, English and French, parses and names voices this tool knows.
    #[test]
    fn every_script_parses_with_known_voices() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packages/eval/fixtures");
        let mut n = 0;
        for dir in ["scripts", "fr/scripts"] {
            for e in std::fs::read_dir(root.join(dir)).unwrap().flatten() {
                if e.path().extension().is_none_or(|x| x != "yaml") {
                    continue;
                }
                let text = std::fs::read_to_string(e.path()).unwrap();
                let script: Script = serde_yaml_ng::from_str(&text)
                    .unwrap_or_else(|err| panic!("{}: {err}", e.path().display()));
                for line in &script.lines {
                    let voice = script.voice_for(line).unwrap();
                    assert!(parse_voice_spec(voice).is_ok(), "{}: {voice}", script.id);
                }
                n += 1;
            }
        }
        assert!(n > 20, "only {n} scripts");
    }
}
