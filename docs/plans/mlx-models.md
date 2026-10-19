# Plan: MLX models, models of your choice, and removing models

Status: **accepted**, built and integrated on 2026-10-19 (see **Integration** at the end), with [ADR-0025](../adr/0025-mlx-and-models-of-your-choice.md). Planned 2026-10-18. Branch `feat/mlx-models`.

## Goal

- On a Mac with Apple silicon and macOS 14 or later, an MLX model found in **Browse Hugging Face** downloads as one row and writes notes, held to the template's JSON Schema like any GGUF model.
- Split GGUFs, GGUFs in sub-folders and gated repos work everywhere. `mtp-`, `mmproj`, `imatrix` and similar helper files stop being offered.
- A Hugging Face token can be saved (keychain), and is sent only for gated repos.
- A model can be added from this computer: a `.gguf` file anywhere, or an MLX folder on a Mac. It is copied, or used where it is.
- Every downloaded or added model, the MLX engine pack and the GPU speech pack can be removed, with the space used and the space freed shown. The note model, a busy model and the speech models OpenBark needs are protected.

## How the work is split

Three build tracks run **in parallel on disjoint files**, then the integrator wires them together. Nobody edits a file owned by another track. The only shared files are the manifests below, where each track adds exactly the lines listed and nothing else.

| Track | Owns (creates or edits) | Must not touch |
|---|---|---|
| **A: Hub, catalog, download, store, import, removal** | `engine/os-models/src/catalog.rs`, `store.rs`, `download.rs`, `hub.rs`, `locations.rs`, `catalog.json` (no content change expected), new `engine/os-models/src/local.rs`, new `engine/os-models/src/remove.rs`, new `engine/os-models/tests/` (fixtures and tests), new `engine/os-engine/src/secrets.rs` | `mlx_pack.rs`, anything in `os-llm`, `os-app`, `os-server`, `os-cli`, `apps/` |
| **B: MLX runtime and pack** | new `engine/os-llm/src/mlx.rs`, new `engine/os-llm/src/mlx_worker.py`, new `engine/os-llm/tests/` (`mlx_fake_worker.py`, `mlx_worker.rs`), new `engine/os-models/src/mlx_pack.rs`, new `engine/os-models/src/mlx_pack.lock.json`, new `engine/os-models/src/mlx_pack.linux.lock.json`, new `scripts/mlx-pack-lock.py`, new `scripts/mlx-pack.in`, new `scripts/mlx-pack-linux.in`, new `scripts/mlx-test-env.sh`, new (generated) `docs/mlx-pack-pins.md` | `catalog.rs`, `store.rs`, `download.rs`, `hub.rs`, `locations.rs`, `os-llm/src/llama.rs`, `os-app`, UI |
| **C: UI and docs** | `apps/desktop/src/screens/Models.tsx`, `apps/desktop/src/components/GpuCard.tsx`, new `apps/desktop/src/components/MlxCard.tsx`, new `apps/desktop/src/components/RemoveModelDialog.tsx`, new `apps/desktop/src/components/AddLocalModelSheet.tsx`, new `apps/desktop/src/components/HfTokenField.tsx`, new tests beside them (`*.test.tsx`), `apps/desktop/src/engine.ts` (model, hub, GPU, MLX, token and import sections only), `apps/desktop/src/app.css` (new classes only), `packages/i18n/i18n-{fr,es,de,pt}.json`, the generated `apps/desktop/src/i18n/messages.ts`, `docs/network.md`, `docs/versions.md`, `docs/guide/*` (new `models.md`, links in `README.md`, `getting-started.md`, `troubleshooting.md`), `docs/languages.md` (one line), `NOTICE` (only if the GPU pack is listed there) | Rust, `scripts/` |
| **Integrator** (after A, B and C) | `engine/os-app/**`, `engine/os-server/**`, `engine/os-cli/**`, `apps/desktop/src-tauri/**`, `engine/os-llm/src/lib.rs`, `engine/os-llm/Cargo.toml` (beyond B's lines), `.github/workflows/**`, `Cargo.lock`, end-to-end scripts (`apps/desktop/e2e/models.mjs`, `scripts/ci/mlx-e2e.sh`), final passes on any file for compile fixes | |

### Shared manifests: exact lines

| File | Track A adds | Track B adds |
|---|---|---|
| `Cargo.toml` (workspace) `[workspace.dependencies]` | `sha1 = "=0.11.0"` under a comment `# Git blob SHA-1 for Hub files not in LFS (ADR-0025).` | `flate2 = "=1.1.10"` under a comment `# MLX engine pack: CPython archive (ADR-0025); the version already in Cargo.lock.` |
| `engine/os-models/Cargo.toml` `[dependencies]` | `sha1.workspace = true` | `flate2.workspace = true` |
| `engine/os-models/src/lib.rs` | `pub mod local;` and `pub mod remove;` after `pub mod locations;`, plus `pub use store::{FileHashes, UnsafePath, dir_name, hash_file, hashes_match, safe_rel_path};` and `pub use catalog::LlmEngine;` | `pub mod mlx_pack;` after `pub mod gpu_pack;` |
| `engine/os-engine/src/lib.rs` | `pub mod secrets;` after `pub mod render;` | |
| `engine/os-llm/src/lib.rs` | | `pub mod mlx;` after `mod llama;`, and `pub use mlx::{MlxBackend, MlxDevice, MlxOptions, ModelCheck, PackCheck};` |
| `engine/os-llm/Cargo.toml` | | `[dev-dependencies]` section with `jsonschema.workspace = true` (already a workspace dependency; tests only) |

Nothing else in these files changes until the integrator. Track B's `os-llm` needs no new runtime crates: it compares the worker script's bytes instead of hashing them, and uses `serde_json`, which it already has.

### Working together without coordination

- Each track builds and tests on its own: `source scripts/dev-env.sh; cargo test -p os-models` (A, B), `cargo test -p os-llm` (B), `cargo test -p os-engine secrets` (A), `pnpm -C apps/desktop test` and `pnpm -C apps/desktop typecheck` (C).
- Track C writes `engine.ts` against the contract in this plan. Until the integrator lands, the desktop and server don't have the new commands. C's tests mock `engine.ts`.
- If a contract turns out wrong, the track writes down the change it needs under **Contract changes** at the end of this file (append only, one line each). The integrator resolves them. No track changes another track's file to fit.
- Commits: one track per commit, message prefixed `Models (A):`, `MLX (B):`, `Models UI (C):` or `Models (integration):`. No co-author or "Generated with" lines. Credit is "the ANI.ML Health team".

## Ground rules (every track)

- **Never touch** port 4317 or the dev box's real `<local data>/OpenBark` folder (the owner's real copy and data). Demo servers use port **4326** or **4327**, with `OPENBARK_SESSIONS_DIR`, `OPENBARK_TEMPLATES_DIR`, `OPENBARK_USERS_FILE`, `OPENBARK_DATA_KEY`, `OPENBARK_MODELS_DIR`, `OPENBARK_ENGINES_DIR` and `OPENBARK_LOCATIONS_FILE` all pointing at scratch folders. `OPENBARK_PLATFORM=macos` shows the Mac screens. Kill any server you start, by exe path.
- **Removal is tested only in temporary folders** (`tempfile`), never against a real models folder, never on the dev box's real `OpenBark` folders.
- **Tests never touch the real keychain.** `secrets.rs` takes a backend; tests use the in-memory one.
- **Network:** only `os-models` may make HTTP calls (CI: `scripts/ci/check-network.sh`). Every new call is listed in `docs/network.md` (track C writes the rows from this plan). The MLX worker never uses the network.
- **Patient text** goes to the MLX worker only over its stdin. Nothing logs prompts or generated text.
- **Windows and Git Bash traps:** write Python and long files with the editor, not heredocs; use `MSYS_NO_PATHCONV=1` or script files for WSL paths; WSL is `wsl -d Ubuntu-24.04`.

## New environment variables

