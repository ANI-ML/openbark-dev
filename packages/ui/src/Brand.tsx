import { useId, type CSSProperties } from "react";

/**
 * The OpenBark mark, wordmark and lockup (docs/brand/DESIGN.md §2).
 *
 * Geometry is copied from docs/brand/logos on the 100 x 100 grid; never
 * redraw it by eye. Colours come from CSS (brand.css): toes, cradle, stem and
 * base are ink on light and white on dark, the capsule is the accent, and the
 * grille shows the surface the mark sits on.
 */

/** "auto" follows the theme; "light" and "dark" force the variant for a tile. */
export type BrandTone = "auto" | "light" | "dark";

interface CommonProps {
  /** Rendered size in px (or any CSS length). */
  size?: number | string;
  tone?: BrandTone;
  /** Accessible name. Leave out when the mark is decorative (it is then hidden). */
  title?: string;
  className?: string;
  style?: CSSProperties;
}

/** Below 24 px rendered size the grille is dropped and the simplified favicon geometry is used (§2.1). */
const isSmall = (size: number | string) => typeof size === "number" && size < 24;

function a11y(title?: string) {
  return title ? ({ role: "img", "aria-label": title } as const) : ({ "aria-hidden": true } as const);
}

function cls(base: string, extra?: string) {
  return extra ? `${base} ${extra}` : base;
}

/** openbark-mark.svg, as shapes on the 100 grid. */
function MarkShapes({ grille = true }: { grille?: boolean }) {
  return (
    <>
      <circle className="ob-mark-fg" cx="30" cy="20" r="7.5" />
      <circle className="ob-mark-fg" cx="50" cy="12" r="7.5" />
      <circle className="ob-mark-fg" cx="70" cy="20" r="7.5" />
      <rect className="ob-mark-capsule" x="35" y="28" width="30" height="42" rx="15" />
      {grille && (
        <>
          <rect className="ob-mark-grille" x="42" y="44" width="16" height="4" rx="2" />
          <rect className="ob-mark-grille" x="42" y="52" width="16" height="4" rx="2" />
        </>
      )}
      <path className="ob-mark-cradle" d="M24 54 A26 26 0 0 0 76 54" strokeWidth="7" strokeLinecap="round" />
      <rect className="ob-mark-fg" x="46.5" y="78" width="7" height="10" rx="3.5" />
      <rect className="ob-mark-fg" x="35" y="87" width="30" height="7" rx="3.5" />
    </>
  );
}

/** openbark-favicon.svg's simplified mark (no grille), on the same 100 grid. */
function SmallMarkShapes() {
  return (
    <>
      <circle className="ob-mark-fg" cx="30" cy="20" r="8.5" />
      <circle className="ob-mark-fg" cx="50" cy="12" r="8.5" />
      <circle className="ob-mark-fg" cx="70" cy="20" r="8.5" />
      <rect className="ob-mark-capsule" x="34" y="28" width="32" height="44" rx="16" />
      <path className="ob-mark-cradle" d="M22 54 A28 28 0 0 0 78 54" strokeWidth="9" strokeLinecap="round" />
      <rect className="ob-mark-fg" x="45" y="80" width="10" height="10" rx="5" />
      <rect className="ob-mark-fg" x="33" y="88" width="34" height="9" rx="4.5" />
    </>
  );
}

/**
 * The full-colour paw-mic mark. Use it for app icons, avatars and loading
 * states, anywhere up to 48 px or where the name is already visible.
 */
export function Mark({ size = 32, tone = "auto", title, className, style }: CommonProps) {
  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      viewBox="0 0 100 100"
      width={size}
      height={size}
      className={cls("ob-mark", className)}
      data-tone={tone === "auto" ? undefined : tone}
      style={style}
      focusable="false"
      {...a11y(title)}
    >
      {isSmall(size) ? <SmallMarkShapes /> : <MarkShapes />}
    </svg>
  );
}

/**
 * The single-colour mark (openbark-mark-mono.svg): inherits currentColor,
 * with the grille cut out so the surface shows through. Use it inline at
 * 20-24 px and at 48 px in empty states.
 */
