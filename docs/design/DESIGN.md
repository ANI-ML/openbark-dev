<!--
Provided by the product owner on 2026-10-17 as the design source of truth.
"OpenScribe" below is the earlier working name; the product is OpenBark.
Everything else is kept verbatim. Implementation status: docs/design/STATUS.md.
-->

# Design System: OpenScribe

> Single source of truth for every screen in OpenScribe (desktop, web, mobile, browser extension, Clinic Hub admin). Claude Code: read this before touching `packages/ui`, and run the pre-flight checklist in §12 before closing any UI task. Where this file and a component library default disagree, this file wins.

---

## 1. Visual theme and atmosphere

OpenScribe is a clinical instrument, not a SaaS dashboard. The feel to aim for is **a quiet, well-lit exam room**: calm neutral surfaces, one restrained accent, generous but not precious spacing, and type that reads instantly from arm's length on a laptop balanced on a treatment table.

- **Density:** 6 of 10 — a daily-use app. Denser than a marketing page, airier than a PIMS. Review and Hub-admin screens may rise to 7 (where all numbers go monospace); onboarding stays at 4.
- **Variance:** 3 of 10 — predictable and symmetric. Clinicians learn one layout and never think about it again. The only deliberate asymmetry is the Review screen's 40/60 transcript↔note split.
- **Motion:** 3 of 10 — restrained. Motion exists to answer an action (recording started, section accepted, note sent) and to show live state (audio level, streaming text). Nothing moves on its own for decoration.

**The one memorable thing:** the evidence link. Hover or tap any sentence in a generated note and the exact transcript span that supports it lights up and the audio scrubs to that moment. Everything else on the screen is disciplined and quiet so that this interaction is the thing people remember and trust.

What it must never feel like: a chat app, a developer tool, a "neon AI" product, or a landing page with cards.

---

## 2. Color palette and roles

One palette, cool-neutral, used identically in every app. No warm/cool gray mixing. Never pure black or pure white text-on-background pairs at full contrast in dark mode (use the tokens).

### Light (default)

| Token | Name | Hex | Role |
|---|---|---|---|
| `--bg` | Exam Room | `#F6F7F8` | App background |
| `--surface` | Paper | `#FFFFFF` | Panels, editors, sheets |
| `--surface-2` | Chart Paper | `#EEF0F2` | Inset areas: transcript gutter, code/mono blocks, kbd |
| `--border` | Hairline | `#E1E4E8` | All 1px structural lines |
| `--border-strong` | Rule | `#C9CED5` | Focused/selected outlines, table header rule |
| `--ink` | Ink | `#1B1F24` | Primary text |
| `--ink-2` | Graphite | `#5B6471` | Secondary text, labels, metadata |
| `--ink-3` | Mist | `#8A93A0` | Placeholder, disabled, timestamps |
| `--accent` | Scrub Teal | `#2F6F6A` | Primary actions, active nav, focus ring, links, evidence highlight |
| `--accent-soft` | Scrub Wash | `#E3F0EE` | Evidence highlight fill, selected row, active tab background |
| `--record` | Pulse Red | `#B3443C` | Recording state only. Never used for errors or destructive buttons |
| `--danger` | Clay | `#A8453A` | Destructive actions, hard errors (hallucinated vitals, failed write-back) |
| `--warn` | Amber Ink | `#9A6414` | "Not discussed", unsupported claim, low-confidence transcript word |
| `--ok` | Fern | `#3C7A4F` | Signed, synced, model ready |

### Dark (auto via `prefers-color-scheme`, user-overridable; this is the mode used in dim imaging rooms)

| Token | Hex | Role |
|---|---|---|
| `--bg` | `#121417` | App background |
| `--surface` | `#1A1D21` | Panels |
| `--surface-2` | `#22262B` | Inset areas |
| `--border` | `#2C3137` | Hairlines |
| `--border-strong` | `#3C434B` | Focus/selected |
| `--ink` | `#E8EAED` | Primary text |
| `--ink-2` | `#A4ACB6` | Secondary |
| `--ink-3` | `#6F7883` | Placeholder |
| `--accent` | `#6FB3AC` | Accent (lightened for contrast) |
| `--accent-soft` | `#1F3634` | Evidence fill |
| `--record` | `#D9655C` | Recording |
| `--danger` | `#D46A5E` | Destructive |
| `--warn` | `#D29A3A` | Caution |
| `--ok` | `#6FB07F` | Success |

