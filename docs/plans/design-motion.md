# Design pass: motion and micro-interactions

Status: proposed, for the product owner's review of the screenshots.
Date: 2026-10-19

The owner asked for a design pass on the website and the app, taking ideas from Kokonut UI and Motion (motion.dev). Inspiration only: the brand guide (`docs/brand/DESIGN.md`, ADR-0016) wins on every visual question. It must stay quiet, trustworthy and warm, flat, with one accent per screen. No Kokonut code is copied; every idea is rewritten in OpenBark's plain CSS and tokens.

## Principles taken from Motion

1. **Transform and opacity only.** Nothing animates width, height, top, left, margins or colours on a loop. Colour changes on hover and press stay as short transitions, as before. (Motion best practices, "Performance".)
2. **Springs for physical motion, curves for everything else.** Moving surfaces (a pressed button springing back, the nav marker, list rows) use a spring with **no bounce**. Motion's own advice is that a serious interface shouldn't overshoot, and a clinical tool is one. Fades and colour keep the brand curves.
3. **Things leave the way they came, and leave faster than they arrive.** Exits use the exit curve at about two thirds of the enter time.
4. **Interruptible.** Hover, press and the nav marker re-target mid-flight; nothing waits for an earlier animation to finish.
5. **Reduced motion everywhere.** `prefers-reduced-motion: reduce` sets every motion token to zero (brand guide §5.4). The app also wraps itself in Motion's `MotionConfig reducedMotion="user"`. Information never depends on motion: the toast's time-left bar is hidden then, and the toast still closes on its timer.

## Motion tokens

These live in `packages/ui/src/tokens.css` (the app) and are mirrored in `website/src/site.css`, which can't import the app's CSS. `apps/desktop/src/motion.ts` holds the same values for Motion in React.

| Token | Value | Use |
|---|---|---|
| `--dur-micro` (`--dur-fast`) | 120 ms | Hover, press, toggles (§5.4 micro) |
| `--dur-standard` (`--dur`) | 200 ms | Menus, toasts, panels (§5.4 standard) |
| `--dur-large` | 320 ms | Sheets, dialogs (§5.4 large) |
| `--dur-exit` | 200 ms | Leaving: shorter than arriving |
| `--ease-enter` (`--ease-in`) | `cubic-bezier(0.2, 0, 0, 1)` | Arriving (§5.4) |
| `--ease-exit` (`--ease-out`) | `cubic-bezier(0.4, 0, 1, 1)` | Leaving (§5.4) |
| `--ease-spring` / `--dur-spring` | `linear(...)` / 480 ms | Spring, bounce 0, looks like 320 ms, then settles |
| `--ease-spring-fast` / `--dur-spring-fast` | `linear(...)` / 320 ms | Spring, bounce 0, looks like 200 ms (press release) |
| `--press-scale` | 0.97 | Buttons and chips on press |
| `--enter-y` | 8 px | How far small things travel as they appear |
| `--skeleton-delay` | 150 ms | Skeletons appear only if loading takes longer than this |

The `linear()` springs are sampled from Motion's spring model (stiffness and damping from `visualDuration` and `bounce`), which is what Motion's CSS spring generator does. In React, the same springs are `{ type: "spring", visualDuration: 0.32, bounce: 0 }` and `{ type: "spring", visualDuration: 0.2, bounce: 0 }`.

Reduced motion: every `--dur-*` token becomes `0ms`, `--press-scale` becomes 1 and `--enter-y` 0. The recording pulse becomes the static ring (§5.4). The global `transition-duration: 0s` rule stays as the backstop.

## Chosen ideas and where each comes from

### Website

| Idea | Source | How it fits OpenBark |
|---|---|---|
| Press feedback on every button: a 0.97 scale on press, a spring back on release | Kokonut **slide-text-button** (`active:scale-[0.97]`); Motion springs | Answers the action, transform only. |
| Hero arrives once: the mark, headline, lede and actions fade up 8 px in sequence, 60 ms apart, 320 ms each | Kokonut **slide-text-button** (`@starting-style` entrance with no JS) | CSS keyframes, once, on load only. |
| The demo's recording timer ticks, so the "Recording" chip is live, not a picture | Kokonut **ai-voice** and **synced-transcript** (live state from data) | A text update; nothing moves. The pulse is still the one expressive animation. |
| One ink marker glides between transcript lines as you move along the note | Kokonut **ai-citations** ("one preview glides between chips and lights up the sentence it backs") | This is OpenBark's evidence link. The marker is ink, a 2 px bar, moved with transform only. |
| "Built carefully" as a flat bento grid: one large tile for the evidence link and four smaller ones, each with a small real-interface figure | Kokonut **bento-grid** (the 6-column layout only, not the drag and resize) | Flat tiles with 1 px borders, no shadows, no hover lift. Every claim is a shipped feature on the features page. |
| Sections fade up 16 px as they come into view, once | Motion's scroll-triggered animation (`inView`), done with an IntersectionObserver | Armed by the first scroll: only sections still below the fold wait, so a page that is never scrolled (a full-page capture, a reader view, print) shows everything. A CSS scroll timeline (`animation-timeline: view()`) was tried first and dropped: full-page captures rendered every section below the fold blank. Off with reduced motion. |
| Download cards: an icon per platform, "This computer" on the visitor's card, which comes first in the list | Kokonut **file-upload** (clear per-item state) | Ink border and a status chip, no accent fill. Reordered in the DOM so keyboard order matches. |
| Version history as a timeline: a rail with a dot per entry, the newest filled and marked "Latest"; the contents list marks where you are | Kokonut **scroll-index** (a rail that shows where you are on a long page) | Ink dots on a 1 px rail. `aria-current="location"` on the contents link. |

