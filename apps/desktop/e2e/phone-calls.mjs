// End-to-end test of outgoing calls through the clinic's own Twilio account
// (ADR-0023), in web mode, against a MOCK Twilio only. Never a real account,
// never a real call.
//
//   FAKE_TWILIO=<path to fake-twilio.exe> OPENBARK_URL=http://localhost:4330 \
//     node e2e/phone-calls.mjs <demo folder>
//
// - The REST calls (docs/network.md R9) go to the fake Twilio account from
//   engine/os-connect (`cargo build --release -p os-connect --features mock
//   --example fake-twilio`), which this script starts on 127.0.0.1:4339. The
//   demo server must run with OPENBARK_TWILIO_API_BASE=http://127.0.0.1:4339.
// - Twilio's Voice SDK is replaced in the phone page by a fake (below) that
//   "answers" and plays two fixture recordings: the clinic's side and the
//   client's side. CALL_WAVS="clinic.wav;client.wav" names them.
//
// The demo is `openbark serve --port 4330` with every folder in scratch
// (OPENBARK_SESSIONS_DIR, OPENBARK_TEMPLATES_DIR, OPENBARK_USERS_FILE,
// OPENBARK_LOCATIONS_FILE, OPENBARK_KEY_FILE, OPENBARK_CONNECTIONS_DIR, a
// throwaway OPENBARK_DATA_KEY) and an empty OPENBARK-DEMO file in its folder.
// The script refuses to run unless:
//   1. the URL is this computer on port 4330 (never 4317, the clinic's own server);
//   2. <demo folder> holds an OPENBARK-DEMO marker and isn't in OpenBark's own data folder;
//   3. GET /api/storage reports <demo folder>/visits as the visits folder;
//   4. Twilio isn't connected yet on the demo.
import { spawn } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, readdirSync } from "node:fs";
import { isAbsolute, join, relative, resolve } from "node:path";
import { chromium } from "@playwright/test";

const base = process.env.OPENBARK_URL ?? "http://localhost:4330";
const out = process.env.OUT_DIR ?? "../../target/phone-calls";
const demo = process.argv[2] ? resolve(process.argv[2]) : null;
const fakeTwilio = process.env.FAKE_TWILIO;
const [clinicWav, clientWav] = (process.env.CALL_WAVS ?? "").split(";");
const OWN_DATA = /[\\/]AppData[\\/]Local[\\/]OpenBark([\\/]|$)/i;
const FAKE_PORT = 4339;

function refuse(why) {
  console.error(`REFUSING TO RUN: ${why}`);
  process.exit(2);
}
const url = new URL(base);
if (url.port === "4317") refuse("port 4317 is the clinic's own OpenBark server");
if (url.port !== "4330") refuse(`the phone demo runs on port 4330, not ${url.port || "the default port"}`);
if (!["localhost", "127.0.0.1"].includes(url.hostname)) refuse("only this computer's demo can be used");
if (!demo) refuse("usage: node e2e/phone-calls.mjs <demo folder>");
if (OWN_DATA.test(demo)) refuse(`${demo} is inside OpenBark's own data folder`);
if (!existsSync(join(demo, "OPENBARK-DEMO"))) refuse(`${demo} has no OPENBARK-DEMO marker file`);
if (!fakeTwilio || !existsSync(fakeTwilio)) refuse("FAKE_TWILIO must name the fake-twilio example binary");
if (!clinicWav || !clientWav || !existsSync(clinicWav) || !existsSync(clientWav))
  refuse('CALL_WAVS must name two WAV files: "clinic.wav;client.wav"');
const storage = await fetch(`${base}/api/storage`).then((r) => r.json());
const visitsDir = resolve(storage.visits.path);
if (relative(join(demo, "visits"), visitsDir) !== "") refuse(`the server's visits folder (${visitsDir}) isn't ${join(demo, "visits")}`);
const phoneNow = await fetch(`${base}/api/phone`).then((r) => r.json());
if (phoneNow.connected) refuse("Twilio is already connected on this demo; start a fresh one");

