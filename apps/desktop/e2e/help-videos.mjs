// Record the in-app help videos (public/help/<topic>.webm) by driving the
// real app with captions and a visible pointer.
//
// Run against a demo server with demo data, never a clinic's own:
//   OPENBARK_URL=http://localhost:4318 node e2e/help-videos.mjs [topic ...]
// The "record" topic plays FAKE_MIC (a WAV) as the microphone.
import { mkdirSync, renameSync, rmSync } from "node:fs";
import { resolve } from "node:path";
import { chromium } from "@playwright/test";

const base = process.env.OPENBARK_URL ?? "http://localhost:4318";
const outDir = resolve("public/help");
const tmpDir = resolve("../../target/help-video-tmp");
const fakeMic = resolve(process.env.FAKE_MIC ?? "../../packages/eval/fixtures/audio/smoke.wav");
const size = { width: 1280, height: 800 };
mkdirSync(outDir, { recursive: true });

const SEEDED = {
  consentAcknowledgedAt: "2026-10-17T20:00:00Z",
  signer: "Dr. Demo",
  clinicName: "Maple Street Animal Hospital",
  clinicLines: "12 Maple Street\n416-555-0134",
  uiLanguage: "en",
};

// Captions and a pointer drawn into the page (videos have no sound).
function overlay() {
  const install = () => {
    if (document.getElementById("__help_overlay")) return;
    const style = document.createElement("style");
    style.textContent = `
      #__help_caption { position: fixed; left: 50%; bottom: 28px; transform: translateX(-50%); z-index: 99999;
        max-width: 880px; padding: 12px 20px; border-radius: 10px; background: rgba(27,31,36,0.88); color: #fff;
        font: 500 19px/1.45 "Geist Sans", system-ui, sans-serif; text-align: center; opacity: 0; transition: opacity 200ms; pointer-events: none; }
      #__help_pointer { position: fixed; z-index: 100000; width: 18px; height: 18px; margin: -9px 0 0 -9px; border-radius: 50%;
        background: rgba(47,111,106,0.35); border: 2px solid #2F6F6A; pointer-events: none; transition: transform 120ms; }
      #__help_pointer.down { transform: scale(0.7); background: rgba(47,111,106,0.6); }`;
    document.head.appendChild(style);
    const cap = document.createElement("div");
    cap.id = "__help_caption";
    const ptr = document.createElement("div");
    ptr.id = "__help_pointer";
    ptr.style.left = "-40px";
    ptr.style.top = "-40px";
    const wrap = document.createElement("div");
    wrap.id = "__help_overlay";
    wrap.append(cap, ptr);
    document.body.appendChild(wrap);
    addEventListener("mousemove", (e) => { ptr.style.left = e.clientX + "px"; ptr.style.top = e.clientY + "px"; }, true);
    addEventListener("mousedown", () => ptr.classList.add("down"), true);
    addEventListener("mouseup", () => ptr.classList.remove("down"), true);
    window.__caption = (text) => { cap.textContent = text; cap.style.opacity = text ? "1" : "0"; };
  };
  if (document.readyState === "loading") addEventListener("DOMContentLoaded", install);
  else install();
}

function helpers(page) {
  const caption = async (text, ms = 2800) => {
    await page.evaluate((t) => window.__caption?.(t), text);
    await page.waitForTimeout(ms);
  };
  const point = async (locator) => {
    await locator.scrollIntoViewIfNeeded();
    const b = await locator.boundingBox();
    if (!b) return;
    await page.mouse.move(b.x + b.width / 2, b.y + Math.min(b.height / 2, 18), { steps: 18 });
    await page.waitForTimeout(250);
  };
  const click = async (locator) => {
    await point(locator);
    await page.mouse.down();
    await page.mouse.up();
    await page.waitForTimeout(350);
  };
  const type = async (locator, text) => {
    await click(locator);
    await page.keyboard.press("Control+A");
    await page.keyboard.type(text, { delay: 40 });
    await page.waitForTimeout(250);
  };
  const nav = (i) => page.locator(".nav-item").nth(i);
  return { caption, point, click, type, nav };
}