### App

| Idea | Source | How it fits OpenBark |
|---|---|---|
| Motion tokens in `tokens.css`, one source for durations, curves and springs | Motion best practices | Brand §5.4 values, plus the springs. |
| Press states on buttons, chips, segments and the Record button (0.97, spring back) | Kokonut **slide-text-button**; old design doc §7 ("feedback on press, not release") | Transform only. |
| The nav marker glides to the new destination (rail and phone tab bar) | Kokonut **morphic-navbar** and **smooth-tab** (indicator follows the selection); Motion `layoutId` | The existing 2 px ink marker, now one element that moves. |
| Toast arrives and leaves (it used to vanish), with a thin time-left bar that pauses while you hover or focus it | Kokonut **undo-button** (time left drawn on the control; the clock holds while you're on it); Motion `AnimatePresence` | Ink bar, transform `scaleX` only. The toast already waited while held; now you can see it. |
| Lists animate when rows come and go: filtering patients, restoring or deleting visits | Motion layout animations and `AnimatePresence` | Rows glide into place and fade out. Lists still render at once on first load, with no stagger. |
| Recording feedback: the Record glyph cross-scales between the mark and the stop square, and the label fades between states | Kokonut **ai-voice** (clear state changes in a voice control) | Opacity and scale on the glyph. The pulse is unchanged. |
| Skeletons shaped like what's coming (rows for lists, lines for the note), shown only after 150 ms so quick loads never flash | Kokonut **ai-loading** (progress that looks like the result) | Flat grey blocks with the existing slow opacity breathe. No shimmer gradient. |
| Small Review and Models touches: the lit transcript line fades in, the Ask and Edit boxes open with a short fade-up, new search results fade in, and download bars move on the spring | Kokonut **ai-citations** and **file-upload** | Opacity and transform only. |

## Rejected, and why

| Kokonut component or idea | Why not |
|---|---|
| gradient-button, loader (gradient rings), ai-text-loading (shimmer through letters), glitch-text | Gradients and shimmer are banned (§3.5). "Thinking" shimmer also suggests a mind (§1, §8). |
| liquid-glass-card, switch-button (theme spreads in a circle) | Glass is banned. The theme spread is flashy for a tool used at 11 pm in a dim exam room. |
| shape-hero, background-paths, dot-field, flow-field, topo-background, drape-background, halftone-background | Decorative motion behind content. OpenBark's backgrounds are flat surfaces (§3.5, §9). |
| particle-button, bell-button, attract-button, tear-button, pull-cord, cradle-text, plucked-text, swoosh-text | Playful and expressive. The recording pulse is the only expressive animation (§5.4). |
| hold-button (press and hold to confirm) | Slow for keyboard and switch users, and OpenBark already uses Undo, PINs and plain confirmations for destructive actions. |
| card-flip, card-stack, carousel-cards, wheel-picker, detent-dial | 3D and momentum toys with no clinical job. Hide content behind gestures. |
| notification-stack (swipe to dismiss stacked toasts) | OpenBark shows one toast at a time; swipe-only dismissal isn't keyboard friendly. |
| type-writer, dynamic-text | Banned: "no typewriter effects" (design doc §7). Live text is driven by data. |
| Hover lifts and shadows on cards | Flat by default, borders over shadows (§5.3). |
| Staggered cascades in app lists | App lists mount at once (design doc §7). The only stagger is the website hero, once. |
| A sliding thumb in every segmented control | Worth doing later; the controls are hand-written in many screens, and changing them all now would collide with other work in progress. |

## Dependency

`motion` 14.1.0 (MIT) is added to `apps/desktop` only, for `AnimatePresence`, layout animations and `layoutId`. CSS can't animate an element that is leaving the page or a list reflowing. The app loads `domMax` through `LazyMotion` and uses the light `m` components. Versions, licence and size are recorded in `docs/versions.md`. The website uses no JS animation library: its motion is CSS (transitions, keyframes, `linear()` springs) plus a few lines in the site's own `site.js` (an IntersectionObserver for the reveals, the demo's marker and timer), all within its `script-src 'self'` policy.

## Accessibility checklist

- Focus rings unchanged: a 2 px accent outline with a 2 px offset on everything.
- Every animation is transform or opacity, and all of it is off with reduced motion.
- Contrast unchanged: no new colours. New chips use ink and the existing status colours.
- Phones from 360 px wide with no horizontal scroll, light and dark, checked by screenshot.
- Keyboard: the reordered download cards are moved in the DOM, so tab order follows what you see.

Credit: interaction ideas from Kokonut UI (MIT, kokonutui.com) and from Motion's animation guidance (motion.dev). No code from either is copied into OpenBark.
