// End-to-end test of deleting visits and recordings (ADR-0022, phase 1).
//
// Run ONLY against the delete demo, never a clinic's own server or data:
//   OPENBARK_URL=http://localhost:4325 SEED_FROM=<another demo's visits folder> \
//     node e2e/deleting.mjs <delete-demo folder>
//
// The delete demo is `openbark serve --port 4325` started with every folder in
// its own scratch folder: OPENBARK_SESSIONS_DIR, OPENBARK_TEMPLATES_DIR,
// OPENBARK_USERS_FILE, OPENBARK_LOCATIONS_FILE and a throwaway OPENBARK_DATA_KEY
// (the seed demo's, so the copied visits open), with an empty file called
// OPENBARK-DEMO in that scratch folder.
//
// Before doing anything the script refuses unless:
//   1. the URL is this computer, on port 4325 (never 4317, the clinic's own server);
//   2. <delete-demo folder> is called demo-delete (or delete-demo) and holds
//      an OPENBARK-DEMO marker file;
//   3. GET /api/storage reports <delete-demo folder>/visits as the visits folder;
//   4. neither folder is inside OpenBark's own data folder, and the demo has no team yet.
// It copies (never moves) seed visits from SEED_FROM, and only ever deletes
// visits it copied in this run; their ids are printed at the end.
import { cpSync, existsSync, mkdirSync, readFileSync, readdirSync, statSync } from "node:fs";
import { basename, isAbsolute, join, relative, resolve } from "node:path";
import { chromium } from "@playwright/test";
const OLD_NAV = ["record", "patients", "templates", "models", "settings", "help"]; // nav by name (data-nav, ADR-0027)

const base = process.env.OPENBARK_URL ?? "http://localhost:4325";
const out = process.env.OUT_DIR ?? "../../target/deleting";
const demo = process.argv[2] ? resolve(process.argv[2]) : null;
const seedFrom = process.env.SEED_FROM ? resolve(process.env.SEED_FROM) : null;
/** A multi-pet visit (Olive Haddad with Pickles), unsigned, with its recording. */
const MULTI = process.env.SEED_MULTI ?? "2026-10-18_015618_olive-haddad";
/** A single-animal visit (Diesel Brennan) whose draft can be signed. */
const SINGLE = process.env.SEED_SINGLE ?? "2026-10-18_015813_diesel-brennan";

const PHASE_1 = "Signed visits can't be deleted yet. Add an addendum to correct the note.";
const OWN_DATA = /[\\/]AppData[\\/]Local[\\/]OpenBark([\\/]|$)/i;

// ---- safety checks, before anything is touched -----------------------------

function refuse(why) {
  console.error(`REFUSING TO RUN: ${why}`);
  process.exit(2);
}
const url = new URL(base);
if (url.port === "4317") refuse("port 4317 is the clinic's own OpenBark server");
if (url.port !== "4325") refuse(`the delete demo runs on port 4325, not ${url.port || "the default port"}`);
if (!["localhost", "127.0.0.1"].includes(url.hostname)) refuse("only this computer's delete demo can be used");
if (!demo) refuse("usage: node e2e/deleting.mjs <delete-demo folder>");
if (OWN_DATA.test(demo)) refuse(`${demo} is inside OpenBark's own data folder`);
if (!/^(demo-delete|delete-demo)$/.test(basename(demo)))
  refuse(`${demo} isn't a delete demo folder (it must be called demo-delete or delete-demo)`);
if (!existsSync(join(demo, "OPENBARK-DEMO"))) refuse(`${demo} has no OPENBARK-DEMO marker file`);
const inside = (child, parent) => {
  const r = relative(parent, child);
  return r !== "" && !r.startsWith("..") && !isAbsolute(r);
};
const storage = await fetch(`${base}/api/storage`).then((r) => r.json());
const visitsDir = resolve(storage.visits.path);
if (!inside(visitsDir, demo)) refuse(`the server's visits folder (${visitsDir}) isn't inside ${demo}`);
if (relative(join(demo, "visits"), visitsDir) !== "")
  refuse(`the server's visits folder (${visitsDir}) isn't ${join(demo, "visits")}`);
