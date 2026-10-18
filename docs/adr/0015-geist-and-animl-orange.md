# ADR-0015: Geist type and ANI.ML Health orange across the app and website

- Status: accepted
- Date: 2026-10-18
- Deciders: product owner (asked that the website be orange like ANI.ML Health, then that the app and website be reviewed with the taste and redesign design skills and changed to match), design, engineering

## Context

The website had moved to ANI.ML Health's orange (#F47C20, taken from animl.health), while the app was still teal (#2F6F6A) with a teal icon. A visitor would download an orange product and open a teal one.

A design review of both, using the `design-taste-frontend` and `redesign-existing-projects` skills, found the following:

- **Type.** The app rendered in Segoe UI on Windows, because ADR-0010 put the system font first, and its numbers fell back to Consolas. Both skills name the platform default as a generic look. Geist was already bundled, but it sat last in the font stack.
- **Brand mark.** The sidebar mark was a plain coloured square, not the paw.
- **Empty states.** "No draft yet" and "The live transcript appears here" were single grey lines that didn't say what to do next.
- **Avatars.** Patient avatars were generic circles.

## Decision

| Area | Before (ADR-0010) | Now |
|---|---|---|
| Accent | Scrub Teal #2F6F6A (dark #6FB3AC) | Burnt orange #B35209 for buttons, links and focus, with white text at 5.1:1; accent-soft #FDF0E6. In dark mode, #FF9A4D with dark text at 9:1, soft #3A2414. |
| Brand mark | Accent-coloured square | The paw on ANI.ML Health orange #F47C20 (new token `--brand`), used for the logo only and never for text. It appears in the sidebar, on the app icon on every platform, and on the website. |
| Type | System font first, Geist as the fallback | Geist Sans first on every platform, Geist Mono for times and sizes. Both are bundled; nothing loads from a CDN. |
| Empty states | One grey line | An icon tile, a line saying what will appear, and the action that fills it ("Draft note" on Review). |
| Avatars | Circles | Rounded squares (12px). |

Unchanged from ADR-0010:
- Pulse Red stays reserved for recording. It is clearly distinct from the orange in hue, and both were checked side by side in screenshots.
- Materials, springs, the 44px targets and WCAG 2.2 AA all stay.
- Phosphor icons stay. There are no emoji, no all-caps and no middle dots.
- The evidence link remains the signature interaction.

The website follows the same tokens. It also applies the skills' layout rules:
- a split hero with the product's real interface as the picture;
- an asymmetric bento instead of three equal cards;
- numbers without boxes, and the models as rows;
- one accent, and no gradient text;
- tinted shadows and a fixed grain overlay;
- staggered reveals and an auto-playing evidence demo, both off with reduced motion;
- a skip link, a 404 page and a licence link.

## Consequences

- One brand from the website to the installed app.
- On Windows the app no longer looks native in its type. That trade is accepted, because every clinic now sees the same face.
- The orange is ANI.ML Health's colour, so the app reads as an ANI.ML Health product. If OpenBark is ever spun out, `--brand` and `--accent` are the only values to change. The icon is regenerated from `apps/desktop/src-tauri/icons/icon.svg` with `pnpm tauri icon`.
- The help videos and DESIGN.md screenshots show teal and need re-recording.

## Alternatives considered

| Option | Why not |
|---|---|
| Keep the app teal, with only the website orange | Two brands for one product. |
| The bright #F47C20 as the accent in light mode | White text on it is 2.9:1, below AA. It stays as the mark colour. |
| Satoshi or Outfit, as the skills suggest | Geist is already bundled and licensed (OFL). A second family would add weight for no gain. |
