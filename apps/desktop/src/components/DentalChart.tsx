import { Fragment, useMemo, useRef, useState, type KeyboardEvent, type MouseEvent, type ReactNode } from "react";
import {
  CHART,
  asItems,
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
 * Renders from props only. Below 768 px the mouth splits into two blocks
 * (patient's right, then left) so each tooth stays at least 24 px wide.
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
   * Renders one evidence-linked finding, so the note view can pass its own
   * statement (hover lights the lines, click plays, flags as elsewhere).
   */
  renderStatement?: (item: ToothFinding, path: string, content: ReactNode) => ReactNode;
}

/** Narrower than this, the mouth is drawn in two blocks (DESIGN.md §9 breakpoint 768). */
export const TWO_BLOCKS = "(max-width: 767px)";

const SLOT = CHART.geometry.slot;
const [TOOTH_W, TOOTH_H] = CHART.geometry.tooth;
// The chart's vertical layout in px; the PDF uses the same numbers (pdf.rs).
const CAPTION_H = 16;
const ROW_H = 64;
const NUMBER_BAND = 14;
const MARK_BAND = 10;
const ROW_GAP = 4;
const BITE_GAP = 8;

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

/** Faithfulness flags by item path: hard (blocks signing) and soft (look before signing). */
export function chartFlags(f: ChartFaithfulness | undefined, t: T) {
  const hard = new Map<string, string[]>();
  const soft = new Map<string, string[]>();
  const addSoft = (p: string, m: string) => soft.set(p, [...(soft.get(p) ?? []), m]);
  if (!f) return { hard, soft };
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
    else if (flag.kind === "shape") addSoft(flag.path, t("This finding's details don't fit together. Check before signing."));
    else if (flag.kind === "duplicate") addSoft(flag.path, t("This finding is listed twice."));
  }
  return { hard, soft };
}

export interface ToothDiagramProps {
  dentition: Dentition;
  items: ToothFinding[];
  showDeciduous: boolean;
  selected?: string[];
  /** Teeth with a faithfulness flag: "!" beside the number. */
  flagged?: Map<string, "hard" | "soft">;
  onToothActivate?: (id: string, how: { additive: boolean }) => void;
  onEvidence?: (ids: number[] | null) => void;
}

/**
 * The diagram alone: one tab stop with a roving tabindex across the teeth.
 * Arrows move along a row (across the midline) and to the nearest tooth of
 * the next row; Home and End go to the row's ends; Enter activates a tooth,
 * Space adds it to the selection.
 */
export function ToothDiagram({ dentition, items, showDeciduous, selected = [], flagged, onToothActivate, onEvidence }: ToothDiagramProps) {
  const t = useT();
  const lowerInner = useLowerInner();
  const twoBlocks = useMediaQuery(TWO_BLOCKS);
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
    return t("{tooth}, {name}: {findings}", { tooth: id, name, findings: words.join("; ") });
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
        aria-pressed={onToothActivate ? isSelected : undefined}
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
        <text className="dc-num" x={SLOT / 2} y={numberY} textAnchor="middle" aria-hidden>
          {slot.id}
        </text>
        {flag && (
          <text className="dc-flag" data-kind={flag} x={SLOT - 2} y={numberY} textAnchor="middle" aria-hidden>
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

  return <div className="dc-diagrams">{twoBlocks ? [block("right"), block("left")] : block("both")}</div>;
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
}: DentalChartProps) {
  const t = useT();
  const lowerInner = useLowerInner();
  const items = useMemo(() => asItems(value), [value]);
  const { hard, soft } = useMemo(() => chartFlags(faithfulness, t), [faithfulness, t]);

  if (items.length === 0) return <p className="not-discussed">{t("Not discussed")}</p>;

  const groups = byTooth(items);
  const flagged = new Map<string, "hard" | "soft">();
  for (const [id, entries] of groups) {
    const paths = entries.map((e) => `${path}/${e.index}`);
    if (paths.some((p) => hard.has(p))) flagged.set(id, "hard");
    else if (paths.some((p) => soft.has(p))) flagged.set(id, "soft");
  }
  const deciduous = showDeciduous ?? items.some((i) => tooth(i.tooth)?.deciduous);

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
      {dentition && (
        <ToothDiagram
          dentition={dentition}
          items={items}
          showDeciduous={deciduous}
          selected={selected}
          flagged={flagged}
          onToothActivate={onToothActivate}
          onEvidence={onEvidence}
        />
      )}
      {list && (
        <ul className="dc-list">
          {groups.map(([id, entries]) => {
            const hardNums = entries.flatMap((e) => hard.get(`${path}/${e.index}`) ?? []);
            return (
              <li key={id} className={hardNums.length ? "dc-line hardfail" : "dc-line"} data-tooth={id} data-selected={selected.includes(id) ? true : undefined}>
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
              </li>
            );
          })}
        </ul>
      )}
      {!dentition && <p className="dc-notice small">{t("Dentition not recorded. Edit the chart to choose dog or cat.")}</p>}
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
