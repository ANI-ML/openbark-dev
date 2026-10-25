//! Adding a model from this computer (ADR-0025 §9): a `.gguf` file anywhere,
//! or an MLX model folder. Copied into the store (`<root>/local/<name>`,
//! hard-linked when it can be), or used where it is. No network.

use std::collections::HashSet;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::catalog::{CatalogEntry, FileSpec, Layout, Slot, Source};
use crate::download::Progress;
use crate::hub::{self, is_helper_gguf, is_tokenizer, missing_weights, mlx_allowed, parse_shard};
use crate::locations;
use crate::store::{ModelStore, hash_file};

/// Free space kept for the OS and the encrypted store, as for downloads.
const MARGIN: u64 = 1 << 30;
/// A safetensors header bigger than this isn't a real one.
const MAX_SAFETENSORS_HEADER: u64 = 100 << 20;
const CONFIG_CAP: u64 = 1 << 20;
const INDEX_CAP: u64 = 8 << 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImportMode {
    /// Into the model store (recommended).
    Copy,
    /// Used where it is; OpenBark never writes to, moves or deletes it.
    InPlace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ImportKind {
    Gguf,
    Mlx,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImportFile {
    pub path: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImportPreview {
    pub kind: ImportKind,
    /// Shown name, e.g. "Qwen3-4B-Q4_K_M".
    pub name: String,
    /// "local/<safe name>" (a "-2", "-3" suffix if taken).
    pub id: String,
    /// What would be copied or used.
    pub files: Vec<ImportFile>,
    pub total_bytes: u64,
    /// MLX only.
    pub model_type: Option<String>,
    /// A copy can be a hard link (no extra space).
    pub same_drive: bool,
    /// Free space where the store is.
    pub free_bytes: Option<u64>,
}

#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("OpenBark can't find {0}.")]
    NotFound(String),
    #[error("Give the full path, starting from the top of the drive.")]
    NotAbsolute,
    #[error("That isn't a GGUF model file.")]
    NotGguf,
    #[error("That's a helper file ({0}), not a model. Choose the main model file.")]
    Helper(String),
    #[error("Some parts of this model are missing from the folder: {}.", .0.join(", "))]
    MissingParts(Vec<String>),
    #[error("That folder isn't an MLX model OpenBark can run: {0}")]
    NotMlx(String),
    #[error("Not enough space: this needs {need} bytes and {available} are free.")]
    DiskSpace { need: u64, available: u64 },
    /// A file in the folder is a link (or shortcut) to a file elsewhere.
    #[error("{0} is a link to a file outside the folder you chose, so OpenBark won't add it.")]
    Link(String),
    #[error(transparent)]
    Io(#[from] io::Error),
}

/// What was found at the path.
struct Found {
    kind: ImportKind,
    name: String,
    /// The folder the files are in (by these names; some may be links).
    dir: PathBuf,
    /// Relative to `dir`.
    files: Vec<ImportFile>,
    /// Where each file's bytes really are, links resolved, in `files` order.
    /// Copies and hashes read these, so a copy never holds a link.
    real: Vec<PathBuf>,
    /// The layout's file: part 1 of a GGUF, or "config.json".
    model: String,
    /// What an in-place entry remembers: the GGUF (part 1) or the MLX folder.
    source: PathBuf,
    model_type: Option<String>,
}

/// Look without changing anything: a .gguf file (GGUF magic, version 2 or 3; a
/// shard brings its siblings), or a folder that passes the MLX runnable checks.
/// The path must be absolute; it is canonicalised.
pub fn inspect(store: &ModelStore, path: &Path) -> Result<ImportPreview, ImportError> {
    let found = examine(path)?;
    let bytes_at = found.real.first().and_then(|p| p.parent()).unwrap_or(&found.dir);
    let same_drive = match (locations::disk_of(bytes_at), locations::disk_of(store.root())) {
        (Some((a, _)), Some((b, _))) => a == b,
        _ => false,
    };
    Ok(ImportPreview {
        kind: found.kind,
        id: free_id(store, &found.name),
        name: found.name,
        total_bytes: found.files.iter().map(|f| f.size).sum(),
        files: found.files,
        model_type: found.model_type,
        same_drive,
        free_bytes: locations::free_space(store.root()),
    })
}

/// Copy: into `<root>/local/<name>/` via `<root>/local/.<name>.tmp` then
/// rename; a hard link first on the same drive; SHA-256 while copying; 1 GiB
/// free-space margin. InPlace: nothing is written except the catalog entry,
/// with `Source::Local { path: Some(abs) }` and each file's size and SHA-256.
/// Progress: (file, done, total) as in download.
pub fn import(
    store: &ModelStore,
    path: &Path,
    mode: ImportMode,
    progress: &mut dyn Progress,
) -> Result<CatalogEntry, ImportError> {
    import_with(store, path, mode, progress, &Options::default())
}

/// Knobs for tests: whether hard links are tried, and the free space seen.
struct Options {
    hard_links: bool,
    free_space: fn(&Path) -> Option<u64>,
}

impl Default for Options {
    fn default() -> Self {
        Self { hard_links: true, free_space: locations::free_space }
    }
}

fn import_with(
    store: &ModelStore,
    path: &Path,
    mode: ImportMode,
    progress: &mut dyn Progress,
    opts: &Options,
) -> Result<CatalogEntry, ImportError> {
    let found = examine(path)?;
    let id = free_id(store, &found.name);
    let local_name = id.strip_prefix("local/").unwrap_or(&id).to_string();
    let layout = match found.kind {
        ImportKind::Gguf => Layout::LlmGguf { model: found.model.clone() },
        ImportKind::Mlx => Layout::LlmMlx { config: found.model.clone() },
    };
    let mut entry = CatalogEntry {
        id,
        display_name: found.name.clone(),
        slot: Slot::Llm,
        license: "unknown".into(),
        attribution: None,
        source: Source::Local { path: None },
        files: Vec::new(),
        layout,
        gated: false,
        mobile: false,
    };
    let mut copied_to = None;
    match mode {
        ImportMode::InPlace => {
            for (f, p) in found.files.iter().zip(&found.real) {
                progress.update(&f.path, 0, f.size);
                let h = hash_file(p)?;
                let size = fs::metadata(p)?.len();
                progress.update(&f.path, size, size);
                entry.files.push(spec(&f.path, size, h.sha256));
            }
            entry.source = Source::Local { path: Some(found.source.to_string_lossy().into_owned()) };
        }
        ImportMode::Copy => {
            let local_root = store.root().join("local");
            fs::create_dir_all(&local_root)?;
            let dest = local_root.join(&local_name);
            let tmp = local_root.join(format!(".{local_name}.tmp"));
            clear_tmp(&tmp)?;
            fs::create_dir(&tmp)?;
            match copy_all(&found, &tmp, &local_root, progress, opts) {
                Ok(files) => entry.files = files,
                Err(e) => {
                    let _ = fs::remove_dir_all(&tmp);
                    return Err(e);
                }
            }
            if let Err(e) = fs::rename(&tmp, &dest) {
                let _ = fs::remove_dir_all(&tmp);
                return Err(e.into());
            }
            copied_to = Some(dest);
        }
    }
    if let Err(e) = hub::save_user_entry(store, entry.clone()) {
        if let Some(dest) = copied_to {
            let _ = fs::remove_dir_all(dest);
        }
        return Err(ImportError::Io(io::Error::other(e.to_string())));
    }
    tracing::info!(id = %entry.id, files = entry.files.len(), ?mode, "model added from this computer");
    Ok(entry)
}

fn spec(path: &str, size: u64, sha256: String) -> FileSpec {
    FileSpec { path: path.to_string(), size, sha256, git_sha1: None, url: None }
}

/// A leftover temporary folder from an import that stopped: ours, so removed,
/// but only if it's a real folder (never a link).
fn clear_tmp(tmp: &Path) -> io::Result<()> {
    match fs::symlink_metadata(tmp) {
        Ok(m) if m.file_type().is_dir() => fs::remove_dir_all(tmp),
        Ok(_) => Err(io::Error::other(format!("{} is in the way", tmp.display()))),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

fn copy_all(
    found: &Found,
    tmp: &Path,
    local_root: &Path,
    progress: &mut dyn Progress,
    opts: &Options,
) -> Result<Vec<FileSpec>, ImportError> {
    let mut specs = Vec::with_capacity(found.files.len());
    let mut space_checked = false;
    for (i, (f, src)) in found.files.iter().zip(&found.real).enumerate() {
        // `src` is the real file (links resolved in `examine`): a hard link
        // to a symlink would only copy the link, dangling or leaving the store.
        let dst = tmp.join(&f.path);
        if opts.hard_links && fs::hard_link(src, &dst).is_ok() {
            progress.update(&f.path, 0, f.size);
            let h = hash_file(&dst)?;
            let size = fs::metadata(&dst)?.len();
            progress.update(&f.path, size, size);
            specs.push(spec(&f.path, size, h.sha256));
            continue;
        }
        if !space_checked {
            let need: u64 = found.files[i..].iter().map(|f| f.size).sum();
            if let Some(available) = (opts.free_space)(local_root)
                && need + MARGIN > available
            {
                return Err(ImportError::DiskSpace { need, available });
            }
            space_checked = true;
        }
        let (size, sha256) = copy_hashing(src, &dst, &f.path, progress)?;
        specs.push(spec(&f.path, size, sha256));
    }
    Ok(specs)
}

/// Copy a file, computing its SHA-256 on the way.
fn copy_hashing(
    src: &Path,
    dst: &Path,
    name: &str,
    progress: &mut dyn Progress,
) -> io::Result<(u64, String)> {
    let mut input = fs::File::open(src)?;
    let total = input.metadata()?.len();
    let mut output = fs::File::create(dst)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    let (mut done, mut reported) = (0u64, 0u64);
    progress.update(name, 0, total);
    loop {
        let n = input.read(&mut buf)?;
        if n == 0 {
            break;
        }
        output.write_all(&buf[..n])?;
        hasher.update(&buf[..n]);
        done += n as u64;
        if done - reported >= 16 << 20 {
            reported = done;
            progress.update(name, done, total);
        }
    }
    output.flush()?;
    output.sync_all()?;
    if done != total {
        return Err(io::Error::other(format!("{} changed while it was being copied", src.display())));
    }
    progress.update(name, done, total);
    Ok((done, hex::encode(hasher.finalize())))
}

fn examine(path: &Path) -> Result<Found, ImportError> {
    if !path.is_absolute() {
        return Err(ImportError::NotAbsolute);
    }
    let not_found = || ImportError::NotFound(path.display().to_string());
    let meta = fs::metadata(path).map_err(|_| not_found())?;
    if meta.is_dir() {
        let canonical = fs::canonicalize(path).map_err(|_| not_found())?;
        return examine_mlx(&locations::plain(&canonical));
    }
    // A file: its folder, canonicalised, and its own name (a link keeps the
    // name it was chosen by; its bytes are read from where it points).
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).ok_or(ImportError::NotGguf)?;
    let parent = path.parent().ok_or(ImportError::NotGguf)?;
    let dir = locations::plain(&fs::canonicalize(parent).map_err(|_| not_found())?);
    let real = locations::plain(&fs::canonicalize(path).map_err(|_| not_found())?);
    examine_gguf(&dir, &name, real)
}

/// Where the bytes of `dir/name` really are. A plain file is itself. A link
/// (or, on Windows, any reparse point) is followed and must lead to a
/// regular file inside `dir` or, for a folder in a Hugging Face cache
/// (`models--<org>--<name>/snapshots/<revision>`), inside that repo's
/// `blobs` folder, where the cache keeps the bytes its snapshot links point
/// to. A link leading anywhere else is refused, so nothing from outside the
/// chosen folder is copied or used. `Ok(None)` when it isn't a file (or is a
/// link to nothing).
fn real_file(dir: &Path, name: &str) -> Result<Option<PathBuf>, ImportError> {
    let path = dir.join(name);
    let Ok(meta) = fs::symlink_metadata(&path) else { return Ok(None) };
    if !crate::remove::is_link(&meta) {
        return Ok(meta.is_file().then_some(path));
    }
    let Ok(target) = fs::canonicalize(&path) else { return Ok(None) };
    let target = locations::plain(&target);
    if !target.is_file() {
        return Ok(None);
    }
    if allowed_roots(dir).iter().any(|root| target.starts_with(root)) {
        Ok(Some(target))
    } else {
        Err(ImportError::Link(name.to_string()))
    }
}

/// The folders a link in `dir` may lead into: `dir` itself, and for a Hugging
/// Face cache snapshot (`.../models--<org>--<name>/snapshots/<revision>`), the
/// repo's `blobs` folder. `dir` is canonical.
fn allowed_roots(dir: &Path) -> Vec<PathBuf> {
    let mut roots = vec![dir.to_path_buf()];
    let name_of = |p: Option<&Path>| p.and_then(Path::file_name).map(|n| n.to_string_lossy().into_owned());
    let snapshots = dir.parent();
    let repo = snapshots.and_then(Path::parent);
    if name_of(snapshots).as_deref() == Some("snapshots")
        && name_of(repo).is_some_and(|n| n.starts_with("models--"))
        && let Some(repo) = repo
        && let Ok(blobs) = fs::canonicalize(repo.join("blobs"))
    {
        roots.push(locations::plain(&blobs));
    }
    roots
}

/// "GGUF" then a little-endian version 2 or 3.
fn gguf_ok(path: &Path) -> bool {
    let mut head = [0u8; 8];
    fs::File::open(path).and_then(|mut f| f.read_exact(&mut head)).is_ok()
        && &head[..4] == b"GGUF"
        && matches!(u32::from_le_bytes([head[4], head[5], head[6], head[7]]), 2 | 3)
}

/// A `.gguf` file chosen as `dir/name` (the bytes at `file`, links resolved):
/// one model, or part of a split one whose other parts come from `dir`.
fn examine_gguf(dir: &Path, name: &str, file: PathBuf) -> Result<Found, ImportError> {
    let name = name.to_string();
    let dir = dir.to_path_buf();
    if is_helper_gguf(&name) {
        return Err(ImportError::Helper(name));
    }
    if !file.is_file() || !gguf_ok(&file) {
        return Err(ImportError::NotGguf);
    }
    let Some(shard) = parse_shard(&name) else {
        let size = fs::metadata(&file)?.len();
        let shown = strip_ext(&name, ".gguf").to_string();
        return Ok(Found {
            kind: ImportKind::Gguf,
            name: shown,
            source: dir.join(&name),
            dir,
            files: vec![ImportFile { path: name.clone(), size }],
            real: vec![file],
            model: name,
            model_type: None,
        });
    };
    // Every part from the same folder: same prefix, same count. A part that
    // is a link must stay inside the folder (the one chosen is the user's pick).
    let mut parts: Vec<Option<(String, u64, PathBuf)>> = vec![None; shard.count as usize];
    for e in fs::read_dir(&dir)?.flatten() {
        let n = e.file_name().to_string_lossy().into_owned();
        let Some(s) = parse_shard(&n).filter(|s| s.prefix == shard.prefix && s.count == shard.count) else {
            continue;
        };
        let real = if n == name { Some(file.clone()) } else { real_file(&dir, &n)? };
        if let Some(real) = real {
            let size = fs::metadata(&real)?.len();
            parts[s.index as usize - 1] = Some((n, size, real));
        }
    }
    let width = name.rsplit_once("-of-").map(|(_, c)| c.len() - ".gguf".len()).unwrap_or(5);
    let missing: Vec<String> = parts
        .iter()
        .enumerate()
        .filter(|(_, p)| p.is_none())
        .map(|(i, _)| format!("{}-{:0w$}-of-{:0w$}.gguf", shard.prefix, i + 1, shard.count, w = width))
        .collect();
    if !missing.is_empty() {
        return Err(ImportError::MissingParts(missing));
    }
    let (files, real): (Vec<ImportFile>, Vec<PathBuf>) =
        parts.into_iter().flatten().map(|(path, size, real)| (ImportFile { path, size }, real)).unzip();
    if real.iter().any(|p| !gguf_ok(p)) {
        return Err(ImportError::NotGguf);
    }
    let model = files[0].path.clone();
    Ok(Found {
        kind: ImportKind::Gguf,
        name: shard.prefix,
        source: dir.join(&model),
        dir,
        files,
        real,
        model,
        model_type: None,
    })
}

fn not_mlx(why: impl Into<String>) -> ImportError {
    ImportError::NotMlx(why.into())
}

fn read_capped(path: &Path, cap: u64) -> io::Result<Vec<u8>> {
    let mut out = Vec::new();
    fs::File::open(path)?.take(cap + 1).read_to_end(&mut out)?;
    if out.len() as u64 > cap {
        return Err(io::Error::other(format!("{} is too big", path.display())));
    }
    Ok(out)
}

/// 8-byte little-endian header length, then a JSON header starting with '{'.
fn safetensors_ok(path: &Path) -> bool {
    let Ok(mut f) = fs::File::open(path) else { return false };
    let Ok(len) = f.metadata().map(|m| m.len()) else { return false };
    let mut head = [0u8; 9];
    if f.read_exact(&mut head).is_err() {
        return false;
    }
    let n = u64::from_le_bytes(head[..8].try_into().expect("8 bytes"));
    head[8] == b'{' && (2..=MAX_SAFETENSORS_HEADER).contains(&n) && 8 + n <= len
}

fn examine_mlx(dir: &Path) -> Result<Found, ImportError> {
    // Allow-listed root files, each with where its bytes really are (a folder
    // from the Hugging Face cache holds links into the repo's `blobs`).
    let mut found: Vec<(ImportFile, PathBuf)> = Vec::new();
    for e in fs::read_dir(dir)?.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if !mlx_allowed(&name) {
            continue;
        }
        if let Some(real) = real_file(dir, &name)? {
            found.push((ImportFile { size: fs::metadata(&real)?.len(), path: name }, real));
        }
    }
    found.sort_by(|a, b| a.0.path.cmp(&b.0.path));
    let (files, real): (Vec<ImportFile>, Vec<PathBuf>) = found.into_iter().unzip();
    let real_of = |name: &str| files.iter().position(|f| f.path == name).map(|i| real[i].clone());
    let names: HashSet<&str> = files.iter().map(|f| f.path.as_str()).collect();
    if !names.contains("config.json") {
        return Err(not_mlx("it has no config.json."));
    }
    let model_type =
        read_capped(&real_of("config.json").unwrap_or_else(|| dir.join("config.json")), CONFIG_CAP)
            .ok()
            .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
            .and_then(|c| c.get("model_type").and_then(|t| t.as_str()).map(String::from))
            .filter(|t| !t.is_empty())
            .ok_or_else(|| not_mlx("its config.json has no model_type."))?;
    let weights: Vec<&str> = names.iter().copied().filter(|n| n.ends_with(".safetensors")).collect();
    if weights.is_empty() {
        return Err(not_mlx("it has no .safetensors weights."));
    }
    let index = match real_of("model.safetensors.index.json") {
        Some(p) => Some(read_capped(&p, INDEX_CAP)?),
        None => None,
    };
    if let Some(missing) = missing_weights(&names, &weights, index.as_deref()) {
        return Err(not_mlx(format!("some weight files are missing: {}.", missing.join(", "))));
    }
    let mut sorted = weights.clone();
    sorted.sort();
    for w in sorted {
        if !real_of(w).is_some_and(|p| safetensors_ok(&p)) {
            return Err(not_mlx(format!("{w} isn't a valid safetensors file.")));
        }
    }
    if !names.iter().any(|n| is_tokenizer(n)) {
        return Err(not_mlx("it has no tokenizer files."));
    }
    let name = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "model".into());
    Ok(Found {
        kind: ImportKind::Mlx,
        name,
        dir: dir.to_path_buf(),
        files,
        real,
        model: "config.json".into(),
        source: dir.to_path_buf(),
        model_type: Some(model_type),
    })
}

fn strip_ext<'a>(name: &'a str, ext: &str) -> &'a str {
    if name.len() > ext.len() && name[name.len() - ext.len()..].eq_ignore_ascii_case(ext) {
        &name[..name.len() - ext.len()]
    } else {
        name
    }
}

