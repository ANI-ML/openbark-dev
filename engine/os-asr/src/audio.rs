use std::path::Path;

/// Sample rate every OpenBark audio model consumes.
pub const SAMPLE_RATE: u32 = 16_000;

#[derive(Debug, thiserror::Error)]
pub enum AudioError {
    #[error("cannot read audio file {0}")]
    Read(String),
    #[error("cannot decode {0}: {1}")]
    Decode(String, String),
    /// A container or codec no decoder here reads (WMA, AMR, DSS…).
    #[error(
        "OpenBark can't read {0}: it isn't a kind of recording OpenBark supports, or the file is damaged. Recordings in WAV, MP3, M4A, MP4, MOV, AAC, FLAC, Ogg, Opus, WebM, MKV, AIFF or CAF work; save or convert it to one of these first."
    )]
    Unsupported(String),
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

    /// Read a WAV file and resample to 16 kHz if needed. Without `stt`
    /// (no sherpa-onnx) WAV files are decoded like any other recording.
    #[cfg(not(feature = "stt"))]
    pub fn load_wav(path: &Path) -> Result<Self, AudioError> {
        Self::decode(path)
    }

    /// Read a WAV file and resample to 16 kHz if needed.
    #[cfg(feature = "stt")]
    pub fn load_wav(path: &Path) -> Result<Self, AudioError> {
        let display = path.display().to_string();
        let wave = sherpa_onnx::Wave::read(path.to_str().ok_or_else(|| AudioError::Read(display.clone()))?)
            .ok_or(AudioError::Read(display))?;
        let rate = wave.sample_rate();
        let samples = if rate == SAMPLE_RATE as i32 {
            wave.samples().to_vec()
        } else {
            let resampler =
                crate::Resampler::create(rate, SAMPLE_RATE as i32).ok_or(AudioError::Resample(rate))?;
            resampler.resample(wave.samples(), true)
        };
        Ok(Self { samples })
    }

    /// Read any supported recording, mix it down to mono and resample to
    /// 16 kHz: WAV (PCM or ADPCM), AIFF, MP3 and MPEG layers 1 and 2, M4A,
    /// MP4 and MOV (AAC, ALAC), FLAC, Ogg (Vorbis, Opus), Opus, WebM and
    /// MKV (Opus, Vorbis) and CAF: what phones, dictaphones, browsers and
    /// call apps produce. Video files work too; their audio track is used.
    pub fn load(path: &Path) -> Result<Self, AudioError> {
        let is_wav = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("wav"));
        if is_wav && let Ok(audio) = Self::load_wav(path) {
            return Ok(audio);
        }
        Self::decode(path)
    }

    fn decode(path: &Path) -> Result<Self, AudioError> {
        let display = path.display().to_string();
        let file =
            std::fs::File::open(path).map_err(|e| AudioError::Decode(display.clone(), e.to_string()))?;
        Self::decode_source(Box::new(file), path.extension().and_then(|e| e.to_str()), display)
    }

    /// Decode a recording held in memory (an upload, or a decrypted file).
    /// `name` gives the format hint by its extension.
    pub fn decode_bytes(bytes: Vec<u8>, name: &str) -> Result<Self, AudioError> {
        let ext = Path::new(name).extension().and_then(|e| e.to_str()).map(str::to_string);
        Self::decode_source(Box::new(std::io::Cursor::new(bytes)), ext.as_deref(), name.to_string())
    }

    fn decode_source(
        source: Box<dyn symphonia::core::io::MediaSource>,
        ext: Option<&str>,
        display: String,
    ) -> Result<Self, AudioError> {
        use symphonia::core::codecs::audio::AudioDecoderOptions;
        use symphonia::core::errors::Error;
        use symphonia::core::formats::probe::Hint;
        use symphonia::core::formats::{FormatOptions, TrackType};
        use symphonia::core::io::MediaSourceStream;
        use symphonia::core::meta::MetadataOptions;

        let fail = |e: &dyn std::fmt::Display| AudioError::Decode(display.clone(), e.to_string());
        let file_name = Path::new(&display)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| display.clone());
        let unsupported = || AudioError::Unsupported(file_name.clone());
        let mss = MediaSourceStream::new(source, Default::default());
        let mut hint = Hint::new();
        if let Some(ext) = ext {
            hint.with_extension(ext);
        }
        let mut format = symphonia::default::get_probe()
            .probe(&hint, mss, FormatOptions::default(), MetadataOptions::default())
            .map_err(|_| unsupported())?;
        let track = format.default_track(TrackType::Audio).ok_or_else(|| fail(&"no audio track"))?;
        let params =
            track.codec_params.as_ref().and_then(|p| p.audio()).ok_or_else(|| fail(&"no audio track"))?;
        if params.codec == symphonia::core::codecs::audio::well_known::CODEC_ID_OPUS {
            // The encoder's lead-in, from the OpusHead header (counted at 48 kHz).
            let pre_skip = params
                .extra_data
                .as_deref()
                .filter(|h| h.len() >= 12 && h.starts_with(b"OpusHead"))
                .map(|h| u16::from_le_bytes([h[10], h[11]]) as usize)
                .unwrap_or(0);
            let track_id = track.id;
            return decode_opus(format, track_id, pre_skip, &fail);
        }
        let mut decoder = symphonia::default::get_codecs()
            .make_audio_decoder(params, &AudioDecoderOptions::default())
            .map_err(|_| unsupported())?;
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

/// Opus, decoded straight to 16 kHz mono (Opus decodes at any of its
/// rates and mixes stereo down itself, so nothing is resampled).
fn decode_opus(
    mut format: Box<dyn symphonia::core::formats::FormatReader>,
    track_id: u32,
    pre_skip_48k: usize,
    fail: &dyn Fn(&dyn std::fmt::Display) -> AudioError,
) -> Result<Audio, AudioError> {
    use symphonia::core::errors::Error;
    let mut decoder = opus_decoder::OpusDecoder::new(SAMPLE_RATE, 1).map_err(|e| fail(&e))?;
    // The longest Opus packet is 120 ms.
    let mut pcm = vec![0f32; (SAMPLE_RATE as usize * 120) / 1000];
    let mut samples: Vec<f32> = Vec::new();
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
        match decoder.decode_float(&packet.data, &mut pcm, false) {
            Ok(n) => samples.extend_from_slice(&pcm[..n.min(pcm.len())]),
            Err(_) => continue, // skip a damaged packet
        }
    }
    let skip = (pre_skip_48k * SAMPLE_RATE as usize / 48_000).min(samples.len());
    samples.drain(..skip);
    if samples.is_empty() {
        return Err(fail(&"no audio in the file"));
    }
    Ok(Audio { samples })
}

/// Resample mono audio to [`SAMPLE_RATE`].
fn resample(samples: Vec<f32>, rate: u32) -> Result<Vec<f32>, AudioError> {
    if rate == SAMPLE_RATE {
        return Ok(samples);
    }
    let resampler =
        crate::Resampler::create(rate as i32, SAMPLE_RATE as i32).ok_or(AudioError::Resample(rate as i32))?;
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
    fn an_unsupported_recording_says_which_formats_work() {
        let err = Audio::decode_bytes(b"MAC not really a recording".to_vec(), "dictation.wma").unwrap_err();
        let msg = err.to_string();
        assert!(msg.starts_with("OpenBark can't read dictation.wma"), "{msg}");
        assert!(msg.contains("WebM") && msg.contains("Opus"), "{msg}");
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
        assert!(matches!(Audio::load(&path), Err(AudioError::Unsupported(name)) if name == "notes.m4a"));
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
