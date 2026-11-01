// Record the in-app help videos by driving the real app with captions and a
// visible pointer: public/help/<topic>.webm (light theme) and
// public/help/<topic>-dark.webm (dark theme; Help picks it in dark mode).
//
// Run against a demo server with demo data, never a clinic's own (see
// docs/guide/README.md for the recipe, its neutral C:/OpenBark-demo paths and
// the seeded patients the topics expect):
//   OPENBARK_URL=http://localhost:4318 HELP_THEME=light node e2e/help-videos.mjs [topic ...]
// The "sync" topic needs two demo servers built with OPENBARK_SYNC_PREVIEW:
// OPENBARK_URL is the clinic computer that becomes the hub, OPENBARK_SYNC_URL
// a second computer that joins it, both fresh (.github/workflows/help-videos.yml).
// HELP_THEME is light (default) or dark. Topics share state in order (record
// adds Pepper's recheck that patients opens, review signs Mochi's consult that
// documents uses, team adds the first member), so reset the demo between the
// light and dark passes.
// The "record" topic plays FAKE_MIC (a WAV) as the microphone: the whole
// consult is captured, so give it a copy of smoke.wav with a few seconds of
// silence at the end (the browser loops the file).
//
// While recording it scans the page for anything that would show this
// computer's own paths (the user name, AppData, temp folders) and fails the
// topic if it finds one; it refuses to start if the server's folders aren't
// neutral. Drafts the videos show are checked too: a cut-off line or a note
// that's mostly "Not discussed" fails the topic.
import { mkdirSync, renameSync, rmSync } from "node:fs";
import { userInfo } from "node:os";
import { resolve } from "node:path";
import { chromium } from "@playwright/test";
const OLD_NAV = ["record", "patients", "templates", "models", "settings", "help"]; // nav by name (data-nav, ADR-0027)

const base = process.env.OPENBARK_URL ?? "http://localhost:4318";
const joiner = process.env.OPENBARK_SYNC_URL ?? "http://localhost:4319";
const outDir = resolve("public/help");
const tmpDir = resolve("../../target/help-video-tmp");
const fakeMic = resolve(process.env.FAKE_MIC ?? "../../packages/eval/fixtures/audio/smoke.wav");
// 16:10 like the player, and as small as the app gets before the side
// navigation folds to icons (1200 px): the player is about 730 px wide, so
// the smaller the window, the more readable the app's small text is in it.
const size = { width: 1200, height: 750 };
const theme = process.env.HELP_THEME === "dark" ? "dark" : "light";
const suffix = theme === "dark" ? "-dark" : "";
const LLM = "gemma-4-e4b-it-q4_0";
mkdirSync(outDir, { recursive: true });

// Text that must never be on screen: this computer's user name and private folders.
const user = userInfo().username;
const escape = (text) => text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
const LEAK = new RegExp(
  [user.length > 2 ? escape(user) : null, "AppData", "\\bTemp\\b", "scratchpad", "[\\\\/]Users[\\\\/]"]
    .filter(Boolean)
    .join("|"),
  "i",
);

const SEEDED = {
  consentAcknowledgedAt: "2026-10-17T20:00:00Z",
  setupDoneAt: "2026-10-17T20:00:00Z",
  signer: "Dr. Demo",
  clinicName: "Maple Street Animal Hospital",
  clinicLines: "12 Maple Street\n416-555-0134",
  uiLanguage: "en",
};

