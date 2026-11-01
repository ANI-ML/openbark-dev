#[cfg(feature = "stt")]
use sherpa_onnx::{
    OfflineCanaryModelConfig, OfflineModelConfig, OfflineRecognizer, OfflineRecognizerConfig,
    OfflineTransducerModelConfig, OfflineWhisperModelConfig,
};

#[cfg(feature = "stt")]
use crate::{AsrBackend, AsrCapabilities, SAMPLE_RATE, require_file};
use crate::{AsrError, default_threads};

/// Offline (non-streaming) ASR model families wired up in M0.
#[derive(Debug, Clone)]
pub enum AsrModel {
    /// NVIDIA NeMo transducers, e.g. Parakeet TDT 0.6B v3.
    NemoTransducer { encoder: String, decoder: String, joiner: String, tokens: String },
    /// OpenAI Whisper ONNX exports (sherpa-onnx layout).
    Whisper { encoder: String, decoder: String, tokens: String, language: Option<String> },
    /// NVIDIA NeMo Canary (180M Flash: English, Spanish, German, French),
    /// told the language spoken; it can't detect one itself (sherpa-onnx
    /// falls back to English). It reports no token times.
    Canary { encoder: String, decoder: String, tokens: String, language: Option<String> },
}

impl AsrModel {
    #[cfg_attr(not(feature = "stt"), allow(dead_code))]
    pub(crate) fn files(&self) -> Vec<&str> {
        match self {
            Self::NemoTransducer { encoder, decoder, joiner, tokens } => {
                vec![encoder, decoder, joiner, tokens]
            }
            Self::Whisper { encoder, decoder, tokens, .. }
            | Self::Canary { encoder, decoder, tokens, .. } => {
                vec![encoder, decoder, tokens]
            }
        }
    }

    #[cfg_attr(not(feature = "stt"), allow(dead_code))]
    fn label(&self) -> &'static str {
        match self {
            Self::NemoTransducer { .. } => "nemo-transducer",
            Self::Whisper { .. } => "whisper",
            Self::Canary { .. } => "canary",
        }
    }
}

/// What the recogniser heard, with when each token began.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Recognised {
    pub text: String,
    /// The tokens as the recogniser spells them (a token that starts a word
    /// begins with a space or `▁`), in order. Empty when the model reports no
    /// times (Whisper) or the times don't match the tokens.
    pub tokens: Vec<TimedToken>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TimedToken {
    pub text: String,
    /// Seconds from the start of the audio.
    pub start: f32,
    /// Seconds, when the model reports it (Parakeet TDT does); else 0.
    pub duration: f32,
}

impl TimedToken {
    /// Does this token start a new word?
    pub fn starts_word(&self) -> bool {
        self.text.starts_with(' ') || self.text.starts_with('\u{2581}')
    }
}

/// Tokens paired with their times, or none when the times are missing or
/// don't line up with the tokens.
#[cfg_attr(not(feature = "stt"), allow(dead_code))]
fn timed_tokens(
    tokens: Vec<String>,
    starts: Option<Vec<f32>>,
    durations: Option<Vec<f32>>,
) -> Vec<TimedToken> {
    let Some(starts) = starts.filter(|s| s.len() == tokens.len()) else { return Vec::new() };
    let durations = durations.filter(|d| d.len() == tokens.len()).unwrap_or_else(|| vec![0.0; tokens.len()]);
    tokens
        .into_iter()
        .zip(starts)
        .zip(durations)
        .map(|((text, start), duration)| TimedToken { text, start, duration })
        .collect()
}

#[cfg(feature = "stt")]
pub struct OfflineAsr {
    recognizer: OfflineRecognizer,
    model: AsrModel,
}

/// A recogniser in a build without speech recognition (`stt` off): it can't
/// be created, so a visit stays "Not transcribed yet" (ADR-0018 section 3).
#[cfg(not(feature = "stt"))]
pub struct OfflineAsr {
    _never: std::convert::Infallible,
}

#[cfg(not(feature = "stt"))]
impl OfflineAsr {
    pub fn new(model: AsrModel) -> Result<Self, AsrError> {
        Self::with_threads(model, default_threads())
    }

    pub fn with_threads(model: AsrModel, threads: i32) -> Result<Self, AsrError> {
        let _ = (model, threads);
        Err(AsrError::NotInThisBuild)
    }

    pub fn recognise(&self, _samples: &[f32]) -> Result<Recognised, AsrError> {
        match self._never {}
    }
}

#[cfg(not(feature = "stt"))]
impl crate::AsrBackend for OfflineAsr {
    fn capabilities(&self) -> crate::AsrCapabilities {
        match self._never {}
    }

    fn transcribe(&self, _samples: &[f32]) -> Result<String, AsrError> {
        match self._never {}
    }
}

#[cfg(feature = "stt")]
impl OfflineAsr {
    pub fn new(model: AsrModel) -> Result<Self, AsrError> {
        Self::with_threads(model, default_threads())
    }

