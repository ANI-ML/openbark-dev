//! The shutdown order (crate::shutdown). Its own test binary, because
//! `shutdown::begin` lasts for the rest of the process.
//!
//! With `OPENBARK_TEST_GGUF` set to a small chat GGUF, the test also loads it,
//! stops a running generation and checks the model is freed. The macOS CI job
//! runs that with Metal; elsewhere only the model-free part runs.

use std::sync::Arc;
use std::time::Duration;

use os_llm::{
    ChatMessage, GenerateRequest, LlamaCppBackend, LlamaCppOptions, LlmBackend, LlmError, shutdown,
};

#[test]
fn shutdown_stops_generation_frees_models_and_refuses_loads() {
    assert!(!shutdown::stopping());
    assert_eq!(shutdown::live_models(), 0);

    let model = std::env::var_os("OPENBARK_TEST_GGUF").map(std::path::PathBuf::from);
    let running = model.map(|path| {
        let backend: Arc<dyn LlmBackend> =
            Arc::new(LlamaCppBackend::load(LlamaCppOptions::new(&path)).expect("test model loads"));
        assert_eq!(shutdown::live_models(), 1, "a loaded model is counted");
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let worker = {
            let backend = backend.clone();
            std::thread::spawn(move || {
                let mut request =
                    GenerateRequest::new(vec![ChatMessage::user("Count from one to a thousand.")]);
                request.max_tokens = 4000;
                let mut first = Some(started_tx);
                backend.generate(&request, &mut |_| {
                    if let Some(tx) = first.take() {
                        let _ = tx.send(());
                    }
                    true
                })
            })
        };
        started_rx.recv_timeout(Duration::from_secs(120)).expect("generation started");
        (backend, worker, path)
    });

    shutdown::begin();
    assert!(shutdown::stopping());

    if let Some((backend, worker, path)) = running {
        let result = worker.join().expect("generation thread");
        assert!(matches!(result, Err(LlmError::Stopped)), "a running generation stops: {:?}", result.err());
        let request = GenerateRequest::new(vec![ChatMessage::user("Hello")]);
        assert!(matches!(backend.generate(&request, &mut |_| true), Err(LlmError::Stopped)));
        assert!(!shutdown::wait_released(Duration::from_millis(50)), "still held");
        drop(backend);
        assert!(shutdown::wait_released(Duration::from_secs(5)), "the model is freed once dropped");
        assert!(matches!(LlamaCppBackend::load(LlamaCppOptions::new(&path)), Err(LlmError::Stopped)));
    }

    // No new model loads once stopping (any existing file; it never gets parsed).
    let file = std::env::current_exe().expect("test binary path");
    assert!(matches!(LlamaCppBackend::load(LlamaCppOptions::new(file)), Err(LlmError::Stopped)));
    assert_eq!(shutdown::live_models(), 0, "a refused load leaves nothing behind");
    assert!(shutdown::wait_released(Duration::ZERO));
}
