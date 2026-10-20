use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

use sha1::Sha1;
use sha2::{Digest, Sha256};

use crate::catalog::{CatalogEntry, FileSpec, Layout};

/// How thoroughly to check an installed file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verify {
    /// Size only (fast; used on every run).
    Size,
    /// Size and hash (SHA-256, or the git blob SHA-1 for small Hub files):
    /// used after download and by `models verify`.
    Full,
}

/// A path from a catalog entry or a Hub listing that isn't safe to use.
#[derive(Debug, thiserror::Error)]
#[error("unsafe path in model files: {0}")]
pub struct UnsafePath(pub String);

/// A model id as a folder path that is valid on every platform: ':' and the
/// Windows-reserved characters `<>"|?*` and control characters become '_';
/// '/' stays a separator. `"hf:bartowski/x/y"` -> `"hf_bartowski/x/y"`.
///
/// Also, so an id can never climb out of the store: a backslash becomes '_',
/// and an empty, "." or ".." component becomes "_".
pub fn dir_name(id: &str) -> String {
    let mapped: String = id
        .chars()
        .map(|c| match c {
            ':' | '<' | '>' | '"' | '|' | '?' | '*' | '\\' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    mapped
        .split('/')
        .map(|part| if part.is_empty() || part == "." || part == ".." { "_" } else { part })
        .collect::<Vec<_>>()
        .join("/")
}

/// A repo-relative path, checked: not empty, not absolute, no "..", "\\", ":",
/// NUL, or empty components. Returns it as a PathBuf with native separators.
pub fn safe_rel_path(p: &str) -> Result<PathBuf, UnsafePath> {
    let bad = || UnsafePath(p.to_string());
    if p.is_empty() || p.contains(['\\', ':', '\0']) {
        return Err(bad());
    }
    let mut out = PathBuf::new();
    for part in p.split('/') {
        if part.is_empty() || part == "." || part == ".." {
            return Err(bad());
        }
        out.push(part);
    }
    if out.is_absolute() || out.has_root() {
        return Err(bad());
    }
    Ok(out)
}

/// `<path>.part`: where a download (or a copy) is written before it's verified.
pub(crate) fn part_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    name.push(".part");
    path.with_file_name(name)
}

/// Both hashes of one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileHashes {
    pub sha256: String,
    pub git_sha1: String,
}

/// One pass over the file: SHA-256 and git blob SHA-1 ("blob {len}\0" + bytes).
pub fn hash_file(path: &Path) -> std::io::Result<FileHashes> {
    let file = File::open(path)?;
    let len = file.metadata()?.len();
    let mut reader = BufReader::with_capacity(1 << 20, file);
    let mut sha256 = Sha256::new();
    let mut sha1 = Sha1::new();
    sha1.update(format!("blob {len}\0").as_bytes());
    let mut buf = vec![0u8; 1 << 20];
    let mut read = 0u64;
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        sha256.update(&buf[..n]);
        sha1.update(&buf[..n]);
        read += n as u64;
    }
    if read != len {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("{} changed while it was being read", path.display()),
        ));
    }
    Ok(FileHashes { sha256: hex::encode(sha256.finalize()), git_sha1: hex::encode(sha1.finalize()) })
}

/// sha256 when the spec has one, otherwise git_sha1. Case-insensitive. A
/// spec with neither never matches.
pub fn hashes_match(spec: &FileSpec, h: &FileHashes) -> bool {
    if !spec.sha256.is_empty() {
        return spec.sha256.eq_ignore_ascii_case(&h.sha256);
    }
    match &spec.git_sha1 {
        Some(sha1) if !sha1.is_empty() => sha1.eq_ignore_ascii_case(&h.git_sha1),
        _ => false,
    }
}

/// Where models live on disk: `<root>/<dir_name(model-id)>/<file>`.
#[derive(Debug, Clone)]
pub struct ModelStore {
    root: PathBuf,
}