    /// A recogniser with its own thread count (the second recogniser runs
    /// beside the main one, so it gets fewer; see [`crate::second_threads`]).
    pub fn with_threads(model: AsrModel, threads: i32) -> Result<Self, AsrError> {
        for file in model.files() {
            require_file(file)?;
        }
        let mut model_config = OfflineModelConfig {
            num_threads: threads,
            provider: Some(crate::provider_for(&model)),
            ..Default::default()
        };
        match &model {
            AsrModel::NemoTransducer { encoder, decoder, joiner, tokens } => {
                model_config.transducer = OfflineTransducerModelConfig {
                    encoder: Some(encoder.clone()),
                    decoder: Some(decoder.clone()),
                    joiner: Some(joiner.clone()),
                };
                model_config.tokens = Some(tokens.clone());
                model_config.model_type = Some("nemo_transducer".into());
            }
            AsrModel::Whisper { encoder, decoder, tokens, language } => {
                model_config.whisper = OfflineWhisperModelConfig {
                    encoder: Some(encoder.clone()),
                    decoder: Some(decoder.clone()),
                    language: language.clone(),
                    task: Some("transcribe".into()),
                    tail_paddings: -1,
                    ..Default::default()
                };
                model_config.tokens = Some(tokens.clone());
            }
            AsrModel::Canary { encoder, decoder, tokens, language } => {
                // The same language in and out: transcribe, never translate.
                model_config.canary = OfflineCanaryModelConfig {
                    encoder: Some(encoder.clone()),
                    decoder: Some(decoder.clone()),
                    src_lang: language.clone(),
                    tgt_lang: language.clone(),
                    use_pnc: true,
                };
                model_config.tokens = Some(tokens.clone());
            }
        }
        let mut config = OfflineRecognizerConfig {
            model_config,
            decoding_method: Some("greedy_search".into()),
            ..Default::default()
        };
        let recognizer = match OfflineRecognizer::create(&config) {
            Some(r) => r,
            // The GPU couldn't start (an NVIDIA library missing or mismatched):
            // the processor still works, just slower.
            None if config.model_config.provider.as_deref() != Some("cpu") => {
                tracing::warn!(
                    model = model.label(),
                    "GPU speech recognition unavailable; using the processor"
                );
                config.model_config.provider = Some("cpu".into());
                OfflineRecognizer::create(&config).ok_or(AsrError::Create)?
            }
            None => return Err(AsrError::Create),
        };
        tracing::info!(
            model = model.label(),
            provider = config.model_config.provider.as_deref().unwrap_or("cpu"),
            threads,
            "speech recogniser ready"
        );
        Ok(Self { recognizer, model })
    }

    /// Recognise one utterance (16 kHz mono), keeping when each token began.
    pub fn recognise(&self, samples: &[f32]) -> Result<Recognised, AsrError> {
        let stream = self.recognizer.create_stream();
        stream.accept_waveform(SAMPLE_RATE as i32, samples);
        self.recognizer.decode(&stream);
        let result = stream.get_result().ok_or(AsrError::Recognize)?;
        tracing::debug!(tokens = ?result.tokens, timestamps = ?result.timestamps, durations = ?result.durations, "recognised");
        Ok(Recognised {
            text: result.text.trim().to_string(),
            tokens: timed_tokens(result.tokens, result.timestamps, result.durations),
        })
    }
}

#[cfg(feature = "stt")]
impl AsrBackend for OfflineAsr {
    fn capabilities(&self) -> AsrCapabilities {
        let languages = match &self.model {
            // Parakeet TDT v3: 25 European languages.
            AsrModel::NemoTransducer { .. } => vec!["en".into(), "fr".into(), "es".into(), "de".into()],
            AsrModel::Whisper { language, .. } => vec![language.clone().unwrap_or_else(|| "auto".into())],
            AsrModel::Canary { language, .. } => vec![language.clone().unwrap_or_else(|| "en".into())],
        };
        AsrCapabilities {
            backend: "sherpa-onnx",
            model: self.model.label().into(),
            streaming: false,
            languages,
        }
    }

    fn transcribe(&self, samples: &[f32]) -> Result<String, AsrError> {
        Ok(self.recognise(samples)?.text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_times_are_kept_only_when_they_line_up() {
        let tokens = || vec![" He".to_string(), "'s".to_string(), " fine".to_string(), ".".to_string()];
        let t = timed_tokens(tokens(), Some(vec![0.0, 0.16, 0.32, 0.64]), Some(vec![0.16, 0.08, 0.24, 0.08]));
        assert_eq!(t.len(), 4);
        assert_eq!((t[2].text.as_str(), t[2].start, t[2].duration), (" fine", 0.32, 0.24));
        let starts: Vec<bool> = t.iter().map(TimedToken::starts_word).collect();
        assert_eq!(starts, [true, false, true, false]);
        // No durations: zero.
        assert_eq!(timed_tokens(tokens(), Some(vec![0.0, 0.1, 0.2, 0.3]), None)[1].duration, 0.0);
        // Whisper gives no times; a mismatch is no times either.
        assert!(timed_tokens(tokens(), None, None).is_empty());
        assert!(timed_tokens(tokens(), Some(vec![0.0]), None).is_empty());
        assert!(TimedToken { text: "\u{2581}dog".into(), start: 0.0, duration: 0.0 }.starts_word());
    }
}
