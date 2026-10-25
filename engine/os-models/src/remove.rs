//! Removing models (ADR-0025 §10): delete exactly the files a catalog entry
//! lists (and their `.part` downloads), never a whole folder, never anything
//! outside the model store, never through a link, and never a model used in
//! place. No network.
//!
//! The app's guards (the note model, a busy model, a running download, the
//! speech models OpenBark needs) are not here; `os-app` applies them first.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::catalog::{CatalogEntry, Source};
use crate::hub;
use crate::locations;
use crate::store::{ModelStore, link_count, part_path, safe_rel_path};

/// What a removal did.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Removal {
    pub id: String,
    /// Space really freed: files whose data had no other name.
    pub freed_bytes: u64,
    /// Bytes in removed files whose data another name still holds (a hard
    /// link, as a copy from this computer on the same drive is): deleting
    /// them freed nothing, because the original keeps that space.
    pub shared_bytes: u64,
    pub files_removed: usize,
    /// Dropped from user-catalog.json.
    pub forgotten: bool,
    /// Used in place: nothing deleted.
    pub kept_in_place: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoveRule {
    /// Built-in or Hub: delete files, keep the row.
    Delete,
    /// Copied from this computer: delete the copy, drop the row.
    DeleteAndForget,
    /// Used in place: delete nothing, drop the row.
    ForgetOnly,
}

#[derive(Debug, thiserror::Error)]
pub enum RemoveError {
    #[error("OpenBark won't delete {0}: it's outside the models folder.")]
    OutsideStore(String),
    #[error("OpenBark won't delete {0}: it's a link to somewhere else.")]
    Link(String),
    #[error("{0} comes with the app, so it can't be removed.")]
    Bundled(String),
    #[error(transparent)]
    Unsafe(#[from] crate::store::UnsafePath),
    #[error(transparent)]
    Io(#[from] io::Error),
}

pub fn rule(entry: &CatalogEntry) -> RemoveRule {
    match &entry.source {
        Source::Local { path: Some(_) } => RemoveRule::ForgetOnly,
        Source::Local { path: None } => RemoveRule::DeleteAndForget,
        _ => RemoveRule::Delete,
    }
}

/// A symbolic link, or (Windows) any reparse point such as a junction.
pub(crate) fn is_link(meta: &fs::Metadata) -> bool {
    if meta.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        if meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return true;
        }
    }
    false
}

fn shown(p: &Path) -> String {
    locations::plain(p).display().to_string()
}

