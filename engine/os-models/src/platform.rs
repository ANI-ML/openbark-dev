//! Which models a device may offer: the platform profile (ADR-0018 section
//! 10, owner decisions of 2026-10-19).
//!
//! On a computer nothing changes. On an iPhone:
//! - the default models are the small, mobile ones: Parakeet int8 for speech
//!   and a small GGUF on llama.cpp (Metal) for drafting, chosen by the
//!   phone's memory tier;
//! - the catalog shows only entries marked `mobile`, and the Hugging Face
//!   browser only GGUFs within the tier's size budget, parameter cap and
//!   phone quantisations. MLX is never shown (OpenBark's MLX engine is a
//!   Python pack, which can't run on iOS);
//! - a model too large for this phone says so in plain words.
//!
//! `OPENBARK_PLATFORM=ios` turns the phone profile on in a computer build,
//! for demos and tests of the phone screens (like `OPENBARK_PLATFORM=macos`).
//! `OPENBARK_DEVICE_RAM_GB` then stands in for the phone's memory.

use std::cell::Cell;

use serde::Serialize;

thread_local! {
    /// Tests only: a profile and phone memory for this thread
    /// ([`override_for_tests`]). Never set in a running app.
    static OVERRIDE: Cell<Option<(Profile, Option<f64>)>> = const { Cell::new(None) };
}

/// Tests: act as `profile` on this thread (with `ram_gb` as the phone's
/// memory) until the guard is dropped. Environment variables are process-wide
/// and tests run in parallel, so tests use this instead of `OPENBARK_PLATFORM`.
#[doc(hidden)]
pub fn override_for_tests(profile: Profile, ram_gb: Option<f64>) -> OverrideGuard {
    OVERRIDE.with(|o| o.set(Some((profile, ram_gb))));
    OverrideGuard(())
}

/// Ends [`override_for_tests`] when dropped.
#[doc(hidden)]
pub struct OverrideGuard(());

impl Drop for OverrideGuard {
    fn drop(&mut self) {
        OVERRIDE.with(|o| o.set(None));
    }
}

/// The models a build offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Profile {
    /// Windows, macOS and Linux: every model, as before.
    Desktop,
    /// iPhone (and the phone screens in a demo): mobile models only.
    Ios,
}

impl Profile {
    /// This build's profile: iOS on an iPhone, or with `OPENBARK_PLATFORM=ios`.
    pub fn current() -> Self {
        if let Some((p, _)) = OVERRIDE.with(Cell::get) {
            return p;
        }
        if cfg!(target_os = "ios") {
            return Profile::Ios;
        }
        match std::env::var("OPENBARK_PLATFORM").as_deref().map(str::trim) {
            Ok("ios") => Profile::Ios,
            _ => Profile::Desktop,
        }
    }

    pub fn is_mobile(self) -> bool {
        self == Profile::Ios
    }
}

/// A phone memory tier (ADR-0018 section 10). Budgets are for the GGUF files
/// on disk: llama.cpp maps them, so most weight pages don't count against
/// iOS's per-app limit, but the KV cache, the Metal buffers and the app do.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Tier {
    /// The phone's nominal memory: 6, 8 or 12 GB.
    pub ram_gb: u32,
    /// Largest GGUF (all parts) offered for drafting.
    pub llm_budget_bytes: u64,
    /// Largest model offered, in billions of parameters (effective size for
    /// Gemma's E-models: E2B counts as 2).
    pub max_params_b: f64,
    /// The drafting model this tier picks by default.
    pub default_llm: &'static str,
}

/// Speech recognition on every phone tier: Parakeet TDT 0.6B v3 int8 (about
/// 670 MB; never loaded while a drafting model is).
pub const MOBILE_STT: &str = "parakeet-tdt-0.6b-v3-int8";

const GB: u64 = 1_000_000_000;

