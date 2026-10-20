//! MLX models on Apple silicon (ADR-0025 decisions 2, 3 and 6).
//!
//! [`MlxBackend`] runs mlx-lm in a private Python worker: the MLX engine
//! pack's own Python, running the single-file worker compiled in here
//! ([`WORKER_SOURCE`]). The worker talks only over its standard input and
//! output, one JSON object per line (contract 9 in
//! `docs/plans/mlx-models.md`). Its own code opens no socket and binds no
//! port, and it is kept off the network by:
//!
//! - an empty environment with the Hugging Face libraries set offline
//!   (`HF_HUB_OFFLINE=1` and friends, no proxy variables, no token), and
//!   only ever a local folder to load, never a repo id;
//! - on macOS, `sandbox-exec` with a profile that denies all network access
//!   at the OS level (`OPENBARK_MLX_SANDBOX=0` turns it off, for support);
//! - a best-effort guard in its first lines: Python's `socket.socket`
//!   methods `connect`, `connect_ex`, `sendto`, `sendmsg`, `bind` and
//!   `listen`, and `socket.create_connection` and `socket.getaddrinfo`,
//!   raise. It doesn't cover the low-level `_socket` module or native code
//!   in the pack's libraries; on macOS the sandbox does. Linux test mode
//!   (developers and CI only) has no OS sandbox.
//!
//! Patient text reaches the worker only through its stdin, and nothing here
//! logs prompts or generated text: the worker's stderr carries metadata only
//! and is logged truncated.
//!
//! JSON Schema output is enforced in the worker by llguidance. It is reported
//! as available ([`LlmCapabilities::json_schema_constrained`]) only when the
//! worker built llguidance for this model and the model passed its check (a
//! short constrained generation). Otherwise a request with a schema is refused
//! without generating: there is no unconstrained fallback for notes.

use std::collections::{BTreeMap, VecDeque};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::{GenerateOutput, GenerateRequest, GenerateStats, LlmBackend, LlmCapabilities, LlmError};

/// The worker script, written into the pack folder by [`write_worker`].
pub const WORKER_SOURCE: &str = include_str!("mlx_worker.py");
/// The worker protocol version (`hello`).
pub const PROTOCOL: u32 = 1;

const WORKER_DIR: &str = "worker";
const WORKER_FILE: &str = "openbark_mlx_worker.py";
const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";
const SANDBOX_PROFILE: &str = "(version 1)(allow default)(deny network*)";

/// No message for this long during a generation kills the worker.
const SILENCE: Duration = Duration::from_secs(120);
const HELLO_TIMEOUT: Duration = Duration::from_secs(120);
const SELFTEST_TIMEOUT: Duration = Duration::from_secs(300);
const LOAD_TIMEOUT: Duration = Duration::from_secs(900);
const SHUTDOWN_WAIT: Duration = Duration::from_secs(2);
const STDERR_CUT: usize = 300;
const STDERR_TAIL: usize = 20;

const STOPPED: &str = "the MLX engine stopped";
const NOT_CONSTRAINED: &str = "this model can't be held to the note's format on MLX";
const MODEL_CHECK_SCHEMA: &str =
    r#"{"type":"object","properties":{"ok":{"type":"boolean"}},"required":["ok"]}"#;

/// Which device MLX runs on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MlxDevice {
    /// Metal when available, else the CPU.
    Auto,
    /// MLX's CPU device (tests, and Mac runners without usable Metal).
    Cpu,
}

/// Whether [`MlxBackend::load`] runs the model check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelCheck {
    Run,
    /// The app's `checks.json` says this model already passed on this Mac.
    AlreadyPassed,
}

#[derive(Debug, Clone)]
pub struct MlxOptions {
    /// The pack's python3 (tests: any python3 with mlx, or one for the fake worker).
    pub python: PathBuf,
    /// The pack folder: the worker is written to `<work_dir>/worker/openbark_mlx_worker.py`,
    /// `HF_HOME` is `<work_dir>/hf-home` (empty), and the worker runs in it.
    pub work_dir: PathBuf,
    /// Absolute folder holding the model's `config.json`.
    pub model_dir: PathBuf,
    /// For `capabilities().model_name`.
    pub model_name: String,
    pub device: MlxDevice,
    /// macOS: run the worker under `/usr/bin/sandbox-exec` with network access denied.
    pub sandbox: bool,
    pub model_check: ModelCheck,
    /// A cap on the model's own context length.
    pub max_context: u32,
    /// Tests only: run this script instead of the embedded worker.
    pub worker_script: Option<PathBuf>,
}

impl MlxOptions {
    /// Options with the defaults: Metal when available, sandboxed, model
    /// check run, context capped at 32768, the embedded worker.
    pub fn new(
        python: PathBuf,
        work_dir: PathBuf,
        model_dir: PathBuf,
        model_name: impl Into<String>,
    ) -> Self {
        Self {
            python,
            work_dir,
            model_dir,
            model_name: model_name.into(),
            device: MlxDevice::Auto,
            sandbox: true,
            model_check: ModelCheck::Run,
            max_context: 32768,
            worker_script: None,
        }
    }
}

