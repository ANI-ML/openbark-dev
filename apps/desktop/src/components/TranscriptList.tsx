import { useEffect, useRef, useState } from "react";
import { fmtTime } from "../context";
import type { Segment } from "../engine";

export interface TranscriptListProps {
  segments: Segment[];
  /** Follow new segments until the user scrolls up (DESIGN.md §5 Transcript pane). */
  follow?: boolean;
  /** Segment ids lit by the evidence link. */
  highlighted?: Set<number>;
  playingId?: number | null;
  onSeek?: (s: Segment) => void;
  speakerNames?: string[];
}

export function TranscriptList({ segments, follow, highlighted, playingId, onSeek, speakerNames }: TranscriptListProps) {
  const box = useRef<HTMLOListElement>(null);
  const [detached, setDetached] = useState(false);

  useEffect(() => {
    if (follow && !detached) box.current?.lastElementChild?.scrollIntoView({ block: "nearest" });
  }, [segments.length, follow, detached]);

  useEffect(() => {
    if (!highlighted || highlighted.size === 0) return;
    const first = Math.min(...highlighted);
    box.current?.querySelector(`[data-id="${first}"]`)?.scrollIntoView({ block: "nearest", behavior: "smooth" });
  }, [highlighted]);

  function onScroll() {
    const el = box.current;
    if (!el || !follow) return;
    setDetached(el.scrollHeight - el.scrollTop - el.clientHeight > 48);
  }

  const name = (s: Segment) =>
    s.speaker == null ? "Speaker" : (speakerNames?.[s.speaker] ?? `Speaker ${s.speaker + 1}`);

  return (
    <div className="transcript-wrap">
      <ol className="transcript" ref={box} onScroll={onScroll}>
        {segments.map((s) => (
          <li
            key={s.id}
            data-id={s.id}
            data-lit={highlighted?.has(s.id) || undefined}
            data-playing={playingId === s.id || undefined}
          >
            <span className="turn-speaker label">{name(s)}</span>
            {onSeek ? (
              <button type="button" className="turn-text" onClick={() => onSeek(s)} title="Play from here">
                {s.text}
              </button>
            ) : (
              <span className="turn-text">{s.text}</span>
            )}
            <span className="turn-time mono faint">{fmtTime(s.start)}</span>
          </li>
        ))}
      </ol>
      {follow && detached && (
        <button
          type="button"
          className="ob-button jump"
          data-variant="ghost"
          onClick={() => {
            setDetached(false);
            box.current?.lastElementChild?.scrollIntoView({ block: "nearest" });
          }}
        >
          Jump to live
        </button>
      )}
    </div>
  );
}
