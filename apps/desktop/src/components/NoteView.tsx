import { Check, Copy, MessageSquareText, Pencil, X } from "lucide-react";
import { useState } from "react";
import { headingLines, measurementText, medicationDetails, namesDrug, shownStatement } from "../copyFormats";
import { asItems, useLowerInner, type Dentition } from "../dental";
import type { ChartCheck, ContentKind, Faithfulness, NoteRun, PendingDefault, Template, TemplateField, TemplatePart } from "../engine";
import { useT } from "../i18n";
import { DentalChart } from "./DentalChart";

/**
 * The structured note with the evidence link:
 * resting on a statement lights its supporting transcript lines; clicking it
 * plays the audio from there. Flags:
 * - number absent from the transcript: danger left-ruled callout (blocks signing)
 * - value cited to the wrong line, or no valid citation: warning dotted underline
 * - nothing said: "Not discussed" in secondary italic, never omitted
 *
 * Look (docs/brand/DESIGN.md §4.1, §7.5): the note body is set in the mono
 * transcript style so it reads as the record; headings (Subjective,
 * Objective...) are h3 with a 1 px rule beneath; a heading being edited by
 * hand carries a 2 px accent bar in the left gutter until it's saved.
 *
 * With `onAsk`, every heading and subheading has an Ask action that opens
 * `renderAsk(heading)` beneath it (the re-prompt panel).
 *
 * A tooth chart heading (ADR-0021) draws the chart: clicking a tooth in an
 * unsigned draft opens the chart editor with that tooth selected; once signed,
 * it only selects the tooth and lights its transcript lines.
 *
 * Normal findings from the template (ADR-0032) show under their heading, set
 * apart and labelled "Template default, not from the recording", with Confirm,
 * Edit and Remove. They aren't part of the note: a heading's Copy leaves them out.
 * "null" never shows: a statement that says nothing is left out.
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
  /** Edit a heading by hand (`tooth`: a tooth clicked on a chart). Absent once signed. */
  onEdit?: (heading: string, opts?: { tooth?: string }) => void;
  /** The editor for the heading being edited (shown in place of its content), else null. */
  renderEdit?: (heading: string) => React.ReactNode;
  /** The patient's species read as dog or cat, offered for a chart with no dentition. */
  suggestedDentition?: Dentition | null;
  /** Write that dentition into a chart heading (an unsigned draft). */
  onChooseDentition?: (heading: string, dentition: Dentition) => void;
  /** The vet's confirmations of the chart's drafted teeth (ADR-0021 decision 10). */
  chartChecks?: ChartCheck[];
  /** Teeth with a drafted entry not confirmed yet; given, the chart shows its draft state. */
  unconfirmedTeeth?: string[];
  /** Confirm or take back drafted teeth (an unsigned draft, for a veterinarian). */
  onConfirmTeeth?: (teeth: string[], confirmed: boolean) => void;
  /** A confirmation is being saved. */
  confirming?: boolean;
  /**
   * Phones (ADR-0027): a tap on a sentence picks it, so Review can show the
   * line it came from with "Play this moment", instead of playing at once.
   */
  onPick?: (ids: number[]) => void;
  /** Normal findings from the template waiting for a veterinarian (ADR-0032). */
  pendingDefaults?: PendingDefault[];
  /** Confirm waiting normal findings under a heading (`text`: the vet's wording of one). A veterinarian, unsigned. */
  onConfirmDefaults?: (heading: string, labels: string[], text?: string) => void;
  /** Remove waiting normal findings under a heading. Unsigned. */
  onRemoveDefaults?: (heading: string, labels: string[]) => void;
  /** A confirmation or removal is being saved. */
  settlingDefaults?: boolean;
}

type Statement = { text?: string; value?: string | number; unit?: string | null; evidence?: number[]; entered_by?: string } & Record<
  string,
  unknown
>;

/** "32.4 kg" or "pink but tacky" (measurements carry a separate unit); "" when nothing was said. */
const fieldText = (f: Statement) => measurementText(f);

