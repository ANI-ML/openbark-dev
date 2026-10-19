//! The secret never shows up in logs, errors, debug output, anything meant
//! for Settings or the phone window, or any part of a request to Twilio
//! except the `Authorization` header (docs/plans/telephony.md, step 3).

use std::io::Write;
use std::sync::{Arc, Mutex};

use crate::mock::{MockServer, Reply};
use crate::secrets::{Secret, SecretId};
use crate::settings::tests::{APP, BIN_OUT, sample};
use crate::store::ConnectStore;
use crate::test_support::{MemoryKeychain, vault_on};
use crate::twilio::TwilioClient;

const SECRET: &str = "SeCrEt-do-not-leak-7f3a9c21e5b8d4";

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl Write for Capture {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn the_secret_is_never_printed_logged_or_sent_anywhere_but_the_auth_header() {
    let _v = vault_on();
    let logs = Capture::default();
    let sink = logs.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_ansi(false)
        .with_writer(move || sink.clone())
        .finish();

    let mut seen: Vec<String> = Vec::new();
    let mut requests = Vec::new();
    tracing::subscriber::with_default(subscriber, || {
        // Storage, with the keychain failing so the sealed file and its warning are used.
        let dir = tempfile::tempdir().unwrap();
        let keychain = MemoryKeychain::unavailable();
        let store = ConnectStore::with_keychain(dir.path().to_path_buf(), Box::new(keychain));
        let mut settings = sample();
        settings.twiml_app_sid = Some(APP.into());
        store.save_twilio(settings).unwrap();
        let secret = Secret::new(SECRET);
        seen.push(format!("{:?}", store.save_secret(SecretId::TwilioApiKeySecret, &secret)));
        let status = store.secret_status(SecretId::TwilioApiKeySecret).unwrap();
        seen.push(format!("{status:?}"));
        seen.push(serde_json::to_string(&status).unwrap());
        let creds = store.twilio_credentials().unwrap().unwrap();
        seen.push(format!("{creds:?} {creds:#?}"));
        seen.push(format!("{store:?} {secret} {secret:?}"));
        let token = creds.voice_token(1_792_324_800).unwrap();
        seen.push(format!("{token:?}"));
        seen.push(token.jwt().to_string());
        seen.push(format!("{:?}", Secret::new("short").hint()));
        seen.push(format!(
            "{:?}",
            store.save_secret(SecretId::TwilioApiKeySecret, &Secret::new("has space"))
        ));

        // REST: success and every kind of failure.
        let server = MockServer::start(|r| {
            if r.target.contains("/Calls/") {
                Reply::json(500, r#"{"code":20500,"message":"Internal error","status":500}"#.into())
            } else if r.target.contains("FriendlyName") {
                Reply::json(401, r#"{"code":20003,"message":"Authenticate","status":401}"#.into())
            } else {
                Reply::ok(r#"{"applications":[],"next_page_uri":null}"#.into())
            }
        });
        let client = creds.client().unwrap();
        seen.push(format!("{client:?}"));
        let client = TwilioClient::with_base_url(
            &server.base,
            &creds.settings.account_sid,
            &creds.settings.api_key_sid,
            creds.secret.clone(),
        )
        .unwrap();
        seen.push(format!("{client:?} {client:#?}"));
        seen.push(format!("{:?}", client.check_key()));
        for e in [
            client.ensure_app(None, BIN_OUT).unwrap_err(),
            client.end_call("CA0123456789abcdef0123456789abcdef").unwrap_err(),
        ] {
            seen.push(format!("{e} {e:?}"));
        }
        let unreachable = {
            let free = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap();
            TwilioClient::with_base_url(
                &format!("http://{free}"),
                &creds.settings.account_sid,
                &creds.settings.api_key_sid,
                creds.secret.clone(),
            )
            .unwrap()
        };
        let e = unreachable.check_key().unwrap_err();
        seen.push(format!("{e} {e:?}"));
        let e = TwilioClient::with_base_url("https://example.com", "AC1", "SK1", creds.secret.clone())
            .unwrap_err();
        seen.push(format!("{e} {e:?}"));
        requests = server.requests();

        // Disconnect.
        seen.push(format!("{:?}", store.remove_twilio()));
    });

    let logs = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
    assert!(logs.contains("keychain unavailable"), "logs were captured: {logs}");
    assert!(logs.contains("twilio"), "request logs were captured: {logs}");
    assert!(!logs.contains(SECRET), "a log line holds the secret:\n{logs}");
    for s in &seen {
        assert!(!s.contains(SECRET), "printed the secret: {s}");
    }
    assert_eq!(requests.len(), 3, "check_key, the app lookup, end_call");
    for r in &requests {
        let auth = r.header("authorization").unwrap();
        assert!(auth.starts_with("Basic "));
        let without_auth = r.raw().replace(&auth, "");
        assert!(!without_auth.contains(SECRET), "the secret left the auth header: {}", r.raw());
    }
}
