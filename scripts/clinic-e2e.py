"""End-to-end test of clinics, invites and usage counts (ADR-0028, the
hospital dashboard, phase 1) against a LOCAL Supabase. Drives the same REST
API and Edge Functions the website portal uses. Synthetic data only; never
point it at the hosted project.

Setup:
  bash scripts/sync-local-supabase.sh start
  eval "$(bash scripts/sync-local-supabase.sh env)"

Run:
  python3 -I scripts/clinic-e2e.py

Accounts: an admin, a member (an existing account invited by email), a
newcomer (no account; invited by email, then signs in), a colleague who joins
with a code, and an outsider. Sign-in codes come from the local mail catcher
(Mailpit). A few checks need the database itself (moving an invite's expiry
into the past, an old usage row for the purge, the list of text columns):
they use psql against the local database (OPENBARK_CLINIC_E2E_DB).

Prints one line per check and exits non-zero if any check failed.
"""
import datetime, json, os, re, subprocess, sys, time, urllib.error, urllib.parse, urllib.request, uuid

SB = os.environ.get("OPENBARK_SYNC_TEST_URL", "http://127.0.0.1:54321")
KEY = os.environ["OPENBARK_SYNC_TEST_KEY"]
SECRET = os.environ["OPENBARK_SYNC_TEST_SECRET"]
MAIL = os.environ.get("OPENBARK_SYNC_TEST_MAIL", "http://127.0.0.1:54324")
DB = os.environ.get("OPENBARK_CLINIC_E2E_DB", "postgresql://postgres:postgres@127.0.0.1:54322/postgres")
if not re.match(r"^https?://(127\.0\.0\.1|localhost)(:\d+)?$", SB):
    sys.exit(f"refusing to run against {SB}: local Supabase only")

TAG = uuid.uuid4().hex[:8]
CLINIC_A = "Riverside Animal Hospital"
CLINIC_A2 = "Riverside Animal Hospital and Referral Centre"
CLINIC_B = "Hillside Vets"
NAMES = {"admin": "Dr Alex Rivera", "member": "Sam Okafor", "newcomer": "Jo Newman", "coder": "Casey Tran",
         "outsider": "Pat Outsider"}
EMAIL = {k: f"clinic-e2e-{TAG}-{k}@example.org" for k in NAMES}
EXTRA = [f"clinic-e2e-{TAG}-{k}@example.org" for k in ("direct", "pending", "spare")]
S = {}  # state shared by the steps
FAILED = []


def log(*a):
    print(time.strftime("%H:%M:%S"), *a, flush=True)


class ApiError(Exception):
    def __init__(self, status, body):
        self.status, self.body = status, body
        msg = body.get("message") if isinstance(body, dict) else None
        self.code = msg[9:] if isinstance(msg, str) and msg.startswith("openbark:") else (
            body.get("error") or body.get("code") if isinstance(body, dict) else None)
        super().__init__(f"{status} {json.dumps(body)[:300]}")


def http(method, url, body=None, headers=None):
    data = json.dumps(body).encode() if body is not None else None
    req = urllib.request.Request(url, data=data, method=method)
    for k, v in (headers or {}).items():
        req.add_header(k, v)
    if data is not None:
        req.add_header("Content-Type", "application/json")
    try:
        with urllib.request.urlopen(req, timeout=60) as r:
            text = r.read()
            return r.status, (json.loads(text) if text else None)
    except urllib.error.HTTPError as e:
        text = e.read()
        try:
            parsed = json.loads(text) if text else {}
        except ValueError:
            parsed = {"raw": text.decode(errors="replace")}
        raise ApiError(e.code, parsed)


def auth_headers(token=None, key=KEY):
    h = {"apikey": key}
    if token:
        h["Authorization"] = f"Bearer {token}"
    return h


def rpc(name, args=None, token=None):
    h = auth_headers(token)
    h["Content-Profile"] = "clinic"
    return http("POST", f"{SB}/rest/v1/rpc/{name}", args or {}, h)[1]


def table(name, token=None, query="select=*", method="GET", body=None, key=KEY):
    h = auth_headers(token, key)
    h["Accept-Profile" if method == "GET" else "Content-Profile"] = "clinic"
    return http(method, f"{SB}/rest/v1/{name}?{query}", body, h)[1]


def function(name, body, token=None, method="POST"):
    return http(method, f"{SB}/functions/v1/{name}", body, auth_headers(token))[1]


def psql(sql):
    out = subprocess.run(["psql", DB, "-At", "-v", "ON_ERROR_STOP=1", "-c", sql], capture_output=True, text=True)
    if out.returncode != 0:
        raise RuntimeError(f"psql: {out.stderr.strip()}")
    return out.stdout.strip()


def check(cond, what):
    if not cond:
        raise AssertionError(what)
    log(f"  ok: {what}")


def refused(code, fn, what):
    """fn() must fail with openbark:<code> (or, for code None, any refusal)."""
    try:
        got = fn()
    except ApiError as e:
        if code is None or e.code == code:
            log(f"  ok: {what} (refused: {e.status} {e.code})")
            return e
        raise AssertionError(f"{what}: expected {code}, got {e.status} {e.code}: {e}")
    raise AssertionError(f"{what}: expected refusal {code}, got {json.dumps(got)[:300]}")


