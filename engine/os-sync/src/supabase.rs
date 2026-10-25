//! The sync service over plain REST (ADR-0017 section 1, point 5): Supabase
//! Auth (`/auth/v1`) for sign-in with an emailed code, PostgREST
//! (`/rest/v1/rpc/...`, schema `sync`) for everything else, and one Edge
//! Function (`/functions/v1/delete-account`). No Supabase SDK, no Storage,
//! no Realtime. Every call is listed in docs/network.md (R12, R13).
//!
//! Requests go out through one [`Transport`], after the plaintext guard
//! (`crate::guard`). Tests swap in a transport that records every body.

use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{SyncError, guard};

/// Where the sync service is: a Supabase project's URL and its publishable
/// (anon) key, which is public by design.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Server {
    pub url: String,
    pub key: String,
}

impl Server {
    /// The host, for the settings screen.
    pub fn host(&self) -> String {
        self.url
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .trim_end_matches('/')
            .to_string()
    }

    /// Checks: https (or http to this computer, for a local test server),
    /// no path, a key.
    pub fn check(&self) -> Result<(), SyncError> {
        let url = self.url.trim_end_matches('/');
        let local = url.starts_with("http://127.0.0.1") || url.starts_with("http://localhost");
        if !(url.starts_with("https://") || local) {
            return Err(SyncError::User("The sync server must be an https:// address.".into()));
        }
        let rest = url.trim_start_matches("https://").trim_start_matches("http://");
        if rest.is_empty() || rest.contains('/') || rest.contains('?') || rest.contains('@') {
            return Err(SyncError::User(
                "Enter just the server's address, such as https://example.supabase.co.".into(),
            ));
        }
        if self.key.trim().is_empty() || self.key.len() > 512 || self.key.contains(char::is_whitespace) {
            return Err(SyncError::User("Enter the server's publishable key.".into()));
        }
        Ok(())
    }
}

/// One HTTP request, as the transport sends it.
#[derive(Debug, Clone)]
pub struct Request {
    pub method: &'static str,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
}

#[derive(Debug, Clone)]
pub struct Response {
    pub status: u16,
    pub body: Vec<u8>,
}

/// Sends requests. [`HttpTransport`] in the app; tests record or fail them.
pub trait Transport: Send + Sync {
    fn send(&self, req: &Request) -> Result<Response, SyncError>;
}

/// The real transport: reqwest, blocking, no redirects, 30 s timeout.
///
/// The client (and the runtime reqwest's blocking client keeps) is made on
/// the first request, on the worker thread that sends it, and dropped on a
/// thread of its own: the app is created and dropped inside async web
/// servers, where creating or dropping a blocking runtime would panic.
pub struct HttpTransport {
    client: std::sync::Mutex<Option<reqwest::blocking::Client>>,
}

impl HttpTransport {
    pub fn new() -> Result<Self, SyncError> {
        Ok(Self { client: std::sync::Mutex::new(None) })
    }

    fn client(&self) -> Result<reqwest::blocking::Client, SyncError> {
        let mut slot = self.client.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(c) = slot.as_ref() {
            return Ok(c.clone());
        }
        let c = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(format!("OpenBark/{}", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| SyncError::Network(e.to_string()))?;
        *slot = Some(c.clone());
        Ok(c)
    }
}

impl Drop for HttpTransport {
    fn drop(&mut self) {
        if let Some(c) = self.client.lock().unwrap_or_else(|p| p.into_inner()).take() {
            let _ = std::thread::Builder::new().name("openbark-sync-drop".into()).spawn(move || drop(c));
        }
    }
}

impl Transport for HttpTransport {
    fn send(&self, req: &Request) -> Result<Response, SyncError> {
        // A clinic hub (ADR-0029): pinned TLS, not reqwest.
        if req.url.starts_with(crate::hub::SCHEME) {
            return crate::hub::HubTransport.send(req);
        }
        let client = self.client()?;
        let mut r = match req.method {
            "GET" => client.get(&req.url),
            _ => client.post(&req.url),
        };
        for (k, v) in &req.headers {
            r = r.header(k.as_str(), v.as_str());
        }
        if let Some(b) = &req.body {
            r = r.body(b.clone());
        }
        let resp = r.send().map_err(|e| {
            if e.is_timeout() || e.is_connect() {
                SyncError::Offline
            } else {
                SyncError::Network(e.without_url().to_string())
            }
        })?;
        let status = resp.status().as_u16();
        let body = resp.bytes().map_err(|e| SyncError::Network(e.without_url().to_string()))?.to_vec();
        drop(client);
        Ok(Response { status, body })
    }
}

/// A signed-in session.
#[derive(Clone, Serialize, Deserialize)]
pub struct AuthSession {
    pub access_token: String,
    pub refresh_token: String,
    /// Unix seconds.
    pub expires_at: i64,
    pub user_id: String,
    pub email: String,
    /// The access token's `session_id` claim: the server binds the device to it.
    pub session_id: String,
}

impl std::fmt::Debug for AuthSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthSession").field("user_id", &self.user_id).finish_non_exhaustive()
    }
}