/// Delete exactly the entry's listed files and their `.part` files, then empty
/// folders up to, never including, the store root. Each path: safe_rel_path,
/// symlink_metadata (refuse links and junctions), canonicalise, must start with
/// the canonical root and differ from it. In place: Ok with kept_in_place and
/// nothing touched. Missing files are fine (freed counts what was there).
///
/// Every path is checked before the first one is deleted, so a refusal
/// deletes nothing.
pub fn remove_files(store: &ModelStore, entry: &CatalogEntry) -> Result<Removal, RemoveError> {
    let mut removal = Removal { id: entry.id.clone(), ..Removal::default() };
    if store.is_bundled(entry) {
        return Err(RemoveError::Bundled(entry.display_name.clone()));
    }
    if entry.in_place().is_some() {
        // Its path is never passed to any delete call.
        removal.kept_in_place = true;
        return Ok(removal);
    }
    let rels = entry.files.iter().map(|f| safe_rel_path(&f.path)).collect::<Result<Vec<_>, _>>()?;
    let root = match fs::canonicalize(store.root()) {
        Ok(r) => r,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(removal),
        Err(e) => return Err(e.into()),
    };
    let dir = store.model_dir(entry);
    let below =
        dir.strip_prefix(store.root()).map_err(|_| RemoveError::OutsideStore(shown(&dir)))?.to_path_buf();
    if below.as_os_str().is_empty() {
        return Err(RemoveError::OutsideStore(shown(&dir)));
    }

    // Look first.
    let mut doomed: Vec<(PathBuf, u64, bool)> = Vec::new();
    for rel in &rels {
        let file = below.join(rel);
        for rel_path in [part_path(&file), file] {
            if let Some(found) = check(store.root(), &root, &rel_path)? {
                doomed.push(found);
            }
        }
    }
    doomed.sort();
    doomed.dedup();

    // Then delete.
    for (path, size, shared) in &doomed {
        match fs::remove_file(path) {
            Ok(()) => {
                if *shared {
                    removal.shared_bytes += size;
                } else {
                    removal.freed_bytes += size;
                }
                removal.files_removed += 1;
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    for (path, _, _) in &doomed {
        prune(&root, path);
    }
    tracing::info!(
        id = %entry.id,
        files = removal.files_removed,
        bytes = removal.freed_bytes,
        shared = removal.shared_bytes,
        "model files removed"
    );
    Ok(removal)
}

/// One path to delete, checked: no link anywhere between the root and it, a
/// plain file, strictly inside the canonical root. `Ok(None)` when it isn't
/// there. Returns the canonical path, its size, and whether its data has
/// another name (a hard link), so deleting it frees nothing.
fn check(
    root: &Path,
    canonical_root: &Path,
    rel: &Path,
) -> Result<Option<(PathBuf, u64, bool)>, RemoveError> {
    // Every component below the root, the file itself included.
    let path = root.join(rel);
    let mut cur = root.to_path_buf();
    for c in rel.components() {
        cur.push(c);
        match fs::symlink_metadata(&cur) {
            Ok(m) if is_link(&m) => return Err(RemoveError::Link(shown(&cur))),
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        }
    }
    let meta = fs::symlink_metadata(&path)?;
    if !meta.is_file() {
        return Err(RemoveError::Io(io::Error::other(format!(
            "{} is a folder, not a model file",
            shown(&path)
        ))));
    }
    let canonical = fs::canonicalize(&path)?;
    if !canonical.starts_with(canonical_root) || canonical == canonical_root {
        return Err(RemoveError::OutsideStore(shown(&path)));
    }
    let shared = link_count(&canonical) > 1;
    Ok(Some((canonical, meta.len(), shared)))
}

/// Remove empty folders above `file`, up to but never including `root`.
fn prune(root: &Path, file: &Path) {
    let mut dir = file.parent();
    while let Some(d) = dir {
        if d == root || !d.starts_with(root) {
            break;
        }
        // Only succeeds on an empty folder.
        if fs::remove_dir(d).is_err() {
            break;
        }
        dir = d.parent();
    }
}

/// Drop from user-catalog.json (hub::remove_user_entry). Never deletes files.
pub fn forget(store: &ModelStore, id: &str) -> Result<bool, RemoveError> {
    hub::remove_user_entry(store, id).map_err(|e| RemoveError::Io(io::Error::other(e.to_string())))
}

/// rule(entry) applied: Delete -> remove_files; DeleteAndForget -> remove_files + forget;
/// ForgetOnly -> forget. `forget_too` adds a forget for a Hub entry ("Remove from list");
/// it is ignored for built-ins.
pub fn remove(
    store: &ModelStore,
    entry: &CatalogEntry,
    builtin: bool,
    forget_too: bool,
) -> Result<Removal, RemoveError> {
    match rule(entry) {
        RemoveRule::Delete => {
            let mut r = remove_files(store, entry)?;
            if forget_too && !builtin {
                r.forgotten = forget(store, &entry.id)?;
            }
            Ok(r)
        }
        RemoveRule::DeleteAndForget => {
            let mut r = remove_files(store, entry)?;
            r.forgotten = forget(store, &entry.id)?;
            Ok(r)
        }
        RemoveRule::ForgetOnly => {
            let forgotten = forget(store, &entry.id)?;
            Ok(Removal { id: entry.id.clone(), forgotten, kept_in_place: true, ..Removal::default() })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{Catalog, FileSpec, Layout, Slot};
    use crate::local::{ImportMode, import};

    fn write(path: &Path, bytes: &[u8]) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    fn hub_entry(id: &str, files: &[(&str, u64)]) -> CatalogEntry {
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
                    sha256: "0".repeat(64),
                    git_sha1: None,
                    url: None,
                })
                .collect(),
            layout: Layout::LlmGguf { model: files[0].0.into() },
            gated: false,
            mobile: false,
        }
    }

    fn store() -> (tempfile::TempDir, ModelStore) {
        let tmp = tempfile::tempdir().unwrap();
        let store = ModelStore::new(tmp.path().join("models"));
        fs::create_dir_all(store.root()).unwrap();
        (tmp, store)
    }

    #[test]
    fn rules_follow_the_source() {
        let mut e = hub_entry("hf:a/b/m", &[("m.gguf", 1)]);
        assert_eq!(rule(&e), RemoveRule::Delete);
        e.source = Source::Local { path: None };
        assert_eq!(rule(&e), RemoveRule::DeleteAndForget);
        e.source = Source::Local { path: Some("/x/m.gguf".into()) };
        assert_eq!(rule(&e), RemoveRule::ForgetOnly);
        assert_eq!(serde_json::to_value(RemoveRule::DeleteAndForget).unwrap(), "delete_and_forget");
    }

    #[test]
    fn only_listed_files_and_their_parts_go() {
        let (_tmp, store) = store();
        let e = hub_entry(
            "hf:unsloth/Q/Big-Q4",
            &[
                ("Q4/Big-00001-of-00003.gguf", 10),
                ("Q4/Big-00002-of-00003.gguf", 20),
                ("Q4/Big-00003-of-00003.gguf", 30),
            ],
        );
        let dir = store.model_dir(&e);
        write(&dir.join("Q4/Big-00001-of-00003.gguf"), &[1u8; 10]);
        write(&dir.join("Q4/Big-00002-of-00003.gguf"), &[2u8; 20]);
        write(&dir.join("Q4/Big-00003-of-00003.gguf.part"), &[3u8; 7]);
        write(&dir.join("Q4/notes.txt"), b"someone's file");
        let other = hub_entry("hf:unsloth/Q/Other", &[("o.gguf", 3)]);
        write(&store.model_dir(&other).join("o.gguf"), b"abc");
        write(&store.root().join("user-catalog.json"), b"{\"schema_version\":1,\"models\":[]}");

        let r = remove_files(&store, &e).unwrap();
        assert_eq!((r.files_removed, r.freed_bytes), (3, 37));
        assert!(!r.kept_in_place && !r.forgotten);
        assert!(dir.join("Q4/notes.txt").is_file(), "a stranger's file stays");
        assert!(!dir.join("Q4/Big-00001-of-00003.gguf").exists());
        assert!(store.model_dir(&other).join("o.gguf").is_file());
        assert!(store.root().join("user-catalog.json").is_file());

        // Without the stranger, empty folders go, up to the root but not the root.
        fs::remove_file(dir.join("Q4/notes.txt")).unwrap();
        write(&dir.join("Q4/Big-00001-of-00003.gguf"), &[1u8; 10]);
        let r = remove_files(&store, &e).unwrap();
        assert_eq!(r.freed_bytes, 10);
        assert!(!store.root().join("hf_unsloth/Q/Big-Q4").exists());
        assert!(store.root().join("hf_unsloth/Q/Other").is_dir(), "a sibling model's folder stays");
        assert!(store.root().is_dir());
        // Removing again finds nothing, and that's fine.
        assert_eq!(remove_files(&store, &e).unwrap().files_removed, 0);
    }

    #[test]
    fn the_root_itself_is_never_removed() {
        let (_tmp, store) = store();
        let e = hub_entry("solo", &[("m.gguf", 3)]);
        write(&store.model_dir(&e).join("m.gguf"), b"abc");
        remove_files(&store, &e).unwrap();
        assert!(!store.root().join("solo").exists());
        assert!(store.root().is_dir());
    }

    #[test]
    fn unsafe_listed_paths_are_refused() {
        let (tmp, store) = store();
        write(&tmp.path().join("precious.txt"), b"keep");
        for bad in ["../../precious.txt", "/etc/passwd", "a\\..\\..\\x", "C:/x"] {
            let e = hub_entry("hf:a/b/m", &[(bad, 4)]);
            assert!(matches!(remove_files(&store, &e), Err(RemoveError::Unsafe(_))), "{bad}");
        }
        // An id that tries to climb out stays inside (dir_name), so nothing outside goes.
        let e = hub_entry("../..", &[("precious.txt", 4)]);
        remove_files(&store, &e).unwrap();
        assert!(tmp.path().join("precious.txt").is_file());
    }

    fn link_dir(target: &Path, link: &Path) -> bool {
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(target, link).is_ok()
        }
        #[cfg(windows)]
        {
            if std::os::windows::fs::symlink_dir(target, link).is_ok() {
                return true;
            }
            // A junction needs no special rights.
            std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(link)
                .arg(target)
                .output()
                .is_ok_and(|o| o.status.success())
        }
    }

    #[test]
    fn links_pointing_outside_are_refused() {
        let (tmp, store) = store();
        let outside = tmp.path().join("outside");
        write(&outside.join("m.gguf"), b"precious");
        let e = hub_entry("hf:a/b/m", &[("m.gguf", 8)]);
        let dir = store.model_dir(&e);
        fs::create_dir_all(dir.parent().unwrap()).unwrap();
        if !link_dir(&outside, &dir) {
            println!("skipped: this account can't make a directory link or junction");
            return;
        }
        let err = remove_files(&store, &e).unwrap_err();
        assert!(matches!(err, RemoveError::Link(_)), "{err}");
        assert!(err.to_string().contains("it's a link to somewhere else"));
        assert_eq!(fs::read(outside.join("m.gguf")).unwrap(), b"precious");
    }

    #[cfg(unix)]
    #[test]
    fn a_file_link_is_refused() {
        let (tmp, store) = store();
        let outside = tmp.path().join("precious.gguf");
        write(&outside, b"precious");
        let e = hub_entry("hf:a/b/m", &[("m.gguf", 8)]);
        let dir = store.model_dir(&e);
        fs::create_dir_all(&dir).unwrap();
        std::os::unix::fs::symlink(&outside, dir.join("m.gguf")).unwrap();
        assert!(matches!(remove_files(&store, &e), Err(RemoveError::Link(_))));
        assert!(outside.is_file());
    }

    /// Mac and Linux installs from before ADR-0025 keep Hub models under the
    /// raw id (`hf:...`); those are removed the same way.
    #[cfg(unix)]
    #[test]
    fn an_old_style_hub_folder_is_removed_too() {
        let (_tmp, store) = store();
        let e = hub_entry("hf:a/b/m", &[("Q4/m.gguf", 3)]);
        write(&store.root().join("hf:a/b/m/Q4/m.gguf"), b"abc");
        assert_eq!(store.model_dir(&e), store.root().join("hf:a/b/m"));
        let r = remove_files(&store, &e).unwrap();
        assert_eq!((r.files_removed, r.freed_bytes), (1, 3));
        assert!(!store.root().join("hf:a").exists(), "emptied folders pruned");
        assert!(store.root().is_dir());
    }

    #[test]
    fn a_refusal_deletes_nothing() {
        let (_tmp, store) = store();
        let e = hub_entry("hf:a/b/m", &[("good.gguf", 3), ("../bad.gguf", 3)]);
        write(&store.model_dir(&e).join("good.gguf"), b"abc");
        assert!(remove_files(&store, &e).is_err());
        assert!(store.model_dir(&e).join("good.gguf").is_file());
    }

    #[test]
    fn models_used_in_place_are_never_touched() {
        let (tmp, store) = store();
        let file = tmp.path().join("LM Studio").join("m.gguf");
        let mut bytes = b"GGUF\x03\0\0\0".to_vec();
        bytes.extend_from_slice(&[5u8; 64]);
        write(&file, &bytes);
        let entry = import(&store, &file, ImportMode::InPlace, &mut |_: &str, _: u64, _: u64| {}).unwrap();
        let before = fs::metadata(&file).unwrap().modified().unwrap();
        // Even with a `.part` next to it.
        write(&tmp.path().join("LM Studio").join("m.gguf.part"), b"x");

        let r = remove_files(&store, &entry).unwrap();
        assert!(r.kept_in_place);
        assert_eq!((r.files_removed, r.freed_bytes), (0, 0));
        let r = remove(&store, &entry, false, false).unwrap();
        assert!(r.kept_in_place && r.forgotten);
        assert_eq!(fs::read(&file).unwrap(), bytes);
        assert_eq!(fs::metadata(&file).unwrap().modified().unwrap(), before);
        assert!(tmp.path().join("LM Studio").join("m.gguf.part").exists());
        assert!(hub::full_catalog(&store).get(&entry.id).is_none(), "forgotten");
    }

    #[test]
    fn a_local_copy_is_deleted_and_dropped() {
        let (tmp, store) = store();
        let file = tmp.path().join("Downloads").join("m.gguf");
        write(&file, b"GGUF\x03\0\0\0weights");
        let entry = import(&store, &file, ImportMode::Copy, &mut |_: &str, _: u64, _: u64| {}).unwrap();
        // On one drive the copy is a hard link: removing it frees nothing,
        // because the original keeps the bytes, and it says so.
        let linked = link_count(&file) == 2;
        let r = remove(&store, &entry, false, false).unwrap();
        assert_eq!((r.files_removed, r.forgotten), (1, true));
        if linked {
            assert_eq!((r.freed_bytes, r.shared_bytes), (0, 15));
        } else {
            assert_eq!((r.freed_bytes, r.shared_bytes), (15, 0));
        }
        assert!(!store.root().join("local").join("m").exists());
        assert!(hub::full_catalog(&store).get(&entry.id).is_none());
        assert!(file.is_file(), "the original stays");
        assert_eq!(link_count(&file), 1);
    }

    /// The finding behind `shared_bytes`: a hard-linked copy used to report
    /// its full size as freed. A real copy frees its size; a linked one frees 0.
    #[test]
    fn freed_space_counts_only_data_with_no_other_name() {
        let (tmp, store) = store();
        let e = hub_entry("hf:a/b/m", &[("m.gguf", 8), ("n.gguf", 5)]);
        let dir = store.model_dir(&e);
        write(&dir.join("n.gguf"), b"12345");
        let original = tmp.path().join("original.gguf");
        write(&original, b"precious");
        fs::hard_link(&original, dir.join("m.gguf")).unwrap();
        let r = remove_files(&store, &e).unwrap();
        assert_eq!((r.files_removed, r.freed_bytes, r.shared_bytes), (2, 5, 8));
        assert_eq!(fs::read(&original).unwrap(), b"precious", "the other name keeps the data");
    }

    #[test]
    fn a_builtin_row_survives_and_hub_rows_can_be_forgotten() {
        let (_tmp, store) = store();
        let builtin = Catalog::builtin().models.into_iter().find(|m| m.files.len() == 1).unwrap();
        let path = store.file_path(&builtin, &builtin.files[0].path);
        write(&path, b"abc");
        let r = remove(&store, &builtin, true, true).unwrap();
        assert_eq!(r.files_removed, 1);
        assert!(!r.forgotten, "forget is ignored for built-ins");
        assert!(hub::full_catalog(&store).get(&builtin.id).is_some(), "the row stays, with Download");

        let e = hub_entry("hf:a/b/m", &[("m.gguf", 3)]);
        hub::save_user_entry(&store, e.clone()).unwrap();
        write(&store.model_dir(&e).join("m.gguf"), b"abc");
        let r = remove(&store, &e, false, false).unwrap();
        assert!(!r.forgotten && r.files_removed == 1);
        assert!(hub::full_catalog(&store).get(&e.id).is_some(), "Remove keeps a Hub row");
        let r = remove(&store, &e, false, true).unwrap();
        assert!(r.forgotten);
        assert!(hub::full_catalog(&store).get(&e.id).is_none(), "Remove from list drops it");
    }

    #[test]
    fn a_missing_store_is_nothing_to_remove() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ModelStore::new(tmp.path().join("never-made"));
        let e = hub_entry("hf:a/b/m", &[("m.gguf", 3)]);
        assert_eq!(remove_files(&store, &e).unwrap().files_removed, 0);
        assert!(!tmp.path().join("never-made").exists());
    }
}
