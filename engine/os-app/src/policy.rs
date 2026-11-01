//! Choices made from this computer's hardware (0.2.3, ADR-0030; ADR-0034):
//! which note model a new install starts with and why, whether the note
//! model runs on the graphics card, how many processor threads it uses, and
//! how long a long visit's note takes at a measured speed. Each is a pure
//! function of what was measured, so it can be tested without the hardware.

use os_llm::ComputeDevice;
use os_models::hub::{self, Fit};
use serde::Serialize;

/// The note model a computer starts with when it fits.
pub const E4B: &str = "gemma-4-e4b-it-q4_0";
/// The smaller one, for computers where E4B won't fit.
pub const E2B: &str = "gemma-4-e2b-it-q4_0";

/// Override for the number of layers on the graphics card: a number, or
/// `all`. Set it to `all` to draft on integrated graphics again.
pub const GPU_LAYERS_ENV: &str = "OPENBARK_GPU_LAYERS";
/// Override for the note model's processor threads.
pub const THREADS_ENV: &str = "OPENBARK_LLM_THREADS";

/// Where the note model writes on this computer, which decides how fast it
/// is (ADR-0034).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DraftsOn {
    /// A dedicated graphics card (NVIDIA or AMD, through CUDA or Vulkan).
    GraphicsCard,
    /// Apple silicon's graphics, through Metal.
    AppleSilicon,
    /// The processor: no graphics card, or integrated graphics the note
    /// model doesn't use ([`gpu_layers`] 0).
    Processor,
}

/// Where the note model writes, from the devices llama.cpp sees and the
/// layers [`gpu_layers`] chose. Integrated graphics turned on again with
/// [`GPU_LAYERS_ENV`] count as the processor: their speed was never measured.
pub fn drafts_on(devices: &[ComputeDevice], layers: Option<u32>) -> DraftsOn {
    if layers == Some(0) {
        return DraftsOn::Processor;
    }
    match main_gpu(devices) {
        Some(d) if is_metal(&d.backend) => DraftsOn::AppleSilicon,
        Some(d) if !d.integrated => DraftsOn::GraphicsCard,
        _ => DraftsOn::Processor,
    }
}

fn is_metal(backend: &str) -> bool {
    backend.eq_ignore_ascii_case("metal") || backend.eq_ignore_ascii_case("mtl")
}

/// On the processor alone, the larger note model is recommended only with
/// at least this many physical cores (ADR-0034). Below it, Gemma 4 E2B is
/// recommended even where E4B fits: the drafting CI job measured E2B on two
/// processor threads at about 9 minutes for a long visit's note, and E4B is
/// about 1.6 to 1.75 times slower on the same processor
/// (docs/eval/phone-models.md), so about 15 minutes there. Eight is a
/// judgement, not a measurement: no CI runner has eight cores. The speed
/// check measures the computer itself.
pub const MIN_CORES_FOR_E4B_ON_PROCESSOR: usize = 8;

/// Why a note model is recommended for this computer (ADR-0034).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Why {
    /// Gemma 4 E4B won't fit this computer's memory.
    Memory,
    /// Notes are written on a dedicated graphics card.
    GraphicsCard,
    /// Notes are written on Apple silicon's graphics.
    AppleSilicon,
    /// The processor alone, with fewer than [`MIN_CORES_FOR_E4B_ON_PROCESSOR`]
    /// physical cores (or a count the system didn't report).
    FewCores,
    /// The processor alone, with at least [`MIN_CORES_FOR_E4B_ON_PROCESSOR`] physical cores.
    ManyCores,
}

/// The note model recommended for a computer, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Recommendation {
    pub model: &'static str,
    pub why: Why,
    pub drafts_on: DraftsOn,
    /// Physical processor cores, when the system reports them.
    pub cores: Option<usize>,
    /// How Gemma 4 E4B fits this computer's memory.
    pub e4b_fit: Fit,
}

