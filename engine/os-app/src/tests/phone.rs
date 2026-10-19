//! Calls through the clinic's own Twilio account (ADR-0023), against the
//! loopback fake Twilio (`os_connect::mock::fake`). Never a real account,
//! never a real call. The secret is sealed in a temporary folder: these
//! tests never touch this computer's keychain.

use os_connect::mock::fake::{FakeTwilio, sid};
use os_connect::{NoKeychain, PhonePrefs};
use os_engine::session::{CallDirection, CallInfo, ConsentOutcome};

use super::*;

/// Made-up credentials, built at run time so no credential-shaped string
/// sits in the source.
fn account() -> String {
    sid("AC", 1)
}
fn key() -> String {
    sid("SK", 2)
}
const SECRET: &str = "fake-secret-for-tests-only-0042";
const CALLER: &str = "+15555550100";
fn bin() -> String {
    format!("https://handler.twilio.com/twiml/{}", sid("EH", 3))
}

/// An App with its own connections folder (no keychain) and a fake Twilio.
fn phone_app(tmp: &Path) -> (App, FakeTwilio) {
    let mut app = app_in(tmp);
    app.connect = ConnectStore::with_keychain(tmp.join("connections"), Box::new(NoKeychain));
    let fake = FakeTwilio::start(&account(), &key(), SECRET, &[CALLER]);
    app.twilio_base = Some(fake.server.base.clone());
    (app, fake)
}

fn request(secret: &str) -> ConnectRequest {
    ConnectRequest {
        account_sid: format!(" {} ", account()),
        api_key_sid: key(),
        api_key_secret: secret.into(),
        computer_name: "Front desk".into(),
        caller_id: "+1 (555) 555-0100".into(),
        outgoing_bin_url: bin(),
    }
}

/// The error's message, for results whose value has no `Debug` (a token).
fn fail<T>(r: Result<T>) -> String {
    match r {
        Ok(_) => panic!("expected an error"),
        Err(e) => e.0,
    }
}

fn connected(app: &App, by: Option<&str>) {
    app.phone_connect(request(SECRET), by).unwrap();
    app.phone_set_prefs(PhonePrefs { enabled: true, ..PhonePrefs::default() }, by).unwrap();
}

#[test]
fn connecting_checks_with_twilio_first_and_keeps_the_secret_out_of_every_reply() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, fake) = phone_app(tmp.path());
    let before = app.phone().unwrap();
    assert!(!before.connected && !before.ready && !before.prefs.enabled, "off until connected and turned on");

    // A wrong secret: Twilio refuses, and nothing is saved.
    let e = app.phone_connect(request("not-the-right-secret-000000"), None).unwrap_err();
    assert!(e.0.contains("Twilio didn't accept this key"), "{}", e.0);
    assert!(!app.connect.settings_path().exists() && !app.connect.sealed_path().exists());

    let done = app.phone_connect(request(SECRET), None).unwrap();
    assert_eq!(done.caller_id.phone_number, CALLER);
    assert_eq!(done.app_change, os_connect::twilio::AppChange::Created);
    let view = done.phone;
    assert!(view.connected && !view.ready, "connected, but the phone stays off until turned on");
    let tw = view.twilio.as_ref().unwrap();
    assert_eq!((tw.computer_name.as_str(), tw.identity.as_str()), ("front-desk", "openbark_front_desk"));
    assert_eq!(tw.twiml_app_sid.as_deref(), Some(sid("AP", 1).as_str()));
    assert!(view.outgoing_bin.as_deref().unwrap().contains(r#"callerId="+15555550100""#));
    assert_eq!(view.secret.as_ref().unwrap().hint, "****0042");
    // The secret is sealed in the scratch folder, never in a reply or the settings file.
    assert!(app.connect.sealed_path().is_file());
    let replies =
        serde_json::to_string(&view).unwrap() + &serde_json::to_string(&app.phone().unwrap()).unwrap();
    assert!(!replies.contains(SECRET));
    assert!(!os_engine::vault::read_to_string(&app.connect.settings_path()).unwrap().contains(SECRET));
    // The fake saw the secret only in the Authorization header.
    for r in fake.server.requests() {
        assert!(!r.target.contains(SECRET) && !r.body.contains(SECRET), "{}", r.raw());
    }
    // The TwiML App points at the outgoing Bin.
    assert_eq!(fake.state.lock().unwrap().apps[0].2, bin());

    // Changing the caller ID or Bin without pasting the secret again keeps it,
    // and reuses the same TwiML App.
    let again = app.phone_connect(request(""), None).unwrap();
    assert_eq!(again.app_change, os_connect::twilio::AppChange::Unchanged);
    assert_eq!(fake.state.lock().unwrap().apps.len(), 1);
    // A different key needs its own secret.
    let mut other = request("");
    other.api_key_sid = sid("SK", 9);
    assert!(app.phone_connect(other, None).unwrap_err().0.contains("Paste the API key secret"));
}

#[test]
fn a_caller_id_twilio_doesnt_know_is_refused_with_where_to_fix_it() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, _fake) = phone_app(tmp.path());
    let mut req = request(SECRET);
    req.caller_id = "+15555550199".into();
    let e = app.phone_connect(req, None).unwrap_err();
    assert!(e.0.contains("+15555550199 isn't one of this Twilio account's numbers"), "{}", e.0);
    assert!(!app.connect.sealed_path().exists(), "nothing saved");
    let mut bad = request(SECRET);
    bad.outgoing_bin_url = "https://console.twilio.com/us1/develop/twiml-bins".into();
    assert!(app.phone_connect(bad, None).unwrap_err().0.contains("https://handler.twilio.com/twiml/"));
}

