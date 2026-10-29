import { Button } from "@openbark/ui";
import { Square } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import * as engine from "../engine";
import { useT } from "../i18n";
import { type BoardStep, ProgressBoard, type StepState } from "./ProgressBoard";

/** The drafting steps the engine reports, in order (os-engine live::Stage). */
const ORDER = ["load_model", "read_transcript", "write", "verify", "save"] as const;
type DraftStage = (typeof ORDER)[number] | "starting";

/**
 * While writing, this long without a new token or heartbeat means the model
 * is probably stuck. Only while writing: reading a long transcript on a slow
 * processor produces no tokens for minutes, and that's normal. A token takes
 * a few seconds at worst on the slowest computers OpenBark runs on.
 */
export const STUCK_AFTER_MS = 60_000;

/** How often the board redraws while text streams in (four times a second). */
const REDRAW_MS = 250;

/** What the model has done so far: kept outside React state, read by the board. */
export interface DraftLive {
  /** The streamed text (to follow the headings). */
  text: string;
  /** Prompt tokens read so far, of the total, while reading. */
  prompt: { done: number; total: number } | null;
  /** When reading started and when the last prompt batch came (for the estimate). */
  promptStartedAt: number;
  promptAt: number;
  /** Tokens written so far and the rate, from the engine's heartbeat. */
  tokens: number;
  tokensPerSecond: number | null;
  /** The last sign of life while writing (a token or a heartbeat). */
  lastTokenAt: number;
}

const freshLive = (): DraftLive => ({
  text: "",
  prompt: null,
  promptStartedAt: 0,
  promptAt: 0,
  tokens: 0,
  tokensPerSecond: null,
  lastTokenAt: 0,
});

export interface DraftTracker {
  /** Drafting right now. */
  active: boolean;
  stage: DraftStage | null;
  /** The note model had to be loaded for this draft. */
  loadedModel: boolean;
  startedAt: number;
  /**
   * The streamed text and speeds. Streamed tokens never go into the screen's
   * state: a long transcript re-rendered per token made weak laptops slower
   * still. The board subscribes and redraws a few times a second.
   */
  live: { current: DraftLive };
  subscribe: (fn: () => void) => () => void;
  /** Stop was pressed and the engine hasn't answered yet. */
  stopping: boolean;
  start: () => void;
  stop: () => void;
  /** Ask the engine to stop this draft (the board's Stop button). */
  cancel: () => void;
}

/**
 * Follow one draft (a note, a client document, a heading redraft): the
 * engine's drafting steps, the prompt being read, and the streamed text, so
 * the board can show which heading is being written. Call `start` just
 * before asking for the draft and `stop` when it returns.
 */
export function useDraftTracker(): DraftTracker {
  const [stage, setStage] = useState<DraftStage | null>(null);
  const [loadedModel, setLoadedModel] = useState(false);
  const [startedAt, setStartedAt] = useState(0);
  const [stopping, setStopping] = useState(false);
  const live = useRef<DraftLive>(freshLive());
  const listeners = useRef(new Set<() => void>());
  const active = useRef(false);
  const notify = () => listeners.current.forEach((f) => f());

  useEffect(() => {
    const offs = [
      engine.on("progress", (p) => {
        if (!active.current || !(ORDER as readonly string[]).includes(p.stage)) return;
        const next = p.stage as DraftStage;
        if (next === "load_model") setLoadedModel(true);
        if (next === "write" && live.current.lastTokenAt === 0) live.current.lastTokenAt = Date.now();
        // Steps only move forward (events from two channels can cross).
        setStage((prev) =>
          prev && prev !== "starting" && ORDER.indexOf(prev) > ORDER.indexOf(next as never) ? prev : next,
        );
      }),
      engine.on("token", (piece) => {
        if (!active.current) return;
        live.current.text += piece;
        live.current.lastTokenAt = Date.now();
        notify();
      }),
      engine.on("draft", (d) => {
        if (!active.current) return;
        const now = Date.now();
        if (d.phase === "prompt") {
          if (!live.current.prompt || d.done === 0) live.current.promptStartedAt = now;
          live.current.prompt = { done: d.done, total: d.total };
          live.current.promptAt = now;
        } else if (d.phase === "writing") {
          live.current.tokens = d.tokens;
          live.current.tokensPerSecond = d.tokens_per_second;
          live.current.lastTokenAt = now;
        }
        notify();
      }),
    ];
    return () => offs.forEach((o) => void o.then((f) => f()));
  }, []);

  const subscribe = useCallback((fn: () => void) => {
    listeners.current.add(fn);
    return () => void listeners.current.delete(fn);
  }, []);

  const start = useCallback(() => {
    active.current = true;
    live.current = freshLive();
    setLoadedModel(false);
    setStopping(false);
    setStartedAt(Date.now());
    setStage("starting");
    listeners.current.forEach((f) => f());
  }, []);
  const stop = useCallback(() => {
    active.current = false;
    setStopping(false);
    setStage(null);
  }, []);
  const cancel = useCallback(() => {
    if (!active.current) return;
    setStopping(true);
    void engine.cancelDraft().catch(() => setStopping(false));
  }, []);

  return { active: stage !== null, stage, loadedModel, startedAt, live, subscribe, stopping, start, stop, cancel };
}