/// The note model for a computer, speed-aware (ADR-0034; ADR-0030 chose by
/// memory only):
///
/// 1. Gemma 4 E2B when Gemma 4 E4B won't fit ([`hub::fit`]). "Tight" fits:
///    most 16 GB laptops report a little under 16 GB and E4B runs on them.
/// 2. Gemma 4 E4B when notes are written on a dedicated graphics card or on
///    Apple silicon.
/// 3. On the processor alone, E4B with at least
///    [`MIN_CORES_FOR_E4B_ON_PROCESSOR`] physical cores, else E2B.
pub fn recommend_note_model(
    e4b_bytes: u64,
    gpu_bytes: Option<u64>,
    ram_bytes: u64,
    on: DraftsOn,
    cores: Option<usize>,
) -> Recommendation {
    let e4b_fit = hub::fit(e4b_bytes, gpu_bytes, ram_bytes);
    let (model, why) = match (e4b_fit, on) {
        (Fit::WontFit, _) => (E2B, Why::Memory),
        (_, DraftsOn::GraphicsCard) => (E4B, Why::GraphicsCard),
        (_, DraftsOn::AppleSilicon) => (E4B, Why::AppleSilicon),
        (_, DraftsOn::Processor) => match cores {
            Some(n) if n >= MIN_CORES_FOR_E4B_ON_PROCESSOR => (E4B, Why::ManyCores),
            _ => (E2B, Why::FewCores),
        },
    };
    Recommendation { model, why, drafts_on: on, cores, e4b_fit }
}

/// The long visit the drafting CI job drafts (`.github/workflows/drafting.yml`,
/// run 40869023832, Gemma 4 E2B on two processor threads): 16.1 minutes of
/// continuous synthetic speech, eight consults back to back. Its SOAP note
/// read a prompt of 7,024 tokens and wrote 3,503 tokens, and took 537 s.
pub const LONG_VISIT_PROMPT_TOKENS: f64 = 7_024.0;
/// See [`LONG_VISIT_PROMPT_TOKENS`].
pub const LONG_VISIT_NOTE_TOKENS: f64 = 3_503.0;

/// How much slower a long draft runs than the speed check's short sample,
/// with the whole long prompt in its context. The drafting CI job compares
/// the two on the same runner (ADR-0034).
pub const LONG_CONTEXT_SLOWDOWN: f64 = 1.0;

