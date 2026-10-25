//! The hub's calls: the 28 SQL functions of
//! `supabase/migrations/20261020120000_sync.sql` (and the delete-account
//! Edge Function) ported to Rust, function by function. Each one checks its
//! caller as the SQL did; `auth.uid()` and the session become the signed
//! request's device and the account it belongs to. The table is in
//! docs/security/hub-authorization.md.
//!
//! The caller has already been checked by `Hub::handle`: an allowed client
//! address, a valid signature by the device's registered key (or, for the
//! two calls that create a device, by the key in the body), a fresh nonce,
//! within the rate limits, and not a removed device (except the two calls
//! that tell a removed device it was removed).
//!
//! Binary values travel as base64 both ways, as with PostgREST.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use os_sync::keys;
use serde_json::{Value, json};

use crate::limits::Guard;
use crate::store::{Account, BlobRec, CodeRec, Device, Envelope, Obj, Space, SpaceKey, Store, blob_key};

/// A refusal: an HTTP status and an `openbark:<code>` message, as the SQL's
/// `sync.fail()` raised.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    pub status: u16,
    pub code: String,
}

pub type Res<T> = Result<T, Refusal>;

pub fn fail(code: &str) -> Refusal {
    Refusal { status: 400, code: code.into() }
}

pub fn deny(status: u16, code: &str) -> Refusal {
    Refusal { status, code: code.into() }
}

/// Calls that change nothing worth writing to disk at once.
pub const READ_ONLY: [&str; 9] = [
    "account_state",
    "device_status",
    "get_account",
    "dek_list",
    "pull",
    "get_blobs",
    "my_keys",
    "list_devices",
    "find_pending",
];

/// Calls a device not yet on the hub may make (signed by the key in the body).
pub const BOOTSTRAP: [&str; 2] = ["create_account", "register_device"];
/// Calls a removed device may still make (to learn it was removed).
pub const REVOKED_OK: [&str; 2] = ["device_status", "rebind_device"];

const PLATFORMS: [&str; 7] = ["windows", "macos", "linux", "ios", "android", "web", "other"];
const KINDS: [&str; 7] = ["visit", "run", "event", "checks", "transcript", "patient", "tombstone"];
/// Pending devices waiting in the lobby at once (the SQL allowed 5 per account).
const MAX_LOBBY: usize = 50;
const PENDING_MINUTES: i64 = 10;

/// What a call works with.
pub struct Ctx<'a> {
    pub st: &'a mut Store,
    /// The calling device's id.
    pub me: String,
    pub now: DateTime<Utc>,
    /// Bytes each account may hold.
    pub quota: u64,
    pub guard: &'a Mutex<Guard>,
    /// The client address's key for join-code lockouts.
    pub peer: String,
}

// ---- arguments -----------------------------------------------------------------

fn s<'v>(a: &'v Value, k: &str) -> Option<&'v str> {
    a.get(k).and_then(Value::as_str)
}

fn uuid_arg(a: &Value, k: &str) -> Res<String> {
    s(a, k).and_then(canonical_uuid).ok_or_else(|| fail("bad_value"))
}

pub fn canonical_uuid(t: &str) -> Option<String> {
    uuid::Uuid::parse_str(t.trim()).ok().map(|u| u.hyphenated().to_string())
}

fn int(a: &Value, k: &str) -> Res<i64> {
    a.get(k).and_then(Value::as_i64).ok_or_else(|| fail("bad_value"))
}

fn bytes(a: &Value, k: &str, max: usize) -> Res<Option<Vec<u8>>> {
    match a.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(t)) => {
            let b = keys::unb64(t).map_err(|_| fail("bad_base64"))?;
            if b.len() > max { Err(fail("too_large")) } else { Ok(Some(b)) }
        }
        Some(_) => Err(fail("bad_value")),
    }
}

fn req_bytes(a: &Value, k: &str, max: usize) -> Res<Vec<u8>> {
    bytes(a, k, max)?.ok_or_else(|| fail("bad_value"))
}

/// Exactly 32 bytes (public keys, verifiers: the SQL's octet_length checks).
fn key32(a: &Value, k: &str) -> Res<Vec<u8>> {
    let b = req_bytes(a, k, 64)?;
    if b.len() == 32 { Ok(b) } else { Err(fail("bad_value")) }
}

/// Sealed bytes with their magic (the SQL's substring checks).
fn sealed(a: &Value, k: &str, magic: &[u8; 4], max: usize) -> Res<Vec<u8>> {
    let b = req_bytes(a, k, max)?;
    if b.starts_with(magic) { Ok(b) } else { Err(fail("bad_value")) }
}

fn sealed_val(v: Option<&Value>, magic: &[u8; 4], max: usize) -> Res<Vec<u8>> {
    let t = v.and_then(Value::as_str).ok_or_else(|| fail("bad_value"))?;
    let b = keys::unb64(t).map_err(|_| fail("bad_base64"))?;
    if b.len() > max {
        return Err(fail("too_large"));
    }
    if b.starts_with(magic) { Ok(b) } else { Err(fail("bad_value")) }
}

fn enc(b: &[u8]) -> String {
    keys::b64(b)
}

fn ts(t: &DateTime<Utc>) -> String {
    t.to_rfc3339()
}

fn platform(a: &Value) -> Res<String> {
    let p = s(a, "platform").ok_or_else(|| fail("bad_value"))?;
    if PLATFORMS.contains(&p) { Ok(p.to_string()) } else { Err(fail("bad_value")) }
}

fn eq_b64(stored: &str, given: &[u8]) -> bool {
    keys::unb64(stored).is_ok_and(|b| os_sync::hub::ct_eq(&b, given))
}

// ---- who is asking --------------------------------------------------------------

