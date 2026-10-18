use serde::{Deserialize, Serialize};

const BUILTIN: &str = include_str!("../catalog.json");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Slot {
    Llm,
    Stt,
    Vad,
    Diarization,
    SpeakerEmbedding,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Source {
    Huggingface {
        repo: String,
        revision: String,
    },
    /// Each file carries its own `url`.
    Url,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileSpec {
    pub path: String,
    pub size: u64,
    pub sha256: String,
    #[serde(default)]
    pub url: Option<String>,
}

/// How the files of a model map onto engine inputs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Layout {
    LlmGguf {
        model: String,
    },
    AsrNemoTransducer {
        encoder: String,
        decoder: String,
        joiner: String,
        tokens: String,
    },
    AsrWhisper {
        encoder: String,
        decoder: String,
        tokens: String,
    },
    VadSilero {
        model: String,
    },
    DiarSegmentation {
        model: String,
    },
    SpeakerEmbedding {
        model: String,
    },
    /// Archives unpacked elsewhere (the GPU speech pack, ADR-0014).
    Archives,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogEntry {
    pub id: String,
    pub display_name: String,
    pub slot: Slot,
    pub license: String,
    #[serde(default)]
    pub attribution: Option<String>,
    pub source: Source,
    pub files: Vec<FileSpec>,
    pub layout: Layout,
}

impl CatalogEntry {
    pub fn total_size(&self) -> u64 {
        self.files.iter().map(|f| f.size).sum()
    }

    /// Download URL for one file, pinned to the catalog revision.
    pub fn url_for(&self, file: &FileSpec) -> Option<String> {
        match &self.source {
            Source::Huggingface { repo, revision } => {
                Some(format!("https://huggingface.co/{repo}/resolve/{revision}/{}", file.path))
            }
            Source::Url => file.url.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Catalog {
    pub schema_version: u32,
    pub models: Vec<CatalogEntry>,
}

impl Catalog {
    /// The catalog compiled into the binary (works fully offline).
    pub fn builtin() -> Self {
        serde_json::from_str(BUILTIN).expect("built-in catalog.json is valid")
    }

    pub fn get(&self, id: &str) -> Option<&CatalogEntry> {
        self.models.iter().find(|m| m.id == id)
    }

    pub fn by_slot(&self, slot: Slot) -> impl Iterator<Item = &CatalogEntry> {
        self.models.iter().filter(move |m| m.slot == slot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn builtin_catalog_is_well_formed() {
        let catalog = Catalog::builtin();
        assert_eq!(catalog.schema_version, 1);
        let mut ids = HashSet::new();
        for m in &catalog.models {
            assert!(ids.insert(&m.id), "duplicate id {}", m.id);
            assert!(!m.files.is_empty(), "{} has no files", m.id);
            for f in &m.files {
                assert_eq!(f.sha256.len(), 64, "{}/{} sha256", m.id, f.path);
                assert!(f.sha256.chars().all(|c| c.is_ascii_hexdigit()));
                assert!(f.size > 0);
                let url = m.url_for(f).unwrap_or_else(|| panic!("{}/{} has no url", m.id, f.path));
                assert!(url.starts_with("https://"));
            }
            if let Source::Huggingface { revision, .. } = &m.source {
                assert_eq!(revision.len(), 40, "{} must pin a full commit sha", m.id);
            }
            // Every file the layout references must be in the file list.
            let names: HashSet<&str> = m.files.iter().map(|f| f.path.as_str()).collect();
            let referenced: Vec<&str> = match &m.layout {
                Layout::LlmGguf { model }
                | Layout::VadSilero { model }
                | Layout::DiarSegmentation { model }
                | Layout::SpeakerEmbedding { model } => vec![model],
                Layout::AsrNemoTransducer { encoder, decoder, joiner, tokens } => {
                    vec![encoder, decoder, joiner, tokens]
                }
                Layout::AsrWhisper { encoder, decoder, tokens } => vec![encoder, decoder, tokens],
                Layout::Archives => vec![],
            };
            for r in referenced {
                assert!(names.contains(r), "{}: layout references missing file {r}", m.id);
            }
        }
    }

    #[test]
    fn only_redistributable_licenses_in_builtin_catalog() {
        // HAI-DEF (MedGemma) and similar must never be in the built-in catalog.
        let allowed = ["Apache-2.0", "MIT", "CC-BY-4.0"];
        for m in Catalog::builtin().models {
            assert!(allowed.contains(&m.license.as_str()), "{} has license {}", m.id, m.license);
            if m.license == "CC-BY-4.0" {
                assert!(m.attribution.is_some(), "{} needs attribution", m.id);
            }
        }
    }
}
