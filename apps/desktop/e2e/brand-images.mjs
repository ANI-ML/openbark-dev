// Brand images that live outside the apps: the GitHub social preview
// (1280x640, uploaded by hand in each repo's Settings) and the website's
// Open Graph image (1200x630). Rendered from HTML with the brand fonts.
//   cd apps/desktop && node e2e/brand-images.mjs
// The website's own copy, website/public/og-image.png, says "open source" only
// once the code is public (scripts/website-build.py SOURCE_URL):
//   node e2e/brand-images.mjs --og                 (code not public yet: "free")
//   SOURCE_PUBLIC=1 node e2e/brand-images.mjs --og (code public)
import { mkdirSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "@playwright/test";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const lockup = readFileSync(join(root, "docs/brand/logos/openbark-lockup-horizontal-outlined-on-dark.svg"), "utf8");
const font = (f) => `url(data:font/woff2;base64,${readFileSync(join(root, "packages/ui/src/fonts", f)).toString("base64")})`;

const page = (w, h, line, sub = "Open source. Runs on your own computer. Your records stay with you.") => `<!doctype html><html><head><meta charset="utf-8"><style>
@font-face { font-family: Manrope; src: ${font("Manrope-latin.woff2")}; font-weight: 200 800; }
html, body { margin: 0; width: ${w}px; height: ${h}px; }
body { background: #182230; color: #F5F7FA; font-family: Manrope, sans-serif; display: flex; flex-direction: column; justify-content: center; padding: 0 ${Math.round(w * 0.085)}px; box-sizing: border-box; gap: ${Math.round(h * 0.06)}px; }
.lockup svg { height: ${Math.round(h * 0.16)}px; width: auto; display: block; }
h1 { margin: 0; font-size: ${Math.round(h * 0.085)}px; line-height: 1.12; font-weight: 800; letter-spacing: -0.02em; max-width: 15em; }
p { margin: 0; font-size: ${Math.round(h * 0.04)}px; line-height: 1.4; font-weight: 500; color: #B4BCC8; }
</style></head><body><div class="lockup">${lockup}</div><h1>${line}</h1><p>${sub}</p></body></html>`;

const browser = await chromium.launch();
if (process.argv.includes("--og")) {
  const open = process.env.SOURCE_PUBLIC === "1";
  const p = await browser.newPage({ viewport: { width: 1200, height: 630 } });
  await p.setContent(
    open
      ? page(1200, 630, "The free, open-source AI scribe for veterinary medicine.")
      : page(1200, 630, "The free AI scribe for veterinary medicine.", "No subscription. Runs on your own computer. Your records stay with you."),
  );
  await p.waitForTimeout(300);
  const target = join(root, "website/public/og-image.png");
  await p.screenshot({ path: target });
  await browser.close();
  console.log(`wrote ${target}`);
  process.exit(0);
}
const out = join(root, "docs/brand/images");
mkdirSync(out, { recursive: true });
for (const [name, w, h] of [["github-social-preview.png", 1280, 640], ["og-image.png", 1200, 630]]) {
  const p = await browser.newPage({ viewport: { width: w, height: h } });
  await p.setContent(page(w, h, "The free, open-source AI scribe for veterinary medicine."));
  await p.waitForTimeout(300);
  await p.screenshot({ path: join(out, name) });
  await p.close();
}
// Windows setup wizard art (scripts/installer-art.py turns these into the BMPs NSIS needs).
const mark = readFileSync(join(root, "docs/brand/logos/openbark-mark-on-dark.svg"), "utf8");
const wiz = (w, h, body, bg) => `<!doctype html><html><head><meta charset="utf-8"><style>
@font-face { font-family: Manrope; src: ${font("Manrope-latin.woff2")}; font-weight: 200 800; }
html, body { margin: 0; width: ${w}px; height: ${h}px; overflow: hidden; }
body { background: ${bg}; font-family: Manrope, sans-serif; }
</style></head><body>${body}</body></html>`;
const sidebar = wiz(164, 314, `<div style="display:flex;flex-direction:column;align-items:center;padding-top:58px;gap:18px;color:#F5F7FA">
  <div style="width:72px;height:72px">${mark.replace("<svg ", '<svg style="width:72px;height:72px" ')}</div>
  <div style="font-size:26px;letter-spacing:-0.03em;line-height:1"><span style="font-weight:500">open</span><span style="font-weight:800">bark</span></div>
  <div style="font-size:12px;line-height:17px;font-weight:500;color:#B4BCC8;text-align:center">Veterinary scribe<br>that stays on<br>this computer</div></div>`, "#182230");
const markLight = readFileSync(join(root, "docs/brand/logos/openbark-mark.svg"), "utf8");
const header = wiz(150, 57, `<div style="position:absolute;right:10px;top:8px;width:41px;height:41px">${markLight.replace("<svg ", '<svg style="width:41px;height:41px" ')}</div>`, "#FFFFFF");
for (const [name, w, h, html] of [["installer-sidebar.png", 164, 314, sidebar], ["installer-header.png", 150, 57, header]]) {
  const p = await browser.newPage({ viewport: { width: w, height: h } });
  await p.setContent(html);
  await p.waitForTimeout(300);
  await p.screenshot({ path: join(out, name) });
  await p.close();
}
await browser.close();
console.log(`wrote ${out}`);