#[test]
fn twilio_errors_reach_the_clinic_in_plain_words() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, fake) = phone_app(tmp.path());
    fake.fail_next(401, 20005, "Account not active");
    let e = app.phone_connect(request(SECRET), None).unwrap_err();
    assert!(e.0.contains("your account needs attention"), "{}", e.0);
    fake.fail_next(429, 20429, "Too many requests");
    assert!(app.phone_connect(request(SECRET), None).unwrap_err().0.contains("Wait a minute"));
}

#[test]
fn only_a_veterinarian_connects_but_anyone_can_call() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, _fake) = phone_app(tmp.path());
    let (dvm, rvt, desk) = team(&app);
    for who in [&rvt, &desk] {
        let e = app.phone_connect(request(SECRET), Some(&who.id)).unwrap_err();
        assert!(e.0.contains("Only a veterinarian"), "{}", e.0);
    }
    assert!(app.phone_connect(request(SECRET), None).unwrap_err().0.contains("Choose who you are"));
    connected(&app, Some(&dvm.id));
    assert!(app.phone_set_prefs(PhonePrefs::default(), Some(&desk.id)).is_err());
    assert!(app.phone_check(Some(&rvt.id)).is_err());
    assert!(app.phone_disconnect(Some(&desk.id)).is_err());
    // Reception can place a call; the clinic's side is labelled by role.
    let call = app
        .phone_prepare(CallRequest { patient_id: None, number: Some("555 555 0142".into()) }, Some(&desk.id));
    let call = call.unwrap();
    assert_eq!(call.number.as_deref(), Some("+15555550142"));
    assert_eq!((call.clinic_label.as_str(), call.by.as_deref()), ("Clinic", Some("Lee")));
    let by_vet = app.phone_prepare(CallRequest::default(), Some(&dvm.id)).unwrap();
    assert_eq!(by_vet.clinic_label, "Vet");
    assert_eq!(app.phone_prepare(CallRequest::default(), Some(&rvt.id)).unwrap().clinic_label, "Tech");
}

