//! The hub's authorization, call by call (docs/security/hub-authorization.md):
//! unsigned, a bad signature, a replayed nonce, an old request, a removed
//! device, another account's data, no join code, a used or expired code,
//! the lockout, the quota, the rate limits and a refused client address.
//! Sealed values here are stand-ins with the right magic: the hub checks
//! formats and sizes, never contents (it can't open them).

use std::net::IpAddr;
use std::path::Path;

use os_sync::keys::{self, DeviceKeys};
use serde_json::{Value, json};

use super::*;

fn vault() {
    os_engine::vault::use_key([42; 32], os_engine::vault::KeySource::Environment, None);
}

struct Dev {
    id: String,
    k: DeviceKeys,
}

impl Dev {
    fn new() -> Self {
        Self { id: uuid::Uuid::now_v7().to_string(), k: DeviceKeys::generate().unwrap() }
    }
}

fn local() -> IpAddr {
    "127.0.0.1".parse().unwrap()
}

fn b64(b: &[u8]) -> String {
    keys::b64(b)
}

fn fake(magic: &[u8; 4], n: usize) -> String {
    let mut b = magic.to_vec();
    b.resize(n, 7);
    b64(&b)
}

const KIT_TOKEN: [u8; 32] = [9; 32];

fn req(dev: &Dev, name: &str, args: &Value) -> HubRequest {
    let body = serde_json::to_vec(args).unwrap();
    let path = format!("/rpc/{name}");
    let headers = os_sync::hub::sign(&dev.k, &dev.id, "POST", &path, &body).unwrap();
    HubRequest { method: "POST".into(), path, headers, body }
}

fn send(hub: &Hub, peer: IpAddr, r: &HubRequest) -> (u16, Value) {
    let out = hub.handle(peer, r);
    (out.status, serde_json::from_slice(&out.body).unwrap_or(Value::Null))
}

fn call(hub: &Hub, dev: &Dev, name: &str, args: Value) -> (u16, Value) {
    send(hub, local(), &req(dev, name, &args))
}

fn code(r: &(u16, Value)) -> &str {
    r.1["code"].as_str().unwrap_or("")
}

fn open(dir: &Path, settings: Settings, limits: Limits) -> Hub {
    vault();
    Hub::open(dir, &settings, limits).unwrap()
}

fn new_hub() -> (tempfile::TempDir, Hub) {
    let dir = tempfile::tempdir().unwrap();
    let hub = open(dir.path(), Settings::default(), Limits::default());
    (dir, hub)
}

struct Acct {
    dev: Dev,
    space: String,
}

fn create_args(dev: &Dev, join_code: Option<&str>) -> Value {
    json!({
        "join_code": join_code,
        "account_id": uuid::Uuid::new_v4().to_string(),
        "device_id": dev.id,
        "platform": "linux",
        "x25519_pub": b64(&dev.k.x_pub()),
        "ed25519_pub": b64(&dev.k.ed_pub()),
        "rebind_verifier": b64(&dev.k.rebind_verifier()),
        "kit_pub": b64(&[1; 32]),
        "kit_verifier": b64(&keys::sha256(&[&KIT_TOKEN])),
        "space_id": uuid::Uuid::new_v4().to_string(),
        "device_envelope": fake(b"OBE1", 109),
        "kit_envelope": fake(b"OBE1", 109),
        "space_key": fake(b"OBW1", 100),
        "roster": fake(b"OBS1", 300),
    })
}

fn create(hub: &Hub) -> Acct {
    let c = hub.new_join_code().unwrap().code;
    let dev = Dev::new();
    let args = create_args(&dev, Some(&c));
    let space = args["space_id"].as_str().unwrap().to_string();
    let r = call(hub, &dev, "create_account", args);
    assert_eq!(r.0, 200, "{r:?}");
    assert_eq!(r.1["device"]["status"], "approved");
    Acct { dev, space }
}

fn register_args(dev: &Dev, join_code: Option<&str>, tag: &[u8; 32]) -> Value {
    json!({
        "join_code": join_code,
        "device_id": dev.id,
        "platform": "ios",
        "x25519_pub": b64(&dev.k.x_pub()),
        "ed25519_pub": b64(&dev.k.ed_pub()),
        "rebind_verifier": b64(&dev.k.rebind_verifier()),
        "pair_mac": b64(&[3; 32]),
        "pair_blob": fake(b"OBW1", 80),
        "pair_tag": b64(tag),
    })
}