// Captions and a pointer drawn into the page (videos have no sound). Flat,
// Manrope, ink on light and a raised dark surface on dark (DESIGN.md §3, §4,
// §5.3); the pointer stays ink or white so amber is left to the app (§3.5).
function overlay(theme) {
  const dark = theme === "dark";
  const install = () => {
    if (document.getElementById("__help_overlay")) return;
    const style = document.createElement("style");
    const capLook = dark
      ? "background: #243040; color: #F5F7FA; border: 1px solid #34425A;"
      : "background: #182230; color: #FFFFFF; border: 1px solid #182230;";
    const ptrLine = dark ? "#F5F7FA" : "#182230";
    const ptrFill = dark ? "rgba(245,247,250,0.28)" : "rgba(24,34,48,0.22)";
    const ptrDown = dark ? "rgba(245,247,250,0.5)" : "rgba(24,34,48,0.45)";
    style.textContent = `
      #__help_caption { position: fixed; left: 50%; bottom: 24px; transform: translateX(-50%); z-index: 99999;
        width: max-content; max-width: min(860px, calc(100vw - 96px)); padding: 12px 20px; border-radius: 14px; ${capLook}
        font: 500 19px/1.45 "Manrope", system-ui, -apple-system, "Segoe UI", sans-serif; text-align: center;
        opacity: 0; transition: opacity 200ms cubic-bezier(0.2, 0, 0, 1); pointer-events: none; }
      #__help_pointer { position: fixed; z-index: 100000; width: 18px; height: 18px; margin: -9px 0 0 -9px; border-radius: 50%;
        background: ${ptrFill}; border: 2px solid ${ptrLine}; pointer-events: none; transition: transform 120ms; }
      #__help_pointer.down { transform: scale(0.7); background: ${ptrDown}; }`;
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
    addEventListener(
      "mousemove",
      (e) => {
        ptr.style.left = e.clientX + "px";
        ptr.style.top = e.clientY + "px";
      },
      true,
    );
    addEventListener("mousedown", () => ptr.classList.add("down"), true);
    addEventListener("mouseup", () => ptr.classList.remove("down"), true);
    window.__captionAt = (where) => {
      cap.style.bottom = where === "top" ? "auto" : "24px";
      cap.style.top = where === "top" ? "92px" : "auto";
    };
    window.__caption = (text) => {
      if (text) cap.textContent = text;
      cap.style.opacity = text ? "1" : "0";
    };
  };
  if (document.readyState === "loading") addEventListener("DOMContentLoaded", install);
  else install();
}

function helpers(page) {
  const caption = async (text, ms = 2800) => {
    await page.evaluate((t) => window.__caption?.(t), text);
    await page.waitForTimeout(ms);
  };
  /** Hide the caption: a caption never carries over onto a screen it doesn't describe. */
  const clear = async () => {
    await page.evaluate(() => window.__caption?.(""));
    await page.waitForTimeout(250);
  };
  const captionAt = (where) => page.evaluate((w) => window.__captionAt?.(w), where);
  const center = async (locator) => {
    await locator.evaluate((el) => el.scrollIntoView({ block: "center", behavior: "smooth" }));
    await page.waitForTimeout(700);
  };
  const point = async (locator) => {
    await locator.scrollIntoViewIfNeeded();
    let b = await locator.boundingBox();
    if (!b) return;
    // Keep the target clear of the screen header and the caption: centre it if it's near an edge.
    if (b.y < 96 || b.y + b.height > size.height - 110) {
      await center(locator);
      b = (await locator.boundingBox()) ?? b;
    }
    await page.mouse.move(b.x + Math.min(b.width / 2, 120), b.y + Math.min(b.height / 2, 18), { steps: 18 });
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
  const nav = (i) => page.locator(`.nav-item[data-nav="${OLD_NAV[i]}"]`);
  return { caption, clear, captionAt, center, point, click, type, nav };
}

/**
 * Refuse to show a draft that would teach the wrong thing: a statement cut
 * off mid-sentence ("The left ear can"), or a note where most headings are
 * empty. Drafted lines are short phrases without full stops, so a line counts
 * as cut off when it ends on a word no statement ends on.
 */
async function checkDraft(note, maxEmpty = 2) {
  const report = await note.first().evaluate((el) => {
    const DANGLING =
      /\b(a|an|the|and|or|but|to|of|with|for|from|by|is|are|was|were|be|been|can|could|will|would|should|may|might|has|have|had|that|which|who|if|as|than|their|your|my|very)$/i;
    const cut = [];
    for (const stmt of el.querySelectorAll(".stmt")) {
      const copy = stmt.cloneNode(true);
      for (const mark of copy.querySelectorAll(".vet-mark")) mark.remove();
      const text = copy.textContent
        .replace(/\[[^\]]*\]/g, "")
        .trim()
        .replace(/[\s.,;:]+$/, "");
      if (text && DANGLING.test(text)) cut.push(text);
    }
    return { cut, empty: el.querySelectorAll(".not-discussed").length, headings: el.querySelectorAll("h3").length };
  });
  if (report.cut.length) throw new Error(`draft has cut-off lines: ${report.cut.slice(0, 3).join(" / ")}`);
  if (report.empty > maxEmpty)
    throw new Error(`draft has ${report.empty} of ${report.headings} headings with nothing under them`);
}