Rules:
- Exactly one accent. Its saturation is ~40%; do not "brighten" it.
- Color carries meaning, so it is scarce: a screen should be ≥ 90% neutral by area. Status colors appear as 8px dots, 1px left rules, text, or soft fills — never as large blocks.
- Species/patient chips are neutral (Chart Paper fill, Ink text). No per-species color coding.
- Shadows are tinted to the background (`rgba(27,31,36,0.06)` light / `rgba(0,0,0,0.4)` dark) and used only on floating layers (menus, sheets, toasts). Panels sit on hairlines, not shadows.

---

## 3. Typography

Software UI: sans-serif only. No serif anywhere in the app.

- **UI and body:** Geist Sans. Fallback stack: `"Geist Sans", "SF Pro Text", "Segoe UI Variable", system-ui, sans-serif`. Bundle the font; never load from a CDN at runtime (offline-first).
- **Mono:** Geist Mono for timestamps, vitals, doses, weights, model sizes, tokens/s, keyboard shortcuts, IDs. Use tabular numerals (`font-variant-numeric: tabular-nums`) everywhere numbers align.
- **Note editor text:** Geist Sans at 16px/1.6, max line length 72ch. The note is the product; it gets the most comfortable reading measure on the screen.

Scale (rem; base 16px):

| Role | Size / line | Weight | Tracking |
|---|---|---|---|
| Display (onboarding titles only) | 2.0 / 1.15 | 600 | -0.02em |
| H1 (screen title) | 1.375 / 1.25 | 600 | -0.015em |
| H2 (panel title, note section header) | 1.0625 / 1.35 | 600 | -0.01em |
| Body | 1.0 / 1.6 | 400 | 0 |
| Body small (metadata, helper) | 0.875 / 1.5 | 400 | 0 |
| Label (form labels, table headers) | 0.8125 / 1.4 | 500 | 0 |
| Mono small | 0.8125 / 1.4 | 400 | 0 |

Rules:
- Hierarchy comes from weight and color (Ink vs Graphite), not size jumps. Headings never exceed 2rem anywhere in the app.
- No all-caps labels. No tracked-out eyebrows above headings. Sentence case everywhere, including buttons and nav.
- Never italicize or color a single word in a heading for emphasis.
- Body text minimum 14px on mobile; the note editor never drops below 16px.
- Transcript text: Body size, Graphite color, with the speaker label in Label weight 500 as a hanging indent — speakers are read by position, not by color.

---

## 4. Spacing, grid, radius

- **Spacing scale (px):** 4, 8, 12, 16, 24, 32, 48, 64. Nothing else. Panel padding is 24 on desktop, 16 on mobile.
- **Radius:** 6px for controls (buttons, inputs, chips), 10px for panels and sheets, 2px for highlight marks inside text, 9999px only for status dots and the record button. One radius per hierarchy level; never mix 6 and 8 on sibling elements.
- **Grid:** CSS Grid for all screen layouts. Desktop app frame: `grid-template-columns: 240px 1fr` (sidebar + content); the sidebar collapses to a 56px icon rail below 1200px and to a bottom tab bar on mobile. Content max-width 1440px, centered.
- **Review screen:** `grid-template-columns: 2fr 3fr` (transcript | note) above 1024px; stacked tabs below. The split is draggable; the ratio persists per device.
- Containers are hairline-bordered regions or plain whitespace groupings. Cards appear only when something floats above the page (model card in the catalog, a toast). No "card kit" grids.
- Full-height views use `min-height: 100dvh`, never `100vh`.

---

## 5. Components

Build on shadcn/ui primitives for accessibility, then restyle every one to these specs — the stock look is not acceptable. Icons: Phosphor, regular weight, 1.5px stroke, 20px in controls and 16px inline; no emoji anywhere, including empty states and toasts.

**Buttons**
- Primary: Scrub Teal fill, white text, 6px radius, 36px tall (44px on touch), weight 500. Hover darkens 6%; active `translateY(1px)`; focus ring 2px Scrub Teal offset 2px. No shadows, no glows.
- Secondary: Paper fill, Hairline border, Ink text. Hover: Chart Paper fill.
- Ghost: text only, Graphite → Ink on hover. Used for toolbar actions.
- Destructive: Ghost by default; becomes Clay fill only inside a confirm dialog.
- One primary button per view. Labels say what happens: "Start recording", "Sign note", "Send to ezyVet", "Download model (2.4 GB)".