fn register(hub: &Hub, tag: &[u8; 32]) -> Dev {
    let c = hub.new_join_code().unwrap().code;
    let dev = Dev::new();
    let r = call(hub, &dev, "register_device", register_args(&dev, Some(&c), tag));
    assert_eq!(r.0, 200, "{r:?}");
    assert_eq!(r.1["device"]["status"], "pending");
    dev
}

/// Approve `dev` (pending, filed under `tag`) into `a`'s account.
fn approve(hub: &Hub, a: &Acct, dev: &Dev, tag: &[u8; 32]) {
    let g = call(hub, &a.dev, "get_account", json!({}));
    let r = call(
        hub,
        &a.dev,
        "approve_device",
        json!({
            "device_id": dev.id, "envelope": fake(b"OBE1", 109), "amk_version": g.1["amk_version"],
            "roster": fake(b"OBS1", 300), "roster_version": g.1["roster_version"].as_i64().unwrap() + 1,
            "pair_tag": b64(tag),
        }),
    );
    assert_eq!(r.0, 200, "{r:?}");
}

fn put_item(id: &str, kind: &str, base: Option<i64>) -> Value {
    json!({"op": "put", "id": id, "kind": kind, "base_version": base, "body": fake(b"OBS1", 1100),
           "wrapped_dek": fake(b"OBW1", 72), "key_version": 1, "padded_size": 1024})
}

const ALL_CALLS: [&str; 20] = [
    "account_state",
    "create_account",
    "register_device",
    "rebind_device",
    "device_status",
    "approve_device",
    "find_pending",
    "approve_with_kit",
    "revoke_device",
    "set_roster",
    "get_account",
    "rotate",
    "dek_list",
    "push",
    "put_blob",
    "pull",
    "get_blobs",
    "my_keys",
    "list_devices",
    "delete_account",
];

#[test]
fn every_call_refuses_an_unsigned_request() {
    let (_d, hub) = new_hub();
    let a = create(&hub);
    for name in ALL_CALLS {
        let mut r = req(&a.dev, name, &json!({}));
        r.headers.retain(|(k, _)| k != os_sync::hub::H_SIGNATURE);
        assert_eq!(
            send(&hub, local(), &r),
            (401, json!({"message": "openbark:unsigned", "code": "unsigned"})),
            "{name}"
        );
        let r = HubRequest {
            method: "POST".into(),
            path: format!("/rpc/{name}"),
            headers: vec![],
            body: b"{}".to_vec(),
        };
        assert_eq!(code(&send(&hub, local(), &r)), "unsigned", "{name}");
    }
}

#[test]
fn every_call_refuses_a_bad_signature() {
    let (_d, hub) = new_hub();
    let a = create(&hub);
    let other = Dev::new();
    for name in ALL_CALLS {
        // The body changed after signing.
        let mut r = req(&a.dev, name, &json!({"space_id": a.space}));
        r.body = serde_json::to_vec(&json!({"space_id": uuid::Uuid::new_v4().to_string()})).unwrap();
        assert_eq!(code(&send(&hub, local(), &r)), "bad_signature", "{name}");
        // Signed by another key in this device's name.
        let mut r = req(&other, name, &json!({}));
        for h in r.headers.iter_mut() {
            if h.0 == os_sync::hub::H_DEVICE {
                h.1 = a.dev.id.clone();
            }
        }
        assert_eq!(code(&send(&hub, local(), &r)), "bad_signature", "{name}");
    }
}

#[test]
fn a_replayed_or_old_request_is_refused() {
    let (_d, hub) = new_hub();
    let a = create(&hub);
    let r = req(&a.dev, "account_state", &json!({}));
    assert_eq!(send(&hub, local(), &r).0, 200);
    assert_eq!(code(&send(&hub, local(), &r)), "replayed");
    // Timed 10 minutes ago (signed then, correctly).
    let body = b"{}".to_vec();
    let t = chrono::Utc::now().timestamp() - 600;
    let nonce = keys::b64(&keys::random::<18>().unwrap());
    let sig =
        a.dev.k.sign(&os_sync::hub::canonical("POST", "/rpc/account_state", &body, t, &nonce, &a.dev.id));
    let r = HubRequest {
        method: "POST".into(),
        path: "/rpc/account_state".into(),
        headers: vec![
            (os_sync::hub::H_DEVICE.into(), a.dev.id.clone()),
            (os_sync::hub::H_TIME.into(), t.to_string()),
            (os_sync::hub::H_NONCE.into(), nonce),
            (os_sync::hub::H_SIGNATURE.into(), keys::b64(&sig)),
        ],
        body,
    };
    assert_eq!(code(&send(&hub, local(), &r)), "stale_request");
}

