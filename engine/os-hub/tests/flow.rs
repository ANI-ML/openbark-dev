//! The sync client (os-sync) against the hub, in one process: join codes,
//! the Recovery Kit, a second device approved with its 6 words, a visit
//! arriving without its audio, a removed device locked out with the keys
//! rotated, recovery from the kit, and the hub's store holding ciphertext
//! only. Requests go straight to `Hub::handle` (TLS has its own test in
//! src/tests.rs; the hub end-to-end workflow runs the real listener).

use std::path::Path;
use std::sync::Arc;

use os_engine::session::{PatientInfo, RunInfo, Session};
use os_hub::{Hub, HubRequest, Limits, Settings};
use os_sync::hub::Join;
use os_sync::state::{MemoryStore, Phase};
use os_sync::supabase::{Request, Response, Transport};
use os_sync::{NextStep, NoHost, SyncError, SyncService, keys};
use serde_json::{Value, json};

const NAMES: [&str; 6] =
    ["Biscuit", "Fairweather", "intermittent vomiting", "bland diet", "Front desk PC", "Exam room"];

fn vault_on() {
    os_engine::vault::use_key([42; 32], os_engine::vault::KeySource::Environment, None);
}

/// Straight to the hub, as the listener would pass it on.
struct Direct(Arc<Hub>);

impl Transport for Direct {
    fn send(&self, req: &Request) -> Result<Response, SyncError> {
        let rest = req.url.strip_prefix(os_sync::hub::SCHEME).expect("a hub url");
        let path = &rest[rest.find('/').expect("a path")..];
        let headers = req.headers.iter().filter(|(k, _)| k != os_sync::hub::H_PIN).cloned().collect();
        let r = self.0.handle(
            "127.0.0.1".parse().unwrap(),
            &HubRequest {
                method: req.method.to_string(),
                path: path.to_string(),
                headers,
                body: req.body.clone().unwrap_or_default(),
            },
        );
        Ok(Response { status: r.status, body: r.body })
    }
}

struct Computer {
    svc: SyncService,
    dir: tempfile::TempDir,
}

impl Computer {
    fn new(hub: &Arc<Hub>) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let svc =
            SyncService::open(dir.path(), Arc::new(Direct(hub.clone())), Box::new(MemoryStore::default()))
                .unwrap();
        Self { svc, dir }
    }

    fn root(&self) -> &Path {
        self.dir.path()
    }

    fn sync(&self) {
        let r = self.svc.sync_once(&NoHost);
        assert!(r.is_ok(), "{r:?}");
    }

    fn visits(&self) -> Vec<Session> {
        os_engine::session::list(self.root())
    }
}

fn join(code: &str) -> Join {
    Join { host: "127.0.0.1".into(), port: 47110, fingerprint: Some([7; 32]), check: None, code: code.into() }
}

/// A visit as a computer saves it: a recording, a transcript and a draft.
fn make_visit(root: &Path, pet: &str, family: &str) -> Session {
    let mut s = Session::create_live(root, &format!("{pet} {family}")).unwrap();
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
    let device = os_engine::device::for_root(root);
    let dir = format!("runs/001_{}_test-model", device.tag);
    std::fs::create_dir_all(s.dir.join(&dir)).unwrap();
    let note =
        json!({"note": {"plan": "Plan: bland diet for three days."}, "faithfulness": {"passed": true}});
    os_engine::vault::write(&s.dir.join(&dir).join("note.json"), note.to_string().as_bytes()).unwrap();
    os_engine::vault::write(&s.dir.join(&dir).join("note.md"), b"## Plan\nbland diet\n").unwrap();
    let run: RunInfo = serde_json::from_value(json!({
        "dir": dir, "run_id": uuid::Uuid::now_v7().to_string(), "device": device.tag, "llm": "test-model",
        "template": "soap@0.1.0", "created_at": chrono::Local::now(), "faithfulness_passed": true, "total_ms": 10,
    }))
    .unwrap();
    s.meta.runs.push(run);
    s.save().unwrap();
    s
}

