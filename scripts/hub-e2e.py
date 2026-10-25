"""End-to-end test of sync through a clinic hub (ADR-0029 section 11): three
OpenBark servers on one machine, no Docker and no Supabase.

  HUB  a desktop with hub mode on (its listener on 127.0.0.1:47110, allowed
       ranges 127.0.0.0/25 so the refusal of an address outside them can be
       tried from 127.0.0.200)
  A    a phone (OPENBARK_PLATFORM=ios): records
  B    a second computer: drafts and signs

It drives the same HTTP API the app uses, and talks to the hub's listener
directly (TLS, Ed25519-signed requests) for the attacks. Synthetic data only;
never point it at real data.

Setup (the workflow does this): the three `openbark serve` processes with
their own folders and data keys, OPENBARK_SYNC_PREVIEW=1 and
OPENBARK_SYNC_INTERVAL_SECS=5; the hub with OPENBARK_HUB_RATE_IP_PER_MIN,
OPENBARK_HUB_RATE_ACCOUNT_PER_MIN and OPENBARK_HUB_CODE_TTL_SECS shrunk. A
speech model and the note model in OPENBARK_MODELS_DIR. Python's
`cryptography` package (Ed25519) for the attack step.

Run, in order:
  python3 -I scripts/hub-e2e.py setup|basic|offline|offline2|remove|recover|check|attacks
and `restarted` after the workflow restarts the hub process.
"""
import base64, hashlib, json, os, socket, ssl, subprocess, sys, time, urllib.parse, urllib.request, uuid
import http.client

HUB = os.environ.get("OPENBARK_HUB_E2E_HUB", "http://127.0.0.1:4335")
A = os.environ.get("OPENBARK_HUB_E2E_A", "http://127.0.0.1:4336")
B = os.environ.get("OPENBARK_HUB_E2E_B", "http://127.0.0.1:4337")
WAV = os.environ.get("OPENBARK_HUB_E2E_WAV", "")
STATE = os.environ.get("OPENBARK_HUB_E2E_STATE", os.path.join(os.getcwd(), "target", "hub-e2e-state.json"))
LLM = os.environ.get("OPENBARK_HUB_E2E_LLM", "gemma-4-e2b-it-q4_0")
A_NAME = os.environ.get("OPENBARK_HUB_E2E_A_NAME", "Dr Rivera's iPhone")
B_NAME = os.environ.get("OPENBARK_HUB_E2E_B_NAME", "Exam room computer")
HUB_DIR = os.environ.get("OPENBARK_HUB_E2E_HUB_DIR", "")
HUB_KEY = os.environ.get("OPENBARK_HUB_E2E_HUB_KEY", "")
BIN = os.environ.get("OPENBARK_HUB_E2E_BIN", "target/release/openbark")
PORT = int(os.environ.get("OPENBARK_HUB_E2E_PORT", "47110"))
CODE_TTL = int(os.environ.get("OPENBARK_HUB_CODE_TTL_SECS", "60"))
NAMES = ["Biscuit", "Fairweather"]


def log(*a):
    print(time.strftime("%H:%M:%S"), *a, flush=True)


def call(base, path, body=None, raw=None, method=None, timeout=600):
    data = raw if raw is not None else (json.dumps(body).encode() if body is not None else None)
    req = urllib.request.Request(base + path, data=data, method=method or ("POST" if data is not None else "GET"))
    req.add_header("x-openbark", "1")
    if body is not None:
        req.add_header("content-type", "application/json")
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:
            return json.loads(r.read() or b"null")
    except urllib.error.HTTPError as e:
        raise RuntimeError(f"{path}: {e.code} {e.read().decode(errors='replace')}")


def wait(what, f, secs=90):
    t0 = time.time()
    while time.time() - t0 < secs:
        try:
            v = f()
        except RuntimeError as e:
            # The hub is off or coming back: try again.
            if "Hub not reachable" not in str(e) and "closed the connection" not in str(e):
                raise
            v = None
        if v:
            log(f"ok: {what} after {time.time() - t0:.1f} s")
            return v, time.time() - t0
        time.sleep(1)
    raise RuntimeError(f"timed out: {what}")


def load():
    return json.load(open(STATE)) if os.path.exists(STATE) else {}


def save(st):
    json.dump(st, open(STATE, "w"), indent=1)


