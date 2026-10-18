import type { ReactNode } from "react";

export type Tone = "ok" | "warn" | "danger" | "record" | "accent" | "neutral";

/**
 * DESIGN.md §7.6 status chip: a 24 px pill with an 8 px dot on the left,
 * always paired with text (status is never colour alone, §10). The record
 * tone with `live` pulses like the recording indicator (§3.3).
 */
export function StatusDot({
  tone = "neutral",
  live,
  className,
  children,
}: {
  tone?: Tone;
  /** Pulse the dot (recording in progress); static under reduced motion. */
  live?: boolean;
  className?: string;
  children: ReactNode;
}) {
  return (
    <span className={className ? `ob-chip ${className}` : "ob-chip"}>
      <span className="ob-dot" data-tone={tone} data-live={live || undefined} aria-hidden />
      {children}
    </span>
  );
}
