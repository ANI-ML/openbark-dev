# ADR-0016: The OpenBark brand guide is the source of truth for how OpenBark looks

- Status: accepted
- Date: 2026-10-18
- Deciders: product owner (supplied the brand package and said "Here is all the openbark branding for the website and apps. Please proceed."; chose Lucide over Phosphor, and the brand guide over the earlier no-middle-dot rule), design, engineering

## Context

The product owner supplied a brand package. It contains a design language (`docs/brand/DESIGN.md`, version 1.0) and eight logo files in `docs/brand/logos`. It calls itself "the single source of truth for how OpenBark looks and feels" on every surface.

It conflicts with three earlier decisions:
- the original design system, `docs/design/DESIGN.md`, with teal, Phosphor and no middle dots;
- ADR-0010 (Apple style: translucent materials, springs, system font);
- ADR-0015 (Geist type and ANI.ML Health orange #B35209, decided earlier today).

## Decision

On every question of visual identity, `docs/brand/DESIGN.md` wins: logo, colour, type, icons, components, motion, layout and copy voice. It replaces ADR-0010 and ADR-0015 wherever they differ.

`docs/design/DESIGN.md` still covers interaction behaviour the brand guide doesn't address:
- the evidence link;
- "Not discussed" for sections that never came up;
- drafts stay unsigned until a veterinarian signs;
- the accessibility floor.

| Area | Before | Now |
|---|---|---|
| Mark | Paw on an orange tile | The paw-microphone mark, copied exactly from `docs/brand/logos`. The app icon is `openbark-app-icon-dark.svg` on every platform. The favicon is `openbark-favicon.svg`. |
| Wordmark | "OpenBark" in Geist | `openbark` in Manrope, with "open" at weight 500 and "bark" at 800. "OpenBark" in running prose. |
| Colour | Orange #B35209 as the accent everywhere | Ink #182230 for text and primary buttons. Amber #E27A2B for Record, the recording state, the focus ring and active marks only, one per screen. Accent text uses #A65218. Dark mode follows the guide (§3.4). |
| Recording colour | Pulse Red | Amber. Recording is the brand moment (§3.3). |
| Type | Geist and Geist Mono | Manrope and JetBrains Mono, bundled (OFL, fontsource 5.3.0). Headlines are 800, never 700. The note and transcript are set in the mono transcript style. |
| Icons | Phosphor | Lucide (lucide-react 1.59.0), 1.75 px stroke, round caps. |
| Surfaces | Translucent chrome, springs | Flat surfaces with 1 px borders. No glass, no gradients, no coloured shadows. Motion runs at 120, 200 and 320 ms; the recording pulse is the only expressive animation. |
| Middle dots | Banned | Used only where the guide shows them: "Recording · 02:14", "Note saved to Bella · Patel", and model chips such as "gemma-4 · local". |

## Consequences

- One brand across the app, the website, the README and the installers.
- The help videos and screenshots predate this and need re-recording.
- Taste-skill rules that the guide contradicts no longer apply here: centred heroes, the Lucide ban and the "no eyebrows" rule. The brand guide is the owner's own.
- Marketing claims must stay true and checkable. The website doesn't say "the first open-source local scribe", because earlier open-source local scribes exist. It doesn't say "zero environmental impact" either; it says "no data centre" instead.

## Alternatives considered

| Option | Why not |
|---|---|
| Keep ADR-0015 orange and Geist | The owner supplied a complete brand to replace them. |
| Keep Phosphor icons | The owner chose Lucide, which is what the guide approves. |
