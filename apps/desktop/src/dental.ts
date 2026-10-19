// Tooth charts for dogs and cats (ADR-0021): typed helpers over the shared
// chart data, packages/dental/chart.json. The engine reads the same file
// (engine/os-engine/src/dental.rs), so the screen, the PDF and the
// faithfulness checks agree on every tooth, outline, finding and code.
//
// The model writes words and fields; AVDC codes are derived here from the
// finding table, never typed by the model or the vet.
import { useContext } from "react";
import chartData from "../../../packages/dental/chart.json";
import { AppContext } from "./context";
import type { Faithfulness } from "./engine";

export type Dentition = "canine" | "feline";
export type Jaw = "upper" | "lower";
export type Side = "right" | "left";
export type ToothType = "incisor" | "canine" | "premolar" | "molar";
export type Shape = "incisor" | "canine" | "premolar" | "carnassial" | "molar";
export type Glyph = "none" | "circle" | "x" | "zigzag" | "notch" | "neck" | "dot";
export type RowName = "upper" | "upper_deciduous" | "lower_deciduous" | "lower";

export interface ToothInfo {
  id: string;
  jaw: Jaw;
  side: Side;
  deciduous: boolean;
  type: ToothType;
  ordinal: number;
  shape: Shape;
  dentitions: Dentition[];
}

export interface DetailDef {
  id: string;
  words: string;
  code: string;
  /** Only with this grade (resorption 4a-4c need stage 4). */
  with_grade: number | null;
  cues: string[][];
  /** Words that rule this type out (the engine's check). */
  exclude?: string[];
}

export interface FindingDef {
  id: string;
  /** Only on this kind of tooth: persistent deciduous on 500-800, unerupted on 100-400. */
  teeth?: "deciduous" | "permanent" | null;
  /** At most one per tooth (two resorption stages conflict). */
  one_per_tooth?: boolean;
  group: string;
  label: string;
  words: string;
  words_grade: string | null;
  words_mm: string | null;
  code: string | null;
  code_grade: string | null;
  code_detail: string | null;
  legend: string | null;
  grade: { min: number; max: number; required: boolean } | null;
  mm: "none" | "optional" | "required";
  details: DetailDef[];
  plannable: boolean;
  text_required: boolean;
  glyph: Glyph;
  cues: string[];
}

interface ChartData {
  /** The finding table's version; each saved run records it. */
  version: number;
  teeth: ToothInfo[];
  rows: Record<Dentition, Record<RowName, string[]>>;
  geometry: { slot: number; tooth: [number, number]; neck: number; crown_mid: number };
  shapes: Record<Shape, string>;
  findings: FindingDef[];
}

export const CHART = chartData as unknown as ChartData;

/** One charted finding on one tooth, as stored in the note (ADR-0021 decision 2). */
export interface ToothFinding {
  tooth: string;
  text: string;
  finding: string;
  detail: string | null;
  grade: number | null;
  mm: number | null;
  planned: boolean;
  evidence: number[];
  entered_by?: string;
}

/** The kinds of flag the faithfulness pass puts on a charted tooth. */
export type ToothFlagKind =
  | "name_conflict"
  | "cue_missing"
  | "other_tooth"
  | "negated"
  | "other_type"
  | "planned"
  | "conflict"
  | "shape"
  | "duplicate"
  | "not_in_mouth"
  | "choose_dentition";

/** A flag on a charted tooth from the faithfulness pass (`tooth_flags`). */
export interface ToothFlag {
  path: string;
  tooth: string;
  kind: ToothFlagKind;
  /** Name conflict: the other tooth. Conflict: the other item's path. */
  other?: string;
  expected?: string[];
}

/** Flags that block signing, like a number said nowhere; the rest are soft. */
export const blocksSigning = (kind: ToothFlagKind): boolean => kind === "not_in_mouth" || kind === "choose_dentition";

export type ChartFaithfulness = Faithfulness & { tooth_flags?: ToothFlag[] };

type T = (text: string, vars?: Record<string, string | number>) => string;

