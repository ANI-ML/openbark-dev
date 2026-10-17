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
    fn files(&self) -> Vec<&str> {
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
        for file in model.files() {
            require_file(file)?;
        }
        let mut model_config = OfflineModelConfig {
            num_threads: default_threads(),
            provider: Some("cpu".into()),
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
        let config = OfflineRecognizerConfig {
            model_config,
            decoding_method: Some("greedy_search".into()),
            ..Default::default()
        };
        let recognizer = OfflineRecognizer::create(&config).ok_or(AsrError::Create)?;
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
