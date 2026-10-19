//! Live recording: microphone → session `audio.wav` on disk, with live
//! voice detection, speech-to-text and speaker labels while the consult runs.
//!
//! When the user presses stop, only the last utterance remains to be
//! recognised; the note can be drafted immediately.
//!
//! ```text
//! capture thread ──blocks──▶ worker: resample 16 kHz ─▶ audio.wav
//!                                            ├──────▶ level meter (events)
//!                                            └──────▶ VAD ─▶ ASR ─▶ speaker ─▶ segment (events)
//! ```
//!
//! The source can also be a WAV file played through the same path (for tests
//! and CI, where there is no microphone).
//!
//! A phone call ([`LiveSource::Call`], ADR-0023 section 7) arrives as two
//! streams, the clinic's microphone and the client's voice. It is written as
//! a two-channel recording (clinic left, client right), each channel gets its
//! own voice detection, and the speaker is the channel: no voice embeddings
//! are computed for calls.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use os_asr::capture::{AudioBlock, MicCapture};
use os_asr::{AsrBackend, Audio, SAMPLE_RATE, WavWriter};
use os_models::ModelStore;
use serde::Serialize;

use crate::awake::{AwakeGuard, AwakeReason};
use crate::session::Session;
use crate::transcribe::TranscribeStats;
use crate::transcript::{Segment, Transcript};
use crate::{EngineError, models};

/// Where live audio comes from.
pub enum LiveSource {
    /// A microphone on this machine, by name, or the system default.
    Microphone(Option<String>),
    /// A WAV file fed through the live path. `realtime` paces it like a mic.
    File { path: PathBuf, realtime: bool },
    /// A recording held in memory (an upload), so it never touches the disk
    /// unencrypted. `name` gives the format by its extension.
    Memory { name: String, bytes: Vec<u8> },
    /// Audio pushed by the caller, e.g. a browser streaming its microphone
    /// over a WebSocket. Recording ends when every sender is dropped.
    Stream { label: String, blocks: mpsc::Receiver<AudioBlock> },
    /// A phone call: both sides pushed by the phone window, block by block.
    /// Recording ends when every sender is dropped, or on stop.
    Call { label: String, blocks: mpsc::Receiver<CallBlock> },
}

/// One block of a phone call: the clinic's side and the client's side, the
/// same number of samples each, at `sample_rate`.
#[derive(Debug, Clone)]
pub struct CallBlock {
    pub clinic: Vec<f32>,
    pub client: Vec<f32>,
    pub sample_rate: u32,
}

impl CallBlock {
    /// Splits interleaved two-channel samples (clinic, client, clinic, ...).
    /// A trailing odd sample is dropped.
    pub fn from_interleaved(samples: &[f32], sample_rate: u32) -> Self {
        let frames = samples.len() / 2;
        let mut clinic = Vec::with_capacity(frames);
        let mut client = Vec::with_capacity(frames);
        for pair in samples.as_chunks::<2>().0 {
            clinic.push(pair[0]);
            client.push(pair[1]);
        }
        Self { clinic, client, sample_rate }
    }
}

/// Speaker numbers on a call's segments: the channel.
pub const CLINIC_CHANNEL: u32 = 0;
pub const CLIENT_CHANNEL: u32 = 1;

#[derive(Debug, Clone)]
pub struct LiveOptions {
    pub stt: String,
    pub label_speakers: bool,
    /// Cosine threshold for the live speaker tracker (tuned in M1).
    pub speaker_threshold: f32,
    /// Clinic-entered patient details, saved with the session at the start.
    pub patient: Option<crate::session::PatientInfo>,
    /// The recording consent the clinic confirmed (M1 item 7).
    pub consent: Option<crate::session::Consent>,
    /// Who started the recording (team members, M1 item 11).
    pub recorded_by: Option<crate::users::Who>,
    /// A phone call's details, saved with the visit at the start (ADR-0023).
    pub call: Option<crate::session::CallInfo>,
}

