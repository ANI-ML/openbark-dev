// End-to-end test of the model manager (ADR-0025): the MLX engine card, an
// MLX model from Hugging Face, split and gated Hub repos, adding a model from
// this computer (copied and used where it is), and removing every kind, with
// the guards (the note model, a model busy writing).
//
// Run ONLY against the models demo, never a clinic's own server or data:
//   OPENBARK_URL=http://localhost:4327 IMPORT_DIR=<folder on the server holding
//   Tiny-Q4_K_M.gguf and Tiny-InPlace.gguf> node e2e/models.mjs
//
// The models demo is `openbark serve --port 4327` (or 4326) started with
// OPENBARK_PLATFORM=macos and OPENBARK_MLX_DEV=1 (the Mac screens, MLX on the
// CPU in test mode), and every folder in a scratch folder called
// openbark-models-demo: OPENBARK_MODELS_DIR, OPENBARK_ENGINES_DIR,
// OPENBARK_SESSIONS_DIR, OPENBARK_TEMPLATES_DIR, OPENBARK_USERS_FILE,
// OPENBARK_LOCATIONS_FILE, OPENBARK_KEY_FILE, OPENBARK_HF_TOKEN_FILE (so the
// demo never reads or changes this computer's Hugging Face token) and a
// throwaway OPENBARK_DATA_KEY. The binary must honour test mode (a debug
// build, or one built with --features mlx-test-mode).
//
// Before doing anything the script refuses unless the URL is this computer
// on port 4327 or 4326 (never 4317), the server's models and visits folders
// are inside an openbark-models-demo folder (never OpenBark's own data
// folder), it shows the Mac screens with MLX in test mode, it has no team,
// and it sees no saved Hugging Face token (a fresh scratch token store).
// It only ever removes models it added in this run.
//
// It downloads, from Hugging Face and the pinned MLX pack hosts: the Linux
// MLX engine pack (about 150 MB), mlx-community/Qwen3-0.6B-4bit (351 MB) and
// bartowski/SmolLM2-135M-Instruct-GGUF Q2_K (88 MB, twice).
import { mkdirSync } from "node:fs";
import { chromium } from "@playwright/test";

const base = process.env.OPENBARK_URL ?? "http://localhost:4327";
const out = process.env.OUT_DIR ?? "../../target/models-e2e";
const importDir = process.env.IMPORT_DIR;
const MLX_REPO = "mlx-community/Qwen3-0.6B-4bit";
const MLX_ID = `hf:${MLX_REPO}/@mlx`;
const SPLIT_REPO = "unsloth/Qwen3-235B-A22B-Instruct-2507-GGUF";
const GATED_REPO = "google/gemma-3-1b-it-qat-q4_0-gguf";
const NOTHING_REPO = "meta-llama/Llama-3.2-1B-Instruct";
const GGUF_REPO = "bartowski/SmolLM2-135M-Instruct-GGUF";
const GGUF_FILE = "SmolLM2-135M-Instruct-Q2_K.gguf";
const GGUF_ID = `hf:${GGUF_REPO}/SmolLM2-135M-Instruct-Q2_K`;
const OWN_DATA = /([\\/]AppData[\\/]Local[\\/]OpenBark|[\\/]\.local[\\/]share[\\/]OpenBark|Application Support[\\/]OpenBark)([\\/]|$)/i;
const MINUTE = 60_000;

// ---- safety checks, before anything is touched -----------------------------

function refuse(why) {
  console.error(`REFUSING TO RUN: ${why}`);
  process.exit(2);
}
const url = new URL(base);
if (url.port === "4317") refuse("port 4317 is the clinic's own OpenBark server");
if (!["4326", "4327"].includes(url.port)) refuse(`the models demo runs on port 4327 or 4326, not ${url.port || "the default"}`);
if (!["localhost", "127.0.0.1"].includes(url.hostname)) refuse("only this computer's models demo can be used");
if (!importDir) refuse("set IMPORT_DIR to the demo's folder of tiny GGUF files (on the server)");
const storage = await fetch(`${base}/api/storage`).then((r) => r.json());
const demoFolder = (p) => /[\\/]openbark-models-demo([\\/]|$)/.test(p) && !OWN_DATA.test(p);
if (!demoFolder(storage.models.path)) refuse(`the server's models folder (${storage.models.path}) isn't in an openbark-models-demo folder`);
if (!demoFolder(storage.visits.path)) refuse(`the server's visits folder (${storage.visits.path}) isn't in an openbark-models-demo folder`);
if (!demoFolder(importDir)) refuse(`IMPORT_DIR (${importDir}) isn't in an openbark-models-demo folder`);
const status = await fetch(`${base}/api/status`).then((r) => r.json());
if (status.team) refuse("this demo has a team; start a fresh one");
if (status.platform !== "macos") refuse("start the demo with OPENBARK_PLATFORM=macos");
const mlx0 = await fetch(`${base}/api/mlx`).then((r) => r.json());
if (mlx0.support !== "test_mode") refuse("start the demo with OPENBARK_MLX_DEV=1 (MLX in test mode)");
const token0 = await fetch(`${base}/api/hf-token`).then((r) => r.json());
if (token0.saved) refuse("the demo sees a Hugging Face token; start it with OPENBARK_HF_TOKEN_FILE in its scratch folder");

