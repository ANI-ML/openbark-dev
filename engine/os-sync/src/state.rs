//! What sync remembers on this device.
//!
//! - **State** (`<visits>/.openbark/sync/state.json`, sealed under this
//!   device's data key by the vault like every visit file): whether sync is
//!   on, the server, the account and space ids, the pull cursor, the clock,
//!   what was last pushed or pulled for each object (hashes and versions,
//!   never contents), the local conflict log, and the outbox.
//! - **Secrets** (the OS keychain, service "OpenBark"; or a sealed file when
//!   the keychain isn't available or a test asks for one): the device's
//!   private keys, the refresh token, and the account keys it was given
//!   (AMK and space keys). They never sync and never leave the device.
//! - **The audit log** (`audit.log` beside the state, append-only, one
//!   sealed line per event): device approvals, removals, key rotations,
//!   stopped visits (ADR-0017 section 10).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::SyncError;
use crate::keys::{self, DeviceKeys, SymKey};
use crate::supabase::Server;

/// Where sync is in its life on this device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    /// Sync is off (the default).
    #[default]
    Off,
    /// A code was emailed; waiting for it.
    CodeSent,
    /// This device asked to join an account and waits for approval.
    Pending,
    /// The account was just created: the Recovery Kit must be checked
    /// (two of its words typed back) before anything syncs.
    KitCheck,
    /// Syncing.
    Active,
    /// Signed out (the session ended); the keys are kept. Sign in again.
    SignedOut,
    /// This device was removed from the account. Its synced copies were
    /// erased; its own visits stay.
    Revoked,
}

/// What was last pushed or pulled for one field of a visit header.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldSync {
    pub hash: String,
    pub hlc: String,
}

/// What was last pushed or pulled for one object.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjSync {
    /// The server's version of it.
    #[serde(default)]
    pub version: Option<i64>,
    #[serde(default)]
    pub hash: String,
    #[serde(default)]
    pub hlc: String,
}

/// One visit's sync bookkeeping, by the visit's id (`session.json` `id`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VisitSync {
    #[serde(default)]
    pub header: ObjSync,
    #[serde(default)]
    pub fields: BTreeMap<String, FieldSync>,
    #[serde(default)]
    pub transcript: ObjSync,
    /// Runs on the server (pushed from here or pulled), by run id.
    #[serde(default)]
    pub runs: BTreeSet<String>,
    /// Signatures and addenda on the server.
    #[serde(default)]
    pub signs: BTreeSet<String>,
    #[serde(default)]
    pub addenda: BTreeSet<String>,
    /// Tooth chart confirmations, per run.
    #[serde(default)]
    pub checks: BTreeMap<String, ObjSync>,
    /// Came from another device (its folder is a synced copy).
    #[serde(default)]
    pub adopted: bool,
}

/// A change made on two devices at once: both values are kept here, and
/// Review can show them (ADR-0017 section 9).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conflict {
    pub visit: String,
    pub field: String,
    pub kept: serde_json::Value,
    pub other: serde_json::Value,
    /// Which device's value was kept: "this device" or the other's name.
    pub kept_from: String,
    pub other_from: String,
    pub at: DateTime<Utc>,
}

/// A pulled row that couldn't be applied yet (its visit is busy, or it
/// arrived before the visit or run it belongs to). Kept sealed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Deferred {
    pub row: crate::RemoteRow,
    pub tries: u32,
}

/// A sealed item waiting to be pushed (the persistent outbox).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutItem {
    pub id: String,
    pub kind: String,
    pub op: String,
    pub base_version: Option<i64>,
    /// The planned version (base + 1); blob parts are uploaded under it.
    pub version: i64,
    pub body: String,
    pub wrapped_dek: String,
    pub key_version: u32,
    pub padded_size: usize,
    #[serde(default)]
    pub blobs: Vec<String>,
    /// The plaintext's hash: the same change isn't queued twice.
    #[serde(default)]
    pub hash: String,
    /// What to record once the server has it.
    pub commit: crate::map::Commit,
}

