// Every main screen, for design review: phone (390 x 844, light and dark)
// and desktop (1440 x 900, light and dark), laid out as ADR-0027 describes,
// plus light-only passes at the edges of the layout: a 375 px iPhone (SE,
// mini) and 1024 and 820 px windows (iPad, a narrow desktop window).
// It records, it doesn't assert: a screen it can't reach is listed, not fatal.
//
// Run ONLY against a demo server holding invented visits (the Design screens
// workflow imports the synthetic smoke consult under invented patients):
//   OPENBARK_URL=http://localhost:4339 VARIANTS=phone node e2e/design-screens.mjs <demo folder>
//
// It refuses unless the URL is this computer (never port 4317) and the demo
// folder holds an OPENBARK-DEMO marker.
//
// The phone pass (the demo started with OPENBARK_PLATFORM=ios) begins with
// the iPhone's first-run walkthrough, one screenshot per step.
//
// RECORDING=1 adds a last pass in Chromium with a fake microphone, for the
// recording screen (WebKit can't fake one). It starts a recording, so it runs
// after every other screen.

import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { chromium, webkit } from "@playwright/test";

const base = process.env.OPENBARK_URL ?? "http://localhost:4339";
const out = process.env.OUT_DIR ?? "../../target/design-screens";
const demo = process.argv[2] ? resolve(process.argv[2]) : null;
const url = new URL(base);

function refuse(why) {
  console.error(`refusing: ${why}`);
  process.exit(2);
}
if (url.port === "4317") refuse("port 4317 is the clinic's own OpenBark server");
if (!["localhost", "127.0.0.1"].includes(url.hostname)) refuse("only this computer's demo can be used");
if (!demo || !existsSync(`${demo}/OPENBARK-DEMO`)) refuse("usage: node e2e/design-screens.mjs <demo folder with OPENBARK-DEMO>");
if (/[\\/]AppData[\\/]Local[\\/]OpenBark/i.test(demo)) refuse("that's inside OpenBark's own data folder");
mkdirSync(out, { recursive: true });

const phone = { viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true, deviceScaleFactor: 2 };
const desktop = { viewport: { width: 1440, height: 900 }, deviceScaleFactor: 1 };
const small = { ...phone, viewport: { width: 375, height: 667 } };
const all = [
  // WebKit for the phone: the iPhone app runs in WKWebView.
  { name: "phone-light", engine: webkit, colorScheme: "light", ...phone },
  { name: "phone-dark", engine: webkit, colorScheme: "dark", ...phone },
  { name: "desktop-light", engine: chromium, colorScheme: "light", ...desktop },
  { name: "desktop-dark", engine: chromium, colorScheme: "dark", ...desktop },
  { name: "phone-375-light", engine: webkit, colorScheme: "light", ...small },
  { name: "desktop-1024-light", engine: chromium, colorScheme: "light", ...desktop, viewport: { width: 1024, height: 768 } },
  { name: "desktop-820-light", engine: chromium, colorScheme: "light", ...desktop, viewport: { width: 820, height: 1000 } },
];
const want = (process.env.VARIANTS ?? "phone,desktop").split(",");
const variants = all.filter((v) => want.some((w) => v.name.startsWith(w)));

/** On phones these open from Settings rows (ADR-0027); the rest are tabs. */
const PHONE_ROW = { templates: "Templates", models: "Models and storage", help: "Guides and videos" };
const missed = [];
const taken = [];

/** Write the index and exit, whatever is still open (a browser, a stuck page). */
function finish() {
  // One index per pass (the workflow runs desktop, then phone, into the same folder).
  writeFileSync(`${out}/index-${want.join("-")}.json`, JSON.stringify({ taken, missed }, null, 2));
  console.log(`${taken.length} screenshots in ${out}`);
  if (missed.length) console.log(`Not reached:\n${missed.join("\n")}`);
  process.exit(0);
}
// A watchdog: never hold the runner past 20 minutes per pass.
setTimeout(() => {
  missed.push("stopped by the 20-minute watchdog");
  finish();
}, 20 * 60 * 1000).unref();

