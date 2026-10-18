import { ChatCircleText, Copy, PencilSimple } from "@phosphor-icons/react";
import type { ContentKind, Faithfulness, NoteRun, Template, TemplateField, TemplatePart } from "../engine";
import { useT } from "../i18n";

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
  /** Edit a heading by hand. Absent once signed. */
  onEdit?: (heading: string) => void;
  /** The editor for the heading being edited (shown in place of its content), else null. */
  renderEdit?: (heading: string) => React.ReactNode;
}

type Statement = { text?: string; value?: string | number; unit?: string | null; evidence?: number[]; entered_by?: string } & Record<
  string,
  unknown
>;

/** "32.4 kg" or "pink but tacky" (measurements carry a separate unit). */
const fieldText = (f: Statement) =>
  f.value == null ? "" : f.unit && !String(f.value).endsWith(f.unit) ? `${f.value} ${f.unit}` : String(f.value);

/** `t` is the translator from the calling component's `useT()`. */
function flagsFor(f: Faithfulness, t: ReturnType<typeof useT>) {
  const hard = new Map<string, string[]>();
  const soft = new Map<string, string>();
  for (const u of f.unsupported_numbers) hard.set(u.path, [...(hard.get(u.path) ?? []), u.number]);
  for (const m of f.citation_mismatches)
    soft.set(m.path, t('"{number}" is in the transcript, but not in the cited line. Check the citation.', { number: m.number }));
  for (const p of f.invalid_evidence) soft.set(p, t("No matching transcript. Keep, edit, or remove?"));
  for (const v of f.unverified_values ?? []) soft.set(v, t("Not said in the cited line. Check before signing."));
  for (const d of f.disputed_numbers ?? [])
    soft.set(
      d.path,
      t("The second recogniser heard {heard} instead of {number}. Listen to the recording before signing.", {
        heard: d.heard_instead.join(` ${t("or")} `),
        number: d.number,
      }),
    );
  return { hard, soft };
}

function get(obj: unknown, key: string): unknown {
  return obj && typeof obj === "object" ? (obj as Record<string, unknown>)[key] : undefined;
}

export function NoteView({ template, run, onEvidence, onPlay, onCopySection, onAsk, renderAsk, onEdit, renderEdit }: NoteViewProps) {
  const t = useT();
  const { hard, soft } = flagsFor(run.faithfulness, t);

  const statement = (s: Statement, path: string, content: React.ReactNode) => {
    const ev = Array.isArray(s.evidence) ? s.evidence : [];
    const hardNums = hard.get(path);
    const softMsg = soft.get(path);
    const vet = typeof s.entered_by === "string" && s.entered_by ? s.entered_by : null;
    const body = (
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
        onMouseEnter={() => onEvidence(ev)}
        onMouseLeave={() => onEvidence(null)}
        onFocus={() => onEvidence(ev)}
        onBlur={() => onEvidence(null)}
        onClick={() => ev.length && onPlay(ev)}
        onKeyDown={(e) => (e.key === "Enter" || e.key === " ") && ev.length && (e.preventDefault(), onPlay(ev))}
      >
        {content}
        {vet && <span className="vet-mark small faint">{` ${t("Edited by {name}", { name: vet })}`}</span>}
      </span>
    );
    return hardNums ? (
      <div className="hardfail">
        {body}
        <p className="small">
          {hardNums.length === 1
            ? t("{numbers} isn't anywhere in the transcript. Check the recording and correct this before signing.", {
                numbers: hardNums.join(", "),
              })
            : t("{numbers} aren't anywhere in the transcript. Check the recording and correct this before signing.", {
                numbers: hardNums.join(", "),
              })}
        </p>
      </div>
    ) : (
      body
    );
  };

  const statements = (value: unknown, path: string) => {
    const items = Array.isArray(value) ? (value as Statement[]) : [];
    if (items.length === 0) return <p className="not-discussed">{t("Not discussed")}</p>;
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
    if (present.length === 0) return <p className="not-discussed">{t("Not discussed")}</p>;
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
            <dt className="label muted">{t("Not discussed")}</dt>
            <dd className="muted">{missing.map((d) => d.label).join(", ")}</dd>
          </div>
        )}
      </dl>
    );
  };

  const medications = (value: unknown, path: string) => {
    const items = Array.isArray(value) ? (value as Statement[]) : [];
    if (items.length === 0) return <p className="not-discussed">{t("Not discussed")}</p>;
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
                  {details.length > 0 ? <span className="mono-num">{`  ${details.join(", ")}`}</span> : <span className="muted">{`  ${t("dose, route and frequency not stated")}`}</span>}
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
        aria-label={t("Ask for changes to {heading}", { heading: title })}
        title={t("Ask the model to redraft this heading")}
      >
        <ChatCircleText size={16} aria-hidden />
        {t("Ask")}
      </button>
    );
  const editButton = (heading: string, title: string) =>
    onEdit && (
      <button
        type="button"
        className="ob-button section-tool"
        data-variant="ghost"
        onClick={() => onEdit(heading)}
        aria-label={t("Edit {heading}", { heading: title })}
        title={t("Edit this heading yourself")}
      >
        <PencilSimple size={16} aria-hidden />
        {t("Edit")}
      </button>
    );
  const edited = run.edit?.heading;
  const refinedNote = (heading: string) =>
    (refined === heading && (
      <p className="refined small muted">
        {t("Redrafted on request:")} <q>{run.refinement?.request}</q>
        {run.refinement?.saved_as_standard ? ` ${t("Saved as a standard.")}` : ""}
      </p>
    )) || (edited === heading && run.edit && (
      <p className="refined small muted">
        {run.edit.removed
          ? t("Edited by {name}: {changed} changed or added, {removed} removed. The earlier draft is kept.", {
              name: run.edit.by,
              changed: run.edit.changed.length,
              removed: run.edit.removed,
            })
          : t("Edited by {name}: {changed} changed or added. The earlier draft is kept.", {
              name: run.edit.by,
              changed: run.edit.changed.length,
            })}
      </p>
    ));

  const part = (p: TemplatePart, sectionValue: unknown, sectionId: string) => {
    const value = get(sectionValue, p.key);
    const heading = `${sectionId}/${p.key}`;
    return (
      <div className="note-part" key={p.key}>
        <div className="note-part-head">
          <h3 className="label">{p.label}</h3>
          {editButton(heading, p.label)}
          {askButton(heading, p.label)}
        </div>
        {refinedNote(heading)}
        {renderAsk?.(heading)}
        {renderEdit?.(heading) || content(p.kind, p.fields, value, `/${heading}`)}
      </div>
    );
  };

  const sectionText = (id: string) => {
    const el = document.getElementById(`note-${id}`);
    return el?.innerText.trim() ?? "";
  };

  return (
    <article className="note" aria-label={t("Draft note")}>
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
                aria-label={t("Copy {heading}", { heading: sec.title })}
              >
                <Copy size={16} aria-hidden />
                {t("Copy")}
              </button>
              {sec.kind && editButton(sec.id, sec.title)}
              {askButton(sec.id, sec.title)}
            </header>
            {refinedNote(sec.id)}
            {renderAsk?.(sec.id)}
            {sec.kind
              ? renderEdit?.(sec.id) || content(sec.kind, sec.fields, value, `/${sec.id}`)
              : (sec.parts ?? []).map((p) => part(p, value, sec.id))}
          </section>
        );
      })}
    </article>
  );
}