const status = await fetch(`${base}/api/status`).then((r) => r.json());
if (status.team) refuse("this delete demo already has a team; start a fresh one");
if (!seedFrom || !existsSync(join(seedFrom, MULTI, "session.json")) || !existsSync(join(seedFrom, SINGLE, "session.json")))
  refuse(`SEED_FROM must be a demo's visits folder holding ${MULTI} and ${SINGLE}`);
if (OWN_DATA.test(seedFrom)) refuse("never seed from OpenBark's own data folder");
if (seedFrom === visitsDir || inside(seedFrom, visitsDir) || inside(visitsDir, seedFrom))
  refuse("SEED_FROM must be a separate folder from the delete demo's visits");

mkdirSync(out, { recursive: true });
const problems = [];
const check = (ok, what) => ok || problems.push(what);
const step = (what) => console.log(`- ${what}`);

// ---- helpers ------------------------------------------------------------------

async function api(method, path, body, { header = true } = {}) {
  const headers = {};
  if (header) headers["x-openbark"] = "1";
  if (body !== undefined) headers["content-type"] = "application/json";
  const res = await fetch(`${base}${path}`, {
    method,
    headers,
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const text = await res.text();
  let json = null;
  try {
    json = JSON.parse(text);
  } catch {
    // Not JSON (a 403 from the guard is plain text).
  }
  return { status: res.status, json, text };
}

const run = Date.now().toString(36);
const created = [];
let copies = 0;
/** Copy a seed visit into the demo under a new folder name, with its own reason to find it by. */
async function seed(source, label) {
  const id = `${source}-e2e${run}${++copies}`;
  cpSync(join(seedFrom, source), join(visitsDir, id), { recursive: true });
  created.push(id);
  const reason = `E2E ${run} ${label}`;
  const view = (await api("GET", `/api/sessions/${id}`)).json;
  const saved = await api("POST", `/api/sessions/${id}/patient`, { ...view.patient, reason });
  check(saved.status === 200, `seed ${id}: ${saved.text}`);
  return { id, reason };
}
const mine = (id) => {
  if (!created.includes(id)) throw new Error(`not deleting ${id}: this run didn't create it`);
  return id;
};

const variants = [
  { name: "desktop-light", viewport: { width: 1440, height: 900 }, colorScheme: "light" },
  { name: "desktop-dark", viewport: { width: 1440, height: 900 }, colorScheme: "dark" },
  { name: "mobile-light", viewport: { width: 390, height: 844 }, colorScheme: "light", isMobile: true, hasTouch: true },
];
const desktop = variants[0];

const browser = await chromium.launch();
let expect403 = false;

async function open(v, user = null) {
  const ctx = await browser.newContext({
    viewport: v.viewport,
    colorScheme: v.colorScheme,
    isMobile: v.isMobile,
    hasTouch: v.hasTouch,
    acceptDownloads: true,
  });
  await ctx.addInitScript((who) => {
    const key = "openbark.settings";
    const current = JSON.parse(localStorage.getItem(key) ?? "{}");
    localStorage.setItem(
      key,
      JSON.stringify({ ...current, updateCheck: "off", consentAcknowledgedAt: "2026-10-17T20:00:00Z", uiLanguage: "en", signer: "Dr Rivera" }),
    );
    if (who) localStorage.setItem("openbark.user", JSON.stringify(who));
    else localStorage.removeItem("openbark.user");
  }, user);
  const page = await ctx.newPage();
  page.on("pageerror", (e) => problems.push(`${v.name}: ${e}`));
  page.on("response", (r) => {
    if (r.status() >= 400 && !(expect403 && r.status() === 403))
      problems.push(`${v.name}: ${r.request().method()} ${new URL(r.url()).pathname} answered ${r.status()}`);
  });
  page.on("console", (m) => {
    // The guard check below makes one request the server must refuse.
    if (m.type() === "error" && !(expect403 && m.text().includes("403"))) problems.push(`${v.name}: console: ${m.text()}`);
  });
  await page.goto(base);
  await page.locator(".screen h1").first().waitFor();
  return { ctx, page };
}

async function shot(page, v, name) {
  await page.waitForTimeout(400);
  await page.screenshot({ path: `${out}/${v.name}-${name}.png` });
  const wide = await page.evaluate(() => document.scrollingElement.scrollWidth > window.innerWidth);
  check(!wide, `${v.name} ${name}: the page scrolls sideways`);
}

const nav = async (page, i) => {
  await page.locator(`.nav-item[data-nav="${OLD_NAV[i]}"]`).click();
  await page.locator(".screen h1").first().waitFor();
  // Settings shows one section at a time (ADR-0027): open Deleted visits when it's offered.
  if (OLD_NAV[i] === "settings") {
    const section = page.locator(".settings-nav-item, .settings-row", { hasText: "Deleted visits" }).first();
    if (await section.count()) await section.click();
  }
};
async function openPatient(page, name) {
  await nav(page, 1);
  await page.locator(".patient-row", { hasText: name }).first().click();
  // The page's heading shows before its visits load.
  await page.locator(".patient-page .visit-list, .patient-page .empty-state").first().waitFor();
}
async function openVisit(page, patient, reason) {
  await openPatient(page, patient);
  await page.locator(".visit-row", { hasText: reason }).click();
  await page.locator(".review .review-meta").waitFor();
  // Delete visit and Delete recording live in Review's More (ADR-0027).
  await openMore(page);
}
async function openMore(page) {
  if (await page.locator(".more-panel").count()) return;
  await page.getByRole("button", { name: "More actions" }).first().click();
  await page.locator(".more-panel").waitFor();
}
async function deleteFromReview(page, reasonLabel) {
  await openMore(page);
  await page.getByRole("button", { name: "Delete visit" }).click();
  await page.getByRole("menu").waitFor();
  await page.getByRole("menuitem", { name: reasonLabel }).click();
  // Back to the patient's page (every seeded visit has a patient).
  await page.locator(".patient-page").waitFor();
}
const titleOf = (locator) => locator.evaluate((el) => el.closest("[title]")?.getAttribute("title") ?? "");

/** Brand checks on what's on screen (brand guide §7.2, §10): contrast, no emoji. */
async function brand(page, v, where, selector) {
  const found = await page.evaluate((sel) => {
    const rgb = (c) => (c.match(/[\d.]+/g) ?? []).map(Number);
    const lum = ([r, g, b]) => {
      const f = (x) => {
        const s = x / 255;
        return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
      };
      return 0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b);
    };
    const bg = (el) => {
      for (let e = el; e; e = e.parentElement) {
        const c = getComputedStyle(e).backgroundColor;
        const v = rgb(c);
        if (v.length >= 3 && (v.length < 4 || v[3] > 0.5)) return v;
      }
      return [255, 255, 255];
    };
    const low = [];
    let emoji = false;
    for (const root of document.querySelectorAll(sel)) {
      if (/\p{Extended_Pictographic}/u.test(root.innerText)) emoji = true;
      for (const el of root.querySelectorAll("p, span, h2, li, button, label")) {
        if (!el.innerText?.trim() || el.closest("[aria-hidden]") || !el.checkVisibility()) continue;
        if ([...el.childNodes].every((n) => n.nodeType !== Node.TEXT_NODE || !n.textContent.trim())) continue;
        const style = getComputedStyle(el);
        if (Number(style.opacity) < 1 || el.closest(":disabled")) continue;
        const a = lum(rgb(style.color));
        const b = lum(bg(el));
        const ratio = (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);
        if (ratio < 4.5) low.push(`${el.innerText.trim().slice(0, 40)} (${ratio.toFixed(2)})`);
      }
    }
    return { low, emoji };
  }, selector);
  check(found.low.length === 0, `${v.name} ${where}: text below 4.5:1 contrast: ${found.low.join("; ")}`);
  check(!found.emoji, `${v.name} ${where}: emoji in the interface`);
}

/** The destructive outline (brand guide §7.2): danger text, danger border at 40%, no fill. */
async function destructive(locator, what) {
  await locator.page().mouse.move(1, 1); // not hovering it
  const s = await locator.evaluate((el) => {
    const c = getComputedStyle(el);
    const probe = document.createElement("span");
    probe.style.color = "var(--danger)";
    document.body.append(probe);
    const danger = getComputedStyle(probe).color;
    probe.remove();
    return { color: c.color, danger, bg: c.backgroundColor, border: c.borderTopColor, variant: el.getAttribute("data-variant") };
  });
  check(s.variant === "danger" && s.color === s.danger, `${what}: not the destructive style (${JSON.stringify(s)})`);
  check(/rgba\(0, 0, 0, 0\)|transparent/.test(s.bg), `${what}: a destructive button has no fill (${s.bg})`);
}

const walk = (dir, outList = []) => {
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    outList.push(p);
    if (statSync(p).isDirectory()) walk(p, outList);
  }
  return outList;
};
const auditLog = () => readFileSync(join(visitsDir, ".openbark", "audit.log"), "utf8");

