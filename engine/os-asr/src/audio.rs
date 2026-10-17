use std::path::Path;

/// Sample rate every OpenBark audio model consumes.
pub const SAMPLE_RATE: u32 = 16_000;

#[derive(Debug, thiserror::Error)]
pub enum AudioError {
    #[error("cannot read audio file {0} (M0 supports mono WAV only)")]
    Read(String),
    #[error("cannot resample from {0} Hz")]
    Resample(i32),
}

/// Mono 16 kHz PCM in [-1, 1].
#[derive(Debug, Clone)]
pub struct Audio {
    pub samples: Vec<f32>,
}

impl Audio {
    pub fn from_samples(samples: Vec<f32>) -> Self {
        Self { samples }
    }

    /// Read a WAV file and resample to 16 kHz if needed.
    pub fn load_wav(path: &Path) -> Result<Self, AudioError> {
        let display = path.display().to_string();
        let wave = sherpa_onnx::Wave::read(path.to_str().ok_or_else(|| AudioError::Read(display.clone()))?)
            .ok_or(AudioError::Read(display))?;
        let rate = wave.sample_rate();
        let samples = if rate == SAMPLE_RATE as i32 {
            wave.samples().to_vec()
        } else {
            let resampler = sherpa_onnx::LinearResampler::create(rate, SAMPLE_RATE as i32)
                .ok_or(AudioError::Resample(rate))?;
            resampler.resample(wave.samples(), true)
        };
        Ok(Self { samples })
    }

    pub fn duration_secs(&self) -> f32 {
        self.samples.len() as f32 / SAMPLE_RATE as f32
    }

    /// Samples between two times (seconds), clamped to the clip.
    pub fn slice(&self, start: f32, end: f32) -> &[f32] {
        let to_idx = |t: f32| ((t.max(0.0) * SAMPLE_RATE as f32) as usize).min(self.samples.len());
        let (a, b) = (to_idx(start), to_idx(end));
        &self.samples[a..b.max(a)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slice_clamps_to_bounds() {
        let audio = Audio::from_samples(vec![0.0; SAMPLE_RATE as usize]);
        assert_eq!(audio.slice(0.5, 2.0).len(), 8_000);
        assert_eq!(audio.slice(-1.0, 0.25).len(), 4_000);
        assert!(audio.slice(0.9, 0.1).is_empty());
        assert!((audio.duration_secs() - 1.0).abs() < f32::EPSILON);
    }
}
