use serde::Serialize;
use sherpa_onnx::{SileroVadModelConfig, VadModelConfig, VoiceActivityDetector};

use crate::{AsrError, Audio, SAMPLE_RATE, default_threads, require_file};

#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
pub struct SpeechSpan {
    pub start: f32,
    pub end: f32,
}

#[derive(Debug, Clone)]
pub struct VadOptions {
    pub model: String,
    pub threshold: f32,
    pub min_silence_secs: f32,
    pub min_speech_secs: f32,
    /// Longer speech is split; keeps ASR chunks well inside model limits.
    pub max_speech_secs: f32,
}

impl VadOptions {
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            threshold: 0.5,
            min_silence_secs: 0.3,
            min_speech_secs: 0.25,
            max_speech_secs: 20.0,
        }
    }
}

pub struct SileroVad {
    options: VadOptions,
}

impl SileroVad {
    pub fn new(options: VadOptions) -> Result<Self, AsrError> {
        require_file(&options.model)?;
        Ok(Self { options })
    }

    /// Find speech spans in a whole clip.
    pub fn segments(&self, audio: &Audio) -> Result<Vec<SpeechSpan>, AsrError> {
        let config = VadModelConfig {
            silero_vad: SileroVadModelConfig {
                model: Some(self.options.model.clone()),
                threshold: self.options.threshold,
                min_silence_duration: self.options.min_silence_secs,
                min_speech_duration: self.options.min_speech_secs,
                window_size: 512,
                max_speech_duration: self.options.max_speech_secs,
            },
            sample_rate: SAMPLE_RATE as i32,
            num_threads: default_threads().min(2),
            provider: Some("cpu".into()),
            ..Default::default()
        };
        let buffer_secs = self.options.max_speech_secs + 10.0;
        let vad = VoiceActivityDetector::create(&config, buffer_secs).ok_or(AsrError::Create)?;
        let mut spans = Vec::new();
        let mut drain = |vad: &VoiceActivityDetector| {
            while let Some(segment) = vad.front() {
                let start = segment.start() as f32 / SAMPLE_RATE as f32;
                let end = start + segment.n() as f32 / SAMPLE_RATE as f32;
                spans.push(SpeechSpan { start, end });
                vad.pop();
            }
        };
        for window in audio.samples.chunks(512) {
            vad.accept_waveform(window);
            drain(&vad);
        }
        vad.flush();
        drain(&vad);
        Ok(spans)
    }
}
