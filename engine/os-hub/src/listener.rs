//! The hub's listener (ADR-0029 sections 2 and 4; docs/network.md L1): TLS
//! 1.3 on one TCP port, IPv4 and IPv6, only while hub mode is on.
//!
//! - A connection from an address outside the allowed ranges is closed
//!   before the TLS handshake.
//! - At most 256 connections at once, 16 from one address (an IPv6 /64
//!   counts as one); 5 seconds for the handshake and for a request's
//!   headers, 20 for its body, 2 minutes for a whole connection.
//! - Before any body is read, [`Hub::admit`] checks the address and its rate
//!   limit, the call, the signature headers and their time, the device and
//!   the body's size for that call: 128 KB, 2 MB for a blob part, 12 MB only
//!   for `push` and `rotate` from a device the hub knows (at most 4 of those
//!   bodies are read at once). The signature is checked over the raw body
//!   before any JSON is parsed ([`Hub::finish`]).
//! - HTTP/1.1 only, `POST /rpc/<name>` only. At most 16 calls run at once,
//!   on blocking threads; more wait up to 10 seconds, then are told to retry.
//!
//! Its own tokio runtime on its own thread: the desktop app and web mode
//! start and stop it from ordinary threads.

use std::collections::HashMap;
use std::convert::Infallible;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use http_body_util::{BodyExt, Full, Limited};
use hyper::body::{Bytes, Incoming};
use hyper::{Request, Response};
use hyper_util::rt::{TokioIo, TokioTimer};
use tokio::sync::{Semaphore, oneshot};

use crate::tls::Identity;
use crate::{Head, Hub, HubError, HubResponse, calls, net};

const MAX_CONNECTIONS: usize = 256;
const MAX_PER_ADDRESS: usize = 16;
/// Bodies over the small size being read at once.
const MAX_BIG_BODIES: usize = 4;
/// Calls running at once.
const MAX_WORKING: usize = 16;
const HANDSHAKE: Duration = Duration::from_secs(5);
const HEADERS: Duration = Duration::from_secs(5);
const BODY: Duration = Duration::from_secs(20);
const CONNECTION: Duration = Duration::from_secs(120);
const WAIT_FOR_WORK: Duration = Duration::from_secs(10);

/// What bounds the memory and threads requests can take.
struct Gates {
    big_bodies: Arc<Semaphore>,
    working: Arc<Semaphore>,
}

