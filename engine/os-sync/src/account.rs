//! Turning sync on, devices and keys (ADR-0017 sections 2 to 4).
//!
//! - **First device:** sign in with an emailed code; the device makes the
//!   AMK, the Recovery Kit, the space and its first key, and registers
//!   itself. Nothing syncs until two of the kit's words are typed back.
//! - **Another device:** signs in, makes its own keys and a 6-word pairing
//!   code, and registers as pending (10 minutes). On a device that already
//!   syncs, the person types the code: that device checks the new one's keys
//!   against the code and wraps the AMK to it, the code bound in. Or the new
//!   device approves itself with the Recovery Kit.
//! - **Removing a device:** refused by the server at once; the keys are
//!   rotated (a new AMK and space key; every object key re-wrapped).

use chrono::{Duration, Utc};
use serde::Serialize;
use serde_json::{Value, json};

use crate::keys::{self, DeviceKeys, Kit, PairingCode, SymKey};
use crate::roster::{Entry, Roster};
use crate::state::{self, AccountKeys, Phase, State};
use crate::{Inner, SyncError, SyncService, default_server, platform};
use crate::supabase::Server;

/// What happens after the code is accepted.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "next", rename_all = "snake_case")]
pub enum NextStep {
    /// A new account: show the Recovery Kit, then check two of its words.
    Kit(KitView),
    /// The account exists: this device waits to be added.
    Pair(PairingView),
    /// This device was already part of the account: syncing.
    Active,
}

#[derive(Debug, Clone, Serialize)]
pub struct KitView {
    pub words: Vec<String>,
    pub qr_svg: String,
    /// Which two words (1-based) to type back.
    pub check: [usize; 2],
}

#[derive(Debug, Clone, Serialize)]
pub struct PairingView {
    pub words: Vec<String>,
    pub qr_svg: String,
    pub expires_at: Option<String>,
}

/// A device waiting to join, found by its code.
#[derive(Debug, Clone, Serialize)]
pub struct PendingView {
    pub device_id: String,
    pub name: String,
    pub platform: String,
}

/// Which two kit words to ask for (1-based), from the kit itself.
pub(crate) fn kit_positions(secrets: &state::Secrets) -> Option<[usize; 2]> {
    let seed = keys::unb64_32(secrets.kit_unchecked.as_deref()?).ok()?;
    let h = keys::sha256(&[b"openbark kit check", &seed]);
    let a = (h[0] as usize % 24) + 1;
    let mut b = (h[1] as usize % 24) + 1;
    if b == a {
        b = a % 24 + 1;
    }
    Some(if a < b { [a, b] } else { [b, a] })
}

fn platform_ok(p: &str) -> &str {
    if ["windows", "macos", "linux", "ios", "android", "web", "other"].contains(&p) { p } else { "other" }
}

impl SyncService {
    /// Choose the server before signing in: `None` for the hosted one.
    pub fn set_server(&self, server: Option<Server>) -> Result<(), SyncError> {
        let mut inner = self.lock();
        if !matches!(inner.state.phase, Phase::Off | Phase::CodeSent | Phase::SignedOut | Phase::Revoked) {
            return Err(SyncError::User("Turn off sync before changing the server.".into()));
        }
        if let Some(s) = &server {
            s.check()?;
        }
        inner.state.server = server;
        self.persist(&mut inner)?;
        self.publish(&inner, false);
        Ok(())
    }

    /// Step 1 of turning sync on: email a 6-digit code.
    pub fn send_code(&self, email: &str) -> Result<(), SyncError> {
        let email = email.trim().to_ascii_lowercase();
        if email.len() < 3 || email.len() > 254 || !email.contains('@') || email.contains(char::is_whitespace) {
            return Err(SyncError::User("Enter your email address.".into()));
        }
        let mut inner = self.lock();
        if matches!(inner.state.phase, Phase::Active | Phase::Pending | Phase::KitCheck) {
            return Err(SyncError::User("Sync is already on.".into()));
        }
        self.client(&inner.state).send_code(&email)?;
        inner.state.email = Some(email);
        if inner.state.phase != Phase::SignedOut {
            inner.state.phase = Phase::CodeSent;
        }
        inner.state.last_error = None;
        self.persist(&mut inner)?;
        self.publish(&inner, false);
        Ok(())
    }

