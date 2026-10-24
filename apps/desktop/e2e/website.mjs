// Screenshots and layout checks of the public website (website/dist served locally):
//   python -m http.server -d website/dist 8099 &  node e2e/website.mjs
// Every page is served with the Content-Security-Policy from website/vercel.json,
// as Vercel serves it, so a blocked script or connection fails the run.
// The account portal is checked signed out, then through a mocked sign-in:
// every call to Supabase is answered here or refused. Nothing reaches the live
// project and no real account is used.
import { mkdirSync, readFileSync } from "node:fs";
import { chromium } from "@playwright/test";

const base = process.env.SITE_URL ?? "http://localhost:8099";
// Sign in links appear only once the portal is switched on (PORTAL_LINKED in scripts/website-build.py).
const portalLinked = /^PORTAL_LINKED = True/m.test(readFileSync(new URL("../../../scripts/website-build.py", import.meta.url), "utf8"));
const out = process.env.OUT_DIR ?? "../../target/website";
mkdirSync(out, { recursive: true });
const problems = [];

// The CSP Vercel sends (website/vercel.json; the root vercel.json must match).
const cspOf = (file) =>
  JSON.parse(readFileSync(new URL(file, import.meta.url), "utf8"))
    .headers.find((h) => h.source === "/(.*)")
    .headers.find((h) => h.key === "Content-Security-Policy").value;