impl AuthSession {
    pub fn expires_soon(&self) -> bool {
        self.expires_at - chrono::Utc::now().timestamp() < 60
    }
}

#[derive(Deserialize)]
struct TokenReply {
    access_token: String,
    refresh_token: String,
    #[serde(default)]
    expires_in: Option<i64>,
    #[serde(default)]
    expires_at: Option<i64>,
    user: UserReply,
}

#[derive(Deserialize)]
struct UserReply {
    id: String,
    #[serde(default)]
    email: Option<String>,
}

/// A claim from a JWT's payload (no signature check: the server checks it;
/// this only reads which session the token is for).
fn jwt_claim(token: &str, claim: &str) -> Option<String> {
    let payload = token.split('.').nth(1)?;
    let bytes =
        base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(payload.trim_end_matches('=')).ok()?;
    let v: Value = serde_json::from_slice(&bytes).ok()?;
    v.get(claim)?.as_str().map(String::from)
}

/// The REST client for one server.
#[derive(Clone)]
pub struct Client {
    pub server: Server,
    transport: Arc<dyn Transport>,
}

impl Client {
    pub fn new(server: Server, transport: Arc<dyn Transport>) -> Self {
        Self { server, transport }
    }

    fn base(&self) -> &str {
        self.server.url.trim_end_matches('/')
    }

    fn send(
        &self,
        path: &str,
        body: &Value,
        bearer: Option<&str>,
        extra: &[(&str, &str)],
    ) -> Result<Value, SyncError> {
        let mut headers = vec![
            ("apikey".to_string(), self.server.key.clone()),
            ("Content-Type".to_string(), "application/json".to_string()),
            ("Accept".to_string(), "application/json".to_string()),
        ];
        if let Some(t) = bearer {
            headers.push(("Authorization".to_string(), format!("Bearer {t}")));
        }
        for (k, v) in extra {
            headers.push((k.to_string(), v.to_string()));
        }
        let req = Request {
            method: "POST",
            url: format!("{}{path}", self.base()),
            headers,
            body: Some(serde_json::to_vec(body).expect("json serialises")),
        };
        let resp = self.transport.send(&req)?;
        let parsed: Value = if resp.body.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&resp.body).unwrap_or(Value::Null)
        };
        if (200..300).contains(&resp.status) {
            return Ok(parsed);
        }
        Err(error_from(resp.status, &parsed))
    }

    // ---- Auth -----------------------------------------------------------

    /// Ask for a 6-digit code by email. The same code creates the account
    /// the first time.
    pub fn send_code(&self, email: &str) -> Result<(), SyncError> {
        let body = json!({"email": email, "create_user": true});
        guard::check_auth(&body)?;
        self.send("/auth/v1/otp", &body, None, &[]).map(|_| ())
    }

    /// Exchange the emailed code for a session.
    pub fn verify_code(&self, email: &str, code: &str) -> Result<AuthSession, SyncError> {
        let body = json!({"email": email, "token": code, "type": "email"});
        guard::check_auth(&body)?;
        let v = self.send("/auth/v1/verify", &body, None, &[])?;
        session_from(v)
    }

    /// A fresh access token (the refresh token rotates).
    pub fn refresh(&self, refresh_token: &str) -> Result<AuthSession, SyncError> {
        let body = json!({"refresh_token": refresh_token});
        guard::check_auth(&body)?;
        let v = self.send("/auth/v1/token?grant_type=refresh_token", &body, None, &[])?;
        session_from(v)
    }

    /// End this session on the server (this device only).
    pub fn sign_out(&self, access: &str) -> Result<(), SyncError> {
        self.send("/auth/v1/logout?scope=local", &json!({}), Some(access), &[]).map(|_| ())
    }

    // ---- REST (schema sync) ---------------------------------------------

    /// Call a `sync` function. The body passes the plaintext guard first.
    pub fn rpc(&self, name: &str, args: Value, access: Option<&str>) -> Result<Value, SyncError> {
        guard::check_request(&args)?;
        self.send(
            &format!("/rest/v1/rpc/{name}"),
            &args,
            access,
            &[("Content-Profile", "sync"), ("Accept-Profile", "sync")],
        )
    }

    /// Delete the account on the server (every sync row and envelope goes
    /// with it). `confirm` is the account's email address.
    pub fn delete_account(&self, access: &str, confirm: &str) -> Result<(), SyncError> {
        let body = json!({"confirm": confirm});
        guard::check_auth(&body)?;
        self.send("/functions/v1/delete-account", &body, Some(access), &[]).map(|_| ())
    }
}

