import { Button } from "@openbark/ui";
import { Fragment, useLayoutEffect, useMemo, useRef, useState, type KeyboardEvent, type MouseEvent, type ReactNode } from "react";
import {
  CHART,
  asItems,
  blocksSigning,
  byTooth,
  findingText,
  findingWords,
  glyphMarks,
  glyphOf,
  legendLines,
  lowerFirst,
  rowsOf,
  tooth,
  toothName,
  useLowerInner,
  type ChartFaithfulness,
  type Dentition,
  type Glyph,
  type ToothFinding,
} from "../dental";
import { fmtStamp, type ChartCheck } from "../engine";
import { useT } from "../i18n";
import { useMediaQuery } from "../useMediaQuery";
import "./DentalChart.css";

/**
 * The tooth chart (ADR-0021 §9): a hand-written SVG of the AVDC layout seen
 * from the front (patient's right on the viewer's left), the findings by
 * tooth as evidence-linked lines, and a legend of the codes used.
 *
 * Teeth that weren't mentioned are drawn blank, never "normal", and the
 * legend says so. Status is never colour alone: the marks are shapes, and the
 * list carries the words and AVDC codes.
 *
 * Look (docs/brand/DESIGN.md): ink lines on the surface, tokens only, so dark
 * mode follows the brand tokens; tooth numbers and codes in mono; the accent
 * only on the selected tooth while the chart has focus, and on the focus ring.
 *
 * Renders from props only. When its own width is below 768 px (a narrow
 * review pane as much as a phone), the mouth splits into two blocks
 * (patient's right, then left) so each tooth stays at least 24 px wide and
 * the tooth numbers at 11 px or more.
 *
 * A drafted chart is a draft until a veterinarian confirms it (ADR-0021
 * decision 10): "Draft chart: check every tooth", and each tooth with a
 * drafted entry carries a "Checked against the recording" checkbox. A tooth
 * with no flag is never shown as checked; only the vet's confirmation is.
 */
export interface DentalChartProps {
  /** Whose teeth to draw; null when the species wasn't entered (no diagram). */
  dentition: Dentition | null;
  /** The chart heading's value: one item per finding. */
  items: unknown;
  faithfulness?: ChartFaithfulness;
  /** JSON pointer of the chart heading ("/dental_chart"); items are `${path}/${i}`. */
  path?: string;
  onEvidence?: (ids: number[] | null) => void;
  onPlay?: (ids: number[]) => void;
  /** Selected teeth (highlighted in the diagram and the list). */
  selected?: string[];
  /** A tooth clicked (or Enter / Space on it). `additive`: Ctrl or Cmd held, or Space. */
  onToothActivate?: (id: string, how: { additive: boolean }) => void;
  /** Force the deciduous rows on or off; by default they show when a deciduous tooth is charted. */
  showDeciduous?: boolean;
  /** Show the per-tooth list and the legend (default true). */
  list?: boolean;
  /**
   * Activating a tooth toggles its selection (a signed chart), rather than
   * opening the editor: the teeth are then announced as toggle buttons.
   */
  toggles?: boolean;
  /** The chart can be edited (an unsigned draft): the no-dentition notice says how to choose. */
  editable?: boolean;
  /** The patient's species, read as dog or cat, when the chart has no dentition yet. */
  suggestedDentition?: Dentition | null;
  /** Write a dentition into the chart (an unsigned draft with none). */
  onChooseDentition?: (dentition: Dentition) => void;
  /**
   * Renders one evidence-linked finding, so the note view can pass its own
   * statement (hover lights the lines, click plays, flags as elsewhere).
   */
  renderStatement?: (item: ToothFinding, path: string, content: ReactNode) => ReactNode;
  /** The vet's confirmations of drafted teeth: who and when (ADR-0021 decision 10). */
  checks?: ChartCheck[];
  /**
   * Teeth with a drafted entry no veterinarian has confirmed. Given, the chart
   * shows its draft state; absent, it says nothing about confirmation.
   */
  unconfirmed?: string[];
  /** Confirm or take back teeth (an unsigned draft, for a veterinarian). */
  onConfirm?: (teeth: string[], confirmed: boolean) => void;
  /** A confirmation is being saved. */
  confirming?: boolean;
}