impl LiveOptions {
    pub fn microphone() -> Self {
        Self {
            stt: models::DEFAULT_STT.into(),
            label_speakers: true,
            speaker_threshold: DEFAULT_SPEAKER_THRESHOLD,
            patient: None,
            consent: None,
            recorded_by: None,
            call: None,
        }
    }
}

/// Tuned on the synthetic fixtures (M1): every fixture labels correctly in
/// 0.6-0.7 (100% purity except 98% on the 50 s clip); below merges speakers,
/// above splits them. Kokoro voices are very distinct, so re-check on real
/// consults; the full-recording diarization pass remains available.
/// Catalog id of the second-opinion recogniser (used when installed).
pub const SECOND_STT: &str = "whisper-large-v3-turbo-int8";

pub const DEFAULT_SPEAKER_THRESHOLD: f32 = 0.65;

/// Progress callbacks, called from the worker thread.
pub trait LiveSink: Send {
    /// Input level, 0.0 (silence) to 1.0 (full scale), about 20 times a second.
    fn level(&mut self, level: f32, seconds: f32);
    fn segment(&mut self, segment: &Segment);
    /// How far processing has got, for a progress bar (imports, and the
    /// finishing steps after a recording stops).
    fn progress(&mut self, _progress: &Progress) {}
}

/// A step of turning audio into a saved transcript.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    /// Decoding a file or upload.
    Read,
    /// Speech to text: `done` and `total` are seconds of audio. A live
    /// recording finishing its last words has `total` 0 (not known).
    Transcribe,
    /// The second recogniser re-hearing segments with numbers: segments.
    Check,
    /// Writing the transcript and audio, or a drafted note.
    Save,
    /// Drafting: loading the note model into memory (first draft only).
    LoadModel,
    /// Drafting: the model reading the transcript and template, before it
    /// writes its first word (longer for long visits).
    ReadTranscript,
    /// Drafting: writing the note (the screen follows the headings as they
    /// appear in the streamed text).
    Write,
    /// Drafting: checking every number and statement against the transcript.
    Verify,
}

#[derive(Debug, Clone, Serialize)]
pub struct Progress {
    pub stage: Stage,
    pub done: f32,
    pub total: f32,
}

