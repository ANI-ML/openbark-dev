//! The microphone's health during a recording, for the Record screen: is
//! audio arriving, is it more than digital silence, which microphone is it,
//! and is it a Bluetooth headset in its low-quality hands-free mode.
//!
//! - **Silent:** blocks arrive but every sample is (almost) zero for a few
//!   seconds. That's a muted microphone, a hardware mute switch, or a
//!   blocked permission: macOS hands an app without microphone access
//!   silence rather than an error, and so does Windows with microphone
//!   access off. A quiet room is never this quiet.
//! - **Stalled:** no audio at all for a few seconds (the device stopped
//!   calling back, or a browser tab stopped sending).
//! - **Lost:** the microphone went away and no other could be opened; the
//!   recording is paused until one comes back.
//!
//! The same rules run in the browser for web mode (apps/desktop/src/mic).

use std::time::{Duration, Instant};

use serde::Serialize;

/// Peak below this (about -80 dBFS) counts as digital silence. Real
/// microphones in a quiet room still pick up -70 to -60 dBFS of noise.
pub const SILENT_PEAK: f32 = 1e-4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MicState {
    Ok,
    Silent,
    Stalled,
    Lost,
}

/// What the input rate and name say about the audio quality.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MicQuality {
    Good,
    /// A Bluetooth headset in hands-free (call) mode: 16 or 24 kHz,
    /// compressed for calls. Usable, but less clear than a built-in mic.
    HandsFree,
    /// Below 16 kHz (8 kHz telephone quality): consonants are lost.
    Narrowband,
}

/// Bluetooth headsets in call mode, as Windows, macOS and Linux name them.
const HANDS_FREE_NAMES: [&str; 8] =
    ["hands-free", "handsfree", "headset", "airpods", "bluetooth", "beats", "buds", "hfp"];

pub fn quality(device: &str, sample_rate: u32) -> MicQuality {
    if sample_rate > 0 && sample_rate < 16_000 {
        return MicQuality::Narrowband;
    }
    let name = device.to_lowercase();
    if sample_rate <= 24_000 && HANDS_FREE_NAMES.iter().any(|n| name.contains(n)) {
        return MicQuality::HandsFree;
    }
    MicQuality::Good
}

/// Sent to the app whenever any of it changes.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MicHealth {
    pub state: MicState,
    /// The microphone's name; empty when the browser supplies the audio.
    pub device: String,
    pub sample_rate: u32,
    pub quality: MicQuality,
    /// After a switch: the microphone recording moved away from.
    pub switched_from: Option<String>,
    /// Seconds of audio recorded so far.
    pub seconds: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct HealthTiming {
    pub silent_after: Duration,
    pub stalled_after: Duration,
}

impl Default for HealthTiming {
    fn default() -> Self {
        Self { silent_after: Duration::from_secs(4), stalled_after: Duration::from_secs(3) }
    }
}

/// Follows a recording's audio and says when its health changes.
pub struct HealthTracker {
    timing: HealthTiming,
    device: String,
    sample_rate: u32,
    switched_from: Option<String>,
    lost: bool,
    last_block: Instant,
    /// When the current run of silent blocks began.
    silent_since: Option<Instant>,
    last_sent: Option<MicHealth>,
}

impl HealthTracker {
    pub fn new(device: &str, sample_rate: u32, now: Instant, timing: HealthTiming) -> Self {
        Self {
            timing,
            device: device.to_string(),
            sample_rate,
            switched_from: None,
            lost: false,
            last_block: now,
            silent_since: None,
            last_sent: None,
        }
    }

    /// Recording from a (possibly different) microphone now.
    pub fn opened(&mut self, device: &str, sample_rate: u32, replaced: Option<&str>, now: Instant) {
        if replaced.is_some() {
            self.switched_from = replaced.map(str::to_string);
        }
        self.device = device.to_string();
        self.sample_rate = sample_rate;
        self.lost = false;
        self.last_block = now;
        self.silent_since = None;
    }

    pub fn lost(&mut self) {
        self.lost = true;
    }

    /// A block arrived with this peak level and rate.
    pub fn block(&mut self, peak: f32, sample_rate: u32, now: Instant) {
        if now.duration_since(self.last_block) >= self.timing.stalled_after {
            // Audio is back after a gap: silence before the gap doesn't count.
            self.silent_since = None;
        }
        self.last_block = now;
        self.lost = false;
        self.sample_rate = sample_rate;
        if peak < SILENT_PEAK {
            self.silent_since.get_or_insert(now);
        } else {
            self.silent_since = None;
        }
    }

    fn state(&self, now: Instant) -> MicState {
        if self.lost {
            MicState::Lost
        } else if now.duration_since(self.last_block) >= self.timing.stalled_after {
            MicState::Stalled
        } else if self.silent_since.is_some_and(|s| now.duration_since(s) >= self.timing.silent_after) {
            MicState::Silent
        } else {
            MicState::Ok
        }
    }

    /// The health now, if it differs from what was last returned.
    pub fn tick(&mut self, now: Instant, seconds: f32) -> Option<MicHealth> {
        let health = MicHealth {
            state: self.state(now),
            device: self.device.clone(),
            sample_rate: self.sample_rate,
            quality: quality(&self.device, self.sample_rate),
            switched_from: self.switched_from.clone(),
            seconds,
        };
        let same =
            self.last_sent.as_ref().is_some_and(|h| MicHealth { seconds: h.seconds, ..health.clone() } == *h);
        if same {
            return None;
        }
        self.last_sent = Some(health.clone());
        Some(health)
    }
}

