// The screens for the App Store listing: the iPhone layout at 440 x 868
// (x3, so 1320 px wide), light and dark, of invented visits only. The
// iPhone's status bar and home indicator are added later by
// scripts/appstore/compose.mjs, which takes the screen to 1320 x 2868.
//
// Run ONLY against a demo server started with OPENBARK_PLATFORM=ios and
// holding invented visits (the App Store assets workflow renders the
// synthetic consult scripts with Kokoro):
//   OPENBARK_URL=http://localhost:4339 OUT_DIR=out node e2e/appstore-screens.mjs <demo folder>
//
// It refuses unless the URL is this computer (never port 4317) and the demo
// folder holds an OPENBARK-DEMO marker. A screen it can't reach is listed.

import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { webkit } from "@playwright/test";

const base = process.env.OPENBARK_URL ?? "http://localhost:4339";
const out = process.env.OUT_DIR ?? "../../target/appstore-screens";
const demo = process.argv[2] ? resolve(process.argv[2]) : null;
const url = new URL(base);

function refuse(why) {
  console.error(`refusing: ${why}`);
  process.exit(2);
}
if (url.port === "4317") refuse("port 4317 is the clinic's own OpenBark server");
if (!["localhost", "127.0.0.1"].includes(url.hostname)) refuse("only this computer's demo can be used");
if (!demo || !existsSync(`${demo}/OPENBARK-DEMO`)) refuse("usage: node e2e/appstore-screens.mjs <demo folder with OPENBARK-DEMO>");
mkdirSync(out, { recursive: true });

const phone = { viewport: { width: 440, height: 868 }, isMobile: true, hasTouch: true, deviceScaleFactor: 3 };
const missed = [];
const taken = [];
setTimeout(() => {
  missed.push("stopped by the 20-minute watchdog");
  finish();
}, 20 * 60 * 1000).unref();

function finish() {
  writeFileSync(`${out}/index.json`, JSON.stringify({ taken, missed }, null, 2));
  console.log(`${taken.length} screenshots in ${out}`);
  if (missed.length) console.log(`Not reached:\n${missed.join("\n")}`);
  process.exit(0);
}

async function open(scheme, firstRun) {
  const browser = await webkit.launch();
  const ctx = await browser.newContext({ ...phone, colorScheme: scheme });
  await ctx.addInitScript((first) => {
    const key = "openbark.settings";
    const current = JSON.parse(localStorage.getItem(key) ?? "{}");
    const accepted = first
      ? { consentAcknowledgedAt: null, walkthroughDoneAt: null, showWalkthrough: false }
      : { updateCheck: "off", consentAcknowledgedAt: "2026-10-17T20:00:00Z", walkthroughDoneAt: "2026-10-17T20:00:00Z" };
    localStorage.setItem(key, JSON.stringify({ ...current, ...accepted, uiLanguage: "en" }));
  }, firstRun);
  const page = await ctx.newPage();
  page.setDefaultTimeout(15000);
  return { browser, page };
}

for (const scheme of ["light", "dark"]) {
  const shot = async (page, name) => {
    await page.waitForTimeout(700);
    const file = `${scheme}-${name}.png`;
    await page.screenshot({ path: `${out}/${file}` });
    taken.push(file);
  };
  const attempt = async (name, fn) => {
    try {
      await fn();
    } catch (e) {
      missed.push(`${scheme} ${name}: ${String(e).split("\n")[0]}`);
    }
  };

  // The first-run walkthrough: its opening promise.
  {
    const { browser, page } = await open(scheme, true);
    await attempt("welcome", async () => {
      await page.goto(base);
      await page.locator(".walkthrough").waitFor({ timeout: 20000 });
      await shot(page, "welcome");
    });
    await browser.close();
  }

  const { browser, page } = await open(scheme, false);
  const tab = async (screen) => {
    // A visit hides the tab bar on phones: back to the list first.
    const item = page.locator(`.nav-item[data-nav="${screen}"]`);
    if (!(await item.isVisible())) await page.locator(".back-top").first().click();
    await item.click();
    await page.locator(".screen h1").first().waitFor({ timeout: 10000 });
  };
  const visit = async (animal) => {
    await tab("visits");
    await page.locator(".row-link:visible", { hasText: animal }).first().click();
    await page.locator(".note:visible, .transcript:visible").first().waitFor({ timeout: 20000 });
  };

  await page.goto(base);
  await page.locator(".recorder").waitFor({ timeout: 20000 });
  await attempt("record", async () => {
    // Served to a browser, the screen names the browser's microphone; the
    // iPhone app names its own (Recorder.tsx).
    await page.evaluate(() => {
      const mic = document.querySelector(".stage-mic-name");
      if (mic) mic.textContent = "iPhone microphone";
    });
    await shot(page, "record");
  });
  await attempt("visits", async () => {
    await tab("visits");
    await page.locator(".row-link:visible").first().waitFor({ timeout: 10000 });
    await shot(page, "visits");
  });
  await attempt("note", async () => {
    await visit("Biscuit");
    await shot(page, "note");
  });
  await attempt("evidence", async () => {
    await page.locator(".stmt").nth(2).click();
    await page.locator(".quote-card").waitFor({ timeout: 5000 });
    await shot(page, "evidence");
    await page.keyboard.press("Escape");
  });
  await attempt("transcript", async () => {
    await page.locator(".review-tabs .ob-segment").nth(1).click();
    await shot(page, "transcript");
  });
  await attempt("dental", async () => {
    await visit("Pepper");
    const tooth = page.locator(".dc-tooth").first();
    await tooth.waitFor({ timeout: 10000 });
    // The chart's section heading at the top of the screen.
    await tooth.evaluate((el) => {
      const svg = el.closest("svg");
      const section = svg?.closest("section, .note-section, div") ?? svg;
      section?.scrollIntoView({ block: "start" });
      window.scrollBy(0, -120);
      document.querySelectorAll(".screen, main, .scroll").forEach((s) => s.scrollBy?.(0, -120));
    });
    await shot(page, "dental");
  });
  await attempt("patients", async () => {
    await tab("patients");
    await page.waitForTimeout(800);
    await shot(page, "patients");
  });
  await attempt("templates", async () => {
    await tab("settings");
    await page.locator(".settings-row", { hasText: "Templates" }).first().click();
    await page.locator(".screen h1").first().waitFor({ timeout: 10000 });
    await shot(page, "templates");
  });
  await attempt("sync", async () => {
    await tab("settings");
    await page.locator(".settings-row", { hasText: "Encrypted sync" }).first().click();
    await page.waitForTimeout(800);
    await shot(page, "sync");
  });
  await attempt("privacy", async () => {
    await tab("settings");
    await page.locator(".settings-row", { hasText: "Privacy and storage" }).first().click();
    await page.waitForTimeout(800);
    await shot(page, "privacy");
  });
  await browser.close();
}