#[test]
fn the_phone_stays_off_until_turned_on_and_tokens_need_it_on() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, _fake) = phone_app(tmp.path());
    let on = PhonePrefs { enabled: true, ..PhonePrefs::default() };
    assert!(app.phone_set_prefs(on.clone(), None).unwrap_err().0.contains("Connect Twilio"));
    assert!(fail(app.phone_token()).contains("isn't connected"));
    app.phone_connect(request(SECRET), None).unwrap();
    assert!(fail(app.phone_token()).contains("The phone is off"));
    assert!(app.phone_prepare(CallRequest::default(), None).unwrap_err().0.contains("The phone is off"));
    let view = app.phone_set_prefs(on, None).unwrap();
    assert!(view.ready);
    let token = app.phone_token().unwrap();
    assert_eq!(token.identity, "openbark_front_desk");
    assert!(!token.token.contains(SECRET));
    let payload = token.token.split('.').nth(1).unwrap();
    let claims: serde_json::Value = serde_json::from_slice(&base64_decode(payload)).unwrap();
    assert_eq!(claims["grants"]["voice"]["outgoing"]["application_sid"], sid("AP", 1));
    assert!(claims["grants"]["voice"].get("incoming").is_none(), "calls out only in this release");
    assert_eq!(claims["exp"].as_u64().unwrap() - claims["iat"].as_u64().unwrap(), 3600);
    // An empty announcement is refused while calls are recorded.
    let blank = PhonePrefs { enabled: true, record_calls: true, announcement: "  ".into() };
    assert!(app.phone_set_prefs(blank, None).unwrap_err().0.contains("announcement"));
}

fn base64_decode(s: &str) -> Vec<u8> {
    // URL-safe base64 without padding, as JWTs use.
    let table = |c: u8| -> u32 {
        match c {
            b'A'..=b'Z' => (c - b'A') as u32,
            b'a'..=b'z' => (c - b'a' + 26) as u32,
            b'0'..=b'9' => (c - b'0' + 52) as u32,
            b'-' => 62,
            _ => 63,
        }
    };
    let mut out = Vec::new();
    let (mut acc, mut bits) = (0u32, 0);
    for c in s.bytes() {
        acc = (acc << 6) | table(c);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    out
}

#[test]
fn calling_a_patients_owner_uses_the_number_on_file() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, _fake) = phone_app(tmp.path());
    connected(&app, None);
    let bella = app.create_patient("Bella", "Patel", "canine").unwrap();
    let none =
        app.phone_prepare(CallRequest { patient_id: Some(bella.id.clone()), number: None }, None).unwrap();
    assert_eq!(none.number, None, "no number on file: the phone window shows the dial pad");
    assert_eq!((none.patient_name.as_str(), none.last_name.as_str()), ("Bella", "Patel"));
    app.patients.set_owner_phone(&bella.id, "555-555-0142").unwrap();
    let call =
        app.phone_prepare(CallRequest { patient_id: Some(bella.id.clone()), number: None }, None).unwrap();
    assert_eq!(call.number.as_deref(), Some("+15555550142"));
    assert!(call.record_calls);
    assert_eq!(call.announcement, os_connect::DEFAULT_ANNOUNCEMENT);
    let pending = app.phone_pending().unwrap().unwrap();
    assert_eq!(pending.patient_id.as_deref(), Some(bella.id.as_str()));
    assert!(app.phone_prepare(CallRequest { patient_id: None, number: Some("12".into()) }, None).is_err());
    assert!(app.phone_prepare(CallRequest { patient_id: Some("nope".into()), number: None }, None).is_err());
    // Nothing recorded, nothing to end.
    assert!(app.end_call(CallEnd::default()).unwrap().is_none());
    assert!(fail(app.call_consent(ConsentOutcome::Agreed, None)).contains("No call"));
}

#[test]
fn check_setup_says_what_is_wrong_and_where() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, fake) = phone_app(tmp.path());
    assert!(app.phone_check(None).unwrap_err().0.contains("isn't connected"));
    connected(&app, None);
    let checks = app.phone_check(None).unwrap();
    let ids: Vec<_> = checks.iter().map(|c| (c.id, c.ok)).collect();
    assert_eq!(
        ids,
        [
            ("key", Some(true)),
            ("caller_id", Some(true)),
            ("twiml_app", Some(true)),
            ("phone_on", Some(true)),
            ("trial", None)
        ]
    );
    assert!(checks[4].message.contains("upgraded Twilio account"));
    // Someone deletes the TwiML App in the console.
    fake.state.lock().unwrap().apps.clear();
    let checks = app.phone_check(None).unwrap();
    let app_check = checks.iter().find(|c| c.id == "twiml_app").unwrap();
    assert_eq!(app_check.ok, Some(false));
    assert!(app_check.message.contains("Choose Connect again"), "{}", app_check.message);
    // The key is revoked: the remaining checks aren't run.
    fake.fail_next(401, 20003, "Authenticate");
    let checks = app.phone_check(None).unwrap();
    assert_eq!(checks[0].ok, Some(false));
    assert!(checks[0].message.contains("make a new key"));
    assert!(!checks.iter().any(|c| c.id == "caller_id"));
}

