//! OpenBark end-to-end encrypted sync (ADR-0017, M3 phase 2).
//!
//! Off by default. When a clinician turns it on, transcripts, notes, visit
//! details and patient records sync between their devices through a
//! Supabase project, sealed on the device first. **Audio never syncs**: it
//! stays on the device that recorded it. The server holds ciphertext, public
//! keys and padded sizes only; it can lock a device out but can never let
//! one in.
//!
//! - [`keys`]: the key hierarchy (device keys, AMK, Recovery Kit, space keys,
//!   object keys), envelopes, pairing.
//! - [`seal`]: the OBS1 object and OBB1 blob formats.
//! - [`guard`]: the plaintext guard every request passes.
//! - [`map`] and [`merge`]: visit folders as objects; merging.
//! - [`roster`]: the signed device list.
//! - [`supabase`]: Auth and REST, the only network code.
//! - [`state`]: what sync keeps on this device (state, secrets, audit log).
//!
//! [`SyncService`] ties them together for one visits folder. Its methods
//! block; the app runs them on worker threads and polls every 30 seconds.
//!
//! Threat model notes for the external review (phase 3) are in
//! docs/security/sync-threat-model.md.

pub mod guard;
pub mod hub;
pub mod keys;
pub mod map;
pub mod merge;
pub mod roster;
pub mod seal;
pub mod state;
pub mod supabase;

mod account;
mod engine;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use os_engine::hlc::{Clock, Hlc};
use serde::{Deserialize, Serialize};

pub use account::{KitView, NextStep, PairingView, PendingView};
pub use engine::{Host, NoHost, SyncReport};
use roster::Roster;
use state::{Paths, Phase, SecretStore, Secrets, State};
use supabase::{AuthSession, Client, Server, Transport};

/// ANI.ML Health's hosted sync project (ADR-0017 owner decision 5). Its
/// publishable key is public by design: row level security protects each
/// account. A clinic can point OpenBark at its own Supabase instead.
pub const HOSTED_URL: &str = "https://nmecovtmijnfnjuadicp.supabase.co";
pub const HOSTED_KEY: &str = concat!("sb_publishable_", "f80ns1rDRfliZVnPi_LNBg_HnypbsgZ");

/// The default server: `OPENBARK_SYNC_URL` and `OPENBARK_SYNC_KEY` when set
/// (tests, self-hosting), else the hosted project.
pub fn default_server() -> Server {
    match (std::env::var("OPENBARK_SYNC_URL"), std::env::var("OPENBARK_SYNC_KEY")) {
        (Ok(url), Ok(key)) if !url.trim().is_empty() && !key.trim().is_empty() => {
            Server { url: url.trim().to_string(), key: key.trim().to_string() }
        }
        _ => Server { url: HOSTED_URL.into(), key: HOSTED_KEY.into() },
    }
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum SyncError {
    /// Something the person can fix, in plain words.
    #[error("{0}")]
    User(String),
    #[error("No connection to the sync service.")]
    Offline,
    /// The clinic hub didn't answer (ADR-0029 section 6).
    #[error("Hub not reachable: syncs when you're back on the clinic network.")]
    HubOffline,
    #[error("Network error: {0}")]
    Network(String),
    /// The server refused, with one of the sync functions' codes.
    #[error("The sync service refused: {0}")]
    Refused(String),
    #[error("The sync service answered unexpectedly: {0}")]
    Server(String),
    #[error("The sign-in has expired.")]
    Unauthorized,
    #[error("Signed out of sync. Sign in again to keep syncing.")]
    SignedOut,
    #[error("This device was removed from your sync account.")]
    Revoked,
    #[error("This device doesn't have the sync keys yet.")]
    NoKeys,
    /// The plaintext guard stopped a request. Nothing was sent.
    #[error("Stopped before sending: {0}")]
    Guard(String),
    #[error("{0}")]
    Crypto(String),
    #[error("{0}")]
    Format(String),
    /// Something the server sent doesn't check out (a signature, a rollback).
    #[error("{0}")]
    Integrity(String),
    #[error("{0}")]
    Keychain(String),
    #[error("{0}")]
    Io(String),
}

impl From<os_engine::EngineError> for SyncError {
    fn from(e: os_engine::EngineError) -> Self {
        SyncError::Io(e.to_string())
    }
}

/// One object row as the server returns it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteRow {
    pub id: String,
    pub kind: String,
    pub version: i64,
    pub seq: i64,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub wrapped_dek: Option<String>,
    #[serde(default)]
    pub key_version: Option<u32>,
    #[serde(default)]
    pub deleted: bool,
}

