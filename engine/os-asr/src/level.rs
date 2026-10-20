//! Loudness levelling for quiet voices, such as a client on a phone on
//! speaker across the room (15 to 25 dB below the vet, telephone band).
//!
//! Two tools:
//! - [`Leveller`]: a streaming automatic gain control for the copy of the
//!   audio that voice detection hears. It lifts quiet speech towards a target
//!   level within a syllable or two, but never lifts the background noise
//!   above a ceiling, so pauses stay pauses. It only drives detection: the
//!   saved recording is untouched.
//! - [`normalise`]: one gain for a whole utterance, so a quiet utterance
//!   reaches the recogniser and the speaker model at a normal level.
//!
//! Both are plain arithmetic, deterministic and cheap (well under 1% of a
//! core at 16 kHz).

use crate::SAMPLE_RATE;

/// 10 ms frames.
const FRAME: usize = SAMPLE_RATE as usize / 100;

/// Level the speech is lifted towards (RMS, dBFS).
pub const TARGET_DBFS: f32 = -20.0;
/// The most a quiet passage is lifted (dB).
pub const MAX_GAIN_DB: f32 = 30.0;
/// The background noise is never lifted above this (dBFS).
pub const NOISE_CEILING_DBFS: f32 = -50.0;

fn db_to_amp(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

/// Streaming gain control (see the module notes). Feed any block size.
pub struct Leveller {
    /// Samples waiting for a whole frame.
    pending: Vec<f32>,
    /// Gain applied at the end of the last frame (ramped across each frame).
    gain: f32,
    /// Speech envelope (frame RMS, fast attack, ~100 ms release).
    envelope: f32,
    /// Noise floor by minimum statistics: the smallest 100 ms level in each
    /// of the last ten half-second blocks.
    smooth: f32,
    block_min: f32,
    block_frames: usize,
    minima: std::collections::VecDeque<f32>,
}

impl Default for Leveller {
    fn default() -> Self {
        Self::new()
    }
}

impl Leveller {
    pub fn new() -> Self {
        Self {
            pending: Vec::new(),
            gain: 1.0,
            envelope: 0.0,
            smooth: 0.0,
            block_min: f32::MAX,
            block_frames: 0,
            minima: std::collections::VecDeque::new(),
        }
    }

    /// The noise floor estimate (RMS), once half a second has been heard.
    pub fn noise_floor(&self) -> Option<f32> {
        self.minima.iter().copied().reduce(f32::min)
    }

    /// Levelled audio for `samples`. Output lags input by less than a frame:
    /// samples are returned once their 10 ms frame is complete.
    pub fn process(&mut self, samples: &[f32]) -> Vec<f32> {
        self.pending.extend_from_slice(samples);
        let whole = self.pending.len() / FRAME * FRAME;
        let mut out = Vec::with_capacity(whole);
        let pending = std::mem::take(&mut self.pending);
        for f in 0..whole / FRAME {
            let frame = &pending[f * FRAME..(f + 1) * FRAME];
            let rms = (frame.iter().map(|s| s * s).sum::<f32>() / FRAME as f32).sqrt().max(1e-7);
            let target = self.frame_gain(rms);
            // Ramp from the last gain to this frame's, so the gain never steps.
            let start = self.gain;
            for (i, s) in frame.iter().enumerate() {
                let g = start + (target - start) * (i + 1) as f32 / FRAME as f32;
                out.push(limit(s * g));
            }
            self.gain = target;
        }
        self.pending = pending;
        self.pending.drain(..whole);
        out
    }

    /// Flush the last partial frame at its current gain.
    pub fn finish(&mut self) -> Vec<f32> {
        let g = self.gain;
        self.pending.drain(..).map(|s| limit(s * g)).collect()
    }

    fn frame_gain(&mut self, rms: f32) -> f32 {
        // Noise floor: 100 ms smoothing, minimum over 5 s.
        self.smooth = if self.smooth == 0.0 { rms } else { 0.9 * self.smooth + 0.1 * rms };
        self.block_min = self.block_min.min(self.smooth);
        self.block_frames += 1;
        if self.block_frames == 50 {
            self.minima.push_back(self.block_min);
            if self.minima.len() > 10 {
                self.minima.pop_front();
            }
            self.block_min = f32::MAX;
            self.block_frames = 0;
        }
        let floor = self.noise_floor().unwrap_or(self.block_min).max(1e-6);
        // Fast attack, ~100 ms release: a quiet voice after a loud one is
        // lifted within a few hundred milliseconds, its first word included.
        self.envelope = if rms > self.envelope { 0.5 * self.envelope + 0.5 * rms } else { 0.9 * self.envelope + 0.1 * rms };
        let want = db_to_amp(TARGET_DBFS) / self.envelope.max(1e-7);
        let cap = (db_to_amp(NOISE_CEILING_DBFS) / floor).min(db_to_amp(MAX_GAIN_DB));
        want.min(cap).max(1.0)
    }
}

/// A soft limiter: transparent below 0.9, never above 0.99.
fn limit(x: f32) -> f32 {
    const KNEE: f32 = 0.9;
    const CEILING: f32 = 0.99;
    let a = x.abs();
    if a <= KNEE { x } else { x.signum() * (KNEE + (CEILING - KNEE) * ((a - KNEE) / (CEILING - KNEE)).tanh()) }
}

/// One gain for a whole utterance: its speech (the louder half of its 10 ms
/// frames) is brought to [`TARGET_DBFS`], lifting by at most [`MAX_GAIN_DB`]
/// and never lowering, with the soft limiter on peaks.
pub fn normalise(samples: &[f32]) -> Vec<f32> {
    let gain = utterance_gain(samples);
    if (gain - 1.0).abs() < 1e-3 {
        return samples.to_vec();
    }
    samples.iter().map(|s| limit(s * gain)).collect()
}

/// The gain [`normalise`] applies.
pub fn utterance_gain(samples: &[f32]) -> f32 {
    let mut energies: Vec<f32> =
        samples.chunks(FRAME).map(|f| f.iter().map(|s| s * s).sum::<f32>() / f.len() as f32).collect();
    if energies.is_empty() {
        return 1.0;
    }
    energies.sort_by(|a, b| b.total_cmp(a));
    let loud = &energies[..energies.len().div_ceil(2)];
    let rms = (loud.iter().sum::<f32>() / loud.len() as f32).sqrt().max(1e-7);
    (db_to_amp(TARGET_DBFS) / rms).clamp(1.0, db_to_amp(MAX_GAIN_DB))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(secs: f32, db: f32) -> Vec<f32> {
        let amp = db_to_amp(db) * std::f32::consts::SQRT_2;
        (0..(secs * SAMPLE_RATE as f32) as usize)
            .map(|i| amp * (2.0 * std::f32::consts::PI * 440.0 * i as f32 / SAMPLE_RATE as f32).sin())
            .collect()
    }

    fn noise(secs: f32, db: f32) -> Vec<f32> {
        let mut x = 12345u32;
        let amp = db_to_amp(db) * 3f32.sqrt();
        (0..(secs * SAMPLE_RATE as f32) as usize)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                amp * ((x as f32 / u32::MAX as f32) * 2.0 - 1.0)
            })
            .collect()
    }

    fn rms_db(s: &[f32]) -> f32 {
        20.0 * (s.iter().map(|x| x * x).sum::<f32>() / s.len() as f32).sqrt().max(1e-9).log10()
    }

    fn mix(a: &[f32], b: &[f32]) -> Vec<f32> {
        a.iter().zip(b).map(|(x, y)| x + y).collect()
    }

    #[test]
    fn a_quiet_voice_after_a_loud_one_is_lifted_within_a_few_hundred_ms() {
        // Room noise at -70 dBFS: a second of room tone, a "vet" at -22 dBFS,
        // a pause, a "phone" at -45.
        let mut signal = vec![0.0; 16_000];
        signal.extend(tone(2.0, -22.0));
        signal.extend(vec![0.0; 11_200]); // 0.7 s pause
        signal.extend(tone(2.0, -45.0));
        let signal = mix(&signal, &noise(signal.len() as f32 / 16_000.0, -70.0));
        let mut lev = Leveller::new();
        let mut out = lev.process(&signal);
        out.extend(lev.finish());
        assert_eq!(out.len(), signal.len());
        let quiet = 3 * 16_000 + 11_200;
        // The loud voice is brought to the target (-20), not limited.
        let vet = rms_db(&out[24_000..48_000]);
        assert!((vet - TARGET_DBFS).abs() < 0.5, "vet {vet}");
        // The quiet voice: lifted by 20 dB, the noise ceiling's limit (floor -70 to -50).
        let lifted = rms_db(&out[quiet + 4_800..quiet + 32_000]);
        assert!((lifted - -25.0).abs() < 1.5, "quiet voice lifted to {lifted}");
        // Its first 100 ms already lifted by at least 15 dB.
        let onset = rms_db(&out[quiet..quiet + 1_600]);
        assert!(onset > -45.0 + 15.0, "onset {onset}");
        // The pause stays near the ceiling, well below the speech.
        let pause = rms_db(&out[48_000 + 6_400..quiet]);
        assert!(pause < -48.0, "pause {pause}");
    }

    #[test]
    fn noise_alone_is_never_lifted_above_the_ceiling() {
        let n = noise(8.0, -75.0);
        let mut lev = Leveller::new();
        let out = lev.process(&n);
        let late = rms_db(&out[5 * 16_000..]);
        assert!(late <= NOISE_CEILING_DBFS + 1.0, "noise lifted to {late}");
        assert!(late > -75.0 + 20.0, "but lifted: {late}");
    }

    #[test]
    fn loud_input_never_clips() {
        let loud = tone(1.0, 0.0); // peaks at +3 dBFS
        let out = Leveller::new().process(&loud);
        assert!(out.iter().all(|s| s.abs() < 1.0));
        // A quiet utterance with one loud click: lifted, the click limited.
        let mut quiet = tone(1.0, -45.0);
        quiet[8_000] = 0.2;
        assert!(normalise(&quiet).iter().all(|s| s.abs() < 1.0));
    }

    #[test]
    fn blocks_of_any_size_give_the_same_output() {
        let signal = mix(&tone(1.5, -40.0), &noise(1.5, -70.0));
        let whole = Leveller::new().process(&signal);
        let mut lev = Leveller::new();
        let mut pieces = Vec::new();
        for chunk in signal.chunks(333) {
            pieces.extend(lev.process(chunk));
        }
        assert_eq!(whole, pieces);
    }

    #[test]
    fn normalise_lifts_a_quiet_utterance_and_leaves_a_normal_one() {
        let quiet = tone(1.0, -45.0);
        assert!((rms_db(&normalise(&quiet)) - TARGET_DBFS).abs() < 0.5);
        let normal = tone(1.0, -18.0);
        assert_eq!(normalise(&normal), normal, "never lowered");
        let silent = vec![0.0f32; 1600];
        assert!(normalise(&silent).iter().all(|s| *s == 0.0));
        assert!((utterance_gain(&silent) - db_to_amp(MAX_GAIN_DB)).abs() < 1e-3);
    }
}