    /// Step 2: the code. Creates the account, asks to join it, or (for a
    /// device already in it) carries on. `backfill_days`: how far back this
    /// device's visits upload (None: all of them). `linked_user`: with a
    /// team, the member whose visits sync.
    pub fn verify_code(
        &self,
        code: &str,
        device_name: &str,
        backfill_days: Option<u32>,
        linked_user: Option<String>,
    ) -> Result<NextStep, SyncError> {
        let code: String = code.chars().filter(|c| c.is_ascii_digit()).collect();
        if code.len() != 6 {
            return Err(SyncError::User("Enter the 6-digit code from the email.".into()));
        }
        let mut inner = self.lock();
        let email = inner.state.email.clone().ok_or_else(|| SyncError::User("Ask for a code first.".into()))?;
        let session = self.client(&inner.state).verify_code(&email, &code)?;
        if let Some(prev) = &inner.state.user_id
            && *prev != session.user_id
            && inner.secrets.keys.is_some()
        {
            return Err(SyncError::User(
                "This computer syncs with a different account. Turn off sync and delete that account's link first.".into(),
            ));
        }
        inner.secrets.refresh_token = Some(session.refresh_token.clone());
        inner.secrets_dirty = true;
        inner.state.user_id = Some(session.user_id.clone());
        inner.session = Some(session);
        let name = device_name.trim();
        if !name.is_empty() {
            inner.state.device_name = name.chars().take(60).collect();
        } else if inner.state.device_name.is_empty() {
            inner.state.device_name = default_device_name();
        }
        if inner.state.since.is_none() {
            inner.state.since = backfill_days.map(|d| Utc::now() - Duration::days(d as i64));
            if backfill_days.is_none() {
                inner.state.since = Some(chrono::DateTime::<Utc>::MIN_UTC);
            }
        }
        if linked_user.is_some() {
            inner.state.linked_user = linked_user;
        }
        if inner.state.server.is_none() {
            inner.state.server = Some(default_server());
        }
        let acct = self.rpc(&mut inner, "account_state", json!({}))?;
        let step = if !acct["account"].as_bool().unwrap_or(false) {
            self.create_account(&mut inner)?
        } else if let Some(device) = inner.secrets.device.clone().filter(|_| inner.state.space_id.is_some()) {
            // This device was in the account before: bind it to this session.
            let keys = DeviceKeys::from_text(&device)?;
            let r = self.rpc(
                &mut inner,
                "rebind_device",
                json!({"device_id": self.device.id, "rebind_secret": keys::b64(keys.rebind_secret())}),
            );
            match r {
                Ok(v) if v["device"]["status"] == "approved" => {
                    inner.state.phase = Phase::Active;
                    self.refresh_keys(&mut inner)?;
                    NextStep::Active
                }
                Ok(v) if v["device"]["status"] == "revoked" => {
                    self.handle_revoked(&mut inner);
                    return Err(SyncError::Revoked);
                }
                Ok(_) | Err(SyncError::Refused(_)) => self.register_pending(&mut inner)?,
                Err(e) => return Err(e),
            }
        } else {
            self.register_pending(&mut inner)?
        };
        inner.state.last_error = None;
        self.persist(&mut inner)?;
        self.publish(&inner, false);
        Ok(step)
    }

    fn create_account(&self, inner: &mut Inner) -> Result<NextStep, SyncError> {
        let device = match inner.secrets.device_keys()? {
            Some(d) => d,
            None => DeviceKeys::generate()?,
        };
        let amk = SymKey::random()?;
        let kit = Kit::generate()?;
        let space = uuid::Uuid::new_v4().to_string();
        let space_key = SymKey::random()?;
        let naming = SymKey::random()?;
        let account = inner.state.user_id.clone().ok_or(SyncError::SignedOut)?;
        let roster = Roster {
            kit_pub: keys::b64(&kit.x_pub()),
            devices: vec![self.self_entry(inner, &device)],
        };
        let sealed_roster = roster.seal(&amk, &account, 1, &device, &self.device.id)?;
        let args = json!({
            "device_id": self.device.id,
            "platform": platform_ok(platform()),
            "x25519_pub": keys::b64(&device.x_pub()),
            "ed25519_pub": keys::b64(&device.ed_pub()),
            "rebind_verifier": keys::b64(&device.rebind_verifier()),
            "kit_pub": keys::b64(&kit.x_pub()),
            "kit_verifier": keys::b64(&kit.verifier()),
            "space_id": space,
            "device_envelope": keys::b64(&keys::seal_to(&device.x_pub(), &amk.0, &keys::amk_aad(1, &self.device.id), None)?),
            "kit_envelope": keys::b64(&keys::seal_to(&kit.x_pub(), &amk.0, &keys::amk_aad(1, "kit"), None)?),
            "space_key": keys::b64(&keys::wrap_space_key(&amk, &space, 1, 1, &space_key, &naming)?),
            "roster": keys::b64(&sealed_roster),
        });
        // Keep the keys before the server has them, so a lost reply never
        // leaves an account this device can't open.
        inner.secrets.device = Some(device.to_text());
        inner.secrets.keys = Some(AccountKeys {
            amk_version: 1,
            amk: keys::b64(&amk.0),
            space_keys: [(1u32, keys::b64(&space_key.0))].into(),
            naming: keys::b64(&naming.0),
        });
        inner.secrets.kit_unchecked = Some(keys::b64(kit.seed()));
        self.store.save(&inner.secrets)?;
        inner.secrets_dirty = false;
        self.rpc(inner, "create_account", args)?;
        let st = &mut inner.state;
        st.space_id = Some(space);
        st.amk_version = 1;
        st.key_version = 1;
        st.roster_version = 1;
        st.phase = Phase::KitCheck;
        inner.roster = Some(roster);
        state::audit(&self.paths, "account_created", json!({"device": self.device.id}));
        Ok(NextStep::Kit(self.kit_view(inner).ok_or(SyncError::NoKeys)?))
    }

