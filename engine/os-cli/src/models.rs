use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow, bail};
use clap::Subcommand;
use indicatif::{ProgressBar, ProgressStyle};
use os_models::hub::{self, Access};
use os_models::local::{self, ImportKind, ImportMode};
use os_models::{Catalog, CatalogEntry, LlmEngine, ModelStore, Verify};

const MLX_NEEDS: &str = "MLX models run on a Mac with Apple silicon and macOS 14 or later.";

#[derive(Subcommand)]
pub enum ModelsCommand {
    /// List catalog models (built-in, from Hugging Face, and added from this
    /// computer) and whether they are installed.
    List,
    /// Download a model (resumable; verified by SHA-256 or git SHA-1).
    Pull {
        id: String,
        /// Hugging Face token for gated repos (or set OPENBARK_HF_TOKEN).
        /// Without it, a gated model uses the saved token. The generic
        /// HF_TOKEN is never read.
        #[arg(long, env = "OPENBARK_HF_TOKEN", hide_env_values = true)]
        hf_token: Option<String>,
    },
    /// Download every model OpenBark needs that isn't installed yet (used by
    /// the installer).
    Setup {
        #[arg(long, env = "OPENBARK_HF_TOKEN", hide_env_values = true)]
        hf_token: Option<String>,
    },
    /// Re-check installed files against their SHA-256 (or git SHA-1).
    Verify { id: Option<String> },
    /// Print the model store directory.
    Path,
    /// Remove a model's files (ADR-0025). A built-in or Hugging Face model
    /// keeps its row, so it can be downloaded again; one copied from this
    /// computer is dropped; one used where it is is only forgotten.
    Rm {
        id: String,
        /// Also drop a Hugging Face model from the list.
        #[arg(long)]
        forget: bool,
    },
    /// Add a model from this computer: a .gguf file, or an MLX model folder
    /// (on a Mac with Apple silicon, or with OPENBARK_MLX_DEV=1).
    Import {
        path: PathBuf,
        /// Use it where it is instead of copying it into the model store.
        #[arg(long)]
        in_place: bool,
    },
    /// Add a model from a Hugging Face repo to the catalog: a GGUF file
    /// (part 1 for a split GGUF), or `@mlx` for the whole MLX model folder.
    /// Then download it with `models pull <id>`.
    AddHf {
        repo: String,
        file: String,
        /// Accept a restricted licence (read it on Hugging Face first).
        #[arg(long)]
        accept_license: bool,
        /// Token for a gated repo (or set OPENBARK_HF_TOKEN); otherwise the saved one.
        #[arg(long, env = "OPENBARK_HF_TOKEN", hide_env_values = true)]
        hf_token: Option<String>,
    },
}

pub fn run(cmd: ModelsCommand, store: &ModelStore) -> Result<()> {
    match cmd {
        ModelsCommand::List => {
            let catalog = hub::full_catalog(store);
            println!("{:<44} {:<18} {:>9}  {:<12} INSTALLED", "ID", "SLOT", "SIZE", "LICENSE");
            for m in &catalog.models {
                println!(
                    "{:<44} {:<18} {:>9}  {:<12} {}",
                    m.id,
                    serde_json::to_value(m.slot)
                        .ok()
                        .and_then(|v| v.as_str().map(String::from))
                        .unwrap_or_default(),
                    human_size(m.total_size()),
                    m.license,
                    if store.is_installed(m, Verify::Size) { "yes" } else { "-" }
                );
            }
            Ok(())
        }
        ModelsCommand::Pull { id, hf_token } => {
            let catalog = hub::full_catalog(store);
            let m = entry(&catalog, &id)?;
            if m.llm_engine() == Some(LlmEngine::Mlx) && !os_app::mlx_runs_here() {
                bail!("{MLX_NEEDS}");
            }
            pull(store, m, hf_token.as_deref())
        }
        ModelsCommand::Setup { hf_token } => {
            let catalog = Catalog::builtin();
            for (id, name, installed) in os_engine::models::required_status(store) {
                if installed {
                    eprintln!("{name}: already installed");
                } else {
                    pull(store, entry(&catalog, &id)?, hf_token.as_deref())?;
                }
            }
            Ok(())
        }
        ModelsCommand::Verify { id } => {
            let catalog = hub::full_catalog(store);
            let entries: Vec<&CatalogEntry> = match &id {
                Some(id) => vec![entry(&catalog, id)?],
                None => catalog.models.iter().filter(|m| store.is_installed(m, Verify::Size)).collect(),
            };
            let mut failed = false;
            for m in entries {
                for f in &m.files {
                    let ok = store.file_ok(m, f, Verify::Full);
                    failed |= !ok;
                    println!("{} {}/{}", if ok { "ok  " } else { "FAIL" }, m.id, f.path);
                }
            }
            if failed {
                bail!("some files failed verification; re-run `openbark models pull <id>`");
            }
            Ok(())
        }
        ModelsCommand::Path => {
            println!("{}", store.root().display());
            Ok(())
        }
        ModelsCommand::Rm { id, forget } => {
            let catalog = hub::full_catalog(store);
            let m = entry(&catalog, &id)?;
            if let Some(why) = os_app::protected_model(&id) {
                bail!("{why}");
            }
            let builtin = Catalog::builtin().get(&id).is_some();
            let removed = os_models::remove::remove(store, m, builtin, forget)?;
            if removed.kept_in_place {
                eprintln!("Forgotten. Nothing was deleted.");
            } else {
                eprintln!(
                    "Removed. {} freed ({} files){}.",
                    human_size(removed.freed_bytes),
                    removed.files_removed,
                    if removed.forgotten { ", and dropped from the list" } else { "" }
                );
            }
            println!("{}", serde_json::to_string(&removed)?);
            Ok(())
        }
        ModelsCommand::Import { path, in_place } => import(store, &path, in_place),
        ModelsCommand::AddHf { repo, file, accept_license, hf_token } => {
            // An MLX folder can't write a note here: don't download gigabytes of it.
            if file == hub::MLX_CHOICE && !os_app::mlx_runs_here() {
                bail!("{MLX_NEEDS}");
            }
            let mut details = hub::repo(&repo, None)?;
            if details.gated_kind.is_some() && details.access == Access::NeedsToken {
                let token = hf_token.or_else(|| os_engine::secrets::hf_token().map(|(t, _)| t));
                if let Some(token) = token {
                    details = hub::repo(&repo, Some(&token))?;
                }
            }
            let added = hub::add_to_catalog(store, &details, &file, accept_license)?;
            eprintln!(
                "Added {} ({}, {} files). Download it with: openbark models pull {}",
                added.display_name,
                human_size(added.total_size()),
                added.files.len(),
                added.id
            );
            println!("{}", added.id);
            Ok(())
        }
    }
}