impl Ctx<'_> {
    fn device(&self) -> Res<&Device> {
        self.st.ix.devices.get(&self.me).ok_or_else(|| deny(401, "unknown_device"))
    }

    /// The SQL's `require_device()`: an approved device, and its account.
    fn require_device(&self) -> Res<String> {
        let d = self.device()?;
        match (&d.account, d.status.as_str()) {
            (Some(acct), "approved") => Ok(acct.clone()),
            _ => Err(deny(403, "not_approved")),
        }
    }

    /// The account's space with this id (the SQL's space_members check).
    fn member_space(&self, account: &str, space: &str) -> Res<()> {
        match self.st.ix.spaces.get(space) {
            Some(sp) if sp.owner == account => Ok(()),
            _ => Err(fail("unknown_space")),
        }
    }

    /// Redeem a hub join code (ADR-0029 section 3): single use, unexpired,
    /// compared as keyed hashes against every live code. Ten wrong codes
    /// from one address lock it out for an hour.
    fn redeem(&mut self, code: Option<&str>) -> Res<()> {
        let now = self.now.timestamp();
        let mut g = self.guard.lock().unwrap_or_else(|p| p.into_inner());
        if g.locked(&self.peer, now) {
            return Err(deny(403, "locked"));
        }
        let code = code.map(os_sync::hub::normalise_code).filter(|c| !c.is_empty());
        let Some(code) = code else { return Err(fail("join_code_required")) };
        let h = crate::code_hash(&self.st.ix.code_key, &code);
        let mut hit = None;
        for (i, c) in self.st.ix.codes.iter().enumerate() {
            let same = os_sync::hub::ct_eq(c.hash.as_bytes(), h.as_bytes());
            if same && hit.is_none() {
                hit = Some(i);
            }
        }
        match hit {
            Some(i) if self.st.ix.codes[i].used_at.is_none() && self.st.ix.codes[i].expires_at > self.now => {
                self.st.ix.codes[i].used_at = Some(self.now);
                g.clear(&self.peer);
                Ok(())
            }
            _ => {
                g.fail(&self.peer, now);
                Err(fail("bad_join_code"))
            }
        }
    }
}

/// Add a join code's record (the hub's own screen asks for it).
pub fn add_code(st: &mut Store, code: &str, now: DateTime<Utc>, ttl: chrono::Duration) {
    let hash = crate::code_hash(&st.ix.code_key, code);
    st.ix.codes.push(CodeRec { hash, created_at: now, expires_at: now + ttl, used_at: None });
}

pub fn dispatch(cx: &mut Ctx, name: &str, a: &Value) -> Res<Value> {
    match name {
        "account_state" => account_state(cx),
        "create_account" => create_account(cx, a),
        "register_device" => register_device(cx, a),
        "rebind_device" => rebind_device(cx, a),
        "device_status" => device_status(cx, a),
        "approve_device" => approve_device(cx, a),
        "find_pending" => find_pending(cx, a),
        "approve_with_kit" => approve_with_kit(cx, a),
        "revoke_device" => revoke_device(cx, a),
        "set_roster" => set_roster(cx, a),
        "get_account" => get_account(cx),
        "rotate" => rotate(cx, a),
        "dek_list" => dek_list(cx, a),
        "push" => push(cx, a),
        "put_blob" => put_blob(cx, a),
        "pull" => pull(cx, a),
        "get_blobs" => get_blobs(cx, a),
        "my_keys" => my_keys(cx, a),
        "list_devices" => list_devices(cx),
        "delete_account" => delete_account(cx),
        _ => Err(deny(404, "unknown_call")),
    }
}

// ---- account and devices -------------------------------------------------------------

/// What this device is: whether it belongs to an account, its status, and
/// (approved only) the account's versions and space. Touches last_seen_at.
pub fn account_state(cx: &mut Ctx) -> Res<Value> {
    let now = cx.now;
    let dev = cx.st.ix.devices.get_mut(&cx.me).ok_or_else(|| deny(401, "unknown_device"))?;
    let approved = dev.status == "approved";
    if approved {
        dev.last_seen_at = Some(now);
    }
    let status = dev.status.clone();
    let acct_id = dev.account.clone();
    let acct = acct_id.as_ref().and_then(|a| cx.st.ix.accounts.get(a));
    let space = if approved {
        acct_id
            .as_ref()
            .and_then(|a| cx.st.space_of(a))
            .map(|(id, sp)| json!({"id": id, "key_version": sp.key_version, "seq": sp.seq}))
    } else {
        None
    };
    Ok(json!({
        "account": acct.is_some(),
        "account_id": if approved { acct_id.clone() } else { None },
        "device": {"id": cx.me, "status": status},
        "amk_version": if approved { acct.map(|a| a.amk_version) } else { None },
        "roster_version": if approved { acct.map(|a| a.roster_version) } else { None },
        "rotate_needed": if approved { acct.map(|a| a.rotate_needed) } else { None },
        "space": space,
    }))
}

