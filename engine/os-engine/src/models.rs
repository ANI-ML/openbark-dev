//! Resolve catalog models in the local store into engine components.
//! Shared by the CLI and the desktop app.

use std::path::{Path, PathBuf};

use os_asr::AsrModel;
use os_models::{Catalog, CatalogEntry, Layout, ModelStore, Verify};

use crate::{EngineError, Transcriber};

pub const DEFAULT_STT: &str = "parakeet-tdt-0.6b-v3-int8";
pub const DEFAULT_LLM: &str = "gemma-4-e4b-it-q4_0";
pub const VAD: &str = "silero-vad";
pub const SEGMENTATION: &str = "pyannote-segmentation-3.0";
pub const EMBEDDING: &str = "wespeaker-en-resnet34-lm";

/// Full-precision recognisers for NVIDIA graphics (ADR-0014). The int8 ones
/// run faster on the processor than on the GPU, so the GPU is only used with
/// these, and only when the GPU speech pack is installed.
pub const GPU_STT: &str = "parakeet-tdt-0.6b-v3-fp32";
pub const GPU_SECOND_STT: &str = "whisper-large-v3-turbo-fp32";

/// Languages Whisper can be told to expect, among the app's.
const WHISPER_LANGUAGES: &[&str] = &["en", "fr", "es", "de", "pt"];

/// The language the second recogniser (Whisper) is told to expect. Left to
/// itself it guesses per utterance, and an accented English sentence can come
/// back in another language; so it follows the app's language, English until
/// the app says otherwise.
static SPEECH_LANGUAGE: std::sync::Mutex<Option<&str>> = std::sync::Mutex::new(Some("en"));

/// One of [`WHISPER_LANGUAGES`] for a language code ("fr", "fr-CA"), or
/// None for one it doesn't offer (and for "auto").
fn whisper_language(code: &str) -> Option<&'static str> {
    let c = code.split(['-', '_']).next().unwrap_or(code).to_ascii_lowercase();
    WHISPER_LANGUAGES.iter().copied().find(|l| *l == c)
}

/// Set the language Whisper expects ("fr", or "fr-CA"); one it doesn't know
/// (or None) leaves it to detect the language itself.
pub fn set_speech_language(code: Option<&str>) {
    let lang = code.and_then(whisper_language);
    *SPEECH_LANGUAGE.lock().unwrap_or_else(|e| e.into_inner()) = lang;
}

pub fn speech_language() -> Option<&'static str> {
    *SPEECH_LANGUAGE.lock().unwrap_or_else(|e| e.into_inner())
}

/// `model` told to expect [`speech_language`] when it is Whisper.
pub fn with_speech_language(mut model: AsrModel) -> AsrModel {
    if let AsrModel::Whisper { language, .. } = &mut model {
        *language = speech_language().map(String::from);
    }
    model
}

/// The recogniser to use for `id`: its full-precision version when the GPU
/// speech pack and that model are installed, else `id` itself.
pub fn best_stt(store: &ModelStore, id: &str) -> String {
    let gpu = match id {
        DEFAULT_STT => GPU_STT,
        crate::live::SECOND_STT => GPU_SECOND_STT,
        _ => return id.to_string(),
    };
    let catalog = Catalog::builtin();
    // Full precision only where it will really run on the GPU: on the
    // processor it's slower than int8.
    let ready = os_asr::asr_provider() == "cuda" && installed(&catalog, store, gpu).is_ok();
    if ready { gpu.to_string() } else { id.to_string() }
}

fn installed<'a>(
    catalog: &'a Catalog,
    store: &ModelStore,
    id: &str,
) -> Result<&'a CatalogEntry, EngineError> {
    let m = catalog.get(id).ok_or_else(|| EngineError::Model(format!("unknown model '{id}'")))?;
    if !store.is_installed(m, Verify::Size) {
        return Err(EngineError::Model(format!(
            "model '{id}' is not installed. Run: openbark models pull {id}"
        )));
    }
    Ok(m)
}

fn file(store: &ModelStore, m: &CatalogEntry, name: &str) -> String {
    store.file_path(m, name).to_string_lossy().into_owned()
}

/// Path of a model whose layout is a single file (VAD, segmentation, embeddings).
pub fn single_file(store: &ModelStore, id: &str) -> Result<String, EngineError> {
    let catalog = Catalog::builtin();
    let m = installed(&catalog, store, id)?;
    match &m.layout {
        Layout::VadSilero { model }
        | Layout::DiarSegmentation { model }
        | Layout::SpeakerEmbedding { model }
        | Layout::LlmGguf { model } => Ok(file(store, m, model)),
        other => Err(EngineError::Model(format!("'{id}' is not a single-file model ({other:?})"))),
    }
}

/// [`asr_model`] told to expect `language` ("fr") when it takes one
/// (Whisper). None, "auto" or a language it doesn't offer: it detects the
/// language itself.
pub fn asr_model_in(store: &ModelStore, id: &str, language: Option<&str>) -> Result<AsrModel, EngineError> {
    let mut model = asr_model(store, id)?;
    if let AsrModel::Whisper { language: lang, .. } = &mut model {
        *lang = language.and_then(whisper_language).map(String::from);
    }
    Ok(model)
}

