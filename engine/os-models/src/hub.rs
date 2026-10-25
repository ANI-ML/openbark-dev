//! Hugging Face Hub browsing: search GGUF (and, on a Mac, MLX) models, list a
//! repo's files, classify licences, and add a chosen model to the user's
//! catalog for download (ADR-0025).
//!
//! Network (docs/network.md, R4), only when the user searches or opens a
//! model in the model manager, or saves a token:
//! - `huggingface.co/api/models...`: search (tokenless), a repo's metadata
//!   (tokenless), its recursive file list (paged; the token only for a gated
//!   repo), GGUF builds of a base model (tokenless);
//! - `huggingface.co/<repo>/resolve/<rev>/config.json` and
//!   `model.safetensors.index.json` for MLX repos (small, capped reads);
//! - `huggingface.co/api/whoami-v2` once, with the token, when it is saved.
//!
//! All fetching goes through the [`Fetch`] trait so tests run on saved live
//! responses without a network.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::catalog::{Catalog, CatalogEntry, FileSpec, Layout, Slot, Source};
use crate::platform::{self, Tier, TooBig};
use crate::store::ModelStore;

const API: &str = "https://huggingface.co/api/models";
/// The only origin a token is ever sent to.
const HF_ORIGIN: &str = "https://huggingface.co/";
/// `Link: rel="next"` pages are followed only under this prefix.
const PAGE_PREFIX: &str = "https://huggingface.co/api/models/";
const MAX_PAGES: usize = 20;
const META_EXPAND: &str = "expand[]=downloads&expand[]=likes&expand[]=tags&expand[]=gated&expand[]=lastModified&expand[]=cardData&expand[]=sha";
const LIST_EXPAND: &str =
    "expand[]=downloads&expand[]=likes&expand[]=tags&expand[]=gated&expand[]=lastModified&expand[]=cardData";
const JSON_CAP: usize = 8 << 20;
const CONFIG_CAP: usize = 1 << 20;
const INDEX_CAP: usize = 8 << 20;

/// `HubFile.path`-style choice that picks a repo's whole MLX model folder.
pub const MLX_CHOICE: &str = "@mlx";

/// Licences OpenBark lets you download with one click: permissive and
/// commercial-use friendly.
const PERMISSIVE: &[&str] =
    &["apache-2.0", "mit", "cc-by-4.0", "bsd-2-clause", "bsd-3-clause", "cc0-1.0", "unlicense", "isc"];

/// Publishers whose GGUF builds are listed first, after the base model's own.
const KNOWN_GGUF_PUBLISHERS: &[&str] = &["ggml-org", "unsloth", "bartowski", "lmstudio-community"];

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
    /// Converted for Apple's MLX framework (Apple silicon), e.g. the
    /// mlx-community builds. Shown, labelled, only on a Mac.
    pub mlx: bool,
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
    /// `expand[]=gguf`: the parameter count of a GGUF repo (phone profile).
    #[serde(default)]
    gguf: Option<serde_json::Value>,
}

impl RawModel {
    fn license(&self) -> Option<String> {
        self.card_data
            .as_ref()
            .and_then(|c| c.get("license"))
            .and_then(|l| l.as_str().map(String::from))
            .or_else(|| self.tags.iter().find_map(|t| t.strip_prefix("license:").map(String::from)))
    }

    /// `"auto"` or `"manual"` when gated.
    fn gated_kind(&self) -> Option<String> {
        match &self.gated {
            serde_json::Value::String(s) if !s.is_empty() && s != "false" => Some(s.clone()),
            serde_json::Value::Bool(true) => Some("auto".into()),
            _ => None,
        }
    }

    /// `cardData.base_model`, the first one if it's a list.
    fn base_model(&self) -> Option<String> {
        let v = self.card_data.as_ref()?.get("base_model")?;
        match v {
            serde_json::Value::String(s) => Some(s.clone()),
            serde_json::Value::Array(a) => a.iter().find_map(|x| x.as_str().map(String::from)),
            _ => None,
        }
        .filter(|s| valid_repo(s))
    }