/// The first device of a person: the account (its id made on the device),
/// its space and first space key, this device (approved), and the AMK
/// wrapped to this device and to the Recovery Kit. Needs a hub join code.
fn create_account(cx: &mut Ctx, a: &Value) -> Res<Value> {
    let account = uuid_arg(a, "account_id")?;
    let device_id = uuid_arg(a, "device_id")?;
    if device_id != cx.me {
        return Err(fail("bad_value"));
    }
    let platform = platform(a)?;
    let x = key32(a, "x25519_pub")?;
    let ed = key32(a, "ed25519_pub")?;
    let rebind = key32(a, "rebind_verifier")?;
    let kit_pub = key32(a, "kit_pub")?;
    let kit_verifier = key32(a, "kit_verifier")?;
    let space = uuid_arg(a, "space_id")?;
    let dev_env = sealed(a, "device_envelope", keys::ENVELOPE_MAGIC, 512)?;
    let kit_env = sealed(a, "kit_envelope", keys::ENVELOPE_MAGIC, 512)?;
    let space_key = sealed(a, "space_key", keys::WRAP_MAGIC, 512)?;
    let roster = sealed(a, "roster", b"OBS1", 65536)?;
    if cx.st.ix.accounts.contains_key(&account) {
        return Err(fail("account_exists"));
    }
    if cx.st.ix.devices.contains_key(&device_id) {
        return Err(fail("device_exists"));
    }
    if cx.st.ix.spaces.contains_key(&space) {
        return Err(fail("space_exists"));
    }
    // Everything checks out: only now is the code used up.
    cx.redeem(s(a, "join_code"))?;
    let now = cx.now;
    cx.st.ix.accounts.insert(
        account.clone(),
        Account {
            amk_version: 1,
            kit_pub: enc(&kit_pub),
            kit_verifier: enc(&kit_verifier),
            roster: enc(&roster),
            roster_version: 1,
            rotate_needed: false,
            created_at: now,
            updated_at: now,
        },
    );
    cx.st.ix.devices.insert(
        device_id.clone(),
        Device {
            account: Some(account.clone()),
            status: "approved".into(),
            platform,
            x25519_pub: enc(&x),
            ed25519_pub: enc(&ed),
            rebind_verifier: enc(&rebind),
            pair_mac: None,
            pair_blob: None,
            pair_tag: None,
            pending_expires_at: None,
            created_at: now,
            approved_at: Some(now),
            revoked_at: None,
            last_seen_at: Some(now),
        },
    );
    for (recipient, dev, env) in
        [(device_id.clone(), Some(device_id.clone()), dev_env), ("kit".into(), None, kit_env)]
    {
        cx.st.ix.envelopes.push(Envelope {
            account: account.clone(),
            recipient,
            device_id: dev,
            amk_version: 1,
            envelope: enc(&env),
            created_at: now,
        });
    }
    cx.st.ix.spaces.insert(space.clone(), Space { owner: account, key_version: 1, seq: 0, created_at: now });
    cx.st.ix.space_keys.push(SpaceKey {
        space,
        version: 1,
        amk_version: 1,
        wrapped: enc(&space_key),
        created_at: now,
    });
    account_state(cx)
}

/// A person's next device asks to join: pending for 10 minutes in the hub's
/// lobby, filed under a tag derived from its 6 words. A new device needs a
/// hub join code; a pending one asking again (a new code on its screen)
/// doesn't, and can't change its keys.
fn register_device(cx: &mut Ctx, a: &Value) -> Res<Value> {
    let device_id = uuid_arg(a, "device_id")?;
    if device_id != cx.me {
        return Err(fail("bad_value"));
    }
    let platform = platform(a)?;
    let x = key32(a, "x25519_pub")?;
    let ed = key32(a, "ed25519_pub")?;
    let rebind = key32(a, "rebind_verifier")?;
    let pair_mac = key32(a, "pair_mac")?;
    let pair_blob = req_bytes(a, "pair_blob", 1024)?;
    let pair_tag = key32(a, "pair_tag")?;
    let now = cx.now;
    let expires = now + chrono::Duration::minutes(PENDING_MINUTES);
    if let Some(existing) = cx.st.ix.devices.get(&device_id) {
        if existing.status != "pending" || existing.account.is_some() {
            return Err(fail(if existing.status == "revoked" { "revoked" } else { "device_exists" }));
        }
        if !eq_b64(&existing.ed25519_pub, &ed) || !eq_b64(&existing.x25519_pub, &x) {
            return Err(fail("keys_changed"));
        }
        let d = cx.st.ix.devices.get_mut(&device_id).expect("checked above");
        d.platform = platform;
        d.rebind_verifier = enc(&rebind);
        d.pair_mac = Some(enc(&pair_mac));
        d.pair_blob = Some(enc(&pair_blob));
        d.pair_tag = Some(enc(&pair_tag));
        d.pending_expires_at = Some(expires);
        return account_state(cx);
    }
    let waiting = cx
        .st
        .ix
        .devices
        .values()
        .filter(|d| d.status == "pending" && d.pending_expires_at.is_some_and(|t| t > now))
        .count();
    if waiting >= MAX_LOBBY {
        return Err(fail("too_many_pending"));
    }
    cx.redeem(s(a, "join_code"))?;
    cx.st.ix.devices.insert(
        device_id,
        Device {
            account: None,
            status: "pending".into(),
            platform,
            x25519_pub: enc(&x),
            ed25519_pub: enc(&ed),
            rebind_verifier: enc(&rebind),
            pair_mac: Some(enc(&pair_mac)),
            pair_blob: Some(enc(&pair_blob)),
            pair_tag: Some(enc(&pair_tag)),
            pending_expires_at: Some(expires),
            created_at: now,
            approved_at: None,
            revoked_at: None,
            last_seen_at: None,
        },
    );
    account_state(cx)
}

/// Supabase bound a device to a new sign-in session here. The hub has no
/// sessions (every request is signed), so this only proves the device's own
/// secret and says whether it was removed.
fn rebind_device(cx: &mut Ctx, a: &Value) -> Res<Value> {
    let device_id = uuid_arg(a, "device_id")?;
    let secret = req_bytes(a, "rebind_secret", 64)?;
    if device_id != cx.me {
        return Err(fail("unknown_device"));
    }
    let d = cx.device()?;
    if !eq_b64(&d.rebind_verifier, &keys::sha256(&[&secret])) {
        return Err(fail("unknown_device"));
    }
    if d.status == "revoked" {
        return Ok(json!({"device": {"id": device_id, "status": "revoked"}}));
    }
    account_state(cx)
}