/** Narrower than this, the mouth is drawn in two blocks (DESIGN.md §9 breakpoint 768). */
export const TWO_BLOCKS = "(max-width: 767px)";
/** The same breakpoint, against the chart's own width. */
export const TWO_BLOCKS_WIDTH = 768;

/**
 * The element's width, followed with a ResizeObserver; null where there is
 * none (component tests), so the caller falls back to the viewport.
 */
function useWidth<E extends HTMLElement>(): [React.RefObject<E | null>, number | null] {
  const ref = useRef<E | null>(null);
  const [width, setWidth] = useState<number | null>(null);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el || typeof ResizeObserver === "undefined") return;
    const read = (w: number) => setWidth(w > 0 ? w : null);
    read(el.getBoundingClientRect().width);
    const observer = new ResizeObserver((entries) => read(entries[0]?.contentRect.width ?? 0));
    observer.observe(el);
    return () => observer.disconnect();
  }, []);
  return [ref, width];
}

const SLOT = CHART.geometry.slot;
const [TOOTH_W, TOOTH_H] = CHART.geometry.tooth;
// The chart's vertical layout in px; the PDF uses the same numbers (pdf.rs).
const CAPTION_H = 16;
const ROW_H = 64;
const NUMBER_BAND = 14;
const MARK_BAND = 10;
const ROW_GAP = 4;
const BITE_GAP = 8;
// A flag beside the tooth number, inside the slot: the number (12 px mono,
// about 22 px wide for three digits) moves left, the mark sits at the right.
const NUMBER_X_FLAGGED = SLOT / 2 - 4;
const BADGE_R = 4;
const MARK_X = SLOT - BADGE_R - 0.5;

type T = ReturnType<typeof useT>;

interface Slot {
  id: string;
  row: number;
  /** From the midline: -1 is the first tooth on the patient's right, 1 the first on the left. */
  offset: number;
}

function layout(dentition: Dentition, deciduous: boolean) {
  const rows = rowsOf(dentition, deciduous);
  const tops: number[] = [];
  let y = CAPTION_H;
  rows.forEach((r, i) => {
    if (i > 0) y += rows[i - 1]?.upper === r.upper ? ROW_GAP : BITE_GAP;
    tops.push(y);
    y += ROW_H;
  });
  const slots: Slot[][] = rows.map((r, row) => {
    const right = r.teeth.filter((id) => tooth(id)?.side === "right").length;
    return r.teeth.map((id, i) => ({ id, row, offset: i < right ? i - right : i - right + 1 }));
  });
  const half = Math.max(...slots.map((s) => Math.max(s.filter((x) => x.offset < 0).length, s.filter((x) => x.offset > 0).length)));
  return { rows, tops, height: y, slots, half };
}

/** The slot's centre in slot widths from the midline. */
const position = (offset: number) => (offset < 0 ? offset + 0.5 : offset - 0.5);

/** Each glyph once per tooth; dashed only when every finding with it is a plan. */
function marksOf(items: ToothFinding[]): { glyph: Glyph; planned: boolean }[] {
  const out: { glyph: Glyph; planned: boolean }[] = [];
  for (const item of items) {
    const glyph = glyphOf(item);
    const seen = out.find((m) => m.glyph === glyph);
    if (seen) seen.planned = seen.planned && !!item.planned;
    else out.push({ glyph, planned: !!item.planned });
  }
  return out;
}

/**
 * Faithfulness flags by item path: `hard` the numbers said nowhere, `blocking`
 * the tooth flags that block signing (both stop signing), and `soft` what to
 * look at before signing. `describe` names another item of the chart by its
 * path, for a conflict.
 */
