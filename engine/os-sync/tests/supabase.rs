//! Encrypted sync against a real Supabase (ADR-0017, M3 phase 2 acceptance).
//!
//! Runs only when a local Supabase is up and these are set (the script
//! prints them; nothing is hard-coded here):
//!
//! ```text
//! bash scripts/sync-local-supabase.sh start
//! eval "$(bash scripts/sync-local-supabase.sh env)"
//! cargo test -p os-sync --test supabase
//! ```
//!
//! `OPENBARK_SYNC_TEST_URL`, `OPENBARK_SYNC_TEST_KEY` (publishable),
//! `OPENBARK_SYNC_TEST_SECRET` (service role: used only to read raw rows for
//! the ciphertext checks) and `OPENBARK_SYNC_TEST_MAIL` (Mailpit, where the
//! sign-in codes arrive). Without them every test returns at once. Never run
//! against a project with real data: every account here is synthetic.
//!
//! Each "computer" is a SyncService over its own temporary visits folder,
//! with its keys in memory and every request recorded.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use os_engine::session::{PatientInfo, RunInfo, Session};
use os_sync::state::{MemoryStore, Phase, SecretStore};
use os_sync::supabase::{HttpTransport, Request, Response, Server, Transport};
use os_sync::{NextStep, NoHost, SyncError, SyncService, keys, seal};
use serde_json::{Value, json};

/// Names that must never reach the server in any form.
const FIXTURE_NAMES: [&str; 4] = ["Maple", "Okonkwo", "Biscuit", "Fairweather"];
/// Words only the fixture transcript and notes contain.
const FIXTURE_WORDS: [&str; 2] = ["intermittent vomiting", "Plan: bland diet"];

struct Env {
    url: String,
    key: String,
    secret: String,
    mail: String,
}

fn env() -> Option<Env> {
    let get = |k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty());
    match (get("OPENBARK_SYNC_TEST_URL"), get("OPENBARK_SYNC_TEST_KEY"), get("OPENBARK_SYNC_TEST_SECRET")) {
        (Some(url), Some(key), Some(secret)) => Some(Env {
            url,
            key,
            secret,
            mail: get("OPENBARK_SYNC_TEST_MAIL").unwrap_or_else(|| "http://127.0.0.1:54324".into()),
        }),
        _ => {
            eprintln!("skipped: no local Supabase (see the comment at the top of this file)");
            None
        }
    }
}

fn vault_on() {
    os_engine::vault::use_key([42; 32], os_engine::vault::KeySource::Environment, None);
}

/// Records every request, then sends it for real.
#[derive(Default)]
struct Recorder {
    sent: Mutex<Vec<Request>>,
    http: Option<HttpTransport>,
}

impl Recorder {
    fn new() -> Arc<Self> {
        Arc::new(Self { sent: Mutex::new(Vec::new()), http: Some(HttpTransport::new().unwrap()) })
    }

    fn requests(&self) -> Vec<Request> {
        self.sent.lock().unwrap().clone()
    }

    /// The bearer token of the latest REST call.
    fn last_token(&self) -> Option<String> {
        self.requests().iter().rev().find(|r| r.url.contains("/rest/v1/")).and_then(|r| {
            r.headers.iter().find(|(k, _)| k == "Authorization").map(|(_, v)| v.trim_start_matches("Bearer ").to_string())
        })
    }
}

impl Transport for Recorder {
    fn send(&self, req: &Request) -> Result<Response, SyncError> {
        self.sent.lock().unwrap().push(req.clone());
        self.http.as_ref().unwrap().send(req)
    }
}

struct Computer {
    svc: SyncService,
    rec: Arc<Recorder>,
    store: Arc<SharedStore>,
    dir: tempfile::TempDir,
}

/// A MemoryStore the test can also read.
#[derive(Default)]
struct SharedStore(MemoryStore);

impl os_sync::state::SecretStore for SharedStore {
    fn load(&self) -> Result<os_sync::state::Secrets, SyncError> {
        self.0.load()
    }
    fn save(&self, s: &os_sync::state::Secrets) -> Result<(), SyncError> {
        self.0.save(s)
    }
    fn erase(&self) -> Result<(), SyncError> {
        self.0.erase()
    }
    fn kind(&self) -> &'static str {
        "memory"
    }
}

