//! The microphone before and around a recording: loading the speech models
//! ahead of time so Record starts at once, and a quick microphone test with
//! a live level meter.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use os_asr::capture::AudioBlock;
use os_asr::mic::{CpalBackend, MicEvent, Supervisor, Timing};
use os_engine::EngineError;
use os_engine::live::{LiveOptions, SpeechModels};
use os_engine::mic_health::{self, MicQuality};
use serde::Serialize;

use super::{App, AppError, Events, Result};

/// Load ahead of time only with at least this much memory free: the speech
/// models take about 1 GB, and drafting needs the rest.
const PREWARM_MIN_FREE: u64 = 3 * 1024 * 1024 * 1024;

/// A microphone test stops by itself after this long.
const MIC_TEST_LIMIT: Duration = Duration::from_secs(60);

/// What the warm models were loaded for.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct WarmKey {
    stt: String,
    label_speakers: bool,
    speakerphone: bool,
}

impl WarmKey {
    fn of(options: &LiveOptions) -> Self {
        Self {
            stt: options.stt.clone(),
            label_speakers: options.label_speakers,
            speakerphone: options.speakerphone,
        }
    }
}

/// Speech models loaded ahead of a recording.
#[derive(Default)]
pub(crate) enum Warm {
    #[default]
    Empty,
    /// Loading, or loaded: the thread's result holds the models until taken.
    Loading(WarmKey, JoinHandle<std::result::Result<SpeechModels, EngineError>>),
}

/// A running microphone test.
pub(crate) struct MicTest {
    stop: Arc<AtomicBool>,
    handle: JoinHandle<()>,
}

/// Sent about 20 times a second while the microphone is tested.
#[derive(Debug, Clone, Serialize)]
pub struct MicLevel {
    pub device: String,
    pub sample_rate: u32,
    pub quality: MicQuality,
    /// 0 (silence) to 1, on the recorder's scale (-60 to 0 dBFS).
    pub level: f32,
    /// The input is digital silence (a muted or blocked microphone).
    pub silent: bool,
    /// The test ended (time limit, or the microphone went away).
    pub done: bool,
}

/// The microphone a test opened.
#[derive(Debug, Clone, Serialize)]
pub struct MicTestStarted {
    pub device: String,
    pub sample_rate: u32,
    pub quality: MicQuality,
    /// The microphone chosen in Settings wasn't connected; this is the default.
    pub replaced: Option<String>,
}

/// The recorder's level scale: -60..0 dBFS to 0..1.
fn level_of(sum_sq: f32, n: usize) -> f32 {
    let rms = (sum_sq / n.max(1) as f32).sqrt();
    let db = 20.0 * rms.max(1e-6).log10();
    ((db + 60.0) / 60.0).clamp(0.0, 1.0)
}

fn free_memory() -> u64 {
    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    sys.available_memory()
}

impl App {
    fn live_options(&self, label_speakers: bool, speakerphone: bool) -> LiveOptions {
        let mut options = LiveOptions::microphone();
        options.label_speakers = label_speakers;
        options.speakerphone = speakerphone;
        options
    }

    /// Load the speech models now (in the background) so pressing Record
    /// starts recording at once. Skipped while recording, while a note model
    /// is loaded, or when memory is short. True when the models are loaded
    /// or loading.
    pub fn prewarm_speech(&self, label_speakers: bool, speakerphone: bool) -> bool {
        let options = self.live_options(label_speakers, speakerphone);
        let key = WarmKey::of(&options);
        // Checked before taking the warm lock: start_recording takes them the
        // other way round.
        let recording = self.recorder.lock().map(|r| r.is_some()).unwrap_or(true);
        if recording {
            return false;
        }
        let Ok(mut warm) = self.warm.lock() else { return false };
        match &*warm {
            Warm::Loading(k, _) if *k == key => return true,
            _ => {}
        }
        *warm = Warm::Empty;
        let free = free_memory();
        if free < PREWARM_MIN_FREE {
            tracing::info!(free_mb = free / 1_048_576, "not loading speech models ahead: memory is short");
            return false;
        }
        let store = self.store.clone();
        let handle = std::thread::Builder::new()
            .name("openbark-prewarm".into())
            .spawn(move || SpeechModels::load(&store, &options, false));
        match handle {
            Ok(h) => {
                *warm = Warm::Loading(key, h);
                true
            }
            Err(_) => false,
        }
    }

    /// Free speech models loaded ahead of time (the Record screen closed, or
    /// a note model needs the memory). A load still running finishes and is
    /// then freed.
    pub fn release_speech(&self) {
        if let Ok(mut warm) = self.warm.lock() {
            *warm = Warm::Empty;
        }
    }

    /// The warm models if they suit this recording, waiting for them if they
    /// are still loading.
    pub(crate) fn take_warm(&self, options: &LiveOptions) -> Option<SpeechModels> {
        let taken = std::mem::take(&mut *self.warm.lock().ok()?);
        let key = WarmKey::of(options);
        match taken {
            Warm::Loading(k, h) if k == key => match h.join() {
                Ok(Ok(m)) => Some(m),
                Ok(Err(e)) => {
                    tracing::info!(error = %e, "speech models didn't load ahead; loading now");
                    None
                }
                Err(_) => None,
            },
            _ => None,
        }
    }