export function chartFlags(f: ChartFaithfulness | undefined, t: T, describe?: (path: string) => string | undefined) {
  const hard = new Map<string, string[]>();
  const blocking = new Map<string, string[]>();
  const soft = new Map<string, string[]>();
  const addSoft = (p: string, m: string) => soft.set(p, [...(soft.get(p) ?? []), m]);
  const addBlocking = (p: string, m: string) => blocking.set(p, [...(blocking.get(p) ?? []), m]);
  if (!f) return { hard, soft, blocking };
  for (const u of f.unsupported_numbers ?? []) hard.set(u.path, [...(hard.get(u.path) ?? []), u.number]);
  for (const m of f.citation_mismatches ?? [])
    addSoft(m.path, t('"{number}" is in the transcript, but not in the cited line. Check the citation.', { number: m.number }));
  for (const p of f.invalid_evidence ?? []) addSoft(p, t("No matching transcript. Keep, edit, or remove?"));
  for (const u of f.unsupported_statements ?? [])
    addSoft(u.path, t("Some of this isn't in the cited line ({words}). Check before signing.", { words: u.missing.join(", ") }));
  for (const d of f.disputed_numbers ?? [])
    addSoft(
      d.path,
      t("The second recogniser heard {heard} instead of {number}. Listen to the recording before signing.", {
        heard: d.heard_instead.join(` ${t("or")} `),
        number: d.number,
      }),
    );
  for (const flag of f.tooth_flags ?? []) {
    if (flag.kind === "name_conflict" && flag.other)
      addSoft(
        flag.path,
        t("Said as {number} and as {name} ({other}). Check which tooth before signing.", {
          number: flag.tooth,
          name: toothName(t, flag.other),
          other: flag.other,
        }),
      );
    else if (flag.kind === "cue_missing") addSoft(flag.path, t("The cited line doesn't mention this finding. Check before signing."));
    else if (flag.kind === "other_tooth")
      addSoft(flag.path, t("The cited line says this about another tooth. Check which tooth before signing."));
    else if (flag.kind === "negated") addSoft(flag.path, t("The cited line says this wasn't there. Check before signing."));
    else if (flag.kind === "other_type")
      addSoft(
        flag.path,
        t("The cited line also has the words for {type}. Check the type before signing.", {
          type: lowerFirst(t(flag.expected?.[0] ?? "")),
        }),
      );
    else if (flag.kind === "planned")
      addSoft(flag.path, t("Charted as done, but the cited line sounds like a plan. Check before signing."));
    else if (flag.kind === "not_planned")
      addSoft(flag.path, t("Charted as a plan, but the cited line says it was done. Check before signing."));
    else if (flag.kind === "history")
      addSoft(flag.path, t("The cited line says this was done at an earlier visit. Check before signing."));
    else if (flag.kind === "uncertain")
      addSoft(flag.path, t("The cited line says this only as possible, to be checked, or as someone's belief. Check before signing."));
    else if (flag.kind === "ambiguous")
      addSoft(flag.path, t("The cited line pairs teeth and findings by their order. Check which tooth has which finding before signing."));
    else if (flag.kind === "contradicted")
      addSoft(flag.path, t("Another line of the transcript says something else about this tooth. Check before signing."));
    else if (flag.kind === "other_pet")
      addSoft(flag.path, t("The cited line says this about another animal in the visit. Check before signing."));
    else if (flag.kind === "conflict") {
      const other = (flag.other && describe?.(flag.other)) || flag.expected?.[0] || "";
      addSoft(flag.path, t("This can't be right beside {finding} on the same tooth. Check before signing.", { finding: other }));
    } else if (flag.kind === "shape") addSoft(flag.path, t("This finding's details don't fit together. Check before signing."));
    else if (flag.kind === "duplicate") addSoft(flag.path, t("This finding is listed twice."));
    else if (flag.kind === "not_in_mouth")
      addBlocking(
        flag.path,
        flag.expected?.[0] === "cat"
          ? t("{tooth} isn't a cat's tooth. Correct the chart before signing.", { tooth: flag.tooth })
          : t("{tooth} isn't a dog's tooth. Correct the chart before signing.", { tooth: flag.tooth }),
      );
    else if (flag.kind === "choose_dentition")
      addBlocking(flag.path, t("Only a dog has {tooth}. Edit the chart to choose dog or cat before signing.", { tooth: flag.tooth }));
  }
  return { hard, soft, blocking };
}