const TEETH = new Map(CHART.teeth.map((t) => [t.id, t]));
const FINDINGS = new Map(CHART.findings.map((f) => [f.id, f]));

export const tooth = (id: string): ToothInfo | undefined => TEETH.get(id);
export const findingDef = (id: string): FindingDef | undefined => FINDINGS.get(id);

/** The patient's species as a tooth chart's dentition, when it is a dog or a cat ("Cat", "feline", "Labrador dog"). */
export function dentitionOf(species: string | null | undefined): Dentition | null {
  const s = ` ${(species ?? "").toLowerCase().replace(/[^a-z]+/g, " ")} `;
  if (/ (canine|dog|dogs|puppy|pup) /.test(s)) return "canine";
  if (/ (feline|cat|cats|kitten) /.test(s)) return "feline";
  return null;
}

/** The dentition's teeth in Triadan order; with none, the dog's (it holds every cat tooth). */
export function teethOf(dentition: Dentition | null): ToothInfo[] {
  const d = dentition ?? "canine";
  return CHART.teeth.filter((t) => t.dentitions.includes(d));
}

export function hasTooth(dentition: Dentition | null, id: string): boolean {
  return tooth(id)?.dentitions.includes(dentition ?? "canine") ?? false;
}

/** The rows to draw, top to bottom, each in drawing order (patient's right first). */
export function rowsOf(dentition: Dentition, deciduous: boolean): { name: RowName; upper: boolean; teeth: string[] }[] {
  const rows = CHART.rows[dentition];
  const names: RowName[] = deciduous ? ["upper", "upper_deciduous", "lower_deciduous", "lower"] : ["upper", "lower"];
  return names.map((name) => ({ name, upper: name.startsWith("upper"), teeth: rows[name] }));
}

/** Triadan order: 101 … 411, then 501 … 808; unknown ids last. */
export function triadanOrder(a: string, b: string): number {
  const key = (id: string) => (tooth(id) ? Number(id) : Number.MAX_SAFE_INTEGER);
  return key(a) - key(b) || a.localeCompare(b);
}

/** Lenient read of the note's chart value. */
export function asItems(value: unknown): ToothFinding[] {
  return Array.isArray(value) ? (value as ToothFinding[]).filter((i) => i && typeof i === "object" && typeof i.tooth === "string") : [];
}

/** The items grouped by tooth in Triadan order, each with its index in the array (for paths). */
export function byTooth(items: ToothFinding[]): [string, { item: ToothFinding; index: number }[]][] {
  const groups = new Map<string, { item: ToothFinding; index: number }[]>();
  items.forEach((item, index) => groups.set(item.tooth, [...(groups.get(item.tooth) ?? []), { item, index }]));
  return [...groups.entries()].sort((a, b) => triadanOrder(a[0], b[0]));
}

const fill = (text: string, vars: Record<string, string>) =>
  text.replace(/\{(\w+)\}/g, (m, k: string) => (k in vars ? (vars[k] as string) : m));

/** "Open extraction" → "open extraction", for words inside a line. */
export const lowerFirst = (s: string) => (s ? s.charAt(0).toLowerCase() + s.slice(1) : s);

/** "7", "2.5": millimetres as said (JavaScript never prints "7.0"). */
const num = (n: number) => String(n);

const english: T = (text, vars) => fill(text, Object.fromEntries(Object.entries(vars ?? {}).map(([k, v]) => [k, String(v)])));

/**
 * Do the item's detail, grade, mm and planned fit its finding, and the finding
 * its tooth (when `tooth` is given)? A plain reason when not, through `t`
 * (English by default); the engine's `fits` gives the same answers.
 */