#[test]
fn an_unknown_device_may_only_create_an_account_or_ask_to_join() {
    let (_d, hub) = new_hub();
    let stranger = Dev::new();
    for name in ALL_CALLS {
        if calls::BOOTSTRAP.contains(&name) {
            continue;
        }
        assert_eq!(code(&call(&hub, &stranger, name, json!({}))), "unknown_device", "{name}");
    }
    // Creating needs the body to name the signing device and key.
    let c = hub.new_join_code().unwrap().code;
    let mut args = create_args(&stranger, Some(&c));
    args["ed25519_pub"] = json!(b64(&Dev::new().k.ed_pub()));
    assert_eq!(code(&call(&hub, &stranger, "create_account", args)), "bad_signature");
    let mut args = create_args(&stranger, Some(&c));
    args["device_id"] = json!(uuid::Uuid::now_v7().to_string());
    assert_eq!(code(&call(&hub, &stranger, "create_account", args)), "unknown_device");
}

#[test]
fn creating_an_account_or_joining_needs_a_valid_unused_unexpired_code() {
    let dir = tempfile::tempdir().unwrap();
    let hub = open(dir.path(), Settings::default(), Limits::default());
    let d = Dev::new();
    assert_eq!(code(&call(&hub, &d, "create_account", create_args(&d, None))), "join_code_required");
    let d = Dev::new();
    assert_eq!(code(&call(&hub, &d, "create_account", create_args(&d, Some("ABCD2345")))), "bad_join_code");
    let d = Dev::new();
    assert_eq!(
        code(&call(&hub, &d, "register_device", register_args(&d, None, &[5; 32]))),
        "join_code_required"
    );
    // A code works once.
    let c = hub.new_join_code().unwrap().code;
    let d = Dev::new();
    assert_eq!(call(&hub, &d, "create_account", create_args(&d, Some(&c))).0, 200);
    assert!(hub.shown_code().unwrap().used);
    let d2 = Dev::new();
    assert_eq!(code(&call(&hub, &d2, "create_account", create_args(&d2, Some(&c)))), "bad_join_code");
    assert_eq!(
        code(&call(&hub, &d2, "register_device", register_args(&d2, Some(&c), &[5; 32]))),
        "bad_join_code"
    );
    // An expired code.
    {
        let mut st = hub.store();
        calls::add_code(&mut st, "EXP1RED0", chrono::Utc::now(), chrono::Duration::seconds(-1));
    }
    let d3 = Dev::new();
    assert_eq!(code(&call(&hub, &d3, "create_account", create_args(&d3, Some("EXP1RED0")))), "bad_join_code");
    // Typed in lower case with a dash, it still matches.
    let c = hub.new_join_code().unwrap().code;
    let typed = format!("{}-{}", &c[..4], &c[4..]).to_lowercase();
    let d4 = Dev::new();
    assert_eq!(call(&hub, &d4, "register_device", register_args(&d4, Some(&typed), &[5; 32])).0, 200);
    // A code isn't used up by a request that fails for another reason.
    let c = hub.new_join_code().unwrap().code;
    let d5 = Dev::new();
    let mut bad = create_args(&d5, Some(&c));
    bad["roster"] = json!(fake(b"XXXX", 100));
    assert_eq!(code(&call(&hub, &d5, "create_account", bad)), "bad_value");
    assert_eq!(call(&hub, &d5, "create_account", create_args(&d5, Some(&c))).0, 200);
}

#[test]
fn ten_wrong_codes_lock_the_address_out() {
    let (_d, hub) = new_hub();
    let attacker: IpAddr = "192.168.1.66".parse().unwrap();
    for i in 0..10 {
        let d = Dev::new();
        let r =
            send(&hub, attacker, &req(&d, "create_account", &create_args(&d, Some(&format!("WR0NG{i:03}")))));
        assert_eq!(code(&r), "bad_join_code");
    }
    let good = hub.new_join_code().unwrap().code;
    let d = Dev::new();
    assert_eq!(
        code(&send(&hub, attacker, &req(&d, "create_account", &create_args(&d, Some(&good))))),
        "locked"
    );
    // Another address is unaffected, and the code still works there.
    let d = Dev::new();
    assert_eq!(call(&hub, &d, "create_account", create_args(&d, Some(&good))).0, 200);
}