/// Phones with 6, 8 and 12 GB. Below 6 GB OpenBark can't run on the phone
/// (ADR-0018 section 3).
pub const TIERS: [Tier; 3] = [
    Tier { ram_gb: 6, llm_budget_bytes: 1_300_000_000, max_params_b: 2.0, default_llm: "qwen3-1.7b-q4_k_m" },
    Tier {
        ram_gb: 8,
        llm_budget_bytes: 3_500_000_000,
        max_params_b: 4.5,
        default_llm: "gemma-4-e2b-it-q4_0",
    },
    Tier {
        ram_gb: 12,
        llm_budget_bytes: 5_300_000_000,
        max_params_b: 8.5,
        default_llm: "gemma-4-e2b-it-q4_0",
    },
];

/// The tier for a phone with `ram_bytes` of memory, as the system reports it
/// (a "6 GB" iPhone reports a little under 6 GB). `None` below 6 GB.
pub fn tier_for_ram(ram_bytes: u64) -> Option<Tier> {
    let gb = ram_bytes as f64 / GB as f64;
    if gb >= 10.5 {
        Some(TIERS[2])
    } else if gb >= 7.0 {
        Some(TIERS[1])
    } else if gb >= 5.0 {
        Some(TIERS[0])
    } else {
        None
    }
}

/// The device memory to plan with: `OPENBARK_DEVICE_RAM_GB` in the phone
/// profile of a computer build (demos and tests), else what the system says.
pub fn device_ram(system_ram_bytes: u64) -> u64 {
    if let Some((_, Some(gb))) = OVERRIDE.with(Cell::get) {
        return (gb * GB as f64) as u64;
    }
    if !cfg!(target_os = "ios")
        && Profile::current().is_mobile()
        && let Some(gb) =
            std::env::var("OPENBARK_DEVICE_RAM_GB").ok().and_then(|v| v.trim().parse::<f64>().ok())
        && gb > 0.0
    {
        return (gb * GB as f64) as u64;
    }
    system_ram_bytes
}

/// Quantisations a phone runs well with llama.cpp's Metal backend: 3 to 5
/// bits (Unsloth's dynamic "XL" builds of those included). 8-bit only for models of 1B parameters or fewer (Qwen3 0.6B ships
/// only Q8_0); never 16 or 32-bit, Q6_K or 2-bit.
const PHONE_QUANTS: &[&str] = &[
    "q3_k_m", "q3_k_l", "q3_k_xl", "iq4_xs", "iq4_nl", "q4_0", "q4_1", "q4_k_s", "q4_k_m", "q4_k_xl", "q5_0",
    "q5_k_s", "q5_k_m", "q5_k_xl",
];

/// Every quantisation name we recognise in a GGUF file name, longest first so
/// `q4_k_m` wins over `q4_k`.
const KNOWN_QUANTS: &[&str] = &[
    "iq4_xs", "iq4_nl", "iq3_xxs", "iq3_xs", "iq3_s", "iq3_m", "iq2_xxs", "iq2_xs", "iq2_s", "iq2_m",
    "iq1_s", "iq1_m", "q2_k_l", "q2_k_s", "q3_k_xl", "q3_k_s", "q3_k_m", "q3_k_l", "q4_k_xl", "q4_k_s",
    "q4_k_m", "q5_k_xl", "q5_k_s", "q5_k_m", "q6_k_xl", "q8_k_xl", "q2_k", "q3_k", "q4_k", "q5_k", "q6_k",
    "q4_0", "q4_1", "q5_0", "q5_1", "q8_0", "mxfp4", "bf16", "f16", "f32",
];

/// The quantisation in a GGUF file name, lower case ("q4_k_m", "q4_0"), if
/// one is recognised. `gemma-4-E2B_q4_0-it.gguf` gives "q4_0".
pub fn quant_of(file: &str) -> Option<&'static str> {
    let name = file.rsplit('/').next().unwrap_or(file).to_ascii_lowercase();
    let bytes = name.as_bytes();
    for q in KNOWN_QUANTS {
        let mut from = 0;
        while let Some(i) = name[from..].find(q) {
            let start = from + i;
            let end = start + q.len();
            let before = start == 0 || !bytes[start - 1].is_ascii_alphanumeric();
            let after = end == bytes.len() || !bytes[end].is_ascii_alphanumeric();
            if before && after {
                return Some(q);
            }
            from = start + 1;
        }
    }
    None
}

