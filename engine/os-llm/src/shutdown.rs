//! Releasing llama.cpp before the process exits.
//!
//! On macOS 15 and later, ggml's Metal backend keeps every GPU buffer in a
//! residency set and runs a keep-alive thread for them. The Metal device
//! itself is a C++ static, freed by `exit()` after `main` (or after
//! `-[NSApplication terminate:]` on Cmd-Q). If a model's buffers are still
//! registered then, ggml aborts the process: `GGML_ASSERT([rsets->data count]
//! == 0)` in `ggml_metal_rsets_free`, a SIGABRT crash report on every quit.
//! Upstream keeps the assert on purpose: the embedding app must free every
//! model and context first (llama.cpp PR 28395, closed unmerged).
//!
//! So the order on the way out is fixed:
//!
//! 1. [`begin`]: running generations stop at their next token or prompt
//!    chunk, and no new model loads.
//! 2. The app drops every model handle it holds (`os_app::App::shutdown`).
//!    [`LlamaCppBackend`](crate::LlamaCppBackend) frees its model before its
//!    `LlamaBackend` (which calls `llama_backend_free`), and its contexts
//!    live only inside `generate`.
//! 3. [`exit`]: with no model left, the normal `exit()` runs the static
//!    destructors safely. If a model is still alive (a load that couldn't
//!    be interrupted), it ends the process without them instead of letting
//!    ggml abort.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

static STOPPING: AtomicBool = AtomicBool::new(false);
static LIVE: AtomicUsize = AtomicUsize::new(0);

/// Stop generating and refuse new loads, for the rest of this process.
pub fn begin() {
    STOPPING.store(true, Ordering::SeqCst);
}

/// [`begin`] has been called.
pub fn stopping() -> bool {
    STOPPING.load(Ordering::SeqCst)
}

/// llama.cpp models alive in this process (each holds GPU buffers).
pub fn live_models() -> usize {
    LIVE.load(Ordering::SeqCst)
}

/// Wait until every llama.cpp model has been freed, or `timeout` passes.
/// True when none is left.
pub fn wait_released(timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if live_models() == 0 {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// End the process with `code`. With every model freed this is
/// `std::process::exit`. With one still alive it skips the C++ static
/// destructors (`_exit`), which would otherwise abort in ggml's Metal
/// teardown; standard output and error are flushed first. Rust destructors
/// don't run on either path, as with `std::process::exit`.
pub fn exit(code: i32) -> ! {
    let live = live_models();
    if live == 0 {
        std::process::exit(code);
    }
    tracing::warn!(live, "a note model was still loaded at exit; skipping the C runtime's teardown");
    end_without_teardown(code)
}

#[cfg(unix)]
fn end_without_teardown(code: i32) -> ! {
    use std::io::Write;
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    unsafe extern "C" {
        fn _exit(status: i32) -> !;
    }
    // SAFETY: `_exit` is async-signal-safe and takes no pointers.
    unsafe { _exit(code) }
}

#[cfg(not(unix))]
fn end_without_teardown(code: i32) -> ! {
    // Windows and Linux builds have no Metal residency sets; the normal exit is safe.
    std::process::exit(code)
}

/// Counts a live model for as long as it exists. Kept as the last field of
/// [`LlamaCppBackend`](crate::LlamaCppBackend), so the count drops only
/// after the model and the backend have been freed.
pub(crate) struct LiveModel(());

impl LiveModel {
    pub(crate) fn new() -> Self {
        LIVE.fetch_add(1, Ordering::SeqCst);
        Self(())
    }
}

impl Drop for LiveModel {
    fn drop(&mut self) {
        LIVE.fetch_sub(1, Ordering::SeqCst);
    }
}
