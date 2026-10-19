//! MlxBackend against the fake worker (any computer with Python 3), the real
//! worker on a plain Python (protocol and network guard, without MLX), and,
//! when `OPENBARK_MLX_TEST_PYTHON` and `OPENBARK_MLX_TEST_MODEL` are set (see
//! `scripts/mlx-test-env.sh`), the real worker on the MLX engine pack with a
//! small model. Without them those tests print "skipped" and pass, unless
//! `OPENBARK_MLX_TEST_REQUIRED=1` (CI's MLX job), which makes them fail.
//!
//! Nothing here touches the real data folders: every folder is a fresh
//! scratch folder under the system temp folder, removed afterwards.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use os_llm::{
    ChatMessage, GenerateRequest, LlmBackend, LlmError, MlxBackend, MlxDevice, MlxOptions, ModelCheck,
    mlx::{WORKER_SOURCE, write_worker},
};

// ---------------------------------------------------------------- helpers

/// A Python 3 to run the fake worker: `python3`, else `python` (Windows).
fn python() -> Option<PathBuf> {
    static PY: OnceLock<Option<PathBuf>> = OnceLock::new();
    PY.get_or_init(|| {
        for name in ["python3", "python"] {
            let out = Command::new(name)
                .args(["-c", "import sys; print(sys.executable) if sys.version_info >= (3, 9) else exit(1)"])
                .stdin(Stdio::null())
                .output();
            if let Ok(out) = out
                && out.status.success()
            {
                let p = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim());
                if p.is_file() {
                    return Some(p);
                }
            }
        }
        None
    })
    .clone()
}

macro_rules! need_python {
    () => {
        match python() {
            Some(p) => p,
            None => {
                println!("skipped: no Python 3 found");
                return;
            }
        }
    };
}