def find(base, pred):
    return next((s for s in call(base, "/api/sessions") if pred(s)), None)


def hub_status():
    return call(HUB, "/api/clinic-hub")


def hub_mode(on, **extra):
    return call(HUB, "/api/clinic-hub", dict({"on": on}, **extra))


def new_code():
    """A fresh join code from the hub's screen: (code without the dash, status)."""
    s = call(HUB, "/api/clinic-hub/code", {})
    return s["join"]["code"].replace("-", ""), s


def link_for(code, fingerprint):
    return f"openbark-hub:1?h=127.0.0.1&p={PORT}&f={fingerprint}&c={code}"


def setup():
    st = {}
    assert call(A, "/api/sync")["phase"] == "off", "sync is off by default"
    s = hub_status()
    assert s["available"] and not s["on"], s
    s = hub_mode(True, port=PORT, allowed=["127.0.0.0/25"])
    assert s["listening"], s
    st["fingerprint"], st["check"] = s["fingerprint"], s["check"]
    log("hub: on, port", PORT, "check", st["check"])
    # Finding the hub on the network (mDNS): logged only, runners may block multicast.
    try:
        log("found on the network:", [(h["name"], h["host"], h["port"]) for h in call(A, "/api/sync/hub/find", {})])
    except RuntimeError as e:
        log("mDNS search not available here:", str(e)[:120])
    # A wrong hub certificate is refused, before the code is used.
    code, _ = new_code()
    bad = st["fingerprint"][:-2] + ("00" if not st["fingerprint"].endswith("00") else "11")
    try:
        call(A, "/api/sync/hub/join", {"link": link_for(code, bad), "device_name": A_NAME, "existing": False})
        raise AssertionError("a wrong certificate was accepted")
    except RuntimeError as e:
        assert "Pair again" in str(e), e
    log("ok: a wrong hub certificate is refused (pair again)")
    try:
        call(A, "/api/sync/hub/join", {"link": code, "address": f"127.0.0.1:{PORT}", "check": "00000-00000",
                                      "device_name": A_NAME, "existing": False})
        raise AssertionError("a wrong certificate check was accepted")
    except RuntimeError as e:
        assert "Pair again" in str(e), e
    log("ok: a wrong certificate check is refused")
    # A: the person's first device, from the QR code's link.
    nxt = call(A, "/api/sync/hub/join", {"link": link_for(code, st["fingerprint"]), "device_name": A_NAME,
                                        "backfill_days": 30, "existing": False})
    assert nxt["next"] == "kit", nxt
    st["kit"] = nxt["words"]
    a, b = nxt["check"]
    call(A, "/api/sync/kit/confirm", {"answers": [[a, nxt["words"][a - 1]], [b, nxt["words"][b - 1]]]})
    assert call(A, "/api/sync")["phase"] == "active"
    assert hub_status()["join"]["used"], "the code is used up"
    log("A: account created on the hub, kit checked")
    # B: the same person's second device, the code typed by hand with the check.
    code, _ = new_code()
    nxt = call(B, "/api/sync/hub/join", {"link": code[:4] + "-" + code[4:].lower(), "address": f"127.0.0.1:{PORT}",
                                        "check": st["check"], "device_name": B_NAME, "backfill_days": 30,
                                        "existing": True})
    assert nxt["next"] == "pair", nxt
    words = " ".join(nxt["words"])
    found = call(A, "/api/sync/pair", {"words": words, "approve": False})
    assert found["name"] == B_NAME, found
    call(A, "/api/sync/pair", {"words": words, "approve": True})
    wait("B joined", lambda: call(B, "/api/sync")["phase"] == "active", 60)
    assert call(B, "/api/sync")["hub"]["check"] == st["check"]
    save(st)


