use std::path::Path;

/// Sample rate every OpenBark audio model consumes.
pub const SAMPLE_RATE: u32 = 16_000;

#[derive(Debug, thiserror::Error)]
pub enum AudioError {
    #[error("cannot read audio file {0}")]
    Read(String),
    #[error("cannot decode {0}: {1}")]
    Decode(String, String),
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

    /// Read any supported recording (WAV, m4a with AAC or ALAC, mp3, CAF,
    /// FLAC, Ogg Vorbis: what phones and recorders produce), mix it down to
    /// mono and resample to 16 kHz.
    pub fn load(path: &Path) -> Result<Self, AudioError> {
        let is_wav = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("wav"));
        if is_wav && let Ok(audio) = Self::load_wav(path) {
            return Ok(audio);
        }
        Self::decode(path)
    }

    fn decode(path: &Path) -> Result<Self, AudioError> {
        use symphonia::core::codecs::audio::AudioDecoderOptions;
        use symphonia::core::errors::Error;
        use symphonia::core::formats::probe::Hint;
        use symphonia::core::formats::{FormatOptions, TrackType};
        use symphonia::core::io::MediaSourceStream;
        use symphonia::core::meta::MetadataOptions;

        let display = path.display().to_string();
        let fail = |e: &dyn std::fmt::Display| AudioError::Decode(display.clone(), e.to_string());
        let file = std::fs::File::open(path).map_err(|e| fail(&e))?;
        let mss = MediaSourceStream::new(Box::new(file), Default::default());
        let mut hint = Hint::new();
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            hint.with_extension(ext);
        }
        let mut format = symphonia::default::get_probe()
            .probe(&hint, mss, FormatOptions::default(), MetadataOptions::default())
            .map_err(|e| fail(&e))?;
        let track = format.default_track(TrackType::Audio).ok_or_else(|| fail(&"no audio track"))?;
        let params =
            track.codec_params.as_ref().and_then(|p| p.audio()).ok_or_else(|| fail(&"no audio track"))?;
        let mut decoder = symphonia::default::get_codecs()
            .make_audio_decoder(params, &AudioDecoderOptions::default())
            .map_err(|e| fail(&e))?;
        let track_id = track.id;

        let mut mono: Vec<f32> = Vec::new();
        let mut rate = 0u32;
        let mut interleaved: Vec<f32> = Vec::new();
        loop {
            let packet = match format.next_packet() {
                Ok(Some(p)) => p,
                Ok(None) => break,
                Err(Error::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                Err(e) => return Err(fail(&e)),
            };
            if packet.track_id != track_id {
                continue;
            }
            let buf = match decoder.decode(&packet) {
                Ok(buf) => buf,
                Err(Error::DecodeError(_)) => continue, // skip a damaged packet
                Err(e) => return Err(fail(&e)),
            };
            rate = buf.spec().rate();
            let channels = buf.spec().channels().count().max(1);
            interleaved.resize(buf.samples_interleaved(), 0.0);
            buf.copy_to_slice_interleaved(&mut interleaved);
            mono.extend(
                interleaved.chunks(channels).map(|frame| frame.iter().sum::<f32>() / channels as f32),
            );
        }
        if mono.is_empty() || rate == 0 {
            return Err(fail(&"no audio in the file"));
        }
        Ok(Self { samples: resample(mono, rate)? })
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

/// Resample mono audio to [`SAMPLE_RATE`].
fn resample(samples: Vec<f32>, rate: u32) -> Result<Vec<f32>, AudioError> {
    if rate == SAMPLE_RATE {
        return Ok(samples);
    }
    let resampler = sherpa_onnx::LinearResampler::create(rate as i32, SAMPLE_RATE as i32)
        .ok_or(AudioError::Resample(rate as i32))?;
    Ok(resampler.resample(&samples, true))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 16-bit PCM WAV, written by hand so the decoder path can be tested
    /// without fixture files.
    fn wav_bytes(rate: u32, channels: u16, frames: &[Vec<i16>]) -> Vec<u8> {
        let data: Vec<u8> = frames.iter().flatten().flat_map(|s| s.to_le_bytes()).collect();
        let mut out = Vec::new();
        out.extend(b"RIFF");
        out.extend((36 + data.len() as u32).to_le_bytes());
        out.extend(b"WAVEfmt ");
        out.extend(16u32.to_le_bytes());
        out.extend(1u16.to_le_bytes());
        out.extend(channels.to_le_bytes());
        out.extend(rate.to_le_bytes());
        out.extend((rate * channels as u32 * 2).to_le_bytes());
        out.extend((channels * 2).to_le_bytes());
        out.extend(16u16.to_le_bytes());
        out.extend(b"data");
        out.extend((data.len() as u32).to_le_bytes());
        out.extend(data);
        out
    }

    #[test]
    fn decoder_mixes_stereo_down_and_resamples() {
        let dir = tempfile::tempdir().unwrap();
        // One second of stereo at 48 kHz: left at half scale, right silent.
        let frames: Vec<Vec<i16>> = (0..48_000).map(|_| vec![16_384, 0]).collect();
        let path = dir.path().join("call.audio"); // not .wav: forces the general decoder
        std::fs::write(&path, wav_bytes(48_000, 2, &frames)).unwrap();
        let audio = Audio::load(&path).unwrap();
        assert!((audio.duration_secs() - 1.0).abs() < 0.01, "{}", audio.duration_secs());
        let mid = audio.samples[8_000];
        assert!((mid - 0.25).abs() < 0.01, "mono is the mean of both channels: {mid}");
    }

    #[test]
    fn unreadable_files_say_so() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("notes.m4a");
        std::fs::write(&path, b"not audio at all").unwrap();
        assert!(matches!(Audio::load(&path), Err(AudioError::Decode(..))));
    }

    #[test]
    fn slice_clamps_to_bounds() {
        let audio = Audio::from_samples(vec![0.0; SAMPLE_RATE as usize]);
        assert_eq!(audio.slice(0.5, 2.0).len(), 8_000);
        assert_eq!(audio.slice(-1.0, 0.25).len(), 4_000);
        assert!(audio.slice(0.9, 0.1).is_empty());
        assert!((audio.duration_secs() - 1.0).abs() < f32::EPSILON);
    }
}