/** Open a visit from Patients, Recent visits, by its patient's name. */
async function openVisit(page, h, name) {
  await h.click(h.nav(1));
  await h.click(page.locator('.nav-item[data-nav="visits"]'));
  await h.click(page.locator("tr.row-link", { hasText: name }).first());
  await page.locator(".note").waitFor();
}

const TOPICS = {
  async "getting-started"(page) {
    const { caption, clear, center, point, click, type, nav } = helpers(page);
    await page.goto(base);
    await page.locator(".dialog").waitFor();
    await caption("OpenBark records consultations and writes the medical record, all on this computer.", 3200);
    await caption("First, a one-time notice: you get each client's consent before recording.", 3200);
    await click(page.locator(".dialog input[type=checkbox]"));
    await click(page.locator(".dialog .ob-button"));
    // A demo without models opens the setup walkthrough (ADR-0034); this
    // video is about the app around it, so it's skipped here.
    const walk = page.locator(".setup-walk");
    if (await walk.waitFor({ timeout: 3000 }).then(() => true, () => false)) {
      await click(walk.getByRole("button", { name: "Skip setup" }));
    }
    await point(nav(0));
    await caption("Record is where every consult starts.");
    await point(nav(1));
    await caption("Patients keeps every animal, and each visit's recording, transcript and note.");
    await point(nav(2));
    await caption("Templates holds your clinic's note templates.");
    await point(nav(3));
    await caption("Models lists the speech and note models. They download once; then OpenBark works offline.", 3400);
    await clear();
    await click(nav(4));
    await caption("In Settings, add your name for signing notes.", 1200);
    await type(page.locator(".settings input.ob-input").first(), "Dr. Demo");
    await page.waitForTimeout(800);
    await clear();
    // The Language section: both of its menus sit above the caption.
    const language = page.locator("section.group").filter({ has: page.locator("h2", { hasText: /^Language$/ }) });
    await center(language);
    await point(language.locator("select").first());
    await caption("Choose the app language,", 1600);
    await point(language.locator("select").nth(1));
    await caption("and the language notes are written in.", 2400);
    await clear();
    const storage = page.locator(".storage-row").first();
    await center(storage);
    await point(storage.getByRole("button", { name: "Change folder" }));
    await caption("In Storage, choose where visits and models are kept, such as another drive.", 3400);
    await clear();
    await point(nav(5));
    await caption("Help has these walkthroughs whenever you need them.", 3000);
  },

  async record(page) {
    const { caption, clear, captionAt, point, click, type } = helpers(page);
    await page.goto(base);
    await page.locator(".recorder").waitFor();
    const search = page.locator(".patient-search input");
    await point(search);
    await caption("Find a patient you've seen before, or enter a new one. These are optional.", 3000);
    await type(search, "Pepp");
    await click(page.locator(".patient-match").first());
    await page.locator(".picked-patient").waitFor();
    await caption("Choosing a patient fills in their details and links the visit to them.", 3000);
    await type(page.locator("#strip-reason"), "Ear infection recheck");
    await clear();
    // One Template choice beside the recorder (ADR-0027).
    const template = page.locator(".record-side select.opt-select");
    await point(template);
    await template.selectOption({ label: "Recheck" });
    await caption("Choose the template. Each has its own headings.", 2800);
    await point(page.locator(".ob-record"));
    await caption("Press Record, or the Space bar.", 2000);
    await click(page.locator(".ob-record"));
    await clear();
    // Captions start once the microphone is really recording.
    await page.locator(".recorder-meta", { hasText: "Recording" }).waitFor({ timeout: 20000 });
    // The live transcript grows under the button: keep its newest line in
    // view, with the caption at the top so it doesn't cover the words.
    await captionAt("top");
    const lines = page.locator(".recorder-transcript .transcript > li");
    const follow = async (text, ms, until) => {
      await page.evaluate((t) => window.__caption?.(t), text);
      const end = Date.now() + ms;
      while (Date.now() < end) {
        await lines
          .last()
          .evaluate((el) => el.scrollIntoView({ block: "end", behavior: "smooth" }), null, { timeout: 500 })
          .catch(() => {});
        if (until && (await until())) return;
        await page.waitForTimeout(600);
      }
    };
    await follow("Listening. Each sentence appears in the transcript as soon as it's finished.", 12000);
    await follow("The audio is saved, encrypted, as you go.", 12000);
    await follow("Keep the conversation natural; OpenBark listens in the background.", 12000);
    // The consult ends with the client's thanks; stop then.
    const thanked = async () => /thank you/i.test((await page.locator(".recorder-transcript").innerText()) ?? "");
    await follow("Talk through your findings and plan as you normally would.", 25000, thanked);
    await page.waitForTimeout(1200);
    await captionAt("bottom");
    await point(page.locator(".ob-record"));
    await caption("Press the button again to stop.", 1800);
    await click(page.locator(".ob-record"));
    await captionAt("top");
    await caption("The last words are transcribed, then the note is written, heading by heading.", 600);
    await page.locator(".review").waitFor({ timeout: 90000 });
    const sign = page.locator(".review-actions .ob-button[data-variant=primary]");
    await page.locator(".pane-note .stmt").first().waitFor({ timeout: 180000 });
    await page.locator(".board").waitFor({ state: "detached", timeout: 180000 });
    await sign.waitFor({ timeout: 30000 });
    await checkDraft(page.locator(".pane-note .note"), 3);
    await captionAt("bottom");
    await clear();
    await point(page.locator(".pane-note .stmt").nth(1));
    await caption("The note is drafted automatically. Every number is checked against the transcript.", 3600);
    await point(page.locator(".pane-note .stmt").nth(4));
    await caption("Next, read it through in Review.", 2400);
  },

  async patients(page) {
    const { caption, clear, point, click, type, nav } = helpers(page);
    await page.goto(base);
    await click(nav(1));
    await page.locator(".patient-list").waitFor();
    await caption("Patients lists every animal, most recently seen first.", 3000);
    await type(page.locator(".patient-filter input"), "Pepp");
    await caption("Type a name to find one.", 2200);
    await clear();
    await click(page.locator(".patient-row").first());
    await page.locator(".visit-list").waitFor();
    await point(page.locator(".visit-list > li").first());
    await caption("A patient's page has every visit, newest first, with its type and whether it's signed.", 3800);
    await clear();
    await click(page.getByRole("button", { name: "Edit details" }));
    const form = page.locator("form.patient-edit");
    await form.waitFor();
    await point(form.locator("select"));
    await caption("Edit details corrects the name or species. Past visits keep what was entered at the time.", 3800);
    await clear();
    await click(form.getByRole("button", { name: "Cancel" }));
    await click(page.locator(".head-actions").getByRole("button", { name: "New visit" }));
    await page.locator(".picked-patient").waitFor();
    await point(page.locator(".picked-patient"));
    await caption("New visit opens Record with the patient already filled in.", 3200);
    await clear();
    await click(nav(1));
    await click(page.locator('.nav-item[data-nav="visits"]'));
    await page.locator("tr.row-link").first().waitFor();
    await caption("Recent visits lists the day's recordings.", 3000);
  },

  async review(page) {
    const h = helpers(page);
    const { caption, clear, captionAt, center, point, click, type } = h;
    await page.goto(base);
    await openVisit(page, h, "Mochi Tanaka");
    await checkDraft(page.locator(".pane-note .note"));
    await caption("Review shows the transcript on the left and the draft note on the right.", 3200);
    const line = page.locator(".pane-note .stmt").nth(3);
    await point(line);
    await caption("Rest on any line to light up where it was said in the recording.", 3400);
    await click(line);
    await caption("Click it to hear that moment of the recording.");
    // Stop playback, so the transcript doesn't scroll along for the rest of the video.
    await page.locator("audio").first().evaluate((a) => a.pause()).catch(() => {});
    await clear();
    // A heading is a section (h3) or a part of one (h4); its tools sit beside the title.
    const title = page.locator(".note h3, .note h4").filter({ hasText: /^(Follow-up|Next recheck)$/ }).first();
    const head = title.locator("xpath=..");
    const part = head.locator("xpath=..");
    await point(head);
    await click(head.getByRole("button", { name: /^Ask for changes/ }));
    const ask = page.locator("form.ask");
    await captionAt("top");
    await type(ask.locator("textarea"), "Say when, and what to watch for at home");
    await caption("Ask tells the model what to change under this heading only.", 3000);
    await point(ask.locator("label.check"));
    await caption("Tick Save as a standard to make it the rule for every draft of this kind.", 3400);
    await clear();
    await click(ask.getByRole("button", { name: /^Redraft/ }));
    await caption("Only that heading is redrafted, from the recording.", 1000);
    await ask.waitFor({ state: "detached", timeout: 180000 });
    await page.locator(".board").waitFor({ state: "detached", timeout: 60000 });
    await page.waitForTimeout(400);
    await checkDraft(page.locator(".pane-note .note"));
    await center(title);
    await point(part.locator(".refined").first());
    await caption("The new draft notes what you asked for; the earlier one is kept.", 3400);
    await clear();
    await captionAt("bottom");
    await click(head.getByRole("button", { name: /^Edit / }));
    const edit = part.locator("form.edit-heading");
    await click(edit.getByRole("button", { name: "Add line" }));
    await type(edit.locator("textarea").last(), "Owner to call if she vomits again tonight");
    await caption("Or edit a heading yourself.", 2200);
    await clear();
    await click(edit.getByRole("button", { name: "Save changes" }));
    await edit.waitFor({ state: "detached" });
    await page.waitForTimeout(600);
    // Back to the heading just edited, so the mark is on screen with its caption.
    await center(title);
    const mark = part.locator(".vet-mark").first();
    await point(mark);
    await caption("Your lines are marked as yours.", 3200);
    await clear();
    const sign = page.locator(".review-actions .ob-button[data-variant=primary]");
    await point(sign);
    await caption("When it's right, Sign note locks it. Anything later goes in as an addendum.", 3200);
    await click(sign);
    await caption("Signed.", 2200);
  },

  async templates(page) {
    const { caption, clear, point, click, type, nav } = helpers(page);
    await page.goto(base);
    await click(nav(2));
    await caption("Templates are the headings of your notes. Each kind of visit has its own.", 3200);
    await clear();
    const soap = page.getByRole("button", { name: "Start from SOAP" }).first();
    await point(soap);
    await caption("Start a new one from SOAP, or from blank headings.", 2600);
    await click(soap);
    await clear();
    await page.locator(".tb-heading").first().waitFor();
    await point(page.locator(".builder-meta input.ob-input").first());
    await caption("Name it and say what it's for.", 1200);
    await type(page.locator(".builder-meta input.ob-input").first(), "Dental cleaning");
    await type(page.locator(".builder-meta input.ob-input").nth(1), "Scale and polish under anaesthesia, with any extractions.");
    await page.waitForTimeout(600);
    await clear();
    const add = page.locator(".builder-structure > .ob-button").last();
    await point(add);
    await caption("Add a heading, and give it a name.", 1600);
    await click(add);
    const heading = page.locator(".tb-heading").last();
    await type(heading.locator("input.tb-title"), "Anaesthesia");
    await page.waitForTimeout(600);
    await clear();
    const format = heading.locator("select", { has: page.locator('option[value="medications"]') });
    await point(format);
    await caption("Choose what goes under it, and write plain-language instructions.", 1600);
    await format.selectOption("medications");
    await type(heading.locator("textarea"), "Premedication, induction and maintenance as said.");
    await page.waitForTimeout(1200);
    await clear();
    const tab = page.locator(".builder-preview [role=tab]").nth(1);
    await click(tab);
    await caption(
      "This is exactly what the model is told. Every template keeps the core rules: only what was said.",
      3800,
    );
    await clear();
    const save = page.locator(".head-actions .ob-button[data-variant=primary]");
    await point(save);
    await caption("Save it, then try it on a recorded consult.", 1600);
    await click(save);
    await page.waitForTimeout(1400);
  },

  async documents(page) {
    const h = helpers(page);
    const { caption, clear, captionAt, point, click } = h;
    await page.goto(base);
    await openVisit(page, h, "Mochi Tanaka");
    const make = page.getByRole("button", { name: "Discharge instructions", exact: true });
    await point(make);
    await caption("Under a note: copy it, save a PDF, or make documents for the owner.", 3000);
    await clear();
    await click(make);
    await captionAt("top");
    await caption("They're written from your corrected note; this takes a few seconds.", 600);
    // Written by the note model: give it time.
    const sheet = page.locator(".ob-sheet");
    await sheet.locator(".note .stmt").first().waitFor({ timeout: 180000 });
    await sheet.locator(".board").waitFor({ state: "detached", timeout: 180000 });
    await checkDraft(sheet.locator(".note"));
    await caption("Discharge instructions are in plain language and follow your corrected note.", 3400);
    await sheet.locator(".sheet-body").evaluate((el) => el.scrollBy({ top: 420, behavior: "smooth" }));
    await page.waitForTimeout(1600);
    await clear();
    await point(sheet.locator(".output-row .ob-button").nth(0));
    await caption("Copy it,", 1400);
    await point(sheet.locator(".output-row .ob-button").nth(1));
    await caption("or save a PDF with your clinic's letterhead.", 2600);
    await clear();
    await page.keyboard.press("Escape");
    await sheet.waitFor({ state: "detached" });
    await captionAt("bottom");
    const copy = page.locator(".output-row .ob-button").first();
    await click(copy);
    await caption("Copy note pastes the note into your PIMS, as plain or formatted text (Settings).", 3400);
  },

  async team(page) {
    const { caption, clear, click, type, nav, point } = helpers(page);
    await page.goto(base);
    await click(nav(4));
    const team = page.locator("section.group", { hasText: "Team" }).first();
    await point(team.locator("h2"));
    await caption("Working with a team? Add everyone in Settings. The first member is a veterinarian.", 3400);
    await type(team.locator("input.ob-input").nth(0), "Dr. Demo");
    await type(team.locator("input.ob-input").nth(1), "2468");
    await click(team.locator(".team-add .ob-button").first());
    await clear();
    await page.locator(".user-picker").waitFor({ timeout: 10000 });
    await caption("Now each person picks their name and enters their PIN.");
    await click(page.locator(".user-picker .who").first());
    await type(page.locator(".user-picker input"), "2468");
    await click(page.locator(".user-picker .ob-button[data-variant=primary]"));
    await page.locator(".user-picker").waitFor({ state: "detached" });
    await clear();
    const team2 = page.locator("section.group", { hasText: "Team" }).first();
    await type(team2.locator(".team-add input.ob-input").nth(0), "Sam");
    await team2.locator(".team-add select").selectOption("rvt");
    await type(team2.locator(".team-add input.ob-input").nth(1), "1357");
    await click(team2.locator(".team-add .ob-button").first());
    await caption(
      "Veterinarians sign. Technicians draft and edit. Reception records. Every revision records who did it.",
      4000,
    );
    await clear();
    await point(page.locator("section.group", { hasText: "Encryption" }).locator("h2"));
    await caption(
      "Recordings, transcripts and notes are encrypted on this computer. Keep the recovery key safe.",
      4000,
    );
  },

  async sync(page) {
    const { caption, clear, click, type, point } = helpers(page);
    const settingsRow = (label) => page.locator(".settings-nav-item, .settings-row", { hasText: label }).first();
    // The clinic computer becomes the hub.
    await page.goto(base);
    await caption("Encrypted sync connects your iPhone and other computers through one computer at the clinic: the clinic hub.", 3600);
    await clear();
    await click(page.locator('.nav-item[data-nav="settings"]'));
    await click(settingsRow("Clinic hub"));
    await caption("On the clinic computer, open Settings, Clinic hub. Use a staff-only computer that stays on.", 3400);
    await click(page.locator("label.opt-row", { hasText: "Make this computer the clinic hub" }));
    await page.getByText(/Listening on port/).waitFor({ timeout: 20000 });
    await caption("Turn on Make this computer the clinic hub. Nothing listens on the network until you do.", 3400);
    await click(page.getByRole("button", { name: "New join code" }));
    const joinBox = page.locator(".hub-join");
    await joinBox.waitFor({ timeout: 20000 });
    await point(joinBox.locator(".pair-words"));
    await caption("New join code shows the code, the hub's address and a certificate check. A code works once, for 30 minutes.", 4200);
    const code = (await joinBox.locator(".pair-words").innerText()).trim();
    const address = (await joinBox.locator("p", { hasText: "Address" }).locator(".mono").innerText()).trim();
    const check = (await joinBox.locator("p", { hasText: "Certificate check" }).locator(".mono").innerText()).trim();
    await clear();

    // Another device on the clinic network joins it.
    await page.goto(joiner);
    await caption("Now on the iPhone or another computer, on the same clinic network.", 3000);
    await click(page.locator('.nav-item[data-nav="settings"]'));
    await click(settingsRow("Encrypted sync"));
    await caption("Open Settings, Encrypted sync, and choose Join your clinic hub.", 3000);
    await click(page.getByRole("button", { name: "Join your clinic hub" }));
    await clear();
    await type(page.locator('input[placeholder="ABCD-EFGH"]'), code);
    await caption("Type the join code from the hub.", 2400);
    await type(page.locator("#hub-address"), address);
    await caption("Type the hub's address, or choose Find on this network.", 2800);
    await type(page.locator('input[placeholder="XXXXX-XXXXX"]'), check);
    await caption("Then the certificate check. It proves this is your clinic's hub.", 3000);
    await clear();
    await type(page.locator('input[placeholder="Front desk PC"]'), "Exam room computer");
    await caption("This is your first device: give it a name, then Join the hub.", 3000);
    await click(page.getByRole("button", { name: "Join the hub" }));
    const words = page.locator(".kit-words li");
    await words.first().waitFor({ timeout: 30000 });
    await clear();
    await point(words.first());
    await caption("These 24 words are your Recovery Kit. Print them, or keep them in your clinic's password manager.", 4200);
    const kit = (await words.allInnerTexts()).map((w) => w.trim());
    await click(page.getByRole("button", { name: "I've kept it safe" }));
    await clear();
    await caption("Type the two words OpenBark asks for. Syncing starts after this.", 2800);
    const asked = page.locator("label.ob-field span.label", { hasText: /^Word \d+$/ });
    for (const label of await asked.all()) {
      const n = Number((await label.innerText()).replace(/\D+/g, ""));
      await type(label.locator("xpath=..").locator("input"), kit[n - 1]);
    }
    await click(page.getByRole("button", { name: "Check and start syncing" }));
    await page.getByRole("button", { name: "Done" }).waitFor({ timeout: 30000 });
    await caption("Transcripts, notes and patient details now sync, encrypted. Recordings never leave the device that recorded them.", 4200);
    await click(page.getByRole("button", { name: "Done" }));
    await page.getByText(/Syncing with the clinic hub/).first().waitFor({ timeout: 30000 });
    await point(page.getByText(/Syncing with the clinic hub/).first());
    await caption("Devices sync every 30 seconds while OpenBark is open and they're on the clinic network.", 3400);
    await point(page.getByRole("button", { name: "Add a device" }));
    await caption("Your next device: join with a new code and choose I already use OpenBark on another device. Then type its 6 words here, in Add a device.", 5200);
    await clear();
    await caption("Preview: use test data only until the independent security review is finished.", 3400);
  },
};

