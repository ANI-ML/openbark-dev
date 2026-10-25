//! The clinic hub client (ADR-0029 sections 3 to 5): the second
//! [`Transport`](crate::supabase::Transport), and what the hub and its
//! devices share.
//!
//! - **Transport.** One HTTP/1.1 request per TLS 1.3 connection to the hub's
//!   self-signed certificate, accepted only when its SHA-256 fingerprint is
//!   the one this device paired with (a different certificate is refused,
//!   with a message to pair again). The handshake signature is still checked
//!   against the certificate's key, so the public certificate alone can't
//!   impersonate the hub. Requests go to `obhub://<host>:<port>/rpc/<name>`;
//!   the fingerprint rides in a local-only header that is never sent.
//! - **Signed requests.** Every request carries this device's id, a time, a
//!   nonce and an Ed25519 signature over the method, path, body hash, time,
//!   nonce and device id ([`canonical`]). The hub allows 5 minutes either
//!   way and refuses a nonce it has seen.
//! - **Join codes.** The hub shows 8 characters (and a QR code with its
//!   address and certificate). A code typed by hand comes with the hub's
//!   10-character certificate check, which [`probe`] compares before the
//!   fingerprint is pinned.
//! - **Finding the hub.** mDNS / DNS-SD browsing for `_openbark-hub._tcp`
//!   ([`browse`]), only when the person asks while joining.
//!
//! Every connection is listed in docs/network.md (L2, L3).

use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, verify_tls12_signature, verify_tls13_signature};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};
use serde::{Deserialize, Serialize};

use crate::SyncError;
use crate::keys::{self, DeviceKeys};
use crate::supabase::{Request, Response, Transport};

/// The scheme of hub request urls (never a real URL scheme).
pub const SCHEME: &str = "obhub://";
/// The DNS-SD service type the hub advertises.
pub const SERVICE_TYPE: &str = "_openbark-hub._tcp.local.";
/// The hub's default port (a setting on the hub).
pub const DEFAULT_PORT: u16 = 47110;
/// How far a request's time may be from the hub's clock, either way.
pub const WINDOW_SECS: i64 = 300;

pub const H_DEVICE: &str = "x-openbark-device";
pub const H_TIME: &str = "x-openbark-time";
pub const H_NONCE: &str = "x-openbark-nonce";
pub const H_SIGNATURE: &str = "x-openbark-signature";
/// Local only: the pinned certificate fingerprint (hex). Never sent.
pub const H_PIN: &str = "x-openbark-pin";

/// The join code and certificate check alphabet: Crockford base32 (no I, L,
/// O or U), so a code read aloud or typed is hard to get wrong.
pub const CODE_ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
/// Characters in a join code (40 bits).
pub const CODE_LEN: usize = 8;
/// Characters in the certificate check (50 bits of the fingerprint).
pub const CHECK_LEN: usize = 10;

/// The largest answer read from a hub (a page of pulled objects).
const MAX_RESPONSE: usize = 64 << 20;
const MAX_HEAD: usize = 16 * 1024;
/// Marks the verifier's refusal, so it can be told apart from other TLS errors.
const MISMATCH: &str = "openbark: the hub certificate is not the paired one";

pub const PAIR_AGAIN: &str = "This isn't the clinic hub this device paired with: its certificate is different. Pair again with a new join code from the hub.";
const CLOSED: &str = "The hub closed the connection. Check the address, and that this device's network is allowed in the hub's settings (Settings, Clinic hub).";

/// The TLS crypto provider: ring (never aws-lc), shared with the hub.
pub fn crypto_provider() -> Arc<CryptoProvider> {
    static P: OnceLock<Arc<CryptoProvider>> = OnceLock::new();
    P.get_or_init(|| Arc::new(rustls::crypto::ring::default_provider())).clone()
}

/// A certificate's fingerprint: SHA-256 of its DER bytes.
pub fn fingerprint(cert_der: &[u8]) -> [u8; 32] {
    keys::sha256(&[cert_der])
}