def denied(fn, what):
    """A permission refusal from PostgREST (no grant), never rows."""
    try:
        got = fn()
    except ApiError as e:
        if e.status in (401, 403) or (isinstance(e.body, dict) and e.body.get("code") == "42501"):
            log(f"  ok: {what} (denied: {e.status})")
            return
        raise AssertionError(f"{what}: expected a permission refusal, got {e}")
    raise AssertionError(f"{what}: expected a permission refusal, got {json.dumps(got)[:300]}")


def nothing(fn, what):
    """Either refused outright or an empty list."""
    try:
        got = fn()
    except ApiError as e:
        log(f"  ok: {what} (refused: {e.status})")
        return
    if got in ([], None):
        log(f"  ok: {what} (no rows)")
        return
    raise AssertionError(f"{what}: got {json.dumps(got)[:300]}")


def messages(email, subject_part):
    q = urllib.parse.quote(f"to:{email}")
    msgs = json.loads(urllib.request.urlopen(f"{MAIL}/api/v1/search?query={q}").read())["messages"] or []
    return [m for m in msgs if subject_part.lower() in (m.get("Subject") or "").lower()]


def message_text(mid):
    m = json.loads(urllib.request.urlopen(f"{MAIL}/api/v1/message/{mid}").read())
    return (m.get("Text") or "") + "\n" + (m.get("HTML") or "")


USED = set()


def code_for(email):
    for _ in range(60):
        for m in messages(email, "sign-in code"):
            if m["ID"] in USED:
                continue
            c = re.search(r"\b(\d{6})\b", json.loads(urllib.request.urlopen(f"{MAIL}/api/v1/message/{m['ID']}").read())["Text"])
            if c:
                USED.add(m["ID"])
                return c.group(1)
        time.sleep(0.5)
    raise RuntimeError(f"no sign-in code for {email}")


def sign_in(email):
    """The portal's own calls: /otp with create_user, then /verify (type email)."""
    for _ in range(10):
        try:
            http("POST", f"{SB}/auth/v1/otp", {"email": email, "create_user": True}, auth_headers())
            break
        except ApiError as e:
            if e.status != 429:
                raise
            time.sleep(1.5)
    out = http("POST", f"{SB}/auth/v1/verify", {"type": "email", "email": email, "token": code_for(email)}, auth_headers())[1]
    return out["access_token"], out["user"]["id"]


def tok(who):
    return S["tok"][who]


def day(offset):
    return (S["today"] + datetime.timedelta(days=offset)).isoformat()


def report(who, offset, device, platform="windows", version="0.2.0", v=0, m=0, d=0, s=0, c=0, clinic=None):
    return rpc("report_usage", {"clinic_id": clinic or S["a"], "day": day(offset), "device_id": device, "platform": platform,
                                "app_version": version, "visits_recorded": v, "minutes_recorded": m, "notes_drafted": d,
                                "notes_signed": s, "calls_recorded": c}, tok(who))


def close(a, b):
    return abs(float(a) - float(b)) < 0.05


STEPS = []


def step(f):
    STEPS.append(f)
    return f


# ---------------------------------------------------------------------------


@step
def accounts():
    S["tok"], S["uid"] = {}, {}
    for who in ("admin", "member", "coder", "outsider"):
        S["tok"][who], S["uid"][who] = sign_in(EMAIL[who])
        log(f"  ok: {who} signed in with an emailed code")
    S["today"] = datetime.date.fromisoformat(psql("select current_date"))
    log(f"  database day (UTC): {S['today']}")


@step
def anon_gets_nothing():
    for t, q in (("clinics", "select=id,name"), ("members", "select=*"), ("invites", "select=*"),
                 ("join_codes", "select=*"), ("code_attempts", "select=*"), ("usage_daily", "select=*")):
        nothing(lambda: table(t, query=q), f"anon reads nothing from clinic.{t}")
    for f, args in (("my_clinics", {}), ("my_invites", {}), ("create_clinic", {"name": "X", "display_name": "Y"})):
        refused(None, lambda: rpc(f, args), f"anon can't call clinic.{f}")
    refused(None, lambda: http("POST", f"{SB}/rest/v1/rpc/purge", {}, {"apikey": KEY, "Content-Profile": "clinic"}),
            "anon can't call clinic.purge")


@step
def create_clinic():
    refused("bad_name", lambda: rpc("create_clinic", {"name": "   ", "display_name": NAMES["admin"]}, tok("admin")),
            "an empty clinic name is refused")
    refused("bad_name", lambda: rpc("create_clinic", {"name": "x" * 121, "display_name": NAMES["admin"]}, tok("admin")),
            "a 121-character clinic name is refused")
    refused("bad_name", lambda: rpc("create_clinic", {"name": "Bad\u0007Name", "display_name": NAMES["admin"]}, tok("admin")),
            "a clinic name with a control character is refused")
    refused("bad_name", lambda: rpc("create_clinic", {"name": CLINIC_A, "display_name": "y" * 81}, tok("admin")),
            "an 81-character display name is refused")
    c = rpc("create_clinic", {"name": "  Riverside   Animal Hospital ", "display_name": NAMES["admin"]}, tok("admin"))
    S["a"] = c["id"]
    check(c["name"] == CLINIC_A and c["role"] == "admin", "admin created the clinic (name tidied) and is its admin")
    mine = rpc("my_clinics", {}, tok("admin"))
    check(len(mine) == 1 and mine[0]["sole_admin"] is True and mine[0]["members"] == 1, "my_clinics: one clinic, sole admin")
    rpc("rename_clinic", {"clinic_id": S["a"], "name": CLINIC_A2}, tok("admin"))
    rpc("rename_clinic", {"clinic_id": S["a"], "name": CLINIC_A}, tok("admin"))
    check(rpc("my_clinics", {}, tok("admin"))[0]["name"] == CLINIC_A, "admin renamed the clinic (and back)")