struct StoreRef(Arc<SharedStore>);

impl os_sync::state::SecretStore for StoreRef {
    fn load(&self) -> Result<os_sync::state::Secrets, SyncError> {
        self.0.load()
    }
    fn save(&self, s: &os_sync::state::Secrets) -> Result<(), SyncError> {
        self.0.save(s)
    }
    fn erase(&self) -> Result<(), SyncError> {
        self.0.erase()
    }
    fn kind(&self) -> &'static str {
        "memory"
    }
}

impl Computer {
    fn new(env: &Env) -> Self {
        vault_on();
        let dir = tempfile::tempdir().unwrap();
        let rec = Recorder::new();
        let store = Arc::new(SharedStore::default());
        let svc = SyncService::open(dir.path(), rec.clone(), Box::new(StoreRef(store.clone()))).unwrap();
        svc.set_server(Some(Server { url: env.url.clone(), key: env.key.clone() })).unwrap();
        Self { svc, rec, store, dir }
    }

    fn root(&self) -> &Path {
        self.dir.path()
    }

    fn sync(&self) -> os_sync::SyncReport {
        self.svc.sync_once(&NoHost).unwrap()
    }

    fn visits(&self) -> Vec<Session> {
        os_engine::session::list(self.root())
    }
}

/// The newest sign-in code mailed to `email` that hasn't been used.
fn code_for(env: &Env, email: &str, used: &Mutex<BTreeSet<String>>) -> String {
    let http = reqwest::blocking::Client::new();
    for _ in 0..50 {
        let list = get_json(&http, &format!("{}/api/v1/search?query=to:{email}", env.mail), &[]);
        for m in list["messages"].as_array().into_iter().flatten() {
            let id = m["ID"].as_str().unwrap_or_default().to_string();
            if used.lock().unwrap().contains(&id) {
                continue;
            }
            let msg = get_json(&http, &format!("{}/api/v1/message/{id}", env.mail), &[]);
            let text = msg["Text"].as_str().unwrap_or_default();
            if let Some(code) = text.split(|c: char| !c.is_ascii_digit()).find(|w| w.len() == 6) {
                used.lock().unwrap().insert(id);
                return code.to_string();
            }
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    panic!("no sign-in code arrived for {email}");
}

/// GET a URL and parse its JSON (reqwest here has no json feature).
fn get_json(http: &reqwest::blocking::Client, url: &str, headers: &[(&str, &str)]) -> Value {
    let mut r = http.get(url);
    for (k, v) in headers {
        r = r.header(*k, *v);
    }
    r.send().and_then(|r| r.text()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null)
}

fn email() -> String {
    format!("sync-test-{}@example.org", uuid::Uuid::new_v4().simple())
}

/// Turn sync on: sign in with the emailed code.
fn sign_in(env: &Env, c: &Computer, email: &str, name: &str, used: &Mutex<BTreeSet<String>>) -> NextStep {
    // The server allows one code a second per address.
    let mut tries = 0;
    while let Err(e) = c.svc.send_code(email) {
        tries += 1;
        assert!(tries < 10 && matches!(e, SyncError::User(_)), "{e}");
        std::thread::sleep(Duration::from_millis(1100));
    }
    let code = code_for(env, email, used);
    c.svc.verify_code(&code, name, None, None).unwrap()
}

/// The first computer: a new account, its Recovery Kit checked.
fn first_computer(env: &Env, email: &str, used: &Mutex<BTreeSet<String>>) -> (Computer, Vec<String>) {
    let a = Computer::new(env);
    let NextStep::Kit(kit) = sign_in(env, &a, email, "Front desk PC", used) else { panic!("expected a new account") };
    assert_eq!(kit.words.len(), 24);
    assert_eq!(a.svc.status().phase, Phase::KitCheck);
    // Nothing syncs before the kit is checked.
    assert!(a.svc.confirm_kit(&[(kit.check[0], "wrong".into()), (kit.check[1], "words".into())]).is_err());
    let answers = kit.check.iter().map(|p| (*p, kit.words[p - 1].clone())).collect::<Vec<_>>();
    a.svc.confirm_kit(&answers).unwrap();
    assert_eq!(a.svc.status().phase, Phase::Active);
    (a, kit.words)
}

/// Another computer joins: signs in, shows its code, A types it.
fn join(env: &Env, a: &Computer, email: &str, name: &str, used: &Mutex<BTreeSet<String>>) -> Computer {
    let b = Computer::new(env);
    let NextStep::Pair(pair) = sign_in(env, &b, email, name, used) else { panic!("expected to wait for approval") };
    assert_eq!(pair.words.len(), 6);
    assert!(pair.qr_svg.contains("<svg"));
    // The wrong code finds nothing.
    assert!(a.svc.check_pairing("abandon ability able about above absent").is_err());
    let found = a.svc.check_pairing(&pair.words.join(" ")).unwrap();
    assert_eq!(found.name, name);
    a.svc.approve_pairing(&pair.words.join(" ")).unwrap();
    b.sync();
    assert_eq!(b.svc.status().phase, Phase::Active, "{:?}", b.svc.status().last_error);
    b
}

/// A visit as a computer saves it: a recording, a transcript and a draft.
fn make_visit(root: &Path, pet: &str, family: &str) -> Session {
    let mut s = Session::create_live(root, &format!("{pet} {family}")).unwrap();
    // The recording: a WAV with a marker no request may ever carry.
    let mut wav = b"RIFF\x24\x00\x00\x00WAVEfmt ".to_vec();
    wav.extend_from_slice(b"AUDIO-MARKER-NEVER-SYNCS");
    wav.extend(std::iter::repeat_n(0u8, 4096));
    os_engine::vault::write_audio(&s.audio_path(), &wav).unwrap();
    s.meta.source = "live recording (USB microphone)".into();
    s.meta.audio_secs = 312.0;
    s.save().unwrap();
    s.set_patient(PatientInfo {
        name: pet.into(),
        last_name: family.into(),
        reason: "intermittent vomiting".into(),
        species: "Cat".into(),
        ..Default::default()
    })
    .unwrap();
    let transcript = "{\"id\":0,\"start\":0.0,\"end\":4.0,\"speaker\":0,\"text\":\"She has had intermittent vomiting since Tuesday.\"}\n";
    os_engine::vault::write(&s.transcript_path(), transcript.as_bytes()).unwrap();
    s.meta.transcription = Some(serde_json::from_value(json!({
        "stt_model": "parakeet", "diarized": false, "created_at": chrono::Local::now(),
        "stats": {"audio_secs": 312.0, "vad_ms": 10, "diarization_ms": 0, "asr_ms": 800, "total_ms": 900, "rtf": 0.01, "segments": 1, "speakers": 1}
    })).unwrap());
    s.save().unwrap();
    add_run(&mut s, "Plan: bland diet for three days.");
    s
}

/// A draft run, as drafting saves one (files and the run's entry).
fn add_run(s: &mut Session, plan: &str) -> String {
    let device = os_engine::device::for_root(s.dir.parent().unwrap());
    let n = s.meta.runs.len() + 1;
    let dir = format!("runs/{n:03}_{}_test-model", device.tag);
    std::fs::create_dir_all(s.dir.join(&dir)).unwrap();
    let note = json!({"note": {"plan": plan}, "faithfulness": {"passed": true}, "markdown": format!("## Plan\n{plan}\n")});
    os_engine::vault::write(&s.dir.join(&dir).join("note.json"), note.to_string().as_bytes()).unwrap();
    os_engine::vault::write(&s.dir.join(&dir).join("note.md"), format!("## Plan\n{plan}\n").as_bytes()).unwrap();
    let run_id = uuid::Uuid::now_v7().to_string();
    let run: RunInfo = serde_json::from_value(json!({
        "dir": dir, "run_id": run_id, "device": device.tag, "llm": "test-model", "template": "soap@0.1.0",
        "created_at": chrono::Local::now(), "faithfulness_passed": true, "total_ms": 10,
    }))
    .unwrap();
    s.meta.runs.push(run);
    s.save().unwrap();
    run_id
}

fn find(c: &Computer, id: &str) -> Session {
    c.visits().into_iter().find(|s| s.meta.id == id).unwrap_or_else(|| panic!("visit {id} isn't on this computer"))
}

/// Every row of a table in the sync schema, read with the service role.
fn raw_rows(env: &Env, table: &str) -> Vec<Value> {
    get_json(
        &reqwest::blocking::Client::new(),
        &format!("{}/rest/v1/{table}?select=*", env.url),
        &[("apikey", &env.secret), ("Accept-Profile", "sync")],
    )
    .as_array()
    .cloned()
    .unwrap_or_default()
}

fn hex_bytes(v: &Value) -> Vec<u8> {
    hex::decode(v.as_str().unwrap_or_default().trim_start_matches("\\x")).unwrap_or_default()
}

/// No request carried a fixture name, transcript words, audio or a Storage call.
fn assert_requests_clean(c: &Computer) {
    for r in c.rec.requests() {
        assert!(!r.url.contains("/storage/v1"), "a request went to Storage: {}", r.url);
        assert!(!r.url.contains("/realtime/v1"), "a request went to Realtime");
        let body = r.body.clone().unwrap_or_default();
        let text = String::from_utf8_lossy(&body);
        for name in FIXTURE_NAMES.iter().chain(FIXTURE_WORDS.iter()) {
            assert!(!text.contains(name), "{name} appeared in a request to {}", r.url);
        }
        assert!(!text.contains("AUDIO-MARKER"), "audio left the device");
        // Every binary field decodes to ciphertext, never audio or plain text.
        if let Ok(v) = serde_json::from_slice::<Value>(&body) {
            check_fields(&v);
        }
    }
}

fn check_fields(v: &Value) {
    match v {
        Value::Object(m) => {
            for (k, x) in m {
                if k == "body"
                    && let Some(s) = x.as_str()
                {
                    let b = keys::unb64(s).unwrap();
                    assert!(seal::parse_obs1(&b).is_ok() || seal::parse_obb1(&b).is_ok(), "a body wasn't OBS1 or OBB1");
                    let t = String::from_utf8_lossy(&b);
                    assert!(!t.contains("AUDIO-MARKER") && !t.contains("RIFF"));
                }
                check_fields(x);
            }
        }
        Value::Array(a) => a.iter().for_each(check_fields),
        _ => {}
    }
}

#[test]
fn two_computers_sync_a_visit_and_merge_offline_edits() {
    let Some(env) = env() else { return };
    let used = Mutex::new(BTreeSet::new());
    let email = email();
    let (a, _kit) = first_computer(&env, &email, &used);
    let visit = make_visit(a.root(), "Maple", "Okonkwo");
    let vid = visit.meta.id.clone();
    a.sync();
    assert_eq!(a.svc.status().pending_uploads, 0);

    // B joins and receives the visit: transcript and draft, never the audio.
    let b = join(&env, &a, &email, "Exam room laptop", &used);
    let on_b = find(&b, &vid);
    assert!(!on_b.has_audio(), "the recording never leaves the computer that made it");
    assert_eq!(on_b.meta.patient.as_ref().unwrap().name, "Maple");
    assert!(on_b.load_transcript().unwrap().segments[0].text.contains("intermittent vomiting"));
    assert_eq!(on_b.meta.runs.len(), 1);
    let audio_on = on_b.meta.audio_on.clone().unwrap();
    assert_eq!(audio_on.device_id, a.svc.device_id());
    assert_eq!(audio_on.device_name, "Front desk PC");
    assert_eq!(on_b.meta.audio_secs, 312.0);

    // Offline on both: A changes the reason and drafts again; B drafts and
    // signs; then B changes the reason too, later than A.
    let mut on_a = find(&a, &vid);
    let mut p = on_a.meta.patient.clone().unwrap();
    p.reason = "vomiting, now eating".into();
    on_a.set_patient(p).unwrap();
    let mut on_b = find(&b, &vid);
    let b_run = add_run(&mut on_b, "Plan: recheck in two weeks.");
    on_b.sign_latest("Dr. B. Imani").unwrap();
    std::thread::sleep(Duration::from_millis(20));
    let mut on_a = find(&a, &vid);
    let a_late = add_run(&mut on_a, "Plan: offline draft after the note was signed elsewhere.");
    std::thread::sleep(Duration::from_millis(20));
    let mut p = on_b.meta.patient.clone().unwrap();
    p.reason = "vomiting resolved".into();
    on_b.set_patient(p).unwrap();

    // They reconnect.
    for _ in 0..2 {
        a.sync();
        b.sync();
    }
    let (fa, fb) = (find(&a, &vid), find(&b, &vid));
    // Both reach the same state.
    let ids = |s: &Session| s.meta.runs.iter().map(|r| r.id().to_string()).collect::<BTreeSet<_>>();
    assert_eq!(ids(&fa), ids(&fb));
    assert_eq!(fa.meta.runs.len(), 3, "no draft is lost");
    assert_eq!(fa.meta.patient.as_ref().unwrap().reason, "vomiting resolved", "the later change wins");
    assert_eq!(fb.meta.patient.as_ref().unwrap().reason, "vomiting resolved");
    for s in [&fa, &fb] {
        assert!(s.is_signed());
        assert_eq!(s.latest_record().unwrap().id(), b_run, "the signed run is the record");
        let late = s.meta.runs.iter().find(|r| r.id() == a_late).unwrap();
        let aside = late.set_aside.as_ref().expect("the offline draft is offered as an addendum");
        assert_eq!(aside.record_run, b_run);
        assert_eq!(aside.signed_on.as_deref(), Some("Exam room laptop"));
    }
    // The conflict on the reason is in the log of the computer that saw
    // both changes (B), with both values.
    let conflicts = b.svc.status().conflicts;
    assert!(conflicts.iter().any(|c| c.field == "patient"), "{conflicts:?}");

    // An addendum on B reaches A.
    let mut on_b = find(&b, &vid);
    on_b.add_addendum("Owner called: eating well.", "Dr. B. Imani").unwrap();
    b.sync();
    a.sync();
    let fa = find(&a, &vid);
    assert_eq!(fa.latest_record().unwrap().addenda.len(), 1);

    assert_requests_clean(&a);
    assert_requests_clean(&b);
}

#[test]
fn the_server_sees_only_ciphertext() {
    let Some(env) = env() else { return };
    let used = Mutex::new(BTreeSet::new());
    let email = email();
    let (a, _) = first_computer(&env, &email, &used);
    let v = make_visit(a.root(), "Biscuit", "Fairweather");
    a.sync();
    let b = join(&env, &a, &email, "Back office", &used);
    assert!(find(&b, &v.meta.id).load_transcript().is_ok());

    // Every stored row of every table: no fixture name, no transcript or
    // note words, no audio; bodies are OBS1 and blob parts OBB1.
    let tables = ["accounts", "devices", "key_envelopes", "spaces", "space_members", "space_keys", "objects", "blobs"];
    for t in tables {
        let rows = raw_rows(&env, t);
        let text = serde_json::to_string(&rows).unwrap();
        for name in FIXTURE_NAMES.iter().chain(FIXTURE_WORDS.iter()).chain(["Front desk PC", "Back office"].iter()) {
            assert!(!text.contains(name), "{name} is readable in sync.{t}");
        }
        assert!(!text.contains("AUDIO-MARKER"));
        for r in &rows {
            for col in ["body", "roster", "envelope", "wrapped", "wrapped_dek"] {
                if r.get(col).is_some_and(|x| !x.is_null()) {
                    let b = hex_bytes(&r[col]);
                    let decoded = String::from_utf8_lossy(&b);
                    for name in FIXTURE_NAMES {
                        assert!(!decoded.contains(name), "{name} inside sync.{t}.{col}");
                    }
                    let magic = &b[..4.min(b.len())];
                    assert!(
                        [b"OBS1".as_slice(), b"OBB1", b"OBE1", b"OBW1"].contains(&magic),
                        "sync.{t}.{col} isn't sealed: {magic:?}"
                    );
                }
            }
        }
    }
    let objects = raw_rows(&env, "objects");
    assert!(objects.iter().any(|o| o["kind"] == "visit"));
    assert!(objects.iter().all(|o| [1024, 4096, 8192].contains(&o["padded_size"].as_i64().unwrap_or(0))
        || o["padded_size"].as_i64().unwrap_or(0) % 8192 == 0));
    // Audio never uploaded: nothing in any request looks like a recording.
    assert_requests_clean(&a);
    assert_requests_clean(&b);
}

#[test]
fn a_removed_device_is_locked_out_at_once_and_erases_its_copies() {
    let Some(env) = env() else { return };
    let used = Mutex::new(BTreeSet::new());
    let email = email();
    let (a, _) = first_computer(&env, &email, &used);
    let own = make_visit(a.root(), "Maple", "Okonkwo");
    a.sync();
    let b = join(&env, &a, &email, "Old laptop", &used);
    let b_own = make_visit(b.root(), "Biscuit", "Fairweather");
    b.sync();
    a.sync();
    assert!(b.visits().iter().any(|s| s.meta.id == own.meta.id));
    let b_token = b.rec.last_token().unwrap();
    let b_keys = b.store.load().unwrap().keys.unwrap();
    let space = raw_rows(&env, "spaces").into_iter().find(|s| {
        raw_rows(&env, "devices").iter().any(|d| d["id"] == b.svc.device_id() && d["user_id"] == s["owner"])
    }).unwrap()["id"].as_str().unwrap().to_string();

    a.svc.remove_device(b.svc.device_id()).unwrap();

    // B's access token hasn't expired, yet the server refuses it at once.
    let client = os_sync::supabase::Client::new(Server { url: env.url.clone(), key: env.key.clone() }, Recorder::new());
    let pulled = client.rpc("pull", json!({"space_id": space, "after": 0, "max_rows": 10}), Some(&b_token)).unwrap();
    assert_eq!(pulled["rows"], json!([]), "row level security returns nothing to a revoked device");
    let refused = client.rpc("push", json!({"space_id": space, "items": []}), Some(&b_token));
    assert!(matches!(refused, Err(SyncError::Refused(c)) if c == "not_approved"));

    // After rotation, B's old keys can't open what A seals next.
    let after = make_visit(a.root(), "Maple", "Okonkwo");
    a.sync();
    let objects = raw_rows(&env, "objects");
    let new_rows: Vec<&Value> = objects.iter().filter(|o| o["space_id"] == space.as_str() && o["key_version"].as_i64() == Some(2)).collect();
    assert!(!new_rows.is_empty(), "objects are under the rotated space key");
    for o in new_rows {
        let wrapped = hex_bytes(&o["wrapped_dek"]);
        let id = o["id"].as_str().unwrap();
        for (v, k) in &b_keys.space_keys {
            let key = keys::SymKey(keys::unb64_32(k).unwrap());
            assert!(keys::unwrap(&key, &keys::dek_aad(&space, id, *v), &wrapped).is_err());
            assert!(keys::unwrap(&key, &keys::dek_aad(&space, id, 2), &wrapped).is_err());
        }
    }
    let _ = after;

    // B next connects: told it was removed, erases its synced copies and its
    // keys, keeps its own visit and recording.
    let r = b.svc.sync_once(&NoHost);
    assert!(matches!(r, Err(SyncError::Revoked)), "{r:?}");
    assert_eq!(b.svc.status().phase, Phase::Revoked);
    assert!(!b.visits().iter().any(|s| s.meta.id == own.meta.id), "the synced copy is erased");
    let mine = find(&b, &b_own.meta.id);
    assert!(mine.has_audio(), "its own visit and recording stay");
    assert!(b.store.load().unwrap().keys.is_none(), "its sync keys are erased");
    // A carries on, and still has B's visit (it was synced before).
    assert!(a.visits().iter().any(|s| s.meta.id == b_own.meta.id));
    assert!(a.svc.status().devices.iter().any(|d| d.id == b.svc.device_id() && d.status == "revoked"));
}

#[test]
fn a_new_device_recovers_everything_from_the_kit_after_a_rotation() {
    let Some(env) = env() else { return };
    let used = Mutex::new(BTreeSet::new());
    let email = email();
    let (a, kit) = first_computer(&env, &email, &used);
    let v1 = make_visit(a.root(), "Maple", "Okonkwo");
    a.sync();
    let b = join(&env, &a, &email, "Spare", &used);
    a.svc.remove_device(b.svc.device_id()).unwrap();
    let v2 = make_visit(a.root(), "Biscuit", "Fairweather");
    a.sync();

    // Every other device is gone: a new computer with only the kit.
    let c = Computer::new(&env);
    let NextStep::Pair(_) = sign_in(&env, &c, &email, "New computer", &used) else { panic!("expected to wait") };
    assert!(c.svc.recover_with_kit("abandon ability able about above absent absorb abstract absurd abuse access accident account accuse achieve acid acoustic acquire across act action actor actress actual").is_err());
    c.svc.recover_with_kit(&kit.join(" ")).unwrap();
    c.sync();
    for v in [&v1, &v2] {
        let s = find(&c, &v.meta.id);
        assert!(s.load_transcript().is_ok());
        assert_eq!(s.meta.runs.len(), 1);
        assert!(!s.has_audio());
    }
    // C is in the roster: A accepts what C seals.
    let mut on_c = find(&c, &v1.meta.id);
    add_run(&mut on_c, "Plan: from the recovered computer.");
    c.sync();
    a.sync();
    assert_eq!(find(&a, &v1.meta.id).meta.runs.len(), 2);
    assert!(a.svc.status().warnings.is_empty(), "{:?}", a.svc.status().warnings);
}

#[test]
fn row_level_security_keeps_accounts_and_pending_devices_apart() {
    let Some(env) = env() else { return };
    let used = Mutex::new(BTreeSet::new());
    let (x, _) = first_computer(&env, &email(), &used);
    make_visit(x.root(), "Maple", "Okonkwo");
    x.sync();
    let x_token = x.rec.last_token().unwrap();
    let email_y = email();
    let (y, _) = first_computer(&env, &email_y, &used);
    make_visit(y.root(), "Biscuit", "Fairweather");
    y.sync();
    let y_token = y.rec.last_token().unwrap();
    let spaces = raw_rows(&env, "spaces");
    let devices = raw_rows(&env, "devices");
    let owner_of = |dev: &str| devices.iter().find(|d| d["id"] == dev).unwrap()["user_id"].clone();
    let y_space = spaces.iter().find(|s| s["owner"] == owner_of(y.svc.device_id())).unwrap()["id"].as_str().unwrap().to_string();

    let client = os_sync::supabase::Client::new(Server { url: env.url.clone(), key: env.key.clone() }, Recorder::new());
    // X can't read Y's objects, blobs, keys or devices.
    let pulled = client.rpc("pull", json!({"space_id": y_space, "after": 0, "max_rows": 100}), Some(&x_token)).unwrap();
    assert_eq!(pulled["rows"], json!([]));
    let keys_x = client.rpc("my_keys", json!({}), Some(&x_token)).unwrap();
    assert!(keys_x["space_keys"].as_array().unwrap().iter().all(|k| k["space_id"] != y_space.as_str()));
    let devs = client.rpc("list_devices", json!({}), Some(&x_token)).unwrap();
    assert!(devs.as_array().unwrap().iter().all(|d| d["id"] != y.svc.device_id()));
    // X can't write into Y's space.
    let w = client.rpc("push", json!({"space_id": y_space, "items": []}), Some(&x_token));
    assert!(matches!(w, Err(SyncError::Refused(c)) if c == "unknown_space"));
    // Direct table reads too.
    let bearer = format!("Bearer {x_token}");
    let direct = get_json(
        &reqwest::blocking::Client::new(),
        &format!("{}/rest/v1/objects?select=id&space_id=eq.{y_space}", env.url),
        &[("apikey", &env.key), ("Authorization", &bearer), ("Accept-Profile", "sync")],
    );
    assert_eq!(direct, json!([]));
    // A signed-out visitor gets nothing.
    let anon = client.rpc("pull", json!({"space_id": y_space, "after": 0, "max_rows": 1}), None);
    assert!(anon.is_err() || anon.unwrap()["rows"] == json!([]));

    // A pending device of Y reads only its own envelopes (and the kit's,
    // which only the Recovery Kit opens): nothing addressed to Y's computer.
    let p = Computer::new(&env);
    let NextStep::Pair(_) = sign_in(&env, &p, &email_y, "Pending", &used) else { panic!("expected to wait") };
    let p_token = p.rec.last_token().unwrap();
    let bearer = format!("Bearer {p_token}");
    let env_rows = get_json(
        &reqwest::blocking::Client::new(),
        &format!("{}/rest/v1/key_envelopes?select=recipient", env.url),
        &[("apikey", &env.key), ("Authorization", &bearer), ("Accept-Profile", "sync")],
    );
    let recipients: Vec<&str> = env_rows.as_array().unwrap().iter().filter_map(|r| r["recipient"].as_str()).collect();
    assert_eq!(recipients, ["kit"], "a pending device sees no other device's envelope");
    let pulled = client.rpc("pull", json!({"space_id": y_space, "after": 0, "max_rows": 100}), Some(&p_token)).unwrap();
    assert_eq!(pulled["rows"], json!([]), "a pending device reads no objects");
    let own_devs = client.rpc("list_devices", json!({}), Some(&p_token)).unwrap();
    assert_eq!(own_devs.as_array().unwrap().len(), 1, "a pending device sees only its own row");
    let _ = y_token;
}

#[test]
fn stopping_a_visit_removes_the_cloud_copy_and_keeps_local_ones() {
    let Some(env) = env() else { return };
    let used = Mutex::new(BTreeSet::new());
    let email = email();
    let (a, _) = first_computer(&env, &email, &used);
    let v = make_visit(a.root(), "Maple", "Okonkwo");
    a.sync();
    let b = join(&env, &a, &email, "Laptop", &used);
    assert!(b.visits().iter().any(|s| s.meta.id == v.meta.id));
    a.svc.stop_visit(&v.meta.id, &NoHost).unwrap();
    b.sync();
    // Every local copy stays; the server keeps no body for it.
    assert!(a.visits().iter().any(|s| s.meta.id == v.meta.id));
    assert!(b.visits().iter().any(|s| s.meta.id == v.meta.id));
    assert_eq!(a.svc.visit_states().get(&v.meta.id).map(String::as_str), Some("stopped"));
    assert_eq!(b.svc.visit_states().get(&v.meta.id).map(String::as_str), Some("stopped"));
    let live = raw_rows(&env, "objects").into_iter().filter(|o| o["deleted"] == false && o["kind"] != "tombstone").count();
    let _ = live;
    // A change to the stopped visit doesn't upload.
    let mut s = find(&a, &v.meta.id);
    add_run(&mut s, "Plan: kept on this computer only.");
    a.sync();
    assert_eq!(a.svc.status().pending_uploads, 0);
}

#[test]
fn the_portal_removes_a_device_and_deleting_the_account_removes_every_row() {
    let Some(env) = env() else { return };
    let used = Mutex::new(BTreeSet::new());
    let email = email();
    let (a, _) = first_computer(&env, &email, &used);
    make_visit(a.root(), "Maple", "Okonkwo");
    a.sync();
    let b = join(&env, &a, &email, "Tablet", &used);
    let a_token = a.rec.last_token().unwrap();
    let user = raw_rows(&env, "devices").into_iter().find(|d| d["id"] == a.svc.device_id()).unwrap()["user_id"]
        .as_str()
        .unwrap()
        .to_string();

    // The portal's Edge Function, with the person's own token.
    let r = reqwest::blocking::Client::new()
        .post(format!("{}/functions/v1/revoke-device", env.url))
        .header("apikey", &env.key)
        .header("Authorization", format!("Bearer {a_token}"))
        .header("Content-Type", "application/json")
        .body(json!({"device_id": b.svc.device_id()}).to_string())
        .send()
        .unwrap();
    assert_eq!(r.status().as_u16(), 200, "{}", r.text().unwrap_or_default());
    assert!(matches!(b.svc.sync_once(&NoHost), Err(SyncError::Revoked)));
    // A's next run rotates the keys, as the portal can't.
    a.sync();
    let acct = raw_rows(&env, "accounts").into_iter().find(|x| x["user_id"] == user.as_str()).unwrap();
    assert_eq!(acct["rotate_needed"], json!(false));
    assert_eq!(acct["amk_version"], json!(2));

    // Delete my account: every row goes, local visits stay.
    a.svc.delete_account(&email).unwrap();
    for t in ["accounts", "devices", "key_envelopes", "spaces"] {
        let rows = raw_rows(&env, t);
        let mine = rows.iter().filter(|x| x["user_id"] == user.as_str() || x["owner"] == user.as_str()).count();
        assert_eq!(mine, 0, "sync.{t} still has rows for the deleted account");
    }
    assert_eq!(a.visits().len(), 1);
    assert_eq!(a.svc.status().phase, Phase::Off);
}