/// A device in the account, for the settings screen.
#[derive(Debug, Clone, Serialize)]
pub struct DeviceView {
    pub id: String,
    pub name: String,
    pub platform: String,
    /// pending, approved or revoked.
    pub status: String,
    pub this_device: bool,
    pub last_seen_at: Option<String>,
    pub approved_at: Option<String>,
}

/// Everything the settings screen shows about sync.
#[derive(Debug, Clone, Serialize, Default)]
pub struct SyncStatus {
    pub phase: Phase,
    pub email: Option<String>,
    pub server: String,
    /// Not the hosted project.
    pub custom_server: bool,
    pub device_id: String,
    pub device_name: String,
    pub devices: Vec<DeviceView>,
    pub last_sync_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub pending_uploads: usize,
    pub conflicts: Vec<state::Conflict>,
    pub warnings: Vec<String>,
    /// While this device waits to join: the code and QR to show.
    pub pairing: Option<PairingView>,
    /// While the new Recovery Kit isn't checked: which words to ask for.
    pub kit_check: Option<[usize; 2]>,
    /// Where the sync keys are kept: keychain or file.
    pub keys_in: String,
    pub since: Option<DateTime<Utc>>,
    pub linked_user: Option<String>,
    /// A sync run is going on now.
    pub busy: bool,
    /// The clinic hub this device syncs with (ADR-0029).
    pub hub: Option<HubView>,
}

/// The clinic hub, for the settings screen.
#[derive(Debug, Clone, Serialize)]
pub struct HubView {
    pub address: String,
    /// The start of the pinned certificate fingerprint (not a security check).
    pub fingerprint: String,
}

/// Sync for one visits folder.
pub struct SyncService {
    root: PathBuf,
    paths: Paths,
    /// This device's sync id: the visits folder's device id, or a fresh one
    /// after this device was removed (a removed id never rejoins).
    device: std::sync::RwLock<os_engine::device::Device>,
    store: Box<dyn SecretStore>,
    transport: Arc<dyn Transport>,
    inner: Mutex<Inner>,
    snapshot: Mutex<SyncStatus>,
    states_cache: Mutex<std::collections::BTreeMap<String, String>>,
}

struct Inner {
    state: State,
    secrets: Secrets,
    secrets_dirty: bool,
    session: Option<AuthSession>,
    clock: Clock,
    roster: Option<Roster>,
    /// Devices as the server lists them (refreshed each run).
    devices: Vec<serde_json::Value>,
    /// The key of the row just opened, for its blob parts.
    last_dek: Option<(String, i64, keys::SymKey)>,
}

/// A hub refusal in plain words where the person can act on it; the code
/// otherwise (the account and sync logic handles those).
fn hub_error(body: &serde_json::Value) -> SyncError {
    let code = body
        .get("message")
        .and_then(serde_json::Value::as_str)
        .and_then(|m| m.strip_prefix("openbark:"))
        .unwrap_or("unexpected")
        .to_string();
    let user = |s: &str| SyncError::User(s.into());
    match code.as_str() {
        "revoked" => SyncError::Revoked,
        "rate_limited" => user("The clinic hub is busy. Sync tries again shortly."),
        "quota_exceeded" => user(
            "Your space on the clinic hub is full. Ask whoever runs the hub to raise the limit (Settings, Clinic hub).",
        ),
        "locked" => user("Too many wrong join codes came from this device. Wait an hour, then try again."),
        "bad_join_code" => user(
            "That join code doesn't work: it may be mistyped, used already, or more than 30 minutes old. Ask the hub for a new one.",
        ),
        "join_code_required" => user("Enter a join code from the clinic hub."),
        "address_not_allowed" => user(
            "The clinic hub doesn't accept this device's network. Ask whoever runs the hub to check its allowed addresses.",
        ),
        "stale_request" => user(
            "This device's clock is more than 5 minutes off the hub's. Set the date and time automatically, then try again.",
        ),
        _ => SyncError::Refused(code),
    }
}

/// This platform's name for the device list.
pub fn platform() -> &'static str {
    if cfg!(target_os = "ios") {
        "ios"
    } else if cfg!(target_os = "android") {
        "android"
    } else if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "other"
    }
}

impl SyncService {
    /// Sync for the visits folder `root`, with the real network and the
    /// keychain (or `OPENBARK_SYNC_SECRETS_DIR`).
    pub fn for_root(root: &Path) -> Result<Self, SyncError> {
        let paths = Paths::for_root(root);
        let device = os_engine::device::for_root(root);
        let store = state::default_store(&paths, &device.id);
        Self::open(root, Arc::new(supabase::HttpTransport::new()?), store)
    }