impl ModelStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// `OPENBARK_MODELS_DIR`, else the folder chosen in Settings, else the
    /// platform data directory (ADR-0013, [`crate::locations`]).
    pub fn default_location() -> Self {
        Self::new(crate::locations::resolve(crate::locations::FolderKind::Models).0)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// In place (`Source::Local { path: Some }`): the MLX folder itself, or a
    /// file's parent folder. Otherwise `<root>/<dir_name(id)>`, except that an
    /// existing folder under the raw id (Mac and Linux installs from before
    /// ADR-0025) is still used.
    pub fn model_dir(&self, entry: &CatalogEntry) -> PathBuf {
        if let Some(p) = entry.in_place() {
            return match entry.layout {
                Layout::LlmMlx { .. } => p.to_path_buf(),
                _ => p.parent().map(Path::to_path_buf).unwrap_or_else(|| p.to_path_buf()),
            };
        }
        let safe = dir_name(&entry.id);
        // Windows never had these folders: a ':' in a folder name fails there
        // (WinError 267), and "hf:x" would name an NTFS stream.
        if cfg!(not(windows)) && safe != entry.id && legacy_id_ok(&entry.id) {
            let legacy = self.root.join(&entry.id);
            if legacy.is_dir() {
                return legacy;
            }
        }
        self.root.join(safe)
    }

    /// A file of the model. Repo paths with folders get native separators.
    pub fn file_path(&self, entry: &CatalogEntry, name: &str) -> PathBuf {
        match safe_rel_path(name) {
            Ok(rel) => self.model_dir(entry).join(rel),
            Err(_) => self.model_dir(entry).join(name),
        }
    }

    pub fn file_ok(&self, entry: &CatalogEntry, file: &FileSpec, verify: Verify) -> bool {
        if safe_rel_path(&file.path).is_err() {
            return false;
        }
        let path = self.file_path(entry, &file.path);
        let Ok(meta) = std::fs::metadata(&path) else { return false };
        if !meta.is_file() || meta.len() != file.size {
            return false;
        }
        verify == Verify::Size || hash_file(&path).is_ok_and(|h| hashes_match(file, &h))
    }

    pub fn is_installed(&self, entry: &CatalogEntry, verify: Verify) -> bool {
        entry.files.iter().all(|f| self.file_ok(entry, f, verify))
    }

    /// Bytes on disk for this model's listed files plus their `.part` files,
    /// counting only data that is OpenBark's alone: a file that is also
    /// reachable under another name (a hard link, as a copy from this computer
    /// on the same drive is) frees nothing when removed, so it isn't counted
    /// here but in [`ModelStore::shared_bytes`]. 0 when used in place (those
    /// bytes aren't OpenBark's).
    pub fn disk_usage(&self, entry: &CatalogEntry) -> u64 {
        if entry.in_place().is_some() {
            return 0;
        }
        self.listed_paths(entry).map(|p| own_len(&p) + own_len(&part_path(&p))).sum()
    }

    /// Bytes in this model's listed files whose data another name also holds
    /// (hard links): on disk, but removing them frees no space. 0 when used
    /// in place.
    pub fn shared_bytes(&self, entry: &CatalogEntry) -> u64 {
        if entry.in_place().is_some() {
            return 0;
        }
        self.listed_paths(entry).map(|p| shared_len(&p)).sum()
    }

    /// Bytes in `.part` files only (a download that stopped part-way).
    pub fn partial_bytes(&self, entry: &CatalogEntry) -> u64 {
        if entry.in_place().is_some() {
            return 0;
        }
        self.listed_paths(entry).map(|p| file_len(&part_path(&p))).sum()
    }

    fn listed_paths<'a>(&'a self, entry: &'a CatalogEntry) -> impl Iterator<Item = PathBuf> + 'a {
        let dir = self.model_dir(entry);
        entry.files.iter().filter_map(move |f| safe_rel_path(&f.path).ok().map(|rel| dir.join(rel)))
    }
}

