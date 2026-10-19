//! ADR-0025 through the crate's public API, as the app uses it: add a model
//! from this computer, see its space, remove it. Temporary folders only; no
//! network.

use std::fs;
use std::path::Path;

use os_models::hub::{self, MLX_CHOICE};
use os_models::local::{self, ImportKind, ImportMode};
use os_models::remove::{self, RemoveRule};
use os_models::{LlmEngine, ModelStore, Source, Verify, dir_name, hash_file, safe_rel_path};

fn quiet() -> impl FnMut(&str, u64, u64) {
    |_: &str, _: u64, _: u64| {}
}

fn tiny_gguf(path: &Path) -> Vec<u8> {
    let mut bytes = b"GGUF".to_vec();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(&[42u8; 256]);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, &bytes).unwrap();
    bytes
}

#[test]
fn add_from_this_computer_then_remove() {
    let tmp = tempfile::tempdir().unwrap();
    let store = ModelStore::new(tmp.path().join("models"));
    let file = tmp.path().join("Downloads").join("Tiny-Q4_K_M.gguf");
    let bytes = tiny_gguf(&file);

    let preview = local::inspect(&store, &file).unwrap();
    assert_eq!(preview.kind, ImportKind::Gguf);
    assert_eq!(preview.id, "local/Tiny-Q4_K_M");
    assert_eq!(serde_json::to_value(ImportMode::InPlace).unwrap(), "in_place");

    let entry = local::import(&store, &file, ImportMode::Copy, &mut quiet()).unwrap();
    assert_eq!(entry.llm_engine(), Some(LlmEngine::Llama));
    assert!(entry.is_local() && entry.in_place().is_none());
    assert!(store.is_installed(&entry, Verify::Full));
    // On one drive the copy is a hard link: on disk, but shared with the
    // original, so removing it frees nothing. Either way the bytes add up.
    let (own, shared) = (store.disk_usage(&entry), store.shared_bytes(&entry));
    assert_eq!(own + shared, bytes.len() as u64);
    assert_eq!(remove::rule(&entry), RemoveRule::DeleteAndForget);
    assert!(hub::full_catalog(&store).get(&entry.id).is_some());

    let removal = remove::remove(&store, &entry, false, false).unwrap();
    assert_eq!((removal.freed_bytes, removal.shared_bytes), (own, shared));
    assert!(removal.forgotten);
    assert!(hub::full_catalog(&store).get(&entry.id).is_none());
    assert_eq!(fs::read(&file).unwrap(), bytes, "the original file is untouched");
    let json = serde_json::to_value(&removal).unwrap();
    assert_eq!(json["kept_in_place"], false);
    assert_eq!(json["shared_bytes"], shared);
}

#[test]
fn used_in_place_is_only_forgotten() {
    let tmp = tempfile::tempdir().unwrap();
    let store = ModelStore::new(tmp.path().join("models"));
    let file = tmp.path().join("LM Studio").join("Tiny.gguf");
    let bytes = tiny_gguf(&file);
    let entry = local::import(&store, &file, ImportMode::InPlace, &mut quiet()).unwrap();
    assert!(matches!(&entry.source, Source::Local { path: Some(_) }));
    assert_eq!(store.disk_usage(&entry), 0);
    assert_eq!(remove::rule(&entry), RemoveRule::ForgetOnly);
    let removal = remove::remove(&store, &entry, false, true).unwrap();
    assert!(removal.kept_in_place && removal.forgotten && removal.files_removed == 0);
    assert_eq!(fs::read(&file).unwrap(), bytes);
}

#[test]
fn public_helpers() {
    assert_eq!(MLX_CHOICE, "@mlx");
    assert_eq!(dir_name("hf:a/b/@mlx"), "hf_a/b/@mlx");
    assert!(safe_rel_path("../x").is_err());
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("abc");
    fs::write(&p, b"abc").unwrap();
    assert_eq!(hash_file(&p).unwrap().git_sha1, "f2ba8f84ab5c1bce84a7b441cb1959cfc7093b7f");
}