    /// Sync with a given transport and secret store (tests).
    pub fn open(
        root: &Path,
        transport: Arc<dyn Transport>,
        store: Box<dyn SecretStore>,
    ) -> Result<Self, SyncError> {
        let paths = Paths::for_root(root);
        let state = state::load(&paths)?;
        let device = match &state.device_id {
            Some(id) => os_engine::device::Device::from_id(id),
            None => os_engine::device::for_root(root),
        };
        let secrets = store.load().unwrap_or_else(|e| {
            tracing::warn!(error = %e, "couldn't read the sync keys");
            Secrets::default()
        });
        let mut clock = Clock::new(device.id.clone());
        if let Some(last) = state.clock.as_deref().and_then(|c| c.parse::<Hlc>().ok()) {
            clock.observe_at(&last, 0);
        }
        let svc = Self {
            root: root.to_path_buf(),
            paths,
            device: std::sync::RwLock::new(device),
            store,
            transport,
            inner: Mutex::new(Inner {
                state,
                secrets,
                secrets_dirty: false,
                session: None,
                clock,
                roster: None,
                devices: Vec::new(),
                last_dek: None,
            }),
            snapshot: Mutex::new(SyncStatus::default()),
            states_cache: Mutex::new(Default::default()),
        };
        {
            let inner = svc.inner.lock().expect("sync lock");
            svc.publish(&inner, false);
        }
        Ok(svc)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn device_id(&self) -> String {
        self.me().id
    }

    /// This device's sync identity.
    fn me(&self) -> os_engine::device::Device {
        self.device.read().unwrap_or_else(|p| p.into_inner()).clone()
    }

    /// Sync is on (signed in, joining or active) on this device.
    pub fn is_on(&self) -> bool {
        !matches!(self.snapshot.lock().expect("snapshot").phase, Phase::Off | Phase::Revoked)
    }

    /// The status, without waiting for a sync run that's going on.
    pub fn status(&self) -> SyncStatus {
        match self.inner.try_lock() {
            Ok(inner) => {
                self.publish(&inner, false);
                self.snapshot.lock().expect("snapshot").clone()
            }
            Err(_) => {
                let mut s = self.snapshot.lock().expect("snapshot").clone();
                s.busy = true;
                s
            }
        }
    }

    /// The local audit log (device approvals, removals, rotations, stopped visits).
    pub fn audit_log(&self) -> Vec<serde_json::Value> {
        state::read_audit(&self.paths)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn client(&self, st: &State) -> Client {
        Client::new(st.server.clone().unwrap_or_else(default_server), self.transport.clone())
    }

    fn publish(&self, inner: &Inner, busy: bool) {
        let st = &inner.state;
        let server = st.server.clone().unwrap_or_else(default_server);
        let roster = inner.roster.as_ref();
        let devices = inner
            .devices
            .iter()
            .filter_map(|d| {
                let id = d["id"].as_str()?.to_string();
                let status = d["status"].as_str().unwrap_or_default().to_string();
                if status == "pending" {
                    return None;
                }
                let name = roster
                    .and_then(|r| r.get(&id))
                    .map(|e| e.name.clone())
                    .or_else(|| st.names.get(&id).cloned())
                    .unwrap_or_default();
                Some(DeviceView {
                    this_device: id == self.me().id,
                    id,
                    name,
                    platform: d["platform"].as_str().unwrap_or_default().to_string(),
                    status,
                    last_seen_at: d["last_seen_at"].as_str().map(String::from),
                    approved_at: d["approved_at"].as_str().map(String::from),
                })
            })
            .collect();
        let status = SyncStatus {
            phase: st.phase,
            email: st.email.clone(),
            server: st.hub.as_ref().map(|h| h.address()).unwrap_or_else(|| server.host()),
            custom_server: st.hub.is_none() && server.url != HOSTED_URL,
            device_id: self.me().id.clone(),
            device_name: st.device_name.clone(),
            devices,
            last_sync_at: st.last_sync_at,
            last_error: st.last_error.clone(),
            pending_uploads: st.outbox.len(),
            conflicts: st.conflicts.iter().rev().take(50).cloned().collect(),
            warnings: st.warnings.clone(),
            pairing: if st.phase == Phase::Pending { self.pairing_view(inner) } else { None },
            kit_check: if st.phase == Phase::KitCheck {
                account::kit_positions(&inner.secrets)
            } else {
                None
            },
            keys_in: self.store.kind().to_string(),
            since: st.since,
            linked_user: st.linked_user.clone(),
            busy,
            hub: st
                .hub
                .as_ref()
                .map(|h| HubView { address: h.address(), fingerprint: h.short_fingerprint() }),
        };
        *self.snapshot.lock().expect("snapshot") = status;
    }

    /// Save the state (and the secrets, if they changed).
    fn persist(&self, inner: &mut Inner) -> Result<(), SyncError> {
        inner.state.clock = Some(inner.clock.now().to_string());
        state::save(&self.paths, &inner.state)?;
        if inner.secrets_dirty {
            self.store.save(&inner.secrets)?;
            inner.secrets_dirty = false;
        }
        Ok(())
    }

    /// A valid access token, refreshing it when it's about to expire.
    fn access(&self, inner: &mut Inner) -> Result<String, SyncError> {
        if let Some(s) = &inner.session
            && !s.expires_soon()
        {
            return Ok(s.access_token.clone());
        }
        self.refresh(inner)
    }

    fn refresh(&self, inner: &mut Inner) -> Result<String, SyncError> {
        let token = inner
            .session
            .as_ref()
            .map(|s| s.refresh_token.clone())
            .or_else(|| inner.secrets.refresh_token.clone())
            .ok_or(SyncError::SignedOut)?;
        let fresh = self.client(&inner.state).refresh(&token)?;
        inner.secrets.refresh_token = Some(fresh.refresh_token.clone());
        inner.secrets_dirty = true;
        let access = fresh.access_token.clone();
        inner.session = Some(fresh);
        // The refresh token rotated: keep the new one before anything else.
        if let Err(e) = self.store.save(&inner.secrets) {
            tracing::warn!(error = %e, "couldn't keep the refreshed sign-in");
        } else {
            inner.secrets_dirty = false;
        }
        Ok(access)
    }

    /// Call a sync function with this device's session; refresh once on an
    /// expired token.
    fn rpc(
        &self,
        inner: &mut Inner,
        name: &str,
        args: serde_json::Value,
    ) -> Result<serde_json::Value, SyncError> {
        if let Some(link) = inner.state.hub.clone() {
            return self.hub_rpc(inner, &link, name, args);
        }
        let access = self.access(inner)?;
        match self.client(&inner.state).rpc(name, args.clone(), Some(&access)) {
            Err(SyncError::Unauthorized) => {
                let access = self.refresh(inner)?;
                self.client(&inner.state).rpc(name, args, Some(&access))
            }
            other => other,
        }
    }

    /// Call the clinic hub (ADR-0029): the same calls, signed with this
    /// device's Ed25519 key instead of a sign-in token, over pinned TLS. The
    /// body passes the plaintext guard first.
    fn hub_rpc(
        &self,
        inner: &Inner,
        link: &hub::HubLink,
        name: &str,
        args: serde_json::Value,
    ) -> Result<serde_json::Value, SyncError> {
        guard::check_request(&args)?;
        let dk = self.device_keys(inner)?;
        let body = serde_json::to_vec(&args).expect("json serialises");
        let mut headers = hub::sign(&dk, &self.me().id, "POST", &format!("/rpc/{name}"), &body)?;
        headers.push(("Content-Type".into(), "application/json".into()));
        headers.push((hub::H_PIN.into(), link.fingerprint.clone()));
        let resp = self.transport.send(&supabase::Request {
            method: "POST",
            url: link.url(name),
            headers,
            body: Some(body),
        })?;
        let parsed: serde_json::Value = serde_json::from_slice(&resp.body).unwrap_or(serde_json::Value::Null);
        if (200..300).contains(&resp.status) { Ok(parsed) } else { Err(hub_error(&parsed)) }
    }

    fn device_keys(&self, inner: &Inner) -> Result<keys::DeviceKeys, SyncError> {
        inner.secrets.device_keys()?.ok_or(SyncError::NoKeys)
    }

    fn account_keys<'a>(&self, inner: &'a Inner) -> Result<&'a state::AccountKeys, SyncError> {
        inner.secrets.keys.as_ref().ok_or(SyncError::NoKeys)
    }

    /// The device name for a device id or a run-folder tag.
    fn name_of(&self, inner: &Inner, id_or_tag: &str) -> Option<String> {
        if let Some(r) = &inner.roster {
            if let Some(e) = r.get(id_or_tag) {
                return Some(e.name.clone());
            }
            if let Some(n) = r.name_for_tag(id_or_tag) {
                return Some(n);
            }
        }
        inner.state.names.get(id_or_tag).cloned()
    }
}