// The 13" iPad in portrait: 1032 x 1376 points less its status bar (24) and
// home indicator (20), x2 so the screen is 2064 px wide. At this width the app
// shows its wide layout (the side rail, the transcript beside the note).
const ipad = { viewport: { width: 1032, height: 1332 }, hasTouch: true, deviceScaleFactor: 2 };
for (const scheme of ["light", "dark"]) {
  const browser = await webkit.launch();
  const ctx = await browser.newContext({ ...ipad, colorScheme: scheme });
  await ctx.addInitScript(() => {
    const key = "openbark.settings";
    const current = JSON.parse(localStorage.getItem(key) ?? "{}");
    const accepted = { updateCheck: "off", consentAcknowledgedAt: "2026-10-17T20:00:00Z", walkthroughDoneAt: "2026-10-17T20:00:00Z" };
    localStorage.setItem(key, JSON.stringify({ ...current, ...accepted, uiLanguage: "en" }));
  });
  const page = await ctx.newPage();
  page.setDefaultTimeout(15000);
  const shot = async (name) => {
    await page.waitForTimeout(800);
    const file = `ipad-${scheme}-${name}.png`;
    await page.screenshot({ path: `${out}/${file}` });
    taken.push(file);
  };
  const attempt = async (name, fn) => {
    try {
      await fn();
    } catch (e) {
      missed.push(`ipad ${scheme} ${name}: ${String(e).split("\n")[0]}`);
    }
  };
  const nav = async (screen) => {
    await page.locator(`.nav-item[data-nav="${screen}"]`).first().click();
    await page.locator(".screen h1").first().waitFor({ timeout: 10000 });
  };
  const visit = async (animal) => {
    await nav("visits");
    await page.locator(".row-link:visible", { hasText: animal }).first().click();
    await page.locator(".note:visible, .transcript:visible").first().waitFor({ timeout: 20000 });
  };
  await page.goto(base);
  await page.locator(".recorder").waitFor({ timeout: 20000 });
  await attempt("record", async () => {
    await page.evaluate(() => {
      const mic = document.querySelector(".stage-mic-name");
      if (mic) mic.textContent = "iPad microphone";
    });
    await shot("record");
  });
  await attempt("visits", async () => {
    await nav("visits");
    await page.locator(".row-link:visible").first().waitFor({ timeout: 10000 });
    await shot("visits");
  });
  await attempt("note", async () => {
    await visit("Biscuit");
    await shot("note");
  });
  await attempt("evidence", async () => {
    await page.locator(".stmt").nth(2).click();
    await page.waitForTimeout(600);
    await shot("evidence");
    await page.keyboard.press("Escape");
  });
  await attempt("dental", async () => {
    await visit("Pepper");
    const tooth = page.locator(".dc-tooth").first();
    await tooth.waitFor({ timeout: 10000 });
    await tooth.evaluate((el) => el.closest("svg")?.scrollIntoView({ block: "center" }));
    await shot("dental");
  });
  await attempt("templates", async () => {
    await nav("templates");
    await shot("templates");
  });
  await attempt("sync", async () => {
    await nav("settings");
    await page.locator(".settings-nav-item", { hasText: "Encrypted sync" }).first().click();
    await shot("sync");
  });
  await browser.close();
}

finish();
