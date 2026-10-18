// Screenshot frames of a help video at given seconds, to check it without a
// video player: node e2e/frames.mjs <topic> <seconds...>
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { chromium } from "@playwright/test";

const [id, ...times] = process.argv.slice(2);
const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: 1280, height: 800 } });
await page.goto(pathToFileURL(resolve(`public/help/${id}.webm`)).href);
await page.waitForTimeout(800);
const duration = await page.evaluate(() => document.querySelector("video")?.duration);
console.log(id, "duration", Number.isFinite(duration) ? duration.toFixed(1) : duration);
for (const t of times) {
  await page.evaluate((s) => {
    const v = document.querySelector("video");
    v.pause();
    v.currentTime = s;
  }, Number(t));
  await page.waitForTimeout(700);
  await page.locator("video").screenshot({ path: `../../target/screenshots/frame-${id}-${t}.png` });
}
await browser.close();
