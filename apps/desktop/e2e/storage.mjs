// Change where visits and models are kept (ADR-0013), through Settings.
// Run ONLY against the throwaway storage demo, whose folders are all copies:
//   OPENBARK_URL=http://localhost:4322 node e2e/storage.mjs <storage-demo folder>
import { existsSync, mkdirSync, readFileSync } from "node:fs";
import { chromium } from "@playwright/test";

const base = process.env.OPENBARK_URL ?? "http://localhost:4322";
const out = process.env.OUT_DIR ?? "../../target/storage";
const root = process.argv[2];
if (!root || !existsSync(`${root}/locations.json`))
  throw new Error("usage: node e2e/storage.mjs <storage-demo folder>");
mkdirSync(out, { recursive: true });

const problems = [];
const check = (ok, what) => ok || problems.push(what);
const browser = await chromium.launch();
const ctx = await browser.newContext({ viewport: { width: 1440, height: 900 } });
await ctx.addInitScript(() => {
  const key = "openbark.settings";
  const current = JSON.parse(localStorage.getItem(key) ?? "{}");
  localStorage.setItem(
    key,
    JSON.stringify({ ...current, consentAcknowledgedAt: "2026-10-17T20:00:00Z", uiLanguage: "en" }),
  );
});
const page = await ctx.newPage();
page.on("pageerror", (e) => problems.push(`${e}`));
const settings = async () => {
  await page.locator('.nav-item[data-nav="settings"]').click();
  await page.locator(".storage-row").first().waitFor();
  await page.locator(".storage-row").first().scrollIntoViewIfNeeded();
};
await page.goto(base);
await page.locator(".screen h1").first().waitFor();
await settings();
await page.screenshot({ path: `${out}/1-storage.png` });

// Visits: browse, type a new folder, move.
await page.locator(".storage-row").nth(0).getByRole("button", { name: "Change folder" }).click();
await page.locator(".folder-browser").waitFor();
await page.locator(".folder-row").first().waitFor();
await page.screenshot({ path: `${out}/2-browse.png` });
await page.locator(".folder-browser input").fill(`${root}/Clinic share`);
await page.getByRole("button", { name: "Choose this folder" }).click();
await page.locator(".folder-target").waitFor();
check((await page.locator(".folder-mode input").first().isChecked()) === true, "Move should be chosen by default");
await page.screenshot({ path: `${out}/3-review.png` });
await page.getByRole("button", { name: "Move and use this folder" }).click();
await page.locator(".ob-notice[data-tone=ok]").waitFor({ timeout: 60000 });
await page.screenshot({ path: `${out}/4-done.png` });
await page.getByRole("button", { name: "Done" }).click();
await page.waitForTimeout(600);
const visitsPath = await page.locator(".storage-row").nth(0).locator(".storage-path").innerText();
check(visitsPath.endsWith("Clinic share"), `visits path should be the new folder, is ${visitsPath}`);
check(!existsSync(`${root}/visits`), "the old visits folder should be gone");
check(existsSync(`${root}/Clinic share/patients.json`), "patients.json should have moved");

// The visits still open from their new place.
await page.locator('.nav-item[data-nav="patients"]').click();
await page.locator(".patient-row").first().waitFor();
const names = await page.locator(".patient-name").allInnerTexts();
check(
  names.some((n) => n.includes("Juniper")),
  `Juniper should still be listed: ${names.join(", ")}`,
);
await page.locator(".patient-row", { hasText: "Juniper" }).click();
await page.locator(".visit-row").first().waitFor();
check((await page.locator(".visit-row").count()) === 2, "Juniper's two visits should still be there");

// Models: a folder that holds other files gets an "OpenBark models" folder inside.
await settings();
await page.locator(".storage-row").nth(1).getByRole("button", { name: "Change folder" }).click();
await page.locator(".folder-browser").waitFor();
await page.locator(".folder-browser input").fill(`${root}/misc`);
await page.getByRole("button", { name: "Choose this folder" }).click();
await page.locator(".folder-target").waitFor();
const target = await page.locator(".folder-target .storage-path").innerText();
check(target.endsWith("OpenBark models"), `models should go in a new folder inside misc, got ${target}`);
await page.screenshot({ path: `${out}/5-models-review.png` });
await page.getByRole("button", { name: "Move and use this folder" }).click();
await page.locator(".ob-notice[data-tone=ok]").waitFor({ timeout: 60000 });
await page.getByRole("button", { name: "Done" }).click();
check(existsSync(`${root}/misc/OpenBark models/fake-model/weights.gguf`), "the model should have moved");
check(existsSync(`${root}/misc/letter.txt`), "the other files in misc stay as they were");

// Choosing the folder already in use is refused, with nothing changed.
await page.locator(".storage-row").nth(1).getByRole("button", { name: "Change folder" }).click();
await page.locator(".folder-browser input").fill(`${root}/misc/OpenBark models`);
await page.getByRole("button", { name: "Choose this folder" }).click();
await page.locator(".folder-target").waitFor();
check((await page.locator(".ob-notice[data-tone=danger]").count()) === 1, "the current folder should be refused");
await page.screenshot({ path: `${out}/6-refused.png` });

const saved = JSON.parse(readFileSync(`${root}/locations.json`, "utf8"));
console.log("locations.json:", saved);
check(saved.visits?.endsWith("Clinic share"), "locations.json should hold the new visits folder");
await browser.close();
console.log(problems.length ? `PROBLEMS:\n${problems.join("\n")}` : "all checks passed");
