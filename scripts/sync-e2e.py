"""End-to-end test of encrypted sync (ADR-0017, M3 phase 2 acceptance): two
OpenBark servers (A and B) syncing through a LOCAL Supabase. Drives the same
HTTP API the web UI uses. Synthetic data only; never point it at real data.

Setup:
  bash scripts/sync-local-supabase.sh start
  eval "$(bash scripts/sync-local-supabase.sh env)"
  Two `openbark serve` processes on 4336 (A) and 4337 (B), each with its own
  OPENBARK_SESSIONS_DIR, OPENBARK_DATA_KEY (a fresh throwaway key),
  OPENBARK_TEMPLATES_DIR, OPENBARK_USERS_FILE, OPENBARK_LOCATIONS_FILE and
  OPENBARK_SYNC_SECRETS_DIR, plus OPENBARK_SYNC_URL / OPENBARK_SYNC_KEY set
  to the local Supabase and OPENBARK_SYNC_INTERVAL_SECS=5. A speech model and
  gemma-4-e4b-it-q4_0 in OPENBARK_MODELS_DIR.

Run (OPENBARK_SYNC_E2E_WAV: a short consult recording):
  python -I scripts/sync-e2e.py setup|basic|offline|offline2|remove|recover|check
in that order. `offline` stops the local API gateway container
(supabase_kong_openbark-sync-dev, in WSL) to take both servers offline.
"""
import json, os, re, subprocess, sys, time, urllib.request, urllib.parse, uuid

A = os.environ.get("OPENBARK_SYNC_E2E_A", "http://127.0.0.1:4336")
B = os.environ.get("OPENBARK_SYNC_E2E_B", "http://127.0.0.1:4337")
WAV = os.environ.get("OPENBARK_SYNC_E2E_WAV", "")
STATE = os.environ.get("OPENBARK_SYNC_E2E_STATE", os.path.join(os.getcwd(), "target", "sync-e2e-state.json"))
SB = os.environ.get("OPENBARK_SYNC_TEST_URL", "http://127.0.0.1:54321")
SECRET = os.environ["OPENBARK_SYNC_TEST_SECRET"]
MAIL = os.environ.get("OPENBARK_SYNC_TEST_MAIL", "http://127.0.0.1:54324")
LLM = "gemma-4-e4b-it-q4_0"
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


def code_for(email, used):
    for _ in range(60):
        q = urllib.parse.quote(f"to:{email}")
        msgs = json.loads(urllib.request.urlopen(f"{MAIL}/api/v1/search?query={q}").read())["messages"] or []
        for m in msgs:
            if m["ID"] in used:
                continue
            text = json.loads(urllib.request.urlopen(f"{MAIL}/api/v1/message/{m['ID']}").read())["Text"]
            c = re.search(r"\b(\d{6})\b", text)
            if c:
                used.add(m["ID"])
                return c.group(1)
        time.sleep(0.5)
    raise RuntimeError("no code")


def raw(table):
    req = urllib.request.Request(f"{SB}/rest/v1/{table}?select=*", headers={"apikey": SECRET, "Accept-Profile": "sync"})
    return json.loads(urllib.request.urlopen(req).read())


def wait(what, f, secs=90):
    t0 = time.time()
    while time.time() - t0 < secs:
        v = f()
        if v:
            log(f"ok: {what} after {time.time() - t0:.1f} s")
            return v, time.time() - t0
        time.sleep(1)
    raise RuntimeError(f"timed out: {what}")


def load():
    return json.load(open(STATE)) if os.path.exists(STATE) else {}


def save(st):
    json.dump(st, open(STATE, "w"), indent=1)


def sign_in(base, email, name, used):
    for _ in range(10):
        try:
            call(base, "/api/sync/code", {"email": email})
            break
        except RuntimeError as e:
            if "Too many" not in str(e):
                raise
            time.sleep(1.2)
    return call(base, "/api/sync/verify", {"code": code_for(email, used), "device_name": name, "backfill_days": 30})


def find(base, pred):
    return next((s for s in call(base, "/api/sessions") if pred(s)), None)