/// The certificate check shown beside a join code: the fingerprint's first
/// 50 bits in the code alphabet, as `XXXXX-XXXXX`.
pub fn check_code(fp: &[u8; 32]) -> String {
    let mut v: u64 = 0;
    for b in &fp[..8] {
        v = (v << 8) | *b as u64;
    }
    let mut s = String::with_capacity(CHECK_LEN + 1);
    for i in 0..CHECK_LEN {
        let idx = (v >> (64 - 5 * (i + 1))) & 31;
        s.push(CODE_ALPHABET[idx as usize] as char);
        if i == 4 {
            s.push('-');
        }
    }
    s
}

/// A code as typed, in the alphabet: upper case, without spaces or dashes,
/// with the letters people confuse mapped (O to 0, I and L to 1).
pub fn normalise_code(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .map(|c| match c.to_ascii_uppercase() {
            'O' => '0',
            'I' | 'L' => '1',
            c => c,
        })
        .collect()
}

fn in_alphabet(s: &str) -> bool {
    s.bytes().all(|b| CODE_ALPHABET.contains(&b))
}

/// Compare two byte strings without stopping at the first difference.
pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Where this device's hub is, and the certificate it pinned.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HubLink {
    pub host: String,
    pub port: u16,
    /// SHA-256 of the hub's certificate, lower-case hex.
    pub fingerprint: String,
}

impl HubLink {
    pub fn check(&self) -> Result<(), SyncError> {
        check_host(&self.host)?;
        if self.port == 0 {
            return Err(SyncError::User("Enter the hub's port.".into()));
        }
        parse_fp(&self.fingerprint).map(|_| ())
    }

    /// `host:port`, with brackets around an IPv6 address.
    pub fn address(&self) -> String {
        join_address(&self.host, self.port)
    }

    /// The request url for one hub call.
    pub fn url(&self, name: &str) -> String {
        format!("{SCHEME}{}/rpc/{name}", self.address())
    }

    pub fn fp(&self) -> Result<[u8; 32], SyncError> {
        parse_fp(&self.fingerprint)
    }

    /// The certificate check, as the hub shows it.
    pub fn check_code(&self) -> String {
        self.fp().map(|f| check_code(&f)).unwrap_or_default()
    }
}

fn join_address(host: &str, port: u16) -> String {
    if host.contains(':') { format!("[{host}]:{port}") } else { format!("{host}:{port}") }
}

fn check_host(host: &str) -> Result<(), SyncError> {
    let ok = !host.is_empty()
        && host.len() <= 253
        && host.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':' | '_'))
        && !host.starts_with('-');
    if ok { Ok(()) } else { Err(SyncError::User("Enter the hub's address, such as 192.168.1.20.".into())) }
}

fn parse_fp(hex_text: &str) -> Result<[u8; 32], SyncError> {
    hex::decode(hex_text.trim())
        .ok()
        .and_then(|b| <[u8; 32]>::try_from(b).ok())
        .ok_or_else(|| SyncError::User("The hub's certificate fingerprint isn't valid. Pair again.".into()))
}

/// `host`, `host:port`, `[v6]:port` or a bare IPv6 address; the default
/// port when none is given.
pub fn parse_address(text: &str) -> Result<(String, u16), SyncError> {
    let t = text.trim().trim_end_matches('/');
    let t = t.strip_prefix("https://").or_else(|| t.strip_prefix(SCHEME)).unwrap_or(t);
    let bad =
        || SyncError::User("Enter the hub's address, such as 192.168.1.20 or 192.168.1.20:47110.".into());
    let (host, port) = if let Some(rest) = t.strip_prefix('[') {
        let (h, after) = rest.split_once(']').ok_or_else(bad)?;
        let port = match after.strip_prefix(':') {
            Some(p) => p.parse::<u16>().map_err(|_| bad())?,
            None if after.is_empty() => DEFAULT_PORT,
            None => return Err(bad()),
        };
        (h.to_string(), port)
    } else if t.matches(':').count() > 1 {
        // A bare IPv6 address.
        (t.to_string(), DEFAULT_PORT)
    } else if let Some((h, p)) = t.split_once(':') {
        (h.to_string(), p.parse::<u16>().map_err(|_| bad())?)
    } else {
        (t.to_string(), DEFAULT_PORT)
    };
    check_host(&host).map_err(|_| bad())?;
    if port == 0 {
        return Err(bad());
    }
    Ok((host, port))
}