/** Redraw on the tracker's live updates, at most every {@link REDRAW_MS}, and once a second for the clock. */
function useLiveTick(tracker: DraftTracker): number {
  const [tick, setTick] = useState(0);
  const { subscribe, stage } = tracker;
  useEffect(() => {
    let pending: ReturnType<typeof setTimeout> | null = null;
    const off = subscribe(() => {
      if (pending) return;
      pending = setTimeout(() => {
        pending = null;
        setTick((n) => n + 1);
      }, REDRAW_MS);
    });
    return () => {
      off();
      if (pending) clearTimeout(pending);
    };
  }, [subscribe]);
  useEffect(() => {
    if (!stage) return;
    const timer = setInterval(() => setTick((n) => n + 1), 1000);
    return () => clearInterval(timer);
  }, [stage]);
  return tick;
}

/**
 * A failed draft's message in the app language: the engine's plain messages
 * for a stopped, repeating or over-long draft (os-app), anything else as sent.
 */
export function draftErrorText(t: ReturnType<typeof useT>, e: unknown): string {
  const message = e instanceof Error ? e.message : String(e);
  switch (message) {
    case engine.DRAFT_STOPPED:
      return t("Drafting stopped. Nothing was saved.");
    case "The note model started repeating itself, so drafting stopped and nothing was saved. Try again, or choose another note model in Models.":
      return t(
        "The note model started repeating itself, so drafting stopped and nothing was saved. Try again, or choose another note model in Models.",
      );
    case "The note ran past its length limit before it was finished, so nothing was saved. Try again, or choose another note model in Models.":
      return t(
        "The note ran past its length limit before it was finished, so nothing was saved. Try again, or choose another note model in Models.",
      );
    default:
      return message;
  }
}

/** How far the writing has got: the last heading whose key has appeared in the output. */
export function headingsWritten(template: Pick<engine.Template, "sections"> | null, text: string): number {
  if (!template) return -1;
  let current = -1;
  template.sections.forEach((s, i) => {
    if (text.includes(`"${s.id}"`)) current = i;
  });
  return current;
}

const fmtSecs = (s: number) => (s < 60 ? `${s} s` : `${Math.floor(s / 60)} min ${String(s % 60).padStart(2, "0")} s`);

/**
 * Seconds left to read the prompt at the rate measured in this draft, or
 * null until there is a rate to go on (at least one batch and five seconds).
 * Never a guess: the rate is this computer's, for this transcript.
 */
export function readingSecondsLeft(live: Pick<DraftLive, "prompt" | "promptStartedAt" | "promptAt">): number | null {
  const p = live.prompt;
  if (!p || p.done <= 0 || p.total <= 0 || p.done >= p.total) return null;
  const secs = (live.promptAt - live.promptStartedAt) / 1000;
  if (secs < 5) return null;
  return Math.round(((p.total - p.done) * secs) / p.done);
}

/** "about 3 min", "about 40 s": an estimate, never to the second. */
function aboutTime(t: ReturnType<typeof useT>, secs: number): string {
  return secs >= 90
    ? t("about {n} min", { n: Math.round(secs / 60) })
    : t("about {n} s", { n: Math.max(5, Math.round(secs / 5) * 5) });
}

/**
 * The drafting board: load the model (when needed), read the transcript
 * (how much of it, and how long that should take at the speed measured),
 * write each heading (tokens and speed), check every number. For a redraft
 * it says so, and the current draft stays visible underneath until the new
 * one is ready. While the note is being written the step reads "Writing
 * note…" (docs/brand/DESIGN.md §8). With `onStop`, a Stop button ends the
 * draft and nothing is saved. If the model goes quiet while writing, the
 * board says it may be stuck. The active step takes the accent unless the
 * caller says something else on screen is already amber.
 */
