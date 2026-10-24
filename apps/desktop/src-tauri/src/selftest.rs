//! A self-test for CI, built only with the `selftest` feature (never in the
//! installers). It drives the real app in its real webview, then quits the
//! way a person does, so the macOS job can check two bugs from 0.1.1:
//!
//! - Quit: a note model loaded on the GPU when the app quits. Before the fix,
//!   ggml's Metal teardown aborted (SIGABRT) on Cmd-Q.
//! - Replay: recordings play in the webview (WKWebView on macOS, WebView2 on
//!   Windows, WebKitGTK on Linux), from the app's own audio URL, under the
//!   app's own Content Security Policy, to the end.
//!
//! Environment (all optional except the first):
//! - `OPENBARK_SELFTEST_OUT`: where the results go (JSON). Turns the test on.
//! - `OPENBARK_SELFTEST_LLM`: a small `.gguf` to load and generate with.
//! - `OPENBARK_SELFTEST_AUDIO=1`: make two tone visits and play them.
//! - `OPENBARK_SELFTEST_SKIP_SHUTDOWN=1`: quit without freeing the model
//!   first (the old behaviour), to show the job can see the crash.
//!
//! The script that runs it (scripts/ci/desktop-selftest.sh) checks the exit
//! status and the results.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tauri::{AppHandle, Manager, Webview, WebviewWindow, Wry};

use crate::AppState;

static STARTED: AtomicBool = AtomicBool::new(false);

pub fn enabled() -> bool {
    std::env::var_os("OPENBARK_SELFTEST_OUT").is_some()
}

/// Quit without freeing the model first (the negative control).
pub fn skip_shutdown() -> bool {
    std::env::var_os("OPENBARK_SELFTEST_SKIP_SHUTDOWN").is_some()
}

/// Called when the main page has loaded; starts the test once.
pub fn page_loaded(webview: &Webview<Wry>) {
    if !enabled() || webview.label() != "main" || STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    let handle = webview.app_handle().clone();
    std::thread::spawn(move || {
        let results = run(&handle);
        let out = PathBuf::from(std::env::var_os("OPENBARK_SELFTEST_OUT").expect("checked in enabled()"));
        let text = serde_json::to_string_pretty(&results).unwrap_or_default();
        if let Err(e) = std::fs::write(&out, text) {
            eprintln!("selftest: couldn't write {}: {e}", out.display());
        }
        eprintln!("selftest: results written; quitting the way the app does");
        quit(&handle);
    });
}

fn run(handle: &AppHandle) -> Value {
    let app = handle.state::<AppState>().inner().clone();
    let mut results = json!({ "platform": std::env::consts::OS });

    if let Some(model) = std::env::var_os("OPENBARK_SELFTEST_LLM") {
        let spec = PathBuf::from(&model).display().to_string();
        let started = Instant::now();
        // The note model into the app's slot, as a draft loads it.
        let loaded = app.preload_llm(&spec);
        let load_secs = started.elapsed().as_secs_f32();
        // Then a draft, as a person starts one: a template proposed from an
        // example note. GitHub's Mac runners draft slowly on their
        // paravirtual GPU, so it gets a short while; one still running when
        // the app quits is the "quit mid-draft" case.
        let (tx, rx) = std::sync::mpsc::channel();
        if loaded.is_ok() {
            let app = app.clone();
            let spec = spec.clone();
            std::thread::spawn(move || {
                let _ = tx.send(app.template_from_example(&spec, EXAMPLE).map(drop).map_err(|e| e.0));
            });
        }
        let draft = rx.recv_timeout(Duration::from_secs(20));
        results["llm"] = json!({
            "model": PathBuf::from(&model).file_name().map(|n| n.to_string_lossy().into_owned()),
            "loaded": loaded.as_ref().map(|_| true).map_err(|e| e.0.clone()),
            "load_secs": load_secs,
            // A tiny test model may well write something unusable; what
            // matters is that it ran on the GPU.
            "draft": match draft {
                Ok(Ok(())) => json!("finished"),
                Ok(Err(e)) => json!(format!("finished: {e}")),
                Err(_) => json!("still running at quit"),
            },
            "secs": started.elapsed().as_secs_f32(),
            "live_models_before_quit": os_llm::shutdown::live_models(),
        });
    }

    if std::env::var_os("OPENBARK_SELFTEST_AUDIO").is_some() {
        results["audio"] = match os_app::demo::audio_visits(&app.sessions_root) {
            Ok(visits) => match handle.get_webview_window("main") {
                Some(window) => play_in_webview(&window, &visits),
                None => json!({ "fatal": "no main window" }),
            },
            Err(e) => json!({ "fatal": format!("couldn't make the visits: {}", e.0) }),
        };
    }
    results
}

/// Play each visit from the app's audio URL (the `obaudio` scheme, as
/// `audioUrl` in engine.ts builds it) and from the blob fallback, in the
/// main window, and collect what the media element reports.
fn play_in_webview(window: &WebviewWindow<Wry>, visits: &[os_app::demo::DemoVisit]) -> Value {
    let ids = serde_json::to_string(&visits).unwrap_or_default();
    let script = PLAY_JS.replace("__VISITS__", &ids);
    if let Err(e) = window.eval(script) {
        return json!({ "fatal": format!("eval failed: {e}") });
    }
    let found: Arc<Mutex<Option<Value>>> = Arc::new(Mutex::new(None));
    let deadline = Instant::now() + Duration::from_secs(300);
    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(500));
        let slot = found.clone();
        let _ = window.eval_with_callback("JSON.stringify(window.__obSelftest || null)", move |raw| {
            // The callback gets the expression's value serialised as JSON: a JSON string.
            let inner: Option<String> = serde_json::from_str(&raw).ok();
            let value = inner.and_then(|s| serde_json::from_str::<Value>(&s).ok()).unwrap_or(Value::Null);
            *slot.lock().unwrap() = Some(value);
        });
        let current = found.lock().unwrap().clone();
        if let Some(v) = current
            && v.get("done").and_then(Value::as_bool) == Some(true)
        {
            return v;
        }
    }
    json!({ "fatal": "timed out waiting for playback", "last": found.lock().unwrap().clone() })
}