/** Paths with a tooth flag, split by whether it blocks signing (for the summary above the note). */
export function toothFlagPaths(f: ChartFaithfulness | undefined): { blocking: Set<string>; soft: Set<string> } {
  const blocking = new Set<string>();
  const soft = new Set<string>();
  for (const flag of f?.tooth_flags ?? []) (blocksSigning(flag.kind) ? blocking : soft).add(flag.path);
  for (const p of blocking) soft.delete(p);
  return { blocking, soft };
}

export interface ToothDiagramProps {
  dentition: Dentition;
  items: ToothFinding[];
  showDeciduous: boolean;
  selected?: string[];
  /** Teeth with a faithfulness flag: a filled "!" badge (blocks signing) or a plain "!" beside the number. */
  flagged?: Map<string, "hard" | "soft">;
  onToothActivate?: (id: string, how: { additive: boolean }) => void;
  onEvidence?: (ids: number[] | null) => void;
  /** Activation toggles the tooth's selection: the teeth are toggle buttons (aria-pressed). */
  toggles?: boolean;
  /** Drafted teeth and their confirmation: "draft" (not confirmed yet) or the confirmer's name. */
  confirmation?: Map<string, { draft: true } | { by: string }>;
}

/**
 * The diagram alone: one tab stop with a roving tabindex across the teeth.
 * Arrows move along a row (across the midline) and to the nearest tooth of
 * the next row; Home and End go to the row's ends; Enter activates a tooth,
 * Space adds it to the selection.
 */
