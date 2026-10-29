//! Choices made from this computer's hardware (0.2.3, ADR-0030): which note
//! model a new install starts with, whether the note model runs on the
//! graphics card, and how many processor threads it uses. Each is a pure
//! function of what was measured, so it can be tested without the hardware.

use os_llm::ComputeDevice;
use os_models::hub::{self, Fit};

/// The note model a computer starts with when it fits.
pub const E4B: &str = "gemma-4-e4b-it-q4_0";
/// The smaller one, for computers where E4B won't fit.
pub const E2B: &str = "gemma-4-e2b-it-q4_0";

/// Override for the number of layers on the graphics card: a number, or
/// `all`. Set it to `all` to draft on integrated graphics again.
pub const GPU_LAYERS_ENV: &str = "OPENBARK_GPU_LAYERS";
/// Override for the note model's processor threads.
pub const THREADS_ENV: &str = "OPENBARK_LLM_THREADS";

/// The default note model for a computer with this much memory: Gemma 4
/// E4B unless OpenBark's own fit check says it won't fit, then Gemma 4 E2B,
/// the smallest built-in note model. "Tight" still gets E4B: most 16 GB
/// laptops report a little under 16 GB and land there, and E4B runs on them.
pub fn default_note_model(e4b_bytes: u64, gpu_bytes: Option<u64>, ram_bytes: u64) -> &'static str {
    match hub::fit(e4b_bytes, gpu_bytes, ram_bytes) {
        Fit::Fits | Fit::Tight => E4B,
        Fit::WontFit => E2B,
    }
}

/// The graphics device the hardware line names and the fit check counts: a
/// dedicated one when there is one (llama.cpp itself uses integrated
/// graphics only when there is no dedicated GPU), else the first other.
pub fn main_gpu(devices: &[ComputeDevice]) -> Option<&ComputeDevice> {
    let gpus = || devices.iter().filter(|d| d.backend != "CPU");
    gpus().find(|d| !d.integrated).or_else(|| gpus().next())
}

/// Layers to put on the graphics card, or `None` for llama.cpp's default
/// (all of them, on the device it picks).
///
/// On Windows and Linux the release builds use Vulkan, which also drives
/// integrated graphics. Those share the processor's memory and were never
/// tested for drafting (ADR-0019), and a clinic's draft stalled on such a
/// laptop. So with only integrated graphics under Vulkan the note model
/// runs on the processor (0 layers). Apple's Metal (unified memory, but the
/// fast path) and dedicated NVIDIA and AMD cards are unchanged. `env` is
/// [`GPU_LAYERS_ENV`]: a number of layers, or `all`.
pub fn gpu_layers(devices: &[ComputeDevice], env: Option<&str>) -> Option<u32> {
    match env.map(str::trim) {
        Some("all") => return None,
        Some(n) if !n.is_empty() => {
            if let Ok(layers) = n.parse::<u32>() {
                return Some(layers);
            }
        }
        _ => {}
    }
    match main_gpu(devices) {
        Some(d) if d.integrated && d.backend.eq_ignore_ascii_case("vulkan") => Some(0),
        _ => None,
    }
}

/// The note model's processor threads from the core count, or `None` to
/// leave llama.cpp's default (4, whatever the processor).
///
/// llama.cpp's threads spin while they wait, so more threads than cores
/// slows everything down, and a thread per logical core starves the app
/// itself on a two-core laptop. With hyper-threading (more logical than
/// physical cores) one thread per physical core leaves the siblings free;
/// without it, one core is left for the app and the system. `env` is
/// [`THREADS_ENV`].
pub fn llm_threads(physical: Option<usize>, logical: usize, env: Option<&str>) -> Option<i32> {
    if let Some(n) = env.and_then(|v| v.trim().parse::<i32>().ok()).filter(|n| *n > 0) {
        return Some(n);
    }
    let physical = physical.filter(|p| *p > 0)?;
    let threads = if logical > physical { physical } else { physical.saturating_sub(1).max(1) };
    Some(threads.min(32) as i32)
}

/// [`llm_threads`] for this computer.
pub fn threads_here() -> Option<i32> {
    let logical = std::thread::available_parallelism().map_or(0, |n| n.get());
    llm_threads(sysinfo::System::physical_core_count(), logical, std::env::var(THREADS_ENV).ok().as_deref())
}