def basic():
    st = load()
    if find(A, lambda s: s["patient"] == " ".join(NAMES)):
        raise SystemExit("already imported; restart the demos fresh")
    q = urllib.parse.urlencode({"name": "young dog dental", "patient_name": NAMES[0], "last_name": NAMES[1],
                                "reason": "dental check", "species": "Dog", "consent_rule": "all_parties",
                                "consent_method": "verbal", "consent_by": "Dr. A. Rivera"})
    t0 = time.time()
    v = call(A, f"/api/sessions?{q}", raw=open(WAV, "rb").read())
    log(f"A: imported and transcribed in {time.time() - t0:.1f} s:", v["id"])
    call(A, "/api/sync/now", {})

    def transcribed_on_b():
        s = find(B, lambda s: s["patient"] == " ".join(NAMES))
        if not s:
            return None
        v = call(B, f"/api/sessions/{urllib.parse.quote(s['id'])}")
        return v["transcript"] and v["transcript"]["segments"] and s
    got, secs = wait("visit on B as Transcribed", transcribed_on_b, 60)
    st["basic_seconds"] = round(secs, 1)
    vb = call(B, f"/api/sessions/{urllib.parse.quote(got['id'])}")
    assert vb["transcript"] and vb["transcript"]["segments"], "transcript arrived"
    assert vb["has_audio"] is False, "no audio on B"
    b_visits = os.environ.get("OPENBARK_HUB_E2E_B_VISITS")
    if b_visits:
        assert not os.path.exists(os.path.join(b_visits, got["id"], "audio.wav")), "no audio file on B"
    assert vb["audio_on"]["device_name"] == A_NAME, vb["audio_on"]
    log(f"B: transcribed visit, no audio, 'audio is on {A_NAME}'")
    t0 = time.time()
    call(B, f"/api/sessions/{urllib.parse.quote(got['id'])}/notes", {"llm": LLM, "template": "soap"})
    log(f"B: drafted in {time.time() - t0:.1f} s")
    try:
        call(B, f"/api/sessions/{urllib.parse.quote(got['id'])}/sign", {"by": "Dr. B. Imani"})
    except RuntimeError as e:
        log("sign refused, editing a heading by hand then signing:", str(e)[:160])
        view = call(B, f"/api/sessions/{urllib.parse.quote(got['id'])}")
        heading = next(iter(view["latest_run"]["note"].keys()))
        call(B, f"/api/sessions/{urllib.parse.quote(got['id'])}/edit",
             {"heading": heading, "value": "Dental check discussed.", "by": "Dr. B. Imani"})
        call(B, f"/api/sessions/{urllib.parse.quote(got['id'])}/sign", {"by": "Dr. B. Imani"})
    log("B: signed")
    call(B, "/api/sync/now", {})
    wait("signed note on A", lambda: (find(A, lambda s: s["patient"] == " ".join(NAMES)) or {}).get("signed"), 60)
    va = call(A, f"/api/sessions/{urllib.parse.quote(v['id'])}")
    assert va["signed_by"] == "Dr. B. Imani" and va["has_audio"] is True
    st["visit_a"], st["visit_b"] = v["id"], got["id"]
    save(st)
    ciphertext_only()


def inspect():
    """The hub's store as its server role sees it (decrypted with the hub's data key)."""
    env = dict(os.environ, OPENBARK_DATA_KEY=HUB_KEY)
    out = subprocess.run([BIN, "hub", "inspect", "--dir", HUB_DIR], env=env, check=True, capture_output=True)
    return json.loads(out.stdout)


def ciphertext_only():
    words = NAMES + [A_NAME, B_NAME, "Dr. B. Imani", "dental check", "Pepper", "Quill", "Owner called"]
    seen = inspect()
    text = json.dumps(seen)
    for w in words:
        assert w not in text, f"{w} readable in the hub's store"
    n = 0
    for list_name in ["object_bodies", "blob_bodies"]:
        for item in seen[list_name]:
            b = base64.b64decode(item["body"])
            assert b[:4] in (b"OBS1", b"OBB1"), (list_name, b[:4])
            assert b[:4] not in (b"RIFF", b"OBA1")
            for w in words:
                assert w.encode() not in b, (list_name, w)
            n += 1
    for acct in seen["accounts"].values():
        assert base64.b64decode(acct["roster"])[:4] == b"OBS1", "the roster is sealed"
    for e in seen["envelopes"]:
        assert base64.b64decode(e["envelope"])[:4] == b"OBE1"
    for k in seen["space_keys"]:
        assert base64.b64decode(k["wrapped"])[:4] == b"OBW1"
    # On disk every file is sealed by the vault besides.
    files = 0
    for root, _, names in os.walk(HUB_DIR):
        for name in names:
            raw = open(os.path.join(root, name), "rb").read()
            assert raw[:4] == b"OBK1", f"{name} isn't sealed at rest"
            for w in words:
                assert w.encode() not in raw
            files += 1
    log(f"ok: the hub holds ciphertext only ({n} sealed bodies, {files} files sealed at rest, no names, no audio)")