/** `t` is the translator from the calling component's `useT()`. */
function flagsFor(f: Faithfulness, t: ReturnType<typeof useT>) {
  const hard = new Map<string, string[]>();
  const soft = new Map<string, string>();
  for (const u of f.unsupported_numbers) hard.set(u.path, [...(hard.get(u.path) ?? []), u.number]);
  for (const m of f.citation_mismatches)
    soft.set(m.path, t('"{number}" is in the transcript, but not in the cited line. Check the citation.', { number: m.number }));
  for (const p of f.invalid_evidence) soft.set(p, t("No matching transcript. Keep, edit, or remove?"));
  for (const v of f.unverified_values ?? []) soft.set(v, t("Not said in the cited line. Check before signing."));
  for (const u of f.unsupported_statements ?? []) {
    const words = t("Some of this isn't in the cited line ({words}). Check before signing.", { words: u.missing.join(", ") });
    soft.set(u.path, u.suggested.length ? `${words} ${t("It may come from line {lines}.", { lines: u.suggested.join(", ") })}` : words);
  }
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

export function NoteView({
  template,
  run,
  onEvidence,
  onPlay,
  onCopySection,
  onAsk,
  renderAsk,
  onEdit,
  renderEdit,
  suggestedDentition = null,
  onChooseDentition,
  chartChecks,
  unconfirmedTeeth,
  onConfirmTeeth,
  confirming = false,
  onPick,
  pendingDefaults = [],
  onConfirmDefaults,
  onRemoveDefaults,
  settlingDefaults = false,
}: NoteViewProps) {
  const choose = onPick ?? onPlay;
  const t = useT();
  const lowerInner = useLowerInner();
  const { hard, soft } = flagsFor(run.faithfulness, t);
  // Signed charts: the tooth last clicked, highlighted with its lines.
  const [pickedTooth, setPickedTooth] = useState<string | null>(null);
  /** A normal finding from the template being reworded before it's confirmed. */
  const [rewording, setRewording] = useState<{ heading: string; label: string; text: string } | null>(null);

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
        onClick={() => ev.length && choose(ev)}
        onKeyDown={(e) => (e.key === "Enter" || e.key === " ") && ev.length && (e.preventDefault(), choose(ev))}
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

  /** The items that say something, with their place in the list ("Tartre : null" is left out). */
  const said = (value: unknown) =>
    (Array.isArray(value) ? (value as Statement[]) : []).flatMap((s, i) => {
      const text = shownStatement(s.text);
      return text == null ? [] : [{ s, i, text }];
    });

  const statements = (value: unknown, path: string) => {
    const items = said(value);
    if (items.length === 0) return <p className="not-discussed">{t("Not discussed")}</p>;
    return (
      <ul className="stmts">
        {items.map(({ s, i, text }) => (
          <li key={i}>{statement(s, `${path}/${i}`, text)}</li>
        ))}
      </ul>
    );
  };

  const paragraph = (value: unknown, path: string) => {
    const items = said(value);
    if (items.length === 0) return <p className="not-discussed">{t("Not discussed")}</p>;
    return (
      <div className="text-box">
        {items.map(({ s, i, text }) => (
          <span key={i}>
            {statement(s, `${path}/${i}`, text)}{" "}
          </span>
        ))}
      </div>
    );
  };

  const fields = (defs: TemplateField[], value: unknown, path: string) => {
    const has = (d: TemplateField) => fieldText((get(value, d.key) ?? {}) as Statement) !== "";
    const present = defs.filter(has);
    const missing = defs.filter((d) => !has(d));
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
    const items = (Array.isArray(value) ? (value as Statement[]) : []).map((m, i) => ({ m, i })).filter(({ m }) => namesDrug(m));
    if (items.length === 0) return <p className="not-discussed">{t("Not discussed")}</p>;
    return (
      <ul className="stmts">
        {items.map(({ m, i }) => {
          const details = medicationDetails(m);
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

  const teeth = (value: unknown, path: string, dentition: Dentition | null) => {
    const heading = path.slice(1);
    const evidenceOf = (id: string) => [...new Set(asItems(value).filter((i) => i.tooth === id).flatMap((i) => i.evidence ?? []))];
    return (
      <DentalChart
        dentition={dentition}
        items={value}
        faithfulness={run.faithfulness}
        path={path}
        onEvidence={onEvidence}
        onPlay={onPlay}
        selected={pickedTooth ? [pickedTooth] : []}
        onToothActivate={(id) => {
          if (onEdit) return onEdit(heading, { tooth: id });
          setPickedTooth((p) => (p === id ? null : id));
          onEvidence(evidenceOf(id));
        }}
        toggles={!onEdit}
        editable={Boolean(onEdit)}
        suggestedDentition={suggestedDentition}
        onChooseDentition={onEdit && onChooseDentition ? (d) => onChooseDentition(heading, d) : undefined}
        checks={chartChecks}
        unconfirmed={unconfirmedTeeth}
        onConfirm={onConfirmTeeth}
        confirming={confirming}
      />
    );
  };

  const content = (kind: ContentKind, defs: TemplateField[] | undefined, value: unknown, path: string, dentition: Dentition | null = null) =>
    kind === "statements"
      ? statements(value, path)
      : kind === "text"
        ? paragraph(value, path)
        : kind === "fields"
          ? fields(defs ?? [], value, path)
          : kind === "teeth"
            ? teeth(value, path, dentition)
            : kind === "medications"
              ? medications(value, path)
              : <p className="not-discussed">{t("This heading can't be shown in this version of OpenBark.")}</p>;

  /** Normal findings from the template waiting under a heading (ADR-0032). */
  const defaultsFor = (heading: string) => pendingDefaults.filter((d) => d.heading === heading);

  /** The waiting normal findings of a heading, set apart, each with Confirm, Edit and Remove. */
  const defaultsBlock = (heading: string, title: string) => {
    const lines = defaultsFor(heading);
    if (lines.length === 0) return null;
    const busy = settlingDefaults;
    return (
      <div className="template-defaults" data-template-default role="group" aria-label={t("Template default, not from the recording")}>
        <p className="template-defaults-label small">{t("Template default, not from the recording")}</p>
        <ul className="template-defaults-list">
          {lines.map((d) => {
            const editing = rewording?.heading === heading && rewording.label === d.label ? rewording : null;
            return (
              <li key={d.label} className="template-default">
                {editing ? (
                  <span className="template-default-edit">
                    <span className="template-default-label">{`${d.label}:`}</span>
                    <input
                      className="ob-input"
                      aria-label={t("Wording for {label}", { label: d.label })}
                      value={editing.text}
                      onChange={(e) => setRewording({ ...editing, text: e.target.value })}
                    />
                    <button
                      type="button"
                      className="ob-button section-tool"
                      data-variant="ghost"
                      disabled={busy || !editing.text.trim()}
                      onClick={() => {
                        onConfirmDefaults?.(heading, [d.label], editing.text.trim());
                        setRewording(null);
                      }}
                    >
                      <Check size={16} aria-hidden />
                      {t("Confirm")}
                    </button>
                    <button type="button" className="ob-button section-tool" data-variant="ghost" onClick={() => setRewording(null)}>
                      {t("Cancel")}
                    </button>
                  </span>
                ) : (
                  <>
                    <span className="template-default-text">{`${d.label}: ${d.text}`}</span>
                    {onConfirmDefaults && (
                      <button
                        type="button"
                        className="ob-button section-tool"
                        data-variant="ghost"
                        disabled={busy}
                        aria-label={t("Confirm {label}", { label: d.label })}
                        onClick={() => onConfirmDefaults(heading, [d.label])}
                      >
                        <Check size={16} aria-hidden />
                        {t("Confirm")}
                      </button>
                    )}
                    {onConfirmDefaults && (
                      <button
                        type="button"
                        className="ob-button section-tool"
                        data-variant="ghost"
                        disabled={busy}
                        aria-label={t("Edit {label}", { label: d.label })}
                        onClick={() => setRewording({ heading, label: d.label, text: d.text })}
                      >
                        <Pencil size={16} aria-hidden />
                        {t("Edit")}
                      </button>
                    )}
                    {onRemoveDefaults && (
                      <button
                        type="button"
                        className="ob-button section-tool"
                        data-variant="ghost"
                        disabled={busy}
                        aria-label={t("Remove {label}", { label: d.label })}
                        onClick={() => onRemoveDefaults(heading, [d.label])}
                      >
                        <X size={16} aria-hidden />
                        {t("Remove")}
                      </button>
                    )}
                  </>
                )}
              </li>
            );
          })}
        </ul>
        {onConfirmDefaults && lines.length > 1 && (
          <button
            type="button"
            className="ob-button"
            disabled={busy}
            aria-label={t("Confirm all normal findings under {heading}", { heading: title })}
            onClick={() => onConfirmDefaults(heading, lines.map((d) => d.label))}
          >
            <Check size={16} aria-hidden />
            {t("Confirm all")}
          </button>
        )}
      </div>
    );
  };

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
        <MessageSquareText size={16} aria-hidden />
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
        <Pencil size={16} aria-hidden />
        {t("Edit")}
      </button>
    );
  const edited = run.edit?.heading;
  /** "Template defaults: Eyes confirmed by Dr Martin." for the edit that confirmed or removed them. */
  const settledNote = (heading: string) => {
    const e = run.edit;
    if (!e || e.heading !== heading) return null;
    const confirmed = e.defaults_confirmed ?? [];
    const removed = e.defaults_removed ?? [];
    if (!confirmed.length && !removed.length) return null;
    return (
      <p className="refined small muted">
        {confirmed.length > 0 && t("Normal findings confirmed by {name}: {systems}.", { name: e.by, systems: confirmed.join(", ") })}
        {confirmed.length > 0 && removed.length > 0 && " "}
        {removed.length > 0 && t("Normal findings removed by {name}: {systems}.", { name: e.by, systems: removed.join(", ") })}
      </p>
    );
  };
  const refinedNote = (heading: string) =>
    settledNote(heading) ||
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

  /** The heading's content in transcript type, or its editor (accent gutter bar until saved). */
  const body = (heading: string, kind: ContentKind, defs: TemplateField[] | undefined, value: unknown, dentition: Dentition | null = null) => {
    const editor = renderEdit?.(heading);
    return editor ? (
      <div className="note-editing">{editor}</div>
    ) : (
      <div className="note-body transcript-type">{content(kind, defs, value, `/${heading}`, dentition)}</div>
    );
  };

  const part = (p: TemplatePart, sectionValue: unknown, sectionId: string) => {
    const value = get(sectionValue, p.key);
    const heading = `${sectionId}/${p.key}`;
    return (
      <div className="note-part" key={p.key}>
        <div className="note-part-head">
          <h4 className="label">{p.label}</h4>
          {editButton(heading, p.label)}
          {askButton(heading, p.label)}
        </div>
        {refinedNote(heading)}
        {renderAsk?.(heading)}
        {body(heading, p.kind, p.fields, value, p.dentition ?? null)}
        {defaultsBlock(heading, p.label)}
      </div>
    );
  };

  const sectionText = (id: string) => {
    // A chart copies as words, never the diagram's tooth numbers.
    const sec = template.sections.find((s) => s.id === id);
    // Waiting normal findings from the template aren't part of the note, so a heading with them copies as words too.
    const waiting = pendingDefaults.some((d) => d.heading === id || d.heading.startsWith(`${id}/`));
    if (sec && (sec.kind === "teeth" || sec.parts?.some((p) => p.kind === "teeth") || waiting)) {
      const value = run.note[id];
      const blocks = sec.kind
        ? [headingLines(sec.kind, sec.fields, value, false, t, lowerInner)]
        : (sec.parts ?? []).map((p) => [p.label, ...headingLines(p.kind, p.fields, (value as Record<string, unknown> | undefined)?.[p.key], false, t, lowerInner)]);
      const text = blocks.map((l) => l.join("\n")).join("\n\n").trim();
      return text || t("Not discussed");
    }
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
              <h3>{sec.title}</h3>
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
            {sec.kind ? (
              <>
                {body(sec.id, sec.kind, sec.fields, value, sec.dentition ?? null)}
                {defaultsBlock(sec.id, sec.title)}
              </>
            ) : (
              (sec.parts ?? []).map((p) => part(p, value, sec.id))
            )}
          </section>
        );
      })}
    </article>
  );
}