/// What a person brings from the hub to join it.
#[derive(Debug, Clone)]
pub struct Join {
    pub host: String,
    pub port: u16,
    /// From the QR code: the full fingerprint.
    pub fingerprint: Option<[u8; 32]>,
    /// Typed by hand: the 10-character certificate check (normalised).
    pub check: Option<String>,
    /// The 8-character join code (normalised).
    pub code: String,
}

/// The QR code's text: `openbark-hub:1?h=<host>&p=<port>&f=<fingerprint hex>&c=<code>`.
pub fn join_link(host: &str, port: u16, fp: &[u8; 32], code: &str) -> String {
    format!("openbark-hub:1?h={host}&p={port}&f={}&c={code}", hex::encode(fp))
}

/// Read what the person entered: the QR code's text (or a pasted link), or
/// an 8-character code with the hub's address and certificate check.
pub fn parse_join(link_or_code: &str, address: &str, check: &str) -> Result<Join, SyncError> {
    let text = link_or_code.trim();
    if let Some(query) = text.strip_prefix("openbark-hub:1?") {
        let mut host = None;
        let mut port = None;
        let mut fp = None;
        let mut code = None;
        for pair in query.split('&') {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            match k {
                "h" => host = Some(v.to_string()),
                "p" => port = v.parse::<u16>().ok(),
                "f" => fp = Some(parse_fp(v)?),
                "c" => code = Some(normalise_code(v)),
                _ => {}
            }
        }
        let bad = || SyncError::User("That hub code isn't complete. Scan it again, or type the code.".into());
        let host = host.ok_or_else(bad)?;
        check_host(&host)?;
        let code = code.filter(|c| c.len() == CODE_LEN && in_alphabet(c)).ok_or_else(bad)?;
        return Ok(Join {
            host,
            port: port.filter(|p| *p != 0).ok_or_else(bad)?,
            fingerprint: fp,
            check: None,
            code,
        });
    }
    let code = normalise_code(text);
    if code.len() != CODE_LEN || !in_alphabet(&code) {
        return Err(SyncError::User(format!(
            "Enter the {CODE_LEN}-character join code shown on the hub, or scan its QR code."
        )));
    }
    let (host, port) = parse_address(address)?;
    let check = normalise_code(check);
    if check.len() != CHECK_LEN || !in_alphabet(&check) {
        return Err(SyncError::User(format!(
            "Enter the {CHECK_LEN}-character certificate check shown on the hub under the join code."
        )));
    }
    Ok(Join { host, port, fingerprint: None, check: Some(check), code })
}

// ---- signed requests -----------------------------------------------------------

/// What a request's signature covers.
pub fn canonical(method: &str, path: &str, body: &[u8], time: i64, nonce: &str, device: &str) -> Vec<u8> {
    format!("OBH1\n{method}\n{path}\n{}\n{time}\n{nonce}\n{device}", hex::encode(keys::sha256(&[body])))
        .into_bytes()
}