#[test]
fn a_pending_device_waits_and_is_found_only_by_its_words() {
    let (_d, hub) = new_hub();
    let a = create(&hub);
    let tag = [11u8; 32];
    let p = register(&hub, &tag);
    // Pending: nothing that needs an approved device.
    for name in [
        "get_account",
        "set_roster",
        "rotate",
        "dek_list",
        "push",
        "put_blob",
        "pull",
        "get_blobs",
        "approve_device",
        "find_pending",
        "revoke_device",
        "delete_account",
    ] {
        assert_eq!(code(&call(&hub, &p, name, json!({}))), "not_approved", "{name}");
    }
    // It sees only itself.
    let l = call(&hub, &p, "list_devices", json!({}));
    assert_eq!(l.1.as_array().unwrap().len(), 1);
    // Asking again needs no code but can't change its keys.
    assert_eq!(call(&hub, &p, "register_device", register_args(&p, None, &tag)).0, 200);
    let mut changed = register_args(&p, None, &tag);
    changed["x25519_pub"] = json!(b64(&[8; 32]));
    assert_eq!(code(&call(&hub, &p, "register_device", changed)), "keys_changed");
    // Found by its tag only.
    assert_eq!(call(&hub, &a.dev, "find_pending", json!({"pair_tag": b64(&[12; 32])})).1, json!([]));
    let found = call(&hub, &a.dev, "find_pending", json!({"pair_tag": b64(&tag)}));
    assert_eq!(found.1[0]["id"], json!(p.id));
    // Approving needs the tag too.
    let g = call(&hub, &a.dev, "get_account", json!({}));
    let wrong = json!({"device_id": p.id, "envelope": fake(b"OBE1", 109), "amk_version": g.1["amk_version"],
        "roster": fake(b"OBS1", 300), "roster_version": 2, "pair_tag": b64(&[12; 32])});
    assert_eq!(code(&call(&hub, &a.dev, "approve_device", wrong)), "not_pending");
    approve(&hub, &a, &p, &tag);
    let s = call(&hub, &p, "account_state", json!({}));
    assert_eq!((s.1["device"]["status"].as_str(), s.1["account"].as_bool()), (Some("approved"), Some(true)));
    assert_eq!(call(&hub, &a.dev, "list_devices", json!({})).1.as_array().unwrap().len(), 2);
    let k = call(&hub, &p, "my_keys", json!({}));
    assert_eq!(k.1["space_keys"].as_array().unwrap().len(), 1);
    assert!(k.1["envelopes"].as_array().unwrap().iter().any(|e| e["recipient"] == json!(p.id)));
}

#[test]
fn another_accounts_data_is_out_of_reach() {
    let (_d, hub) = new_hub();
    let a = create(&hub);
    let b = create(&hub);
    let oid = uuid::Uuid::new_v4().to_string();
    let r =
        call(&hub, &a.dev, "push", json!({"space_id": a.space, "items": [put_item(&oid, "visit", None)]}));
    assert_eq!(r.1[0]["status"], "ok", "{r:?}");
    let blob = json!({"space_id": a.space, "object_id": uuid::Uuid::new_v4().to_string(), "object_version": 1,
        "part": 0, "body": fake(b"OBB1", 2000)});
    assert_eq!(call(&hub, &a.dev, "put_blob", blob.clone()).0, 200);
    // B, signed correctly, asks for A's space.
    let mut theirs = blob.clone();
    theirs["object_version"] = json!(2);
    for (name, args) in [
        ("pull", json!({"space_id": a.space, "after": 0, "max_rows": 100})),
        ("dek_list", json!({"space_id": a.space})),
        ("get_blobs", json!({"space_id": a.space, "object_id": blob["object_id"], "object_version": 1})),
        (
            "push",
            json!({"space_id": a.space, "items": [put_item(&uuid::Uuid::new_v4().to_string(), "visit", None)]}),
        ),
        ("put_blob", theirs),
    ] {
        assert_eq!(code(&call(&hub, &b.dev, name, args)), "unknown_space", "{name}");
    }
    // B can't remove or approve A's devices, and doesn't see them.
    assert_eq!(code(&call(&hub, &b.dev, "revoke_device", json!({"device_id": a.dev.id}))), "unknown_device");
    let l = call(&hub, &b.dev, "list_devices", json!({}));
    assert!(l.1.as_array().unwrap().iter().all(|d| d["id"] != json!(a.dev.id)));
    // A's keys aren't B's.
    let k = call(&hub, &b.dev, "my_keys", json!({}));
    assert_eq!(k.1["envelopes"].as_array().unwrap().len(), 2, "B's own and B's kit");
    // Deleting B's account leaves A's alone.
    assert_eq!(call(&hub, &b.dev, "delete_account", json!({})).0, 200);
    let p = call(&hub, &a.dev, "pull", json!({"space_id": a.space, "after": 0, "max_rows": 100}));
    assert_eq!(p.1["rows"].as_array().unwrap().len(), 1);
}