export function ToothDiagram({
  dentition,
  items,
  showDeciduous,
  selected = [],
  flagged,
  onToothActivate,
  onEvidence,
  toggles = false,
  confirmation,
}: ToothDiagramProps) {
  const t = useT();
  const lowerInner = useLowerInner();
  const narrowViewport = useMediaQuery(TWO_BLOCKS);
  const [box, width] = useWidth<HTMLDivElement>();
  const twoBlocks = width != null ? width < TWO_BLOCKS_WIDTH : narrowViewport;
  const { rows, tops, height, slots, half } = useMemo(() => layout(dentition, showDeciduous), [dentition, showDeciduous]);
  const found = useMemo(() => {
    const m = new Map<string, ToothFinding[]>();
    for (const item of items) m.set(item.tooth, [...(m.get(item.tooth) ?? []), item]);
    return m;
  }, [items]);
  const all = slots.flat();
  const [focusState, setFocus] = useState<string | null>(null);
  const focusId =
    focusState && all.some((s) => s.id === focusState)
      ? focusState
      : (selected.find((id) => all.some((s) => s.id === id)) ?? all[0]?.id ?? null);
  const refs = useRef(new Map<string, SVGGElement>());

  const label = (id: string) => {
    const name = toothName(t, id);
    const here = found.get(id) ?? [];
    if (here.length === 0) return t("{tooth}, {name}: not charted", { tooth: id, name });
    const words = here.map((i) => findingWords(t, i, lowerInner)).map((w) => (lowerInner ? lowerFirst(w) : w));
    const base = t("{tooth}, {name}: {findings}", { tooth: id, name, findings: words.join("; ") });
    const flag = flagged?.get(id);
    const state = confirmation?.get(id);
    const parts = [base];
    if (flag === "hard") parts.push(t("Blocks signing until it's corrected."));
    if (flag === "soft") parts.push(t("Needs checking before signing."));
    if (state && "draft" in state) parts.push(t("Draft, not confirmed yet."));
    if (state && "by" in state) parts.push(t("Confirmed by {name}.", { name: state.by }));
    return parts.join(". ").replace(/\.\. /g, ". ");
  };

  const moveTo = (e: KeyboardEvent, target: Slot | undefined) => {
    e.preventDefault();
    if (!target) return;
    setFocus(target.id);
    refs.current.get(target.id)?.focus();
  };

  const onKey = (e: KeyboardEvent<SVGGElement>, slot: Slot) => {
    const row = slots[slot.row] ?? [];
    const at = row.findIndex((s) => s.id === slot.id);
    const nearest = (r: number) => {
      const other = slots[r];
      if (!other?.length) return undefined;
      const x = position(slot.offset);
      return other.reduce((best, s) => (Math.abs(position(s.offset) - x) < Math.abs(position(best.offset) - x) ? s : best));
    };
    switch (e.key) {
      case "ArrowLeft":
        return moveTo(e, row[at - 1]);
      case "ArrowRight":
        return moveTo(e, row[at + 1]);
      case "Home":
        return moveTo(e, row[0]);
      case "End":
        return moveTo(e, row[row.length - 1]);
      case "ArrowUp":
        return moveTo(e, nearest(slot.row - 1));
      case "ArrowDown":
        return moveTo(e, nearest(slot.row + 1));
      case "Enter":
        e.preventDefault();
        return onToothActivate?.(slot.id, { additive: false });
      case " ":
        e.preventDefault();
        return onToothActivate?.(slot.id, { additive: true });
    }
  };

  const evidenceOf = (id: string) => [...new Set((found.get(id) ?? []).flatMap((i) => (Array.isArray(i.evidence) ? i.evidence : [])))];

  const toothEl = (slot: Slot, midline: number) => {
    const info = tooth(slot.id);
    if (!info) return null;
    const upper = rows[slot.row]?.upper ?? true;
    const top = tops[slot.row] ?? 0;
    const here = found.get(slot.id) ?? [];
    const isSelected = selected.includes(slot.id);
    const flag = flagged?.get(slot.id);
    const x = midline + position(slot.offset) * SLOT - SLOT / 2;
    const numberY = upper ? 11 : ROW_H - 2;
    const box = upper
      ? `translate(${(SLOT - TOOTH_W) / 2} ${NUMBER_BAND + TOOTH_H}) scale(1 -1)`
      : `translate(${(SLOT - TOOTH_W) / 2} ${MARK_BAND})`;
    return (
      <g
        key={slot.id}
        ref={(el) => {
          if (el) refs.current.set(slot.id, el);
          else refs.current.delete(slot.id);
        }}
        className="dc-tooth"
        role="button"
        tabIndex={slot.id === focusId ? 0 : -1}
        aria-label={label(slot.id)}
        aria-pressed={onToothActivate && toggles ? isSelected : undefined}
        data-tooth={slot.id}
        data-charted={here.length ? true : undefined}
        data-selected={isSelected ? true : undefined}
        transform={`translate(${x} ${top})`}
        onClick={(e: MouseEvent) => onToothActivate?.(slot.id, { additive: e.ctrlKey || e.metaKey })}
        onKeyDown={(e) => onKey(e, slot)}
        onFocus={() => {
          setFocus(slot.id);
          onEvidence?.(evidenceOf(slot.id));
        }}
        onBlur={() => onEvidence?.(null)}
        onMouseEnter={() => onEvidence?.(evidenceOf(slot.id))}
        onMouseLeave={() => onEvidence?.(null)}
      >
        <rect className="dc-hit" width={SLOT} height={ROW_H} />
        <rect className="dc-focus" x={1} y={1} width={SLOT - 2} height={ROW_H - 2} rx={6} />
        <g transform={box}>
          <path className="dc-outline" d={CHART.shapes[info.shape]} />
          {marksOf(here).flatMap(({ glyph, planned }) =>
            glyphMarks(glyph).map((m, k) => (
              <path
                key={`${glyph}-${k}`}
                className="dc-mark"
                data-glyph={glyph}
                data-filled={m.filled ? true : undefined}
                data-planned={planned && !m.filled ? true : undefined}
                strokeDasharray={planned && !m.filled ? "3 2" : undefined}
                d={m.d}
              />
            )),
          )}
        </g>
        {/* A flagged tooth's number moves left so its mark sits clear of the last digit. */}
        <text className="dc-num" x={flag ? NUMBER_X_FLAGGED : SLOT / 2} y={numberY} textAnchor="middle" aria-hidden>
          {slot.id}
        </text>
        {flag === "hard" && <circle className="dc-flag-badge" cx={MARK_X} cy={numberY - 4} r={BADGE_R} aria-hidden />}
        {flag && (
          <text className="dc-flag" data-kind={flag} x={MARK_X} y={flag === "hard" ? numberY - 0.5 : numberY} textAnchor="middle" aria-hidden>
            !
          </text>
        )}
      </g>
    );
  };

  const block = (side: "both" | "right" | "left") => {
    const width = (side === "both" ? 2 : 1) * half * SLOT;
    const midline = side === "left" ? 0 : side === "right" ? width : width / 2;
    const shown = all.filter((s) => side === "both" || (side === "right" ? s.offset < 0 : s.offset > 0));
    const captions: [string, number][] =
      side === "both"
        ? [
            [t("Patient's right"), width / 4],
            [t("Patient's left"), (3 * width) / 4],
          ]
        : [[side === "right" ? t("Patient's right") : t("Patient's left"), width / 2]];
    return (
      <svg
        key={side}
        className="dc-svg"
        data-block={side}
        viewBox={`0 0 ${width} ${height}`}
        width={width}
        role="group"
        aria-label={side === "both" ? t("Tooth chart") : captions[0]?.[0]}
      >
        {captions.map(([text, cx]) => (
          <text key={text} className="dc-caption" x={cx} y={11} textAnchor="middle" aria-hidden>
            {text}
          </text>
        ))}
        {side === "both" && <line className="dc-midline" x1={midline} x2={midline} y1={CAPTION_H} y2={height} />}
        {shown.map((s) => toothEl(s, midline))}
      </svg>
    );
  };

  return (
    <div className="dc-diagrams" ref={box} data-blocks={twoBlocks ? 2 : 1}>
      {twoBlocks ? [block("right"), block("left")] : block("both")}
    </div>
  );
}