/// An id used as a folder path before ADR-0025: ':' was allowed, but it still
/// may not climb out of the store (no "..", "." or empty parts, no "\\" or
/// NUL, not absolute).
fn legacy_id_ok(id: &str) -> bool {
    !id.is_empty()
        && !id.contains(['\\', '\0'])
        && !id.starts_with('/')
        && id.split('/').all(|p| !p.is_empty() && p != "." && p != "..")
}

fn file_len(path: &Path) -> u64 {
    std::fs::metadata(path).ok().filter(|m| m.is_file()).map(|m| m.len()).unwrap_or(0)
}

/// The file's size when its data has no other name, else 0.
fn own_len(path: &Path) -> u64 {
    match file_len(path) {
        0 => 0,
        len if link_count(path) <= 1 => len,
        _ => 0,
    }
}

/// The file's size when its data also has another name (a hard link), else 0.
fn shared_len(path: &Path) -> u64 {
    match file_len(path) {
        0 => 0,
        len if link_count(path) > 1 => len,
        _ => 0,
    }
}

/// How many names a file's data has: 1 for an ordinary file, more for a
/// hard link (a copy from this computer on the same drive, ADR-0025 §9, or a
/// file two catalog entries share). Links are followed; 1 when unknown.
pub(crate) fn link_count(path: &Path) -> u64 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata(path).map(|m| m.nlink()).unwrap_or(1)
    }
    #[cfg(windows)]
    {
        windows_link_count(path).unwrap_or(1)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = path;
        1
    }
}