**The Record button**
- The single circular element in the app: 64px (desktop) / 72px (mobile), Pulse Red fill when idle-ready, white icon. While recording, the fill becomes a Pulse Red ring around a live waveform; a soft 2s breathing opacity on the ring (0.75→1.0) is the only perpetual animation permitted in the app, because it communicates "audio is being captured". Elapsed time in Geist Mono beneath it. Pause is a secondary button beside it, never a mode toggle hidden in a menu.

**Inputs**
- Label above (Label style), input 36px, Paper fill, Hairline border, focus → Scrub Teal border + ring. Helper text below in Mist; error text below in Clay with the specific fix ("Token needs the `read` scope"). No floating labels, no inside-the-box labels.
- Search fields show the shortcut as a `<kbd>` on the right (`⌘K`), Chart Paper fill, Hairline border, Geist Mono.

**Note editor**
- Paper surface, 24px padding, 72ch measure. Section headers are H2 with a hairline rule beneath; each section has a right-aligned ghost toolbar on hover/focus: Accept, Redo with instruction, Dictate correction, Collapse.
- **Evidence marks:** when the pointer rests on a sentence, it gets a 2px-radius Scrub Wash background and the supporting transcript span gets the same fill plus a 2px Scrub Teal left rule; the waveform shows a Scrub Teal range. Tap on mobile does the same and offers "Play".
- **Unsupported sentence:** Amber Ink dotted underline with a popover: "No matching transcript. Keep, edit, or remove?"
- **Not discussed:** rendered as Graphite italic text "Not discussed" in place of the section body. Never silently omitted, never invented.
- **Hard-fail content** (a vital sign or dose with no transcript support) is wrapped in a Clay left-ruled callout and the Sign button is disabled until resolved.
- Streaming: tokens appear in place with no typewriter cursor effect; a thin 2px Scrub Teal progress rule sits at the top of the panel while generation runs.

**Transcript pane**
- Speaker turns as blocks with a hanging speaker label (DVM / Client / Tech, user-renamable), timestamps in Geist Mono Mist at the right edge, low-confidence words with a Mist dotted underline. Click any word to seek audio. Auto-scroll follows live transcription until the user scrolls up; a "Jump to live" ghost button appears when detached.

**Model manager**
- Catalog rows (not cards): name, publisher, size in Geist Mono, license chip, context length, and a hardware-fit indicator: three words only — "Fits", "Tight", "Won't fit" — as text with a Fern / Amber Ink / Mist dot. Download shows a determinate progress rule and the remaining size; never an indeterminate spinner.
- Slots (Transcription, Notes, Fast edits, Speakers, Search) are a vertical list with the active model name; changing a slot opens a sheet, not a new page.

**Template builder**
- Two panes: structure list on the left (drag handles, Phosphor `DotsSixVertical`), live preview on the right rendered against a sample transcript. Section instructions are plain textareas. Conditionals read as sentences: "Show when species is feline".

**Dental chart**
- SVG tooth chart (Triadan numbering) in Ink hairlines on Paper; findings as neutral glyph overlays with a single Amber Ink or Clay dot for pathology; selected tooth gets a Scrub Teal outline. Legend is text, not a color key.

**Tables**
- Hairline rules between rows only; header in Label style Graphite with a Rule-colored bottom border; numeric columns right-aligned in Geist Mono; row hover Chart Paper. No zebra striping, no vertical lines.

**Chips / status**
- Neutral chips: Chart Paper fill, Ink text, 6px radius, 24px tall. Status chips use an 8px dot plus text ("Signed", "Synced to Instinct", "Model ready") — the dot is colored, the text is Ink.

**Loading**
- Skeletons that match the final layout dimensions with a slow (1.6s) left-to-right shimmer in Chart Paper. The only spinner in the app is the 16px inline one inside a button that was just pressed.

**Empty states**
- A short instruction and the one primary action. Recorder with no patient: "Pick a patient or start without one." Templates: "No custom templates yet. Start from SOAP or build from an example note." No illustrations of sad folders, no emoji.

