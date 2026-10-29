//! Drafting on weak computers (0.2.3): Stop ends a draft with a plain
//! message and saves nothing, and the log holds counts and timings, never
//! names, transcript words or note text. The note model is a fake: no model
//! file is needed.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use os_llm::{Control, LlmError, Phase};

use super::*;

/// Reads its prompt slowly, as llama.cpp does on a weak processor, until
/// told to stop.
struct SlowReader {
    reading: Arc<AtomicBool>,
}

impl LlmBackend for SlowReader {
    fn capabilities(&self) -> os_llm::LlmCapabilities {
        FakeLlm::loaded("fake").0.capabilities()
    }

    fn generate(
        &self,
        request: &os_llm::GenerateRequest,
        _on_token: &mut dyn FnMut(&str) -> bool,
    ) -> std::result::Result<os_llm::GenerateOutput, LlmError> {
        let control: Control = request.control.clone().expect("a draft carries a control");
        let started = Instant::now();
        let mut done = 0;
        loop {
            if control.is_cancelled() {
                return Err(LlmError::Cancelled);
            }
            control.report(Phase::Prompt { done, total: 100_000 });
            self.reading.store(true, Ordering::SeqCst);
            assert!(started.elapsed() < Duration::from_secs(30), "never stopped");
            std::thread::sleep(Duration::from_millis(5));
            done += 1;
        }
    }
}

/// Writes a SOAP note with one statement in it.
struct Writer(String);

impl LlmBackend for Writer {
    fn capabilities(&self) -> os_llm::LlmCapabilities {
        FakeLlm::loaded("fake").0.capabilities()
    }

    fn generate(
        &self,
        _request: &os_llm::GenerateRequest,
        on_token: &mut dyn FnMut(&str) -> bool,
    ) -> std::result::Result<os_llm::GenerateOutput, LlmError> {
        for piece in self.0.as_bytes().chunks(16) {
            on_token(std::str::from_utf8(piece).unwrap());
        }
        let stats = os_llm::GenerateStats {
            prompt_tokens: 812,
            completion_tokens: 96,
            time_to_first_token_ms: 400,
            total_ms: 900,
            tokens_per_second: 12.5,
            hit_max_tokens: false,
        };
        Ok(os_llm::GenerateOutput { text: self.0.clone(), stats })
    }
}

/// A SOAP note whose first list holds `text`, citing line 0.
fn soap_note_with(text: &str) -> String {
    let soap = LoadedTemplate::builtin("soap").unwrap();
    let mut note = os_engine::template::empty_note(&soap.template);
    'found: for (_, section) in note.as_object_mut().unwrap() {
        if let Some(parts) = section.as_object_mut() {
            for (_, part) in parts {
                if part.is_array() {
                    *part = serde_json::json!([{ "text": text, "evidence": [0] }]);
                    break 'found;
                }
            }
        }
    }
    note.to_string()
}

/// Records every drafting event.
#[derive(Default)]
struct Board {
    phases: Mutex<Vec<DraftPhase>>,
    stages: Mutex<Vec<Stage>>,
}

impl Events for Board {
    fn segment(&self, _: &Segment) {}
    fn token(&self, _: &str) {}
    fn level(&self, _: f32, _: f32) {}
    fn download(&self, _: &str, _: u64, _: u64) {}
    fn progress(&self, p: &Progress) {
        self.stages.lock().unwrap().push(p.stage);
    }
    fn draft(&self, phase: &DraftPhase) {
        self.phases.lock().unwrap().push(*phase);
    }
}