async function context(v, extra = {}) {
  const browser = await v.engine.launch(extra.launch ?? {});
  const ctx = await browser.newContext({
    viewport: v.viewport,
    colorScheme: v.colorScheme,
    isMobile: v.isMobile,
    hasTouch: v.hasTouch,
    deviceScaleFactor: v.deviceScaleFactor,
    permissions: extra.permissions,
  });
  await ctx.addInitScript((firstRun) => {
    const key = "openbark.settings";
    const current = JSON.parse(localStorage.getItem(key) ?? "{}");
    // A first run (the walkthrough pass) has accepted nothing yet.
    const accepted = firstRun
      ? { consentAcknowledgedAt: null, walkthroughDoneAt: null, showWalkthrough: false }
      : { consentAcknowledgedAt: "2026-10-17T20:00:00Z" };
    localStorage.setItem(key, JSON.stringify({ ...current, ...accepted, uiLanguage: "en" }));
  }, Boolean(extra.firstRun));
  const page = await ctx.newPage();
  // A screen that isn't there is listed as missed, never waited for forever.
  page.setDefaultTimeout(15000);
  return { browser, ctx, page };
}

// The iPhone's first-run walkthrough: phone width only, on the iPhone profile.
for (const v of variants.filter((x) => x.viewport.width <= 640)) {
  let browser = null;
  try {
    const opened = await context(v, { firstRun: true });
    browser = opened.browser;
    const page = opened.page;
    const shot = async (n) => {
      await page.waitForTimeout(500);
      const file = `${v.name}-walkthrough-${n}.png`;
      await page.screenshot({ path: `${out}/${file}` });
      taken.push(file);
    };
    await page.goto(base);
    await page.locator(".walkthrough").waitFor({ timeout: 20000 });
    await shot(1);
    await page.getByRole("button", { name: "Get started" }).click();
    await shot(2);
    await page.locator(".walkthrough .check input").check();
    await page.getByRole("button", { name: "Continue" }).click();
    await shot(3);
    await page.getByRole("button", { name: "Continue" }).click();
    await shot(4);
    await page.getByRole("button", { name: "Later" }).click();
    await page.locator(".recorder").waitFor({ timeout: 10000 });
  } catch (e) {
    missed.push(`${v.name} walkthrough: ${String(e).split("\n")[0]}`);
  } finally {
    await browser?.close().catch(() => {});
  }
}

