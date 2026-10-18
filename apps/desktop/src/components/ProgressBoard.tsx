import { Circle, CircleCheck, CircleDot, CircleMinus } from "lucide-react";
import type { ReactNode } from "react";

export type StepState = "done" | "active" | "pending" | "skipped";

export interface BoardStep {
  key: string;
  label: string;
  /** A line under the label: how far, what's happening now. */
  detail?: string;
  state: StepState;
  /** Smaller steps inside this one (the headings being written). */
  children?: BoardStep[];
}

const ICON = { done: CircleCheck, active: CircleDot, pending: Circle, skipped: CircleMinus } as const;

/**
 * A loading board for long work (transcribing a recording, drafting a
 * note): what OpenBark is doing, as named steps that tick off, with one
 * determinate bar (never a spinner), how far it has got, and how long it
 * has taken or should take. Step changes are announced to screen readers.
 *
 * Progress is ink (docs/brand/DESIGN.md §3.5). The active step takes the
 * accent only with `accent`, which the caller sets when nothing else on the
 * screen is amber (one accent per screen; the Record button is amber).
 */
export function ProgressBoard({
  label,
  title,
  subtitle,
  steps,
  fraction,
  status,
  aside,
  compact = false,
  accent = false,
}: {
  /** Accessible name of the progress bar. */
  label: string;
  title: string;
  subtitle?: string;
  steps: BoardStep[];
  /** 0..1, only ever moving forward. */
  fraction: number;
  /** Right of the title: "45%" or "45%, about a minute left". */
  status?: string;
  /** Under the steps: elapsed time, an estimate, a reassurance. */
  aside?: ReactNode;
  /** Inside a panel (one heading being redrafted): fewer lines. */
  compact?: boolean;
  /** Mark the active step with the accent (only when nothing else on screen is amber). */
  accent?: boolean;
}) {
  const pct = Math.round(Math.min(1, Math.max(0, fraction)) * 100);
  const active = steps.find((s) => s.state === "active");
  return (
    <div
      className={`board${compact ? " board-compact" : ""}`}
      data-accent={accent || undefined}
      role="group"
      aria-label={label}
    >
      <div className="board-head">
        <div className="board-titles">
          <p className="board-title">{title}</p>
          {subtitle && <p className="small muted">{subtitle}</p>}
        </div>
        <span className="mono faint small">{status ?? `${pct}%`}</span>
      </div>
      <div
        className="ob-progress board-bar"
        role="progressbar"
        aria-label={label}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={pct}
        aria-valuetext={active ? `${active.label}${active.detail ? `: ${active.detail}` : ""}` : `${pct}%`}
      >
        <span style={{ transform: `scaleX(${pct / 100})` }} />
      </div>
      <ol className="board-steps">
        {steps.map((s) => {
          const Icon = ICON[s.state];
          return (
            <li key={s.key} className="board-step" data-state={s.state}>
              <Icon className="board-icon" size={compact ? 18 : 20} aria-hidden />
              <div className="board-step-text">
                <span className="board-step-label">{s.label}</span>
                {s.detail && <span className="small muted">{s.detail}</span>}
                {s.children && s.children.length > 0 && (
                  <ul className="board-sub" aria-label={s.label}>
                    {s.children.map((c) => {
                      const SubIcon = ICON[c.state];
                      return (
                        <li key={c.key} data-state={c.state}>
                          <SubIcon size={14} aria-hidden />
                          <span>{c.label}</span>
                        </li>
                      );
                    })}
                  </ul>
                )}
              </div>
            </li>
          );
        })}
      </ol>
      {aside && <div className="small faint board-aside">{aside}</div>}
      <p className="sr-only" aria-live="polite">
        {active ? active.label : ""}
      </p>
    </div>
  );
}
