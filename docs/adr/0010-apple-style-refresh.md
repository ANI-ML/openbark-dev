# ADR-0010: An Apple-style refresh of the design system

- Status: accepted
- Date: 2026-10-18
- Deciders: product owner (asked that Apple's design approach be the primary reference for refreshing the look of the whole app, on all platforms), design, engineering

## Context

DESIGN.md (the product owner's design system) gives OpenBark a calm clinical look: cool neutrals, one teal accent, hairline panels, Geist type and restrained motion. The product owner asked for a refresh modelled on Apple's interface design. The reference is Apple's WWDC design talks as translated for the web: fluid, interruptible motion; springs; translucent materials for floating chrome; size-specific typography; and the reduced-motion, reduced-transparency and increased-contrast settings.

Most of DESIGN.md already agrees with that approach (restraint, feedback, accessibility, one accent). A few rules conflict with it, and those need deciding.

## Decision

Amend DESIGN.md as follows. The tokens live in `packages/ui/src/tokens.css`, so every app (desktop, web, phone, and later the extension and Hub admin) changes together.

| Area | Before | Now |
|---|---|---|
| Type | Geist Sans, bundled | The platform's system font (SF Pro on Apple devices, Segoe UI on Windows), which carries optical sizing and tracking tables; Geist stays bundled as the fallback. Numbers: the system mono (SF Mono), falling back to Geist Mono. |
| Type scale | One tracking value per role | Tracking tightens as type grows (screen titles 1.75rem, -0.022em, line 1.15; body near 0); emphasis by weight. The note reads at 17px. |
| Motion | Ease curves of 180 to 240 ms | Critically damped springs (damping 1.0, response 0.35 s), expressed as CSS `linear()` curves; a JS spring (`animateSpring`) for gestures, starting from the on-screen value and the finger's velocity. Sheets and dialogs enter and leave along the same path. Buttons, rows and chips respond on press (scale 0.97). |
| Materials | No translucency ("no glassmorphism panels") | Translucent materials for **floating chrome only**: sidebar, sticky headers, the phone tab bar, sheets, dialogs and toasts. Content panels stay solid. A soft scroll edge replaces hard dividers under sticky headers. |
| Surfaces | Hairline panels, radius 6/10 | Grouped surfaces on a grey canvas, radius 9 (controls), 14 (panels), 18 (sheets); capsule chips. |
| Buttons | Paper with hairline; ghost in grey | Tinted grey by default, accent fill for the one primary action, accent text for plain actions. |
| Sheets | Slide from the right, attached to the edge | A floating sheet inset from the edges. On phones a bottom sheet with a grabber: it follows the finger, resists upwards, and a flick down dismisses it by projected momentum. |
| Settings | Reduced motion | Also reduced transparency (materials turn solid) and increased contrast (stronger borders and text). |

Unchanged: one teal accent, Pulse Red for recording only, Phosphor icons, no emoji, sentence case, no all-caps or middle-dot strings, tabular numbers, the evidence link as the signature interaction, "Not discussed" rendering, the 44px touch targets, and WCAG 2.2 AA contrast.

## Consequences

- Every screen picks up the refresh through shared tokens and components (`Sheet`, `Dialog`, `animateSpring`). Components no longer hand-roll backdrops or Escape handling.
- On Windows the app now uses Segoe UI rather than Geist. This is deliberate: system type reads as native on each platform.
- `backdrop-filter` costs GPU time. It's limited to chrome, and turns off with reduced transparency.
- Screenshots in the help videos predate the refresh; re-record them when the screens settle.

## Alternatives considered

| Option | Why not |
|---|---|
| Keep DESIGN.md as is | The product owner asked for the refresh. |
| A full motion library (Framer Motion) | About 50 KB, for what CSS springs plus a 60-line spring cover. |
| Translucent content panels | Text over translucency loses legibility; Apple uses materials for chrome, not reading surfaces. |
