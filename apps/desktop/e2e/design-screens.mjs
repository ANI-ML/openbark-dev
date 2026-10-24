// Every main screen as it looks today, for design review: phone (390 x 844,
// light and dark) and desktop (1440 x 900, light and dark). It records, it
// doesn't assert: a screen it can't reach is listed, not fatal.
//
// Run ONLY against a demo server holding invented visits (the Design screens
// workflow imports the synthetic smoke consult under invented patients):
//   OPENBARK_URL=http://localhost:4339 VARIANTS=phone node e2e/design-screens.mjs <demo folder>
//
// It refuses unless the URL is this computer (never port 4317) and the demo
// folder holds an OPENBARK-DEMO marker.

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
const all = [
  // WebKit for the phone: the iPhone app runs in WKWebView.
  { name: "phone-light", engine: webkit, colorScheme: "light", ...phone },
  { name: "phone-dark", engine: webkit, colorScheme: "dark", ...phone },
  { name: "desktop-light", engine: chromium, colorScheme: "light", ...desktop },
  { name: "desktop-dark", engine: chromium, colorScheme: "dark", ...desktop },
];
const want = (process.env.VARIANTS ?? "phone,desktop").split(",");
const variants = all.filter((v) => want.some((w) => v.name.startsWith(w)));

const NAV = { Record: 0, Patients: 1, Templates: 2, Models: 3, Settings: 4, Help: 5 };
const missed = [];
const taken = [];

for (const v of variants) {
  const browser = await v.engine.launch();
  const ctx = await browser.newContext({
    viewport: v.viewport,
    colorScheme: v.colorScheme,
    isMobile: v.isMobile,
    hasTouch: v.hasTouch,
    deviceScaleFactor: v.deviceScaleFactor,
  });
  await ctx.addInitScript(() => {
    const key = "openbark.settings";
    const current = JSON.parse(localStorage.getItem(key) ?? "{}");
    localStorage.setItem(
      key,
      JSON.stringify({ ...current, consentAcknowledgedAt: "2026-10-17T20:00:00Z", uiLanguage: "en" }),
    );
  });
  const page = await ctx.newPage();
  const shot = async (screen, opts = {}) => {
    await page.waitForTimeout(600);
    const file = `${v.name}-${screen}.png`;
    await page.screenshot({ path: `${out}/${file}`, ...opts });
    taken.push(file);
  };
  const nav = async (screen) => {
    // Phones show four tabs (DESIGN.md §9); Models and Help are rows under Settings.
    const more = { Models: 0, Help: 1 };
    if (v.viewport.width <= 640 && screen in more) {
      await page.locator(".nav-item").nth(NAV.Settings).click();
      await page.locator(".phone-more-row").nth(more[screen]).click();
    } else {
      await page.locator(".nav-item").nth(NAV[screen]).click();
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
  await attempt("patients", async () => {
    await nav("Patients");
    await page.waitForTimeout(800);
    await shot("patients");
  });
  await attempt("visits", async () => {
    // The visit list: the second segment of the Patients bar.
    await page.locator(".patients-bar .ob-segment").nth(1).click();
    await page.waitForTimeout(800);
    await shot("visits");
  });
  await attempt("review", async () => {
    await page.locator(".row-link:visible").first().click();
    await page.locator(".note, .transcript").first().waitFor({ timeout: 20000 });
    await shot("review");
    await shot("review-full", { fullPage: true });
  });
  await attempt("patient-detail", async () => {
    await nav("Patients");
    await page.locator(".patients-bar .ob-segment").nth(0).click();
    await page.locator(".row-link:visible").first().click();
    await page.waitForTimeout(1000);
    await shot("patient-detail");
  });
  for (const screen of ["Templates", "Models", "Settings", "Help"]) {
    await attempt(screen.toLowerCase(), async () => {
      await nav(screen);
      await page.waitForTimeout(800);
      await shot(screen.toLowerCase());
      await shot(`${screen.toLowerCase()}-full`, { fullPage: true });
    });
  }
  await ctx.close();
  await browser.close();
}

writeFileSync(`${out}/index.json`, JSON.stringify({ taken, missed }, null, 2));
console.log(`${taken.length} screenshots in ${out}`);
if (missed.length) console.log(`Not reached:\n${missed.join("\n")}`);