// ---- 1. single-user: delete, Undo, restore, Delete now ---------------------------

step("single-user: delete a multi-pet visit from Pickles's page, Undo, restore, Delete now");
{
  const v = desktop;
  const olive = await seed(MULTI, "single-user");
  const { ctx, page } = await open(v);
  await openVisit(page, "Pickles", olive.reason);

  // Keyboard: Delete visit in More, Enter opens the menu, arrows move, Escape returns focus.
  await page.getByRole("button", { name: "Delete visit" }).focus();
  const focused = () => page.evaluate(() => document.activeElement?.textContent?.trim() ?? "");
  check((await focused()) === "Delete visit", `Tab should reach Delete visit, reached "${await focused()}"`);
  const ring = await page.evaluate(() => {
    const s = getComputedStyle(document.activeElement);
    return { style: s.outlineStyle, width: parseFloat(s.outlineWidth) };
  });
  check(ring.style !== "none" && ring.width >= 2, `Delete visit has no visible focus ring: ${JSON.stringify(ring)}`);
  await destructive(page.getByRole("button", { name: "Delete visit" }), "Delete visit");
  await page.keyboard.press("Enter");
  await page.getByRole("menu").waitFor();
  check((await focused()) === "Test or practice recording", "Enter should open the menu on its first reason");
  await page.keyboard.press("ArrowDown");
  check((await focused()) === "Recorded by mistake", "ArrowDown should move to the next reason");
  await page.keyboard.press("Escape");
  check((await page.getByRole("menu").count()) === 0, "Escape should close the menu");
  check((await focused()) === "Delete visit", "focus should return to Delete visit");

  // The menu names the other animal.
  await page.getByRole("button", { name: "Delete visit" }).click();
  const menu = await page.locator(".delete-menu").innerText();
  check(menu.includes("This visit also has notes for Olive. They're deleted with it."), `the menu should name Olive: ${menu}`);
  const items = await page.getByRole("menuitem").allInnerTexts();
  check(
    JSON.stringify(items) ===
      JSON.stringify([
        "Test or practice recording",
        "Recorded by mistake",
        "Duplicate visit",
        "Client withdrew consent",
        "Another reason",
      ]),
    `menu reasons: ${items.join(" / ")}`,
  );
  await brand(page, v, "menu", ".delete-menu");
  await page.getByRole("menuitem", { name: "Recorded by mistake" }).click();
  const toast = page.locator(".ob-toast");
  await toast.waitFor();
  check((await toast.innerText()).includes("Visit deleted"), "the toast should say Visit deleted");
  // The pointer on the toast keeps it open while the page is checked.
  await toast.hover();
  await page.locator(".patient-page .visit-list, .patient-page .empty-state").first().waitFor();
  check((await page.locator("h1").first().innerText()).includes("Pickles"), "back on Pickles's page");
  check((await page.locator(".visit-row", { hasText: olive.reason }).count()) === 0, "Pickles's page still lists the visit");
  const line = await page.locator(".deleted-line").innerText();
  check(
    line === "1 deleted visit is kept for 30 days. Restore it from Deleted visits in Settings.",
    `Pickles's page should show the deleted line: "${line}"`,
  );
  await shot(page, v, "1-after-delete");
  await toast.getByRole("button", { name: "Undo" }).click();
  await page.locator(".ob-toast", { hasText: "Visit restored" }).waitFor();
  await page.locator(".review .review-meta").waitFor();
  check((await api("GET", `/api/sessions/${olive.id}`)).status === 200, "Undo should restore the same id");
  check((await api("GET", "/api/deleted?by=Dr%20Rivera")).json?.length === 0, "nothing left in Deleted visits after Undo");

  // Delete again: both patients' pages lose it and show the line.
  await deleteFromReview(page, "Duplicate visit");
  for (const name of ["Pickles", "Olive"]) {
    await openPatient(page, name);
    check((await page.locator(".visit-row", { hasText: olive.reason }).count()) === 0, `${name}'s page still lists it`);
    check((await page.locator(".deleted-line").count()) === 1, `${name}'s page should show the deleted line`);
  }
  check((await api("GET", `/api/sessions/${olive.id}`)).json?.error === "This visit was deleted. Restore it from Deleted visits in Settings.", "opening the old id should say it was deleted");

  // Settings, Deleted visits: Restore.
  await nav(page, 4);
  let row = page.locator(".deleted-row", { hasText: "Olive Haddad" });
  await row.waitFor();
  await row.scrollIntoViewIfNeeded();
  const text = await row.innerText();
  for (const want of ["Olive and Pickles", "Visit of ", "by Dr Rivera", "Duplicate visit", "Removed for good on "])
    check(text.includes(want), `the Deleted visits row should show "${want}": ${text}`);
  await destructive(row.getByRole("button", { name: "Delete now" }), "Delete now");
  await brand(page, v, "Deleted visits", ".deleted-visits");
  const storageLine = await page.locator(".storage-row", { hasText: "Deleted visits:" }).count();
  check(storageLine === 1, "Storage should show the space Deleted visits take");
  await row.getByRole("button", { name: "Restore" }).click();
  await page.locator(".ob-toast", { hasText: "Visit restored" }).waitFor();
  check((await api("GET", `/api/sessions/${olive.id}`)).status === 200, "Restore should bring the visit back");

  // Delete again, then Delete now.
  await openVisit(page, "Olive", olive.reason);
  await deleteFromReview(page, "Test or practice recording");
  await nav(page, 4);
  row = page.locator(".deleted-row", { hasText: "Olive Haddad" });
  await row.waitFor();
  await row.getByRole("button", { name: "Delete now" }).click();
  const dialog = page.getByRole("dialog");
  await dialog.waitFor();
  const dtext = await dialog.innerText();
  check(dtext.includes("Delete this visit now?"), "Delete now dialog title");
  check(
    dtext.includes(
      "OpenBark removes its files now, and it can't be restored. Backups of this computer may still hold encrypted copies, which the recovery key can open.",
    ),
    "Delete now dialog body",
  );
  check((await dialog.locator("[data-variant=primary]").count()) === 0, "the Delete now dialog has no primary button");
  await brand(page, v, "Delete now dialog", ".dialog");
  await dialog.getByRole("button", { name: "Delete now" }).click();
  await page.locator(".ob-toast", { hasText: "Visit deleted for good" }).waitFor();
  check((await page.locator(".deleted-empty").innerText()) === "No deleted visits.", "Deleted visits should be empty");

  // On disk: gone everywhere, and the record holds no names.
  const everything = walk(visitsDir).map((p) => relative(visitsDir, p));
  check(!everything.some((p) => /olive/i.test(p)), `a folder or file still names Olive: ${everything.filter((p) => /olive/i.test(p))}`);
  check(readdirSync(join(visitsDir, ".openbark", "deleted")).length === 0, ".openbark/deleted should be empty");
  const log = auditLog();
  const lines = log.trim().split("\n");
  check(lines.length >= 7, `the record should hold log_started and 6 entries, has ${lines.length}`);
  check(lines.every((l) => l.startsWith("OBL1:")), "every line of audit.log should start with OBL1:");
  for (const word of ["Olive", "Haddad", "Pickles"]) check(!log.includes(word), `audit.log contains ${word}`);
  check(!/olive-haddad/i.test(log), "audit.log contains the folder name");
  await ctx.close();
}