/// `nNumberOfLinks` from GetFileInformationByHandle (std's own accessor for
/// it is still unstable).
#[cfg(windows)]
fn windows_link_count(path: &Path) -> Option<u64> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle};
    let file = File::open(path).ok()?;
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: `file` is an open handle for the whole call, and `info` is a
    // properly sized, writable BY_HANDLE_FILE_INFORMATION.
    let ok = unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) };
    (ok != 0).then_some(u64::from(info.nNumberOfLinks))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{Slot, Source};

    fn entry(id: &str, files: &[(&str, u64)]) -> CatalogEntry {
        CatalogEntry {
            id: id.into(),
            display_name: id.into(),
            slot: Slot::Llm,
            license: "mit".into(),
            attribution: None,
            source: Source::Huggingface { repo: "a/b".into(), revision: "0".repeat(40) },
            files: files
                .iter()
                .map(|(p, s)| FileSpec {
                    path: (*p).into(),
                    size: *s,
                    sha256: String::new(),
                    git_sha1: None,
                    url: None,
                })
                .collect(),
            layout: Layout::LlmGguf { model: files[0].0.into() },
            gated: false,
            mobile: false,
        }
    }

    #[test]
    fn hashes_of_known_content() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x");
        std::fs::write(&path, b"abc").unwrap();
        let h = hash_file(&path).unwrap();
        assert_eq!(h.sha256, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(h.git_sha1, "f2ba8f84ab5c1bce84a7b441cb1959cfc7093b7f");
    }

    /// The live Hub's blob id for this file (revision 73e3e38d...), so the
    /// git blob SHA-1 is computed the way the Hub lists it.
    #[test]
    fn git_sha1_matches_the_hub() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/hub/config_mlx-community_Qwen3-0.6B-4bit.json");
        assert_eq!(hash_file(&path).unwrap().git_sha1, "6c974b02334f2f5f68de4cb2796ee4fe80bfc712");
    }

    #[test]
    fn hashes_match_prefers_sha256() {
        let h = FileHashes { sha256: "AB".repeat(32), git_sha1: "cd".repeat(20) };
        let mut spec = entry("x", &[("f", 1)]).files.remove(0);
        assert!(!hashes_match(&spec, &h), "no hash at all never matches");
        spec.git_sha1 = Some("CD".repeat(20));
        assert!(hashes_match(&spec, &h));
        spec.sha256 = "ab".repeat(32);
        assert!(hashes_match(&spec, &h));
        spec.sha256 = "00".repeat(32);
        assert!(!hashes_match(&spec, &h), "sha256 wins over a matching git sha1");
    }

    #[test]
    fn folder_names_are_safe_everywhere() {
        assert_eq!(dir_name("hf:bartowski/x/y"), "hf_bartowski/x/y");
        assert_eq!(dir_name("gemma-4-e4b"), "gemma-4-e4b");
        assert_eq!(dir_name("a<b>c\"d|e?f*g\u{1}h"), "a_b_c_d_e_f_g_h");
        assert_eq!(dir_name("hf:x/../../etc"), "hf_x/_/_/etc");
        assert_eq!(dir_name("a\\b"), "a_b");
        assert_eq!(
            dir_name("hf:mlx-community/Qwen3-0.6B-4bit/@mlx"),
            "hf_mlx-community/Qwen3-0.6B-4bit/@mlx"
        );
    }

    #[test]
    fn unsafe_repo_paths_are_refused() {
        for bad in ["", "/etc/passwd", "a/../b", "..", "a//b", "a/", "a\\b", "C:/x", "a\0b", "./a", "a/./b"] {
            assert!(safe_rel_path(bad).is_err(), "{bad:?}");
        }
        assert_eq!(
            safe_rel_path("Q4_K_M/m-00001-of-00003.gguf").unwrap(),
            Path::new("Q4_K_M").join("m-00001-of-00003.gguf")
        );
        assert_eq!(safe_rel_path("config.json").unwrap(), PathBuf::from("config.json"));
    }

    /// WinError 267 before ADR-0025: a colon in a folder name.
    #[test]
    fn the_folder_for_a_hub_id_can_be_created() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ModelStore::new(tmp.path());
        let e = entry("hf:bartowski/Qwen_Qwen3-4B-GGUF/Qwen_Qwen3-4B-Q4_K_M", &[("m.gguf", 3)]);
        let dir = store.model_dir(&e);
        assert_eq!(dir, tmp.path().join("hf_bartowski/Qwen_Qwen3-4B-GGUF/Qwen_Qwen3-4B-Q4_K_M"));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(store.file_path(&e, "m.gguf"), b"abc").unwrap();
        assert!(store.is_installed(&e, Verify::Size));
    }

    #[cfg(unix)]
    #[test]
    fn an_existing_folder_under_the_raw_id_is_still_used() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ModelStore::new(tmp.path());
        let e = entry("hf:a/b/c", &[("m.gguf", 3)]);
        assert_eq!(store.model_dir(&e), tmp.path().join("hf_a/b/c"));
        std::fs::create_dir_all(tmp.path().join("hf:a/b/c")).unwrap();
        assert_eq!(store.model_dir(&e), tmp.path().join("hf:a/b/c"));
        // An old id that would climb out is never followed.
        std::fs::create_dir_all(tmp.path().join("x/up")).unwrap();
        let sneaky = entry("hf:a/../../x/up", &[("m.gguf", 3)]);
        assert_eq!(store.model_dir(&sneaky), tmp.path().join("hf_a/_/_/x/up"));
    }

    #[test]
    fn old_ids_may_have_colons_but_never_climb_out() {
        assert!(legacy_id_ok("hf:bartowski/Qwen_Qwen3-4B-GGUF/Qwen_Qwen3-4B-Q4_K_M"));
        assert!(legacy_id_ok("gemma-4-e4b"));
        for bad in ["", "/abs", "hf:a/../b", "hf:a//b", "hf:a/./b", "a\\b", "a\0b", "hf:a/"] {
            assert!(!legacy_id_ok(bad), "{bad:?}");
        }
    }

    #[test]
    fn in_place_models_live_where_they_are() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ModelStore::new(tmp.path().join("store"));
        let file = tmp.path().join("elsewhere").join("m.gguf");
        let mut e = entry("local/m", &[("m.gguf", 3)]);
        e.source = Source::Local { path: Some(file.to_string_lossy().into_owned()) };
        assert_eq!(store.model_dir(&e), tmp.path().join("elsewhere"));
        e.layout = Layout::LlmMlx { config: "config.json".into() };
        e.source = Source::Local { path: Some(tmp.path().join("mlx").to_string_lossy().into_owned()) };
        assert_eq!(store.model_dir(&e), tmp.path().join("mlx"));
        // Copied local models live in the store.
        e.source = Source::Local { path: None };
        assert_eq!(store.model_dir(&e), tmp.path().join("store").join("local").join("m"));
    }

    #[test]
    fn disk_usage_counts_listed_and_partial_files_only() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ModelStore::new(tmp.path());
        let e = entry("hf:a/b/split", &[("Q4/m-00001-of-00002.gguf", 10), ("Q4/m-00002-of-00002.gguf", 20)]);
        let dir = store.model_dir(&e);
        std::fs::create_dir_all(dir.join("Q4")).unwrap();
        std::fs::write(dir.join("Q4/m-00001-of-00002.gguf"), [0u8; 10]).unwrap();
        std::fs::write(dir.join("Q4/m-00002-of-00002.gguf.part"), [0u8; 7]).unwrap();
        std::fs::write(dir.join("Q4/stranger.txt"), [0u8; 100]).unwrap();
        assert_eq!(store.disk_usage(&e), 17);
        assert_eq!(store.partial_bytes(&e), 7);

        let mut in_place = e.clone();
        in_place.source =
            Source::Local { path: Some(dir.join("Q4/m-00001-of-00002.gguf").to_string_lossy().into_owned()) };
        assert_eq!(store.disk_usage(&in_place), 0);
        assert_eq!(store.partial_bytes(&in_place), 0);
    }

    /// A hard link (a copy from this computer on the same drive) frees
    /// nothing when removed: it counts as shared, not as OpenBark's own.
    #[test]
    fn hard_linked_files_count_as_shared_not_as_their_own() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ModelStore::new(tmp.path().join("store"));
        let e = entry("local/m", &[("m.gguf", 10), ("n.gguf", 7)]);
        let dir = store.model_dir(&e);
        std::fs::create_dir_all(&dir).unwrap();
        let original = tmp.path().join("m.gguf");
        std::fs::write(&original, [1u8; 10]).unwrap();
        std::fs::hard_link(&original, dir.join("m.gguf")).unwrap();
        std::fs::write(dir.join("n.gguf"), [2u8; 7]).unwrap();
        assert_eq!(link_count(&dir.join("m.gguf")), 2);
        assert_eq!(link_count(&dir.join("n.gguf")), 1);
        assert_eq!(link_count(&dir.join("absent.gguf")), 1, "unknown counts as one name");
        assert_eq!(store.disk_usage(&e), 7);
        assert_eq!(store.shared_bytes(&e), 10);
        // Once the original is gone, the copy is OpenBark's alone.
        std::fs::remove_file(&original).unwrap();
        assert_eq!(store.disk_usage(&e), 17);
        assert_eq!(store.shared_bytes(&e), 0);
    }

    #[test]
    fn file_ok_verifies_by_git_sha1() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ModelStore::new(tmp.path());
        let mut e = entry("hf:a/b/@mlx", &[("config.json", 3)]);
        e.files[0].git_sha1 = Some("f2ba8f84ab5c1bce84a7b441cb1959cfc7093b7f".into());
        let dir = store.model_dir(&e);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("config.json"), b"abc").unwrap();
        assert!(store.file_ok(&e, &e.files[0], Verify::Full));
        std::fs::write(dir.join("config.json"), b"abd").unwrap();
        assert!(store.file_ok(&e, &e.files[0], Verify::Size));
        assert!(!store.file_ok(&e, &e.files[0], Verify::Full));
    }
}
