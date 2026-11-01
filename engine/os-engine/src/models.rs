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

/// NVIDIA Canary 180M Flash (int8), told the language spoken (English,
/// French, Spanish or German); it can't detect one itself. In the catalogue
/// so the French eval can measure it; it failed the bar, so the app never
/// lists or downloads it ([`offered`], ADR-0033).
pub const CANARY_STT: &str = "canary-180m-flash-int8";

/// Languages Whisper can be told to expect, among the app's. These are also
/// the choices of Settings, Recording, Spoken language.
const WHISPER_LANGUAGES: &[&str] = &["en", "fr", "es", "de", "pt"];

/// Languages Canary 180M Flash can be told.
const CANARY_LANGUAGES: &[&str] = &["en", "fr", "es", "de"];

/// The main recogniser for a spoken language, for each language where a
/// candidate passed the bar set in docs/eval/french.md before the results
/// (ADR-0033). Every other language, and Auto, keep Parakeet. Empty: in the
/// run of 2026-10-31 neither Canary nor Whisper passed, for French or English.
pub const LOCKED_MAIN: &[(&str, &str)] = &[];

/// The app's language. Left to itself Whisper guesses the language per
/// utterance, and an accented English sentence can come back in another
/// language; so with the spoken language on Auto it is told the app's
/// language, English until the app says otherwise (as in 0.2.4).
static SPEECH_LANGUAGE: std::sync::Mutex<Option<&str>> = std::sync::Mutex::new(Some("en"));

/// Settings, Recording, Spoken language; None is Auto (the default).
static SPOKEN_LANGUAGE: std::sync::Mutex<Option<&str>> = std::sync::Mutex::new(None);

/// One of [`WHISPER_LANGUAGES`] for a language code ("fr", "fr-CA"), or
/// None for one it doesn't offer (and for "auto").
fn whisper_language(code: &str) -> Option<&'static str> {
    let c = code.split(['-', '_']).next().unwrap_or(code).to_ascii_lowercase();
    WHISPER_LANGUAGES.iter().copied().find(|l| *l == c)
}

/// The app's language ("fr", or "fr-CA"), which Whisper expects while the
/// spoken language is Auto; one it doesn't know (or None) leaves Whisper to
/// detect the language itself.
pub fn set_speech_language(code: Option<&str>) {
    let lang = code.and_then(whisper_language);
    *SPEECH_LANGUAGE.lock().unwrap_or_else(|e| e.into_inner()) = lang;
}

/// Settings, Recording, Spoken language ("fr"); None, "auto" or a language
/// it doesn't offer is Auto: Parakeet detects the language of each sentence
/// and Whisper is told the app's language, as in 0.2.4.
pub fn set_spoken_language(code: Option<&str>) {
    let lang = code.and_then(whisper_language);
    *SPOKEN_LANGUAGE.lock().unwrap_or_else(|e| e.into_inner()) = lang;
}

/// The spoken language set in Settings, or None for Auto.
pub fn spoken_language() -> Option<&'static str> {
    *SPOKEN_LANGUAGE.lock().unwrap_or_else(|e| e.into_inner())
}

/// The language a recogniser that takes one is told: the spoken language
/// when set, else the app's.
pub fn speech_language() -> Option<&'static str> {
    spoken_language().or_else(|| *SPEECH_LANGUAGE.lock().unwrap_or_else(|e| e.into_inner()))
}

fn canary_language(code: Option<&str>) -> Option<&'static str> {
    code.and_then(|c| CANARY_LANGUAGES.iter().copied().find(|l| *l == c))
}

/// `model` told to expect [`speech_language`] when it takes a language
/// (Whisper, Canary).
pub fn with_speech_language(mut model: AsrModel) -> AsrModel {
    match &mut model {
        AsrModel::Whisper { language, .. } => *language = speech_language().map(String::from),
        AsrModel::Canary { language, .. } => *language = canary_language(speech_language()).map(String::from),
        AsrModel::NemoTransducer { .. } => {}
    }
    model
}

/// The recogniser [`LOCKED_MAIN`] gives a spoken language, if any.
pub fn locked_main(spoken: Option<&str>) -> Option<&'static str> {
    let lang = spoken?;
    LOCKED_MAIN.iter().find(|(l, _)| *l == lang).map(|(_, id)| *id)
}

