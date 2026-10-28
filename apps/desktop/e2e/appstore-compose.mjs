// Lays out the App Store images from the screens appstore-screens.mjs took:
//   screenshots-light/, screenshots-dark/  captioned, 1320 x 2868 (6.9" iPhone)
//   plain-light/, plain-dark/             the screen alone with the iPhone's
//                                         status bar, 1320 x 2868
//   header.png (1920 x 1080), header@2x.png (3840 x 2160)
//   search-preview.png                    how the first three look in search
// Flat, per docs/brand/DESIGN.md: Manrope, ink and one amber, no gradients.
//
//   node e2e/appstore-compose.mjs <screens folder> <out folder>

import { existsSync, mkdirSync, readFileSync } from "node:fs";
import { resolve } from "node:path";
import { chromium } from "@playwright/test";

const [src, dst] = process.argv.slice(2).map((p) => resolve(p));
if (!src || !dst) {
  console.error("usage: node e2e/appstore-compose.mjs <screens folder> <out folder>");
  process.exit(2);
}
const icon = readFileSync(resolve("../../docs/brand/logos/openbark-app-icon-dark.svg"), "utf8");

// The order is the listing's: the first three are what search shows.
const SLIDES = [
  ["record", "Record the visit. Get the note.", "A free, open-source scribe for veterinary teams."],
  ["evidence", "Check each line against what was said", "Tap a sentence to see where it came from in the transcript."],
  ["welcome", "Recordings stay on this iPhone", "Speech is transcribed on the phone, even offline."],
  ["visits", "Every visit, sorted by what needs you", "One clear status for each visit."],
  ["transcript", "Who said what, labelled", "The full transcript beside every note."],
  ["dental", "Chart a dental as you talk", "Drafted from the conversation. You confirm every tooth."],
  ["templates", "Templates for every kind of visit", "Wellness, dental, dermatology, exotics and more. Make them your own."],
  ["sync", "Finish the note at your desk", "Encrypted sync through your clinic's own computer, in preview."],
];

const T = {
  light: { bg: "#F2F4F7", ink: "#182230", text2: "#4B5563", frame: "#182230" },
  dark: { bg: "#0F151D", ink: "#F5F7FA", text2: "#B4BCC8", frame: "#2A3546" },
};

const png = (name) => {
  const p = `${src}/${name}.png`;
  return existsSync(p) ? `data:image/png;base64,${readFileSync(p).toString("base64")}` : null;
};

const FONT = `<link rel="preconnect" href="https://fonts.googleapis.com"><link href="https://fonts.googleapis.com/css2?family=Manrope:wght@500;600;800&display=swap" rel="stylesheet">`;

// An iPhone screen, 440 x 956 points: status bar (54), the app (868), home
// indicator (34). `bar` and `foot` are the app's own top and bottom colours.
function screen(img, scheme, w) {
  const s = w / 440;
  const fg = scheme === "dark" ? "#FFFFFF" : "#000000";
  return `
  <div class="screen" style="width:${w}px;height:${956 * s}px;border-radius:${62 * s}px">
    <div class="status" data-fill="top" style="height:${54 * s}px;padding:0 ${34 * s}px;font-size:${17 * s}px;color:${fg}">
      <span style="padding-left:${16 * s}px">9:41</span>
      <div class="island" style="width:${124 * s}px;height:${36 * s}px;top:${11 * s}px"></div>
      <svg viewBox="0 0 78 14" style="height:${13 * s}px;padding-right:${4 * s}px" fill="${fg}">
        <rect x="0" y="9" width="3.4" height="5" rx="1"/><rect x="5" y="6.5" width="3.4" height="7.5" rx="1"/>
        <rect x="10" y="4" width="3.4" height="10" rx="1"/><rect x="15" y="1" width="3.4" height="13" rx="1"/>
        <path d="M30.5 3.2a11 11 0 0 1 15 0l-1.5 1.6a8.8 8.8 0 0 0-12 0zM33.4 6.3a6.8 6.8 0 0 1 9.2 0l-1.6 1.6a4.6 4.6 0 0 0-6 0zM38 13l-2.6-2.7a3.6 3.6 0 0 1 5.2 0z"/>
        <rect x="52.5" y="1" width="22" height="12" rx="3.6" fill="none" stroke="${fg}" stroke-opacity=".4"/>
        <rect x="54.5" y="3" width="18" height="8" rx="2"/><rect x="75.5" y="4.6" width="1.6" height="4.6" rx=".8" fill-opacity=".4"/>
      </svg>
    </div>
    <img src="${img}" style="width:${w}px;height:${868 * s}px">
    <div class="home" data-fill="bottom" style="height:${34 * s}px"><i style="width:${140 * s}px;height:${5 * s}px;border-radius:${3 * s}px;background:${fg}"></i></div>
  </div>`;
}