/// Parameters in billions, read from a model or file name: "Qwen3-1.7B" gives
/// 1.7, "gemma-3-270m" 0.27, and Gemma's E-models their effective size
/// ("gemma-4-E2B" gives 2). Mixture-of-experts names ("30B-A3B") give the
/// total, since every expert must be in memory.
pub fn params_from_name(name: &str) -> Option<f64> {
    let lower = name.to_ascii_lowercase();
    lower.split(|c: char| !(c.is_ascii_alphanumeric() || c == '.')).find_map(|token| {
        // Gemma's E-models: "e2b" is 2B effective.
        let token =
            token.strip_prefix('e').filter(|t| t.starts_with(|c: char| c.is_ascii_digit())).unwrap_or(token);
        let (number, scale) = if let Some(n) = token.strip_suffix('b') {
            (n, 1.0)
        } else if let Some(n) = token.strip_suffix('m') {
            (n, 0.001)
        } else {
            return None;
        };
        if number.is_empty() || !number.starts_with(|c: char| c.is_ascii_digit()) {
            return None;
        }
        number.parse::<f64>().ok().map(|n| n * scale)
    })
}

/// Why a GGUF isn't offered on this phone.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TooBig {
    /// The phone has less than 6 GB: no drafting on it at all.
    PhoneTooSmall,
    /// More parameters than the tier allows.
    TooManyParams { params_b: f64, max_params_b: f64 },
    /// The files are larger than the tier's budget.
    TooLarge { bytes: u64, budget_bytes: u64 },
    /// A quantisation a phone doesn't run well (16-bit, 6-bit, 2-bit, or
    /// 8-bit above 1B), or one we can't read from the name.
    Quantisation,
}

impl TooBig {
    /// What the interface says, with the phone's memory.
    pub fn message(&self, ram_gb: Option<u32>) -> String {
        let phone = ram_gb.map_or_else(|| "this iPhone".to_string(), |g| format!("this iPhone ({g} GB)"));
        match self {
            TooBig::PhoneTooSmall => {
                "OpenBark needs an iPhone with 6 GB of memory or more to write notes on the phone.".into()
            }
            TooBig::TooManyParams { params_b, max_params_b } => format!(
                "Too large for {phone}: about {params_b}B parameters. Models up to {max_params_b}B fit."
            ),
            TooBig::TooLarge { bytes, budget_bytes } => format!(
                "Too large for {phone}: {:.1} GB. Models up to {:.1} GB fit.",
                *bytes as f64 / GB as f64,
                *budget_bytes as f64 / GB as f64
            ),
            TooBig::Quantisation => format!(
                "Not offered on {phone}: phones need a 3 to 5-bit build (Q4_K_M or Q4_0, for example)."
            ),
        }
    }
}

