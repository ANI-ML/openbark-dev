// Recordings play to the end in each browser engine, web mode (the macOS
// replay bug of 0.1.1). WebKit is the strict one: it plays media over HTTP
// only with byte ranges, and is what Safari and the macOS app use.
//
// Run ONLY against the playback demo, never a clinic's own server or data:
//   OPENBARK_URL=http://localhost:4338 node e2e/audio-playback.mjs <demo folder> <visits.json>
//
// The demo is `openbark serve --port 4338` started with every folder in its
// own scratch folder (OPENBARK_SESSIONS_DIR=<demo folder>/visits, and
// OPENBARK_TEMPLATES_DIR, OPENBARK_USERS_FILE, OPENBARK_LOCATIONS_FILE and a
// throwaway OPENBARK_DATA_KEY), with an empty file called OPENBARK-DEMO in
// <demo folder>. <visits.json> is what `cargo run -p os-cli --example
// audio_visits -- <demo folder>/visits` printed. BROWSERS picks the engines
// (default: webkit,chromium,firefox; ones not installed are skipped unless
// REQUIRE_ALL=1).
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { isAbsolute, join, relative, resolve } from "node:path";
import { chromium, firefox, webkit } from "@playwright/test";

const base = process.env.OPENBARK_URL ?? "http://localhost:4338";
const out = process.env.OUT_DIR ?? "../../target/audio-playback";
const demo = process.argv[2] ? resolve(process.argv[2]) : null;
const visitsFile = process.argv[3] ? resolve(process.argv[3]) : null;
const OWN_DATA = /[\\/](AppData[\\/]Local|Library[\\/]Application Support|\.local[\\/]share)[\\/]OpenBark([\\/]|$)/i;

function refuse(why) {
  console.error(`REFUSING TO RUN: ${why}`);
  process.exit(2);
}
const url = new URL(base);
if (url.port === "4317") refuse("port 4317 is the clinic's own OpenBark server");
if (url.port !== "4338") refuse(`the playback demo runs on port 4338, not ${url.port || "the default port"}`);
if (!["localhost", "127.0.0.1"].includes(url.hostname)) refuse("only this computer's playback demo can be used");
if (!demo || !visitsFile) refuse("usage: node e2e/audio-playback.mjs <demo folder> <visits.json>");
if (OWN_DATA.test(demo)) refuse(`${demo} is inside OpenBark's own data folder`);
if (!existsSync(join(demo, "OPENBARK-DEMO"))) refuse(`${demo} has no OPENBARK-DEMO marker file`);
const storage = await fetch(`${base}/api/storage`).then((r) => r.json());
const visitsDir = resolve(storage.visits.path);
const rel = relative(demo, visitsDir);
if (rel === "" || rel.startsWith("..") || isAbsolute(rel)) refuse(`the server's visits folder (${visitsDir}) isn't inside ${demo}`);
const visits = JSON.parse(readFileSync(visitsFile, "utf8"));
if (!Array.isArray(visits) || visits.length === 0) refuse(`${visitsFile} lists no visits`);

mkdirSync(out, { recursive: true });
const problems = [];
const check = (ok, what) => ok || problems.push(what);

// ---- the server's answers to a player's range requests --------------------
for (const v of visits) {
  const path = `${base}/api/sessions/${encodeURIComponent(v.id)}/audio`;
  const probe = await fetch(path, { headers: { range: "bytes=0-1" } });
  const total = Number((probe.headers.get("content-range") ?? "").split("/")[1]);
  check(probe.status === 206, `${v.label}: bytes=0-1 answered ${probe.status}, not 206`);
  check(probe.headers.get("content-type") === "audio/wav", `${v.label}: content type ${probe.headers.get("content-type")}`);
  check(probe.headers.get("accept-ranges") === "bytes", `${v.label}: no Accept-Ranges`);
  check(probe.headers.get("cache-control") === "no-store", `${v.label}: decrypted audio may be cached`);
  check(total === 44 + Math.round(v.secs * 16000) * 2 * v.channels, `${v.label}: total ${total}`);
  const tail = await fetch(path, { headers: { range: `bytes=${total - 100}-` } });
  check(tail.status === 206 && (await tail.arrayBuffer()).byteLength === 100, `${v.label}: the last 100 bytes`);
  const past = await fetch(path, { headers: { range: `bytes=${total}-` } });
  check(past.status === 416, `${v.label}: a range past the end answered ${past.status}, not 416`);
}

// ---- each engine plays each recording, seeks near the end and plays out ----
async function play(page, src, secs) {
  return page.evaluate(
    ({ src, secs }) =>
      new Promise((resolve) => {
        const r = { events: [], expected_secs: secs };
        const a = new Audio();
        a.muted = true;
        a.preload = "auto";
        let finished = false;
        const finish = (extra) => {
          if (finished) return;
          finished = true;
          Object.assign(r, extra);
          r.current_time = a.currentTime;
          if (a.error) r.media_error = { code: a.error.code, message: a.error.message || "" };
          a.pause();
          resolve(r);
        };
        const timer = setTimeout(() => finish({ timeout: true }), 60000);
        for (const ev of ["loadedmetadata", "canplay", "playing", "seeked", "stalled", "waiting", "ended"])
          a.addEventListener(ev, () => r.events.push(`${ev}@${a.currentTime.toFixed(2)}`));
        a.addEventListener("error", () => (clearTimeout(timer), finish({ error: true })));
        a.addEventListener(
          "loadedmetadata",
          async () => {
            r.duration = a.duration;
            try {
              await a.play();
              await new Promise((ok) => setTimeout(ok, 1500));
              r.played_to = a.currentTime;
              a.currentTime = Math.max(0, a.duration - 2);
            } catch (e) {
              clearTimeout(timer);
              finish({ play_rejected: String(e?.message ?? e) });
            }
          },
          { once: true },
        );
        a.addEventListener("ended", () => (clearTimeout(timer), finish({ ended: true })), { once: true });
        a.src = src;
      }),
    { src, secs },
  );
}

const engines = { webkit, chromium, firefox };
const wanted = (process.env.BROWSERS ?? "webkit,chromium,firefox").split(",").map((s) => s.trim()).filter(Boolean);
const results = {};
for (const name of wanted) {
  const type = engines[name];
  if (!type) refuse(`unknown browser ${name}`);
  let browser;
  try {
    browser = await type.launch();
  } catch (e) {
    if (process.env.REQUIRE_ALL === "1") problems.push(`${name}: couldn't start (${String(e.message).split("\n")[0]})`);
    else console.log(`- ${name}: not installed here, skipped`);
    continue;
  }
  const page = await browser.newPage();
  await page.goto(base);
  results[name] = [];
  for (const v of visits) {
    const r = await play(page, `/api/sessions/${encodeURIComponent(v.id)}/audio`, v.secs);
    r.label = v.label;
    results[name].push(r);
    const ok = r.ended && !r.error && Math.abs(r.duration - v.secs) < 0.5;
    console.log(`- ${name}: ${v.label}: ${ok ? "played to the end" : "FAILED"} (duration ${r.duration}, events ${r.events.join(" ")})`);
    check(ok, `${name}: ${v.label} didn't play to the end: ${JSON.stringify(r)}`);
  }
  await browser.close();
}
writeFileSync(join(out, "results.json"), JSON.stringify(results, null, 2));

if (problems.length) {
  console.error(`\n${problems.length} problem(s):`);
  for (const p of problems) console.error(`  - ${p}`);
  process.exit(1);
}
console.log("\nAll recordings played to the end.");