/// Write [`WORKER_SOURCE`] to `<work_dir>/worker/openbark_mlx_worker.py`
/// unless identical bytes are already there. Returns the script's path.
pub fn write_worker(work_dir: &Path) -> std::io::Result<PathBuf> {
    let dir = work_dir.join(WORKER_DIR);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(WORKER_FILE);
    if std::fs::read(&path).is_ok_and(|b| b == WORKER_SOURCE.as_bytes()) {
        return Ok(path);
    }
    let tmp = dir.join(format!("{WORKER_FILE}.{}.tmp", std::process::id()));
    std::fs::write(&tmp, WORKER_SOURCE.as_bytes())?;
    if let Err(e) = std::fs::rename(&tmp, &path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(path)
}

/// The result of the pack check (ADR-0025 decision 6).
#[derive(Debug, Clone, Serialize)]
pub struct PackCheck {
    pub ok: bool,
    /// "gpu" or "cpu".
    pub device: String,
    pub metal: bool,
    /// python, mlx, mlx_lm, transformers, llguidance (and tokenizers when known).
    pub versions: BTreeMap<String, String>,
    /// imports, socket_guard, metal, matmul, mask.
    pub checks: Vec<(String, bool)>,
    /// The first failure, in plain words.
    pub message: Option<String>,
}

// ---------------------------------------------------------------- protocol

#[derive(Serialize, Debug, Clone, PartialEq)]
struct WireMessage<'a> {
    role: &'a str,
    content: &'a str,
}

/// A request to the worker. Not `Debug`: a generate request holds patient text.
#[derive(Serialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Request<'a> {
    Hello {
        protocol: u32,
    },
    Selftest {
        id: u64,
    },
    Load {
        id: u64,
        path: &'a str,
        /// The cap on the prompt plus new tokens (`MlxOptions::max_context`);
        /// the worker refuses past `min(model's own context, this)`.
        max_context: u32,
    },
    Generate {
        id: u64,
        messages: Vec<WireMessage<'a>>,
        max_tokens: u32,
        temperature: f32,
        seed: u32,
        json_schema: Option<&'a str>,
    },
    Cancel {
        id: u64,
    },
    #[allow(dead_code)]
    Unload {
        id: u64,
    },
    Shutdown,
}

#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Finish {
    Stop,
    Length,
    Cancelled,
    #[serde(other)]
    Other,
}

#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum ErrorKind {
    BadRequest,
    Load,
    UnsupportedArchitecture,
    Template,
    Grammar,
    ContextOverflow,
    Inference,
    #[serde(other)]
    Other,
}

#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(default)]
struct WireStats {
    prompt_tokens: u32,
    completion_tokens: u32,
    time_to_first_token_ms: u64,
    total_ms: u64,
    tokens_per_second: f64,
    hit_max_tokens: bool,
    peak_memory_bytes: u64,
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "event", rename_all = "snake_case")]
enum Event {
    Hello {
        protocol: u32,
        python: String,
        #[serde(default)]
        versions: BTreeMap<String, String>,
        device: String,
        #[serde(default)]
        metal: bool,
    },
    Selftest {
        #[serde(default)]
        id: Option<u64>,
        ok: bool,
        #[serde(default)]
        checks: Vec<(String, bool)>,
        #[serde(default)]
        message: Option<String>,
    },
    Loaded {
        #[serde(default)]
        id: Option<u64>,
        model_type: String,
        context_length: u32,
        #[serde(default)]
        eos_token_ids: Vec<u32>,
        #[serde(default)]
        vocab_size: u32,
        constrained_ready: bool,
        #[serde(default)]
        load_ms: u64,
    },
    /// Reading the prompt (contract change: keeps the silence timer from
    /// stopping a worker that is still reading a long prompt on the CPU).
    Progress {
        #[serde(default)]
        id: Option<u64>,
        #[serde(default)]
        prompt_done: u32,
        #[serde(default)]
        prompt_total: u32,
    },
    Token {
        #[serde(default)]
        id: Option<u64>,
        text: String,
    },
    Done {
        #[serde(default)]
        id: Option<u64>,
        text: String,
        finish: Finish,
        #[serde(default)]
        stats: WireStats,
    },
    Unloaded {
        #[serde(default)]
        id: Option<u64>,
    },
    Error {
        #[serde(default)]
        id: Option<u64>,
        kind: ErrorKind,
        #[serde(default)]
        message: String,
        #[serde(default)]
        prompt_tokens: u32,
        #[serde(default)]
        max_new: u32,
        #[serde(default)]
        context: u32,
    },
}

impl Event {
    /// The request this event answers (`None` for hello and id-less errors).
    fn id(&self) -> Option<u64> {
        match self {
            Event::Hello { .. } => None,
            Event::Selftest { id, .. }
            | Event::Loaded { id, .. }
            | Event::Progress { id, .. }
            | Event::Token { id, .. }
            | Event::Done { id, .. }
            | Event::Unloaded { id }
            | Event::Error { id, .. } => *id,
        }
    }

    /// For this request: its own id, or an error without one.
    fn answers(&self, id: u64) -> bool {
        match self.id() {
            Some(i) => i == id,
            None => matches!(self, Event::Error { .. }),
        }
    }
}