def setup():
    st = {"email": f"e2e-{uuid.uuid4().hex[:10]}@example.org"}
    used = set()
    assert call(A, "/api/sync")["phase"] == "off", "sync is off by default"
    nxt = sign_in(A, st["email"], "Front desk PC", used)
    assert nxt["next"] == "kit", nxt
    st["kit"] = nxt["words"]
    a, b = nxt["check"]
    call(A, "/api/sync/kit/confirm", {"answers": [[a, nxt["words"][a - 1]], [b, nxt["words"][b - 1]]]})
    assert call(A, "/api/sync")["phase"] == "active"
    log("A: account created, kit checked")
    nxt = sign_in(B, st["email"], "Exam room laptop", used)
    assert nxt["next"] == "pair", nxt
    words = " ".join(nxt["words"])
    found = call(A, "/api/sync/pair", {"words": words, "approve": False})
    assert found["name"] == "Exam room laptop", found
    call(A, "/api/sync/pair", {"words": words, "approve": True})
    wait("B joined", lambda: call(B, "/api/sync")["phase"] == "active", 60)
    st["used"] = sorted(used)
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
    sid_meta = None
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
    b_visits = os.environ.get("OPENBARK_SYNC_E2E_B_VISITS")
    if b_visits:
        assert not os.path.exists(os.path.join(b_visits, got["id"], "audio.wav")), "no audio file on B"
    assert vb["audio_on"]["device_name"] == "Front desk PC", vb["audio_on"]
    log("B: transcribed visit, no audio, 'audio is on Front desk PC'")
    t0 = time.time()
    run = call(B, f"/api/sessions/{urllib.parse.quote(got['id'])}/notes", {"llm": LLM, "template": "soap"})
    log(f"B: drafted in {time.time() - t0:.1f} s, faithfulness passed:", run.get("run", run).get("faithfulness", {}).get("passed") if isinstance(run, dict) else None)
    try:
        call(B, f"/api/sessions/{urllib.parse.quote(got['id'])}/sign", {"by": "Dr. B. Imani"})
    except RuntimeError as e:
        log("sign refused, editing a heading by hand then signing:", str(e)[:160])
        view = call(B, f"/api/sessions/{urllib.parse.quote(got['id'])}")
        heading = next(iter(view["latest_run"]["note"].keys()))
        call(B, f"/api/sessions/{urllib.parse.quote(got['id'])}/edit", {"heading": heading, "value": "Dental check discussed.", "by": "Dr. B. Imani"})
        call(B, f"/api/sessions/{urllib.parse.quote(got['id'])}/sign", {"by": "Dr. B. Imani"})
    log("B: signed")
    call(B, "/api/sync/now", {})
    wait("signed note on A", lambda: (find(A, lambda s: s["patient"] == " ".join(NAMES)) or {}).get("signed"), 60)
    va = call(A, f"/api/sessions/{urllib.parse.quote(v['id'])}")
    assert va["signed_by"] == "Dr. B. Imani" and va["has_audio"] is True
    st["visit_a"], st["visit_b"] = v["id"], got["id"]
    save(st)
    ciphertext_only()


def ciphertext_only():
    words = NAMES + ["Front desk PC", "Exam room laptop", "Dr. B. Imani", "dental check"]
    for t in ["accounts", "devices", "key_envelopes", "spaces", "space_keys", "objects", "blobs"]:
        rows = raw(t)
        text = json.dumps(rows)
        for w in words:
            assert w not in text, f"{w} readable in sync.{t}"
        for r in rows:
            for col in ["body", "roster", "envelope", "wrapped", "wrapped_dek"]:
                if r.get(col):
                    b = bytes.fromhex(r[col][2:])
                    assert b[:4] in (b"OBS1", b"OBB1", b"OBE1", b"OBW1"), (t, col, b[:4])
                    for w in words:
                        assert w.encode() not in b, (t, col, w)
                    assert b[:4] != b"RIFF" and b[:4] != b"OBA1"
    log(f"ok: server holds ciphertext only ({len(raw('objects'))} objects, {len(raw('blobs'))} blob parts, no names, no audio)")


def kong(action):
    subprocess.run(["wsl", "-d", "Ubuntu-24.04", "--", "docker", action, "supabase_kong_openbark-sync-dev"], check=True,
                   stdout=subprocess.DEVNULL)