mkdirSync(out, { recursive: true });
const problems = [];
const check = (ok, what) => {
  if (!ok) problems.push(what);
  console.log(`  ${ok ? "ok" : "FAIL"}: ${what}`);
};
const step = (what) => console.log(`- ${what}`);
const inside = (child, parent) => {
  const r = relative(parent, child);
  return r !== "" && !r.startsWith("..") && !isAbsolute(r);
};

// Made-up IDs, built from pieces so no credential-shaped string sits in the source.
const sid = (prefix, n) => `${prefix}${(0xfeed).toString(16).padStart(16, "0")}${n.toString(16).padStart(16, "0")}`;
const ACCOUNT = sid("AC", 1);
const KEY = sid("SK", 2);
const SECRET = ["e2e", "fake", "secret", "only", "0042"].join("-");
const CALLER = "+15555550100";
const BIN = `https://handler.twilio.com/twiml/${sid("EH", 3)}`;

step(`starting the fake Twilio account on 127.0.0.1:${FAKE_PORT}`);
const fake = spawn(fakeTwilio, [String(FAKE_PORT), ACCOUNT, KEY, SECRET, CALLER], { stdio: ["ignore", "pipe", "inherit"] });
await new Promise((ok, fail) => {
  fake.stdout.once("data", ok);
  fake.once("exit", (code) => fail(new Error(`fake Twilio exited (${code})`)));
});

async function api(method, path, body) {
  const res = await fetch(`${base}${path}`, {
    method,
    headers: { "x-openbark": "1", ...(body ? { "content-type": "application/json" } : {}) },
    body: body ? JSON.stringify(body) : undefined,
  });
  return { status: res.status, body: await res.json().catch(() => null) };
}
const visits = () => readdirSync(visitsDir).filter((d) => existsSync(join(visitsDir, d, "session.json")));

// ---- the fake Voice SDK, injected into the phone page only ---------------------

// Defined on every page: a popup's first document is about:blank, so a path
// check would miss phone.html. The main window never looks at it.
const FAKE_VOICE = `
{
  class FakeCall {
    constructor(device) {
      this.device = device; this.h = {}; this.done = false;
      this.parameters = { CallSid: "CA" + "feed".padStart(16, "0") + "7".padStart(16, "0") };
      setTimeout(() => this.emit("ringing"), 300);
      setTimeout(() => this.answer(), 1200);
    }
    on(e, cb) { (this.h[e] ??= []).push(cb); }
    emit(e, ...a) { (this.h[e] || []).forEach((f) => f(...a)); }
    async answer() {
      if (this.done) return;
      const ctx = new AudioContext();
      const load = async (u) => ctx.decodeAudioData(await (await fetch(u)).arrayBuffer());
      const [c, k] = await Promise.all([load("/e2e-fixture/clinic.wav"), load("/e2e-fixture/client.wav")]);
      this.emit("accept");
      // Give the recording a moment to start, as a real answer would.
      await new Promise((r) => setTimeout(r, 1500));
      const play = (buf, delay) => {
        const s = ctx.createBufferSource(); s.buffer = buf;
        const d = ctx.createMediaStreamDestination(); s.connect(d); s.start(ctx.currentTime + delay);
        return d.stream;
      };
      for (const { p, remote } of this.device.processors) {
        await p.createProcessedStream(remote ? play(k, c.duration + 0.6) : play(c, 0.2));
      }
      window.__fakeAudioSecs = c.duration + k.duration + 1;
    }
    disconnect() { if (this.done) return; this.done = true; this.emit("disconnect"); }
    mute(on) { window.__fakeMuted = on; }
    sendDigits(d) { (window.__fakeDigits ??= []).push(d); }
  }
  window.__openbarkFakeVoice = {
    Device: class {
      constructor(token, options) {
        this.processors = []; this.h = {};
        window.__fakeDevice = { token, options };
        this.audio = { addProcessor: async (p, remote) => { this.processors.push({ p, remote: Boolean(remote) }); } };
      }
      on(e, cb) { (this.h[e] ??= []).push(cb); }
      updateToken() {}
      destroy() {}
      async connect({ params }) {
        const fail = localStorage.getItem("e2e-fake-fail");
        if (fail) { localStorage.removeItem("e2e-fake-fail"); const e = new Error("fake failure"); e.code = Number(fail); throw e; }
        window.__fakeTo = params.To;
        return new FakeCall(this);
      }
    },
  };
}`;

