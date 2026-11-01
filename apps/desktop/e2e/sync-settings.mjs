// Screenshots of Settings > Encrypted sync and of a synced visit in Review
// (DESIGN.md §12: desktop light, desktop dark, mobile), against demo servers
// syncing through a local Supabase (M3 phase 2). Synthetic data only.
//   OPENBARK_URL=http://localhost:4336 OUT_DIR=... node e2e/sync-settings.mjs [visit-id]
import { mkdirSync } from "node:fs";
import { chromium } from "@playwright/test";

const base = process.env.OPENBARK_URL ?? "http://localhost:4336";
const out = process.env.OUT_DIR ?? "../../target/screenshots/sync";
const visit = process.argv[2];
const tag = process.env.TAG ?? "a";
mkdirSync(out, { recursive: true });

const variants = [
  { name: "desktop-light", viewport: { width: 1440, height: 900 }, colorScheme: "light" },
  { name: "desktop-dark", viewport: { width: 1440, height: 900 }, colorScheme: "dark" },
  { name: "mobile-light", viewport: { width: 390, height: 844 }, colorScheme: "light", isMobile: true, hasTouch: true },
];

const browser = await chromium.launch();
const problems = [];
for (const v of variants) {
  const ctx = await browser.newContext({ viewport: v.viewport, colorScheme: v.colorScheme, isMobile: v.isMobile, hasTouch: v.hasTouch });
  await ctx.addInitScript(() => {
    localStorage.setItem("openbark.settings", JSON.stringify({ updateCheck: "off", consentAcknowledgedAt: "2026-10-17T20:00:00Z", uiLanguage: "en" }));
  });
  const page = await ctx.newPage();
  page.on("pageerror", (e) => problems.push(`${v.name}: ${e}`));
  const overflow = async (what) => {
    if (await page.evaluate(() => document.documentElement.scrollWidth > window.innerWidth)) problems.push(`${v.name} ${what}: horizontal scroll`);
  };
  await page.goto(`${base}/`);
  await page.locator('.nav-item[data-nav="settings"]').click();
  // Encrypted sync is its own section (ADR-0027).
  await page.locator(".settings-nav-item, .settings-row", { hasText: "Encrypted sync" }).first().click();
  const section = page.locator(".sync-section");
  await section.waitFor();
  await section.scrollIntoViewIfNeeded();
  await page.waitForTimeout(600);
  await section.screenshot({ path: `${out}/${tag}-${v.name}-settings.png` });
  await overflow("settings");
  if (visit) {
    // Patients, then the visit's row in the recent visits list.
    await page.locator('.nav-item[data-nav="patients"]').click();
    await page.getByText(visit, { exact: false }).first().click();
    // A patient's page lists their visits: open the latest.
    await page.getByText("SOAP note").first().click();
    await page.waitForTimeout(1500);
    await page.screenshot({ path: `${out}/${tag}-${v.name}-review.png` });
    await overflow("review");
  }
  await ctx.close();
}
await browser.close();
if (problems.length) {
  console.error(problems.join("\n"));
  process.exit(1);
}
console.log(`screenshots in ${out}`);
