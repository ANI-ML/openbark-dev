# OpenBark — Design Language

> Open-source, local-first veterinary AI scribe. Runs on the clinic's own hardware. Listens in the exam room, writes the record.
> This document is the single source of truth for how OpenBark looks and feels. Follow it for every surface: web app, mobile app, docs site, README, marketing page, CLI output.

---

## 1. Brand idea

**The paw is the microphone.**

OpenBark does one thing: it hears a veterinary conversation and turns it into a clinical note. The mark says exactly that — a microphone whose capsule is a paw pad, with three toes above it. No speech bubbles, no sparkles, no "AI" iconography. The product is a tool, not a mascot.

Three words that should describe every screen:

| Word | What it means in practice |
|---|---|
| **Quiet** | The UI gets out of the way of the clinician. Minimal chrome, generous whitespace, one accent colour used sparingly. |
| **Trustworthy** | Local-first is a promise. Nothing flashy, nothing that looks like it phones home. Clear states, honest copy, no dark patterns. |
| **Warm** | It's a vet tool. Amber accent, soft radii, friendly lowercase wordmark. Clinical, not cold. |

What OpenBark is **not**: a chat app, a dashboard-first product, an "AI assistant" with a personality. Don't add a mascot, don't anthropomorphise the model, don't use robot/brain/sparkle icons.

---

## 2. Logo

### 2.1 The mark

Geometry lives on a 100 × 100 grid. Reproduce exactly from `logos/openbark-mark.svg`; never redraw by eye.

```
Toes      3 × circle r=7.5  at (30,20) (50,12) (70,20)      ink
Capsule   rect 30×42, rx=15 at (35,28)                        accent
Grille    2 × rect 16×4, rx=2 at (42,44) (42,52)              surface (white on light)
Cradle    arc  M24 54 A26 26 0 0 0 76 54, stroke 7, round     ink
Stem      rect 7×10,  rx=3.5 at (46.5,78)                     ink
Base      rect 30×7,  rx=3.5 at (35,87)                       ink
```

