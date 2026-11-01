// End-to-end microphone checks in web mode, with fake microphones only:
// Chromium plays a WAV as the microphone, Firefox uses its fake stream.
//
//   OPENBARK_URL=http://localhost:4347 SPEECH_WAV=<speech.wav> SILENT_WAV=<silence.wav> \
//     node e2e/mic-robust.mjs <demo folder>
//
// Covers: the permission prompt dismissed then granted; the microphone test
// (meter and verdict); a recording whose microphone is unplugged mid-way and
// comes back (it must keep going and keep every word); unplugged with no
// microphone left (paused, then resumed); digital silence (warned within a
// few seconds); an AudioContext that stays suspended until a gesture
// (Safari); the tab in the background; and Firefox end to end.
//
// The demo is `openbark serve --port 4347` with every folder in scratch
// (OPENBARK_SESSIONS_DIR, OPENBARK_TEMPLATES_DIR, OPENBARK_USERS_FILE,
// OPENBARK_LOCATIONS_FILE, OPENBARK_KEY_FILE, a throwaway OPENBARK_DATA_KEY)
// and an empty OPENBARK-DEMO file in its folder. The script refuses to run
// unless the URL is this computer on port 4347, the folder has the marker,
// isn't OpenBark's own data folder, and is what the server uses for visits.
import { existsSync, mkdirSync } from "node:fs";
import { join, relative, resolve } from "node:path";
import { chromium, firefox, webkit } from "@playwright/test";

const base = process.env.OPENBARK_URL ?? "http://localhost:4347";
const out = process.env.OUT_DIR ?? "../../target/mic-robust";
const demo = process.argv[2] ? resolve(process.argv[2]) : null;
const speech = process.env.SPEECH_WAV;
const silence = process.env.SILENT_WAV;
const only = (process.env.ONLY ?? "").split(",").filter(Boolean);
const OWN_DATA = /[\\/]AppData[\\/]Local[\\/]OpenBark([\\/]|$)/i;

function refuse(why) {
  console.error(`REFUSING TO RUN: ${why}`);
  process.exit(2);
}
const url = new URL(base);
if (url.port === "4317") refuse("port 4317 is the clinic's own OpenBark server");
if (url.port !== "4347") refuse(`the microphone demo runs on port 4347, not ${url.port || "the default port"}`);
if (!["localhost", "127.0.0.1"].includes(url.hostname)) refuse("only this computer's demo can be used");
if (!demo) refuse("usage: node e2e/mic-robust.mjs <demo folder>");
if (OWN_DATA.test(demo)) refuse(`${demo} is inside OpenBark's own data folder`);
if (!existsSync(join(demo, "OPENBARK-DEMO"))) refuse(`${demo} has no OPENBARK-DEMO marker file`);
if (!speech || !existsSync(speech)) refuse("SPEECH_WAV must name a WAV file of speech");
if (!silence || !existsSync(silence)) refuse("SILENT_WAV must name a WAV file of digital silence");
const storage = await fetch(`${base}/api/storage`).then((r) => r.json());
if (relative(join(demo, "visits"), resolve(storage.visits.path)) !== "")
  refuse(`the server's visits folder (${storage.visits.path}) isn't ${join(demo, "visits")}`);

mkdirSync(out, { recursive: true });
const problems = [];
const check = (ok, what) => {
  if (!ok) problems.push(what);
  console.log(`  ${ok ? "ok" : "FAIL"}: ${what}`);
};
const step = (what) => console.log(`- ${what}`);
const want = (name) => only.length === 0 || only.includes(name);

const chromiumArgs = (wav, prompt = false) => [
  ...(prompt ? [] : ["--use-fake-ui-for-media-stream"]),
  "--use-fake-device-for-media-stream",
  `--use-file-for-fake-audio-capture=${wav}`,
];

