//! A recording through the real live path whose microphone misbehaves:
//! the device switches mid-recording (48 kHz, then a Bluetooth headset's
//! 16 kHz, then its 8 kHz mode), the input goes digitally silent, and audio
//! stops arriving for a while. The recorder must keep every second of audio
//! it was sent, at true length, keep transcribing after each switch, and say
//! what happened.
//!
//! Needs the speech models and a speech recording, so it runs only when
//! `OPENBARK_TEST_MODELS_DIR` and `OPENBARK_TEST_SPEECH_WAV` are set; without
//! them it says so and passes.

use std::path::Path;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use os_asr::capture::AudioBlock;
use os_engine::live::{LiveOptions, LiveRecorder, LiveSink, LiveSource};
use os_engine::mic_health::{MicHealth, MicQuality, MicState};
use os_engine::transcript::Segment;
use os_models::ModelStore;

#[derive(Default)]
struct Seen {
    health: Vec<MicHealth>,
    segments: Vec<Segment>,
    levels: usize,
}

struct Sink(Arc<Mutex<Seen>>);

impl LiveSink for Sink {
    fn level(&mut self, _level: f32, _seconds: f32) {
        self.0.lock().unwrap().levels += 1;
    }
    fn segment(&mut self, segment: &Segment) {
        self.0.lock().unwrap().segments.push(segment.clone());
    }
    fn mic(&mut self, health: &MicHealth) {
        self.0.lock().unwrap().health.push(health.clone());
    }
}

/// `samples` (16 kHz) at `rate`, by simple linear interpolation: a stand-in
/// for a device running at that rate.
fn at_rate(samples: &[f32], rate: u32) -> Vec<f32> {
    let n = (samples.len() as u64 * rate as u64 / 16_000) as usize;
    (0..n)
        .map(|i| {
            let x = i as f64 * 16_000.0 / rate as f64;
            let j = x as usize;
            let f = (x - j as f64) as f32;
            let a = samples.get(j).copied().unwrap_or(0.0);
            let b = samples.get(j + 1).copied().unwrap_or(a);
            a + (b - a) * f
        })
        .collect()
}

/// Sends `samples` in 10 ms blocks, in real time (the watchdog works on the clock).
fn send(tx: &mpsc::Sender<AudioBlock>, samples: &[f32], rate: u32) {
    let block = (rate / 100) as usize;
    for chunk in samples.chunks(block) {
        tx.send(AudioBlock { samples: chunk.to_vec(), sample_rate: rate }).unwrap();
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn a_recording_survives_device_switches_silence_and_a_stall() {
    let (Some(models), Some(wav)) =
        (std::env::var_os("OPENBARK_TEST_MODELS_DIR"), std::env::var_os("OPENBARK_TEST_SPEECH_WAV"))
    else {
        eprintln!("skipped: set OPENBARK_TEST_MODELS_DIR and OPENBARK_TEST_SPEECH_WAV to run");
        return;
    };
    let speech = os_asr::Audio::load(Path::new(&wav)).unwrap().samples;
    // Three stretches of speech of 8 s each (or what the recording has).
    let part = (8 * 16_000).min(speech.len() / 3);
    let pieces: Vec<&[f32]> = speech.chunks(part).take(3).collect();
    let store = ModelStore::new(std::path::PathBuf::from(models));
    let tmp = tempfile::tempdir().unwrap();
    let seen = Arc::new(Mutex::new(Seen::default()));
    let (tx, rx) = mpsc::channel();
    let mut options = LiveOptions::microphone();
    options.label_speakers = false;
    let recorder = LiveRecorder::start(
        &store,
        tmp.path(),
        LiveSource::Stream { label: "browser".into(), blocks: rx },
        options,
        Box::new(Sink(seen.clone())),
    )
    .unwrap();

    // A 48 kHz microphone, then a Bluetooth headset at 16 kHz, then at 8 kHz.
    send(&tx, &at_rate(pieces[0], 48_000), 48_000);
    send(&tx, pieces[1], 16_000);
    send(&tx, &at_rate(pieces[2], 8_000), 8_000);
    // Five seconds of digital silence (a muted microphone).
    send(&tx, &vec![0.0; 5 * 8_000], 8_000);
    // Then nothing at all for four seconds (the browser tab stopped sending).
    std::thread::sleep(Duration::from_secs(4));
    // And speech again.
    send(&tx, &at_rate(pieces[0], 44_100), 44_100);
    drop(tx);
    let result = recorder.wait().unwrap();

    let sent_secs = (pieces[0].len() * 2 + pieces[1].len() + pieces[2].len()) as f32 / 16_000.0 + 5.0;
    assert!(
        (result.stats.audio_secs - sent_secs).abs() < 0.1,
        "every second sent is kept at its true length, and the stall adds nothing: {} s recorded, {} s sent",
        result.stats.audio_secs,
        sent_secs
    );
    let seen = seen.lock().unwrap();
    let states: Vec<MicState> = seen.health.iter().map(|h| h.state).collect();
    eprintln!("health: {:?}", seen.health);
    assert!(states.contains(&MicState::Silent), "the muted stretch is reported: {states:?}");
    assert!(states.contains(&MicState::Stalled), "the gap is reported: {states:?}");
    assert_eq!(states.last(), Some(&MicState::Ok), "and the recovery: {states:?}");
    let silent = seen.health.iter().find(|h| h.state == MicState::Silent).unwrap();
    assert!(silent.seconds > 2.0, "reported a few seconds into the silence, not at once");
    assert!(
        seen.health.iter().any(|h| h.sample_rate == 8_000 && h.quality == MicQuality::Narrowband),
        "8 kHz input is called narrowband"
    );
    // Speech before and after the switches is transcribed.
    let starts: Vec<f32> = result.transcript.segments.iter().map(|s| s.start).collect();
    let first = pieces[0].len() as f32 / 16_000.0;
    let second_end = first + pieces[1].len() as f32 / 16_000.0;
    let third_end = second_end + pieces[2].len() as f32 / 16_000.0;
    assert!(starts.iter().any(|&s| s < first), "48 kHz part transcribed: {starts:?}");
    assert!(starts.iter().any(|&s| s >= first && s < second_end), "16 kHz part transcribed: {starts:?}");
    assert!(starts.iter().any(|&s| s >= second_end && s < third_end), "8 kHz part transcribed: {starts:?}");
    assert!(starts.iter().any(|&s| s >= third_end + 5.0), "speech after the stall transcribed: {starts:?}");
    assert!(seen.levels > 100);
}