@step
def invite_by_email():
    a = S["a"]
    refused(None, lambda: function("clinic-invite", {"clinic_id": a, "emails": [EMAIL["member"]]}), "the function refuses a call with no token")
    refused("not_admin", lambda: function("clinic-invite", {"clinic_id": a, "emails": [EMAIL["member"]]}, tok("outsider")),
            "the function refuses someone who isn't an admin of the clinic")
    refused("bad_clinic", lambda: function("clinic-invite", {"clinic_id": "nope", "emails": [EMAIL["member"]]}, tok("admin")),
            "the function refuses a malformed clinic id")
    refused(None, lambda: function("clinic-invite", None, tok("admin"), method="GET"), "the function refuses GET")
    out = function("clinic-invite", {"clinic_id": a, "role": "member",
                                     "emails": [EMAIL["member"], EMAIL["newcomer"].upper(), "not an email", f" {EMAIL['member']} "]},
                   tok("admin"))["results"]
    by = {r["email"]: r for r in out}
    log("  results:", json.dumps(out))
    check(by[EMAIL["member"]]["status"] == "invited" and by[EMAIL["member"]]["delivery"] == "sign_in",
          "existing account: invited, sees it at the next sign-in (no email)")
    check(by[EMAIL["newcomer"]]["status"] == "invited" and by[EMAIL["newcomer"]]["delivery"] == "emailed",
          "no account yet: invited and emailed through Supabase Auth")
    check(by["not an email"]["status"] == "invalid", "a malformed address is reported, not stored")
    check(len(out) == 3 and all("account" not in r for r in out), "the duplicate was folded in; no account field leaks")
    for _ in range(40):
        got = messages(EMAIL["newcomer"], "invited")
        if got:
            break
        time.sleep(0.5)
    check(got, "the newcomer received the invite email")
    text = message_text(got[0]["ID"])
    check("/auth/v1/verify" not in text and "token" not in text.lower() and "/portal" in text,
          "the invite email has no sign-in link or token, only the account page")
    check(not messages(EMAIL["member"], "invited"), "the existing account got no invite email")
    denied(lambda: rpc("invite_members_for", {"user_id": S["uid"]["admin"], "clinic_id": a, "emails": [EXTRA[0]], "role": "member"},
                       tok("admin")), "a signed-in person can't call invite_members_for (service role only)")
    direct = rpc("invite_members", {"clinic_id": a, "emails": [EXTRA[0]], "role": "member"}, tok("admin"))
    check(direct == [{"email": EXTRA[0], "status": "invited"}], "the direct RPC records an invite without saying who has an account")
    refused("not_admin", lambda: rpc("invite_members", {"clinic_id": a, "emails": [EXTRA[1]], "role": "member"}, tok("outsider")),
            "an outsider can't invite to the clinic")
    refused("bad_role", lambda: rpc("invite_members", {"clinic_id": a, "emails": [EXTRA[1]], "role": "owner"}, tok("admin")),
            "an unknown role is refused")
    adm = rpc("clinic_admin", {"clinic_id": a}, tok("admin"))
    S["direct_invite"] = next(i["id"] for i in adm["invites"] if i["email"] == EXTRA[0])
    check({i["email"] for i in adm["invites"]} == {EMAIL["member"], EMAIL["newcomer"], EXTRA[0]}, "the admin sees three pending invites")


@step
def member_accepts():
    inv = rpc("my_invites", {}, tok("member"))
    check(len(inv) == 1 and inv[0]["clinic_name"] == CLINIC_A and inv[0]["invited_by"] == NAMES["admin"] and inv[0]["role"] == "member",
          "the member sees the pending invite, the clinic's name and who sent it")
    check(rpc("my_invites", {}, tok("outsider")) == [], "the outsider sees no invites")
    refused("no_invite", lambda: rpc("accept_invite", {"invite_id": inv[0]["id"], "display_name": "Mallory"}, tok("outsider")),
            "the outsider can't accept someone else's invite")
    refused("no_invite", lambda: rpc("decline_invite", {"invite_id": inv[0]["id"]}, tok("outsider")),
            "the outsider can't decline someone else's invite")
    got = rpc("accept_invite", {"invite_id": inv[0]["id"], "display_name": NAMES["member"]}, tok("member"))
    check(got["clinic_id"] == S["a"] and got["role"] == "member", "the member accepted")
    check(rpc("my_invites", {}, tok("member")) == [], "the accepted invite is gone")
    mine = rpc("my_clinics", {}, tok("member"))
    check(len(mine) == 1 and mine[0]["share_usage"] is False and mine[0]["display_name"] == NAMES["member"],
          "my_clinics: member, sharing off by default")


@step
def newcomer_signs_in():
    S["tok"]["newcomer"], S["uid"]["newcomer"] = sign_in(EMAIL["newcomer"])
    log("  ok: the invited newcomer signed in with the portal's /otp + /verify (a code, no link)")
    inv = rpc("my_invites", {}, tok("newcomer"))
    check(len(inv) == 1 and inv[0]["clinic_name"] == CLINIC_A, "the newcomer sees the invite after signing in")
    rpc("accept_invite", {"invite_id": inv[0]["id"], "display_name": NAMES["newcomer"]}, tok("newcomer"))
    check(rpc("my_clinics", {}, tok("newcomer"))[0]["role"] == "member", "the newcomer joined")