#[test]
fn stop_ends_a_draft_with_a_plain_message_and_saves_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let app = Arc::new(app_in(tmp.path()));
    let s = seed_visit(&app.sessions_root, "Mochi Tanaka", &[], false);
    let id = id_of(&s);
    let runs = Session::open(&s.dir).unwrap().meta.runs.len();
    assert!(!app.cancel_draft(), "nothing to stop yet");
    let reading = Arc::new(AtomicBool::new(false));
    load_fake(&app, "slow", Arc::new(SlowReader { reading: reading.clone() }));
    let board = Arc::new(Board::default());
    let worker = {
        let (app, id, board) = (app.clone(), id.clone(), board.clone());
        std::thread::spawn(move || app.generate_note(&id, "slow", "soap", None, None, None, board.as_ref()))
    };
    let waited = Instant::now();
    while !reading.load(Ordering::SeqCst) {
        assert!(waited.elapsed() < Duration::from_secs(20), "the draft never started reading");
        std::thread::sleep(Duration::from_millis(5));
    }
    let pressed = Instant::now();
    assert!(app.cancel_draft());
    let err = worker.join().unwrap().unwrap_err();
    assert!(pressed.elapsed() < Duration::from_secs(5), "stopped promptly");
    assert_eq!(err.0, DRAFT_STOPPED);
    assert_eq!(Session::open(&s.dir).unwrap().meta.runs.len(), runs, "no half-saved note");
    assert!(!app.cancel_draft(), "the finished draft left the slot");
    assert!(
        board.phases.lock().unwrap().iter().any(|p| matches!(p, Phase::Prompt { total: 100_000, .. })),
        "the board heard the prompt being read"
    );
    assert!(!board.stages.lock().unwrap().contains(&Stage::Save));
    assert_eq!(app.last_draft().unwrap().outcome, "stopped");
    // A later draft isn't stopped by the earlier Stop.
    load_fake(&app, "writer", Arc::new(Writer(soap_note_with("Eating well"))));
    app.generate_note(&id, "writer", "soap", None, None, None, &Quiet).unwrap();
    assert_eq!(Session::open(&s.dir).unwrap().meta.runs.len(), runs + 1);
}

/// The hard rule for the log (0.2.3): stages, counts and timings, never the
/// patient's name, the transcript or the note.
#[test]
fn the_log_never_holds_names_transcript_words_or_note_text() {
    let tmp = tempfile::tempdir().unwrap();
    let app = Arc::new(app_in(tmp.path()));
    let s = seed_visit(&app.sessions_root, "Mochi Tanaka", &[], false);
    let id = id_of(&s);
    let logs_dir = tmp.path().join("logs");
    let dispatch = logs::dispatcher(&logs_dir).unwrap();
    tracing::dispatcher::with_default(&dispatch, || {
        // A draft that is saved, one that fails, and one that is stopped.
        load_fake(&app, "writer", Arc::new(Writer(soap_note_with("Zanzibarite drops in both ears"))));
        app.generate_note(&id, "writer", "soap", None, None, None, &Quiet).unwrap();
        load_fake(&app, "broken", Arc::new(Writer("{\"Zanzibarite\": ".into())));
        let err = app.generate_note(&id, "broken", "soap", None, None, None, &Quiet).unwrap_err();
        assert!(!err.0.is_empty());
        let reading = Arc::new(AtomicBool::new(false));
        load_fake(&app, "slow", Arc::new(SlowReader { reading: reading.clone() }));
        std::thread::scope(|scope| {
            let worker = scope.spawn(|| {
                tracing::dispatcher::with_default(&dispatch, || {
                    app.generate_note(&id, "slow", "soap", None, None, None, &Quiet)
                })
            });
            while !reading.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(5));
            }
            app.cancel_draft();
            assert!(worker.join().unwrap().is_err());
        });
        // A panic goes to the log, scrubbed.
        logs::install_panic_hook();
        let _ = std::panic::catch_unwind(|| panic!("broke in /v/2026-10-28_101200_mochi-tanaka/runs"));
    });
    let log = std::fs::read_to_string(logs_dir.join(logs::FILE)).unwrap();
    for line in
        ["drafting started", "drafting finished; saved", "kind=\"bad_json\"", "drafting stopped", "panic"]
    {
        assert!(log.contains(line), "missing {line:?} in:\n{log}");
    }
    assert!(log.contains("prompt_tokens=812"), "counts are logged:\n{log}");
    let lower = log.to_lowercase();
    for word in ["mochi", "tanaka", "ears look red", "both ears", "zanzibarite"] {
        assert!(!lower.contains(word), "{word:?} reached the log:\n{log}");
    }
    // Copy diagnostics carries the same log and the last draft's timings.
    let text = app.diagnostics_from(Some("writer"), &logs_dir).text;
    assert!(text.contains("drafting finished; saved"), "the log's end is included: {text}");
    assert!(text.contains("Last draft: soap with slow"), "{text}");
    assert!(text.contains("Note model: writer (chosen)"), "{text}");
    assert!(!text.to_lowercase().contains("mochi"));
}
