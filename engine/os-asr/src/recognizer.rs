use sherpa_onnx::{
    OfflineModelConfig, OfflineRecognizer, OfflineRecognizerConfig, OfflineTransducerModelConfig,
    OfflineWhisperModelConfig,
};

use crate::{AsrBackend, AsrCapabilities, AsrError, SAMPLE_RATE, default_threads, require_file};

/// Offline (non-streaming) ASR model families wired up in M0.
#[derive(Debug, Clone)]
pub enum AsrModel {
    /// NVIDIA NeMo transducers, e.g. Parakeet TDT 0.6B v3.
    NemoTransducer { encoder: String, decoder: String, joiner: String, tokens: String },
    /// OpenAI Whisper ONNX exports (sherpa-onnx layout).
    Whisper { encoder: String, decoder: String, tokens: String, language: Option<String> },
}

impl AsrModel {
    pub(crate) fn files(&self) -> Vec<&str> {
        match self {
            Self::NemoTransducer { encoder, decoder, joiner, tokens } => {
                vec![encoder, decoder, joiner, tokens]
            }
            Self::Whisper { encoder, decoder, tokens, .. } => vec![encoder, decoder, tokens],
        }
    }

    fn label(&self) -> &'static str {
        match self {
            Self::NemoTransducer { .. } => "nemo-transducer",
            Self::Whisper { .. } => "whisper",
        }
    }
}

pub struct OfflineAsr {
    recognizer: OfflineRecognizer,
    model: AsrModel,
}

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
}

impl AsrBackend for OfflineAsr {
    fn capabilities(&self) -> AsrCapabilities {
        let languages = match &self.model {
            // Parakeet TDT v3: 25 European languages.
            AsrModel::NemoTransducer { .. } => vec!["en".into(), "fr".into(), "es".into(), "de".into()],
            AsrModel::Whisper { language, .. } => vec![language.clone().unwrap_or_else(|| "auto".into())],
        };
        AsrCapabilities {
            backend: "sherpa-onnx",
            model: self.model.label().into(),
            streaming: false,
            languages,
        }
    }

    fn transcribe(&self, samples: &[f32]) -> Result<String, AsrError> {
        let stream = self.recognizer.create_stream();
        stream.accept_waveform(SAMPLE_RATE as i32, samples);
        self.recognizer.decode(&stream);
        let result = stream.get_result().ok_or(AsrError::Recognize)?;
        Ok(result.text.trim().to_string())
    }
}