impl Progress {
    pub fn new(stage: Stage, done: f32, total: f32) -> Self {
        Self { stage, done, total }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct LiveResult {
    pub session_dir: PathBuf,
    pub device: String,
    pub transcript: Transcript,
    pub stats: TranscribeStats,
    /// Seconds between stop and the transcript being complete.
    pub finish_ms: u64,
}

pub struct LiveRecorder {
    stop: Arc<AtomicBool>,
    /// Set by [`LiveRecorder::discard`]: a call's recording is deleted
    /// instead of saved (the client declined, ADR-0023 section 7).
    discard: Arc<AtomicBool>,
    worker: Option<JoinHandle<Result<LiveResult, EngineError>>>,
    pub session_dir: PathBuf,
}

impl LiveRecorder {
    /// Create a session and start recording into it.
    pub fn start(
        store: &ModelStore,
        sessions_root: &Path,
        source: LiveSource,
        options: LiveOptions,
        sink: Box<dyn LiveSink>,
    ) -> Result<Self, EngineError> {
        // Load models up front so errors surface before recording starts.
        let vad = os_asr::SileroVad::new(os_asr::VadOptions::new(models::single_file(store, models::VAD)?))?;
        let asr = os_asr::OfflineAsr::new(models::asr_model(store, &models::best_stt(store, &options.stt))?)?;
        let is_call = matches!(source, LiveSource::Call { .. });
        // Calls: the channel says who is speaking, so no embeddings (ADR-0023 section 7).
        let tracker = if options.label_speakers && !is_call {
            Some(os_diar::SpeakerTracker::new(
                &models::single_file(store, models::EMBEDDING)?,
                options.speaker_threshold,
            )?)
        } else {
            None
        };
        let label = match &source {
            LiveSource::Microphone(_) => "recording".to_string(),
            LiveSource::File { path, .. } => {
                path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
            }
            LiveSource::Stream { label, .. } | LiveSource::Call { label, .. } => label.clone(),
            LiveSource::Memory { name, .. } => {
                Path::new(name).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
            }
        };
        let lexicon = crate::vocab::Lexicon::default_with_user();
        // Second recogniser for segments that matter (M1 item 4); optional.
        let second = models::asr_model(store, &models::best_stt(store, SECOND_STT))
            .ok()
            .and_then(|m| os_asr::OfflineAsr::with_threads(m, os_asr::second_threads()).ok());
        if second.is_none() {
            tracing::info!("second recogniser not installed; number cross-check off");
        }
        let label =
            options.patient.as_ref().map(|p| p.display_name()).filter(|n| !n.is_empty()).unwrap_or(label);
        let mut session = Session::create_live(sessions_root, &label)?;
        if let Some(p) = options.patient.clone() {
            session.set_patient(p)?;
        }
        if let Some(c) = options.consent.clone() {
            session.set_consent(c)?;
        }
        if let Some(who) = options.recorded_by.clone() {
            session.set_recorded_by(who)?;
        }
        if let Some(call) = options.call.clone() {
            session.set_call(call)?;
        }
        let session_dir = session.dir.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let discard = Arc::new(AtomicBool::new(false));
        let (stop2, discard2) = (stop.clone(), discard.clone());
        let worker = std::thread::Builder::new()
            .name("openbark-live".into())
            .spawn(move || match source {
                LiveSource::Call { blocks, .. } => {
                    let parts = CallParts { vad, asr, lexicon, second };
                    run_call(session, blocks, options, parts, sink, stop2, discard2)
                }
                source => run(session, source, options, vad, asr, tracker, lexicon, second, sink, stop2),
            })
            .map_err(|e| EngineError::Session(e.to_string()))?;
        Ok(Self { stop, discard, worker: Some(worker), session_dir })
    }

    /// Stop a call's recording and delete what was recorded: the audio and
    /// anything transcribed so far. The visit folder and `session.json` stay,
    /// so the visit can say the client declined. Other sources save as `stop`.
    pub fn discard(mut self) -> Result<LiveResult, EngineError> {
        self.discard.store(true, Ordering::SeqCst);
        self.stop.store(true, Ordering::SeqCst);
        self.join()
    }

    /// Stop recording; returns once the transcript is complete and saved.
    pub fn stop(mut self) -> Result<LiveResult, EngineError> {
        self.stop.store(true, Ordering::SeqCst);
        self.join()
    }

    /// Wait for a file source to finish on its own.
    pub fn wait(mut self) -> Result<LiveResult, EngineError> {
        self.join()
    }

    fn join(&mut self) -> Result<LiveResult, EngineError> {
        self.worker
            .take()
            .expect("joined once")
            .join()
            .map_err(|_| EngineError::Session("recording thread panicked".into()))?
    }
}

impl Drop for LiveRecorder {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
    }
}

/// Feed blocks to `tx` from the chosen source until `stop` is set (or the
/// file ends). Returns the device name, and for a file or upload its length
/// in seconds.
fn spawn_source(
    source: LiveSource,
    tx: mpsc::Sender<AudioBlock>,
    stop: Arc<AtomicBool>,
) -> Result<(JoinHandle<()>, String, Option<f32>), EngineError> {
    if let LiveSource::Stream { label, blocks } = source {
        // Forward caller-pushed blocks until the caller hangs up or we stop.
        let handle = std::thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                match blocks.recv_timeout(Duration::from_millis(100)) {
                    Ok(block) => {
                        if tx.send(block).is_err() {
                            break;
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
        });
        return Ok((handle, label, None));
    }
    let (ready_tx, ready_rx) = mpsc::channel::<Result<(String, Option<f32>), String>>();
    let handle = std::thread::spawn(move || match source {
        LiveSource::Microphone(device) => {
            // cpal streams aren't Send: create, hold and drop on this thread.
            match MicCapture::start(device.as_deref(), tx) {
                Ok(mic) => {
                    let _ = ready_tx.send(Ok((mic.device_name.clone(), None)));
                    while !stop.load(Ordering::SeqCst) {
                        std::thread::sleep(Duration::from_millis(20));
                    }
                }
                Err(e) => {
                    let _ = ready_tx.send(Err(e.to_string()));
                }
            }
        }
        source @ (LiveSource::File { .. } | LiveSource::Memory { .. }) => {
            let (loaded, realtime, origin) = match source {
                LiveSource::File { path, realtime } => {
                    (Audio::load(&path), realtime, path.display().to_string())
                }
                LiveSource::Memory { name, bytes } => (Audio::decode_bytes(bytes, &name), false, name),
                _ => unreachable!(),
            };
            match loaded {
                Ok(audio) => {
                    let secs = audio.samples.len() as f32 / SAMPLE_RATE as f32;
                    let _ = ready_tx.send(Ok((format!("file: {origin}"), Some(secs))));
                    let block = (SAMPLE_RATE / 50) as usize; // 20 ms
                    for chunk in audio.samples.chunks(block) {
                        if stop.load(Ordering::SeqCst) {
                            break;
                        }
                        if tx.send(AudioBlock { samples: chunk.to_vec(), sample_rate: SAMPLE_RATE }).is_err()
                        {
                            break;
                        }
                        if realtime {
                            std::thread::sleep(Duration::from_millis(20));
                        }
                    }
                }
                Err(e) => {
                    let _ = ready_tx.send(Err(e.to_string()));
                }
            }
        }
        LiveSource::Stream { .. } => unreachable!("handled above"),
        LiveSource::Call { .. } => unreachable!("calls run in run_call"),
    });
    let (device, secs) = ready_rx
        .recv()
        .map_err(|_| EngineError::Session("audio source failed to start".into()))?
        .map_err(EngineError::Session)?;
    Ok((handle, device, secs))
}

#[allow(clippy::too_many_arguments)]
fn run(
    mut session: Session,
    source: LiveSource,
    options: LiveOptions,
    vad: os_asr::SileroVad,
    asr: os_asr::OfflineAsr,
    mut tracker: Option<os_diar::SpeakerTracker>,
    lexicon: crate::vocab::Lexicon,
    second: Option<os_asr::OfflineAsr>,
    mut sink: Box<dyn LiveSink>,
    stop: Arc<AtomicBool>,
) -> Result<LiveResult, EngineError> {
    // Background second opinions: (segment index, audio, primary text) in,
    // (index, opinion, hints) out. Runs alongside capture so stop isn't delayed.
    type Job = (usize, Vec<f32>, String);
    type Done = (usize, crate::crosscheck::SecondOpinion, Vec<crate::vocab::Hint>);
    let (job_tx, done_rx, checker) = match second {
        Some(second) => {
            let (job_tx, job_rx) = mpsc::channel::<Job>();
            let (done_tx, done_rx) = mpsc::channel::<Done>();
            let lex = lexicon.clone();
            let handle = std::thread::Builder::new()
                .name("openbark-second-opinion".into())
                .spawn(move || {
                    for (i, samples, primary) in job_rx {
                        if let Ok(text) = second.transcribe(&samples) {
                            let opinion = crate::crosscheck::compare(&primary, &text);
                            let hints = crate::crosscheck::second_opinion_hints(&lex, &primary, &text);
                            if done_tx.send((i, opinion, hints)).is_err() {
                                break;
                            }
                        }
                    }
                })
                .map_err(|e| EngineError::Session(e.to_string()))?;
            (Some(job_tx), Some(done_rx), Some(handle))
        }
        None => (None, None, None),
    };
    let _awake = AwakeGuard::acquire(AwakeReason::Recording);
    let started = Instant::now();
    let (tx, rx) = mpsc::channel::<AudioBlock>();
    let (source, device, total_secs) = spawn_source(source, tx, stop.clone())?;
    // Imports report seconds transcribed, at most every 1% (or 2 s of audio).
    let report_every = total_secs.map(|t| (t / 100.0).max(2.0));
    let mut reported = 0.0f32;
    if let Some(total) = total_secs {
        sink.progress(&Progress::new(Stage::Transcribe, 0.0, total));
    }
    // Encrypted as it's written when the vault is on (ADR-0008).
    let mut wav = if crate::vault::enabled() {
        crate::vault::AudioWriter::create(&session.audio_path())
            .and_then(|w| WavWriter::with_sink(Box::new(w), SAMPLE_RATE))
    } else {
        WavWriter::create(&session.audio_path(), SAMPLE_RATE)
    }
    .map_err(|e| EngineError::Session(e.to_string()))?;
    let mut streaming = vad.streaming()?;
    let mut transcript = Transcript::default();
    let mut resampler: Option<(u32, os_asr::sherpa_onnx::LinearResampler)> = None;
    let mut level_acc = (0.0f32, 0usize);
    let level_every = (SAMPLE_RATE / 20) as usize;
    let mut asr_ms = 0u64;

    let mut checks_queued = 0usize;
    let mut handle_utterances = |utterances: Vec<os_asr::Utterance>,
                                 transcript: &mut Transcript,
                                 sink: &mut Box<dyn LiveSink>,
                                 asr_ms: &mut u64|
     -> Result<(), EngineError> {
        for u in utterances {
            if u.samples.len() < (SAMPLE_RATE / 5) as usize {
                continue;
            }
            let t = Instant::now();
            let text = asr.transcribe(&u.samples)?;
            let speaker = tracker.as_mut().and_then(|tr| tr.assign(&u.samples));
            *asr_ms += t.elapsed().as_millis() as u64;
            if text.is_empty() {
                continue;
            }
            let segment = Segment {
                id: transcript.segments.len() as u32,
                start: u.start,
                end: u.end(),
                speaker,
                hints: lexicon.hints(&text),
                check: None,
                text,
            };
            sink.segment(&segment);
            if let Some(tx) = &job_tx
                && crate::crosscheck::worth_checking(&segment)
                && tx.send((transcript.segments.len(), u.samples.clone(), segment.text.clone())).is_ok()
            {
                checks_queued += 1;
            }
            transcript.segments.push(segment);
        }
        Ok(())
    };

    let mut stopped_at: Option<Instant> = None;
    loop {
        let block = match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(b) => b,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if stop.load(Ordering::SeqCst) {
                    stopped_at.get_or_insert_with(Instant::now);
                }
                continue;
            }
            // Source finished (file ended, or capture stopped and dropped the sender).
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };
        if stop.load(Ordering::SeqCst) {
            stopped_at.get_or_insert_with(Instant::now);
        }
        let samples = if block.sample_rate == SAMPLE_RATE {
            block.samples
        } else {
            if resampler.as_ref().map(|(r, _)| *r) != Some(block.sample_rate) {
                let r = os_asr::sherpa_onnx::LinearResampler::create(
                    block.sample_rate as i32,
                    SAMPLE_RATE as i32,
                )
                .ok_or_else(|| {
                    EngineError::Session(format!("cannot resample from {} Hz", block.sample_rate))
                })?;
                resampler = Some((block.sample_rate, r));
            }
            resampler.as_ref().expect("set above").1.resample(&block.samples, false)
        };
        wav.write(&samples).map_err(|e| EngineError::Session(e.to_string()))?;

        for s in &samples {
            level_acc.0 += s * s;
            level_acc.1 += 1;
            if level_acc.1 >= level_every {
                let rms = (level_acc.0 / level_acc.1 as f32).sqrt();
                // Map roughly -60..0 dBFS to 0..1 so quiet speech still moves the meter.
                let db = 20.0 * rms.max(1e-6).log10();
                sink.level(((db + 60.0) / 60.0).clamp(0.0, 1.0), wav.duration_secs());
                level_acc = (0.0, 0);
            }
        }
        let utterances = streaming.accept(&samples);
        handle_utterances(utterances, &mut transcript, &mut sink, &mut asr_ms)?;
        if let (Some(total), Some(every)) = (total_secs, report_every) {
            let done = wav.duration_secs();
            if done - reported >= every {
                reported = done;
                sink.progress(&Progress::new(Stage::Transcribe, done.min(total), total));
            }
        }
    }
    let stopped_at = stopped_at.unwrap_or_else(Instant::now);
    let _ = source.join();

