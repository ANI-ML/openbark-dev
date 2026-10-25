//! The clinic hub (ADR-0029): the sync server a clinic's own computer runs
//! for the clinic's other devices, in place of a hosted Supabase project.
//!
//! - [`Hub`]: the calls (the 28 SQL functions of ADR-0017's schema, in
//!   [`calls`]), who may make them, and the store ([`store`]), sealed under
//!   the hub computer's data key.
//! - [`listener`]: TLS 1.3 on one port, only while hub mode is on, only for
//!   the allowed client ranges ([`net`]).
//! - [`tls`]: the hub's self-signed certificate, made once and kept.
//! - [`mdns`]: advertising `_openbark-hub._tcp` on the local network.
//!
//! The server role never holds a key that opens content: objects, blobs,
//! envelopes, wrapped keys and the roster arrive sealed on the devices
//! (ADR-0017 section 8). Authorization is mapped call by call in
//! docs/security/hub-authorization.md.

pub mod calls;
pub mod limits;
pub mod listener;
pub mod mdns;
pub mod net;
pub mod store;
pub mod tls;

use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, RwLock};

use chrono::{DateTime, Utc};
use os_sync::hub::{self as wire, CODE_ALPHABET, WINDOW_SECS};
use os_sync::keys;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use calls::{Ctx, Refusal, Res, deny, fail};
use limits::Guard;
use net::Cidr;
use store::Store;

pub use listener::Listener;
pub use mdns::Advertiser;

/// The largest request body (a push of 100 objects, or every wrapped key of
/// a rotation).
pub const MAX_BODY: usize = 12 << 20;
/// The default quota per account (ADR-0029 section 3).
pub const DEFAULT_QUOTA_MB: u64 = 20 * 1024;
/// How long a join code shown on the hub's screen works.
pub const CODE_TTL_SECS: i64 = 30 * 60;

#[derive(Debug, Clone, thiserror::Error)]
pub enum HubError {
    /// Something the person can fix, in plain words.
    #[error("{0}")]
    User(String),
    #[error("{0}")]
    Io(String),
}

/// The hub's settings (Settings, Clinic hub).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    /// Hub mode: off by default.
    #[serde(default)]
    pub on: bool,
    #[serde(default = "default_port")]
    pub port: u16,
    /// Client address ranges the listener accepts.
    #[serde(default = "net::default_ranges")]
    pub allowed: Vec<String>,
    /// Storage per account, in MB of sealed objects and blob parts.
    #[serde(default = "default_quota")]
    pub quota_mb: u64,
}

fn default_port() -> u16 {
    wire::DEFAULT_PORT
}

fn default_quota() -> u64 {
    DEFAULT_QUOTA_MB
}

impl Default for Settings {
    fn default() -> Self {
        Self { on: false, port: default_port(), allowed: net::default_ranges(), quota_mb: default_quota() }
    }
}

impl Settings {
    fn path(dir: &Path) -> PathBuf {
        dir.join("settings.json")
    }

    pub fn load(dir: &Path) -> Self {
        os_engine::vault::read(&Self::path(dir))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, dir: &Path) -> Result<(), HubError> {
        std::fs::create_dir_all(dir).map_err(|e| HubError::Io(e.to_string()))?;
        let bytes = serde_json::to_vec(self).map_err(|e| HubError::Io(e.to_string()))?;
        os_engine::vault::write(&Self::path(dir), &bytes).map_err(|e| HubError::Io(e.to_string()))
    }

    /// The ranges, checked; an empty list is refused (nothing could connect).
    pub fn ranges(&self) -> Result<Vec<Cidr>, HubError> {
        let r = net::parse_ranges(&self.allowed).map_err(HubError::User)?;
        if r.is_empty() {
            return Err(HubError::User("Allow at least one address range.".into()));
        }
        if self.port < 1024 {
            return Err(HubError::User("Choose a port from 1024 to 65535.".into()));
        }
        if self.quota_mb == 0 {
            return Err(HubError::User("The storage limit must be at least 1 MB.".into()));
        }
        Ok(r)
    }
}

/// Limits that tests shrink (`OPENBARK_HUB_RATE_IP_PER_MIN`,
/// `OPENBARK_HUB_RATE_ACCOUNT_PER_MIN`, `OPENBARK_HUB_CODE_TTL_SECS`).
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Requests a minute from one client address (or IPv6 /64).
    pub per_ip_per_min: u32,
    /// Requests a minute for one account (or one device not in an account yet).
    pub per_account_per_min: u32,
    pub code_ttl_secs: i64,
}

impl Default for Limits {
    fn default() -> Self {
        Self { per_ip_per_min: 3000, per_account_per_min: 2400, code_ttl_secs: CODE_TTL_SECS }
    }
}