pub fn asr_model(store: &ModelStore, id: &str) -> Result<AsrModel, EngineError> {
    let catalog = Catalog::builtin();
    let m = installed(&catalog, store, id)?;
    match &m.layout {
        Layout::AsrNemoTransducer { encoder, decoder, joiner, tokens } => Ok(AsrModel::NemoTransducer {
            encoder: file(store, m, encoder),
            decoder: file(store, m, decoder),
            joiner: file(store, m, joiner),
            tokens: file(store, m, tokens),
        }),
        Layout::AsrWhisper { encoder, decoder, tokens } => Ok(AsrModel::Whisper {
            encoder: file(store, m, encoder),
            decoder: file(store, m, decoder),
            tokens: file(store, m, tokens),
            language: None,
        }),
        other => Err(EngineError::Model(format!("'{id}' is not a speech-to-text model ({other:?})"))),
    }
}

/// A catalog id, or a direct path to a `.gguf` file (bring-your-own model).
pub fn llm_path(store: &ModelStore, spec: &str) -> Result<PathBuf, EngineError> {
    let path = Path::new(spec);
    if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("gguf")) {
        return if path.is_file() {
            Ok(path.to_path_buf())
        } else {
            Err(EngineError::Model(format!("{} does not exist", path.display())))
        };
    }
    let catalog = Catalog::builtin();
    let m = installed(&catalog, store, spec)?;
    match &m.layout {
        Layout::LlmGguf { model } => Ok(store.file_path(m, model)),
        other => Err(EngineError::Model(format!("'{spec}' is not an LLM ({other:?})"))),
    }
}

/// Build the transcription pipeline from installed models. `language`: the
/// language the recogniser is told to expect, when it takes one
/// ([`asr_model_in`]).
#[allow(clippy::too_many_arguments)]
pub fn transcriber(
    store: &ModelStore,
    stt: &str,
    language: Option<&str>,
    diarize: bool,
    num_speakers: Option<u32>,
    cluster_threshold: Option<f32>,
    window_shift: Option<f32>,
    speakerphone: bool,
) -> Result<Transcriber, EngineError> {
    let mut vad_options = os_asr::VadOptions::new(single_file(store, VAD)?);
    if speakerphone {
        vad_options = vad_options.speakerphone();
    }
    let vad = os_asr::SileroVad::new(vad_options)?;
    let asr = os_asr::OfflineAsr::new(asr_model_in(store, stt, language)?)?;
    let diarizer = if diarize {
        let mut options =
            os_diar::DiarOptions::new(single_file(store, SEGMENTATION)?, single_file(store, EMBEDDING)?);
        options.num_speakers = num_speakers;
        if let Some(t) = cluster_threshold {
            options.threshold = t;
        }
        if let Some(w) = window_shift {
            options.window_shift_ratio = w;
        }
        Some(os_diar::Diarizer::new(&options)?)
    } else {
        None
    };
    Ok(Transcriber { vad, asr: Box::new(asr), diarizer, speakerphone })
}

/// Models the default pipeline needs, with install status (for onboarding).
pub fn required_status(store: &ModelStore) -> Vec<(String, String, bool)> {
    required_status_for(store, os_models::platform::Profile::Desktop)
}

/// [`required_status`] for a profile. On a phone drafting is opt-in
/// (ADR-0018 section 10), so only the speech models are needed to start:
/// recording and transcribing work with them alone.
pub fn required_status_for(
    store: &ModelStore,
    profile: os_models::platform::Profile,
) -> Vec<(String, String, bool)> {
    required_status_with(store, profile, DEFAULT_LLM)
}

/// [`required_status_for`] with the computer's note model: the app picks it
/// from the computer's memory (os-app's `policy::default_note_model`).
pub fn required_status_with(
    store: &ModelStore,
    profile: os_models::platform::Profile,
    llm: &str,
) -> Vec<(String, String, bool)> {
    let catalog = Catalog::builtin();
    let ids: &[&str] = if profile.is_mobile() {
        &[VAD, os_models::platform::MOBILE_STT, SEGMENTATION, EMBEDDING]
    } else {
        &[VAD, DEFAULT_STT, SEGMENTATION, EMBEDDING, llm]
    };
    ids.iter()
        .filter_map(|id| catalog.get(id))
        .map(|m| (m.id.clone(), m.display_name.clone(), store.is_installed(m, Verify::Size)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whisper_follows_the_app_language() {
        let whisper = || AsrModel::Whisper {
            encoder: "e".into(),
            decoder: "d".into(),
            tokens: "t".into(),
            language: None,
        };
        let language = |m: AsrModel| match m {
            AsrModel::Whisper { language, .. } => language,
            _ => unreachable!(),
        };
        assert_eq!(language(with_speech_language(whisper())).as_deref(), Some("en"));
        set_speech_language(Some("fr-CA"));
        assert_eq!(language(with_speech_language(whisper())).as_deref(), Some("fr"));
        // A language not offered here: Whisper detects it itself.
        set_speech_language(Some("pl"));
        assert_eq!(language(with_speech_language(whisper())), None);
        set_speech_language(Some("en"));
    }
}
