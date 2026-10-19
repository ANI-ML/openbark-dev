//! Where models and visits live (ADR-0013).
//!
//! A clinic can keep the models (large, re-downloadable) and the visits
//! (recordings, transcripts and notes: the clinic's data) in folders of its
//! choosing, chosen in Settings and saved in `<local data>/OpenBark/locations.json`.
//! `OPENBARK_MODELS_DIR` and `OPENBARK_SESSIONS_DIR` still win, so demos and
//! tests stay isolated; `OPENBARK_LOCATIONS_FILE` moves the file itself.
//!
//! [`move_tree`] moves a folder's contents to the new place without ever
//! overwriting a different file, and undoes itself if anything fails.

use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// OpenBark's own housekeeping folder inside the visits folder (ADR-0022):
/// Deleted visits (`deleted/`), the deletion record (`audit.log`) and, later,
/// the clinic's retention rule (`policy.json`). It moves with the visits.
pub const HOUSEKEEPING_DIR: &str = ".openbark";

/// The two kinds of folder a clinic can place.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FolderKind {
    Models,
    Visits,
}

impl FolderKind {
    fn env(self) -> &'static str {
        match self {
            FolderKind::Models => "OPENBARK_MODELS_DIR",
            FolderKind::Visits => "OPENBARK_SESSIONS_DIR",
        }
    }

    fn default_name(self) -> &'static str {
        match self {
            FolderKind::Models => "models",
            FolderKind::Visits => "sessions",
        }
    }

    /// The subfolder made inside a chosen folder that already holds other things.
    pub fn subfolder(self) -> &'static str {
        match self {
            FolderKind::Models => "OpenBark models",
            FolderKind::Visits => "OpenBark visits",
        }
    }
}

/// What `locations.json` holds: only the folders the clinic chose.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Locations {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub models: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visits: Option<PathBuf>,
}

impl Locations {
    pub fn get(&self, kind: FolderKind) -> Option<&PathBuf> {
        match kind {
            FolderKind::Models => self.models.as_ref(),
            FolderKind::Visits => self.visits.as_ref(),
        }
    }

    pub fn set(&mut self, kind: FolderKind, path: Option<PathBuf>) {
        match kind {
            FolderKind::Models => self.models = path,
            FolderKind::Visits => self.visits = path,
        }
    }
}

/// Where a folder's location came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// `<local data>/OpenBark/...`
    Default,
    /// Chosen in Settings (`locations.json`).
    Chosen,
    /// Set by an environment variable or a command-line option; not changeable in the app.
    Fixed,
}

fn app_dir() -> PathBuf {
    dirs::data_local_dir().unwrap_or_else(|| PathBuf::from(".")).join("OpenBark")
}

/// `OPENBARK_LOCATIONS_FILE`, else `<local data>/OpenBark/locations.json`.
pub fn config_path() -> PathBuf {
    match std::env::var_os("OPENBARK_LOCATIONS_FILE") {
        Some(p) => PathBuf::from(p),
        None => app_dir().join("locations.json"),
    }
}

/// The saved choices; empty when there's no file or it can't be read.
pub fn load() -> Locations {
    load_from(&config_path())
}

fn load_from(path: &Path) -> Locations {
    match fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
            tracing::warn!(error = %e, file = %path.display(), "locations file unreadable; using the defaults");
            Locations::default()
        }),
        Err(_) => Locations::default(),
    }
}

/// Save the choices (written to a temporary file, then renamed into place).
pub fn save(locations: &Locations) -> io::Result<()> {
    let path = config_path();
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_string_pretty(locations).expect("locations serialise"))?;
    fs::rename(&tmp, &path)
}

/// The default place for a kind of folder, inside the app's data folder.
pub fn default_path(kind: FolderKind) -> PathBuf {
    app_dir().join(kind.default_name())
}

/// The app's own data folder (team list, key, templates, locations.json).
pub fn app_data_dir() -> PathBuf {
    app_dir()
}