/// The headers that sign one request as `device`.
pub fn sign(
    dk: &DeviceKeys,
    device: &str,
    method: &str,
    path: &str,
    body: &[u8],
) -> Result<Vec<(String, String)>, SyncError> {
    let time = chrono::Utc::now().timestamp();
    let nonce = keys::b64(&keys::random::<18>()?);
    let sig = dk.sign(&canonical(method, path, body, time, &nonce, device));
    Ok(vec![
        (H_DEVICE.into(), device.into()),
        (H_TIME.into(), time.to_string()),
        (H_NONCE.into(), nonce),
        (H_SIGNATURE.into(), keys::b64(&sig)),
    ])
}

// ---- the transport ---------------------------------------------------------------

/// The hub transport: pinned TLS 1.3, one request per connection, blocking.
pub struct HubTransport;

impl Transport for HubTransport {
    fn send(&self, req: &Request) -> Result<Response, SyncError> {
        send(req)
    }
}

#[derive(Debug)]
enum Expect {
    Pin([u8; 32]),
    Check(String),
}

#[derive(Debug)]
struct Verifier {
    expect: Expect,
    seen: Mutex<Option<[u8; 32]>>,
    provider: Arc<CryptoProvider>,
}

impl ServerCertVerifier for Verifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let fp = fingerprint(end_entity.as_ref());
        let ok = match &self.expect {
            Expect::Pin(pin) => keys::eq32(pin, &fp),
            Expect::Check(c) => ct_eq(normalise_code(&check_code(&fp)).as_bytes(), c.as_bytes()),
        };
        *self.seen.lock().unwrap_or_else(|p| p.into_inner()) = Some(fp);
        if ok { Ok(ServerCertVerified::assertion()) } else { Err(rustls::Error::General(MISMATCH.into())) }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider.signature_verification_algorithms.supported_schemes()
    }
}

type Tls = rustls::StreamOwned<rustls::ClientConnection, TcpStream>;

fn io_err(e: std::io::Error) -> SyncError {
    let text = e.to_string();
    if text.contains(MISMATCH) {
        return SyncError::User(PAIR_AGAIN.into());
    }
    use std::io::ErrorKind as K;
    match e.kind() {
        K::TimedOut | K::WouldBlock | K::ConnectionRefused | K::HostUnreachable | K::NetworkUnreachable => {
            SyncError::HubOffline
        }
        K::ConnectionReset | K::ConnectionAborted | K::UnexpectedEof | K::BrokenPipe => {
            SyncError::User(CLOSED.into())
        }
        _ => SyncError::Network(text),
    }
}

fn connect(host: &str, port: u16, expect: Expect) -> Result<(Tls, Arc<Verifier>), SyncError> {
    let addrs: Vec<SocketAddr> = (host, port).to_socket_addrs().map_err(|_| SyncError::HubOffline)?.collect();
    let mut sock = None;
    for a in &addrs {
        if let Ok(s) = TcpStream::connect_timeout(a, Duration::from_secs(5)) {
            sock = Some(s);
            break;
        }
    }
    let sock = sock.ok_or(SyncError::HubOffline)?;
    let _ = sock.set_nodelay(true);
    sock.set_read_timeout(Some(Duration::from_secs(60))).map_err(io_err)?;
    sock.set_write_timeout(Some(Duration::from_secs(60))).map_err(io_err)?;
    let provider = crypto_provider();
    let verifier = Arc::new(Verifier { expect, seen: Mutex::new(None), provider: provider.clone() });
    let config = rustls::ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|e| SyncError::Network(e.to_string()))?
        .dangerous()
        .with_custom_certificate_verifier(verifier.clone())
        .with_no_client_auth();
    // The name is only sent as SNI; the pin decides.
    let name = ServerName::try_from(host.to_string())
        .or_else(|_| ServerName::try_from("openbark-hub".to_string()))
        .map_err(|e| SyncError::Network(e.to_string()))?;
    let conn = rustls::ClientConnection::new(Arc::new(config), name)
        .map_err(|e| SyncError::Network(e.to_string()))?;
    Ok((rustls::StreamOwned::new(conn, sock), verifier))
}