/// Largest absolute sample.
pub fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0f32, |m, s| m.max(s.abs()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(t0: Instant, ms: u64) -> Instant {
        t0 + Duration::from_millis(ms)
    }

    #[test]
    fn reports_ok_once_then_only_changes() {
        let t0 = Instant::now();
        let mut h = HealthTracker::new("Built-in Microphone", 48_000, t0, HealthTiming::default());
        h.block(0.2, 48_000, at(t0, 50));
        let first = h.tick(at(t0, 100), 0.1).unwrap();
        assert_eq!(first.state, MicState::Ok);
        assert_eq!(first.quality, MicQuality::Good);
        h.block(0.3, 48_000, at(t0, 150));
        assert_eq!(h.tick(at(t0, 200), 0.2), None, "no change, nothing sent");
    }

    #[test]
    fn digital_silence_warns_after_four_seconds_and_clears_on_sound() {
        let t0 = Instant::now();
        let mut h = HealthTracker::new("Built-in Microphone", 48_000, t0, HealthTiming::default());
        h.tick(t0, 0.0);
        for ms in (0..4_000).step_by(50) {
            h.block(0.0, 48_000, at(t0, ms));
            assert_eq!(h.tick(at(t0, ms), 0.0), None, "too early at {ms} ms");
        }
        h.block(0.0, 48_000, at(t0, 4_050));
        assert_eq!(h.tick(at(t0, 4_050), 4.0).unwrap().state, MicState::Silent);
        h.block(0.05, 48_000, at(t0, 4_100));
        assert_eq!(h.tick(at(t0, 4_100), 4.1).unwrap().state, MicState::Ok);
    }

    #[test]
    fn a_quiet_room_is_not_silence() {
        let t0 = Instant::now();
        let mut h = HealthTracker::new("USB mic", 44_100, t0, HealthTiming::default());
        h.tick(t0, 0.0);
        // -66 dBFS of room noise for ten seconds.
        for ms in (0..10_000).step_by(50) {
            h.block(0.0005, 44_100, at(t0, ms));
            assert_eq!(h.tick(at(t0, ms), 0.0), None);
        }
    }

    #[test]
    fn no_audio_for_three_seconds_is_stalled() {
        let t0 = Instant::now();
        let mut h = HealthTracker::new("USB mic", 44_100, t0, HealthTiming::default());
        h.block(0.1, 44_100, t0);
        h.tick(t0, 0.0);
        assert_eq!(h.tick(at(t0, 2_900), 0.0), None);
        assert_eq!(h.tick(at(t0, 3_000), 0.0).unwrap().state, MicState::Stalled);
        h.block(0.1, 44_100, at(t0, 3_100));
        assert_eq!(h.tick(at(t0, 3_100), 0.0).unwrap().state, MicState::Ok);
    }

    #[test]
    fn silence_before_a_stall_does_not_count_after_it() {
        let t0 = Instant::now();
        let mut h = HealthTracker::new("USB mic", 44_100, t0, HealthTiming::default());
        h.tick(t0, 0.0);
        for ms in (0..3_000).step_by(50) {
            h.block(0.0, 44_100, at(t0, ms));
        }
        assert_eq!(h.tick(at(t0, 6_500), 0.0).unwrap().state, MicState::Stalled);
        // The first block back is quiet: not "silent" for the 6 s before.
        h.block(0.0, 44_100, at(t0, 6_600));
        assert_eq!(h.tick(at(t0, 6_600), 0.0).unwrap().state, MicState::Ok);
    }

    #[test]
    fn a_switch_to_a_bluetooth_headset_is_reported_with_its_quality() {
        let t0 = Instant::now();
        let mut h = HealthTracker::new("MacBook Pro Microphone", 48_000, t0, HealthTiming::default());
        h.block(0.1, 48_000, t0);
        h.tick(t0, 0.0);
        h.lost();
        assert_eq!(h.tick(at(t0, 100), 1.0).unwrap().state, MicState::Lost);
        h.opened("AirPods Pro", 16_000, Some("MacBook Pro Microphone"), at(t0, 200));
        let after = h.tick(at(t0, 200), 1.0).unwrap();
        assert_eq!(after.state, MicState::Ok);
        assert_eq!(after.device, "AirPods Pro");
        assert_eq!(after.quality, MicQuality::HandsFree);
        assert_eq!(after.switched_from.as_deref(), Some("MacBook Pro Microphone"));
    }

    #[test]
    fn quality_from_rate_and_name() {
        assert_eq!(quality("Headset (AirPods Hands-Free AG Audio)", 8_000), MicQuality::Narrowband);
        assert_eq!(quality("Headset (AirPods Hands-Free AG Audio)", 16_000), MicQuality::HandsFree);
        assert_eq!(quality("AirPods Pro", 24_000), MicQuality::HandsFree);
        assert_eq!(quality("Jabra Speak 510 USB", 16_000), MicQuality::Good);
        assert_eq!(quality("MacBook Pro Microphone", 48_000), MicQuality::Good);
        assert_eq!(quality("Headset Microphone (USB)", 48_000), MicQuality::Good);
        assert_eq!(quality("", 0), MicQuality::Good);
    }

    #[test]
    fn peak_is_the_loudest_sample_either_way() {
        assert_eq!(peak(&[0.1, -0.5, 0.3]), 0.5);
        assert_eq!(peak(&[]), 0.0);
    }
}