impl Limits {
    pub fn from_env() -> Self {
        let get = |k: &str| std::env::var(k).ok().and_then(|v| v.trim().parse::<i64>().ok());
        let d = Self::default();
        Self {
            per_ip_per_min: get("OPENBARK_HUB_RATE_IP_PER_MIN")
                .map_or(d.per_ip_per_min, |v| v.clamp(10, 100_000) as u32),
            per_account_per_min: get("OPENBARK_HUB_RATE_ACCOUNT_PER_MIN")
                .map_or(d.per_account_per_min, |v| v.clamp(10, 100_000) as u32),
            code_ttl_secs: get("OPENBARK_HUB_CODE_TTL_SECS")
                .map_or(d.code_ttl_secs, |v| v.clamp(5, CODE_TTL_SECS)),
        }
    }
}

/// One request as the listener received it.
#[derive(Debug, Clone)]
pub struct HubRequest {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct HubResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

impl HubResponse {
    fn json(status: u16, v: &Value) -> Self {
        Self { status, body: serde_json::to_vec(v).unwrap_or_default() }
    }

    pub fn refusal(r: &Refusal) -> Self {
        Self::json(r.status, &json!({"message": format!("openbark:{}", r.code), "code": r.code}))
    }
}

/// The join code on the hub's screen. Kept in memory only; the store has
/// its keyed hash.
#[derive(Debug, Clone, Serialize)]
pub struct ShownCode {
    pub code: String,
    pub expires_at: DateTime<Utc>,
    pub used: bool,
}

/// A device as the hub's own screen lists it. The hub knows no names: those
/// are in each account's sealed roster.
#[derive(Debug, Clone, Serialize)]
pub struct DeviceSummary {
    pub id: String,
    /// The account's first 8 characters, to group a person's devices.
    pub account: Option<String>,
    pub platform: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub last_seen_at: Option<DateTime<Utc>>,
}

/// The keyed hash a join code is kept as.
pub fn code_hash(key_b64: &str, code: &str) -> String {
    let key = keys::unb64_32(key_b64).unwrap_or([0; 32]);
    hex::encode(keys::hmac(&key, &[b"openbark hub join code v1|", wire::normalise_code(code).as_bytes()]))
}

pub struct Hub {
    dir: PathBuf,
    store: Mutex<Store>,
    guard: Mutex<Guard>,
    ranges: RwLock<Vec<Cidr>>,
    quota: AtomicU64,
    limits: Limits,
    /// Requests timed before the hub started are refused: the nonces it
    /// remembers went with the last run.
    started: i64,
    shown: Mutex<Option<ShownCode>>,
}

impl Hub {
    /// Open the hub's store in `dir`. Refused without encryption at rest.
    pub fn open(dir: &Path, settings: &Settings, limits: Limits) -> Result<Self, HubError> {
        if !os_engine::vault::enabled() {
            return Err(HubError::User(
                "The clinic hub needs encryption at rest, which isn't available on this computer.".into(),
            ));
        }
        let ranges = settings.ranges()?;
        let mut store = Store::open(dir)?;
        let (o, b, d) = store.purge(Utc::now());
        if o + b + d > 0 {
            store.save()?;
        }
        Ok(Self {
            dir: dir.to_path_buf(),
            store: Mutex::new(store),
            guard: Mutex::new(Guard::default()),
            ranges: RwLock::new(ranges),
            quota: AtomicU64::new(settings.quota_mb.saturating_mul(1024 * 1024)),
            limits,
            started: Utc::now().timestamp(),
            shown: Mutex::new(None),
        })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn store(&self) -> MutexGuard<'_, Store> {
        self.store.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn guard(&self) -> MutexGuard<'_, Guard> {
        self.guard.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// New ranges and quota, from changed settings.
    pub fn apply(&self, settings: &Settings) -> Result<(), HubError> {
        let ranges = settings.ranges()?;
        *self.ranges.write().unwrap_or_else(|p| p.into_inner()) = ranges;
        self.quota.store(settings.quota_mb.saturating_mul(1024 * 1024), Ordering::Relaxed);
        Ok(())
    }

    /// Whether a client address may connect at all.
    pub fn allows(&self, ip: IpAddr) -> bool {
        net::allowed(&self.ranges.read().unwrap_or_else(|p| p.into_inner()), ip)
    }

    /// Answer one request from `peer`.
    pub fn handle(&self, peer: IpAddr, req: &HubRequest) -> HubResponse {
        match self.handle_inner(peer, req) {
            Ok(v) => HubResponse::json(200, &v),
            Err(r) => {
                tracing::debug!(code = %r.code, status = r.status, "hub refused a request");
                HubResponse::refusal(&r)
            }
        }
    }

    fn handle_inner(&self, peer: IpAddr, req: &HubRequest) -> Res<Value> {
        let peer = net::canonical(peer);
        if !self.allows(peer) {
            return Err(deny(403, "address_not_allowed"));
        }
        let peer_key = net::limit_key(peer);
        let now = Utc::now();
        let unix = now.timestamp();
        if !self.guard().allow(&format!("ip:{peer_key}"), self.limits.per_ip_per_min) {
            return Err(deny(429, "rate_limited"));
        }
        if req.method != "POST" {
            return Err(deny(405, "method_not_allowed"));
        }
        let name = req
            .path
            .strip_prefix("/rpc/")
            .filter(|n| {
                !n.is_empty() && n.len() <= 32 && n.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
            })
            .ok_or_else(|| deny(404, "unknown_call"))?;
        if req.body.len() > MAX_BODY {
            return Err(deny(413, "too_large"));
        }
        // The signature: who, when, which nonce.
        let header =
            |k: &str| req.headers.iter().find(|(n, _)| n.eq_ignore_ascii_case(k)).map(|(_, v)| v.as_str());
        let (Some(dev_h), Some(time_h), Some(nonce), Some(sig_h)) =
            (header(wire::H_DEVICE), header(wire::H_TIME), header(wire::H_NONCE), header(wire::H_SIGNATURE))
        else {
            return Err(deny(401, "unsigned"));
        };
        let device = calls::canonical_uuid(dev_h).ok_or_else(|| deny(401, "unsigned"))?;
        let time: i64 = time_h.trim().parse().map_err(|_| deny(401, "unsigned"))?;
        let nonce_ok = (16..=64).contains(&nonce.len())
            && nonce.bytes().all(|b| b.is_ascii_alphanumeric() || b"+/=-_".contains(&b));
        if !nonce_ok {
            return Err(deny(401, "unsigned"));
        }
        let sig = keys::unb64(sig_h).map_err(|_| deny(401, "unsigned"))?;
        if (unix - time).abs() > WINDOW_SECS || time < self.started {
            return Err(deny(401, "stale_request"));
        }
        let args: Value = serde_json::from_slice(&req.body).map_err(|_| fail("bad_request"))?;
        if !args.is_object() {
            return Err(fail("bad_request"));
        }
        let msg = wire::canonical(&req.method, &req.path, &req.body, time, nonce, dev_h);
        let mut st = self.store();
        // The registered key, or for a device the hub doesn't know yet (only
        // creating an account or asking to join) the key it brings.
        let known = st.ix.devices.get(&device).cloned();
        let key = match &known {
            Some(d) => keys::unb64(&d.ed25519_pub).ok(),
            None if calls::BOOTSTRAP.contains(&name) => {
                let body_dev = args.get("device_id").and_then(Value::as_str).and_then(calls::canonical_uuid);
                if body_dev.as_deref() != Some(device.as_str()) {
                    return Err(deny(401, "unknown_device"));
                }
                args.get("ed25519_pub").and_then(Value::as_str).and_then(|t| keys::unb64(t).ok())
            }
            None => return Err(deny(401, "unknown_device")),
        };
        let key: [u8; 32] = key.and_then(|k| k.try_into().ok()).ok_or_else(|| deny(401, "bad_signature"))?;
        if !keys::verify(&key, &msg, &sig) {
            return Err(deny(401, "bad_signature"));
        }
        {
            let mut g = self.guard();
            if !g.nonce(&device, nonce, unix, WINDOW_SECS) {
                return Err(deny(401, "replayed"));
            }
            let who = match known.as_ref().and_then(|d| d.account.as_deref()) {
                Some(a) => format!("acct:{a}"),
                None => format!("dev:{device}"),
            };
            if !g.allow(&who, self.limits.per_account_per_min) {
                return Err(deny(429, "rate_limited"));
            }
        }
        if known.as_ref().is_some_and(|d| d.status == "revoked") && !calls::REVOKED_OK.contains(&name) {
            return Err(deny(403, "revoked"));
        }
        let mut cx = Ctx {
            st: &mut *st,
            me: device,
            now,
            quota: self.quota.load(Ordering::Relaxed),
            guard: &self.guard,
            peer: peer_key,
        };
        let out = calls::dispatch(&mut cx, name, &args)?;
        if !calls::READ_ONLY.contains(&name) {
            st.save().map_err(|e| {
                tracing::error!(error = %e, "the hub couldn't save its index");
                deny(500, "internal")
            })?;
        }
        Ok(out)
    }

    /// A new join code for the hub's screen (single use, 30 minutes).
    pub fn new_join_code(&self) -> Result<ShownCode, HubError> {
        let bytes = keys::random::<{ wire::CODE_LEN }>().map_err(|e| HubError::Io(e.to_string()))?;
        // 256 is a multiple of 32: every character is equally likely.
        let code: String = bytes.iter().map(|b| CODE_ALPHABET[(b & 31) as usize] as char).collect();
        let now = Utc::now();
        let ttl = chrono::Duration::seconds(self.limits.code_ttl_secs);
        let mut st = self.store();
        calls::add_code(&mut st, &code, now, ttl);
        st.save()?;
        let shown = ShownCode { code, expires_at: now + ttl, used: false };
        *self.shown.lock().unwrap_or_else(|p| p.into_inner()) = Some(shown.clone());
        Ok(shown)
    }

    /// The join code on screen, while it lasts (and whether it was used).
    pub fn shown_code(&self) -> Option<ShownCode> {
        let mut shown = self.shown.lock().unwrap_or_else(|p| p.into_inner()).clone()?;
        if shown.expires_at <= Utc::now() {
            return None;
        }
        let st = self.store();
        let h = code_hash(&st.ix.code_key, &shown.code);
        shown.used = st.ix.codes.iter().any(|c| c.hash == h && c.used_at.is_some());
        Some(shown)
    }

    /// Every device the hub knows, newest first.
    pub fn devices(&self) -> Vec<DeviceSummary> {
        let st = self.store();
        let mut v: Vec<DeviceSummary> = st
            .ix
            .devices
            .iter()
            .map(|(id, d)| DeviceSummary {
                id: id.clone(),
                account: d.account.as_ref().map(|a| a.chars().take(8).collect()),
                platform: d.platform.clone(),
                status: d.status.clone(),
                created_at: d.created_at,
                last_seen_at: d.last_seen_at,
            })
            .collect();
        v.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        v
    }

    /// Accounts and the bytes they hold.
    pub fn usage(&self) -> (usize, u64) {
        let st = self.store();
        let bytes: u64 = st.ix.accounts.keys().map(|a| st.used_bytes(a)).sum();
        (st.ix.accounts.len(), bytes)
    }

    /// Remove a device from the hub's own screen (the website portal's
    /// revoke-device function before): refused at once, keys marked for
    /// rotation, which the account's next syncing device does.
    pub fn remove_device(&self, device_id: &str) -> Result<(), HubError> {
        let id = calls::canonical_uuid(device_id).ok_or_else(|| HubError::User("Unknown device.".into()))?;
        let mut st = self.store();
        let account =
            st.ix.devices.get(&id).ok_or_else(|| HubError::User("Unknown device.".into()))?.account.clone();
        calls::revoke(&mut st, account.as_deref(), &id, Utc::now())
            .map_err(|r| HubError::User(format!("The device couldn't be removed ({}).", r.code)))?;
        st.save()
    }

    /// Hourly housekeeping.
    pub fn purge(&self) {
        let mut st = self.store();
        let (o, b, d) = st.purge(Utc::now());
        if o + b + d > 0
            && let Err(e) = st.save()
        {
            tracing::warn!(error = %e, "the hub couldn't save after housekeeping");
        }
    }

    /// The decrypted store as the server role sees it (`openbark hub
    /// inspect`, for checks that it holds ciphertext only).
    pub fn inspect(dir: &Path) -> Result<Value, HubError> {
        let st = Store::open(dir)?;
        let mut v = serde_json::to_value(&st.ix).map_err(|e| HubError::Io(e.to_string()))?;
        if let Some(o) = v.as_object_mut() {
            o.remove("code_key");
        }
        // A running hub may replace a file between reading the index and
        // reading the file: those are skipped (the next look sees the new one).
        let mut bodies = Vec::new();
        for (space, objs) in &st.ix.objects {
            for (id, o) in objs {
                if o.deleted {
                    continue;
                }
                if let Ok(b) = st.read_object(space, id, o.version) {
                    bodies.push(json!({"space": space, "id": id, "body": keys::b64(&b)}));
                }
            }
        }
        let mut parts = Vec::new();
        for (space, blobs) in &st.ix.blobs {
            for k in blobs.keys() {
                if let Some((o, ver, p)) = store::split_blob_key(k)
                    && let Ok(b) = st.read_blob(space, o, ver, p)
                {
                    parts.push(json!({"space": space, "key": k, "body": keys::b64(&b)}));
                }
            }
        }
        v["object_bodies"] = json!(bodies);
        v["blob_bodies"] = json!(parts);
        Ok(v)
    }
}

#[cfg(test)]
mod tests;