/// Quit as a person does: on macOS `-[NSApplication terminate:]` (what Cmd-Q
/// and the Quit menu item send), elsewhere the app's own exit.
fn quit(handle: &AppHandle) {
    #[cfg(target_os = "macos")]
    {
        let _ = handle.run_on_main_thread(|| {
            // SAFETY: plain Objective-C messages on the main thread.
            unsafe { terminate_soon() }
        });
    }
    #[cfg(not(target_os = "macos"))]
    handle.exit(0);
}

/// `[NSApp performSelector:@selector(terminate:) withObject:nil afterDelay:0]`:
/// AppKit's run loop then sends `terminate:` itself, as it does for the Quit
/// menu item, rather than from inside the event loop's callback (which is
/// where `run_on_main_thread` runs this).
#[cfg(target_os = "macos")]
unsafe fn terminate_soon() {
    use std::ffi::{c_char, c_void};
    #[link(name = "objc")]
    unsafe extern "C" {
        fn objc_getClass(name: *const c_char) -> *mut c_void;
        fn sel_registerName(name: *const c_char) -> *mut c_void;
        fn objc_msgSend();
    }
    type Id = *mut c_void;
    type GetApp = unsafe extern "C" fn(Id, Id) -> Id;
    type PerformAfter = unsafe extern "C" fn(Id, Id, Id, Id, f64);
    unsafe {
        let msg_send = objc_msgSend as unsafe extern "C" fn();
        let get_app = std::mem::transmute::<unsafe extern "C" fn(), GetApp>(msg_send);
        let perform_after = std::mem::transmute::<unsafe extern "C" fn(), PerformAfter>(msg_send);
        let app = get_app(
            objc_getClass(c"NSApplication".as_ptr()),
            sel_registerName(c"sharedApplication".as_ptr()),
        );
        perform_after(
            app,
            sel_registerName(c"performSelector:withObject:afterDelay:".as_ptr()),
            sel_registerName(c"terminate:".as_ptr()),
            std::ptr::null_mut(),
            0.0,
        );
    }
}

const EXAMPLE: &str = "Subjective: Bella, 4 year old spayed Labrador, two days of vomiting.
Objective: T 38.6 C, HR 96.
Assessment: Gastritis.
Plan: Bland diet for three days.";

const PLAY_JS: &str = r#"
(async () => {
  const S = (window.__obSelftest = { done: false, cases: [] });
  const visits = __VISITS__;
  const T = window.__TAURI_INTERNALS__;
  function play(src, label, secs) {
    return new Promise((resolve) => {
      const r = { label, expected_secs: secs, events: [] };
      const a = new Audio();
      a.muted = true;
      a.preload = "auto";
      let finished = false;
      const finish = (extra) => {
        if (finished) return;
        finished = true;
        Object.assign(r, extra || {});
        r.current_time = a.currentTime;
        r.ready_state = a.readyState;
        if (a.error) r.media_error = { code: a.error.code, message: a.error.message || "" };
        a.pause();
        a.removeAttribute("src");
        a.load();
        resolve(r);
      };
      const timer = setTimeout(() => finish({ timeout: true }), 60000);
      for (const ev of ["loadedmetadata", "canplay", "playing", "seeked", "stalled", "waiting", "ended"]) {
        a.addEventListener(ev, () => r.events.push(ev + "@" + a.currentTime.toFixed(2)));
      }
      a.addEventListener("error", () => { clearTimeout(timer); finish({ error: true }); });
      a.addEventListener("loadedmetadata", async () => {
        r.duration = a.duration;
        try {
          await a.play();
          await new Promise((ok) => setTimeout(ok, 1500));
          r.played_to = a.currentTime;
          // Seek near the end (a byte range far into the file), then play out.
          a.currentTime = Math.max(0, a.duration - 2);
        } catch (e) {
          r.play_rejected = String((e && e.message) || e);
          clearTimeout(timer);
          finish({});
        }
      }, { once: true });
      a.addEventListener("ended", () => { clearTimeout(timer); finish({ ended: true }); }, { once: true });
      a.src = src;
    });
  }
  try {
    for (const v of visits) {
      const url = T.convertFileSrc(v.id, "obaudio");
      S.cases.push(Object.assign(await play(url, v.label + " (app URL)", v.secs), { url_scheme: url.split(":")[0] }));
      const bytes = await T.invoke("session_audio", { id: v.id });
      const blob = URL.createObjectURL(new Blob([bytes], { type: "audio/wav" }));
      S.cases.push(await play(blob, v.label + " (blob fallback)", v.secs));
      URL.revokeObjectURL(blob);
    }
  } catch (e) {
    S.fatal = String((e && e.message) || e);
  }
  S.user_agent = navigator.userAgent;
  S.done = true;
})();
"#;