/// A network path (`\\server\share`, `//server/share`, `\\?\UNC\...`).
///
/// Windows treats any two leading separators, in any mix of `/` and `\`
/// (`//server`, `/\server`, `\\?\UNC\...`, `\\.\pipe\...`), and NT's `\??\`
/// prefix as network or device paths, so all of them count.
pub fn is_network_path(path: &Path) -> bool {
    let s = path.to_string_lossy();
    let mut chars = s.chars();
    let sep = |c: Option<char>| matches!(c, Some('/' | '\\'));
    (sep(chars.next()) && sep(chars.next())) || s.starts_with(r"\??\")
}

/// Where a kind of folder is now, and why.
pub fn resolve(kind: FolderKind) -> (PathBuf, Source) {
    if let Some(dir) = std::env::var_os(kind.env()) {
        return (PathBuf::from(dir), Source::Fixed);
    }
    if let Some(dir) = load().get(kind) {
        return (dir.clone(), Source::Chosen);
    }
    (app_dir().join(kind.default_name()), Source::Default)
}

/// The environment variable that fixes a kind of folder, for messages.
pub fn env_var(kind: FolderKind) -> &'static str {
    kind.env()
}

/// Strip Windows' `\\?\` prefix so paths compare with mount points and read
/// normally in messages.
pub fn plain(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    match s.strip_prefix(r"\\?\UNC\") {
        Some(rest) => PathBuf::from(format!(r"\\{rest}")),
        None => match s.strip_prefix(r"\\?\") {
            Some(rest) => PathBuf::from(rest),
            None => path.to_path_buf(),
        },
    }
}

/// The nearest existing folder at or above `path`, made absolute.
fn existing_ancestor(path: &Path) -> Option<PathBuf> {
    let mut p = Some(path);
    while let Some(dir) = p {
        if let Ok(c) = fs::canonicalize(dir) {
            return Some(plain(&c));
        }
        p = dir.parent();
    }
    None
}

/// Free bytes on the disk that holds `path` (or would, once created).
pub fn free_space(path: &Path) -> Option<u64> {
    let path = existing_ancestor(path)?;
    let disks = sysinfo::Disks::new_with_refreshed_list();
    disks
        .list()
        .iter()
        .filter(|d| path.starts_with(plain(d.mount_point())))
        .max_by_key(|d| d.mount_point().as_os_str().len())
        .map(|d| d.available_space())
}

/// The disk that holds `path` (or would, once created): its mount point,
/// and whether it can be unplugged.
pub fn disk_of(path: &Path) -> Option<(PathBuf, bool)> {
    let path = existing_ancestor(path)?;
    let disks = sysinfo::Disks::new_with_refreshed_list();
    disks
        .list()
        .iter()
        .filter(|d| path.starts_with(plain(d.mount_point())))
        .max_by_key(|d| d.mount_point().as_os_str().len())
        .map(|d| (plain(d.mount_point()), d.is_removable()))
}

/// The cloud service syncing a folder, judged by its path (OneDrive and the like).
pub fn synced_by(path: &Path) -> Option<&'static str> {
    const SERVICES: [(&str, &str); 6] = [
        ("onedrive", "OneDrive"),
        ("dropbox", "Dropbox"),
        ("google drive", "Google Drive"),
        ("googledrive", "Google Drive"),
        ("mobile documents", "iCloud Drive"),
        ("icloud", "iCloud Drive"),
    ];
    path.components().find_map(|c| {
        let part = c.as_os_str().to_string_lossy().to_lowercase();
        SERVICES.iter().find(|(key, _)| part.starts_with(key)).map(|(_, name)| *name)
    })
}

/// Total size of the files under `dir` (0 if it doesn't exist).
pub fn dir_size(dir: &Path) -> u64 {
    files_under(dir).map(|files| files.iter().map(|(_, n)| n).sum()).unwrap_or(0)
}

/// Every file under `root`, as (path relative to root, size), sorted.
fn files_under(root: &Path) -> io::Result<Vec<(PathBuf, u64)>> {
    let mut out = Vec::new();
    if !root.exists() {
        return Ok(out);
    }
    let mut stack = vec![PathBuf::new()];
    while let Some(rel) = stack.pop() {
        for entry in fs::read_dir(root.join(&rel))? {
            let entry = entry?;
            let kind = entry.file_type()?;
            let child = rel.join(entry.file_name());
            if kind.is_dir() {
                stack.push(child);
            } else if kind.is_file() {
                out.push((child, entry.metadata()?.len()));
            }
        }
    }
    out.sort();
    Ok(out)
}