    /// Parameters, from the Hub's GGUF summary.
    fn params(&self) -> Option<u64> {
        self.gguf.as_ref()?.get("total")?.as_u64().filter(|n| *n > 0)
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
            mlx: self.tags.iter().any(|t| t == "mlx") || self.id.starts_with("mlx-community/"),
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
    #[error("{0} wasn't found on Hugging Face, or it's private.")]
    NotFound(String),
    #[error("Hugging Face didn't accept that token. Copy it again from huggingface.co/settings/tokens.")]
    BadToken,
}

// ---------------------------------------------------------------------------
// Fetching

/// One HTTP answer, as much as the Hub code needs.
pub(crate) struct Fetched {
    pub status: u16,
    pub body: Vec<u8>,
    /// The `Link: <...>; rel="next"` URL, if any.
    pub next: Option<String>,
}

impl Fetched {
    fn ok(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

/// GET a URL, reading at most `cap` bytes of body. The real one is [`Http`];
/// tests feed saved responses.
pub(crate) trait Fetch {
    fn get(&self, url: &str, token: Option<&str>, cap: usize) -> Result<Fetched, HubError>;
}

/// The network, through reqwest.
pub(crate) struct Http;

impl Fetch for Http {
    fn get(&self, url: &str, token: Option<&str>, cap: usize) -> Result<Fetched, HubError> {
        use std::io::Read;
        let client = reqwest::blocking::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .user_agent(concat!("OpenBark/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| HubError::Network(e.to_string()))?;
        let mut req = client.get(url);
        // Never send a token anywhere but huggingface.co. Redirects to other
        // hosts drop it (reqwest's remove_sensitive_headers, see download.rs).
        if let Some(t) = token.filter(|t| !t.is_empty() && url.starts_with(HF_ORIGIN)) {
            req = req.bearer_auth(t);
        }
        let res = req.send().map_err(|e| HubError::Network(e.to_string()))?;
        let status = res.status().as_u16();
        let next = res
            .headers()
            .get_all(reqwest::header::LINK)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .find_map(next_link);
        let mut body = Vec::new();
        res.take(cap as u64 + 1).read_to_end(&mut body).map_err(|e| HubError::Network(e.to_string()))?;
        if body.len() > cap {
            return Err(HubError::Invalid(format!("Hugging Face sent more than expected for {url}.")));
        }
        Ok(Fetched { status, body, next })
    }
}

/// The `rel="next"` URL of a `Link` header.
pub(crate) fn next_link(header: &str) -> Option<String> {
    header.split(',').find_map(|part| {
        let mut pieces = part.split(';');
        let url = pieces.next()?.trim().strip_prefix('<')?.strip_suffix('>')?;
        pieces
            .any(|p| {
                let p = p.trim().to_ascii_lowercase();
                p == "rel=\"next\"" || p == "rel=next"
            })
            .then(|| url.to_string())
    })
}

fn parse<T: for<'de> Deserialize<'de>>(f: &Fetched, what: &str) -> Result<T, HubError> {
    serde_json::from_slice(&f.body)
        .map_err(|e| HubError::Network(format!("unexpected {what} from Hugging Face: {e}")))
}

fn get_json<T: for<'de> Deserialize<'de>>(
    f: &dyn Fetch,
    url: &str,
    token: Option<&str>,
) -> Result<T, HubError> {
    let res = f.get(url, token, JSON_CAP)?;
    if !res.ok() {
        return Err(HubError::Http(res.status));
    }
    parse(&res, "response")
}

// ---------------------------------------------------------------------------
// Search

/// Search GGUF models (runnable by the built-in llama.cpp engine), most
/// downloaded first. With `mlx` (on a Mac), MLX builds are included too,
/// marked [`HubModel::mlx`]. Always tokenless (R4): `token` is ignored.
pub fn search(query: &str, limit: usize, token: Option<&str>, mlx: bool) -> Result<Vec<HubModel>, HubError> {
    let _ = token;
    search_with(&Http, query, limit, mlx)
}

fn search_with(f: &dyn Fetch, query: &str, limit: usize, mlx: bool) -> Result<Vec<HubModel>, HubError> {
    let q: String = query.chars().filter(|c| c.is_ascii_alphanumeric() || "-_. /".contains(*c)).collect();
    let limit = limit.clamp(1, 50);
    let url = |filter: &str| {
        format!(
            "{API}?search={}&filter={filter}&sort=downloads&direction=-1&limit={limit}&{LIST_EXPAND}",
            q.trim().replace(' ', "+"),
        )
    };
    let mut raw: Vec<RawModel> = get_json(f, &url("gguf"), None)?;
    if mlx {
        raw.extend(get_json::<Vec<RawModel>>(f, &url("mlx"), None)?);
    }
    Ok(merge(raw.into_iter().map(RawModel::into_model).collect(), limit))
}

/// Search on a phone (ADR-0018 section 10): GGUF only, never MLX, and only
/// repos whose names don't put them over the tier's parameter cap. A name
/// without a size stays; its files are checked when the repo is opened.
/// `None` (a phone under 6 GB): nothing.
pub fn search_mobile(query: &str, limit: usize, tier: Option<Tier>) -> Result<Vec<HubModel>, HubError> {
    search_mobile_with(&Http, query, limit, tier)
}

fn search_mobile_with(
    f: &dyn Fetch,
    query: &str,
    limit: usize,
    tier: Option<Tier>,
) -> Result<Vec<HubModel>, HubError> {
    let Some(tier) = tier else { return Ok(Vec::new()) };
    let mut found = search_with(f, query, limit, false)?;
    found.retain(|m| !m.mlx && platform::params_from_name(&m.id).is_none_or(|p| p <= tier.max_params_b));
    Ok(found)
}

/// One list, most downloaded first, without duplicates.
fn merge(mut models: Vec<HubModel>, limit: usize) -> Vec<HubModel> {
    models.sort_by_key(|m| std::cmp::Reverse(m.downloads));
    let mut seen = HashSet::new();
    models.retain(|m| seen.insert(m.id.clone()));
    models.truncate(limit);
    models
}

/// GGUF builds of `base_model` (`filter=base_model:quantized:<base>&filter=gguf`,
/// sort by downloads), at most `limit` (clamped 1..=10). The base model's own
/// publisher first, then ggml-org, unsloth, bartowski, lmstudio-community, then
/// the rest by downloads. Tokenless.
pub fn gguf_alternatives(base_model: &str, limit: usize) -> Result<Vec<HubModel>, HubError> {
    gguf_alternatives_with(&Http, base_model, limit)
}

fn gguf_alternatives_with(f: &dyn Fetch, base_model: &str, limit: usize) -> Result<Vec<HubModel>, HubError> {
    if !valid_repo(base_model) {
        return Err(HubError::Invalid(format!("'{base_model}' is not a model id like publisher/name")));
    }
    let url = format!(
        "{API}?filter=base_model:quantized:{base_model}&filter=gguf&sort=downloads&direction=-1&limit=50&{LIST_EXPAND}"
    );
    let raw: Vec<RawModel> = get_json(f, &url, None)?;
    let own = base_model.split('/').next().unwrap_or_default().to_ascii_lowercase();
    let tier = |m: &HubModel| {
        let p = m.publisher.to_ascii_lowercase();
        if p == own {
            0
        } else {
            KNOWN_GGUF_PUBLISHERS
                .iter()
                .position(|k| *k == p)
                .map_or(KNOWN_GGUF_PUBLISHERS.len() + 1, |i| i + 1)
        }
    };
    let mut models = merge(raw.into_iter().map(RawModel::into_model).collect(), 50);
    models.sort_by_key(|m| (tier(m), std::cmp::Reverse(m.downloads)));
    models.truncate(limit.clamp(1, 10));
    Ok(models)
}

/// `GET https://huggingface.co/api/whoami-v2` with the token. Ok(name) or BadToken.
pub fn whoami(token: &str) -> Result<String, HubError> {
    whoami_with(&Http, token)
}

fn whoami_with(f: &dyn Fetch, token: &str) -> Result<String, HubError> {
    #[derive(Deserialize)]
    struct Who {
        name: Option<String>,
    }
    let res = f.get("https://huggingface.co/api/whoami-v2", Some(token), JSON_CAP)?;
    match res.status {
        200..=299 => parse::<Who>(&res, "account details")?
            .name
            .filter(|n| !n.is_empty())
            .ok_or_else(|| HubError::Network("Hugging Face didn't say whose token this is".into())),
        401 | 403 => Err(HubError::BadToken),
        s => Err(HubError::Http(s)),
    }
}

// ---------------------------------------------------------------------------
// One repo

/// Who can download a repo's files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Access {
    Open,
    /// Gated, hashes hidden, no token saved.
    NeedsToken,
    /// Gated, hashes hidden even with the saved token.
    NoAccessYet,
}

/// One file to download.
#[derive(Debug, Clone, Serialize)]
pub struct HubPart {
    pub path: String,
    pub size: u64,
    /// LFS files: the SHA-256 the Hub reports. `None` when masked.
    pub sha256: Option<String>,
    /// Plain git files: their blob SHA-1.
    pub git_sha1: Option<String>,
}

impl HubPart {
    fn has_hash(&self) -> bool {
        self.sha256.is_some() || self.git_sha1.is_some()
    }
}

/// A GGUF choice: one file, or a complete set of split parts.
#[derive(Debug, Clone, Serialize)]
pub struct HubFile {
    /// The single file, or part 1 of a split GGUF.
    pub path: String,
    /// Sum of all parts.
    pub size: u64,
    /// `None` = masked (gated without access).
    pub sha256: Option<String>,
    /// Every file to download; `parts[0].path == path`.
    pub parts: Vec<HubPart>,
}

/// Why an MLX repo can't run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MlxProblem {
    NoConfig,
    /// config.json didn't parse or has no model_type.
    BadConfig,
    NoWeights,
    IncompleteShards {
        missing: Vec<String>,
    },
    NoTokenizer,
    /// Gated, no access yet.
    HashesHidden,
}

/// The MLX model folder of a repo, as one choice.
#[derive(Debug, Clone, Serialize)]
pub struct HubMlx {
    /// The allow-listed root files (ADR-0025 §5).
    pub files: Vec<HubPart>,
    pub size: u64,
    /// From config.json.
    pub model_type: Option<String>,
    /// e.g. "4-bit", from config.json `quantization.bits`.
    pub quantization: Option<String>,
    /// `None` = runnable (the architecture is checked by the app).
    pub problem: Option<MlxProblem>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HubRepo {
    pub model: HubModel,
    /// Commit the files are pinned to.
    pub revision: String,
    /// GGUF choices: singles and complete shard groups, smallest first.
    pub files: Vec<HubFile>,
    /// Some only when `model.mlx`.
    pub mlx: Option<HubMlx>,
    pub access: Access,
    /// "auto" | "manual" | None.
    pub gated_kind: Option<String>,
    /// cardData.base_model (the first if a list).
    pub base_model: Option<String>,
    /// Parameters, from the Hub's GGUF summary. Asked for on a phone only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<u64>,
}

#[derive(Deserialize, Clone, Debug)]
struct RawTreeEntry {
    #[serde(rename = "type")]
    kind: String,
    path: String,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    oid: Option<String>,
    lfs: Option<RawLfs>,
}

#[derive(Deserialize, Clone, Debug)]
struct RawLfs {
    oid: String,
    #[serde(default)]
    size: u64,
}

impl RawTreeEntry {
    fn is_file(&self) -> bool {
        self.kind == "file"
    }

    fn name(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or(&self.path)
    }

    fn masked(&self) -> bool {
        self.lfs.as_ref().is_some_and(|l| real_sha256(&l.oid).is_none())
    }

    fn part(&self) -> HubPart {
        match &self.lfs {
            Some(l) => HubPart {
                path: self.path.clone(),
                size: l.size.max(self.size),
                sha256: real_sha256(&l.oid),
                git_sha1: None,
            },
            None => HubPart {
                path: self.path.clone(),
                size: self.size,
                sha256: None,
                git_sha1: self.oid.as_deref().and_then(real_sha1),
            },
        }
    }
}

fn is_hex(s: &str, len: usize) -> bool {
    s.len() == len && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// A hash that isn't exactly 64 hex characters is masked (gated, no access).
fn real_sha256(s: &str) -> Option<String> {
    is_hex(s, 64).then(|| s.to_ascii_lowercase())
}

fn real_sha1(s: &str) -> Option<String> {
    is_hex(s, 40).then(|| s.to_ascii_lowercase())
}

fn valid_repo(repo: &str) -> bool {
    let mut parts = repo.split('/');
    let ok = |p: Option<&str>| {
        p.is_some_and(|p| {
            !p.is_empty()
                && p != "."
                && p != ".."
                && p.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
        })
    };
    ok(parts.next()) && ok(parts.next()) && parts.next().is_none()
}

/// A split-GGUF part: `<prefix>-00001-of-00003.gguf`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shard {
    pub prefix: String,
    pub index: u32,
    pub count: u32,
}

/// `"<prefix>-00001-of-00003.gguf"` -> prefix, 1, 3. Case-insensitive extension.
pub fn parse_shard(file_name: &str) -> Option<Shard> {
    parse_shard_ext(file_name, ".gguf")
}

fn parse_shard_ext(file_name: &str, ext: &str) -> Option<Shard> {
    if file_name.len() <= ext.len() || !file_name[file_name.len() - ext.len()..].eq_ignore_ascii_case(ext) {
        return None;
    }
    let stem = &file_name[..file_name.len() - ext.len()];
    let (head, count) = stem.rsplit_once("-of-")?;
    let (prefix, index) = head.rsplit_once('-')?;
    let digits = |s: &str| !s.is_empty() && s.len() <= 6 && s.bytes().all(|b| b.is_ascii_digit());
    if prefix.is_empty() || !digits(index) || !digits(count) {
        return None;
    }
    let (index, count): (u32, u32) = (index.parse().ok()?, count.parse().ok()?);
    (index >= 1 && count >= 1 && index <= count).then(|| Shard { prefix: prefix.to_string(), index, count })
}

/// GGUF files that aren't a model on their own: vision projectors (mmproj),
/// multi-token-prediction drafts (`mtp-`, anything under `MTP/`), importance
/// matrices, vocab-only files, draft models, LoRA adapters. Case-insensitive.
///
/// Draft, LoRA and adapter files are recognised by whole words of the file
/// name (split on `-`, `_`, `.` and spaces), and folders only by their exact
/// name, so full models such as `...-lorablated-Q4_K_M.gguf`, `Explora-7B` or
/// a `Floral-v1` folder aren't mistaken for helpers.
pub fn is_helper_gguf(path: &str) -> bool {
    const HELPER_FOLDERS: [&str; 6] = ["mtp", "mmproj", "lora", "loras", "adapter", "adapters"];
    const HELPER_WORDS: [&str; 4] = ["draft", "lora", "adapter", "mmproj"];
    let lower = path.to_ascii_lowercase();
    let mut parts: Vec<&str> = lower.split('/').collect();
    let name = parts.pop().unwrap_or_default();
    let stem = name.strip_suffix(".gguf").unwrap_or(name);
    parts.iter().any(|p| HELPER_FOLDERS.contains(p))
        || name.starts_with("mtp-")
        || name.starts_with("imatrix")
        || name.starts_with("ggml-vocab")
        || name.contains("mmproj")
        || stem.split(['-', '_', '.', ' ']).any(|w| HELPER_WORDS.contains(&w))
}

/// The ADR-0025 §5 allow-list for an MLX model folder: root files only, never
/// Python (remote code), sub-folders or other weight formats.
pub fn mlx_allowed(path: &str) -> bool {
    const EXACT: &[&str] = &[
        "config.json",
        "generation_config.json",
        "model.safetensors.index.json",
        "tokenizer.json",
        "tokenizer_config.json",
        "special_tokens_map.json",
        "added_tokens.json",
        "vocab.json",
        "merges.txt",
        "vocab.txt",
        "tokenizer.model",
        "chat_template.jinja",
        "chat_template.json",
        "processor_config.json",
        "preprocessor_config.json",
    ];
    if path.is_empty() || path.contains(['/', '\\', ':', '\0']) || path.starts_with('.') {
        return false;
    }
    if path.to_ascii_lowercase().ends_with(".py") {
        return false;
    }
    let weights = |prefix: &str| path.starts_with(prefix) && path.ends_with(".safetensors");
    EXACT.contains(&path)
        || weights("model")
        || weights("weights")
        || (path.ends_with(".tiktoken") && path.len() > ".tiktoken".len())
        || path.to_ascii_uppercase().starts_with("LICENSE")
}

pub(crate) fn is_tokenizer(path: &str) -> bool {
    matches!(path, "tokenizer.json" | "tokenizer.model" | "vocab.json" | "vocab.txt")
        || path.ends_with(".tiktoken")
}

/// Details of one repo, with files pinned to the current commit.
///
/// Metadata first, without a token. If the repo is gated and `token` is Some,
/// the tree (and config.json/index for MLX) are fetched with it.
pub fn repo(repo: &str, token: Option<&str>) -> Result<HubRepo, HubError> {
    repo_with(&Http, repo, token)
}

/// [`repo`] on a phone: the Hub's GGUF summary is asked for too, for the
/// parameter check, and MLX is never offered (ADR-0018 section 10).
pub fn repo_mobile(repo: &str, token: Option<&str>) -> Result<HubRepo, HubError> {
    repo_mobile_with(&Http, repo, token)
}

fn repo_with(f: &dyn Fetch, repo: &str, token: Option<&str>) -> Result<HubRepo, HubError> {
    repo_inner(f, repo, token, false)
}

fn repo_mobile_with(f: &dyn Fetch, repo: &str, token: Option<&str>) -> Result<HubRepo, HubError> {
    let mut r = repo_inner(f, repo, token, true)?;
    r.mlx = None;
    r.model.mlx = false;
    Ok(r)
}

fn repo_inner(f: &dyn Fetch, repo: &str, token: Option<&str>, mobile: bool) -> Result<HubRepo, HubError> {
    if !valid_repo(repo) {
        return Err(HubError::Invalid(format!("'{repo}' is not a model id like publisher/name")));
    }
    let expand = if mobile { format!("{META_EXPAND}&expand[]=gguf") } else { META_EXPAND.to_string() };
    let meta = f.get(&format!("{API}/{repo}?{expand}"), None, JSON_CAP)?;
    match meta.status {
        200..=299 => {}
        401 | 404 => return Err(HubError::NotFound(repo.to_string())),
        s => return Err(HubError::Http(s)),
    }
    let raw: RawModel = parse(&meta, "model details")?;
    let revision =
        raw.sha.clone().filter(|s| is_hex(s, 40)).ok_or_else(|| HubError::Invalid("no revision".into()))?;
    let gated_kind = raw.gated_kind();
    let base_model = raw.base_model();
    let params = raw.params();
    let token = token.filter(|t| gated_kind.is_some() && !t.is_empty());

    let tree = tree_with(f, repo, &revision, token)?;
    let masked = tree.iter().any(|e| e.is_file() && e.masked());
    let access = match (gated_kind.is_some() && masked, token.is_some()) {
        (false, _) => Access::Open,
        (true, false) => Access::NeedsToken,
        (true, true) => Access::NoAccessYet,
    };
    let files = gguf_choices(&tree);
    let model = raw.into_model();
    // A phone never reads an MLX repo's config: MLX can't run there.
    let mlx = if model.mlx && !mobile {
        let listed = |name: &str| tree.iter().any(|e| e.is_file() && e.path == name);
        let readable = access == Access::Open;
        let resolve = |name: &str, cap: usize| -> Result<Option<Vec<u8>>, HubError> {
            if !readable || !listed(name) {
                return Ok(None);
            }
            let res = f.get(&format!("{HF_ORIGIN}{repo}/resolve/{revision}/{name}"), token, cap)?;
            Ok(res.ok().then_some(res.body))
        };
        let config = resolve("config.json", CONFIG_CAP)?;
        let index = resolve("model.safetensors.index.json", INDEX_CAP)?;
        Some(mlx_choice(&tree, config.as_deref(), index.as_deref(), access != Access::Open))
    } else {
        None
    };
    Ok(HubRepo { model, revision, files, mlx, access, gated_kind, base_model, params })
}

/// The repo's GGUF choices on a phone of `tier`: those offered, and those
/// held back with the reason (ADR-0018 section 10).
pub fn mobile_files(repo: &HubRepo, tier: Option<Tier>) -> (Vec<HubFile>, Vec<(HubFile, TooBig)>) {
    let params = repo.params.map(|n| n as f64 / 1e9).or_else(|| platform::params_from_name(&repo.model.id));
    let mut kept = Vec::new();
    let mut held = Vec::new();
    for f in &repo.files {
        let p = platform::params_from_name(&f.path).or(params);
        match platform::check_gguf(tier, &f.path, f.size, p) {
            Ok(()) => kept.push(f.clone()),
            Err(why) => held.push((f.clone(), why)),
        }
    }
    (kept, held)
}

/// `{API}/{repo}/tree/{rev}?recursive=true&expand=false`, every page.
fn tree_with(
    f: &dyn Fetch,
    repo: &str,
    revision: &str,
    token: Option<&str>,
) -> Result<Vec<RawTreeEntry>, HubError> {
    let mut url = format!("{API}/{repo}/tree/{revision}?recursive=true&expand=false");
    let mut entries = Vec::new();
    for page in 0.. {
        let res = f.get(&url, token, JSON_CAP)?;
        match res.status {
            200..=299 => {}
            401 | 403 if token.is_some() => return Err(HubError::BadToken),
            401 | 404 => return Err(HubError::NotFound(repo.to_string())),
            s => return Err(HubError::Http(s)),
        }
        entries.extend(parse::<Vec<RawTreeEntry>>(&res, "file list")?);
        match res.next {
            Some(next) if next.starts_with(PAGE_PREFIX) && page + 1 < MAX_PAGES => url = next,
            Some(next) if next.starts_with(PAGE_PREFIX) => {
                tracing::warn!(repo, pages = MAX_PAGES, "file list cut short");
                break;
            }
            Some(_) => {
                tracing::warn!(repo, "ignored a file-list page outside the Hub API");
                break;
            }
            None => break,
        }
    }
    Ok(entries)
}

/// Singles and complete shard groups, helpers left out, smallest first.
fn gguf_choices(tree: &[RawTreeEntry]) -> Vec<HubFile> {
    let ggufs = tree.iter().filter(|e| {
        e.is_file() && e.path.to_ascii_lowercase().ends_with(".gguf") && !is_helper_gguf(&e.path)
    });
    let mut files = Vec::new();
    let mut groups: BTreeMap<(String, String, u32), BTreeMap<u32, HubPart>> = BTreeMap::new();
    let mut broken: HashSet<(String, String, u32)> = HashSet::new();
    for e in ggufs {
        match parse_shard(e.name()) {
            Some(s) => {
                let folder = e.path.rsplit_once('/').map(|(d, _)| d.to_string()).unwrap_or_default();
                let key = (folder, s.prefix, s.count);
                if groups.entry(key.clone()).or_default().insert(s.index, e.part()).is_some() {
                    broken.insert(key);
                }
            }
            None => {
                let part = e.part();
                files.push(HubFile {
                    path: part.path.clone(),
                    size: part.size,
                    sha256: part.sha256.clone(),
                    parts: vec![part],
                });
            }
        }
    }
    for (key, parts) in groups {
        let count = key.2;
        if broken.contains(&key)
            || parts.len() as u32 != count
            || !(1..=count).all(|i| parts.contains_key(&i))
        {
            continue;
        }
        let parts: Vec<HubPart> = parts.into_values().collect();
        let sha256 = if parts.iter().all(|p| p.sha256.is_some()) { parts[0].sha256.clone() } else { None };
        files.push(HubFile {
            path: parts[0].path.clone(),
            size: parts.iter().map(|p| p.size).sum(),
            sha256,
            parts,
        });
    }
    files.sort_by(|a, b| a.size.cmp(&b.size).then_with(|| a.path.cmp(&b.path)));
    files
}

/// The MLX folder of a repo: allow-listed root files, and why it can't run.
fn mlx_choice(tree: &[RawTreeEntry], config: Option<&[u8]>, index: Option<&[u8]>, hidden: bool) -> HubMlx {
    let picked: Vec<&RawTreeEntry> = tree.iter().filter(|e| e.is_file() && mlx_allowed(&e.path)).collect();
    let files: Vec<HubPart> = picked.iter().map(|e| e.part()).collect();
    let size = files.iter().map(|p| p.size).sum();
    let names: HashSet<&str> = files.iter().map(|p| p.path.as_str()).collect();

    let config: Option<serde_json::Value> = config.and_then(|c| serde_json::from_slice(c).ok());
    let model_type = config
        .as_ref()
        .and_then(|c| c.get("model_type").and_then(|v| v.as_str()))
        .filter(|s| !s.is_empty())
        .map(String::from);
    let quantization = config.as_ref().and_then(|c| {
        ["quantization", "quantization_config"]
            .iter()
            .find_map(|k| c.get(*k).and_then(|q| q.get("bits")).and_then(|b| b.as_u64()))
            .map(|bits| format!("{bits}-bit"))
    });

    let weights: Vec<&str> = names.iter().copied().filter(|n| n.ends_with(".safetensors")).collect();
    let problem = if hidden || picked.iter().any(|e| e.masked()) {
        Some(MlxProblem::HashesHidden)
    } else if !names.contains("config.json") {
        Some(MlxProblem::NoConfig)
    } else if model_type.is_none() {
        Some(MlxProblem::BadConfig)
    } else if weights.is_empty() {
        Some(MlxProblem::NoWeights)
    } else if let Some(missing) = missing_weights(&names, &weights, index) {
        Some(MlxProblem::IncompleteShards { missing })
    } else if !names.iter().any(|n| is_tokenizer(n)) {
        Some(MlxProblem::NoTokenizer)
    } else {
        None
    };
    let mut files = files;
    files.sort_by(|a, b| a.path.cmp(&b.path));
    HubMlx { files, size, model_type, quantization, problem }
}

/// Weight files the index names that aren't selected, or the missing parts of
/// a `-NNNNN-of-NNNNN.safetensors` set. `None` when complete.
pub(crate) fn missing_weights(
    names: &HashSet<&str>,
    weights: &[&str],
    index: Option<&[u8]>,
) -> Option<Vec<String>> {
    let indexed: Option<Vec<String>> = index
        .and_then(|i| serde_json::from_slice::<serde_json::Value>(i).ok())
        .and_then(|v| v.get("weight_map").and_then(|m| m.as_object()).cloned())
        .map(|m| {
            let mut files: Vec<String> = m.values().filter_map(|v| v.as_str().map(String::from)).collect();
            files.sort();
            files.dedup();
            files
        });
    let mut missing: Vec<String> = match indexed {
        Some(files) => files.into_iter().filter(|f| !names.contains(f.as_str())).collect(),
        None => {
            let mut sets: HashMap<(String, u32), HashSet<u32>> = HashMap::new();
            for w in weights {
                if let Some(s) = parse_shard_ext(w, ".safetensors") {
                    sets.entry((s.prefix, s.count)).or_default().insert(s.index);
                }
            }
            sets.into_iter()
                .flat_map(|((prefix, count), have)| {
                    (1..=count)
                        .filter(move |i| !have.contains(i))
                        .map(move |i| format!("{prefix}-{i:05}-of-{count:05}.safetensors"))
                })
                .collect()
        }
    };
    missing.sort();
    (!missing.is_empty()).then_some(missing)
}

// ---------------------------------------------------------------------------
// The user catalog

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

/// The user catalog for changing it: refuses to go on (and so to overwrite
/// it) when the file is there but can't be read.
fn read_user_catalog(store: &ModelStore) -> Result<Vec<CatalogEntry>, HubError> {
    let path = user_catalog_path(store);
    match std::fs::read_to_string(&path) {
        Ok(s) => serde_json::from_str::<Catalog>(&s).map(|c| c.models).map_err(|e| {
            HubError::Invalid(format!("{} can't be read ({e}), so OpenBark won't change it.", path.display()))
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(HubError::Invalid(format!("{} can't be read: {e}", path.display()))),
    }
}

fn write_user_catalog(store: &ModelStore, models: Vec<CatalogEntry>) -> Result<(), HubError> {
    let io = |e: std::io::Error| HubError::Invalid(format!("couldn't save the model list: {e}"));
    std::fs::create_dir_all(store.root()).map_err(io)?;
    let path = user_catalog_path(store);
    let tmp = path.with_extension("json.tmp");
    let json =
        serde_json::to_string_pretty(&Catalog { schema_version: 1, models }).expect("catalog serialises");
    std::fs::write(&tmp, json).map_err(io)?;
    std::fs::rename(&tmp, &path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        io(e)
    })
}

/// Add or replace (same id) an entry in user-catalog.json (atomic write: temp file + rename).
pub fn save_user_entry(store: &ModelStore, entry: CatalogEntry) -> Result<(), HubError> {
    let mut user = read_user_catalog(store)?;
    user.retain(|m| m.id != entry.id);
    user.push(entry);
    write_user_catalog(store, user)
}

/// Drop an entry from user-catalog.json. Ok(false) if it wasn't there. Never touches model files.
pub fn remove_user_entry(store: &ModelStore, id: &str) -> Result<bool, HubError> {
    let mut user = read_user_catalog(store)?;
    let before = user.len();
    user.retain(|m| m.id != id);
    if user.len() == before {
        return Ok(false);
    }
    write_user_catalog(store, user)?;
    Ok(true)
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

fn file_spec(p: &HubPart) -> FileSpec {
    FileSpec {
        path: p.path.clone(),
        size: p.size,
        sha256: p.sha256.clone().unwrap_or_default(),
        git_sha1: if p.sha256.is_some() { None } else { p.git_sha1.clone() },
        url: None,
    }
}

fn strip_gguf(s: &str) -> &str {
    if s.len() > 5 && s[s.len() - 5..].eq_ignore_ascii_case(".gguf") { &s[..s.len() - 5] } else { s }
}

/// MLX on a phone (ADR-0018 section 10).
pub const MLX_NOT_ON_PHONE: &str =
    "MLX models run only on a Mac. On an iPhone, OpenBark uses small GGUF models.";

const HIDDEN_HASHES: &str =
    "Hugging Face is hiding this model's file checksums. Request access on its page, then add your token.";

/// Add a model from a Hub repo to the user catalog (ready to download).
/// `choice` is a GGUF [`HubFile::path`] or [`MLX_CHOICE`]. Refuses: a
/// restricted licence without `accepted_license`; any part with no hash; an
/// MLX problem. Ids:
/// - single GGUF: `hf:<repo>/<file path without .gguf>`
/// - split GGUF: `hf:<repo>/<prefix basename>` (no folder, no `-0000k-of-0000N`)
/// - MLX: `hf:<repo>/@mlx`
pub fn add_to_catalog(
    store: &ModelStore,
    repo: &HubRepo,
    choice: &str,
    accepted_license: bool,
) -> Result<CatalogEntry, HubError> {
    add_to_catalog_for(store, repo, choice, accepted_license, None)
}

/// [`add_to_catalog`], on a phone when `phone` is `Some(tier)`: MLX is
/// refused, the file must pass the tier's checks, and the entry is marked
/// `mobile` (ADR-0018 section 10).
pub fn add_to_catalog_for(
    store: &ModelStore,
    repo: &HubRepo,
    choice: &str,
    accepted_license: bool,
    phone: Option<Option<Tier>>,
) -> Result<CatalogEntry, HubError> {
    if let Some(tier) = phone {
        if choice == MLX_CHOICE {
            return Err(HubError::Invalid(MLX_NOT_ON_PHONE.into()));
        }
        let (_, held) = mobile_files(repo, tier);
        if let Some((_, why)) = held.iter().find(|(f, _)| f.path == choice) {
            return Err(HubError::Invalid(why.message(tier.map(|t| t.ram_gb))));
        }
    }
    if repo.model.license_class != LicenseClass::Permissive && !accepted_license {
        return Err(HubError::Invalid(format!(
            "{} uses the {} licence. Read it on Hugging Face and accept it to download.",
            repo.model.id,
            repo.model.license.as_deref().unwrap_or("unstated")
        )));
    }
    let name = &repo.model.id;
    let (id, display_name, parts, layout) = if choice == MLX_CHOICE {
        let mlx = repo
            .mlx
            .as_ref()
            .ok_or_else(|| HubError::Invalid(format!("{name} isn't an MLX model repository.")))?;
        match &mlx.problem {
            None => {}
            Some(MlxProblem::HashesHidden) => return Err(HubError::Invalid(HIDDEN_HASHES.into())),
            Some(p) => {
                return Err(HubError::Invalid(format!(
                    "{name} can't run with MLX ({}).",
                    mlx_problem_text(p)
                )));
            }
        }
        (
            format!("hf:{name}/{MLX_CHOICE}"),
            format!("{name} (MLX)"),
            mlx.files.clone(),
            Layout::LlmMlx { config: "config.json".into() },
        )
    } else {
        let f = repo
            .files
            .iter()
            .find(|f| f.path == choice)
            .ok_or_else(|| HubError::Invalid(format!("{choice} is not a GGUF file in {name}")))?;
        let (id_tail, label) = if f.parts.len() > 1 {
            let file_name = f.path.rsplit('/').next().unwrap_or(&f.path);
            let prefix =
                parse_shard(file_name).map(|s| s.prefix).unwrap_or_else(|| strip_gguf(file_name).to_string());
            let label = format!("{prefix}, {} parts", f.parts.len());
            (prefix, label)
        } else {
            let tail = strip_gguf(&f.path).to_string();
            (tail.clone(), tail)
        };
        (
            format!("hf:{name}/{id_tail}"),
            format!("{name} ({label})"),
            f.parts.clone(),
            Layout::LlmGguf { model: f.path.clone() },
        )
    };
    if parts.is_empty() || parts.iter().any(|p| !p.has_hash()) {
        return Err(HubError::Invalid(HIDDEN_HASHES.into()));
    }
    let entry = CatalogEntry {
        id,
        display_name,
        slot: Slot::Llm,
        license: repo.model.license.clone().unwrap_or_else(|| "unknown".into()),
        attribution: Some(format!("{name} on Hugging Face")),
        source: Source::Huggingface { repo: name.clone(), revision: repo.revision.clone() },
        files: parts.iter().map(file_spec).collect(),
        layout,
        gated: repo.gated_kind.is_some(),
        mobile: phone.is_some(),
    };
    // The same model at another revision: its folder is the same, so files
    // (or stopped downloads) the new listing doesn't keep would be left
    // behind, uncounted, or resumed with the new revision's bytes.
    if let Some(old) = load_user_catalog(store).into_iter().find(|m| m.id == entry.id)
        && !stale_files(store, &old, &entry).is_empty()
    {
        return Err(HubError::Invalid(OLDER_ON_DISK.into()));
    }
    save_user_entry(store, entry.clone())?;
    Ok(entry)
}

const OLDER_ON_DISK: &str =
    "An older version of this model is on this computer. Remove it on the Models screen, then add it again.";

/// Files of `old` on disk (whole or as a stopped `.part` download) that
/// `new` doesn't list with the same size and hash: they'd be orphaned, or a
/// stopped download would resume with another revision's bytes.
fn stale_files(store: &ModelStore, old: &CatalogEntry, new: &CatalogEntry) -> Vec<String> {
    if old.in_place().is_some() {
        return Vec::new();
    }
    let same = |a: &FileSpec, b: &FileSpec| {
        a.path == b.path && a.size == b.size && a.sha256 == b.sha256 && a.git_sha1 == b.git_sha1
    };
    old.files
        .iter()
        .filter(|f| !new.files.iter().any(|n| same(f, n)))
        .filter(|f| {
            let path = store.file_path(old, &f.path);
            path.exists() || crate::store::part_path(&path).exists()
        })
        .map(|f| f.path.clone())
        .collect()
}

fn mlx_problem_text(p: &MlxProblem) -> String {
    match p {
        MlxProblem::NoConfig => "no config.json".into(),
        MlxProblem::BadConfig => "config.json can't be read".into(),
        MlxProblem::NoWeights => "no safetensors weights".into(),
        MlxProblem::IncompleteShards { missing } => format!("missing {}", missing.join(", ")),
        MlxProblem::NoTokenizer => "no tokenizer files".into(),
        MlxProblem::HashesHidden => "checksums hidden".into(),
    }
}

// ---------------------------------------------------------------------------
// Fit

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
    use std::cell::RefCell;
    use std::path::Path;

    const Q235: &str = "unsloth/Qwen3-235B-A22B-Instruct-2507-GGUF";
    const Q235_REV: &str = "437d6915c5c512869e4cf8b16b840bbe3fb172bc";
    const GEMMA4: &str = "unsloth/gemma-4-E4B-it-GGUF";
    const GEMMA4_REV: &str = "bfc15c382204943c3a8fff0c750b94ae2364d7a3";
    const GATED: &str = "google/gemma-3-1b-it-qat-q4_0-gguf";
    const GATED_REV: &str = "d1be121d36172a4b0b964657e2ee859d61138593";
    const QWEN_MLX: &str = "mlx-community/Qwen3-0.6B-4bit";
    const QWEN_MLX_REV: &str = "73e3e38d981303bc594367cd910ea6eb48349da8";
    const GEMMA_MLX: &str = "lmstudio-community/gemma-4-E4B-it-MLX-4bit";
    const GEMMA_MLX_REV: &str = "ce713a0013bb7090305edfb8e3773a07ccae35c1";

    fn fixture(name: &str) -> Vec<u8> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/hub").join(name);
        std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
    }

    /// Saved live responses by URL. Every request is recorded with whether
    /// it carried a token.
    #[derive(Default)]
    struct Fixtures {
        routes: HashMap<String, (u16, Vec<u8>, Option<String>)>,
        calls: RefCell<Vec<(String, Option<String>)>>,
    }

    impl Fixtures {
        fn route(mut self, url: impl Into<String>, status: u16, body: Vec<u8>) -> Self {
            self.routes.insert(url.into(), (status, body, None));
            self
        }

        fn paged(mut self, url: impl Into<String>, body: Vec<u8>, next: &str) -> Self {
            self.routes.insert(url.into(), (200, body, Some(next.into())));
            self
        }

        fn repo(self, repo: &str, rev: &str) -> Self {
            let n = repo.replace('/', "_");
            self.route(meta_url(repo), 200, fixture(&format!("meta_{n}.json"))).route(
                tree_url(repo, rev),
                200,
                fixture(&format!("tree_{n}.json")),
            )
        }

        fn mlx(self, repo: &str, rev: &str) -> Self {
            let n = repo.replace('/', "_");
            self.repo(repo, rev)
                .route(resolve_url(repo, rev, "config.json"), 200, fixture(&format!("config_{n}.json")))
                .route(
                    resolve_url(repo, rev, "model.safetensors.index.json"),
                    200,
                    fixture(&format!("index_{n}.json")),
                )
        }

        fn tokens_sent(&self) -> Vec<(String, Option<String>)> {
            self.calls.borrow().clone()
        }
    }

    impl Fetch for Fixtures {
        fn get(&self, url: &str, token: Option<&str>, cap: usize) -> Result<Fetched, HubError> {
            self.calls.borrow_mut().push((url.to_string(), token.map(String::from)));
            let (status, body, next) =
                self.routes.get(url).cloned().unwrap_or_else(|| panic!("no fixture for {url}"));
            assert!(body.len() <= cap, "{url} is over its cap");
            Ok(Fetched { status, body, next })
        }
    }

    fn meta_url(repo: &str) -> String {
        format!("{API}/{repo}?{META_EXPAND}")
    }

    fn tree_url(repo: &str, rev: &str) -> String {
        format!("{API}/{repo}/tree/{rev}?recursive=true&expand=false")
    }

    fn resolve_url(repo: &str, rev: &str, file: &str) -> String {
        format!("https://huggingface.co/{repo}/resolve/{rev}/{file}")
    }

    fn tmp_store() -> (tempfile::TempDir, ModelStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = ModelStore::new(dir.path());
        (dir, store)
    }

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
    fn mlx_builds_are_labelled_and_merged() {
        let raw = |id: &str, tags: &[&str], downloads: u64| RawModel {
            id: id.into(),
            downloads,
            likes: 0,
            tags: tags.iter().map(|t| t.to_string()).collect(),
            gated: serde_json::Value::Bool(false),
            last_modified: None,
            card_data: None,
            sha: None,
            gguf: None,
        };
        let models = merge(
            vec![
                raw("unsloth/gemma-4-E4B-it-GGUF", &["gguf"], 50).into_model(),
                raw("mlx-community/gemma-4-E4B-it-4bit", &["mlx", "safetensors"], 90).into_model(),
                raw("someone/qwen-mlx", &["mlx"], 10).into_model(),
                raw("unsloth/gemma-4-E4B-it-GGUF", &["gguf"], 50).into_model(),
            ],
            10,
        );
        let ids: Vec<_> = models.iter().map(|m| (m.id.as_str(), m.mlx)).collect();
        assert_eq!(
            ids,
            [
                ("mlx-community/gemma-4-E4B-it-4bit", true),
                ("unsloth/gemma-4-E4B-it-GGUF", false),
                ("someone/qwen-mlx", true)
            ]
        );
    }

    #[test]
    fn repo_ids_are_validated() {
        assert!(valid_repo("google/gemma-4-E4B-it-qat-q4_0-gguf"));
        assert!(!valid_repo("google"));
        assert!(!valid_repo("../etc/passwd"));
        assert!(!valid_repo("a/b/c"));
        assert!(!valid_repo("a/.."));
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
    fn shard_names_are_parsed() {
        assert_eq!(
            parse_shard("Qwen3-235B-A22B-Instruct-2507-Q4_K_M-00001-of-00003.gguf"),
            Some(Shard { prefix: "Qwen3-235B-A22B-Instruct-2507-Q4_K_M".into(), index: 1, count: 3 })
        );
        assert_eq!(parse_shard("m-00002-of-00002.GGUF").map(|s| s.index), Some(2));
        for not in [
            "m.gguf",
            "m-00004-of-00003.gguf",
            "m-0000a-of-00003.gguf",
            "-00001-of-00002.gguf",
            "m-00001-of-00002.bin",
            "m-of-00002.gguf",
        ] {
            assert_eq!(parse_shard(not), None, "{not}");
        }
    }

    #[test]
    fn helper_ggufs_are_recognised() {
        for helper in [
            "mmproj-F16.gguf",
            "mmproj-model-f16.gguf",
            "mtp-gemma-4-E4B-it.gguf",
            "MTP/mtp-gemma-4-E4B-it-Q8_0.gguf",
            "MTP/whatever.gguf",
            "imatrix.gguf",
            "ggml-vocab-llama.gguf",
            "Qwen3-0.6B-DRAFT.gguf",
            "Llama-3-LoRA-F16.gguf",
            "adapter_model.gguf",
        ] {
            assert!(is_helper_gguf(helper), "{helper}");
        }
        for model in
            ["gemma-4-E4B-it-Q4_K_M.gguf", "Q4_K_M/Qwen3-235B-Q4_K_M-00001-of-00003.gguf", "smtp-model.gguf"]
        {
            assert!(!is_helper_gguf(model), "{model}");
        }
        // Whole words only: full models whose names merely contain the letters.
        for model in [
            "Llama-3.1-70B-Instruct-lorablated.Q4_K_M.gguf",
            "Hermes-3-Llama-3.1-8B-lorablated-Q4_K_M.gguf",
            "Explora-7B-Q4_K_M.gguf",
            "Q4_K_M/Floral-v1-Q4_K_M-00001-of-00002.gguf",
            "Draftsman-12B-Q4_K_M.gguf",
            "Adaptera-3B.Q8_0.gguf",
            "loras-not-a-folder-7B-Q4_0.gguf",
        ] {
            assert!(!is_helper_gguf(model), "{model}");
        }
        // Words and folders that do mean a helper.
        for helper in [
            "Qwen3-0.6B_draft.Q8_0.gguf",
            "Mistral-7B-lora-Q8_0.gguf",
            "LoRA/Mistral-7B-Q8_0.gguf",
            "adapters/style.gguf",
            "Qwen2.5-Omni-7B-mmproj-Q8_0.gguf",
            "mmproj/model-f16.gguf",
        ] {
            assert!(is_helper_gguf(helper), "{helper}");
        }
    }

    #[test]
    fn the_mlx_allow_list() {
        for ok in [
            "config.json",
            "model.safetensors",
            "model-00001-of-00002.safetensors",
            "weights.00.safetensors",
            "tokenizer.json",
            "qwen.tiktoken",
            "chat_template.jinja",
            "LICENSE",
            "LICENSE.txt",
            "processor_config.json",
        ] {
            assert!(mlx_allowed(ok), "{ok}");
        }
        for no in [
            "modeling_qwen.py",
            "tokenization.py",
            "sub/model.safetensors",
            "README.md",
            ".gitattributes",
            "model.gguf",
            "pytorch_model.bin",
            "consolidated.safetensors",
            "tokenizer.py",
        ] {
            assert!(!mlx_allowed(no), "{no}");
        }
    }

    #[test]
    fn link_headers_are_read() {
        assert_eq!(
            next_link(r#"<https://huggingface.co/api/models/a/b/tree/main?cursor=xyz>; rel="next""#)
                .as_deref(),
            Some("https://huggingface.co/api/models/a/b/tree/main?cursor=xyz")
        );
        assert_eq!(
            next_link(r#"<https://x/prev>; rel="prev", <https://x/n>; rel="next""#).as_deref(),
            Some("https://x/n")
        );
        assert_eq!(next_link(r#"<https://x/prev>; rel="prev""#), None);
    }

    /// The live 99-entry recursive listing: 20 folders of split GGUFs, each
    /// offered as one row with its parts; imatrix and README left out.
    #[test]
    fn split_ggufs_in_folders_are_grouped() {
        let f = Fixtures::default().repo(Q235, Q235_REV);
        let r = repo_with(&f, Q235, None).unwrap();
        assert_eq!(r.revision, Q235_REV);
        assert_eq!(r.files.len(), 20);
        assert_eq!(r.access, Access::Open);
        assert_eq!(r.base_model.as_deref(), Some("Qwen/Qwen3-235B-A22B-Instruct-2507"));
        assert!(r.mlx.is_none());
        let q4 = r.files.iter().find(|f| f.path.starts_with("Q4_K_M/")).unwrap();
        assert_eq!(q4.path, "Q4_K_M/Qwen3-235B-A22B-Instruct-2507-Q4_K_M-00001-of-00003.gguf");
        assert_eq!(q4.parts.len(), 3);
        assert_eq!(q4.size, 49944699040 + 49930349824 + 42279026016);
        assert!(
            q4.sha256.is_some() && q4.parts.iter().all(|p| p.sha256.as_ref().is_some_and(|h| h.len() == 64))
        );
        assert_eq!(r.files.iter().find(|f| f.path.starts_with("BF16/")).unwrap().parts.len(), 10);
        assert!(r.files.windows(2).all(|w| w[0].size <= w[1].size), "smallest first");
        // Metadata is tokenless; so is the tree of a repo that isn't gated.
        assert!(f.tokens_sent().iter().all(|(_, t)| t.is_none()));

        let (_tmp, store) = tmp_store();
        let e = add_to_catalog(&store, &r, &q4.path, false).unwrap();
        assert_eq!(
            e.id,
            "hf:unsloth/Qwen3-235B-A22B-Instruct-2507-GGUF/Qwen3-235B-A22B-Instruct-2507-Q4_K_M"
        );
        assert_eq!(e.files.len(), 3);
        assert_eq!(e.files[2].path, "Q4_K_M/Qwen3-235B-A22B-Instruct-2507-Q4_K_M-00003-of-00003.gguf");
        assert!(matches!(&e.layout, Layout::LlmGguf { model } if model == &q4.path));
        assert!(!e.gated);
    }

    #[test]
    fn incomplete_shard_sets_are_not_offered() {
        let tree: Vec<RawTreeEntry> =
            serde_json::from_slice(&fixture("tree_unsloth_Qwen3-235B-A22B-Instruct-2507-GGUF.json")).unwrap();
        let without_one: Vec<RawTreeEntry> = tree
            .into_iter()
            .filter(|e| e.path != "Q2_K/Qwen3-235B-A22B-Instruct-2507-Q2_K-00002-of-00002.gguf")
            .collect();
        let files = gguf_choices(&without_one);
        assert_eq!(files.len(), 19);
        assert!(!files.iter().any(|f| f.path.starts_with("Q2_K/")));
    }

    #[test]
    fn the_tree_is_paged_only_on_the_hub_api() {
        let tree: Vec<serde_json::Value> =
            serde_json::from_slice(&fixture("tree_unsloth_Qwen3-235B-A22B-Instruct-2507-GGUF.json")).unwrap();
        let (a, b) = tree.split_at(50);
        let page2 = format!("{API}/{Q235}/tree/{Q235_REV}?recursive=true&expand=false&cursor=abc");
        let f = Fixtures::default()
            .route(meta_url(Q235), 200, fixture("meta_unsloth_Qwen3-235B-A22B-Instruct-2507-GGUF.json"))
            .paged(tree_url(Q235, Q235_REV), serde_json::to_vec(a).unwrap(), &page2)
            .route(page2.clone(), 200, serde_json::to_vec(b).unwrap());
        assert_eq!(repo_with(&f, Q235, None).unwrap().files.len(), 20);

        // A next link off the Hub API is never followed.
        let f = Fixtures::default()
            .route(meta_url(Q235), 200, fixture("meta_unsloth_Qwen3-235B-A22B-Instruct-2507-GGUF.json"))
            .paged(
                tree_url(Q235, Q235_REV),
                serde_json::to_vec(a).unwrap(),
                "https://evil.example/api/models/x",
            );
        let r = repo_with(&f, Q235, None).unwrap();
        assert!(r.files.len() < 20);
        assert_eq!(f.tokens_sent().len(), 2);

        // At most 20 pages, even if the Hub keeps saying there's more.
        let mut f = Fixtures::default().route(
            meta_url(Q235),
            200,
            fixture("meta_unsloth_Qwen3-235B-A22B-Instruct-2507-GGUF.json"),
        );
        let page = |i: usize| format!("{API}/{Q235}/tree/{Q235_REV}?recursive=true&expand=false&cursor={i}");
        f = f.paged(tree_url(Q235, Q235_REV), b"[]".to_vec(), &page(1));
        for i in 1..40 {
            f = f.paged(page(i), b"[]".to_vec(), &page(i + 1));
        }
        repo_with(&f, Q235, None).unwrap();
        assert_eq!(f.tokens_sent().len(), 1 + MAX_PAGES);
    }

    /// `mtp-` drafts (also under `MTP/`) and vision projectors aren't models.
    #[test]
    fn helper_files_are_not_offered() {
        let f = Fixtures::default().repo(GEMMA4, GEMMA4_REV);
        let r = repo_with(&f, GEMMA4, None).unwrap();
        assert_eq!(r.files.len(), 21);
        for file in &r.files {
            let lower = file.path.to_ascii_lowercase();
            assert!(lower.starts_with("gemma-4-e4b-it-"), "{}", file.path);
            assert_eq!(file.parts.len(), 1);
        }
        let e = add_to_catalog(&tmp_store().1, &r, "gemma-4-E4B-it-Q4_K_M.gguf", false).unwrap();
        assert_eq!(e.id, "hf:unsloth/gemma-4-E4B-it-GGUF/gemma-4-E4B-it-Q4_K_M");
    }

    /// A gated repo's tree answers 200 with `****` hashes.
    #[test]
    fn gated_repos_hide_hashes_until_access_is_given() {
        let f = Fixtures::default().repo(GATED, GATED_REV);
        let r = repo_with(&f, GATED, None).unwrap();
        assert_eq!(r.access, Access::NeedsToken);
        assert_eq!(r.gated_kind.as_deref(), Some("manual"));
        assert_eq!(r.files.len(), 1);
        assert_eq!(r.files[0].sha256, None, "a masked hash is never kept");
        assert!(f.tokens_sent().iter().all(|(_, t)| t.is_none()));
        let (_tmp, store) = tmp_store();
        let err = add_to_catalog(&store, &r, "gemma-3-1b-it-q4_0.gguf", true).unwrap_err();
        assert!(err.to_string().contains("hiding this model's file checksums"), "{err}");
        assert!(load_user_catalog(&store).is_empty());

        // With a token: metadata stays tokenless, the tree gets it.
        let f = Fixtures::default().repo(GATED, GATED_REV);
        let r = repo_with(&f, GATED, Some("hf_token_value")).unwrap();
        assert_eq!(r.access, Access::NoAccessYet);
        let calls = f.tokens_sent();
        assert_eq!(calls[0], (meta_url(GATED), None));
        assert_eq!(calls[1], (tree_url(GATED, GATED_REV), Some("hf_token_value".into())));

        // A token the Hub refuses.
        let f = Fixtures::default()
            .route(meta_url(GATED), 200, fixture("meta_google_gemma-3-1b-it-qat-q4_0-gguf.json"))
            .route(tree_url(GATED, GATED_REV), 401, b"{}".to_vec());
        assert!(matches!(repo_with(&f, GATED, Some("hf_bad")), Err(HubError::BadToken)));
    }

    #[test]
    fn gated_repos_with_access_record_it() {
        let mut tree: Vec<serde_json::Value> =
            serde_json::from_slice(&fixture("tree_google_gemma-3-1b-it-qat-q4_0-gguf.json")).unwrap();
        tree[2]["lfs"]["oid"] = serde_json::Value::String("ab".repeat(32));
        let f = Fixtures::default()
            .route(meta_url(GATED), 200, fixture("meta_google_gemma-3-1b-it-qat-q4_0-gguf.json"))
            .route(tree_url(GATED, GATED_REV), 200, serde_json::to_vec(&tree).unwrap());
        let r = repo_with(&f, GATED, Some("hf_ok")).unwrap();
        assert_eq!(r.access, Access::Open);
        let (_tmp, store) = tmp_store();
        let e = add_to_catalog(&store, &r, "gemma-3-1b-it-q4_0.gguf", true).unwrap();
        assert!(e.gated, "downloads will send the token");
        assert_eq!(e.files[0].sha256, "ab".repeat(32));
    }

    /// Public repos never get the token, even when one is saved.
    #[test]
    fn open_repos_never_get_the_token() {
        let f = Fixtures::default().mlx(QWEN_MLX, QWEN_MLX_REV);
        repo_with(&f, QWEN_MLX, Some("hf_token_value")).unwrap();
        assert!(f.tokens_sent().iter().all(|(_, t)| t.is_none()), "{:?}", f.tokens_sent());
    }

    #[test]
    fn missing_or_private_repos_are_not_found() {
        let f = Fixtures::default().route(
            meta_url("openbark-nobody/does-not-exist-xyz"),
            401,
            fixture("meta_missing.json"),
        );
        let err = repo_with(&f, "openbark-nobody/does-not-exist-xyz", None).unwrap_err();
        assert!(matches!(err, HubError::NotFound(_)));
        assert_eq!(
            err.to_string(),
            "openbark-nobody/does-not-exist-xyz wasn't found on Hugging Face, or it's private."
        );
        let f = Fixtures::default().route(meta_url("a/b"), 404, b"{}".to_vec());
        assert!(matches!(repo_with(&f, "a/b", None), Err(HubError::NotFound(_))));
        let f = Fixtures::default().route(meta_url("a/b"), 500, b"{}".to_vec());
        assert!(matches!(repo_with(&f, "a/b", None), Err(HubError::Http(500))));
    }

    #[test]
    fn an_mlx_repo_is_one_whole_folder() {
        let f = Fixtures::default().mlx(QWEN_MLX, QWEN_MLX_REV);
        let r = repo_with(&f, QWEN_MLX, None).unwrap();
        assert!(r.files.is_empty());
        let mlx = r.mlx.as_ref().unwrap();
        let names: Vec<&str> = mlx.files.iter().map(|p| p.path.as_str()).collect();
        assert_eq!(
            names,
            [
                "added_tokens.json",
                "config.json",
                "merges.txt",
                "model.safetensors",
                "model.safetensors.index.json",
                "special_tokens_map.json",
                "tokenizer.json",
                "tokenizer_config.json",
                "vocab.json"
            ]
        );
        assert_eq!(mlx.model_type.as_deref(), Some("qwen3"));
        assert_eq!(mlx.quantization.as_deref(), Some("4-bit"));
        assert_eq!(mlx.problem, None);
        let config = mlx.files.iter().find(|p| p.path == "config.json").unwrap();
        assert_eq!(config.git_sha1.as_deref(), Some("6c974b02334f2f5f68de4cb2796ee4fe80bfc712"));
        assert_eq!(config.sha256, None);
        let weights = mlx.files.iter().find(|p| p.path == "model.safetensors").unwrap();
        assert!(weights.sha256.as_ref().is_some_and(|h| h.len() == 64) && weights.git_sha1.is_none());
        assert_eq!(r.base_model.as_deref(), Some("Qwen/Qwen3-0.6B"));

        let (_tmp, store) = tmp_store();
        let e = add_to_catalog(&store, &r, MLX_CHOICE, false).unwrap();
        assert_eq!(e.id, "hf:mlx-community/Qwen3-0.6B-4bit/@mlx");
        assert!(matches!(&e.layout, Layout::LlmMlx { config } if config == "config.json"));
        assert_eq!(e.files.len(), 9);
        let c = e.files.iter().find(|f| f.path == "config.json").unwrap();
        assert_eq!(
            (c.sha256.as_str(), c.git_sha1.as_deref()),
            ("", Some("6c974b02334f2f5f68de4cb2796ee4fe80bfc712"))
        );
        assert_eq!(e.total_size(), mlx.size);
        // A GGUF choice in an MLX repo is refused plainly.
        assert!(add_to_catalog(&store, &r, "model.safetensors", false).is_err());
    }

    #[test]
    fn a_sharded_mlx_repo_needs_every_indexed_file() {
        let f = Fixtures::default().mlx(GEMMA_MLX, GEMMA_MLX_REV);
        let r = repo_with(&f, GEMMA_MLX, None).unwrap();
        let mlx = r.mlx.as_ref().unwrap();
        assert_eq!(mlx.problem, None);
        assert_eq!(mlx.model_type.as_deref(), Some("gemma4"));
        assert_eq!(mlx.quantization.as_deref(), Some("4-bit"));
        let names: Vec<&str> = mlx.files.iter().map(|p| p.path.as_str()).collect();
        assert!(
            names.contains(&"model-00001-of-00002.safetensors")
                && names.contains(&"model-00002-of-00002.safetensors")
        );
        assert!(names.contains(&"chat_template.jinja") && names.contains(&"processor_config.json"));
        assert!(!names.contains(&"README.md"));
        // tokenizer.json is in LFS here: verified by SHA-256.
        assert!(mlx.files.iter().find(|p| p.path == "tokenizer.json").unwrap().sha256.is_some());

        // The same repo with one weight file gone.
        let tree: Vec<serde_json::Value> =
            serde_json::from_slice(&fixture("tree_lmstudio-community_gemma-4-E4B-it-MLX-4bit.json")).unwrap();
        let tree: Vec<RawTreeEntry> = serde_json::from_value(serde_json::Value::Array(
            tree.into_iter().filter(|e| e["path"] != "model-00002-of-00002.safetensors").collect(),
        ))
        .unwrap();
        let config = fixture("config_lmstudio-community_gemma-4-E4B-it-MLX-4bit.json");
        let index = fixture("index_lmstudio-community_gemma-4-E4B-it-MLX-4bit.json");
        let m = mlx_choice(&tree, Some(&config), Some(&index), false);
        assert_eq!(
            m.problem,
            Some(MlxProblem::IncompleteShards { missing: vec!["model-00002-of-00002.safetensors".into()] })
        );
        // Without an index, the -NNNNN-of-NNNNN names tell.
        let m = mlx_choice(&tree, Some(&config), None, false);
        assert_eq!(
            m.problem,
            Some(MlxProblem::IncompleteShards { missing: vec!["model-00002-of-00002.safetensors".into()] })
        );
    }

    fn entry(path: &str, size: u64) -> RawTreeEntry {
        RawTreeEntry { kind: "file".into(), path: path.into(), size, oid: Some("1".repeat(40)), lfs: None }
    }

    fn lfs(path: &str, size: u64) -> RawTreeEntry {
        RawTreeEntry {
            kind: "file".into(),
            path: path.into(),
            size,
            oid: Some("2".repeat(40)),
            lfs: Some(RawLfs { oid: "3".repeat(64), size }),
        }
    }

    #[test]
    fn mlx_problems_are_named() {
        let cfg = br#"{"model_type":"llama","quantization":{"bits":8}}"#.as_slice();
        let good = vec![entry("config.json", 10), lfs("model.safetensors", 100), entry("tokenizer.json", 5)];
        let m = mlx_choice(&good, Some(cfg), None, false);
        assert_eq!((m.problem.clone(), m.quantization.as_deref()), (None, Some("8-bit")));

        let no_config = vec![lfs("model.safetensors", 100), entry("tokenizer.json", 5)];
        assert_eq!(mlx_choice(&no_config, None, None, false).problem, Some(MlxProblem::NoConfig));
        assert_eq!(mlx_choice(&good, Some(b"not json"), None, false).problem, Some(MlxProblem::BadConfig));
        assert_eq!(mlx_choice(&good, Some(b"{\"x\":1}"), None, false).problem, Some(MlxProblem::BadConfig));
        assert_eq!(mlx_choice(&good, None, None, false).problem, Some(MlxProblem::BadConfig));
        let no_weights = vec![entry("config.json", 10), entry("tokenizer.json", 5), lfs("model.gguf", 9)];
        assert_eq!(mlx_choice(&no_weights, Some(cfg), None, false).problem, Some(MlxProblem::NoWeights));
        let no_tok = vec![entry("config.json", 10), lfs("model.safetensors", 100)];
        assert_eq!(mlx_choice(&no_tok, Some(cfg), None, false).problem, Some(MlxProblem::NoTokenizer));
        let index = br#"{"weight_map":{"a":"model.safetensors","b":"extra.safetensors"}}"#;
        assert_eq!(
            mlx_choice(&good, Some(cfg), Some(index), false).problem,
            Some(MlxProblem::IncompleteShards { missing: vec!["extra.safetensors".into()] })
        );
        assert_eq!(mlx_choice(&good, Some(cfg), None, true).problem, Some(MlxProblem::HashesHidden));

        // Python files, sub-folders and other weight formats are never picked.
        let mut extra = good.clone();
        extra.extend([
            entry("modeling_x.py", 5),
            lfs("sub/model.safetensors", 7),
            lfs("pytorch_model.bin", 7),
        ]);
        let m = mlx_choice(&extra, Some(cfg), None, false);
        assert_eq!(m.files.len(), 3);
        assert_eq!(m.size, 115);
    }

    #[test]
    fn gguf_alternatives_put_known_publishers_first() {
        let url = format!(
            "{API}?filter=base_model:quantized:Qwen/Qwen3-0.6B&filter=gguf&sort=downloads&direction=-1&limit=50&{LIST_EXPAND}"
        );
        let f = Fixtures::default().route(url, 200, fixture("alternatives_Qwen_Qwen3-0.6B.json"));
        let alts = gguf_alternatives_with(&f, "Qwen/Qwen3-0.6B", 7).unwrap();
        let ids: Vec<&str> = alts.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "Qwen/Qwen3-0.6B-GGUF",
                "ggml-org/Qwen3-0.6B-GGUF",
                "unsloth/Qwen3-0.6B-GGUF",
                "bartowski/Qwen_Qwen3-0.6B-GGUF",
                "lmstudio-community/Qwen3-0.6B-GGUF",
                "MaziyarPanahi/Qwen3-0.6B-GGUF",
                "NobodyWho/Qwen_Qwen3-0.6B-GGUF",
            ]
        );
        assert!(f.tokens_sent().iter().all(|(_, t)| t.is_none()));
        assert_eq!(gguf_alternatives_with(&f, "Qwen/Qwen3-0.6B", 100).unwrap().len(), 10);
        assert_eq!(gguf_alternatives_with(&f, "Qwen/Qwen3-0.6B", 0).unwrap().len(), 1);
        assert!(gguf_alternatives_with(&f, "../x", 5).is_err());
    }

    // ---- the phone profile (ADR-0018 section 10) ----------------------------

    fn mobile_meta_url(repo: &str) -> String {
        format!("{API}/{repo}?{META_EXPAND}&expand[]=gguf")
    }

    fn mobile_repo(f: Fixtures, repo: &str) -> Fixtures {
        let n = repo.replace('/', "_");
        f.route(mobile_meta_url(repo), 200, fixture(&format!("meta_{n}.json")))
    }

    #[test]
    fn phone_search_is_gguf_only_and_drops_models_over_the_cap() {
        let url = format!("{API}?search=qwen&filter=gguf&sort=downloads&direction=-1&limit=10&{LIST_EXPAND}");
        let body = br#"[
          {"id": "unsloth/Qwen3-235B-A22B-GGUF", "downloads": 900, "tags": ["gguf"]},
          {"id": "unsloth/Qwen3-1.7B-GGUF", "downloads": 800, "tags": ["gguf"]},
          {"id": "someone/qwen-mlx", "downloads": 700, "tags": ["mlx", "gguf"]},
          {"id": "unsloth/Phi-4-mini-instruct-GGUF", "downloads": 600, "tags": ["gguf"]}
        ]"#;
        // Only the GGUF search is routed: asking for MLX builds would panic.
        let f = Fixtures::default().route(url, 200, body.to_vec());
        let found = search_mobile_with(&f, "qwen", 10, Some(platform::TIERS[1])).unwrap();
        let ids: Vec<&str> = found.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, ["unsloth/Qwen3-1.7B-GGUF", "unsloth/Phi-4-mini-instruct-GGUF"]);
        assert_eq!(f.tokens_sent().len(), 1);
        // A phone under 6 GB searches nothing.
        assert!(search_mobile_with(&Fixtures::default(), "qwen", 10, None).unwrap().is_empty());
    }

    #[test]
    fn phone_repo_files_are_held_back_with_reasons() {
        let f = mobile_repo(Fixtures::default(), GEMMA4).route(
            tree_url(GEMMA4, GEMMA4_REV),
            200,
            fixture("tree_unsloth_gemma-4-E4B-it-GGUF.json"),
        );
        let r = repo_mobile_with(&f, GEMMA4, None).unwrap();
        assert!(r.mlx.is_none());
        // 12 GB: the 4-bit and 3-bit builds within 5.3 GB.
        let (kept, held) = mobile_files(&r, Some(platform::TIERS[2]));
        let kept_names: Vec<&str> = kept.iter().map(|k| k.path.as_str()).collect();
        assert!(kept_names.contains(&"gemma-4-E4B-it-Q4_K_M.gguf"), "{kept_names:?}");
        assert!(kept_names.contains(&"gemma-4-E4B-it-Q4_0.gguf"));
        assert!(kept.iter().all(|k| k.size <= platform::TIERS[2].llm_budget_bytes));
        let why = |name: &str| held.iter().find(|(h, _)| h.path == name).map(|(_, w)| *w);
        assert_eq!(why("gemma-4-E4B-it-BF16.gguf"), Some(TooBig::Quantisation));
        assert_eq!(why("gemma-4-E4B-it-Q6_K.gguf"), Some(TooBig::Quantisation));
        assert!(matches!(why("gemma-4-E4B-it-Q5_K_M.gguf"), Some(TooBig::TooLarge { .. })));
        assert_eq!(kept.len() + held.len(), r.files.len());
        // 8 GB: every E4B build is over 3.5 GB.
        let (kept, held) = mobile_files(&r, Some(platform::TIERS[1]));
        assert!(kept.is_empty());
        assert!(matches!(
            held.iter().find(|(h, _)| h.path == "gemma-4-E4B-it-Q4_K_M.gguf").map(|(_, w)| *w),
            Some(TooBig::TooLarge { .. })
        ));
        // Adding a held-back file on the phone is refused, in plain words; an
        // offered one is marked mobile.
        let (_dir, store) = tmp_store();
        let err = add_to_catalog_for(
            &store,
            &r,
            "gemma-4-E4B-it-Q4_K_M.gguf",
            false,
            Some(Some(platform::TIERS[1])),
        )
        .unwrap_err();
        assert_eq!(err.to_string(), "Too large for this iPhone (8 GB): 5.0 GB. Models up to 3.5 GB fit.");
        let e = add_to_catalog_for(
            &store,
            &r,
            "gemma-4-E4B-it-Q4_K_M.gguf",
            false,
            Some(Some(platform::TIERS[2])),
        )
        .unwrap();
        assert!(e.mobile);
        // On a computer nothing changes: not marked mobile.
        let (_dir2, store2) = tmp_store();
        assert!(!add_to_catalog(&store2, &r, "gemma-4-E4B-it-Q4_K_M.gguf", false).unwrap().mobile);
    }

    #[test]
    fn phones_never_see_mlx() {
        let f = mobile_repo(Fixtures::default(), QWEN_MLX).route(
            tree_url(QWEN_MLX, QWEN_MLX_REV),
            200,
            fixture("tree_mlx-community_Qwen3-0.6B-4bit.json"),
        );
        // No config.json or index is read: those aren't routed.
        let r = repo_mobile_with(&f, QWEN_MLX, None).unwrap();
        assert!(r.mlx.is_none() && !r.model.mlx);
        let (_dir, store) = tmp_store();
        let err =
            add_to_catalog_for(&store, &r, MLX_CHOICE, false, Some(Some(platform::TIERS[2]))).unwrap_err();
        assert_eq!(err.to_string(), MLX_NOT_ON_PHONE);
    }

    #[test]
    fn search_never_sends_a_token() {
        let url = |filter: &str| {
            format!("{API}?search=qwen&filter={filter}&sort=downloads&direction=-1&limit=5&{LIST_EXPAND}")
        };
        let f = Fixtures::default()
            .route(url("gguf"), 200, fixture("alternatives_Qwen_Qwen3-0.6B.json"))
            .route(url("mlx"), 200, b"[]".to_vec());
        let found = search_with(&f, "qwen", 5, true).unwrap();
        assert_eq!(found.len(), 5);
        assert_eq!(f.tokens_sent().len(), 2);
        assert!(f.tokens_sent().iter().all(|(_, t)| t.is_none()));
    }

    #[test]
    fn whoami_checks_the_token() {
        let f = Fixtures::default().route(
            "https://huggingface.co/api/whoami-v2",
            200,
            br#"{"type":"user","name":"clinic-vet"}"#.to_vec(),
        );
        assert_eq!(whoami_with(&f, "hf_abc").unwrap(), "clinic-vet");
        assert_eq!(f.tokens_sent()[0].1.as_deref(), Some("hf_abc"));
        let f = Fixtures::default().route(
            "https://huggingface.co/api/whoami-v2",
            401,
            br#"{"error":"Invalid credentials"}"#.to_vec(),
        );
        let err = whoami_with(&f, "hf_bad").unwrap_err();
        assert!(matches!(err, HubError::BadToken));
        assert_eq!(
            err.to_string(),
            "Hugging Face didn't accept that token. Copy it again from huggingface.co/settings/tokens."
        );
    }

    fn sample_repo(license: &str, class: LicenseClass) -> HubRepo {
        HubRepo {
            model: HubModel {
                id: "someone/model-GGUF".into(),
                publisher: "someone".into(),
                downloads: 1,
                likes: 0,
                license: Some(license.into()),
                license_class: class,
                gated: false,
                last_modified: None,
                mlx: false,
            },
            revision: "a".repeat(40),
            files: vec![HubFile {
                path: "m.Q4_K_M.gguf".into(),
                size: 10,
                sha256: Some("b".repeat(64)),
                parts: vec![HubPart {
                    path: "m.Q4_K_M.gguf".into(),
                    size: 10,
                    sha256: Some("b".repeat(64)),
                    git_sha1: None,
                }],
            }],
            mlx: None,
            access: Access::Open,
            gated_kind: None,
            base_model: None,
            params: None,
        }
    }

    #[test]
    fn restricted_licence_needs_acceptance() {
        let (_tmp, store) = tmp_store();
        let repo = sample_repo("gemma", LicenseClass::Restricted);
        assert!(add_to_catalog(&store, &repo, "m.Q4_K_M.gguf", false).is_err());
        let entry = add_to_catalog(&store, &repo, "m.Q4_K_M.gguf", true).unwrap();
        assert_eq!(entry.id, "hf:someone/model-GGUF/m.Q4_K_M");
        assert!(full_catalog(&store).get(&entry.id).is_some());
        assert!(add_to_catalog(&store, &repo, MLX_CHOICE, true).is_err(), "not an MLX repo");
    }

    /// Adding a model again at a newer revision while the old one's files (or
    /// a stopped download of them) are on disk would orphan them or resume a
    /// download with another revision's bytes: refused, with what to do.
    #[test]
    fn a_newer_revision_waits_until_the_old_files_are_removed() {
        let (_tmp, store) = tmp_store();
        let old_repo = sample_repo("mit", LicenseClass::Permissive);
        let old = add_to_catalog(&store, &old_repo, "m.Q4_K_M.gguf", false).unwrap();
        // The same revision again is fine, files or not.
        let file = store.file_path(&old, "m.Q4_K_M.gguf");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, [0u8; 10]).unwrap();
        add_to_catalog(&store, &old_repo, "m.Q4_K_M.gguf", false).unwrap();

        let mut new_repo = old_repo.clone();
        new_repo.revision = "c".repeat(40);
        new_repo.files[0].parts[0].sha256 = Some("d".repeat(64));
        new_repo.files[0].parts[0].size = 12;
        new_repo.files[0].size = 12;
        let err = add_to_catalog(&store, &new_repo, "m.Q4_K_M.gguf", false).unwrap_err();
        assert_eq!(err.to_string(), OLDER_ON_DISK);
        let kept = load_user_catalog(&store);
        assert_eq!(kept.len(), 1);
        assert!(
            matches!(&kept[0].source, Source::Huggingface { revision, .. } if *revision == "a".repeat(40))
        );
        // A stopped download of the old revision counts too.
        std::fs::remove_file(&file).unwrap();
        std::fs::write(crate::store::part_path(&file), [0u8; 4]).unwrap();
        assert!(add_to_catalog(&store, &new_repo, "m.Q4_K_M.gguf", false).is_err());
        // Once removed, the newer revision replaces it.
        std::fs::remove_file(crate::store::part_path(&file)).unwrap();
        let new = add_to_catalog(&store, &new_repo, "m.Q4_K_M.gguf", false).unwrap();
        assert_eq!(new.files[0].size, 12);
        // A new revision that lists the same file unchanged keeps it.
        std::fs::write(&file, [0u8; 12]).unwrap();
        let mut same_files = new_repo.clone();
        same_files.revision = "e".repeat(40);
        add_to_catalog(&store, &same_files, "m.Q4_K_M.gguf", false).unwrap();
    }

    #[test]
    fn the_user_catalog_is_saved_atomically_and_entries_can_be_dropped() {
        let (tmp, store) = tmp_store();
        let repo = sample_repo("mit", LicenseClass::Permissive);
        let mut e = add_to_catalog(&store, &repo, "m.Q4_K_M.gguf", false).unwrap();
        e.display_name = "renamed".into();
        save_user_entry(&store, e.clone()).unwrap();
        let user = load_user_catalog(&store);
        assert_eq!(user.len(), 1, "same id replaces");
        assert_eq!(user[0].display_name, "renamed");
        assert!(!tmp.path().join("user-catalog.json.tmp").exists());

        // A model file next to it is never touched by forgetting.
        let model = store.file_path(&e, "m.Q4_K_M.gguf");
        std::fs::create_dir_all(model.parent().unwrap()).unwrap();
        std::fs::write(&model, b"x").unwrap();
        assert!(remove_user_entry(&store, &e.id).unwrap());
        assert!(!remove_user_entry(&store, &e.id).unwrap());
        assert!(load_user_catalog(&store).is_empty());
        assert!(model.exists());

        // A damaged user catalog is never overwritten.
        std::fs::write(user_catalog_path(&store), b"{not json").unwrap();
        assert!(save_user_entry(&store, e.clone()).is_err());
        assert!(remove_user_entry(&store, &e.id).is_err());
        assert_eq!(std::fs::read(user_catalog_path(&store)).unwrap(), b"{not json");
    }
}