/// Was this device removed? Proven by the device's own secret; says nothing else.
fn device_status(cx: &mut Ctx, a: &Value) -> Res<Value> {
    let device_id = uuid_arg(a, "device_id")?;
    let secret = req_bytes(a, "rebind_secret", 64)?;
    match cx.st.ix.devices.get(&device_id) {
        Some(d) if eq_b64(&d.rebind_verifier, &keys::sha256(&[&secret])) => Ok(json!(d.status)),
        _ => Ok(json!("unknown")),
    }
}

fn device_row(id: &str, d: &Device) -> Value {
    json!({
        "id": id, "status": d.status, "platform": d.platform,
        "x25519_pub": d.x25519_pub, "ed25519_pub": d.ed25519_pub,
        "pair_mac": d.pair_mac, "pair_blob": d.pair_blob,
        "pending_expires_at": d.pending_expires_at.as_ref().map(ts),
        "created_at": ts(&d.created_at),
        "approved_at": d.approved_at.as_ref().map(ts),
        "revoked_at": d.revoked_at.as_ref().map(ts),
        "last_seen_at": d.last_seen_at.as_ref().map(ts),
    })
}

/// An approving device looks up the pending device whose 6 words it was
/// given, by the tag derived from them. Only a device that saw the words can
/// find it; the pairing HMAC is then checked on the approving device.
fn find_pending(cx: &mut Ctx, a: &Value) -> Res<Value> {
    cx.require_device()?;
    let tag = key32(a, "pair_tag")?;
    let now = cx.now;
    let rows: Vec<Value> = cx
        .st
        .ix
        .devices
        .iter()
        .filter(|(_, d)| d.status == "pending" && d.account.is_none())
        .filter(|(_, d)| d.pending_expires_at.is_some_and(|t| t > now))
        .filter(|(_, d)| d.pair_tag.as_deref().is_some_and(|t| eq_b64(t, &tag)))
        .map(|(id, d)| device_row(id, d))
        .collect();
    Ok(Value::Array(rows))
}

/// An approved device approves a pending one: the AMK wrapped to it, and the
/// roster with it added (both made on the approving device). The pending
/// device joins the approver's account. The tag proves the approver had the
/// words.
fn approve_device(cx: &mut Ctx, a: &Value) -> Res<Value> {
    let account = cx.require_device()?;
    let target_id = uuid_arg(a, "device_id")?;
    let envelope = sealed(a, "envelope", keys::ENVELOPE_MAGIC, 512)?;
    let amk_version = int(a, "amk_version")?;
    let roster = sealed(a, "roster", b"OBS1", 65536)?;
    let roster_version = int(a, "roster_version")?;
    let tag = key32(a, "pair_tag")?;
    let acct = cx.st.ix.accounts.get(&account).ok_or_else(|| fail("no_account"))?.clone();
    let target = cx.st.ix.devices.get(&target_id).ok_or_else(|| fail("not_pending"))?;
    let tag_ok = target.pair_tag.as_deref().is_some_and(|t| eq_b64(t, &tag));
    if target.status != "pending" || target.account.as_deref().is_some_and(|x| x != account) || !tag_ok {
        return Err(fail("not_pending"));
    }
    if target.pending_expires_at.is_none_or(|t| t < cx.now) {
        return Err(fail("expired"));
    }
    if amk_version != acct.amk_version {
        return Err(fail("stale_amk"));
    }
    if roster_version != acct.roster_version + 1 {
        return Err(fail("stale_roster"));
    }
    let now = cx.now;
    upsert_envelope(cx.st, &account, &target_id, Some(&target_id), acct.amk_version, &envelope, now);
    let d = cx.st.ix.devices.get_mut(&target_id).expect("checked above");
    d.account = Some(account.clone());
    d.status = "approved".into();
    d.approved_at = Some(now);
    d.pair_mac = None;
    d.pair_blob = None;
    d.pair_tag = None;
    d.pending_expires_at = None;
    let ac = cx.st.ix.accounts.get_mut(&account).expect("checked above");
    ac.roster = enc(&roster);
    ac.roster_version = roster_version;
    ac.updated_at = now;
    Ok(json!({"approved": target_id}))
}

fn upsert_envelope(
    st: &mut Store,
    account: &str,
    recipient: &str,
    device: Option<&str>,
    amk_version: i64,
    envelope: &[u8],
    now: DateTime<Utc>,
) {
    st.ix
        .envelopes
        .retain(|e| e.account != account || e.recipient != recipient || e.amk_version != amk_version);
    st.ix.envelopes.push(Envelope {
        account: account.into(),
        recipient: recipient.into(),
        device_id: device.map(String::from),
        amk_version,
        envelope: enc(envelope),
        created_at: now,
    });
}

/// The account whose kit verifier is SHA-256 of `token` (compared with
/// every account, without stopping early).
fn account_for_kit(st: &Store, token: &[u8]) -> Option<String> {
    let want = keys::sha256(&[token]);
    let mut found = None;
    for (id, acct) in &st.ix.accounts {
        if eq_b64(&acct.kit_verifier, &want) && found.is_none() {
            found = Some(id.clone());
        }
    }
    found
}