/** Settings for a fresh page, and a hook that keeps every stream getUserMedia returns. */
async function prepare(ctx) {
  await ctx.addInitScript(() => {
    localStorage.setItem(
      "openbark.settings",
      JSON.stringify({ updateCheck: "off", consentAcknowledgedAt: "2026-10-17T20:00:00Z", uiLanguage: "en", draftOnStop: false }),
    );
    const md = navigator.mediaDevices;
    if (!md?.getUserMedia) return;
    const real = md.getUserMedia.bind(md);
    window.__ob = { streams: [], unplugged: false, calls: 0 };
    md.getUserMedia = async (c) => {
      window.__ob.calls++;
      if (window.__ob.unplugged) throw Object.assign(new Error("Requested device not found"), { name: "NotFoundError" });
      const s = await real(c);
      window.__ob.streams.push(s);
      return s;
    };
  });
}

/** Unplug the current microphone: its track ends. */
const unplug = (page, nothingLeft = false) =>
  page.evaluate((nothing) => {
    window.__ob.unplugged = nothing;
    const t = window.__ob.streams.at(-1).getAudioTracks()[0];
    t.stop();
    t.dispatchEvent(new Event("ended"));
  }, nothingLeft);

const notice = (page) => page.locator(".mic-notice").allInnerTexts();
const status = (page) => page.locator(".recorder-meta").innerText();
const elapsed = async (page) => {
  const text = await page.locator(".recorder-record").innerText().catch(() => "");
  const m = text.match(/(\d+):(\d\d)/);
  return m ? Number(m[1]) * 60 + Number(m[2]) : 0;
};

async function newest() {
  const list = await fetch(`${base}/api/sessions`).then((r) => r.json());
  const items = Array.isArray(list) ? list : (list.sessions ?? list.items ?? []);
  items.sort((a, b) => String(b.created_at ?? b.id).localeCompare(String(a.created_at ?? a.id)));
  return fetch(`${base}/api/sessions/${encodeURIComponent(items[0].id)}`).then((r) => r.json());
}

async function startRecording(page) {
  await page.getByRole("button", { name: "Start recording" }).click();
  await page.waitForFunction(() => /Recording\./.test(document.querySelector(".recorder-meta")?.textContent ?? ""), null, {
    timeout: 60_000,
  });
}

async function stopAndOpen(page) {
  await page.getByRole("button", { name: "Stop recording" }).click();
  await page.locator(".review-bar, .pane-pad").first().waitFor({ timeout: 5 * 60_000 });
  return newest();
}

// ---- 1. permission dismissed, then granted ----------------------------------------
if (want("permission")) {
  step("Chromium: the permission question closed, then the microphone allowed");
  // The full browser in headless mode answers the prompt (as dismissed or
  // refused); the headless shell can't show one at all.
  const browser = await chromium.launch({ args: chromiumArgs(speech, true), channel: "chromium" });
  const ctx = await browser.newContext({ viewport: { width: 1440, height: 900 } });
  await prepare(ctx);
  const page = await ctx.newPage();
  await page.goto(base);
  await page.locator(".recorder").waitFor();
  await page.getByRole("button", { name: "Start recording" }).click();
  const alert = page.locator(".ob-notice[data-tone=danger][role=alert]").first();
  await alert.waitFor({ timeout: 10_000 });
  const text = await alert.innerText();
  console.log(`    message: ${text}`);
  check(/choose Allow|Allow the microphone/.test(text), "a refused or dismissed prompt says how to allow the microphone");
  check(!/aborted|worklet module/i.test(text), "no raw browser error");
  await ctx.grantPermissions(["microphone"], { origin: base });
  await startRecording(page);
  check(true, "allowed later, Record works without reloading");
  await page.waitForTimeout(3000);
  await stopAndOpen(page);
  await browser.close();
}