// ---- 2. single-user: delete only the recording of a signed visit -------------------

step("single-user: a signed visit can't be deleted; its recording can");
{
  const v = desktop;
  const diesel = await seed(SINGLE, "signed");
  const signed = await api("POST", `/api/sessions/${diesel.id}/sign`, { by: "Dr Rivera" });
  check(signed.status === 200, `signing the seed: ${signed.text}`);
  const refused = await api("POST", `/api/sessions/${mine(diesel.id)}/delete`, {
    by: "Dr Rivera",
    reason: { kind: "mistake" },
  });
  check(refused.status === 400 && refused.json?.error === PHASE_1, `deleting a signed visit: ${refused.text}`);
  const { ctx, page } = await open(v);
  await openVisit(page, "Diesel", diesel.reason);
  const del = page.getByRole("button", { name: "Delete visit" });
  check(await del.isDisabled(), "Delete visit should be disabled on a signed visit");
  check((await titleOf(del)) === PHASE_1, `Delete visit's tooltip: ${await titleOf(del)}`);
  await page.locator("audio.player").waitFor();
  await page.getByRole("button", { name: "Delete recording" }).click();
  const dialog = page.getByRole("dialog");
  await dialog.waitFor();
  const dtext = await dialog.innerText();
  check(dtext.includes("Delete this recording? The note stays."), "recording dialog title");
  check(
    dtext.includes(
      "The transcript, drafts and signed notes are kept. You won't be able to play this visit back or transcribe it again.",
    ),
    "recording dialog body",
  );
  check(!dtext.includes("The note isn't signed yet."), "no unsigned line on a signed visit");
  check(
    dtext.includes("The file you imported, emergency.wav, is still where it was. Delete it there too if you need to."),
    `the imported-file line: ${dtext}`,
  );
  check((await page.evaluate(() => document.activeElement?.textContent?.trim())) === "Keep recording", "Keep recording has focus");
  await destructive(dialog.getByRole("button", { name: "Delete recording" }), "Delete recording (dialog)");
  await brand(page, v, "recording dialog", ".dialog");
  await dialog.getByRole("button", { name: "Delete recording" }).click();
  await page.locator(".recording-gone").waitFor();
  check((await page.locator("audio.player").count()) === 0, "the player should be gone");
  const gone = await page.locator(".recording-gone").innerText();
  check(/^Recording deleted \d{4}-\d\d-\d\d \d\d:\d\d by Dr Rivera\. The transcript and notes are kept\.$/.test(gone), `the line: ${gone}`);
  check((await page.locator(".transcript li").count()) > 0, "the transcript should remain");
  check(!existsSync(join(visitsDir, diesel.id, "audio.wav")), "audio.wav should be gone");
  check(existsSync(join(visitsDir, diesel.id, "transcript.jsonl")), "transcript.jsonl should stay");
  check((await api("GET", `/api/sessions/${diesel.id}/audio`)).status === 400, "playback should be refused");
  const [download] = await Promise.all([page.waitForEvent("download"), page.getByRole("button", { name: "Save PDF" }).click()]);
  check(download.suggestedFilename().endsWith(".pdf"), `Save PDF: ${download.suggestedFilename()}`);
  await page.getByRole("button", { name: "Add addendum" }).click();
  await page.locator("#addendum-text").fill("Owner called on day 2: eating again.");
  await page.getByRole("button", { name: "Sign addendum" }).click();
  await page.locator(".addendum", { hasText: "Owner called on day 2: eating again." }).waitFor();
  await shot(page, v, "2-recording-deleted");
  await ctx.close();
}

