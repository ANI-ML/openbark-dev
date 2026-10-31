// What "Copy note" puts on the clipboard (M1 item 12): plain text for PIMS
// text boxes, formatted text (HTML) for rich editors, or Markdown.
import { asItems, byTooth, findingText, lowerFirst } from "./dental";
import type { ContentKind, NoteRun, Template, TemplateField } from "./engine";

export type CopyFormat = "plain" | "formatted" | "markdown";

export interface CopyOptions {
  format: CopyFormat;
  /** Keep the transcript line numbers, e.g. "[4, 7]". */
  citations: boolean;
  /** Lines above the note: patient, reason, recorded, signed. */
  header: string[];
  /** (heading, text) pairs below the note. */
  addenda: [string, string][];
  /** "Not discussed" in the interface language. */
  notDiscussed?: string;
  /** The translator, for a tooth chart's finding words (English without it). */
  t?: Translate;
  /** Lowercase words inside a line (not in German). */
  lowerInner?: boolean;
}

type Translate = (text: string, vars?: Record<string, string | number>) => string;

/** Whole values that mean "nothing" (ADR-0032): "null" never shows in a note. */
const BLANK = new Set(["null", "n/a", "na", "nil", "undefined", "-", "--", "—", "…", "...", "?"]);

/** Is this whole value blank: empty, or a word such as "null" or "n/a"? */
export function isBlank(value: unknown): boolean {
  if (value == null) return true;
  const t = String(value).trim().replace(/[.,;]+$/, "").trim().toLowerCase();
  return t === "" || BLANK.has(t);
}

/** Blank, or "none" on its own (a measurement or a statement with no label). */
export function isBlankOrNone(value: unknown): boolean {
  return isBlank(value) || String(value).trim().replace(/\.$/, "").trim().toLowerCase() === "none";
}