    fn self_entry(&self, inner: &Inner, device: &DeviceKeys) -> Entry {
        Entry {
            id: self.device.id.clone(),
            name: inner.state.device_name.clone(),
            platform: platform_ok(platform()).to_string(),
            x25519_pub: keys::b64(&device.x_pub()),
            ed25519_pub: keys::b64(&device.ed_pub()),
            added_at: Utc::now(),
            removed_at: None,
            last_ctr: None,
        }
    }

    fn kit_view(&self, inner: &Inner) -> Option<KitView> {
        let seed = keys::unb64_32(inner.secrets.kit_unchecked.as_deref()?).ok()?;
        let kit = Kit::from_seed(seed);
        let words = kit.words();
        Some(KitView {
            qr_svg: keys::qr_svg(&format!("openbark-kit:1:{}", words.join("-"))),
            words,
            check: kit_positions(&inner.secrets)?,
        })
    }

    /// The new Recovery Kit, while its words aren't checked yet (to print).
    pub fn kit(&self) -> Option<KitView> {
        let inner = self.lock();
        self.kit_view(&inner)
    }

    /// Check the two words. Syncing starts only after this; the kit's seed
    /// is then erased from this device.
    pub fn confirm_kit(&self, answers: &[(usize, String)]) -> Result<(), SyncError> {
        let mut inner = self.lock();
        let seed = keys::unb64_32(inner.secrets.kit_unchecked.as_deref().ok_or(SyncError::NoKeys)?)?;
        let words = Kit::from_seed(seed).words();
        let want = kit_positions(&inner.secrets).ok_or(SyncError::NoKeys)?;
        for pos in want {
            let given = answers.iter().find(|(p, _)| *p == pos).map(|(_, w)| w.as_str()).unwrap_or_default();
            let typed = keys::normalise_words(given).unwrap_or_default();
            if typed.len() != 1 || typed[0] != words[pos - 1] {
                return Err(SyncError::User(format!(
                    "Word {pos} doesn't match the Recovery Kit. Check the printed kit and try again."
                )));
            }
        }
        inner.secrets.kit_unchecked = None;
        inner.secrets_dirty = true;
        inner.state.phase = Phase::Active;
        state::audit(&self.paths, "kit_checked", json!({}));
        self.persist(&mut inner)?;
        self.publish(&inner, false);
        Ok(())
    }

    fn register_pending(&self, inner: &mut Inner) -> Result<NextStep, SyncError> {
        let device = match inner.secrets.device_keys()? {
            Some(d) => d,
            None => {
                let d = DeviceKeys::generate()?;
                inner.secrets.device = Some(d.to_text());
                d
            }
        };
        let code = PairingCode::generate()?;
        let pk = code.key(&self.device.id)?;
        let blob = keys::wrap(
            &keys::pair_blob_key(&pk),
            self.device.id.as_bytes(),
            &serde_json::to_vec(&json!({"name": inner.state.device_name, "platform": platform()})).expect("json"),
        )?;
        inner.secrets.pairing = Some(code.text());
        inner.secrets_dirty = true;
        self.store.save(&inner.secrets)?;
        inner.secrets_dirty = false;
        self.rpc(
            inner,
            "register_device",
            json!({
                "device_id": self.device.id,
                "platform": platform_ok(platform()),
                "x25519_pub": keys::b64(&device.x_pub()),
                "ed25519_pub": keys::b64(&device.ed_pub()),
                "rebind_verifier": keys::b64(&device.rebind_verifier()),
                "pair_mac": keys::b64(&keys::pair_mac(&pk, &self.device.id, &device.x_pub(), &device.ed_pub())),
                "pair_blob": keys::b64(&blob),
            }),
        )?;
        inner.state.phase = Phase::Pending;
        inner.state.pending_since = Some(Utc::now());
        Ok(NextStep::Pair(self.pairing_view(inner).ok_or(SyncError::NoKeys)?))
    }

    pub(crate) fn pairing_view(&self, inner: &Inner) -> Option<PairingView> {
        let code = PairingCode::parse(inner.secrets.pairing.as_deref()?).ok()?;
        let device = inner.secrets.device_keys().ok()??;
        let fp = keys::fingerprint(&self.device.id, &device.x_pub(), &device.ed_pub());
        Some(PairingView {
            qr_svg: keys::qr_svg(&keys::pairing_uri(&self.device.id, &fp, &code)),
            words: code.words,
            expires_at: inner.state.pending_since.map(|t| (t + Duration::minutes(10)).to_rfc3339()),
        })
    }