/// The main recogniser for a recording that asks for `requested`: for the
/// app's default, the spoken language's own recogniser ([`LOCKED_MAIN`])
/// when that language is set and the recogniser is installed; else
/// `requested`.
pub fn main_stt(store: &ModelStore, requested: &str) -> String {
    if requested == DEFAULT_STT
        && let Some(id) = locked_main(spoken_language())
        && installed(&Catalog::builtin(), store, id).is_ok()
    {
        return id.to_string();
    }
    requested.to_string()
}

/// Whether Settings, Models lists this model: Canary only when a spoken
/// language uses it ([`LOCKED_MAIN`]); every other model as before.
pub fn offered(id: &str) -> bool {
    id != CANARY_STT || LOCKED_MAIN.iter().any(|(_, m)| *m == id)
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
/// (Whisper, Canary). None, "auto" or a language it doesn't offer: Whisper
/// detects the language itself (Canary falls back to English).
pub fn asr_model_in(store: &ModelStore, id: &str, language: Option<&str>) -> Result<AsrModel, EngineError> {
    let mut model = asr_model(store, id)?;
    let lang = language.and_then(whisper_language);
    match &mut model {
        AsrModel::Whisper { language, .. } => *language = lang.map(String::from),
        AsrModel::Canary { language, .. } => *language = canary_language(lang).map(String::from),
        AsrModel::NemoTransducer { .. } => {}
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
        Layout::AsrCanary { encoder, decoder, tokens } => Ok(AsrModel::Canary {
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

/// [`required_status_for`] with the note model the app asks for. On a
/// computer that is the model recommended from its memory, where notes are
/// written and its processor cores (os-app's `policy::recommend_note_model`,
/// ADR-0034), unless the other built-in note model is already downloaded. A
/// phone's list has no note model, so `llm` is unused there.
pub fn required_status_with(
    store: &ModelStore,
    profile: os_models::platform::Profile,
    llm: &str,
) -> Vec<(String, String, bool)> {
    let catalog = Catalog::builtin();
    let mut ids: Vec<&str> = if profile.is_mobile() {
        vec![VAD, os_models::platform::MOBILE_STT, SEGMENTATION, EMBEDDING]
    } else {
        vec![VAD, DEFAULT_STT, SEGMENTATION, EMBEDDING, llm]
    };
    // A spoken language with its own recogniser (ADR-0033): downloaded only
    // when the setting needs it, never on a phone.
    if !profile.is_mobile()
        && let Some(id) = locked_main(spoken_language())
    {
        ids.push(id);
    }
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

        // Settings, Spoken language overrides the app's language, for Whisper
        // and Canary alike; Auto goes back to the app's (ADR-0033).
        let canary = || AsrModel::Canary {
            encoder: "e".into(),
            decoder: "d".into(),
            tokens: "t".into(),
            language: None,
        };
        let canary_language = |m: AsrModel| match m {
            AsrModel::Canary { language, .. } => language,
            _ => unreachable!(),
        };
        set_spoken_language(Some("fr"));
        assert_eq!(spoken_language(), Some("fr"));
        assert_eq!(language(with_speech_language(whisper())).as_deref(), Some("fr"));
        assert_eq!(canary_language(with_speech_language(canary())).as_deref(), Some("fr"));
        // Portuguese: Whisper is told; Canary can't be (it isn't the main
        // recogniser for it).
        set_spoken_language(Some("pt"));
        assert_eq!(language(with_speech_language(whisper())).as_deref(), Some("pt"));
        assert_eq!(canary_language(with_speech_language(canary())), None);
        set_spoken_language(Some("auto"));
        assert_eq!(spoken_language(), None);
        assert_eq!(language(with_speech_language(whisper())).as_deref(), Some("en"));
        set_spoken_language(None);
    }

    #[test]
    fn a_locked_main_recogniser_comes_from_the_table_only() {
        for (lang, id) in LOCKED_MAIN {
            assert_eq!(locked_main(Some(*lang)), Some(*id));
            assert!(offered(id));
        }
        assert_eq!(locked_main(None), None, "Auto keeps Parakeet");
        assert_eq!(locked_main(Some("pt")), None, "no Portuguese fixtures, so no switch");
        assert_eq!(locked_main(Some("es")), None, "no Spanish fixtures, so no switch");
        assert_eq!(offered(CANARY_STT), LOCKED_MAIN.iter().any(|(_, m)| *m == CANARY_STT));
        assert!(offered(DEFAULT_STT) && offered(crate::live::SECOND_STT));
    }
}
