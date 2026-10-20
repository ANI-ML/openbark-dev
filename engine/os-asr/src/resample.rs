//! Linear resampling to the models' 16 kHz.
//!
//! With `stt` this is sherpa-onnx's `LinearResampler`, exactly as before.
//! Without it (a build with no native speech code) a linear interpolator in
//! Rust does the same job, keeping its place across blocks for live audio.

#[cfg(feature = "stt")]
pub struct Resampler(sherpa_onnx::LinearResampler);

#[cfg(feature = "stt")]
impl Resampler {
    pub fn create(from: i32, to: i32) -> Option<Self> {
        sherpa_onnx::LinearResampler::create(from, to).map(Self)
    }

    /// Resample one block. `flush` ends the stream.
    pub fn resample(&self, samples: &[f32], flush: bool) -> Vec<f32> {
        self.0.resample(samples, flush)
    }
}

#[cfg(not(feature = "stt"))]
pub struct Resampler {
    from: f64,
    to: f64,
    state: std::sync::Mutex<State>,
}

#[cfg(not(feature = "stt"))]
#[derive(Default)]
struct State {
    /// The last input sample of the previous block.
    last: Option<f32>,
    /// Position of the next output sample, in input samples, measured from
    /// `last` (0.0 is `last` itself).
    pos: f64,
}

#[cfg(not(feature = "stt"))]
impl Resampler {
    pub fn create(from: i32, to: i32) -> Option<Self> {
        (from > 0 && to > 0).then(|| Self {
            from: from as f64,
            to: to as f64,
            state: std::sync::Mutex::new(State { last: None, pos: 1.0 }),
        })
    }

    /// Resample one block. `flush` ends the stream.
    pub fn resample(&self, samples: &[f32], flush: bool) -> Vec<f32> {
        let step = self.from / self.to;
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        // Input with the previous block's last sample in front, at index 0.
        let prev = state.last;
        let at = |i: usize| -> f32 {
            match prev {
                Some(p) if i == 0 => p,
                Some(_) => samples[i - 1],
                None => samples[i],
            }
        };
        let len = samples.len() + usize::from(prev.is_some());
        let mut out = Vec::with_capacity((samples.len() as f64 / step) as usize + 2);
        let mut pos = if prev.is_some() { state.pos } else { state.pos - 1.0 };
        while len > 0 && pos <= (len - 1) as f64 {
            let i = pos.floor() as usize;
            let frac = (pos - i as f64) as f32;
            let a = at(i);
            let b = if i + 1 < len { at(i + 1) } else { a };
            out.push(a + (b - a) * frac);
            pos += step;
        }
        if flush {
            *state = State { last: None, pos: 1.0 };
        } else if len > 0 {
            state.last = Some(at(len - 1));
            state.pos = pos - (len - 1) as f64;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::Resampler;

    #[test]
    fn halves_and_doubles() {
        let r = Resampler::create(32_000, 16_000).unwrap();
        let input: Vec<f32> = (0..3200).map(|i| (i as f32 * 0.01).sin()).collect();
        let out = r.resample(&input, true);
        assert!((out.len() as i64 - 1600).abs() <= 2, "{}", out.len());
        let up = Resampler::create(8_000, 16_000).unwrap().resample(&input[..800], true);
        assert!((up.len() as i64 - 1600).abs() <= 2, "{}", up.len());
    }

    #[test]
    fn blocks_join_without_gaps() {
        // Rust path only: sherpa-onnx keeps its own filter state.
        if cfg!(feature = "stt") {
            return;
        }
        let input: Vec<f32> = (0..4800).map(|i| i as f32).collect();
        let whole = Resampler::create(48_000, 16_000).unwrap().resample(&input, false);
        let r = Resampler::create(48_000, 16_000).unwrap();
        let mut pieces = Vec::new();
        for block in input.chunks(441) {
            pieces.extend(r.resample(block, false));
        }
        assert_eq!(pieces.len(), whole.len());
        for (a, b) in pieces.iter().zip(&whole) {
            assert!((a - b).abs() < 1e-3, "{a} {b}");
        }
    }
}
