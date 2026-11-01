// Screenshots for DESIGN.md §12 (desktop light, desktop dark, mobile) of the
// web-mode UI. Usage: OPENBARK_URL=http://localhost:4317 pnpm screenshots
import { mkdirSync } from "node:fs";
import { chromium } from "@playwright/test";

const base = process.env.OPENBARK_URL ?? "http://localhost:4317";
const out = process.env.OUT_DIR ?? "../../target/screenshots";
mkdirSync(out, { recursive: true });

const variants = [
  { name: "desktop-light", viewport: { width: 1440, height: 900 }, colorScheme: "light", notice: true },
  { name: "desktop-dark", viewport: { width: 1440, height: 900 }, colorScheme: "dark" },
  { name: "laptop-light", viewport: { width: 1100, height: 760 }, colorScheme: "light" },
  { name: "mobile-light", viewport: { width: 390, height: 844 }, colorScheme: "light", isMobile: true, hasTouch: true },
  { name: "desktop-fr", viewport: { width: 1440, height: 900 }, colorScheme: "light", lang: "fr" },
  {
    name: "mobile-de",
    viewport: { width: 390, height: 844 },
    colorScheme: "light",
    isMobile: true,
    hasTouch: true,
    lang: "de",
  },
];

const browser = await chromium.launch();
for (const v of variants) {
  const ctx = await browser.newContext({
    viewport: v.viewport,
    colorScheme: v.colorScheme,
    isMobile: v.isMobile,
    hasTouch: v.hasTouch,
  });
  // The one-time consent notice is accepted up front (one variant shows it first).
  // The first variant also turns on the optional per-recording consent step to capture it.
  const seed = {
    consentAcknowledgedAt: v.notice ? null : "2026-10-17T20:00:00Z",
    setupDoneAt: "2026-10-17T20:00:00Z",
    consentEachTime: Boolean(v.notice),
    uiLanguage: v.lang ?? "en",
  };
  await ctx.addInitScript((settings) => {
    const key = "openbark.settings";
    const current = JSON.parse(localStorage.getItem(key) ?? "{}");
    if (!("consentAcknowledgedAt" in current)) localStorage.setItem(key, JSON.stringify({ ...current, ...settings }));
  }, seed);
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
  // Navigate by data-nav name so every interface language works (ADR-0027).
  // Phones show four tabs; Templates, Models and Help are rows under Settings.
  const PHONE_MORE = { Templates: 0, Models: 1, Help: 4 };
  const phone = v.viewport.width <= 640;
  const nav = async (screen) => {
    if (phone && screen in PHONE_MORE) {
      await page.locator('.nav-item[data-nav="settings"]').click();
      await page.locator(".settings-row").nth(PHONE_MORE[screen]).click();
    } else {
      await page.locator(`.nav-item[data-nav="${screen.toLowerCase()}"]`).click();
    }
    await page.locator(".screen h1").first().waitFor();
  };
  const english = !v.lang;

  await page.goto(base);
  if (v.notice) {
    await page.locator(".dialog").waitFor();
    await shot("consent-notice");
    await page.locator(".dialog input[type=checkbox]").check();
    await page.locator(".dialog .ob-button").click();
  }
  await page.locator(".recorder").waitFor();
  await shot("record");
  if (v.notice) {
    // With the consent step on, Record opens it (nothing records until it's confirmed).
    await page.locator(".ob-record").click();
    await page
      .locator("form.consent")
      .waitFor({ timeout: 3000 })
      .catch(() => {});
    await shot("record-consent");
    await page
      .locator("form.consent .ob-button[data-variant=ghost]")
      .last()
      .click()
      .catch(() => {});
  }

  // Patients opens on the patient list; the visits table is under Recent visits.
  const recentVisits = async () => {
    await page.locator('.nav-item[data-nav="visits"]').click();
    await page
      .locator(".row-link")
      .first()
      .waitFor()
      .catch(() => {});
  };
  await nav("Patients");
  await page
    .locator(".patient-list, .empty-state, .empty")
    .first()
    .waitFor()
    .catch(() => {});
  await shot("patients");
  await recentVisits();
  await shot("recent-visits");

  // A signed encounter if there is one (addenda, client documents), else the newest.
  // Visits are table rows on wider screens and stacked rows on phones; both are .row-link.
  const signedRow = page.locator('.row-link:has(.ob-dot[data-tone="ok"])').first();
  const firstRow = (await signedRow.count()) ? signedRow : page.locator(".row-link").first();
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
    const discharge = page.getByRole("button", { name: "Discharge instructions", exact: true });
    if (await discharge.count()) {
      await discharge.click();
      await page
        .locator(".ob-sheet .note")
        .waitFor({ timeout: 5000 })
        .catch(() => {});
      await shot("review-discharge");
      await page.keyboard.press("Escape");
    }
  }

  // A draft (second encounter): re-prompt and hand edit.
  await nav("Patients");
  await recentVisits();
  const draftRow = page.locator('.row-link:has(.ob-dot[data-tone="warn"])').first();
  if (english && (await draftRow.count())) {
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
  await page
    .locator(".tpl-row")
    .first()
    .waitFor()
    .catch(() => {});
  await shot("templates");
  // Templates are grouped button rows; the first group is the team's own when there are any.
  const team = page.locator(".tpl-group").first().locator(".tpl-row").first();
  if (await team.count()) {
    await team.click();
    await page.locator(".tb-heading").first().waitFor();
    await page.waitForTimeout(500);
    await shot("template-builder");
    await page.locator(".builder-preview [role=tab]").nth(1).click();
    await shot("template-prompt");
  }

  await nav("Models");
  await shot("models");
  await page.locator(".screen-head .ob-button[data-variant=primary]").click();
  await page
    .locator(".hub-row")
    .first()
    .waitFor({ timeout: 20000 })
    .catch(() => {});
  await shot("hub");
  await page.keyboard.press("Escape");

  await nav("Settings");
  await shot("settings");

  await nav("Help");
  await page.locator(".help-video").waitFor();
  await shot("help");

  console.log(`${v.name}: ${errors.length ? errors.join(" | ") : "no console errors, no horizontal scroll"}`);
  await ctx.close();
}
await browser.close();