// ---- 2. the microphone test --------------------------------------------------------
if (want("test")) {
  step("Chromium: Test microphone on Record");
  const browser = await chromium.launch({ args: chromiumArgs(speech) });
  const ctx = await browser.newContext({ viewport: { width: 1440, height: 900 }, permissions: ["microphone"] });
  await prepare(ctx);
  const page = await ctx.newPage();
  await page.goto(base);
  const rail = page.locator(".recorder-rail");
  await rail.getByRole("button", { name: "Test microphone" }).click();
  await rail.locator(".mic-check-verdict", { hasText: "Hearing you." }).waitFor({ timeout: 15_000 });
  const level = Number(await rail.locator("[role=meter]").getAttribute("aria-valuenow"));
  check(level > 0, `the meter moves (${level})`);
  const options = await rail.locator("select option").allInnerTexts();
  console.log(`    microphones listed: ${options.join(" | ")}`);
  check(options.length >= 2, "the fake microphone is listed by name once allowed");
  await page.screenshot({ path: `${out}/mic-test.png` });
  await rail.getByRole("button", { name: "Stop test" }).click();
  check(
    await page.evaluate(() => window.__ob.streams.every((s) => s.getAudioTracks().every((t) => t.readyState === "ended"))),
    "stopping the test lets the microphone go",
  );
  // A double click opens one microphone, and Stop closes everything.
  await rail.getByRole("button", { name: "Test microphone" }).dblclick();
  await rail.locator(".mic-check-verdict", { hasText: "Hearing you." }).waitFor({ timeout: 15_000 });
  await rail.getByRole("button", { name: "Stop test" }).click();
  await page.waitForTimeout(500);
  const states = await page.evaluate(() => window.__ob.streams.map((s) => s.getAudioTracks()[0].readyState));
  check(states.every((x) => x === "ended"), `after a double click and Stop, every microphone is closed (${states.join(", ")})`);
  // Record pressed while a test is opening: the test's microphone closes.
  await rail.getByRole("button", { name: "Test microphone" }).click();
  await startRecording(page);
  await page.waitForTimeout(1500);
  const live = await page.evaluate(() => window.__ob.streams.filter((s) => s.getAudioTracks()[0].readyState === "live").length);
  check(live === 1, `while recording, only the recording's microphone is open (${live})`);
  await stopAndOpen(page);
  await browser.close();
}

// ---- 3. unplugged mid-recording: moves to another, keeps every word --------------------------
if (want("switch")) {
  step("Chromium: the microphone unplugged mid-recording, another takes over");
  const browser = await chromium.launch({ args: chromiumArgs(speech) });
  const ctx = await browser.newContext({ viewport: { width: 1440, height: 900 }, permissions: ["microphone"] });
  await prepare(ctx);
  const page = await ctx.newPage();
  page.on("pageerror", (e) => problems.push(`switch: ${e}`));
  await page.goto(base);
  await page.locator(".recorder").waitFor();
  await startRecording(page);
  const t0 = Date.now();
  await page.waitForTimeout(12_000);
  const before = await elapsed(page);
  const switchedAt = (Date.now() - t0) / 1000;
  await unplug(page);
  await page.waitForFunction(() => window.__ob.streams.length >= 2, null, { timeout: 10_000 });
  check(true, "a new microphone was opened by itself");
  await page.waitForTimeout(12_000);
  const after = await elapsed(page);
  check(after - before >= 10, `the timer kept going after the switch (${before} s then ${after} s)`);
  check((await status(page)).includes("Recording."), "still recording");
  await page.screenshot({ path: `${out}/after-switch.png` });
  const wall = (Date.now() - t0) / 1000;
  const view = await stopAndOpen(page);
  const secs = view.stats?.audio_secs ?? 0;
  console.log(`    ${secs.toFixed(1)} s recorded over ${wall.toFixed(1)} s; switched at ${switchedAt.toFixed(1)} s`);
  check(secs > wall - 3 && secs < wall + 1, "the recording is as long as the time recorded (the switch loses well under a second)");
  const segs = view.transcript?.segments ?? [];
  check(segs.some((s) => s.end < switchedAt - 1), "speech before the switch is transcribed");
  check(segs.some((s) => s.start > switchedAt + 1), "speech after the switch is transcribed");
  await browser.close();
}

// ---- 4. unplugged with nothing left: paused, then resumed ---------------------------------------------
if (want("lost")) {
  step("Chromium: unplugged with no other microphone, then plugged back in");
  const browser = await chromium.launch({ args: chromiumArgs(speech) });
  const ctx = await browser.newContext({ viewport: { width: 1440, height: 900 }, permissions: ["microphone"] });
  await prepare(ctx);
  const page = await ctx.newPage();
  await page.goto(base);
  await startRecording(page);
  await page.waitForTimeout(5000);
  await unplug(page, true);
  await page.locator(".mic-notice").waitFor({ timeout: 5000 });
  const text = (await notice(page)).join(" ");
  check(/disconnected, so recording is paused/.test(text), "a clear 'paused' notice");
  check((await status(page)).includes("Paused"), "the status line says paused");
  const pausedAt = await elapsed(page);
  await page.screenshot({ path: `${out}/paused.png` });
  await page.waitForTimeout(4000);
  check((await elapsed(page)) - pausedAt <= 1, "nothing is recorded while there's no microphone");
  await page.evaluate(() => (window.__ob.unplugged = false));
  await page.waitForFunction(() => !document.querySelector(".mic-notice"), null, { timeout: 8000 });
  check(true, "recording resumed by itself once a microphone was back");
  await page.waitForTimeout(4000);
  check((await elapsed(page)) - pausedAt >= 3, "the timer runs again");
  await stopAndOpen(page);
  await browser.close();
}