    // The last words: a stopped recording's total isn't known (0).
    let heard = wav.duration_secs();
    sink.progress(&Progress::new(Stage::Transcribe, heard, total_secs.unwrap_or(0.0)));
    let last = streaming.finish();
    handle_utterances(last, &mut transcript, &mut sink, &mut asr_ms)?;
    // Collect second opinions. A recording has only the last few still
    // pending at stop; an import may have many, so count them as they land.
    drop(job_tx);
    if let (Some(done_rx), Some(checker)) = (done_rx, checker) {
        let total = checks_queued as f32;
        let mut checked = 0usize;
        if checks_queued > 0 {
            sink.progress(&Progress::new(Stage::Check, 0.0, total));
        }
        // Ends when the checker has worked through the queue and hung up.
        for (i, opinion, hints) in done_rx {
            if let Some(seg) = transcript.segments.get_mut(i) {
                for h in hints {
                    if !seg.hints.iter().any(|x| x.heard.eq_ignore_ascii_case(&h.heard)) {
                        seg.hints.push(h);
                    }
                }
                seg.check = Some(opinion);
            }
            checked += 1;
            sink.progress(&Progress::new(Stage::Check, checked as f32, total));
        }
        let _ = checker.join();
    }
    sink.progress(&Progress::new(Stage::Save, 0.0, 1.0));
    let audio_secs = wav.finish().map_err(|e| EngineError::Session(e.to_string()))?;
    session.finish_live_audio(&device, audio_secs)?;