export function DraftBoard({
  tracker,
  template,
  title,
  subtitle,
  lines,
  lastSecs,
  compact = false,
  accent = true,
  onStop,
}: {
  tracker: DraftTracker;
  /** The template being drafted with: its headings are listed and ticked. */
  template: Pick<engine.Template, "sections"> | null;
  title: string;
  subtitle?: string;
  /** Transcript lines being read. */
  lines?: number;
  /** How long the last draft of this visit took, for an estimate. */
  lastSecs?: number | null;
  compact?: boolean;
  /** Mark the active step with the accent; false when something else on screen is amber. */
  accent?: boolean;
  /** Show Stop: end this draft, saving nothing. */
  onStop?: () => void;
}) {
  const t = useT();
  useLiveTick(tracker);
  const { stage, loadedModel, startedAt, stopping } = tracker;
  if (!stage) return null;
  const live = tracker.live.current;
  const now = Date.now();
  const elapsed = Math.max(0, Math.round((now - startedAt) / 1000));
  const at = stage === "starting" ? ORDER.indexOf("read_transcript") : ORDER.indexOf(stage);
  const state = (s: (typeof ORDER)[number]): StepState => {
    const i = ORDER.indexOf(s);
    return i < at ? "done" : i === at ? "active" : "pending";
  };
  const sections = template?.sections ?? [];
  const writing = state("write");
  const reading = state("read_transcript");
  const current = writing === "active" ? Math.max(0, headingsWritten(template, live.text)) : -1;
  const quietSecs = writing === "active" && live.lastTokenAt > 0 ? Math.round((now - live.lastTokenAt) / 1000) : 0;
  const stuck = writing === "active" && quietSecs * 1000 >= STUCK_AFTER_MS;
  const children: BoardStep[] = compact
    ? []
    : sections.map((s, i) => ({
        key: s.id,
        label: s.title,
        state:
          writing === "done"
            ? "done"
            : writing === "pending"
              ? "pending"
              : i < current
                ? "done"
                : i === current
                  ? "active"
                  : "pending",
      }));
  const left = reading === "active" ? readingSecondsLeft(live) : null;
  const readDetail =
    reading === "active" && live.prompt && live.prompt.total > 0
      ? [
          t("{done} of {total}", {
            done: Math.min(live.prompt.done, live.prompt.total).toLocaleString(),
            total: live.prompt.total.toLocaleString(),
          }),
          left != null ? t("{time} left", { time: aboutTime(t, left) }) : null,
        ]
          .filter(Boolean)
          .join(", ")
      : lines
        ? lines === 1
          ? t("1 line")
          : t("{n} lines", { n: lines })
        : undefined;
  const writeDetail =
    writing === "active"
      ? [
          sections[current] ? t("Now: {heading}", { heading: sections[current].title }) : null,
          live.tokens > 0
            ? live.tokensPerSecond
              ? t("{n} tokens, {rate} a second", { n: live.tokens, rate: live.tokensPerSecond.toFixed(1) })
              : t("{n} tokens", { n: live.tokens })
            : null,
        ]
          .filter(Boolean)
          .join(". ")
      : undefined;
  const steps: BoardStep[] = [
    ...(loadedModel
      ? [
          {
            key: "model",
            label: t("Loading the note model"),
            detail: t("Only the first draft after opening OpenBark"),
            state: state("load_model"),
          },
        ]
      : []),
    {
      key: "read",
      label: t("Reading the transcript"),
      detail: readDetail || undefined,
      state: reading,
    },
    {
      key: "write",
      label: writing === "active" ? t("Writing note…") : t("Writing the note"),
      detail: writeDetail || undefined,
      state: writing,
      children,
    },
    {
      key: "verify",
      label: t("Checking every number against the transcript"),
      state: stage === "save" ? "done" : state("verify"),
    },
  ];
  // Model 0-6%, reading 6-16% (by tokens read), writing 16-95% by headings, checking and saving after.
  const readFraction = live.prompt && live.prompt.total > 0 ? Math.min(1, live.prompt.done / live.prompt.total) : 0;
  const fraction =
    stage === "load_model"
      ? 0.03
      : stage === "starting" || stage === "read_transcript"
        ? 0.06 + 0.1 * readFraction
        : stage === "write"
          ? 0.16 + 0.79 * (sections.length ? (Math.max(0, current) + 0.5) / sections.length : 0.5)
          : 0.97;
  const aside = (
    <>
      <span>
        {t("{time} so far", { time: fmtSecs(elapsed) })}
        {lastSecs
          ? `. ${t("The last draft of this visit took {time}.", { time: fmtSecs(Math.round(lastSecs)) })}`
          : "."}
      </span>
      {onStop && (
        <Button size="compact" variant="ghost" onClick={onStop} disabled={stopping}>
          <Square size={16} aria-hidden />
          {stopping ? t("Stopping…") : t("Stop")}
        </Button>
      )}
    </>
  );
  return (
    <>
      <ProgressBoard
        label={title}
        title={title}
        subtitle={subtitle}
        steps={steps}
        fraction={fraction}
        aside={aside}
        compact={compact}
        accent={accent}
      />
      {stuck && (
        <p className="ob-notice" data-tone="danger" role="alert">
          {t(
            "Nothing new from the note model for {time}. It may be stuck. Stop, then try again, or choose a smaller note model in Models.",
            { time: fmtSecs(quietSecs) },
          )}
        </p>
      )}
    </>
  );
}