// ---- 5. digital silence -------------------------------------------------------------------------
if (want("silence")) {
  step("Chromium: a microphone that sends only silence");
  const browser = await chromium.launch({ args: chromiumArgs(silence) });
  const ctx = await browser.newContext({ viewport: { width: 1440, height: 900 }, permissions: ["microphone"] });
  await prepare(ctx);
  const page = await ctx.newPage();
  await page.goto(base);
  const t0 = Date.now();
  await startRecording(page);
  await page.locator(".mic-notice").waitFor({ timeout: 10_000 });
  const secs = (Date.now() - t0) / 1000;
  const text = (await notice(page)).join(" ");
  console.log(`    after ${secs.toFixed(1)} s: ${text}`);
  check(/sending silence/.test(text) && secs < 9, "silence is warned about within a few seconds");
  await page.screenshot({ path: `${out}/silence.png` });
  await stopAndOpen(page).catch(() => problems.push("silence: the visit didn't open"));
  await browser.close();
}

// ---- 6. AudioContext suspended until a gesture (Safari's policy) ----------------------------------------
if (want("suspended")) {
  step("Chromium, as Safari: the audio context stays suspended until a click");
  const browser = await chromium.launch({ args: chromiumArgs(speech) });
  const ctx = await browser.newContext({ viewport: { width: 1440, height: 900 }, permissions: ["microphone"] });
  await prepare(ctx);
  await ctx.addInitScript(() => {
    // Contexts start suspended, and resume() works only after a real gesture.
    const Real = window.AudioContext;
    let gesture = false;
    window.addEventListener("pointerdown", () => (gesture = true), true);
    window.AudioContext = class extends Real {
      constructor(o) {
        super(o);
        void this.suspend();
        const resume = this.resume.bind(this);
        this.resume = () => (gesture ? resume() : new Promise(() => {}));
      }
    };
  });
  const page = await ctx.newPage();
  await page.goto(base);
  // Keyboard start: no pointer gesture yet.
  await page.locator(".recorder").focus();
  await page.keyboard.press("Space");
  await page.locator(".mic-notice").waitFor({ timeout: 15_000 });
  const text = (await notice(page)).join(" ");
  check(/No sound is reaching OpenBark/.test(text), "a suspended context is noticed, not recorded as nothing");
  await page.mouse.click(700, 120);
  await page.waitForFunction(() => !document.querySelector(".mic-notice"), null, { timeout: 8000 });
  check(true, "a click resumes it");
  await page.waitForTimeout(3000);
  await stopAndOpen(page);
  await browser.close();
}

// ---- 7. the tab in the background -------------------------------------------------------------------------
if (want("background")) {
  step("Chromium: the recording tab in the background for 15 s");
  const browser = await chromium.launch({ args: chromiumArgs(speech) });
  const ctx = await browser.newContext({ viewport: { width: 1440, height: 900 }, permissions: ["microphone"] });
  await prepare(ctx);
  const page = await ctx.newPage();
  await page.goto(base);
  await startRecording(page);
  const t0 = Date.now();
  await page.waitForTimeout(3000);
  const other = await ctx.newPage();
  await other.goto("about:blank");
  await other.bringToFront();
  await other.waitForTimeout(15_000);
  await page.bringToFront();
  await page.waitForTimeout(2000);
  const wall = (Date.now() - t0) / 1000;
  const view = await stopAndOpen(page);
  const secs = view.stats?.audio_secs ?? 0;
  console.log(`    ${secs.toFixed(1)} s recorded over ${wall.toFixed(1)} s`);
  check(secs > wall - 3, "audio kept flowing while the tab was in the background");
  await browser.close();
}