/// A scratch folder, removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Self {
        static N: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join("openbark-mlx-tests").join(format!(
            "{}-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed),
            label
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn fake_script() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("mlx_fake_worker.py")
}

/// Options for the fake worker with a model folder named `model`.
fn fake(python: &Path, scratch: &Scratch, model: &str) -> MlxOptions {
    let work = scratch.path().join("work");
    let model_dir = scratch.path().join("models").join(model);
    std::fs::create_dir_all(&work).unwrap();
    std::fs::create_dir_all(&model_dir).unwrap();
    let mut o = MlxOptions::new(python.to_path_buf(), work, model_dir, format!("fake/{model}"));
    o.sandbox = false;
    o.worker_script = Some(fake_script());
    o
}

fn counter(o: &MlxOptions, name: &str) -> u32 {
    std::fs::read_to_string(o.model_dir.join(name)).map(|s| s.trim().parse().unwrap_or(0)).unwrap_or(0)
}

fn req(text: &str) -> GenerateRequest {
    GenerateRequest::new(vec![ChatMessage::system("You write notes."), ChatMessage::user(text)])
}

fn schema_req(text: &str, schema: &str) -> GenerateRequest {
    let mut r = req(text);
    r.json_schema = Some(schema.to_string());
    r
}

const OBJ_SCHEMA: &str = r#"{"type":"object","properties":{"note":{"type":"string"}},"required":["note"]}"#;

fn collect(b: &MlxBackend, r: &GenerateRequest) -> (Result<os_llm::GenerateOutput, LlmError>, Vec<String>) {
    let mut pieces = Vec::new();
    let out = b.generate(r, &mut |p| {
        pieces.push(p.to_string());
        true
    });
    (out, pieces)
}

// ---------------------------------------------------------------- fake worker

#[test]
fn loads_and_reports_capabilities() {
    let py = need_python!();
    let s = Scratch::new("caps");
    let o = fake(&py, &s, "ok");
    let b = MlxBackend::load(o.clone()).unwrap();
    assert_eq!(b.model_check_ran(), Some(true));
    let c = b.capabilities();
    assert_eq!(c.backend, "mlx");
    assert_eq!(c.model_name, "fake/ok");
    assert!(c.streaming && !c.audio_input && c.json_schema_constrained);
    assert_eq!(c.context_length, 32768, "capped by max_context");
    assert_eq!(counter(&o, "generates"), 1, "the model check generated once");
    drop(b);

    let mut o = fake(&py, &s, "ok");
    o.max_context = 100_000;
    o.model_check = ModelCheck::AlreadyPassed;
    let b = MlxBackend::load(o.clone()).unwrap();
    assert_eq!(b.model_check_ran(), None);
    assert_eq!(b.capabilities().context_length, 40960);
    assert!(b.capabilities().json_schema_constrained);
    assert_eq!(counter(&o, "generates"), 1, "no model check when it already passed");
}

/// `max_context` is enforced by the worker, not only reported: a prompt
/// that llama.cpp would refuse is refused on MLX too, with the same error.
#[test]
fn max_context_is_enforced_by_the_worker() {
    let py = need_python!();
    let s = Scratch::new("maxctx");
    let mut o = fake(&py, &s, "ok");
    o.max_context = 4096;
    o.model_check = ModelCheck::AlreadyPassed;
    let b = MlxBackend::load(o.clone()).unwrap();
    assert_eq!(b.capabilities().context_length, 4096);
    let mut r = req(&"x".repeat(3000));
    r.max_tokens = 2048;
    match b.generate(&r, &mut |_| panic!("nothing is generated")) {
        Err(LlmError::ContextOverflow { prompt, max_new: 2048, ctx: 4096 }) => {
            assert_eq!(prompt, 3000 + "You write notes.".len() as u32)
        }
        other => panic!("{other:?}"),
    }
    // Within the cap it writes, and the worker is the same one.
    r.max_tokens = 512;
    assert!(b.generate(&r, &mut |_| true).is_ok());
    assert_eq!(counter(&o, "loads"), 1);
}

#[test]
fn streams_tokens_in_order() {
    let py = need_python!();
    let s = Scratch::new("stream");
    let b = MlxBackend::load(fake(&py, &s, "ok")).unwrap();
    let (out, pieces) = collect(&b, &req("hi"));
    let out = out.unwrap();
    assert_eq!(pieces, vec!["Hello", ", ", "w\u{f6}rld", " \u{1F43E}", "\nbye"]);
    assert_eq!(out.text, pieces.concat());
    assert_eq!(out.stats.completion_tokens, 5);
    assert_eq!(out.stats.prompt_tokens, ("You write notes.".len() + 2) as u32);
    assert_eq!(out.stats.time_to_first_token_ms, 5);
    assert_eq!(out.stats.total_ms, 20);
    assert!((out.stats.tokens_per_second - 12.5).abs() < 1e-9);
    assert!(!out.stats.hit_max_tokens);
    // Under a schema the backend is constrained, so the request goes through.
    let (out, _) = collect(&b, &schema_req("hi", OBJ_SCHEMA));
    assert_eq!(out.unwrap().text, r#"{"note":"fine"}"#);
}

#[test]
fn framing_survives_noise_split_lines_and_big_tokens() {
    let py = need_python!();
    let s = Scratch::new("noisy");
    let b = MlxBackend::load(fake(&py, &s, "noisy")).unwrap();
    let (out, pieces) = collect(&b, &req("hi"));
    let out = out.unwrap();
    assert_eq!(pieces.len(), 3, "garbage, blank and other requests' lines are skipped");
    assert_eq!(pieces[0], "split ");
    assert_eq!(pieces[1].len(), 1 << 20);
    assert_eq!(pieces[2], "\u{e9}\u{1F415}\n\"q\"");
    assert_eq!(out.text, pieces.concat());
}

#[test]
fn cancel_stops_the_generation_and_the_worker_carries_on() {
    let py = need_python!();
    let s = Scratch::new("cancel");
    let o = fake(&py, &s, "slow");
    let b = MlxBackend::load(o.clone()).unwrap();
    let started = Instant::now();
    let mut seen = Vec::new();
    let out = b
        .generate(&req("long"), &mut |p| {
            seen.push(p.to_string());
            seen.len() < 3
        })
        .unwrap();
    assert!(started.elapsed() < Duration::from_secs(5), "cancel took {:?}", started.elapsed());
    assert_eq!(seen, vec!["t0 ", "t1 ", "t2 "]);
    assert_eq!(out.text, "t0 t1 t2 ");
    assert!(!out.stats.hit_max_tokens);
    // Same worker, next request.
    let mut n = 0;
    b.generate(&req("again"), &mut |_| {
        n += 1;
        n < 2
    })
    .unwrap();
    assert_eq!(counter(&o, "loads"), 1, "no restart was needed");
}

#[test]
fn a_worker_ignoring_cancel_is_read_to_the_end() {
    let py = need_python!();
    let s = Scratch::new("deaf");
    let b = MlxBackend::load(fake(&py, &s, "deaf")).unwrap();
    let mut calls = 0;
    let out = b
        .generate(&req("x"), &mut |_| {
            calls += 1;
            false
        })
        .unwrap();
    assert_eq!(calls, 1, "on_token isn't called again after it said stop");
    assert_eq!(out.text, "d0 ");
    assert!(!out.stats.hit_max_tokens);
    // The leftover tokens of that request don't leak into the next one.
    let (out, pieces) = collect(&b, &req("y"));
    assert_eq!(pieces.len(), 30);
    assert!(out.unwrap().text.starts_with("d0 d1 "));
}

#[test]
fn every_error_kind_maps_to_an_llm_error() {
    let py = need_python!();
    let s = Scratch::new("errors");
    match MlxBackend::load(fake(&py, &s, "fail-load")) {
        Err(LlmError::Load(m)) => assert_eq!(m, "the model couldn't be loaded"),
        other => panic!("{:?}", other.err()),
    }
    match MlxBackend::load(fake(&py, &s, "fail-arch")) {
        Err(LlmError::Load(m)) => assert!(m.contains("can't run this kind of model"), "{m}"),
        other => panic!("{:?}", other.err()),
    }
    type Case = (&'static str, fn(&LlmError) -> bool);
    let cases: [Case; 5] = [
        ("fail-template", |e| matches!(e, LlmError::Template(_))),
        ("fail-grammar", |e| matches!(e, LlmError::Grammar(_))),
        ("fail-inference", |e| matches!(e, LlmError::Inference(_))),
        ("fail-bad_request", |e| matches!(e, LlmError::Inference(_))),
        ("fail-context_overflow", |e| {
            matches!(e, LlmError::ContextOverflow { prompt: 5000, max_new: 2048, ctx: 4096 })
        }),
    ];
    for (model, ok) in cases {
        let mut o = fake(&py, &s, model);
        o.model_check = ModelCheck::AlreadyPassed;
        let b = MlxBackend::load(o.clone()).unwrap();
        let err = b.generate(&req("x"), &mut |_| true).unwrap_err();
        assert!(ok(&err), "{model}: {err:?}");
        // A request error leaves the worker running.
        let _ = b.generate(&req("x"), &mut |_| true);
        assert_eq!(counter(&o, "loads"), 1, "{model}");
    }
}

#[test]
fn a_crashed_worker_is_restarted_once_on_the_next_generate() {
    let py = need_python!();
    let s = Scratch::new("crash");
    let o = fake(&py, &s, "crash-once");
    let b = MlxBackend::load(MlxOptions { model_check: ModelCheck::AlreadyPassed, ..o.clone() }).unwrap();
    match b.generate(&req("x"), &mut |_| true) {
        Err(LlmError::Inference(m)) => assert_eq!(m, "the MLX engine stopped"),
        other => panic!("{other:?}"),
    }
    let (out, _) = collect(&b, &req("x"));
    assert_eq!(out.unwrap().text, "Hello, w\u{f6}rld \u{1F43E}\nbye");
    assert_eq!(counter(&o, "loads"), 2, "the model was loaded again in the new worker");

    let o = fake(&py, &s, "crash-always");
    let b = MlxBackend::load(MlxOptions { model_check: ModelCheck::AlreadyPassed, ..o.clone() }).unwrap();
    for _ in 0..2 {
        match b.generate(&req("x"), &mut |_| true) {
            Err(LlmError::Inference(m)) => assert_eq!(m, "the MLX engine stopped"),
            other => panic!("{other:?}"),
        }
    }
    assert_eq!(counter(&o, "loads"), 2, "one restart, which crashed again");
}

#[test]
fn a_silent_worker_is_killed() {
    let py = need_python!();
    let s = Scratch::new("hang");
    let o = fake(&py, &s, "hang-once");
    let b = MlxBackend::load(MlxOptions { model_check: ModelCheck::AlreadyPassed, ..o.clone() }).unwrap();
    b.set_silence_timeout(Duration::from_millis(800));
    let started = Instant::now();
    match b.generate(&req("x"), &mut |_| true) {
        Err(LlmError::Inference(m)) => assert_eq!(m, "the MLX engine stopped"),
        other => panic!("{other:?}"),
    }
    assert!(started.elapsed() < Duration::from_secs(10), "{:?}", started.elapsed());
    let (out, _) = collect(&b, &req("x"));
    assert!(out.is_ok());
    assert_eq!(counter(&o, "loads"), 2);
}

#[test]
fn prompt_progress_keeps_a_slow_worker_alive() {
    let py = need_python!();
    let s = Scratch::new("slowprompt");
    let o = fake(&py, &s, "slowprompt");
    let b = MlxBackend::load(MlxOptions { model_check: ModelCheck::AlreadyPassed, ..o.clone() }).unwrap();
    // Silent for longer than this in total, but never between two messages.
    b.set_silence_timeout(Duration::from_millis(1500));
    let (out, pieces) = collect(&b, &req("a long transcript"));
    assert_eq!(out.unwrap().text, "read it all");
    assert_eq!(pieces, vec!["read ", "it all"]);
    assert_eq!(counter(&o, "loads"), 1, "no restart");
}

#[test]
fn unconstrained_models_refuse_schemas_without_generating() {
    let py = need_python!();
    let s = Scratch::new("unconstrained");
    let o = fake(&py, &s, "unconstrained");
    let b = MlxBackend::load(o.clone()).unwrap();
    assert_eq!(b.model_check_ran(), Some(false), "no constrained check is possible");
    assert!(!b.capabilities().json_schema_constrained);
    assert_eq!(counter(&o, "generates"), 0);
    match b.generate(&schema_req("x", OBJ_SCHEMA), &mut |_| panic!("no tokens")) {
        Err(LlmError::Grammar(m)) => assert_eq!(m, "this model can't be held to the note's format on MLX"),
        other => panic!("{other:?}"),
    }
    assert_eq!(counter(&o, "generates"), 0, "refused before reaching the worker");
    assert!(b.generate(&req("x"), &mut |_| true).is_ok(), "plain text still works");

    // A failed model check also turns schemas off.
    let o = fake(&py, &s, "badcheck");
    let b = MlxBackend::load(o.clone()).unwrap();
    assert_eq!(b.model_check_ran(), Some(false));
    assert!(!b.capabilities().json_schema_constrained);
    assert!(matches!(b.generate(&schema_req("x", OBJ_SCHEMA), &mut |_| true), Err(LlmError::Grammar(_))));
    assert_eq!(counter(&o, "generates"), 1, "only the model check ran");
}

#[test]
fn the_worker_starts_from_an_empty_environment() {
    let py = need_python!();
    let s = Scratch::new("env");
    for device in [MlxDevice::Auto, MlxDevice::Cpu] {
        let mut o = fake(&py, &s, "envcheck");
        o.device = device;
        o.model_check = ModelCheck::AlreadyPassed;
        let b = MlxBackend::load(o.clone()).unwrap();
        let out = b.generate(&req("env"), &mut |_| true).unwrap();
        let v: serde_json::Value = serde_json::from_str(&out.text).unwrap();
        let env = v["env"].as_object().unwrap();
        let keys: BTreeSet<&str> = env.keys().map(String::as_str).collect();
        let mut allowed: BTreeSet<&str> = [
            "PATH",
            "HOME",
            "HF_HOME",
            "HF_HUB_OFFLINE",
            "TRANSFORMERS_OFFLINE",
            "HF_DATASETS_OFFLINE",
            "HF_HUB_DISABLE_TELEMETRY",
            "HF_HUB_DISABLE_IMPLICIT_TOKEN",
            "DO_NOT_TRACK",
            "PYTHONNOUSERSITE",
            "TOKENIZERS_PARALLELISM",
            "NO_PROXY",
            "OPENBARK_MLX_DEVICE",
        ]
        .into_iter()
        .collect();
        // Python sets this itself when the locale is C (PEP 538 locale coercion).
        allowed.insert("LC_CTYPE");
        if cfg!(windows) {
            allowed.insert("SYSTEMROOT");
            // The Microsoft Store's Python sets this for itself at start-up.
            allowed.insert("PYTHONUSERBASE");
        }
        // Windows reports names in upper case; compare that way.
        let keys_upper: BTreeSet<String> = keys.iter().map(|k| k.to_uppercase()).collect();
        let allowed_upper: BTreeSet<String> = allowed.iter().map(|k| k.to_uppercase()).collect();
        let extra: Vec<&String> = keys_upper.difference(&allowed_upper).collect();
        assert!(extra.is_empty(), "unexpected variables: {extra:?}");
        let get =
            |k: &str| env.iter().find(|(n, _)| n.eq_ignore_ascii_case(k)).map(|(_, v)| v.as_str().unwrap());
        assert_eq!(get("PATH"), Some("/usr/bin:/bin"));
        for k in [
            "HF_HUB_OFFLINE",
            "TRANSFORMERS_OFFLINE",
            "HF_DATASETS_OFFLINE",
            "HF_HUB_DISABLE_TELEMETRY",
            "HF_HUB_DISABLE_IMPLICIT_TOKEN",
            "DO_NOT_TRACK",
            "PYTHONNOUSERSITE",
        ] {
            assert_eq!(get(k), Some("1"), "{k}");
        }
        assert_eq!(get("TOKENIZERS_PARALLELISM"), Some("false"));
        assert_eq!(get("NO_PROXY"), Some("*"));
        let hf_home = PathBuf::from(get("HF_HOME").unwrap());
        assert_eq!(hf_home, o.work_dir.join("hf-home"));
        assert!(hf_home.is_dir());
        if device == MlxDevice::Cpu {
            assert_eq!(get("OPENBARK_MLX_DEVICE"), Some("cpu"));
        }
        assert_eq!(v["isolated"], 1);
        assert_eq!(v["dont_write_bytecode"], 1);
        assert_eq!(v["utf8_mode"], 1);
        let cwd = PathBuf::from(v["cwd"].as_str().unwrap());
        assert_eq!(cwd.canonicalize().unwrap(), o.work_dir.canonicalize().unwrap());
    }
}

#[test]
fn drop_shuts_the_worker_down() {
    let py = need_python!();
    let s = Scratch::new("drop");
    let o = fake(&py, &s, "ok");
    let b = MlxBackend::load(o.clone()).unwrap();
    drop(b);
    assert_eq!(std::fs::read_to_string(o.model_dir.join("shutdown")).unwrap(), "shutdown");

    // A worker that ignores shutdown is killed after 2 s.
    let o = fake(&py, &s, "stubborn");
    let b = MlxBackend::load(MlxOptions { model_check: ModelCheck::AlreadyPassed, ..o }).unwrap();
    let started = Instant::now();
    drop(b);
    let took = started.elapsed();
    assert!(took >= Duration::from_millis(1900) && took < Duration::from_secs(6), "{took:?}");
}

#[test]
fn load_refuses_a_missing_or_relative_model_folder() {
    let py = need_python!();
    let s = Scratch::new("missing");
    let mut o = fake(&py, &s, "ok");
    o.model_dir = s.path().join("nowhere");
    assert!(matches!(MlxBackend::load(o.clone()), Err(LlmError::ModelNotFound(_))));
    o.model_dir = PathBuf::from("relative/model");
    assert!(matches!(MlxBackend::load(o), Err(LlmError::ModelNotFound(_))));
}

#[test]
fn a_python_that_doesnt_start_is_a_load_error() {
    let s = Scratch::new("nopython");
    let mut o =
        MlxOptions::new(s.path().join("no-python-here"), s.path().join("work"), s.path().to_path_buf(), "x");
    o.sandbox = false;
    o.worker_script = Some(fake_script());
    match MlxBackend::load(o) {
        Err(LlmError::Load(m)) => assert!(m.contains("didn't start"), "{m}"),
        other => panic!("{:?}", other.err()),
    }
}

#[test]
fn write_worker_writes_once_and_repairs() {
    let s = Scratch::new("write");
    let p = write_worker(s.path()).unwrap();
    assert_eq!(p, s.path().join("worker").join("openbark_mlx_worker.py"));
    assert_eq!(std::fs::read(&p).unwrap(), WORKER_SOURCE.as_bytes());
    // Identical bytes: not rewritten (a read-only copy proves it).
    let mut perm = std::fs::metadata(&p).unwrap().permissions();
    perm.set_readonly(true);
    std::fs::set_permissions(&p, perm.clone()).unwrap();
    assert_eq!(write_worker(s.path()).unwrap(), p);
    #[allow(clippy::permissions_set_readonly_false)]
    perm.set_readonly(false);
    std::fs::set_permissions(&p, perm).unwrap();
    // A stale or edited copy is replaced.
    std::fs::write(&p, b"print('old worker')\n").unwrap();
    write_worker(s.path()).unwrap();
    assert_eq!(std::fs::read(&p).unwrap(), WORKER_SOURCE.as_bytes());
    let leftovers: Vec<_> = std::fs::read_dir(p.parent().unwrap()).unwrap().flatten().collect();
    assert_eq!(leftovers.len(), 1, "no temp files left behind");
}

// ---------------------------------------------------------------- real worker, plain Python

/// The real worker on a Python without MLX: the protocol works, the network
/// guard holds, and the check reports the missing packages plainly.
#[test]
fn real_worker_on_plain_python_reports_missing_packages() {
    let py = need_python!();
    let has_mlx = Command::new(&py)
        .args(["-I", "-c", "import mlx.core"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if has_mlx {
        println!("skipped: this Python has MLX (the real-worker tests cover it)");
        return;
    }
    let s = Scratch::new("plain");
    let check = MlxBackend::pack_check(&py, s.path(), MlxDevice::Cpu, false).unwrap();
    assert!(!check.ok);
    assert_eq!(check.device, "cpu");
    assert!(!check.metal);
    assert!(check.versions.contains_key("python"));
    let get = |name: &str| check.checks.iter().find(|(n, _)| n == name).map(|(_, ok)| *ok);
    assert_eq!(get("imports"), Some(false));
    assert_eq!(get("socket_guard"), Some(true), "{:?}", check.checks);
    assert_eq!(get("metal"), Some(true), "Metal isn't needed off a Mac or on the CPU device");
    assert_eq!(get("matmul"), Some(false));
    assert_eq!(get("mask"), Some(false));
    let msg = check.message.unwrap();
    assert!(msg.contains("packages didn't load"), "{msg}");
    assert!(s.path().join("worker").join("openbark_mlx_worker.py").is_file());
    assert!(s.path().join("hf-home").is_dir());
}

/// The real worker refuses to start unless the Hugging Face libraries are offline.
#[test]
fn real_worker_refuses_to_start_online() {
    let py = need_python!();
    let s = Scratch::new("online");
    let script = write_worker(s.path()).unwrap();
    let mut cmd = Command::new(&py);
    cmd.args(["-I", "-B", "-X", "utf8", "-u"]).arg(&script).env_clear();
    if let Some(v) = std::env::var_os("SYSTEMROOT") {
        cmd.env("SYSTEMROOT", v);
    }
    let out = cmd.stdin(Stdio::null()).output().unwrap();
    assert_eq!(out.status.code(), Some(3));
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8_lossy(&out.stderr).contains("HF_HUB_OFFLINE=1"));
}

// ---------------------------------------------------------------- real worker, MLX pack

/// `OPENBARK_MLX_TEST_PYTHON` and `OPENBARK_MLX_TEST_MODEL`, both existing.
fn real_env() -> Option<(PathBuf, PathBuf)> {
    let py = PathBuf::from(std::env::var_os("OPENBARK_MLX_TEST_PYTHON")?);
    let model = PathBuf::from(std::env::var_os("OPENBARK_MLX_TEST_MODEL")?);
    assert!(py.is_file(), "OPENBARK_MLX_TEST_PYTHON={} isn't a file", py.display());
    assert!(
        model.join("config.json").is_file(),
        "OPENBARK_MLX_TEST_MODEL={} has no config.json",
        model.display()
    );
    Some((py, model))
}

/// `OPENBARK_MLX_TEST_REQUIRED=1`: a real-worker test may not skip.
fn real_required() -> bool {
    std::env::var("OPENBARK_MLX_TEST_REQUIRED").is_ok_and(|v| v.trim() == "1")
}

macro_rules! need_real {
    () => {
        match real_env() {
            Some(v) => v,
            None => {
                assert!(
                    !real_required(),
                    "OPENBARK_MLX_TEST_REQUIRED=1, but OPENBARK_MLX_TEST_PYTHON and OPENBARK_MLX_TEST_MODEL aren't set"
                );
                println!("skipped: set OPENBARK_MLX_TEST_PYTHON and OPENBARK_MLX_TEST_MODEL (scripts/mlx-test-env.sh)");
                return;
            }
        }
    };
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

#[test]
fn real_pack_check_passes_on_the_cpu() {
    let (py, _) = need_real!();
    let s = Scratch::new("real-pack");
    let check = MlxBackend::pack_check(&py, s.path(), MlxDevice::Cpu, true).unwrap();
    println!("{check:?}");
    assert!(check.ok, "{:?}", check.message);
    assert_eq!(check.device, "cpu");
    let names: Vec<&str> = check.checks.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["imports", "socket_guard", "metal", "matmul", "mask"]);
    assert!(check.checks.iter().all(|(_, ok)| *ok));
    for k in ["python", "mlx", "mlx_lm", "transformers", "llguidance"] {
        assert!(check.versions.contains_key(k), "{k}");
    }
    assert_eq!(check.versions["mlx_lm"], "0.32.0");
    assert_eq!(check.versions["llguidance"], "1.9.1");
    assert!(check.message.is_none());
}

const TRANSCRIPT: &str = "[0] Vet: This is Bella, a six year old spayed female spaniel.\n\
[1] Owner: She vomited twice since yesterday and is eating less.\n\
[2] Vet: She weighs 21.4 kilograms. Temperature 38.6. Mild dehydration, abdomen soft, no pain.\n\
[3] Vet: I'll give maropitant by injection today, bland diet for three days, and recheck in 48 hours if she isn't better.";

#[test]
fn real_worker_drafts_a_soap_note_held_to_its_schema() {
    let (py, model) = need_real!();
    let s = Scratch::new("real-draft");
    let mut o = MlxOptions::new(py, s.path().to_path_buf(), model, "mlx-community/Qwen3-0.6B-4bit");
    o.device = MlxDevice::Cpu;
    let started = Instant::now();
    let b = MlxBackend::load(o).unwrap();
    println!("loaded with the model check in {:?}", started.elapsed());
    assert_eq!(b.model_check_ran(), Some(true), "the model check passes");
    let caps = b.capabilities();
    assert!(caps.json_schema_constrained);
    assert_eq!(caps.backend, "mlx");
    assert_eq!(caps.context_length, 32768, "40960 capped by max_context");

    // Facts held to a small, bounded schema at temperature 0: the transcript's
    // values come through the constraint.
    let facts_schema = serde_json::json!({
        "type": "object",
        "properties": {
            "patient_name": {"type": "string", "maxLength": 40},
            "weight_kg": {"type": "number"},
            "problems": {"type": "array", "items": {"type": "string", "maxLength": 80}, "maxItems": 3},
            "plan": {"type": "array", "items": {"type": "string", "maxLength": 80}, "maxItems": 3}
        },
        "required": ["patient_name", "weight_kg", "problems", "plan"],
        "additionalProperties": false
    });
    let system = "You write short veterinary SOAP notes as JSON. Use the transcript segment numbers as evidence.                   Keep every text to a few words. Use null or empty lists for anything not discussed.";
    let user = format!(
        "Transcript:
{TRANSCRIPT}

Write the note."
    );
    let mut facts = GenerateRequest::new(vec![ChatMessage::system(system), ChatMessage::user(user.clone())]);
    facts.json_schema = Some(facts_schema.to_string());
    facts.max_tokens = 200;
    facts.temperature = 0.0;
    let started = Instant::now();
    let out = b.generate(&facts, &mut |_| true).unwrap();
    println!("facts in {:?}: {}", started.elapsed(), out.text);
    assert!(!out.stats.hit_max_tokens);
    let v: serde_json::Value = serde_json::from_str(&out.text).expect("the facts are JSON");
    assert!(jsonschema::validator_for(&facts_schema).unwrap().is_valid(&v), "{v}");
    assert!(v["patient_name"].as_str().unwrap().contains("Bella"), "{v}");
    assert!((v["weight_kg"].as_f64().unwrap() - 21.4).abs() < 1e-9, "{v}");
    assert!(!v["problems"].as_array().unwrap().is_empty(), "{v}");

    // The SOAP template's own schema, with the settings notes use
    // (temperature 0.2, seed 42): the draft finishes and validates.
    let schema_path = repo_root().join("packages/templates/soap/schema.json");
    let schema_text = std::fs::read_to_string(&schema_path).unwrap();
    let schema: serde_json::Value = serde_json::from_str(&schema_text).unwrap();
    let mut r = GenerateRequest::new(vec![ChatMessage::system(system), ChatMessage::user(user)]);
    r.json_schema = Some(schema_text.clone());
    r.max_tokens = 1200;
    let started = Instant::now();
    let mut tokens = 0usize;
    let out = b
        .generate(&r, &mut |_| {
            tokens += 1;
            true
        })
        .unwrap();
    println!(
        "draft: {} tokens in {:?} ({:.1} tok/s), hit_max_tokens={}",
        out.stats.completion_tokens,
        started.elapsed(),
        out.stats.tokens_per_second,
        out.stats.hit_max_tokens
    );
    assert!(tokens > 10);
    assert!(!out.stats.hit_max_tokens, "the note finished before max_tokens");
    let note: serde_json::Value = serde_json::from_str(&out.text).expect("the draft is JSON");
    let validator = jsonschema::validator_for(&schema).unwrap();
    let errors: Vec<String> = validator.iter_errors(&note).map(|e| e.to_string()).collect();
    assert!(errors.is_empty(), "the draft breaks the schema: {errors:?}");
    println!("{note}");

    // Cancel mid-draft: stops at once, and the worker carries on.
    let mut n = 0;
    let mut stopped_at = None;
    let out = b
        .generate(&r, &mut |_| {
            n += 1;
            if n < 5 {
                return true;
            }
            stopped_at = Some(Instant::now());
            false
        })
        .unwrap();
    let after_stop = stopped_at.unwrap().elapsed();
    println!("cancelled after 5 tokens; returned {after_stop:?} later");
    assert_eq!(n, 5);
    assert!(!out.stats.hit_max_tokens);
    assert!(after_stop < Duration::from_secs(10), "{after_stop:?}");
    let mut small = GenerateRequest::new(vec![ChatMessage::user("Say hello.")]);
    small.max_tokens = 4;
    let out = b.generate(&small, &mut |_| true).unwrap();
    assert!(out.stats.completion_tokens <= 4);
}

/// Golden: template schemas compile in llguidance over the test model's
/// tokenizer, with no errors (ADR-0025 decision 3). Covers every
/// `packages/templates/*/schema.json`, plus every `*.json` in the folder named
/// by `OPENBARK_MLX_TEST_SCHEMAS` (schemas os-engine generates: the other
/// built-in templates, the edit and from-example schemas).
#[test]
fn golden_template_schemas_compile_in_llguidance() {
    let (py, model) = need_real!();
    let mut schemas: Vec<PathBuf> = std::fs::read_dir(repo_root().join("packages/templates"))
        .unwrap()
        .flatten()
        .map(|e| e.path().join("schema.json"))
        .filter(|p| p.is_file())
        .collect();
    assert!(!schemas.is_empty(), "no packages/templates/*/schema.json");
    if let Some(extra) = std::env::var_os("OPENBARK_MLX_TEST_SCHEMAS") {
        let extra: Vec<PathBuf> = std::fs::read_dir(&extra)
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "json"))
            .collect();
        assert!(!extra.is_empty(), "OPENBARK_MLX_TEST_SCHEMAS holds no .json files");
        schemas.extend(extra);
    }
    schemas.sort();
    let code = r#"
import json, sys
import llguidance, llguidance.hf
from transformers import AutoTokenizer
tok = AutoTokenizer.from_pretrained(sys.argv[1], trust_remote_code=False)
llt = llguidance.hf.from_tokenizer(tok)
bad = []
for p in sys.argv[2:]:
    with open(p, encoding="utf-8") as f:
        schema = json.load(f)
    g = llguidance.LLMatcher.grammar_from_json_schema(schema, defaults={"whitespace_flexible": False})
    is_err, msgs = llguidance.LLMatcher.validate_grammar_with_warnings(g, llt)
    m = llguidance.LLMatcher(llt, g, log_level=0)
    if is_err or m.is_error():
        bad.append([p, msgs, m.get_error()])
    else:
        print("ok", p, "warnings:", len(msgs))
print(json.dumps(bad))
"#;
    let out = Command::new(&py)
        .arg("-I")
        .arg("-c")
        .arg(code)
        .arg(&model)
        .args(&schemas)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HF_HUB_OFFLINE", "1")
        .env("TRANSFORMERS_OFFLINE", "1")
        .env("HF_HOME", std::env::temp_dir().join("openbark-mlx-tests-hf-home"))
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    println!("{stdout}");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let last = stdout.lines().last().unwrap();
    assert_eq!(last, "[]", "schemas llguidance can't compile: {last}");
    assert_eq!(stdout.lines().filter(|l| l.starts_with("ok ")).count(), schemas.len());
}

/// The real worker caps the context at `max_context`, below the model's own.
#[test]
fn real_worker_enforces_max_context() {
    let (py, model) = need_real!();
    let s = Scratch::new("real-maxctx");
    let mut o = MlxOptions::new(py, s.path().to_path_buf(), model, "mlx-community/Qwen3-0.6B-4bit");
    o.device = MlxDevice::Cpu;
    o.max_context = 64;
    o.model_check = ModelCheck::AlreadyPassed;
    let b = MlxBackend::load(o).unwrap();
    assert_eq!(b.capabilities().context_length, 64);
    let mut r = GenerateRequest::new(vec![ChatMessage::user("Say hello.")]);
    r.max_tokens = 100;
    match b.generate(&r, &mut |_| true) {
        Err(LlmError::ContextOverflow { max_new: 100, ctx: 64, .. }) => {}
        other => panic!("{other:?}"),
    }
    r.max_tokens = 4;
    assert!(b.generate(&r, &mut |_| true).is_ok());
}

/// The socket guard holds inside the real pack even with a network available.
#[test]
fn real_worker_socket_guard_holds() {
    let (py, _) = need_real!();
    let s = Scratch::new("real-guard");
    let check = MlxBackend::pack_check(&py, s.path(), MlxDevice::Cpu, false).unwrap();
    assert!(check.checks.contains(&("socket_guard".to_string(), true)), "{:?}", check.checks);
}