#[test]
fn a_removed_device_is_refused_at_once() {
    let (_d, hub) = new_hub();
    let a = create(&hub);
    let tag = [21u8; 32];
    let p = register(&hub, &tag);
    approve(&hub, &a, &p, &tag);
    assert_eq!(call(&hub, &p, "account_state", json!({})).0, 200);
    assert_eq!(call(&hub, &a.dev, "revoke_device", json!({"device_id": p.id})).1, json!({"revoked": p.id}));
    for name in ALL_CALLS {
        if calls::REVOKED_OK.contains(&name) {
            continue;
        }
        assert_eq!(code(&call(&hub, &p, name, json!({}))), "revoked", "{name}");
    }
    // It may learn that it was removed, with its own secret.
    let st = call(
        &hub,
        &p,
        "device_status",
        json!({"device_id": p.id, "rebind_secret": b64(p.k.rebind_secret())}),
    );
    assert_eq!(st.1, json!("revoked"));
    let st = call(&hub, &p, "device_status", json!({"device_id": p.id, "rebind_secret": b64(&[0; 32])}));
    assert_eq!(st.1, json!("unknown"));
    // The keys are due to rotate, and its envelopes are gone.
    assert_eq!(call(&hub, &a.dev, "account_state", json!({})).1["rotate_needed"], json!(true));
    // Removed from the hub's screen too: a lobby device.
    let q = register(&hub, &[22; 32]);
    hub.remove_device(&q.id).unwrap();
    assert_eq!(code(&call(&hub, &q, "account_state", json!({}))), "revoked");
}

#[test]
fn recovering_with_the_kit_finds_the_account() {
    let (_d, hub) = new_hub();
    let a = create(&hub);
    let p = register(&hub, &[31; 32]);
    let k = call(&hub, &p, "my_keys", json!({"kit_token": b64(&[1; 32])}));
    assert_eq!(code(&k), "wrong_kit");
    let k = call(&hub, &p, "my_keys", json!({"kit_token": b64(&KIT_TOKEN)}));
    let envs = k.1["envelopes"].as_array().unwrap().clone();
    assert!(!envs.is_empty() && envs.iter().all(|e| e["recipient"] == "kit"));
    let r = call(
        &hub,
        &p,
        "approve_with_kit",
        json!({"kit_token": b64(&[1; 32]), "envelope": fake(b"OBE1", 109), "amk_version": 1}),
    );
    assert_eq!(code(&r), "wrong_kit");
    let r = call(
        &hub,
        &p,
        "approve_with_kit",
        json!({"kit_token": b64(&KIT_TOKEN), "envelope": fake(b"OBE1", 109), "amk_version": 1}),
    );
    assert_eq!(r.1["device"]["status"], "approved", "{r:?}");
    assert_eq!(call(&hub, &a.dev, "list_devices", json!({})).1.as_array().unwrap().len(), 2);
}

