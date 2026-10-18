# Design implementation status

Tracks the build against `DESIGN.md`. Updated 2026-10-17. Screenshots: `apps/desktop/e2e/screenshots.mjs` captures desktop light/dark, laptop and mobile from web mode, and fails on console errors or horizontal scroll.

## Done

- §10 tokens implemented exactly (`packages/ui/src/tokens.css`). No raw hex outside the token file. One extra pair is needed for contrast: `--on-accent` and `--on-record`.
- Geist Sans and Mono are vendored as variable woff2 files under OFL-1.1, with no CDN. Phosphor icons. No emoji in the UI, code or copy; the Markdown export uses `[check against recording]` instead of a glyph.
- §4 frame: a 240px sidebar that becomes a 56px rail below 1200px and a bottom tab bar on mobile. Review is a 2fr/3fr split above 1024px and switches to Transcript/Note tabs below. Uses `100dvh`.
- §5 Record button: 64px (72px on touch); Pulse Red idle; while recording, a ring with a live waveform and the 2 s breathing opacity (the only perpetual animation). Elapsed time is shown in mono. Space toggles recording.
- §5 Note: the evidence link (hovering or focusing a statement lights its transcript lines; clicking plays them), "Not discussed" in Graphite italic, a hard-fail Clay callout that blocks **Sign note**, an Amber dotted underline for citation problems, a 2px progress rule while drafting, and a per-section Copy on hover.
- §5 Transcript: a hanging speaker label, mono timestamps on the right, click to seek, follow-live with **Jump to live**.
- §5 Model manager: catalog rows (not cards), sizes in mono, licence chips, Fits/Tight/Won't fit shown as a dot plus a word, determinate download progress, and a Hugging Face browser in a right-hand sheet. Permissive licences are shown by default; restrictive ones need explicit acceptance.
- §5 Tables, chips, toasts, skeletons and sheets follow the specs.
- §5 Template builder: two panes, a structure list with `DotsSixVertical` drag handles (plus Move up/down buttons for the keyboard) and plain-textarea instructions, and a live preview with a "What the model is told" tab. Empty state: "No team templates yet. Start from SOAP, or start a new one from blank headings."
- §5 Note: **Ask** on every heading and subheading opens an inline re-prompt panel (reuse chips, a "Save as a standard" box, a progress rule). A redrafted heading shows its request beneath the title.
- §8 copy uses sentence case, outcome verbs and specific errors. §9: keyboard paths, visible focus rings, status shown as text plus a dot, a live region for recording and draft events, and `prefers-reduced-motion` respected.

## Not yet

- Recorder left column (patient context from the PIMS or entered manually): arrives with patient context in prompts.
- Note editor: Dictate correction. Copy, Ask (redo with an instruction), Edit (inline heading editor; vet lines marked "Edited by") and signed addenda exist.
- Draggable Review split that remembers its ratio per device.
- Renaming speakers (DVM, Client, Tech) and low-confidence word underlines.
- `[` and `]` keys to jump between unsupported sentences.
- Template builder conditionals ("Show when species is feline") and building a template from an example note.
- Onboarding (3 screens), dental chart, encounter filters, Hub admin, browser extension popup.
- shadcn/ui primitives: plain accessible elements are used for now. Revisit when menus, comboboxes and dialogs grow.