    let stats = TranscribeStats {
        audio_secs,
        vad_ms: 0,
        diarization_ms: 0,
        asr_ms,
        total_ms: started.elapsed().as_millis() as u64,
        rtf: if audio_secs > 0.0 { asr_ms as f32 / 1000.0 / audio_secs } else { 0.0 },
        segments: transcript.segments.len(),
        speakers: transcript.speaker_count(),
    };
    session.save_transcript(&transcript, &options.stt, options.label_speakers, &stats)?;
    Ok(LiveResult {
        session_dir: session.dir.clone(),
        device,
        transcript,
        stats,
        finish_ms: stopped_at.elapsed().as_millis() as u64,
    })
}

/// What a call's worker needs besides the session.
struct CallParts {
    vad: os_asr::SileroVad,
    asr: os_asr::OfflineAsr,
    lexicon: crate::vocab::Lexicon,
    second: Option<os_asr::OfflineAsr>,
}

/// One side of a call: its resampler and voice detector.
struct Side {
    speaker: u32,
    resampler: Option<(u32, os_asr::sherpa_onnx::LinearResampler)>,
    vad: os_asr::StreamingVad,
}

impl Side {
    fn resample_16k(&mut self, samples: Vec<f32>, rate: u32) -> Result<Vec<f32>, EngineError> {
        if rate == SAMPLE_RATE {
            return Ok(samples);
        }
        if self.resampler.as_ref().map(|(r, _)| *r) != Some(rate) {
            let r = os_asr::sherpa_onnx::LinearResampler::create(rate as i32, SAMPLE_RATE as i32)
                .ok_or_else(|| EngineError::Session(format!("cannot resample from {rate} Hz")))?;
            self.resampler = Some((rate, r));
        }
        Ok(self.resampler.as_ref().expect("set above").1.resample(&samples, false))
    }
}