@step
def join_code():
    a = S["a"]
    refused("not_admin", lambda: rpc("create_join_code", {"clinic_id": a}, tok("member")), "a member can't make join codes")
    jc = rpc("create_join_code", {"clinic_id": a}, tok("admin"))
    check(re.match(r"^[0-9A-HJKMNP-TV-Z]{4}-[0-9A-HJKMNP-TV-Z]{4}$", jc["code"]), f"a join code in Crockford base32: {jc['code']}")
    stored = psql(f"select encode(code_hash, 'hex') from clinic.join_codes where id = '{jc['id']}'")
    check(len(stored) == 64 and jc["code"].replace("-", "") not in psql("select string_agg(encode(code_hash, 'escape'), ',') from clinic.join_codes"),
          "only the SHA-256 of the code is stored")
    typed = jc["code"].replace("-", " ").lower().replace("0", "o").replace("1", "l")
    got = rpc("join_with_code", {"code": typed, "display_name": NAMES["coder"]}, tok("coder"))
    check(got.get("clinic_id") == a and got.get("role") == "member", f"the colleague joined with the code typed as '{typed}'")
    again = rpc("join_with_code", {"code": jc["code"], "display_name": NAMES["outsider"]}, tok("outsider"))
    check(again.get("error") == "wrong_code", "the code works once only")
    old = rpc("create_join_code", {"clinic_id": a}, tok("admin"))
    psql(f"update clinic.join_codes set expires_at = now() - interval '1 minute' where id = '{old['id']}'")
    check(rpc("join_with_code", {"code": old["code"], "display_name": NAMES["outsider"]}, tok("outsider")).get("error") == "wrong_code",
          "an expired code is refused")
    refused("bad_code", lambda: rpc("join_with_code", {"code": "hello", "display_name": NAMES["outsider"]}, tok("outsider")),
            "a malformed code is refused")
    check(psql("select clinic.norm_code(' il0o-ab cd ')") == "1100ABCD" and psql("select clinic.norm_code('UUUU-1234') is null") == "t",
          "typing is forgiving (case, spaces, hyphens; I and L read as 1, O as 0) and U is never a code letter")
    live = rpc("create_join_code", {"clinic_id": a}, tok("admin"))
    left = None
    for i in range(8):
        left = rpc("join_with_code", {"code": "ZZZZZZZ" + "0123456789"[i], "display_name": NAMES["outsider"]}, tok("outsider"))
    check(left.get("error") == "wrong_code" and left.get("attempts_left") == 0, "ten wrong codes use up the hour's attempts")
    refused("too_many_attempts", lambda: rpc("join_with_code", {"code": live["code"], "display_name": NAMES["outsider"]}, tok("outsider")),
            "after ten wrong codes even the right one is refused for an hour")
    check(rpc("my_clinics", {}, tok("outsider")) == [], "the outsider is still not a member")
    rpc("revoke_join_code", {"code_id": live["id"]}, tok("admin"))
    check(all(c["id"] != live["id"] for c in rpc("clinic_admin", {"clinic_id": a}, tok("admin"))["codes"]), "the admin revoked a code")
    psql(f"delete from clinic.code_attempts where user_id = '{S['uid']['outsider']}'")


@step
def expired_and_revoked_invites():
    a = S["a"]
    rpc("invite_members", {"clinic_id": a, "emails": [EMAIL["outsider"]], "role": "member"}, tok("admin"))
    iid = psql(f"select id from clinic.invites where clinic_id = '{a}' and email = '{EMAIL['outsider']}'")
    psql(f"update clinic.invites set expires_at = now() - interval '1 day' where id = '{iid}'")
    check(rpc("my_invites", {}, tok("outsider")) == [], "an expired invite isn't offered")
    refused("expired", lambda: rpc("accept_invite", {"invite_id": iid, "display_name": NAMES["outsider"]}, tok("outsider")),
            "an expired invite can't be accepted")
    again = rpc("invite_members", {"clinic_id": a, "emails": [EMAIL["outsider"]], "role": "member"}, tok("admin"))
    check(again[0]["status"] == "renewed", "inviting again renews it (resend)")
    check(len(rpc("my_invites", {}, tok("outsider"))) == 1, "the renewed invite is offered")
    refused("not_admin", lambda: rpc("revoke_invite", {"invite_id": iid}, tok("member")), "a member can't revoke invites")
    rpc("revoke_invite", {"invite_id": iid}, tok("admin"))
    refused("no_invite", lambda: rpc("accept_invite", {"invite_id": iid, "display_name": NAMES["outsider"]}, tok("outsider")),
            "a revoked invite can't be accepted")
    check(rpc("my_invites", {}, tok("outsider")) == [], "a revoked invite isn't offered")
    rpc("revoke_invite", {"invite_id": S["direct_invite"]}, tok("admin"))