def offline():
    st = load()
    hub_mode(False)
    log("hub off (both devices offline)")
    try:
        va = call(A, f"/api/sessions/{urllib.parse.quote(st['visit_a'])}")
        p = va["patient"]
        p["reason"] = "dental check, owner asked about diet"
        call(A, f"/api/sessions/{urllib.parse.quote(st['visit_a'])}/patient", p)
        time.sleep(1.5)
        vb = call(B, f"/api/sessions/{urllib.parse.quote(st['visit_b'])}")
        p = vb["patient"]
        p["reason"] = "dental check, recheck in 6 months"
        call(B, f"/api/sessions/{urllib.parse.quote(st['visit_b'])}/patient", p)
        call(B, f"/api/sessions/{urllib.parse.quote(st['visit_b'])}/addenda",
             {"text": "Owner called: eating well.", "by": "Dr. B. Imani"})
        try:
            call(A, "/api/sync/now", {})
        except RuntimeError as e:
            log("A while offline:", str(e)[:120])
        s = call(A, "/api/sync")
        log("A status while offline:", s.get("last_error"), "pending:", s.get("pending_uploads"))
        assert "Hub not reachable" in (s.get("last_error") or ""), s.get("last_error")
    finally:
        hub_mode(True)
    log("hub back on")

    def same():
        try:
            call(A, "/api/sync/now", {})
            call(B, "/api/sync/now", {})
        except RuntimeError as e:
            log("retrying:", str(e)[:80])
            return False
        va = call(A, f"/api/sessions/{urllib.parse.quote(st['visit_a'])}")
        vb = call(B, f"/api/sessions/{urllib.parse.quote(st['visit_b'])}")
        return (va["patient"]["reason"] == vb["patient"]["reason"] and len(va["addenda"]) == len(vb["addenda"]) == 1
                and va["patient"]["reason"] == "dental check, recheck in 6 months") and (va, vb)
    wait("both reach the same state", same, 120)
    conflicts = call(B, "/api/sync")["conflicts"] + call(A, "/api/sync")["conflicts"]
    log("conflict log:", [(c["field"], c["kept_from"], c["other_from"]) for c in conflicts])
    assert any(c["field"] == "patient" for c in conflicts)


def offline2():
    """A drafts offline while B drafts and signs the same visit offline."""
    q = urllib.parse.urlencode({"name": "recheck", "patient_name": "Pepper", "last_name": "Quill", "reason": "recheck",
                                "species": "Dog", "consent_rule": "all_parties", "consent_method": "verbal",
                                "consent_by": "Dr. A. Rivera"})
    v = call(A, f"/api/sessions?{q}", raw=open(WAV, "rb").read())

    def on_b():
        s = find(B, lambda s: s["patient"] == "Pepper Quill")
        return s and call(B, f"/api/sessions/{urllib.parse.quote(s['id'])}")["transcript"] and s
    vb, _ = wait("second visit on B", on_b, 60)
    hub_mode(False)
    try:
        call(B, f"/api/sessions/{urllib.parse.quote(vb['id'])}/notes", {"llm": LLM, "template": "soap"})
        call(B, f"/api/sessions/{urllib.parse.quote(vb['id'])}/sign", {"by": "Dr. B. Imani"})
        log("B (offline): drafted and signed")
        call(A, f"/api/sessions/{urllib.parse.quote(v['id'])}/notes", {"llm": LLM, "template": "soap"})
        log("A (offline): drafted after B's signature")
    finally:
        hub_mode(True)

    def same():
        try:
            call(A, "/api/sync/now", {})
            call(B, "/api/sync/now", {})
        except RuntimeError as e:
            log("retrying:", str(e)[:80])
            return False
        a = call(A, f"/api/sessions/{urllib.parse.quote(v['id'])}")
        b = call(B, f"/api/sessions/{urllib.parse.quote(vb['id'])}")
        ok = (a["signed_by"] == b["signed_by"] == "Dr. B. Imani" and len(a.get("set_aside", [])) == 1
              and len(b.get("set_aside", [])) == 1 and a["latest_markdown"] == b["latest_markdown"])
        return ok and (a, b)
    (a, b), _ = wait("both keep both drafts; the signed one is the record", same, 120)
    log("set aside on A:", a["set_aside"][0]["reason"], "signed on", a["set_aside"][0]["signed_on"])
    assert a["set_aside"][0]["markdown"], "the offline draft is kept, offered as an addendum"