/// A pending device that holds the Recovery Kit approves itself: the kit
/// token proves it (and names the account: the one whose kit verifier is its
/// SHA-256). The device opened the kit's envelope itself and wrapped the AMK
/// to its own key; it adds itself to the roster straight after.
fn approve_with_kit(cx: &mut Ctx, a: &Value) -> Res<Value> {
    let token = req_bytes(a, "kit_token", 64)?;
    let envelope = sealed(a, "envelope", keys::ENVELOPE_MAGIC, 512)?;
    let amk_version = int(a, "amk_version")?;
    let me = cx.device()?.clone();
    if me.status != "pending" {
        return Err(fail("not_pending"));
    }
    let account = account_for_kit(cx.st, &token).ok_or_else(|| fail("wrong_kit"))?;
    if me.account.as_deref().is_some_and(|x| x != account) {
        return Err(fail("wrong_kit"));
    }
    let acct = cx.st.ix.accounts.get(&account).expect("found above").clone();
    if amk_version != acct.amk_version {
        return Err(fail("stale_amk"));
    }
    let now = cx.now;
    let me_id = cx.me.clone();
    upsert_envelope(cx.st, &account, &me_id, Some(&me_id), acct.amk_version, &envelope, now);
    let d = cx.st.ix.devices.get_mut(&me_id).expect("checked above");
    d.account = Some(account);
    d.status = "approved".into();
    d.approved_at = Some(now);
    d.last_seen_at = Some(now);
    d.pair_mac = None;
    d.pair_blob = None;
    d.pair_tag = None;
    d.pending_expires_at = None;
    account_state(cx)
}

/// The shared part of removing a device: refused at once, its envelopes
/// gone, and the keys marked for rotation (the next approved device to sync
/// rotates them). `account` None: a device still in the lobby.
pub fn revoke(st: &mut Store, account: Option<&str>, target: &str, now: DateTime<Utc>) -> Res<Value> {
    let d = st.ix.devices.get_mut(target).ok_or_else(|| fail("unknown_device"))?;
    if d.account.as_deref() != account {
        return Err(fail("unknown_device"));
    }
    if d.status == "revoked" {
        return Ok(json!({"revoked": target}));
    }
    let was_approved = d.status == "approved";
    d.status = "revoked".into();
    d.revoked_at = Some(now);
    d.pair_mac = None;
    d.pair_blob = None;
    d.pair_tag = None;
    if let Some(acct) = account {
        st.ix.envelopes.retain(|e| e.account != acct || e.device_id.as_deref() != Some(target));
        if was_approved && let Some(ac) = st.ix.accounts.get_mut(acct) {
            ac.rotate_needed = true;
            ac.updated_at = now;
        }
    }
    Ok(json!({"revoked": target}))
}

/// From an approved device (it may remove itself).
fn revoke_device(cx: &mut Ctx, a: &Value) -> Res<Value> {
    let account = cx.require_device()?;
    let target = uuid_arg(a, "device_id")?;
    revoke(cx.st, Some(&account), &target, cx.now)
}

/// Replace the roster (a removed device marked, a name changed).
fn set_roster(cx: &mut Ctx, a: &Value) -> Res<Value> {
    let account = cx.require_device()?;
    let roster = sealed(a, "roster", b"OBS1", 65536)?;
    let version = int(a, "roster_version")?;
    let now = cx.now;
    let ac = cx.st.ix.accounts.get_mut(&account).ok_or_else(|| fail("no_account"))?;
    if version != ac.roster_version + 1 {
        return Err(fail("stale_roster"));
    }
    ac.roster = enc(&roster);
    ac.roster_version = version;
    ac.updated_at = now;
    Ok(json!({"roster_version": version}))
}

/// The roster and this account's kit public key (approved devices).
fn get_account(cx: &mut Ctx) -> Res<Value> {
    let account = cx.require_device()?;
    let ac = cx.st.ix.accounts.get(&account).ok_or_else(|| fail("no_account"))?;
    Ok(json!({
        "amk_version": ac.amk_version, "kit_pub": ac.kit_pub, "roster": ac.roster,
        "roster_version": ac.roster_version, "rotate_needed": ac.rotate_needed,
    }))
}

