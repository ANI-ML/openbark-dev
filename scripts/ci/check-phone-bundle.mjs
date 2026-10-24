// The main window stays offline (ADR-0023 section 4, docs/network.md).
//
//   node scripts/ci/check-phone-bundle.mjs apps/desktop/dist
//
// Checks the built UI:
// 1. Every script the main window can load (index.html's scripts and
//    preloads, and everything they import, statically or lazily) has no code
//    from Twilio's Voice SDK. Only the phone page (phone.html) may.
// 2. The main window's <meta> Content-Security-Policy allows no remote host:
//    only this computer ('self', Tauri's ipc and asset schemes, the app's
//    obaudio scheme for recordings, loopback WebSockets for web mode, data:
//    and blob:). blob: is allowed for media only (a recording's fallback
//    copy), never for scripts, workers, frames or connections.
// 3. The phone page does load the SDK, and the SDK's sounds come from local
//    files, never sdk.twilio.com.
// 4. The end-to-end test's stand-in for the SDK isn't in the build (it is
//    only in builds made with `vite build --mode e2e`). Skip with --e2e.
import { readFileSync, existsSync } from "node:fs";
import { join, dirname, posix } from "node:path";
import { readdirSync } from "node:fs";

const dist = process.argv.slice(2).find((a) => !a.startsWith("--")) ?? "apps/desktop/dist";
const e2eBuild = process.argv.includes("--e2e");
const problems = [];
// Markers that only Twilio's Voice SDK contains (its signalling and insights hosts).
const SDK_MARKERS = ["voice-js.", "eventgw.", "chunderw"];
const LOCAL_SOURCES = new Set([
  "'self'", "'none'", "'unsafe-inline'", "data:", "blob:", "ipc:", "asset:",
  "http://ipc.localhost", "http://asset.localhost", "ws://127.0.0.1:*", "ws://localhost:*",
  "obaudio:", "http://obaudio.localhost",
]);
// Sources allowed in one directive only.
const ONLY_IN = { "blob:": "media-src", "obaudio:": "media-src", "http://obaudio.localhost": "media-src" };

function read(rel) {
  const path = join(dist, rel);
  if (!existsSync(path)) throw new Error(`missing ${path}: build the UI first (pnpm --filter @openbark/desktop build)`);
  return readFileSync(path, "utf8");
}

/** A page's scripts and everything they import, as dist-relative paths. */
function graph(html) {
  const start = [...html.matchAll(/<(?:script|link)[^>]+(?:src|href)="\/([^"]+\.js)"/g)].map((m) => m[1]);
  const seen = new Set();
  const queue = [...start];
  while (queue.length) {
    const file = queue.shift();
    if (seen.has(file)) continue;
    seen.add(file);
    const code = read(file);
    for (const m of code.matchAll(/(?:from\s*|import\s*\(?\s*)["'`](\.{1,2}\/[^"'`]+\.js)["'`]/g)) {
      queue.push(posix.normalize(posix.join(dirname(file).replace(/\\/g, "/"), m[1])));
    }
  }
  return seen;
}

const index = read("index.html");
const main = graph(index);
for (const file of main) {
  const code = read(file);
  for (const marker of SDK_MARKERS) {
    if (code.includes(marker)) problems.push(`${file} (loaded by the main window) contains Twilio SDK code ("${marker}")`);
  }
}

const meta = index.match(/<meta http-equiv="Content-Security-Policy" content="([^"]+)"/);
if (!meta) {
  problems.push("index.html has no <meta> Content-Security-Policy");
} else {
  for (const directive of meta[1].split(";").map((d) => d.trim()).filter(Boolean)) {
    const [name, ...sources] = directive.split(/\s+/);
    for (const source of sources) {
      if (!LOCAL_SOURCES.has(source)) problems.push(`index.html CSP ${name} allows a remote source: ${source}`);
      else if (ONLY_IN[source] && ONLY_IN[source] !== name)
        problems.push(`index.html CSP ${name} allows ${source}, which only ${ONLY_IN[source]} may`);
    }
  }
}

const phone = graph(read("phone.html"));
const phoneCode = [...phone].map(read).join("\n");
if (!SDK_MARKERS.some((m) => phoneCode.includes(m))) problems.push("phone.html doesn't load Twilio's Voice SDK");
if (!phoneCode.includes("/phone-sounds/outgoing.wav")) problems.push("the phone page doesn't give the SDK its local sounds");
for (const sound of ["outgoing", "disconnect", "incoming", "dtmf-0", "dtmf-star", "dtmf-hash"]) {
  if (!existsSync(join(dist, "phone-sounds", `${sound}.wav`))) problems.push(`missing phone-sounds/${sound}.wav`);
}

if (!e2eBuild) {
  for (const file of readdirSync(join(dist, "assets")).filter((f) => f.endsWith(".js"))) {
    if (read(join("assets", file)).includes("__openbarkFakeVoice")) {
      problems.push(`assets/${file} contains the end-to-end test's stand-in for the SDK; build without --mode e2e`);
    }
  }
}

if (problems.length) {
  for (const p of problems) console.error(`::error::${p}`);
  process.exit(1);
}
console.log(`phone bundle check: the main window's ${main.size} scripts have no Twilio code and its CSP allows no remote host`);