/// A worker error, as the engine sees it (contract 9's mapping).
fn map_error(kind: ErrorKind, message: String, prompt: u32, max_new: u32, ctx: u32) -> LlmError {
    match kind {
        ErrorKind::Load | ErrorKind::UnsupportedArchitecture => LlmError::Load(message),
        ErrorKind::Template => LlmError::Template(message),
        ErrorKind::Grammar => LlmError::Grammar(message),
        ErrorKind::ContextOverflow => LlmError::ContextOverflow { prompt, max_new, ctx },
        ErrorKind::BadRequest | ErrorKind::Inference | ErrorKind::Other => LlmError::Inference(message),
    }
}

fn error_of(ev: Event) -> LlmError {
    match ev {
        Event::Error { kind, message, prompt_tokens, max_new, context, .. } => {
            map_error(kind, message, prompt_tokens, max_new, context)
        }
        _ => LlmError::Inference("the MLX engine answered out of turn".into()),
    }
}

// ---------------------------------------------------------------- worker process

enum Line {
    Event(Box<Event>),
    Eof,
}

#[derive(Debug)]
enum Recv {
    Eof,
    Timeout,
}

#[derive(Clone)]
struct Spawn {
    python: PathBuf,
    work_dir: PathBuf,
    script: PathBuf,
    device: MlxDevice,
    sandbox: bool,
}

fn env_flag_off(name: &str) -> bool {
    std::env::var(name).is_ok_and(|v| v.trim() == "0")
}

struct Worker {
    child: Child,
    stdin: Option<ChildStdin>,
    events: Receiver<Line>,
    stderr_tail: Arc<Mutex<VecDeque<String>>>,
    next_id: u64,
    closed: bool,
}

impl Worker {
    fn spawn(s: &Spawn) -> Result<Self, LlmError> {
        let hf_home = s.work_dir.join("hf-home");
        std::fs::create_dir_all(&hf_home)
            .map_err(|e| LlmError::Load(format!("couldn't prepare the MLX engine's folder: {e}")))?;
        let worker_args = |cmd: &mut Command| {
            cmd.arg("-I").arg("-B").arg("-X").arg("utf8").arg("-u").arg(&s.script);
        };
        let wants_sandbox = cfg!(target_os = "macos") && s.sandbox && !env_flag_off("OPENBARK_MLX_SANDBOX");
        let mut cmd = if wants_sandbox && Path::new(SANDBOX_EXEC).exists() {
            let mut c = Command::new(SANDBOX_EXEC);
            c.arg("-p").arg(SANDBOX_PROFILE).arg(&s.python);
            worker_args(&mut c);
            c
        } else {
            if wants_sandbox {
                tracing::warn!(
                    "sandbox-exec is missing: the MLX worker runs without the OS network sandbox (the offline \
                     environment and the worker's Python socket guard stay)"
                );
            }
            let mut c = Command::new(&s.python);
            worker_args(&mut c);
            c
        };
        cmd.env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HF_HOME", &hf_home)
            .env("HF_HUB_OFFLINE", "1")
            .env("TRANSFORMERS_OFFLINE", "1")
            .env("HF_DATASETS_OFFLINE", "1")
            .env("HF_HUB_DISABLE_TELEMETRY", "1")
            .env("HF_HUB_DISABLE_IMPLICIT_TOKEN", "1")
            .env("DO_NOT_TRACK", "1")
            .env("PYTHONNOUSERSITE", "1")
            .env("TOKENIZERS_PARALLELISM", "false")
            .env("NO_PROXY", "*");
        if let Some(home) = std::env::var_os("HOME") {
            cmd.env("HOME", home);
        }
        match s.device {
            MlxDevice::Cpu => {
                cmd.env("OPENBARK_MLX_DEVICE", "cpu");
            }
            MlxDevice::Auto => {
                if let Some(v) = std::env::var_os("OPENBARK_MLX_DEVICE") {
                    cmd.env("OPENBARK_MLX_DEVICE", v);
                }
            }
        }
        #[cfg(windows)]
        if let Some(v) = std::env::var_os("SYSTEMROOT") {
            // Python won't start on Windows without it (fake-worker tests only).
            cmd.env("SYSTEMROOT", v);
        }
        cmd.current_dir(&s.work_dir).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child =
            cmd.spawn().map_err(|e| LlmError::Load(format!("the MLX engine's Python didn't start: {e}")))?;
        let stdin = child.stdin.take();
        let stdout = child.stdout.take().ok_or_else(|| LlmError::Load("no pipe to the MLX engine".into()))?;
        let stderr = child.stderr.take().ok_or_else(|| LlmError::Load("no pipe to the MLX engine".into()))?;

        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("mlx-worker-out".into())
            .spawn(move || read_events(stdout, tx))
            .map_err(|e| LlmError::Load(format!("couldn't start reading the MLX engine: {e}")))?;
        let tail = Arc::new(Mutex::new(VecDeque::new()));
        let tail2 = Arc::clone(&tail);
        std::thread::Builder::new()
            .name("mlx-worker-err".into())
            .spawn(move || read_stderr(stderr, tail2))
            .map_err(|e| LlmError::Load(format!("couldn't start reading the MLX engine: {e}")))?;
        Ok(Self { child, stdin, events: rx, stderr_tail: tail, next_id: 1, closed: false })
    }

    fn next_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn send(&mut self, req: &Request) -> Result<(), Recv> {
        let mut line = serde_json::to_string(req).map_err(|_| Recv::Eof)?;
        line.push('\n');
        let stdin = self.stdin.as_mut().ok_or(Recv::Eof)?;
        stdin.write_all(line.as_bytes()).and_then(|_| stdin.flush()).map_err(|_| Recv::Eof)
    }

