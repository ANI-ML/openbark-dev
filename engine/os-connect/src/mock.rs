//! A tiny HTTP server on loopback that stands in for Twilio in tests. It
//! records every request and answers from a closure. No real network.

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
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
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