@step
def usage_reporting():
    a = S["a"]
    S["d1"], S["d2"], S["d3"] = str(uuid.uuid4()), str(uuid.uuid4()), str(uuid.uuid4())
    refused("not_sharing", lambda: report("member", 0, S["d1"], v=1), "no counts accepted before the member turns on sharing")
    refused("not_member", lambda: report("outsider", 0, str(uuid.uuid4()), v=1), "an outsider can't report to the clinic")
    rpc("set_usage_sharing", {"clinic_id": a, "share": True}, tok("member"))
    rpc("set_usage_sharing", {"clinic_id": a, "share": True}, tok("coder"))
    check(rpc("my_clinics", {}, tok("member"))[0]["share_usage"] is True, "the member turned on sharing")
    report("member", 0, S["d1"], v=9, m=61.25, d=10, s=8, c=1)
    report("member", 0, S["d1"], v=12, m=95.5, d=14, s=11, c=2)
    rows = table("usage_daily", tok("member"), f"select=*&device_id=eq.{S['d1']}&day=eq.{day(0)}")
    check(len(rows) == 1 and rows[0]["visits_recorded"] == 12 and close(rows[0]["minutes_recorded"], 95.5),
          "a second report for the same device and day replaces the totals")
    report("member", -1, S["d1"], v=7, m=40, d=7, s=7)
    report("member", 1, S["d1"], v=1, m=5, d=1, s=1)
    report("member", -7, S["d1"], v=2, m=12.3, d=2, s=2)
    report("member", 0, S["d2"], platform="macos", version="0.2.1-beta.1", v=3, m=20.5, d=4, s=2)
    report("coder", -2, S["d3"], platform="ios", version="0.2.1-beta.1", v=5, m=30, d=0, s=0, c=0)
    log("  ok: reports accepted for today, yesterday, today + 1 (clinics east of UTC) and 7 days ago")
    d1 = S["d1"]
    refused("future_day", lambda: report("member", 2, d1, v=1), "a day in the future (UTC + 2 days) is refused")
    refused("too_old", lambda: report("member", -8, d1, v=1), "a day more than 7 days ago is refused")
    for field, value in (("v", 1001), ("v", -1), ("m", 1440.1), ("m", -0.5), ("d", 5001), ("s", 1001), ("c", 1001)):
        refused("out_of_range", lambda: report("member", 0, d1, **{field: value}), f"out of range: {field}={value}")
    refused("out_of_range", lambda: rpc("report_usage", {"clinic_id": a, "day": day(0), "device_id": d1, "platform": "windows",
                                                         "app_version": "0.2.0", "visits_recorded": None, "minutes_recorded": 0,
                                                         "notes_drafted": 0, "notes_signed": 0, "calls_recorded": 0}, tok("member")),
            "a missing count is refused")
    refused("bad_platform", lambda: report("member", 0, d1, platform="android"), "an unknown platform is refused")
    for bad in ("v1.0", "1.2", "1.2.3; drop table", "1.2.3-" + "x" * 40):
        refused("bad_version", lambda: report("member", 0, d1, version=bad), f"a malformed app version is refused: {bad[:20]}")
    S["b"] = rpc("create_clinic", {"name": CLINIC_B, "display_name": NAMES["outsider"]}, tok("outsider"))["id"]
    refused("not_member", lambda: report("member", 0, d1, v=1, clinic=S["b"]), "the member can't report to a clinic they aren't in")
    row = {"clinic_id": a, "user_id": S["uid"]["coder"], "device_id": str(uuid.uuid4()), "day": day(0), "platform": "linux",
           "app_version": "0.2.0", "visits_recorded": 999}
    denied(lambda: table("usage_daily", tok("member"), "select=*", "POST", row), "no direct insert into usage_daily (even as another user)")
    denied(lambda: table("usage_daily", tok("member"), f"user_id=eq.{S['uid']['member']}", "PATCH", {"visits_recorded": 999}),
           "no direct update of usage_daily")
    denied(lambda: table("usage_daily", tok("member"), f"user_id=eq.{S['uid']['member']}", "DELETE"), "no direct delete from usage_daily")
    denied(lambda: table("members", tok("outsider"), "select=*", "POST",
                         {"clinic_id": a, "user_id": S["uid"]["outsider"], "role": "admin", "display_name": "x"}),
           "no direct insert into members")
    denied(lambda: table("members", tok("member"), f"user_id=eq.{S['uid']['member']}", "PATCH", {"role": "admin"}),
           "a member can't promote themselves by writing the table")
    denied(lambda: table("invites", tok("member"), "select=*", "POST", {"clinic_id": a, "email": "x@example.org", "role": "admin"}),
           "no direct insert into invites")