/// Seen versions of each object: rollback detection.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Seen {
    pub version: i64,
    pub dev: String,
    pub ctr: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct State {
    #[serde(default)]
    pub phase: Phase,
    #[serde(default)]
    pub server: Option<Server>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub user_id: Option<String>,
    /// This device's name in the roster ("Front desk PC").
    #[serde(default)]
    pub device_name: String,
    #[serde(default)]
    pub space_id: Option<String>,
    #[serde(default)]
    pub amk_version: u32,
    #[serde(default)]
    pub key_version: u32,
    #[serde(default)]
    pub roster_version: i64,
    /// Visits from this time on sync (single-user mode).
    #[serde(default)]
    pub since: Option<DateTime<Utc>>,
    /// With a team: the member who linked the account. Only visits they
    /// recorded, drafted or signed sync.
    #[serde(default)]
    pub linked_user: Option<String>,
    /// This device's sync id when it isn't the visits folder's device id
    /// (after this device was removed, it joins again under a new id).
    #[serde(default)]
    pub device_id: Option<String>,
    /// When this device asked to join (its code lasts 10 minutes).
    #[serde(default)]
    pub pending_since: Option<DateTime<Utc>>,
    /// This device's counter for the objects it seals.
    #[serde(default)]
    pub counter: u64,
    /// The last clock timestamp this device issued or saw.
    #[serde(default)]
    pub clock: Option<String>,
    /// The pull cursor (server seq).
    #[serde(default)]
    pub cursor: i64,
    #[serde(default)]
    pub visits: BTreeMap<String, VisitSync>,
    #[serde(default)]
    pub patients: BTreeMap<String, ObjSync>,
    #[serde(default)]
    pub seen: BTreeMap<String, Seen>,
    #[serde(default)]
    pub conflicts: Vec<Conflict>,
    #[serde(default)]
    pub deferred: Vec<Deferred>,
    #[serde(default)]
    pub outbox: Vec<OutItem>,
    /// Visits whose syncing was stopped (a tombstone): kept here, never sent.
    #[serde(default)]
    pub stopped: BTreeSet<String>,
    /// Synced visits deleted on this device: never brought back by a pull.
    #[serde(default)]
    pub dropped: BTreeSet<String>,
    /// Device names by id and by run-folder tag, from the roster.
    #[serde(default)]
    pub names: BTreeMap<String, String>,
    #[serde(default)]
    pub last_sync_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub last_error: Option<String>,
    /// Integrity warnings (a rollback, an unknown signer): shown, kept.
    #[serde(default)]
    pub warnings: Vec<String>,
}

impl State {
    pub fn warn(&mut self, w: impl Into<String>) {
        let w = w.into();
        tracing::warn!(warning = %w, "sync integrity");
        if !self.warnings.contains(&w) {
            self.warnings.push(w);
            if self.warnings.len() > 50 {
                self.warnings.remove(0);
            }
        }
    }
}

/// Paths of sync's files for a visits folder.
#[derive(Debug, Clone)]
pub struct Paths {
    pub dir: PathBuf,
}

impl Paths {
    pub fn for_root(root: &Path) -> Self {
        Self { dir: root.join(os_engine::HOUSEKEEPING_DIR).join("sync") }
    }

    pub fn state(&self) -> PathBuf {
        self.dir.join("state.json")
    }

    pub fn audit(&self) -> PathBuf {
        self.dir.join("audit.log")
    }

    pub fn secrets_file(&self) -> PathBuf {
        self.dir.join("secrets.json")
    }
}

