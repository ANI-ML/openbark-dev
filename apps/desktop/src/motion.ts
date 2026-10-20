import type { Transition } from "motion/react";

/**
 * Motion tokens for Motion in React, mirroring the CSS tokens in
 * packages/ui/src/tokens.css (DESIGN.md §5.4; docs/plans/design-motion.md).
 * Transform and opacity only. The app root sets MotionConfig
 * reducedMotion="user", so all of this stops under prefers-reduced-motion.
 */

/** Durations in seconds: micro, standard, large and exit (§5.4). */
export const DUR = { micro: 0.12, standard: 0.2, large: 0.32, exit: 0.2 } as const;

/** The brand's enter and exit curves as cubic-bezier points. */
export const EASE_ENTER = [0.2, 0, 0, 1] as const;
export const EASE_EXIT = [0.4, 0, 1, 1] as const;

/** A spring with no bounce that looks like 320 ms: things that move (the nav marker, list rows). */
export const SPRING: Transition = { type: "spring", visualDuration: DUR.large, bounce: 0 };

/** The quicker no-bounce spring, looks like 200 ms. */
export const SPRING_FAST: Transition = { type: "spring", visualDuration: DUR.standard, bounce: 0 };

/** Fades in: standard duration, the enter curve. */
export const FADE_IN: Transition = { duration: DUR.standard, ease: EASE_ENTER };

/** Leaving: quicker, the exit curve. */
export const EXIT: Transition = { duration: DUR.exit * 0.75, ease: EASE_EXIT };

/** How far small things travel as they appear, in px (--enter-y). */
export const ENTER_Y = 8;

/** Motion's animation and layout features, in a chunk of their own (LazyMotion). */
export const loadMotionFeatures = () => import("./motionFeatures").then((mod) => mod.default);

/** Lists longer than this skip layout animation, so filtering a big list stays instant. */
export const LAYOUT_LIST_LIMIT = 80;