def restarted():
    """After the workflow restarted the hub process: same certificate, same store, devices carry on."""
    st = load()
    s = hub_status()
    assert s["listening"] and s["fingerprint"] == st["fingerprint"], "the certificate survived the restart"
    wait("A syncs with the restarted hub", lambda: call(A, "/api/sync/now", {})["last_error"] is None, 60)
    wait("B syncs with the restarted hub", lambda: call(B, "/api/sync/now", {})["last_error"] is None, 60)
    log("ok: the restarted hub kept its certificate and store")


def remove():
    st = load()
    devices = call(A, "/api/sync")["devices"]
    b = next(d for d in devices if d["name"] == B_NAME)
    st["b_old_id"] = b["id"]
    save(st)
    call(A, f"/api/sync/devices/{b['id']}/remove", {})
    log("A removed B; keys rotated")
    wait("B locked out and erased its synced copy", lambda: call(B, "/api/sync")["phase"] == "revoked"
         and not find(B, lambda s: s["patient"] == " ".join(NAMES)), 60)
    hub_devices = hub_status()["devices"]
    assert any(d["id"] == b["id"] and d["status"] == "revoked" for d in hub_devices), hub_devices
    log("ok: the hub lists B as removed and refuses it")


def recover():
    st = load()
    code, _ = new_code()
    nxt = call(B, "/api/sync/hub/join", {"link": link_for(code, st["fingerprint"]), "device_name": "Replacement laptop",
                                        "backfill_days": 30, "existing": True})
    assert nxt["next"] == "pair", nxt
    call(B, "/api/sync/recover", {"words": " ".join(st["kit"])})
    wait("B recovered every visit from the kit", lambda: (call(B, "/api/sync/now", {}) or True)
         and find(B, lambda s: s["patient"] == " ".join(NAMES) and s["signed"]), 60)
    ciphertext_only()


def check():
    ciphertext_only()


# ---- attacks: a client of our own, straight to the hub's listener -------------------------------