    /// Open a microphone (the one chosen in Settings, or the default) and
    /// send its level through [`Events::mic_test`] until
    /// [`App::stop_mic_test`], a recording starts, or a minute passes.
    pub fn start_mic_test(&self, device: Option<String>, events: Arc<dyn Events>) -> Result<MicTestStarted> {
        self.stop_mic_test();
        if self.recorder.lock().map(|r| r.is_some()).unwrap_or(false) {
            return Err(AppError("A recording is running; its waveform shows the microphone.".into()));
        }
        let stop = Arc::new(AtomicBool::new(false));
        let (ready_tx, ready_rx) = mpsc::channel::<std::result::Result<MicTestStarted, String>>();
        let stop2 = stop.clone();
        let handle = std::thread::Builder::new()
            .name("openbark-mic-test".into())
            .spawn(move || run_mic_test(device, events, stop2, ready_tx))
            .map_err(|e| AppError(e.to_string()))?;
        let started = ready_rx
            .recv()
            .map_err(|_| AppError("The microphone test couldn't start.".into()))?
            .map_err(AppError)?;
        *self.mic_test.lock().map_err(|_| AppError("microphone lock poisoned".into()))? =
            Some(MicTest { stop, handle });
        Ok(started)
    }

    /// Stop the microphone test, if one is running.
    pub fn stop_mic_test(&self) {
        let test = self.mic_test.lock().ok().and_then(|mut t| t.take());
        if let Some(test) = test {
            test.stop.store(true, Ordering::SeqCst);
            let _ = test.handle.join();
        }
    }
}

fn run_mic_test(
    device: Option<String>,
    events: Arc<dyn Events>,
    stop: Arc<AtomicBool>,
    ready: mpsc::Sender<std::result::Result<MicTestStarted, String>>,
) {
    let (tx, rx) = mpsc::channel::<AudioBlock>();
    let (mut mic, first) = match Supervisor::start(CpalBackend, device, tx, Timing::default()) {
        Ok(started) => started,
        Err(e) => {
            let _ = ready.send(Err(e.to_string()));
            return;
        }
    };
    let MicEvent::Opened { device: mut name, mut sample_rate, replaced } = first else { return };
    let _ = ready.send(Ok(MicTestStarted {
        quality: mic_health::quality(&name, sample_rate),
        device: name.clone(),
        sample_rate,
        replaced,
    }));
    let started = Instant::now();
    let (mut sum_sq, mut n, mut peak) = (0.0f32, 0usize, 0.0f32);
    let mut silent_since: Option<Instant> = None;
    let mut sent = Instant::now();
    let mut lost = false;
    while !stop.load(Ordering::SeqCst) && started.elapsed() < MIC_TEST_LIMIT {
        if let Ok(block) = rx.recv_timeout(Duration::from_millis(50)) {
            sample_rate = block.sample_rate;
            for s in &block.samples {
                sum_sq += s * s;
                peak = peak.max(s.abs());
            }
            n += block.samples.len();
        }
        let now = Instant::now();
        for event in mic.poll(now) {
            match event {
                MicEvent::Opened { device, sample_rate: rate, .. } => {
                    name = device;
                    sample_rate = rate;
                    lost = false;
                }
                MicEvent::Lost { .. } => lost = true,
                MicEvent::Stalled { .. } => {}
            }
        }
        if now.duration_since(sent) >= Duration::from_millis(50) {
            sent = now;
            if n > 0 && peak < mic_health::SILENT_PEAK {
                silent_since.get_or_insert(now);
            } else if n > 0 {
                silent_since = None;
            }
            events.mic_test(&MicLevel {
                device: name.clone(),
                sample_rate,
                quality: mic_health::quality(&name, sample_rate),
                level: if n > 0 { level_of(sum_sq, n) } else { 0.0 },
                silent: silent_since.is_some_and(|s| now.duration_since(s) >= Duration::from_secs(2)),
                done: lost,
            });
            (sum_sq, n, peak) = (0.0, 0, 0.0);
        }
    }
    events.mic_test(&MicLevel {
        device: name.clone(),
        sample_rate,
        quality: mic_health::quality(&name, sample_rate),
        level: 0.0,
        silent: false,
        done: true,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_test_meter_uses_the_recorders_scale() {
        assert_eq!(level_of(0.0, 100), 0.0);
        // A full-scale sine is about -3 dBFS.
        let sine: Vec<f32> = (0..1000).map(|i| (i as f32 * 0.1).sin()).collect();
        let l = level_of(sine.iter().map(|s| s * s).sum(), sine.len());
        assert!((0.9..1.0).contains(&l), "{l}");
        // -40 dBFS speech, a quiet talker far away, still moves it.
        assert!((level_of(0.01f32.powi(2) * 100.0, 100) - 1.0 / 3.0).abs() < 0.01);
    }
}
