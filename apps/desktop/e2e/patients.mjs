// Screenshots and checks of Patients and visits (ADR-0011): the patient list,
// a patient's page, editing details, and choosing a patient on Record. Run
// against a demo server, never a clinic's own data:
//   OPENBARK_URL=http://localhost:4321 node e2e/patients.mjs
// The demo should hold a patient with two visits (Juniper).
import { mkdirSync } from "node:fs";
import { chromium } from "@playwright/test";

const base = process.env.OPENBARK_URL ?? "http://localhost:4321";
const out = process.env.OUT_DIR ?? "../../target/patients";
mkdirSync(out, { recursive: true });

const variants = [
  { name: "desktop-light", viewport: { width: 1440, height: 900 }, colorScheme: "light" },
  { name: "desktop-dark", viewport: { width: 1440, height: 900 }, colorScheme: "dark" },
  { name: "mobile-light", viewport: { width: 390, height: 844 }, colorScheme: "light", isMobile: true, hasTouch: true },
];

const problems = [];
const check = (ok, what) => ok || problems.push(what);
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
      JSON.stringify({ ...current, consentAcknowledgedAt: "2026-10-17T20:00:00Z", uiLanguage: "en" }),
    );
  });
  const page = await ctx.newPage();
  page.on("pageerror", (e) => problems.push(`${v.name}: ${e}`));
  page.on("console", (m) => m.type() === "error" && problems.push(`${v.name}: ${m.text()}`));
  const shot = async (screen, opts = {}) => {
    await page.waitForTimeout(450);
    await page.screenshot({ path: `${out}/${v.name}-${screen}.png`, ...opts });
    const overflow = await page.evaluate(() => document.documentElement.scrollWidth > window.innerWidth);
    check(!overflow, `${v.name} ${screen}: horizontal scroll`);
  };
  const nav = async (i) => {
    if (v.isMobile) {
      const menu = page.locator(".nav-toggle, [aria-label='Menu']");
      if (await menu.count()) await menu.first().click();
    }
    await page.locator(".nav-item").nth(i).click();
    await page.locator(".screen h1").first().waitFor();
  };

  await page.goto(base);
  await page.locator(".recorder").waitFor();

  // 1. The patient list.
  await nav(1);
  await page.locator(".patient-list").waitFor();
  check((await page.locator(".nav-item").nth(1).innerText()).includes("Patients"), `${v.name}: nav says Patients`);
  const rows = await page.locator(".patient-row").count();
  check(rows > 0, `${v.name}: no patient rows`);
  await shot("patients", { fullPage: !v.isMobile });

  // Filter by name.
  await page.locator(".patient-filter input").fill("jun");
  await page.waitForTimeout(200);
  check((await page.locator(".patient-row").count()) === 1, `${v.name}: filter "jun" should leave one patient`);
  await shot("patients-filtered");
  await page.locator(".patient-filter input").fill("");

  // Recent visits: the day's table.
  await page.locator(".patients-bar .ob-segment").nth(1).click();
  await page.locator("tr.row-link").first().waitFor();
  await shot("recent-visits");
  await page.locator(".patients-bar .ob-segment").nth(0).click();

  // 2. A patient's page.
  await page.locator(".patient-row", { hasText: "Juniper" }).first().click();
  await page.locator(".visit-list").waitFor();
  const visits = await page.locator(".visit-row").count();
  check(visits >= 2, `${v.name}: Juniper should have 2 visits, has ${visits}`);
  await shot("patient", { fullPage: !v.isMobile });

  // Edit details (opened and cancelled: the demo's record stays as it is).
  await page.getByRole("button", { name: "Edit details" }).click();
  await page.locator(".patient-edit").waitFor();
  await shot("patient-edit");
  await page.locator(".patient-edit").getByRole("button", { name: "Cancel" }).click();

  // Reprocess: the same type again first, then the other types.
  await page.locator(".reprocess-btn:not([disabled])").first().click();
  await page.locator(".reprocess-row.again").waitFor();
  check((await page.locator(".reprocess-list .reprocess-row").count()) > 5, `${v.name}: other visit types listed`);
  await shot("reprocess");
  if (v.name === "desktop-light") {
    await page.locator(".reprocess-row.again").click();
    await page.locator(".review-bar").waitFor({ timeout: 20000 });
    // Drafting starts by itself (the button turns to its busy state or tokens stream in).
    await page.waitForTimeout(1500);
    await shot("reprocess-drafting");
    await page.locator(".nav-item").nth(1).click();
    await page.locator(".patient-row", { hasText: "Juniper" }).first().click();
    await page.locator(".visit-list").waitFor();
  } else {
    await page.keyboard.press("Escape");
    await page.waitForTimeout(500);
  }

  // A visit opens in Review.
  await page.locator(".visit-row").first().click();
  await page.locator(".review-bar, .pane-pad").first().waitFor({ timeout: 20000 });
  await shot("visit-review");

  // 3. New visit from the patient's page: Record opens with the patient chosen.
  await nav(1);
  await page.locator(".patient-row", { hasText: "Juniper" }).first().click();
  await page.locator(".visit-list").waitFor();
  await page.locator(".head-actions").getByRole("button", { name: "New visit" }).click();
  await page.locator(".picked-patient").waitFor();
  check(
    (await page.locator(".picked-name").innerText()).includes("Juniper"),
    `${v.name}: Record should start with Juniper`,
  );
  await shot("record-picked");

  // 4. Choosing a patient on Record.
  await page.getByRole("button", { name: "Choose another" }).click();
  await page.locator(".patient-search input").fill("ol");
  await page.locator(".patient-match").first().waitFor();
  await shot("record-find");
  await page.locator(".patient-match").first().click();
  await page.locator(".picked-patient").waitFor();
  const filled = await page
    .locator(".patient-fields input")
    .first()
    .inputValue()
    .catch(() => "");
  check(filled.length > 0, `${v.name}: choosing a patient should fill the name`);

  // A name no one has yet.
  await page.getByRole("button", { name: "Choose another" }).click();
  await page.locator(".patient-search input").fill("zzz");
  await page.waitForTimeout(200);
  check((await page.locator(".patient-search p").count()) === 1, `${v.name}: no-match hint`);
  await shot("record-no-match");
  await ctx.close();
}
await browser.close();
console.log(problems.length ? `PROBLEMS:\n${problems.join("\n")}` : "all checks passed");