/// Key rotation after a device was removed: a new AMK wrapped to every
/// remaining approved device and to the kit, a new space key version, every
/// live object's DEK re-wrapped (no object is re-encrypted), and the roster.
/// All or nothing; refused if the devices or objects changed meanwhile.
fn rotate(cx: &mut Ctx, a: &Value) -> Res<Value> {
    let account = cx.require_device()?;
    let amk_version = int(a, "amk_version")?;
    let key_version = int(a, "key_version")?;
    let roster_version = int(a, "roster_version")?;
    let space = uuid_arg(a, "space_id")?;
    let kit_env = sealed(a, "kit_envelope", keys::ENVELOPE_MAGIC, 512)?;
    let space_key = sealed(a, "space_key", keys::WRAP_MAGIC, 512)?;
    let roster = sealed(a, "roster", b"OBS1", 65536)?;
    let acct = cx.st.ix.accounts.get(&account).ok_or_else(|| fail("no_account"))?.clone();
    let sp = match cx.st.ix.spaces.get(&space) {
        Some(sp) if sp.owner == account => sp.clone(),
        _ => return Err(fail("unknown_space")),
    };
    if amk_version != acct.amk_version + 1 || key_version != sp.key_version + 1 {
        return Err(fail("stale_amk"));
    }
    if roster_version != acct.roster_version + 1 {
        return Err(fail("stale_roster"));
    }
    // Exactly the approved devices, each once.
    let mut envelopes: Vec<(String, Vec<u8>)> = Vec::new();
    for e in a.get("envelopes").and_then(Value::as_array).ok_or_else(|| fail("bad_value"))? {
        let id = e
            .get("device_id")
            .and_then(Value::as_str)
            .and_then(canonical_uuid)
            .ok_or_else(|| fail("bad_value"))?;
        envelopes.push((id, sealed_val(e.get("envelope"), keys::ENVELOPE_MAGIC, 512)?));
    }
    let approved: BTreeSet<&String> = cx
        .st
        .ix
        .devices
        .iter()
        .filter(|(_, d)| d.status == "approved" && d.account.as_deref() == Some(account.as_str()))
        .map(|(id, _)| id)
        .collect();
    let given: BTreeSet<&String> = envelopes.iter().map(|(id, _)| id).collect();
    if approved != given || given.len() != envelopes.len() {
        return Err(fail("devices_changed"));
    }
    // Every live object re-wrapped at its current version, each once.
    let live: BTreeMap<&String, &Obj> = cx
        .st
        .ix
        .objects
        .get(&space)
        .map(|m| m.iter().filter(|(_, o)| !o.deleted).collect())
        .unwrap_or_default();
    let mut rewrapped: Vec<(String, Vec<u8>)> = Vec::new();
    for r in a.get("rewrapped").and_then(Value::as_array).ok_or_else(|| fail("bad_value"))? {
        let id =
            r.get("id").and_then(Value::as_str).and_then(canonical_uuid).ok_or_else(|| fail("bad_value"))?;
        let version = r.get("version").and_then(Value::as_i64).ok_or_else(|| fail("bad_value"))?;
        let w = sealed_val(r.get("wrapped_dek"), keys::WRAP_MAGIC, 256)?;
        if live.get(&id).is_none_or(|o| o.version != version) {
            return Err(fail("objects_changed"));
        }
        rewrapped.push((id, w));
    }
    let ids: BTreeSet<&String> = rewrapped.iter().map(|(id, _)| id).collect();
    if ids.len() != rewrapped.len() || ids.len() != live.len() {
        return Err(fail("objects_changed"));
    }
    let now = cx.now;
    for (id, env) in &envelopes {
        upsert_envelope(cx.st, &account, id, Some(id), amk_version, env, now);
    }
    upsert_envelope(cx.st, &account, "kit", None, amk_version, &kit_env, now);
    cx.st.ix.envelopes.retain(|e| e.account != account || e.amk_version >= amk_version);
    cx.st.ix.space_keys.retain(|k| k.space != space || k.version < key_version);
    cx.st.ix.space_keys.push(SpaceKey {
        space: space.clone(),
        version: key_version,
        amk_version,
        wrapped: enc(&space_key),
        created_at: now,
    });
    cx.st.ix.space_keys.retain(|k| k.space != space || k.version >= key_version);
    if let Some(objs) = cx.st.ix.objects.get_mut(&space) {
        for (id, w) in &rewrapped {
            if let Some(o) = objs.get_mut(id) {
                o.wrapped_dek = Some(enc(w));
                o.key_version = Some(key_version);
            }
        }
    }
    if let Some(s) = cx.st.ix.spaces.get_mut(&space) {
        s.key_version = key_version;
    }
    let ac = cx.st.ix.accounts.get_mut(&account).expect("checked above");
    ac.amk_version = amk_version;
    ac.roster = enc(&roster);
    ac.roster_version = roster_version;
    ac.rotate_needed = false;
    ac.updated_at = now;
    account_state(cx)
}

