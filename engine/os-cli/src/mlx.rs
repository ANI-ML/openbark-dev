//! `openbark mlx`: the MLX engine pack (ADR-0025 §4), for support, CI and
//! headless installs. The Models screen does the same through `os-app`.

use anyhow::{Result, anyhow, bail};
use clap::Subcommand;
use os_models::ModelStore;
use os_models::mlx_pack;
use serde_json::json;

use crate::models::{bar, human_size};

#[derive(Subcommand)]
pub enum MlxCommand {
    /// Download and set up the MLX engine pack, then run its check. Only on
    /// a Mac with Apple silicon and macOS 14, or with OPENBARK_MLX_DEV=1 on
    /// Linux x86_64 (test mode, on the CPU).
    Install,
    /// Run the pack's check again: imports, the socket guard, Metal (or the
    /// CPU with OPENBARK_MLX_DEVICE=cpu), a matrix multiply and the mask.
    Check,
    /// Remove the MLX engine pack. MLX models stay on disk.
    Remove,
    /// Whether MLX can run here, and the pack's state, as JSON.
    Status,
}

pub fn run(cmd: MlxCommand, store: &ModelStore) -> Result<()> {
    match cmd {
        MlxCommand::Install => {
            let total = mlx_pack::entry().map(|e| e.total_size()).unwrap_or(0);
            let progress = bar(total);
            progress.set_message("MLX engine pack");
            let check = os_app::install_mlx_pack(store, &mut |done, _total| progress.set_position(done))
                .map_err(|e| anyhow!(e.0))?;
            progress.finish_with_message("set up");
            println!("{}", serde_json::to_string_pretty(&check)?);
            if !check.ok {
                bail!("{}", check.message.unwrap_or_else(|| "the MLX engine didn't pass its check".into()));
            }
            Ok(())
        }
        MlxCommand::Check => {
            let check = os_app::run_mlx_pack_check().map_err(|e| anyhow!(e.0))?;
            println!("{}", serde_json::to_string_pretty(&check)?);
            if !check.ok {
                bail!("{}", check.message.unwrap_or_else(|| "the MLX engine didn't pass its check".into()));
            }
            Ok(())
        }
        MlxCommand::Remove => {
            let freed = mlx_pack::remove(store)?;
            eprintln!("Removed. {} freed.", human_size(freed));
            println!("{}", json!({ "id": mlx_pack::PACK_ID, "freed_bytes": freed }));
            Ok(())
        }
        MlxCommand::Status => {
            let check = mlx_pack::check_key(env!("CARGO_PKG_VERSION"), "pack")
                .and_then(|k| mlx_pack::check_result(&k));
            let status = json!({
                "support": mlx_pack::support(),
                "macos_version": mlx_pack::macos_version(),
                "chip": mlx_pack::chip(),
                "lock_id": mlx_pack::lock_id(),
                "engines_dir": mlx_pack::engines_dir(),
                "pack_dir": mlx_pack::pack_dir(),
                "installed": mlx_pack::installed(),
                "ready": os_app::mlx_ready(),
                "disk_bytes": mlx_pack::disk_bytes(),
                "check": check,
                "device": if matches!(os_app::mlx_device(), os_llm::MlxDevice::Cpu) { "cpu" } else { "auto" },
                "sandbox": os_app::mlx_sandbox(),
            });
            println!("{}", serde_json::to_string_pretty(&status)?);
            Ok(())
        }
    }
}
