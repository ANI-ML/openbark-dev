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
    /// Audio pushed by the caller, e.g. a browser streaming its microphone
    /// over a WebSocket. Recording ends when every sender is dropped.
    Stream { label: String, blocks: mpsc::Receiver<AudioBlock> },
}

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
}

impl LiveOptions {
    pub fn microphone() -> Self {
        Self {
            stt: models::DEFAULT_STT.into(),
            label_speakers: true,
            speaker_threshold: DEFAULT_SPEAKER_THRESHOLD,
            patient: None,
            consent: None,
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
        let asr = os_asr::OfflineAsr::new(models::asr_model(store, &options.stt)?)?;
        let tracker = if options.label_speakers {
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
            LiveSource::Stream { label, .. } => label.clone(),
        };
        let lexicon = crate::vocab::Lexicon::default_with_user();
        // Second recogniser for segments that matter (M1 item 4); optional.
        let second = models::asr_model(store, SECOND_STT).ok().and_then(|m| os_asr::OfflineAsr::new(m).ok());
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
        let session_dir = session.dir.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let stop2 = stop.clone();
        let worker = std::thread::Builder::new()
            .name("openbark-live".into())
            .spawn(move || run(session, source, options, vad, asr, tracker, lexicon, second, sink, stop2))
            .map_err(|e| EngineError::Session(e.to_string()))?;
        Ok(Self { stop, worker: Some(worker), session_dir })
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
/// file ends). Returns the device name.
fn spawn_source(
    source: LiveSource,
    tx: mpsc::Sender<AudioBlock>,
    stop: Arc<AtomicBool>,
) -> Result<(JoinHandle<()>, String), EngineError> {
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
        return Ok((handle, label));
    }
    let (ready_tx, ready_rx) = mpsc::channel::<Result<String, String>>();
    let handle = std::thread::spawn(move || match source {
        LiveSource::Microphone(device) => {
            // cpal streams aren't Send: create, hold and drop on this thread.
            match MicCapture::start(device.as_deref(), tx) {
                Ok(mic) => {
                    let _ = ready_tx.send(Ok(mic.device_name.clone()));
                    while !stop.load(Ordering::SeqCst) {
                        std::thread::sleep(Duration::from_millis(20));
                    }
                }
                Err(e) => {
                    let _ = ready_tx.send(Err(e.to_string()));
                }
            }
        }
        LiveSource::File { path, realtime } => match Audio::load_wav(&path) {
            Ok(audio) => {
                let _ = ready_tx.send(Ok(format!("file: {}", path.display())));
                let block = (SAMPLE_RATE / 50) as usize; // 20 ms
                for chunk in audio.samples.chunks(block) {
                    if stop.load(Ordering::SeqCst) {
                        break;
                    }
                    if tx.send(AudioBlock { samples: chunk.to_vec(), sample_rate: SAMPLE_RATE }).is_err() {
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
        },
        LiveSource::Stream { .. } => unreachable!("handled above"),
    });
    let device = ready_rx
        .recv()
        .map_err(|_| EngineError::Session("audio source failed to start".into()))?
        .map_err(EngineError::Session)?;
    Ok((handle, device))
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
    let (source, device) = spawn_source(source, tx, stop.clone())?;
    let mut wav = WavWriter::create(&session.audio_path(), SAMPLE_RATE)
        .map_err(|e| EngineError::Session(e.to_string()))?;
    let mut streaming = vad.streaming()?;
    let mut transcript = Transcript::default();
    let mut resampler: Option<(u32, os_asr::sherpa_onnx::LinearResampler)> = None;
    let mut level_acc = (0.0f32, 0usize);
    let level_every = (SAMPLE_RATE / 20) as usize;
    let mut asr_ms = 0u64;

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
            {
                let _ = tx.send((transcript.segments.len(), u.samples.clone(), segment.text.clone()));
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
    }
    let stopped_at = stopped_at.unwrap_or_else(Instant::now);
    let _ = source.join();

    let last = streaming.finish();
    handle_utterances(last, &mut transcript, &mut sink, &mut asr_ms)?;
    // Collect second opinions (only the last few are still pending at stop).
    drop(job_tx);
    if let (Some(done_rx), Some(checker)) = (done_rx, checker) {
        let _ = checker.join();
        for (i, opinion, hints) in done_rx.try_iter() {
            if let Some(seg) = transcript.segments.get_mut(i) {
                for h in hints {
                    if !seg.hints.iter().any(|x| x.heard.eq_ignore_ascii_case(&h.heard)) {
                        seg.hints.push(h);
                    }
                }
                seg.check = Some(opinion);
            }
        }
    }
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