/// Whether `a` and `b` are the same folder or one is inside the other.
pub fn overlaps(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| {
        existing_ancestor(p).map(|base| {
            // Re-attach the part that doesn't exist yet.
            let tail =
                p.strip_prefix(p.ancestors().find(|x| x.exists()).unwrap_or(p)).unwrap_or(Path::new(""));
            base.join(tail)
        })
    };
    match (norm(a), norm(b)) {
        (Some(a), Some(b)) => {
            let lower = |p: &Path| PathBuf::from(p.to_string_lossy().to_lowercase());
            let (a, b) =
                if cfg!(windows) || cfg!(target_os = "macos") { (lower(&a), lower(&b)) } else { (a, b) };
            a.starts_with(&b) || b.starts_with(&a)
        }
        _ => false,
    }
}

/// What a folder holds, for choosing where OpenBark's files go inside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Contents {
    /// Doesn't exist yet, or is empty.
    Empty,
    /// Already holds OpenBark data of this kind (models or visits).
    OpenBark,
    /// Holds other things.
    Other,
}

/// Look at a folder: empty, OpenBark's, or other. It counts as OpenBark's
/// only when everything in it is OpenBark's (models: a folder per catalog
/// model, the GPU speech pack and the user catalog; visits: visit folders,
/// the patient list and the `.openbark` housekeeping folder, which holds
/// Deleted visits and the deletion record), so a folder that also holds the clinic's other
/// files gets an OpenBark folder inside it instead, and a later move never
/// takes those files along (review 2026-10-18).
pub fn contents(kind: FolderKind, dir: &Path, model_ids: &[&str]) -> Contents {
    let Ok(entries) = fs::read_dir(dir) else { return Contents::Empty };
    let (mut ours, mut other) = (false, false);
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        // Files the operating system leaves everywhere don't count either way.
        if matches!(name.to_ascii_lowercase().as_str(), "desktop.ini" | "thumbs.db" | ".ds_store") {
            continue;
        }
        let mine = match kind {
            FolderKind::Models => {
                (path.is_dir() && models_folder_is_ours(&name, model_ids))
                    || name == "user-catalog.json"
                    || name == "user-catalog.json.tmp"
            }
            FolderKind::Visits => {
                name == "patients.json"
                    || (name == HOUSEKEEPING_DIR && path.is_dir())
                    || path.join("session.json").is_file()
            }
        };
        if mine { ours = true } else { other = true }
    }
    match (ours, other) {
        (_, true) => Contents::Other,
        (true, false) => Contents::OpenBark,
        (false, false) => Contents::Empty,
    }
}

/// The MLX engine pack's download folder (`mlx_pack::PACK_ID`, ADR-0025 §4).
const MLX_PACK_DOWNLOADS: &str = "mlx-engine-pack";

/// A folder at the top of the models folder that OpenBark made: a model's
/// folder (by id, or the first part of an id's safe folder name), a Hub
/// model's `hf_...` folder (`hf:...` on Mac and Linux installs from before
/// ADR-0025), models added from this computer (`local`), and the GPU speech
/// and MLX engine packs' downloads.
fn models_folder_is_ours(name: &str, model_ids: &[&str]) -> bool {
    name.starts_with("hf_")
        || name.starts_with("hf:")
        || name == "local"
        || name == MLX_PACK_DOWNLOADS
        || name == crate::gpu_pack::PACK_ID
        || model_ids.iter().any(|id| {
            *id == name || crate::store::dir_name(id).split('/').next().is_some_and(|first| first == name)
        })
}

/// What [`move_tree`] did.
#[derive(Debug, Default, Clone, Serialize)]
pub struct Moved {
    pub files: usize,
    pub bytes: u64,
    /// Files already in the new folder (same name and size), left as they were.
    pub already_there: usize,
}

/// Move everything under `from` into `to`. Files with the same name and size
/// already in `to` are kept and the old copy removed when `same_size_is_same`
/// (models: the store re-verifies them); any other clash stops the move before
/// anything changes. Each file is renamed when it can be (same disk) and
/// copied otherwise; if a copy fails, what was copied is removed and `from`
/// is left as it was. `progress(done, total)` reports bytes.
pub fn move_tree(
    from: &Path,
    to: &Path,
    same_size_is_same: bool,
    progress: impl FnMut(u64, u64),
) -> io::Result<Moved> {
    move_tree_with(from, to, same_size_is_same, true, progress)
}