**Errors**
- Inline, specific, actionable, in the interface's voice: "ezyVet rejected the note: consultation 48213 is locked. Unlock it in ezyVet, then send again." Never "Something went wrong." Never an apology.

**Dialogs and sheets**
- Confirmations only for irreversible or outbound actions (delete audio, send to PIMS, send to client). Sheets slide from the right on desktop and the bottom on mobile; 10px radius, hairline border, tinted shadow, backdrop `rgba(27,31,36,0.32)`.

**Toasts**
- Bottom-right (desktop) / top (mobile), Paper surface, hairline, one line, same verb as the action ("Sent to ezyVet"), auto-dismiss 4s, with an undo where possible.

---

## 6. Screen blueprints

**Onboarding (3 screens, Display type allowed here only)**
1. "Where should OpenScribe run?" — This device / Connect to a clinic hub. Hardware probe runs silently; result shown as one sentence: "MacBook Pro, 36 GB unified memory — Gemma 4 12B and Whisper turbo recommended."
2. "Download your models" — list with sizes and one progress rule; time estimate in plain words.
3. "Recording consent" — region picker and the consent script preview. Done.

**Recorder (home)**
- Left: patient context (name, species, breed, weight, age; from PIMS or manual). Center: Record button, waveform, elapsed, live transcript beneath. Right rail: template picker, multi-pet toggle, language, bookmarks. Keyboard: Space toggles record when the recorder has focus.

**Review**
- 2fr/3fr split as above; a slim top bar with patient, template, model used (Geist Mono), and the Sign button at the right. Below the note: output row — Copy formatted, PDF, Client summary, Send to [PIMS].

**Encounters list**
- Table (not cards): patient, time, template, status chip, synced-to. Filters as chips above.

**Model manager / Settings**
- Left settings nav (list), right content with hairline-separated groups. Advanced model knobs live under a disclosure, never on the primary path.

**Clinic Hub admin**
- Density 7: hairline-divided metric rows (not metric cards), all numbers Geist Mono; GPU utilization as a thin inline bar; queue as a table.

**Browser extension popup**
- 360px wide, Paper surface; shows detected PIMS and patient, the pending note sections with checkboxes, and one primary button "Insert into [PIMS]".

**Mobile**
- Bottom tab bar: Record, Encounters, Templates, Settings. The Record screen is one-handed: Record button in the lower third, transcript above. Review is tabbed (Transcript | Note) with a thumb-reachable Sign action. All targets ≥ 44px. No horizontal scrolling anywhere.

---

## 7. Motion and interaction

- Curves: `cubic-bezier(0.16, 1, 0.3, 1)` at 180–240ms for UI state changes; springs (stiffness 180, damping 24) only for sheets and the Record button's size change.
- Motion answers actions: section accept collapses with a 200ms height/opacity transition; a sent note slides its status chip in; a rejected sentence fades to Mist before removal.
- Live state: waveform and streaming text are driven by data, not easing. No typewriter effects, no cursors.
- No scroll-triggered reveals, no parallax, no hover lifts on panels, no staggered cascades on lists. Lists mount instantly.
- Animate only `transform` and `opacity`. `prefers-reduced-motion` disables the breathing ring (static ring remains) and all slides.
- Isolate the waveform and streaming components so re-renders never touch the app shell.

---

## 8. Writing in the interface

- Plain veterinary English, sentence case, active voice. The user manages "models", "templates", "encounters", and "connections" — never "backends", "endpoints", "pipelines" on the primary path.
- Buttons name the outcome and keep the name through the flow: "Sign note" → toast "Note signed".
- Errors state what happened and what to do next. Empty states invite the first action.
- Numbers are specific: "2.4 GB", "11 s to first draft", "3 unsupported sentences".
- Banned words: elevate, seamless, unleash, next-gen, supercharge, effortless, magic, AI-powered (as an adjective in UI copy).

---

## 9. Accessibility floor (non-negotiable)

- WCAG 2.2 AA contrast for all text and UI states in both modes (tokens above are tuned for this; re-check after any change).
- Full keyboard operation: recorder, editor, section toolbar, evidence navigation (`[` / `]` move between unsupported sentences), and PIMS transfer.
- Visible focus rings (2px Scrub Teal, 2px offset) on every interactive element; never remove outlines.
- Status is never color-only: dots are always accompanied by text.
- Screen reader: live region announces "Recording started/paused", "Draft ready", "Note signed"; transcript and note are proper headings/lists.
- Respect `prefers-reduced-motion` and `prefers-color-scheme`; allow explicit override in Settings.

