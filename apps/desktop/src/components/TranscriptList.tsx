import { memo, useEffect, useRef, useState } from "react";
import { fmtTime } from "../context";
import type { Hint, Segment } from "../engine";
import { useT } from "../i18n";

/** Text with each hinted phrase underlined (low-confidence words, dotted). */
export function HintedText({ text, hints }: { text: string; hints?: Hint[] }) {
  const t = useT();
  if (!hints?.length) return <>{text}</>;
  const parts: React.ReactNode[] = [];
  let rest = text;
  hints.forEach((h, i) => {
    const at = rest.toLowerCase().indexOf(h.heard.toLowerCase());
    if (at < 0) return;
    parts.push(rest.slice(0, at));
    const label = t('Heard "{heard}". Likely {likely}.', { heard: h.heard, likely: h.likely.join(` ${t("or")} `) });
    parts.push(
      <span key={i} className="hint" title={label} aria-label={label}>
        {rest.slice(at, at + h.heard.length)}
      </span>,
    );
    rest = rest.slice(at + h.heard.length);
  });
  parts.push(rest);
  return <>{parts}</>;
}

export interface TranscriptListProps {
  segments: Segment[];
  /** Follow new segments until the user scrolls up; the newest is the current utterance (in ink). */
  follow?: boolean;
  /** Segment ids lit by the evidence link. */
  highlighted?: Set<number>;
  playingId?: number | null;
  onSeek?: (s: Segment) => void;
  speakerNames?: string[];
}

/**
 * The transcript in the record's own type (docs/brand/DESIGN.md §4.1, §7.5):
 * mono 15/26 in secondary ink, with the utterance being heard now in ink
 * while following a live recording. Speaker labels are captions at weight
 * 600 and are never colour-coded.
 *
 * Memoised: Review redraws while a note is drafted, and a long visit's
 * transcript (700 lines and more) shouldn't redraw with it.
 */
export const TranscriptList = memo(function TranscriptList({
  segments,
  follow,
  highlighted,
  playingId,
  onSeek,
  speakerNames,
}: TranscriptListProps) {
  const t = useT();
  const box = useRef<HTMLOListElement>(null);
  const [detached, setDetached] = useState(false);

  /** Scroll the list itself to its end. scrollIntoView would scroll the page
   * too, carrying the recorder's controls and progress off a phone's screen. */
  const toEnd = () => {
    const el = box.current;
    if (el) el.scrollTop = el.scrollHeight;
  };

  useEffect(() => {
    if (follow && !detached) toEnd();
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
    s.speaker == null ? t("Speaker") : (speakerNames?.[s.speaker] ?? t("Speaker {n}", { n: s.speaker + 1 }));

  return (
    <div className="transcript-wrap">
      <ol className="transcript" ref={box} onScroll={onScroll}>
        {segments.map((s, i) => (
          <li
            key={s.id}
            data-id={s.id}
            data-lit={highlighted?.has(s.id) || undefined}
            data-playing={playingId === s.id || undefined}
            data-current={(follow && i === segments.length - 1) || undefined}
          >
            <span className="turn-speaker caption">{name(s)}</span>
            {onSeek ? (
              <button
                type="button"
                className="turn-text transcript-type"
                onClick={() => onSeek(s)}
                title={t("Play from here")}
              >
                <HintedText text={s.text} hints={s.hints} />
              </button>
            ) : (
              <span className="turn-text transcript-type">
                <HintedText text={s.text} hints={s.hints} />
              </span>
            )}
            <span className="turn-time mono faint">{fmtTime(s.start)}</span>
            {s.check && (s.check.primary_only.length > 0 || s.check.second_only.length > 0) && (
              <span className="turn-check small">{t("Second listen heard: {text}", { text: s.check.text })}</span>
            )}
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
            toEnd();
          }}
        >
          {t("Jump to live")}
        </button>
      )}
    </div>
  );
});
