// Screenshots and layout checks of the public website (website/dist served locally):
//   python -m http.server -d website/dist 8099 &  node e2e/website.mjs
import { mkdirSync } from "node:fs";
import { chromium } from "@playwright/test";

const base = process.env.SITE_URL ?? "http://localhost:8099";
const out = process.env.OUT_DIR ?? "../../target/website";
mkdirSync(out, { recursive: true });
const problems = [];
const browser = await chromium.launch();
for (const v of [
  { name: "desktop-light", viewport: { width: 1440, height: 900 }, colorScheme: "light" },
  { name: "desktop-dark", viewport: { width: 1440, height: 900 }, colorScheme: "dark" },
  { name: "phone-light", viewport: { width: 390, height: 844 }, colorScheme: "light", isMobile: true, hasTouch: true },
]) {
  const ctx = await browser.newContext({ viewport: v.viewport, colorScheme: v.colorScheme, isMobile: v.isMobile, hasTouch: v.hasTouch });
  const page = await ctx.newPage();
  page.on("pageerror", (e) => problems.push(`${v.name}: ${e}`));
  page.on("console", (m) => m.type() === "error" && problems.push(`${v.name}: ${m.text()}`));
  for (const p of ["index.html", "features.html", "404.html"]) {
    await page.goto(`${base}/${p}`);
    await page.waitForLoadState("networkidle");
    await page.waitForTimeout(1200);
    const overflow = await page.evaluate(() => document.documentElement.scrollWidth > window.innerWidth);
    if (overflow) problems.push(`${v.name} ${p}: horizontal scroll`);
    await page.screenshot({ path: `${out}/${v.name}-${p.replace(".html", "")}.png` });
    if (!v.isMobile) {
      // Scroll through once so every section has risen in, then capture the whole page.
      await page.evaluate(async () => {
        for (let y = 0; y < document.body.scrollHeight; y += 500) {
          window.scrollTo({ top: y, behavior: "instant" });
          await new Promise((r) => setTimeout(r, 60));
        }
        window.scrollTo({ top: 0, behavior: "instant" });
      });
      await page.waitForTimeout(1200);
      const hidden = await page.evaluate(
        () => [...document.querySelectorAll(".reveal")].filter((el) => getComputedStyle(el).opacity !== "1").length,
      );
      if (hidden) problems.push(`${v.name} ${p}: ${hidden} sections never appeared`);
      await page.screenshot({ path: `${out}/${v.name}-${p.replace(".html", "")}-full.png`, fullPage: true });
    }
  }
  // The demo: hovering a note line lights its transcript line.
  await page.goto(`${base}/index.html`);
  await page.locator('.note button[data-ev="4"]').hover();
  const lit = await page.locator('#tx li.lit').getAttribute("data-id");
  if (lit !== "4") problems.push(`${v.name}: demo lit ${lit}, expected 4`);
  // The features filter.
  await page.goto(`${base}/features.html`);
  await page.locator("#fx-q").fill("dental");
  const shown = await page.locator(".fx-item:not([hidden])").count();
  if (shown < 1) problems.push(`${v.name}: filter "dental" shows nothing`);
  await ctx.close();
}
await browser.close();
console.log(problems.length ? `PROBLEMS:\n${problems.join("\n")}` : "all checks passed");
