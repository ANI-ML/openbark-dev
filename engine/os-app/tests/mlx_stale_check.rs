//! The MLX engine's check after an OpenBark or macOS update (ADR-0025 §6),
//! end to end on Linux in test mode: a pack whose last check was recorded
//! for another app version is checked again by itself when a model loads,
//! instead of the draft saying "Download the MLX engine".
//!
//! The "pack" is a stand-in: its python3 is a script that runs the fake
//! worker from os-llm's tests, which speaks the worker protocol. Everything
//! is in a temporary folder. This file is its own test process, so the
//! environment it sets (test mode, the engines folder) touches no other test.

#![cfg(all(target_os = "linux", target_arch = "x86_64", debug_assertions))]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use os_models::{CatalogEntry, ModelStore, hub, mlx_pack};

const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

fn fake_worker() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../os-llm/tests/mlx_fake_worker.py").canonicalize().unwrap()
}

/// A stand-in pack for this lock: a manifest, and a python3 that runs the
/// fake worker whatever script it is given.
fn fake_pack(engines: &Path) -> PathBuf {
    let id = mlx_pack::lock_id().expect("test mode has a lock");
    let pack = engines.join("mlx").join(&id);
    std::fs::create_dir_all(pack.join("python/bin")).unwrap();
    std::fs::write(
        pack.join("manifest.json"),
        serde_json::json!({"lock_id": id, "installed_at": "2026-10-19T00:00:00Z", "files": [], "unpacked_bytes": 0})
            .to_string(),
    )
    .unwrap();
    let python = pack.join("python/bin/python3");
    std::fs::write(&python, format!("#!/bin/sh\nexec python3 -I -B -u '{}'\n", fake_worker().display()))
        .unwrap();
    std::fs::set_permissions(&python, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(mlx_pack::python().as_deref(), Some(python.as_path()));
    pack
}

/// A downloaded MLX model from the Hub in the store.
fn mlx_model(store: &ModelStore) -> CatalogEntry {
    let entry: CatalogEntry = serde_json::from_value(serde_json::json!({
        "id": "hf:someone/Tiny-4bit/@mlx",
        "display_name": "someone/Tiny-4bit (MLX)",
        "slot": "llm",
        "license": "apache-2.0",
        "source": { "kind": "huggingface", "repo": "someone/Tiny-4bit", "revision": "a".repeat(40) },
        "files": [
            { "path": "config.json", "size": 2, "git_sha1": "1".repeat(40) },
            { "path": "model.safetensors", "size": 4, "sha256": "2".repeat(64) },
            { "path": "tokenizer.json", "size": 2, "git_sha1": "3".repeat(40) },
        ],
        "layout": { "type": "llm_mlx", "config": "config.json" },
    }))
    .unwrap();
    hub::save_user_entry(store, entry.clone()).unwrap();
    for f in &entry.files {
        let path = store.file_path(&entry, &f.path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, vec![b'{'; f.size as usize]).unwrap();
    }
    entry
}

#[test]
fn an_update_reruns_the_pack_check_by_itself_when_a_model_loads() {
    if std::process::Command::new("python3").arg("--version").output().is_err() {
        println!("skipped: no python3");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let engines = tmp.path().join("engines");
    // SAFETY: this test file is its own process, and this is its only test.
    unsafe {
        std::env::set_var("OPENBARK_MLX_DEV", "1");
        std::env::set_var("OPENBARK_ENGINES_DIR", &engines);
        std::env::set_var("OPENBARK_LOCATIONS_FILE", tmp.path().join("locations.json"));
    }
    assert!(os_app::mlx_runs_here(), "test mode");
    fake_pack(&engines);
    let store = ModelStore::new(tmp.path().join("models"));
    let model = mlx_model(&store);
    let now = mlx_pack::check_key(APP_VERSION, "pack").unwrap();

    // The pack passed its check under the version before the update.
    let before = mlx_pack::check_key("0.0.1-before-the-update", "pack").unwrap();
    mlx_pack::record_check(&before, true, None).unwrap();
    assert!(mlx_pack::check_result(&now).is_none(), "the old result doesn't apply");
    assert!(!os_app::mlx_ready());

    // Drafting loads the model: the check runs again, passes, and it loads.
    let backend = os_app::open_llm(&store, &model.id, None).expect("loads after re-checking");
    assert_eq!(backend.capabilities().backend, "mlx");
    assert!(backend.capabilities().json_schema_constrained);
    assert!(mlx_pack::check_result(&now).is_some_and(|r| r.ok), "the new result is kept");
    assert!(os_app::mlx_ready());
    drop(backend);

    // A check that failed on this computer says so, and points to Try again.
    mlx_pack::record_check(&now, false, Some("Metal isn't available on this Mac")).unwrap();
    let refusal = os_app::open_llm(&store, &model.id, None).err().unwrap().0;
    assert_eq!(
        refusal,
        "The MLX engine didn't pass its check on this computer. Choose Try again on the MLX engine card in Models."
    );

    // Only a pack that isn't there asks for a download.
    let pack = mlx_pack::pack_dir().unwrap();
    std::fs::remove_file(pack.join("manifest.json")).unwrap();
    let refusal = os_app::open_llm(&store, &model.id, None).err().unwrap().0;
    assert_eq!(refusal, "Download the MLX engine on the Models screen first.");
}