/// A phone call: two channels in, a two-channel recording out, each channel
/// transcribed on its own and labelled by channel (ADR-0023 section 7).
#[allow(clippy::too_many_arguments)]
fn run_call(
    mut session: Session,
    blocks: mpsc::Receiver<CallBlock>,
    options: LiveOptions,
    parts: CallParts,
    mut sink: Box<dyn LiveSink>,
    stop: Arc<AtomicBool>,
    discard: Arc<AtomicBool>,
) -> Result<LiveResult, EngineError> {
    let CallParts { vad, asr, lexicon, second } = parts;
    let _awake = AwakeGuard::acquire(AwakeReason::Recording);
    let started = Instant::now();
    let audio_path = session.audio_path();
    // Encrypted from the first chunk when the vault is on (ADR-0008).
    let mut wav = if crate::vault::enabled() {
        crate::vault::AudioWriter::create(&audio_path)
            .and_then(|w| WavWriter::with_sink_channels(Box::new(w), SAMPLE_RATE, 2))
    } else {
        std::fs::File::create(&audio_path)
            .and_then(|f| WavWriter::with_sink_channels(Box::new(std::io::BufWriter::new(f)), SAMPLE_RATE, 2))
    }
    .map_err(|e| EngineError::Session(e.to_string()))?;
    let mut sides = [
        Side { speaker: CLINIC_CHANNEL, resampler: None, vad: vad.streaming()? },
        Side { speaker: CLIENT_CHANNEL, resampler: None, vad: vad.streaming()? },
    ];
    let mut transcript = Transcript::default();
    // Segments needing a second listen, checked when the call ends (calls are
    // short, and the checker shouldn't compete with live recognition).
    let mut to_check: Vec<(usize, Vec<f32>)> = Vec::new();
    let mut asr_ms = 0u64;
    let mut level_acc = (0.0f32, 0.0f32, 0usize);
    let level_every = (SAMPLE_RATE / 20) as usize;
    let mut stopped_at: Option<Instant> = None;

    let hear = |utterances: Vec<os_asr::Utterance>,
                speaker: u32,
                transcript: &mut Transcript,
                sink: &mut Box<dyn LiveSink>,
                to_check: &mut Vec<(usize, Vec<f32>)>,
                asr_ms: &mut u64|
     -> Result<(), EngineError> {
        for u in utterances {
            if u.samples.len() < (SAMPLE_RATE / 5) as usize {
                continue;
            }
            let t = Instant::now();
            let text = asr.transcribe(&u.samples)?;
            *asr_ms += t.elapsed().as_millis() as u64;
            if text.is_empty() {
                continue;
            }
            let segment = Segment {
                id: transcript.segments.len() as u32,
                start: u.start,
                end: u.end(),
                speaker: Some(speaker),
                hints: lexicon.hints(&text),
                check: None,
                text,
            };
            sink.segment(&segment);
            if second.is_some() && crate::crosscheck::worth_checking(&segment) {
                to_check.push((transcript.segments.len(), u.samples));
            }
            transcript.segments.push(segment);
        }
        Ok(())
    };

    loop {
        // After stop, only what has already arrived is kept.
        let block = if stop.load(Ordering::SeqCst) {
            stopped_at.get_or_insert_with(Instant::now);
            match blocks.try_recv() {
                Ok(b) => b,
                Err(_) => break,
            }
        } else {
            match blocks.recv_timeout(Duration::from_millis(100)) {
                Ok(b) => b,
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        };
        if discard.load(Ordering::SeqCst) {
            break;
        }
        let rate = block.sample_rate;
        let mut clinic = sides[0].resample_16k(block.clinic, rate)?;
        let mut client = sides[1].resample_16k(block.client, rate)?;
        // The two sides arrive together; keep them frame-aligned.
        let frames = clinic.len().max(client.len());
        clinic.resize(frames, 0.0);
        client.resize(frames, 0.0);
        wav.write_stereo(&clinic, &client).map_err(|e| EngineError::Session(e.to_string()))?;
        for (a, b) in clinic.iter().zip(&client) {
            level_acc.0 += a * a;
            level_acc.1 += b * b;
            level_acc.2 += 1;
            if level_acc.2 >= level_every {
                let n = level_acc.2 as f32;
                let rms = (level_acc.0 / n).sqrt().max((level_acc.1 / n).sqrt());
                let db = 20.0 * rms.max(1e-6).log10();
                sink.level(((db + 60.0) / 60.0).clamp(0.0, 1.0), wav.duration_secs());
                level_acc = (0.0, 0.0, 0);
            }
        }
        for (side, samples) in sides.iter_mut().zip([&clinic, &client]) {
            let heard = side.vad.accept(samples);
            hear(heard, side.speaker, &mut transcript, &mut sink, &mut to_check, &mut asr_ms)?;
        }
    }
    let stopped_at = stopped_at.unwrap_or_else(Instant::now);

    if discard.load(Ordering::SeqCst) {
        // The client declined: nothing recorded is kept (ADR-0023 section 7).
        drop(wav);
        for path in [audio_path.clone(), session.dir.join(format!("{}.tmp", crate::session::AUDIO_FILE))] {
            if path.exists() {
                crate::trash::remove_file_retrying(&path)?;
            }
        }
        return Ok(LiveResult {
            session_dir: session.dir.clone(),
            device: "phone call".into(),
            transcript: Transcript::default(),
            stats: TranscribeStats::default(),
            finish_ms: stopped_at.elapsed().as_millis() as u64,
        });
    }

    let heard_secs = wav.duration_secs();
    sink.progress(&Progress::new(Stage::Transcribe, heard_secs, 0.0));
    for side in sides.iter_mut() {
        let last = side.vad.finish();
        hear(last, side.speaker, &mut transcript, &mut sink, &mut to_check, &mut asr_ms)?;
    }
    if let Some(second) = &second
        && !to_check.is_empty()
    {
        let total = to_check.len() as f32;
        sink.progress(&Progress::new(Stage::Check, 0.0, total));
        for (n, (i, samples)) in to_check.iter().enumerate() {
            if let (Some(seg), Ok(text)) = (transcript.segments.get_mut(*i), second.transcribe(samples)) {
                for h in crate::crosscheck::second_opinion_hints(&lexicon, &seg.text, &text) {
                    if !seg.hints.iter().any(|x| x.heard.eq_ignore_ascii_case(&h.heard)) {
                        seg.hints.push(h);
                    }
                }
                seg.check = Some(crate::crosscheck::compare(&seg.text, &text));
            }
            sink.progress(&Progress::new(Stage::Check, (n + 1) as f32, total));
        }
    }
    // Both channels in time order, numbered as the note will cite them.
    transcript.segments.sort_by(|a, b| a.start.total_cmp(&b.start));
    for (i, seg) in transcript.segments.iter_mut().enumerate() {
        seg.id = i as u32;
    }
    sink.progress(&Progress::new(Stage::Save, 0.0, 1.0));
    let audio_secs = wav.finish().map_err(|e| EngineError::Session(e.to_string()))?;
    session.finish_live_audio("phone call", audio_secs)?;
    let stats = TranscribeStats {
        audio_secs,
        vad_ms: 0,
        diarization_ms: 0,
        asr_ms,
        total_ms: started.elapsed().as_millis() as u64,
        rtf: if audio_secs > 0.0 { asr_ms as f32 / 1000.0 / audio_secs } else { 0.0 },
        segments: transcript.segments.len(),
        speakers: transcript.speaker_count(),
    };
    // Speakers are labelled (by channel), so the transcript says so.
    session.save_transcript(&transcript, &options.stt, true, &stats)?;
    Ok(LiveResult {
        session_dir: session.dir.clone(),
        device: "phone call".into(),
        transcript,
        stats,
        finish_ms: stopped_at.elapsed().as_millis() as u64,
    })
}

#[cfg(test)]
mod call_tests {
    use super::*;

    #[test]
    fn interleaved_call_audio_splits_into_the_two_sides() {
        let b = CallBlock::from_interleaved(&[0.1, -0.1, 0.2, -0.2, 0.3], 48_000);
        assert_eq!(b.clinic, vec![0.1, 0.2]);
        assert_eq!(b.client, vec![-0.1, -0.2]);
        assert_eq!(b.sample_rate, 48_000);
    }
}