def attacks():
    from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
    from cryptography.hazmat.primitives.serialization import Encoding, PublicFormat
    st = load()
    fp = hub_status()["fingerprint"]

    def tls(src):
        ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT)
        ctx.check_hostname = False
        ctx.verify_mode = ssl.CERT_NONE
        ctx.minimum_version = ssl.TLSVersion.TLSv1_3
        c = http.client.HTTPSConnection("127.0.0.1", PORT, context=ctx, source_address=(src, 0), timeout=15)
        c.connect()
        got = hashlib.sha256(c.sock.getpeercert(binary_form=True)).hexdigest()
        assert got == fp, "the listener presents the paired certificate"
        return c

    class Dev:
        def __init__(self):
            self.key = Ed25519PrivateKey.generate()
            self.pub = self.key.public_key().public_bytes(Encoding.Raw, PublicFormat.Raw)
            self.id = str(uuid.uuid4())
            self.secret = os.urandom(32)

        def headers(self, name, body, t=None, nonce=None):
            t = int(time.time()) if t is None else t
            nonce = nonce or base64.b64encode(os.urandom(18)).decode()
            msg = f"OBH1\nPOST\n/rpc/{name}\n{hashlib.sha256(body).hexdigest()}\n{t}\n{nonce}\n{self.id}".encode()
            return {"x-openbark-device": self.id, "x-openbark-time": str(t), "x-openbark-nonce": nonce,
                    "x-openbark-signature": base64.b64encode(self.key.sign(msg)).decode(),
                    "content-type": "application/json"}

    def send(c, name, body, headers):
        c.request("POST", f"/rpc/{name}", body=body, headers=headers)
        r = c.getresponse()
        data = r.read()
        return r.status, json.loads(data or b"null")

    def rpc(c, dev, name, args, **kw):
        body = json.dumps(args).encode()
        return send(c, name, body, dev.headers(name, body, **kw))

    def code_of(r):
        return (r[1] or {}).get("code") if isinstance(r[1], dict) else None

    b64 = lambda b: base64.b64encode(b).decode()
    fake = lambda magic, n: b64(magic + bytes(n - 4))
    kit_token = os.urandom(32)

    def create_args(dev, join_code):
        return {"join_code": join_code, "account_id": str(uuid.uuid4()), "device_id": dev.id, "platform": "linux",
                "x25519_pub": b64(os.urandom(32)), "ed25519_pub": b64(dev.pub),
                "rebind_verifier": b64(hashlib.sha256(dev.secret).digest()), "kit_pub": b64(os.urandom(32)),
                "kit_verifier": b64(hashlib.sha256(kit_token).digest()), "space_id": str(uuid.uuid4()),
                "device_envelope": fake(b"OBE1", 109), "kit_envelope": fake(b"OBE1", 109),
                "space_key": fake(b"OBW1", 100), "roster": fake(b"OBS1", 300)}

    def register_args(dev, join_code, tag):
        return {"join_code": join_code, "device_id": dev.id, "platform": "ios", "x25519_pub": b64(os.urandom(32)),
                "ed25519_pub": b64(dev.pub), "rebind_verifier": b64(hashlib.sha256(dev.secret).digest()),
                "pair_mac": b64(os.urandom(32)), "pair_blob": fake(b"OBW1", 80), "pair_tag": b64(tag)}

    c = tls("127.0.0.10")
    log("ok: the listener presents the hub's certificate (TLS 1.3)")
    # Unsigned.
    r = send(c, "account_state", b"{}", {"content-type": "application/json"})
    assert r[0] == 401 and code_of(r) == "unsigned", r
    log("ok: an unsigned request is refused")
    # Creating an account: no code, a wrong code.
    p1 = Dev()
    r = rpc(c, p1, "create_account", create_args(p1, None))
    assert code_of(r) == "join_code_required", r
    r = rpc(c, p1, "create_account", create_args(p1, "WR0NGC0D"))
    assert code_of(r) == "bad_join_code", r
    log("ok: creating an account without a valid join code is refused")
    code, _ = new_code()
    args = create_args(p1, code)
    space = args["space_id"]
    r = rpc(c, p1, "create_account", args)
    assert r[0] == 200 and r[1]["device"]["status"] == "approved", r
    p2 = Dev()
    r = rpc(c, p2, "create_account", create_args(p2, code))
    assert code_of(r) == "bad_join_code", r
    log("ok: a used join code is refused")
    # Registering a device: no code.
    p3 = Dev()
    r = rpc(c, p3, "register_device", register_args(p3, None, os.urandom(32)))
    assert code_of(r) == "join_code_required", r
    log("ok: registering a device without a join code is refused")
    # A bad signature, a replayed nonce, an old request.
    body = b"{}"
    h = p1.headers("account_state", body)
    r = send(c, "account_state", b'{"x":1}', h)
    assert code_of(r) == "bad_signature", r
    h = p1.headers("account_state", body)
    assert send(c, "account_state", body, h)[0] == 200
    r = send(c, "account_state", body, h)
    assert code_of(r) == "replayed", r
    r = rpc(c, p1, "account_state", {}, t=int(time.time()) - 600)
    assert code_of(r) == "stale_request", r
    other = Dev()
    h = other.headers("account_state", body)
    h["x-openbark-device"] = p1.id
    assert code_of(send(c, "account_state", body, h)) == "bad_signature"
    log("ok: a bad signature, a replayed nonce and an old request are refused")
    # Another account's data: A's space.
    a_space = next(sid for sid, sp in inspect()["spaces"].items() if sid != space)
    for name, a in [("pull", {"space_id": a_space, "after": 0, "max_rows": 100}), ("dek_list", {"space_id": a_space}),
                    ("get_blobs", {"space_id": a_space, "object_id": str(uuid.uuid4()), "object_version": 1})]:
        r = rpc(c, p1, name, a)
        assert code_of(r) == "unknown_space", (name, r)
    log("ok: another account's space is out of reach")
    # A removed device: P2 joins P1's account, is removed, and is refused.
    tag = os.urandom(32)
    code, _ = new_code()
    p4 = Dev()
    assert rpc(c, p4, "register_device", register_args(p4, code, tag))[0] == 200
    acct = rpc(c, p1, "get_account", {})[1]
    r = rpc(c, p1, "approve_device", {"device_id": p4.id, "envelope": fake(b"OBE1", 109), "amk_version": acct["amk_version"],
                                     "roster": fake(b"OBS1", 300), "roster_version": acct["roster_version"] + 1,
                                     "pair_tag": b64(tag)})
    assert r[0] == 200, r
    assert rpc(c, p4, "account_state", {})[1]["device"]["status"] == "approved"
    assert rpc(c, p1, "revoke_device", {"device_id": p4.id})[0] == 200
    r = rpc(c, p4, "account_state", {})
    assert r[0] == 403 and code_of(r) == "revoked", r
    r = rpc(c, p4, "pull", {"space_id": space, "after": 0, "max_rows": 10})
    assert code_of(r) == "revoked", r
    log("ok: a removed device is refused")
    # An expired code.
    code, _ = new_code()
    log(f"waiting {CODE_TTL + 2} s for a join code to expire")
    time.sleep(CODE_TTL + 2)
    c = tls("127.0.0.10")
    p5 = Dev()
    r = rpc(c, p5, "create_account", create_args(p5, code))
    assert code_of(r) == "bad_join_code", r
    log("ok: an expired join code is refused")
    # The quota: shrink it, fill P1's space, put it back.
    used = hub_status().get("used_mb", 0)
    quota = int(used) + 6
    hub_mode(True, quota_mb=quota)
    refused = None
    for i in range(quota + 4):
        r = rpc(c, p1, "put_blob", {"space_id": space, "object_id": str(uuid.uuid4()), "object_version": 1, "part": 0,
                                    "body": fake(b"OBB1", 1_000_000)})
        if code_of(r) == "quota_exceeded":
            refused = i
            break
        assert r[0] == 200, r
    hub_mode(True, quota_mb=20480)
    assert refused is not None, "the quota held"
    log(f"ok: the storage quota held ({quota} MB; refused after {refused} parts of 1 MB)")
    # Ten wrong codes lock an address out, even for a good code.
    c2 = tls("127.0.0.11")
    for i in range(10):
        d = Dev()
        assert code_of(rpc(c2, d, "create_account", create_args(d, f"BAD{i:05d}"))) == "bad_join_code"
    code, _ = new_code()
    d = Dev()
    r = rpc(c2, d, "create_account", create_args(d, code))
    assert r[0] == 403 and code_of(r) == "locked", r
    log("ok: ten wrong join codes lock the address out for an hour")
    # A client address outside the allowed ranges: closed before the handshake.
    try:
        tls("127.0.0.200")
        raise AssertionError("an address outside the allowed ranges got through")
    except (ssl.SSLError, ConnectionError, OSError) as e:
        log("ok: an address outside the allowed ranges is refused:", type(e).__name__)
    # Rate limits: per account (P1), then per address (unsigned junk from 127.0.0.12).
    c = tls("127.0.0.10")
    limited = None
    for i in range(3 * int(os.environ.get("OPENBARK_HUB_RATE_ACCOUNT_PER_MIN", "600"))):
        r = rpc(c, p1, "account_state", {})
        if r[0] == 429:
            limited = i
            break
    assert limited is not None and code_of(r) == "rate_limited", "the account rate limit held"
    log(f"ok: the per-account rate limit held (after {limited} requests)")
    c3 = tls("127.0.0.12")
    limited = None
    for i in range(3 * int(os.environ.get("OPENBARK_HUB_RATE_IP_PER_MIN", "1500"))):
        r = send(c3, "account_state", b"{}", {"content-type": "application/json"})
        if r[0] == 429:
            limited = i
            break
    assert limited is not None, "the address rate limit held"
    log(f"ok: the per-address rate limit held (after {limited} requests)")
    # The devices are unaffected.
    wait("A still syncs", lambda: call(A, "/api/sync/now", {})["last_error"] is None, 60)


if __name__ == "__main__":
    {"setup": setup, "basic": basic, "offline": offline, "offline2": offline2, "restarted": restarted,
     "remove": remove, "recover": recover, "check": check, "attacks": attacks}[sys.argv[1]]()
    log("done:", sys.argv[1])
