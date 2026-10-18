import { ChatCircleText, Copy } from "@phosphor-icons/react";
import type { ContentKind, Faithfulness, NoteRun, Template, TemplateField, TemplatePart } from "../engine";

/**
 * The structured note with the evidence link (DESIGN.md §1, §5 Note editor):
 * resting on a statement lights its supporting transcript lines; clicking it
 * plays the audio from there. Flags:
 * - number absent from the transcript: Clay left-ruled callout (blocks signing)
 * - value cited to the wrong line, or no valid citation: Amber dotted underline
 * - nothing said: "Not discussed" in Graphite italic, never omitted
 *
 * With `onAsk`, every heading and subheading has an Ask action that opens
 * `renderAsk(heading)` beneath it (the re-prompt panel).
 */
export interface NoteViewProps {
  template: Template;
  run: NoteRun;
  onEvidence: (ids: number[] | null) => void;
  onPlay: (ids: number[]) => void;
  onCopySection: (title: string, text: string) => void;
  /** Re-prompt a heading ("plan" or "plan/medications"). Absent once signed. */
  onAsk?: (heading: string) => void;
  /** The re-prompt panel for the heading being asked about, else null. */
  renderAsk?: (heading: string) => React.ReactNode;
}

type Statement = { text?: string; value?: string | number; unit?: string | null; evidence?: number[] } & Record<
  string,
  unknown
>;

/** "32.4 kg" or "pink but tacky" (measurements carry a separate unit). */
const fieldText = (f: Statement) =>
  f.value == null ? "" : f.unit && !String(f.value).endsWith(f.unit) ? `${f.value} ${f.unit}` : String(f.value);

function flagsFor(f: Faithfulness) {
  const hard = new Map<string, string[]>();
  const soft = new Map<string, string>();
  for (const u of f.unsupported_numbers) hard.set(u.path, [...(hard.get(u.path) ?? []), u.number]);
  for (const m of f.citation_mismatches)
    soft.set(m.path, `"${m.number}" is in the transcript, but not in the cited line. Check the citation.`);
  for (const p of f.invalid_evidence) soft.set(p, "No matching transcript. Keep, edit, or remove?");
  for (const v of f.unverified_values ?? []) soft.set(v, "Not said in the cited line. Check before signing.");
  for (const d of f.disputed_numbers ?? [])
    soft.set(d.path, `The second recogniser heard ${d.heard_instead.join(" or ")} instead of ${d.number}. Listen to the recording before signing.`);
  return { hard, soft };
}

function get(obj: unknown, key: string): unknown {
  return obj && typeof obj === "object" ? (obj as Record<string, unknown>)[key] : undefined;
}