// Every topic by default; sync only when its second server is named.
const wanted = process.argv.slice(2).length
  ? process.argv.slice(2)
  : Object.keys(TOPICS).filter((id) => id !== "sync" || process.env.OPENBARK_SYNC_URL);
const unknown = wanted.filter((id) => !TOPICS[id]);
if (unknown.length) throw new Error(`unknown topics: ${unknown.join(", ")}`);

// The folders shown in Settings, Storage must be neutral demo folders (on
// both servers when the sync topic is recorded).
const servers = wanted.includes("sync") ? [base, joiner] : [base];
for (const path of servers.flatMap((server) => ["/api/storage", "/api/status"].map((p) => `${server}${p}`))) {
  const text = await fetch(path).then((r) => r.text());
  const hit = text.match(LEAK);
  if (hit) {
    console.error(`Refusing to record: ${path} shows "${hit[0]}". Start the demo server with neutral folders (docs/guide/README.md).`);
    process.exit(2);
  }
}

// Load the note model before any video, so "Loading the note model" (only the
// first draft after starting OpenBark) never shows. Drafting a template from
// an example loads it and saves nothing.
if (wanted.some((id) => ["record", "review", "documents"].includes(id))) {
  const res = await fetch(`${base}/api/templates/from-example`, {
    method: "POST",
    headers: { "content-type": "application/json", "X-OpenBark": "1" },
    body: JSON.stringify({ llm: LLM, text: "Subjective\nObjective\nAssessment\nPlan" }),
  });
  if (!res.ok) console.warn(`Couldn't load the note model first (${res.status}); the first draft will show it loading.`);
}