@step
def member_reads():
    a = S["a"]
    rows = table("usage_daily", tok("member"))
    check(len(rows) == 5 and all(r["user_id"] == S["uid"]["member"] for r in rows), "the member reads only their own 5 usage rows")
    mem = table("members", tok("member"))
    check(len(mem) == 1 and mem[0]["user_id"] == S["uid"]["member"], "the member reads only their own membership row")
    cl = table("clinics", tok("member"), "select=id,name")
    check(cl == [{"id": a, "name": CLINIC_A}], "the member reads only their clinic's name")
    denied(lambda: table("clinics", tok("member"), "select=created_by"), "nobody reads who created a clinic")
    mine = rpc("my_usage", {"clinic_id": a, "from_day": day(-7), "to_day": day(1)}, tok("member"))
    today = next(x for x in mine if x["day"] == day(0))
    check(len(mine) == 4 and today["visits"] == 15 and close(today["minutes"], 116.0), "my_usage: the member's own counts per day")
    for f, args in (("clinic_dashboard", {"clinic_id": a, "from_day": day(-6), "to_day": day(0)}), ("clinic_admin", {"clinic_id": a}),
                    ("remove_member", {"clinic_id": a, "user_id": S["uid"]["coder"]}),
                    ("set_member_role", {"clinic_id": a, "user_id": S["uid"]["member"], "role": "admin"}),
                    ("rename_clinic", {"clinic_id": a, "name": "Mine now"}),
                    ("delete_clinic", {"clinic_id": a, "confirm_name": CLINIC_A})):
        refused("not_admin", lambda: rpc(f, args, tok("member")), f"a member can't call {f}")
    for t, q in (("usage_daily", "select=*"), ("clinics", f"select=id,name&id=eq.{a}"), ("members", f"select=*&clinic_id=eq.{a}")):
        check(table(t, tok("outsider"), q) == [], f"the outsider reads nothing of the clinic from clinic.{t}")
    for t in ("invites", "join_codes", "code_attempts"):
        denied(lambda: table(t, tok("outsider")), f"no signed-in person reads clinic.{t}")
        denied(lambda: table(t, tok("admin")), f"not even an admin reads clinic.{t} directly")
    refused("not_admin", lambda: rpc("clinic_dashboard", {"clinic_id": a, "from_day": day(-6), "to_day": day(0)}, tok("outsider")),
            "the outsider can't see the dashboard")


EXPECTED = {
    # member: d1 today 12/95.5/14/11/2, -1 7/40/7/7/0, +1 1/5/1/1/0, -7 2/12.3/2/2/0; d2 today 3/20.5/4/2/0
    # coder: d3 -2 5/30/0/0/0
    "member": (25, 173.3, 28, 23, 2),
    "coder": (5, 30.0, 0, 0, 0),
}


@step
def dashboard():
    a = S["a"]
    d = rpc("clinic_dashboard", {"clinic_id": a, "from_day": day(-7), "to_day": day(1)}, tok("admin"))
    t = d["totals"]
    want = [sum(x) for x in zip(EXPECTED["member"], EXPECTED["coder"])]
    check((t["visits"], t["drafted"], t["signed"], t["calls"]) == (want[0], want[2], want[3], want[4]) and close(t["minutes"], want[1]),
          f"totals match what was reported: {t}")
    check(t["devices"] == 3 and t["reporting"] == 2 and d["members_total"] == 4 and d["members_sharing"] == 2,
          "3 devices, 2 of 4 members sharing and reporting")
    days = {x["day"]: x for x in d["days"]}
    check(days[day(0)]["visits"] == 15 and days[day(-2)]["visits"] == 5 and days[day(1)]["visits"] == 1 and days[day(-7)]["visits"] == 2
          and len(days) == 5, "visits per day match")
    mem = {m["name"]: m for m in d["members"] if not m["former"]}
    m = mem[NAMES["member"]]
    check((m["visits"], m["drafted"], m["signed"], m["calls"]) == (25, 28, 23, 2) and close(m["minutes"], 173.3) and m["last_active"] == day(1),
          "the member's totals and last active day")
    check(mem[NAMES["coder"]]["visits"] == 5 and mem[NAMES["newcomer"]]["share_usage"] is False and mem[NAMES["admin"]]["visits"] == 0,
          "the colleague's totals; the newcomer shows as not sharing")
    check(not any(m["former"] for m in d["members"]), "no former members yet")
    plat = {p["platform"]: p for p in d["platforms"]}
    check(plat["windows"]["visits"] == 22 and plat["macos"]["visits"] == 3 and plat["ios"]["visits"] == 5
          and all(p["devices"] == 1 for p in plat.values()), "visits and devices per platform")
    ver = {v["app_version"]: v["devices"] for v in d["versions"]}
    check(ver == {"0.2.0": 1, "0.2.1-beta.1": 2}, f"devices per app version: {ver}")
    week = rpc("clinic_dashboard", {"clinic_id": a, "from_day": day(-6), "to_day": day(0)}, tok("admin"))
    check(week["totals"]["visits"] == 30 - 1 - 2, "a 7-day range leaves out the rows outside it")
    refused("bad_range", lambda: rpc("clinic_dashboard", {"clinic_id": a, "from_day": day(0), "to_day": day(-1)}, tok("admin")),
            "a reversed range is refused")
    refused("bad_range", lambda: rpc("clinic_dashboard", {"clinic_id": a, "from_day": day(-401), "to_day": day(0)}, tok("admin")),
            "a range over 400 days is refused")
    adm = rpc("clinic_admin", {"clinic_id": a}, tok("admin"))
    am = {m["name"]: m for m in adm["members"]}
    check(am[NAMES["member"]]["devices"] == 2 and sorted(am[NAMES["member"]]["platforms"]) == ["macos", "windows"]
          and sorted(am[NAMES["member"]]["app_versions"]) == ["0.2.0", "0.2.1-beta.1"] and am[NAMES["member"]]["share_usage"] is True,
          "clinic_admin: the member's devices, platforms, app versions and sharing")
    check(set(am) == {NAMES[k] for k in ("admin", "member", "newcomer", "coder")}, "clinic_admin lists the four members by display name")
    check(not any("@" in json.dumps(m) for m in adm["members"]), "members' email addresses are never returned")
    check(adm["invites_left_today"] == 50 - 8, f"the daily allowance counts invites, renewals and codes: {adm['invites_left_today']} left")