#[test]
fn disconnect_removes_the_key_and_turns_the_phone_off() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, _fake) = phone_app(tmp.path());
    connected(&app, None);
    let out = app.phone_disconnect(None).unwrap();
    assert!(out.removed.sealed_file && out.removed.settings);
    assert!(out.note.contains("delete this key in your Twilio console"));
    let view = app.phone().unwrap();
    assert!(!view.connected && !view.prefs.enabled && view.twilio.is_none() && view.secret.is_none());
    assert!(!app.connect.sealed_path().exists());
    assert!(app.phone_token().is_err());
}

#[test]
fn recording_calls_off_means_calls_start_unrecorded() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, _fake) = phone_app(tmp.path());
    connected(&app, None);
    app.phone_set_prefs(PhonePrefs { enabled: true, record_calls: false, ..PhonePrefs::default() }, None)
        .unwrap();
    let (_tx, rx) = std::sync::mpsc::channel();
    let events: Arc<dyn Events> = Arc::new(NoEvents);
    let e = app
        .start_call(CallStart { number: "+15555550142".into(), ..Default::default() }, rx, events)
        .unwrap_err();
    assert!(e.0.contains("Recording calls is off"), "{}", e.0);
    assert!(!app.call_active());
    assert!(app.list_sessions().is_empty(), "no visit");
}

#[test]
fn a_calls_note_knows_who_is_who() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app_in(tmp.path());
    let mut s = os_engine::session::Session::create_live(&app.sessions_root, "Bella").unwrap();
    s.set_patient(Patient { name: "Bella".into(), ..Default::default() }).unwrap();
    s.set_call(CallInfo {
        direction: CallDirection::Outgoing,
        number: "+15555550142".into(),
        started_at: chrono::Local::now(),
        duration_secs: 60.0,
        call_sid: None,
        clinic_label: "Vet".into(),
        declined_at_secs: None,
    })
    .unwrap();
    let context = app.note_options(&s, None).context.unwrap();
    assert!(context.starts_with("patient name Bella; this is a phone call"), "{context}");
    assert!(context.contains("Speaker 1 is the clinic (Vet), Speaker 2 is the client"));
}

// ---- review fixes (2026-10-19): announcement, deletion, folder names ----------

/// A call visit being "recorded" (audio and transcript on disk) with the
/// recorder gone, as after a crash or a failed recorder: no models needed.
fn call_in_progress(app: &App, announced: bool) -> String {
    let mut s =
        os_engine::session::Session::create_live(&app.sessions_root, crate::phone::CALL_LABEL).unwrap();
    s.set_call(CallInfo {
        direction: CallDirection::Outgoing,
        number: "+15555550142".into(),
        started_at: chrono::Local::now(),
        duration_secs: 0.0,
        call_sid: None,
        clinic_label: "Clinic".into(),
        declined_at_secs: None,
    })
    .unwrap();
    s.set_consent(RecordingConsent {
        rule: ConsentRule::AllParties,
        method: ConsentMethod::Verbal,
        confirmed_by: "Dr Lee".into(),
        confirmed_at: None,
        script: Some("This call is recorded. Is that all right?".into()),
        outcome: None,
        announced_at_secs: None,
    })
    .unwrap();
    silent_wav(&s.audio_path());
    s.save_transcript(&Transcript { segments: Vec::new() }, "parakeet", true, &Default::default()).unwrap();
    let id = crate::folder_name(&s.dir);
    *app.call.lock().unwrap() = Some(crate::phone::CallState::for_test(&id, announced));
    id
}

fn audio_left(app: &App, id: &str) -> (bool, bool) {
    let dir = app.session_dir(id).unwrap();
    (dir.join("audio.wav").exists(), dir.join("transcript.jsonl").exists())
}