#[test]
fn push_and_pull_keep_the_sql_semantics() {
    let (_d, hub) = new_hub();
    let a = create(&hub);
    let (v, r, s) = (
        uuid::Uuid::new_v4().to_string(),
        uuid::Uuid::new_v4().to_string(),
        uuid::Uuid::new_v4().to_string(),
    );
    let out = call(
        &hub,
        &a.dev,
        "push",
        json!({"space_id": a.space, "items": [put_item(&v, "visit", None), put_item(&r, "run", None)]}),
    );
    assert_eq!(
        (out.1[0]["status"].as_str(), out.1[0]["version"].as_i64(), out.1[0]["seq"].as_i64()),
        (Some("ok"), Some(1), Some(1))
    );
    // A stale base: conflict with the current row; a run never changes.
    let out = call(
        &hub,
        &a.dev,
        "push",
        json!({"space_id": a.space, "items": [put_item(&v, "visit", None), put_item(&r, "run", None),
        put_item(&s, "visit", Some(4))]}),
    );
    assert_eq!(out.1[0]["status"], "conflict");
    assert_eq!(out.1[0]["current"]["version"], 1);
    assert!(out.1[0]["current"]["body"].as_str().unwrap().starts_with("T0JTMQ"), "OBS1 in base64");
    assert_eq!(out.1[1]["status"], "exists");
    assert_eq!((out.1[2]["status"].as_str(), &out.1[2]["current"]), (Some("conflict"), &Value::Null));
    // An old key version.
    let mut old = put_item(&s, "visit", None);
    old["key_version"] = json!(0);
    assert_eq!(
        call(&hub, &a.dev, "push", json!({"space_id": a.space, "items": [old]})).1[0]["status"],
        "stale_key"
    );
    // A kind change refuses the whole push.
    let out = call(
        &hub,
        &a.dev,
        "push",
        json!({"space_id": a.space, "items": [put_item(&s, "visit", None), put_item(&v, "patient", Some(1))]}),
    );
    assert_eq!(code(&out), "kind_changed");
    // Update, then delete; parts of old versions go.
    let blob = |ver: i64| json!({"space_id": a.space, "object_id": v, "object_version": ver, "part": 0, "body": fake(b"OBB1", 500)});
    assert_eq!(code(&call(&hub, &a.dev, "put_blob", blob(1))), "committed");
    assert_eq!(call(&hub, &a.dev, "put_blob", blob(2)).0, 200);
    let out =
        call(&hub, &a.dev, "push", json!({"space_id": a.space, "items": [put_item(&v, "visit", Some(1))]}));
    assert_eq!(out.1[0]["version"], 2);
    let parts =
        call(&hub, &a.dev, "get_blobs", json!({"space_id": a.space, "object_id": v, "object_version": 2}));
    assert_eq!(parts.1.as_array().unwrap().len(), 1);
    let out = call(&hub, &a.dev, "push", json!({"space_id": a.space, "items": [{"op": "delete", "id": v}]}));
    assert_eq!(out.1[0]["version"], 3);
    let parts =
        call(&hub, &a.dev, "get_blobs", json!({"space_id": a.space, "object_id": v, "object_version": 2}));
    assert_eq!(parts.1, json!([]));
    // Pull: in commit order, after a cursor, with the head. (The refused
    // pushes took no numbers: v is 1, r 2, v again 3, v deleted 4.)
    let p = call(&hub, &a.dev, "pull", json!({"space_id": a.space, "after": 0, "max_rows": 10}));
    let seqs: Vec<i64> = p.1["rows"].as_array().unwrap().iter().map(|r| r["seq"].as_i64().unwrap()).collect();
    assert_eq!(seqs, vec![2, 4]);
    assert_eq!(p.1["head"], 4);
    let deleted = &p.1["rows"][1];
    assert_eq!((deleted["deleted"].as_bool(), &deleted["body"]), (Some(true), &Value::Null));
    let p = call(&hub, &a.dev, "pull", json!({"space_id": a.space, "after": 2, "max_rows": 10}));
    assert_eq!(p.1["rows"].as_array().unwrap().len(), 1);
    // Malformed: not sealed, too many items.
    let mut plain = put_item(&s, "visit", None);
    plain["body"] = json!(b64(b"{\"name\":\"Maple\"}"));
    assert_eq!(
        code(&call(&hub, &a.dev, "push", json!({"space_id": a.space, "items": [plain]}))),
        "bad_value"
    );
    let many: Vec<Value> =
        (0..101).map(|_| put_item(&uuid::Uuid::new_v4().to_string(), "visit", None)).collect();
    assert_eq!(code(&call(&hub, &a.dev, "push", json!({"space_id": a.space, "items": many}))), "bad_items");
}