/// A running listener. Dropping it (or [`Listener::stop`]) closes the port.
pub struct Listener {
    pub port: u16,
    stop: Option<oneshot::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

fn bind(addr: SocketAddr) -> std::io::Result<std::net::TcpListener> {
    use socket2::{Domain, Protocol, Socket, Type};
    let domain = if addr.is_ipv4() { Domain::IPV4 } else { Domain::IPV6 };
    let s = Socket::new(domain, Type::STREAM, Some(Protocol::TCP))?;
    if addr.is_ipv6() {
        s.set_only_v6(true)?;
    }
    // Rebinding at once after hub mode is turned off and on. Not on Windows,
    // where it would let another program share the port.
    #[cfg(not(windows))]
    s.set_reuse_address(true)?;
    s.bind(&addr.into())?;
    s.listen(128)?;
    s.set_nonblocking(true)?;
    Ok(s.into())
}

impl Listener {
    /// Listen on `port` (all interfaces; the allowed ranges decide who may
    /// connect). Fails at once if the port is taken.
    pub fn start(hub: Arc<Hub>, identity: &Identity, port: u16) -> Result<Self, HubError> {
        let config = identity.server_config()?;
        let v4 = bind(SocketAddr::from((Ipv4Addr::UNSPECIFIED, port))).map_err(|e| {
            HubError::User(format!("The hub couldn't listen on port {port}: {e}. Choose another port."))
        })?;
        let port = v4.local_addr().map(|a| a.port()).unwrap_or(port);
        let mut sockets = vec![v4];
        match bind(SocketAddr::from((Ipv6Addr::UNSPECIFIED, port))) {
            Ok(v6) => sockets.push(v6),
            Err(e) => tracing::info!(error = %e, "the hub listens on IPv4 only"),
        }
        let (tx, rx) = oneshot::channel();
        let thread = std::thread::Builder::new()
            .name("openbark-hub".into())
            .spawn(move || {
                let rt = match tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(2)
                    .thread_name("openbark-hub-io")
                    .enable_all()
                    .build()
                {
                    Ok(rt) => rt,
                    Err(e) => {
                        tracing::error!(error = %e, "the hub couldn't start");
                        return;
                    }
                };
                rt.block_on(serve(hub, tokio_rustls::TlsAcceptor::from(config), sockets, rx));
                rt.shutdown_timeout(Duration::from_secs(2));
            })
            .map_err(|e| HubError::Io(e.to_string()))?;
        tracing::info!(port, "clinic hub listening");
        Ok(Self { port, stop: Some(tx), thread: Some(thread) })
    }

    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        if let Some(tx) = self.stop.take() {
            let _ = tx.send(());
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
            tracing::info!(port = self.port, "clinic hub stopped");
        }
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Open connections per client address key (an IPv6 /64 counts as one).
type PerAddress = Arc<Mutex<HashMap<String, usize>>>;

async fn serve(
    hub: Arc<Hub>,
    acceptor: tokio_rustls::TlsAcceptor,
    sockets: Vec<std::net::TcpListener>,
    stop: oneshot::Receiver<()>,
) {
    let slots = Arc::new(Semaphore::new(MAX_CONNECTIONS));
    let per: PerAddress = Arc::new(Mutex::new(HashMap::new()));
    let gates = Arc::new(Gates {
        big_bodies: Arc::new(Semaphore::new(MAX_BIG_BODIES)),
        working: Arc::new(Semaphore::new(MAX_WORKING)),
    });
    let mut tasks = Vec::new();
    for s in sockets {
        match tokio::net::TcpListener::from_std(s) {
            Ok(l) => tasks.push(tokio::spawn(accept_loop(
                l,
                hub.clone(),
                acceptor.clone(),
                slots.clone(),
                per.clone(),
                gates.clone(),
            ))),
            Err(e) => tracing::warn!(error = %e, "a hub socket couldn't start"),
        }
    }
    let purge_hub = hub.clone();
    let purge: tokio::task::JoinHandle<()> = tokio::spawn(async move {
        let mut every = tokio::time::interval(Duration::from_secs(3600));
        loop {
            every.tick().await;
            let h = purge_hub.clone();
            let _ = tokio::task::spawn_blocking(move || h.purge()).await;
        }
    });
    tasks.push(purge);
    let _ = stop.await;
    for t in tasks {
        t.abort();
    }
}

/// Counts a connection against its address while it lives.
struct Slot {
    key: String,
    per: PerAddress,
}

impl Drop for Slot {
    fn drop(&mut self) {
        let mut m = self.per.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(n) = m.get_mut(&self.key) {
            *n -= 1;
            if *n == 0 {
                m.remove(&self.key);
            }
        }
    }
}

/// Take a connection slot for `key`, unless it holds `MAX_PER_ADDRESS` already.
fn take_slot(per: &PerAddress, key: String) -> Option<Slot> {
    let mut m = per.lock().unwrap_or_else(|p| p.into_inner());
    let n = m.entry(key.clone()).or_insert(0);
    if *n >= MAX_PER_ADDRESS {
        return None;
    }
    *n += 1;
    Some(Slot { key, per: per.clone() })
}

async fn accept_loop(
    listener: tokio::net::TcpListener,
    hub: Arc<Hub>,
    acceptor: tokio_rustls::TlsAcceptor,
    slots: Arc<Semaphore>,
    per: PerAddress,
    gates: Arc<Gates>,
) {
    loop {
        let (tcp, addr) = match listener.accept().await {
            Ok(x) => x,
            Err(_) => {
                tokio::time::sleep(Duration::from_millis(50)).await;
                continue;
            }
        };
        let ip = net::canonical(addr.ip());
        // Default deny: closed before the handshake.
        if !hub.allows(ip) {
            tracing::debug!(%ip, "hub: a connection from outside the allowed ranges was closed");
            drop(tcp);
            continue;
        }
        let Some(slot) = take_slot(&per, net::limit_key(ip)) else {
            drop(tcp);
            continue;
        };
        let Ok(permit) = slots.clone().try_acquire_owned() else {
            drop(tcp);
            continue;
        };
        let hub = hub.clone();
        let acceptor = acceptor.clone();
        let gates = gates.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let _slot = slot;
            let _ = tcp.set_nodelay(true);
            let tls = match tokio::time::timeout(HANDSHAKE, acceptor.accept(tcp)).await {
                Ok(Ok(s)) => s,
                _ => return,
            };
            let service = hyper::service::service_fn(move |req| respond(hub.clone(), gates.clone(), ip, req));
            let mut builder = hyper::server::conn::http1::Builder::new();
            builder.timer(TokioTimer::new()).header_read_timeout(HEADERS).keep_alive(true);
            let conn = builder.serve_connection(TokioIo::new(tls), service);
            let _ = tokio::time::timeout(CONNECTION, conn).await;
        });
    }
}