mkdirSync(out, { recursive: true });
const problems = [];
const check = (ok, what) => {
  if (!ok) {
    problems.push(what);
    console.log(`  PROBLEM: ${what}`);
  }
  return ok;
};
const step = (what) => console.log(`- ${what}`);
const added = new Set();

async function api(method, path, body) {
  const headers = { "x-openbark": "1" };
  if (body !== undefined) headers["content-type"] = "application/json";
  const res = await fetch(`${base}${path}`, { method, headers, body: body === undefined ? undefined : JSON.stringify(body) });
  const json = await res.json().catch(() => ({}));
  return { status: res.status, json };
}
const rows = async () => (await api("GET", "/api/models")).json;
const rowOf = async (id) => (await rows()).find((r) => r.id === id);

// ---- the browser ------------------------------------------------------------

const browser = await chromium.launch();
const ctx = await browser.newContext({ viewport: { width: 1440, height: 1000 }, colorScheme: "light" });
await ctx.addInitScript(() => {
  const key = "openbark.settings";
  const current = JSON.parse(localStorage.getItem(key) ?? "{}");
  if (!("consentAcknowledgedAt" in current))
    localStorage.setItem(key, JSON.stringify({ ...current, consentAcknowledgedAt: "2026-10-19T08:00:00Z", setupDoneAt: "2026-10-19T08:00:00Z", uiLanguage: "en" }));
});
const page = await ctx.newPage();
const errors = [];
page.on("pageerror", (e) => errors.push(String(e)));
let shots = 0;
const shot = async (name, opts = {}) => {
  await page.waitForTimeout(400);
  const path = `${out}/${String(++shots).padStart(2, "0")}-${name}.png`;
  await page.screenshot({ path, fullPage: opts.fullPage ?? false });
  console.log(`  screenshot: ${path}`);
};
const toast = async (text, timeout = 30_000) => {
  const t = page.locator(".ob-toast", { hasText: text }).last();
  const ok = await t.waitFor({ timeout }).then(() => true, () => false);
  check(ok, `no toast "${text}"`);
  return ok ? (await t.innerText()).trim() : "";
};
const sheet = () => page.locator(".ob-sheet");
const dialog = () => page.locator(".dialog");
const catalogRow = (name) => page.locator("tbody tr", { has: page.locator("td > div", { hasText: name }) }).first();
const mlxCard = () => page.locator(".mlx-card");

async function goModels() {
  await page.goto(base);
  await page.locator('.nav-item[data-nav="models"]').click();
  await page.locator(".screen h1", { hasText: "Models" }).waitFor();
}

async function openRepo(query, repo, { allLicences = false, onlyMlx = false } = {}) {
  await page.getByRole("button", { name: "Browse Hugging Face" }).first().click();
  await sheet().waitFor();
  if (allLicences) await sheet().getByLabel("Also show models with restrictive or custom licences").check();
  if (onlyMlx) await sheet().getByLabel("Only show MLX-optimized models").check();
  await sheet().locator("input.ob-input").fill(query);
  await sheet().getByRole("button", { name: "Search", exact: true }).click();
  const hit = sheet().locator(".hub-row", { has: page.locator(".hub-name", { hasText: new RegExp(`^${repo.replace(/[.*+?^${}()|[\]\\/]/g, "\\$&")}$`) }) });
  await hit.first().waitFor({ timeout: MINUTE });
  await hit.first().click();
}

async function closeSheet() {
  // With a repo open the header button goes back to the results; Escape closes.
  await page.keyboard.press("Escape");
  await sheet().waitFor({ state: "detached" });
}

