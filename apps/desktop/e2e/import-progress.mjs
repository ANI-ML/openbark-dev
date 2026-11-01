// Import a long recording through the web UI and watch the progress bar:
// every step should show, the percentage should only go up, and the visit
// should open when it's done. Run against a demo server, never a clinic's
// own data:
//   OPENBARK_URL=http://localhost:4321 node e2e/import-progress.mjs <recording> [microphone.wav]
// PHONE_ON_SPEAKER=1 ticks "Phone on speaker" first (ADR-0026) and checks
// that the import and the recording ask the engine for it.
import { mkdirSync } from "node:fs";
import { chromium } from "@playwright/test";

const base = process.env.OPENBARK_URL ?? "http://localhost:4321";
const out = process.env.OUT_DIR ?? "../../target/progress";
const file = process.argv[2];
const phoneOnSpeaker = process.env.PHONE_ON_SPEAKER === "1";
if (!file) throw new Error("usage: node e2e/import-progress.mjs <recording>");
mkdirSync(out, { recursive: true });

const browser = await chromium.launch();
const problems = [];
for (const v of [
  { name: "desktop", viewport: { width: 1440, height: 900 }, colorScheme: "light" },
  { name: "mobile", viewport: { width: 390, height: 844 }, colorScheme: "dark", isMobile: true, hasTouch: true },
]) {
  const ctx = await browser.newContext({
    viewport: v.viewport,
    colorScheme: v.colorScheme,
    isMobile: v.isMobile,
    hasTouch: v.hasTouch,
  });
  await ctx.addInitScript(() => {
    const key = "openbark.settings";
    const current = JSON.parse(localStorage.getItem(key) ?? "{}");
    localStorage.setItem(
      key,
      JSON.stringify({
        ...current,
        updateCheck: "off", consentAcknowledgedAt: "2026-10-17T20:00:00Z",
        setupDoneAt: "2026-10-17T20:00:00Z",
        uiLanguage: "en",
        draftOnStop: false,
      }),
    );
  });
  const page = await ctx.newPage();
  page.on("pageerror", (e) => problems.push(`${v.name}: ${e}`));
  const uploads = [];
  page.on("request", (r) => {
    if (r.method() === "POST" && new URL(r.url()).pathname === "/api/sessions") uploads.push(r.url());
  });
  await page.goto(base);
  await page.locator(".recorder").waitFor();
  if (phoneOnSpeaker) await tickPhoneOnSpeaker(page, v.name);

  const chooser = page.waitForEvent("filechooser");
  await page.getByRole("button", { name: "Import a recording" }).click();
  await (await chooser).setFiles(file);

  // Sample the bar until the visit opens.
  const seen = [];
  let last = -1;
  let shots = 0;
  const started = Date.now();
  while (Date.now() - started < 15 * 60_000) {
    if (await page.locator(".review-bar, .pane-pad").count()) break;
    const bar = page.locator(".processing [role=progressbar]");
    if (await bar.count()) {
      const pct = Number(await bar.getAttribute("aria-valuenow"));
      const text = (await page.locator(".processing-text").innerText()).replace(/\s+/g, " ");
      const step = text.split(":")[0];
      if (!seen.length || seen[seen.length - 1].step !== step) seen.push({ step, pct, text, at: Date.now() - started });
      if (pct < last) problems.push(`${v.name}: went back from ${last}% to ${pct}% (${text})`);
      last = pct;
      // A few frames: early, middle, and late.
      if ((shots === 0 && pct >= 8) || (shots === 1 && pct >= 45) || (shots === 2 && pct >= 85)) {
        await page.screenshot({ path: `${out}/${v.name}-${++shots}.png` });
      }
    }
    await page.waitForTimeout(250);
  }
  const opened = (await page.locator(".review-bar, .pane-pad").count()) > 0;
  if (!opened) problems.push(`${v.name}: the visit didn't open`);
  const asked = uploads.some((u) => new URL(u).searchParams.get("speakerphone") === String(phoneOnSpeaker));
  if (!asked) problems.push(`${v.name}: the upload didn't say speakerphone=${phoneOnSpeaker} (${uploads.join(", ")})`);
  console.log(`${v.name}: ${opened ? "opened" : "did not open"} after ${Math.round((Date.now() - started) / 1000)} s`);
  for (const s of seen)
    console.log(`  ${String(Math.round(s.at / 1000)).padStart(4)} s  ${String(s.pct).padStart(3)}%  ${s.text}`);
  await ctx.close();
}
await browser.close();