export function misfit(
  item: Pick<ToothFinding, "finding" | "detail" | "grade" | "mm" | "planned" | "text"> & { tooth?: string },
  t: T = english,
): string | null {
  const f = findingDef(item.finding);
  if (!f) return t("\"{finding}\" isn't a finding the chart knows.", { finding: item.finding });
  if (f.text_required && !item.text?.trim()) return t("Other needs a description.");
  const finding = t(f.words);
  const g = item.grade;
  if (!f.grade && g != null) return t("{finding} takes no number.", { finding });
  if (f.grade && g == null && f.grade.required)
    return t("{finding} needs a number from {min} to {max}.", { finding, min: f.grade.min, max: f.grade.max });
  if (f.grade && g != null && (!Number.isInteger(g) || g < f.grade.min || g > f.grade.max))
    return t("{finding} takes a number from {min} to {max}.", { finding, min: f.grade.min, max: f.grade.max });
  if (f.mm === "none" && item.mm != null) return t("{finding} takes no millimetres.", { finding });
  if (f.mm === "required" && item.mm == null) return t("{finding} needs a measurement in millimetres.", { finding });
  if (item.mm != null && !(item.mm >= 0)) return t("{finding} needs a measurement of 0 mm or more.", { finding });
  if (item.detail != null) {
    const d = f.details.find((x) => x.id === item.detail);
    if (!d) return t("{finding} can't be \"{detail}\".", { finding, detail: item.detail });
    if (d.with_grade != null && g !== d.with_grade) return t("\"{detail}\" is only for stage {stage}.", { detail: t(d.words), stage: d.with_grade });
  }
  if (item.planned && !f.plannable) return t("{finding} can't be planned.", { finding });
  const info = item.tooth ? tooth(item.tooth) : undefined;
  if (f.teeth === "deciduous" && info && !info.deciduous)
    return t("{finding} is only for deciduous teeth (500 to 800), not {tooth}.", { finding, tooth: info.id });
  if (f.teeth === "permanent" && info?.deciduous)
    return t("{finding} is only for permanent teeth (100 to 400), not {tooth}.", { finding, tooth: info.id });
  return null;
}

/** How one finding is printed: its words (a translation key and values), its code, and whether it is a plan. */
export interface Printed {
  key: string;
  vars: Record<string, string>;
  code: string | null;
  planned: boolean;
  /** Printed from the words as said (`other`, or fields that don't fit). */
  asSaid: boolean;
}

/** The printed words and AVDC code of an item (ADR-0021 decision 3); the same table as the engine's `code_of`. */
export function printed(item: ToothFinding): Printed {
  const f = findingDef(item.finding);
  const asSaid: Printed = { key: item.text ?? "", vars: {}, code: null, planned: !!item.planned, asSaid: true };
  if (!f || f.text_required || misfit(item)) return asSaid;
  const detail = f.details.find((d) => d.id === item.detail);
  const p: Printed = { key: f.words, vars: {}, code: f.code, planned: !!item.planned, asSaid: false };
  if (item.mm != null && f.words_mm) {
    p.key = f.words_mm;
    p.vars.mm = num(item.mm);
  }
  if (item.grade != null && f.words_grade) {
    const sub = detail?.with_grade != null ? detail.code : "";
    p.key = f.words_grade;
    p.vars.n = String(item.grade);
    p.vars.stage = `${item.grade}${sub}`;
    if (f.code_grade) p.code = f.code_grade.replace("{grade}", String(item.grade)).replace("{sub}", sub);
  }
  if (detail && detail.with_grade == null) {
    p.key = detail.words;
    p.code = f.code_detail ? f.code_detail.replace("{code}", detail.code) : null;
  }
  return p;
}

/** The finding's words without the code: "Planned: open extraction". */
export function findingWords(t: T, item: ToothFinding, lowerInner = true): string {
  const p = printed(item);
  let words = p.asSaid ? p.key : fill(t(p.key), p.vars);
  if (p.planned) words = t("Planned: {finding}", { finding: lowerInner ? lowerFirst(words) : words });
  return words;
}

/** The words with the code: "Planned: open extraction (XSS)". */
export function findingText(t: T, item: ToothFinding, lowerInner = true): string {
  const code = printed(item).code;
  const words = findingWords(t, item, lowerInner);
  return code ? `${words} (${code})` : words;
}

const TOOTH_WORDS: Record<ToothType, string[]> = {
  incisor: ["first incisor", "second incisor", "third incisor"],
  canine: ["canine"],
  premolar: ["first premolar", "second premolar", "third premolar", "fourth premolar"],
  molar: ["first molar", "second molar", "third molar"],
};