fn import(store: &ModelStore, path: &Path, in_place: bool) -> Result<()> {
    let path = std::path::absolute(path)?;
    let preview = local::inspect(store, &path)?;
    if preview.kind == ImportKind::Mlx && !os_app::mlx_runs_here() {
        bail!("{MLX_NEEDS}");
    }
    let mode = if in_place { ImportMode::InPlace } else { ImportMode::Copy };
    eprintln!(
        "{} ({}, {} files) → {}",
        preview.name,
        human_size(preview.total_bytes),
        preview.files.len(),
        if in_place {
            "used where it is".to_string()
        } else {
            store.root().join("local").display().to_string()
        }
    );
    let bar = bar(preview.total_bytes);
    let mut done_before = 0u64;
    let mut current = String::new();
    let mut file_done = 0u64;
    let added = local::import(store, &path, mode, &mut |file: &str, done: u64, _total: u64| {
        if file != current {
            done_before += file_done;
            current = file.to_string();
            bar.set_message(file.to_string());
        }
        file_done = done;
        bar.set_position((done_before + done).min(preview.total_bytes));
    })?;
    bar.finish_with_message("added");
    println!("{}", added.id);
    Ok(())
}

fn entry<'a>(catalog: &'a Catalog, id: &str) -> Result<&'a CatalogEntry> {
    catalog.get(id).ok_or_else(|| {
        let ids: Vec<&str> = catalog.models.iter().map(|m| m.id.as_str()).collect();
        anyhow!("unknown model '{id}'. Known: {}", ids.join(", "))
    })
}

pub(crate) fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1000.0 && unit < UNITS.len() - 1 {
        size /= 1000.0;
        unit += 1;
    }
    format!("{size:.1} {}", UNITS[unit])
}

pub(crate) fn bar(total: u64) -> ProgressBar {
    let bar = ProgressBar::new(total);
    bar.set_style(
        ProgressStyle::with_template("{msg:30!} {bar:40} {bytes}/{total_bytes} {bytes_per_sec} eta {eta}")
            .expect("valid template"),
    );
    bar
}

fn pull(store: &ModelStore, entry: &CatalogEntry, hf_token: Option<&str>) -> Result<()> {
    eprintln!(
        "{} ({}, {}) → {}",
        entry.display_name,
        entry.license,
        human_size(entry.total_size()),
        store.model_dir(entry).display()
    );
    if let Some(attribution) = &entry.attribution {
        eprintln!("  {attribution}");
    }
    // A gated repo's model: the saved token when none was given (ADR-0025 §8).
    let saved = match hf_token {
        None if entry.gated => os_engine::secrets::hf_token().map(|(t, _)| t),
        _ => None,
    };
    let bar = bar(entry.total_size());
    let mut done_before = 0u64;
    let mut current = String::new();
    let mut file_done = 0u64;
    os_models::download(
        store,
        entry,
        hf_token.or(saved.as_deref()),
        &mut |file: &str, done: u64, _total: u64| {
            if file != current {
                done_before += file_done;
                current = file.to_string();
                bar.set_message(file.to_string());
            }
            file_done = done;
            bar.set_position(done_before + done);
        },
    )?;
    bar.finish_with_message("verified");
    Ok(())
}