/// A name that is a safe folder name everywhere.
fn safe_name(name: &str) -> String {
    let mapped: String =
        name.chars().map(|c| if c.is_ascii_alphanumeric() || "-_.".contains(c) { c } else { '_' }).collect();
    let trimmed = mapped.trim_start_matches(['.', '-']).trim_end_matches(['.', ' ']);
    let mut out: String = trimmed.chars().take(64).collect();
    if out.is_empty() {
        out = "model".into();
    }
    // Windows won't make a folder called CON, NUL, COM1... (with any extension).
    let stem = out.split('.').next().unwrap_or_default().to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.as_bytes()[3].is_ascii_digit());
    if reserved {
        out.insert(0, '_');
    }
    out
}

/// "local/<safe name>", with "-2", "-3"... when taken in the catalog or on disk.
fn free_id(store: &ModelStore, name: &str) -> String {
    let base = safe_name(name);
    let catalog = hub::full_catalog(store);
    let local = store.root().join("local");
    (1..)
        .map(|n| if n == 1 { base.clone() } else { format!("{base}-{n}") })
        .find(|cand| catalog.get(&format!("local/{cand}")).is_none() && !local.join(cand).exists())
        .map(|cand| format!("local/{cand}"))
        .expect("some name is free")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Verify;

    fn gguf(version: u32, body: &[u8]) -> Vec<u8> {
        let mut v = b"GGUF".to_vec();
        v.extend_from_slice(&version.to_le_bytes());
        v.extend_from_slice(body);
        v
    }

    fn safetensors(header: &str, data: usize) -> Vec<u8> {
        let mut v = (header.len() as u64).to_le_bytes().to_vec();
        v.extend_from_slice(header.as_bytes());
        v.extend(std::iter::repeat_n(7u8, data));
        v
    }

    fn write(path: &Path, bytes: &[u8]) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    fn mlx_folder(dir: &Path) {
        write(&dir.join("config.json"), br#"{"model_type":"qwen3","quantization":{"bits":4}}"#);
        write(
            &dir.join("model.safetensors"),
            &safetensors(r#"{"w":{"dtype":"F16","shape":[2],"data_offsets":[0,4]}}"#, 4),
        );
        write(&dir.join("tokenizer.json"), b"{}");
        write(&dir.join("tokenizer_config.json"), b"{}");
        write(&dir.join("modeling_qwen.py"), b"import os");
        write(&dir.join("README.md"), b"hi");
    }

    fn quiet() -> impl FnMut(&str, u64, u64) {
        |_: &str, _: u64, _: u64| {}
    }

    struct Env {
        _tmp: tempfile::TempDir,
        outside: PathBuf,
        store: ModelStore,
    }

    fn env() -> Env {
        let tmp = tempfile::tempdir().unwrap();
        let outside = fs::canonicalize(tmp.path()).unwrap().join("Downloads");
        fs::create_dir_all(&outside).unwrap();
        let outside = locations::plain(&outside);
        let store = ModelStore::new(tmp.path().join("models"));
        Env { _tmp: tmp, outside, store }
    }

    #[test]
    fn a_gguf_file_is_inspected_without_changing_anything() {
        let e = env();
        let f = e.outside.join("Qwen3-4B-Q4_K_M.gguf");
        write(&f, &gguf(3, &[1u8; 100]));
        let p = inspect(&e.store, &f).unwrap();
        assert_eq!(
            (p.kind, p.name.as_str(), p.id.as_str()),
            (ImportKind::Gguf, "Qwen3-4B-Q4_K_M", "local/Qwen3-4B-Q4_K_M")
        );
        assert_eq!(p.total_bytes, 108);
        assert_eq!(p.files.len(), 1);
        assert!(p.same_drive, "the temp folder and the store are on one drive");
        assert!(!e.store.root().exists(), "inspecting writes nothing");
    }

    #[test]
    fn only_real_gguf_files_pass() {
        let e = env();
        let v1 = e.outside.join("old.gguf");
        write(&v1, &gguf(1, b"x"));
        assert!(matches!(inspect(&e.store, &v1), Err(ImportError::NotGguf)));
        let text = e.outside.join("notes.gguf");
        write(&text, b"hello there");
        assert!(matches!(inspect(&e.store, &text), Err(ImportError::NotGguf)));
        let short = e.outside.join("short.gguf");
        write(&short, b"GGUF");
        assert!(matches!(inspect(&e.store, &short), Err(ImportError::NotGguf)));
        assert!(matches!(inspect(&e.store, Path::new("relative/m.gguf")), Err(ImportError::NotAbsolute)));
        assert!(matches!(inspect(&e.store, &e.outside.join("nope.gguf")), Err(ImportError::NotFound(_))));
        let v2 = e.outside.join("v2.gguf");
        write(&v2, &gguf(2, b"x"));
        assert!(inspect(&e.store, &v2).is_ok());
    }

    #[test]
    fn helper_files_are_refused() {
        let e = env();
        for name in ["mmproj-F16.gguf", "mtp-gemma-4-E4B-it.gguf"] {
            let f = e.outside.join(name);
            write(&f, &gguf(3, b"x"));
            let err = inspect(&e.store, &f).unwrap_err();
            assert!(matches!(&err, ImportError::Helper(n) if n == name), "{err}");
            assert_eq!(
                err.to_string(),
                format!("That's a helper file ({name}), not a model. Choose the main model file.")
            );
        }
    }

    #[test]
    fn a_shard_brings_its_siblings_or_names_the_missing_ones() {
        let e = env();
        let part = |i: u32| e.outside.join(format!("Big-Q4_K_M-{i:05}-of-00003.gguf"));
        write(&part(1), &gguf(3, &[1u8; 10]));
        write(&part(3), &gguf(3, &[3u8; 30]));
        write(&e.outside.join("Other-00002-of-00003.gguf"), &gguf(3, b"x"));
        let err = inspect(&e.store, &part(3)).unwrap_err();
        assert!(
            matches!(&err, ImportError::MissingParts(m) if m == &["Big-Q4_K_M-00002-of-00003.gguf"]),
            "{err}"
        );
        assert_eq!(
            err.to_string(),
            "Some parts of this model are missing from the folder: Big-Q4_K_M-00002-of-00003.gguf."
        );
        write(&part(2), &gguf(3, &[2u8; 20]));
        let p = inspect(&e.store, &part(2)).unwrap();
        assert_eq!(p.name, "Big-Q4_K_M");
        let names: Vec<&str> = p.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(
            names,
            [
                "Big-Q4_K_M-00001-of-00003.gguf",
                "Big-Q4_K_M-00002-of-00003.gguf",
                "Big-Q4_K_M-00003-of-00003.gguf"
            ]
        );
        assert_eq!(p.total_bytes, 18 + 28 + 38);
    }

    #[test]
    fn mlx_folders_are_checked() {
        let e = env();
        let dir = e.outside.join("Qwen3-0.6B-4bit");
        mlx_folder(&dir);
        let p = inspect(&e.store, &dir).unwrap();
        assert_eq!(p.kind, ImportKind::Mlx);
        assert_eq!(p.model_type.as_deref(), Some("qwen3"));
        let names: Vec<&str> = p.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(
            names,
            ["config.json", "model.safetensors", "tokenizer.json", "tokenizer_config.json"],
            "never .py or README"
        );

        let msg = |r: Result<ImportPreview, ImportError>| match r {
            Err(ImportError::NotMlx(m)) => m,
            other => panic!("{:?}", other.map(|p| p.name)),
        };
        // A bad safetensors header.
        write(&dir.join("model.safetensors"), b"\x05\0\0\0\0\0\0\0[1,2]");
        assert_eq!(msg(inspect(&e.store, &dir)), "model.safetensors isn't a valid safetensors file.");
        write(&dir.join("model.safetensors"), &(u64::MAX).to_le_bytes());
        assert!(msg(inspect(&e.store, &dir)).contains("isn't a valid safetensors"));
        // An index naming a file that isn't there.
        mlx_folder(&dir);
        write(
            &dir.join("model.safetensors.index.json"),
            br#"{"weight_map":{"a":"model.safetensors","b":"model-2.safetensors"}}"#,
        );
        assert_eq!(msg(inspect(&e.store, &dir)), "some weight files are missing: model-2.safetensors.");
        fs::remove_file(dir.join("model.safetensors.index.json")).unwrap();
        // No tokenizer.
        fs::remove_file(dir.join("tokenizer.json")).unwrap();
        assert_eq!(msg(inspect(&e.store, &dir)), "it has no tokenizer files.");
        mlx_folder(&dir);
        // No model_type.
        write(&dir.join("config.json"), b"{}");
        assert_eq!(msg(inspect(&e.store, &dir)), "its config.json has no model_type.");
        fs::remove_file(dir.join("config.json")).unwrap();
        assert_eq!(msg(inspect(&e.store, &dir)), "it has no config.json.");
        mlx_folder(&dir);
        fs::remove_file(dir.join("model.safetensors")).unwrap();
        assert_eq!(msg(inspect(&e.store, &dir)), "it has no .safetensors weights.");
        // Shards without an index must be complete.
        mlx_folder(&dir);
        fs::rename(dir.join("model.safetensors"), dir.join("model-00001-of-00002.safetensors")).unwrap();
        assert_eq!(
            msg(inspect(&e.store, &dir)),
            "some weight files are missing: model-00002-of-00002.safetensors."
        );
    }

    #[test]
    fn copy_goes_into_the_store_and_is_recorded() {
        let e = env();
        let f = e.outside.join("Qwen3-4B-Q4_K_M.gguf");
        let bytes = gguf(3, &[9u8; 4000]);
        write(&f, &bytes);
        let mut seen = Vec::new();
        let entry = import(&e.store, &f, ImportMode::Copy, &mut |n: &str, d: u64, t: u64| {
            seen.push((n.to_string(), d, t))
        })
        .unwrap();
        assert_eq!(entry.id, "local/Qwen3-4B-Q4_K_M");
        assert!(matches!(entry.source, Source::Local { path: None }));
        assert_eq!(entry.license, "unknown");
        assert!(matches!(&entry.layout, Layout::LlmGguf { model } if model == "Qwen3-4B-Q4_K_M.gguf"));
        let copy = e.store.root().join("local/Qwen3-4B-Q4_K_M/Qwen3-4B-Q4_K_M.gguf");
        assert_eq!(fs::read(&copy).unwrap(), bytes);
        assert_eq!(e.store.model_dir(&entry), e.store.root().join("local").join("Qwen3-4B-Q4_K_M"));
        assert!(e.store.is_installed(&entry, Verify::Full), "the recorded SHA-256 matches");
        assert_eq!(seen.last().unwrap().1, bytes.len() as u64);
        assert!(hub::full_catalog(&e.store).get(&entry.id).is_some());
        assert!(!e.store.root().join("local/.Qwen3-4B-Q4_K_M.tmp").exists());
        assert_eq!(fs::read(&f).unwrap(), bytes, "the original is untouched");

        // The same file again gets a new name.
        let again = import(&e.store, &f, ImportMode::Copy, &mut quiet()).unwrap();
        assert_eq!(again.id, "local/Qwen3-4B-Q4_K_M-2");
        assert_eq!(inspect(&e.store, &f).unwrap().id, "local/Qwen3-4B-Q4_K_M-3");
    }

    /// On the same drive the copy is a hard link: a change made through one
    /// name shows through the other, and no second copy takes space.
    #[test]
    fn copy_on_the_same_drive_is_a_hard_link() {
        let e = env();
        let f = e.outside.join("m.gguf");
        write(&f, &gguf(3, b"abcdef"));
        let entry = import(&e.store, &f, ImportMode::Copy, &mut quiet()).unwrap();
        let copy = e.store.file_path(&entry, "m.gguf");
        let mut w = fs::OpenOptions::new().write(true).open(&f).unwrap();
        w.write_all(b"GGUF\x03\0\0\0ABCDEF").unwrap();
        drop(w);
        assert_eq!(fs::read(&copy).unwrap(), b"GGUF\x03\0\0\0ABCDEF");

        // Without links, it's a real copy.
        let g = e.outside.join("n.gguf");
        write(&g, &gguf(3, b"abcdef"));
        let opts = Options { hard_links: false, ..Options::default() };
        let entry = import_with(&e.store, &g, ImportMode::Copy, &mut quiet(), &opts).unwrap();
        let copy = e.store.file_path(&entry, "n.gguf");
        fs::write(&g, gguf(3, b"zzzzzz")).unwrap();
        assert_eq!(fs::read(&copy).unwrap(), gguf(3, b"abcdef"));
    }

    #[test]
    fn copying_needs_the_free_space_margin() {
        let e = env();
        let f = e.outside.join("m.gguf");
        write(&f, &gguf(3, &[0u8; 1000]));
        let opts = Options { hard_links: false, free_space: |_| Some(1 << 29) };
        let err = import_with(&e.store, &f, ImportMode::Copy, &mut quiet(), &opts).unwrap_err();
        assert!(
            matches!(err, ImportError::DiskSpace { need: 1008, available } if available == 1 << 29),
            "{err}"
        );
        assert!(hub::load_user_catalog(&e.store).is_empty());
        assert!(fs::read_dir(e.store.root().join("local")).unwrap().next().is_none(), "nothing left behind");
    }

    #[test]
    fn in_place_writes_only_the_catalog_entry() {
        let e = env();
        let part = |i: u32| e.outside.join(format!("Big-{i:05}-of-00002.gguf"));
        write(&part(1), &gguf(3, &[1u8; 10]));
        write(&part(2), &gguf(3, &[2u8; 20]));
        let before: Vec<_> = (1..=2).map(|i| fs::metadata(part(i)).unwrap().modified().unwrap()).collect();
        let entry = import(&e.store, &part(2), ImportMode::InPlace, &mut quiet()).unwrap();
        assert_eq!(entry.id, "local/Big");
        assert_eq!(entry.in_place(), Some(part(1).as_path()));
        assert_eq!(e.store.model_dir(&entry), e.outside);
        assert_eq!(entry.files.len(), 2);
        assert!(entry.files.iter().all(|f| f.sha256.len() == 64 && !f.path.contains('/')));
        assert!(e.store.is_installed(&entry, Verify::Full));
        assert!(!e.store.root().join("local").exists(), "nothing copied");
        let names: Vec<String> = fs::read_dir(e.store.root())
            .unwrap()
            .map(|d| d.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["user-catalog.json"]);
        let after: Vec<_> = (1..=2).map(|i| fs::metadata(part(i)).unwrap().modified().unwrap()).collect();
        assert_eq!(before, after);

        // An MLX folder used where it is.
        let dir = e.outside.join("Qwen3-mlx");
        mlx_folder(&dir);
        let entry = import(&e.store, &dir, ImportMode::InPlace, &mut quiet()).unwrap();
        assert_eq!(entry.in_place(), Some(dir.as_path()));
        assert!(matches!(&entry.layout, Layout::LlmMlx { config } if config == "config.json"));
        assert_eq!(e.store.model_dir(&entry), dir);
        assert_eq!(entry.files.len(), 4);
        assert!(e.store.is_installed(&entry, Verify::Full));
    }

    #[test]
    fn an_mlx_folder_is_copied_with_only_its_allowed_files() {
        let e = env();
        let dir = e.outside.join("Qwen3 0.6B (4bit)");
        mlx_folder(&dir);
        let entry = import(&e.store, &dir, ImportMode::Copy, &mut quiet()).unwrap();
        assert_eq!(entry.id, "local/Qwen3_0.6B__4bit_");
        assert_eq!(entry.display_name, "Qwen3 0.6B (4bit)");
        let copied = e.store.model_dir(&entry);
        assert!(copied.join("config.json").is_file());
        assert!(!copied.join("modeling_qwen.py").exists() && !copied.join("README.md").exists());
        assert!(e.store.is_installed(&entry, Verify::Full));
    }

    /// A symbolic link to a file; false when this account can't make one
    /// (Windows without Developer Mode), and the test says it skipped.
    fn symlink_file(target: &Path, link: &Path) -> bool {
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(target, link).is_ok()
        }
        #[cfg(windows)]
        {
            std::os::windows::fs::symlink_file(target, link).is_ok()
        }
    }

    const MLX_FILES: [&str; 4] =
        ["config.json", "model.safetensors", "tokenizer.json", "tokenizer_config.json"];

    /// The Hugging Face cache's layout: `snapshots/<revision>/*` are links
    /// into the repo's `blobs` folder, relative (as huggingface_hub makes
    /// them) or absolute. Returns the snapshot folder and the blob sizes'
    /// total, or None when links can't be made here.
    fn hf_cache(root: &Path, relative: bool) -> Option<(PathBuf, u64)> {
        let repo = root.join("hub").join("models--mlx-community--Qwen3-0.6B-4bit");
        let blobs = repo.join("blobs");
        let snap = repo.join("snapshots").join("73e3e38d981303bc594367cd910ea6eb48349da8");
        fs::create_dir_all(&blobs).unwrap();
        fs::create_dir_all(&snap).unwrap();
        let staging = root.join("staging");
        mlx_folder(&staging);
        let mut total = 0;
        for (i, name) in MLX_FILES.iter().enumerate() {
            let blob = blobs.join(format!("{i}{}", "ab".repeat(20)));
            fs::rename(staging.join(name), &blob).unwrap();
            total += fs::metadata(&blob).unwrap().len();
            let target = if relative {
                Path::new("..").join("..").join("blobs").join(blob.file_name().unwrap())
            } else {
                blob
            };
            if !symlink_file(&target, &snap.join(name)) {
                return None;
            }
        }
        Some((snap, total))
    }

    /// The major finding: a copy hard-linked the links themselves, leaving
    /// dangling links (relative) or links out of the store (absolute) that
    /// couldn't be removed. Now the real files are copied or hard-linked.
    #[test]
    fn a_folder_from_the_hugging_face_cache_is_copied_as_real_files() {
        for (relative, hard_links) in [(true, true), (false, true), (true, false)] {
            let e = env();
            let Some((snap, total)) = hf_cache(&e.outside, relative) else {
                println!("skipped: this account can't create symbolic links");
                return;
            };
            let p = inspect(&e.store, &snap).unwrap();
            assert_eq!(p.kind, ImportKind::Mlx);
            assert_eq!(p.total_bytes, total, "the real files' sizes, not the links'");
            let opts = Options { hard_links, ..Options::default() };
            let entry = import_with(&e.store, &snap, ImportMode::Copy, &mut quiet(), &opts).unwrap();
            let dir = e.store.model_dir(&entry);
            for name in MLX_FILES {
                let meta = fs::symlink_metadata(dir.join(name)).unwrap();
                assert!(
                    meta.is_file() && !crate::remove::is_link(&meta),
                    "{name} is a real file in the store"
                );
            }
            assert!(e.store.is_installed(&entry, Verify::Full));
            // The copy can be removed like any other, and the cache is untouched.
            let r = crate::remove::remove(&e.store, &entry, false, false).unwrap();
            assert_eq!(r.files_removed, 4);
            assert_eq!(r.freed_bytes + r.shared_bytes, total);
            if hard_links {
                assert_eq!(r.freed_bytes, 0, "hard links free nothing: the cache keeps the bytes");
            }
            for name in MLX_FILES {
                assert!(snap.join(name).is_file(), "{name} is still in the cache");
            }
        }
    }

    #[test]
    fn a_folder_from_the_hugging_face_cache_can_be_used_where_it_is() {
        let e = env();
        let Some((snap, total)) = hf_cache(&e.outside, true) else {
            println!("skipped: this account can't create symbolic links");
            return;
        };
        let entry = import(&e.store, &snap, ImportMode::InPlace, &mut quiet()).unwrap();
        assert_eq!(entry.total_size(), total);
        assert_eq!(e.store.model_dir(&entry), snap);
        assert!(e.store.is_installed(&entry, Verify::Full));
        assert!(!e.store.root().join("local").exists(), "nothing copied");
    }

    /// A link leading out of the chosen folder (other than a Hugging Face
    /// snapshot's links into its own `blobs`) is refused: nothing from
    /// elsewhere is copied or used, and nothing is left behind.
    #[test]
    fn a_link_leading_out_of_the_chosen_folder_is_refused() {
        let e = env();
        let dir = e.outside.join("Qwen3-mlx");
        mlx_folder(&dir);
        let secret = e.outside.join("private.json");
        write(&secret, b"{\"not\":\"a tokenizer\"}");
        fs::remove_file(dir.join("tokenizer.json")).unwrap();
        if !symlink_file(&secret, &dir.join("tokenizer.json")) {
            println!("skipped: this account can't create symbolic links");
            return;
        }
        let err = inspect(&e.store, &dir).unwrap_err();
        assert!(matches!(&err, ImportError::Link(n) if n == "tokenizer.json"), "{err}");
        assert_eq!(
            err.to_string(),
            "tokenizer.json is a link to a file outside the folder you chose, so OpenBark won't add it."
        );
        for mode in [ImportMode::Copy, ImportMode::InPlace] {
            assert!(matches!(import(&e.store, &dir, mode, &mut quiet()), Err(ImportError::Link(_))));
        }
        assert!(hub::load_user_catalog(&e.store).is_empty());
        let local = e.store.root().join("local");
        assert!(!local.exists() || fs::read_dir(&local).unwrap().next().is_none(), "nothing left behind");

        // A link that stays inside the folder is fine.
        fs::remove_file(dir.join("tokenizer.json")).unwrap();
        write(&dir.join("real-tokenizer.json"), b"{}");
        assert!(symlink_file(Path::new("real-tokenizer.json"), &dir.join("tokenizer.json")));
        assert!(inspect(&e.store, &dir).is_ok());

        // A "snapshots" folder that isn't in a Hugging Face cache gets no exception.
        let fake = e.outside.join("not-a-cache").join("snapshots").join("rev");
        mlx_folder(&fake);
        let blobs = e.outside.join("not-a-cache").join("blobs");
        write(&blobs.join("tok"), b"{}");
        fs::remove_file(fake.join("tokenizer.json")).unwrap();
        assert!(symlink_file(&blobs.join("tok"), &fake.join("tokenizer.json")));
        assert!(matches!(inspect(&e.store, &fake), Err(ImportError::Link(_))));

        // The other parts of a split GGUF follow the same rule.
        let shards = e.outside.join("shards");
        write(&shards.join("Big-00001-of-00002.gguf"), &gguf(3, &[1u8; 10]));
        let elsewhere = e.outside.join("elsewhere-00002-of-00002.gguf");
        write(&elsewhere, &gguf(3, &[2u8; 10]));
        assert!(symlink_file(&elsewhere, &shards.join("Big-00002-of-00002.gguf")));
        let err = inspect(&e.store, &shards.join("Big-00001-of-00002.gguf")).unwrap_err();
        assert!(matches!(&err, ImportError::Link(n) if n == "Big-00002-of-00002.gguf"), "{err}");
    }

    /// A GGUF chosen through a link keeps the name it was chosen by, and its
    /// copy holds the real bytes, not a link.
    #[test]
    fn a_gguf_chosen_through_a_link_is_copied_as_a_real_file() {
        let e = env();
        let blob = e.outside.join("blobs").join("sha256-0123");
        let bytes = gguf(3, &[4u8; 50]);
        write(&blob, &bytes);
        let picked = e.outside.join("picked").join("Mine-Q4_K_M.gguf");
        fs::create_dir_all(picked.parent().unwrap()).unwrap();
        if !symlink_file(&blob, &picked) {
            println!("skipped: this account can't create symbolic links");
            return;
        }
        let p = inspect(&e.store, &picked).unwrap();
        assert_eq!((p.name.as_str(), p.total_bytes), ("Mine-Q4_K_M", bytes.len() as u64));
        let entry = import(&e.store, &picked, ImportMode::Copy, &mut quiet()).unwrap();
        let copy = e.store.file_path(&entry, "Mine-Q4_K_M.gguf");
        let meta = fs::symlink_metadata(&copy).unwrap();
        assert!(meta.is_file() && !crate::remove::is_link(&meta));
        assert_eq!(fs::read(&copy).unwrap(), bytes);
    }

    #[test]
    fn names_are_made_safe() {
        assert_eq!(safe_name("Qwen3-4B-Q4_K_M"), "Qwen3-4B-Q4_K_M");
        assert_eq!(safe_name("../../etc"), "_.._etc");
        assert_eq!(safe_name("..."), "model");
        assert_eq!(safe_name("a:b/c\\d"), "a_b_c_d");
        assert_eq!(safe_name(&"x".repeat(200)).len(), 64);
        for reserved in ["CON", "nul", "Aux.Q4", "com1", "LPT9"] {
            assert!(safe_name(reserved).starts_with('_'), "{reserved}");
        }
        assert_eq!(safe_name("console"), "console");
        assert_eq!(safe_name("COM10"), "COM10");
    }
}