// Runs in the page: report any visible text, field value or label that would show private paths.
function leakWatch(source) {
  const re = new RegExp(source, "i");
  const scan = () => {
    if (!document.body) return;
    const parts = [document.title, document.body.innerText];
    for (const el of document.querySelectorAll("input, textarea, select")) parts.push(el.value);
    for (const el of document.querySelectorAll("[title], [aria-label], [placeholder], [aria-valuetext]"))
      parts.push(el.getAttribute("title"), el.getAttribute("aria-label"), el.getAttribute("placeholder"), el.getAttribute("aria-valuetext"));
    for (const text of parts) {
      const m = text && text.match(re);
      if (m) {
        const at = text.indexOf(m[0]);
        window.__helpLeak?.(text.slice(Math.max(0, at - 40), at + 60));
      }
    }
  };
  setInterval(scan, 400);
  addEventListener("DOMContentLoaded", scan);
}

const failedDir = resolve("../../target/help-video-failed");
const browser = await chromium.launch({
  args: [
    "--use-fake-ui-for-media-stream",
    "--use-fake-device-for-media-stream",
    `--use-file-for-fake-audio-capture=${fakeMic}`,
  ],
});
let failures = 0;
for (const id of wanted) {
  rmSync(tmpDir, { recursive: true, force: true });
  const ctx = await browser.newContext({
    viewport: size,
    colorScheme: theme,
    recordVideo: { dir: tmpDir, size },
    permissions: ["microphone", "clipboard-read", "clipboard-write"],
    acceptDownloads: true,
  });
  // The demo server takes its throwaway key from OPENBARK_DATA_KEY, because
  // its own keychain entry would be this computer's real key. Show the
  // Encryption card as an installed app shows it (key in the keychain).
  await ctx.route("**/api/status", async (route) => {
    const res = await route.fetch();
    const body = await res.json().catch(() => null);
    if (body?.encryption?.source === "environment") body.encryption.source = "keychain";
    await route.fulfill(body ? { response: res, json: body } : { response: res });
  });
  const leaks = new Set();
  await ctx.exposeBinding("__helpLeak", (_source, text) => leaks.add(text));
  const fresh = id === "getting-started";
  await ctx.addInitScript(
    ({ seed, fresh, theme }) => {
      try {
        localStorage.setItem("openbark.theme", theme);
        if (!fresh && !localStorage.getItem("openbark.settings"))
          localStorage.setItem("openbark.settings", JSON.stringify(seed));
      } catch {
        // Storage can be unavailable; the theme still follows colorScheme.
      }
    },
    { seed: SEEDED, fresh, theme },
  );
  await ctx.addInitScript(overlay, theme);
  await ctx.addInitScript(leakWatch, LEAK.source);
  const page = await ctx.newPage();
  const errors = [];
  page.on("pageerror", (e) => errors.push(String(e)));
  try {
    await TOPICS[id](page);
    await page.waitForTimeout(500);
  } catch (e) {
    errors.push(String(e).split("\n")[0]);
  }
  const video = page.video();
  await ctx.close();
  const path = await video.path();
  const name = `${id}${suffix}.webm`;
  if (leaks.size) errors.push(`private text on screen: ${[...leaks].slice(0, 3).join(" / ")}`);
  if (errors.length) {
    // Never ship a broken or leaking video: keep it aside to look at.
    failures++;
    mkdirSync(failedDir, { recursive: true });
    renameSync(path, `${failedDir}/${name}`);
    console.log(`${name}: FAILED ${errors.join(" | ")}`);
  } else {
    renameSync(path, `${outDir}/${name}`);
    console.log(`${name}: ok`);
  }
}
await browser.close();
rmSync(tmpDir, { recursive: true, force: true });
if (failures) process.exit(1);