/// Every live object's id, version and wrapped DEK (for rotation).
fn dek_list(cx: &mut Ctx, a: &Value) -> Res<Value> {
    let account = cx.require_device()?;
    let space = uuid_arg(a, "space_id")?;
    cx.member_space(&account, &space)?;
    let rows: Vec<Value> = cx
        .st
        .ix
        .objects
        .get(&space)
        .map(|m| {
            m.iter()
                .filter(|(_, o)| !o.deleted)
                .map(|(id, o)| {
                    json!({"id": id, "version": o.version, "wrapped_dek": o.wrapped_dek, "key_version": o.key_version})
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(Value::Array(rows))
}

// ---- push and pull ----------------------------------------------------------------

/// One change a push plans, applied only if the whole push is good.
enum Planned {
    Put { id: String, obj: Obj, body: Vec<u8>, old: Option<i64> },
    Delete { id: String, obj: Obj, old: i64 },
}

/// Commit sealed objects, in order, under the hub's lock (so seq order is
/// commit order). Results as the SQL gave them: ok, conflict (with the
/// current row), exists, stale_key, deleted. A malformed item or a kind
/// change refuses the whole push; so does going over the quota.
fn push(cx: &mut Ctx, a: &Value) -> Res<Value> {
    let account = cx.require_device()?;
    let space = uuid_arg(a, "space_id")?;
    let items = a.get("items").and_then(Value::as_array).ok_or_else(|| fail("bad_items"))?;
    if items.len() > 100 {
        return Err(fail("bad_items"));
    }
    cx.member_space(&account, &space)?;
    let sp = cx.st.ix.spaces.get(&space).expect("checked above").clone();
    let now = cx.now;
    let mut next_seq = sp.seq;
    // What each object is after the items so far.
    let mut cur: HashMap<String, Option<Obj>> = HashMap::new();
    let mut fresh_bodies: HashMap<String, Vec<u8>> = HashMap::new();
    let mut plan: Vec<Planned> = Vec::new();
    let mut results: Vec<Value> = Vec::new();
    let mut delta: i64 = 0;
    for item in items {
        let op = item.get("op").and_then(Value::as_str).unwrap_or("put");
        let id = item
            .get("id")
            .and_then(Value::as_str)
            .and_then(canonical_uuid)
            .ok_or_else(|| fail("bad_items"))?;
        let now_obj = match cur.get(&id) {
            Some(o) => o.clone(),
            None => cx.st.ix.objects.get(&space).and_then(|m| m.get(&id)).cloned(),
        };
        if op == "delete" {
            let Some(o) = now_obj.filter(|o| !o.deleted) else {
                results.push(json!({"id": id, "status": "ok"}));
                continue;
            };
            next_seq += 1;
            let gone = Obj {
                kind: o.kind.clone(),
                version: o.version + 1,
                seq: next_seq,
                wrapped_dek: None,
                key_version: None,
                padded_size: None,
                deleted: true,
                size: 0,
                created_at: o.created_at,
                updated_at: now,
                deleted_at: Some(now),
            };
            delta -= o.size as i64;
            results.push(json!({"id": id, "status": "ok", "version": gone.version, "seq": next_seq}));
            cur.insert(id.clone(), Some(gone.clone()));
            plan.push(Planned::Delete { id, obj: gone, old: o.version });
            continue;
        } else if op != "put" {
            return Err(fail("bad_items"));
        }
        let kv = item.get("key_version").and_then(Value::as_i64);
        if kv != Some(sp.key_version) {
            results.push(json!({"id": id, "status": "stale_key", "key_version": sp.key_version}));
            continue;
        }
        let base = item.get("base_version").and_then(Value::as_i64);
        let kind = item.get("kind").and_then(Value::as_str).unwrap_or_default().to_string();
        if let Some(o) = &now_obj {
            if o.deleted {
                results.push(json!({"id": id, "status": "deleted", "version": o.version}));
                continue;
            }
            if o.kind == "run" || o.kind == "event" {
                results.push(json!({"id": id, "status": "exists", "version": o.version, "seq": o.seq}));
                continue;
            }
            if base != Some(o.version) {
                let body = match fresh_bodies.get(&id) {
                    Some(b) => b.clone(),
                    None => cx.st.read_object(&space, &id, o.version).map_err(|_| deny(500, "internal"))?,
                };
                results.push(json!({"id": id, "status": "conflict", "current": {
                    "id": id, "kind": o.kind, "version": o.version, "seq": o.seq, "body": enc(&body),
                    "wrapped_dek": o.wrapped_dek, "key_version": o.key_version, "deleted": o.deleted,
                }}));
                continue;
            }
            if o.kind != kind {
                return Err(fail("kind_changed"));
            }
        } else if base.is_some() {
            results.push(json!({"id": id, "status": "conflict", "current": null}));
            continue;
        }
        if !KINDS.contains(&kind.as_str()) {
            return Err(fail("bad_value"));
        }
        let body = sealed_val(item.get("body"), b"OBS1", 73728)?;
        let wrapped = sealed_val(item.get("wrapped_dek"), keys::WRAP_MAGIC, 256)?;
        let version = now_obj.as_ref().map(|o| o.version).unwrap_or(0) + 1;
        next_seq += 1;
        let obj = Obj {
            kind,
            version,
            seq: next_seq,
            wrapped_dek: Some(enc(&wrapped)),
            key_version: Some(sp.key_version),
            padded_size: item.get("padded_size").and_then(Value::as_i64),
            deleted: false,
            size: body.len() as u64,
            created_at: now_obj.as_ref().map(|o| o.created_at).unwrap_or(now),
            updated_at: now,
            deleted_at: None,
        };
        delta += body.len() as i64 - now_obj.as_ref().map(|o| o.size as i64).unwrap_or(0);
        results.push(json!({"id": id, "status": "ok", "version": version, "seq": next_seq}));
        cur.insert(id.clone(), Some(obj.clone()));
        fresh_bodies.insert(id.clone(), body.clone());
        plan.push(Planned::Put { id, obj, body, old: now_obj.map(|o| o.version) });
    }
    if delta > 0 && cx.st.used_bytes(&account) + delta as u64 > cx.quota {
        return Err(deny(507, "quota_exceeded"));
    }
    // Apply: bodies first, then the index.
    for p in &plan {
        if let Planned::Put { id, obj, body, .. } = p {
            cx.st.write_object(&space, id, obj.version, body).map_err(|_| deny(500, "internal"))?;
        }
    }
    for p in plan {
        let (id, obj, old) = match p {
            Planned::Put { id, obj, old, .. } => (id, obj, old),
            Planned::Delete { id, obj, old } => (id, obj, Some(old)),
        };
        if let Some(v) = old
            && v != obj.version
        {
            cx.st.remove_object(&space, &id, v);
        }
        // Parts of older versions go with them (all of them on delete).
        let below = if obj.deleted { i64::MAX } else { obj.version };
        cx.st.drop_blobs(&space, &id, below);
        let by_seq = cx.st.by_seq.entry(space.clone()).or_default();
        if let Some(prev) = cx.st.ix.objects.get(&space).and_then(|m| m.get(&id)) {
            by_seq.remove(&prev.seq);
        }
        by_seq.insert(obj.seq, id.clone());
        cx.st.ix.objects.entry(space.clone()).or_default().insert(id, obj);
    }
    if let Some(s) = cx.st.ix.spaces.get_mut(&space) {
        s.seq = next_seq;
    }
    if let Some(d) = cx.st.ix.devices.get_mut(&cx.me) {
        d.last_seen_at = Some(now);
    }
    Ok(Value::Array(results))
}

/// Upload one blob part for an object version that isn't committed yet. A
/// committed version's parts never change.
fn put_blob(cx: &mut Ctx, a: &Value) -> Res<Value> {
    let account = cx.require_device()?;
    let space = uuid_arg(a, "space_id")?;
    let object = uuid_arg(a, "object_id")?;
    let version = int(a, "object_version")?;
    let part = int(a, "part")?;
    cx.member_space(&account, &space)?;
    if version < 1 || !(0..64).contains(&part) {
        return Err(fail("bad_value"));
    }
    if cx.st.ix.objects.get(&space).and_then(|m| m.get(&object)).is_some_and(|o| o.version >= version) {
        return Err(fail("committed"));
    }
    let body = sealed(a, "body", b"OBB1", 1_100_000)?;
    let key = blob_key(&object, version, part);
    let old = cx.st.ix.blobs.get(&space).and_then(|m| m.get(&key)).map(|b| b.size).unwrap_or(0);
    let used = cx.st.used_bytes(&account);
    if (body.len() as u64) > old && used + body.len() as u64 - old > cx.quota {
        return Err(deny(507, "quota_exceeded"));
    }
    cx.st.write_blob(&space, &object, version, part, &body).map_err(|_| deny(500, "internal"))?;
    cx.st
        .ix
        .blobs
        .entry(space)
        .or_default()
        .insert(key, BlobRec { size: body.len() as u64, created_at: cx.now });
    Ok(json!({"ok": true}))
}

/// Rows after a cursor, oldest first (approved devices of the space's
/// account only: the SQL's row level security).
fn pull(cx: &mut Ctx, a: &Value) -> Res<Value> {
    let account = cx.require_device()?;
    let space = uuid_arg(a, "space_id")?;
    let after = int(a, "after")?;
    let max = a.get("max_rows").and_then(Value::as_i64).unwrap_or(200).clamp(1, 500) as usize;
    cx.member_space(&account, &space)?;
    let head = cx.st.ix.spaces.get(&space).map(|s| s.seq);
    let ids: Vec<String> = cx
        .st
        .by_seq
        .get(&space)
        .map(|m| m.range(after.saturating_add(1)..).take(max).map(|(_, id)| id.clone()).collect())
        .unwrap_or_default();
    let mut rows = Vec::with_capacity(ids.len());
    for id in ids {
        let Some(o) = cx.st.ix.objects.get(&space).and_then(|m| m.get(&id)) else { continue };
        let body = if o.deleted {
            Value::Null
        } else {
            json!(enc(&cx.st.read_object(&space, &id, o.version).map_err(|_| deny(500, "internal"))?))
        };
        rows.push(json!({
            "id": id, "kind": o.kind, "version": o.version, "seq": o.seq, "body": body,
            "wrapped_dek": o.wrapped_dek, "key_version": o.key_version, "deleted": o.deleted,
        }));
    }
    Ok(json!({"head": head, "rows": rows}))
}

/// One object version's blob parts, in order.
fn get_blobs(cx: &mut Ctx, a: &Value) -> Res<Value> {
    let account = cx.require_device()?;
    let space = uuid_arg(a, "space_id")?;
    let object = uuid_arg(a, "object_id")?;
    let version = int(a, "object_version")?;
    cx.member_space(&account, &space)?;
    let mut parts: Vec<i64> = cx
        .st
        .ix
        .blobs
        .get(&space)
        .map(|m| {
            m.keys()
                .filter_map(|k| crate::store::split_blob_key(k.as_str()))
                .filter(|(o, v, _)| *o == object && *v == version)
                .map(|(_, _, p)| p)
                .collect()
        })
        .unwrap_or_default();
    parts.sort_unstable();
    let mut out = Vec::with_capacity(parts.len());
    for p in parts {
        let body = cx.st.read_blob(&space, &object, version, p).map_err(|_| deny(500, "internal"))?;
        out.push(json!({"part": p, "body": enc(&body)}));
    }
    Ok(Value::Array(out))
}

/// This device's AMK envelopes and the space keys (approved devices). A
/// pending device sees its own envelopes only; one recovering with the kit
/// gives the kit token and sees that account's kit envelopes (which only the
/// kit opens).
fn my_keys(cx: &mut Ctx, a: &Value) -> Res<Value> {
    let me = cx.device()?.clone();
    let approved = me.status == "approved";
    let kit_account = match bytes(a, "kit_token", 64)? {
        Some(token) if !approved => Some(account_for_kit(cx.st, &token).ok_or_else(|| fail("wrong_kit"))?),
        _ => None,
    };
    let mut envs: Vec<&Envelope> = cx
        .st
        .ix
        .envelopes
        .iter()
        .filter(|e| {
            let own = me.account.as_deref() == Some(e.account.as_str())
                && (e.device_id.as_deref() == Some(cx.me.as_str()) || (approved && e.recipient == "kit"));
            let kit = kit_account.as_deref() == Some(e.account.as_str()) && e.recipient == "kit";
            own || kit
        })
        .collect();
    envs.sort_by_key(|e| e.amk_version);
    let envelopes: Vec<Value> = envs
        .iter()
        .map(|e| json!({"recipient": e.recipient, "amk_version": e.amk_version, "envelope": e.envelope}))
        .collect();
    let mut space_keys: Vec<Value> = Vec::new();
    if approved && let Some(acct) = &me.account {
        let spaces: BTreeSet<&String> =
            cx.st.ix.spaces.iter().filter(|(_, s)| &s.owner == acct).map(|(id, _)| id).collect();
        let mut ks: Vec<&SpaceKey> =
            cx.st.ix.space_keys.iter().filter(|k| spaces.contains(&k.space)).collect();
        ks.sort_by_key(|k| k.version);
        space_keys = ks
            .iter()
            .map(|k| json!({"space_id": k.space, "version": k.version, "amk_version": k.amk_version, "wrapped": k.wrapped}))
            .collect();
    }
    Ok(json!({"envelopes": envelopes, "space_keys": space_keys}))
}

/// The account's devices as the hub knows them (no names: those are in the
/// sealed roster). A pending device sees only itself.
fn list_devices(cx: &mut Ctx) -> Res<Value> {
    let me = cx.device()?.clone();
    let mut rows: Vec<(&DateTime<Utc>, Value)> = match (&me.account, me.status.as_str()) {
        (Some(acct), "approved") => cx
            .st
            .ix
            .devices
            .iter()
            .filter(|(_, d)| d.account.as_deref() == Some(acct.as_str()))
            .map(|(id, d)| (&d.created_at, device_row(id, d)))
            .collect(),
        _ => vec![(&me.created_at, device_row(&cx.me, &me))],
    };
    rows.sort_by_key(|r| *r.0);
    Ok(Value::Array(rows.into_iter().map(|(_, v)| v).collect()))
}

/// The delete-account Edge Function: every device, envelope, space, object
/// and blob of the caller's account. Local visits on the devices stay.
fn delete_account(cx: &mut Ctx) -> Res<Value> {
    let account = cx.require_device()?;
    cx.st.delete_account(&account);
    Ok(json!({"deleted": true}))
}
