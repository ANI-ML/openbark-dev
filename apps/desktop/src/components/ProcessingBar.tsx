import { useRef } from "react";
import { fmtBytes, fmtTime } from "../context";
import type { ProcessingProgress, ProcessingStage } from "../engine";
import { useT } from "../i18n";

/**
 * Where each step sits on the one bar, from start to end. Transcribing and
 * checking numbers take nearly all the time (on an 8.5-minute import, about
 * the same each); an upload only happens in web mode, so the bar keeps its
 * room only when one was seen. Without the second recogniser there's no
 * checking step and the bar jumps ahead, never back.
 */
const SPANS: Record<"upload" | "local", Partial<Record<ProcessingStage, [number, number]>>> = {
  upload: {
    upload: [0, 0.12],
    convert: [0.12, 0.14],
    read: [0.14, 0.16],
    transcribe: [0.16, 0.6],
    check: [0.6, 0.97],
    save: [0.97, 1],
  },
  local: { convert: [0, 0.02], read: [0.02, 0.04], transcribe: [0.04, 0.58], check: [0.58, 0.97], save: [0.97, 1] },
};

/**
 * How far an import, or a stopped recording, has got: one determinate bar
 * (DESIGN.md §5, never a spinner), what's happening in words, and an
 * estimate of the time left for the whole job once there's enough to go on.
 */
export function ProcessingBar({ progress }: { progress: ProcessingProgress | null }) {
  const t = useT();
  const sawUpload = useRef(false);
  /** When the bar started moving on the audio itself, and from where. */
  const begin = useRef<{ at: number; fraction: number } | null>(null);
  const best = useRef(0);
  if (!progress) {
    sawUpload.current = false;
    begin.current = null;
    best.current = 0;
    return null;
  }
  const { stage, done, total } = progress;
  if (stage === "upload") sawUpload.current = true;

  const [a, b] = SPANS[sawUpload.current ? "upload" : "local"][stage] ?? [0, 1];
  const part = total > 0 ? Math.min(1, done / total) : 0;
  // Events can arrive out of order across the two channels; the bar only goes forward.
  const fraction = Math.max(best.current, a + (b - a) * part);
  best.current = fraction;

  const label =
    stage === "upload"
      ? t("Uploading the recording: {done} of {total}", { done: fmtBytes(done), total: fmtBytes(total) })
      : stage === "convert"
        ? t("Converting the recording in this browser")
        : stage === "read"
          ? t("Reading the recording")
          : stage === "transcribe"
            ? total > 0
              ? t("Transcribing: {done} of {total}", { done: fmtTime(done), total: fmtTime(total) })
              : t("Finishing the transcript")
            : stage === "check"
              ? t("Checking numbers with the second recogniser: {done} of {total}", {
                  done: Math.round(done),
                  total: Math.round(total),
                })
              : t("Saving");

  // The whole job's time left, from how fast the bar has moved since the
  // audio started being transcribed (uploads and model loading aside).
  if (!begin.current && stage === "transcribe" && total > 0) begin.current = { at: Date.now(), fraction };
  let left = "";
  const from = begin.current;
  if (from && stage !== "save") {
    const elapsed = (Date.now() - from.at) / 1000;
    const moved = fraction - from.fraction;
    if (elapsed > 5 && moved > 0.02) {
      const secs = ((1 - fraction) * elapsed) / moved;
      left =
        secs < 50
          ? t("Less than a minute left")
          : secs < 90
            ? t("About a minute left")
            : t("About {n} minutes left", { n: Math.round(secs / 60) });
    }
  }

  const pct = Math.round(fraction * 100);
  return (
    <div className="processing">
      <div
        className="ob-progress"
        role="progressbar"
        aria-label={t("Processing the recording")}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={pct}
        aria-valuetext={left ? `${label}. ${left}` : label}
      >
        <span style={{ transform: `scaleX(${fraction})` }} />
      </div>
      <div className="processing-text small">
        <span>{label}</span>
        <span className="mono faint">{left ? `${pct}%, ${left}` : `${pct}%`}</span>
      </div>
    </div>
  );
}
