//! A tiny HTTP server on loopback that stands in for Twilio in tests. It
//! records every request and answers from a closure. No real network.
//!
//! Built for this crate's tests, and with the `mock` feature for other
//! crates' tests and the end-to-end demo (`examples/fake-twilio.rs`).

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

/// One request as the server saw it.
#[derive(Debug, Clone)]
pub struct Recorded {
    pub method: String,
    /// Path and query.
    pub target: String,
    headers: Vec<(String, String)>,
    pub body: String,
}

impl Recorded {
    /// A header's value; names are compared without case.
    pub fn header(&self, name: &str) -> Option<String> {
        self.headers.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v.clone())
    }

    /// The whole request as text, for scanning.
    pub fn raw(&self) -> String {
        let headers: Vec<String> = self.headers.iter().map(|(n, v)| format!("{n}: {v}")).collect();
        format!("{} {}\n{}\n\n{}", self.method, self.target, headers.join("\n"), self.body)
    }
}

/// The server's answer.
#[derive(Debug, Clone)]
pub struct Reply {
    pub status: u16,
    body: String,
    location: Option<String>,
}

impl Reply {
    pub fn ok(body: String) -> Self {
        Self::json(200, body)
    }

    pub fn json(status: u16, body: String) -> Self {
        Self { status, body, location: None }
    }

    pub fn redirect(to: &str) -> Self {
        Self { status: 302, body: String::new(), location: Some(to.to_string()) }
    }
}

type Handler = dyn Fn(&Recorded) -> Reply + Send + Sync + 'static;

pub struct MockServer {
    /// `http://127.0.0.1:<port>`.
    pub base: String,
    addr: SocketAddr,
    requests: Arc<Mutex<Vec<Recorded>>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl MockServer {
    pub fn start(handler: impl Fn(&Recorded) -> Reply + Send + Sync + 'static) -> Self {
        Self::start_on("127.0.0.1:0", handler)
    }

    /// The same on a chosen loopback address.
    pub fn start_on(addr: &str, handler: impl Fn(&Recorded) -> Reply + Send + Sync + 'static) -> Self {
        let listener = TcpListener::bind(addr).expect("bind loopback");
        assert!(
            listener.local_addr().is_ok_and(|a| a.ip().is_loopback()),
            "the mock listens on loopback only"
        );
        let addr = listener.local_addr().expect("local address");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let handler: Arc<Handler> = Arc::new(handler);
        let (reqs, halt) = (requests.clone(), stop.clone());
        let thread = std::thread::spawn(move || {
            for stream in listener.incoming() {
                if halt.load(Ordering::SeqCst) {
                    break;
                }
                let Ok(stream) = stream else { continue };
                serve(stream, &*handler, &reqs);
            }
        });
        Self { base: format!("http://{addr}"), addr, requests, stop, thread: Some(thread) }
    }

    /// Every request so far, in order.
    pub fn requests(&self) -> Vec<Recorded> {
        self.requests.lock().unwrap().clone()
    }
}

impl Drop for MockServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(self.addr); // wake the accept loop
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn serve(stream: TcpStream, handler: &Handler, requests: &Mutex<Vec<Recorded>>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let Ok(mut write) = stream.try_clone() else { return };
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    if reader.read_line(&mut line).unwrap_or(0) == 0 {
        return; // the wake-up connection, or a client that gave up
    }
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let target = parts.next().unwrap_or("").to_string();
    let mut headers = Vec::new();
    loop {
        let mut h = String::new();
        if reader.read_line(&mut h).unwrap_or(0) == 0 || h == "\r\n" || h == "\n" {
            break;
        }
        if let Some((n, v)) = h.trim_end().split_once(':') {
            headers.push((n.trim().to_string(), v.trim().to_string()));
        }
    }
    let len = headers
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.parse::<usize>().ok())
        .unwrap_or(0);
    let mut body = vec![0u8; len];
    if reader.read_exact(&mut body).is_err() {
        return;
    }
    let req = Recorded { method, target, headers, body: String::from_utf8_lossy(&body).into_owned() };
    let reply = handler(&req);
    requests.lock().unwrap().push(req);
    let location = reply.location.map(|l| format!("Location: {l}\r\n")).unwrap_or_default();
    let response = format!(
        "HTTP/1.1 {} Mock\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{location}Connection: close\r\n\r\n{}",
        reply.status,
        reply.body.len(),
        reply.body
    );
    let _ = write.write_all(response.as_bytes());
    let _ = write.flush();
}

/// A fake Twilio account for tests and the end-to-end demo: answers the REST
/// calls OpenBark makes (docs/network.md R9) like a real account would, with
/// state. Never a real account, never a real call.
pub mod fake {
    use std::sync::{Arc, Mutex};

    use base64::Engine as _;

    use super::{MockServer, Recorded, Reply};

    /// What the fake account holds.
    #[derive(Debug, Default)]
    pub struct State {
        /// TwiML Apps: (sid, friendly name, voice URL).
        pub apps: Vec<(String, String, String)>,
        /// The next request fails with (HTTP status, Twilio code, message).
        pub fail_next: Option<(u16, u32, String)>,
        /// Calls ended through `POST /Calls/{sid}.json`.
        pub ended: Vec<String>,
    }

