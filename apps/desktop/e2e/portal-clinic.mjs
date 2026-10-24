// The account portal's clinic pages (ADR-0028) against a LOCAL Supabase.
//
//   bash scripts/sync-local-supabase.sh start
//   eval "$(bash scripts/sync-local-supabase.sh env)"
//   python3 -m http.server -d website/dist 8099 &
//   cd apps/desktop && node e2e/portal-clinic.mjs
//
// The page is website/dist/portal.html exactly as published, served with the
// production Content-Security-Policy. It talks to the hosted project's
// address; this script answers every one of those requests by forwarding it
// to the local Supabase (swapping the publishable key for the local one), so
// nothing ever reaches the hosted project. Three people use three browser
// contexts: an admin who creates a clinic, invites by email and makes a join
// code; a colleague who accepts the invite; another who joins with the code.
// Counts are reported as the app will (phase 2), straight to report_usage,
// and the dashboard must show them. Synthetic accounts only.
import { mkdirSync, readFileSync } from "node:fs";
import { chromium } from "@playwright/test";

const base = process.env.SITE_URL ?? "http://localhost:8099";
const SB = process.env.OPENBARK_SYNC_TEST_URL ?? "http://127.0.0.1:54321";
const KEY = process.env.OPENBARK_SYNC_TEST_KEY;
const MAIL = process.env.OPENBARK_SYNC_TEST_MAIL ?? "http://127.0.0.1:54324";
const out = process.env.OUT_DIR ?? "../../target/portal-clinic";
if (!/^http:\/\/(127\.0\.0\.1|localhost)(:\d+)?$/.test(SB)) throw new Error(`refusing to run against ${SB}: local Supabase only`);
if (!KEY) throw new Error("OPENBARK_SYNC_TEST_KEY isn't set (eval scripts/sync-local-supabase.sh env)");
mkdirSync(out, { recursive: true });

const HOSTED = "https://nmecovtmijnfnjuadicp.supabase.co";
const SITE_ORIGIN = new URL(base).origin;
const CSP = JSON.parse(readFileSync(new URL("../../../website/vercel.json", import.meta.url), "utf8"))
  .headers.find((h) => h.source === "/(.*)")
  .headers.find((h) => h.key === "Content-Security-Policy").value;
const tag = Math.random().toString(16).slice(2, 10);
const email = (who) => `portal-e2e-${tag}-${who}@example.org`;
const CLINIC = "Lakeside Veterinary Clinic";
const problems = [];
const log = (...a) => console.log(new Date().toISOString().slice(11, 19), ...a);
const check = (cond, what) => {
  if (cond) log(`  ok: ${what}`);
  else { problems.push(what); log(`  FAIL: ${what}`); }
};

async function codeFor(address, used) {
  for (let i = 0; i < 60; i++) {
    const q = encodeURIComponent(`to:${address}`);
    const msgs = (await (await fetch(`${MAIL}/api/v1/search?query=${q}`)).json()).messages ?? [];
    for (const m of msgs) {
      if (used.has(m.ID) || !/sign-in code/i.test(m.Subject ?? "")) continue;
      const text = (await (await fetch(`${MAIL}/api/v1/message/${m.ID}`)).json()).Text ?? "";
      const c = text.match(/\b(\d{6})\b/);
      if (c) { used.add(m.ID); return c[1]; }
    }
    await new Promise((r) => setTimeout(r, 500));
  }
  throw new Error(`no sign-in code for ${address}`);
}

async function rpc(name, args, token) {
  const r = await fetch(`${SB}/rest/v1/rpc/${name}`, {
    method: "POST",
    headers: { apikey: KEY, Authorization: `Bearer ${token}`, "Content-Profile": "clinic", "Content-Type": "application/json" },
    body: JSON.stringify(args),
  });
  const body = await r.text();
  if (!r.ok) throw new Error(`${name}: ${r.status} ${body}`);
  return body ? JSON.parse(body) : null;
}

const browser = await chromium.launch();
const used = new Set();

