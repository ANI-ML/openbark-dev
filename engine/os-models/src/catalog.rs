use std::path::Path;

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
    /// Added from this computer (ADR-0025 §9). `path` is the absolute path of
    /// the file or folder when it is used where it is; `None` when copied into
    /// the store under `local/<name>`.
    Local {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileSpec {
    /// Repo-relative, '/' separated.
    pub path: String,
    pub size: u64,
    /// May be empty only when `git_sha1` is set (small Hub files outside LFS).
    #[serde(default)]
    pub sha256: String,
    /// Git blob SHA-1 (40 hex) of a Hub file that isn't in LFS: SHA-1 of
    /// `"blob <len>\0"` followed by the bytes (ADR-0025 §5).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_sha1: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
}

/// How the files of a model map onto engine inputs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Layout {
    /// For split GGUFs, `model` is part 1; every part is in `files`.
    LlmGguf {
        model: String,
    },
    /// An MLX model folder; the folder is the parent of `config`
    /// (always `config.json` from the Hub).
    LlmMlx {
        config: String,
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

impl Layout {
    /// The files this layout names directly (each must be in the entry's `files`).
    pub fn referenced_files(&self) -> Vec<&str> {
        match self {
            Layout::LlmGguf { model }
            | Layout::VadSilero { model }
            | Layout::DiarSegmentation { model }
            | Layout::SpeakerEmbedding { model } => vec![model],
            Layout::LlmMlx { config } => vec![config],
            Layout::AsrNemoTransducer { encoder, decoder, joiner, tokens } => {
                vec![encoder, decoder, joiner, tokens]
            }
            Layout::AsrWhisper { encoder, decoder, tokens } => vec![encoder, decoder, tokens],
            Layout::Archives => vec![],
        }
    }
}

/// Which engine writes notes with a language model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LlmEngine {
    /// llama.cpp, GGUF files.
    Llama,
    /// MLX on Apple silicon (ADR-0025).
    Mlx,
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
    /// The Hub repo is gated: downloads send the saved token (and only these).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub gated: bool,
    /// Suited to a phone (ADR-0018 section 10): the iPhone app lists only
    /// these, and only those within its memory tier can be downloaded there.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub mobile: bool,
}

impl CatalogEntry {
    pub fn total_size(&self) -> u64 {
        self.files.iter().map(|f| f.size).sum()
    }

    /// Download URL for one file, pinned to the catalog revision. `None` for
    /// models added from this computer.
    pub fn url_for(&self, file: &FileSpec) -> Option<String> {
        match &self.source {
            Source::Huggingface { repo, revision } => {
                Some(format!("https://huggingface.co/{repo}/resolve/{revision}/{}", encode_path(&file.path)))
            }
            Source::Url => file.url.clone(),
            Source::Local { .. } => None,
        }
    }

    /// `LlmGguf` runs on llama.cpp, `LlmMlx` on MLX; other slots have no LLM engine.
    pub fn llm_engine(&self) -> Option<LlmEngine> {
        match self.layout {
            Layout::LlmGguf { .. } => Some(LlmEngine::Llama),
            Layout::LlmMlx { .. } => Some(LlmEngine::Mlx),
            _ => None,
        }
    }

    /// Where a model used in place lives (the file, or the MLX folder).
    pub fn in_place(&self) -> Option<&Path> {
        match &self.source {
            Source::Local { path: Some(p) } => Some(Path::new(p)),
            _ => None,
        }
    }

    /// Added from this computer, copied or used in place.
    pub fn is_local(&self) -> bool {
        matches!(self.source, Source::Local { .. })
    }
}

/// Percent-encode a repo path for a URL, keeping '/' between components.
fn encode_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for b in path.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~/".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
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

    /// Every file the layout references must be in the file list.
    fn layout_files_listed(m: &CatalogEntry) {
        let names: HashSet<&str> = m.files.iter().map(|f| f.path.as_str()).collect();
        for r in m.layout.referenced_files() {
            assert!(names.contains(r), "{}: layout references missing file {r}", m.id);
        }
    }

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
            assert!(!m.is_local() && !m.gated, "{}: built-ins are neither local nor gated", m.id);
            layout_files_listed(m);
        }
    }

    #[test]
    fn only_redistributable_licenses_in_builtin_catalog() {
        // HAI-DEF (MedGemma) and similar must never be in the built-in catalog.
        // The NVIDIA Nemotron Open Model License allows use and redistribution
        // provided recipients get a copy of it and the statement "Licensed by
        // NVIDIA Corporation under the NVIDIA Nemotron Model License."
        // (NOTICE, apps/desktop/public/licenses/), so it is allowed here with
        // that statement as its attribution.
        const NEMOTRON: &str = "NVIDIA Nemotron Open Model License";
        let allowed = ["Apache-2.0", "MIT", "CC-BY-4.0", NEMOTRON];
        for m in Catalog::builtin().models {
            assert!(allowed.contains(&m.license.as_str()), "{} has license {}", m.id, m.license);
            if m.license == "CC-BY-4.0" {
                assert!(m.attribution.is_some(), "{} needs attribution", m.id);
            }
            if m.license == NEMOTRON {
                let a = m.attribution.as_deref().unwrap_or_default();
                assert!(
                    a.contains("Licensed by NVIDIA Corporation under the NVIDIA Nemotron Model License"),
                    "{} needs NVIDIA's statement",
                    m.id
                );
            }
        }
    }

    /// A user-catalog.json written before ADR-0025 (no `gated`, no `git_sha1`).
    const OLD_USER_CATALOG: &str = r#"{
      "schema_version": 1,
      "models": [{
        "id": "hf:bartowski/Qwen_Qwen3-4B-GGUF/Qwen_Qwen3-4B-Q4_K_M",
        "display_name": "bartowski/Qwen_Qwen3-4B-GGUF (Qwen_Qwen3-4B-Q4_K_M)",
        "slot": "llm",
        "license": "apache-2.0",
        "attribution": "bartowski/Qwen_Qwen3-4B-GGUF on Hugging Face",
        "source": {"kind": "huggingface", "repo": "bartowski/Qwen_Qwen3-4B-GGUF", "revision": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"},
        "files": [{"path": "Qwen_Qwen3-4B-Q4_K_M.gguf", "size": 2497280000, "sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", "url": null}],
        "layout": {"type": "llm_gguf", "model": "Qwen_Qwen3-4B-Q4_K_M.gguf"}
      }]
    }"#;

    #[test]
    fn old_user_catalogs_still_load() {
        let c: Catalog = serde_json::from_str(OLD_USER_CATALOG).unwrap();
        let m = &c.models[0];
        assert!(!m.gated);
        assert_eq!(m.files[0].git_sha1, None);
        assert_eq!(m.llm_engine(), Some(LlmEngine::Llama));
        assert!(!m.is_local() && m.in_place().is_none());
        // Written back, the new fields stay out when unused.
        let json = serde_json::to_string(m).unwrap();
        assert!(!json.contains("gated") && !json.contains("git_sha1"), "{json}");
    }

    fn mlx_entry() -> CatalogEntry {
        CatalogEntry {
            id: "hf:mlx-community/Qwen3-0.6B-4bit/@mlx".into(),
            display_name: "mlx-community/Qwen3-0.6B-4bit (MLX)".into(),
            slot: Slot::Llm,
            license: "apache-2.0".into(),
            attribution: None,
            source: Source::Huggingface {
                repo: "mlx-community/Qwen3-0.6B-4bit".into(),
                revision: "73e3e38d981303bc594367cd910ea6eb48349da8".into(),
            },
            files: vec![
                FileSpec {
                    path: "config.json".into(),
                    size: 937,
                    sha256: String::new(),
                    git_sha1: Some("6c974b02334f2f5f68de4cb2796ee4fe80bfc712".into()),
                    url: None,
                },
                FileSpec {
                    path: "model.safetensors".into(),
                    size: 335450584,
                    sha256: "c".repeat(64),
                    git_sha1: None,
                    url: None,
                },
            ],
            layout: Layout::LlmMlx { config: "config.json".into() },
            gated: true,
            mobile: false,
        }
    }

    #[test]
    fn mlx_layout_and_git_sha1_round_trip() {
        let m = mlx_entry();
        layout_files_listed(&m);
        assert_eq!(m.llm_engine(), Some(LlmEngine::Mlx));
        let json = serde_json::to_value(&m).unwrap();
        assert_eq!(json["layout"], serde_json::json!({"type": "llm_mlx", "config": "config.json"}));
        assert_eq!(json["gated"], true);
        assert_eq!(json["files"][0]["git_sha1"], "6c974b02334f2f5f68de4cb2796ee4fe80bfc712");
        assert_eq!(json["files"][0]["sha256"], "");
        assert!(json["files"][1].get("git_sha1").is_none());
        let back: CatalogEntry = serde_json::from_value(json).unwrap();
        assert_eq!(back.files[0].git_sha1.as_deref(), Some("6c974b02334f2f5f68de4cb2796ee4fe80bfc712"));
        assert!(back.gated);
        assert_eq!(serde_json::to_value(LlmEngine::Mlx).unwrap(), "mlx");
    }

    #[test]
    #[should_panic(expected = "layout references missing file")]
    fn mlx_layout_must_list_its_config() {
        let mut m = mlx_entry();
        m.files.remove(0);
        layout_files_listed(&m);
    }

    #[test]
    fn a_file_spec_without_sha256_loads() {
        let f: FileSpec = serde_json::from_str(
            r#"{"path":"config.json","size":9,"git_sha1":"0123456789012345678901234567890123456789"}"#,
        )
        .unwrap();
        assert_eq!(f.sha256, "");
        assert!(f.git_sha1.is_some());
    }

    #[test]
    fn local_source_json() {
        let copied: Source = serde_json::from_str(r#"{"kind":"local"}"#).unwrap();
        assert!(matches!(copied, Source::Local { path: None }));
        assert_eq!(serde_json::to_string(&copied).unwrap(), r#"{"kind":"local"}"#);
        let used = Source::Local { path: Some("/Users/x/models/m.gguf".into()) };
        assert_eq!(
            serde_json::to_string(&used).unwrap(),
            r#"{"kind":"local","path":"/Users/x/models/m.gguf"}"#
        );

        let mut m = mlx_entry();
        m.source = used;
        assert!(m.is_local());
        assert_eq!(m.in_place(), Some(Path::new("/Users/x/models/m.gguf")));
        assert_eq!(m.url_for(&m.files[0]), None, "a local model has nowhere to download from");
        m.source = copied;
        assert!(m.is_local() && m.in_place().is_none());
    }

    #[test]
    fn urls_keep_folders_and_encode_odd_characters() {
        let mut m = mlx_entry();
        m.files[0].path = "Q4_K_M/Model 1+x-00001-of-00003.gguf".into();
        assert_eq!(
            m.url_for(&m.files[0]).unwrap(),
            "https://huggingface.co/mlx-community/Qwen3-0.6B-4bit/resolve/73e3e38d981303bc594367cd910ea6eb48349da8/Q4_K_M/Model%201%2Bx-00001-of-00003.gguf"
        );
    }
}