Rules:
- The capsule is the **only** accent-coloured element. Toes, cradle, stem and base are always ink (or white on dark).
- Grille slots are the surface colour showing through — on a dark tile they are ink, on the accent tile they are ink, on light they are white.
- Below 24 px rendered size, drop the grille (see `openbark-favicon.svg`).
- Never rotate, skew, add gradients, outlines, drop shadows, or a second colour to the toes.
- Clear space around the mark = the height of one toe (15 units on the 100 grid, i.e. 15% of the mark's size) on all sides.

### 2.2 The wordmark

`openbark` — always lowercase, always one word.

- Typeface: **Manrope** (Google Fonts, OFL).
- `open` at weight **500**, `bark` at weight **800**. Same colour, same size, no space, no colour change between the halves.
- Letter-spacing: **−0.03em**.
- Colour: ink `#182230` on light; white `#FFFFFF` on dark.
- Never use the wordmark in the accent colour.
- Never set it in title case ("OpenBark") inside the logo. In running prose, "OpenBark" (capital O, capital B) is correct.

### 2.3 Lockups

| Lockup | File | Use |
|---|---|---|
| Mark only | `openbark-mark.svg` | App icon, avatars, loading states, anywhere ≤ 48 px or where the name is already visible |
| Mark on dark | `openbark-mark-on-dark.svg` | Dark surfaces |
| Mono mark | `openbark-mark-mono.svg` | Inherits `currentColor`. Use inline in UI, README badges, print, single-colour contexts |
| Horizontal lockup | `openbark-lockup-horizontal.svg` | Headers, nav bars, README hero, docs. Mark height = 1.2× wordmark cap height; gap = 0.3× wordmark size |
| App icon — dark (primary) | `openbark-app-icon-dark.svg` | iOS / Android / PWA icon, GitHub org avatar |
| App icon — light | `openbark-app-icon-light.svg` | Light-mode contexts, macOS dock alt |
| App icon — accent | `openbark-app-icon-accent.svg` | Marketing only; never the default install icon |
| Favicon | `openbark-favicon.svg` | 16–32 px. Simplified, no grille |

App-icon tiles use a corner radius of **22.7%** of the tile (116 / 512) and place the mark at **68%** of tile width, centred.

The horizontal lockup SVG uses live text. Load Manrope on any page that renders it, or convert to outlines (`inkscape --export-text-to-path`) before using it where fonts can't load (email, Open Graph images, PDFs).

### 2.4 Don'ts

- Don't put the mark inside a circle.
- Don't place the lockup on a photo or busy background without a solid tile behind it.
- Don't pair the mark with a second icon (no "paw + waveform", no "paw + document").
- Don't recolour the toes.
- Don't stretch to fit a space — scale proportionally and crop the container instead.

---

## 3. Colour

### 3.1 Core palette

| Token | Hex | Role |
|---|---|---|
| `--ob-ink` | `#182230` | Primary text, mark, dark tiles, primary buttons |
| `--ob-accent` | `#E27A2B` | Capsule, recording state, primary actions, focus ring, links on dark |
| `--ob-surface` | `#FFFFFF` | Page background (light) |
| `--ob-surface-2` | `#F2F4F7` | Cards, light app-icon tile, input backgrounds, zebra rows |
| `--ob-surface-3` | `#E5E8EE` | Borders, dividers, disabled fills |
| `--ob-text-2` | `#4B5563` | Secondary text, descriptions |
| `--ob-text-3` | `#6B7280` | Captions, timestamps, placeholders (use only ≥ 12 px) |

### 3.2 Accent scale

Generated from `#E27A2B`. Use the scale, not ad-hoc tints.

| Token | Hex | Use |
|---|---|---|
| `--ob-accent-50` | `#FDF4EC` | Accent-tinted backgrounds, selected rows |
| `--ob-accent-100` | `#FAE3CC` | Hover on tinted backgrounds |
| `--ob-accent-300` | `#F0B07A` | Decorative, charts |
| `--ob-accent-500` | `#E27A2B` | **The** accent |
| `--ob-accent-600` | `#C9661F` | Hover/active on accent buttons; accent text on white (passes 4.5:1 only at 600+) |
| `--ob-accent-700` | `#A65218` | Accent text on light surfaces at small sizes |

### 3.3 Semantic

| Token | Hex | Use |
|---|---|---|
| `--ob-recording` | `#E27A2B` | Live recording indicator (pulsing dot). Same as accent — recording *is* the brand moment |
| `--ob-success` | `#1F8A5B` | Saved, synced, model loaded |
| `--ob-warning` | `#B7791F` | Low disk, model not downloaded, unsaved |
| `--ob-danger` | `#C2410C` | Destructive actions, failed transcription. Warm red so it sits with the amber, never pure `#FF0000` |
| `--ob-info` | `#2F6FE4` | Neutral notices |

### 3.4 Dark mode

| Token | Hex |
|---|---|
| `--ob-surface` | `#0F151D` |
| `--ob-surface-2` | `#182230` |
| `--ob-surface-3` | `#243040` |
| `--ob-ink` (text) | `#F5F7FA` |
| `--ob-text-2` | `#B4BCC8` |
| `--ob-text-3` | `#8A94A3` |
| `--ob-accent` | `#E98B45` (one step lighter so it holds contrast on ink) |

Dark mode is a first-class target — exam rooms are often dim and the mobile app will be used at 11 pm. Design both from the start; don't bolt dark on later.

### 3.5 Colour rules

- One accent per screen. If two things are amber, one of them is wrong — unless one is the recording indicator.
- Text on white: ink or `--ob-text-2`. Never accent for body text.
- Buttons: primary = ink fill, white text. Accent fill is reserved for the **Record** action and nothing else.
- Charts/waveforms: ink bars, with the "current" or "active" bar in accent (same logic as the mark).
- No gradients anywhere. No glassmorphism. No coloured drop shadows.

---

## 4. Typography

**Manrope** for everything. Load weights 400, 500, 600, 800.

```html
<link href="https://fonts.googleapis.com/css2?family=Manrope:wght@400;500;600;800&display=swap" rel="stylesheet">
```

Fallback stack: `'Manrope', system-ui, -apple-system, 'Segoe UI', sans-serif`. Self-host the woff2 in the app bundle — OpenBark is local-first and must render identically offline.

Monospace (transcripts, timestamps, code, model IDs): `'JetBrains Mono', ui-monospace, 'SF Mono', Menlo, monospace`.

### 4.1 Scale

| Token | Size / line | Weight | Tracking | Use |
|---|---|---|---|---|
| `display` | 48 / 52 | 800 | −0.03em | Marketing hero only |
| `h1` | 32 / 38 | 800 | −0.02em | Page title |
| `h2` | 24 / 30 | 800 | −0.02em | Section title |
| `h3` | 18 / 24 | 600 | −0.01em | Card title, SOAP section header |
| `body` | 15 / 24 | 400 | 0 | Default UI text |
| `body-strong` | 15 / 24 | 600 | 0 | Labels, emphasised body |
| `small` | 13 / 18 | 400 | 0 | Secondary text |
| `caption` | 12 / 16 | 500 | +0.01em | Timestamps, metadata; min size |
| `transcript` | 15 / 26 | 400 (mono) | 0 | Live transcript and note body |

Rules:
- Headlines are weight 800, never 700. The 500 ↔ 800 contrast from the wordmark is the typographic signature; carry it through (labels 500, values 800 in stat tiles, for instance).
- Sentence case everywhere. No ALL CAPS labels, no Title Case Buttons.
- Clinical notes themselves (the SOAP output) render in `transcript` style so they visibly read as *the record*, distinct from UI text.

---

## 5. Shape, space, elevation

### 5.1 Radius

| Token | px | Use |
|---|---|---|
| `--r-sm` | 6 | Chips, tags, inline code |
| `--r-md` | 10 | Buttons, inputs, menu items |
| `--r-lg` | 14 | Cards, panels, toasts |
| `--r-xl` | 22 | Modals, sheets, the Record button |
| `--r-full` | 9999 | Avatars, status dots, pills |

The mark's capsule (rx = half width) sets the tone: soft, fully-rounded ends on anything pill-shaped; moderate radii on rectangles.

### 5.2 Spacing

4-px base. Use the scale, not arbitrary values.

`4 · 8 · 12 · 16 · 20 · 24 · 32 · 40 · 48 · 64 · 80`

- Card padding: 20 (compact) or 24 (default).
- Page gutter: 24 on mobile, 48 on desktop.
- Vertical rhythm between sections: 40.
- Max content width for reading (notes, docs): 720 px.

### 5.3 Elevation

Flat by default. Borders over shadows.

| Level | Treatment |
|---|---|
| 0 — resting | `1px solid var(--ob-surface-3)`, no shadow |
| 1 — raised (dropdowns, popovers) | `0 4px 16px rgba(24,34,48,0.08)` + border |
| 2 — modal / sheet | `0 16px 48px rgba(24,34,48,0.16)` + border |

Never stack shadows; never use coloured shadows.

### 5.4 Motion

- Durations: 120 ms (micro: hover, toggle), 200 ms (standard: menus, panels), 320 ms (large: sheets, page transitions).
- Easing: `cubic-bezier(0.2, 0, 0, 1)` for enter, `cubic-bezier(0.4, 0, 1, 1)` for exit.
- The **recording pulse** is the one expressive animation allowed: accent dot, scale 1 → 1.35, opacity 1 → 0, 1.6 s loop, ease-out. Everything else is utilitarian.
- Respect `prefers-reduced-motion`: disable the pulse (use a static accent dot) and cut all durations to 0.

---

## 6. Iconography

- Line icons, **1.75 px stroke**, round caps and joins, 24 px grid. [Lucide](https://lucide.dev) is the approved set; it matches the mark's round-capped cradle.
- Icons take `currentColor`. Never multi-colour.
- No filled icons except status dots.
- Never use emoji in UI. Never use sparkle/magic-wand/robot/brain icons to signal "AI".
- The OpenBark mono mark (`openbark-mark-mono.svg`) may be used inline as an icon at 20–24 px, e.g. beside a model name or in an "About" row.

---

## 7. Core components

### 7.1 Record button

The single most important control. It is the only accent-filled button in the product.

- Idle: accent fill `#E27A2B`, white mic/paw glyph (use the mono mark), 56 px circle on mobile, 48 px on desktop. Label beneath: "Record" in `caption`.
- Recording: ink fill, accent pulse ring behind it, label "Recording · 02:14" in mono.
- Paused: `--ob-surface-3` fill, ink glyph.
- Processing: ink fill, indeterminate thin ring in accent, label "Writing note…".
- Minimum tap target 56 × 56 on touch.

### 7.2 Buttons

| Variant | Fill | Text | Border |
|---|---|---|---|
| Primary | ink | white | — |
| Secondary | surface | ink | `--ob-surface-3` |
| Ghost | transparent | ink | — |
| Destructive | transparent | `--ob-danger` | `--ob-danger` at 40% |
| Record | accent | white | — (see 7.1) |

Height 40 (default), 32 (compact), 48 (prominent). Padding-x 16. Weight 600. Radius `--r-md`.

### 7.3 Inputs

Height 40, `--ob-surface-2` fill, 1 px `--ob-surface-3` border, radius `--r-md`. Focus: 2 px accent ring with 2 px offset, border turns ink. Labels above, `body-strong`, 6 px gap. Helper text `small` in `--ob-text-2`.

### 7.4 Cards

`--ob-surface` fill, 1 px border, radius `--r-lg`, padding 24. Title `h3`. No header bars, no coloured left borders, no icons in the top-left corner.

### 7.5 Transcript / note view

- Two-column on desktop: live transcript (left, mono, `--ob-text-2` with the current utterance in ink) and generated note (right, mono, ink).
- Speaker labels in `caption`, weight 600: **Vet**, **Client**, **Tech**. Never colour-code speakers.
- SOAP headings (`Subjective`, `Objective`, `Assessment`, `Plan`) in `h3`, with a 1 px rule beneath.
- Edited text shows a 2 px accent bar in the left gutter until saved.

### 7.6 Status and model chips

Pill, `--r-full`, height 24, `caption` weight 600, 8 px dot on the left.
- Model loaded: success dot · "gemma-3n · local"
- Downloading: warning dot · "Downloading 1.2 / 3.4 GB"
- No model: danger dot · "No model loaded"
- Always show **where inference is running** ("local" or the device name). Local-first is the promise; make it visible.

### 7.7 Empty states

Mono mark at 48 px in `--ob-text-3`, one-line `h3`, one-line `body` in `--ob-text-2`, one primary action. No illustrations.

---

## 8. Voice and copy

- Plain, short, clinical. Write like a good RVT talking to a colleague, not a startup talking to a user.
- Sentence case. Full stops on sentences, none on labels.
- Say "note", "record", "transcript", "template". Don't say "magic", "AI-powered", "smart", "assistant", "insights".
- Be honest about the machine: "Writing note…" not "Thinking…". "Transcription failed — audio was too quiet" not "Oops, something went wrong".
- Lead with what the clinician gets, not what the model does.
- Product name: **OpenBark** in prose, `openbark` in the logo, the CLI command and the repo name.

Example strings:

| Context | Copy |
|---|---|
| Record button idle | Record |
| Record button active | Recording · 02:14 |
| Processing | Writing note… |
| First launch | No model loaded. Download Gemma (3.4 GB) to start, or load one from Hugging Face. |
| Save confirmation | Note saved to Bella · Patel |
| Offline notice | Running locally. Nothing leaves this device. |
| Delete recording | Delete this recording? The note stays. |

---

## 9. Layout

- **Desktop web**: left rail 240 px (patients, templates, settings), content max 1200 px, 48 px gutters. Transcript/note split 5:7.
- **Mobile**: single column, Record button fixed bottom-centre above a 64 px tab bar (Record · Notes · Templates · Settings). Safe-area aware.
- **Breakpoints**: 480 / 768 / 1024 / 1280.
- Content never touches the viewport edge. Minimum 16 px side gutter on phones.
- Tables scroll horizontally inside a bordered box; never let the page scroll sideways.

---

## 10. Accessibility

- Text contrast ≥ 4.5:1 (≥ 3:1 at 24 px+). `--ob-text-3` only at 12 px+ on white; use `--ob-text-2` for anything the user must read.
- Accent `#E27A2B` on white is **3.2:1** — fine for the 56 px Record button and large graphics, not for text. Use `--ob-accent-600`/`700` for accent text.
- Every icon-only control has `aria-label`. Status is never colour-alone; pair dots with text.
- Full keyboard operation; visible focus ring (2 px accent, 2 px offset) on everything.
- Recording state is announced via `aria-live="polite"`.
- Honour `prefers-reduced-motion` and `prefers-color-scheme`.

---

## 11. Files in this package

```
openbark-brand/
├── DESIGN.md                          ← this file
└── logos/
    ├── openbark-mark.svg              ← primary mark, 100×100, ink + accent
    ├── openbark-mark-on-dark.svg      ← white + accent for dark surfaces
    ├── openbark-mark-mono.svg         ← currentColor, inline-safe
    ├── openbark-lockup-horizontal.svg ← mark + wordmark (live Manrope text)
    ├── openbark-app-icon-dark.svg     ← 512×512, primary app icon
    ├── openbark-app-icon-light.svg    ← 512×512
    ├── openbark-app-icon-accent.svg   ← 512×512, marketing only
    └── openbark-favicon.svg           ← 32×32, simplified
```

### Suggested CSS tokens

```css
:root {
  --ob-ink: #182230;
  --ob-accent: #E27A2B;
  --ob-accent-50: #FDF4EC;
  --ob-accent-100: #FAE3CC;
  --ob-accent-300: #F0B07A;
  --ob-accent-500: #E27A2B;
  --ob-accent-600: #C9661F;
  --ob-accent-700: #A65218;
  --ob-surface: #FFFFFF;
  --ob-surface-2: #F2F4F7;
  --ob-surface-3: #E5E8EE;
  --ob-text: #182230;
  --ob-text-2: #4B5563;
  --ob-text-3: #6B7280;
  --ob-success: #1F8A5B;
  --ob-warning: #B7791F;
  --ob-danger: #C2410C;
  --ob-info: #2F6FE4;
  --r-sm: 6px; --r-md: 10px; --r-lg: 14px; --r-xl: 22px; --r-full: 9999px;
  --font-sans: 'Manrope', system-ui, -apple-system, 'Segoe UI', sans-serif;
  --font-mono: 'JetBrains Mono', ui-monospace, 'SF Mono', Menlo, monospace;
  --ease-in: cubic-bezier(0.2, 0, 0, 1);
  --ease-out: cubic-bezier(0.4, 0, 1, 1);
}
@media (prefers-color-scheme: dark) {
  :root {
    --ob-surface: #0F151D;
    --ob-surface-2: #182230;
    --ob-surface-3: #243040;
    --ob-text: #F5F7FA;
    --ob-text-2: #B4BCC8;
    --ob-text-3: #8A94A3;
    --ob-accent: #E98B45;
  }
}
```

### Generating raster icons

```bash
# requires librsvg (brew install librsvg / apt install librsvg2-bin)
for s in 16 32 48 64 128 192 256 512 1024; do
  rsvg-convert -w $s -h $s logos/openbark-app-icon-dark.svg -o icon-$s.png
done
rsvg-convert -w 32 -h 32 logos/openbark-favicon.svg -o favicon-32.png
```

---

*Version 1.0 · October 2026. When in doubt: quieter, warmer, one accent.*