// ---- 3. layout in three variants -----------------------------------------------

for (const v of variants) {
  step(`layout: ${v.name}`);
  const visit = await seed(MULTI, v.name);
  const { ctx, page } = await open(v);
  await openVisit(page, "Pickles", visit.reason);
  await page.getByRole("button", { name: "Delete visit" }).click();
  await page.getByRole("menu").waitFor();
  await shot(page, v, "3-menu");
  await page.keyboard.press("Escape");
  if (v.isMobile) await page.getByRole("tab", { name: "Transcript" }).click();
  await openMore(page);
  await page.getByRole("button", { name: "Delete recording" }).click();
  await page.getByRole("dialog").waitFor();
  check((await page.getByRole("dialog").innerText()).includes("The note isn't signed yet."), `${v.name}: the unsigned line`);
  check((await page.getByRole("dialog").innerText()).includes("The recording covers every animal in this visit."), `${v.name}: the multi-pet line`);
  await shot(page, v, "4-recording-dialog");
  await brand(page, v, "recording dialog", ".dialog");
  await page.getByRole("button", { name: "Keep recording" }).click();
  await page.getByRole("dialog").waitFor({ state: "detached" });
  check(existsSync(join(visitsDir, visit.id, "audio.wav")), `${v.name}: Keep recording kept it`);
  await deleteFromReview(page, "Duplicate visit");
  await page.locator(".patient-page").waitFor();
  await page.locator(".deleted-line").scrollIntoViewIfNeeded();
  await shot(page, v, "5-patient-page");
  await nav(page, 4);
  const row = page.locator(".deleted-row", { hasText: "Olive Haddad" });
  await row.waitFor();
  await row.scrollIntoViewIfNeeded();
  await shot(page, v, "6-deleted-visits");
  await brand(page, v, "Deleted visits", ".deleted-visits");
  await row.getByRole("button", { name: "Delete now" }).click();
  await page.getByRole("dialog").waitFor();
  await shot(page, v, "7-delete-now");
  await page.getByRole("dialog").getByRole("button", { name: "Delete now" }).click();
  await page.locator(".ob-toast", { hasText: "Visit deleted for good" }).waitFor();
  check(!existsSync(join(visitsDir, visit.id)), `${v.name}: the visit should be gone`);
  await ctx.close();
}