    /// A pending device: a new code (the old request expired).
    pub fn new_pairing_code(&self) -> Result<PairingView, SyncError> {
        let mut inner = self.lock();
        if inner.state.phase != Phase::Pending {
            return Err(SyncError::User("This device isn't waiting to be added.".into()));
        }
        match self.register_pending(&mut inner)? {
            NextStep::Pair(v) => {
                self.persist(&mut inner)?;
                self.publish(&inner, false);
                Ok(v)
            }
            _ => Err(SyncError::NoKeys),
        }
    }

    /// A pending device: has it been approved? Opens its envelope when it
    /// has, and starts syncing.
    pub(crate) fn poll_pending(&self, inner: &mut Inner) -> Result<bool, SyncError> {
        let acct = self.rpc(inner, "account_state", json!({}))?;
        match acct["device"]["status"].as_str() {
            Some("approved") => {}
            Some("pending") | None => {
                if acct["device"].is_null() {
                    // Expired and purged, or never registered on this session.
                    self.register_pending(inner)?;
                }
                return Ok(false);
            }
            _ => return Ok(false),
        }
        let code = PairingCode::parse(inner.secrets.pairing.as_deref().ok_or(SyncError::NoKeys)?)?;
        let bind = keys::pair_bind(&code.key(&self.device.id)?);
        let keys_v = self.rpc(inner, "my_keys", json!({}))?;
        let device = self.device_keys(inner)?;
        let (version, amk) = keys_v["envelopes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|e| e["recipient"].as_str() == Some(self.device.id.as_str()))
            .filter_map(|e| {
                let v = e["amk_version"].as_u64()? as u32;
                let env = keys::unb64(e["envelope"].as_str()?).ok()?;
                keys::open_envelope(device.x_secret(), &env, &keys::amk_aad(v, &self.device.id), Some(&bind))
                    .ok()
                    .map(|k| (v, SymKey(k)))
            })
            .max_by_key(|(v, _)| *v)
            .ok_or_else(|| {
                SyncError::Integrity(
                    "The approval didn't open with this device's code. Ask to be added again.".into(),
                )
            })?;
        inner.secrets.keys = Some(AccountKeys { amk_version: version, amk: keys::b64(&amk.0), ..Default::default() });
        inner.secrets.pairing = None;
        inner.secrets_dirty = true;
        self.refresh_keys(inner)?;
        inner.state.phase = Phase::Active;
        state::audit(&self.paths, "joined", json!({"device": self.device.id}));
        Ok(true)
    }

    /// Fetch the space keys and the roster with the AMK this device has.
    pub(crate) fn refresh_keys(&self, inner: &mut Inner) -> Result<(), SyncError> {
        let device = self.device_keys(inner)?;
        let v = self.rpc(inner, "my_keys", json!({}))?;
        // A newer AMK after a rotation: open this device's envelope for it.
        let mut keys_now = inner.secrets.keys.clone().unwrap_or_default();
        for e in v["envelopes"].as_array().into_iter().flatten() {
            let ver = e["amk_version"].as_u64().unwrap_or_default() as u32;
            if e["recipient"].as_str() != Some(self.device.id.as_str()) || ver <= keys_now.amk_version && !keys_now.amk.is_empty() {
                continue;
            }
            let env = keys::unb64(e["envelope"].as_str().unwrap_or_default())?;
            if let Ok(k) = keys::open_envelope(device.x_secret(), &env, &keys::amk_aad(ver, &self.device.id), None) {
                keys_now.amk_version = ver;
                keys_now.amk = keys::b64(&k);
            }
        }
        let amk = keys_now.amk()?;
        let mut space_id = inner.state.space_id.clone();
        for k in v["space_keys"].as_array().into_iter().flatten() {
            let space = k["space_id"].as_str().unwrap_or_default().to_string();
            let ver = k["version"].as_u64().unwrap_or_default() as u32;
            let amk_ver = k["amk_version"].as_u64().unwrap_or_default() as u32;
            if amk_ver != keys_now.amk_version {
                continue;
            }
            let wrapped = keys::unb64(k["wrapped"].as_str().unwrap_or_default())?;
            let (sk, naming) = keys::unwrap_space_key(&amk, &space, ver, amk_ver, &wrapped)?;
            keys_now.space_keys.insert(ver, keys::b64(&sk.0));
            if !keys_now.naming.is_empty() && keys_now.naming != keys::b64(&naming.0) {
                return Err(SyncError::Integrity("the naming key changed".into()));
            }
            keys_now.naming = keys::b64(&naming.0);
            inner.state.key_version = inner.state.key_version.max(ver);
            space_id = Some(space);
        }
        if keys_now.space_keys.is_empty() {
            return Err(SyncError::NoKeys);
        }
        inner.state.amk_version = keys_now.amk_version;
        inner.state.space_id = space_id;
        inner.secrets.keys = Some(keys_now);
        inner.secrets_dirty = true;
        self.refresh_roster(inner)?;
        Ok(())
    }

    /// Fetch and check the roster; add this device if it isn't listed (a
    /// device that approved itself with the Recovery Kit).
    pub(crate) fn refresh_roster(&self, inner: &mut Inner) -> Result<(), SyncError> {
        let acct = self.rpc(inner, "get_account", json!({}))?;
        let version = acct["roster_version"].as_i64().unwrap_or_default();
        let amk = self.account_keys(inner)?.amk()?;
        let account = inner.state.user_id.clone().ok_or(SyncError::SignedOut)?;
        let sealed = keys::unb64(acct["roster"].as_str().unwrap_or_default())?;
        let roster = Roster::open(&sealed, &amk, &account, version)?;
        if acct["kit_pub"].as_str() != Some(roster.kit_pub.as_str()) {
            inner.state.warn("The server's Recovery Kit key doesn't match the signed device list; the signed one is used.");
        }
        inner.state.roster_version = version;
        for d in &roster.devices {
            inner.state.names.insert(d.id.clone(), d.name.clone());
            inner.state.names.insert(d.tag(), d.name.clone());
        }
        let listed = roster.active().any(|d| d.id == self.device.id);
        inner.roster = Some(roster);
        if !listed {
            let device = self.device_keys(inner)?;
            let entry = self.self_entry(inner, &device);
            self.update_roster(inner, |r| {
                r.devices.retain(|d| d.id != entry.id);
                r.devices.push(entry.clone());
            })?;
        }
        if inner.state.amk_version == 0 {
            inner.state.amk_version = acct["amk_version"].as_u64().unwrap_or(1) as u32;
        }
        Ok(())
    }

    /// Change the roster and store it (version + 1), signed by this device.
    fn update_roster(&self, inner: &mut Inner, change: impl Fn(&mut Roster)) -> Result<(), SyncError> {
        for _ in 0..3 {
            let mut roster = inner.roster.clone().ok_or(SyncError::NoKeys)?;
            change(&mut roster);
            let version = inner.state.roster_version + 1;
            let sealed = self.seal_roster(inner, &roster, version)?;
            match self.rpc(inner, "set_roster", json!({"roster": keys::b64(&sealed), "roster_version": version})) {
                Ok(_) => {
                    inner.state.roster_version = version;
                    inner.roster = Some(roster);
                    return Ok(());
                }
                Err(SyncError::Refused(c)) if c == "stale_roster" => self.refresh_roster(inner)?,
                Err(e) => return Err(e),
            }
        }
        Err(SyncError::Server("the device list kept changing".into()))
    }

    fn seal_roster(&self, inner: &Inner, roster: &Roster, version: i64) -> Result<Vec<u8>, SyncError> {
        let amk = self.account_keys(inner)?.amk()?;
        let account = inner.state.user_id.clone().ok_or(SyncError::SignedOut)?;
        roster.seal(&amk, &account, version, &self.device_keys(inner)?, &self.device.id)
    }

    /// On a device that syncs: find the pending device whose code was typed.
    /// Its keys are checked against the code (the HMAC), so a device the
    /// server made up, or keys it swapped, never match.
    pub fn check_pairing(&self, words: &str) -> Result<PendingView, SyncError> {
        let code = PairingCode::parse(words)?;
        let mut inner = self.lock();
        self.require_active(&inner)?;
        self.find_pending(&mut inner, &code).map(|(v, _, _)| v)
    }

    fn find_pending(&self, inner: &mut Inner, code: &PairingCode) -> Result<(PendingView, [u8; 32], [u8; 32]), SyncError> {
        let devices = self.rpc(inner, "list_devices", json!({}))?;
        let now = Utc::now();
        for d in devices.as_array().into_iter().flatten() {
            if d["status"] != "pending" {
                continue;
            }
            let expired = d["pending_expires_at"]
                .as_str()
                .and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok())
                .is_none_or(|t| t < now);
            if expired {
                continue;
            }
            let (Some(id), Some(x), Some(ed), Some(mac)) =
                (d["id"].as_str(), d["x25519_pub"].as_str(), d["ed25519_pub"].as_str(), d["pair_mac"].as_str())
            else {
                continue;
            };
            let (x, ed, mac) = (keys::unb64_32(x)?, keys::unb64_32(ed)?, keys::unb64_32(mac)?);
            let pk = code.key(id)?;
            if !keys::eq32(&keys::pair_mac(&pk, id, &x, &ed), &mac) {
                continue;
            }
            let info: Value = d["pair_blob"]
                .as_str()
                .and_then(|b| keys::unb64(b).ok())
                .and_then(|b| keys::unwrap(&keys::pair_blob_key(&pk), id.as_bytes(), &b).ok())
                .and_then(|b| serde_json::from_slice(&b).ok())
                .unwrap_or(Value::Null);
            return Ok((
                PendingView {
                    device_id: id.to_string(),
                    name: info["name"].as_str().unwrap_or("New device").chars().take(60).collect(),
                    platform: platform_ok(d["platform"].as_str().unwrap_or("other")).to_string(),
                },
                x,
                pk,
            ));
        }
        Err(SyncError::User(
            "No device is waiting with that code. Check the words on the new device; a code lasts 10 minutes.".into(),
        ))
    }