#[test]
fn rotation_is_all_or_nothing() {
    let (_d, hub) = new_hub();
    let a = create(&hub);
    let v = uuid::Uuid::new_v4().to_string();
    call(&hub, &a.dev, "push", json!({"space_id": a.space, "items": [put_item(&v, "visit", None)]}));
    let w = uuid::Uuid::new_v4().to_string();
    call(&hub, &a.dev, "push", json!({"space_id": a.space, "items": [put_item(&w, "visit", None)]}));
    let args = |rewrapped: Value, envelopes: Value| {
        json!({"amk_version": 2, "envelopes": envelopes, "kit_envelope": fake(b"OBE1", 109), "space_id": a.space,
            "key_version": 2, "space_key": fake(b"OBW1", 100), "roster": fake(b"OBS1", 300), "roster_version": 2,
            "rewrapped": rewrapped})
    };
    let env = json!([{"device_id": a.dev.id, "envelope": fake(b"OBE1", 109)}]);
    let rw = |id: &str| json!({"id": id, "version": 1, "wrapped_dek": fake(b"OBW1", 72)});
    // An object left out, or one given twice in its place.
    assert_eq!(code(&call(&hub, &a.dev, "rotate", args(json!([rw(&v)]), env.clone()))), "objects_changed");
    assert_eq!(
        code(&call(&hub, &a.dev, "rotate", args(json!([rw(&v), rw(&v)]), env.clone()))),
        "objects_changed"
    );
    // A device missing, or one that isn't approved.
    assert_eq!(
        code(&call(&hub, &a.dev, "rotate", args(json!([rw(&v), rw(&w)]), json!([])))),
        "devices_changed"
    );
    let extra = json!([{"device_id": a.dev.id, "envelope": fake(b"OBE1", 109)},
        {"device_id": uuid::Uuid::now_v7().to_string(), "envelope": fake(b"OBE1", 109)}]);
    assert_eq!(code(&call(&hub, &a.dev, "rotate", args(json!([rw(&v), rw(&w)]), extra))), "devices_changed");
    let r = call(&hub, &a.dev, "rotate", args(json!([rw(&v), rw(&w)]), env.clone()));
    assert_eq!(r.1["amk_version"], 2, "{r:?}");
    let k = call(&hub, &a.dev, "my_keys", json!({}));
    assert!(k.1["envelopes"].as_array().unwrap().iter().all(|e| e["amk_version"] == 2));
    assert_eq!(k.1["space_keys"][0]["version"], 2);
    let d = call(&hub, &a.dev, "dek_list", json!({"space_id": a.space}));
    assert!(d.1.as_array().unwrap().iter().all(|o| o["key_version"] == 2));
    // The same rotation again is stale.
    assert_eq!(code(&call(&hub, &a.dev, "rotate", args(json!([rw(&v), rw(&w)]), env))), "stale_amk");
}

#[test]
fn the_quota_holds() {
    let dir = tempfile::tempdir().unwrap();
    let hub = open(dir.path(), Settings { quota_mb: 1, ..Settings::default() }, Limits::default());
    let a = create(&hub);
    let part = |n: i64| {
        json!({"space_id": a.space, "object_id": uuid::Uuid::new_v4().to_string(), "object_version": 1,
        "part": 0, "body": fake(b"OBB1", 400_000 + n as usize)})
    };
    assert_eq!(call(&hub, &a.dev, "put_blob", part(0)).0, 200);
    assert_eq!(call(&hub, &a.dev, "put_blob", part(1)).0, 200);
    let r = call(&hub, &a.dev, "put_blob", part(2));
    assert_eq!((r.0, code(&r)), (507, "quota_exceeded"));
    // Objects count too.
    let mut big: Vec<Value> =
        (0..100).map(|_| put_item(&uuid::Uuid::new_v4().to_string(), "visit", None)).collect();
    for i in big.iter_mut() {
        i["body"] = json!(fake(b"OBS1", 70_000));
    }
    assert_eq!(
        code(&call(&hub, &a.dev, "push", json!({"space_id": a.space, "items": big}))),
        "quota_exceeded"
    );
    // Another account has its own quota.
    let b = create(&hub);
    let r = call(
        &hub,
        &b.dev,
        "put_blob",
        json!({"space_id": b.space, "object_id": uuid::Uuid::new_v4().to_string(),
        "object_version": 1, "part": 0, "body": fake(b"OBB1", 400_000)}),
    );
    assert_eq!(r.0, 200);
}

#[test]
fn rate_limits_hold_per_address_and_per_account() {
    let dir = tempfile::tempdir().unwrap();
    let limits = Limits { per_ip_per_min: 40, per_account_per_min: 15, code_ttl_secs: CODE_TTL_SECS };
    let hub = open(dir.path(), Settings::default(), limits);
    let a = create(&hub);
    let mut limited = None;
    for i in 0..20 {
        if code(&call(&hub, &a.dev, "account_state", json!({}))) == "rate_limited" {
            limited = Some(i);
            break;
        }
    }
    assert!(limited.is_some_and(|i| i >= 13), "per account: {limited:?}");
    // Unsigned junk from one address runs out too, and doesn't touch another's.
    let junk =
        HubRequest { method: "POST".into(), path: "/rpc/pull".into(), headers: vec![], body: b"{}".to_vec() };
    let noisy: IpAddr = "10.0.0.9".parse().unwrap();
    let codes: Vec<String> = (0..45).map(|_| code(&send(&hub, noisy, &junk)).to_string()).collect();
    assert!(codes.iter().any(|c| c == "rate_limited"));
    assert_eq!(code(&send(&hub, "10.0.0.10".parse().unwrap(), &junk)), "unsigned");
}