function phone(img, scheme, w) {
  const b = Math.round(w * 0.024);
  return `<div class="phone" style="padding:${b}px;border-radius:${(62 * w) / 440 + b}px;background:${T[scheme === "dark" ? "dark" : "light"].frame}">${screen(img, scheme, w)}</div>`;
}

const BASE_CSS = `
  *{box-sizing:border-box;margin:0}
  body{font-family:Manrope,system-ui,sans-serif;-webkit-font-smoothing:antialiased}
  .screen{position:relative;overflow:hidden;display:flex;flex-direction:column;background:#fff}
  .screen img{display:block}
  .status{display:flex;align-items:center;justify-content:space-between;font-weight:600;position:relative;flex:none}
  .island{position:absolute;left:50%;transform:translateX(-50%);background:#000;border-radius:999px}
  .home{display:flex;align-items:center;justify-content:center;flex:none}
`;

// Fill the status bar and home indicator with the app's own edge colours.
const FILL = `
  async function fill(){
    for (const el of document.querySelectorAll('.screen')) {
      const img = el.querySelector('img');
      await img.decode();
      const c = document.createElement('canvas'); c.width = img.naturalWidth; c.height = img.naturalHeight;
      const x = c.getContext('2d'); x.drawImage(img, 0, 0);
      const at = (y) => { const d = x.getImageData(4, y, 1, 1).data; return 'rgb(' + d[0] + ',' + d[1] + ',' + d[2] + ')'; };
      el.querySelector('[data-fill=top]').style.background = at(2);
      el.querySelector('[data-fill=bottom]').style.background = at(img.naturalHeight - 3);
    }
    await document.fonts.ready;
    document.body.dataset.ready = '1';
  }
  fill();
`;

const page = (body, css) => `<!doctype html><html><head><meta charset="utf-8">${FONT}<style>${BASE_CSS}${css}</style></head><body>${body}<script>${FILL}</script></body></html>`;

const browser = await chromium.launch();
const ctx = await browser.newContext({ viewport: { width: 1320, height: 2868 }, deviceScaleFactor: 1 });
const p = await ctx.newPage();
async function render(html, file, size, scale = 1) {
  if (scale !== 1 || size.width !== 1320 || size.height !== 2868) {
    const c = await browser.newContext({ viewport: size, deviceScaleFactor: scale });
    const q = await c.newPage();
    await q.setContent(html, { waitUntil: "networkidle" });
    await q.waitForSelector("body[data-ready]");
    await q.screenshot({ path: file });
    await c.close();
    return;
  }
  await p.setViewportSize(size);
  await p.setContent(html, { waitUntil: "networkidle" });
  await p.waitForSelector("body[data-ready]");
  await p.screenshot({ path: file });
}

const made = [];
for (const scheme of ["light", "dark"]) {
  const t = T[scheme];
  mkdirSync(`${dst}/screenshots-${scheme}`, { recursive: true });
  mkdirSync(`${dst}/plain-${scheme}`, { recursive: true });
  let n = 0;
  for (const [name, title, sub] of SLIDES) {
    const img = png(`${scheme}-${name}`);
    if (!img) continue;
    n += 1;
    const id = `${String(n).padStart(2, "0")}-${name}`;
    const captioned = page(
      `<div class="slide"><h1>${title}</h1><p>${sub}</p><div class="dev">${phone(img, scheme, 920)}</div></div>`,
      `body{background:${t.bg};width:1320px;height:2868px;overflow:hidden}
       .slide{padding:170px 90px 0;text-align:center}
       h1{font-size:98px;line-height:1.08;font-weight:800;letter-spacing:-0.025em;color:${t.ink};text-wrap:balance}
       p{margin-top:34px;font-size:46px;line-height:1.3;font-weight:500;color:${t.text2};text-wrap:balance}
       .dev{position:absolute;left:50%;top:700px;transform:translateX(-50%)}`,
    );
    await render(captioned, `${dst}/screenshots-${scheme}/${id}.png`, { width: 1320, height: 2868 });
    // The plain screen: exactly the iPhone's 1320 x 2868, no frame.
    await render(
      page(screen(img, scheme, 1320), `body{width:1320px;height:2868px;overflow:hidden} .screen{border-radius:0!important}`),
      `${dst}/plain-${scheme}/${id}.png`,
      { width: 1320, height: 2868 },
    );
    made.push(id);
  }
}

