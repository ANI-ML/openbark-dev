use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use anyhow::Result;
use serde_json::json;
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

/// Hardware report used by onboarding to recommend models (M1) and by bench reports.
pub fn run() -> Result<()> {
    let mut sys = System::new();
    sys.refresh_memory();
    sys.refresh_cpu_all();
    let cpu = sys.cpus().first().map(|c| c.brand().trim().to_string()).unwrap_or_default();
    let compiled: Vec<&str> = [
        cfg!(feature = "cuda").then_some("cuda"),
        cfg!(feature = "vulkan").then_some("vulkan"),
        cfg!(feature = "metal").then_some("metal"),
        Some("cpu"),
    ]
    .into_iter()
    .flatten()
    .collect();
    let report = json!({
        "product": os_engine::PRODUCT_NAME,
        "version": env!("CARGO_PKG_VERSION"),
        "os": System::long_os_version(),
        "arch": std::env::consts::ARCH,
        "cpu": cpu,
        "logical_cores": sys.cpus().len(),
        "physical_cores": System::physical_core_count(),
        "ram_total_gb": sys.total_memory() as f64 / 1e9,
        "ram_available_gb": sys.available_memory() as f64 / 1e9,
        "llm_backends_compiled": compiled,
        "llm_devices": os_llm::devices(),
        "audio_backend": "sherpa-onnx (cpu)",
    });
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

/// Samples this process's resident memory every 100 ms and keeps the peak.
pub struct PeakMemory {
    stop: Arc<AtomicBool>,
    peak: Arc<AtomicU64>,
    handle: Option<JoinHandle<()>>,
}

impl PeakMemory {
    pub fn start() -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let peak = Arc::new(AtomicU64::new(0));
        let (s, p) = (stop.clone(), peak.clone());
        let handle = std::thread::spawn(move || {
            let pid = Pid::from_u32(std::process::id());
            let mut sys = System::new();
            while !s.load(Ordering::Relaxed) {
                sys.refresh_processes_specifics(
                    ProcessesToUpdate::Some(&[pid]),
                    true,
                    ProcessRefreshKind::nothing().with_memory(),
                );
                if let Some(proc_) = sys.process(pid) {
                    p.fetch_max(proc_.memory(), Ordering::Relaxed);
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        });
        Self { stop, peak, handle: Some(handle) }
    }

    /// Stop sampling; returns peak bytes.
    pub fn stop(mut self) -> u64 {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
        self.peak.load(Ordering::Relaxed)
    }
}