/// Finding 1: hanging up before "I've read it" keeps no recording, and the
/// visit says why. Nothing claims an announcement that wasn't confirmed.
#[test]
fn a_call_hung_up_before_the_announcement_keeps_no_recording() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, _fake) = phone_app(tmp.path());
    let id = call_in_progress(&app, false);
    // "Did the client agree?" can't be answered before the announcement.
    assert!(fail(app.call_consent(ConsentOutcome::Agreed, None)).contains("Read the announcement"));
    let view = app.end_call(CallEnd { duration_secs: Some(12.0), call_sid: None }).unwrap().unwrap();
    assert_eq!(audio_left(&app, &id), (false, false), "no audio, no transcript");
    let consent = view.consent.unwrap();
    assert_eq!(consent.outcome, Some(ConsentOutcome::NotAnnounced));
    assert_eq!(consent.announced_at_secs, None, "no announcement is claimed");
    assert!(view.audio_deleted.is_some() && !view.has_audio);
    assert_eq!(view.call.unwrap().duration_secs, 12.0);
    assert!(!app.call_active());
}

/// Finding 1, the other way: once announced, hanging up keeps the recording,
/// and with no answer the client stayed on the line.
#[test]
fn an_announced_call_keeps_its_recording_at_hang_up() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, _fake) = phone_app(tmp.path());
    let id = call_in_progress(&app, false);
    app.call_announced(Some(0.0)).unwrap();
    let view = app.end_call(CallEnd::default()).unwrap().unwrap();
    assert_eq!(audio_left(&app, &id), (true, true));
    let consent = view.consent.unwrap();
    assert_eq!((consent.outcome, consent.announced_at_secs), (Some(ConsentOutcome::StayedOnLine), Some(0.0)));
    // An agreed call says so.
    let id2 = call_in_progress(&app, false);
    app.call_consent(ConsentOutcome::Agreed, Some(0.0)).unwrap();
    let view = app.end_call(CallEnd::default()).unwrap().unwrap();
    assert_eq!(view.consent.unwrap().outcome, Some(ConsentOutcome::Agreed));
    assert_eq!(audio_left(&app, &id2), (true, true));
    assert!(app.call_consent(ConsentOutcome::NotAnnounced, None).is_err(), "not a client's answer");
}

/// Finding 3: declining deletes the recording even when the recorder has
/// failed or is gone, and the visit still says the client declined.
#[test]
fn declining_deletes_the_recording_whatever_state_the_recorder_is_in() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, _fake) = phone_app(tmp.path());
    let id = call_in_progress(&app, true);
    let view = app.call_consent(ConsentOutcome::Declined, None).unwrap().unwrap();
    assert_eq!(audio_left(&app, &id), (false, false));
    assert_eq!(view.consent.as_ref().unwrap().outcome, Some(ConsentOutcome::Declined));
    assert!(view.call.as_ref().unwrap().declined_at_secs.is_some());
    // The call goes on; hanging up later keeps it deleted and sets the length.
    assert!(app.call_announced(None).unwrap_err().0.contains("already deleted"));
    let ended = app.end_call(CallEnd { duration_secs: Some(30.0), call_sid: None }).unwrap().unwrap();
    assert_eq!(ended.call.unwrap().duration_secs, 30.0);
    assert_eq!(ended.consent.unwrap().outcome, Some(ConsentOutcome::Declined));
}

/// Finding 3: a sample rate no browser produces is refused before anything starts.
#[test]
fn a_sample_rate_that_would_stop_the_recorder_is_refused() {
    assert!(check_call_sample_rate(0).is_err());
    assert!(check_call_sample_rate(7_999).is_err());
    assert!(check_call_sample_rate(200_000).is_err());
    check_call_sample_rate(48_000).unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let (app, _fake) = phone_app(tmp.path());
    connected(&app, None);
    let (_tx, rx) = std::sync::mpsc::channel();
    let start = CallStart { number: "+15555550142".into(), sample_rate: Some(0), ..Default::default() };
    let e = app.start_call(start, rx, Arc::new(NoEvents)).unwrap_err();
    assert!(e.0.contains("0 Hz"), "{}", e.0);
    assert!(app.list_sessions().is_empty() && !app.call_active());
}

