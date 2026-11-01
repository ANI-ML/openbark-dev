// Screenshots of the M2 features for review: the template library, example
// instructions (Ask and the builder), multi-pet tabs, and MLX labels in the
// Mac model browser. Run against a demo server, never a clinic's own data:
//   OPENBARK_URL=http://localhost:4321 node e2e/showcase.mjs
// The server should run with OPENBARK_PLATFORM=macos to show the MLX labels,
// and hold a dental encounter (patient Biscuit) and a multi-pet one (Olive).
import { mkdirSync } from "node:fs";
import { chromium } from "@playwright/test";

const base = process.env.OPENBARK_URL ?? "http://localhost:4321";
const out = process.env.OUT_DIR ?? "../../target/showcase";
mkdirSync(out, { recursive: true });

const variants = [
  { name: "desktop-light", viewport: { width: 1440, height: 900 }, colorScheme: "light" },
  { name: "desktop-dark", viewport: { width: 1440, height: 900 }, colorScheme: "dark" },
  { name: "mobile-light", viewport: { width: 390, height: 844 }, colorScheme: "light", isMobile: true, hasTouch: true },
];

const NAV = { Record: 0, Patients: 1, Templates: 2, Models: 3, Settings: 4, Help: 5 };
const problems = [];
const browser = await chromium.launch();
for (const v of variants) {
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
      JSON.stringify({ ...current, updateCheck: "off", consentAcknowledgedAt: "2026-10-17T20:00:00Z", uiLanguage: "en" }),
    );
  });
  const page = await ctx.newPage();
  page.on("pageerror", (e) => problems.push(`${v.name}: ${e}`));
  page.on("console", (m) => m.type() === "error" && problems.push(`${v.name}: ${m.text()}`));
  const shot = async (screen, opts = {}) => {
    await page.waitForTimeout(500);
    await page.screenshot({ path: `${out}/${v.name}-${screen}.png`, ...opts });
    const overflow = await page.evaluate(() => document.documentElement.scrollWidth > window.innerWidth);
    if (overflow) problems.push(`${v.name} ${screen}: horizontal scroll`);
  };
  const nav = async (screen) => {
    if (v.isMobile) {
      const menu = page.locator(".nav-toggle, [aria-label='Menu']");
      if (await menu.count()) await menu.first().click();
    }
    // Phones show four tabs (ADR-0027); Templates, Models and Help are rows under Settings.
    const more = { Templates: "Templates", Models: "Models and storage", Help: "Guides and videos" };
    if (v.viewport.width <= 640 && screen in more) {
      await page.locator('.nav-item[data-nav="settings"]').click();
      await page.locator(".settings-row", { hasText: more[screen] }).first().click();
    } else {
      await page.locator(`.nav-item[data-nav="${screen.toLowerCase()}"]`).click();
    }
    await page.locator(".screen h1").first().waitFor();
  };

  await page.goto(base);
  await page.locator(".recorder").waitFor();

  // 1. The template library.
  await nav("Templates");
  await page.locator(".tpl-row").first().waitFor();
  await shot("templates", { fullPage: !v.isMobile });

  // 2. The builder's examples: duplicate the dental template.
  await page.locator(".tpl-row", { hasText: "Dental procedure" }).first().click();
  await page.locator(".tb-examples").first().waitFor();
  await page.locator(".tb-examples").nth(2).scrollIntoViewIfNeeded();
  await shot("builtin-examples");
  await page.getByRole("button", { name: "Duplicate" }).click();
  await page.locator(".tb-list .tb-examples button:not([disabled])").first().waitFor();
  await page.locator(".tb-examples").nth(2).scrollIntoViewIfNeeded();
  await shot("builder-examples");
  await page.locator(".builder-advanced summary").click();
  await page.locator(".builder-advanced .tb-examples").scrollIntoViewIfNeeded();
  await shot("builder-practice-examples");

  // 3. Ask on a dental heading shows that heading's examples.
  await nav("Patients");
  await page.locator('.nav-item[data-nav="visits"]').click();
  await page.locator("tr.row-link", { hasText: "Biscuit" }).first().click();
  await page.locator(".note").first().waitFor({ timeout: 20000 });
  const ask = page.getByRole("button", { name: "Ask for changes to Findings by tooth" });
  await ask.scrollIntoViewIfNeeded();
  await ask.click();
  await page.locator(".ask-examples").waitFor();
  await page.locator(".ask").scrollIntoViewIfNeeded();
  await shot("ask-examples");

  // 4. A multi-pet visit: one tab per animal.
  await nav("Patients");
  await page.locator('.nav-item[data-nav="visits"]').click();
  await page.locator("tr.row-link", { hasText: "Olive" }).first().click();
  await page.locator(".pet-tabs").waitFor({ timeout: 20000 });
  await shot("multi-pet");
  await page.locator(".pet-tabs .ob-segment").nth(1).click();
  await page.waitForTimeout(800);
  await shot("multi-pet-second");

  // 5. Models on a Mac: MLX-optimized builds are labelled.
  await nav("Models");
  await page.getByRole("button", { name: "Browse Hugging Face" }).click();
  await page.locator(".hub-row").first().waitFor({ timeout: 30000 });
  await shot("hub-mlx");
  await page.locator("label.check", { hasText: "Only show MLX-optimized models" }).click();
  await page.waitForTimeout(300);
  await shot("hub-mlx-only");
  await page.locator(".hub-row").first().click();
  await page.locator(".ob-sheet .ob-notice, .ob-sheet table").first().waitFor({ timeout: 30000 });
  await shot("hub-mlx-repo");

  await ctx.close();
}
await browser.close();
if (problems.length) {
  console.error(problems.join("\n"));
  process.exit(1);
}
console.log(`screenshots in ${out}`);