    /// Add the pending device whose code was typed: the AMK wrapped to its
    /// key with the code bound in, and the roster with it listed.
    pub fn approve_pairing(&self, words: &str) -> Result<PendingView, SyncError> {
        let code = PairingCode::parse(words)?;
        let mut inner = self.lock();
        self.require_active(&inner)?;
        let (view, x, pk) = self.find_pending(&mut inner, &code)?;
        let keys_now = self.account_keys(&inner)?.clone();
        let amk = keys_now.amk()?;
        let envelope = keys::seal_to(
            &x,
            &amk.0,
            &keys::amk_aad(keys_now.amk_version, &view.device_id),
            Some(&keys::pair_bind(&pk)),
        )?;
        let devices = self.rpc(&mut inner, "list_devices", json!({}))?;
        let ed = devices
            .as_array()
            .into_iter()
            .flatten()
            .find(|d| d["id"].as_str() == Some(view.device_id.as_str()))
            .and_then(|d| d["ed25519_pub"].as_str().map(String::from))
            .ok_or(SyncError::NoKeys)?;
        let entry = Entry {
            id: view.device_id.clone(),
            name: view.name.clone(),
            platform: view.platform.clone(),
            x25519_pub: keys::b64(&x),
            ed25519_pub: ed,
            added_at: Utc::now(),
            removed_at: None,
            last_ctr: None,
        };
        for _ in 0..3 {
            let mut roster = inner.roster.clone().ok_or(SyncError::NoKeys)?;
            roster.devices.retain(|d| d.id != entry.id);
            roster.devices.push(entry.clone());
            let version = inner.state.roster_version + 1;
            let sealed = self.seal_roster(&inner, &roster, version)?;
            let r = self.rpc(
                &mut inner,
                "approve_device",
                json!({
                    "device_id": view.device_id,
                    "envelope": keys::b64(&envelope),
                    "amk_version": keys_now.amk_version,
                    "roster": keys::b64(&sealed),
                    "roster_version": version,
                }),
            );
            match r {
                Ok(_) => {
                    inner.state.roster_version = version;
                    inner.state.names.insert(entry.id.clone(), entry.name.clone());
                    inner.state.names.insert(entry.tag(), entry.name.clone());
                    inner.roster = Some(roster);
                    state::audit(&self.paths, "device_approved", json!({"device": view.device_id}));
                    self.persist(&mut inner)?;
                    self.load_devices(&mut inner);
                    self.publish(&inner, false);
                    return Ok(view);
                }
                Err(SyncError::Refused(c)) if c == "stale_roster" => self.refresh_roster(&mut inner)?,
                Err(e) => return Err(e),
            }
        }
        Err(SyncError::Server("the device list kept changing".into()))
    }

