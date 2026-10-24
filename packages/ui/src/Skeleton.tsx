/**
 * Skeletons shaped like what is coming (design-motion, after Kokonut UI's
 * loading patterns): rows for a list, lines for text, or one block. Flat
 * grey with a slow opacity breathe, no shimmer gradient (§3.5). They fade in
 * only after --skeleton-delay (150 ms), so a quick load never flashes. One
 * polite status names what is loading; the shapes are hidden from screen
 * readers.
 */
export interface SkeletonProps {
  /** List rows: an icon, two lines and a short value, in a bordered list. */
  rows?: number;
  /** Lines of text, with natural ragged widths. */
  lines?: number;
  /** A single block of this height (px), when neither rows nor lines is given. */
  height?: number;
  /** What is loading, for screen readers ("Loading"). */
  label?: string;
  className?: string;
}

const WIDTHS = ["92%", "84%", "70%", "88%", "62%", "78%"];

export function Skeleton({ rows, lines, height = 120, label = "Loading", className }: SkeletonProps) {
  const cls = (base: string) => (className ? `${base} ${className}` : base);
  if (rows) {
    return (
      <div className={cls("ob-skeleton-rows")} role="status" aria-label={label}>
        {Array.from({ length: rows }, (_, i) => (
          <div key={i} className="ob-skeleton-row" aria-hidden>
            <span className="ob-skeleton ob-skeleton-icon" />
            <span className="ob-skeleton-text">
              <span className="ob-skeleton" style={{ width: i % 2 ? "34%" : "46%" }} />
              <span className="ob-skeleton ob-skeleton-short" style={{ width: i % 2 ? "22%" : "28%" }} />
            </span>
            <span className="ob-skeleton ob-skeleton-meta" />
          </div>
        ))}
      </div>
    );
  }
  if (lines) {
    return (
      <div className={cls("ob-skeleton-lines")} role="status" aria-label={label}>
        {Array.from({ length: lines }, (_, i) => (
          <span key={i} className="ob-skeleton" aria-hidden style={{ width: WIDTHS[i % WIDTHS.length] }} />
        ))}
      </div>
    );
  }
  return <div className={cls("ob-skeleton")} role="status" aria-label={label} style={{ height }} />;
}
