//! Hugging Face Hub browsing: search GGUF models, inspect files, classify
//! licences, and add a chosen file to the user's catalog for download.
//!
//! Network: `huggingface.co/api/models…` (listed in docs/network.md, R4),
//! only when the user searches or opens a model in the model manager.

use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::catalog::{Catalog, CatalogEntry, FileSpec, Layout, Slot, Source};
use crate::store::ModelStore;

const API: &str = "https://huggingface.co/api/models";

/// Licences OpenBark lets you download with one click: permissive and
/// commercial-use friendly.
const PERMISSIVE: &[&str] =
    &["apache-2.0", "mit", "cc-by-4.0", "bsd-2-clause", "bsd-3-clause", "cc0-1.0", "unlicense", "isc"];

/// How freely a model can be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LicenseClass {
    /// Permissive: one-click download.
    Permissive,
    /// Custom or restrictive terms (e.g. non-commercial, use policies): the
    /// user must read and accept the licence before downloading.
    Restricted,
    /// No licence stated: treated as restricted.
    Unknown,
}

pub fn classify(license: Option<&str>) -> LicenseClass {
    match license.map(str::to_ascii_lowercase) {
        Some(l) if PERMISSIVE.contains(&l.as_str()) => LicenseClass::Permissive,
        Some(l) if !l.is_empty() && l != "unknown" => LicenseClass::Restricted,
        _ => LicenseClass::Unknown,
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct HubModel {
    pub id: String,
    pub publisher: String,
    pub downloads: u64,
    pub likes: u64,
    pub license: Option<String>,
    pub license_class: LicenseClass,
    pub gated: bool,
    pub last_modified: Option<String>,
}

#[derive(Deserialize)]
struct RawModel {
    id: String,
    #[serde(default)]
    downloads: u64,
    #[serde(default)]
    likes: u64,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    gated: serde_json::Value,
    #[serde(rename = "lastModified")]
    last_modified: Option<String>,
    #[serde(rename = "cardData")]
    card_data: Option<serde_json::Value>,
    sha: Option<String>,
}

impl RawModel {
    fn license(&self) -> Option<String> {
        self.card_data
            .as_ref()
            .and_then(|c| c.get("license"))
            .and_then(|l| l.as_str().map(String::from))
            .or_else(|| self.tags.iter().find_map(|t| t.strip_prefix("license:").map(String::from)))
    }

    fn into_model(self) -> HubModel {
        let license = self.license();
        HubModel {
            publisher: self.id.split('/').next().unwrap_or_default().to_string(),
            license_class: classify(license.as_deref()),
            license,
            gated: !matches!(self.gated, serde_json::Value::Bool(false) | serde_json::Value::Null),
            downloads: self.downloads,
            likes: self.likes,
            last_modified: self.last_modified,
            id: self.id,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum HubError {
    #[error("Hugging Face returned HTTP {0}")]
    Http(u16),
    #[error("could not reach Hugging Face: {0}")]
    Network(String),
    #[error("{0}")]
    Invalid(String),
}

fn client() -> Result<reqwest::blocking::Client, HubError> {
    reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .user_agent(concat!("OpenBark/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| HubError::Network(e.to_string()))
}

fn get_json<T: for<'de> Deserialize<'de>>(url: &str, token: Option<&str>) -> Result<T, HubError> {
    let mut req = client()?.get(url);
    if let Some(t) = token {
        req = req.bearer_auth(t);
    }
    let res = req.send().map_err(|e| HubError::Network(e.to_string()))?;
    if !res.status().is_success() {
        return Err(HubError::Http(res.status().as_u16()));
    }
    let text = res.text().map_err(|e| HubError::Network(e.to_string()))?;
    serde_json::from_str(&text).map_err(|e| HubError::Network(format!("unexpected response: {e}")))
}

/// Search GGUF models (runnable by the built-in llama.cpp engine), most
/// downloaded first.
pub fn search(query: &str, limit: usize, token: Option<&str>) -> Result<Vec<HubModel>, HubError> {
    let q: String = query.chars().filter(|c| c.is_ascii_alphanumeric() || "-_. /".contains(*c)).collect();
    let url = format!(
        "{API}?search={}&filter=gguf&sort=downloads&direction=-1&limit={}\
         &expand[]=downloads&expand[]=likes&expand[]=tags&expand[]=gated&expand[]=lastModified&expand[]=cardData",
        q.trim().replace(' ', "+"),
        limit.clamp(1, 50)
    );
    let raw: Vec<RawModel> = get_json(&url, token)?;
    Ok(raw.into_iter().map(RawModel::into_model).collect())
}

#[derive(Debug, Clone, Serialize)]
pub struct HubFile {
    pub path: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct HubRepo {
    pub model: HubModel,
    /// Commit the files are pinned to.
    pub revision: String,
    /// Single-file GGUF weights (multimodal projectors and split shards excluded).
    pub files: Vec<HubFile>,
}

#[derive(Deserialize)]
struct RawTreeEntry {
    #[serde(rename = "type")]
    kind: String,
    path: String,
    #[serde(default)]
    size: u64,
    lfs: Option<RawLfs>,
}

#[derive(Deserialize)]
struct RawLfs {
    oid: String,
    size: u64,
}

fn valid_repo(repo: &str) -> bool {
    let mut parts = repo.split('/');
    let ok = |p: Option<&str>| {
        p.is_some_and(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c)))
    };
    ok(parts.next()) && ok(parts.next()) && parts.next().is_none()
}

/// Details of one repo, with GGUF files pinned to the current commit.
pub fn repo(repo: &str, token: Option<&str>) -> Result<HubRepo, HubError> {
    if !valid_repo(repo) {
        return Err(HubError::Invalid(format!("'{repo}' is not a model id like publisher/name")));
    }
    let raw: RawModel = get_json(
        &format!(
            "{API}/{repo}?expand[]=downloads&expand[]=likes&expand[]=tags&expand[]=gated&expand[]=lastModified&expand[]=cardData&expand[]=sha"
        ),
        token,
    )?;
    let revision = raw.sha.clone().ok_or_else(|| HubError::Invalid("no revision".into()))?;
    let tree: Vec<RawTreeEntry> = get_json(&format!("{API}/{repo}/tree/{revision}"), token)?;
    let mut files: Vec<HubFile> = tree
        .into_iter()
        .filter(|e| e.kind == "file" && e.path.to_ascii_lowercase().ends_with(".gguf"))
        .filter(|e| !e.path.to_ascii_lowercase().contains("mmproj") && !e.path.contains("-of-0"))
        .filter_map(|e| e.lfs.map(|l| HubFile { path: e.path, size: l.size.max(e.size), sha256: l.oid }))
        .collect();
    files.sort_by_key(|f| f.size);
    Ok(HubRepo { model: raw.into_model(), revision, files })
}

/// The user's own catalog additions, stored next to their models.
pub fn user_catalog_path(store: &ModelStore) -> PathBuf {
    store.root().join("user-catalog.json")
}

pub fn load_user_catalog(store: &ModelStore) -> Vec<CatalogEntry> {
    std::fs::read_to_string(user_catalog_path(store))
        .ok()
        .and_then(|s| serde_json::from_str::<Catalog>(&s).ok())
        .map(|c| c.models)
        .unwrap_or_default()
}

/// Built-in catalog plus the user's additions.
pub fn full_catalog(store: &ModelStore) -> Catalog {
    let mut catalog = Catalog::builtin();
    for m in load_user_catalog(store) {
        if catalog.get(&m.id).is_none() {
            catalog.models.push(m);
        }
    }
    catalog
}

/// Add one GGUF file from a Hub repo to the user catalog (ready to download).
/// Restricted licences require `accepted_license`.
pub fn add_to_catalog(
    store: &ModelStore,
    repo: &HubRepo,
    file: &str,
    accepted_license: bool,
) -> Result<CatalogEntry, HubError> {
    if repo.model.license_class != LicenseClass::Permissive && !accepted_license {
        return Err(HubError::Invalid(format!(
            "{} uses the {} licence. Read it on Hugging Face and accept it to download.",
            repo.model.id,
            repo.model.license.as_deref().unwrap_or("unstated")
        )));
    }
    let f = repo
        .files
        .iter()
        .find(|f| f.path == file)
        .ok_or_else(|| HubError::Invalid(format!("{file} is not a GGUF file in {}", repo.model.id)))?;
    let id = format!("hf:{}/{}", repo.model.id, f.path.trim_end_matches(".gguf"));
    let entry = CatalogEntry {
        id: id.clone(),
        display_name: format!("{} ({})", repo.model.id, f.path.trim_end_matches(".gguf")),
        slot: Slot::Llm,
        license: repo.model.license.clone().unwrap_or_else(|| "unknown".into()),
        attribution: Some(format!("{} on Hugging Face", repo.model.id)),
        source: Source::Huggingface { repo: repo.model.id.clone(), revision: repo.revision.clone() },
        files: vec![FileSpec { path: f.path.clone(), size: f.size, sha256: f.sha256.clone(), url: None }],
        layout: Layout::LlmGguf { model: f.path.clone() },
    };
    let mut user = load_user_catalog(store);
    user.retain(|m| m.id != id);
    user.push(entry.clone());
    std::fs::create_dir_all(store.root()).map_err(|e| HubError::Invalid(e.to_string()))?;
    let json =
        serde_json::to_string_pretty(&Catalog { schema_version: 1, models: user }).expect("serialises");
    std::fs::write(user_catalog_path(store), json).map_err(|e| HubError::Invalid(e.to_string()))?;
    Ok(entry)
}

/// "Fits", "Tight" or "Won't fit" for a model of `bytes` on this machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Fit {
    Fits,
    Tight,
    WontFit,
}

/// GGUF weights plus ~25% for context and runtime. GPU memory if present
/// (`gpu_bytes`), otherwise system RAM, keeping room for the OS and speech models.
pub fn fit(model_bytes: u64, gpu_bytes: Option<u64>, ram_bytes: u64) -> Fit {
    let need = model_bytes as f64 * 1.25;
    let budget = match gpu_bytes {
        Some(v) if v > 0 => v as f64 * 0.9,
        _ => (ram_bytes as f64 - 4e9).max(0.0) * 0.7,
    };
    if need <= budget * 0.8 {
        Fit::Fits
    } else if need <= budget * 1.05 {
        Fit::Tight
    } else {
        Fit::WontFit
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn licences_are_classified() {
        assert_eq!(classify(Some("apache-2.0")), LicenseClass::Permissive);
        assert_eq!(classify(Some("MIT")), LicenseClass::Permissive);
        assert_eq!(classify(Some("gemma")), LicenseClass::Restricted);
        assert_eq!(classify(Some("cc-by-nc-4.0")), LicenseClass::Restricted);
        assert_eq!(classify(Some("health-ai-developer-foundations")), LicenseClass::Restricted);
        assert_eq!(classify(None), LicenseClass::Unknown);
    }

    #[test]
    fn repo_ids_are_validated() {
        assert!(valid_repo("google/gemma-4-E4B-it-qat-q4_0-gguf"));
        assert!(!valid_repo("google"));
        assert!(!valid_repo("../etc/passwd"));
        assert!(!valid_repo("a/b/c"));
    }

    #[test]
    fn fit_uses_gpu_memory_first() {
        let gb = 1_000_000_000u64;
        assert_eq!(fit(5 * gb, Some(16 * gb), 64 * gb), Fit::Fits);
        assert_eq!(fit(11 * gb, Some(16 * gb), 64 * gb), Fit::Tight);
        assert_eq!(fit(18 * gb, Some(16 * gb), 64 * gb), Fit::WontFit);
        assert_eq!(fit(5 * gb, None, 16 * gb), Fit::Fits);
        assert_eq!(fit(9 * gb, None, 16 * gb), Fit::WontFit);
    }

    #[test]
    fn restricted_licence_needs_acceptance() {
        let dir = std::env::temp_dir().join(format!("ob-hub-{}", std::process::id()));
        let store = ModelStore::new(&dir);
        let repo = HubRepo {
            model: HubModel {
                id: "someone/model-GGUF".into(),
                publisher: "someone".into(),
                downloads: 1,
                likes: 0,
                license: Some("gemma".into()),
                license_class: LicenseClass::Restricted,
                gated: false,
                last_modified: None,
            },
            revision: "a".repeat(40),
            files: vec![HubFile { path: "m.Q4_K_M.gguf".into(), size: 10, sha256: "b".repeat(64) }],
        };
        assert!(add_to_catalog(&store, &repo, "m.Q4_K_M.gguf", false).is_err());
        let entry = add_to_catalog(&store, &repo, "m.Q4_K_M.gguf", true).unwrap();
        assert_eq!(entry.id, "hf:someone/model-GGUF/m.Q4_K_M");
        assert!(full_catalog(&store).get(&entry.id).is_some());
        std::fs::remove_dir_all(&dir).ok();
    }
}