/// [`gpu_layers`] for this computer.
pub fn gpu_layers_here() -> Option<u32> {
    gpu_layers(&os_llm::devices(), std::env::var(GPU_LAYERS_ENV).ok().as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;

    const GB: u64 = 1_000_000_000;
    const E4B_BYTES: u64 = 5_154_941_280;

    #[test]
    fn e4b_unless_it_wont_fit() {
        // RAM only (no dedicated graphics), as most weak laptops report it.
        assert_eq!(default_note_model(E4B_BYTES, None, 8 * GB), E2B);
        assert_eq!(default_note_model(E4B_BYTES, None, 12 * GB), E2B);
        // A "16 GB" laptop reports a little less and is Tight: still E4B.
        assert_eq!(default_note_model(E4B_BYTES, None, 15_500_000_000), E4B);
        assert_eq!(default_note_model(E4B_BYTES, None, 16 * GB), E4B);
        assert_eq!(default_note_model(E4B_BYTES, None, 32 * GB), E4B);
        // A dedicated card's memory decides when there is one.
        assert_eq!(default_note_model(E4B_BYTES, Some(16 * GB), 8 * GB), E4B);
        assert_eq!(default_note_model(E4B_BYTES, Some(4 * GB), 32 * GB), E2B);
    }

    fn device(backend: &str, integrated: bool) -> ComputeDevice {
        ComputeDevice {
            name: format!("{backend}0"),
            description: "graphics".into(),
            backend: backend.into(),
            memory_total_bytes: 8 * GB,
            integrated,
        }
    }

    #[test]
    fn integrated_graphics_under_vulkan_draft_on_the_processor() {
        let cpu = device("CPU", false);
        // Integrated graphics only, under Vulkan: the processor.
        assert_eq!(gpu_layers(&[device("Vulkan", true), cpu.clone()], None), Some(0));
        // A dedicated card as well: unchanged (llama.cpp skips the integrated one).
        assert_eq!(gpu_layers(&[device("Vulkan", true), device("Vulkan", false), cpu.clone()], None), None);
        assert_eq!(gpu_layers(&[device("CUDA", false), cpu.clone()], None), None);
        // Apple's Metal is unchanged, whatever it reports.
        assert_eq!(gpu_layers(&[device("MTL", true), cpu.clone()], None), None);
        assert_eq!(gpu_layers(&[device("Metal", true), cpu.clone()], None), None);
        // No graphics at all: nothing to decide.
        assert_eq!(gpu_layers(std::slice::from_ref(&cpu), None), None);
        // The escape hatch.
        assert_eq!(gpu_layers(&[device("Vulkan", true), cpu.clone()], Some("all")), None);
        assert_eq!(gpu_layers(&[device("Vulkan", true), cpu.clone()], Some("12")), Some(12));
        assert_eq!(gpu_layers(&[device("CUDA", false), cpu.clone()], Some("0")), Some(0));
        assert_eq!(gpu_layers(&[device("Vulkan", true), cpu], Some("lots")), Some(0), "unreadable: ignored");
    }

    #[test]
    fn the_main_gpu_is_the_dedicated_one() {
        let devices = [device("Vulkan", true), device("Vulkan", false), device("CPU", false)];
        assert!(!main_gpu(&devices).unwrap().integrated);
        let only = [device("Vulkan", true), device("CPU", false)];
        assert!(main_gpu(&only).unwrap().integrated);
        assert!(main_gpu(&[device("CPU", false)]).is_none());
    }

    #[test]
    fn threads_follow_the_physical_cores() {
        // Two cores, four threads (a weak laptop): two.
        assert_eq!(llm_threads(Some(2), 4, None), Some(2));
        // Two cores, no hyper-threading: one, leaving one for the app.
        assert_eq!(llm_threads(Some(2), 2, None), Some(1));
        assert_eq!(llm_threads(Some(1), 1, None), Some(1));
        assert_eq!(llm_threads(Some(8), 16, None), Some(8));
        assert_eq!(llm_threads(Some(6), 6, None), Some(5));
        assert_eq!(llm_threads(Some(64), 128, None), Some(32));
        // Unknown core count: llama.cpp decides.
        assert_eq!(llm_threads(None, 8, None), None);
        // The override wins; nonsense is ignored.
        assert_eq!(llm_threads(Some(8), 16, Some("2")), Some(2));
        assert_eq!(llm_threads(Some(8), 16, Some("0")), Some(8));
        assert_eq!(llm_threads(Some(8), 16, Some("x")), Some(8));
    }
}
