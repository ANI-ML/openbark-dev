// What "Copy note" puts on the clipboard (M1 item 12): plain text for PIMS
// text boxes, formatted text (HTML) for rich editors, or Markdown.
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
}

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

function lines(kind: ContentKind, fields: TemplateField[] | undefined, value: unknown, citations: boolean): string[] {
  if (kind === "fields") {
    return (fields ?? []).flatMap((f) => {
      const v = get(value, f.key) as Item | null | undefined;
      if (v?.value == null) return [];
      const text = v.unit && !String(v.value).endsWith(v.unit) ? `${v.value} ${v.unit}` : String(v.value);
      return [`${f.label}: ${text}${cite(v, citations)}`];
    });
  }
  const items = Array.isArray(value) ? (value as Item[]) : [];
  if (kind === "statements") return items.map((s) => `${s.text ?? ""}${cite(s, citations)}`);
  if (kind === "text") {
    const sentences = items.map((s) => `${s.text ?? ""}${cite(s, citations)}`);
    return sentences.length ? [sentences.join(" ")] : [];
  }
  return items.map((m) => {
    const details = ["dose", "route", "frequency", "duration"].map((k) => m[k]).filter((x): x is string => typeof x === "string" && !!x);
    return `${String(m.drug ?? "")}${details.length ? `: ${details.join(", ")}` : ""}${cite(m, citations)}`;
  });
}

function blocks(template: Template, run: NoteRun, citations: boolean): Block[] {
  const out: Block[] = [];
  for (const sec of template.sections) {
    const value = run.note[sec.id];
    if (sec.kind) {
      const l = lines(sec.kind, sec.fields, value, citations);
      out.push({ heading: sec.title, lines: l, empty: l.length === 0, paragraph: sec.kind === "text" });
    } else {
      (sec.parts ?? []).forEach((p, i) => {
        const l = lines(p.kind, p.fields, get(value, p.key), citations);
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
  const bs = blocks(template, run, o.citations);
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