    /// A pending device approves itself with the Recovery Kit.
    pub fn recover_with_kit(&self, words: &str) -> Result<(), SyncError> {
        let kit = Kit::from_words(words)?;
        let mut inner = self.lock();
        if inner.state.phase != Phase::Pending {
            return Err(SyncError::User("Sign in on this device first.".into()));
        }
        let v = self.rpc(&mut inner, "my_keys", json!({}))?;
        let (version, amk) = v["envelopes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|e| e["recipient"] == "kit")
            .filter_map(|e| {
                let ver = e["amk_version"].as_u64()? as u32;
                let env = keys::unb64(e["envelope"].as_str()?).ok()?;
                keys::open_envelope(&kit.x_secret(), &env, &keys::amk_aad(ver, "kit"), None).ok().map(|k| (ver, SymKey(k)))
            })
            .max_by_key(|(v, _)| *v)
            .ok_or_else(|| SyncError::User("This Recovery Kit doesn't open this account's keys.".into()))?;
        let device = self.device_keys(&inner)?;
        let envelope = keys::seal_to(&device.x_pub(), &amk.0, &keys::amk_aad(version, &self.device.id), None)?;
        self.rpc(
            &mut inner,
            "approve_with_kit",
            json!({"kit_token": keys::b64(&kit.token()), "envelope": keys::b64(&envelope), "amk_version": version}),
        )?;
        inner.secrets.keys = Some(AccountKeys { amk_version: version, amk: keys::b64(&amk.0), ..Default::default() });
        inner.secrets.pairing = None;
        inner.secrets_dirty = true;
        self.refresh_keys(&mut inner)?;
        if inner.roster.as_ref().is_some_and(|r| r.kit_pub != keys::b64(&kit.x_pub())) {
            inner.state.warn("The device list names a different Recovery Kit.");
        }
        inner.state.phase = Phase::Active;
        state::audit(&self.paths, "recovered_with_kit", json!({"device": self.device.id}));
        self.persist(&mut inner)?;
        self.publish(&inner, false);
        Ok(())
    }

    fn require_active(&self, inner: &Inner) -> Result<(), SyncError> {
        if inner.state.phase == Phase::Active { Ok(()) } else { Err(SyncError::User("Sync isn't on on this device.".into())) }
    }

    pub(crate) fn load_devices(&self, inner: &mut Inner) {
        if let Ok(v) = self.rpc(inner, "list_devices", json!({})) {
            inner.devices = v.as_array().cloned().unwrap_or_default();
        }
    }

    /// Remove a device (this one too). The server refuses it at once; the
    /// keys are rotated so it can't open anything new.
    pub fn remove_device(&self, device_id: &str) -> Result<(), SyncError> {
        let mut inner = self.lock();
        self.require_active(&inner)?;
        let last_ctr = inner.state.seen.values().filter(|s| s.dev == device_id).map(|s| s.ctr).max();
        self.rpc(&mut inner, "revoke_device", json!({"device_id": device_id}))?;
        state::audit(&self.paths, "device_removed", json!({"device": device_id}));
        if device_id == self.device.id {
            self.handle_revoked(&mut inner);
            self.persist(&mut inner)?;
            self.publish(&inner, false);
            return Ok(());
        }
        let now = Utc::now();
        let id = device_id.to_string();
        self.update_roster(&mut inner, |r| {
            if let Some(e) = r.devices.iter_mut().find(|d| d.id == id) {
                e.removed_at.get_or_insert(now);
                e.last_ctr = e.last_ctr.or(last_ctr).or(Some(0));
            }
        })?;
        self.rotate(&mut inner)?;
        self.load_devices(&mut inner);
        self.persist(&mut inner)?;
        self.publish(&inner, false);
        Ok(())
    }

    /// Rotate the keys after a removal: a new AMK for the remaining devices
    /// (as listed in the signed roster and approved on the server) and the
    /// Recovery Kit (its key from the roster), a new space key, every object
    /// key re-wrapped. No object is re-encrypted.
    pub(crate) fn rotate(&self, inner: &mut Inner) -> Result<(), SyncError> {
        for _ in 0..3 {
            match self.try_rotate(inner) {
                Err(SyncError::Refused(c)) if ["objects_changed", "stale_roster", "stale_amk"].contains(&c.as_str()) => {
                    self.refresh_keys(inner)?;
                }
                other => return other,
            }
        }
        Err(SyncError::Server("the keys couldn't be rotated; it will be tried again".into()))
    }

    fn try_rotate(&self, inner: &mut Inner) -> Result<(), SyncError> {
        self.refresh_roster(inner)?;
        let space = inner.state.space_id.clone().ok_or(SyncError::NoKeys)?;
        let old = self.account_keys(inner)?.clone();
        let roster = inner.roster.clone().ok_or(SyncError::NoKeys)?;
        let devices = self.rpc(inner, "list_devices", json!({}))?;
        let approved: Vec<String> = devices
            .as_array()
            .into_iter()
            .flatten()
            .filter(|d| d["status"] == "approved")
            .filter_map(|d| d["id"].as_str().map(String::from))
            .collect();
        let new_amk_v = old.amk_version + 1;
        let new_kv = inner.state.key_version + 1;
        let amk = SymKey::random()?;
        let sk = SymKey::random()?;
        let naming = old.naming()?;
        let mut envelopes = Vec::new();
        for id in &approved {
            // Only devices the signed roster lists, with the keys it lists.
            let entry = roster.active().find(|e| &e.id == id).ok_or_else(|| {
                SyncError::Integrity(format!("the server lists a device ({id}) the signed device list doesn't"))
            })?;
            let x = entry.x_pub().ok_or(SyncError::NoKeys)?;
            envelopes.push(json!({
                "device_id": id,
                "envelope": keys::b64(&keys::seal_to(&x, &amk.0, &keys::amk_aad(new_amk_v, id), None)?),
            }));
        }
        let kit_pub = keys::unb64_32(&roster.kit_pub)?;
        let kit_envelope = keys::seal_to(&kit_pub, &amk.0, &keys::amk_aad(new_amk_v, "kit"), None)?;
        let deks = self.rpc(inner, "dek_list", json!({"space_id": space}))?;
        let mut rewrapped = Vec::new();
        for d in deks.as_array().into_iter().flatten() {
            let id = d["id"].as_str().unwrap_or_default();
            let kv = d["key_version"].as_u64().unwrap_or_default() as u32;
            let wrapped = keys::unb64(d["wrapped_dek"].as_str().unwrap_or_default())?;
            let dek = keys::unwrap(&old.space_key(kv)?, &keys::dek_aad(&space, id, kv), &wrapped)?;
            rewrapped.push(json!({
                "id": id,
                "version": d["version"],
                "wrapped_dek": keys::b64(&keys::wrap(&sk, &keys::dek_aad(&space, id, new_kv), &dek)?),
            }));
        }
        let roster_v = inner.state.roster_version + 1;
        // The roster is sealed under the new AMK.
        let account = inner.state.user_id.clone().ok_or(SyncError::SignedOut)?;
        let sealed_roster = roster.seal(&amk, &account, roster_v, &self.device_keys(inner)?, &self.device.id)?;
        let new_keys = AccountKeys {
            amk_version: new_amk_v,
            amk: keys::b64(&amk.0),
            space_keys: [(new_kv, keys::b64(&sk.0))].into(),
            naming: old.naming.clone(),
        };
        self.rpc(
            inner,
            "rotate",
            json!({
                "amk_version": new_amk_v,
                "envelopes": envelopes,
                "kit_envelope": keys::b64(&kit_envelope),
                "space_id": space,
                "key_version": new_kv,
                "space_key": keys::b64(&keys::wrap_space_key(&amk, &space, new_kv, new_amk_v, &sk, &naming)?),
                "roster": keys::b64(&sealed_roster),
                "roster_version": roster_v,
                "rewrapped": rewrapped,
            }),
        )?;
        inner.secrets.keys = Some(new_keys);
        inner.secrets_dirty = true;
        inner.state.amk_version = new_amk_v;
        inner.state.key_version = new_kv;
        inner.state.roster_version = roster_v;
        // Anything queued under the old space key is sealed again.
        inner.state.outbox.clear();
        state::audit(&self.paths, "keys_rotated", json!({"amk_version": new_amk_v, "key_version": new_kv}));
        self.persist(inner)?;
        Ok(())
    }

    /// This device was removed: erase the synced copies it received (visits
    /// that came from other devices) and its sync keys. Its own visits, and
    /// their recordings, stay.
    pub(crate) fn handle_revoked(&self, inner: &mut Inner) {
        let adopted: Vec<String> =
            inner.state.visits.iter().filter(|(_, v)| v.adopted).map(|(id, _)| id.clone()).collect();
        let mut erased = 0;
        for s in os_engine::session::list(&self.root) {
            if adopted.contains(&s.meta.id) && std::fs::remove_dir_all(&s.dir).is_ok() {
                erased += 1;
            }
        }
        if let Err(e) = self.store.erase() {
            tracing::warn!(error = %e, "couldn't erase the sync keys");
        }
        inner.secrets = state::Secrets::default();
        inner.session = None;
        inner.roster = None;
        inner.devices.clear();
        let server = inner.state.server.clone();
        let email = inner.state.email.clone();
        inner.state = State { phase: Phase::Revoked, server, email, ..Default::default() };
        state::audit(&self.paths, "revoked_here", json!({"erased_visits": erased}));
        let _ = state::save(&self.paths, &inner.state);
    }

    /// Turn off sync on this device: sign out and stop. The device keys and
    /// what was synced stay, so turning it on again needs no approval.
    pub fn turn_off(&self) -> Result<(), SyncError> {
        let mut inner = self.lock();
        if let Some(s) = inner.session.take() {
            let _ = self.client(&inner.state).sign_out(&s.access_token);
        }
        inner.secrets.refresh_token = None;
        inner.secrets.pairing = None;
        inner.secrets_dirty = true;
        inner.state.phase = if inner.state.phase == Phase::Revoked { Phase::Revoked } else { Phase::Off };
        state::audit(&self.paths, "turned_off", json!({}));
        self.persist(&mut inner)?;
        self.publish(&inner, false);
        Ok(())
    }

    /// Delete the account on the server: every row, object and key envelope.
    /// Local visits on every device are untouched. `confirm`: the account's
    /// email address.
    pub fn delete_account(&self, confirm: &str) -> Result<(), SyncError> {
        let mut inner = self.lock();
        let email = inner.state.email.clone().unwrap_or_default();
        if confirm.trim().to_ascii_lowercase() != email {
            return Err(SyncError::User("Type your account's email address to confirm.".into()));
        }
        let access = self.access(&mut inner)?;
        self.client(&inner.state).delete_account(&access, confirm.trim())?;
        let _ = self.store.erase();
        let server = inner.state.server.clone();
        inner.secrets = state::Secrets::default();
        inner.session = None;
        inner.roster = None;
        inner.devices.clear();
        inner.state = State { server, ..Default::default() };
        state::audit(&self.paths, "account_deleted", json!({}));
        self.persist(&mut inner)?;
        self.publish(&inner, false);
        Ok(())
    }
}

/// A name for this computer until the person gives one.
fn default_device_name() -> String {
    let host = std::env::var("COMPUTERNAME").or_else(|_| std::env::var("HOSTNAME")).unwrap_or_default();
    let host = host.trim();
    if host.is_empty() { "This computer".into() } else { host.chars().take(60).collect() }
}
