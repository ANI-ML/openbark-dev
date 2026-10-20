//! The shutdown order on the way out of the process, with a real model (CI;
//! scripts/ci/metal-exit.sh runs it on macOS with Metal).
//!
//! ```text
//! exit_order <scratch dir> <model.gguf> app    # free the model, return from main: exit 0
//! exit_order <scratch dir> <model.gguf> busy   # quit while a draft runs: exit 0
//! exit_order <scratch dir> <model.gguf> leak   # keep the model at exit (the 0.1.1 bug)
//! ```
//!
//! On macOS 15+ with Metal, `leak` aborts in ggml's Metal teardown
//! (`ggml_metal_rsets_free`), which shows the job can see the crash; `app`
//! and `busy` must exit cleanly. The scratch dir holds the visits folder and
//! nothing else; set OPENBARK_DATA_KEY and the other OPENBARK_* paths to
//! scratch files too.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

const EXAMPLE: &str = "Subjective: Bella, 4 year old spayed Labrador, two days of vomiting.\n\
                       Objective: T 38.6 C, HR 96.\nAssessment: Gastritis.\nPlan: Bland diet for three days.";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [scratch, model, mode] = args.as_slice() else {
        eprintln!("usage: exit_order <scratch dir> <model.gguf> app|busy|leak");
        std::process::exit(2);
    };
    let scratch = PathBuf::from(scratch);
    let app = Arc::new(os_app::App::new(
        os_models::ModelStore::new(scratch.join("models")),
        scratch.join("visits"),
    ));
    match mode.as_str() {
        "app" | "leak" => {
            let started = Instant::now();
            let outcome = app.template_from_example(model, EXAMPLE);
            eprintln!(
                "exit_order: generated in {:.1}s ({}); live models: {}",
                started.elapsed().as_secs_f32(),
                if outcome.is_ok() {
                    "a proposal"
                } else {
                    "no usable proposal, as expected of a tiny model"
                },
                os_llm::shutdown::live_models()
            );
            assert_eq!(os_llm::shutdown::live_models(), 1, "the model stays loaded in the app's slot");
            if mode == "leak" {
                eprintln!("exit_order: returning from main with the model still loaded");
                std::mem::forget(app);
                return;
            }
            let freed = app.shutdown(os_app::SHUTDOWN_WAIT);
            eprintln!("exit_order: freed before exit: {freed}; returning from main");
            assert!(freed);
            assert_eq!(os_llm::shutdown::live_models(), 0);
        }
        "busy" => {
            let draft = {
                let app = app.clone();
                let model = model.clone();
                std::thread::spawn(move || app.template_from_example(&model, &EXAMPLE.repeat(8)))
            };
            // Quit once the model is loaded and generating.
            let deadline = Instant::now() + Duration::from_secs(120);
            while os_llm::shutdown::live_models() == 0 && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(20));
            }
            std::thread::sleep(Duration::from_millis(500));
            let started = Instant::now();
            let freed = app.shutdown(os_app::SHUTDOWN_WAIT);
            eprintln!(
                "exit_order: quit during a draft; freed: {freed} in {:.2}s; draft: {:?}",
                started.elapsed().as_secs_f32(),
                draft.join().map(|r| r.err().map(|e| e.0))
            );
            assert!(freed);
            os_app::exit_process(0);
        }
        other => {
            eprintln!("unknown mode {other}");
            std::process::exit(2);
        }
    }
}