async function person(who, viewport = { width: 1440, height: 1000 }) {
  const ctx = await browser.newContext({ viewport, acceptDownloads: true });
  await ctx.route(`${base}/**`, async (route) => {
    const res = await route.fetch();
    await route.fulfill({ response: res, headers: { ...res.headers(), "content-security-policy": CSP } });
  });
  // Everything meant for the hosted project goes to the local one instead.
  await ctx.route(`${HOSTED}/**`, async (route) => {
    const req = route.request();
    const cors = {
      "access-control-allow-origin": SITE_ORIGIN,
      "access-control-allow-headers": req.headers()["access-control-request-headers"] || "authorization, apikey, content-type",
      "access-control-allow-methods": "GET, POST, PATCH, DELETE, OPTIONS",
      "access-control-expose-headers": "content-range, x-supabase-api-version",
    };
    if (req.method() === "OPTIONS") return route.fulfill({ status: 204, headers: cors });
    const headers = { ...req.headers() };
    delete headers.origin; // the Edge Functions allow calls without an Origin; the token authorises
    delete headers.referer;
    headers.apikey = KEY;
    if (!headers.authorization || !/^Bearer eyJ/.test(headers.authorization)) delete headers.authorization;
    const url = SB + req.url().slice(HOSTED.length);
    try {
      const res = await route.fetch({ url, headers });
      await route.fulfill({ response: res, headers: { ...res.headers(), ...cors } });
    } catch (e) {
      problems.push(`${who}: forwarding ${req.method()} ${url} failed: ${e}`);
      await route.abort();
    }
  });
  await ctx.addInitScript(() => {
    window.__csp = [];
    document.addEventListener("securitypolicyviolation", (e) => window.__csp.push(`${e.violatedDirective} ${e.blockedURI}`));
  });
  const page = await ctx.newPage();
  page.on("pageerror", (e) => problems.push(`${who}: ${e}`));
  page.on("console", (m) => m.type() === "error" && !/Failed to load resource/.test(m.text()) && problems.push(`${who}: console: ${m.text()}`));
  await page.goto(`${base}/portal.html`);
  await page.locator("#email").fill(email(who));
  await page.locator('#email-form button[type="submit"]').click();
  await page.locator("#code-form").waitFor({ state: "visible" });
  await page.locator("#code").fill(await codeFor(email(who), used));
  await page.locator('#code-form button[type="submit"]').click();
  await page.locator("#view-signed-in").waitFor({ state: "visible" });
  log(`  ok: ${who} signed in on the portal with an emailed code`);
  return { ctx, page };
}
const tokenOf = async (page) =>
  page.evaluate(() => JSON.parse(localStorage.getItem("sb-nmecovtmijnfnjuadicp-auth-token") || "{}").access_token);
const errorShown = async (page) =>
  page.evaluate(() => [...document.querySelectorAll(".pt-msg[data-tone=error]")].map((m) => `${m.id}: ${m.textContent}`).join("; "));