/// What the server role can see holds no names, no text and no audio.
fn assert_ciphertext_only(dir: &Path) {
    let seen = Hub::inspect(dir).unwrap();
    let text = seen.to_string();
    for w in NAMES.iter().chain(["AUDIO-MARKER"].iter()) {
        assert!(!text.contains(w), "{w} is readable on the hub");
    }
    let mut n = 0;
    for list in ["object_bodies", "blob_bodies"] {
        for item in seen[list].as_array().unwrap() {
            let b = keys::unb64(item["body"].as_str().unwrap()).unwrap();
            assert!(b.starts_with(b"OBS1") || b.starts_with(b"OBB1"), "{list}: {:?}", &b[..4]);
            for w in NAMES.iter().chain(["AUDIO-MARKER-NEVER-SYNCS"].iter()) {
                assert!(!b.windows(w.len()).any(|x| x == w.as_bytes()), "{w} in a stored body");
            }
            n += 1;
        }
    }
    assert!(n > 0, "the hub stored something");
    let roster = seen["accounts"].as_object().unwrap().values().next().unwrap()["roster"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(keys::unb64(&roster).unwrap().starts_with(b"OBS1"), "the roster is sealed");
}

#[test]
fn devices_join_sync_and_are_removed_through_the_hub() {
    vault_on();
    let hub_dir = tempfile::tempdir().unwrap();
    let hub = Arc::new(Hub::open(hub_dir.path(), &Settings::default(), Limits::default()).unwrap());

    // The first device makes the account with a join code.
    let a = Computer::new(&hub);
    assert!(matches!(
        a.svc.join_hub(&join("ABCDEFGH"), "Front desk PC", None, None, false),
        Err(SyncError::User(_))
    ));
    assert_eq!(a.svc.status().phase, Phase::Off, "nothing half-joined");
    let code = hub.new_join_code().unwrap().code;
    let NextStep::Kit(kit) = a.svc.join_hub(&join(&code), "Front desk PC", None, None, false).unwrap() else {
        panic!("expected a new account")
    };
    let answers: Vec<(usize, String)> = kit.check.iter().map(|p| (*p, kit.words[p - 1].clone())).collect();
    a.svc.confirm_kit(&answers).unwrap();
    assert_eq!(a.svc.status().phase, Phase::Active);
    assert!(a.svc.status().hub.is_some());
    // The same code can't be used again.
    let x = Computer::new(&hub);
    assert!(x.svc.join_hub(&join(&code), "Stranger", None, None, false).is_err());

    // The same person's second device: a new code, then A types its words.
    let b = Computer::new(&hub);
    let code = hub.new_join_code().unwrap().code;
    let NextStep::Pair(pair) = b.svc.join_hub(&join(&code), "Exam room laptop", None, None, true).unwrap()
    else {
        panic!("expected to wait for approval")
    };
    assert!(a.svc.check_pairing("abandon ability able about above absent").is_err());
    assert_eq!(a.svc.check_pairing(&pair.words.join(" ")).unwrap().name, "Exam room laptop");
    let approved = a.svc.approve_pairing(&pair.words.join(" ")).unwrap();
    let confirm = approved.confirm.expect("a hub approval shows a confirmation code");
    b.sync();
    // Approved, but nothing syncs until the person types A's code.
    assert_eq!(b.svc.status().phase, Phase::Confirm, "{:?}", b.svc.status().last_error);
    assert!(
        b.svc.status().devices.iter().any(|d| d.name == "Front desk PC"),
        "B shows whose account it joined"
    );
    assert!(b.svc.confirm_pairing("AAAA-AAAA").is_err());
    b.svc.confirm_pairing(&confirm.to_lowercase()).unwrap();
    assert_eq!(b.svc.status().phase, Phase::Active);
    b.sync();

    // A visit from A arrives on B transcribed, with its draft, never its audio.
    let s = make_visit(a.root(), "Biscuit", "Fairweather");
    a.sync();
    b.sync();
    let got = b.visits().into_iter().find(|v| v.meta.id == s.meta.id).expect("the visit reached B");
    assert!(got.transcript_path().is_file());
    assert!(!got.has_audio());
    assert_eq!(got.meta.runs.len(), 1);
    assert_ciphertext_only(hub_dir.path());

    // A removes B: the hub refuses B at once, B erases its synced copy, and
    // the keys rotate.
    let b_id = b.svc.device_id();
    a.svc.remove_device(&b_id).unwrap();
    let r = b.svc.sync_once(&NoHost);
    assert!(matches!(r, Err(SyncError::Revoked)), "{r:?}");
    assert_eq!(b.svc.status().phase, Phase::Revoked);
    assert!(b.visits().iter().all(|v| v.meta.id != s.meta.id), "B kept a synced copy");
    a.sync();
    assert!(hub.devices().iter().any(|d| d.id == b_id && d.status == "revoked"));

    // B comes back with the Recovery Kit (and a new join code).
    let code = hub.new_join_code().unwrap().code;
    assert!(matches!(
        b.svc.join_hub(&join(&code), "Replacement laptop", None, None, true).unwrap(),
        NextStep::Pair(_)
    ));
    b.svc.recover_with_kit(&kit.words.join(" ")).unwrap();
    b.sync();
    assert!(b.visits().iter().any(|v| v.meta.id == s.meta.id), "{:?}", b.svc.status().last_error);
    assert_ciphertext_only(hub_dir.path());

    // Turned off and on again on the same hub: no code, no approval.
    a.svc.turn_off().unwrap();
    assert!(matches!(a.svc.join_hub(&join("ZZZZZZZZ"), "", None, None, false).unwrap(), NextStep::Active));
    a.sync();
    let v: Value = serde_json::to_value(a.svc.status()).unwrap();
    assert_eq!(v["phase"], "active");
}

/// The review's finding 1: a device of another account that sees a person's
/// 6 words approves their new device first. The new device ends up in the
/// wrong account but syncs nothing, because only the person's own device
/// shows the code it asks for; it starts over and joins the right account.
#[test]
fn another_account_with_the_words_cant_take_a_new_device() {
    vault_on();
    let hub_dir = tempfile::tempdir().unwrap();
    let hub = Arc::new(Hub::open(hub_dir.path(), &Settings::default(), Limits::default()).unwrap());
    let first = |c: &Computer, name: &str| {
        let code = hub.new_join_code().unwrap().code;
        let NextStep::Kit(kit) = c.svc.join_hub(&join(&code), name, None, None, false).unwrap() else {
            panic!("expected a new account")
        };
        let answers: Vec<(usize, String)> =
            kit.check.iter().map(|p| (*p, kit.words[p - 1].clone())).collect();
        c.svc.confirm_kit(&answers).unwrap();
    };
    let a = Computer::new(&hub);
    first(&a, "Front desk PC");
    let x = Computer::new(&hub);
    first(&x, "Someone else's laptop");

    // The person's new device, with a visit of its own.
    let b = Computer::new(&hub);
    let local = make_visit(b.root(), "Juniper", "Ashdown");
    let objects_before = Hub::inspect(hub_dir.path()).unwrap()["object_bodies"].as_array().unwrap().len();
    let code = hub.new_join_code().unwrap().code;
    let NextStep::Pair(pair) = b.svc.join_hub(&join(&code), "Exam room laptop", None, None, true).unwrap()
    else {
        panic!("expected to wait for approval")
    };
    let words = pair.words.join(" ");
    // X saw the words and approves first, into its own account.
    let x_code = x.svc.approve_pairing(&words).unwrap().confirm.unwrap();
    assert!(a.svc.approve_pairing(&words).is_err(), "already taken");
    for _ in 0..3 {
        b.sync();
    }
    assert_eq!(b.svc.status().phase, Phase::Confirm);
    assert!(
        b.svc.status().devices.iter().any(|d| d.name == "Someone else's laptop"),
        "B shows it was put in someone else's account"
    );
    // Nothing of B's left B.
    let objects_now = Hub::inspect(hub_dir.path()).unwrap()["object_bodies"].as_array().unwrap().len();
    assert_eq!(objects_now, objects_before, "B uploaded before it was confirmed");
    // The person has no code that matches X's account (A never approved).
    assert!(b.svc.confirm_pairing("AAAA-AAAA").is_err());
    assert!(b.svc.turn_off().is_err(), "can't skip the check by turning sync off and on");
    // (X's own code would match: X would have to give it to the person.)
    assert_eq!(x_code.len(), 9);
    // B starts over: it leaves X's account and joins the person's.
    let old_id = b.svc.device_id();
    b.svc.start_over().unwrap();
    assert_eq!(b.svc.status().phase, Phase::Off);
    assert!(hub.devices().iter().any(|d| d.id == old_id && d.status == "revoked"), "B left X's account");
    let code = hub.new_join_code().unwrap().code;
    let NextStep::Pair(pair) = b.svc.join_hub(&join(&code), "Exam room laptop", None, None, true).unwrap()
    else {
        panic!("expected to wait for approval")
    };
    let a_code = a.svc.approve_pairing(&pair.words.join(" ")).unwrap().confirm.unwrap();
    b.sync();
    b.svc.confirm_pairing(&a_code).unwrap();
    b.sync();
    a.sync();
    assert!(a.visits().iter().any(|v| v.meta.id == local.meta.id), "{:?}", b.svc.status().last_error);
    assert!(x.visits().iter().all(|v| v.meta.id != local.meta.id));
}