/// Finding 5: the phone window closing mid-call ends the call in the engine,
/// so the next call isn't refused; an unannounced recording is deleted.
#[test]
fn closing_the_phone_window_ends_the_call() {
    let tmp = tempfile::tempdir().unwrap();
    let (app, _fake) = phone_app(tmp.path());
    assert!(app.phone_window_closed().unwrap().is_none(), "no call, nothing to do");
    let id = call_in_progress(&app, false);
    assert!(app.call_active());
    let view = app.phone_window_closed().unwrap().unwrap();
    assert!(!app.call_active(), "a new call can start");
    assert_eq!(audio_left(&app, &id), (false, false));
    assert_eq!(view.consent.unwrap().outcome, Some(ConsentOutcome::NotAnnounced));
    // Announced before the window closed: what was recorded is kept.
    let id2 = call_in_progress(&app, true);
    app.phone_window_closed().unwrap();
    assert_eq!(audio_left(&app, &id2), (true, true));
}

/// Finding 2: a call's visit folder (and so its id) never holds the number.
#[test]
fn a_calls_folder_name_holds_no_number() {
    assert!(!crate::phone::CALL_LABEL.chars().any(|c| c.is_ascii_digit()));
}

fn call_started(view: &SessionView) -> String {
    view.call.as_ref().unwrap().started_at.to_rfc3339()
}

struct NoEvents;
impl Events for NoEvents {
    fn segment(&self, _: &Segment) {}
    fn token(&self, _: &str) {}
    fn level(&self, _: f32, _: f32) {}
    fn download(&self, _: &str, _: u64, _: u64) {}
}