// ---- 7b. a long recording (LONG_SECS, off unless set) ---------------------------------------
const longSecs = Number(process.env.LONG_SECS ?? 0);
if (longSecs > 0 && want("long")) {
  step(`Chromium: a ${longSecs} s recording`);
  const browser = await chromium.launch({ args: chromiumArgs(speech) });
  const ctx = await browser.newContext({ viewport: { width: 1440, height: 900 }, permissions: ["microphone"] });
  await prepare(ctx);
  const page = await ctx.newPage();
  await page.goto(base);
  await startRecording(page);
  const t0 = Date.now();
  const heap = () => page.evaluate(() => performance.memory?.usedJSHeapSize ?? 0);
  const heap0 = await heap();
  await page.waitForTimeout(longSecs * 1000);
  const heap1 = await heap();
  const wall = (Date.now() - t0) / 1000;
  const shown = await elapsed(page);
  const view = await stopAndOpen(page);
  const secs = view.stats?.audio_secs ?? 0;
  console.log(
    `    ${secs.toFixed(1)} s recorded over ${wall.toFixed(1)} s (timer ${shown} s); ${view.transcript?.segments?.length ?? 0} segments; JS heap ${(heap0 / 1e6).toFixed(1)} MB then ${(heap1 / 1e6).toFixed(1)} MB`,
  );
  check(Math.abs(secs - wall) < 3, "no drift or loss over a long recording");
  check(heap1 < heap0 + 50e6, "the page's memory stays flat");
  await browser.close();
}

// ---- 8. Firefox end to end ----------------------------------------------------------------------------------
if (want("firefox")) {
  step("Firefox: the microphone test and a recording (fake stream)");
  const browser = await firefox.launch({
    firefoxUserPrefs: { "media.navigator.streams.fake": true, "media.navigator.permission.disabled": true },
  });
  const ctx = await browser.newContext({ viewport: { width: 1440, height: 900 } });
  await prepare(ctx);
  const page = await ctx.newPage();
  page.on("pageerror", (e) => problems.push(`firefox: ${e}`));
  await page.goto(base);
  const rail = page.locator(".recorder-rail");
  await rail.getByRole("button", { name: "Test microphone" }).click();
  await page.waitForFunction(
    () => Number(document.querySelector(".recorder-rail [role=meter]")?.getAttribute("aria-valuenow")) > 0,
    null,
    { timeout: 15_000 },
  );
  check(true, "the meter moves in Firefox");
  await rail.getByRole("button", { name: "Stop test" }).click();
  await startRecording(page);
  await page.waitForTimeout(6000);
  const view = await stopAndOpen(page);
  check((view.stats?.audio_secs ?? 0) > 4, `Firefox recorded (${(view.stats?.audio_secs ?? 0).toFixed(1)} s)`);
  await browser.close();
}

// ---- 9. WebKit -----------------------------------------------------------------------------------------------
if (want("webkit")) {
  step("WebKit: what the page says");
  const browser = await webkit.launch();
  const ctx = await browser.newContext({ viewport: { width: 1440, height: 900 } });
  await prepare(ctx);
  const page = await ctx.newPage();
  await page.goto(base);
  const hasMic = await page.evaluate(() => Boolean(navigator.mediaDevices?.getUserMedia));
  await page.getByRole("button", { name: "Start recording" }).click();
  if (hasMic) {
    await page.waitForTimeout(5000);
    console.log(`    status: ${await status(page)}; notices: ${(await notice(page)).join(" ")}`);
  } else {
    const alert = page.locator(".ob-notice[data-tone=danger][role=alert]").first();
    await alert.waitFor({ timeout: 10_000 });
    const text = await alert.innerText();
    console.log(`    no microphone API in this WebKit build; message: ${text}`);
    check(/Chrome, Edge, Firefox or Safari|secure page/.test(text), "a browser without a microphone API gets a plain message");
  }
  await browser.close();
}

console.log(problems.length ? `PROBLEMS:\n${problems.join("\n")}` : "all checks passed");
process.exit(problems.length ? 1 : 0);
