// End-to-end test of the dental chart (ADR-0021, plan steps 16 and 17).
//
// Run ONLY against a dental demo, never a clinic's own server or data:
//   OPENBARK_URL=http://localhost:4329 OUT_DIR=<screenshots folder> \
//     node e2e/dental-chart.mjs <demo folder>
//
// The demo is `openbark serve` with every folder in its own scratch folder
// (visits, templates, team file, folder choices, key file), a throwaway
// OPENBARK_DATA_KEY, and an empty file called OPENBARK-DEMO in that folder.
// It imports the synthetic dental fixtures (packages/eval/fixtures/audio,
// voiced with Kokoro by tools/fixture-tts) and drafts them with the Dental
// procedure template on the demo's own models.
//
// A drafted chart is a draft until a veterinarian confirms each drafted tooth
// (ADR-0021 decision 10): the script checks that the engine refuses to sign
// before that, confirms one tooth by keyboard and the rest by click, takes
// screenshots of the draft and confirmed states (light, dark, 360 px), and
// signs from the Sign button.
//
// Before doing anything the script refuses unless:
//   1. the URL is this computer, on port 4321, 4329 or 4331, and never 4317 (the clinic's own server);
//   2. <demo folder> holds an OPENBARK-DEMO marker and isn't inside OpenBark's own data folder;
//   3. GET /api/storage reports a visits folder inside <demo folder>;
//   4. the demo has no team (the fixtures are imported working alone).
import { existsSync, mkdirSync, readFileSync } from "node:fs";
import { dirname, isAbsolute, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "@playwright/test";

const here = dirname(fileURLToPath(import.meta.url));
const base = process.env.OPENBARK_URL ?? "http://localhost:4329";
const demo = process.argv[2] ? resolve(process.argv[2]) : null;
const out = process.env.OUT_DIR ?? (demo ? join(demo, "..", "dental-shots") : "dental-shots");
const fixtures = process.env.FIXTURES ?? resolve(here, "../../../packages/eval/fixtures/audio");
const LLM = process.env.LLM ?? "gemma-4-e4b-it-q4_0";
const OWN_DATA = /[\\/]AppData[\\/]Local[\\/]OpenBark([\\/]|$)/i;
const SIGNER = "Dr Rivera";

// ---- safety checks, before anything is touched -----------------------------

function refuse(why) {
  console.error(`REFUSING TO RUN: ${why}`);
  process.exit(2);
}
const url = new URL(base);
if (url.port === "4317") refuse("port 4317 is the clinic's own OpenBark server");
if (!["4321", "4329", "4331"].includes(url.port))
  refuse(`dental demos run on port 4321, 4329 or 4331, not ${url.port || "the default port"}`);
if (!["localhost", "127.0.0.1"].includes(url.hostname)) refuse("only this computer's demo can be used");
if (!demo) refuse("usage: node e2e/dental-chart.mjs <demo folder>");
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
if (status0.team) refuse("the demo has a team; start a fresh one");
for (const f of ["dental-chart", "dental-feline", "dental-young-dog"])
  if (!existsSync(join(fixtures, `${f}.wav`))) refuse(`no ${f}.wav in ${fixtures}: run tools/fixture-tts --kokoro first`);

mkdirSync(out, { recursive: true });
const problems = [];
const check = (ok, what) => {
  if (!ok) problems.push(what);
  return ok;
};
const step = (what) => console.log(`- ${what}`);

async function api(method, path, body) {
  const headers = { "x-openbark": "1" };
  if (body !== undefined && !(body instanceof Uint8Array)) headers["content-type"] = "application/json";
  const payload = body === undefined ? undefined : body instanceof Uint8Array ? body : JSON.stringify(body);
  const res = await fetch(`${base}${path}`, { method, headers, body: payload });
  const buf = new Uint8Array(await res.arrayBuffer());
  const text = new TextDecoder().decode(buf);
  let json = null;
  try {
    json = JSON.parse(text);
  } catch {
    // A PDF, or a 403 from the guard in plain text.
  }
  return { status: res.status, json, text, buf };
}
const ok = async (method, path, body) => {
  const r = await api(method, path, body);
  if (r.status !== 200) throw new Error(`${method} ${path}: ${r.status} ${r.text.slice(0, 300)}`);
  return r;
};

// ---- seed: import and draft three synthetic dental consults ------------------

/** Import a fixture as a visit (transcribed on the demo), then draft it with the dental template. */
async function seedVisit(fixture, name, species, reason) {
  const existing = (await ok("GET", "/api/sessions")).json.find((s) => s.reason === reason);
  if (existing) return existing.id;
  const q = new URLSearchParams({ name: `${fixture}.wav`, patient_name: name, species, reason, by: SIGNER });
  const wav = new Uint8Array(readFileSync(join(fixtures, `${fixture}.wav`)));
  const view = (await ok("POST", `/api/sessions?${q}`, wav)).json;
  await ok("POST", `/api/sessions/${view.id}/notes`, { llm: LLM, template: "dental", by: SIGNER });
  return view.id;
}
step("Seed: import and draft Juno (dog), Pepper (cat) and Coco (species not entered)");
const JUNO = await seedVisit("dental-chart", "Juno", "Dog", "Dental chart e2e: dog");
const PEPPER = await seedVisit("dental-feline", "Pepper", "Cat", "Dental chart e2e: cat");
const COCO = await seedVisit("dental-young-dog", "Coco", "", "Dental chart e2e: no species");
const latest = async (id) => {
  return (await ok("GET", `/api/sessions/${id}`)).json.latest_run;
};
const chartOf = (run) => run?.note?.dental_chart ?? [];
const junoRun = await latest(JUNO);
const junoTeeth = [...new Set(chartOf(junoRun).map((i) => i.tooth))].sort();
check(junoTeeth.length > 0, "Juno's draft charted no teeth");
check(junoRun?.faithfulness?.passed !== false, "Juno's draft has a hard fail");
console.log(`  Juno charted: ${junoTeeth.join(", ")}`);

// ---- the browser ---------------------------------------------------------------

const variants = {
  light: { name: "desktop-light", viewport: { width: 1440, height: 900 }, colorScheme: "light" },
  dark: { name: "desktop-dark", viewport: { width: 1440, height: 900 }, colorScheme: "dark" },
  phone: { name: "phone-360", viewport: { width: 360, height: 780 }, colorScheme: "light", isMobile: true, hasTouch: true },
  phoneDark: { name: "phone-360-dark", viewport: { width: 360, height: 780 }, colorScheme: "dark", isMobile: true, hasTouch: true },
};
const browser = await chromium.launch();

async function open(v) {
  const ctx = await browser.newContext({ viewport: v.viewport, colorScheme: v.colorScheme, isMobile: v.isMobile, hasTouch: v.hasTouch });
  await ctx.addInitScript((signer) => {
    const key = "openbark.settings";
    const current = JSON.parse(localStorage.getItem(key) ?? "{}");
    localStorage.setItem(key, JSON.stringify({ ...current, consentAcknowledgedAt: "2026-10-17T20:00:00Z", uiLanguage: "en", signer }));
  }, SIGNER);
  const page = await ctx.newPage();
  page.on("pageerror", (e) => problems.push(`${v.name}: ${e}`));
  page.on("console", (m) => m.type() === "error" && !/Failed to load resource/.test(m.text()) && problems.push(`${v.name} console: ${m.text()}`));
  await page.goto(base);
  await page.locator(".screen h1").first().waitFor();
  return { ctx, page };
}
const nav = async (page, i) => {
  await page.locator(".nav-item").nth(i).click();
  await page.locator(".screen h1").first().waitFor();
};
async function openVisit(page, v, patient, reason) {
  await nav(page, 1);
  await page.locator(".patient-row", { hasText: patient }).first().click();
  await page.locator(".patient-page .visit-list, .patient-page .empty-state").first().waitFor();
  await page.locator(".visit-row", { hasText: reason }).first().click();
  await page.locator(".review .visit-facts").waitFor();
  if (v.isMobile) await page.getByRole("tab", { name: "Note" }).click();
  await page.locator("#note-dental_chart").waitFor();
}
/** A screenshot with `locator` in view; `top`: its top edge near the top of the screen. */
async function shot(page, v, name, locator, { top = false } = {}) {
  await page.waitForTimeout(400);
  if (locator && top)
    await locator.evaluate((el) => {
      el.scrollIntoView({ block: "start" });
      // The note pane or the page scrolls; leave room above for a sticky bar.
      for (let e = el.parentElement; e; e = e.parentElement) if (e.scrollTop > 0) e.scrollTop -= 72;
      if (document.scrollingElement.scrollTop > 0) document.scrollingElement.scrollTop -= 72;
    });
  else if (locator) await locator.scrollIntoViewIfNeeded();
  await page.screenshot({ path: join(out, `${v.name}-${name}.png`), fullPage: false });
  const wide = await page.evaluate(() => document.scrollingElement.scrollWidth > window.innerWidth);
  check(!wide, `${v.name} ${name}: the page scrolls sideways`);
}
const charted = (page) => page.locator("#note-dental_chart .dc-tooth[data-charted]").evaluateAll((els) => els.map((e) => e.dataset.tooth).sort());

// ---- 1. Juno's chart in every variant ------------------------------------------

for (const v of [variants.light, variants.dark, variants.phone, variants.phoneDark]) {
  step(`${v.name}: Juno's chart, list and legend`);
  const { ctx, page } = await open(v);
  await openVisit(page, v, "Juno", "Dental chart e2e: dog");
  const chart = page.locator("#note-dental_chart");
  await chart.locator(".dc-svg").first().waitFor();
  const drawn = await charted(page);
  check(JSON.stringify([...new Set(drawn)]) === JSON.stringify(junoTeeth), `${v.name}: marks on ${drawn.join(",")} but the draft charted ${junoTeeth.join(",")}`);
  const strayMarks = await chart.locator(".dc-tooth:not([data-charted]) .dc-mark").count();
  check(strayMarks === 0, `${v.name}: ${strayMarks} marks on teeth that weren't charted`);
  check((await chart.locator(".dc-legend").innerText()).includes("Blank: not charted in this recording"), `${v.name}: the legend has no "Blank" line`);
  // The mouth splits by the chart's own width (768 px), not the window's.
  const blocks = await chart.locator(".dc-svg").evaluateAll((els) => els.map((e) => e.dataset.block));
  const chartWidth = await chart.locator(".dc-diagrams").evaluate((e) => e.getBoundingClientRect().width);
  const wanted = chartWidth < 768 ? "right,left" : "both";
  check(blocks.join() === wanted, `${v.name}: a ${Math.round(chartWidth)} px chart shows blocks ${blocks.join()}, not ${wanted}`);
  if (v.isMobile) check(blocks.join() === "right,left", `${v.name}: phone width shows blocks ${blocks.join()}`);
  // Each tooth is at least 24 px wide (WCAG 2.2 target size).
  const narrow = await chart.locator(".dc-tooth .dc-hit").evaluateAll((els) => els.filter((e) => e.getBoundingClientRect().width < 23.5).length);
  check(narrow === 0, `${v.name}: ${narrow} teeth narrower than 24 px`);
  if (v.colorScheme === "dark") {
    const strokes = await page.evaluate(() => {
      const probe = document.createElement("span");
      probe.style.color = "var(--ob-text-3)";
      document.body.append(probe);
      const want = getComputedStyle(probe).color;
      probe.remove();
      const blank = document.querySelector("#note-dental_chart .dc-tooth:not([data-charted]) .dc-outline");
      return { want, got: blank ? getComputedStyle(blank).stroke : null };
    });
    check(strokes.got === strokes.want, `${v.name}: an uncharted tooth's stroke ${strokes.got} isn't --ob-text-3 ${strokes.want}`);
  }
  await shot(page, v, "01-chart", page.locator("#note-dental_chart"), { top: true });
  await chart.locator(".dc-list").scrollIntoViewIfNeeded();
  await shot(page, v, "02-list-and-legend", chart.locator(".dc-legend"));
  if (v.name === "desktop-light" || v.name === "desktop-dark") {
    // Hovering a tooth lights the transcript lines of its findings.
    const first = junoTeeth[0];
    await chart.locator(`.dc-tooth[data-tooth="${first}"]`).first().hover();
    const lit = await page.locator(".transcript li[data-lit]").count();
    check(lit > 0, `${v.name}: hovering ${first} lit no transcript line`);
    // Tapping a finding in the list lights the line it came from.
    await page.mouse.move(0, 0);
    await chart.locator(".dc-list .stmt").first().focus();
    check((await page.locator(".transcript li[data-lit]").count()) > 0, `${v.name}: focusing a finding lit no transcript line`);
    await shot(page, v, "03-evidence-lit", chart);
  }
  if (v.isMobile) {
    // On a phone, a tap on a finding focuses it and lights its line (shown on the Transcript tab).
    await chart.locator(".dc-list .stmt").first().tap();
    await page.waitForTimeout(200);
  }
  await ctx.close();
}

// ---- 2. Editing by clicking a tooth (desktop light, then dark for the shot) -----

step("Desktop light: click a tooth, add a finding, save; mark two teeth missing");
{
  const v = variants.light;
  const { ctx, page } = await open(v);
  await openVisit(page, v, "Juno", "Dental chart e2e: dog");
  const chart = page.locator("#note-dental_chart");
  const target = junoTeeth.includes("304") ? "304" : junoTeeth[0];
  await chart.locator(`.dc-tooth[data-tooth="${target}"]`).first().click();
  const editor = page.locator(".dce");
  await editor.waitFor();
  check((await editor.locator(".dce-selection").innerText()).includes(target), `the editor didn't open with ${target} selected`);
  // Each finding the model charted shows the sentence it came from.
  const sources = await editor.locator(".dce-row-source").count();
  check(sources > 0, "the editor shows no source sentence for the model's findings");
  await editor.getByRole("button", { name: "Add finding" }).click();
  await editor.locator(".dce-form select").first().selectOption("extraction");
  await editor.locator(".dce-form select").nth(1).selectOption("open");
  await shot(page, v, "04-editor-add-form", editor);
  await editor.locator(".dce-form").getByRole("button", { name: "Add" }).click();
  await editor.getByRole("button", { name: "Save changes" }).click();
  await editor.waitFor({ state: "detached" });
  await page.locator(".ob-toast").first().waitFor();
  const afterAdd = chartOf(await latest(JUNO));
  check(
    afterAdd.some((i) => i.tooth === target && i.finding === "extraction" && i.detail === "open" && i.entered_by === SIGNER),
    `the added ${target} open extraction isn't saved as ${SIGNER}'s`,
  );
  check((await chart.locator(`.dc-line[data-tooth="${target}"]`).innerText()).includes(`Edited by ${SIGNER}`), `${target}'s line doesn't say Edited by`);

  // Ctrl-click two teeth nobody mentioned, Mark missing, Save.
  const free = ["105", "106"].filter((t) => !junoTeeth.includes(t));
  await chart.locator(`.dc-tooth[data-tooth="${free[0]}"]`).first().click();
  await editor.waitFor();
  if (free[1]) await editor.locator(`.dc-tooth[data-tooth="${free[1]}"]`).first().click({ modifiers: ["Control"] });
  await editor.getByRole("button", { name: "Mark missing" }).click();
  await editor.getByRole("button", { name: "Save changes" }).click();
  await editor.waitFor({ state: "detached" });
  const afterMissing = await latest(JUNO);
  for (const t of free)
    check(chartOf(afterMissing).some((i) => i.tooth === t && i.finding === "missing" && i.entered_by === SIGNER), `${t} wasn't marked missing as the vet's own`);
  check(afterMissing.faithfulness?.passed !== false, "the draft can't be signed after the vet's edits");

  // Keyboard: Tab into the chart, arrows, Enter opens the editor, Cancel closes it.
  await chart.locator(".dc-tooth[tabindex='0']").first().focus();
  await page.keyboard.press("Home");
  await page.keyboard.press("ArrowRight");
  await page.keyboard.press("Enter");
  await editor.waitFor();
  await editor.getByRole("button", { name: "Cancel" }).click();
  await editor.waitFor({ state: "detached" });
  await ctx.close();
}
{
  const v = variants.dark;
  const { ctx, page } = await open(v);
  await openVisit(page, v, "Juno", "Dental chart e2e: dog");
  await page.locator("#note-dental_chart .dc-tooth[data-tooth='309']").first().click();
  const editor = page.locator(".dce");
  await editor.waitFor();
  await editor.getByRole("button", { name: "Add finding" }).click();
  await shot(page, v, "04-editor-add-form", editor);
  await editor.getByRole("button", { name: "Cancel" }).click();
  await ctx.close();
}
{
  const v = variants.phone;
  const { ctx, page } = await open(v);
  await openVisit(page, v, "Juno", "Dental chart e2e: dog");
  await page.locator("#note-dental_chart .dc-tooth[data-tooth='309']").first().tap();
  const editor = page.locator(".dce");
  await editor.waitFor();
  await shot(page, v, "04-editor", editor);
  await editor.getByRole("button", { name: "Cancel" }).click();
  await ctx.close();
}

// ---- 3. The cat and the visit with no species ------------------------------------

step("Pepper (cat): a cat's 30 permanent teeth, no 105");
{
  const v = variants.light;
  const { ctx, page } = await open(v);
  await openVisit(page, v, "Pepper", "Dental chart e2e: cat");
  const chart = page.locator("#note-dental_chart");
  await chart.locator(".dc-svg").first().waitFor();
  const ids = await chart.locator(".dc-tooth").evaluateAll((els) => els.map((e) => e.dataset.tooth));
  const permanent = ids.filter((id) => Number(id) < 500);
  check(permanent.length === 30, `the cat chart draws ${permanent.length} permanent teeth`);
  check(!ids.includes("105") && !ids.includes("305"), "the cat chart draws a dog-only tooth");
  await shot(page, v, "05-cat-chart", chart, { top: true });
  await ctx.close();
}
step("Coco (no species): no diagram, and the editor asks dog or cat");
{
  const v = variants.light;
  const { ctx, page } = await open(v);
  await openVisit(page, v, "Coco", "Dental chart e2e: no species");
  const chart = page.locator("#note-dental_chart");
  const items = chartOf(await latest(COCO));
  if (items.length) {
    check((await chart.locator(".dc-svg").count()) === 0, "a chart with no dentition draws a diagram");
    check((await chart.innerText()).includes("Dentition not recorded"), "no 'Dentition not recorded' notice");
  }
  await page.getByRole("button", { name: "Edit Dental chart" }).click();
  const editor = page.locator(".dce");
  await editor.waitFor();
  check(await editor.getByText("Dog or cat?").isVisible(), "the editor doesn't ask dog or cat");
  check(await editor.getByRole("button", { name: "Save changes" }).isDisabled(), "Save is enabled before choosing dog or cat");
  await shot(page, v, "06-dog-or-cat", editor);
  await editor.getByRole("button", { name: "Cancel" }).click();
  await ctx.close();
}

// ---- 4. The draft chart: each drafted tooth confirmed by the vet --------------------

/** Wait until `fn` is true (or fail after `ms`). */
async function until(fn, ms, what) {
  const end = Date.now() + ms;
  while (Date.now() < end) {
    if (await fn()) return true;
    await new Promise((r) => setTimeout(r, 100));
  }
  return check(false, what);
}
const boxFor = (chart, tooth) => chart.getByRole("checkbox", { name: `${tooth}: checked against the recording` });

step("The draft chart: the engine refuses to sign until each drafted tooth is confirmed");
const draft = (await ok("GET", `/api/sessions/${JUNO}`)).json;
check(draft.unconfirmed_teeth.length > 0, "Juno's draft has no unconfirmed teeth");
console.log(`  Juno to confirm: ${draft.unconfirmed_teeth.join(", ")}`);
{
  const refused = await api("POST", `/api/sessions/${JUNO}/sign`, { by: SIGNER });
  check(refused.status === 400 && refused.text.includes("Not confirmed yet"), `signing with unconfirmed teeth gave ${refused.status} ${refused.text.slice(0, 200)}`);
  check(!(await ok("GET", `/api/sessions/${JUNO}`)).json.signed_at, "the note was signed with unconfirmed teeth");
}
for (const v of [variants.light, variants.dark, variants.phone, variants.phoneDark]) {
  step(`${v.name}: "Draft chart: check every tooth", a checkbox per drafted tooth, Sign off`);
  const { ctx, page } = await open(v);
  await openVisit(page, v, "Juno", "Dental chart e2e: dog");
  const chart = page.locator("#note-dental_chart");
  const banner = chart.locator(".dc-draft[data-state='draft']");
  check(await banner.isVisible(), `${v.name}: no draft notice on the chart`);
  check((await banner.innerText()).includes("Draft chart: check every tooth"), `${v.name}: the draft notice doesn't say "Draft chart: check every tooth"`);
  const boxes = chart.getByRole("checkbox", { name: /: checked against the recording$/ });
  check((await boxes.count()) === draft.unconfirmed_teeth.length, `${v.name}: ${await boxes.count()} checkboxes for ${draft.unconfirmed_teeth.length} drafted teeth`);
  const small = await chart.locator(".dc-confirm-check").evaluateAll((els) => els.filter((e) => e.getBoundingClientRect().height < 23.5).length);
  check(small === 0, `${v.name}: ${small} confirm targets shorter than 24 px`);
  // No tooth is shown as checked until the vet confirms it.
  check((await chart.locator(".dc-confirmed").count()) === 0, `${v.name}: a tooth shows as confirmed before anyone confirmed it`);
  const sign = page.getByRole("button", { name: "Sign note" });
  if (await sign.count()) check(await sign.isDisabled(), `${v.name}: Sign is on with unconfirmed teeth`);
  check((await page.locator(".note-summary").innerText()).includes("aren't confirmed yet"), `${v.name}: the summary doesn't say teeth aren't confirmed`);
  await shot(page, v, "08-draft-confirm", banner, { top: true });
  await chart.locator(".dc-list").scrollIntoViewIfNeeded();
  await shot(page, v, "08b-draft-confirm-list", chart.locator(`.dc-line[data-tooth="${draft.unconfirmed_teeth[0]}"]`));
  await ctx.close();
}
step("Desktop light: confirm one tooth by keyboard (Space), the rest by click; Sign comes on");
{
  const v = variants.light;
  const { ctx, page } = await open(v);
  await openVisit(page, v, "Juno", "Dental chart e2e: dog");
  const chart = page.locator("#note-dental_chart");
  const first = draft.unconfirmed_teeth[0];
  await boxFor(chart, first).focus();
  await page.keyboard.press("Space");
  await until(() => boxFor(chart, first).isChecked(), 5000, `Space didn't confirm ${first}`);
  const mid = (await ok("GET", `/api/sessions/${JUNO}`)).json;
  check(!mid.unconfirmed_teeth.includes(first), `${first} is still unconfirmed after Space`);
  check(mid.chart_checks.some((c) => c.tooth === first && c.by === SIGNER && c.at), `${first}'s confirmation doesn't record who and when`);
  check((await chart.locator(`.dc-line[data-tooth="${first}"] .dc-confirmed`).innerText()).startsWith(`Confirmed by ${SIGNER}`), `${first}'s line doesn't say who confirmed it`);
  // Taken back and confirmed again.
  await boxFor(chart, first).click();
  await until(async () => !(await boxFor(chart, first).isChecked()), 5000, `unticking ${first} didn't take it back`);
  check((await ok("GET", `/api/sessions/${JUNO}`)).json.unconfirmed_teeth.includes(first), `${first} wasn't taken back`);
  for (const t of draft.unconfirmed_teeth) {
    await boxFor(chart, t).click();
    await until(() => boxFor(chart, t).isChecked(), 5000, `clicking ${t} didn't confirm it`);
  }
  const done = (await ok("GET", `/api/sessions/${JUNO}`)).json;
  check(done.unconfirmed_teeth.length === 0, `still unconfirmed: ${done.unconfirmed_teeth.join(", ")}`);
  check(await chart.locator(".dc-draft[data-state='confirmed']").isVisible(), "no 'Every drafted tooth is confirmed' line");
  check(!(await page.getByRole("button", { name: "Sign note" }).isDisabled()), "Sign is still off after every tooth is confirmed");
  await shot(page, v, "09-confirmed", chart, { top: true });
  await ctx.close();
}
for (const v of [variants.dark, variants.phone]) {
  const { ctx, page } = await open(v);
  await openVisit(page, v, "Juno", "Dental chart e2e: dog");
  const chart = page.locator("#note-dental_chart");
  await chart.locator(".dc-list").scrollIntoViewIfNeeded();
  await shot(page, v, "09-confirmed", chart.locator(`.dc-line[data-tooth="${draft.unconfirmed_teeth[0]}"]`));
  await ctx.close();
}

// ---- 5. Signed: read-only; the PDF has the chart ---------------------------------

step("Sign Juno's note from the Sign button: the chart is read-only, and the PDF draws it");
{
  const v = variants.light;
  const { ctx, page } = await open(v);
  await openVisit(page, v, "Juno", "Dental chart e2e: dog");
  await page.getByRole("button", { name: "Sign note" }).click();
  await page.getByText(`Signed by ${SIGNER}`).first().waitFor();
  check(!!(await ok("GET", `/api/sessions/${JUNO}`)).json.signed_at, "the Sign button didn't sign the note");
  const chart = page.locator("#note-dental_chart");
  check((await chart.getByRole("checkbox").count()) === 0, "a signed chart still has confirm checkboxes");
  check((await chart.locator(".dc-confirmed").count()) > 0, "a signed chart doesn't show who confirmed its teeth");
  const after = await api("POST", `/api/sessions/${JUNO}/chart/confirm`, { teeth: [draft.unconfirmed_teeth[0]], confirmed: false, by: SIGNER });
  check(after.status === 400 && after.text.includes("signed"), `a signed chart's confirmation could be taken back: ${after.status}`);
  check((await page.getByRole("button", { name: "Edit Dental chart" }).count()) === 0, "a signed note still offers Edit");
  await chart.locator(".dc-tooth[data-tooth='309']").first().click();
  await page.waitForTimeout(300);
  check((await page.locator(".dce").count()) === 0, "clicking a tooth on a signed note opened the editor");
  check((await chart.locator(".dc-line[data-tooth='309'][data-selected]").count()) === 1, "clicking a signed chart's tooth didn't select its line");
  await shot(page, v, "07-signed", chart, { top: true });
  await ctx.close();
  const pdf = await ok("POST", `/api/sessions/${JUNO}/pdf`, { language: "en" });
  const raw = new TextDecoder("latin1").decode(pdf.buf);
  for (const want of ["Dental chart", "Blank: not charted"]) check(raw.includes(want), `the PDF lacks "${want}"`);
  check(/ re?\s|\d m\s|\d l\s|\d c\s/.test(raw) && / S\b|\bS\n/.test(raw), "the PDF has no path operators");
}

await browser.close();
if (problems.length) {
  console.error(`\n${problems.length} problem(s):`);
  for (const p of problems) console.error(`  ${p}`);
  process.exit(1);
}
console.log(`\nDENTAL CHART E2E PASSED. Screenshots: ${out}`);
