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
  for (const p of ["index.html", "features.html", "history.html", "404.html"]) {
    await page.goto(`${base}/${p}`);
    await page.waitForLoadState("networkidle");
    await page.waitForTimeout(1200);
    const overflow = await page.evaluate(() => document.documentElement.scrollWidth > window.innerWidth);
    if (overflow) problems.push(`${v.name} ${p}: horizontal scroll`);
    await page.screenshot({ path: `${out}/${v.name}-${p.replace(".html", "")}.png` });
    // Brand guide checks: the lockup links home, the favicon is the brand one,
    // primary buttons are ink (not accent), and nothing is glass or gradient.
    const brand = await page.evaluate(() => {
      const issues = [];
      const home = document.querySelector('.site-head a.brand[href="index.html"] svg.lockup');
      if (!home) issues.push("no lockup linking home");
      const icon = document.querySelector('link[rel="icon"]')?.getAttribute("href");
      if (icon !== "favicon.svg") issues.push(`favicon is ${icon}`);
      const primary = document.querySelector(".btn-primary");
      const ink = getComputedStyle(document.documentElement).getPropertyValue("--primary").trim().toLowerCase();
      if (primary && !["#182230", "#f5f7fa"].includes(ink)) issues.push(`primary is ${ink}`);
      for (const el of document.querySelectorAll("body, body *")) {
        const cs = getComputedStyle(el);
        if (cs.backgroundImage.includes("gradient")) issues.push(`gradient on ${el.className || el.tagName}`);
        if (cs.backdropFilter && cs.backdropFilter !== "none") issues.push(`glass on ${el.className || el.tagName}`);
      }
      const h1 = document.querySelector("h1");
      if (h1 && getComputedStyle(h1).fontWeight !== "800") issues.push(`h1 weight ${getComputedStyle(h1).fontWeight}`);
      // One accent: amber fills are for the recording dot only (the lockup's
      // capsule is an SVG fill, not a background).
      const amber = ["rgb(226, 122, 43)", "rgb(233, 139, 69)"];
      for (const el of document.querySelectorAll("body *")) {
        if (amber.includes(getComputedStyle(el).backgroundColor) && !el.classList.contains("rec-dot")) issues.push(`amber fill on ${el.className || el.tagName}`);
      }
      // OpenBark is in beta: nothing on the site may still say pre-alpha.
      if (/pre-?alpha/i.test(document.body.innerText)) issues.push("says pre-alpha");
      // Sharing: canonical and og:url on www.openbark.io, a large preview image.
      const meta = (sel) => document.querySelector(sel)?.getAttribute("content") ?? document.querySelector(sel)?.getAttribute("href");
      const page = location.pathname.split("/").pop() || "index.html";
      const want = { "index.html": "/", "features.html": "/features", "history.html": "/history" }[page];
      if (want) {
        if (meta('link[rel="canonical"]') !== `https://www.openbark.io${want}`) issues.push(`canonical is ${meta('link[rel="canonical"]')}`);
        if (meta('meta[property="og:url"]') !== `https://www.openbark.io${want}`) issues.push(`og:url is ${meta('meta[property="og:url"]')}`);
      }
      if (meta('meta[property="og:image"]') !== "https://www.openbark.io/og-image.png") issues.push(`og:image is ${meta('meta[property="og:image"]')}`);
      if (meta('meta[name="twitter:card"]') !== "summary_large_image") issues.push("no large twitter card");
      if (document.querySelector(".stage")?.textContent !== "Beta") issues.push("header badge is not Beta");
      // "Open source" only once the code can be seen: a page that says it must link the repository.
      const source = document.querySelector('a[href^="https://github.com/ANI-ML/openbark"]:not([href*="releases"])');
      if (!source && /open.source AI scribe/i.test(document.title + " " + (document.querySelector("h1")?.textContent ?? ""))) issues.push("says open source but links no source code");
      // Evidence is behind "How we know", closed until asked for.
      if (document.querySelector(".evidence:not(details .evidence)")) issues.push("evidence shown outside How we know");
      if (document.querySelector("details.how[open]")) issues.push("How we know open by default");
      return issues;
    });
    for (const b of brand) problems.push(`${v.name} ${p}: ${b}`);
    await page.screenshot({ path: `${out}/${v.name}-${p.replace(".html", "")}-full.png`, fullPage: true });
  }
  // The landing page: the beta hero, the four short sections, then downloads,
  // the maker and contact, in that order.
  await page.goto(`${base}/index.html`);
  const landing = await page.evaluate(() => {
    const issues = [];
    const h1 = document.querySelector(".hero h1");
    if (!["The free, open-source AI scribe for veterinary medicine.", "The free AI scribe for veterinary medicine."].includes(h1?.textContent)) issues.push(`hero h1 is "${h1?.textContent}"`);
    const cs = h1 && getComputedStyle(h1);
    if (cs && (cs.fontSize !== (innerWidth > 600 ? "48px" : "36px"))) issues.push(`hero h1 size ${cs.fontSize}`);
    const dl = document.querySelector("#hero-download");
    // With installers published it downloads; until then it asks to join the beta (never a dead download).
    const published = !document.querySelector('#downloads [aria-disabled="true"]:not([data-files="ios"] *)');
    if (!dl?.classList.contains("btn-primary")) issues.push("hero button is not primary");
    if (published ? !/^Download the beta/.test(dl.textContent.trim()) : dl.textContent.trim() !== "Ask to join the beta" || dl.getAttribute("href") !== "#contact")
      issues.push(`hero button is "${dl?.textContent.trim()}" -> ${dl?.getAttribute("href")}`);
    if (!/Beta/.test(document.querySelector("#hero-meta")?.textContent ?? "")) issues.push("no beta note in the hero");
    if (!/validated/.test(document.querySelector("#hero-meta")?.textContent ?? "")) issues.push("hero beta note lost its caveat");
    const order = ["free", "private", "planet", "careful", "download", "who", "contact"].map((id) => document.getElementById(id));
    order.forEach((el, i) => { if (!el) issues.push(`missing section ${i}`); });
    for (let i = 1; i < order.length; i++) {
      if (order[i - 1] && order[i] && !(order[i - 1].compareDocumentPosition(order[i]) & Node.DOCUMENT_POSITION_FOLLOWING)) issues.push(`section ${order[i].id} out of order`);
    }
    const who = [...document.querySelectorAll("#tx .who")].map((w) => w.textContent);
    if (who.some((w) => !["Vet", "Client"].includes(w))) issues.push(`speaker labels ${who.join(",")}`);
    // Labels never wrap or collide with the words.
    for (const w of document.querySelectorAll("#tx .who")) if (w.scrollWidth > w.clientWidth + 1) issues.push(`speaker label "${w.textContent}" overflows`);
    const text = document.body.innerText;
    for (const banned of [/zero (environmental )?impact/i, /the first open.source/i, /nothing (will )?ever leaves/i]) if (banned.test(text)) issues.push(`says ${banned}`);
    if (document.querySelector("#stats, .stat")) issues.push("evaluation stats are back on the landing page");
    return issues;
  });
  for (const b of landing) problems.push(`${v.name} landing: ${b}`);
  // The demo: hovering a note line lights its transcript line.
  await page.locator('.note button[data-ev="4"]').hover();
  const lit = await page.locator('#tx li.lit').getAttribute("data-id");
  if (lit !== "4") problems.push(`${v.name}: demo lit ${lit}, expected 4`);
  // Contact and coffee links, and the Version history tab.
  const mail = await page.locator('#contact a[href^="mailto:"]').getAttribute("href");
  if (!mail?.startsWith("mailto:we.care@animl.health")) problems.push(`${v.name}: contact link is ${mail}`);
  const coffee = await page.locator('#contact a[href*="buymeacoffee.com"]').getAttribute("href");
  if (coffee !== "https://buymeacoffee.com/animlhealth") problems.push(`${v.name}: coffee link is ${coffee}`);
  if (!v.isMobile) {
    await page.locator('.site-nav a[href="history.html"]').click();
    await page.waitForURL(/history/);
    const entries = await page.locator(".hx-entry").count();
    if (entries < 3) problems.push(`${v.name}: history shows ${entries} entries`);
    // Major changes only: no entry lists more than 8 items, and no commit hashes show.
    const crowded = await page.evaluate(() => [...document.querySelectorAll(".hx-entry")].filter((el) => el.querySelectorAll(".hx-items li").length > 8).length);
    if (crowded) problems.push(`${v.name}: ${crowded} history entries list more than 8 items`);
    const hashes = await page.evaluate(() => (document.querySelector("#hx")?.innerText.match(/[0-9a-f]{7}/g) || []).length);
    if (hashes) problems.push(`${v.name}: history shows ${hashes} commit hashes`);
  }
  // Every in-site anchor lands on an element that exists (the footer Privacy link once pointed nowhere).
  for (const p of ["index.html", "features.html", "history.html"]) {
    await page.goto(`${base}/${p}`);
    const links = await page.evaluate(() => [...new Set([...document.querySelectorAll('a[href*="#"]')].map((a) => a.getAttribute("href")))]);
    for (const href of links) {
      if (/^(https?:|mailto:)/.test(href)) continue;
      const [file, id] = href.split("#");
      if (!id) continue;
      if (file && file !== p) await page.goto(`${base}/${file}`);
      if (!(await page.evaluate((x) => !!document.getElementById(x), id))) problems.push(`${v.name} ${p}: link ${href} has no target`);
      if (file && file !== p) await page.goto(`${base}/${p}`);
    }
  }
  // The features filter.
  await page.goto(`${base}/features.html`);
  await page.locator("#fx-q").fill("dental");
  const shown = await page.locator(".fx-item:not([hidden])").count();
  if (shown < 1) problems.push(`${v.name}: filter "dental" shows nothing`);
  await ctx.close();
}
await browser.close();
console.log(problems.length ? `PROBLEMS:\n${problems.join("\n")}` : "all checks passed");
