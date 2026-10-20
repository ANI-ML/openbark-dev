//! A simulated speakerphone, for the `phone:` key of a consult script: one
//! speaker (the client) talks through a phone line and a phone's loudspeaker
//! into the room, the other (the vet) talks straight into the computer's mic.
//!
//! The far-end chain, in order:
//! 1. telephone band, 300-3400 Hz (4th-order Butterworth high- and low-pass);
//! 2. down to 8 kHz, G.711-style 8-bit mu-law companding, and back to 16 kHz;
//! 3. the phone's own level control (a fast 3:1 compressor) and a little
//!    loudspeaker distortion (soft clipping);
//! 4. room reverb (a Schroeder reverberator, about 0.5 s, mostly reverberant
//!    since the phone is a metre or more away);
//! 5. a level `level_db` below the vet's (active speech, RMS).
//!
//! The vet gets a light touch of the same room. Light background noise
//! (`noise_db` below the vet) runs under the whole mix. Everything is
//! deterministic for a given `seed`, so the WAVs are regenerated, not stored.

use os_asr::sherpa_onnx::LinearResampler;
use serde::Deserialize;

pub const RATE: u32 = 16_000;

#[derive(Debug, Clone, Deserialize)]
pub struct PhoneSpec {
    /// The script speaker on the phone (usually `client`).
    pub speaker: String,
    /// The phone voice's level relative to the vet's, in dB (negative: quieter).
    #[serde(default = "default_level")]
    pub level_db: f32,
    /// Background noise relative to the vet's level, in dB.
    #[serde(default = "default_noise")]
    pub noise_db: f32,
    #[serde(default)]
    pub seed: u64,
}

fn default_level() -> f32 {
    -20.0
}

fn default_noise() -> f32 {
    -45.0
}

/// A deterministic xorshift generator (no dependency for a test fixture).
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    /// Uniform in [-1, 1).
    pub fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        ((self.0 >> 40) as f32 / (1u64 << 23) as f32) - 1.0
    }
}

/// An RBJ-cookbook biquad.
#[derive(Clone)]
struct Biquad {
    b: [f32; 3],
    a: [f32; 2],
    z: [f32; 2],
}

impl Biquad {
    fn new(freq: f32, q: f32, high: bool) -> Self {
        let w = 2.0 * std::f32::consts::PI * freq / RATE as f32;
        let (sin, cos) = w.sin_cos();
        let alpha = sin / (2.0 * q);
        let a0 = 1.0 + alpha;
        let b = if high {
            [(1.0 + cos) / 2.0, -(1.0 + cos), (1.0 + cos) / 2.0]
        } else {
            [(1.0 - cos) / 2.0, 1.0 - cos, (1.0 - cos) / 2.0]
        };
        Self { b: b.map(|x| x / a0), a: [-2.0 * cos / a0, (1.0 - alpha) / a0], z: [0.0; 2] }
    }

    fn run(&mut self, x: f32) -> f32 {
        let y = self.b[0] * x + self.z[0];
        self.z[0] = self.b[1] * x - self.a[0] * y + self.z[1];
        self.z[1] = self.b[2] * x - self.a[1] * y;
        y
    }
}

/// 4th-order Butterworth band-pass as two high-pass and two low-pass sections.
fn telephone_band(samples: &[f32]) -> Vec<f32> {
    let qs = [0.541_196_1, 1.306_563];
    let mut filters: Vec<Biquad> = qs
        .iter()
        .map(|q| Biquad::new(300.0, *q, true))
        .chain(qs.iter().map(|q| Biquad::new(3400.0, *q, false)))
        .collect();
    samples.iter().map(|&x| filters.iter_mut().fold(x, |v, f| f.run(v))).collect()
}

/// G.711 mu-law: compand, keep sign plus 7 bits, expand.
fn mu_law(x: f32) -> f32 {
    const MU: f32 = 255.0;
    let x = x.clamp(-1.0, 1.0);
    let y = x.signum() * (1.0 + MU * x.abs()).ln() / (1.0 + MU).ln();
    let q = (y * 127.0).round() / 127.0;
    q.signum() * ((1.0 + MU).powf(q.abs()) - 1.0) / MU
}

fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
}

/// RMS over every line of one speaker.
pub fn rms_of(lines: &[&[f32]]) -> f32 {
    let (sum, n) =
        lines.iter().fold((0.0f64, 0usize), |(s, n), l| (s + l.iter().map(|x| (x * x) as f64).sum::<f64>(), n + l.len()));
    if n == 0 { 0.0 } else { (sum / n as f64).sqrt() as f32 }
}

/// The phone line and the phone's loudspeaker (steps 1 to 3), at 16 kHz.
pub fn phone_line(samples: &[f32]) -> Vec<f32> {
    let band = telephone_band(samples);
    // The line runs at 8 kHz; the band-pass above is the anti-alias filter.
    let down = LinearResampler::create(RATE as i32, 8_000).expect("resampler");
    let up = LinearResampler::create(8_000, RATE as i32).expect("resampler");
    let peak = band.iter().fold(0.0f32, |m, x| m.max(x.abs())).max(1e-6);
    // Speech at a typical line level (peaks around -6 dBFS) before companding.
    let narrow: Vec<f32> = down.resample(&band, true).iter().map(|x| mu_law(x * 0.5 / peak)).collect();
    let wide = up.resample(&narrow, true);
    // The phone's level control: 3:1 above -24 dBFS, 3 ms attack, 120 ms release.
    let (attack, release) = ((-1.0 / (0.003 * RATE as f32)).exp(), (-1.0 / (0.12 * RATE as f32)).exp());
    let threshold = 10f32.powf(-24.0 / 20.0);
    let mut env = 0.0f32;
    wide.iter()
        .map(|&x| {
            let a = x.abs();
            env = if a > env { attack * env + (1.0 - attack) * a } else { release * env + (1.0 - release) * a };
            let gain = if env > threshold { (threshold / env).powf(1.0 - 1.0 / 3.0) } else { 1.0 };
            // A small loudspeaker driven hard: soft clipping.
            let drive = 2.0;
            (drive * x * gain * 2.0).tanh() / drive.tanh()
        })
        .collect()
}

/// A Schroeder reverberator: four damped combs in parallel, two all-passes in
/// series. Returns the wet signal only, `tail_secs` longer than the input.
pub fn reverb(samples: &[f32], rt60: f32, seed: u64, tail_secs: f32) -> Vec<f32> {
    let mut rng = Rng::new(seed ^ 0x5EED);
    // Freeverb's tunings, scaled to 16 kHz, jittered a little per room.
    let combs: Vec<usize> = [565.0f32, 587.0, 541.0, 516.0]
        .iter()
        .map(|d| (d * (1.0 + 0.08 * rng.next())) as usize)
        .collect();
    let allpasses = [82usize, 202];
    let predelay = (0.012 * RATE as f32) as usize;
    let n = samples.len() + (tail_secs * RATE as f32) as usize;
    let input = |i: usize| if i >= predelay { samples.get(i - predelay).copied().unwrap_or(0.0) } else { 0.0 };
    let mut out = vec![0.0f32; n];
    for &d in &combs {
        let g = 10f32.powf(-3.0 * d as f32 / (rt60 * RATE as f32));
        let mut buf = vec![0.0f32; d];
        let mut lp = 0.0f32;
        for (i, o) in out.iter_mut().enumerate() {
            let y = buf[i % d];
            lp = 0.7 * y + 0.3 * lp; // high frequencies die away faster
            buf[i % d] = input(i) + g * lp;
            *o += y / combs.len() as f32;
        }
    }
    for &d in &allpasses {
        let mut buf = vec![0.0f32; d];
        for (i, o) in out.iter_mut().enumerate() {
            let b = buf[i % d];
            let y = -0.5 * *o + b;
            buf[i % d] = *o + 0.5 * y;
            *o = y;
        }
    }
    out
}