pub fn load(paths: &Paths) -> Result<State, SyncError> {
    match os_engine::vault::read(&paths.state()) {
        Ok(bytes) => {
            serde_json::from_slice(&bytes).map_err(|e| SyncError::Format(format!("sync state: {e}")))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(State::default()),
        Err(e) => Err(SyncError::Io(e.to_string())),
    }
}

pub fn save(paths: &Paths, state: &State) -> Result<(), SyncError> {
    std::fs::create_dir_all(&paths.dir).map_err(|e| SyncError::Io(e.to_string()))?;
    let bytes = serde_json::to_vec(state).expect("state serialises");
    os_engine::vault::write(&paths.state(), &bytes).map_err(|e| SyncError::Io(e.to_string()))
}

/// One line of the local audit log.
pub fn audit(paths: &Paths, event: &str, detail: serde_json::Value) {
    let line = serde_json::json!({"at": Utc::now(), "event": event, "detail": detail});
    if std::fs::create_dir_all(&paths.dir).is_ok()
        && let Err(e) = os_engine::vault::append_line(&paths.audit(), line.to_string().as_bytes())
    {
        tracing::warn!(error = %e, "couldn't write the sync audit log");
    }
}

pub fn read_audit(paths: &Paths) -> Vec<serde_json::Value> {
    os_engine::vault::read_lines(&paths.audit())
        .map(|lines| lines.into_iter().filter_map(|(_, b)| serde_json::from_slice(&b).ok()).collect())
        .unwrap_or_default()
}

// ---- secrets -----------------------------------------------------------------

/// The account keys this device was given.
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct AccountKeys {
    pub amk_version: u32,
    #[serde(default)]
    pub amk: String,
    /// Space key versions (base64) and the naming key.
    #[serde(default)]
    pub space_keys: BTreeMap<u32, String>,
    #[serde(default)]
    pub naming: String,
}

impl AccountKeys {
    pub fn amk(&self) -> Result<SymKey, SyncError> {
        Ok(SymKey(keys::unb64_32(&self.amk).map_err(|_| SyncError::NoKeys)?))
    }

    pub fn space_key(&self, version: u32) -> Result<SymKey, SyncError> {
        let k = self.space_keys.get(&version).ok_or(SyncError::NoKeys)?;
        Ok(SymKey(keys::unb64_32(k)?))
    }

    pub fn naming(&self) -> Result<SymKey, SyncError> {
        Ok(SymKey(keys::unb64_32(&self.naming).map_err(|_| SyncError::NoKeys)?))
    }
}

/// Everything secret sync keeps for this device.
#[derive(Default, Serialize, Deserialize)]
pub struct Secrets {
    #[serde(default)]
    pub device: Option<String>,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub keys: Option<AccountKeys>,
    /// The Recovery Kit's seed, only until its words are checked.
    #[serde(default)]
    pub kit_unchecked: Option<String>,
    /// The pairing code this pending device shows.
    #[serde(default)]
    pub pairing: Option<String>,
}

impl Secrets {
    pub fn device_keys(&self) -> Result<Option<DeviceKeys>, SyncError> {
        self.device.as_deref().map(DeviceKeys::from_text).transpose()
    }
}

/// Where secrets are kept.
pub trait SecretStore: Send + Sync {
    fn load(&self) -> Result<Secrets, SyncError>;
    fn save(&self, secrets: &Secrets) -> Result<(), SyncError>;
    fn erase(&self) -> Result<(), SyncError>;
    /// "keychain", "file" or "memory", for the status screen.
    fn kind(&self) -> &'static str;
}

/// The OS keychain, one item per device (`sync-<device id>`).
pub struct KeychainStore {
    account: String,
}

impl KeychainStore {
    pub fn new(device_id: &str) -> Self {
        Self { account: format!("sync-{device_id}") }
    }
}

impl SecretStore for KeychainStore {
    fn load(&self) -> Result<Secrets, SyncError> {
        match os_engine::keychain::get(&self.account).map_err(SyncError::Keychain)? {
            Some(text) => {
                serde_json::from_str(&text).map_err(|_| SyncError::Format("damaged sync secrets".into()))
            }
            None => Ok(Secrets::default()),
        }
    }

    fn save(&self, secrets: &Secrets) -> Result<(), SyncError> {
        let text = serde_json::to_string(secrets).expect("secrets serialise");
        os_engine::keychain::set(&self.account, &text).map_err(SyncError::Keychain)?;
        // Some stores accept a write they can't keep: read it back.
        match os_engine::keychain::get(&self.account).map_err(SyncError::Keychain)? {
            Some(back) if back == text => Ok(()),
            _ => Err(SyncError::Keychain("the keychain didn't keep the sync keys".into())),
        }
    }

    fn erase(&self) -> Result<(), SyncError> {
        os_engine::keychain::delete(&self.account).map(|_| ()).map_err(SyncError::Keychain)
    }

    fn kind(&self) -> &'static str {
        "keychain"
    }
}