/// Whether a GGUF of `bytes` (all parts), named `file`, from a model of
/// `params_b` billion parameters (if known), may be offered on a phone of
/// `tier`.
pub fn check_gguf(tier: Option<Tier>, file: &str, bytes: u64, params_b: Option<f64>) -> Result<(), TooBig> {
    let tier = tier.ok_or(TooBig::PhoneTooSmall)?;
    let params = params_b.or_else(|| params_from_name(file));
    if let Some(p) = params
        && p > tier.max_params_b
    {
        return Err(TooBig::TooManyParams { params_b: p, max_params_b: tier.max_params_b });
    }
    match quant_of(file) {
        Some(q) if PHONE_QUANTS.contains(&q) => {}
        Some("q8_0") if params.is_some_and(|p| p <= 1.0) => {}
        _ => return Err(TooBig::Quantisation),
    }
    if bytes > tier.llm_budget_bytes {
        return Err(TooBig::TooLarge { bytes, budget_bytes: tier.llm_budget_bytes });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiers_follow_the_reported_memory() {
        // What a 6 GB, 8 GB and 12 GB iPhone report, roughly.
        assert_eq!(tier_for_ram(5_700_000_000).map(|t| t.ram_gb), Some(6));
        assert_eq!(tier_for_ram(7_600_000_000).map(|t| t.ram_gb), Some(8));
        assert_eq!(tier_for_ram(11_400_000_000).map(|t| t.ram_gb), Some(12));
        assert_eq!(tier_for_ram(3_900_000_000), None, "4 GB phones can't run OpenBark");
        // The budgets grow with the tier.
        assert!(TIERS.windows(2).all(|w| w[0].llm_budget_bytes < w[1].llm_budget_bytes));
        assert!(TIERS.windows(2).all(|w| w[0].max_params_b < w[1].max_params_b));
    }

    #[test]
    fn quantisations_are_read_from_file_names() {
        assert_eq!(quant_of("gemma-4-E2B_q4_0-it.gguf"), Some("q4_0"));
        assert_eq!(quant_of("Qwen3-1.7B-Q4_K_M.gguf"), Some("q4_k_m"));
        assert_eq!(quant_of("Q4_K_M/Qwen3-235B-A22B-Q4_K_M-00001-of-00003.gguf"), Some("q4_k_m"));
        assert_eq!(quant_of("gemma-4-E2B-it-IQ4_XS.gguf"), Some("iq4_xs"));
        assert_eq!(quant_of("model-BF16.gguf"), Some("bf16"));
        assert_eq!(quant_of("Qwen3-0.6B-Q8_0.gguf"), Some("q8_0"));
        assert_eq!(quant_of("phi-4-mini-Q6_K.gguf"), Some("q6_k"));
        assert_eq!(quant_of("mystery.gguf"), None);
    }

    #[test]
    fn parameters_are_read_from_names() {
        assert_eq!(params_from_name("unsloth/Qwen3-1.7B-GGUF"), Some(1.7));
        assert_eq!(params_from_name("Qwen3-4B-Instruct-2507-Q4_K_M.gguf"), Some(4.0));
        assert_eq!(params_from_name("google/gemma-4-E2B-it-qat-q4_0-gguf"), Some(2.0));
        assert_eq!(params_from_name("gemma-4-E4B_q4_0-it.gguf"), Some(4.0));
        assert_eq!(params_from_name("ggml-org/gemma-3-270m-it-qat-GGUF"), Some(0.27));
        assert_eq!(params_from_name("unsloth/Qwen3-30B-A3B-GGUF"), Some(30.0));
        assert_eq!(params_from_name("bartowski/Llama-3.2-3B-Instruct-GGUF"), Some(3.0));
        assert_eq!(params_from_name("unsloth/Phi-4-mini-instruct-GGUF"), None, "no size in the name");
        assert_eq!(params_from_name("gemma-4-12b-it-qat-q4_0.gguf"), Some(12.0));
    }

    #[test]
    fn the_checks_hold_per_tier() {
        let [t6, t8, t12] = TIERS;
        // Gemma 4 E2B QAT (3.35 GB): 8 and 12 GB phones; on 6 GB its 2B
        // effective parameters pass but the file is too large.
        let e2b = ("gemma-4-E2B_q4_0-it.gguf", 3_349_516_256);
        assert!(matches!(check_gguf(Some(t6), e2b.0, e2b.1, None), Err(TooBig::TooLarge { .. })));
        assert_eq!(check_gguf(Some(t8), e2b.0, e2b.1, None), Ok(()));
        assert_eq!(check_gguf(Some(t12), e2b.0, e2b.1, None), Ok(()));
        // Gemma 4 E4B QAT (5.15 GB): 12 GB phones only.
        let e4b = ("gemma-4-E4B_q4_0-it.gguf", 5_154_941_280);
        assert!(check_gguf(Some(t8), e4b.0, e4b.1, None).is_err());
        assert_eq!(check_gguf(Some(t12), e4b.0, e4b.1, None), Ok(()));
        // Qwen3 1.7B Q4_K_M (1.1 GB): every tier.
        for t in TIERS {
            assert_eq!(check_gguf(Some(t), "Qwen3-1.7B-Q4_K_M.gguf", 1_107_409_472, None), Ok(()));
        }
        // 12B never fits a phone; 16-bit and 6-bit builds aren't offered.
        assert!(matches!(
            check_gguf(Some(t12), "gemma-4-12b-it-qat-q4_0.gguf", 6_975_879_296, None),
            Err(TooBig::TooManyParams { .. })
        ));
        assert_eq!(
            check_gguf(Some(t12), "Qwen3-1.7B-BF16.gguf", 3_400_000_000, None),
            Err(TooBig::Quantisation)
        );
        assert_eq!(
            check_gguf(Some(t12), "Qwen3-1.7B-Q6_K.gguf", 1_400_000_000, None),
            Err(TooBig::Quantisation)
        );
        // 8-bit only up to 1B.
        assert_eq!(check_gguf(Some(t6), "Qwen3-0.6B-Q8_0.gguf", 639_446_688, None), Ok(()));
        assert_eq!(
            check_gguf(Some(t8), "Qwen3-1.7B-Q8_0.gguf", 1_834_426_016, None),
            Err(TooBig::Quantisation)
        );
        // Size alone: a 4B Q4_K_M (2.5 GB) on a 6 GB phone fails on parameters,
        // a name without a size fails on bytes.
        assert!(matches!(
            check_gguf(Some(t6), "Phi-4-mini-instruct-Q4_K_M.gguf", 2_491_874_272, None),
            Err(TooBig::TooLarge { .. })
        ));
        // Parameters from the Hub beat the name.
        assert!(matches!(
            check_gguf(Some(t8), "Phi-4-mini-instruct-Q4_K_M.gguf", 2_491_874_272, Some(3.84)),
            Ok(())
        ));
        // Below 6 GB: nothing.
        assert_eq!(check_gguf(None, "Qwen3-1.7B-Q4_K_M.gguf", 1, None), Err(TooBig::PhoneTooSmall));
    }

    #[test]
    fn tier_defaults_are_mobile_catalog_models_that_fit_their_tier() {
        let catalog = crate::Catalog::builtin();
        for t in TIERS {
            let m = catalog
                .get(t.default_llm)
                .unwrap_or_else(|| panic!("{} isn't in the catalog", t.default_llm));
            assert!(m.mobile, "{} must be marked mobile", m.id);
            let crate::Layout::LlmGguf { model } = &m.layout else { panic!("{} isn't a GGUF", m.id) };
            assert_eq!(
                check_gguf(Some(t), model, m.total_size(), None),
                Ok(()),
                "{} on {} GB",
                m.id,
                t.ram_gb
            );
        }
        assert!(catalog.get(MOBILE_STT).is_some_and(|m| m.mobile));
        // Every mobile note model fits the largest tier; the 12B never does.
        for m in catalog.models.iter().filter(|m| m.mobile) {
            if let crate::Layout::LlmGguf { model } = &m.layout {
                assert_eq!(check_gguf(Some(TIERS[2]), model, m.total_size(), None), Ok(()), "{}", m.id);
            }
        }
        assert!(!catalog.get("gemma-4-12b-it-q4_0").unwrap().mobile);
    }

    #[test]
    fn messages_are_plain() {
        let m = TooBig::TooLarge { bytes: 5_154_941_280, budget_bytes: 3_500_000_000 }.message(Some(8));
        assert_eq!(m, "Too large for this iPhone (8 GB): 5.2 GB. Models up to 3.5 GB fit.");
        let m = TooBig::TooManyParams { params_b: 12.0, max_params_b: 4.5 }.message(Some(8));
        assert_eq!(m, "Too large for this iPhone (8 GB): about 12B parameters. Models up to 4.5B fit.");
    }
}