// The header: what OpenBark is, beside two screens.
{
  const a = png("light-record");
  const b = png("light-note") ?? png("light-visits");
  const html = page(
    `<div class="hero">
       <div class="copy">
         <div class="icon">${icon}</div>
         <h1>Record the visit.<br>Get the note.</h1>
         <p>The free, open-source scribe for veterinary teams.</p>
         <ul class="local">
           <li><i></i>Local models on your own devices</li>
           <li><i></i>Never the cloud: recordings stay on your iPhone</li>
         </ul>
       </div>
       <div class="devs">${a ? phone(a, "light", 400) : ""}${b ? phone(b, "light", 400) : ""}</div>
     </div>`,
    `body{background:#F2F4F7;width:1920px;height:1080px;overflow:hidden}
     .hero{display:grid;grid-template-columns:1fr 920px;height:1080px;padding-left:130px;align-items:center}
     .icon svg{width:132px;height:132px;display:block}
     h1{margin-top:44px;font-size:92px;line-height:1.04;font-weight:800;letter-spacing:-0.03em;color:#182230}
     p{margin-top:30px;max-width:620px;font-size:34px;line-height:1.38;font-weight:500;color:#4B5563}
     .local{list-style:none;padding:0;margin-top:40px;display:flex;flex-direction:column;gap:16px}
     .local li{display:flex;align-items:center;gap:16px;font-size:30px;font-weight:600;color:#182230}
     .local i{width:14px;height:14px;border-radius:999px;background:#1F8A5B;flex:none}
     .devs{display:flex;gap:44px;align-self:start;padding-top:140px}
     .devs .phone:last-child{margin-top:90px}`,
  );
  await render(html, `${dst}/header.png`, { width: 1920, height: 1080 });
  await render(html, `${dst}/header@2x.png`, { width: 1920, height: 1080 }, 2);
}

// How the listing reads in App Store search: icon, name, subtitle and the
// first three screenshots.
{
  const firsts = made.slice(0, 3).map((id) => `data:image/png;base64,${readFileSync(`${dst}/screenshots-light/${id}.png`).toString("base64")}`);
  const html = page(
    `<div class="card">
       <div class="row"><div class="icon">${icon}</div>
         <div class="name"><b>OpenBark</b><span>AI scribe for veterinary teams</span></div>
         <div class="get">Get</div></div>
       <div class="shots">${firsts.map((s) => `<img src="${s}">`).join("")}</div>
     </div>`,
    `body{background:#fff;width:1290px;height:1500px;overflow:hidden}
     .card{padding:60px 48px}
     .row{display:flex;align-items:center;gap:36px}
     .icon svg{width:190px;height:190px;display:block}
     .name{flex:1;display:flex;flex-direction:column;gap:8px}
     .name b{font-size:52px;font-weight:600;color:#000}.name span{font-size:38px;color:#6B7280;font-weight:500}
     .get{background:#EEF0F4;color:#2F6FE4;font-weight:800;font-size:40px;padding:18px 54px;border-radius:999px}
     .shots{display:grid;grid-template-columns:repeat(3,1fr);gap:24px;margin-top:48px}
     .shots img{width:100%;border-radius:34px;border:2px solid #E5E8EE;display:block}`,
  );
  await render(html, `${dst}/search-preview.png`, { width: 1290, height: 1500 });
}

await browser.close();
console.log(`Composed ${made.length} screenshots: ${made.join(", ")}`);