/** "right maxillary fourth premolar", "left mandibular deciduous canine" (lowercase: it follows the number). */
export function toothName(t: T, id: string): string {
  const info = tooth(id);
  if (!info) return id;
  const words = TOOTH_WORDS[info.type];
  let part = t(words[Math.min(info.ordinal, words.length) - 1] ?? words[0] ?? "");
  if (info.deciduous) part = t("deciduous {tooth}", { tooth: part });
  const key = `${info.side} ${info.jaw === "upper" ? "maxillary" : "mandibular"} {tooth}`;
  return t(key, { tooth: part });
}

export function glyphOf(item: ToothFinding): Glyph {
  return findingDef(item.finding)?.glyph ?? "dot";
}

/** The legend for the codes these items use, in finding-table order. */
export function legendLines(t: T, items: ToothFinding[], lowerInner = true): string[] {
  const out: string[] = [];
  for (const f of CHART.findings) {
    for (const item of items.filter((i) => i.finding === f.id)) {
      const p = printed(item);
      const detail = f.details.find((d) => d.id === item.detail && d.with_grade == null);
      let line: string | null = null;
      if (f.legend && p.code) line = t(f.legend);
      else if (p.code) {
        const words = t(detail?.words ?? f.words);
        line = `${p.code}: ${lowerInner ? lowerFirst(words) : words}`;
      } else if (f.legend && "mm" in p.vars) line = t(f.legend);
      if (line && !out.includes(line)) out.push(line);
    }
  }
  return out;
}

/** German keeps capitals on nouns inside a line; the other interface languages lowercase. */
export function useLowerInner(): boolean {
  return (useContext(AppContext)?.settings.uiLanguage ?? "en") !== "de";
}

/** An ellipse as SVG path data. */
const ellipse = (cx: number, cy: number, rx: number, ry: number) =>
  `M ${cx - rx} ${cy} A ${rx} ${ry} 0 1 0 ${cx + rx} ${cy} A ${rx} ${ry} 0 1 0 ${cx - rx} ${cy} Z`;

/**
 * Each glyph's marks in the tooth box, with whether they are filled. The PDF
 * draws the same marks (engine/os-engine/src/pdf.rs `glyph_marks`).
 */
export function glyphMarks(glyph: Glyph): { d: string; filled: boolean }[] {
  const { neck, crown_mid: mid } = CHART.geometry;
  switch (glyph) {
    case "circle":
      return [{ d: ellipse(10, 20, 14, 20.5), filled: false }];
    case "x":
      return [{ d: "M 1 1 L 19 39 M 19 1 L 1 39", filled: false }];
    case "zigzag":
      return [{ d: `M 1 ${mid} L 4 ${mid - 3} L 7 ${mid + 3} L 10 ${mid - 3} L 13 ${mid + 3} L 16 ${mid - 3} L 19 ${mid}`, filled: false }];
    case "notch":
      return [
        { d: `M 0 ${neck - 3} L 5 ${neck} L 0 ${neck + 3} Z M 20 ${neck - 3} L 15 ${neck} L 20 ${neck + 3} Z`, filled: true },
      ];
    case "neck":
      return [{ d: `M -2 ${neck} L 22 ${neck}`, filled: false }];
    case "dot":
      return [{ d: ellipse(10, -5, 2, 2), filled: true }];
    default:
      return [];
  }
}

/** A new item for a tooth from the editor: the vet's own, with no evidence. */
export function newItem(
  t: T,
  toothId: string,
  fields: Pick<ToothFinding, "finding" | "detail" | "grade" | "mm" | "planned"> & { text?: string },
  lowerInner = true,
): ToothFinding {
  const base: ToothFinding = { tooth: toothId, text: fields.text ?? "", finding: fields.finding, detail: fields.detail, grade: fields.grade, mm: fields.mm, planned: fields.planned, evidence: [] };
  return findingDef(fields.finding)?.text_required ? base : { ...base, text: findingWords(t, base, lowerInner) };
}