async function confirmRemove(expectTitle, expectBody) {
  await dialog().waitFor();
  const text = await dialog().innerText();
  check(text.includes(expectTitle), `dialog title: wanted "${expectTitle}", got "${text.split("\n")[0]}"`);
  if (expectBody) check(text.includes(expectBody), `dialog body lacks "${expectBody}": ${text}`);
  await dialog().locator(".ob-button[data-variant=danger]").click();
  await dialog().waitFor({ state: "detached", timeout: 2 * MINUTE });
}

async function addLocal(path, inPlace) {
  await page.getByRole("button", { name: "Add from this computer" }).click();
  await sheet().waitFor();
  await sheet().locator("input.ob-input").fill(path);
  await sheet().getByRole("button", { name: "Check", exact: true }).click();
  await sheet().getByRole("button", { name: "Add", exact: true }).waitFor({ timeout: MINUTE });
  if (inPlace) await sheet().getByLabel("Use it where it is, don't copy").check();
  await shot(inPlace ? "add-local-preview-in-place" : "add-local-preview-copy");
  await sheet().getByRole("button", { name: "Add", exact: true }).click();
  await toast("Model added", 2 * MINUTE);
  await sheet().waitFor({ state: "detached" }).catch(() => {});
}

try {
  // ---- 1. the Models screen on the Mac UI ----------------------------------
  step("Models screen (Mac UI, MLX test mode)");
  await goModels();
  await mlxCard().waitFor();
  await shot("models-mac");

  // ---- 2. the MLX engine card: download, set up, check ----------------------
  step("MLX engine card: Download");
  await mlxCard().getByRole("button", { name: /^Download \(/ }).click();
  await mlxCard().locator("[role=progressbar]").waitFor({ timeout: MINUTE }).catch(() => {});
  await shot("mlx-card-downloading");
  const ready = await mlxCard().locator(".mlx-ready", { hasText: "Ready" }).waitFor({ timeout: 20 * MINUTE }).then(() => true, () => false);
  check(ready, "the MLX card never said Ready");
  await shot("mlx-card-ready");
  const mlx1 = (await api("GET", "/api/mlx")).json;
  check(mlx1.pack_installed && mlx1.check === "passed", `MLX status after install: ${JSON.stringify(mlx1)}`);
  check(mlx1.installed_bytes > 100_000_000, `pack on disk: ${mlx1.installed_bytes}`);

  // ---- 3. an MLX model from the Hub -----------------------------------------
  step(`Hub: open ${MLX_REPO}, the MLX row, download`);
  await openRepo("Qwen3-0.6B-4bit", MLX_REPO, { onlyMlx: true });
  await sheet().getByText("Whole model folder").waitFor({ timeout: MINUTE });
  await sheet().locator(".hub-alternatives").waitFor({ timeout: 30_000 }).catch(() => {});
  check(await sheet().locator(".hub-alternatives").count(), "no GGUF builds listed for the MLX repo");
  await shot("hub-mlx-repo");
  await shot("hub-mlx-repo-full", { fullPage: true });
  await sheet().locator("tr", { hasText: "Whole model folder" }).getByRole("button", { name: "Download" }).click();
  added.add(MLX_ID);
  await sheet().waitFor({ state: "detached" });
  const mlxRow = catalogRow(`${MLX_REPO} (MLX)`);
  await mlxRow.waitFor({ timeout: MINUTE });
  const mlxReady = await mlxRow.locator(".row-state", { hasText: "Ready" }).waitFor({ timeout: 15 * MINUTE }).then(() => true, () => false);
  check(mlxReady, "the MLX model never finished downloading");
  await shot("mlx-model-downloaded");
  const mlxApiRow = await rowOf(MLX_ID);
  check(mlxApiRow?.installed && mlxApiRow.engine === "mlx" && mlxApiRow.parts === 9, `MLX row: ${JSON.stringify(mlxApiRow)}`);

  // ---- 4. a split GGUF in sub-folders --------------------------------------
  step(`Hub: ${SPLIT_REPO} lists split GGUFs in sub-folders as one row each`);
  await openRepo("Qwen3-235B-A22B-Instruct-2507-GGUF", SPLIT_REPO);
  await sheet().locator(".hub-parts").first().waitFor({ timeout: MINUTE });
  const partChips = await sheet().locator(".hub-parts").allInnerTexts();
  check(partChips.some((c) => /\d+ parts/.test(c)), `no "n parts" chips: ${partChips}`);
  const paths = await sheet().locator("tbody td .mono").allInnerTexts();
  check(paths.some((p) => p.includes("/")), "no files from sub-folders");
  check(!paths.some((p) => /mmproj|^mtp-|\/mtp-/i.test(p)), `helper files offered: ${paths.filter((p) => /mmproj|mtp-/i.test(p))}`);
  await shot("hub-split-gguf");
  await closeSheet();
  const split = (await api("GET", `/api/hub/repo?id=${encodeURIComponent(SPLIT_REPO)}`)).json;
  check(split.files?.some((f) => f.parts > 1 && f.path.includes("/")), "the API lists no split GGUF in a sub-folder");

  // ---- 5. a gated repo with no token ---------------------------------------
  step(`Hub: ${GATED_REPO} with no token`);
  await openRepo("gemma-3-1b-it-qat-q4_0-gguf", GATED_REPO, { allLicences: true });
  const gatedNotice = sheet().getByText("This model is gated. Request access on Hugging Face, then add your token.");
  check(await gatedNotice.first().waitFor({ timeout: MINUTE }).then(() => true, () => false), "no gated message");
  check(await sheet().getByText("Request access").count(), "no Request access link");
  check(await sheet().getByText("Get a token").count(), "no Get a token link");
  check(await sheet().locator("input[type=password]").count(), "no token field");
  const downloads = sheet().locator("tbody .ob-button");
  const enabled = await downloads.evaluateAll((bs) => bs.filter((b) => !b.disabled).length);
  check(enabled === 0, `${enabled} Download buttons are enabled on a gated repo without access`);
  await shot("hub-gated-no-token");
  await closeSheet();
  const gated = (await api("GET", `/api/hub/repo?id=${encodeURIComponent(GATED_REPO)}`)).json;
  check(gated.repo?.access === "needs_token", `gated access: ${gated.repo?.access}`);

  // ---- 5b. a gated repo with nothing OpenBark can run ------------------------
  // Search lists only GGUF and MLX repos, so this one is checked through the
  // API the sheet uses; the sheet's wording for it is covered by vitest.
  step(`Hub: ${NOTHING_REPO} (gated, safetensors only)`);
  const nothing = (await api("GET", `/api/hub/repo?id=${encodeURIComponent(NOTHING_REPO)}`)).json;
  check(nothing.repo?.gated_kind && nothing.files?.length === 0 && !nothing.mlx, `gated repo with nothing to run: ${JSON.stringify(nothing).slice(0, 300)}`);
  const builds = (await api("GET", `/api/hub/alternatives?base=${encodeURIComponent(NOTHING_REPO)}`)).json;
  check(Array.isArray(builds) && builds.length > 0, `no GGUF builds of ${NOTHING_REPO}: ${JSON.stringify(builds).slice(0, 200)}`);
  console.log(`  GGUF builds offered instead: ${(builds ?? []).map((b) => b.id).join(", ")}`);

  // ---- 6. add from this computer: copied, and used where it is --------------
  step("Add from this computer: copy a tiny GGUF");
  await addLocal(`${importDir}/Tiny-Q4_K_M.gguf`, false);
  const copied = (await rows()).find((r) => r.origin === "local_copy");
  check(copied?.installed && copied.publisher === "This computer", `copied row: ${JSON.stringify(copied)}`);
  // On the demo's one drive the copy is a hard link: shared, not extra space.
  check(copied && copied.disk_bytes + copied.shared_bytes === copied.size_bytes, `copied row's bytes: ${JSON.stringify(copied)}`);
  const linkedCopy = Boolean(copied && copied.shared_bytes > 0 && copied.disk_bytes === 0);
  if (linkedCopy) check(await catalogRow("Tiny-Q4_K_M").getByText("Linked to another file, so it takes no extra space").count(), "the linked copy's row doesn't say it takes no extra space");
  if (copied) added.add(copied.id);
  step("Add from this computer: use one where it is");
  await addLocal(`${importDir}/Tiny-InPlace.gguf`, true);
  const inPlace = (await rows()).find((r) => r.origin === "local_in_place");
  check(inPlace?.installed && inPlace.disk_bytes === 0 && inPlace.path, `in-place row: ${JSON.stringify(inPlace)}`);
  if (inPlace) added.add(inPlace.id);
  await catalogRow("Tiny-InPlace").scrollIntoViewIfNeeded();
  await shot("added-from-this-computer");

  // ---- 7. a downloaded GGUF: the note-model guard, remove, download again ---
  step(`Hub: download ${GGUF_FILE}`);
  await openRepo("SmolLM2-135M-Instruct-GGUF", GGUF_REPO);
  await sheet().locator("tr", { hasText: GGUF_FILE }).getByRole("button", { name: "Download" }).click();
  added.add(GGUF_ID);
  await sheet().waitFor({ state: "detached" });
  const ggufRow = () => catalogRow(`${GGUF_REPO} (SmolLM2-135M-Instruct-Q2_K)`);
  check(await ggufRow().locator(".row-state", { hasText: "Ready" }).waitFor({ timeout: 10 * MINUTE }).then(() => true, () => false), "the GGUF never finished downloading");
  const before = await rowOf(GGUF_ID);
  check(before?.installed && before.disk_bytes === 88202080, `GGUF row after download: ${JSON.stringify(before)}`);

  step("The note model can't be removed (screen and engine)");
  await page.getByLabel("Model that writes notes").selectOption(GGUF_ID);
  await ggufRow().scrollIntoViewIfNeeded();
  const removeBtn = ggufRow().getByRole("button", { name: "Remove" });
  check(await removeBtn.isDisabled(), "Remove is enabled on the note model");
  check(await ggufRow().getByText("Writes your notes. Choose another model first.").count(), "no note-model help text");
  await shot("note-model-guard");
  const refusal = await api("POST", "/api/models/remove", { id: GGUF_ID, note_model: GGUF_ID, forget: false, by: null });
  check(refusal.status === 400 && refusal.json.error === "This model writes your notes. Choose another one under In use first.", `engine note-model refusal: ${JSON.stringify(refusal)}`);
  // Back to the default note model (not downloaded in the demo), so every
  // model added here can be removed.
  await page.evaluate(() => {
    const key = "openbark.settings";
    const s = JSON.parse(localStorage.getItem(key) ?? "{}");
    localStorage.setItem(key, JSON.stringify({ ...s, llm: "gemma-4-e4b-it-q4_0" }));
  });
  await goModels();

  step("Remove the downloaded GGUF: files go, the row stays, Download brings it back");
  await ggufRow().getByRole("button", { name: "Remove" }).click();
  await confirmRemove("Remove", "You can download it again from the catalog.");
  const freedGguf = await toast("freed.");
  check(/Removed\. 88(\.\d)? MB freed\./.test(freedGguf), `GGUF toast: "${freedGguf}"`);
  await shot("removed-gguf-toast");
  const after = await rowOf(GGUF_ID);
  check(after && !after.installed && after.disk_bytes === 0 && after.partial_bytes === 0, `GGUF row after removal: ${JSON.stringify(after)}`);
  await ggufRow().getByRole("button", { name: /^Download \(/ }).click();
  check(await ggufRow().locator(".row-state", { hasText: "Ready" }).waitFor({ timeout: 10 * MINUTE }).then(() => true, () => false), "the GGUF didn't download again");
  const again = await rowOf(GGUF_ID);
  check(again?.installed && again.disk_bytes === 88202080, `GGUF row after downloading again: ${JSON.stringify(again)}`);

  // ---- 8. busy: a model writing can't be removed, nor its engine -------------
  step("Busy guard: an MLX model writing a template from an example");
  const writing = api("POST", "/api/templates/from-example", {
    llm: MLX_ID,
    text: "Subjective: vomiting for a day.\nObjective: T 38.9, mildly dehydrated.\nAssessment: gastritis.\nPlan: maropitant, bland diet.",
  });
  let inUse = false;
  for (let i = 0; i < 120 && !inUse; i++) {
    await page.waitForTimeout(1000);
    inUse = (await api("GET", "/api/mlx")).json.in_use;
  }
  check(inUse, "the MLX model never showed as loaded");
  const busy = await api("POST", "/api/models/remove", { id: MLX_ID, note_model: null, forget: false, by: null });
  check(busy.json.error === "A note is being written with this model. Remove it when that's finished.", `busy model: ${JSON.stringify(busy)}`);
  const busyEngine = await api("POST", "/api/mlx/remove", { note_model: null, by: null });
  check(busyEngine.json.error === "A note is being written with an MLX model. Remove the engine when that's finished.", `busy engine: ${JSON.stringify(busyEngine)}`);
  check((await rowOf(MLX_ID))?.installed, "the busy model's files were touched");
  const written = await Promise.race([writing, new Promise((r) => setTimeout(() => r({ status: "timeout" }), 15 * MINUTE))]);
  console.log(`  template from example finished: ${written.status} ${written.json?.error ?? ""}`);
  check(written.status !== "timeout", "the running generation never finished");

  // ---- 9. remove each kind, with the space freed -----------------------------
  await goModels();
  step(`Remove the copied model (${linkedCopy ? "a hard link: frees nothing" : "a real copy"})`);
  await catalogRow("Tiny-Q4_K_M").getByRole("button", { name: "Remove" }).click();
  if (linkedCopy) {
    await confirmRemove("Remove Tiny-Q4_K_M?", "OpenBark's copy is linked to the original file, so removing it frees no space.");
    await toast("Removed. It was linked to another file, so no space was freed.");
  } else {
    await confirmRemove("Remove Tiny-Q4_K_M?", "This deletes OpenBark's copy and frees");
    check(/^Removed\. .+ freed\.$/.test(await toast("freed.")), "copied model: no freed toast");
  }
  check(!(await rows()).some((r) => r.origin === "local_copy"), "the copied model's row is still there");

  step("Forget the model used where it is");
  await catalogRow("Tiny-InPlace").getByRole("button", { name: "Forget" }).click();
  await confirmRemove("Forget Tiny-InPlace?", "The file stays where it is, and nothing is deleted.");
  await toast("Forgotten. Nothing was deleted.");
  check(!(await rows()).some((r) => r.origin === "local_in_place"), "the in-place row is still there");

  step("Remove the MLX model, then remove it from the list");
  await catalogRow(`${MLX_REPO} (MLX)`).getByRole("button", { name: "Remove" }).click();
  await confirmRemove(`Remove ${MLX_REPO} (MLX)?`, "You can download it again from the catalog.");
  const freedMlx = await toast("freed.");
  check(/Removed\. 351(\.\d)? MB freed\./.test(freedMlx), `MLX model toast: "${freedMlx}"`);
  const mlxAfter = await rowOf(MLX_ID);
  check(mlxAfter && !mlxAfter.installed && mlxAfter.disk_bytes === 0, `MLX row after removal: ${JSON.stringify(mlxAfter)}`);
  await catalogRow(`${MLX_REPO} (MLX)`).getByRole("button", { name: "Remove from list" }).click();
  await toast("Removed from the list.");
  check(!(await rowOf(MLX_ID)), "the MLX row is still listed");

  step("Remove the MLX engine");
  const packBytes = (await api("GET", "/api/mlx")).json.installed_bytes;
  await mlxCard().getByRole("button", { name: "Remove" }).click();
  await confirmRemove("Remove the MLX engine?", "MLX models stay on this computer");
  const freedPack = await toast("freed.");
  check(/^Removed\. .+ freed\.$/.test(freedPack), `engine toast: "${freedPack}"`);
  await mlxCard().getByRole("button", { name: /^Download \(/ }).waitFor({ timeout: 30_000 });
  const mlx2 = (await api("GET", "/api/mlx")).json;
  check(!mlx2.pack_installed && mlx2.installed_bytes === 0, `MLX status after removal: ${JSON.stringify(mlx2)}`);
  await shot("mlx-engine-removed");
  console.log(`  the pack used ${packBytes} bytes; toast: ${freedPack}`);

  step("Remove the GGUF and drop it from the list");
  await ggufRow().getByRole("button", { name: "Remove" }).click();
  await dialog().getByLabel("Also remove it from the list").check();
  await confirmRemove("Remove", null);
  await toast("freed.");
  check(!(await rowOf(GGUF_ID)), "the GGUF row is still listed after Remove from list");
  await shot("models-after");

  step("The speech models OpenBark needs say so instead of offering Remove");
  const vad = await api("POST", "/api/models/remove", { id: "silero-vad", note_model: null, forget: false, by: null });
  check(vad.json.error === "OpenBark needs this to transcribe visits.", `needed model: ${JSON.stringify(vad)}`);
} catch (e) {
  problems.push(`stopped: ${e.message}`);
  await shot("stopped").catch(() => {});
} finally {
  await browser.close();
}

if (errors.length) problems.push(...errors.map((e) => `page error: ${e}`));
console.log(`\nadded in this run: ${[...added].join(", ") || "nothing"}`);
if (problems.length) {
  console.log(`\n${problems.length} problem(s):\n- ${problems.join("\n- ")}`);
  process.exit(1);
}
console.log("\nno problems");