/// [`move_tree`], with renaming switched off in tests to exercise the copy
/// path a move to another disk takes.
fn move_tree_with(
    from: &Path,
    to: &Path,
    same_size_is_same: bool,
    allow_rename: bool,
    mut progress: impl FnMut(u64, u64),
) -> io::Result<Moved> {
    let files = files_under(from)?;
    let total: u64 = files.iter().map(|(_, n)| n).sum();
    // Look before touching anything.
    let mut plan = Vec::with_capacity(files.len());
    for (rel, size) in &files {
        let dest = to.join(rel);
        match fs::metadata(&dest) {
            Ok(m) if m.is_file() && m.len() == *size && same_size_is_same => plan.push((rel, *size, false)),
            Ok(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    format!("{} already has a different {}", plain(to).display(), rel.display()),
                ));
            }
            Err(_) => plan.push((rel, *size, true)),
        }
    }
    fs::create_dir_all(to)?;
    let mut done = 0u64;
    let mut placed: Vec<(PathBuf, bool)> = Vec::new(); // (relative path, renamed)
    progress(0, total);
    let result = (|| -> io::Result<Moved> {
        let mut moved = Moved::default();
        for (rel, size, copy) in &plan {
            if !copy {
                moved.already_there += 1;
                done += size;
                progress(done, total);
                continue;
            }
            let src = from.join(rel);
            let dest = to.join(rel);
            if let Some(dir) = dest.parent() {
                fs::create_dir_all(dir)?;
            }
            if allow_rename && fs::rename(&src, &dest).is_ok() {
                placed.push((rel.to_path_buf(), true));
                done += size;
                progress(done, total);
            } else {
                placed.push((rel.to_path_buf(), false));
                copy_with_progress(&src, &dest, |n| {
                    progress(done + n, total);
                })?;
                if fs::metadata(&dest)?.len() != *size {
                    return Err(io::Error::other(format!("{} didn't copy completely", rel.display())));
                }
                done += size;
            }
            moved.files += 1;
            moved.bytes += size;
        }
        Ok(moved)
    })();
    match result {
        Ok(moved) => {
            // Copied (not renamed) originals, and same-size duplicates, go now.
            for (rel, _, copy) in &plan {
                let renamed = placed.iter().any(|(p, r)| p == *rel && *r);
                if (*copy && !renamed) || (!copy && same_size_is_same) {
                    let _ = fs::remove_file(from.join(rel));
                }
            }
            remove_empty_dirs(from);
            Ok(moved)
        }
        Err(e) => {
            // Undo: renamed files go back, copies are deleted.
            for (rel, renamed) in placed.iter().rev() {
                if *renamed {
                    let _ = fs::rename(to.join(rel), from.join(rel));
                } else {
                    let _ = fs::remove_file(to.join(rel));
                }
            }
            remove_empty_dirs(to);
            Err(e)
        }
    }
}

fn copy_with_progress(src: &Path, dest: &Path, mut progress: impl FnMut(u64)) -> io::Result<()> {
    let mut input = fs::File::open(src)?;
    let mut output = fs::File::create(dest)?;
    let mut buf = vec![0u8; 1 << 20];
    let mut copied = 0u64;
    let mut reported = 0u64;
    loop {
        let n = input.read(&mut buf)?;
        if n == 0 {
            break;
        }
        output.write_all(&buf[..n])?;
        copied += n as u64;
        if copied - reported >= 16 << 20 {
            reported = copied;
            progress(copied);
        }
    }
    output.flush()?;
    output.sync_all()?;
    progress(copied);
    Ok(())
}