/// A file sealed under this device's data key (tests, servers, and computers
/// without a usable keychain).
pub struct FileStore {
    path: PathBuf,
}

impl FileStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl SecretStore for FileStore {
    fn load(&self) -> Result<Secrets, SyncError> {
        match os_engine::vault::read(&self.path) {
            Ok(b) => serde_json::from_slice(&b).map_err(|_| SyncError::Format("damaged sync secrets".into())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Secrets::default()),
            Err(e) => Err(SyncError::Io(e.to_string())),
        }
    }

    fn save(&self, secrets: &Secrets) -> Result<(), SyncError> {
        if let Some(d) = self.path.parent() {
            std::fs::create_dir_all(d).map_err(|e| SyncError::Io(e.to_string()))?;
        }
        os_engine::vault::write(&self.path, &serde_json::to_vec(secrets).expect("serialises"))
            .map_err(|e| SyncError::Io(e.to_string()))
    }

    fn erase(&self) -> Result<(), SyncError> {
        match std::fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(SyncError::Io(e.to_string())),
        }
    }

    fn kind(&self) -> &'static str {
        "file"
    }
}

/// In memory only (tests).
#[derive(Default)]
pub struct MemoryStore(std::sync::Mutex<Option<String>>);

impl SecretStore for MemoryStore {
    fn load(&self) -> Result<Secrets, SyncError> {
        match self.0.lock().expect("lock").as_deref() {
            Some(t) => Ok(serde_json::from_str(t).expect("valid")),
            None => Ok(Secrets::default()),
        }
    }

    fn save(&self, secrets: &Secrets) -> Result<(), SyncError> {
        *self.0.lock().expect("lock") = Some(serde_json::to_string(secrets).expect("serialises"));
        Ok(())
    }

    fn erase(&self) -> Result<(), SyncError> {
        *self.0.lock().expect("lock") = None;
        Ok(())
    }

    fn kind(&self) -> &'static str {
        "memory"
    }
}

/// The store for this device: `OPENBARK_SYNC_SECRETS_DIR` (a sealed file
/// there), else the keychain, falling back to a sealed file in the sync
/// folder when the keychain can't keep them.
pub fn default_store(paths: &Paths, device_id: &str) -> Box<dyn SecretStore> {
    if let Some(dir) = std::env::var_os("OPENBARK_SYNC_SECRETS_DIR").filter(|d| !d.is_empty()) {
        return Box::new(FileStore::new(PathBuf::from(dir).join(format!("sync-{device_id}.json"))));
    }
    if os_engine::keychain::available() {
        let k = KeychainStore::new(device_id);
        if k.load().is_ok() {
            return Box::new(Fallback { primary: k, file: FileStore::new(paths.secrets_file()) });
        }
    }
    Box::new(FileStore::new(paths.secrets_file()))
}

/// The keychain, or a sealed file when the keychain refuses a write.
struct Fallback {
    primary: KeychainStore,
    file: FileStore,
}

impl SecretStore for Fallback {
    fn load(&self) -> Result<Secrets, SyncError> {
        if self.file.path.is_file() {
            return self.file.load();
        }
        self.primary.load()
    }

    fn save(&self, secrets: &Secrets) -> Result<(), SyncError> {
        if !self.file.path.is_file() && self.primary.save(secrets).is_ok() {
            return Ok(());
        }
        tracing::warn!("keychain unavailable for sync keys; keeping them in a sealed file");
        self.file.save(secrets)
    }

    fn erase(&self) -> Result<(), SyncError> {
        let a = self.primary.erase();
        self.file.erase()?;
        a
    }

    fn kind(&self) -> &'static str {
        if self.file.path.is_file() { "file" } else { "keychain" }
    }
}