// ---- 4. with a team --------------------------------------------------------------

step("team: reception, technician and veterinarian");
{
  const v = desktop;
  const dvm = (await api("POST", "/api/users", { name: "Dr Rivera", role: "dvm", pin: "2468" })).json;
  const rvt = (await api("POST", "/api/users", { name: "Sam Ortiz", role: "rvt", pin: "1357", by: dvm?.id })).json;
  const desk = (await api("POST", "/api/users", { name: "Lee Park", role: "reception", pin: "1111", by: dvm?.id })).json;
  check(dvm?.id && rvt?.id && desk?.id, "adding the team");
  const unsigned = await seed(MULTI, "team");
  const signedVisit = await seed(SINGLE, "team signed");
  const signed = await api("POST", `/api/sessions/${signedVisit.id}/sign`, { by: dvm.id });
  check(signed.status === 200, `signing as the veterinarian: ${signed.text}`);

  // Reception: no Delete visit, no Delete recording, no Deleted visits; the engine refuses.
  {
    const { ctx, page } = await open(v, desk);
    await openVisit(page, "Pickles", unsigned.reason);
    check((await page.getByRole("button", { name: "Delete visit" }).count()) === 0, "reception sees Delete visit");
    check((await page.getByRole("button", { name: "Delete recording" }).count()) === 0, "reception sees Delete recording");
    await nav(page, 4);
    check((await page.locator(".deleted-visits").count()) === 0, "reception sees Deleted visits");
    const r = await api("POST", `/api/sessions/${mine(unsigned.id)}/delete`, { by: desk.id, reason: { kind: "mistake" } });
    check(
      r.status === 400 &&
        r.json?.error === "Lee Park can record and enter patient details; deleting visits needs a veterinarian or technician.",
      `reception's delete: ${r.text}`,
    );
    check(existsSync(join(visitsDir, unsigned.id, "session.json")), "reception's refused delete left the visit");
    await ctx.close();
  }

  // Technician: deletes an unsigned visit; can't delete a signed one or its recording.
  {
    const { ctx, page } = await open(v, rvt);
    await openVisit(page, "Pickles", unsigned.reason);
    await deleteFromReview(page, "Client withdrew consent");
    await page.locator(".patient-page").waitFor();
    check(!existsSync(join(visitsDir, unsigned.id)), "the technician's delete should move the visit");
    await openVisit(page, "Diesel", signedVisit.reason);
    const del = page.getByRole("button", { name: "Delete visit" });
    check(await del.isDisabled(), "the technician's Delete visit on a signed visit should be disabled");
    check((await titleOf(del)) === PHASE_1, `its tooltip: ${await titleOf(del)}`);
    const rec = page.getByRole("button", { name: "Delete recording" });
    check(await rec.isDisabled(), "the technician's Delete recording on a signed visit should be disabled");
    const why = await rec.evaluate((el) => document.getElementById(el.getAttribute("aria-describedby") ?? "")?.textContent);
    check(why === "Only a veterinarian can delete the recording of a signed visit.", `why it can't: ${why}`);
    const r = await api("POST", `/api/sessions/${mine(signedVisit.id)}/recording/delete`, { by: rvt.id });
    check(r.json?.error === "Only a veterinarian can delete the recording of a signed visit.", `technician's recording delete: ${r.text}`);
    await ctx.close();
  }

  // Veterinarian: sees the technician's deletion; signed visits still wait for phase 2.
  {
    const { ctx, page } = await open(v, dvm);
    await openVisit(page, "Diesel", signedVisit.reason);
    check(await page.getByRole("button", { name: "Delete visit" }).isDisabled(), "phase 1: a signed visit can't be deleted");
    check(!(await page.getByRole("button", { name: "Delete recording" }).isDisabled()), "the veterinarian can delete the recording");
    await nav(page, 4);
    const row = page.locator(".deleted-row", { hasText: "Olive Haddad" });
    await row.waitFor();
    const text = await row.innerText();
    check(text.includes("by Sam Ortiz") && text.includes("Client withdrew consent"), `the veterinarian's view of the row: ${text}`);
    await shot(page, v, "8-team-deleted-visits");

    // The guard: a POST without X-OpenBark is refused, from a page and from outside.
    expect403 = true;
    const fromPage = await page.evaluate(
      async (id) =>
        (
          await fetch(`/api/sessions/${id}/delete`, {
            method: "POST",
            headers: { "content-type": "application/json" },
            body: JSON.stringify({ by: "x", reason: { kind: "mistake" } }),
          })
        ).status,
      signedVisit.id,
    );
    await page.waitForTimeout(200);
    expect403 = false;
    check(fromPage === 403, `a delete without X-OpenBark from the page: ${fromPage}`);
    const outside = await api("POST", `/api/sessions/${mine(signedVisit.id)}/delete`, { by: dvm.id, reason: { kind: "mistake" } }, { header: false });
    check(outside.status === 403, `a delete without X-OpenBark: ${outside.status}`);
    await ctx.close();
  }
}

await browser.close();

const log = auditLog();
check(log.trim().split("\n").every((l) => l.startsWith("OBL1:")), "every audit.log line is encrypted");
for (const word of ["Olive", "Haddad", "Pickles", "Diesel", "Brennan"]) check(!log.includes(word), `audit.log contains ${word}`);
console.log(`visits copied (and the only ones deleted) in this run: ${created.join(", ")}`);
console.log(`deletion record: ${log.trim().split("\n").length} lines`);
console.log(problems.length ? `PROBLEMS:\n${problems.join("\n")}` : "all checks passed");
process.exit(problems.length ? 1 : 0);