#[test]
fn a_client_outside_the_allowed_ranges_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let settings = Settings { allowed: vec!["127.0.0.1/32".into()], ..Settings::default() };
    let hub = open(dir.path(), settings, Limits::default());
    assert!(hub.allows(local()));
    assert!(!hub.allows("127.0.0.2".parse().unwrap()));
    assert!(!hub.allows("8.8.8.8".parse().unwrap()));
    let d = Dev::new();
    let r = send(&hub, "127.0.0.2".parse().unwrap(), &req(&d, "account_state", &json!({})));
    assert_eq!((r.0, code(&r)), (403, "address_not_allowed"));
    // The defaults: private ranges, never the internet.
    hub.apply(&Settings::default()).unwrap();
    assert!(hub.allows("192.168.4.4".parse().unwrap()));
    assert!(!hub.allows("8.8.8.8".parse().unwrap()));
    assert!(hub.apply(&Settings { allowed: vec![], ..Settings::default() }).is_err());
    assert!(hub.apply(&Settings { allowed: vec!["nonsense".into()], ..Settings::default() }).is_err());
}

#[test]
fn the_store_survives_a_restart_and_holds_ciphertext_only() {
    let dir = tempfile::tempdir().unwrap();
    let (space, dev) = {
        let hub = open(dir.path(), Settings::default(), Limits::default());
        let a = create(&hub);
        call(
            &hub,
            &a.dev,
            "push",
            json!({"space_id": a.space, "items": [put_item(&uuid::Uuid::new_v4().to_string(), "visit", None)]}),
        );
        (a.space, a.dev)
    };
    let hub = open(dir.path(), Settings::default(), Limits::default());
    let p = call(&hub, &dev, "pull", json!({"space_id": space, "after": 0, "max_rows": 10}));
    assert_eq!(p.1["rows"].as_array().unwrap().len(), 1, "{p:?}");
    // Every file is sealed by the vault.
    for entry in walk(dir.path()) {
        let bytes = std::fs::read(&entry).unwrap();
        assert!(bytes.starts_with(b"OBK1"), "{} isn't sealed", entry.display());
    }
    let seen = Hub::inspect(dir.path()).unwrap();
    assert!(seen.get("code_key").is_none());
    assert_eq!(seen["object_bodies"].as_array().unwrap().len(), 1);
}

fn walk(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(walk(&p));
        } else {
            out.push(p);
        }
    }
    out
}

#[test]
fn over_tls_the_pinned_certificate_decides() {
    let dir = tempfile::tempdir().unwrap();
    let settings = Settings { allowed: vec!["127.0.0.1/32".into()], ..Settings::default() };
    let hub = std::sync::Arc::new(open(dir.path(), settings.clone(), Limits::default()));
    let id = tls::load_or_create(dir.path()).unwrap();
    assert_eq!(tls::load_or_create(dir.path()).unwrap().fingerprint(), id.fingerprint(), "kept");
    let listener = Listener::start(hub.clone(), &id, 0).unwrap();
    let port = listener.port;
    let d = Dev::new();
    let send_tls = |pin: [u8; 32]| {
        let body = serde_json::to_vec(&json!({})).unwrap();
        let mut headers = os_sync::hub::sign(&d.k, &d.id, "POST", "/rpc/account_state", &body).unwrap();
        headers.push((os_sync::hub::H_PIN.into(), hex::encode(pin)));
        os_sync::hub::send(&os_sync::supabase::Request {
            method: "POST",
            url: format!("obhub://127.0.0.1:{port}/rpc/account_state"),
            headers,
            body: Some(body),
        })
    };
    // The right certificate: the hub answers (this device isn't known yet).
    let r = send_tls(id.fingerprint()).unwrap();
    assert_eq!(r.status, 401);
    assert!(String::from_utf8_lossy(&r.body).contains("unknown_device"));
    // Another certificate: refused before anything is sent.
    let e = send_tls([0; 32]).unwrap_err();
    assert!(e.to_string().contains("Pair again"), "{e}");
    // The check typed by hand, and a wrong one.
    assert_eq!(os_sync::hub::probe("127.0.0.1", port, &id.check_code()).unwrap(), id.fingerprint());
    assert!(os_sync::hub::probe("127.0.0.1", port, "00000-00000").is_err());
    // A client address outside the ranges is closed before the handshake.
    hub.apply(&Settings { allowed: vec!["10.0.0.0/8".into()], ..settings }).unwrap();
    let e = send_tls(id.fingerprint()).unwrap_err();
    assert!(!e.to_string().contains("Pair again"), "{e}");
    listener.stop();
}