/// Remove empty folders under `root`, deepest first, and `root` if it ends up empty.
fn remove_empty_dirs(root: &Path) {
    fn walk(dir: &Path) -> bool {
        let Ok(entries) = fs::read_dir(dir) else { return false };
        let mut empty = true;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if walk(&path) {
                    let _ = fs::remove_dir(&path);
                } else {
                    empty = false;
                }
            } else {
                empty = false;
            }
        }
        empty
    }
    if walk(root) {
        let _ = fs::remove_dir(root);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, bytes: &[u8]) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    #[test]
    fn moves_everything_and_cleans_up() {
        let tmp = tempfile::tempdir().unwrap();
        let from = tmp.path().join("old");
        let to = tmp.path().join("new").join("OpenBark visits");
        write(&from.join("patients.json"), b"[]");
        write(&from.join("2026-10-18_1/session.json"), b"{}");
        write(&from.join("2026-10-18_1/runs/001/note.json"), &[7u8; 5000]);
        let mut last = (0, 0);
        let moved = move_tree(&from, &to, false, |d, t| last = (d, t)).unwrap();
        assert_eq!(moved.files, 3);
        assert_eq!(last, (5004, 5004));
        assert_eq!(fs::read(to.join("2026-10-18_1/runs/001/note.json")).unwrap().len(), 5000);
        assert!(!from.exists(), "the old folder is removed once empty");
    }

    #[test]
    fn copies_across_disks_and_removes_the_originals() {
        let tmp = tempfile::tempdir().unwrap();
        let from = tmp.path().join("old");
        let to = tmp.path().join("new");
        write(&from.join("gemma/model.gguf"), &[3u8; 3 << 20]);
        write(&from.join("catalog.json"), b"{}");
        let mut reports = 0;
        let moved = move_tree_with(&from, &to, true, false, |_, _| reports += 1).unwrap();
        assert_eq!(moved.files, 2);
        assert_eq!(fs::metadata(to.join("gemma/model.gguf")).unwrap().len(), 3 << 20);
        assert!(!from.exists(), "originals are removed after every copy succeeded");
        assert!(reports >= 3);
    }

    #[test]
    fn a_different_file_already_there_stops_before_anything_moves() {
        let tmp = tempfile::tempdir().unwrap();
        let from = tmp.path().join("old");
        let to = tmp.path().join("new");
        write(&from.join("a/session.json"), b"{\"x\":1}");
        write(&from.join("b/session.json"), b"{}");
        write(&to.join("b/session.json"), b"{\"different\":true}");
        let err = move_tree(&from, &to, false, |_, _| {}).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert!(from.join("a/session.json").is_file() && from.join("b/session.json").is_file());
        assert!(!to.join("a").exists(), "nothing was moved");
    }

    #[test]
    fn same_size_models_already_there_are_kept() {
        let tmp = tempfile::tempdir().unwrap();
        let from = tmp.path().join("old");
        let to = tmp.path().join("new");
        write(&from.join("gemma/model.gguf"), &[1u8; 100]);
        write(&from.join("vad/silero.onnx"), &[2u8; 10]);
        write(&to.join("gemma/model.gguf"), &[1u8; 100]);
        let moved = move_tree(&from, &to, true, |_, _| {}).unwrap();
        assert_eq!((moved.files, moved.already_there), (1, 1));
        assert!(to.join("vad/silero.onnx").is_file());
        assert!(!from.exists());
    }

    #[test]
    fn every_spelling_of_a_network_path_counts() {
        for p in [
            r"\\server\share",
            "//server/share",
            r"/\server\x",
            r"\/server/x",
            r"\\?\UNC\server\x",
            r"\\.\pipe\x",
            r"\??\UNC\server\x",
        ] {
            assert!(is_network_path(Path::new(p)), "{p}");
        }
        for p in [r"C:\Clinic", "/home/vet/OpenBark", r"\Users\x", "relative"] {
            assert!(!is_network_path(Path::new(p)), "{p}");
        }
    }

    #[test]
    fn folders_inside_each_other_overlap() {
        let tmp = tempfile::tempdir().unwrap();
        let a = tmp.path().join("data");
        fs::create_dir_all(&a).unwrap();
        assert!(overlaps(&a, &a.join("models")));
        assert!(overlaps(&a.join("models"), &a));
        assert!(!overlaps(&a.join("models"), &a.join("visits")));
    }

    #[test]
    fn contents_tell_openbark_folders_from_others() {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path();
        assert_eq!(contents(FolderKind::Visits, &d.join("missing"), &[]), Contents::Empty);
        write(&d.join("v/2026/session.json"), b"{}");
        assert_eq!(contents(FolderKind::Visits, &d.join("v"), &[]), Contents::OpenBark);
        write(&d.join("m/gemma-4-e4b/x.gguf"), b"");
        write(&d.join("m/desktop.ini"), b"");
        assert_eq!(contents(FolderKind::Models, &d.join("m"), &["gemma-4-e4b"]), Contents::OpenBark);
        write(&d.join("docs/letter.txt"), b"hi");
        assert_eq!(contents(FolderKind::Models, &d.join("docs"), &["gemma-4-e4b"]), Contents::Other);
        // A folder of the user's own files with one model file in it is not ours.
        write(&d.join("mixed/gemma-4-e4b/x.gguf"), b"");
        write(&d.join("mixed/holiday.jpg"), b"");
        assert_eq!(contents(FolderKind::Models, &d.join("mixed"), &["gemma-4-e4b"]), Contents::Other);
        write(&d.join("loose/model.gguf"), b"");
        assert_eq!(contents(FolderKind::Models, &d.join("loose"), &["gemma-4-e4b"]), Contents::Other);
    }

    /// ADR-0025: Hub models' folders (safe and old names), models added from
    /// this computer and both packs' downloads are OpenBark's.
    #[test]
    fn hub_local_and_pack_folders_count_as_openbark() {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path().join("m");
        for folder in [
            "hf_bartowski/x/m.gguf",
            "local/mine/m.gguf",
            "mlx-engine-pack/a.whl",
            "gpu-speech-pack/a.tar.bz2",
        ] {
            write(&d.join(folder), b"");
        }
        #[cfg(unix)]
        write(&d.join("hf:unsloth/x/m.gguf"), b"");
        write(&d.join("user-catalog.json"), b"{}");
        assert_eq!(contents(FolderKind::Models, &d, &[]), Contents::OpenBark);
        // A model id with folders counts by its first folder.
        write(&d.join("nested/model/x.gguf"), b"");
        assert_eq!(contents(FolderKind::Models, &d, &[]), Contents::Other);
        assert_eq!(contents(FolderKind::Models, &d, &["nested/model"]), Contents::OpenBark);
        // A file called local isn't the folder; other things still make it someone else's.
        write(&d.join("holiday.jpg"), b"");
        assert_eq!(contents(FolderKind::Models, &d, &["nested/model"]), Contents::Other);
    }

    /// ADR-0022: the housekeeping folder (Deleted visits, the deletion record)
    /// is OpenBark's own, so a visits folder holding it still looks like ours
    /// on a new computer, even when every visit in it was deleted.
    #[test]
    fn the_housekeeping_folder_counts_as_openbark() {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path();
        write(&d.join("v/2026/session.json"), b"{}");
        write(&d.join("v/.openbark/audit.log"), b"");
        assert_eq!(contents(FolderKind::Visits, &d.join("v"), &[]), Contents::OpenBark);
        write(&d.join("only/.openbark/deleted/0199/session.json"), b"{}");
        assert_eq!(contents(FolderKind::Visits, &d.join("only"), &[]), Contents::OpenBark);
        write(&d.join("mixed/.openbark/audit.log"), b"");
        write(&d.join("mixed/notes.docx"), b"");
        assert_eq!(contents(FolderKind::Visits, &d.join("mixed"), &[]), Contents::Other);
        // A file that happens to be called .openbark isn't the folder.
        write(&d.join("odd/.openbark"), b"");
        assert_eq!(contents(FolderKind::Visits, &d.join("odd"), &[]), Contents::Other);
    }

    #[test]
    fn moving_visits_carries_deleted_visits_and_the_record() {
        let tmp = tempfile::tempdir().unwrap();
        let from = tmp.path().join("old");
        let to = tmp.path().join("new");
        write(&from.join("2026-10-18_1/session.json"), b"{}");
        write(&from.join(".openbark/deleted/0199-a/session.json"), b"{\"x\":1}");
        write(&from.join(".openbark/audit.log"), b"OBL1:abc\n");
        let moved = move_tree(&from, &to, false, |_, _| {}).unwrap();
        assert_eq!(moved.files, 3);
        assert!(to.join(".openbark/deleted/0199-a/session.json").is_file());
        assert_eq!(fs::read(to.join(".openbark/audit.log")).unwrap(), b"OBL1:abc\n");
        assert!(!from.exists());
    }

    #[test]
    fn saved_choices_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("locations.json");
        let mut l = Locations::default();
        l.set(FolderKind::Models, Some(PathBuf::from("D:/AI/OpenBark models")));
        fs::write(&file, serde_json::to_string(&l).unwrap()).unwrap();
        assert_eq!(load_from(&file), l);
        assert_eq!(load_from(&tmp.path().join("missing.json")), Locations::default());
    }
}
