import { useEffect, useRef } from "react";

/**
 * A source of microphone levels (0..1). Kept outside React state so the
 * waveform redraws on every level without re-rendering the app shell
 * (DESIGN.md §7: isolate live components).
 */
export interface LevelSource {
  subscribe(listener: (level: number) => void): () => void;
}

export function createLevelSource(): LevelSource & { push(level: number): void } {
  const listeners = new Set<(level: number) => void>();
  return {
    subscribe(l) {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    push(level) {
      listeners.forEach((l) => l(level));
    },
  };
}

export interface WaveformProps {
  levels: LevelSource;
  /** Number of bars (history length). */
  bars?: number;
  className?: string;
}

/** Rolling level bars, drawn straight to the DOM from data (no easing). */
export function Waveform({ levels, bars = 48, className }: WaveformProps) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const spans = Array.from(el.children) as HTMLSpanElement[];
    const history = new Array<number>(bars).fill(0);
    return levels.subscribe((level) => {
      history.shift();
      history.push(level);
      spans.forEach((s, i) => {
        s.style.transform = `scaleY(${Math.max(0.08, history[i] ?? 0)})`;
      });
    });
  }, [levels, bars]);
  return (
    <div ref={ref} className={className ? `ob-wave ${className}` : "ob-wave"} aria-hidden>
      {Array.from({ length: bars }, (_, i) => (
        <span key={i} />
      ))}
    </div>
  );
}