/** A statement read as "Label: value" pieces ("T: 38.5 P: null"), or null when it isn't one. */
function labelPieces(text: string): { label: string; value: string }[] | null {
  const out: { label: string; value: string }[] = [];
  for (const entry of text.split(";")) {
    if (!entry.trim()) continue;
    const m = entry.match(/^\s*[-•*]?\s*(\p{L}[^:：,;?!"]{0,39}?)\s*[:：]/u);
    const first = (m?.[1] ?? "").trim();
    if (!m || !first || first.split(/\s+/).length > 5) return null;
    let label = first;
    const rest = entry.slice(m[0].length);
    // A short label inside the value starts another piece ("T: P: R:", "roses TRC: < 2 s").
    const next = /(^|\s)(\p{L}[\p{L}\p{N}.]{0,5})\s*[:：]/gu;
    let from = 0;
    for (let found = next.exec(rest); found; found = next.exec(rest)) {
      const at = found.index + (found[1] ?? "").length;
      out.push({ label, value: rest.slice(from, at).trim() });
      label = found[2] ?? "";
      from = next.lastIndex;
    }
    out.push({ label, value: rest.slice(from).trim() });
  }
  return out.length ? out : null;
}

/**
 * A statement as it should read, or null when it says nothing: "null", "Tartre : null",
 * "T: null P: null". Pieces with a blank value are left out of one that has others.
 * Drafts from 0.2.5 on are tidied by the engine; this covers notes saved before.
 */
export function shownStatement(text: string | undefined | null): string | null {
  const s = text ?? "";
  if (isBlankOrNone(s)) return null;
  if (s.includes("\n") || !/[:：]/.test(s)) return s;
  const pieces = labelPieces(s);
  if (!pieces || !pieces.some((p) => isBlank(p.value))) return s;
  const kept = pieces.filter((p) => !isBlank(p.value)).map((p) => `${p.label}: ${p.value}`);
  return kept.length ? kept.join(", ") : null;
}

/** A measurement's text, "32.4 kg" or "pink but tacky", or "" when nothing was said ("null" included). */
export function measurementText(v: { value?: unknown; unit?: string | null } | null | undefined): string {
  if (v?.value == null || (typeof v.value === "string" && isBlankOrNone(v.value))) return "";
  const value = String(v.value);
  const unit = v.unit && !isBlank(v.unit) && !value.endsWith(v.unit) ? v.unit : null;
  return unit ? `${value} ${unit}` : value;
}

/** A medication's dose, route, frequency and duration as said ("null" left out). */
export function medicationDetails(m: Record<string, unknown>): string[] {
  return ["dose", "route", "frequency", "duration"]
    .map((k) => m[k])
    .filter((x): x is string => typeof x === "string" && !isBlankOrNone(x));
}

/** Does a medication name a drug ("null" doesn't)? */
export const namesDrug = (m: Record<string, unknown>) => typeof m.drug === "string" && !isBlankOrNone(m.drug);

const english: Translate = (text, vars) => text.replace(/\{(\w+)\}/g, (m, k: string) => (vars && k in vars ? String(vars[k]) : m));

type Item = Record<string, unknown> & { evidence?: number[]; text?: string; value?: unknown; unit?: string | null };

interface Block {
  /** A text box: one paragraph, no bullet. */
  paragraph?: boolean;
  heading: string;
  sub?: string;
  lines: string[];
  empty: boolean;
}

const get = (o: unknown, k: string): unknown => (o && typeof o === "object" ? (o as Record<string, unknown>)[k] : undefined);

function cite(item: Item, on: boolean): string {
  const ev = Array.isArray(item.evidence) ? item.evidence : [];
  return on && ev.length ? ` [${ev.join(", ")}]` : "";
}

/** A tooth chart in words: one line per tooth, "108: complicated crown fracture (T/FX/CCF) [4]; open extraction (XSS) [8]". */
function teethLines(value: unknown, citations: boolean, t: Translate, lowerInner: boolean): string[] {
  return byTooth(asItems(value)).map(([tooth, entries]) => {
    const findings = entries.map(({ item }) => {
      const text = findingText(t, item, lowerInner);
      return `${lowerInner ? lowerFirst(text) : text}${cite(item as unknown as Item, citations)}`;
    });
    return `${tooth}: ${findings.join("; ")}`;
  });
}

/** One heading's lines as Copy note writes them (also a chart heading's own Copy). */
export function headingLines(
  kind: ContentKind,
  fields: TemplateField[] | undefined,
  value: unknown,
  citations: boolean,
  t: Translate = english,
  lowerInner = true,
): string[] {
  if (kind === "teeth") return teethLines(value, citations, t, lowerInner);
  if (kind === "fields") {
    return (fields ?? []).flatMap((f) => {
      const v = get(value, f.key) as Item | null | undefined;
      const text = measurementText(v);
      if (!v || !text) return [];
      return [`${f.label}: ${text}${cite(v, citations)}`];
    });
  }
  const items = Array.isArray(value) ? (value as Item[]) : [];
  // "null" never prints (ADR-0032): a statement that says nothing is left out.
  const said = items.flatMap((s) => {
    const text = shownStatement(s.text);
    return text == null ? [] : [`${text}${cite(s, citations)}`];
  });
  if (kind === "statements") return said;
  if (kind === "text") return said.length ? [said.join(" ")] : [];
  return items.filter(namesDrug).map((m) => {
    const details = medicationDetails(m);
    return `${String(m.drug ?? "")}${details.length ? `: ${details.join(", ")}` : ""}${cite(m, citations)}`;
  });
}

function blocks(template: Template, run: NoteRun, citations: boolean, t?: Translate, lowerInner?: boolean): Block[] {
  const out: Block[] = [];
  for (const sec of template.sections) {
    const value = run.note[sec.id];
    if (sec.kind) {
      const l = headingLines(sec.kind, sec.fields, value, citations, t, lowerInner);
      out.push({ heading: sec.title, lines: l, empty: l.length === 0, paragraph: sec.kind === "text" });
    } else {
      (sec.parts ?? []).forEach((p, i) => {
        const l = headingLines(p.kind, p.fields, get(value, p.key), citations, t, lowerInner);
        out.push({ heading: i === 0 ? sec.title : "", sub: p.label, lines: l, empty: l.length === 0, paragraph: p.kind === "text" });
      });
    }
  }
  return out;
}

const escapeHtml = (s: string) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");

/** The note as text for the chosen format, plus HTML for rich pasting. */
export function formatNote(template: Template, run: NoteRun, o: CopyOptions): { text: string; html?: string } {
  const nd = o.notDiscussed ?? "Not discussed";
  const bs = blocks(template, run, o.citations, o.t, o.lowerInner);
  if (o.format === "markdown") {
    const md: string[] = [`# ${template.name}`, "", ...o.header.map((h) => `${h}  `)];
    for (const b of bs) {
      if (b.heading) md.push("", `## ${b.heading}`);
      if (b.sub) md.push("", `**${b.sub}**`);
      md.push("", ...(b.empty ? [`*${nd}*`] : b.lines.map((l) => (b.paragraph ? l : `- ${l}`))));
    }
    for (const [h, t] of o.addenda) md.push("", `## ${h}`, "", t);
    return { text: md.join("\n").trim() + "\n" };
  }
  const plain: string[] = [template.name.toUpperCase(), ...o.header];
  for (const b of bs) {
    if (b.heading) plain.push("", b.heading.toUpperCase());
    if (b.sub) plain.push(`${b.sub}:`);
    plain.push(...(b.empty ? [nd] : b.lines.map((l) => (b.paragraph ? l : `- ${l}`))));
  }
  for (const [h, t] of o.addenda) plain.push("", h.toUpperCase(), t);
  const text = plain.join("\n").trim() + "\n";
  if (o.format === "plain") return { text };
  const html: string[] = [`<h2>${escapeHtml(template.name)}</h2>`];
  if (o.header.length) html.push(`<p>${o.header.map(escapeHtml).join("<br>")}</p>`);
  for (const b of bs) {
    if (b.heading) html.push(`<h3>${escapeHtml(b.heading)}</h3>`);
    if (b.sub) html.push(`<p><strong>${escapeHtml(b.sub)}</strong></p>`);
    html.push(
      b.empty
        ? `<p><em>${escapeHtml(nd)}</em></p>`
        : b.paragraph
          ? b.lines.map((l) => `<p>${escapeHtml(l)}</p>`).join("")
          : `<ul>${b.lines.map((l) => `<li>${escapeHtml(l)}</li>`).join("")}</ul>`,
    );
  }
  for (const [h, t] of o.addenda) html.push(`<h3>${escapeHtml(h)}</h3><p>${escapeHtml(t)}</p>`);
  return { text, html: html.join("") };
}

/** Put text (and HTML, when given and supported) on the clipboard. */
export async function copyToClipboard(content: { text: string; html?: string }): Promise<void> {
  if (content.html && typeof ClipboardItem !== "undefined" && navigator.clipboard?.write) {
    await navigator.clipboard.write([
      new ClipboardItem({
        "text/plain": new Blob([content.text], { type: "text/plain" }),
        "text/html": new Blob([content.html], { type: "text/html" }),
      }),
    ]);
    return;
  }
  await navigator.clipboard.writeText(content.text);
}