/// Connect once to read the hub's certificate, accepting it only if its
/// check matches `check` (what the person typed from the hub's screen).
/// Returns the fingerprint to pin.
pub fn probe(host: &str, port: u16, check: &str) -> Result<[u8; 32], SyncError> {
    let (mut tls, verifier) = connect(host, port, Expect::Check(normalise_code(check)))?;
    while tls.conn.is_handshaking() {
        tls.conn.complete_io(&mut tls.sock).map_err(io_err)?;
    }
    let fp = verifier
        .seen
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .ok_or_else(|| SyncError::Network("the hub sent no certificate".into()))?;
    tls.conn.send_close_notify();
    let _ = tls.conn.complete_io(&mut tls.sock);
    Ok(fp)
}

/// Send one request: `req.url` is `obhub://host:port/path`, with the pinned
/// fingerprint in the local-only [`H_PIN`] header.
pub fn send(req: &Request) -> Result<Response, SyncError> {
    let rest = req.url.strip_prefix(SCHEME).ok_or_else(|| SyncError::Network("not a hub request".into()))?;
    let slash = rest.find('/').ok_or_else(|| SyncError::Network("a hub request needs a path".into()))?;
    let (addr, path) = rest.split_at(slash);
    let (host, port) = parse_address(addr)?;
    let pin = req
        .headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(H_PIN))
        .map(|(_, v)| parse_fp(v))
        .transpose()?
        .ok_or_else(|| SyncError::User(PAIR_AGAIN.into()))?;
    let body: &[u8] = req.body.as_deref().unwrap_or_default();
    let mut head = format!(
        "{} {path} HTTP/1.1\r\nHost: {}\r\nUser-Agent: OpenBark/{}\r\nContent-Length: {}\r\nConnection: close\r\n",
        req.method,
        join_address(&host, port),
        env!("CARGO_PKG_VERSION"),
        body.len()
    );
    for (k, v) in &req.headers {
        let skip = [H_PIN, "content-length", "host", "connection", "user-agent"];
        if skip.iter().any(|s| k.eq_ignore_ascii_case(s)) {
            continue;
        }
        if k.is_empty() || k.contains([':', '\r', '\n', ' ']) || v.contains(['\r', '\n']) {
            return Err(SyncError::Guard(format!("a header ({k}) isn't valid")));
        }
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str("\r\n");
    let (mut tls, _) = connect(&host, port, Expect::Pin(pin))?;
    tls.write_all(head.as_bytes()).map_err(io_err)?;
    tls.write_all(body).map_err(io_err)?;
    tls.flush().map_err(io_err)?;
    let resp = read_response(&mut tls)?;
    tls.conn.send_close_notify();
    let _ = tls.conn.complete_io(&mut tls.sock);
    Ok(resp)
}

