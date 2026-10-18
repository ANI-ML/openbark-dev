import { useCallback, useEffect, useRef, useState } from "react";
import * as engine from "../engine";
import { useT } from "../i18n";
import { type BoardStep, ProgressBoard, type StepState } from "./ProgressBoard";

/** The drafting steps the engine reports, in order (os-engine live::Stage). */
const ORDER = ["load_model", "read_transcript", "write", "verify", "save"] as const;
type DraftStage = (typeof ORDER)[number] | "starting";

export interface DraftTracker {
  /** Drafting right now. */
  active: boolean;
  stage: DraftStage | null;
  /** The note model had to be loaded for this draft. */
  loadedModel: boolean;
  /** What the model has written so far. */
  text: string;
  startedAt: number;
  /** Seconds since the draft started (ticks once a second). */
  elapsed: number;
  start: () => void;
  stop: () => void;
}

/**
 * Follow one draft (a note, a client document, a heading redraft): the
 * engine's drafting steps and the streamed text, so the board can show which
 * heading is being written. Call `start` just before asking for the draft and
 * `stop` when it returns.
 */
export function useDraftTracker(): DraftTracker {
  const [stage, setStage] = useState<DraftStage | null>(null);
  const [loadedModel, setLoadedModel] = useState(false);
  const [text, setText] = useState("");
  const [startedAt, setStartedAt] = useState(0);
  const [now, setNow] = useState(0);
  const live = useRef(false);

  useEffect(() => {
    const offs = [
      engine.on("progress", (p) => {
        if (!live.current || !(ORDER as readonly string[]).includes(p.stage)) return;
        const next = p.stage as DraftStage;
        if (next === "load_model") setLoadedModel(true);
        // Steps only move forward (events from two channels can cross).
        setStage((prev) =>
          prev && prev !== "starting" && ORDER.indexOf(prev) > ORDER.indexOf(next as never) ? prev : next,
        );
      }),
      engine.on("token", (piece) => {
        if (live.current) setText((t) => t + piece);
      }),
    ];
    return () => offs.forEach((o) => void o.then((f) => f()));
  }, []);

  useEffect(() => {
    if (!stage) return;
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, [stage]);

  const start = useCallback(() => {
    live.current = true;
    setText("");
    setLoadedModel(false);
    const t = Date.now();
    setStartedAt(t);
    setNow(t);
    setStage("starting");
  }, []);
  const stop = useCallback(() => {
    live.current = false;
    setStage(null);
  }, []);

  return {
    active: stage !== null,
    stage,
    loadedModel,
    text,
    startedAt,
    elapsed: stage ? Math.max(0, Math.round((now - startedAt) / 1000)) : 0,
    start,
    stop,
  };
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
 * The drafting board: load the model (when needed), read the transcript,
 * write each heading, check every number. For a redraft it says so, and the
 * current draft stays visible underneath until the new one is ready.
 * While the note is being written the step reads "Writing note…"
 * (docs/brand/DESIGN.md §8). The active step takes the accent unless the
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
}) {
  const t = useT();
  const { stage, loadedModel, text, elapsed } = tracker;
  if (!stage) return null;
  const at = stage === "starting" ? ORDER.indexOf("read_transcript") : ORDER.indexOf(stage);
  const state = (s: (typeof ORDER)[number]): StepState => {
    const i = ORDER.indexOf(s);
    return i < at ? "done" : i === at ? "active" : "pending";
  };
  const sections = template?.sections ?? [];
  const writing = state("write");
  const current = writing === "active" ? Math.max(0, headingsWritten(template, text)) : -1;
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
      detail: lines ? (lines === 1 ? t("1 line") : t("{n} lines", { n: lines })) : undefined,
      state: state("read_transcript"),
    },
    {
      key: "write",
      label: writing === "active" ? t("Writing note…") : t("Writing the note"),
      detail:
        writing === "active" && sections[current]
          ? t("Now: {heading}", { heading: sections[current].title })
          : undefined,
      state: writing,
      children,
    },
    {
      key: "verify",
      label: t("Checking every number against the transcript"),
      state: stage === "save" ? "done" : state("verify"),
    },
  ];
  // Model 0-6%, reading 6-16%, writing 16-95% by headings, checking and saving after.
  const fraction =
    stage === "load_model"
      ? 0.03
      : stage === "starting" || stage === "read_transcript"
        ? 0.1
        : stage === "write"
          ? 0.16 + 0.79 * (sections.length ? (Math.max(0, current) + 0.5) / sections.length : 0.5)
          : 0.97;
  const aside = (
    <>
      {t("{time} so far", { time: fmtSecs(elapsed) })}
      {lastSecs ? `. ${t("The last draft of this visit took {time}.", { time: fmtSecs(Math.round(lastSecs)) })}` : "."}
    </>
  );
  return (
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
  );
}