---

## 10. Tokens (implement exactly)

```css
:root {
  --bg: #F6F7F8; --surface: #FFFFFF; --surface-2: #EEF0F2;
  --border: #E1E4E8; --border-strong: #C9CED5;
  --ink: #1B1F24; --ink-2: #5B6471; --ink-3: #8A93A0;
  --accent: #2F6F6A; --accent-soft: #E3F0EE;
  --record: #B3443C; --danger: #A8453A; --warn: #9A6414; --ok: #3C7A4F;
  --shadow: 0 8px 24px -12px rgba(27,31,36,0.18), 0 1px 2px rgba(27,31,36,0.06);
  --radius-control: 6px; --radius-panel: 10px; --radius-mark: 2px;
  --font-sans: "Geist Sans", "SF Pro Text", "Segoe UI Variable", system-ui, sans-serif;
  --font-mono: "Geist Mono", "SF Mono", ui-monospace, monospace;
  --ease: cubic-bezier(0.16, 1, 0.3, 1); --dur-fast: 180ms; --dur: 240ms;
}
@media (prefers-color-scheme: dark) { :root:not([data-theme="light"]) {
  --bg: #121417; --surface: #1A1D21; --surface-2: #22262B;
  --border: #2C3137; --border-strong: #3C434B;
  --ink: #E8EAED; --ink-2: #A4ACB6; --ink-3: #6F7883;
  --accent: #6FB3AC; --accent-soft: #1F3634;
  --record: #D9655C; --danger: #D46A5E; --warn: #D29A3A; --ok: #6FB07F;
  --shadow: 0 8px 24px -12px rgba(0,0,0,0.6), 0 1px 2px rgba(0,0,0,0.4);
}}
:root[data-theme="dark"] { /* same values as above */ }
body { background: var(--bg); color: var(--ink); font-family: var(--font-sans); }
```

Tailwind: map these to `theme.extend.colors` as `bg`, `surface`, `ink`, `accent`, etc. using `rgb(var(--x) / <alpha-value>)` or direct var references; do not use Tailwind's default `gray`/`slate`/`zinc` palettes anywhere in the app.

---

## 11. Anti-patterns (never)

- No emoji in UI, code, copy, or alt text.
- No Inter, Roboto, Open Sans; no serif anywhere in the app.
- No pure `#000000` or `#FFFFFF` text/background pairs in dark mode.
- No purple/blue neon, gradients on text, outer glows, glassmorphism panels, mesh-gradient backgrounds.
- No three-equal-cards rows; no card kit with a uniform radius and gray shadow under everything.
- No all-caps tracked labels, no eyebrows, no middle-dot metadata strings, no "→" on buttons.
- No centered marketing-style hero inside the app; no "Scroll to explore".
- No typewriter cursors, no perpetual motion except the recording ring, no hover lifts, no stagger reveals.
- No circular indeterminate spinners for model downloads or note generation.
- No warning/error expressed by color alone; no per-species color coding.
- No placeholder names like "Fluffy / John Doe / Acme Vet"; sample data uses plausible patients ("Maple, 7y FS Golden Retriever, 31.4 kg", client "R. Okonkwo").
- No vendor-style claims in copy ("99.8% accurate"); show the user's own measured numbers instead.
- No runtime font or icon CDN loads; everything ships in the bundle.

---

## 12. Pre-flight checklist for every UI PR

- [ ] Uses only tokens from §10; no raw hex in components.
- [ ] One primary button per view; labels are outcome verbs in sentence case.
- [ ] Panels sit on hairlines; shadows only on floating layers.
- [ ] Numbers that align use Geist Mono with tabular figures.
- [ ] Evidence marks, unsupported-sentence underline, and "Not discussed" rendering follow §5.
- [ ] Loading = layout-matched skeleton or determinate progress; empty and error states written per §8.
- [ ] Keyboard path complete; focus ring visible; status has text, not just color.
- [ ] Works at 360px wide with no horizontal scroll; targets ≥ 44px on touch.
- [ ] Dark mode checked; `prefers-reduced-motion` checked.
- [ ] Screenshot attached to the PR for desktop light, desktop dark, and mobile.
