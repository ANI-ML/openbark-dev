// The phone's model policy in the real interface (ADR-0018 section 10): the
// Models screen and Settings, Storage, as an iPhone shows them, in light,
// dark and at phone width.
//
// Run ONLY against a phone demo, never a clinic's own server or data:
//   OPENBARK_URL=http://localhost:4335 node e2e/phone-models.mjs <demo folder>
//
// The demo is `openbark serve` started with OPENBARK_PLATFORM=ios and
// OPENBARK_DEVICE_RAM_GB=8 (an 8 GB iPhone), every folder in its own scratch
// folder and a throwaway OPENBARK_DATA_KEY, with an empty OPENBARK-DEMO file
// in that folder. With HUB=1 it also opens a live Hugging Face repo
// (unsloth/gemma-4-E4B-it-GGUF, network.md R4) and checks the held-back files.
//
// Before doing anything the script refuses unless the URL is this computer
// on port 4335 (never 4317) and the demo folder holds an OPENBARK-DEMO marker.

import { existsSync, mkdirSync } from "node:fs";
import { resolve } from "node:path";
import { chromium } from "@playwright/test";

const base = process.env.OPENBARK_URL ?? "http://localhost:4335";
const out = process.env.OUT_DIR ?? "../../target/phone-models";
const demo = process.argv[2] ? resolve(process.argv[2]) : null;
const url = new URL(base);

function refuse(why) {
  console.error(`refusing: ${why}`);
  process.exit(2);
}
if (url.port === "4317") refuse("port 4317 is the clinic's own OpenBark server");
if (url.port !== "4335") refuse(`the phone demo runs on port 4335, not ${url.port || "the default port"}`);
if (!["localhost", "127.0.0.1"].includes(url.hostname)) refuse("only this computer's demo can be used");
if (!demo || !existsSync(`${demo}/OPENBARK-DEMO`)) refuse("usage: node e2e/phone-models.mjs <demo folder with OPENBARK-DEMO>");
if (/[\\/]AppData[\\/]Local[\\/]OpenBark/i.test(demo)) refuse("that's inside OpenBark's own data folder");

const status = await (await fetch(`${base}/api/status`)).json();
if (status.profile !== "ios") refuse("start the demo with OPENBARK_PLATFORM=ios");
mkdirSync(out, { recursive: true });

const problems = [];
const check = (ok, what) => ok || problems.push(what);
check(status.mobile_tier === 8, `the demo should be an 8 GB iPhone, is ${status.mobile_tier}`);
check(status.default_llm === "gemma-4-e2b-it-q4_0", `8 GB default should be Gemma 4 E2B, is ${status.default_llm}`);
check(
  !status.models.some((m) => m.id.startsWith("gemma")),
  "a phone needs only the speech models to start (drafting is opt-in)",
);

const browser = await chromium.launch();
const variants = [
  { name: "desktop-light", viewport: { width: 1280, height: 900 }, colorScheme: "light" },
  { name: "desktop-dark", viewport: { width: 1280, height: 900 }, colorScheme: "dark" },
  { name: "phone-light", viewport: { width: 390, height: 844 }, colorScheme: "light", isMobile: true, hasTouch: true },
];
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
    localStorage.setItem(key, JSON.stringify({ ...current, consentAcknowledgedAt: "2026-10-17T20:00:00Z", uiLanguage: "en" }));
  });
  const page = await ctx.newPage();
  page.on("pageerror", (e) => problems.push(`${v.name}: ${e}`));
  await page.goto(base);
  await page.locator(".screen h1").first().waitFor();

  // Models: from the side bar on a wide screen, from Settings on a phone.
  if (v.isMobile) {
    await page.locator(".nav-item", { hasText: "Settings" }).first().click();
    await page.getByRole("button", { name: "Models" }).first().click();
  } else {
    await page.locator(".nav-item", { hasText: "Models" }).first().click();
  }
  await page.getByRole("heading", { name: "Models", level: 1 }).waitFor();
  await page.locator(".ob-table tbody tr").first().waitFor();
  const tier = await page.getByTestId("phone-tier").innerText();
  check(tier.includes("(8 GB)"), `${v.name}: the tier notice should name 8 GB: ${tier}`);
  check((await page.getByRole("button", { name: "Add from this computer" }).count()) === 0, `${v.name}: no add-from-computer on a phone`);
  check((await page.locator("#mlx-engine-card, .mlx-card").count()) === 0, `${v.name}: no MLX engine card on a phone`);
  const text = await page.locator(".ob-table").first().innerText();
  check(!text.includes("Gemma 4 12B"), `${v.name}: the 12B isn't listed on a phone`);
  check(text.includes("Qwen3 1.7B"), `${v.name}: Qwen3 1.7B is listed on a phone`);
  const e4b = page.locator("tr", { hasText: "Gemma 4 E4B" });
  check(
    (await e4b.innerText()).includes("Too large for this iPhone (8 GB)"),
    `${v.name}: E4B should say it's too large for this iPhone`,
  );
  check(await e4b.getByRole("button", { name: /Download/ }).isDisabled(), `${v.name}: E4B can't be downloaded`);
  await page.screenshot({ path: `${out}/models-${v.name}.png`, fullPage: true });

  if (process.env.HUB === "1" && v.name === "desktop-light") {
    await page.getByRole("button", { name: "Browse Hugging Face" }).click();
    await page.getByText("Small note-writing models in GGUF format that fit this iPhone.", { exact: false }).waitFor();
    check((await page.getByText("Only show MLX-optimized models").count()) === 0, "no MLX filter on a phone");
    await page.locator(".search input").fill("unsloth/gemma-4-E4B-it-GGUF");
    await page.getByRole("button", { name: "Search" }).click();
    await page.locator(".hub-name", { hasText: "unsloth/gemma-4-E4B-it-GGUF" }).first().click();
    await page.getByTestId("phone-note").waitFor({ timeout: 30000 });
    check((await page.getByText(/Not offered on this iPhone \(\d+\)/).count()) === 1, "held-back files are listed");
    await page.screenshot({ path: `${out}/hub-held.png`, fullPage: true });
    await page.keyboard.press("Escape");
  }

  // Settings, Storage: no folder picker on a phone.
  await page.locator(".nav-item", { hasText: "Settings" }).first().click();
  await page.locator(".storage-row").first().waitFor();
  await page.locator(".storage-row").first().scrollIntoViewIfNeeded();
  check((await page.getByRole("button", { name: "Change folder" }).count()) === 0, `${v.name}: no Change folder on a phone`);
  check(
    (await page.getByText("Recordings and notes stay in OpenBark's own storage on this phone.").count()) === 1,
    `${v.name}: Storage says where recordings stay`,
  );
  await page.screenshot({ path: `${out}/storage-${v.name}.png` });
  await ctx.close();
}
await browser.close();
console.log(problems.length ? `PROBLEMS:\n${problems.join("\n")}` : "all checks passed");
process.exit(problems.length ? 1 : 0);