fn read_some<R: Read>(r: &mut R, buf: &mut [u8]) -> Result<usize, SyncError> {
    loop {
        match r.read(buf) {
            Ok(n) => return Ok(n),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(0),
            Err(e) => return Err(io_err(e)),
        }
    }
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// An HTTP/1.1 response with a Content-Length (or ended by closing).
fn read_response<R: Read>(r: &mut R) -> Result<Response, SyncError> {
    let mut buf = Vec::with_capacity(16 * 1024);
    let mut chunk = vec![0u8; 64 * 1024];
    let head_end = loop {
        if let Some(p) = find(&buf, b"\r\n\r\n") {
            break p + 4;
        }
        if buf.len() > MAX_HEAD {
            return Err(SyncError::Server("the hub's answer has too many headers".into()));
        }
        let n = read_some(r, &mut chunk)?;
        if n == 0 {
            return Err(SyncError::User(CLOSED.into()));
        }
        buf.extend_from_slice(&chunk[..n]);
    };
    let head = std::str::from_utf8(&buf[..head_end])
        .map_err(|_| SyncError::Server("the hub's answer isn't readable".into()))?;
    let mut lines = head.split("\r\n");
    let status = lines
        .next()
        .and_then(|l| l.split(' ').nth(1))
        .and_then(|s| s.parse::<u16>().ok())
        .ok_or_else(|| SyncError::Server("the hub's answer has no status".into()))?;
    let mut length: Option<usize> = None;
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            let k = k.trim();
            if k.eq_ignore_ascii_case("content-length") {
                length = v.trim().parse().ok();
            } else if k.eq_ignore_ascii_case("transfer-encoding") {
                return Err(SyncError::Server("the hub answered in an unexpected encoding".into()));
            }
        }
    }
    let mut body = buf[head_end..].to_vec();
    match length {
        Some(n) if n > MAX_RESPONSE => return Err(SyncError::Server("the hub's answer is too large".into())),
        Some(n) => {
            while body.len() < n {
                let k = read_some(r, &mut chunk)?;
                if k == 0 {
                    return Err(SyncError::User(CLOSED.into()));
                }
                body.extend_from_slice(&chunk[..k]);
            }
            body.truncate(n);
        }
        None => loop {
            let k = read_some(r, &mut chunk)?;
            if k == 0 {
                break;
            }
            body.extend_from_slice(&chunk[..k]);
            if body.len() > MAX_RESPONSE {
                return Err(SyncError::Server("the hub's answer is too large".into()));
            }
        },
    }
    Ok(Response { status, body })
}

// ---- finding the hub ----------------------------------------------------------------

/// A hub found on the local network. Nothing here is checked yet: the join
/// code and the certificate check (or the QR code) decide.
#[derive(Debug, Clone, Serialize)]
pub struct FoundHub {
    pub name: String,
    /// The address to use (IPv4 first).
    pub host: String,
    pub port: u16,
    pub addresses: Vec<String>,
    /// The first 16 hex characters of the certificate fingerprint, as advertised.
    pub fingerprint_prefix: String,
}