/// Direct sound plus reverb at a direct-to-reverberant ratio (dB), keeping
/// the direct sound's level. Longer than the input by the reverb tail.
pub fn in_room(samples: &[f32], drr_db: f32, seed: u64) -> Vec<f32> {
    let tail = 0.4;
    let wet = reverb(samples, 0.5, seed, tail);
    let (dry_rms, wet_rms) = (rms(samples), rms(&wet[..samples.len()]).max(1e-9));
    let wet_gain = dry_rms / wet_rms * 10f32.powf(-drr_db / 20.0);
    let mut out = wet.iter().map(|w| w * wet_gain).collect::<Vec<_>>();
    for (o, s) in out.iter_mut().zip(samples) {
        *o += s;
    }
    out
}

/// Background noise: mostly low-passed (pinkish) with a little hiss, at `rms`.
pub fn room_noise(n: usize, level: f32, seed: u64) -> Vec<f32> {
    let mut rng = Rng::new(seed ^ 0xA0A0);
    let mut lp = 0.0f32;
    let raw: Vec<f32> = (0..n)
        .map(|_| {
            let w = rng.next();
            lp = 0.97 * lp + 0.03 * w;
            lp * 4.0 + 0.1 * w
        })
        .collect();
    let gain = level / rms(&raw).max(1e-9);
    raw.into_iter().map(|x| x * gain).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(freq: f32, secs: f32, amp: f32) -> Vec<f32> {
        (0..(secs * RATE as f32) as usize)
            .map(|i| amp * (2.0 * std::f32::consts::PI * freq * i as f32 / RATE as f32).sin())
            .collect()
    }

    fn db(x: f32) -> f32 {
        20.0 * x.max(1e-9).log10()
    }

    #[test]
    fn the_telephone_band_passes_speech_and_cuts_highs_and_lows() {
        let mid = rms(&telephone_band(&tone(1000.0, 1.0, 0.3))[4000..]);
        let high = rms(&telephone_band(&tone(6000.0, 1.0, 0.3))[4000..]);
        let low = rms(&telephone_band(&tone(80.0, 1.0, 0.3))[4000..]);
        assert!((db(mid) - db(0.3 / 2f32.sqrt())).abs() < 1.0, "1 kHz passes: {}", db(mid));
        assert!(db(mid) - db(high) > 30.0, "6 kHz is cut: {} vs {}", db(mid), db(high));
        assert!(db(mid) - db(low) > 30.0, "80 Hz is cut: {} vs {}", db(mid), db(low));
    }

    #[test]
    fn the_phone_line_has_no_energy_above_4_khz() {
        // A 1 kHz tone plus a 5 kHz tone: after the line, 5 kHz is gone.
        let mix: Vec<f32> =
            tone(1000.0, 1.0, 0.2).iter().zip(tone(5000.0, 1.0, 0.2)).map(|(a, b)| a + b).collect();
        let out = phone_line(&mix);
        // Correlate with each tone to measure what's left of it.
        let amp = |f: f32| {
            let (mut s, mut c) = (0.0f32, 0.0f32);
            for (i, x) in out.iter().enumerate().skip(4000) {
                let w = 2.0 * std::f32::consts::PI * f * i as f32 / RATE as f32;
                s += x * w.sin();
                c += x * w.cos();
            }
            (s * s + c * c).sqrt()
        };
        assert!(db(amp(1000.0)) - db(amp(5000.0)) > 30.0);
    }

    #[test]
    fn mu_law_keeps_quiet_and_loud_samples_close() {
        for x in [0.001f32, 0.01, 0.1, 0.5, -0.3] {
            assert!((mu_law(x) - x).abs() <= x.abs() * 0.07 + 0.0005, "{x} -> {}", mu_law(x));
        }
    }

    #[test]
    fn the_room_adds_a_decaying_tail_and_is_deterministic() {
        let click: Vec<f32> = (0..1600).map(|i| if i < 160 { 0.5 } else { 0.0 }).collect();
        let a = in_room(&click, 0.0, 7);
        let b = in_room(&click, 0.0, 7);
        assert_eq!(a, b);
        assert!(a.len() > click.len());
        let early = rms(&a[1600..3200]);
        let late = rms(&a[a.len() - 1600..]);
        assert!(early > late * 2.0, "the tail decays: {early} then {late}");
    }

    #[test]
    fn noise_has_the_level_asked_for() {
        let n = room_noise(32_000, 0.01, 3);
        assert!((rms(&n) - 0.01).abs() < 1e-4);
    }
}