/// A whole call through the real recorder: needs the speech models and two
/// WAV files (the clinic's side and the client's side), so it runs only when
/// `OPENBARK_TEST_MODELS_DIR` and `OPENBARK_TEST_CALL_WAVS` ("clinic.wav;client.wav")
/// are set. Without them it says so and passes.
#[test]
fn a_recorded_call_becomes_a_two_channel_visit_and_a_declined_one_keeps_no_audio() {
    let (Some(models), Some(wavs)) =
        (std::env::var_os("OPENBARK_TEST_MODELS_DIR"), std::env::var("OPENBARK_TEST_CALL_WAVS").ok())
    else {
        eprintln!("skipped: set OPENBARK_TEST_MODELS_DIR and OPENBARK_TEST_CALL_WAVS to run a whole call");
        return;
    };
    let paths: Vec<&str> = wavs.split(';').collect();
    let clinic = os_asr::Audio::load(Path::new(paths[0])).unwrap().samples;
    let client = os_asr::Audio::load(Path::new(paths[1])).unwrap().samples;
    let tmp = tempfile::tempdir().unwrap();
    let (mut app, _fake) = phone_app(tmp.path());
    app.store = ModelStore::new(PathBuf::from(models));
    connected(&app, None);
    let bella = app.create_patient("Bella", "Patel", "canine").unwrap();
    app.phone_prepare(
        CallRequest { patient_id: Some(bella.id.clone()), number: Some("+15555550142".into()) },
        Some("Dr Lee"),
    )
    .unwrap();
    let feed = |tx: std::sync::mpsc::Sender<os_engine::live::CallBlock>| {
        // The client speaks after the clinic: the clinic's side, then the client's.
        let n = clinic.len() + client.len();
        let at = |v: &Vec<f32>, i: usize| v.get(i).copied().unwrap_or(0.0);
        for start in (0..n).step_by(1600) {
            let end = (start + 1600).min(n);
            let c: Vec<f32> = (start..end).map(|i| at(&clinic, i)).collect();
            let k: Vec<f32> = (start..end)
                .map(|i| if i >= clinic.len() { at(&client, i - clinic.len()) } else { 0.0 })
                .collect();
            tx.send(os_engine::live::CallBlock { clinic: c, client: k, sample_rate: 16_000 }).unwrap();
        }
    };
    let events: Arc<dyn Events> = Arc::new(NoEvents);
    let (tx, rx) = std::sync::mpsc::channel();
    let id = app
        .start_call(
            CallStart {
                number: "+15555550142".into(),
                patient_id: Some(bella.id.clone()),
                call_sid: Some(sid("CA", 7)),
                sample_rate: Some(16_000),
                // The window kept 3 s of audio while the engine got ready.
                since_answer_secs: Some(3.0),
            },
            rx,
            events.clone(),
        )
        .unwrap();
    feed(tx);
    app.call_consent(ConsentOutcome::Agreed, Some(0.0)).unwrap();
    let view = app.end_call(CallEnd { duration_secs: Some(42.0), call_sid: None }).unwrap().unwrap();
    assert_eq!(view.id, id);
    assert!(!id.contains("5555550142"), "the number never goes into the folder name: {id}");
    let started = chrono::DateTime::parse_from_rfc3339(&call_started(&view)).unwrap();
    assert!(
        chrono::Local::now().signed_duration_since(started).num_seconds() >= 3,
        "times count from the answer"
    );
    assert_eq!(view.source, "Phone call");
    let call = view.call.as_ref().unwrap();
    // Single-user: no role, so the clinic's side is "Clinic".
    assert_eq!(
        (call.number.as_str(), call.duration_secs, call.clinic_label.as_str()),
        ("+15555550142", 42.0, "Clinic")
    );
    assert_eq!(call.call_sid.as_deref(), Some(sid("CA", 7).as_str()));
    let consent = view.consent.unwrap();
    assert_eq!(consent.outcome, Some(ConsentOutcome::Agreed));
    let segments = view.transcript.unwrap().segments;
    assert!(
        segments.iter().any(|s| s.speaker == Some(0)) && segments.iter().any(|s| s.speaker == Some(1)),
        "{segments:?}"
    );
    assert!(segments.windows(2).all(|w| w[0].start <= w[1].start), "in time order");
    let bytes = os_engine::vault::read(&Path::new(&view.dir).join("audio.wav")).unwrap();
    assert_eq!(u16::from_le_bytes([bytes[22], bytes[23]]), 2, "two channels");
    assert!(os_engine::vault::is_encrypted(&Path::new(&view.dir).join("audio.wav")));

    // A declined call keeps the visit, not the audio.
    let (tx, rx) = std::sync::mpsc::channel();
    app.start_call(CallStart { number: "+15555550142".into(), ..Default::default() }, rx, events.clone())
        .unwrap();
    feed(tx);
    // Let the recorder finish what it was sent: declining must still leave no audio.
    std::thread::sleep(std::time::Duration::from_secs(4));
    let declined = app.call_consent(ConsentOutcome::Declined, None).unwrap().unwrap();
    assert!(!declined.has_audio && declined.audio_deleted.is_some());
    assert_eq!(declined.consent.as_ref().unwrap().outcome, Some(ConsentOutcome::Declined));
    assert!(declined.call.as_ref().unwrap().declined_at_secs.is_some());
    let ended = app.end_call(CallEnd { duration_secs: Some(90.0), call_sid: None }).unwrap().unwrap();
    assert_eq!(ended.call.unwrap().duration_secs, 90.0);
    assert!(!Path::new(&ended.dir).join("audio.wav").exists());
    assert!(ended.transcript.is_none_or(|t| t.segments.is_empty()));

    // No patient chosen: the folder is "call", never the number.
    let (tx, rx) = std::sync::mpsc::channel();
    let id = app
        .start_call(CallStart { number: "+15555550142".into(), ..Default::default() }, rx, events.clone())
        .unwrap();
    assert!(id.ends_with("_call") && !id.contains("5555550142"), "{id}");
    // The recorder fails (audio it can't resample), then the client declines:
    // what it wrote is still deleted, and the visit says so.
    tx.send(os_engine::live::CallBlock {
        clinic: vec![0.1; 1600],
        client: vec![0.1; 1600],
        sample_rate: 16_000,
    })
    .unwrap();
    tx.send(os_engine::live::CallBlock { clinic: vec![0.1; 1600], client: vec![0.1; 1600], sample_rate: 0 })
        .unwrap();
    std::thread::sleep(std::time::Duration::from_secs(2));
    let declined = app.call_consent(ConsentOutcome::Declined, None).unwrap().unwrap();
    assert!(!Path::new(&declined.dir).join("audio.wav").exists(), "a failed recorder's audio is deleted too");
    assert_eq!(declined.consent.unwrap().outcome, Some(ConsentOutcome::Declined));
    app.end_call(CallEnd::default()).unwrap();
    drop(tx);
}