export function MarkMono({ size = 24, title, className, style }: Omit<CommonProps, "tone">) {
  // One mask per instance; the id must be a valid URL fragment.
  const mask = `ob-grille-${useId().replace(/[^a-zA-Z0-9_-]/g, "")}`;
  const small = isSmall(size);
  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      viewBox="0 0 100 100"
      width={size}
      height={size}
      className={cls("ob-mark-mono", className)}
      style={style}
      focusable="false"
      {...a11y(title)}
    >
      {small ? (
        <>
          <g fill="currentColor">
            <circle cx="30" cy="20" r="8.5" />
            <circle cx="50" cy="12" r="8.5" />
            <circle cx="70" cy="20" r="8.5" />
            <rect x="34" y="28" width="32" height="44" rx="16" />
            <rect x="45" y="80" width="10" height="10" rx="5" />
            <rect x="33" y="88" width="34" height="9" rx="4.5" />
          </g>
          <path d="M22 54 A28 28 0 0 0 78 54" fill="none" stroke="currentColor" strokeWidth="9" strokeLinecap="round" />
        </>
      ) : (
        <>
          <defs>
            {/* Mask luminance, not colours: white keeps, black cuts the grille. */}
            <mask id={mask}>
              <rect width="100" height="100" fill="#fff" />
              <rect x="42" y="44" width="16" height="4" rx="2" fill="#000" />
              <rect x="42" y="52" width="16" height="4" rx="2" fill="#000" />
            </mask>
          </defs>
          <g fill="currentColor">
            <circle cx="30" cy="20" r="7.5" />
            <circle cx="50" cy="12" r="7.5" />
            <circle cx="70" cy="20" r="7.5" />
            <rect x="35" y="28" width="30" height="42" rx="15" mask={`url(#${mask})`} />
            <rect x="46.5" y="78" width="7" height="10" rx="3.5" />
            <rect x="35" y="87" width="30" height="7" rx="3.5" />
          </g>
          <path d="M24 54 A26 26 0 0 0 76 54" fill="none" stroke="currentColor" strokeWidth="7" strokeLinecap="round" />
        </>
      )}
    </svg>
  );
}

/**
 * "openbark": always lowercase, one word, Manrope, "open" 500 and "bark"
 * 800, -0.03em. Ink on light, white on dark, never the accent (§2.2).
 */
export function Wordmark({
  size = 20,
  tone = "auto",
  title = "OpenBark",
  className,
  style,
}: Omit<CommonProps, "title"> & { title?: string }) {
  return (
    <span
      className={cls("ob-wordmark", className)}
      data-tone={tone === "auto" ? undefined : tone}
      style={{ fontSize: size, ...style }}
      role="img"
      aria-label={title}
    >
      <span className="ob-wordmark-open">open</span>
      <span className="ob-wordmark-bark">bark</span>
    </span>
  );
}

/**
 * The horizontal lockup (openbark-lockup-horizontal.svg): the mark at 72%
 * on a 372 x 80 canvas beside the 60 px wordmark, so the mark is 1.2x the
 * wordmark size and the proportions match §2.3 at any height. Live text:
 * Manrope is bundled with the tokens, so it renders the same offline.
 */
export function Lockup({ size = 32, tone = "auto", title = "OpenBark", className, style }: CommonProps) {
  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      viewBox="0 0 372 80"
      height={size}
      width={typeof size === "number" ? (size * 372) / 80 : undefined}
      className={cls("ob-mark ob-lockup", className)}
      data-tone={tone === "auto" ? undefined : tone}
      style={typeof size === "number" ? style : { aspectRatio: "372 / 80", ...style }}
      focusable="false"
      role="img"
      aria-label={title}
    >
      <g transform="translate(4 4) scale(0.72)">
        <MarkShapes />
      </g>
      <text className="ob-lockup-word" x="92" y="60">
        <tspan className="ob-wordmark-open">open</tspan>
        <tspan className="ob-wordmark-bark">bark</tspan>
      </text>
    </svg>
  );
}