const browser = await chromium.launch({ args: ["--autoplay-policy=no-user-gesture-required"] });
const context = await browser.newContext({ viewport: { width: 1280, height: 900 } });
await context.addInitScript(() => {
  if (!localStorage.getItem("openbark.settings")) {
    localStorage.setItem(
      "openbark.settings",
      JSON.stringify({ consentAcknowledgedAt: "2026-10-19T10:00:00Z", signer: "Dr Lee", uiLanguage: "en", draftOnStop: true }),
    );
  }
});
await context.addInitScript(FAKE_VOICE);
// A safety net: nothing in this test may reach Twilio, so any Twilio host is refused.
const twilioTried = [];
await context.route(/twilio\.com/, (route) => {
  twilioTried.push(route.request().url());
  return route.abort();
});
await context.routeWebSocket(/twilio\.com/, (ws) => {
  twilioTried.push(ws.url());
  ws.close();
});
await context.route("**/e2e-fixture/*.wav", (route) =>
  route.fulfill({ contentType: "audio/wav", body: readFileSync(route.request().url().endsWith("clinic.wav") ? clinicWav : clientWav) }),
);
const hosts = new Set();
context.on("request", (r) => hosts.add(new URL(r.url()).host));

const main = await context.newPage();
const errors = [];
main.on("pageerror", (e) => errors.push(`main: ${e.message}`));
try {
  // ---- 1. Settings: connect the clinic's Twilio account ------------------------
  step("Settings > Phone calls: connect Twilio");
  await main.goto(`${base}/`);
  await main.getByRole("button", { name: "Settings" }).first().click();
  const section = main.locator("section.phone-settings");
  await section.getByRole("button", { name: "Connect Twilio" }).click();
  const sheet = main.getByRole("dialog", { name: "Connect Twilio" });
  const fill = (label, value) => sheet.getByLabel(new RegExp(`^${label}`)).fill(value);
  await fill("Account SID", ACCOUNT);
  await fill("API key SID", KEY);
  await fill("API key secret", "not-the-right-secret-000000");
  await fill("This computer's name", "Front desk");
  await fill("Caller ID", "+1 555 555 0100");
  await sheet.getByRole("button", { name: "Show the Twilio text" }).click();
  const binText = await sheet.locator("pre.phone-bin-text").textContent();
  check(binText.includes('callerId="+15555550100"') && binText.includes("{{#e164}}{{To}}{{/e164}}"), "the outgoing Bin's text, ready to paste");
  check(!binText.toLowerCase().includes("record"), "the Bin asks Twilio to record nothing");
  await fill("Outgoing Bin URL", BIN);
  await main.screenshot({ path: join(out, "connect-sheet.png") });
  await sheet.getByRole("button", { name: "Connect" }).click();
  const refusal = await sheet.getByRole("alert").textContent();
  check(refusal.includes("Twilio didn't accept this key"), `a wrong secret is refused in plain words: "${refusal}"`);
  check(!existsSync(join(demo, "connections", "connections.bin")), "nothing saved after a refusal");
  await fill("API key secret", SECRET);
  await sheet.getByRole("button", { name: "Connect" }).click();
  await section.getByText("Twilio · connected").waitFor();
  await sheet.waitFor({ state: "detached" });
  check(true, "connected to the fake Twilio account");
  check(existsSync(join(demo, "connections", "connections.bin")), "the secret is sealed in the scratch token store (no keychain)");
  const sealed = readFileSync(join(demo, "connections", "connections.bin"));
  check(sealed.subarray(0, 4).toString() === "OBK1" && !sealed.includes(Buffer.from(SECRET)), "sealed, never in plain text");
  check(!(await main.content()).includes(SECRET), "the secret is never shown again");
  // Saved by the engine first, then shown checked.
  await section.getByLabel("Turn on the phone on this computer").click();
  await main.getByText("Phone settings saved").waitFor();
  check(await section.getByLabel("Turn on the phone on this computer").isChecked(), "the phone is on");
  await section.getByRole("button", { name: "Check setup" }).click();
  await section.getByText("Twilio accepts this computer's key.").waitFor();
  check(await section.getByText("Real calls need an upgraded Twilio account.", { exact: false }).isVisible(), "Check setup says real calls need an upgraded account");
  await section.screenshot({ path: join(out, "settings-connected-light.png") });

  // ---- 2. A patient with an owner's phone number --------------------------------
  step("a patient with the owner's phone number");
  const bella = (await api("POST", "/api/patients", { name: "Bella", last_name: "Patel", species: "canine" })).body;
  await main.goto(`${base}/`);
  await main.getByRole("button", { name: "Patients" }).first().click();
  await main.getByText("Bella", { exact: false }).first().click();
  await main.getByRole("button", { name: "Edit details" }).click();
  await main.getByLabel("Owner's phone").fill("(555) 555-0142");
  await main.getByRole("button", { name: "Save details" }).click();
  await main.getByText("+1 555 555 0142").waitFor();
  check(true, "the owner's phone number is saved and shown");
  check((await api("GET", `/api/patients/${bella.id}`)).body.patient.owner_phone === "+15555550142", "stored in E.164");

  // ---- 3. A recorded call: announcement, client agrees, hang up -----------------
  step("Call: a recorded call the client agrees to");
  const before = visits().length;
  const [phone] = await Promise.all([context.waitForEvent("page"), main.getByRole("button", { name: "Call" }).click()]);
  phone.on("pageerror", (e) => errors.push(`phone: ${e.message}`));
  await phone.getByText("Patel · Bella").waitFor();
  check(await phone.getByText("This call will be recorded and announced.").isVisible(), "the pre-call line");
  await phone.setViewportSize({ width: 360, height: 760 });
  await phone.screenshot({ path: join(out, "phone-precall-360-light.png"), fullPage: true });
  await phone.getByRole("button", { name: "Call", exact: true }).click();
  await phone.getByText("Read this to the client now:").waitFor({ timeout: 15000 });
  check((await phone.evaluate(() => window.__fakeTo)) === "+15555550142", "dialled the owner's number");
  const opts = await phone.evaluate(() => window.__fakeDevice.options);
  check(opts.codecPreferences[0] === "opus", "Opus first");
  check(Object.values(opts.sounds).every((u) => u.startsWith("/phone-sounds/")) && Object.keys(opts.sounds).length === 15, "every SDK sound is a local file");
  const token = await phone.evaluate(() => window.__fakeDevice.token);
  check(token.split(".").length === 3 && !token.includes(SECRET), "the phone page holds a token, never the secret");
  await phone.getByText(/^Recording · /).waitFor();
  await phone.screenshot({ path: join(out, "phone-announcement-360-light.png"), fullPage: true });
  await phone.getByRole("button", { name: "I've read it" }).click();
  await phone.getByRole("button", { name: "Yes, keep recording" }).click();
  await phone.getByRole("button", { name: "Mute" }).click();
  check(await phone.getByRole("button", { name: "Unmute" }).isVisible(), "Mute");
  await phone.getByRole("button", { name: "Unmute" }).click();
  await phone.getByRole("button", { name: "Keypad" }).click();
  await phone.locator(".phone-controls .phone-pad").getByRole("button", { name: "1" }).click();
  check(JSON.stringify(await phone.evaluate(() => window.__fakeDigits)) === '["1"]', "keypad digits go to the call");
  await phone.getByRole("button", { name: "Keypad" }).click();
  await phone.waitForFunction(() => window.__fakeAudioSecs > 0, null, { timeout: 30000 });
  const secs = await phone.evaluate(() => window.__fakeAudioSecs);
  await phone.waitForTimeout((secs + 2) * 1000);
  await phone.emulateMedia({ colorScheme: "dark" });
  await phone.waitForTimeout(800); // let the theme's colour transition finish
  await phone.screenshot({ path: join(out, "phone-in-call-360-dark.png"), fullPage: true });
  await phone.emulateMedia({ colorScheme: "light" });
  await phone.getByRole("button", { name: "Hang up" }).click();
  await phone.getByText(/^Call ended · .*Writing note…$/).waitFor({ timeout: 120000 });
  check(true, "the phone window says the note is being written");
  check(visits().length === before + 1, "one new visit");

  // The main window opens the visit and drafts it with the Phone call template.
  await main.locator(".visit-facts").getByText("Phone call", { exact: true }).waitFor({ timeout: 30000 });
  const facts = await main.locator(".visit-facts").textContent();
  check(/Outgoing · 0:\d\d · \+1 555 555 0142/.test(facts), `the call's line: ${facts.match(/Outgoing[^A-Z]*/)?.[0]}`);
  check(facts.includes("Announcement read by Dr Lee at 0:00. Client agreed."), "the consent line with the client's answer");
  const speakers = await main.locator(".turn-speaker").allTextContents();
  check(speakers.includes("Clinic") && speakers.includes("Client"), `speakers by channel: ${[...new Set(speakers)].join(", ")}`);
  check((await main.locator("select.template-pick").inputValue().catch(() => "callback")) === "callback", "Phone call template chosen");
  step("drafting with the Phone call template (the local model; this can take minutes)");
  await main.getByRole("heading", { name: "Reason for call" }).waitFor({ timeout: 900000 });
  check(true, "drafted with the Phone call template");
  await main.screenshot({ path: join(out, "review-call-light.png"), fullPage: true });
  const callVisit = visits().sort().at(-1);
  const meta = (await api("GET", `/api/sessions/${callVisit}`)).body;
  check(meta.call?.call_sid?.startsWith("CA") && meta.consent?.outcome === "agreed", "call SID and consent outcome saved with the visit");
  const wav = readFileSync(join(visitsDir, callVisit, "audio.wav"));
  check(wav.subarray(0, 4).toString() === "OBA1", "the recording is encrypted on disk");

  // ---- 4. The client declines: no audio kept, the call goes on ------------------
  step("Call: the client declines");
  await phone.getByRole("button", { name: "New call" }).click();
  await phone.getByRole("button", { name: "Call", exact: true }).click();
  await phone.getByRole("button", { name: "I've read it" }).click({ timeout: 15000 });
  await phone.getByRole("button", { name: "No, stop recording" }).click();
  await phone.getByText("Not recorded. The client declined.").waitFor();
  check(await phone.getByText(/^On call · .* · Not recorded$/).isVisible(), "the call goes on, unrecorded");
  await phone.waitForTimeout(3000);
  await phone.getByRole("button", { name: "Hang up" }).click();
  await phone.getByText(/^Call ended · .*Not recorded\.$/).waitFor({ timeout: 60000 });
  // The main window opens the declined visit once it's saved.
  await main.getByText("Client declined recording at", { exact: false }).waitFor({ timeout: 30000 });
  check(await main.getByText("Not recorded. The client declined.").isVisible(), "Review says the client declined");
  const declined = visits().sort().at(-1);
  check(declined !== callVisit, "the declined call is its own visit");
  check(!existsSync(join(visitsDir, declined, "audio.wav")) && !existsSync(join(visitsDir, declined, "transcript.jsonl")), "no audio or transcript on disk");
  const declinedMeta = (await api("GET", `/api/sessions/${declined}`)).body;
  check(declinedMeta.consent?.outcome === "declined" && declinedMeta.call?.declined_at_secs != null, "the visit records that the client declined, and when");

  // ---- 5. Don't record this call ----------------------------------------------
  step("Call: not recorded");
  const count = visits().length;
  await phone.getByRole("button", { name: "New call" }).click();
  await phone.getByLabel("Don't record this call").check();
  await phone.getByRole("button", { name: "Call", exact: true }).click();
  await phone.getByText(/^On call · /).waitFor({ timeout: 15000 });
  check(!(await phone.getByText("Read this to the client now:").isVisible()), "no announcement when not recording");
  await phone.getByRole("button", { name: "Hang up" }).click();
  await phone.getByText(/^Call ended · .*Not recorded\.$/).waitFor();
  check(visits().length === count, "nothing saved for an unrecorded call");

  // ---- 6. Errors in plain words -------------------------------------------------
  step("errors");
  await phone.getByRole("button", { name: "New call" }).click();
  await phone.evaluate(() => localStorage.setItem("e2e-fake-fail", "31005"));
  await phone.getByRole("button", { name: "Call", exact: true }).click();
  await phone.getByText("Can't reach Twilio. Check this computer's internet connection.").waitFor();
  check(true, "a signalling failure says to check the internet connection");
  await phone.getByRole("button", { name: "New call" }).click();
  await phone.waitForFunction(() => document.querySelector(".phone-dial input")?.value);
  await phone.getByLabel("Number").fill("555");
  await phone.waitForTimeout(500);
  check((await phone.getByLabel("Number").inputValue()) === "555", "what was typed stays on the dial pad");
  await phone.getByRole("button", { name: "Call", exact: true }).click();
  check((await phone.getByRole("alert").textContent()).includes("isn't a full phone number"), "a short number is refused");
  await phone.emulateMedia({ colorScheme: "dark" });
  await phone.waitForTimeout(800); // let the theme's colour transition finish
  await phone.screenshot({ path: join(out, "phone-error-360-dark.png"), fullPage: true });

  // ---- 7. Policies and hosts ----------------------------------------------------
  step("policies and hosts");
  const csp = (await fetch(`${base}/phone.html`)).headers.get("content-security-policy");
  check(Boolean(csp?.includes("wss://voice-js.roaming.twilio.com")) && !csp.includes("sdk.twilio.com"), "the phone page's CSP allows Twilio's calling service, not its sound host");
  const mainCsp = await main.evaluate(() => document.querySelector('meta[http-equiv="Content-Security-Policy"]')?.getAttribute("content"));
  check(Boolean(mainCsp) && !/https?:\/\/(?!ipc\.localhost|asset\.localhost)|wss:/.test(mainCsp), "the main window's CSP allows no remote host");
  check(twilioTried.length === 0, `nothing tried to reach Twilio${twilioTried.length ? `: ${twilioTried.join(", ")}` : ""}`);
  const remote = [...hosts].filter((h) => !/^(localhost|127\.0\.0\.1)(:\d+)?$/.test(h));
  check(remote.length === 0, `no request left this computer${remote.length ? `: ${remote.join(", ")}` : ""}`);

  // ---- 8. Settings at phone width, dark; then disconnect -------------------------
  step("Settings at 360 px, dark, then Disconnect");
  await main.setViewportSize({ width: 360, height: 800 });
  await main.emulateMedia({ colorScheme: "dark" });
  await main.goto(`${base}/`);
  await main.getByRole("button", { name: "Settings" }).first().click();
  const s2 = main.locator("section.phone-settings");
  await s2.scrollIntoViewIfNeeded();
  await s2.screenshot({ path: join(out, "settings-connected-360-dark.png") });
  const scrollW = await main.evaluate(() => document.documentElement.scrollWidth);
  check(scrollW <= 360, `no sideways scroll at 360 px (${scrollW})`);
  await s2.getByRole("button", { name: "Disconnect" }).click();
  await s2.getByRole("alertdialog").getByRole("button", { name: "Disconnect" }).click();
  await main.getByText("Also delete this key in your Twilio console", { exact: false }).waitFor();
  check(!existsSync(join(demo, "connections", "connections.bin")), "Disconnect removes the sealed secret");
  check(!(await api("GET", "/api/phone")).body.connected, "disconnected");
} catch (e) {
  problems.push(`stopped: ${e.message.split("\n")[0]}`);
  await main.screenshot({ path: join(out, "failure-main.png"), fullPage: true }).catch(() => {});
  for (const p of context.pages()) if (p !== main) await p.screenshot({ path: join(out, "failure-phone.png"), fullPage: true }).catch(() => {});
} finally {
  await browser.close();
  fake.kill();
}
if (errors.length) problems.push(...errors.map((e) => `page error: ${e}`));
check(inside(visitsDir, demo), "everything stayed in the demo folder");
console.log(problems.length ? `\n${problems.length} problem(s):\n${problems.map((p) => `  - ${p}`).join("\n")}` : "\nall phone checks passed");
process.exit(problems.length ? 1 : 0);
