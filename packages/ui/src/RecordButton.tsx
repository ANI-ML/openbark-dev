import { Microphone, Stop } from "@phosphor-icons/react";
import { Waveform, type LevelSource } from "./Waveform";

export interface RecordButtonProps {
  recording: boolean;
  /** True while starting or stopping (the button is disabled meanwhile). */
  busy?: boolean;
  disabled?: boolean;
  onToggle: () => void;
  levels: LevelSource;
  /** Accessible labels in the app's language. */
  labels?: { start: string; stop: string };
}

/**
 * DESIGN.md §5 "The Record button": the app's single circular element.
 * Idle: Pulse Red fill, white icon. Recording: a Pulse Red ring around a live
 * waveform, with the 2 s breathing ring (the only perpetual animation).
 */
export function RecordButton({ recording, busy, disabled, onToggle, levels, labels }: RecordButtonProps) {
  return (
    <button
      type="button"
      className="ob-record"
      data-recording={recording || undefined}
      onClick={onToggle}
      disabled={disabled || busy}
      aria-pressed={recording}
      aria-label={recording ? (labels?.stop ?? "Stop recording") : (labels?.start ?? "Start recording")}
    >
      {recording ? (
        <>
          <span className="ob-record-ring" aria-hidden />
          <Waveform levels={levels} bars={9} className="ob-record-wave" />
          <Stop size={16} weight="fill" className="ob-record-stop" aria-hidden />
        </>
      ) : (
        <Microphone size={24} weight="regular" aria-hidden />
      )}
    </button>
  );
}