for (const v of variants) {
  const { browser, ctx, page } = await context(v);
  const narrow = v.viewport.width <= 640;
  const shot = async (screen, opts = {}) => {
    await page.waitForTimeout(600);
    const file = `${v.name}-${screen}.png`;
    await page.screenshot({ path: `${out}/${file}`, ...opts });
    taken.push(file);
  };
  const nav = async (screen) => {
    if (narrow && screen in PHONE_ROW) {
      await page.locator('.nav-item[data-nav="settings"]').click();
      await page.locator(".settings-row", { hasText: PHONE_ROW[screen] }).first().click();
    } else {
      await page.locator(`.nav-item[data-nav="${screen}"]`).click();
    }
    await page.locator(".screen h1").first().waitFor({ timeout: 10000 });
  };
  const attempt = async (screen, fn) => {
    try {
      await fn();
    } catch (e) {
      missed.push(`${v.name} ${screen}: ${String(e).split("\n")[0]}`);
    }
  };

  await page.goto(base);
  await page.locator(".recorder").waitFor({ timeout: 20000 });
  await attempt("record", () => shot("record"));
  await attempt("record-full", () => shot("record-full", { fullPage: true }));
  if (narrow) {
    await attempt("record-patient", async () => {
      await page.locator(".patient-row-button").click();
      await page.locator(".ob-sheet").first().waitFor({ timeout: 5000 });
      await shot("record-patient");
      await page.keyboard.press("Escape");
      await page.waitForTimeout(500);
    });
    await attempt("record-template", async () => {
      await page.locator("button.opt-row", { hasText: "Template" }).click();
      await page.locator(".ob-sheet").first().waitFor({ timeout: 5000 });
      await shot("record-template");
      await page.keyboard.press("Escape");
      await page.waitForTimeout(500);
    });
  }
  await attempt("visits", async () => {
    await nav("visits");
    await page.locator(".row-link:visible").first().waitFor({ timeout: 10000 });
    await shot("visits");
  });
  await attempt("review", async () => {
    // The drafted visit (Maple), so Review shows a note; any visit otherwise.
    const drafted = page.locator(".row-link:visible", { hasText: "Maple" }).first();
    await ((await drafted.count()) ? drafted : page.locator(".row-link:visible").first()).click();
    await page.locator(".note:visible, .transcript:visible").first().waitFor({ timeout: 20000 });
    await shot("review");
    await shot("review-full", { fullPage: true });
  });
  await attempt("review-more", async () => {
    await page.getByRole("button", { name: "More actions" }).first().click();
    await page.locator(".more-panel").waitFor({ timeout: 5000 });
    await shot("review-more");
    await page.keyboard.press("Escape");
  });
  if (narrow) {
    await attempt("review-evidence", async () => {
      await page.locator(".stmt").first().click();
      await page.locator(".quote-card").waitFor({ timeout: 5000 });
      await shot("review-evidence");
    });
    await attempt("review-transcript", async () => {
      await page.locator(".review-tabs .ob-segment").nth(1).click();
      await shot("review-transcript");
    });
    await attempt("review-back", async () => {
      await page.locator(".back-top").first().click();
      await page.locator(".screen.visits").waitFor({ timeout: 5000 });
    });
  }
  await attempt("patients", async () => {
    await nav("patients");
    await page.waitForTimeout(800);
    await shot("patients");
  });
  await attempt("patient-detail", async () => {
    await page.locator(".patient-row:visible").first().click();
    await page.waitForTimeout(1000);
    await shot("patient-detail");
  });
  for (const screen of ["templates", "models", "help"]) {
    await attempt(screen, async () => {
      await nav(screen);
      await page.waitForTimeout(800);
      await shot(screen);
      await shot(`${screen}-full`, { fullPage: true });
    });
  }
  await attempt("settings", async () => {
    await nav("settings");
    await page.waitForTimeout(800);
    await shot("settings");
    await shot("settings-full", { fullPage: true });
  });
  // One section at a time: open a few, by their rows (phones) or the section list.
  const sections = narrow
    ? { recording: "Microphone and consent", sync: "Encrypted sync", privacy: "Privacy and storage", you: "You and your team" }
    : { recording: "Recording and consent", sync: "Encrypted sync", privacy: "Privacy and storage", phone: "Phone calls" };
  for (const [id, label] of Object.entries(sections)) {
    await attempt(`settings-${id}`, async () => {
      if (narrow) {
        await nav("settings");
        await page.locator(".settings-row", { hasText: label }).first().click();
      } else {
        await page.locator(".settings-nav-item", { hasText: label }).first().click();
      }
      await page.waitForTimeout(800);
      await shot(`settings-${id}`, { fullPage: true });
    });
  }
  await ctx.close();
  await browser.close();
}

// The recording screen, with a fake microphone (Chromium only).
if (process.env.RECORDING === "1") {
  // Phones only: on a computer the demo has no note model (Gemma 4 E4B), so Record waits for it.
  const recs = [
    { ...all[0], name: "phone-light-chromium", engine: chromium },
    { ...all[1], name: "phone-dark-chromium", engine: chromium },
  ].filter((v) => want.some((w) => v.name.startsWith(w)));
  for (const v of recs) {
    let browser = null;
    try {
      const opened = await context(v, {
        launch: { args: ["--use-fake-device-for-media-stream", "--use-fake-ui-for-media-stream"] },
        permissions: ["microphone"],
      });
      browser = opened.browser;
      const page = opened.page;
      await page.goto(base);
      await page.locator(".recorder").waitFor({ timeout: 20000 });
      await page.getByRole("button", { name: "Start recording" }).click();
      await page.getByRole("timer").first().waitFor({ timeout: 20000 });
      await page.waitForTimeout(6000);
      const file = `${v.name}-recording.png`;
      await page.screenshot({ path: `${out}/${file}` });
      taken.push(file);
      // Stop and save, so the server isn't left recording.
      const stop = page.getByRole("button", { name: /Stop and save|Stop recording/ }).first();
      await stop.click().catch(() => {});
      await page.waitForTimeout(3000);
    } catch (e) {
      missed.push(`${v.name} recording: ${String(e).split("\n")[0]}`);
    } finally {
      await browser?.close().catch(() => {});
    }
  }
}

finish();