/// How long a long visit's note takes at these speeds, in seconds: the
/// prompt read at `read_per_second` and the note written at
/// `write_per_second` tokens a second, times `slowdown`
/// ([`LONG_CONTEXT_SLOWDOWN`]). `None` without two positive speeds.
pub fn long_visit_note_secs(read_per_second: f64, write_per_second: f64, slowdown: f64) -> Option<u64> {
    let ok = |v: f64| v.is_finite() && v > 0.0;
    if !ok(read_per_second) || !ok(write_per_second) || !ok(slowdown) {
        return None;
    }
    let secs =
        (LONG_VISIT_PROMPT_TOKENS / read_per_second + LONG_VISIT_NOTE_TOKENS / write_per_second) * slowdown;
    Some(secs.round() as u64)
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

    fn pick(gpu: Option<u64>, ram: u64, on: DraftsOn, cores: Option<usize>) -> (&'static str, Why) {
        let r = recommend_note_model(E4B_BYTES, gpu, ram, on, cores);
        (r.model, r.why)
    }

    #[test]
    fn eight_gb_on_the_processor_gets_e2b_for_memory() {
        assert_eq!(pick(None, 8 * GB, DraftsOn::Processor, Some(8)), (E2B, Why::Memory));
        assert_eq!(pick(None, 12 * GB, DraftsOn::Processor, Some(16)), (E2B, Why::Memory));
    }

    #[test]
    fn sixteen_gb_on_the_processor_follows_the_cores() {
        // E4B fits (a "16 GB" laptop reports a little less and is Tight), but
        // four cores write notes slowly: the smaller model.
        assert_eq!(pick(None, 16 * GB, DraftsOn::Processor, Some(4)), (E2B, Why::FewCores));
        assert_eq!(pick(None, 15_500_000_000, DraftsOn::Processor, Some(4)), (E2B, Why::FewCores));
        // Eight cores: the larger model.
        assert_eq!(pick(None, 16 * GB, DraftsOn::Processor, Some(8)), (E4B, Why::ManyCores));
        assert_eq!(pick(None, 15_500_000_000, DraftsOn::Processor, Some(8)), (E4B, Why::ManyCores));
        // Seven, or a count the system didn't give: the smaller one.
        assert_eq!(pick(None, 16 * GB, DraftsOn::Processor, Some(7)), (E2B, Why::FewCores));
        assert_eq!(pick(None, 16 * GB, DraftsOn::Processor, None), (E2B, Why::FewCores));
    }

    #[test]
    fn a_dedicated_eight_gb_card_gets_e4b() {
        let r = recommend_note_model(E4B_BYTES, Some(8 * GB), 16 * GB, DraftsOn::GraphicsCard, Some(4));
        assert_eq!((r.model, r.why), (E4B, Why::GraphicsCard));
        assert_eq!(r.e4b_fit, Fit::Tight);
        // The card's memory decides: a 4 GB card can't hold E4B whatever the memory.
        assert_eq!(pick(Some(4 * GB), 32 * GB, DraftsOn::GraphicsCard, Some(8)), (E2B, Why::Memory));
    }

    #[test]
    fn apple_silicon_with_sixteen_gb_gets_e4b() {
        // Metal reports about two thirds of the memory as its working set.
        let (ram, metal) = (17_179_869_184, Some(11_453_246_122));
        assert_eq!(pick(metal, ram, DraftsOn::AppleSilicon, Some(8)), (E4B, Why::AppleSilicon));
        // An 8 GB Mac: E4B won't fit.
        let (ram, metal) = (8_589_934_592, Some(5_726_623_061));
        assert_eq!(pick(metal, ram, DraftsOn::AppleSilicon, Some(8)), (E2B, Why::Memory));
    }

    #[test]
    fn thirty_two_gb_on_the_processor_is_decided_by_the_cores_not_the_memory() {
        assert_eq!(pick(None, 32 * GB, DraftsOn::Processor, Some(6)), (E2B, Why::FewCores));
        assert_eq!(pick(None, 32 * GB, DraftsOn::Processor, Some(16)), (E4B, Why::ManyCores));
    }

    #[test]
    fn where_notes_are_written() {
        let cpu = device("CPU", false);
        assert_eq!(drafts_on(std::slice::from_ref(&cpu), None), DraftsOn::Processor);
        assert_eq!(drafts_on(&[device("Vulkan", true), cpu.clone()], Some(0)), DraftsOn::Processor);
        // Integrated graphics forced on again: never measured, so the processor.
        assert_eq!(drafts_on(&[device("Vulkan", true), cpu.clone()], None), DraftsOn::Processor);
        assert_eq!(drafts_on(&[device("CUDA", false), cpu.clone()], None), DraftsOn::GraphicsCard);
        let both = [device("Vulkan", true), device("Vulkan", false), cpu.clone()];
        assert_eq!(drafts_on(&both, None), DraftsOn::GraphicsCard);
        assert_eq!(drafts_on(&[device("MTL", true), cpu.clone()], None), DraftsOn::AppleSilicon);
        assert_eq!(drafts_on(&[device("Metal", false), cpu.clone()], None), DraftsOn::AppleSilicon);
        // The override to the processor wins over a card.
        assert_eq!(drafts_on(&[device("CUDA", false), cpu], Some(0)), DraftsOn::Processor);
    }

    #[test]
    fn the_long_visit_estimate_reproduces_the_ci_draft() {
        // drafting.yml run 40869023832: 3,503 tokens written at 10.8 tokens/s,
        // 537 s in all, so the prompt and the model load took about 213 s.
        let read = LONG_VISIT_PROMPT_TOKENS / (537.0 - LONG_VISIT_NOTE_TOKENS / 10.8);
        assert_eq!(long_visit_note_secs(read, 10.8, 1.0), Some(537));
        // Three times as fast both ways: a third of the time.
        assert_eq!(long_visit_note_secs(read * 3.0, 32.4, 1.0), Some(179));
        // The slowdown factor scales it.
        assert_eq!(long_visit_note_secs(read, 10.8, 2.0), Some(1074));
        // A graphics card: a thousand tokens a second read, fifty written.
        assert_eq!(long_visit_note_secs(1_000.0, 50.0, 1.0), Some(77));
    }

    #[test]
    fn no_estimate_without_two_speeds() {
        assert_eq!(long_visit_note_secs(0.0, 10.0, 1.0), None);
        assert_eq!(long_visit_note_secs(30.0, 0.0, 1.0), None);
        assert_eq!(long_visit_note_secs(f64::NAN, 10.0, 1.0), None);
        assert_eq!(long_visit_note_secs(30.0, f64::INFINITY, 1.0), None);
        assert_eq!(long_visit_note_secs(-1.0, 10.0, 1.0), None);
        assert_eq!(long_visit_note_secs(30.0, 10.0, 0.0), None);
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