    fn recv(&mut self, timeout: Duration) -> Result<Event, Recv> {
        match self.events.recv_timeout(timeout) {
            Ok(Line::Event(ev)) => Ok(*ev),
            Ok(Line::Eof) | Err(RecvTimeoutError::Disconnected) => Err(Recv::Eof),
            Err(RecvTimeoutError::Timeout) => Err(Recv::Timeout),
        }
    }

    /// Wait for the event answering `id`, ignoring anything else.
    fn wait(&mut self, id: u64, timeout: Duration) -> Result<Event, Recv> {
        let deadline = Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(Recv::Timeout);
            }
            let ev = self.recv(left)?;
            if ev.answers(id) {
                return Ok(ev);
            }
        }
    }

    /// The last lines the worker wrote to stderr, for start-up failures only
    /// (no request has been sent then, so they can't hold patient text).
    fn startup_reason(&self) -> String {
        // Give the stderr reader a moment to catch the last lines.
        std::thread::sleep(Duration::from_millis(100));
        let tail = self.stderr_tail.lock().map(|t| t.iter().rev().find(|l| !l.trim().is_empty()).cloned());
        match tail {
            Ok(Some(line)) => format!(" ({})", line.trim()),
            _ => String::new(),
        }
    }

    fn kill(&mut self) {
        if self.closed {
            return;
        }
        self.closed = true;
        self.stdin = None;
        let _ = self.child.kill();
        let _ = self.child.wait();
    }

    /// Send shutdown, close stdin, wait up to 2 s, then kill.
    fn shutdown(&mut self) {
        if self.closed {
            return;
        }
        let _ = self.send(&Request::Shutdown);
        self.stdin = None;
        let deadline = Instant::now() + SHUTDOWN_WAIT;
        while Instant::now() < deadline {
            match self.child.try_wait() {
                Ok(Some(_)) => {
                    self.closed = true;
                    return;
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(20)),
                Err(_) => break,
            }
        }
        tracing::debug!("the MLX worker didn't stop within 2 s; killing it");
        self.kill();
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn read_line_bytes(r: &mut impl BufRead, buf: &mut Vec<u8>) -> bool {
    buf.clear();
    matches!(r.read_until(b'\n', buf), Ok(n) if n > 0)
}

fn read_events(stdout: impl Read, tx: Sender<Line>) {
    let mut r = BufReader::new(stdout);
    let mut buf = Vec::new();
    while read_line_bytes(&mut r, &mut buf) {
        let text = String::from_utf8_lossy(&buf);
        let line = text.trim();
        if line.is_empty() {
            continue;
        }
        match serde_json::from_str::<Event>(line) {
            Ok(ev) => {
                if tx.send(Line::Event(Box::new(ev))).is_err() {
                    return;
                }
            }
            // Never log the line itself: it may hold patient text.
            Err(_) => {
                tracing::debug!(target: "mlx_worker", bytes = line.len(), "ignored an unreadable line from the MLX worker")
            }
        }
    }
    let _ = tx.send(Line::Eof);
}

fn cut(line: &str, max: usize) -> String {
    match line.char_indices().nth(max) {
        Some((i, _)) => format!("{}…", &line[..i]),
        None => line.to_string(),
    }
}

fn read_stderr(stderr: impl Read, tail: Arc<Mutex<VecDeque<String>>>) {
    let mut r = BufReader::new(stderr);
    let mut buf = Vec::new();
    while read_line_bytes(&mut r, &mut buf) {
        let text = String::from_utf8_lossy(&buf);
        let line = cut(text.trim_end(), STDERR_CUT);
        tracing::debug!(target: "mlx_worker", "{line}");
        if let Ok(mut t) = tail.lock() {
            if t.len() == STDERR_TAIL {
                t.pop_front();
            }
            t.push_back(line);
        }
    }
}

struct Hello {
    python: String,
    versions: BTreeMap<String, String>,
    device: String,
    metal: bool,
}

fn hello(w: &mut Worker) -> Result<Hello, LlmError> {
    let started = w.send(&Request::Hello { protocol: PROTOCOL }).is_ok();
    let reply = if started { w.recv(HELLO_TIMEOUT) } else { Err(Recv::Eof) };
    match reply {
        Ok(Event::Hello { protocol, python, versions, device, metal }) => {
            if protocol != PROTOCOL {
                return Err(LlmError::Load(format!(
                    "the MLX engine speaks protocol {protocol}, and OpenBark needs {PROTOCOL}"
                )));
            }
            tracing::info!(python = %python, ?versions, device = %device, metal, "MLX worker started");
            Ok(Hello { python, versions, device, metal })
        }
        Ok(ev @ Event::Error { .. }) => Err(match error_of(ev) {
            LlmError::Inference(m) => LlmError::Load(m),
            other => other,
        }),
        Ok(_) => Err(LlmError::Load("the MLX engine didn't say hello".into())),
        Err(Recv::Timeout) => {
            w.kill();
            Err(LlmError::Load("the MLX engine took too long to start".into()))
        }
        Err(Recv::Eof) => Err(LlmError::Load(format!("the MLX engine didn't start{}", w.startup_reason()))),
    }
}

#[derive(Debug, Clone)]
struct ModelInfo {
    model_type: String,
    context_length: u32,
    constrained_ready: bool,
}

fn load_model(w: &mut Worker, model_dir: &Path, max_context: u32) -> Result<ModelInfo, LlmError> {
    let path = model_dir.to_string_lossy().into_owned();
    let id = w.next_id();
    if w.send(&Request::Load { id, path: &path, max_context }).is_err() {
        return Err(LlmError::Load(STOPPED.into()));
    }
    match w.wait(id, LOAD_TIMEOUT) {
        Ok(Event::Loaded {
            model_type,
            context_length,
            eos_token_ids,
            vocab_size,
            constrained_ready,
            load_ms,
            ..
        }) => {
            tracing::info!(
                model_type = %model_type,
                context_length,
                vocab_size,
                eos_tokens = eos_token_ids.len(),
                constrained_ready,
                load_ms,
                "MLX model loaded"
            );
            Ok(ModelInfo { model_type, context_length, constrained_ready })
        }
        Ok(ev @ Event::Error { .. }) => Err(match error_of(ev) {
            LlmError::Inference(m) => LlmError::Load(m),
            other => other,
        }),
        Ok(_) => Err(LlmError::Load("the MLX engine answered out of turn".into())),
        Err(Recv::Timeout) => {
            w.kill();
            Err(LlmError::Load("the MLX engine took too long to load the model".into()))
        }
        Err(Recv::Eof) => Err(LlmError::Load(STOPPED.into())),
    }
}

/// Run one generation on `w`. `Err(Ok(e))` is a request error (the worker
/// is fine); `Err(Err(_))` means the worker died or went silent.
fn run_generate(
    w: &mut Worker,
    request: &GenerateRequest,
    silence: Duration,
    on_token: &mut dyn FnMut(&str) -> bool,
) -> Result<GenerateOutput, Result<LlmError, Recv>> {
    let id = w.next_id();
    let messages =
        request.messages.iter().map(|m| WireMessage { role: &m.role, content: &m.content }).collect();
    let req = Request::Generate {
        id,
        messages,
        max_tokens: request.max_tokens,
        temperature: request.temperature,
        seed: request.seed,
        json_schema: request.json_schema.as_deref(),
    };
    w.send(&req).map_err(Err)?;
    drop(req);
    let mut streamed = String::new();
    let mut cancelled = false;
    loop {
        let ev = w.recv(silence).map_err(Err)?;
        if !ev.answers(id) {
            continue;
        }
        match ev {
            Event::Progress { prompt_done, prompt_total, .. } => {
                tracing::trace!(prompt_done, prompt_total, "MLX worker reading the prompt");
            }
            Event::Token { text, .. } => {
                if cancelled {
                    continue;
                }
                // The app is quitting (crate::shutdown): don't hold it up.
                if crate::shutdown::stopping() {
                    let _ = w.send(&Request::Cancel { id });
                    return Err(Ok(LlmError::Stopped));
                }
                streamed.push_str(&text);
                if !on_token(&text) {
                    cancelled = true;
                    w.send(&Request::Cancel { id }).map_err(Err)?;
                }
            }
            Event::Done { text, finish, stats, .. } => {
                tracing::debug!(
                    prompt_tokens = stats.prompt_tokens,
                    completion_tokens = stats.completion_tokens,
                    total_ms = stats.total_ms,
                    peak_memory_bytes = stats.peak_memory_bytes,
                    finish = ?finish,
                    "MLX generation finished"
                );
                let stopped_early = cancelled || finish == Finish::Cancelled;
                return Ok(GenerateOutput {
                    text: if cancelled { streamed } else { text },
                    stats: GenerateStats {
                        prompt_tokens: stats.prompt_tokens,
                        completion_tokens: stats.completion_tokens,
                        time_to_first_token_ms: stats.time_to_first_token_ms,
                        total_ms: stats.total_ms,
                        tokens_per_second: stats.tokens_per_second,
                        hit_max_tokens: !stopped_early && (stats.hit_max_tokens || finish == Finish::Length),
                    },
                });
            }
            ev @ Event::Error { .. } => return Err(Ok(error_of(ev))),
            _ => {}
        }
    }
}

// ---------------------------------------------------------------- backend

struct State {
    worker: Option<Worker>,
}

/// An MLX model, run by the pack's Python worker. See the module docs.
pub struct MlxBackend {
    spawn: Spawn,
    model_dir: PathBuf,
    max_context: u32,
    caps: LlmCapabilities,
    model_check_ran: Option<bool>,
    silence_ms: AtomicU64,
    state: Mutex<State>,
}

impl MlxBackend {
    /// Spawn, hello, load, then the model check unless `AlreadyPassed`.
    pub fn load(opts: MlxOptions) -> Result<Self, LlmError> {
        if !opts.model_dir.is_absolute() || !opts.model_dir.is_dir() {
            return Err(LlmError::ModelNotFound(opts.model_dir.display().to_string()));
        }
        let script = match &opts.worker_script {
            Some(s) => s.clone(),
            None => write_worker(&opts.work_dir)
                .map_err(|e| LlmError::Load(format!("couldn't write the MLX worker: {e}")))?,
        };
        let spawn = Spawn {
            python: opts.python.clone(),
            work_dir: opts.work_dir.clone(),
            script,
            device: opts.device,
            sandbox: opts.sandbox,
        };
        let mut worker = Worker::spawn(&spawn)?;
        hello(&mut worker)?;
        let info = load_model(&mut worker, &opts.model_dir, opts.max_context)?;

        let model_check_ran = match opts.model_check {
            ModelCheck::AlreadyPassed => None,
            ModelCheck::Run => Some(model_check(&mut worker, &info)?),
        };
        let passed = model_check_ran.unwrap_or(true);
        let context_length = if opts.max_context > 0 {
            info.context_length.min(opts.max_context)
        } else {
            info.context_length
        };
        let caps = LlmCapabilities {
            backend: "mlx",
            model_name: opts.model_name.clone(),
            context_length,
            streaming: true,
            json_schema_constrained: info.constrained_ready && passed,
            audio_input: false,
        };
        tracing::info!(
            model_type = %info.model_type,
            context_length,
            constrained = caps.json_schema_constrained,
            model_check = ?model_check_ran,
            "MLX backend ready"
        );
        Ok(Self {
            spawn,
            model_dir: opts.model_dir,
            max_context: opts.max_context,
            caps,
            model_check_ran,
            silence_ms: AtomicU64::new(SILENCE.as_millis() as u64),
            state: Mutex::new(State { worker: Some(worker) }),
        })
    }

    /// Spawn a worker, run its self-test, shut it down. Used after install
    /// and by "Try again".
    pub fn pack_check(
        python: &Path,
        work_dir: &Path,
        device: MlxDevice,
        sandbox: bool,
    ) -> Result<PackCheck, LlmError> {
        let script = write_worker(work_dir)
            .map_err(|e| LlmError::Load(format!("couldn't write the MLX worker: {e}")))?;
        let spawn =
            Spawn { python: python.to_path_buf(), work_dir: work_dir.to_path_buf(), script, device, sandbox };
        let mut worker = Worker::spawn(&spawn)?;
        let h = hello(&mut worker)?;
        let id = worker.next_id();
        if worker.send(&Request::Selftest { id }).is_err() {
            return Err(LlmError::Load("the MLX engine stopped during its check".into()));
        }
        let reply = worker.wait(id, SELFTEST_TIMEOUT);
        worker.shutdown();
        let mut versions = h.versions;
        versions.insert("python".into(), h.python);
        match reply {
            Ok(Event::Selftest { ok, checks, message, .. }) => {
                tracing::info!(ok, ?checks, device = %h.device, metal = h.metal, "MLX pack check");
                Ok(PackCheck { ok, device: h.device, metal: h.metal, versions, checks, message })
            }
            Ok(ev @ Event::Error { .. }) => Err(match error_of(ev) {
                LlmError::Inference(m) => LlmError::Load(m),
                other => other,
            }),
            Ok(_) => Err(LlmError::Load("the MLX engine answered out of turn".into())),
            Err(Recv::Timeout) => Err(LlmError::Load("the MLX engine took too long to check itself".into())),
            Err(Recv::Eof) => Err(LlmError::Load("the MLX engine stopped during its check".into())),
        }
    }

    /// `Some(passed)` when [`load`](Self::load) ran the model check; `None`
    /// when it was `AlreadyPassed`.
    pub fn model_check_ran(&self) -> Option<bool> {
        self.model_check_ran
    }

    /// Tests only: how long a generation may go without a message from the
    /// worker before it is killed (120 s otherwise).
    #[doc(hidden)]
    pub fn set_silence_timeout(&self, timeout: Duration) {
        self.silence_ms.store(timeout.as_millis().max(1) as u64, Ordering::Relaxed);
    }

    fn silence(&self) -> Duration {
        Duration::from_millis(self.silence_ms.load(Ordering::Relaxed))
    }

    /// Start a new worker and load the model again (after it stopped).
    fn restart(&self) -> Result<Worker, LlmError> {
        tracing::warn!("restarting the MLX engine");
        let mut w = Worker::spawn(&self.spawn)?;
        hello(&mut w)?;
        load_model(&mut w, &self.model_dir, self.max_context)?;
        Ok(w)
    }
}

/// The model check (ADR-0025 decision 6): at most 32 tokens under
/// `{"ok": boolean}` at temperature 0; passes when the text parses with a
/// boolean `ok`. A worker that dies here fails the load.
fn model_check(w: &mut Worker, info: &ModelInfo) -> Result<bool, LlmError> {
    if !info.constrained_ready {
        tracing::info!("MLX model check skipped: the worker can't hold this model to a JSON Schema");
        return Ok(false);
    }
    let mut request = GenerateRequest::new(vec![
        crate::ChatMessage::system("You answer with JSON only."),
        crate::ChatMessage {
            role: "user".into(),
            content: "Is this a test? Answer with ok set to true.".into(),
        },
    ]);
    request.max_tokens = 32;
    request.temperature = 0.0;
    request.seed = 0;
    request.json_schema = Some(MODEL_CHECK_SCHEMA.to_string());
    match run_generate(w, &request, SILENCE, &mut |_| true) {
        Ok(out) => {
            let passed = serde_json::from_str::<serde_json::Value>(out.text.trim())
                .ok()
                .is_some_and(|v| v.get("ok").is_some_and(serde_json::Value::is_boolean));
            tracing::info!(passed, completion_tokens = out.stats.completion_tokens, "MLX model check");
            Ok(passed)
        }
        Err(Ok(e)) => {
            tracing::info!(error = %e, "MLX model check failed");
            Ok(false)
        }
        Err(Err(_)) => {
            w.kill();
            Err(LlmError::Load(STOPPED.into()))
        }
    }
}

impl LlmBackend for MlxBackend {
    fn capabilities(&self) -> LlmCapabilities {
        self.caps.clone()
    }

    fn generate(
        &self,
        request: &GenerateRequest,
        on_token: &mut dyn FnMut(&str) -> bool,
    ) -> Result<GenerateOutput, LlmError> {
        if request.json_schema.is_some() && !self.caps.json_schema_constrained {
            return Err(LlmError::Grammar(NOT_CONSTRAINED.into()));
        }
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if state.worker.is_none() {
            state.worker = Some(self.restart()?);
        }
        let worker = state.worker.as_mut().expect("worker present");
        match run_generate(worker, request, self.silence(), on_token) {
            Ok(out) => Ok(out),
            Err(Ok(e)) => Err(e),
            Err(Err(why)) => {
                match why {
                    Recv::Timeout => {
                        tracing::warn!("the MLX engine went silent during a generation; stopping it")
                    }
                    Recv::Eof => tracing::warn!("the MLX engine stopped during a generation"),
                }
                if let Some(mut w) = state.worker.take() {
                    w.kill();
                }
                Err(LlmError::Inference(STOPPED.into()))
            }
        }
    }
}

impl Drop for MlxBackend {
    /// Send shutdown, wait 2 s, then kill.
    fn drop(&mut self) {
        let state = self.state.get_mut().unwrap_or_else(|p| p.into_inner());
        if let Some(mut w) = state.worker.take() {
            w.shutdown();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn to_json(r: &Request) -> serde_json::Value {
        serde_json::from_str(&serde_json::to_string(r).unwrap()).unwrap()
    }

    fn event(v: serde_json::Value) -> Event {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn requests_match_the_protocol_table() {
        assert_eq!(to_json(&Request::Hello { protocol: 1 }), json!({"op":"hello","protocol":1}));
        assert_eq!(to_json(&Request::Selftest { id: 1 }), json!({"id":1,"op":"selftest"}));
        assert_eq!(
            to_json(&Request::Load { id: 2, path: "/abs/model/dir", max_context: 32768 }),
            json!({"id":2,"op":"load","path":"/abs/model/dir","max_context":32768})
        );
        let generate = Request::Generate {
            id: 3,
            messages: vec![
                WireMessage { role: "system", content: "s" },
                WireMessage { role: "user", content: "u" },
            ],
            max_tokens: 2048,
            temperature: 0.25,
            seed: 42,
            json_schema: Some("{}"),
        };
        assert_eq!(
            to_json(&generate),
            json!({"id":3,"op":"generate","messages":[{"role":"system","content":"s"},{"role":"user","content":"u"}],
                   "max_tokens":2048,"temperature":0.25,"seed":42,"json_schema":"{}"})
        );
        let generate = Request::Generate {
            id: 3,
            messages: vec![],
            max_tokens: 1,
            temperature: 0.0,
            seed: 0,
            json_schema: None,
        };
        assert_eq!(to_json(&generate)["json_schema"], serde_json::Value::Null);
        assert_eq!(to_json(&Request::Cancel { id: 3 }), json!({"op":"cancel","id":3}));
        assert_eq!(to_json(&Request::Unload { id: 4 }), json!({"id":4,"op":"unload"}));
        assert_eq!(to_json(&Request::Shutdown), json!({"op":"shutdown"}));
        // The tag comes first on the wire.
        assert!(
            serde_json::to_string(&Request::Hello { protocol: 1 }).unwrap().starts_with(r#"{"op":"hello""#)
        );
    }

    #[test]
    fn events_match_the_protocol_table() {
        let hello = event(json!({"event":"hello","protocol":1,"python":"3.12.15",
            "versions":{"mlx":"0.32.3","mlx_lm":"0.32.0","transformers":"5.19.0","llguidance":"1.9.1"},
            "device":"gpu","metal":true}));
        match hello {
            Event::Hello { protocol, python, versions, device, metal } => {
                assert_eq!((protocol, python.as_str(), device.as_str(), metal), (1, "3.12.15", "gpu", true));
                assert_eq!(versions["mlx_lm"], "0.32.0");
            }
            other => panic!("{other:?}"),
        }
        let st = event(
            json!({"id":1,"event":"selftest","ok":true,"checks":[["imports",true],["socket_guard",true],
            ["metal",true],["matmul",true],["mask",true]],"message":null}),
        );
        assert_eq!(
            st,
            Event::Selftest {
                id: Some(1),
                ok: true,
                checks: ["imports", "socket_guard", "metal", "matmul", "mask"]
                    .iter()
                    .map(|c| (c.to_string(), true))
                    .collect(),
                message: None
            }
        );
        let loaded = event(json!({"id":2,"event":"loaded","model_type":"qwen3","context_length":40960,
            "eos_token_ids":[151645,151643],"vocab_size":151936,"constrained_ready":true,"load_ms":2140}));
        assert_eq!(
            loaded,
            Event::Loaded {
                id: Some(2),
                model_type: "qwen3".into(),
                context_length: 40960,
                eos_token_ids: vec![151645, 151643],
                vocab_size: 151936,
                constrained_ready: true,
                load_ms: 2140
            }
        );
        assert_eq!(
            event(json!({"id":3,"event":"token","text":"Hel"})),
            Event::Token { id: Some(3), text: "Hel".into() }
        );
        let done =
            event(json!({"id":3,"event":"done","text":"Hello","finish":"stop","stats":{"prompt_tokens":812,
            "completion_tokens":403,"time_to_first_token_ms":950,"total_ms":14100,"tokens_per_second":30.6,
            "hit_max_tokens":false,"peak_memory_bytes":2900000000u64}}));
        match done {
            Event::Done { id, text, finish, stats } => {
                assert_eq!((id, text.as_str(), finish), (Some(3), "Hello", Finish::Stop));
                assert_eq!(stats.prompt_tokens, 812);
                assert_eq!(stats.completion_tokens, 403);
                assert_eq!(stats.time_to_first_token_ms, 950);
                assert_eq!(stats.total_ms, 14100);
                assert!((stats.tokens_per_second - 30.6).abs() < 1e-9);
                assert!(!stats.hit_max_tokens);
                assert_eq!(stats.peak_memory_bytes, 2_900_000_000);
            }
            other => panic!("{other:?}"),
        }
        for (f, want) in
            [("length", Finish::Length), ("cancelled", Finish::Cancelled), ("odd", Finish::Other)]
        {
            match event(json!({"id":3,"event":"done","text":"","finish":f})) {
                Event::Done { finish, stats, .. } => {
                    assert_eq!(finish, want);
                    assert_eq!(stats, WireStats::default());
                }
                other => panic!("{other:?}"),
            }
        }
        assert_eq!(event(json!({"id":4,"event":"unloaded"})), Event::Unloaded { id: Some(4) });
        assert_eq!(
            event(json!({"id":3,"event":"progress","prompt_done":64,"prompt_total":812})),
            Event::Progress { id: Some(3), prompt_done: 64, prompt_total: 812 }
        );
        let err = event(json!({"id":5,"event":"error","kind":"context_overflow","message":"too long",
            "prompt_tokens":100,"max_new":50,"context":120}));
        assert!(matches!(error_of(err), LlmError::ContextOverflow { prompt: 100, max_new: 50, ctx: 120 }));
        // An id-less error answers whatever is waiting; other events don't.
        let anon = event(json!({"event":"error","kind":"bad_request","message":"x"}));
        assert!(anon.answers(9));
        assert!(!event(json!({"id":8,"event":"token","text":""})).answers(9));
    }

    #[test]
    fn error_kinds_map_to_llm_errors() {
        let m = |k: &str| {
            let ev = event(json!({"id":1,"event":"error","kind":k,"message":"plain words"}));
            error_of(ev)
        };
        assert!(matches!(m("load"), LlmError::Load(s) if s == "plain words"));
        assert!(matches!(m("unsupported_architecture"), LlmError::Load(_)));
        assert!(matches!(m("template"), LlmError::Template(_)));
        assert!(matches!(m("grammar"), LlmError::Grammar(_)));
        assert!(matches!(m("context_overflow"), LlmError::ContextOverflow { prompt: 0, max_new: 0, ctx: 0 }));
        assert!(matches!(m("inference"), LlmError::Inference(_)));
        assert!(matches!(m("bad_request"), LlmError::Inference(_)));
        assert!(matches!(m("something_new"), LlmError::Inference(_)));
    }

    #[test]
    fn stderr_lines_are_cut() {
        assert_eq!(cut("short", 300), "short");
        let long = "é".repeat(400);
        let c = cut(&long, 300);
        assert_eq!(c.chars().count(), 301);
        assert!(c.ends_with('…'));
    }

    #[test]
    fn worker_source_starts_with_the_socket_guard() {
        // The guard must run before any other import.
        let code: Vec<&str> = WORKER_SOURCE
            .lines()
            .map(str::trim)
            .filter(|l| l.starts_with("import ") || l.starts_with("from "))
            .collect();
        assert_eq!(code.first(), Some(&"import socket as _socket"));
        let guard = WORKER_SOURCE.find("_socket.getaddrinfo = _blocked").unwrap();
        let first_other = WORKER_SOURCE.find("import os").unwrap();
        assert!(guard < first_other);
        for name in [
            "socket.connect =",
            "socket.connect_ex =",
            "socket.sendto =",
            "socket.sendmsg =",
            "socket.bind =",
            "socket.listen =",
            "create_connection =",
            "getaddrinfo =",
        ] {
            assert!(WORKER_SOURCE.contains(name), "{name}");
        }
        // The worker caps the context as llama.cpp does (max_context).
        assert!(WORKER_SOURCE.contains("max_context"));
        assert!(WORKER_SOURCE.contains(r#"os.environ.get("HF_HUB_OFFLINE") != "1""#));
        assert!(WORKER_SOURCE.contains("trust_remote_code=False"));
        assert!(!WORKER_SOURCE.contains("trust_remote_code=True"));
        assert!(WORKER_SOURCE.contains("enable_thinking=False"));
        assert!(WORKER_SOURCE.contains(r#""whitespace_flexible": False"#));
    }
}
