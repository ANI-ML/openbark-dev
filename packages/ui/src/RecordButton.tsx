import { Square } from "lucide-react";
import { MarkMono } from "./Brand";
import type { LevelSource } from "./Waveform";

export type RecordState = "idle" | "recording" | "paused" | "processing";

export interface RecordButtonLabels {
  /** Accessible name while idle. */
  start: string;
  /** Accessible name while recording. */
  stop: string;
  /** Accessible name while paused. */
  resume?: string;
  /** Visible label while idle: "Record". */
  idle?: string;
  /** Visible label while recording, before the elapsed time: "Recording". */
  recording?: string;
  /** Visible label while paused: "Paused". */
  paused?: string;
  /** Visible label and accessible name while the note is being written: "Writing note…". */
  processing?: string;
}

export interface RecordButtonProps {
  recording: boolean;
  /** True while starting or stopping (the button is disabled meanwhile). */
  busy?: boolean;
  disabled?: boolean;
  onToggle: () => void;
  /**
   * Microphone levels. Kept for compatibility: the button itself no longer
   * draws a waveform (§7.1); pass the same source to the stage's <Waveform>.
   */
  levels?: LevelSource;
  /** Labels in the app's language. */
  labels?: RecordButtonLabels;
  /** Seconds recorded so far, shown as "Recording · 02:14". */
  elapsed?: number;
  /** Recording is paused: grey fill, ink glyph. */
  paused?: boolean;
  /** The note is being written: ink fill with a thin indeterminate accent ring. Disables the button. */
  processing?: boolean;
  /** Hide the visible label under the button (the screen shows its own). */
  hideLabel?: boolean;
  /**
   * "large": the 96 px circle the Record screen uses, with a 15 px label
   * (ADR-0027, amending §7.1). Other places keep the default 48/56 px.
   */
  size?: "default" | "large";
  className?: string;
}

/** 02:14, or 1:02:14 past an hour. */
export function formatElapsed(seconds: number): string {
  const s = Math.max(0, Math.floor(seconds));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const pad = (n: number) => String(n).padStart(2, "0");
  return h > 0 ? `${h}:${pad(m)}:${pad(s % 60)}` : `${pad(m)}:${pad(s % 60)}`;
}

/**
 * DESIGN.md §7.1 Record button: the only accent-filled button in the product.
 * Idle: accent fill with the white mono mark, "Record" beneath.
 * Recording: ink fill with the accent pulse ring, "Recording · 02:14" in mono.
 * Paused: grey fill, ink glyph. Processing: ink fill, thin indeterminate
 * accent ring, "Writing note…". 48 px on desktop, 56 px on touch. The state
 * is announced politely; the elapsed time is not (it changes every second).
 */
export function RecordButton({
  recording,
  busy,
  disabled,
  onToggle,
  labels,
  elapsed,
  paused,
  processing,
  hideLabel,
  size = "default",
  className,
}: RecordButtonProps) {
  const state: RecordState = processing ? "processing" : paused ? "paused" : recording ? "recording" : "idle";
  const text = {
    idle: labels?.idle ?? "Record",
    recording: labels?.recording ?? "Recording",
    paused: labels?.paused ?? "Paused",
    processing: labels?.processing ?? "Writing note…",
  };
  const name =
    state === "processing"
      ? text.processing
      : state === "paused"
        ? (labels?.resume ?? "Resume recording")
        : state === "recording"
          ? (labels?.stop ?? "Stop recording")
          : (labels?.start ?? "Start recording");
  const visible =
    state === "recording" && elapsed != null ? `${text.recording} · ${formatElapsed(elapsed)}` : text[state];
  const announcement = state === "idle" ? "" : text[state];

  return (
    <div
      className={className ? `ob-record-wrap ${className}` : "ob-record-wrap"}
      data-state={state}
      data-size={size === "large" ? "large" : undefined}
    >
      <span className="ob-record-target">
        {state === "recording" && <span className="ob-record-pulse" aria-hidden />}
        {state === "processing" && <span className="ob-record-spinner" aria-hidden />}
        <button
          type="button"
          className="ob-record"
          data-state={state}
          data-recording={recording || undefined}
          onClick={onToggle}
          disabled={disabled || busy || processing}
          aria-pressed={Boolean(recording || paused)}
          aria-label={name}
        >
          {/* Keyed, so a state change mounts the new glyph, which scales up
              into place (record.css, @starting-style). */}
          <span className="ob-record-icon" key={state === "recording" ? "stop" : "mark"}>
            {state === "recording" ? (
              <Square size={size === "large" ? 32 : 20} aria-hidden />
            ) : (
              <MarkMono size={size === "large" ? 48 : 24} className="ob-record-glyph" />
            )}
          </span>
        </button>
      </span>
      {!hideLabel && (
        // While recording the label is a clock: role="timer" is not announced
        // every second; the state changes are (the live region below).
        <span
          className="ob-record-label"
          data-state={state}
          key={state}
          role={state === "recording" ? "timer" : undefined}
        >
          {visible}
        </span>
      )}
      <span className="ob-visually-hidden" aria-live="polite">
        {announcement}
      </span>
    </div>
  );
}
