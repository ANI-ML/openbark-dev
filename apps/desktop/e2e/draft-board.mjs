// The drafting board (Review): a redraft of a visit that already has a
// draft, and a first draft, watched step by step. Run against a demo server,
// never a clinic's own data:
//   OPENBARK_URL=http://localhost:4321 node e2e/draft-board.mjs
import { mkdirSync } from "node:fs";
import { chromium } from "@playwright/test";

const base = process.env.OPENBARK_URL ?? "http://localhost:4321";
const out = process.env.OUT_DIR ?? "../../target/draft-board";
mkdirSync(out, { recursive: true });

const problems = [];
const check = (ok, what) => ok || problems.push(what);
const browser = await chromium.launch();

async function watch(page, name) {
  const seen = [];
  const started = Date.now();
  let shots = 0;
  let headingsSeen = 0;
  // The board appears, ticks through its steps, and goes away when the draft is saved.
  await page.locator(".draft-board-wrap .board, .ob-sheet .board").first().waitFor({ timeout: 15000 });
  while (Date.now() - started < 8 * 60_000) {
    const board = page.locator(".draft-board-wrap .board, .ob-sheet .board").first();
    if (!(await board.count())) break;
    const active = await board.locator(".board-step[data-state=active] .board-step-label").first().innerText().catch(() => "");
    const detail = await board.locator(".board-step[data-state=active] .small").first().innerText().catch(() => "");
    const doneHeadings = await board.locator(".board-sub li[data-state=done]").count();
    headingsSeen = Math.max(headingsSeen, doneHeadings);
    const key = `${active} | ${detail}`;
    if (seen[seen.length - 1]?.key !== key) {
      seen.push({ key, at: Math.round((Date.now() - started) / 1000), pct: await board.locator("[role=progressbar]").getAttribute("aria-valuenow") });
      // Frames: reading, early writing, late writing.
      if (shots < 4 && (active.startsWith("Reading") || active.startsWith("Writing") || active.startsWith("Checking"))) {
        await page.screenshot({ path: `${out}/${name}-${++shots}.png` });
      }
    }
    await page.waitForTimeout(200);
  }
  console.log(`${name}: board for ${Math.round((Date.now() - started) / 1000)} s; headings ticked: ${headingsSeen}`);
  for (const s of seen) console.log(`  ${String(s.at).padStart(4)} s  ${String(s.pct).padStart(3)}%  ${s.key}`);
  check(seen.some((s) => s.key.startsWith("Reading")), `${name}: reading step shown`);
  check(seen.some((s) => s.key.startsWith("Writing")), `${name}: writing step shown`);
  check(headingsSeen >= 2, `${name}: headings ticked off while writing (${headingsSeen})`);
}

const ctx = await browser.newContext({ viewport: { width: 1440, height: 900 } });
await ctx.addInitScript(() => {
  const key = "openbark.settings";
  const current = JSON.parse(localStorage.getItem(key) ?? "{}");
  localStorage.setItem(key, JSON.stringify({ ...current, consentAcknowledgedAt: "2026-10-17T20:00:00Z", uiLanguage: "en" }));
});
const page = await ctx.newPage();
page.on("pageerror", (e) => problems.push(`${e}`));

// 1. Draft again on a visit that has a draft: the old note dims under the board.
await page.goto(base);
await page.locator('.nav-item[data-nav="patients"]').click();
await page.locator('.nav-item[data-nav="visits"]').click();
await page.locator('tr.row-link:has(.ob-dot[data-tone="warn"])').first().click();
await page.locator(".note").first().waitFor({ timeout: 20000 });
await page.getByRole("button", { name: /^Draft again|^Draft as/ }).first().click();
await page.locator(".note-under-redraft").waitFor({ timeout: 10000 });
check((await page.locator(".draft-board-wrap .board-title").innerText()).startsWith("Drafting again"), "redraft title");
await watch(page, "redraft");
check((await page.locator(".note-under-redraft").count()) === 0, "the new draft replaces the dimmed one");

// 2. A first draft: a visit with no draft yet (Recent visits, "Transcribed").
await page.locator('.nav-item[data-nav="patients"]').click();
await page.locator('.nav-item[data-nav="visits"]').click();
const fresh = page.locator('tr.row-link:not(:has(.ob-dot[data-tone="warn"])):not(:has(.ob-dot[data-tone="ok"]))').first();
if (await fresh.count()) {
  await fresh.click();
  await page.locator(".review-bar").waitFor();
  await page.getByRole("button", { name: /^Draft/ }).first().click();
  await watch(page, "first-draft");
} else {
  console.log("no undrafted visit in the demo; skipped the first-draft check");
}
await browser.close();
console.log(problems.length ? `PROBLEMS:\n${problems.join("\n")}` : "all checks passed");
