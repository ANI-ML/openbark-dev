//! The microphone before and around a recording: loading the speech models
//! ahead of time so Record starts at once, and a quick microphone test with
//! a live level meter.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use os_asr::capture::AudioBlock;
use os_asr::mic::{CpalBackend, MicBackend, MicEvent, Supervisor, Timing};
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
    /// Loaded and handed back: a recording that couldn't open its microphone.
    Ready(WarmKey, Box<SpeechModels>),
}

/// Numbers each microphone test, so the app can ignore a stopped test's
/// last events once a new one has started.
static MIC_TESTS: AtomicU64 = AtomicU64::new(0);

/// A running microphone test.
pub(crate) struct MicTest {
    number: u64,
    stop: Arc<AtomicBool>,
    handle: JoinHandle<()>,
}

/// Sent about 20 times a second while the microphone is tested.
#[derive(Debug, Clone, Serialize)]
pub struct MicLevel {
    /// Which test this is (see [`MicTestStarted::test`]).
    pub test: u64,
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
    /// This test's number: its events carry it.
    pub test: u64,
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
            Warm::Loading(k, _) | Warm::Ready(k, _) if *k == key => return true,
            // One loader at a time: toggling options while one runs doesn't
            // start another beside it (the next Record loads what it needs).
            Warm::Loading(_, h) if !h.is_finished() => return false,
            _ => {}
        }
        // Free what was loaded for other options before measuring memory.
        // (On a graphics card the same models load at Record anyway, and
        // they're freed before a note model loads.)
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
            Warm::Ready(k, m) if k == key => Some(*m),
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

    /// Speech models a recording didn't use, kept for the next one.
    pub(crate) fn keep_warm(&self, options: &LiveOptions, models: SpeechModels) {
        if let Ok(mut warm) = self.warm.lock()
            && matches!(*warm, Warm::Empty)
        {
            *warm = Warm::Ready(WarmKey::of(options), Box::new(models));
        }
    }

    /// Open a microphone (the one chosen in Settings, or the default) and
    /// send its level through [`Events::mic_test`] until
    /// [`App::stop_mic_test`], a recording starts, or a minute passes.
    pub fn start_mic_test(&self, device: Option<String>, events: Arc<dyn Events>) -> Result<MicTestStarted> {
        self.start_mic_test_on(CpalBackend, device, events)
    }

    /// [`App::start_mic_test`] on any device layer (tests use a fake).
    pub(crate) fn start_mic_test_on<B>(
        &self,
        backend: B,
        device: Option<String>,
        events: Arc<dyn Events>,
    ) -> Result<MicTestStarted>
    where
        B: MicBackend + Send + 'static,
    {
        // Held until the new test is stored: a second start waits, and so
        // does a recording's stop_mic_test, which then stops this one.
        let mut slot = self.mic_test.lock().map_err(|_| AppError("microphone lock poisoned".into()))?;
        if let Some(old) = slot.take() {
            old.stop.store(true, Ordering::SeqCst);
            let _ = old.handle.join();
        }
        if self.recorder.lock().map(|r| r.is_some()).unwrap_or(false) {
            return Err(AppError("A recording is running; its waveform shows the microphone.".into()));
        }
        let test = MIC_TESTS.fetch_add(1, Ordering::SeqCst) + 1;
        let stop = Arc::new(AtomicBool::new(false));
        let (ready_tx, ready_rx) = mpsc::channel::<std::result::Result<MicTestStarted, String>>();
        let stop2 = stop.clone();
        let handle = std::thread::Builder::new()
            .name("openbark-mic-test".into())
            .spawn(move || run_mic_test(backend, test, device, events, stop2, ready_tx))
            .map_err(|e| AppError(e.to_string()))?;
        let started = ready_rx
            .recv()
            .map_err(|_| AppError("The microphone test couldn't start.".into()))?
            .map_err(AppError)?;
        *slot = Some(MicTest { number: test, stop, handle });
        Ok(started)
    }

    /// Stop test number `number` if it's the one running (a newer test the
    /// app started since is left alone).
    pub fn stop_mic_test_number(&self, number: u64) {
        let test = self.mic_test.lock().ok().and_then(|mut t| t.take_if(|t| t.number == number));
        if let Some(test) = test {
            test.stop.store(true, Ordering::SeqCst);
            let _ = test.handle.join();
        }
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

fn run_mic_test<B: MicBackend>(
    backend: B,
    test: u64,
    device: Option<String>,
    events: Arc<dyn Events>,
    stop: Arc<AtomicBool>,
    ready: mpsc::Sender<std::result::Result<MicTestStarted, String>>,
) {
    let (tx, rx) = mpsc::channel::<AudioBlock>();
    let (mut mic, first) = match Supervisor::start(backend, device, tx, Timing::default()) {
        Ok(started) => started,
        Err(e) => {
            let _ = ready.send(Err(e.to_string()));
            return;
        }
    };
    let MicEvent::Opened { device: mut name, mut sample_rate, replaced } = first else { return };
    let _ = ready.send(Ok(MicTestStarted {
        test,
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
                test,
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
        test,
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

    use std::sync::Mutex;
    use std::sync::atomic::AtomicUsize;
    use std::time::Duration;

    use os_asr::capture::{CaptureError, Fault};
    use os_asr::mic::MicStream;
    use os_models::ModelStore;

    use super::super::{Events, Progress, Segment};

    /// A microphone that opens instantly and counts its open streams.
    #[derive(Clone, Default)]
    struct FakeMic {
        open: Arc<AtomicUsize>,
        opened: Arc<AtomicUsize>,
    }

    struct FakeStream(Arc<AtomicUsize>);

    impl Drop for FakeStream {
        fn drop(&mut self) {
            self.0.fetch_sub(1, Ordering::SeqCst);
        }
    }

    impl MicStream for FakeStream {
        fn device_name(&self) -> &str {
            "Fake mic"
        }
        fn sample_rate(&self) -> u32 {
            48_000
        }
        fn fault(&self) -> Option<Fault> {
            None
        }
        fn idle_for(&self) -> Duration {
            Duration::ZERO
        }
    }

    impl MicBackend for FakeMic {
        type Stream = FakeStream;
        fn default_name(&self) -> Option<String> {
            Some("Fake mic".into())
        }
        fn has(&self, name: &str) -> bool {
            name == "Fake mic"
        }
        fn open(
            &self,
            _name: Option<&str>,
            _tx: mpsc::Sender<AudioBlock>,
        ) -> std::result::Result<FakeStream, CaptureError> {
            self.open.fetch_add(1, Ordering::SeqCst);
            self.opened.fetch_add(1, Ordering::SeqCst);
            Ok(FakeStream(self.open.clone()))
        }
    }

    #[derive(Default)]
    struct Seen(Mutex<Vec<(u64, bool)>>);

    impl Events for Seen {
        fn segment(&self, _: &Segment) {}
        fn token(&self, _: &str) {}
        fn level(&self, _: f32, _: f32) {}
        fn download(&self, _: &str, _: u64, _: u64) {}
        fn progress(&self, _: &Progress) {}
        fn mic_test(&self, level: &MicLevel) {
            self.0.lock().unwrap().push((level.test, level.done));
        }
    }

    fn app(tmp: &std::path::Path) -> App {
        App::new(ModelStore::new(tmp.join("models")), tmp.join("sessions"))
    }

    #[test]
    fn starting_a_second_test_stops_the_first() {
        let tmp = tempfile::tempdir().unwrap();
        let app = app(tmp.path());
        let mic = FakeMic::default();
        let events = Arc::new(Seen::default());
        let first = app.start_mic_test_on(mic.clone(), None, events.clone()).unwrap();
        let second = app.start_mic_test_on(mic.clone(), None, events.clone()).unwrap();
        assert_eq!(mic.opened.load(Ordering::SeqCst), 2);
        assert_eq!(mic.open.load(Ordering::SeqCst), 1, "the first test's microphone was let go");
        assert!(second.test > first.test);
        // The first test's last event says done and carries its own number,
        // so the app can tell it from the running test's.
        let seen = events.0.lock().unwrap().clone();
        assert!(seen.contains(&(first.test, true)));
        // Stopping the first by number leaves the second running.
        app.stop_mic_test_number(first.test);
        assert_eq!(mic.open.load(Ordering::SeqCst), 1);
        app.stop_mic_test_number(second.test);
        assert_eq!(mic.open.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn tests_started_from_two_threads_at_once_leave_one_microphone_open() {
        let tmp = tempfile::tempdir().unwrap();
        let app = app(tmp.path());
        let mic = FakeMic::default();
        let events: Arc<dyn Events> = Arc::new(Seen::default());
        std::thread::scope(|s| {
            for _ in 0..4 {
                s.spawn(|| app.start_mic_test_on(mic.clone(), None, events.clone()).unwrap());
            }
        });
        assert_eq!(mic.open.load(Ordering::SeqCst), 1);
        app.stop_mic_test();
        assert_eq!(mic.open.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_recording_stops_a_test_that_is_still_opening() {
        // stop_mic_test waits for a start in progress (one lock across the
        // start), then stops it: Record never runs beside a test.
        let tmp = tempfile::tempdir().unwrap();
        let app = app(tmp.path());
        let mic = FakeMic::default();
        let events: Arc<dyn Events> = Arc::new(Seen::default());
        std::thread::scope(|s| {
            s.spawn(|| app.start_mic_test_on(mic.clone(), None, events.clone()));
            s.spawn(|| {
                std::thread::sleep(Duration::from_millis(1));
                app.stop_mic_test();
            });
        });
        // Whichever ran first, at most the test that won is left, and a
        // recording's second stop (after it's stored) ends that too.
        app.stop_mic_test();
        assert_eq!(mic.open.load(Ordering::SeqCst), 0);
    }

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