export function NoteView({ template, run, onEvidence, onPlay, onCopySection, onAsk, renderAsk }: NoteViewProps) {
  const { hard, soft } = flagsFor(run.faithfulness);

  const statement = (s: Statement, path: string, content: React.ReactNode) => {
    const ev = Array.isArray(s.evidence) ? s.evidence : [];
    const hardNums = hard.get(path);
    const softMsg = soft.get(path);
    const body = (
      <span
        className="stmt"
        data-soft={softMsg ? true : undefined}
        tabIndex={0}
        role="button"
        title={softMsg ?? (ev.length ? "Play the supporting audio" : undefined)}
        onMouseEnter={() => onEvidence(ev)}
        onMouseLeave={() => onEvidence(null)}
        onFocus={() => onEvidence(ev)}
        onBlur={() => onEvidence(null)}
        onClick={() => ev.length && onPlay(ev)}
        onKeyDown={(e) => (e.key === "Enter" || e.key === " ") && ev.length && (e.preventDefault(), onPlay(ev))}
      >
        {content}
      </span>
    );
    return hardNums ? (
      <div className="hardfail">
        {body}
        <p className="small">
          {hardNums.join(", ")} {hardNums.length === 1 ? "isn't" : "aren't"} anywhere in the transcript. Check the
          recording and correct this before signing.
        </p>
      </div>
    ) : (
      body
    );
  };

  const statements = (value: unknown, path: string) => {
    const items = Array.isArray(value) ? (value as Statement[]) : [];
    if (items.length === 0) return <p className="not-discussed">Not discussed</p>;
    return (
      <ul className="stmts">
        {items.map((s, i) => (
          <li key={i}>{statement(s, `${path}/${i}`, s.text)}</li>
        ))}
      </ul>
    );
  };

  const fields = (defs: TemplateField[], value: unknown, path: string) => {
    const present = defs.filter((d) => get(get(value, d.key), "value") != null);
    const missing = defs.filter((d) => get(get(value, d.key), "value") == null);
    if (present.length === 0) return <p className="not-discussed">Not discussed</p>;
    return (
      <dl className="fields">
        {present.map((d) => {
          const f = get(value, d.key) as Statement;
          return (
            <div key={d.key}>
              <dt className="label muted">{d.label}</dt>
              <dd>{statement(f, `${path}/${d.key}`, <span className="mono-num">{fieldText(f)}</span>)}</dd>
            </div>
          );
        })}
        {missing.length > 0 && (
          <div className="fields-missing">
            <dt className="label muted">Not discussed</dt>
            <dd className="muted">{missing.map((d) => d.label).join(", ")}</dd>
          </div>
        )}
      </dl>
    );
  };

  const medications = (value: unknown, path: string) => {
    const items = Array.isArray(value) ? (value as Statement[]) : [];
    if (items.length === 0) return <p className="not-discussed">Not discussed</p>;
    return (
      <ul className="stmts">
        {items.map((m, i) => {
          const details = ["dose", "route", "frequency", "duration"]
            .map((k) => m[k])
            .filter((v): v is string => typeof v === "string" && v.length > 0);
          return (
            <li key={i}>
              {statement(
                m,
                `${path}/${i}`,
                <>
                  <strong>{String(m.drug ?? "")}</strong>
                  {details.length > 0 ? <span className="mono-num">{`  ${details.join(", ")}`}</span> : <span className="muted">  dose, route and frequency not stated</span>}
                </>,
              )}
            </li>
          );
        })}
      </ul>
    );
  };

  const content = (kind: ContentKind, defs: TemplateField[] | undefined, value: unknown, path: string) =>
    kind === "statements"
      ? statements(value, path)
      : kind === "fields"
        ? fields(defs ?? [], value, path)
        : medications(value, path);

  const refined = run.refinement?.heading;
  const askButton = (heading: string, title: string) =>
    onAsk && (
      <button
        type="button"
        className="ob-button section-tool"
        data-variant="ghost"
        onClick={() => onAsk(heading)}
        aria-label={`Ask for changes to ${title}`}
        title="Ask the model to redraft this heading"
      >
        <ChatCircleText size={16} aria-hidden />
        Ask
      </button>
    );
  const refinedNote = (heading: string) =>
    refined === heading && (
      <p className="refined small muted">
        Redrafted on request: <q>{run.refinement?.request}</q>
        {run.refinement?.saved_as_standard ? " Saved as a standard." : ""}
      </p>
    );

  const part = (p: TemplatePart, sectionValue: unknown, sectionId: string) => {
    const value = get(sectionValue, p.key);
    const heading = `${sectionId}/${p.key}`;
    return (
      <div className="note-part" key={p.key}>
        <div className="note-part-head">
          <h3 className="label">{p.label}</h3>
          {askButton(heading, p.label)}
        </div>
        {refinedNote(heading)}
        {renderAsk?.(heading)}
        {content(p.kind, p.fields, value, `/${heading}`)}
      </div>
    );
  };

  const sectionText = (id: string) => {
    const el = document.getElementById(`note-${id}`);
    return el?.innerText.trim() ?? "";
  };

  return (
    <article className="note" aria-label="Draft note">
      {template.sections.map((sec) => {
        const value = run.note[sec.id];
        return (
          <section key={sec.id} id={`note-${sec.id}`} className="note-section">
            <header className="note-section-head">
              <h2>{sec.title}</h2>
              <button
                type="button"
                className="ob-button section-tool"
                data-variant="ghost"
                onClick={() => onCopySection(sec.title, sectionText(sec.id))}
                aria-label={`Copy ${sec.title}`}
              >
                <Copy size={16} aria-hidden />
                Copy
              </button>
              {askButton(sec.id, sec.title)}
            </header>
            {refinedNote(sec.id)}
            {renderAsk?.(sec.id)}
            {sec.kind ? content(sec.kind, sec.fields, value, `/${sec.id}`) : (sec.parts ?? []).map((p) => part(p, value, sec.id))}
          </section>
        );
      })}
    </article>
  );
}
