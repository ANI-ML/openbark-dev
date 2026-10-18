// Screenshots for DESIGN.md §12 (desktop light, desktop dark, mobile) of the
// web-mode UI. Usage: OPENBARK_URL=http://localhost:4317 pnpm screenshots
import { mkdirSync } from "node:fs";
import { chromium } from "@playwright/test";

const base = process.env.OPENBARK_URL ?? "http://localhost:4317";
const out = process.env.OUT_DIR ?? "../../target/screenshots";
mkdirSync(out, { recursive: true });

const variants = [
  { name: "desktop-light", viewport: { width: 1440, height: 900 }, colorScheme: "light" },
  { name: "desktop-dark", viewport: { width: 1440, height: 900 }, colorScheme: "dark" },
  { name: "laptop-light", viewport: { width: 1100, height: 760 }, colorScheme: "light" },
  { name: "mobile-light", viewport: { width: 390, height: 844 }, colorScheme: "light", isMobile: true, hasTouch: true },
];

const browser = await chromium.launch();
for (const v of variants) {
  const ctx = await browser.newContext({ viewport: v.viewport, colorScheme: v.colorScheme, isMobile: v.isMobile, hasTouch: v.hasTouch });
  const page = await ctx.newPage();
  const errors = [];
  page.on("pageerror", (e) => errors.push(String(e)));
  page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
  const shot = async (screen) => {
    await page.waitForTimeout(400);
    await page.screenshot({ path: `${out}/${v.name}-${screen}.png`, fullPage: false });
    const overflow = await page.evaluate(() => document.documentElement.scrollWidth > window.innerWidth);
    if (overflow) errors.push(`${screen}: horizontal scroll at ${window.innerWidth}px`);
  };
  const nav = (label) => page.getByRole("button", { name: label, exact: true }).first().click();

  await page.goto(base);
  await page.getByRole("heading", { name: "Record" }).waitFor();
  await shot("record");
  const recordButton = page.getByRole("button", { name: /Start recording/ }).first();
  if (await recordButton.count()) {
    await recordButton.click();
    await page.locator("form.consent").waitFor({ timeout: 3000 }).catch(() => {});
    await shot("record-consent");
    await page.getByRole("button", { name: "Cancel", exact: true }).click().catch(() => {});
  }

  await nav("Encounters");
  await page.getByRole("heading", { name: "Encounters" }).waitFor();
  await shot("encounters");

  const firstRow = page.locator("tr.row-link").first();
  if (await firstRow.count()) {
    await firstRow.click();
    await page.locator(".note, .pane-pad").first().waitFor();
    await page.waitForTimeout(600);
    await shot("review");
    const stmt = page.locator(".stmt").nth(3);
    if (await stmt.count()) {
      await stmt.hover();
      await shot("review-evidence");
    }
    const addenda = page.locator(".addenda");
    if (await addenda.count()) {
      await addenda.scrollIntoViewIfNeeded();
      await shot("review-signed-addenda");
    }
  }

  // A draft (second encounter): re-prompt and hand edit.
  await nav("Encounters");
  await page.locator("tr.row-link").first().waitFor().catch(() => {});
  const draftRow = page.locator("tr.row-link").nth(1);
  if (await draftRow.count()) {
    await draftRow.click();
    await page.locator(".note, .pane-pad").first().waitFor();
    await page.waitForTimeout(600);
    const ask = page.getByRole("button", { name: /^Ask for changes to / }).nth(1);
    if (await ask.count()) {
      await ask.click();
      await page.locator("form.ask").scrollIntoViewIfNeeded();
      await shot("review-ask");
    }
    const edit = page.getByRole("button", { name: "Edit Follow-up" });
    if (await edit.count()) {
      await edit.click();
      await page.locator("form.edit-heading").scrollIntoViewIfNeeded();
      await shot("review-edit");
    }
  }

  await nav("Templates");
  await page.getByRole("heading", { name: "Templates" }).waitFor();
  await shot("templates");
  const team = page.locator("tr.row-link").nth(1);
  if (await team.count()) {
    await team.click();
    await page.locator(".tb-heading").first().waitFor();
    await page.waitForTimeout(500);
    await shot("template-builder");
    await page.getByRole("tab", { name: "What the model is told" }).click();
    await shot("template-prompt");
    await page.getByRole("button", { name: "Templates", exact: true }).nth(1).click().catch(() => {});
  }

  await nav("Models");
  await page.getByRole("heading", { name: "Models" }).waitFor();
  await shot("models");
  await page.getByRole("button", { name: "Browse Hugging Face" }).click();
  await page.locator(".hub-row").first().waitFor({ timeout: 20000 }).catch(() => {});
  await shot("hub");
  await page.keyboard.press("Escape");

  await nav("Settings");
  await page.getByRole("heading", { name: "Settings" }).waitFor();
  await shot("settings");

  console.log(`${v.name}: ${errors.length ? errors.join(" | ") : "no console errors, no horizontal scroll"}`);
  await ctx.close();
}
await browser.close();
