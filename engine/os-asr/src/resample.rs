//! Streaming resampling to the recogniser's 16 kHz, for live audio whose
//! rate is whatever the microphone (or browser) runs at, and which can change
//! mid-recording when the device does (a Bluetooth headset dropping to its
//! 16 kHz or 8 kHz hands-free mode, or a switch to another microphone).

use crate::SAMPLE_RATE;

/// The lowest and highest input rates accepted. Telephone audio is 8 kHz;
/// nothing real captures below that or above 384 kHz.
pub const MIN_RATE: u32 = 8_000;
pub const MAX_RATE: u32 = 384_000;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("cannot resample from {0} Hz")]
pub struct RateError(pub u32);

/// Resamples blocks to [`SAMPLE_RATE`], keeping the filter state between
/// blocks of the same rate. A block at a new rate starts a new filter.
#[derive(Default)]
pub struct StreamResampler {
    current: Option<(u32, sherpa_onnx::LinearResampler)>,
}

impl StreamResampler {
    pub fn new() -> Self {
        Self::default()
    }

    /// `samples` at `rate`, as 16 kHz samples. 16 kHz input passes through.
    pub fn process(&mut self, samples: Vec<f32>, rate: u32) -> Result<Vec<f32>, RateError> {
        if rate == SAMPLE_RATE {
            // A switch back to 16 kHz drops the old filter's few buffered samples.
            self.current = None;
            return Ok(samples);
        }
        if !(MIN_RATE..=MAX_RATE).contains(&rate) {
            return Err(RateError(rate));
        }
        if self.current.as_ref().map(|(r, _)| *r) != Some(rate) {
            let r = sherpa_onnx::LinearResampler::create(rate as i32, SAMPLE_RATE as i32)
                .ok_or(RateError(rate))?;
            self.current = Some((rate, r));
        }
        Ok(self.current.as_ref().expect("set above").1.resample(&samples, false))
    }

    /// The rate of the last block resampled (`None` at 16 kHz or before any).
    pub fn rate(&self) -> Option<u32> {
        self.current.as_ref().map(|(r, _)| *r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(rate: u32, hz: f32, secs: f32, amp: f32) -> Vec<f32> {
        let n = (rate as f32 * secs) as usize;
        (0..n).map(|i| amp * (2.0 * std::f32::consts::PI * hz * i as f32 / rate as f32).sin()).collect()
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
    }

    /// Frequency from upward zero crossings, skipping the filter's start-up.
    fn frequency(x: &[f32], rate: u32) -> f32 {
        let x = &x[x.len() / 10..];
        let ups = x.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count();
        ups as f32 * rate as f32 / x.len() as f32
    }

    /// Streams `input` through in blocks of `block` samples.
    fn stream(r: &mut StreamResampler, input: &[f32], rate: u32, block: usize) -> Vec<f32> {
        let mut out = Vec::new();
        for chunk in input.chunks(block) {
            out.extend(r.process(chunk.to_vec(), rate).unwrap());
        }
        out
    }

    #[test]
    fn every_common_microphone_rate_comes_out_at_16k_with_pitch_and_level_kept() {
        // 8 and 11.025 kHz: Bluetooth hands-free and old USB headsets; 16 kHz:
        // headsets and conference mics; 22.05 to 96 kHz: built-in and USB mics.
        for rate in [8_000u32, 11_025, 16_000, 22_050, 24_000, 32_000, 44_100, 48_000, 88_200, 96_000] {
            // A tone every rate can carry (below 4 kHz, the 8 kHz Nyquist limit).
            let input = tone(rate, 1_000.0, 2.0, 0.5);
            let mut r = StreamResampler::new();
            // 10 ms blocks, as a sound card delivers them.
            let out = stream(&mut r, &input, rate, (rate / 100) as usize);
            let expected = 2.0 * SAMPLE_RATE as f32;
            assert!(
                (out.len() as f32 - expected).abs() < 64.0,
                "{rate} Hz: {} samples out, expected about {expected}",
                out.len()
            );
            let f = frequency(&out, SAMPLE_RATE);
            assert!((f - 1_000.0).abs() < 10.0, "{rate} Hz: pitch came out at {f} Hz");
            let level = rms(&out[out.len() / 10..]);
            assert!((level - 0.5 / 2f32.sqrt()).abs() < 0.03, "{rate} Hz: level {level}");
        }
    }

    #[test]
    fn content_above_8k_is_filtered_out_not_folded_back() {
        // A 12 kHz tone at 48 kHz can't exist at 16 kHz: it must vanish, not
        // alias down to 4 kHz and garble speech.
        let input = tone(48_000, 12_000.0, 1.0, 0.5);
        let out = stream(&mut StreamResampler::new(), &input, 48_000, 480);
        assert!(rms(&out[out.len() / 10..]) < 0.02, "aliasing: rms {}", rms(&out));
    }

    #[test]
    fn block_size_does_not_change_the_result() {
        let input = tone(44_100, 440.0, 1.0, 0.3);
        let whole = stream(&mut StreamResampler::new(), &input, 44_100, input.len());
        let small = stream(&mut StreamResampler::new(), &input, 44_100, 128);
        let odd = stream(&mut StreamResampler::new(), &input, 44_100, 1_003);
        assert_eq!(whole.len(), small.len());
        assert_eq!(whole.len(), odd.len());
        for (a, b) in whole.iter().zip(&small) {
            assert!((a - b).abs() < 1e-4);
        }
    }

    #[test]
    fn a_device_switch_mid_recording_changes_rate_without_a_gap() {
        // 48 kHz built-in mic, then a headset at 16 kHz, then its 8 kHz mode.
        let mut r = StreamResampler::new();
        let a = stream(&mut r, &tone(48_000, 500.0, 1.0, 0.4), 48_000, 480);
        assert_eq!(r.rate(), Some(48_000));
        let b = stream(&mut r, &tone(16_000, 500.0, 1.0, 0.4), 16_000, 160);
        assert_eq!(r.rate(), None);
        let c = stream(&mut r, &tone(8_000, 500.0, 1.0, 0.4), 8_000, 80);
        assert_eq!(r.rate(), Some(8_000));
        for (name, part) in [("48k", &a), ("16k", &b), ("8k", &c)] {
            assert!((part.len() as i64 - 16_000).abs() < 64, "{name}: {} samples", part.len());
        }
    }

    #[test]
    fn long_recordings_do_not_drift() {
        // Ten minutes at 44.1 kHz in 20 ms blocks: the 16 kHz output stays
        // within a few milliseconds of real time, so segment times stay true.
        let mut r = StreamResampler::new();
        let block = vec![0.01f32; 882];
        let mut out = 0usize;
        for _ in 0..(10 * 60 * 50) {
            out += r.process(block.clone(), 44_100).unwrap().len();
        }
        let expected = 10 * 60 * 16_000;
        assert!((out as i64 - expected as i64).abs() < 64, "{out} vs {expected}");
    }

    #[test]
    fn impossible_rates_are_refused() {
        let mut r = StreamResampler::new();
        assert_eq!(r.process(vec![0.0; 10], 0), Err(RateError(0)));
        assert_eq!(r.process(vec![0.0; 10], 4_000), Err(RateError(4_000)));
        assert_eq!(r.process(vec![0.0; 10], 1_000_000), Err(RateError(1_000_000)));
    }
}