fn session_from(v: Value) -> Result<AuthSession, SyncError> {
    let t: TokenReply =
        serde_json::from_value(v).map_err(|_| SyncError::Server("unexpected sign-in reply".into()))?;
    let session_id = jwt_claim(&t.access_token, "session_id")
        .ok_or_else(|| SyncError::Server("the sign-in token has no session".into()))?;
    let expires_at =
        t.expires_at.unwrap_or_else(|| chrono::Utc::now().timestamp() + t.expires_in.unwrap_or(600));
    Ok(AuthSession {
        access_token: t.access_token,
        refresh_token: t.refresh_token,
        expires_at,
        user_id: t.user.id,
        email: t.user.email.unwrap_or_default(),
        session_id,
    })
}

/// A server error as a SyncError: `openbark:<code>` messages from the sync
/// functions, Auth's codes, or the HTTP status.
fn error_from(status: u16, body: &Value) -> SyncError {
    let message = body
        .get("message")
        .or_else(|| body.get("msg"))
        .or_else(|| body.get("error_description"))
        .or_else(|| body.get("error"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    if let Some(code) = message.strip_prefix("openbark:") {
        return SyncError::Refused(code.to_string());
    }
    let code =
        body.get("error_code").or_else(|| body.get("code")).and_then(Value::as_str).unwrap_or_default();
    match (status, code) {
        (_, "otp_expired") => {
            SyncError::User("That code has expired or was already used. Ask for a new one.".into())
        }
        (_, "over_email_send_rate_limit") | (429, _) => {
            SyncError::User("Too many codes were asked for. Wait a minute and try again.".into())
        }
        (_, "refresh_token_not_found") | (_, "session_not_found") | (_, "refresh_token_already_used") => {
            SyncError::SignedOut
        }
        (401, _) => SyncError::Unauthorized,
        (403, _) if code == "bad_jwt" => SyncError::Unauthorized,
        (400, _) if message.to_ascii_lowercase().contains("token") && code.is_empty() => {
            SyncError::User("That code isn't right. Check the latest email and try again.".into())
        }
        _ => SyncError::Server(format!("{status} {}", if message.is_empty() { code } else { message })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_server_address_is_checked() {
        let ok = Server { url: "https://abc.supabase.co".into(), key: "sb_publishable_x".into() };
        ok.check().unwrap();
        assert_eq!(ok.host(), "abc.supabase.co");
        Server { url: "http://127.0.0.1:54321".into(), key: "k".into() }.check().unwrap();
        for bad in [
            "http://example.org",
            "https://abc.supabase.co/rest/v1",
            "https://user@abc.supabase.co",
            "ftp://x",
        ] {
            assert!(Server { url: bad.into(), key: "k".into() }.check().is_err(), "{bad}");
        }
        assert!(Server { url: "https://abc.supabase.co".into(), key: " ".into() }.check().is_err());
    }

    #[test]
    fn server_errors_map_to_plain_meanings() {
        assert!(matches!(
            error_from(400, &json!({"code": "P0001", "message": "openbark:not_approved"})),
            SyncError::Refused(c) if c == "not_approved"
        ));
        assert!(matches!(error_from(401, &json!({})), SyncError::Unauthorized));
        assert!(matches!(
            error_from(400, &json!({"error_code": "refresh_token_not_found"})),
            SyncError::SignedOut
        ));
        assert!(matches!(error_from(403, &json!({"error_code": "otp_expired"})), SyncError::User(_)));
    }

    #[test]
    fn the_session_id_comes_from_the_token() {
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(br#"{"sub":"u1","session_id":"5e8f0c2a-0000-4000-8000-000000000001"}"#);
        let token = format!("x.{payload}.y");
        assert_eq!(jwt_claim(&token, "session_id").as_deref(), Some("5e8f0c2a-0000-4000-8000-000000000001"));
        assert_eq!(jwt_claim("garbage", "session_id"), None);
    }
}