export function DentalChart({
  dentition,
  items: value,
  faithfulness,
  path = "/dental_chart",
  onEvidence,
  onPlay,
  selected = [],
  onToothActivate,
  showDeciduous,
  list = true,
  renderStatement,
  toggles = false,
  editable = false,
  suggestedDentition = null,
  onChooseDentition,
  checks = [],
  unconfirmed,
  onConfirm,
  confirming = false,
}: DentalChartProps) {
  const t = useT();
  const lowerInner = useLowerInner();
  const items = useMemo(() => asItems(value), [value]);
  const { hard, soft, blocking } = useMemo(() => {
    const describe = (p: string) => {
      const index = Number(p.slice(path.length + 1));
      const item = p.startsWith(`${path}/`) && Number.isInteger(index) ? items[index] : undefined;
      return item ? findingText(t, item, lowerInner) : undefined;
    };
    return chartFlags(faithfulness, t, describe);
  }, [faithfulness, t, items, path, lowerInner]);

  if (items.length === 0) return <p className="not-discussed">{t("Not discussed")}</p>;

  const groups = byTooth(items);
  const flagged = new Map<string, "hard" | "soft">();
  for (const [id, entries] of groups) {
    const paths = entries.map((e) => `${path}/${e.index}`);
    if (paths.some((p) => hard.has(p) || blocking.has(p))) flagged.set(id, "hard");
    else if (paths.some((p) => soft.has(p))) flagged.set(id, "soft");
  }
  const deciduous = showDeciduous ?? items.some((i) => tooth(i.tooth)?.deciduous);
  // The draft state (ADR-0021 decision 10): teeth with an entry the model
  // drafted, and whether a veterinarian has confirmed each.
  const isVet = (i: ToothFinding) => typeof i.entered_by === "string" && i.entered_by !== "";
  const drafted = groups.filter(([, entries]) => entries.some(({ item }) => !isVet(item))).map(([id]) => id);
  const showDraft = unconfirmed !== undefined && drafted.length > 0;
  const waiting = showDraft ? drafted.filter((id) => unconfirmed.includes(id)) : [];
  const checkOf = (id: string) => [...checks].reverse().find((c) => c.tooth === id);
  const confirmation = new Map<string, { draft: true } | { by: string }>();
  if (showDraft)
    for (const id of drafted) {
      const check = checkOf(id);
      if (waiting.includes(id)) confirmation.set(id, { draft: true });
      else if (check) confirmation.set(id, { by: check.by });
    }

  const statement = (item: ToothFinding, p: string, content: ReactNode) => {
    if (renderStatement) return renderStatement(item, p, content);
    const ev = Array.isArray(item.evidence) ? item.evidence : [];
    const softMsg = soft.get(p)?.join(" ");
    const vet = typeof item.entered_by === "string" && item.entered_by ? item.entered_by : null;
    return (
      <span
        className="stmt"
        data-soft={softMsg ? true : undefined}
        data-vet={vet ? true : undefined}
        tabIndex={0}
        role="button"
        title={
          vet
            ? ev.length
              ? t("Entered by {name}; play the recording it came from", { name: vet })
              : t("Entered by {name}; not from the recording", { name: vet })
            : (softMsg ?? (ev.length ? t("Play the supporting audio") : undefined))
        }
        onMouseEnter={() => onEvidence?.(ev)}
        onMouseLeave={() => onEvidence?.(null)}
        onFocus={() => onEvidence?.(ev)}
        onBlur={() => onEvidence?.(null)}
        onClick={() => ev.length && onPlay?.(ev)}
        onKeyDown={(e) => (e.key === "Enter" || e.key === " ") && ev.length && (e.preventDefault(), onPlay?.(ev))}
      >
        {content}
        {vet && <span className="vet-mark small faint">{` ${t("Edited by {name}", { name: vet })}`}</span>}
      </span>
    );
  };

  const legend = list ? legendLines(t, items, lowerInner) : [];
  if (list && dentition) {
    legend.push(t("Blank: not charted in this recording. It doesn't mean normal."), t("Right and left are the patient's."));
  }

  return (
    <div className="dc">
      {showDraft && editable && waiting.length > 0 && (
        <div className="dc-draft" role="status" data-state="draft">
          <p className="dc-draft-title">{t("Draft chart: check every tooth")}</p>
          <p>
            {onConfirm
              ? t("Compare each drafted tooth with the recording, then tick Checked against the recording. Signing stays off until every drafted tooth is confirmed.")
              : t("A veterinarian checks each drafted tooth against the recording before the note can be signed.")}
          </p>
          <p className="small">{t("{n} of {m} drafted teeth confirmed", { n: drafted.length - waiting.length, m: drafted.length })}</p>
        </div>
      )}
      {showDraft && editable && waiting.length === 0 && (
        <p className="dc-draft small" role="status" data-state="confirmed">
          {t("Every drafted tooth is confirmed.")}
        </p>
      )}
      {dentition && (
        <ToothDiagram
          dentition={dentition}
          items={items}
          showDeciduous={deciduous}
          selected={selected}
          flagged={flagged}
          onToothActivate={onToothActivate}
          onEvidence={onEvidence}
          toggles={toggles}
          confirmation={showDraft ? confirmation : undefined}
        />
      )}
      {list && (
        <ul className="dc-list">
          {groups.map(([id, entries]) => {
            const hardNums = entries.flatMap((e) => hard.get(`${path}/${e.index}`) ?? []);
            const blocks = [...new Set(entries.flatMap((e) => blocking.get(`${path}/${e.index}`) ?? []))];
            return (
              <li
                key={id}
                className={hardNums.length || blocks.length ? "dc-line hardfail" : "dc-line"}
                data-tooth={id}
                data-selected={selected.includes(id) ? true : undefined}
              >
                <div className="dc-line-main">
                  <span className="dc-line-tooth">{id}</span>
                  <span className="dc-line-findings">
                    {entries.map(({ item, index }, n) => {
                      const text = findingText(t, item, lowerInner);
                      return (
                        <Fragment key={index}>
                          {n > 0 && "; "}
                          {statement(item, `${path}/${index}`, n > 0 && lowerInner ? lowerFirst(text) : text)}
                        </Fragment>
                      );
                    })}
                  </span>
                </div>
                {hardNums.length > 0 && (
                  <p className="small">
                    {[...new Set(hardNums)].length === 1
                      ? t("{numbers} isn't anywhere in the transcript. Check the recording and correct this before signing.", {
                          numbers: [...new Set(hardNums)].join(", "),
                        })
                      : t("{numbers} aren't anywhere in the transcript. Check the recording and correct this before signing.", {
                          numbers: [...new Set(hardNums)].join(", "),
                        })}
                  </p>
                )}
                {blocks.map((m) => (
                  <p key={m} className="small">
                    {m}
                  </p>
                ))}
                {showDraft && drafted.includes(id) && (
                  <div className="dc-confirm small" data-state={waiting.includes(id) ? "draft" : "confirmed"}>
                    {onConfirm && (
                      <label className="dc-confirm-check">
                        <input
                          type="checkbox"
                          checked={!waiting.includes(id)}
                          disabled={confirming}
                          aria-label={t("{tooth}: checked against the recording", { tooth: id })}
                          onChange={(e) => onConfirm([id], e.target.checked)}
                        />
                        <span>{t("Checked against the recording")}</span>
                      </label>
                    )}
                    {waiting.includes(id)
                      ? !onConfirm && <span className="dc-draft-tag">{t("Draft, not confirmed yet")}</span>
                      : (() => {
                          const check = checkOf(id);
                          return (
                            check && (
                              <span className="dc-confirmed muted">
                                {t("Confirmed by {name}, {at}", { name: check.by, at: fmtStamp(check.at) })}
                              </span>
                            )
                          );
                        })()}
                  </div>
                )}
              </li>
            );
          })}
        </ul>
      )}
      {!dentition &&
        (editable ? (
          <div className="dc-notice small">
            <p>{t("Dentition not recorded. Edit the chart to choose dog or cat.")}</p>
            {suggestedDentition && onChooseDentition && (
              <p>
                <Button variant="ghost" size="compact" onClick={() => onChooseDentition(suggestedDentition)}>
                  {suggestedDentition === "feline"
                    ? t("The patient is recorded as a cat: use a cat's chart")
                    : t("The patient is recorded as a dog: use a dog's chart")}
                </Button>
              </p>
            )}
          </div>
        ) : (
          <p className="dc-notice small">{t("Dentition not recorded.")}</p>
        ))}
      {legend.length > 0 && (
        <ul className="dc-legend small" aria-label={t("Legend")}>
          {legend.map((line) => {
            const at = line.indexOf(":");
            const code = at > 0 && /^[A-Z0-9/ -]+$/.test(line.slice(0, at).trim()) ? line.slice(0, at) : null;
            return (
              <li key={line}>
                {code ? (
                  <>
                    <span className="dc-code">{code}</span>
                    {line.slice(at)}
                  </>
                ) : (
                  line
                )}
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}