@step
def roles_and_last_admin():
    a, u = S["a"], S["uid"]
    refused("last_admin", lambda: rpc("leave_clinic", {"clinic_id": a}, tok("admin")), "the only admin can't leave")
    refused("last_admin", lambda: rpc("set_member_role", {"clinic_id": a, "user_id": u["admin"], "role": "member"}, tok("admin")),
            "the only admin can't step down")
    refused("last_admin", lambda: rpc("remove_member", {"clinic_id": a, "user_id": u["admin"]}, tok("admin")),
            "the only admin can't remove themselves")
    refused("bad_role", lambda: rpc("set_member_role", {"clinic_id": a, "user_id": u["member"], "role": "owner"}, tok("admin")),
            "an unknown role is refused")
    rpc("set_member_role", {"clinic_id": a, "user_id": u["member"], "role": "admin"}, tok("admin"))
    rpc("set_member_role", {"clinic_id": a, "user_id": u["admin"], "role": "member"}, tok("member"))
    log("  ok: the admin made the member an admin, who then made the first admin a member")
    refused("last_admin", lambda: rpc("leave_clinic", {"clinic_id": a}, tok("member")), "now the member is the only admin and can't leave")
    refused("last_admin", lambda: rpc("set_member_role", {"clinic_id": a, "user_id": u["member"], "role": "member"}, tok("member")),
            "nor step down")
    # While an admin, the member invites someone; the invite outlives the member's account (invited_by goes null).
    rpc("invite_members", {"clinic_id": a, "emails": [EXTRA[1]], "role": "member"}, tok("member"))
    rpc("set_member_role", {"clinic_id": a, "user_id": u["admin"], "role": "admin"}, tok("member"))
    rpc("set_member_role", {"clinic_id": a, "user_id": u["member"], "role": "member"}, tok("admin"))
    roles = {m["name"]: m["role"] for m in rpc("clinic_admin", {"clinic_id": a}, tok("admin"))["members"]}
    check(roles[NAMES["admin"]] == "admin" and roles[NAMES["member"]] == "member", "roles restored")
    check(rpc("my_clinics", {}, tok("admin"))[0]["sole_admin"] is True, "my_clinics says the admin is the sole admin again")
    refused("not_admin", lambda: rpc("set_member_role", {"clinic_id": a, "user_id": u["coder"], "role": "admin"}, tok("coder")),
            "a member can't change roles")
    rpc("set_display_name", {"clinic_id": a, "display_name": "Sam O."}, tok("member"))
    rpc("set_display_name", {"clinic_id": a, "display_name": NAMES["member"]}, tok("member"))
    log("  ok: a member changed their own display name (and back)")


@step
def remove_member_former():
    a = S["a"]
    rpc("remove_member", {"clinic_id": a, "user_id": S["uid"]["coder"]}, tok("admin"))
    check(rpc("my_clinics", {}, tok("coder")) == [], "the removed colleague no longer belongs to the clinic")
    refused("not_member", lambda: report("coder", 0, S["d3"], platform="ios", v=1), "a removed member can't report")
    check(len(table("usage_daily", tok("coder"))) == 1, "the removed member can still read their own past row")
    d = rpc("clinic_dashboard", {"clinic_id": a, "from_day": day(-7), "to_day": day(1)}, tok("admin"))
    former = [m for m in d["members"] if m["former"]]
    check(len(former) == 1 and former[0]["visits"] == 5 and former[0]["name"] is None and former[0]["user_id"] is None
          and former[0]["people"] == 1, "their past counts stay, as one 'Former member' row with no name or id")
    check(NAMES["coder"] not in json.dumps(d) and S["uid"]["coder"] not in json.dumps(d), "the dashboard holds no trace of who they were")
    check(d["totals"]["visits"] == 30, "clinic totals still include them")


@step
def device_cap():
    a = S["a"]
    rpc("set_usage_sharing", {"clinic_id": a, "share": True}, tok("newcomer"))
    for _ in range(10):
        report("newcomer", 0, str(uuid.uuid4()), platform="linux")
    refused("too_many_devices", lambda: report("newcomer", 0, str(uuid.uuid4()), platform="linux"),
            "an 11th device in 30 days is refused")
    rpc("set_usage_sharing", {"clinic_id": a, "share": False}, tok("newcomer"))
    refused("not_sharing", lambda: report("newcomer", 0, str(uuid.uuid4())), "turning sharing off stops reports")