const TOPICS = {
  async "getting-started"(page) {
    const { caption, point, click, type, nav } = helpers(page);
    await page.goto(base);
    await page.locator(".dialog").waitFor();
    await caption("OpenBark records consultations and writes the medical record, all on this computer.", 3200);
    await caption("First, a one-time notice: you get each client's consent before recording.", 3200);
    await click(page.locator(".dialog input[type=checkbox]"));
    await click(page.locator(".dialog .ob-button"));
    await point(nav(0));
    await caption("Record is where every consult starts.");
    await point(nav(1));
    await caption("Encounters keeps every recording, transcript and note.");
    await point(nav(2));
    await caption("Templates holds your clinic's note templates.");
    await point(nav(3));
    await caption("Models lists the speech and note models. They download once; then OpenBark works offline.", 3400);
    await click(nav(4));
    await caption("In Settings, add your name for signing notes.");
    await type(page.locator(".settings input.ob-input").first(), "Dr. Demo");
    const lang = page.locator(".settings select").first();
    await point(lang);
    await caption("Choose the app language, and the language notes are written in.", 3200);
    await point(nav(5));
    await caption("Help has these walkthroughs whenever you need them.", 3000);
  },

  async record(page) {
    const { caption, click, type } = helpers(page);
    await page.goto(base);
    await page.locator(".recorder").waitFor();
    await caption("Enter the patient and the reason for the visit. These are optional.");
    const fields = page.locator(".recorder-patient .ob-input");
    await type(fields.nth(0), "Pepper");
    await type(fields.nth(1), "Okonkwo");
    await type(fields.nth(2), "Ear infection recheck");
    await caption("Press Record, or the Space bar.");
    await click(page.locator(".ob-record"));
    await caption("Each sentence appears in the transcript as soon as it's finished.", 6000);
    await caption("The audio is saved, encrypted, as you go. Speakers are labelled for you.", 6000);
    await caption("Keep the conversation natural; OpenBark listens in the background.", 6000);
    await caption("Press the button again to stop.", 1800);
    await click(page.locator(".ob-record"));
    await page.locator(".review").waitFor({ timeout: 60000 });
    await caption("The note is drafted automatically. Numbers are checked against the transcript when it finishes.", 5000);
  },

  async review(page) {
    const { caption, point, click, type, nav } = helpers(page);
    await page.goto(base);
    await click(nav(1));
    await click(page.locator('tr.row-link:has(.ob-dot[data-tone="warn"])').first());
    await page.locator(".note").waitFor();
    await caption("Review shows the transcript on the left and the draft note on the right.", 3200);
    const line = page.locator(".pane-note .stmt").nth(2);
    await point(line);
    await caption("Rest on any line to light up where it was said in the recording.", 3400);
    await click(line);
    await caption("Click it to hear that moment of the recording.");
    const part = page.locator(".note-part", { hasText: "Follow-up" }).first();
    await point(part.locator(".note-part-head"));
    await click(part.getByRole("button", { name: /^Ask for changes/ }));
    await type(page.locator("form.ask textarea"), "One line per follow-up, with when and why");
    await caption("Ask redrafts just that heading from the recording. Save it as a standard to make it the rule.", 3800);
    await click(page.locator("form.ask .ob-button[data-variant=ghost]"));
    await point(part.locator(".note-part-head"));
    await click(part.getByRole("button", { name: /^Edit / }));
    await click(page.locator("form.edit-heading .ob-button[data-variant=ghost]").first());
    await type(page.locator("form.edit-heading textarea").last(), "Owner to call if the head shaking returns");
    await caption("Or edit a heading yourself. Your lines are marked as yours.");
    await click(page.locator("form.edit-heading .ob-button[data-variant=primary]"));
    await page.waitForTimeout(1200);
    const sign = page.locator(".review-actions .ob-button[data-variant=primary]");
    await point(sign);
    await caption("When it's right, Sign note locks it. Anything later goes in as an addendum.", 3200);
    await click(sign);
    await caption("Signed.", 2200);
  },

  async templates(page) {
    const { caption, click, type, nav } = helpers(page);
    await page.goto(base);
    await click(nav(2));
    await caption("Templates are the headings of your notes.");
    await click(page.locator(".head-actions .ob-button").first());
    await page.locator(".tb-heading").first().waitFor();
    await type(page.locator(".builder-meta input.ob-input").first(), "Dental procedure");
    await caption("Start from SOAP, or from blank headings.");
    await click(page.locator(".builder-structure > .ob-button").last());
    const heading = page.locator(".tb-heading").last();
    await type(heading.locator("input.tb-title"), "Anaesthesia");
    await heading.locator("select").selectOption("medications");
    await type(heading.locator("textarea"), "Premedication, induction and maintenance as said.");
    await caption("Give each heading what goes under it, and plain-language instructions.", 3200);
    await click(page.locator(".builder-preview [role=tab]").nth(1));
    await caption("This is exactly what the model is told. Every template keeps the core rules: only what was said.", 3800);
    await click(page.locator(".head-actions .ob-button[data-variant=primary]"));
    await caption("Save it, then try it on a recorded consult.", 2800);
  },

  async documents(page) {
    const { caption, point, click, nav } = helpers(page);
    await page.goto(base);
    await click(nav(1));
    await click(page.locator('tr.row-link:has(.ob-dot[data-tone="ok"])').first());
    await page.locator(".note").waitFor();
    await caption("Under a note: copy it, save a PDF, or make documents for the owner.");
    await click(page.getByRole("button", { name: "Discharge instructions", exact: true }));
    await page.locator(".ob-sheet .note").waitFor({ timeout: 10000 });
    await caption("Discharge instructions are in plain language and follow your corrected note.", 3400);
    await page.locator(".ob-sheet .sheet-body").evaluate((el) => el.scrollBy({ top: 500, behavior: "smooth" }));
    await page.waitForTimeout(1500);
    await point(page.locator(".ob-sheet .output-row .ob-button").nth(1));
    await caption("Copy it, or save a PDF with your clinic's letterhead.");
    await page.keyboard.press("Escape");
    const copy = page.locator(".output-row .ob-button").first();
    await click(copy);
    await caption("Copy note pastes the note into your PIMS, as plain or formatted text (Settings).", 3400);
  },

  async team(page) {
    const { caption, click, type, nav, point } = helpers(page);
    await page.goto(base);
    await click(nav(4));
    const team = page.locator("section.group", { hasText: "Team" }).first();
    await point(team.locator("h2"));
    await caption("Working with a team? Add everyone in Settings. The first member is a veterinarian.", 3400);
    await type(team.locator("input.ob-input").nth(0), "Dr. Demo");
    await type(team.locator("input.ob-input").nth(1), "2468");
    await click(team.locator(".team-add .ob-button").first());
    await page.locator(".user-picker").waitFor({ timeout: 10000 });
    await caption("Now each person picks their name and enters their PIN.");
    await click(page.locator(".user-picker .who").first());
    await type(page.locator(".user-picker input"), "2468");
    await click(page.locator(".user-picker .ob-button[data-variant=primary]"));
    await page.locator(".user-picker").waitFor({ state: "detached" });
    const team2 = page.locator("section.group", { hasText: "Team" }).first();
    await type(team2.locator(".team-add input.ob-input").nth(0), "Sam");
    await team2.locator(".team-add select").selectOption("rvt");
    await type(team2.locator(".team-add input.ob-input").nth(1), "1357");
    await click(team2.locator(".team-add .ob-button").first());
    await caption("Veterinarians sign. Technicians draft and edit. Reception records. Every revision records who did it.", 4000);
    await point(page.locator("section.group", { hasText: "Encryption" }).locator("h2"));
    await caption("Recordings, transcripts and notes are encrypted on this computer. Keep the recovery key safe.", 4000);
  },
};