def offline():
    st = load()
    kong("stop")
    log("sync service unreachable (both servers offline)")
    try:
        # A edits the patient; B drafts and signs a second visit made earlier on A.
        q = urllib.parse.urlencode({"name": "recheck", "patient_name": "Pepper", "last_name": "Quill", "reason": "recheck",
                                    "species": "Cat", "consent_rule": "all_parties", "consent_method": "verbal",
                                    "consent_by": "Dr. A. Rivera"})
        va = call(A, f"/api/sessions/{urllib.parse.quote(st['visit_a'])}")
        p = va["patient"]
        p["reason"] = "dental check, owner asked about diet"
        call(A, f"/api/sessions/{urllib.parse.quote(st['visit_a'])}/patient", p)
        time.sleep(1.5)
        vb = call(B, f"/api/sessions/{urllib.parse.quote(st['visit_b'])}")
        p = vb["patient"]
        p["reason"] = "dental check, recheck in 6 months"
        call(B, f"/api/sessions/{urllib.parse.quote(st['visit_b'])}/patient", p)
        # B adds an addendum to the signed note while offline.
        call(B, f"/api/sessions/{urllib.parse.quote(st['visit_b'])}/addenda", {"text": "Owner called: eating well.", "by": "Dr. B. Imani"})
        s = call(A, "/api/sync")
        log("A while offline:", s.get("last_error"), "pending:", s.get("pending_uploads"))
    finally:
        kong("start")
    log("sync service back")
    def same():
        try:
            call(A, "/api/sync/now", {})
            call(B, "/api/sync/now", {})
        except RuntimeError as e:  # the service is still starting
            log("retrying:", str(e)[:80])
            return False
        va = call(A, f"/api/sessions/{urllib.parse.quote(st['visit_a'])}")
        vb = call(B, f"/api/sessions/{urllib.parse.quote(st['visit_b'])}")
        return (va["patient"]["reason"] == vb["patient"]["reason"] and len(va["addenda"]) == len(vb["addenda"]) == 1
                and va["patient"]["reason"] == "dental check, recheck in 6 months") and (va, vb)
    (va, vb), secs = wait("both reach the same state", same, 120)
    conflicts = call(B, "/api/sync")["conflicts"] + call(A, "/api/sync")["conflicts"]
    log("conflict log:", [(c["field"], c["kept_from"], c["other_from"]) for c in conflicts])
    assert any(c["field"] == "patient" for c in conflicts)


def offline2():
    """A drafts offline while B drafts and signs the same visit offline."""
    st = load()
    q = urllib.parse.urlencode({"name": "recheck", "patient_name": "Pepper", "last_name": "Quill", "reason": "recheck",
                                "species": "Dog", "consent_rule": "all_parties", "consent_method": "verbal",
                                "consent_by": "Dr. A. Rivera"})
    v = call(A, f"/api/sessions?{q}", raw=open(WAV, "rb").read())
    def on_b():
        s = find(B, lambda s: s["patient"] == "Pepper Quill")
        return s and call(B, f"/api/sessions/{urllib.parse.quote(s['id'])}")["transcript"] and s
    vb, _ = wait("second visit on B", on_b, 60)
    kong("stop")
    try:
        call(B, f"/api/sessions/{urllib.parse.quote(vb['id'])}/notes", {"llm": LLM, "template": "soap"})
        call(B, f"/api/sessions/{urllib.parse.quote(vb['id'])}/sign", {"by": "Dr. B. Imani"})
        log("B (offline): drafted and signed")
        call(A, f"/api/sessions/{urllib.parse.quote(v['id'])}/notes", {"llm": LLM, "template": "soap"})
        log("A (offline): drafted after B's signature")
    finally:
        kong("start")
    def same():
        call(A, "/api/sync/now", {})
        call(B, "/api/sync/now", {})
        a = call(A, f"/api/sessions/{urllib.parse.quote(v['id'])}")
        b = call(B, f"/api/sessions/{urllib.parse.quote(vb['id'])}")
        ok = (a["signed_by"] == b["signed_by"] == "Dr. B. Imani" and len(a.get("set_aside", [])) == 1
              and len(b.get("set_aside", [])) == 1 and a["latest_markdown"] == b["latest_markdown"])
        return ok and (a, b)
    (a, b), _ = wait("both keep both drafts; the signed one is the record", same, 120)
    log("set aside on A:", a["set_aside"][0]["reason"], "signed on", a["set_aside"][0]["signed_on"])
    assert a["set_aside"][0]["markdown"], "the offline draft is kept, offered as an addendum"


def remove():
    st = load()
    devices = call(A, "/api/sync")["devices"]
    b = next(d for d in devices if d["name"] == "Exam room laptop")
    call(A, f"/api/sync/devices/{b['id']}/remove", {})
    log("A removed B; keys rotated:", [r["amk_version"] for r in raw("accounts")])
    # B is told at its next request.
    wait("B locked out and erased its synced copy", lambda: call(B, "/api/sync")["phase"] == "revoked"
         and not find(B, lambda s: s["patient"] == " ".join(NAMES)), 60)
    assert raw("devices") and any(d["id"] == b["id"] and d["status"] == "revoked" for d in raw("devices"))


def recover():
    st = load()
    used = set(st.get("used", []))
    nxt = sign_in(B, st["email"], "Replacement laptop", used)
    assert nxt["next"] == "pair", nxt
    call(B, "/api/sync/recover", {"words": " ".join(st["kit"])})
    wait("B recovered every visit from the kit", lambda: (call(B, "/api/sync/now", {}) or True)
         and find(B, lambda s: s["patient"] == " ".join(NAMES) and s["signed"]), 60)
    ciphertext_only()


if __name__ == "__main__":
    {"setup": setup, "basic": basic, "offline": offline, "offline2": offline2, "remove": remove, "recover": recover,
     "check": ciphertext_only}[sys.argv[1]]()
    log("done:", sys.argv[1])