const CSP = cspOf("../../../website/vercel.json");
if (cspOf("../../../vercel.json") !== CSP) problems.push("CSP: vercel.json and website/vercel.json differ");
const directive = (name) => (CSP.split(";").map((d) => d.trim().split(/\s+/)).find((d) => d[0] === name) ?? []).slice(1);
const SUPABASE = "https://nmecovtmijnfnjuadicp.supabase.co";
if (!directive("connect-src").includes(SUPABASE)) problems.push("CSP: connect-src doesn't allow the Supabase project");
if (directive("connect-src").some((s) => s.startsWith("wss:"))) problems.push("CSP: connect-src allows websockets; the portal uses none");
const portalHtml = readFileSync(new URL("../../../website/dist/portal.html", import.meta.url), "utf8");
const sdk = portalHtml.match(/<script src="(https:[^"]+)" integrity="(sha384-[^"]+)" crossorigin="anonymous"/);
if (!sdk) problems.push("portal: supabase-js isn't loaded from a pinned URL with SRI");
else if (!directive("script-src").includes(sdk[1])) problems.push(`CSP: script-src doesn't allow ${sdk[1]}`);
if (!/@supabase\/supabase-js@\d+\.\d+\.\d+\//.test(sdk?.[1] ?? "")) problems.push("portal: supabase-js URL isn't pinned to an exact version");
// No secret ever ships: the portal holds only the publishable key.
for (const f of ["portal.html", "portal.js"]) {
  const text = readFileSync(new URL(`../../../website/dist/${f}`, import.meta.url), "utf8");
  if (/service_role|sb_secret_|eyJ[A-Za-z0-9_-]{20,}\.[A-Za-z0-9_-]{20,}/.test(text)) problems.push(`${f}: looks like it contains a secret key`);
}

// A made-up account for the mocked sign-in.
const USER = { id: "0b7e2f4a-5c1d-4e8f-9a3b-6d2c1f0e9a87", email: "jordan.lee@example.com" };

// A made-up clinic (ADR-0028) the account is the only admin of, with three
// colleagues, a pending invite to another clinic, and daily counts: the
// mocked clinic functions below answer with these.
const CLINIC = { id: "6a1d0c3e-9b2f-4e7a-8c5d-2f1e0d9c8b7a", name: "Riverside Animal Hospital" };
const iso = (d) => d.toISOString().slice(0, 10);
const clinicRpc = (name, args) => {
  const members = [
    { user_id: USER.id, name: "Jordan Lee", role: "admin", joined_at: "2026-10-01T15:00:00Z", share_usage: true, last_active: iso(new Date()), devices: 2, platforms: ["macos", "ios"], app_versions: ["0.2.0"] },
    { user_id: "1c2d3e4f-0000-4000-8000-000000000001", name: "Dr Samira Okafor", role: "member", joined_at: "2026-10-02T15:00:00Z", share_usage: true, last_active: iso(new Date()), devices: 1, platforms: ["windows"], app_versions: ["0.2.1-beta.1"] },
    { user_id: "1c2d3e4f-0000-4000-8000-000000000002", name: "Casey Tran", role: "member", joined_at: "2026-10-03T15:00:00Z", share_usage: false, last_active: null, devices: 0, platforms: [], app_versions: [] },
    { user_id: "1c2d3e4f-0000-4000-8000-000000000003", name: "Alex Rivera", role: "admin", joined_at: "2026-10-04T15:00:00Z", share_usage: true, last_active: null, devices: 0, platforms: [], app_versions: [] },
  ];
  if (name === "my_clinics") return [{ id: CLINIC.id, name: CLINIC.name, role: "admin", display_name: "Jordan Lee", share_usage: true, joined_at: "2026-10-01T15:00:00Z", members: 4, sole_admin: true }];
  if (name === "my_invites") return [{ id: "9e8d7c6b-0000-4000-8000-0000000000aa", clinic_id: "9e8d7c6b-0000-4000-8000-0000000000bb", clinic_name: "Hillside Vets", role: "member", expires_at: "2026-11-07T15:00:00Z", invited_by: "Dr Pat Kim", member: false }];
  if (name === "my_usage") return [{ day: iso(new Date()), visits: 6, minutes: 75.5, drafted: 7, signed: 6, calls: 1 }];
  if (name === "clinic_admin") return {
    clinic: { id: CLINIC.id, name: CLINIC.name, created_at: "2026-10-01T15:00:00Z" }, me: USER.id, invites_left_today: 47, members,
    invites: [{ id: "5b4a3c2d-0000-4000-8000-0000000000cc", email: "a.very.long.address.for.layout@riverside-animal-hospital.example.com", role: "member", created_at: "2026-10-20T15:00:00Z", last_sent_at: "2026-10-20T15:00:00Z", expires_at: "2026-11-03T15:00:00Z", expired: false, sent_count: 1 }],
    codes: [{ id: "7f6e5d4c-0000-4000-8000-0000000000dd", created_at: "2026-10-22T15:00:00Z", expires_at: "2026-11-05T15:00:00Z" }],
  };
  if (name === "clinic_dashboard") {
    const days = [];
    for (let d = new Date(`${args.from_day}T12:00:00Z`); iso(d) <= args.to_day; d.setUTCDate(d.getUTCDate() + 1)) {
      const n = d.getUTCDate() % 7;
      if (n) days.push({ day: iso(d), visits: n * 3, minutes: n * 31.5, drafted: n * 3 + 1, signed: n * 3, calls: n % 2 });
    }
    const sum = (k) => days.reduce((s, x) => s + x[k], 0);
    const visits = sum("visits");
    return {
      clinic: CLINIC, from_day: args.from_day, to_day: args.to_day, members_total: 4, members_sharing: 3,
      totals: { visits, minutes: sum("minutes"), drafted: sum("drafted"), signed: sum("signed"), calls: sum("calls"), devices: 3, reporting: 2 },
      days,
      members: [
        { user_id: USER.id, name: "Jordan Lee", role: "admin", former: false, share_usage: true, visits: Math.round(visits / 2), minutes: 300.5, drafted: 40, signed: 38, calls: 3, days_active: 12, last_active: args.to_day },
        { user_id: members[1].user_id, name: "Dr Samira Okafor", role: "member", former: false, share_usage: true, visits: visits - Math.round(visits / 2) - 4, minutes: 250, drafted: 30, signed: 29, calls: 1, days_active: 10, last_active: args.to_day },
        { user_id: members[2].user_id, name: "Casey Tran", role: "member", former: false, share_usage: false, visits: 0, minutes: 0, drafted: 0, signed: 0, calls: 0, days_active: 0, last_active: null },
        { user_id: members[3].user_id, name: "Alex Rivera", role: "admin", former: false, share_usage: true, visits: 0, minutes: 0, drafted: 0, signed: 0, calls: 0, days_active: 0, last_active: null },
        { user_id: null, name: null, role: null, former: true, share_usage: null, people: 1, visits: 4, minutes: 20, drafted: 4, signed: 4, calls: 0, days_active: 2, last_active: args.from_day },
      ],
      platforms: [{ platform: "macos", devices: 1, visits: 40, minutes: 400 }, { platform: "windows", devices: 1, visits: 30, minutes: 300 }, { platform: "ios", devices: 1, visits: 10, minutes: 100 }],
      versions: [{ app_version: "0.2.1-beta.1", devices: 2 }, { app_version: "0.2.0", devices: 1 }],
    };
  }
  return undefined;
};
const b64 = (o) => Buffer.from(JSON.stringify(o)).toString("base64url");
const session = () => {
  const now = Math.floor(Date.now() / 1000);
  const claims = { sub: USER.id, email: USER.email, role: "authenticated", aud: "authenticated", iat: now, exp: now + 3600, session_id: "5f0c9a1e-2b7d-4c3a-8e6f-1a2b3c4d5e6f" };
  return {
    access_token: `${b64({ alg: "ES256", typ: "JWT", kid: "e2e" })}.${b64(claims)}.ZTJlLW5vdC1zaWduZWQ`,
    token_type: "bearer",
    expires_in: 3600,
    expires_at: now + 3600,
    refresh_token: "e2e-refresh",
    user: { id: USER.id, aud: "authenticated", role: "authenticated", email: USER.email, app_metadata: { provider: "email" }, user_metadata: {}, created_at: new Date().toISOString() },
  };
};

const browser = await chromium.launch();
for (const v of [
  { name: "desktop-light", viewport: { width: 1440, height: 900 }, colorScheme: "light" },
  { name: "desktop-dark", viewport: { width: 1440, height: 900 }, colorScheme: "dark" },
  { name: "phone-light", viewport: { width: 390, height: 844 }, colorScheme: "light", isMobile: true, hasTouch: true },
]) {
  const ctx = await browser.newContext({ viewport: v.viewport, colorScheme: v.colorScheme, isMobile: v.isMobile, hasTouch: v.hasTouch });
  // Serve every page with the production CSP, and collect any violation.
  await ctx.route(`${base}/**`, async (route) => {
    const res = await route.fetch();
    await route.fulfill({ response: res, headers: { ...res.headers(), "content-security-policy": CSP } });
  });
  await ctx.addInitScript(() => {
    window.__csp = [];
    document.addEventListener("securitypolicyviolation", (e) => window.__csp.push(`${e.violatedDirective} ${e.blockedURI}`));
  });
  // Supabase: refused unless the mocked sign-in below is running.
  const mock = { on: false, profile: null, deleted: null, calls: [] };
  await ctx.route(`${SUPABASE}/**`, async (route) => {
    const req = route.request();
    const url = new URL(req.url());
    const headers = { "access-control-allow-origin": "*", "access-control-allow-headers": "*", "access-control-allow-methods": "GET, POST, PATCH, DELETE, OPTIONS", "content-type": "application/json" };
    const reply = (status, body) => route.fulfill({ status, headers, body: body === undefined ? "" : JSON.stringify(body) });
    if (req.method() === "OPTIONS") return reply(204);
    mock.calls.push(`${req.method()} ${url.pathname}`);
    if (!mock.on) {
      problems.push(`${v.name}: unexpected call to Supabase: ${req.method()} ${url.pathname}`);
      return route.abort();
    }
    if (url.pathname === "/auth/v1/otp" && req.method() === "POST") return reply(200, {});
    if (url.pathname === "/auth/v1/verify" && req.method() === "POST") return reply(200, session());
    if (url.pathname === "/rest/v1/profiles" && req.method() === "GET") return reply(200, mock.profile ? [mock.profile] : []);
    if (url.pathname === "/rest/v1/profiles" && req.method() === "POST") { mock.profile = JSON.parse(req.postData() || "{}"); return reply(201); }
    if (url.pathname === "/rest/v1/profiles" && req.method() === "PATCH") { mock.profile = { ...mock.profile, ...JSON.parse(req.postData() || "{}") }; return reply(204); }
    // Clinics: only the read functions the signed-in page calls on its own.
    if (url.pathname.startsWith("/rest/v1/rpc/") && req.method() === "POST" && req.headers()["content-profile"] === "clinic") {
      const answer = clinicRpc(url.pathname.slice("/rest/v1/rpc/".length), JSON.parse(req.postData() || "{}"));
      if (answer !== undefined) return reply(200, answer);
    }
    if (url.pathname === "/functions/v1/delete-account" && req.method() === "POST") { mock.deleted = JSON.parse(req.postData() || "{}"); return reply(200, { deleted: true }); }
    if (url.pathname === "/auth/v1/logout") return reply(204);
    problems.push(`${v.name}: Supabase call the mock doesn't answer: ${req.method()} ${url.pathname}`);
    return route.abort();
  });
  const page = await ctx.newPage();
  page.on("pageerror", (e) => problems.push(`${v.name}: ${e}`));
  page.on("console", (m) => m.type() === "error" && problems.push(`${v.name}: ${m.text()}`));
  for (const p of ["index.html", "features.html", "history.html", "portal.html", "404.html"]) {
    await page.goto(`${base}/${p}`);
    await page.waitForLoadState("networkidle");
    await page.waitForTimeout(1200);
    const overflow = await page.evaluate(() => document.documentElement.scrollWidth > window.innerWidth);
    if (overflow) problems.push(`${v.name} ${p}: horizontal scroll`);
    await page.screenshot({ path: `${out}/${v.name}-${p.replace(".html", "")}.png` });
    // Brand guide checks: the lockup links home, the favicon is the brand one,
    // primary buttons are ink (not accent), and nothing is glass or gradient.
    const brand = await page.evaluate(({ linked, onPortal }) => {
      const issues = [];
      const home = document.querySelector('.site-head a.brand[href="index.html"] svg.lockup');
      if (!home) issues.push("no lockup linking home");
      const icon = document.querySelector('link[rel="icon"]')?.getAttribute("href");
      if (icon !== "favicon.svg") issues.push(`favicon is ${icon}`);
      const primary = document.querySelector(".btn-primary");
      const ink = getComputedStyle(document.documentElement).getPropertyValue("--primary").trim().toLowerCase();
      if (primary && !["#182230", "#f5f7fa"].includes(ink)) issues.push(`primary is ${ink}`);
      for (const el of document.querySelectorAll("body, body *")) {
        const cs = getComputedStyle(el);
        if (cs.backgroundImage.includes("gradient")) issues.push(`gradient on ${el.className || el.tagName}`);
        if (cs.backdropFilter && cs.backdropFilter !== "none") issues.push(`glass on ${el.className || el.tagName}`);
      }
      const h1 = document.querySelector("h1");
      if (h1 && getComputedStyle(h1).fontWeight !== "800") issues.push(`h1 weight ${getComputedStyle(h1).fontWeight}`);
      // One accent: amber fills are for the recording dot only (the lockup's
      // capsule is an SVG fill, not a background).
      const amber = ["rgb(226, 122, 43)", "rgb(233, 139, 69)"];
      for (const el of document.querySelectorAll("body *")) {
        if (amber.includes(getComputedStyle(el).backgroundColor) && !el.classList.contains("rec-dot")) issues.push(`amber fill on ${el.className || el.tagName}`);
      }
      // OpenBark is in beta: nothing on the site may still say pre-alpha.
      if (/pre-?alpha/i.test(document.body.innerText)) issues.push("says pre-alpha");
      // Sharing: canonical and og:url on www.openbark.io, a large preview image.
      const meta = (sel) => document.querySelector(sel)?.getAttribute("content") ?? document.querySelector(sel)?.getAttribute("href");
      const page = location.pathname.split("/").pop() || "index.html";
      const want = { "index.html": "/", "features.html": "/features", "history.html": "/history", "portal.html": "/portal" }[page];
      if (want) {
        if (meta('link[rel="canonical"]') !== `https://www.openbark.io${want}`) issues.push(`canonical is ${meta('link[rel="canonical"]')}`);
        if (meta('meta[property="og:url"]') !== `https://www.openbark.io${want}`) issues.push(`og:url is ${meta('meta[property="og:url"]')}`);
      }
      if (meta('meta[property="og:image"]') !== "https://www.openbark.io/og-image.png") issues.push(`og:image is ${meta('meta[property="og:image"]')}`);
      if (meta('meta[name="twitter:card"]') !== "summary_large_image") issues.push("no large twitter card");
      if (document.querySelector(".stage")?.textContent !== "Beta") issues.push("header badge is not Beta");
      // "Open source" only once the code can be seen: a page that says it must link the repository.
      const source = document.querySelector('a[href^="https://github.com/ANI-ML/openbark"]:not([href*="releases"])');
      if (!source && /open.source AI scribe/i.test(document.title + " " + (document.querySelector("h1")?.textContent ?? ""))) issues.push("says open source but links no source code");
      // Evidence is behind "How we know", closed until asked for.
      if (document.querySelector(".evidence:not(details .evidence)")) issues.push("evidence shown outside How we know");
      if (document.querySelector("details.how[open]")) issues.push("How we know open by default");
      // The account link: in the header on every page and every width (an icon on
      // phones), and in the footer. Until the portal is switched on, only the
      // portal page itself has it, and no other page links to the portal.
      const acct = document.querySelector('.site-nav a.nav-account[href="portal.html"]');
      if (!linked) {
        if (!onPortal && document.querySelector('a[href="portal.html"]')) issues.push("links to the portal before it is switched on");
        if (onPortal && !acct) issues.push("no account link in the portal page header");
      } else if (!acct) issues.push("no Sign in link in the header");
      else {
        const r = acct.getBoundingClientRect();
        if (r.width < 24 || r.height < 24 || r.right > innerWidth) issues.push(`header Sign in link hidden or clipped (${Math.round(r.width)}x${Math.round(r.height)} at ${Math.round(r.left)})`);
        if (acct.textContent.trim() !== "Sign in") issues.push(`header account link says "${acct.textContent.trim()}"`);
      }
      if (linked && !document.querySelector('.site-foot a[href="portal.html"]')) issues.push("no Sign in link in the footer");
      // The CSP let everything through.
      if (window.__csp?.length) issues.push(`CSP violations: ${window.__csp.join("; ")}`);
      return issues;
    }, { linked: portalLinked, onPortal: p === "portal.html" });
    for (const b of brand) problems.push(`${v.name} ${p}: ${b}`);
    await page.screenshot({ path: `${out}/${v.name}-${p.replace(".html", "")}-full.png`, fullPage: true });
  }
  // The landing page: the beta hero, the four short sections, then downloads,
  // the maker and contact, in that order.
  await page.goto(`${base}/index.html`);
  const landing = await page.evaluate(() => {
    const issues = [];
    const h1 = document.querySelector(".hero h1");
    if (!["The free, open-source AI scribe for veterinary medicine.", "The free AI scribe for veterinary medicine."].includes(h1?.textContent)) issues.push(`hero h1 is "${h1?.textContent}"`);
    const cs = h1 && getComputedStyle(h1);
    if (cs && (cs.fontSize !== (innerWidth > 600 ? "48px" : "36px"))) issues.push(`hero h1 size ${cs.fontSize}`);
    const dl = document.querySelector("#hero-download");
    // With installers published it downloads; until then it asks to join the beta (never a dead download).
    const published = !document.querySelector('#downloads [aria-disabled="true"]:not([data-files="ios"] *)');
    if (!dl?.classList.contains("btn-primary")) issues.push("hero button is not primary");
    if (!/^Download the beta/.test(dl.textContent.trim()))
      issues.push(`hero button is "${dl?.textContent.trim()}" -> ${dl?.getAttribute("href")}`);
    if (!/beta/i.test(document.querySelector("#hero-meta")?.textContent ?? "")) issues.push("no beta note in the hero");
    if (!/Review every draft/.test(document.querySelector("#hero-meta")?.textContent ?? "")) issues.push("hero beta note lost its caveat");
    const order = ["free", "private", "planet", "careful", "download", "who", "contact"].map((id) => document.getElementById(id));
    order.forEach((el, i) => { if (!el) issues.push(`missing section ${i}`); });
    for (let i = 1; i < order.length; i++) {
      if (order[i - 1] && order[i] && !(order[i - 1].compareDocumentPosition(order[i]) & Node.DOCUMENT_POSITION_FOLLOWING)) issues.push(`section ${order[i].id} out of order`);
    }
    const who = [...document.querySelectorAll("#tx .who")].map((w) => w.textContent);
    if (who.some((w) => !["Vet", "Client"].includes(w))) issues.push(`speaker labels ${who.join(",")}`);
    // Labels never wrap or collide with the words.
    for (const w of document.querySelectorAll("#tx .who")) if (w.scrollWidth > w.clientWidth + 1) issues.push(`speaker label "${w.textContent}" overflows`);
    const text = document.body.innerText;
    for (const banned of [/zero (environmental )?impact/i, /the first open.source/i, /nothing (will )?ever leaves/i]) if (banned.test(text)) issues.push(`says ${banned}`);
    if (document.querySelector("#stats, .stat")) issues.push("evaluation stats are back on the landing page");
    return issues;
  });
  for (const b of landing) problems.push(`${v.name} landing: ${b}`);
  // The demo: hovering a note line lights its transcript line.
  await page.locator('.note button[data-ev="4"]').hover();
  const lit = await page.locator('#tx li.lit').getAttribute("data-id");
  if (lit !== "4") problems.push(`${v.name}: demo lit ${lit}, expected 4`);
  // Contact and coffee links, and the Version history tab.
  const mail = await page.locator('#contact a[href^="mailto:"]').getAttribute("href");
  if (!mail?.startsWith("mailto:we.care@animl.health")) problems.push(`${v.name}: contact link is ${mail}`);
  const coffee = await page.locator('#contact a[href*="buymeacoffee.com"]').getAttribute("href");
  if (coffee !== "https://buymeacoffee.com/animlhealth") problems.push(`${v.name}: coffee link is ${coffee}`);
  if (!v.isMobile) {
    await page.locator('.site-nav a[href="history.html"]').click();
    await page.waitForURL(/history/);
    const entries = await page.locator(".hx-entry").count();
    if (entries < 3) problems.push(`${v.name}: history shows ${entries} entries`);
    // Major changes only: no entry lists more than 8 items, and no commit hashes show.
    const crowded = await page.evaluate(() => [...document.querySelectorAll(".hx-entry")].filter((el) => el.querySelectorAll(".hx-items li").length > 8).length);
    if (crowded) problems.push(`${v.name}: ${crowded} history entries list more than 8 items`);
    const hashes = await page.evaluate(() => (document.querySelector("#hx")?.innerText.match(/[0-9a-f]{7}/g) || []).length);
    if (hashes) problems.push(`${v.name}: history shows ${hashes} commit hashes`);
  }
  // Every in-site anchor lands on an element that exists (the footer Privacy link once pointed nowhere).
  for (const p of ["index.html", "features.html", "history.html", "portal.html"]) {
    await page.goto(`${base}/${p}`);
    const links = await page.evaluate(() => [...new Set([...document.querySelectorAll('a[href*="#"]')].map((a) => a.getAttribute("href")))]);
    for (const href of links) {
      if (/^(https?:|mailto:)/.test(href)) continue;
      const [file, id] = href.split("#");
      if (!id) continue;
      if (file && file !== p) await page.goto(`${base}/${file}`);
      if (!(await page.evaluate((x) => !!document.getElementById(x), id))) problems.push(`${v.name} ${p}: link ${href} has no target`);
      if (file && file !== p) await page.goto(`${base}/${p}`);
    }
  }
  // The features filter.
  await page.goto(`${base}/features.html`);
  await page.locator("#fx-q").fill("dental");
  const shown = await page.locator(".fx-item:not([hidden])").count();
  if (shown < 1) problems.push(`${v.name}: filter "dental" shows nothing`);
  // Hidden must mean not drawn (a display rule once overrode the hidden attribute).
  const drawn = await page.evaluate(() => [...document.querySelectorAll(".fx-item")].filter((li) => li.getClientRects().length).length);
  if (drawn !== shown) problems.push(`${v.name}: filter leaves ${shown} items but ${drawn} are drawn`);

  // The account portal, signed out: supabase-js loaded through the CSP and SRI,
  // the email step showing, and no call to Supabase until someone acts.
  await page.goto(`${base}/portal.html`);
  await page.waitForLoadState("networkidle");
  const signedOut = await page.evaluate(() => {
    const issues = [];
    if (typeof window.supabase?.createClient !== "function") issues.push("supabase-js didn't load (CSP, SRI or network)");
    const state = document.getElementById("portal")?.getAttribute("data-state");
    if (state !== "out") issues.push(`state is ${state}, expected out`);
    if (document.getElementById("view-signed-out")?.hidden !== false) issues.push("signed-out view hidden");
    if (document.getElementById("view-signed-in")?.hidden !== true) issues.push("signed-in view showing while signed out");
    if (document.getElementById("email-form")?.hidden !== false) issues.push("email step not showing");
    for (const id of ["code-form", "view-signed-in", "view-loading", "delete-form"]) if (document.getElementById(id)?.getClientRects().length) issues.push(`#${id} drawn while signed out`);
    const email = document.getElementById("email");
    if (email?.type !== "email" || email?.autocomplete !== "email" || email?.disabled) issues.push("email field missing or disabled");
    if (document.querySelector('#portal input[type="password"]')) issues.push("a password field");
    if (/patient|transcript|recording/i.test(document.querySelector(".pt-signin")?.innerText ?? "")) issues.push("sign-in card mentions clinical data");
    if (document.getElementById("signin-msg")?.getAttribute("data-tone") === "error") issues.push(`error shown: ${document.getElementById("signin-msg").textContent}`);
    return issues;
  });
  for (const b of signedOut) problems.push(`${v.name} portal: ${b}`);
  if (mock.calls.length) problems.push(`${v.name} portal: called Supabase while signed out: ${mock.calls.join(", ")}`);

  // The account portal through a mocked sign-in: email, code, profile, delete.
  mock.on = true;
  await page.locator("#email").fill(USER.email);
  await page.locator('#email-form button[type="submit"]').click();
  await page.locator("#code-form").waitFor({ state: "visible" });
  if ((await page.locator("#code-email").textContent()) !== USER.email) problems.push(`${v.name} portal: code step doesn't name the email`);
  if (!(await page.locator("#resend").isDisabled())) problems.push(`${v.name} portal: a new code can be asked for at once`);
  await page.screenshot({ path: `${out}/${v.name}-portal-code.png` });
  await page.locator("#code").fill("123456");
  await page.locator('#code-form button[type="submit"]').click();
  await page.locator("#view-signed-in").waitFor({ state: "visible" });
  await page.waitForFunction(() => document.querySelectorAll("#downloads .pt-dl-row").length === 3 && !/Checking/.test(document.getElementById("downloads-line").textContent));
  // The clinic dashboard (ADR-0028): the mocked clinic, 30 days of bars, the members.
  await page.waitForFunction(() => document.querySelectorAll("#usage-svg rect").length === 30 && document.querySelectorAll("#members-table tbody tr").length === 4, null, { timeout: 10000 }).catch(() => {});
  await page.waitForTimeout(300);
  const clinic = await page.evaluate((name) => {
    const issues = [];
    const shown = (id) => !!document.getElementById(id)?.getClientRects().length;
    if (!shown("clinic-dash")) issues.push("clinic dashboard not shown");
    if (document.getElementById("clinic-h")?.textContent !== name) issues.push("clinic name not shown");
    if (document.querySelectorAll("#usage-svg rect").length !== 30) issues.push(`chart has ${document.querySelectorAll("#usage-svg rect").length} bars, not 30`);
    if (!/visits recorded per day/i.test(document.getElementById("usage-svg-label")?.textContent ?? "")) issues.push("chart has no text alternative");
    if (document.querySelectorAll("#usage-days tbody tr").length !== 30) issues.push("the per-day table doesn't have 30 rows");
    if (document.querySelectorAll("#usage-stats dd").length !== 5) issues.push("usage tiles missing");
    if (!/Former members/.test(document.getElementById("usage-members")?.textContent ?? "")) issues.push("no former members row");
    if (!/Not sharing/.test(document.getElementById("usage-members")?.textContent ?? "")) issues.push("a member who isn't sharing isn't marked");
    if (!shown("clinic-invited") || !/Hillside Vets/.test(document.getElementById("clinic-invite-list")?.textContent ?? "")) issues.push("the pending invite isn't shown");
    if (document.querySelectorAll("#members-table tbody tr").length !== 4) issues.push("members table doesn't list four members");
    if (/@/.test(document.getElementById("members-table")?.textContent ?? "")) issues.push("members table shows an email address");
    if (/\u00b7/.test(document.getElementById("clinic-area")?.innerText ?? "")) issues.push("a middle dot in the clinic area");
    return issues;
  }, CLINIC.name);
  for (const b of clinic) problems.push(`${v.name} portal clinic: ${b}`);
  await page.locator("#clinic-dash").screenshot({ path: `${out}/${v.name}-portal-clinic-usage.png` });
  for (const tab of ["members", "invite", "settings"]) {
    await page.locator(`#tab-${tab}`).click();
    if (await page.evaluate(() => document.documentElement.scrollWidth > innerWidth)) problems.push(`${v.name} portal clinic: horizontal scroll on the ${tab} tab`);
    await page.locator("#clinic-dash").screenshot({ path: `${out}/${v.name}-portal-clinic-${tab}.png` });
  }
  await page.locator("#tab-usage").click();
  const account = await page.evaluate((email) => {
    const issues = [];
    if (document.getElementById("account-email")?.textContent !== email) issues.push("account email not shown");
    for (const id of ["view-signed-out", "view-loading", "delete-form"]) if (document.getElementById(id)?.getClientRects().length) issues.push(`#${id} drawn while signed in`);
    if (document.querySelector(".nav-account")?.textContent.trim() !== "Account") issues.push("header doesn't say Account while signed in");
    const rows = [...document.querySelectorAll("#downloads .pt-dl-row")].map((r) => r.getAttribute("data-os"));
    const mine = /Windows/.test(navigator.userAgent) ? "windows" : /Mac/.test(navigator.userAgent) ? "mac" : "linux";
    if (rows[0] !== mine) issues.push(`downloads start with ${rows[0]}, not this computer (${mine})`);
    if (!document.querySelector('#view-signed-in a[href="history.html"]')) issues.push("no Version history link");
    if (!document.getElementById("delete-open")) issues.push("no Delete my account");
    if (document.querySelector(".pt-msg[data-tone=error]")) issues.push(`error shown: ${document.querySelector(".pt-msg[data-tone=error]").textContent}`);
    if (document.documentElement.scrollWidth > innerWidth) issues.push("horizontal scroll");
    return issues;
  }, USER.email);
  for (const b of account) problems.push(`${v.name} portal signed in: ${b}`);
  await page.screenshot({ path: `${out}/${v.name}-portal-account.png` });
  await page.screenshot({ path: `${out}/${v.name}-portal-account-full.png`, fullPage: true });
  // The profile saves only the person's own row, with the fields the table has.
  await page.locator("#p-name").fill("Jordan Lee");
  await page.locator("#p-clinic").fill("Riverside Animal Hospital");
  await page.locator('input[name="role"][value="veterinarian"]').check();
  await page.locator("#p-country").selectOption("CA");
  await page.locator("#p-updates").check();
  await page.locator('#profile-form button[type="submit"]').click();
  await page.locator('#profile-msg[data-tone="ok"]').waitFor();
  const want = { id: USER.id, name: "Jordan Lee", clinic: "Riverside Animal Hospital", role: "veterinarian", country: "CA", updates_opt_in: true };
  if (JSON.stringify(mock.profile) !== JSON.stringify(want)) problems.push(`${v.name} portal: profile saved as ${JSON.stringify(mock.profile)}`);
  // Delete: the button stays off until the email is typed; then the account goes and the page signs out.
  await page.locator("#delete-open").click();
  if (!/only admin of Riverside Animal Hospital/.test((await page.locator("#delete-clinic-warn").textContent()) ?? "") || !(await page.locator("#delete-clinic-warn").isVisible()))
    problems.push(`${v.name} portal: no warning that deleting the account deletes the clinic it is the only admin of`);
  if (!(await page.locator("#delete-submit").isDisabled())) problems.push(`${v.name} portal: delete enabled before confirming`);
  await page.locator("#delete-email").fill(USER.email.toUpperCase());
  if (await page.locator("#delete-submit").isDisabled()) problems.push(`${v.name} portal: delete still off after typing the email`);
  await page.locator("#delete-submit").scrollIntoViewIfNeeded();
  await page.screenshot({ path: `${out}/${v.name}-portal-delete.png` });
  await page.locator("#delete-submit").click();
  await page.locator("#view-signed-out").waitFor({ state: "visible" });
  const gone = await page.evaluate(() => ({
    msg: document.getElementById("signin-msg")?.textContent ?? "",
    stored: localStorage.getItem("sb-nmecovtmijnfnjuadicp-auth-token"),
    label: document.querySelector(".nav-account")?.textContent.trim(),
  }));
  if (!/deleted/.test(gone.msg)) problems.push(`${v.name} portal: after deleting, the page says "${gone.msg}"`);
  if (gone.stored) problems.push(`${v.name} portal: session still stored after deleting`);
  if (gone.label !== "Sign in") problems.push(`${v.name} portal: header says "${gone.label}" after deleting`);
  if (mock.deleted?.confirm?.toLowerCase() !== USER.email) problems.push(`${v.name} portal: delete sent ${JSON.stringify(mock.deleted)}`);
  mock.on = false;
  if (await page.evaluate(() => window.__csp.length)) problems.push(`${v.name} portal: CSP violations ${await page.evaluate(() => window.__csp.join("; "))}`);
  await ctx.close();
}
await browser.close();
console.log(problems.length ? `PROBLEMS:\n${problems.join("\n")}` : "all checks passed");
process.exitCode = problems.length ? 1 : 0;