@step
def account_deletion():
    a, b, u = S["a"], S["b"], S["uid"]
    refused("confirmation_mismatch", lambda: function("delete-account", {"confirm": "someone@example.org"}, tok("member")),
            "delete-account wants the account's own email")
    function("delete-account", {"confirm": EMAIL["member"]}, tok("member"))
    log("  ok: the member deleted their account through the delete-account function")
    check(psql(f"select count(*) from clinic.members where user_id = '{u['member']}'") == "0", "their membership went with the account")
    check(psql(f"select count(*) from clinic.usage_daily where user_id = '{u['member']}'") == "0", "their usage rows went with the account")
    check(psql(f"select count(*) from clinic.clinics where id = '{a}'") == "1", "the clinic stays (it has another admin)")
    check(psql(f"select count(*) from clinic.invites where email = '{EXTRA[1]}' and invited_by is null") == "1",
          "an invite they sent stays, with no sender")
    d = rpc("clinic_dashboard", {"clinic_id": a, "from_day": day(-7), "to_day": day(1)}, tok("admin"))
    check(d["totals"]["visits"] == 5 and d["members_total"] == 2, "the dashboard no longer counts them")
    # The outsider is the only admin of clinic B; the newcomer joins it and reports there.
    rpc("invite_members", {"clinic_id": b, "emails": [EMAIL["newcomer"]], "role": "member"}, tok("outsider"))
    inv = next(i for i in rpc("my_invites", {}, tok("newcomer")) if i["clinic_id"] == b)
    rpc("accept_invite", {"invite_id": inv["id"], "display_name": NAMES["newcomer"]}, tok("newcomer"))
    rpc("set_usage_sharing", {"clinic_id": b, "share": True}, tok("newcomer"))
    report("newcomer", 0, str(uuid.uuid4()), platform="linux", v=4, clinic=b)
    rpc("invite_members", {"clinic_id": b, "emails": [EXTRA[2]], "role": "member"}, tok("outsider"))
    rpc("create_join_code", {"clinic_id": b}, tok("outsider"))
    check(rpc("my_clinics", {}, tok("outsider"))[0]["sole_admin"] is True, "the outsider is clinic B's only admin (the portal warns them)")
    function("delete-account", {"confirm": EMAIL["outsider"]}, tok("outsider"))
    log("  ok: the last admin of clinic B deleted their account")
    for t in ("clinics", "members", "usage_daily", "invites", "join_codes"):
        col = "id" if t == "clinics" else "clinic_id"
        check(psql(f"select count(*) from clinic.{t} where {col} = '{b}'") == "0", f"clinic B went with its last admin: no rows in {t}")
    check([c["id"] for c in rpc("my_clinics", {}, tok("newcomer"))] == [a], "the newcomer is left with clinic A only")


@step
def purge():
    a, u = S["a"], S["uid"]
    psql(f"insert into clinic.usage_daily (clinic_id, user_id, device_id, day, platform, app_version) values "
         f"('{a}', '{u['admin']}', gen_random_uuid(), current_date - 401, 'linux', '0.1.0'), "
         f"('{a}', '{u['admin']}', gen_random_uuid(), current_date - 399, 'linux', '0.1.0')")
    psql(f"update clinic.invites set expires_at = now() - interval '31 days' where email = '{EXTRA[1]}'")
    out = json.loads(psql("select clinic.purge()"))
    check(out["usage"] == 1 and out["invites"] == 1, f"purge removed the 401-day-old row and the long-expired invite: {out}")
    check(psql(f"select count(*) from clinic.usage_daily where user_id = '{u['admin']}' and day = current_date - 399") == "1",
          "a 399-day-old row stays")
    if psql("select count(*) from pg_extension where extname = 'pg_cron'") == "1":
        check(psql("select schedule from cron.job where jobname = 'openbark-clinic-purge'") == "41 3 * * *", "pg_cron runs the purge daily")
    else:
        log("  note: pg_cron isn't installed in this database; the migration skipped the schedule")
    denied(lambda: rpc("purge", {}, tok("admin")), "a signed-in person can't run the purge")


ALLOWED_TEXT = {
    "clinics.name", "members.display_name", "members.role", "invites.email", "invites.role",
    "usage_daily.platform", "usage_daily.app_version", "join_codes.code_hash",
}


@step
def only_names_as_text():
    cols = set(psql("select table_name || '.' || column_name from information_schema.columns where table_schema = 'clinic' "
                    "and data_type not in ('uuid', 'date', 'integer', 'bigint', 'smallint', 'numeric', 'boolean', "
                    "'timestamp with time zone') order by 1").split())
    check(cols == ALLOWED_TEXT, f"the only non-numeric columns are names, the invite email, the hash and constrained enums: {sorted(cols)}")
    names = set(psql("select name from clinic.clinics").splitlines())
    check(names <= {CLINIC_A, CLINIC_B}, f"clinic names are only the clinic names typed: {sorted(names)}")
    ours = set(psql(f"select name from clinic.clinics where created_by = '{S['uid']['admin']}'").splitlines())
    check(ours == {CLINIC_A}, "this run's clinic name is the only text in clinics")
    dn = set(psql(f"select display_name from clinic.members where clinic_id = '{S['a']}'").splitlines())
    check(dn <= set(NAMES.values()), f"display names are only the names people chose: {sorted(dn)}")
    emails = set(psql(f"select email from clinic.invites where clinic_id = '{S['a']}'").splitlines()) - {""}
    check(emails <= set(EMAIL.values()) | set(EXTRA), "invite emails are only addresses an admin typed")
    enums = psql(f"select string_agg(distinct platform || ' ' || app_version, ',') from clinic.usage_daily where clinic_id = '{S['a']}'")
    check(all(re.match(r"^(windows|macos|linux|ios|web) \d+\.\d+\.\d+([-+][0-9A-Za-z.-]+)?$", x) for x in enums.split(",")),
          "usage rows hold only platform names and version numbers as text")


def main():
    log(f"clinic e2e against {SB} (run {TAG})")
    for f in STEPS:
        log(f"== {f.__name__}")
        try:
            f()
        except Exception as e:  # keep going: later steps report independently where they can
            FAILED.append(f.__name__)
            log(f"  FAIL: {f.__name__}: {e}")
    if FAILED:
        log(f"FAILED steps: {', '.join(FAILED)}")
        sys.exit(1)
    log(f"all {len(STEPS)} steps passed")


if __name__ == "__main__":
    main()
