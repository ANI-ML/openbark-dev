export type Tone = "ok" | "warn" | "danger" | "record" | "accent" | "neutral";

/** DESIGN.md §5: an 8px coloured dot always paired with Ink text (never colour alone). */
export function StatusDot({ tone = "neutral", children }: { tone?: Tone; children: React.ReactNode }) {
  return (
    <span className="ob-chip">
      <span className="ob-dot" data-tone={tone} aria-hidden />
      {children}
    </span>
  );
}