    pub struct FakeTwilio {
        pub server: MockServer,
        pub state: Arc<Mutex<State>>,
    }

    /// A made-up SID: a prefix and 32 hex digits built from `n`, so no
    /// credential-shaped string sits in the source.
    pub fn sid(prefix: &str, n: u64) -> String {
        format!("{prefix}{:016x}{:016x}", 0xfeed_u64, n)
    }

    fn err(status: u16, code: u32, message: &str) -> Reply {
        let more = format!("https://www.twilio.com/docs/errors/{code}");
        let body = serde_json::json!({"code": code, "message": message, "more_info": more, "status": status});
        Reply::json(status, body.to_string())
    }

    fn app_json(sid: &str, name: &str, url: &str) -> serde_json::Value {
        serde_json::json!({"sid": sid, "friendly_name": name, "voice_url": url, "voice_method": "POST"})
    }

    fn form(body: &str) -> Vec<(String, String)> {
        reqwest::Url::parse(&format!("http://form.invalid/?{body}"))
            .map(|u| u.query_pairs().map(|(k, v)| (k.into_owned(), v.into_owned())).collect())
            .unwrap_or_default()
    }

    fn field(f: &[(String, String)], key: &str) -> Option<String> {
        f.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
    }

    impl FakeTwilio {
        /// An upgraded account `account` whose Standard key `key`/`secret`
        /// works, with these verified caller IDs.
        pub fn start(account: &str, key: &str, secret: &str, caller_ids: &[&str]) -> Self {
            Self::start_on("127.0.0.1:0", account, key, secret, caller_ids)
        }

        /// The same on a chosen loopback address (the demo's fixed port).
        pub fn start_on(addr: &str, account: &str, key: &str, secret: &str, caller_ids: &[&str]) -> Self {
            let state = Arc::new(Mutex::new(State::default()));
            let auth = format!(
                "Basic {}",
                base64::engine::general_purpose::STANDARD.encode(format!("{key}:{secret}"))
            );
            let prefix = format!("/2010-04-01/Accounts/{account}/");
            let callers: Vec<String> = caller_ids.iter().map(|c| c.to_string()).collect();
            let st = state.clone();
            let server = MockServer::start_on(addr, move |r: &Recorded| {
                let mut s = st.lock().unwrap();
                if let Some((status, code, message)) = s.fail_next.take() {
                    return err(status, code, &message);
                }
                if r.header("authorization").as_deref() != Some(auth.as_str()) {
                    return err(401, 20003, "Authenticate");
                }
                let target = r.target.split('?').next().unwrap_or("").to_string();
                let Some(rest) = target.strip_prefix(&prefix) else {
                    return err(404, 20404, "The requested resource was not found");
                };
                let post = r.method == "POST";
                if rest == "Applications.json" {
                    if !post {
                        let list: Vec<_> = s.apps.iter().map(|(sid, n, u)| app_json(sid, n, u)).collect();
                        return Reply::ok(
                            serde_json::json!({"applications": list, "next_page_uri": null}).to_string(),
                        );
                    }
                    let f = form(&r.body);
                    let name = field(&f, "FriendlyName").unwrap_or_default();
                    let url = field(&f, "VoiceUrl").unwrap_or_default();
                    let sid = sid("AP", s.apps.len() as u64 + 1);
                    s.apps.push((sid.clone(), name.clone(), url.clone()));
                    return Reply::json(201, app_json(&sid, &name, &url).to_string());
                }
                if let Some(id) = rest.strip_prefix("Applications/").and_then(|p| p.strip_suffix(".json")) {
                    let url = field(&form(&r.body), "VoiceUrl");
                    return match s.apps.iter_mut().find(|(sid, _, _)| sid == id) {
                        Some(app) => {
                            if post && let Some(u) = url {
                                app.2 = u;
                            }
                            Reply::ok(app_json(&app.0, &app.1, &app.2).to_string())
                        }
                        None => err(404, 20404, "The requested resource was not found"),
                    };
                }
                if rest == "IncomingPhoneNumbers.json" {
                    return Reply::ok(r#"{"incoming_phone_numbers":[],"next_page_uri":null}"#.into());
                }
                if rest == "OutgoingCallerIds.json" {
                    let list: Vec<_> = callers
                        .iter()
                        .enumerate()
                        .map(|(i, c)| {
                            serde_json::json!({"sid": sid("PN", i as u64 + 1), "phone_number": c, "friendly_name": "Clinic"})
                        })
                        .collect();
                    return Reply::ok(
                        serde_json::json!({"outgoing_caller_ids": list, "next_page_uri": null}).to_string(),
                    );
                }
                if post && let Some(id) = rest.strip_prefix("Calls/").and_then(|p| p.strip_suffix(".json")) {
                    s.ended.push(id.to_string());
                    return Reply::ok("{}".into());
                }
                err(404, 20404, "The requested resource was not found")
            });
            Self { server, state }
        }

        /// The next request fails with this Twilio error.
        pub fn fail_next(&self, status: u16, code: u32, message: &str) {
            self.state.lock().unwrap().fail_next = Some((status, code, message.to_string()));
        }
    }
}