/// Browse the local network for hubs for `wait`, then stop.
pub fn browse(wait: Duration) -> Result<Vec<FoundHub>, SyncError> {
    let daemon = mdns_sd::ServiceDaemon::new()
        .map_err(|e| SyncError::Network(format!("couldn't search the local network: {e}")))?;
    let rx = daemon
        .browse(SERVICE_TYPE)
        .map_err(|e| SyncError::Network(format!("couldn't search the local network: {e}")))?;
    let deadline = Instant::now() + wait;
    let mut found: Vec<FoundHub> = Vec::new();
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        match rx.recv_timeout(left) {
            Ok(mdns_sd::ServiceEvent::ServiceResolved(info)) => {
                let mut addrs: Vec<IpAddr> = info
                    .get_addresses()
                    .iter()
                    .map(|a| a.to_ip_addr())
                    .filter(|a| !matches!(a, IpAddr::V6(v6) if v6.segments()[0] & 0xffc0 == 0xfe80))
                    .collect();
                addrs.sort_by_key(|a| (!a.is_ipv4(), *a));
                let Some(first) = addrs.first() else { continue };
                let name =
                    info.get_fullname().trim_end_matches(SERVICE_TYPE).trim_end_matches('.').to_string();
                let hub = FoundHub {
                    name: name.chars().take(63).collect(),
                    host: first.to_string(),
                    port: info.get_port(),
                    addresses: addrs.iter().map(|a| a.to_string()).collect(),
                    fingerprint_prefix: info
                        .get_property_val_str("fp")
                        .unwrap_or_default()
                        .chars()
                        .filter(|c| c.is_ascii_hexdigit())
                        .take(16)
                        .collect(),
                };
                found.retain(|h| h.name != hub.name);
                found.push(hub);
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }
    let _ = daemon.stop_browse(SERVICE_TYPE);
    let _ = daemon.shutdown();
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_codes_and_links_parse() {
        let fp = [7u8; 32];
        let link = join_link("192.168.1.20", 47110, &fp, "ABCD2345");
        let j = parse_join(&link, "", "").unwrap();
        assert_eq!(
            (j.host.as_str(), j.port, j.fingerprint, j.code.as_str()),
            ("192.168.1.20", 47110, Some(fp), "ABCD2345")
        );
        let j = parse_join("abcd-2345", "hub.local", &check_code(&fp)).unwrap();
        assert_eq!((j.host.as_str(), j.port), ("hub.local", DEFAULT_PORT));
        assert_eq!(j.check.as_deref(), Some(normalise_code(&check_code(&fp)).as_str()));
        assert!(parse_join("abcd", "hub.local", &check_code(&fp)).is_err(), "too short");
        assert!(parse_join("abcd2345", "hub.local", "").is_err(), "no certificate check");
        assert!(parse_join("abcd2345", "", &check_code(&fp)).is_err(), "no address");
        assert!(parse_join("openbark-hub:1?h=x&p=1&f=00", "", "").is_err(), "bad fingerprint");
    }

    #[test]
    fn addresses_parse() {
        assert_eq!(parse_address("192.168.1.20:5000").unwrap(), ("192.168.1.20".into(), 5000));
        assert_eq!(parse_address("192.168.1.20").unwrap(), ("192.168.1.20".into(), DEFAULT_PORT));
        assert_eq!(parse_address("[fd00::1]:47111").unwrap(), ("fd00::1".into(), 47111));
        assert_eq!(parse_address("fd00::1").unwrap(), ("fd00::1".into(), DEFAULT_PORT));
        for bad in ["", "a b", "host:0", "host:x", "[fd00::1", "http://x/y"] {
            assert!(parse_address(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_check_is_ten_characters_of_the_fingerprint() {
        let c = check_code(&[0xff; 32]);
        assert_eq!(c, "ZZZZZ-ZZZZZ");
        assert_eq!(normalise_code("zzzzz zzzzz"), "ZZZZZZZZZZ");
        assert_eq!(normalise_code("o1-Il"), "0111");
        assert_ne!(check_code(&[1; 32]), check_code(&[2; 32]));
    }

    #[test]
    fn a_signature_covers_every_part() {
        let dk = DeviceKeys::generate().unwrap();
        let h = sign(&dk, "dev", "POST", "/rpc/pull", b"{}").unwrap();
        let get = |k: &str| h.iter().find(|(n, _)| n == k).unwrap().1.clone();
        let t: i64 = get(H_TIME).parse().unwrap();
        let sig = keys::unb64(&get(H_SIGNATURE)).unwrap();
        let msg = canonical("POST", "/rpc/pull", b"{}", t, &get(H_NONCE), "dev");
        assert!(keys::verify(&dk.ed_pub(), &msg, &sig));
        for other in [
            canonical("POST", "/rpc/push", b"{}", t, &get(H_NONCE), "dev"),
            canonical("POST", "/rpc/pull", b"{ }", t, &get(H_NONCE), "dev"),
            canonical("POST", "/rpc/pull", b"{}", t + 1, &get(H_NONCE), "dev"),
            canonical("POST", "/rpc/pull", b"{}", t, "other", "dev"),
            canonical("POST", "/rpc/pull", b"{}", t, &get(H_NONCE), "dev2"),
        ] {
            assert!(!keys::verify(&dk.ed_pub(), &other, &sig));
        }
    }

    #[test]
    fn responses_are_read_by_length() {
        let raw = b"HTTP/1.1 403 Forbidden\r\ncontent-type: application/json\r\ncontent-length: 5\r\n\r\nhelloEXTRA";
        let r = read_response(&mut &raw[..]).unwrap();
        assert_eq!((r.status, r.body.as_slice()), (403, b"hello".as_slice()));
        let cut = b"HTTP/1.1 200 OK\r\ncontent-length: 50\r\n\r\nshort";
        assert!(read_response(&mut &cut[..]).is_err());
    }
}