try {
  log("== sign in");
  const admin = await person("admin");
  const member = await person("member");

  log("== the admin creates a clinic");
  const A = admin.page;
  await A.locator("#create-open").click();
  await A.locator("#create-name").fill(CLINIC);
  await A.locator("#create-you").fill("Dr Morgan Lee");
  await A.locator('#create-form button[type="submit"]').click();
  await A.locator("#clinic-dash").waitFor({ state: "visible" });
  check((await A.locator("#clinic-h").textContent()) === CLINIC, "the new clinic's page shows its name");
  check((await A.locator("#tab-invite").getAttribute("aria-selected")) === "true", "a new clinic opens on Invite people");

  log("== the admin invites by email");
  await A.locator("#invite-emails").fill(`${email("member")}\n${email("newcomer")}, not-an-address`);
  await A.locator('#invite-form button[type="submit"]').click();
  await A.locator("#invite-results li").nth(2).waitFor();
  const results = await A.locator("#invite-results").innerText();
  check(/Invitation emailed/.test(results) && /they'll see the invite when they sign in/.test(results) && /Not an email address/.test(results),
    "results say who was emailed, who will see it at sign-in, and what wasn't an address");
  await A.locator("#invites-table tbody tr").nth(1).waitFor();
  check((await A.locator("#invites-table tbody tr").count()) === 2, "two invites wait to be accepted");
  await A.screenshot({ path: `${out}/admin-invite.png`, fullPage: true });

  log("== the admin makes a join code");
  await A.locator("#code-make").click();
  await A.locator("#code-shown").waitFor({ state: "visible" });
  const code = (await A.locator("#code-value").textContent()).trim();
  check(/^[0-9A-HJKMNP-TV-Z]{4}-[0-9A-HJKMNP-TV-Z]{4}$/.test(code), `a join code is shown once: ${code}`);
  await A.locator("#codes-table tbody tr").first().waitFor();

  log("== the colleague accepts the invite");
  const B = member.page;
  await B.reload();
  await B.locator("#clinic-invited").waitFor({ state: "visible" });
  check(new RegExp(CLINIC).test(await B.locator("#clinic-invite-list").innerText()), "the invite names the clinic");
  await B.locator("#inv-name-0").fill("Riley Chen");
  await B.locator("#clinic-invite-list .btn-primary").click();
  await B.locator("#clinic-dash").waitFor({ state: "visible" });
  await B.waitForFunction(() => /member/.test(document.getElementById("clinic-sub").textContent));
  check((await B.locator("#clinic-admin").isHidden()) && /not sharing/.test(await B.locator("#mine-sharing").textContent()),
    "a member sees their clinic, no admin tabs, and that they aren't sharing");
  await B.screenshot({ path: `${out}/member.png`, fullPage: true });

  log("== another colleague joins with the code");
  const coder = await person("coder");
  const C = coder.page;
  await C.locator("#join-open").click();
  await C.locator("#join-code").fill(code.toLowerCase().replace("-", " "));
  await C.locator("#join-you").fill("Sam Patel");
  await C.locator('#join-form button[type="submit"]').click();
  await C.locator("#clinic-dash").waitFor({ state: "visible" });
  check(/You joined/.test(await C.locator("#create-msg").textContent()), "joining with the code works from the portal");

  log("== counts arrive (as the app will send them)");
  const bToken = await tokenOf(B);
  const clinicId = (await rpc("my_clinics", {}, bToken))[0].id;
  await rpc("set_usage_sharing", { clinic_id: clinicId, share: true }, bToken);
  const day = (n) => { const d = new Date(); d.setDate(d.getDate() + n); return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`; };
  const device = crypto.randomUUID();
  const send = (d, v, m, dr, s, c) => rpc("report_usage", { clinic_id: clinicId, day: day(d), device_id: device, platform: "windows", app_version: "0.2.0",
    visits_recorded: v, minutes_recorded: m, notes_drafted: dr, notes_signed: s, calls_recorded: c }, bToken);
  await send(0, 8, 64.5, 9, 7, 1);
  await send(-1, 4, 30, 4, 4, 0);
  log("  ok: the colleague turned on sharing and two days of counts were reported");

  log("== the dashboard");
  await A.reload();
  await A.locator("#clinic-dash").waitFor({ state: "visible" });
  await A.waitForFunction(() => document.querySelectorAll("#usage-svg rect").length === 30 && /12 visits/.test(document.getElementById("usage-summary").textContent), null, { timeout: 15000 });
  const tiles = await A.locator("#usage-stats dd").allTextContents();
  check(tiles[0] === "12" && tiles[1] === "1.6" && tiles[2] === "13" && tiles[3] === "11" && tiles[4] === "1", `the tiles show the reported counts: ${tiles.join(", ")}`);
  check(/1 of 3 members shared counts from 1 device/.test(await A.locator("#usage-summary").textContent()), "the summary says who shared");
  const byMember = await A.locator("#usage-members").innerText();
  check(/Riley Chen\s+12/.test(byMember) && /Sam Patel\s+Not sharing/.test(byMember), "by member: the colleague's 12 visits; the other isn't sharing");
  const bars = await A.locator("#usage-svg rect:not(.cl-zero)").count();
  check(bars === 2, `two days have bars (${bars})`);
  check(/Windows\s+1\s+12/.test(await A.locator("#usage-platforms").innerText()), "by platform: Windows, one device, 12 visits");
  await A.screenshot({ path: `${out}/admin-usage.png`, fullPage: true });
  await A.locator('input[name="clinic-range"][value="7"]').check();
  await A.waitForFunction(() => document.querySelectorAll("#usage-svg rect").length === 7);
  check(true, "the 7-day period redraws with 7 bars");

  log("== CSV");
  const [download] = await Promise.all([A.waitForEvent("download"), A.locator("#usage-csv").click()]);
  const csv = readFileSync(await download.path(), "utf8");
  check(/^OpenBark usage counts,Lakeside Veterinary Clinic/.test(csv) && /Riley Chen,member,yes,12,94\.5,13,11,1,2,/.test(csv) && !/@/.test(csv),
    "the CSV has the counts and names, and no email address");

  log("== members");
  await A.locator("#tab-members").click();
  await A.locator("#members-table tbody tr").nth(2).waitFor();
  check((await A.locator("#members-table tbody tr").count()) === 3, "three members are listed");
  const riley = A.locator("#members-table tbody tr", { hasText: "Riley Chen" });
  await riley.getByRole("button", { name: "Make admin" }).click();
  await A.locator("#members-msg[data-tone=ok]").waitFor();
  await A.locator("#members-table tbody tr", { hasText: "Riley Chen" }).getByRole("button", { name: "Make member" }).click();
  await A.waitForFunction(() => /is now a member/.test(document.getElementById("members-msg").textContent));
  check(true, "an admin can make someone an admin and back");
  await A.locator("#members-table tbody tr", { hasText: "Sam Patel" }).getByRole("button", { name: "Remove" }).click();
  await A.getByRole("button", { name: "Remove Sam Patel" }).click();
  await A.waitForFunction(() => document.querySelectorAll("#members-table tbody tr").length === 2);
  check(true, "an admin removed a member");
  await A.screenshot({ path: `${out}/admin-members.png`, fullPage: true });

  log("== pending invites");
  await A.locator("#tab-invite").click();
  const row = A.locator("#invites-table tbody tr", { hasText: email("newcomer") });
  await row.getByRole("button", { name: "Revoke" }).click();
  await A.waitForFunction(() => /was revoked/.test(document.getElementById("invites-msg").textContent));
  check((await A.locator("#invites-table tbody tr", { hasText: email("newcomer") }).count()) === 0, "an admin revoked a pending invite");

  log("== settings");
  await A.locator("#tab-settings").click();
  await A.locator("#rename-name").fill("Lakeside Veterinary Hospital");
  await A.locator('#rename-form button[type="submit"]').click();
  await A.waitForFunction(() => document.getElementById("clinic-h").textContent === "Lakeside Veterinary Hospital");
  check(true, "an admin renamed the clinic");

  log("== warnings and leaving");
  await A.locator("#delete-open").click();
  check(/only admin of Lakeside Veterinary Hospital \(2 members\)/.test(await A.locator("#delete-clinic-warn").textContent()),
    "before deleting their account, the only admin is told the clinic would go");
  await A.locator("#delete-cancel").click();
  await B.reload();
  await B.locator("#clinic-dash").waitFor({ state: "visible" });
  await B.locator("#leave-open").click();
  await B.locator('#leave-form button[type="submit"]').click();
  await B.waitForFunction(() => /You left/.test(document.getElementById("create-msg").textContent));
  check(await B.locator("#clinic-dash").isHidden(), "a member left the clinic");

  log("== the admin deletes the clinic");
  await A.locator("#clinic-delete-open").click();
  await A.locator("#clinic-delete-name").fill("Lakeside Veterinary Hospital");
  await A.locator("#clinic-delete-submit").click();
  await A.waitForFunction(() => /was deleted/.test(document.getElementById("create-msg").textContent));
  check(await A.locator("#clinic-dash").isHidden(), "the clinic is gone");

  for (const [who, p] of [["admin", A], ["member", B], ["coder", C]]) {
    const err = await errorShown(p);
    check(!err, `${who}: no error messages (${err || "none"})`);
    const csp = await p.evaluate(() => window.__csp.join("; "));
    check(!csp, `${who}: no CSP violations (${csp || "none"})`);
  }

  log("== phone width");
  const phone = await person("phone", { width: 390, height: 844 });
  await phone.page.locator("#create-open").click();
  await phone.page.locator("#create-name").fill("Small Animal Clinic");
  await phone.page.locator("#create-you").fill("Dr Ash Kim");
  await phone.page.locator('#create-form button[type="submit"]').click();
  await phone.page.locator("#clinic-dash").waitFor({ state: "visible" });
  await phone.page.locator("#tab-usage").click();
  await phone.page.waitForTimeout(500);
  check(!(await phone.page.evaluate(() => document.documentElement.scrollWidth > innerWidth)), "no horizontal scroll at phone width");
  await phone.page.screenshot({ path: `${out}/phone-usage.png`, fullPage: true });
} catch (e) {
  problems.push(`stopped: ${e.stack || e}`);
}
await browser.close();
console.log(problems.length ? `PROBLEMS:\n${problems.join("\n")}` : "all portal clinic checks passed");
process.exitCode = problems.length ? 1 : 0;
