use anyhow::{Result, anyhow, bail};
use clap::Subcommand;
use indicatif::{ProgressBar, ProgressStyle};
use os_models::{Catalog, CatalogEntry, ModelStore, Verify};

#[derive(Subcommand)]
pub enum ModelsCommand {
    /// List catalog models and whether they are installed.
    List,
    /// Download a model (resumable; verified by SHA-256).
    Pull {
        id: String,
        /// Hugging Face token for gated repos (or set HF_TOKEN).
        #[arg(long, env = "HF_TOKEN", hide_env_values = true)]
        hf_token: Option<String>,
    },
    /// Re-check installed files against their SHA-256.
    Verify { id: Option<String> },
    /// Print the model store directory.
    Path,
}

pub fn run(cmd: ModelsCommand, store: &ModelStore) -> Result<()> {
    let catalog = Catalog::builtin();
    match cmd {
        ModelsCommand::List => {
            println!("{:<28} {:<18} {:>9}  {:<10} INSTALLED", "ID", "SLOT", "SIZE", "LICENSE");
            for m in &catalog.models {
                println!(
                    "{:<28} {:<18} {:>9}  {:<10} {}",
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
            let entry = entry(&catalog, &id)?;
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
            let bar = ProgressBar::new(entry.total_size());
            bar.set_style(
                ProgressStyle::with_template(
                    "{msg:30!} {bar:40} {bytes}/{total_bytes} {bytes_per_sec} eta {eta}",
                )
                .expect("valid template"),
            );
            let mut done_before = 0u64;
            let mut current = String::new();
            let mut file_done = 0u64;
            os_models::download(
                store,
                entry,
                hf_token.as_deref(),
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
        ModelsCommand::Verify { id } => {
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
    }
}

fn entry<'a>(catalog: &'a Catalog, id: &str) -> Result<&'a CatalogEntry> {
    catalog.get(id).ok_or_else(|| {
        let ids: Vec<&str> = catalog.models.iter().map(|m| m.id.as_str()).collect();
        anyhow!("unknown model '{id}'. Known: {}", ids.join(", "))
    })
}

fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1000.0 && unit < UNITS.len() - 1 {
        size /= 1000.0;
        unit += 1;
    }
    format!("{size:.1} {}", UNITS[unit])
}