const wanted = process.argv.slice(2).length ? process.argv.slice(2) : Object.keys(TOPICS);
const browser = await chromium.launch({
  args: ["--use-fake-ui-for-media-stream", "--use-fake-device-for-media-stream", `--use-file-for-fake-audio-capture=${fakeMic}`],
});
for (const id of wanted) {
  rmSync(tmpDir, { recursive: true, force: true });
  const ctx = await browser.newContext({
    viewport: size,
    recordVideo: { dir: tmpDir, size },
    permissions: ["microphone", "clipboard-read", "clipboard-write"],
    acceptDownloads: true,
  });
  const fresh = id === "getting-started";
  await ctx.addInitScript(
    ({ seed, fresh }) => {
      if (!fresh && !localStorage.getItem("openbark.settings")) localStorage.setItem("openbark.settings", JSON.stringify(seed));
    },
    { seed: SEEDED, fresh },
  );
  await ctx.addInitScript(overlay);
  const page = await ctx.newPage();
  const errors = [];
  page.on("pageerror", (e) => errors.push(String(e)));
  try {
    await TOPICS[id](page);
  } catch (e) {
    errors.push(String(e).split("\n")[0]);
  }
  const video = page.video();
  await ctx.close();
  const path = await video.path();
  renameSync(path, `${outDir}/${id}.webm`);
  console.log(`${id}: ${errors.length ? errors.join(" | ") : "ok"}`);
}
await browser.close();
rmSync(tmpDir, { recursive: true, force: true });