fn reply(r: HubResponse) -> Response<Full<Bytes>> {
    let mut resp = Response::new(Full::new(Bytes::from(r.body)));
    *resp.status_mut() =
        hyper::StatusCode::from_u16(r.status).unwrap_or(hyper::StatusCode::INTERNAL_SERVER_ERROR);
    resp.headers_mut()
        .insert(hyper::header::CONTENT_TYPE, hyper::header::HeaderValue::from_static("application/json"));
    resp.headers_mut()
        .insert(hyper::header::CACHE_CONTROL, hyper::header::HeaderValue::from_static("no-store"));
    resp
}

fn refused(status: u16, code: &str) -> Response<Full<Bytes>> {
    reply(HubResponse::refusal(&calls::deny(status, code)))
}

async fn respond(
    hub: Arc<Hub>,
    gates: Arc<Gates>,
    ip: IpAddr,
    req: Request<Incoming>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    let head = Head {
        method: req.method().as_str().to_string(),
        path: req.uri().path().to_string(),
        headers: req
            .headers()
            .iter()
            .filter_map(|(k, v)| v.to_str().ok().map(|v| (k.as_str().to_string(), v.to_string())))
            .collect(),
        content_length: req
            .headers()
            .get(hyper::header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<usize>().ok()),
    };
    // Who, when, which call and how much body: before reading any of it.
    let h = hub.clone();
    let admitted = match tokio::task::spawn_blocking(move || h.admit(ip, &head)).await {
        Ok(Ok(a)) => a,
        Ok(Err(r)) => return Ok(reply(HubResponse::refusal(&r))),
        Err(_) => return Ok(refused(500, "internal")),
    };
    // Few large bodies at once.
    let big = if admitted.body_limit > calls::SMALL_BODY {
        match gates.big_bodies.clone().try_acquire_owned() {
            Ok(p) => Some(p),
            Err(_) => return Ok(refused(503, "busy")),
        }
    } else {
        None
    };
    let body = match tokio::time::timeout(BODY, Limited::new(req.into_body(), admitted.body_limit).collect())
        .await
    {
        Ok(Ok(c)) => c.to_bytes().to_vec(),
        Ok(Err(_)) => return Ok(refused(413, "too_large")),
        Err(_) => return Ok(refused(408, "timeout")),
    };
    drop(big);
    // Few calls at once.
    let Ok(Ok(_working)) = tokio::time::timeout(WAIT_FOR_WORK, gates.working.clone().acquire_owned()).await
    else {
        return Ok(refused(503, "busy"));
    };
    let out = tokio::task::spawn_blocking(move || hub.finish(admitted, &body))
        .await
        .unwrap_or_else(|_| HubResponse::refusal(&calls::deny(500, "internal")));
    Ok(reply(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connections_count_per_address_and_per_ipv6_64() {
        let per: PerAddress = Arc::new(Mutex::new(HashMap::new()));
        let a: IpAddr = "fd00:1:2:3::1".parse().unwrap();
        let b: IpAddr = "fd00:1:2:3::ffff".parse().unwrap();
        let mut held = Vec::new();
        for i in 0..MAX_PER_ADDRESS {
            let ip = if i % 2 == 0 { a } else { b };
            held.push(take_slot(&per, net::limit_key(ip)).expect("room"));
        }
        assert!(take_slot(&per, net::limit_key(a)).is_none(), "the /64 is full");
        assert!(take_slot(&per, net::limit_key("fd00:1:2:4::1".parse().unwrap())).is_some());
        held.pop();
        assert!(take_slot(&per, net::limit_key(b)).is_some(), "a slot came back");
    }
}