| Variable | Read by | Meaning |
|---|---|---|
| `OPENBARK_ENGINES_DIR` | B (`mlx_pack::engines_dir`) | Where engine packs are unpacked. Default `<local data>/OpenBark/engines` (the same `dirs::data_local_dir()/OpenBark` base as `locations.rs::app_dir`) |
| `OPENBARK_MLX_DEV=1` | B (`mlx_pack::support`) | Linux x86_64 only: offer MLX on the CPU with the Linux lock ("test mode"). Honoured only by debug builds and builds with the `mlx-test-mode` feature (`cargo build -p os-cli --features mlx-test-mode`); release builds for clinics ignore it |
| `OPENBARK_MLX_DEVICE=cpu` | B (worker spawn) | Force MLX's CPU device (macOS CI runners without usable Metal) |
| `OPENBARK_MLX_SANDBOX=0` | B (worker spawn) | macOS: don't wrap the worker in `sandbox-exec` (support only) |
| `OPENBARK_HF_TOKEN` | A (`secrets::hf_token`) | A Hugging Face token set by whoever runs the server. Overrides a saved one; the UI says "Set by the server". The generic `HF_TOKEN` is never read (review fix) |
| `OPENBARK_HF_TOKEN_FILE` | A (`secrets`) | Demo, test and scratch runs: keep the token only in this file (vault-encrypted, else memory); the keychain and the data folder's `hf-token.obk` are never read, written or deleted |
| `OPENBARK_MLX_TEST_PYTHON`, `OPENBARK_MLX_TEST_MODEL` | B (tests only) | Python of an unpacked pack, and an MLX model folder, for the real-worker tests. Unset: those tests print "skipped" and pass, unless `OPENBARK_MLX_TEST_REQUIRED=1` (CI's MLX job), which makes them fail |

## Contracts

All JSON is what serde produces from these types. Enum values are `snake_case` strings unless noted.

### 1. Catalog (track A, `catalog.rs`)

```rust
pub enum Source {
    Huggingface { repo: String, revision: String },
    Url,
    /// Added from this computer (ADR-0025 §9). `path` is the absolute path of
    /// the file or folder when it is used where it is; `None` when copied into
    /// the store under `local/<name>`.
    Local {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<String>,
    },
}
// JSON: {"kind":"local"} or {"kind":"local","path":"/Users/x/models/m.gguf"}

pub struct FileSpec {
    pub path: String,                 // repo-relative, '/' separated
    pub size: u64,
    #[serde(default)]
    pub sha256: String,               // may be "" only when git_sha1 is set
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_sha1: Option<String>,     // git blob SHA-1 (40 hex) for Hub files not in LFS
    #[serde(default)]
    pub url: Option<String>,
}

pub enum Layout {
    LlmGguf { model: String },        // for split GGUFs: part 1's path; all parts are in `files`
    /// An MLX model folder; the folder is the parent of `config`.
    LlmMlx { config: String },        // always "config.json" from the Hub
    // ... existing variants unchanged ...
}
// JSON: {"type":"llm_mlx","config":"config.json"}

pub struct CatalogEntry {
    // ... existing fields ...
    /// The Hub repo is gated: downloads send the saved token (and only these).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub gated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LlmEngine { Llama, Mlx }

impl CatalogEntry {
    pub fn llm_engine(&self) -> Option<LlmEngine>;          // LlmGguf -> Llama, LlmMlx -> Mlx, else None
    pub fn in_place(&self) -> Option<&std::path::Path>;     // Source::Local { path: Some(p) }
    pub fn is_local(&self) -> bool;                         // Source::Local { .. }
    pub fn url_for(&self, file: &FileSpec) -> Option<String>; // Local -> None (unchanged otherwise)
}
```

- Old `user-catalog.json` files still load (all new fields default).
- The built-in catalog test keeps requiring a 64-hex `sha256` for every file. A new test covers `LlmMlx` in the "layout references existing files" check.

### 2. Store (track A, `store.rs`)

```rust
/// A model id as a folder path that is valid on every platform: ':' and the
/// Windows-reserved characters <>"|?* and control characters become '_';
/// '/' stays a separator. "hf:bartowski/x/y" -> "hf_bartowski/x/y".
pub fn dir_name(id: &str) -> String;

/// A repo-relative path, checked: not empty, not absolute, no "..", "\\", ":",
/// NUL, or empty components. Returns it as a PathBuf with native separators.
pub fn safe_rel_path(p: &str) -> Result<PathBuf, UnsafePath>;
#[derive(Debug, thiserror::Error)] #[error("unsafe path in model files: {0}")]
pub struct UnsafePath(pub String);

pub struct FileHashes { pub sha256: String, pub git_sha1: String }
/// One pass over the file: SHA-256 and git blob SHA-1 ("blob {len}\0" + bytes).
pub fn hash_file(path: &Path) -> std::io::Result<FileHashes>;
/// sha256 when the spec has one, otherwise git_sha1. Case-insensitive.
pub fn hashes_match(spec: &FileSpec, h: &FileHashes) -> bool;

impl ModelStore {
    /// In place (Source::Local{path:Some}): the folder itself, or a file's parent folder.
    /// Otherwise <root>/<dir_name(id)>, except that an existing folder under the raw
    /// id (Mac and Linux installs from before this change) is still used.
    pub fn model_dir(&self, entry: &CatalogEntry) -> PathBuf;
    pub fn file_path(&self, entry: &CatalogEntry, name: &str) -> PathBuf; // unchanged meaning
    pub fn file_ok(&self, entry: &CatalogEntry, file: &FileSpec, verify: Verify) -> bool; // uses hashes_match
    /// Bytes on disk for this model's listed files plus their `.part` files. 0 when used in place.
    pub fn disk_usage(&self, entry: &CatalogEntry) -> u64;
    /// Bytes in `.part` files only (a download that stopped part-way).
    pub fn partial_bytes(&self, entry: &CatalogEntry) -> u64;
}
```

For an in-place **file** import, `files[i].path` are bare file names (the file and, for split GGUFs, its sibling parts) and `model_dir` is their folder. For an in-place **folder**, paths are relative to the folder.

### 3. Download (track A, `download.rs`)

Signature unchanged: `pub fn download(store, entry, token: Option<&str>, progress) -> Result<(), DownloadError>`.

- The token is sent only when `entry.gated` and only to `https://huggingface.co/` (cross-host redirects drop `Authorization` in reqwest; confirm in reqwest 0.13.5's `src/redirect.rs` `remove_sensitive_headers` and note it in a code comment).
- `fs::create_dir_all(part_path.parent())` before writing nested `.part` files; every `file.path` goes through `safe_rel_path` first.
- Verification uses `hash_file` + `hashes_match` (SHA-256 or git SHA-1).
- `Source::Local` is refused with `DownloadError::Local`.
- New error variants (messages are shown to users as-is, so keep them plain):

```rust
#[error("This model is gated on Hugging Face. Request access on its page, then add your token.")]
Gated { repo: String },          // 401/403 with X-Error-Code: GatedRepo
#[error("Hugging Face couldn't find {url}. It may have been removed or made private.")]
NotFound { url: String },        // 404, or 401 without GatedRepo
#[error("{0} was added from this computer, so it can't be downloaded.")]
Local(String),
#[error(transparent)]
UnsafePath(#[from] crate::store::UnsafePath),
```

### 4. Hub (track A, `hub.rs`)

```rust
pub const MLX_CHOICE: &str = "@mlx";

#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)] #[serde(rename_all = "snake_case")]
pub enum Access { Open, NeedsToken, NoAccessYet }

#[derive(Serialize, Clone, Debug)]
pub struct HubPart { pub path: String, pub size: u64, pub sha256: Option<String>, pub git_sha1: Option<String> }

#[derive(Serialize, Clone, Debug)]
pub struct HubFile {
    pub path: String,          // the single file, or part 1 of a split GGUF
    pub size: u64,             // sum of all parts
    pub sha256: Option<String>,// None = masked (gated without access)
    pub parts: Vec<HubPart>,   // every file to download; parts[0].path == path
}

#[derive(Serialize, Clone, Debug)] #[serde(tag = "kind", rename_all = "snake_case")]
pub enum MlxProblem {
    NoConfig,
    BadConfig,                              // config.json didn't parse or has no model_type
    NoWeights,
    IncompleteShards { missing: Vec<String> },
    NoTokenizer,
    HashesHidden,                           // gated, no access yet
}

#[derive(Serialize, Clone, Debug)]
pub struct HubMlx {
    pub files: Vec<HubPart>,                // the allow-listed root files (ADR-0025 §5)
    pub size: u64,
    pub model_type: Option<String>,         // from config.json
    pub quantization: Option<String>,       // e.g. "4-bit" from config.json "quantization.bits"
    pub problem: Option<MlxProblem>,        // None = runnable (architecture is checked by the app)
}

#[derive(Serialize, Clone, Debug)]
pub struct HubRepo {
    pub model: HubModel,
    pub revision: String,
    pub files: Vec<HubFile>,                // GGUF choices: singles and complete shard groups, smallest first
    pub mlx: Option<HubMlx>,                // Some only when model.mlx
    pub access: Access,
    pub gated_kind: Option<String>,         // "auto" | "manual" | None
    pub base_model: Option<String>,         // cardData.base_model (first if a list)
}

#[derive(Debug, thiserror::Error)]
pub enum HubError {
    #[error("Hugging Face returned HTTP {0}")] Http(u16),
    #[error("could not reach Hugging Face: {0}")] Network(String),
    #[error("{0}")] Invalid(String),
    #[error("{0} wasn't found on Hugging Face, or it's private.")] NotFound(String),
    #[error("Hugging Face didn't accept that token. Copy it again from huggingface.co/settings/tokens.")] BadToken,
}

/// Unchanged signature. Tokenless (R4).
pub fn search(query: &str, limit: usize, token: Option<&str>, mlx: bool) -> Result<Vec<HubModel>, HubError>;

/// Metadata first, without a token. If the repo is gated and `token` is Some,
/// the tree (and config.json/index for MLX) are fetched with it. Tree is
/// `{API}/{repo}/tree/{rev}?recursive=true&expand=false`, paged by `Link: rel="next"`
/// (only URLs starting with https://huggingface.co/api/models/, at most 20 pages).
/// For MLX repos, `config.json` (cap 1 MiB) and `model.safetensors.index.json`
/// (cap 8 MiB) are read from `https://huggingface.co/{repo}/resolve/{rev}/...`.
pub fn repo(repo: &str, token: Option<&str>) -> Result<HubRepo, HubError>;

/// GGUF builds of `base_model` (`filter=base_model:quantized:<base>&filter=gguf`,
/// sort by downloads), at most `limit` (clamped 1..=10). The base model's own
/// publisher first, then ggml-org, unsloth, bartowski, lmstudio-community, then
/// the rest by downloads. Tokenless.
pub fn gguf_alternatives(base_model: &str, limit: usize) -> Result<Vec<HubModel>, HubError>;

/// `GET https://huggingface.co/api/whoami-v2` with the token. Ok(name) or BadToken.
pub fn whoami(token: &str) -> Result<String, HubError>;

/// `choice` is a GGUF `HubFile.path` or `MLX_CHOICE`. Refuses: restricted licence
/// without acceptance; any part with no hash; an MLX problem. Ids:
///   single GGUF  -> "hf:<repo>/<file path without .gguf>"
///   split GGUF   -> "hf:<repo>/<prefix basename>" (no folder, no -0000k-of-0000N)
///   MLX          -> "hf:<repo>/@mlx"
/// Sets `gated` from the repo. Writes with `save_user_entry`.
pub fn add_to_catalog(store: &ModelStore, repo: &HubRepo, choice: &str, accepted_license: bool)
    -> Result<CatalogEntry, HubError>;

/// Add or replace (same id) an entry in user-catalog.json (atomic write: temp file + rename).
pub fn save_user_entry(store: &ModelStore, entry: CatalogEntry) -> Result<(), HubError>;
/// Drop an entry from user-catalog.json. Ok(false) if it wasn't there. Never touches model files.
pub fn remove_user_entry(store: &ModelStore, id: &str) -> Result<bool, HubError>;

// Pure helpers, unit-tested:
pub struct Shard { pub prefix: String, pub index: u32, pub count: u32 }
pub fn parse_shard(file_name: &str) -> Option<Shard>;   // "<prefix>-00001-of-00003.gguf"
pub fn is_helper_gguf(path: &str) -> bool;               // mmproj, mtp-, MTP/, imatrix, ggml-vocab, -draft, lora, adapter
pub fn mlx_allowed(path: &str) -> bool;                   // ADR-0025 §5 allow-list, root files only
```

`HubModel` is unchanged. A masked hash is anything that isn't exactly 64 hex characters; it becomes `None`. Access: gated and masked without a token is `NeedsToken`; with a token, `NoAccessYet`; otherwise `Open`. A 401 or 404 on the metadata of a repo is `NotFound`.

### 5. Adding from this computer (track A, `local.rs`)

```rust
#[derive(Deserialize, Serialize, Clone, Copy, PartialEq, Eq, Debug)] #[serde(rename_all = "snake_case")]
pub enum ImportMode { Copy, InPlace }

#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)] #[serde(rename_all = "snake_case")]
pub enum ImportKind { Gguf, Mlx }

#[derive(Serialize, Clone, Debug)]
pub struct ImportFile { pub path: String, pub size: u64 }

#[derive(Serialize, Clone, Debug)]
pub struct ImportPreview {
    pub kind: ImportKind,
    pub name: String,              // shown name, e.g. "Qwen3-4B-Q4_K_M"
    pub id: String,                // "local/<safe name>" (a "-2", "-3" suffix if taken)
    pub files: Vec<ImportFile>,    // what would be copied or used
    pub total_bytes: u64,
    pub model_type: Option<String>,// MLX only
    pub same_drive: bool,          // copy can be a hard link (no extra space)
    pub free_bytes: Option<u64>,   // free space where the store is
}

#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("OpenBark can't find {0}.")] NotFound(String),
    #[error("Give the full path, starting from the top of the drive.")] NotAbsolute,
    #[error("That isn't a GGUF model file.")] NotGguf,
    #[error("That's a helper file ({0}), not a model. Choose the main model file.")] Helper(String),
    #[error("Some parts of this model are missing from the folder: {}.", .0.join(", "))] MissingParts(Vec<String>),
    #[error("That folder isn't an MLX model OpenBark can run: {0}")] NotMlx(String),
    #[error("Not enough space: this needs {need} bytes and {available} are free.")] DiskSpace { need: u64, available: u64 },
    #[error(transparent)] Io(#[from] std::io::Error),
}

/// Look without changing anything: a .gguf file (GGUF magic, version 2 or 3; a
/// shard brings its siblings), or a folder that passes the MLX runnable checks.
/// The path must be absolute; it is canonicalised.
pub fn inspect(store: &ModelStore, path: &Path) -> Result<ImportPreview, ImportError>;

/// Copy: into <root>/local/<name>/ via <root>/local/.<name>.tmp then rename; hard
/// link first on the same drive; SHA-256 (and git SHA-1 not needed) while copying;
/// 1 GiB free-space margin. InPlace: nothing is written except the catalog entry,
/// with Source::Local{path: Some(abs)} and each file's size and SHA-256 recorded.
/// Progress: (file, done, total) as in download.
pub fn import(store: &ModelStore, path: &Path, mode: ImportMode, progress: &mut dyn crate::Progress)
    -> Result<CatalogEntry, ImportError>;
```

Entries: `slot: Llm`, `license: "unknown"`, `attribution: None`, `display_name: "<name>"`, layout `LlmGguf { model }` or `LlmMlx { config: "config.json" }`.

### 6. Removing (track A, `remove.rs`)

```rust
#[derive(Serialize, Clone, Debug, Default)]
pub struct Removal {
    pub id: String,
    pub freed_bytes: u64,
    pub files_removed: usize,
    pub forgotten: bool,       // dropped from user-catalog.json
    pub kept_in_place: bool,   // used in place: nothing deleted
}

#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)] #[serde(rename_all = "snake_case")]
pub enum RemoveRule {
    Delete,            // built-in or Hub: delete files, keep the row
    DeleteAndForget,   // copied from this computer: delete the copy, drop the row
    ForgetOnly,        // used in place: delete nothing, drop the row
}

#[derive(Debug, thiserror::Error)]
pub enum RemoveError {
    #[error("OpenBark won't delete {0}: it's outside the models folder.")] OutsideStore(String),
    #[error("OpenBark won't delete {0}: it's a link to somewhere else.")] Link(String),
    #[error(transparent)] Unsafe(#[from] crate::store::UnsafePath),
    #[error(transparent)] Io(#[from] std::io::Error),
}

pub fn rule(entry: &CatalogEntry) -> RemoveRule;

/// Delete exactly the entry's listed files and their `.part` files, then empty
/// folders up to, never including, the store root. Each path: safe_rel_path,
/// symlink_metadata (refuse links and junctions), canonicalise, must start with
/// the canonical root and differ from it. In place: Ok with kept_in_place and
/// nothing touched. Missing files are fine (freed counts what was there).
pub fn remove_files(store: &ModelStore, entry: &CatalogEntry) -> Result<Removal, RemoveError>;

/// Drop from user-catalog.json (hub::remove_user_entry). Never deletes files.
pub fn forget(store: &ModelStore, id: &str) -> Result<bool, RemoveError>;

/// rule(entry) applied: Delete -> remove_files; DeleteAndForget -> remove_files + forget;
/// ForgetOnly -> forget. `forget_too` adds a forget for a Hub entry ("Remove from list");
/// it is ignored for built-ins.
pub fn remove(store: &ModelStore, entry: &CatalogEntry, builtin: bool, forget_too: bool)
    -> Result<Removal, RemoveError>;
```

The guards (note model, busy, downloading, needed speech models) are **not** here; the app applies them (contract 10).

### 7. Hugging Face token (track A, `os-engine/src/secrets.rs`)

```rust
pub const HF_TOKEN_ACCOUNT: &str = "huggingface-token";   // keychain service "OpenBark"

#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)] #[serde(rename_all = "snake_case")]
pub enum TokenSource { Server, Keychain, VaultFile, Session }

pub fn hf_token() -> Option<(String, TokenSource)>;  // env OPENBARK_HF_TOKEN (Server); keychain; vault file; session
pub fn set_hf_token(token: &str) -> std::io::Result<TokenSource>; // keychain, else vault file `<local data>/OpenBark/hf-token.obk` when vault::enabled(), else session memory
pub fn clear_hf_token() -> std::io::Result<()>;     // all three stores
pub fn plausible_token(token: &str) -> bool;         // 8..=256 printable ASCII, no whitespace

// For tests: the same logic over a backend.
pub trait SecretBackend: Send + Sync { fn get(&self, account: &str) -> Result<Option<String>, String>; fn set(&self, account: &str, value: &str) -> Result<(), String>; fn delete(&self, account: &str) -> Result<(), String>; }
pub struct MemoryBackend; // in-memory, for tests
```

Never log or `Debug`-print a token (no `#[derive(Debug)]` on anything holding one). The file fallback uses `vault::write`/`vault::read` only; never plain text.

### 8. MLX engine pack (track B, `os-models/src/mlx_pack.rs`)

Lock file format (both lock files; generated, never hand-edited):

```json
{
  "format": 1,
  "pack": "mlx-engine-pack",
  "target": "aarch64-apple-darwin",
  "macos_min": "14.0",
  "generated_by": "scripts/mlx-pack-lock.py, uv 0.13.1",
  "generated_on": "2026-10-18",
  "exclude_newer": "2026-10-11T00:00:00Z",
  "python": { "version": "3.12.15", "release": "20261009",
              "file": "cpython-3.12.15+20261009-aarch64-apple-darwin-install_only.tar.gz",
              "url": "https://github.com/astral-sh/python-build-standalone/releases/download/20261009/cpython-3.12.15%2B20261009-aarch64-apple-darwin-install_only.tar.gz",
              "size": 25165569, "sha256": "83dcb04777c798bd95f3687acf9ae84b09860272ee6bdfb05be8bcf9447f75b8", "root": "python" },
  "wheels": [ { "name": "mlx", "version": "0.32.3", "file": "mlx-0.32.3-cp312-cp312-macosx_14_0_arm64.whl",
                "url": "https://files.pythonhosted.org/packages/...", "size": 651041,
                "sha256": "7aecc3d4a904279492dae58e28378a3c4bec4533e7066e64ae5c78c7a5511f22", "unpacked": 0 } ],
  "mlx_lm_architectures": ["gemma3", "gemma3_text", "gemma4", "llama", "mistral", "qwen3", "..."],
  "unpacked_bytes": 0
}
```

(Python values above were checked live on 2026-10-18 against `SHA256SUMS` and GitHub's asset digest; the mlx wheel against PyPI. The script fills the rest.) The Linux lock has `"target": "x86_64-unknown-linux-gnu"`, `"macos_min": null`, the `cpython-3.12.15+20261009-x86_64-unknown-linux-gnu-install_only.tar.gz` archive (66,913,335 bytes, SHA-256 `5864001bf969db329fadc2b654c4878c6027d170c6bd77e28f88666260bcaf63`) and `mlx-cpu` instead of `mlx-metal`.

```rust
pub const PACK_ID: &str = "mlx-engine-pack";   // download folder <models>/mlx-engine-pack

#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)] #[serde(rename_all = "snake_case")]
pub enum Support { Yes, TestMode, NeedsAppleSilicon, NeedsMacos14, NotMac }

#[derive(Deserialize, Serialize, Clone, Debug)] pub struct Lock { /* the fields above */ }
#[derive(Deserialize, Serialize, Clone, Debug)] pub struct LockFile { pub name: String, pub version: String, pub file: String, pub url: String, pub size: u64, pub sha256: String, #[serde(default)] pub unpacked: u64 }

pub fn support() -> Support;              // macOS aarch64 >= 14 -> Yes; macOS x86_64 -> NeedsAppleSilicon;
                                          // macOS < 14 -> NeedsMacos14; Linux x86_64 + OPENBARK_MLX_DEV=1 -> TestMode; else NotMac
pub fn macos_version() -> Option<String>; // sysinfo::System::os_version() on macOS
pub fn chip() -> String;                  // sysinfo CPU brand, e.g. "Apple M3"
pub fn lock() -> Option<&'static Lock>;   // the mac lock for Yes, the Linux lock for TestMode, else None
pub fn lock_id() -> Option<String>;       // first 12 hex of SHA-256 of the lock file's text
pub fn entry() -> Option<CatalogEntry>;   // Source::Url, Layout::Archives, files = python + wheels (path = file name)
pub fn engines_dir() -> PathBuf;          // OPENBARK_ENGINES_DIR or <local data>/OpenBark/engines
pub fn pack_dir() -> Option<PathBuf>;     // <engines>/mlx/<lock id>
pub fn python() -> Option<PathBuf>;       // <pack>/python/bin/python3
pub fn installed() -> bool;               // <pack>/manifest.json exists and its lock_id matches
/// Unpack the downloaded files from <models>/mlx-engine-pack into <engines>/mlx/<lock id>.tmp,
/// then rename into place and delete the downloads. Python: tar + flate2, refusing absolute
/// paths, "..", hard links, and symlinks whose target leaves the folder; keeps unix modes.
/// Wheels: zip into python/lib/python3.12/site-packages, refusing unsafe names and any
/// "*.data/" entry. Then `python3 -I -m compileall -q <site-packages>`. Writes manifest.json
/// {lock_id, installed_at, files, unpacked_bytes}. progress(done, total) counts files.
pub fn unpack(store: &ModelStore, progress: impl FnMut(u64, u64)) -> std::io::Result<()>;
/// Delete <engines>/mlx/<lock id> (and <models>/mlx-engine-pack leftovers). Same containment rules
/// as remove.rs: canonical path strictly inside canonical engines_dir, not a link.
/// Returns bytes freed. Also clears this lock's entries in checks.json.
pub fn remove(store: &ModelStore) -> std::io::Result<u64>;
pub fn disk_bytes() -> u64;               // size of <engines>/mlx/<lock id>
pub fn tidy();                            // start-up: delete other lock ids' folders and *.tmp under <engines>/mlx
pub fn architectures() -> &'static [String];
pub fn architecture_supported(model_type: &str) -> bool;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
pub struct CheckKey { pub app_version: String, pub lock_id: String, pub os_version: String, pub chip: String, pub subject: String }
// subject: "pack", or "<model id>@<revision or size+mtime of config.json for local>"
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CheckRecord { pub ok: bool, pub at: String, pub message: Option<String> }
pub fn checks_file() -> PathBuf;          // <engines>/mlx/checks.json
pub fn check_result(key: &CheckKey) -> Option<CheckRecord>;
pub fn record_check(key: &CheckKey, ok: bool, message: Option<&str>) -> std::io::Result<()>;
```

### 9. MLX backend and worker (track B, `os-llm/src/mlx.rs`, `mlx_worker.py`)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MlxDevice { Auto, Cpu }                // Auto = Metal when available

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelCheck { Run, AlreadyPassed }      // AlreadyPassed: the app's checks.json says so

#[derive(Debug, Clone)]
pub struct MlxOptions {
    pub python: PathBuf,        // the pack's python3 (tests: any python3 with mlx, or the fake worker's)
    pub work_dir: PathBuf,      // the pack folder: worker written to <work_dir>/worker/openbark_mlx_worker.py,
                                // HF_HOME = <work_dir>/hf-home (empty), cwd = work_dir
    pub model_dir: PathBuf,     // absolute folder with config.json
    pub model_name: String,     // for capabilities().model_name
    pub device: MlxDevice,
    pub sandbox: bool,          // macOS: wrap in /usr/bin/sandbox-exec (deny network*)
    pub model_check: ModelCheck,
    pub max_context: u32,       // cap on the model's own context length (default 32768)
    pub worker_script: Option<PathBuf>, // tests only: run this script instead of the embedded worker
}

pub const WORKER_SOURCE: &str = include_str!("mlx_worker.py");
pub const PROTOCOL: u32 = 1;
/// Write WORKER_SOURCE to <work_dir>/worker/openbark_mlx_worker.py unless identical bytes are there.
pub fn write_worker(work_dir: &Path) -> std::io::Result<PathBuf>;

#[derive(Debug, Clone, Serialize)]
pub struct PackCheck {
    pub ok: bool,
    pub device: String,                  // "gpu" | "cpu"
    pub metal: bool,
    pub versions: std::collections::BTreeMap<String, String>, // python, mlx, mlx_lm, transformers, llguidance
    pub checks: Vec<(String, bool)>,     // imports, socket_guard, metal, matmul, mask
    pub message: Option<String>,         // first failure, plain words, no stack trace
}

pub struct MlxBackend { /* Mutex<Worker>, caps, ... */ }

impl MlxBackend {
    /// Spawn, hello, load, then the model check unless AlreadyPassed.
    pub fn load(opts: MlxOptions) -> Result<Self, LlmError>;
    /// Spawn a worker, run selftest, shut it down. Used after install and by "Try again".
    pub fn pack_check(python: &Path, work_dir: &Path, device: MlxDevice, sandbox: bool) -> Result<PackCheck, LlmError>;
    /// Some(passed) when load() ran the model check; None when it was AlreadyPassed.
    pub fn model_check_ran(&self) -> Option<bool>;
}
impl LlmBackend for MlxBackend { /* capabilities(), generate() */ }
impl Drop for MlxBackend { /* send shutdown, wait 2 s, then kill */ }
```

- `capabilities()`: `backend: "mlx"`, `streaming: true`, `audio_input: false`, `context_length = min(loaded.context_length, max_context)`, `json_schema_constrained = loaded.constrained_ready && model check passed (now or AlreadyPassed)`.
- `generate()` with `json_schema: Some(_)` while not constrained returns `LlmError::Grammar("this model can't be held to the note's format on MLX")` without generating. One request at a time (internal `Mutex`). When `on_token` returns false, send `cancel` and keep reading until `done`.
- If the worker dies (EOF on stdout) the call fails with `LlmError::Inference("the MLX engine stopped")`; the next `generate` restarts the worker and reloads the model once before failing again. No message for 120 s during a generation kills the worker and fails the same way.
- **Spawn** (macOS): `/usr/bin/sandbox-exec -p '(version 1)(allow default)(deny network*)' <python> -I -B -X utf8 -u <worker.py>`; without sandbox (Linux, `OPENBARK_MLX_SANDBOX=0`): `<python> -I -B -X utf8 -u <worker.py>`. `env_clear()`, then only: `PATH=/usr/bin:/bin`, `HOME`, `HF_HOME=<work_dir>/hf-home`, `HF_HUB_OFFLINE=1`, `TRANSFORMERS_OFFLINE=1`, `HF_DATASETS_OFFLINE=1`, `HF_HUB_DISABLE_TELEMETRY=1`, `HF_HUB_DISABLE_IMPLICIT_TOKEN=1`, `DO_NOT_TRACK=1`, `PYTHONNOUSERSITE=1`, `TOKENIZERS_PARALLELISM=false`, `NO_PROXY=*`, `OPENBARK_MLX_DEVICE` (if set). On Windows (fake-worker tests only) also `SYSTEMROOT`. stdin, stdout, stderr piped; stderr lines go to `tracing::debug!(target: "mlx_worker")`, each cut to 300 characters.

**Worker protocol** (JSON Lines, UTF-8, one object per line; requests on stdin, replies on the worker's protocol stream, which is a dup of fd 1 made before fd 1 is pointed at stderr, so library prints can't corrupt it):

| Request | Replies |
|---|---|
| `{"op":"hello","protocol":1}` | `{"event":"hello","protocol":1,"python":"3.12.15","versions":{"mlx":"0.32.3","mlx_lm":"0.32.0","transformers":"5.19.0","llguidance":"1.9.1"},"device":"gpu","metal":true}` |
| `{"id":1,"op":"selftest"}` | `{"id":1,"event":"selftest","ok":true,"checks":[["imports",true],["socket_guard",true],["metal",true],["matmul",true],["mask",true]],"message":null}` |
| `{"id":2,"op":"load","path":"/abs/model/dir"}` | `{"id":2,"event":"loaded","model_type":"qwen3","context_length":40960,"eos_token_ids":[151645,151643],"vocab_size":151936,"constrained_ready":true,"load_ms":2140}` |
| `{"id":3,"op":"generate","messages":[{"role":"system","content":"..."},{"role":"user","content":"..."}],"max_tokens":2048,"temperature":0.2,"seed":42,"json_schema":"{...}" or null}` | zero or more `{"id":3,"event":"token","text":"..."}`, then `{"id":3,"event":"done","text":"<all text>","finish":"stop"|"length"|"cancelled","stats":{"prompt_tokens":812,"completion_tokens":403,"time_to_first_token_ms":950,"total_ms":14100,"tokens_per_second":30.6,"hit_max_tokens":false,"peak_memory_bytes":2900000000}}` |
| `{"op":"cancel","id":3}` | the running generate ends with `"finish":"cancelled"` |
| `{"id":4,"op":"unload"}` | `{"id":4,"event":"unloaded"}` |
| `{"op":"shutdown"}` | exits 0 (also on stdin EOF) |
| any failure | `{"id":n,"event":"error","kind":"bad_request"|"load"|"unsupported_architecture"|"template"|"grammar"|"context_overflow"|"inference","message":"...","prompt_tokens":0,"max_new":0,"context":0}` (the three numbers only for `context_overflow`) |

- `kind` maps to `LlmError`: `load`/`unsupported_architecture` -> `Load`, `template` -> `Template`, `grammar` -> `Grammar`, `context_overflow` -> `ContextOverflow{prompt,max_new,ctx}`, others -> `Inference`. Error messages never quote request content.
- `stats` map one to one onto `GenerateStats` (`peak_memory_bytes` is logged, not stored).
- **Worker rules:** the first statements, before any other import, replace `socket.socket.connect`, `socket.socket.connect_ex`, `socket.create_connection` and `socket.getaddrinfo` with functions that raise; it refuses to start unless `HF_HUB_OFFLINE=1`; `load` accepts only an absolute existing folder (never a repo id) and never `trust_remote_code`; on the CPU device it calls `model.set_dtype(mx.float32)`; the chat template is applied with `add_generation_prompt=True` and `enable_thinking=False`; the schema processor is llguidance (`LLMatcher.grammar_from_json_schema(schema, defaults={"whitespace_flexible": False})`, tokenizer from `llguidance.hf.from_tokenizer(tw._tokenizer, eos_token=<all eos ids>)`), applied as an mlx-lm `logits_processors` entry that consumes tokens after the prompt, with `llguidance.mlx.apply_token_bitmask` on Metal and the plain-MLX unpack on CPU (ADR research sketch); `constrained_ready` is false when llguidance or a fast tokenizer can't be built; the stdin reader thread sets a cancel flag checked between tokens; it never writes prompts or outputs anywhere but the protocol stream.
- **Model check** (in `load` when `ModelCheck::Run`): generate with `{"type":"object","properties":{"ok":{"type":"boolean"}},"required":["ok"]}`, `max_tokens: 32`, `temperature: 0`; passes when the text parses as JSON with a boolean `ok`.

### 10. App layer (integrator, `os-app`)

```rust
// Replaces Mutex<Option<(String, Arc<LlamaCppBackend>)>>.
llm: Mutex<Option<(String, Arc<dyn LlmBackend>)>>,
downloading: Mutex<HashSet<String>>,   // ids with a running download or import

#[derive(Serialize)] #[serde(rename_all = "snake_case")]
pub enum Origin { Builtin, Hub, LocalCopy, LocalInPlace }

#[derive(Serialize)] #[serde(rename_all = "snake_case")]
pub enum RowRemove { Delete, DeleteAndForget, ForgetOnly, Needed, WithGpuPack }

#[derive(Serialize)]
pub struct ModelRow {
    pub id: String, pub name: String, pub publisher: String, pub slot: Slot, pub size_bytes: u64,
    pub license: String, pub installed: bool, pub fit: Fit, pub builtin: bool,
    // new:
    pub disk_bytes: u64,            // store.disk_usage (0 in place)
    pub partial_bytes: u64,         // a stopped download
    pub origin: Origin,
    pub engine: Option<os_models::LlmEngine>, // llm rows only
    pub parts: usize,               // number of files
    pub remove: RowRemove,          // Needed for VAD, DEFAULT_STT, SEGMENTATION, EMBEDDING and live::SECOND_STT
                                    // (NOT DEFAULT_LLM: the default note model can be removed like any other);
                                    // WithGpuPack for GPU_STT / GPU_SECOND_STT; else remove::rule(entry)
    pub missing: bool,              // in place and the path is gone
    pub gated: bool,
    pub path: Option<String>,       // in place: where it is
}
// publisher: "This computer" for Source::Local.

#[derive(Serialize)] #[serde(rename_all = "snake_case")]
pub enum CheckState { NotRun, Passed, Failed }

#[derive(Serialize)]
pub struct MlxStatus {
    pub support: os_models::mlx_pack::Support,
    pub macos_version: Option<String>,
    pub pack_installed: bool,
    pub installing: bool,
    pub check: CheckState,
    pub check_message: Option<String>,
    pub metal: bool,
    pub download_bytes: u64,          // still to download (0 when installed)
    pub installed_bytes: u64,         // mlx_pack::disk_bytes()
    pub versions: BTreeMap<String, String>, // python, mlx, mlx_lm, llguidance (from the lock)
    pub in_use: bool,                 // an MLX model is loaded now
}

#[derive(Serialize)]
pub struct HfTokenStatus { pub saved: bool, pub source: Option<os_engine::secrets::TokenSource>, pub user: Option<String> }
// user: the name from whoami when the token was saved in this run; null otherwise (no call on status).

// GpuStatus gains:
pub installed_bytes: u64,   // the pack's libraries plus the two full-precision models on disk

#[derive(Serialize)]
pub struct HubFileView { pub path: String, pub size_bytes: u64, pub fit: Fit, pub parts: usize }
#[derive(Serialize)]
pub struct HubMlxView {
    pub size_bytes: u64, pub files: usize, pub fit: Fit,
    pub model_type: Option<String>, pub quantization: Option<String>,
    pub problem: Option<os_models::hub::MlxProblem>,
    pub arch_supported: bool,          // mlx_pack::architecture_supported (false when model_type is None)
    pub support: os_models::mlx_pack::Support,
    pub pack_installed: bool,
}
#[derive(Serialize)]
pub struct HubRepoView { pub repo: HubRepo, pub files: Vec<HubFileView>, pub mlx: Option<HubMlxView> }

impl App {
    pub fn models(&self) -> Vec<ModelRow>;
    pub fn download_model(&self, id: &str, token: Option<&str>, events: &dyn Events) -> Result<()>; // token None -> saved token
    pub fn remove_model(&self, id: &str, note_model: Option<&str>, forget: bool, by: Option<&str>) -> Result<Removal>;
    pub fn inspect_import(&self, path: &str) -> Result<ImportPreview>;
    pub fn import_model(&self, path: &str, mode: ImportMode, by: Option<&str>, events: &dyn Events) -> Result<ModelRow>;
    pub fn mlx_status(&self) -> MlxStatus;
    pub fn install_mlx(&self, events: &dyn Events) -> Result<MlxStatus>;   // download, unpack, pack_check, record
    pub fn check_mlx(&self) -> Result<MlxStatus>;                          // "Try again"
    pub fn remove_mlx(&self, note_model: Option<&str>, by: Option<&str>) -> Result<Removal>; // id "mlx-engine-pack"
    pub fn remove_gpu(&self, by: Option<&str>) -> Result<GpuStatus>;       // gains `by`
    pub fn hf_token_status(&self) -> HfTokenStatus;
    pub fn set_hf_token(&self, token: &str, by: Option<&str>) -> Result<HfTokenStatus>; // whoami first, then save
    pub fn clear_hf_token(&self, by: Option<&str>) -> Result<HfTokenStatus>;
    pub fn hub_repo(&self, repo: &str) -> Result<HubRepoView>;            // saved token for gated repos
    pub fn hub_add(&self, repo: &str, file: &str, accept_license: bool) -> Result<ModelRow>; // file may be "@mlx"
    pub fn hub_alternatives(&self, base_model: &str) -> Result<Vec<HubModel>>;
}
```

**Guards, in this order** (each an `AppError` with this exact text):

| Check | Message |
|---|---|
| team set up and `by` isn't a veterinarian (new `Need::Models`, same rule as `Need::Team`) | "Only a veterinarian can change the models." |
| `begin_work()` fails | (its existing messages) |
| id not in the catalog | "No model called {id} in the catalog." |
| `remove == Needed` | "OpenBark needs this to transcribe visits." |
| `remove == WithGpuPack` | "This comes with the GPU speech pack. Remove the pack instead." |
| id in `downloading` | "This model is still downloading." |
| `note_model == Some(id)` | "This model writes your notes. Choose another one under In use first." |
| the loaded model is this id and `Arc::strong_count(&backend) > 1` (a draft, refinement or template-from-example holds it) | "A note is being written with this model. Remove it when that's finished." |
| otherwise, if loaded, drop it from the slot first (while holding the slot lock, so nothing reloads it mid-removal) | |
| `remove_mlx`: note model is an MLX model | "Your note model uses the MLX engine. Choose another one under In use first." |
| `remove_mlx`: an MLX model is loaded and busy | "A note is being written with an MLX model. Remove the engine when that's finished." |

**Loading** (`load_llm`): `LlmGguf` -> `LlamaCppBackend` (part 1 path for split GGUFs); `LlmMlx` -> `MlxBackend` only when `support()` is `Yes` or `TestMode`, the pack is installed and its check passed (else "Download the MLX engine on the Models screen first."); a missing in-place model -> "{name} isn't where it was ({path}). Add it again from this computer.". After loading, a backend whose `capabilities().json_schema_constrained` is false is refused: "{name} didn't pass OpenBark's check, so it can't write notes." The model-check result is cached through `mlx_pack::record_check` and read back as `ModelCheck::AlreadyPassed`.

**Start-up:** `mlx_pack::tidy()` once (as `gpu_pack::loaded_at_start` does for the GPU pack).

### 11. Tauri commands and web routes (integrator)

| Tauri command (args, camelCase in TS) | HTTP route | Body / query | Returns |
|---|---|---|---|
| `models` | `GET /api/models` | | `ModelRow[]` |
| `download_model {id}` | `POST /api/models/{id}/download` | | `void` (events `download`) |
| `remove_model {id, noteModel, forget, by}` | `POST /api/models/remove` | `{id, note_model, forget, by}` | `Removal` |
| `inspect_import {path}` | `POST /api/models/import/inspect` | `{path}` | `ImportPreview` |
| `import_model {path, mode, by}` | `POST /api/models/import` | `{path, mode, by}` | `ModelRow` (events `download` with the new id while copying) |
| `mlx_status` | `GET /api/mlx` | | `MlxStatus` |
| `install_mlx` | `POST /api/mlx/install` | | `MlxStatus` (events `download` with `mlx-engine-pack`) |
| `check_mlx` | `POST /api/mlx/check` | | `MlxStatus` |
| `remove_mlx {noteModel, by}` | `POST /api/mlx/remove` | `{note_model, by}` | `Removal` |
| `remove_gpu {by}` | `POST /api/gpu/remove` | `{by}` (body optional) | `GpuStatus` |
| `hf_token_status` | `GET /api/hf-token` | | `HfTokenStatus` |
| `set_hf_token {token, by}` | `POST /api/hf-token` | `{token, by}` | `HfTokenStatus` |
| `clear_hf_token {by}` | `POST /api/hf-token/clear` | `{by}` | `HfTokenStatus` |
| `hub_repo {id}` | `GET /api/hub/repo?id=` | | `HubRepoView` |
| `hub_add {repo, file, acceptLicense}` | `POST /api/hub/add` | `{repo, file, accept_license}` | `ModelRow` |
| `hub_alternatives {base}` | `GET /api/hub/alternatives?base=` | | `HubModel[]` |

The desktop file picker is `@tauri-apps/plugin-dialog` `open()`, already allowed (visit import uses it); the integrator confirms `dialog:allow-open` covers `directory: true`.

### 12. TypeScript (track C, `engine.ts`)

```ts
export type Origin = "builtin" | "hub" | "local_copy" | "local_in_place";
export type RowRemove = "delete" | "delete_and_forget" | "forget_only" | "needed" | "with_gpu_pack";
export interface ModelRow {
  id: string; name: string; publisher: string; slot: Slot; size_bytes: number; license: string;
  installed: boolean; fit: Fit; builtin: boolean;
  disk_bytes: number; partial_bytes: number; origin: Origin; engine: "llama" | "mlx" | null;
  parts: number; remove: RowRemove; missing: boolean; gated: boolean; path: string | null;
}
export interface Removal { id: string; freed_bytes: number; files_removed: number; forgotten: boolean; kept_in_place: boolean }

export type MlxSupport = "yes" | "test_mode" | "needs_apple_silicon" | "needs_macos14" | "not_mac";
export interface MlxStatus {
  support: MlxSupport; macos_version: string | null; pack_installed: boolean; installing: boolean;
  check: "not_run" | "passed" | "failed"; check_message: string | null; metal: boolean;
  download_bytes: number; installed_bytes: number; versions: Record<string, string>; in_use: boolean;
}
// GpuStatus gains: installed_bytes: number;

export type HubAccess = "open" | "needs_token" | "no_access_yet";
export type MlxProblem =
  | { kind: "no_config" } | { kind: "bad_config" } | { kind: "no_weights" }
  | { kind: "incomplete_shards"; missing: string[] } | { kind: "no_tokenizer" } | { kind: "hashes_hidden" };
export interface HubRepoView {
  repo: { model: HubModel; revision: string; access: HubAccess; gated_kind: "auto" | "manual" | null; base_model: string | null };
  files: { path: string; size_bytes: number; fit: Fit; parts: number }[];
  mlx: {
    size_bytes: number; files: number; fit: Fit; model_type: string | null; quantization: string | null;
    problem: MlxProblem | null; arch_supported: boolean; support: MlxSupport; pack_installed: boolean;
  } | null;
}
export const MLX_CHOICE = "@mlx";

export type ImportMode = "copy" | "in_place";
export interface ImportPreview {
  kind: "gguf" | "mlx"; name: string; id: string; files: { path: string; size: number }[];
  total_bytes: number; model_type: string | null; same_drive: boolean; free_bytes: number | null;
}
export interface HfTokenStatus { saved: boolean; source: "server" | "keychain" | "vault_file" | "session" | null; user: string | null }

export const removeModel = (id: string, noteModel: string | null, forget = false): Promise<Removal>;
export const inspectImport = (path: string): Promise<ImportPreview>;
export const importModel = (path: string, mode: ImportMode): Promise<ModelRow>;
export const pickModelFile = (): Promise<string | null>;   // desktop: dialog open, filter gguf; web: null
export const pickModelFolder = (): Promise<string | null>; // desktop: dialog open {directory: true}; web: null
export const mlxStatus = (): Promise<MlxStatus>;
export const installMlx = (): Promise<MlxStatus>;
export const checkMlx = (): Promise<MlxStatus>;
export const removeMlx = (noteModel: string | null): Promise<Removal>;
export const removeGpu = (): Promise<GpuStatus>;            // now sends { by: actor }
export const hfTokenStatus = (): Promise<HfTokenStatus>;
export const setHfToken = (token: string): Promise<HfTokenStatus>;
export const clearHfToken = (): Promise<HfTokenStatus>;
export const hubAlternatives = (base: string): Promise<HubModel[]>;
```

Each follows the existing `isDesktop ? invoke(...) : api(...)` pattern with the commands and routes in contract 11; `by` is the module's `actor`.

## Track A: Hub, catalog, download, store, import, removal

Steps, each with tests in the same commit:

1. **Catalog types** (contract 1) and their tests: old user catalogs load; `LlmMlx` in the layout check; `git_sha1` round-trips; `Source::Local` JSON.
2. **Store** (contract 2): `dir_name`, `safe_rel_path`, `hash_file` (test vectors: `config.json` of `mlx-community/Qwen3-0.6B-4bit` gives git SHA-1 `6c974b02334f2f5f68de4cb2796ee4fe80bfc712`; "abc" gives SHA-256 `ba7816bf...15ad` and git SHA-1 `f2ba8f84ab5c1bce84a7b441cb1959cfc7093b7f`), legacy-folder fallback, `disk_usage`, `partial_bytes`. A Windows test that creates the folder for an `hf:` id (it failed before, WinError 267).
3. **Download** (contract 3): nested `.part` folders, unsafe paths refused, git SHA-1 verification, token only for gated entries, `GatedRepo` header read, new errors. Test against a local `TcpListener` HTTP stub (no internet in unit tests).
4. **Hub** (contract 4): recursive paged tree, shard grouping, helper exclusions, masked hashes, access states, MLX allow-list and runnable checks, config/index reads, `gguf_alternatives` ordering, `whoami`, ids, `save_user_entry`/`remove_user_entry`. Parse tests run on saved live responses in `engine/os-models/tests/fixtures/hub/` (start from the research probes, whose location is in the track brief, and save new ones with `curl` for: the 99-entry unsloth recursive tree, `unsloth/gemma-4-E4B-it-GGUF`, `google/gemma-3-1b-it-qat-q4_0-gguf` (masked), `mlx-community/Qwen3-0.6B-4bit` tree and config, `lmstudio-community/gemma-4-E4B-it-MLX-4bit` with its index). Keep the network calls behind a small `Fetch` trait so tests feed fixtures.
5. **Local import** (contract 5): GGUF magic and version, shard siblings, helpers refused, MLX folder checks (safetensors header: 8-byte little-endian length then `{`), copy via temp folder and rename, hard link on the same drive, in place records hashes and writes nothing else, name collisions, disk-space refusal. All in `tempfile` folders with small synthetic files.
6. **Removal** (contract 6): listed files and `.part` files deleted; a stranger's file in the same folder kept; empty folders pruned up to but not including the root; a symlink (Unix) or junction (Windows, `mklink /J` via `std::os::windows::fs::symlink_dir` or skip if not permitted) pointing outside refused; in place never touched (assert the file's bytes and mtime unchanged); built-in row survives (`full_catalog` still lists it); Hub `forget_too` drops it; local copy dropped.
7. **Token store** (contract 7) with `MemoryBackend` tests; env override; plausibility.
8. **Models-folder check** (`locations.rs`): count as OpenBark's the `hf_*` and `hf:*` folders, `local`, `mlx-engine-pack` (literal; track B's `PACK_ID`) and `gpu-speech-pack`. Test.
9. `cargo test -p os-models` and `cargo test -p os-engine secrets` pass on Windows and in WSL; `cargo clippy -p os-models -- -D warnings` clean.

## Track B: MLX runtime and pack

1. **Lock generator** `scripts/mlx-pack-lock.py` (run in WSL with uv 0.13.1; checks `uv --version`), inputs `scripts/mlx-pack.in` (`mlx-lm==0.32.0`, `llguidance==1.9.1`) and `scripts/mlx-pack-linux.in` (adds `mlx[cpu]==0.32.3`). It resolves with `MACOSX_DEPLOYMENT_TARGET=14.0 uv pip compile ... --python-platform aarch64-apple-darwin --python-version 3.12 --only-binary :all: --exclude-newer <today-7d> --format pylock.toml` (Linux: `--python-platform x86_64-manylinux_2_35`), picks one wheel per package (cp312, abi3 or py3; the most specific `macosx_X_Y_arm64` tag at or below 14.0), cross-checks each against `https://pypi.org/pypi/<name>/<version>/json` (url, size, sha256), takes python-build-standalone `20261009` and checks it against `SHA256SUMS` and the GitHub asset `digest`, with `--verify` downloads everything into a fresh scratch folder, re-hashes, refuses `.data/`, records unpacked sizes, lists `mlx_lm/models/*.py` and parses `MODEL_REMAPPING` in `mlx_lm/utils.py` with `ast` (never importing it) for `mlx_lm_architectures`, and writes both lock files and `docs/mlx-pack-pins.md` (a table: package, version, licence, file, size, SHA-256). A working prototype from the research exists; its location is in the track brief.
2. **`mlx_pack.rs`** (contract 8) with unit tests: lock shape (format 1, https only, hosts `files.pythonhosted.org` and `github.com` only, 64-hex hashes, unique names, python present, architectures non-empty, includes `qwen3` and `gemma4`), `entry()` file list, `support()` table via an injectable `(os, arch, version, dev)` helper, unpack on a synthetic tar.gz and wheel (absolute paths, `..`, outside symlinks and `.data/` refused; modes kept), `remove` containment, `tidy`, checks.json round trip.
3. **`mlx_worker.py`** (contract 9). Self-contained, standard library plus mlx, mlx_lm, transformers, llguidance. Start with the socket guard.
4. **`mlx.rs`** (contract 9): spawn, protocol types (serde, with unit tests for every message in the table), reader thread, cancel, timeouts, restart once, error mapping, `write_worker`, `pack_check`, Drop.
5. **Fake worker** `engine/os-llm/tests/mlx_fake_worker.py` (standard library only) that speaks the protocol and can be told (through the model folder name) to stream, fail each `kind`, hang, crash or ignore cancel. Tests in `engine/os-llm/tests/mlx_worker.rs` drive `MlxBackend` with `worker_script` set to it, using `python3` (WSL) or `python` (Windows): framing, streaming, cancel, every error mapping, crash then restart, timeout, unconstrained refusal, capabilities.
6. **Real-worker tests** in the same file, skipped unless `OPENBARK_MLX_TEST_PYTHON` and `OPENBARK_MLX_TEST_MODEL` are set: pack check passes on CPU, load `mlx-community/Qwen3-0.6B-4bit`, model check passes, a SOAP draft against `packages/templates/soap/schema.json` parses and validates (the `jsonschema` dev-dependency), cancel mid-draft, and the socket guard (a `selftest` with networking available still reports `socket_guard: true`). Also a golden test that every `packages/templates/*/schema.json` compiles in llguidance (a worker op is not needed: a small `scripts/mlx-test-env.sh` step runs the pack's python with llguidance on each schema).
7. **`scripts/mlx-test-env.sh`** (WSL): builds a scratch pack from the Linux lock using `cargo test -p os-models mlx_pack_install -- --ignored` (an ignored test that downloads through `os_models::download`, unpacks into `OPENBARK_ENGINES_DIR`, and prints the python path), downloads the test model (pinned revision `73e3e38d981303bc594367cd910ea6eb48349da8`) into a scratch folder with `curl` and checks sizes, then prints the two `OPENBARK_MLX_TEST_*` exports. Network only to the hosts in R1, R6 and B8.
8. `cargo test -p os-llm` (fake worker) passes on Windows and WSL; the real-worker tests pass in WSL; clippy clean.

## Track C: UI and docs

1. **`engine.ts`** (contract 12): types and calls. Keep existing exports working.
2. **Catalog rows** (`Models.tsx`): show "{size} on disk" for installed rows and "{size} partly downloaded" for stopped downloads; "This computer" publisher and **Used where it is** / **Missing** chips; "{n} parts" for split GGUFs and "MLX" chip for MLX rows (Mac or test mode). A **Remove** button (Lucide `Trash2`, ghost style) on every row with files on disk or in place, except `needed` (shows a muted "Needed to transcribe visits") and `with_gpu_pack` (hidden). The note model's Remove is disabled with "Writes your notes. Choose another model first." Hub rows get **Remove from list** in the dialog.
3. **`RemoveModelDialog.tsx`**: the copy below for each `RowRemove`, sends `noteModel = app.settings.llm`, shows engine refusals inline, then a toast "Removed. {size} freed." (or "Forgotten. Nothing was deleted.", "Removed from the list."), and reloads rows.
4. **`MlxCard.tsx`** (like `GpuCard.tsx`): hidden for `not_mac` unless the platform is `macos` (then it shows the `needs_*` explanation); states: not installed (Download with size), downloading (progress from `download` events for `mlx-engine-pack`), setting up, check failed (**Try again**), ready ("{size} on disk", **Remove**), test mode chip. Remove goes through `RemoveModelDialog` with the pack text. **GpuCard**: show "{size} on disk" and the freed toast on Remove.
5. **Hub sheet** (`HubBrowser` in `Models.tsx`): GGUF rows with "{n} parts"; the MLX row ("Whole model folder", "{n} files", size, fit, Download with `MLX_CHOICE`) when `mlx.support` is `yes`/`test_mode` and the pack is installed and `arch_supported` and no `problem`; otherwise the right explanation and, when the pack is missing, **Get the MLX engine** (scrolls to the card). On a Mac (or anywhere when `support` isn't `yes`), when an MLX repo is open and `base_model` is set, list `hubAlternatives(base_model)` as "GGUF builds of this model" (open on click). Gated: the access message, **Request access** (opens `https://huggingface.co/<repo>`), **Get a token** (opens `https://huggingface.co/settings/tokens`), and `HfTokenField`. Replace "This repository has no single-file GGUF weights OpenBark can run." with "No GGUF or MLX files OpenBark can run in this repository."
6. **`HfTokenField.tsx`**: password-type input, Save, Remove; states "Saved in this computer's keychain.", "Saved, encrypted, on this computer.", "Kept until OpenBark closes.", "Set by the server."; whoami name when known ("Signed in as {name}."); never shows the token back. Also reachable from a small "Hugging Face token" link in the Hub sheet footer.
7. **`AddLocalModelSheet.tsx`**: header button **Add from this computer** on the Models screen. Desktop: **Choose a .gguf file** and, on a Mac or test mode, **Choose an MLX folder**; web: a path field and **Check**. Then the preview (name, size, files), the mode choice (copy recommended; "Use it where it is, don't copy"), **Add**, copy progress from `download` events for `preview.id`, toast "Model added".
8. **Tests** (vitest, mocked `engine.ts`): every RemoveModelDialog variant and refusal; MlxCard states; Hub sheet MLX row vs explanations, shard parts, gated states and token field; Add sheet copy vs in place; note model's Remove disabled.
9. **i18n**: every new string through `t()`, translations added to `packages/i18n/i18n-{fr,es,de,pt}.json` and `python scripts/i18n-build.py packages/i18n/i18n-*.json` re-run (placeholders kept). Also the engine messages in this plan (they are shown as-is; add them so a translated UI can map them where the existing code passes errors through `t()`). Note in `docs/languages.md` that these new translations are unreviewed.
10. **Docs**: `docs/network.md` (R1 hosts `us.aws.cdn.hf.co` and `/api/resolve-cache/`, MLX model files, token only for gated repos; R4 recursive paged tree, `config.json`/index reads, GGUF alternatives query, token for gated repos only, `whoami-v2` once when saved; new **R6** MLX engine pack; new **B8** developer and CI MLX test downloads; a line that removal and import make no calls), `docs/versions.md` (python-build-standalone `20261009`, CPython 3.12.15; mlx/mlx-metal/mlx-cpu `0.32.3`, mlx-lm `0.32.0`, llguidance `1.9.1`, transformers `5.19.0`, tokenizers `0.23.3`, uv `0.13.1` build machine only, `sha1 =0.11.0`, `flate2 =1.1.10`; link to `docs/mlx-pack-pins.md` for the full wheel list), `docs/guide/models.md` (choosing, downloading, MLX on a Mac, gated models and tokens, adding from this computer, removing and getting a model back), links from the guide README, getting-started and troubleshooting.
11. `pnpm -C apps/desktop typecheck` and `pnpm -C apps/desktop test` pass. Screens follow `docs/brand/DESIGN.md`; check light and dark and phone width with the Vite dev server and mocks.

## Integrator

1. Merge A, B, C; resolve **Contract changes**; `cargo build` the workspace; regenerate `Cargo.lock` (only `sha1` and a direct `flate2` should be new).
2. `os-llm/src/lib.rs`: keep B's lines; nothing else unless needed.
3. `os-app` (contract 10): the `dyn LlmBackend` slot, loading by layout, the constrained guard, MLX status/install/check/remove, model removal guards, import, token, Hub view, `downloading` set, `Need::Models`, `GpuStatus.installed_bytes`, `remove_gpu(by)`, start-up `tidy`. Tests in `os-app/src/tests.rs` with the existing test-env traps (scratch users file, data key, locations file, `OPENBARK_ENGINES_DIR`): every guard, including "busy" (hold an `Arc` clone of the loaded backend) and "never cancels a running draft"; removal of a built-in leaves the row with Download; in place untouched.
4. `os-server` routes and `src-tauri` commands (contract 11); `remove_gpu` accepts an empty body.
5. `os-cli`: `models rm <id> [--forget]`, `models import <path> [--in-place]`, `models add-hf <repo> <file|@mlx> [--accept-license]`, `mlx install|check|remove|status`, and `--llm` accepting MLX ids (uses `MlxBackend`).
6. **End to end, Linux CPU in WSL** (`scripts/ci/mlx-e2e.sh`, scratch folders only): `OPENBARK_MLX_DEV=1`, `openbark mlx install`, `openbark mlx check`, `openbark models add-hf mlx-community/Qwen3-0.6B-4bit @mlx`, `openbark models pull hf:mlx-community/Qwen3-0.6B-4bit/@mlx`, then draft a fixture transcript under `unshare -rn` (no network) and validate the note against the SOAP schema; then `models rm` and `mlx remove` and check the folders are gone and the freed bytes match.
7. **Demo server** in WSL on port 4327 (`OPENBARK_PLATFORM=macos OPENBARK_MLX_DEV=1`, all scratch env vars) and a Playwright run `apps/desktop/e2e/models.mjs`: MLX card install, Hub MLX row download, GGUF shard row, gated repo states (with a fixture-free live repo, no token), Add from this computer (copy and in place, synthetic tiny GGUF from the test fixtures), Remove for each kind with the freed toast, the note-model refusal. Kill the server by exe path afterwards.
8. **CI**: a Linux job running step 6 (cache the pack downloads by lock id), and a `macos-15` job that installs the real macOS pack, runs `openbark mlx check` (Metal if available, else `OPENBARK_MLX_DEVICE=cpu`, recorded), and drafts with the 0.6B model under `sandbox-exec`. Both upload logs without patient text (fixtures only).
9. `scripts/ci/check-network.sh`, `cargo clippy --workspace -- -D warnings`, `cargo test` (Windows and WSL), `pnpm -C apps/desktop test` and `typecheck` all pass. Update this plan's status and the ADR's status to "accepted" when it ships.

## Copy

Plain, warm, short (brand guide §8). English source; translations in the catalogs.

### Models screen

- Row: "{size} on disk" · "{size} partly downloaded" · "This computer" · "Used where it is" · "Missing" · "{n} parts" · "{n} files" · "MLX"
- Buttons: "Remove" · "Remove from list" · "Forget" · "Add from this computer"
- Disabled: "Writes your notes. Choose another model first." · "Needed to transcribe visits"

### Remove dialog

| Case | Title | Body | Confirm |
|---|---|---|---|
| Built-in or Hub | "Remove {name}?" | "This frees {size} on this computer. You can download it again from the catalog." (Hub adds a checkbox: "Also remove it from the list") | "Remove" |
| Copied from this computer | "Remove {name}?" | "This deletes OpenBark's copy and frees {size}. The original file stays where it was." | "Remove" |
| Used where it is | "Forget {name}?" | "OpenBark will stop using this model. The file stays where it is, and nothing is deleted." | "Forget" |
| Stopped download | "Remove {name}?" | "This deletes the {size} downloaded so far." | "Remove" |
| MLX engine | "Remove the MLX engine?" | "This frees {size}. MLX models stay on this computer, but can't write notes until you download the engine again." | "Remove" |
| GPU speech pack | (existing) | adds "This frees {size}." | (existing) |

Toasts: "Removed. {size} freed." · "Forgotten. Nothing was deleted." · "Removed from the list."

### MLX engine card

- Title "MLX engine" · subtitle "Runs MLX models from Hugging Face on this Mac's graphics. GGUF models keep using the built-in engine."
- "Download ({size})" · "Downloading {progress}" · "Setting up" · "Checking" · "Ready" · "{size} on disk" · "Try again" · "Test mode" · "MLX on the processor, for testing only. It's slow."
- "MLX needs a Mac with Apple silicon (M1 or later). GGUF models work on this Mac."
- "MLX needs macOS 14 Sonoma or later. This Mac has macOS {version}. GGUF models work on this Mac."
- "The MLX engine didn't pass its check on this Mac, so MLX models can't write notes. GGUF models still work."

### Hub sheet

- MLX row: "Whole model folder" · "{n} files"
- "Download the MLX engine first. It's on the Models screen." · "Get the MLX engine"
- "MLX models run on a Mac with Apple silicon and macOS 14 or later."
- "GGUF builds of this model" · "They run with the built-in engine on any computer."
- "OpenBark's MLX engine can't run this kind of model yet ({type})."
- Problems: "This repository has no config.json, so it can't run." · "This repository's config.json can't be read." · "This repository has no safetensors weights." · "Some weight files are missing: {files}." · "This repository has no tokenizer files."
- "No GGUF or MLX files OpenBark can run in this repository."
- Gated: "This model is gated. Request access on Hugging Face, then add your token." · "Hugging Face hasn't given your token access yet. Check your request on the model's page." · "The publisher reviews requests by hand." · "Request access" · "Get a token" · "A Read token is enough."
- Token: "Hugging Face token" · "Save token" · "Remove token" · "Saved in this computer's keychain." · "Saved, encrypted, on this computer." · "Kept until OpenBark closes." · "Set by the server." · "Signed in as {name}." · "Hugging Face didn't accept that token. Copy it again from huggingface.co/settings/tokens."

### Add from this computer

- "Add from this computer" · "Choose a .gguf file" · "Choose an MLX folder" · "Path on the computer running OpenBark" · "Check"
- "Copy into OpenBark's models folder (recommended)" · "Use it where it is, don't copy"
- "Copying keeps the model working if the original moves. On the same drive, OpenBark links it instead, so it takes no extra space."
- "Used where it is, OpenBark never changes or deletes the file. If it moves, add it again."
- "Add" · "Copying {progress}" · "Model added"

### Engine messages

All `AppError`, `DownloadError`, `HubError`, `ImportError` and `RemoveError` texts in the contracts above.

## Order and rough size

| Track | Size | Depends on |
|---|---|---|
| A | about 2 days | nothing |
| B | about 3 days (lock script, worker, backend, tests in WSL) | nothing |
| C | about 2 days | nothing (mocks) |
| Integrator | about 2 days, plus CI time | A, B, C |

## Risks

- **Metal speed is unmeasured.** The llguidance mask syncs every token. Until a real Mac measures it (ADR-0009's comparison), the UI quotes no speed for MLX and keeps GGUF the default.
- **`sandbox-exec` is deprecated** by Apple but still present. If a macOS release removes it, the socket guard and offline environment remain; the spawn falls back without it and logs that.
- **Gatekeeper and code signing** for the downloaded Python: files written by OpenBark's own downloader carry no quarantine flag, and python-build-standalone binaries are ad-hoc signed, as arm64 requires. The macOS CI smoke proves this on a fresh runner.
- **transformers updates** can change tokenizer or chat-template behaviour. The pack is pinned, and the model check catches a model that stops producing valid JSON.
- **Schema features** handled differently by llguidance and llama.cpp's GBNF converter: the golden test compiles every template schema.
- **Web mode note-model check** uses the requesting browser's choice; another browser's choice isn't known. The busy and loaded checks still protect a running draft.
- **In-place models** can vanish. The row shows **Missing** and loading refuses them with a plain reason.

## Contract changes

(Append one line per change: date, track, contract number, what and why. The integrator resolves.)
- 2026-10-19, track A, contract 7: `MemoryBackend` holds its values, so it is `pub struct MemoryBackend { /* private */ }` with `MemoryBackend::new()` and `Default`, not a unit struct. Build it with `MemoryBackend::new()`.
- 2026-10-19, track B, contract 9 (additive): `MlxOptions::new(python, work_dir, model_dir, model_name)` gives the defaults (Auto, sandbox on, model check Run, `max_context` 32768, embedded worker), and `#[doc(hidden)] MlxBackend::set_silence_timeout(Duration)` exists for tests only. `hello` and `PackCheck.versions` also carry `tokenizers`.
- 2026-10-19, track B, contract 8 (additive): `mlx_pack::support_for(os, arch, macos_version, dev)` is the pure helper behind `support()`, and `mlx_pack::check_key(app_version, subject) -> Option<CheckKey>` builds a key for this computer and lock (os_version is the macOS version, else the long OS name).
- 2026-10-19, track B, tests only: new variable `OPENBARK_MLX_TEST_SCHEMAS` (a folder of `*.json`) adds schemas to the golden llguidance test in `engine/os-llm/tests/mlx_worker.rs`. os-llm can't build os-engine's generated schemas itself; the integrator should add an os-engine test or example that writes every built-in template's schema, `edit::editable_schema` for each, and the from-example draft schema to a folder for it (checked by hand on 2026-10-19: all 41 plus `soap/schema.json` compile with no errors).
- 2026-10-19, track B, contract 9 (protocol addition): while it reads the prompt the worker sends `{"id":n,"event":"progress","prompt_done":k,"prompt_total":N}` (mlx-lm's prompt progress, in steps of 64 tokens on the CPU and 512 on Metal), and a cancel stops it between steps. Measured in WSL: MLX's CPU device reads only about 4 to 5 prompt tokens a second (859 tokens took 202 s), so without these messages the 120 s silence rule would stop every real-length draft in test mode. `MlxBackend` treats them like any message (they reset the silence timer). The worker also samples like the built-in engine (greedy at temperature 0, else top-k 40, top-p 0.95, temperature).

- 2026-10-19, review fixes, contract 6: `Removal` gains `shared_bytes` (bytes deleted whose data another name still holds, so nothing freed for them); contract 10: `ModelRow` gains `shared_bytes`, and `disk_bytes` counts only data with no other name.
- 2026-10-19, review fixes, contract 5: `ImportError::Link(name)` ("{0} is a link to a file outside the folder you chose, so OpenBark won't add it."); links are resolved before copying or hashing.
- 2026-10-19, review fixes, contracts 10 and 11: `inspect_import(path, by)`, with the `Need::Models` check; the Tauri command and the route take `by`.
- 2026-10-19, review fixes, contract 9: the `load` request carries `max_context`, enforced by the worker.
- 2026-10-19, review fixes, contract 7: `hf_token()` reads only `OPENBARK_HF_TOKEN`; `OPENBARK_HF_TOKEN_FILE` redirects the store for scratch runs.

## Integration (2026-10-19)

Every contract change above is accepted as written. How the integrator resolved them, and what it decided beyond the contracts:

- **Contract 7:** the app and tests build `MemoryBackend::new()`. os-app's tests never touch the token (no keychain); the token logic is tested in `secrets.rs`.
- **Contracts 8 and 9 (additive):** used as written. The app keys the pack check with `check_key(version, "pack")` and keeps whether Metal was usable as a second record, subject `"pack:metal"`, because `CheckRecord` has no device field. A model's check is keyed `"<id>@<revision>"` (Hub) or `"<id>@<size>+<mtime of config.json>"` (from this computer). Removing the pack clears this lock's records.
- **Golden schemas (`OPENBARK_MLX_TEST_SCHEMAS`):** `cargo run -p os-engine --example write_schemas -- <dir>` writes every built-in template's note schema, its edit schema and the from-example schema (41 files; `from_example::draft_schema` is now public for it). `scripts/ci/mlx-e2e.sh` compiles them, with `packages/templates/*/schema.json`, in llguidance over the test model's tokenizer when `OPENBARK_GOLDEN_SCHEMAS` is set, and the Linux CI job sets it. Checked in WSL: 42 of 42 compile.
- **Progress event:** accepted; it keeps real-length CPU drafts alive under the 120 s silence rule (a 1,010-token prompt took 224 s to read in WSL).
- **Worker script check:** bytes are compared, not SHA-256 (ADR-0025 decision 2 now says so).
- **Beyond the contracts:**
  - Busy goes beyond the slot: every model handed out is also kept as a `Weak`, so a draft that started before another model took the slot still keeps its model (and, for MLX, the engine) from being removed.
  - A second download of a model that is already downloading is refused with "This model is still downloading." (two downloads would share `.part` files).
  - `remove_mlx`, `check_mlx` and a second `install_mlx` are refused while the pack is installing: "The MLX engine is already downloading." (translated in the four catalogs).
  - `inspect_import` and `import_model` refuse an MLX folder where MLX can't run, with "MLX models run on a Mac with Apple silicon and macOS 14 or later."
  - `hub_repo` and `hub_add` fetch without a token first and read the saved token only for a gated repo whose files are hidden; `download_model` reads it only for an entry marked `gated`. Nothing else reads the keychain.
  - `remove_gpu` deletes the GPU recognisers' listed files (`remove::remove_files`) instead of their whole folders.
  - The command line's `models rm` applies only the speech-model guards (it has no per-device note model); `openbark mlx status` prints the support, lock, folders and check as JSON.
  - The Linux CI job installs the pack through OpenBark's own verified downloader on every run (it first cached the unpacked pack, which skipped the SHA-256 checks; the review fix removed that cache). The macOS job installs the real pack every time and is allowed to fail (runner Metal is outside our control).
  - UI fixes found by the end-to-end run: a sheet's tables no longer shrink when an MLX repo also lists GGUF alternatives (`.sheet-body > * { flex-shrink: 0 }`), and copy progress from **Add from this computer** no longer leaves a stuck progress bar on the catalog row.
- **Not verified here (no Mac):** Metal, `sandbox-exec` around the real worker, and the macOS pack's code signing on a fresh machine. The `mlx-macos` CI job is there to prove them.

## Review fixes (2026-10-19)

Nineteen verified review findings, fixed before release 0.1.1:

- **An update no longer breaks MLX drafting.** The pack check is keyed by app version and macOS version, so after an update no result matched and drafting said "Download the MLX engine" until the Models screen re-ran the check. `open_mlx` now runs the check itself when the result is missing (local, a few seconds), and a failed check has its own message pointing to **Try again**.
- **Copying a folder of links copies the real files.** The Hugging Face cache's snapshots are links into `blobs`; hard-linking them copied the links (dangling, or leading out of the store, and then impossible to remove). Links are resolved first; a link must lead inside the chosen folder or, for a cache snapshot, the repo's `blobs`; any other is refused (`ImportError::Link`).
- **Freed space is honest.** A file whose data has another name (a hard-linked copy) frees nothing: `ModelRow.shared_bytes` and `Removal.shared_bytes` count it, `disk_bytes` and `freed_bytes` don't, and the dialog and toast say so. Windows reads link counts with `GetFileInformationByHandle` (`windows-sys`, already in `Cargo.lock`).
- **MLX only where MLX runs.** `hub_add` with `@mlx`, `download_model` of an MLX entry and the command line's `add-hf ... @mlx` and `pull` refuse where MLX can't run; the **In use** list disables MLX models until the engine is installed; the MLX folder button needs a Mac that runs MLX.
- **The token is the clinic's own.** Only `OPENBARK_HF_TOKEN` is read (never `HF_TOKEN`), and `OPENBARK_HF_TOKEN_FILE` keeps scratch runs away from the keychain.
- **Looking at a path needs a veterinarian** with a team: `inspect_import` takes `by` (contract 11: `inspect_import {path, by}`, `POST /api/models/import/inspect` `{path, by}`).
- **The worker enforces `max_context`.** `load` carries it (`{"op":"load","path":...,"max_context":32768}`), and the worker refuses past `min(model's context, max_context)` with `context_overflow`, as llama.cpp does.
- **Link chains in the pack** are followed as the OS would, and refused if they lead out (`mlx_pack.rs` and the lock script); a failed unpack leaves no half-made folder.
- **Test mode is built in only where asked:** `OPENBARK_MLX_DEV` works in debug builds and with the `mlx-test-mode` feature, never in clinic releases.
- **The status call never waits for a model load:** `GET /api/mlx` runs off the async runtime and reads a hint when the slot's lock is held; `remove_model` takes the slot before the download list.
- **Helper-file names are matched by whole words**, so `...-lorablated-...`, `Explora-7B` and a `Floral-v1` folder are models again.
- **A newer revision waits for the old files to go**: adding a model again at another revision while files (or a stopped download) from the old one are on disk is refused with what to do; a `.part` longer than its file is fetched again.
- **A gated repo with nothing to run says so** (no token prompt) and lists GGUF builds of it; a slow list of GGUF builds for a repo no longer open is dropped.
- **The socket guard's docs say what it covers**, and it now also covers `sendto`, `sendmsg`, `bind` and `listen`; the offline environment, the sandbox on macOS and the empty environment remain the real protections.
- **Licence wording matches the pins**, CI re-verifies the pack on every run, and CI's MLX job runs the real-worker tests with `OPENBARK_MLX_TEST_REQUIRED=1`.