// A recording that stops: the finishing steps show the same bar. Chromium
// plays a WAV as the microphone.
const mic = process.argv[3];
if (mic) {
  const rec = await chromium.launch({
    args: [
      "--use-fake-ui-for-media-stream",
      "--use-fake-device-for-media-stream",
      `--use-file-for-fake-audio-capture=${mic}`,
    ],
  });
  const ctx = await rec.newContext({ viewport: { width: 1440, height: 900 }, permissions: ["microphone"] });
  await ctx.addInitScript(() => {
    const key = "openbark.settings";
    const current = JSON.parse(localStorage.getItem(key) ?? "{}");
    localStorage.setItem(
      key,
      JSON.stringify({
        ...current,
        updateCheck: "off", consentAcknowledgedAt: "2026-10-17T20:00:00Z",
        setupDoneAt: "2026-10-17T20:00:00Z",
        uiLanguage: "en",
        draftOnStop: false,
      }),
    );
  });
  const page = await ctx.newPage();
  page.on("pageerror", (e) => problems.push(`record: ${e}`));
  const sockets = [];
  page.on("websocket", (ws) => sockets.push(ws.url()));
  await page.goto(base);
  await page.locator(".recorder").waitFor();
  if (phoneOnSpeaker) await tickPhoneOnSpeaker(page, "record");
  await page.getByRole("button", { name: "Start recording" }).click();
  await page.getByRole("button", { name: "Stop recording" }).waitFor();
  await page.waitForTimeout(25_000);
  await page.getByRole("button", { name: "Stop recording" }).click();
  const seen = [];
  const started = Date.now();
  while (Date.now() - started < 5 * 60_000) {
    if (await page.locator(".review-bar, .pane-pad").count()) break;
    const bar = page.locator(".processing [role=progressbar]");
    if (await bar.count()) {
      const text = (await page.locator(".processing-text").innerText()).replace(/\s+/g, " ");
      if (seen[seen.length - 1] !== text) seen.push(text);
      if (seen.length === 1) await page.screenshot({ path: `${out}/record-stop.png` });
    }
    await page.waitForTimeout(100);
  }
  const opened = (await page.locator(".review-bar, .pane-pad").count()) > 0;
  if (!opened) problems.push("record: the visit didn't open after stopping");
  const live = sockets.find((u) => new URL(u).pathname === "/api/live");
  if (!live || new URL(live).searchParams.get("speakerphone") !== String(phoneOnSpeaker))
    problems.push(`record: the live stream didn't say speakerphone=${phoneOnSpeaker} (${live})`);
  console.log(
    `record: ${opened ? "opened" : "did not open"} ${Math.round((Date.now() - started) / 1000)} s after stop`,
  );
  for (const s of seen) console.log(`  ${s}`);
  await rec.close();
}
console.log(problems.length ? `PROBLEMS:\n${problems.join("\n")}` : "all checks passed");

/** Tick "Phone on speaker" on Record and check its hint (ADR-0026). */
async function tickPhoneOnSpeaker(page, name) {
  const box = page.getByRole("checkbox", { name: "Phone on speaker" });
  // Phones fold the options away less often than the patient, but scroll to it.
  await box.scrollIntoViewIfNeeded();
  if (!(await box.isChecked())) await box.check();
  const hint = page.getByText("Picks up the quieter voice of a client on a phone on speaker.", { exact: false });
  if (!(await hint.isVisible())) problems.push(`${name}: the Phone on speaker hint isn't shown`);
}
