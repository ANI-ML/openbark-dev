// End-to-end test of per-user template editing (ADR-0024 plan steps 19 and 20).
//
// Run ONLY against a template demo, never a clinic's own server or data:
//   OPENBARK_URL=http://localhost:4327 SEED_FROM=<another demo's visits folder> \
//     PHASE=1 node e2e/template-editing.mjs <tpl-demo folder>
//   (restart the demo with OPENBARK_TEST_BUILTINS_DIR=<tpl-demo>/builtins-next)
//   OPENBARK_URL=http://localhost:4327 PHASE=2 node e2e/template-editing.mjs <tpl-demo folder>
//
// The demo is a `test-builtins` build of `openbark serve` with every folder in
// its own scratch folder (visits, templates, team file, folder choices, key
// file) and a throwaway OPENBARK_DATA_KEY, with an empty file called
// OPENBARK-DEMO in that folder. Profiles land beside its team file.
//
// Before doing anything the script refuses unless:
//   1. the URL is this computer, on port 4321 or 4327, and never 4317 (the clinic's own server);
//   2. <tpl-demo folder> holds an OPENBARK-DEMO marker and isn't inside OpenBark's own data folder;
//   3. GET /api/storage reports a visits folder inside <tpl-demo folder>;
//   4. phase 1: GET /api/templates lists only the 20 built-ins, and there is no team yet.
import { cpSync, existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { isAbsolute, join, relative, resolve } from "node:path";
import { chromium } from "@playwright/test";
const OLD_NAV = ["record", "patients", "templates", "models", "settings", "help"]; // nav by name (data-nav, ADR-0027)

const base = process.env.OPENBARK_URL ?? "http://localhost:4327";
const phase = process.env.PHASE ?? "1";
const demo = process.argv[2] ? resolve(process.argv[2]) : null;
const seedFrom = process.env.SEED_FROM ? resolve(process.env.SEED_FROM) : null;
const out = process.env.OUT_DIR ?? (demo ? join(demo, "..", "template-shots") : "template-shots");
const LLM = process.env.LLM ?? "gemma-4-e4b-it-q4_0";
const OWN_DATA = /[\\/]AppData[\\/]Local[\\/]OpenBark([\\/]|$)/i;
/** Seed visits: a feline consult (no patient yet) and a dental cleaning. */
const FELINE = process.env.SEED_FELINE ?? "2026-10-18_134653_feline-vomiting";
const DENTAL = process.env.SEED_DENTAL ?? "2026-10-18_015540_biscuit-okafor";

// ---- safety checks, before anything is touched -----------------------------

function refuse(why) {
  console.error(`REFUSING TO RUN: ${why}`);
  process.exit(2);
}
const url = new URL(base);
if (url.port === "4317") refuse("port 4317 is the clinic's own OpenBark server");
if (!["4321", "4327"].includes(url.port)) refuse(`template demos run on port 4321 or 4327, not ${url.port || "the default port"}`);
if (!["localhost", "127.0.0.1"].includes(url.hostname)) refuse("only this computer's demo can be used");
if (!demo) refuse("usage: node e2e/template-editing.mjs <tpl-demo folder>");
if (OWN_DATA.test(demo)) refuse(`${demo} is inside OpenBark's own data folder`);
if (!existsSync(join(demo, "OPENBARK-DEMO"))) refuse(`${demo} has no OPENBARK-DEMO marker file`);
const inside = (child, parent) => {
  const r = relative(parent, child);
  return r !== "" && !r.startsWith("..") && !isAbsolute(r);
};
const storage = await fetch(`${base}/api/storage`).then((r) => r.json());
const visitsDir = resolve(storage.visits.path);
if (!inside(visitsDir, demo)) refuse(`the server's visits folder (${visitsDir}) isn't inside ${demo}`);
const status0 = await fetch(`${base}/api/status`).then((r) => r.json());
if (phase === "1") {
  const list = await fetch(`${base}/api/templates`).then((r) => r.json());
  if (list.length !== 20 || list.some((t) => t.layer !== "builtin")) refuse("the demo isn't fresh: it has more than the 20 built-ins");
  if (status0.team) refuse("the demo already has a team; start a fresh one");
  if (!seedFrom || OWN_DATA.test(seedFrom)) refuse("SEED_FROM must be another demo's visits folder");
  for (const v of [FELINE, DENTAL]) if (!existsSync(join(seedFrom, v, "session.json"))) refuse(`SEED_FROM has no ${v}`);
}

mkdirSync(out, { recursive: true });
const problems = [];
const check = (ok, what) => {
  if (!ok) problems.push(what);
  return ok;
};
const step = (what) => console.log(`- ${what}`);
const statePath = join(demo, "e2e-state.json");

async function api(method, path, body, { header = true } = {}) {
  const headers = {};
  if (header) headers["x-openbark"] = "1";
  if (body !== undefined) headers["content-type"] = "application/json";
  const res = await fetch(`${base}${path}`, { method, headers, body: body === undefined ? undefined : JSON.stringify(body) });
  const text = await res.text();
  let json = null;
  try {
    json = JSON.parse(text);
  } catch {
    // A 403 from the guard is plain text.
  }
  return { status: res.status, json, text };
}
const ok = async (method, path, body) => {
  const r = await api(method, path, body);
  if (r.status !== 200) throw new Error(`${method} ${path}: ${r.status} ${r.text}`);
  return r.json;
};

const variants = {
  light: { name: "desktop-light", viewport: { width: 1440, height: 900 }, colorScheme: "light" },
  dark: { name: "desktop-dark", viewport: { width: 1440, height: 900 }, colorScheme: "dark" },
  phone: { name: "phone", viewport: { width: 360, height: 780 }, colorScheme: "light", isMobile: true, hasTouch: true },
};
const browser = await chromium.launch();

async function open(v, user = null) {
  const ctx = await browser.newContext({ viewport: v.viewport, colorScheme: v.colorScheme, isMobile: v.isMobile, hasTouch: v.hasTouch });
  await ctx.addInitScript((who) => {
    const key = "openbark.settings";
    const current = JSON.parse(localStorage.getItem(key) ?? "{}");
    localStorage.setItem(key, JSON.stringify({ ...current, updateCheck: "off", consentAcknowledgedAt: "2026-10-17T20:00:00Z", setupDoneAt: "2026-10-17T20:00:00Z", uiLanguage: "en", signer: "Dr Rivera" }));
    if (who) localStorage.setItem("openbark.user", JSON.stringify(who));
    else localStorage.removeItem("openbark.user");
  }, user);
  const page = await ctx.newPage();
  page.on("pageerror", (e) => problems.push(`${v.name}: ${e}`));
  await page.goto(base);
  await page.locator(".screen h1").first().waitFor();
  return { ctx, page };
}
async function shot(page, v, name) {
  await page.waitForTimeout(400);
  await page.screenshot({ path: join(out, `${v.name}-${name}.png`), fullPage: false });
  const wide = await page.evaluate(() => document.scrollingElement.scrollWidth > window.innerWidth);
  check(!wide, `${v.name} ${name}: the page scrolls sideways`);
}
const nav = async (page, i) => {
  await page.locator(`.nav-item[data-nav="${OLD_NAV[i]}"]`).click();
  await page.locator(".screen h1").first().waitFor();
};
async function openTemplate(page, name) {
  await nav(page, 2);
  await page.locator(".tpl-row", { hasText: name }).first().click();
  await page.locator(".builder .builder-meta").waitFor();
}
const badgesOf = async (page, name) =>
  (await page.locator(".tpl-row", { hasText: name }).first().locator(".tpl-badges").innerText().catch(() => "")).trim();
const toast = async (page) => (await page.locator(".ob-toast").first().innerText()).trim();

function soapWith(view, plan) {
  const t = structuredClone(view.template);
  delete t.standards;
  delete t.origin;
  t.sections.find((s) => s.id === "plan").instruction = plan;
  return t;
}

// ==== phase 1 ===================================================================

if (phase === "1") {
  // Seed: copies (never moves) of two transcribed visits from another demo.
  for (const v of [FELINE, DENTAL]) {
    if (!existsSync(join(visitsDir, v))) cpSync(join(seedFrom, v), join(visitsDir, v), { recursive: true });
  }

  // ---- working alone ----------------------------------------------------------
  step("Working alone: change SOAP's Plan instruction and save my version");
  const L = variants.light;
  {
    // Phone width (360 px): the list and an unchanged built-in, with no sideways scroll.
    const P = variants.phone;
    const { ctx: c, page: p } = await open(P);
    await nav(p, 2);
    await shot(p, P, "01-list-alone");
    await openTemplate(p, "SOAP note");
    await shot(p, P, "02-editor-unchanged-builtin");
    await c.close();
  }
  let { ctx, page } = await open(L);
  await nav(page, 2);
  await shot(page, L, "01-list-alone");
  await openTemplate(page, "SOAP note");
  await shot(page, L, "02-editor-unchanged-builtin");
  const save = page.getByRole("button", { name: "Save my version" });
  check(await save.isDisabled(), "Save my version is enabled before any change");
  await page.getByLabel("Instruction for Plan").fill("Plan: treatments with doses as said, then rechecks with their dates.");
  await page.waitForTimeout(500);
  await save.click();
  await page.locator(".ob-toast").waitFor();
  check((await toast(page)) === "Saved as your version 1", `first save toast: ${await toast(page)}`);
  const alone = await ok("GET", "/api/templates/soap?by=Dr%20Rivera");
  check(alone.layer === "team" && alone.template.version === "1", `alone save layer ${alone.layer} v${alone.template.version}`);
  check(alone.prompt.includes("rechecks with their dates") && alone.always_applied.core.every((r) => alone.prompt.includes(r.text)), "the prompt holds the change and every core rule");

  step("Working alone: a conflicting instruction shows a warning");
  await page.getByLabel("Instruction for Plan").fill("If vitals weren't said, write normal values.");
  await page.locator(".lint .ob-notice").first().waitFor();
  check(await page.getByRole("button", { name: "Save" }).isEnabled(), "a warning blocked Save");
  await page.locator("details.builder-advanced > summary").click();
  await page.locator(".always-applied").scrollIntoViewIfNeeded();
  await shot(page, L, "03-always-applied-and-lint");
  await page.getByLabel("Instruction for Plan").fill("Plan: treatments with doses as said, then rechecks with their dates.");
  await page.locator(".builder-title .ob-button").first().click();
  await page.locator(".tpl-row").first().waitFor();
  check((await badgesOf(page, "SOAP note")).includes("Edited"), "SOAP isn't marked Edited");

  step("Working alone: draft the feline visit with SOAP");
  await ok("POST", `/api/sessions/${FELINE}/notes`, { llm: LLM, template: "soap", by: "Dr Rivera" });
  const feline = await ok("GET", `/api/sessions/${FELINE}`);
  check(feline.latest_template?.origin?.layer === "team" && feline.latest_template.origin.version === "1", "the feline draft doesn't record the edited version");

  step("Working alone: reset to original, Undo, reset again");
  await openTemplate(page, "SOAP note");
  await page.getByRole("button", { name: "Reset to original" }).click();
  await page.locator(".ob-toast").waitFor();
  await page.locator(".ob-toast").getByRole("button", { name: "Undo" }).click();
  await page.waitForTimeout(800);
  check((await ok("GET", "/api/templates/soap")).layer === "team", "Undo didn't bring the version back");
  await page.getByRole("button", { name: "Reset to original" }).click();
  await page.waitForTimeout(800);
  check((await ok("GET", "/api/templates/soap")).layer === "builtin", "the second reset didn't stick");
  const after = await ok("GET", `/api/sessions/${FELINE}`);
  check(after.latest_template?.origin?.version === "1", "the earlier draft lost its label");
  await ctx.close();

  // ---- with a team -------------------------------------------------------------
  step("With a team: a veterinarian (the first, so the admin), a technician and reception");
  const vet = await ok("POST", "/api/users", { name: "Dr Rivera", role: "dvm", pin: "2468" });
  const tech = await ok("POST", "/api/users", { name: "Sam", role: "rvt", pin: "1357", by: vet.id });
  const desk = await ok("POST", "/api/users", { name: "Lee", role: "reception", pin: "1111", by: vet.id });
  check(vet.admin && !tech.admin && !desk.admin, "the first member isn't the only admin");

  step("Technician: change a Dental heading instruction and save");
  ({ ctx, page } = await open(L, tech));
  await openTemplate(page, "Dental procedure");
  const dental = await ok("GET", `/api/templates/dental?by=${tech.id}`);
  const firstHeading = dental.template.sections[0].title;
  await page.getByLabel(`Instruction for ${firstHeading}`).fill("Name each tooth by its Triadan number, as said.");
  await page.waitForTimeout(500);
  await page.getByRole("button", { name: "Save my version" }).click();
  await page.locator(".ob-toast").waitFor();
  check((await toast(page)) === "Saved as your version 1", `technician save toast: ${await toast(page)}`);
  await page.locator(".builder-title .ob-button").first().click();
  await page.locator(".tpl-row").first().waitFor();
  check((await badgesOf(page, "Dental procedure")).includes("Your version"), "Dental isn't marked Your version for the technician");
  await shot(page, L, "04-list-your-version");
  const vetsDental = await ok("GET", `/api/templates/dental?by=${vet.id}`);
  check(!vetsDental.prompt.includes("Triadan number, as said"), "the veterinarian's prompt holds the technician's text");

  step("Technician drafts the dental visit; the veterinarian drafts it again");
  await ok("POST", `/api/sessions/${DENTAL}/notes`, { llm: LLM, template: "dental", by: tech.id });
  let dv = await ok("GET", `/api/sessions/${DENTAL}`);
  check(dv.latest_template?.origin?.layer === "personal" && dv.latest_template.origin.owner?.name === "Sam", "the technician's draft doesn't name their version");
  await ctx.close();

  // Review label as the technician (their version), with Ask's scope choice.
  ({ ctx, page } = await open(L, tech));
  await page.locator('.nav-item[data-nav="patients"]').click();
  await page.locator(".patient-row", { hasText: "Biscuit" }).first().click();
  await page.locator(".visit-row").first().click();
  await page.locator(".draft-origin").waitFor();
  check((await page.locator(".draft-origin").innerText()).includes("Dental procedure, your version 1"), "Review doesn't say 'your version 1'");
  const askButton = page.getByRole("button", { name: /Ask for changes|Ask/ }).first();
  if (await askButton.count()) {
    await askButton.click();
    const saveStd = page.getByLabel(/Save as a standard/);
    if (await saveStd.count()) {
      await saveStd.check();
      await page.getByLabel("For the hospital").check();
      await shot(page, L, "05-review-label-and-ask-scope");
    }
  } else {
    await shot(page, L, "05-review-label");
  }
  await ctx.close();

  await ok("POST", `/api/sessions/${DENTAL}/notes`, { llm: LLM, template: "dental", by: vet.id });
  dv = await ok("GET", `/api/sessions/${DENTAL}`);
  check(dv.latest_template?.origin?.layer === "builtin", "the veterinarian's draft isn't the built-in");

  step("Technician shares Dental: a wrong PIN is refused, the right one shares");
  ({ ctx, page } = await open(L, tech));
  await openTemplate(page, "Dental procedure");
  await page.getByRole("button", { name: "Share with the hospital" }).click();
  const dialog = page.getByRole("dialog");
  await dialog.waitFor();
  await dialog.getByLabel("Admin approving").selectOption(vet.id);
  await dialog.getByLabel("Admin's PIN").fill("0000");
  await shot(page, L, "06-share-dialog");
  {
    const P = variants.phone;
    const { ctx: c, page: p } = await open(P, tech);
    await openTemplate(p, "Dental procedure");
    await p.locator(".builder-notice").scrollIntoViewIfNeeded();
    await shot(p, P, "06-editor-your-version");
    await c.close();
  }
  await dialog.getByRole("button", { name: "Share with the hospital" }).click();
  await dialog.getByText("That PIN isn't right.").waitFor();
  await dialog.getByLabel("Admin's PIN").fill("2468");
  await dialog.getByRole("button", { name: "Share with the hospital" }).click();
  await page.locator(".ob-toast").waitFor();
  check((await toast(page)) === "Shared with the hospital", `share toast: ${await toast(page)}`);
  await ctx.close();
  const vetList = await ok("GET", `/api/templates?by=${vet.id}`);
  check(vetList.find((t) => t.id === "dental").layer === "team", "Dental isn't the hospital's for the veterinarian");
  const techList = await ok("GET", `/api/templates?by=${tech.id}`);
  check(techList.find((t) => t.id === "dental").layer === "team", "the technician still has their own Dental");

  step("Veterinarian (admin): reset the hospital's version with their PIN, Undo, reset again");
  ({ ctx, page } = await open(variants.dark, vet));
  await nav(page, 2);
  check((await badgesOf(page, "Dental procedure")).includes("Edited"), "Dental isn't marked Edited for the veterinarian");
  await shot(page, variants.dark, "07-list-dark");
  await openTemplate(page, "Dental procedure");
  await shot(page, variants.dark, "08-editor-hospital-version-dark");
  await page.getByRole("button", { name: "Reset the hospital's version" }).click();
  const approve = page.getByRole("dialog");
  await approve.getByLabel("Admin's PIN").fill("2468");
  await approve.getByRole("button", { name: "Reset the hospital's version" }).click();
  await page.locator(".ob-toast").waitFor();
  await page.locator(".ob-toast").getByRole("button", { name: "Undo" }).click();
  await page.waitForTimeout(800);
  check((await ok("GET", `/api/templates/dental?by=${vet.id}`)).layer === "team", "Undo didn't bring the hospital's version back");
  await ok("POST", "/api/templates/dental/team/reset", { by: vet.id, approver: { id: vet.id, pin: "2468" } });
  check((await ok("GET", `/api/templates/dental?by=${vet.id}`)).layer === "builtin", "the hospital's version wasn't reset");
  await ctx.close();

  step("Reception reads only; saving is refused in plain words");
  ({ ctx, page } = await open(L, desk));
  await openTemplate(page, "SOAP note");
  check(await page.getByLabel("Instruction for Plan").isDisabled(), "reception can edit");
  await shot(page, L, "09-editor-reception");
  await ctx.close();
  const deskSave = await api("POST", "/api/templates", { template: soapWith(await ok("GET", "/api/templates/soap"), "x"), by: desk.id });
  check(deskSave.status === 400 && deskSave.json?.error === "Lee can read templates; changing them needs a veterinarian or technician.", `reception save: ${deskSave.text}`);

  step("Standards: for me freely, for the hospital with an admin's PIN");
  await ok("POST", "/api/templates/soap/standards", { heading: "", text: "Weights in kilograms with one decimal.", by: tech.id, scope: "personal" });
  check((await ok("GET", `/api/templates/soap?by=${tech.id}`)).prompt.includes("Weights in kilograms with one decimal."), "the technician's standard isn't in their prompt");
  check(!(await ok("GET", `/api/templates/soap?by=${vet.id}`)).prompt.includes("Weights in kilograms"), "the technician's standard reached the veterinarian");
  const teamStd = await api("POST", "/api/templates/soap/standards", { heading: "", text: "Everyone: weights in kilograms.", by: tech.id, scope: "team" });
  check(teamStd.status === 400 && /admin/.test(teamStd.json?.error ?? ""), `team standard without a PIN: ${teamStd.text}`);

  step("A signed note drafted with a personal version doesn't change when that version is reset");
  await ok("POST", "/api/templates", { template: soapWith(await ok("GET", `/api/templates/soap?by=${tech.id}`), "Tech plan words."), by: tech.id });
  await ok("POST", `/api/sessions/${FELINE}/notes`, { llm: LLM, template: "soap", by: tech.id });
  let fv = await ok("GET", `/api/sessions/${FELINE}`);
  const signedRun = fv.latest_run;
  const signable = signedRun?.faithfulness?.passed;
  if (signable) {
    await ok("POST", `/api/sessions/${FELINE}/sign`, { by: vet.id });
    const before = (await ok("GET", `/api/sessions/${FELINE}`)).latest_markdown;
    await ok("POST", "/api/templates/soap/reset", { by: tech.id });
    fv = await ok("GET", `/api/sessions/${FELINE}`);
    check(fv.latest_markdown === before, "the signed note changed after the reset");
    check(fv.latest_template?.origin?.owner?.name === "Sam", "the signed note lost its label");
    await ok("POST", `/api/sessions/${FELINE}/addenda`, { text: "Owner called: eating again.", by: vet.id });
  } else {
    console.log("  (the draft has an unsupported number, so it can't be signed: checked the label only)");
    check(fv.latest_template?.origin?.owner?.name === "Sam", "the draft doesn't name the technician's version");
    await ok("POST", "/api/templates/soap/reset", { by: tech.id });
  }

  step("Shared by the hospital: personal versions off, then back");
  await ok("POST", "/api/templates", { template: soapWith(await ok("GET", `/api/templates/soap?by=${tech.id}`), "Kept while off."), by: tech.id });
  ({ ctx, page } = await open(L, vet));
  await nav(page, 4);
  await page.getByRole("radio", { name: "Shared by the hospital" }).click();
  const modeDialog = page.getByRole("dialog");
  await modeDialog.getByLabel("Admin's PIN").fill("2468");
  await shot(page, L, "10-settings-templates-setting");
  await modeDialog.getByRole("button", { name: "Change templates setting" }).click();
  await page.locator(".ob-toast").waitFor();
  await ctx.close();
  check((await ok("GET", `/api/templates/soap?by=${tech.id}`)).layer === "builtin", "the technician's version is used while shared by the hospital");
  await ok("POST", "/api/team/settings", { by: vet.id, templates: "personal", approver: { id: vet.id, pin: "2468" } });
  check((await ok("GET", `/api/templates/soap?by=${tech.id}`)).layer === "personal", "the technician's version didn't come back");
  await ok("POST", "/api/templates/soap/reset", { by: tech.id });

  step("The veterinarian saves a version of SOAP, for the newer-release check");
  await ok("POST", "/api/templates", { template: soapWith(await ok("GET", `/api/templates/soap?by=${vet.id}`), "Vet plan words."), by: vet.id });
  const builtin = await ok("GET", "/api/templates/soap");
  const next = structuredClone(builtin.template);
  delete next.standards;
  delete next.origin;
  next.version = "0.1.9";
  next.sections.splice(3, 0, { id: "pain_score", title: "Pain score", kind: "statements" });
  mkdirSync(join(demo, "builtins-next", "soap"), { recursive: true });
  // JSON is YAML: the demo reads it as a newer built-in SOAP note.
  writeFileSync(join(demo, "builtins-next", "soap", "template.yaml"), JSON.stringify(next, null, 2));
  writeFileSync(statePath, JSON.stringify({ vet, tech, desk }));
  console.log("Phase 1 done. Restart the demo with `tpl-demo.sh next`, then run PHASE=2.");
}

// ==== phase 2: a newer release of SOAP ================================================

if (phase === "2") {
  const { vet, tech } = JSON.parse(readFileSync(statePath, "utf8"));
  step("After a release changes SOAP: Newer original, Compare, Keep mine, Use the original and Undo");
  const listed = await ok("GET", `/api/templates?by=${vet.id}`);
  check(listed.find((t) => t.id === "soap").original_changed?.reason === "newer_builtin", "no Newer original for the veterinarian's SOAP");
  const diff = await ok("GET", `/api/templates/soap/compare?by=${vet.id}&with=base`);
  check(diff.headings.length === 1 && diff.headings[0].title === "Pain score" && diff.headings[0].changes[0].kind === "added", `compare: ${JSON.stringify(diff)}`);
  for (const v of [variants.light, variants.dark, variants.phone]) {
    const { ctx, page } = await open(v, vet);
    await nav(page, 2);
    check((await badgesOf(page, "SOAP note")).includes("Newer original"), `${v.name}: no Newer original badge`);
    await shot(page, v, "11-list-newer-original");
    await openTemplate(page, "SOAP note");
    await page.locator(".original-notice").waitFor();
    await shot(page, v, "12-editor-newer-original");
    await page.locator(".original-notice").getByRole("button", { name: "Compare" }).click();
    await page.getByText("Pain score").first().waitFor();
    await shot(page, v, "13-compare-sheet");
    await ctx.close();
  }
  const { ctx, page } = await open(variants.light, vet);
  await openTemplate(page, "SOAP note");
  await page.locator(".original-notice").getByRole("button", { name: "Keep mine" }).click();
  await page.locator(".ob-toast").waitFor();
  await page.getByText("Based on an earlier SOAP note.").waitFor();
  check(!(await ok("GET", `/api/templates?by=${vet.id}`)).find((t) => t.id === "soap").original_changed, "Keep mine didn't remove the badge");
  await page.getByRole("button", { name: "Reset to original" }).click();
  await page.locator(".ob-toast").getByRole("button", { name: "Undo" }).click();
  await page.waitForTimeout(800);
  check((await ok("GET", `/api/templates/soap?by=${vet.id}`)).layer === "personal", "Undo after Use the original didn't restore");
  await ctx.close();

  step("Guards: every new write route needs the X-OpenBark header");
  for (const [method, path] of [
    ["POST", "/api/templates"],
    ["POST", "/api/templates/preview"],
    ["POST", "/api/templates/soap/reset"],
    ["POST", "/api/templates/soap/reset/undo"],
    ["POST", "/api/templates/soap/keep"],
    ["POST", "/api/templates/soap/share"],
    ["POST", "/api/templates/soap/team/reset"],
    ["POST", "/api/templates/soap/delete"],
    ["POST", "/api/templates/soap/standards"],
    ["POST", "/api/templates/soap/standards/x/remove"],
    ["POST", "/api/templates/soap/standards/x/mute"],
    ["POST", "/api/team/settings"],
    ["POST", "/api/team/notice/dismiss"],
    ["POST", `/api/users/${tech.id}/admin`],
    ["DELETE", `/api/templates/soap?by=${vet.id}`],
    ["DELETE", `/api/templates/soap/standards/x?by=${vet.id}`],
  ]) {
    const r = await api(method, path, method === "POST" ? { by: vet.id } : undefined, { header: false });
    check(r.status === 403, `${method} ${path} without the header answered ${r.status}`);
  }

  step("Removing the technician: the count, then their profile is gone and visits keep their labels");
  await ok("POST", "/api/templates", { template: soapWith(await ok("GET", `/api/templates/soap?by=${tech.id}`), "Tech again."), by: tech.id });
  const summary = await ok("GET", `/api/users/${tech.id}/profile?by=${vet.id}`);
  check(summary.versions >= 1, `profile summary: ${JSON.stringify(summary)}`);
  const profile = join(demo, "profiles", tech.id);
  check(existsSync(profile), "the technician has no profile folder");
  await ok("DELETE", `/api/users/${tech.id}?by=${vet.id}`);
  check(!existsSync(profile), "the technician's profile folder is still there");
  const visits = readdirSync(join(demo, "visits")).filter((v) => existsSync(join(demo, "visits", v, "session.json")));
  for (const v of visits) {
    const view = await ok("GET", `/api/sessions/${v}`);
    if (view.latest_template?.origin?.owner?.name === "Sam") step(`  ${v} still says it was drafted with Sam's version`);
  }
}

await browser.close();
if (problems.length) {
  console.error(`\n${problems.length} problem(s):\n${problems.map((p) => `  ${p}`).join("\n")}`);
  process.exit(1);
}
console.log(`\nPhase ${phase}: all checks passed. Screenshots in ${out}`);
