import { Button, StatusDot } from "@openbark/ui";
import { ArrowLeft, CaretDown, CaretUp, CopySimple, DotsSixVertical, Plus, Trash, X } from "@phosphor-icons/react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useApp } from "../context";
import * as engine from "../engine";
import { useT } from "../i18n";
import type { ContentKind, Standard, Template, TemplateField, TemplatePart, TemplateSection } from "../engine";

/**
 * DESIGN.md §5 Template builder: a team's own note templates. A template is
 * headings, what goes under each, and plain-language instructions; the
 * engine turns that into the structure the model fills, so every template
 * keeps evidence links, "Not discussed" and the faithfulness checks.
 */
export function Templates() {
  const t = useT();
  const [open, setOpen] = useState<{ id: string | null; from?: Template } | null>(null);
  const [list, setList] = useState<engine.TemplateSummary[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(() => engine.listTemplates().then(setList, (e) => setError(String(e?.message ?? e))), []);
  useEffect(() => void refresh(), [refresh]);

  if (open) {
    return (
      <Builder
        id={open.id}
        from={open.from}
        onClose={() => {
          setOpen(null);
          void refresh();
        }}
        onOpen={(id, from) => setOpen({ id, from })}
        onSaved={() => void refresh()}
      />
    );
  }

  const team = list?.filter((x) => !x.builtin) ?? [];
  const startFromSoap = async () => {
    try {
      const soap = await engine.template("soap");
      setOpen({ id: null, from: copyOf(soap.template, t("SOAP note (team)")) });
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  return (
    <div className="screen templates">
      <header className="screen-head">
        <h1>{t("Templates")}</h1>
        <div className="head-actions">
          <Button onClick={startFromSoap}>
            <CopySimple size={20} aria-hidden />
            {t("Start from SOAP")}
          </Button>
          <Button variant="primary" onClick={() => setOpen({ id: null, from: blank() })}>
            <Plus size={20} aria-hidden />
            {t("New template")}
          </Button>
        </div>
      </header>
      <p className="small muted measure">
        {t(
          "A template is the headings of a note and what goes under each. The model fills every heading from the recording and links each line to where it was said; anything not discussed is shown as not discussed. Requests saved as standards while reviewing a note appear under their heading.",
        )}
      </p>
      {error && (
        <p className="ob-notice" data-tone="danger" role="alert">
          {error}
        </p>
      )}
      {list === null ? (
        <div className="ob-skeleton" style={{ height: 120 }} />
      ) : (
        <div className="table-wrap">
          <table className="ob-table">
            <thead>
              <tr>
                <th>{t("Name")}</th>
                <th>{t("Description")}</th>
                <th className="num">{t("Headings")}</th>
                <th className="num">{t("Standards")}</th>
                <th>{t("Version")}</th>
                <th>{t("Kind")}</th>
              </tr>
            </thead>
            <tbody>
              {list.map((tpl) => (
                <tr
                  key={tpl.id}
                  tabIndex={0}
                  className="row-link"
                  onClick={() => setOpen({ id: tpl.id })}
                  onKeyDown={(e) => e.key === "Enter" && setOpen({ id: tpl.id })}
                >
                  <td>{tpl.name}</td>
                  <td className="muted reason-cell">{tpl.description}</td>
                  <td className="num">{tpl.headings}</td>
                  <td className="num">{tpl.standards}</td>
                  <td className="mono">{tpl.version}</td>
                  <td>
                    {tpl.builtin ? <StatusDot>{t("Built in")}</StatusDot> : <StatusDot tone="accent">{t("Team")}</StatusDot>}
                    {tpl.document === "client" && <span className="small muted"> {t("For the client")}</span>}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      {list !== null && team.length === 0 && (
        <p className="empty small muted">{t("No team templates yet. Start from SOAP, or start a new one from blank headings.")}</p>
      )}
    </div>
  );
}

// ---- builder ------------------------------------------------------------------

type Kind = ContentKind | "parts";
const KINDS: { value: Kind; label: string }[] = [
  { value: "statements", label: "Bullet points" },
  { value: "fields", label: "Measurements" },
  { value: "medications", label: "Medications" },
  { value: "parts", label: "Subheadings" },
];

let uidCounter = 0;
const uid = () => `u${++uidCounter}`;
type WithUid<T> = T & { _uid?: string };

function withUids(t: Template): Template {
  return {
    ...t,
    sections: t.sections.map((s) => ({
      ...s,
      _uid: uid(),
      ...(s.parts ? { parts: s.parts.map((p) => ({ ...p, _uid: uid() })) } : {}),
    })) as TemplateSection[],
  };
}

/** The template as the engine takes it: no UI ids, no standards (kept separately). */
function clean(t: Template): Template {
  const json = JSON.stringify({ ...t, standards: undefined }, (k, v) => (k === "_uid" ? undefined : v));
  return JSON.parse(json) as Template;
}

function blank(): Template {
  return { id: "", version: "1", name: "", description: "", sections: [{ id: "", title: "", kind: "statements" }] };
}

function copyOf(t: Template, name: string): Template {
  return { ...clean(t), id: "", version: "1", name };
}

function move<T>(items: T[], from: number, to: number): T[] {
  if (to < 0 || to >= items.length || from === to) return items;
  const next = items.slice();
  next.splice(to, 0, ...next.splice(from, 1));
  return next;
}

interface BuilderProps {
  id: string | null;
  from?: Template;
  onClose: () => void;
  onOpen: (id: string | null, from?: Template) => void;
  onSaved: () => void;
}

function Builder({ id, from, onClose, onOpen, onSaved }: BuilderProps) {
  const app = useApp();
  const t = useT();
  const [draft, setDraft] = useState<Template | null>(from ? withUids(from) : null);
  const [saved, setSaved] = useState<Template | null>(null);
  const [builtin, setBuiltin] = useState(false);
  const [standards, setStandards] = useState<Standard[]>([]);
  const [preview, setPreview] = useState<engine.TemplateView | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [tab, setTab] = useState<"note" | "prompt">("note");
  const [confirm, setConfirm] = useState<"delete" | "leave" | null>(null);
  const [busy, setBusy] = useState(false);
  const [encounters, setEncounters] = useState<engine.SessionSummary[]>([]);
  const [tryOn, setTryOn] = useState("");
  const nameRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (id) {
      engine.template(id).then(
        (v) => {
          setDraft(withUids(v.template));
          setSaved(clean(v.template));
          setBuiltin(v.builtin);
          setStandards(v.template.standards ?? []);
        },
        (e) => setError(String(e?.message ?? e)),
      );
    } else {
      nameRef.current?.focus();
    }
    engine.listSessions().then(
      (rows) => {
        setEncounters(rows.slice(0, 20));
        setTryOn(rows[0]?.id ?? "");
      },
      () => {},
    );
  }, [id]);

  // Live check and prompt preview, a moment after typing stops.
  useEffect(() => {
    if (!draft) return;
    const timer = setTimeout(() => {
      engine.previewTemplate(clean(draft)).then(
        (v) => {
          setPreview(v);
          setProblem(null);
        },
        (e) => setProblem(e instanceof Error ? e.message : String(e)),
      );
    }, 300);
    return () => clearTimeout(timer);
  }, [draft]);

  const dirty = useMemo(
    () => !saved || (draft != null && JSON.stringify(clean(draft)) !== JSON.stringify(saved)),
    [draft, saved],
  );
  const readOnly = builtin;

  if (!draft) {
    return (
      <div className="screen">
        {error ? (
          <p className="ob-notice" data-tone="danger" role="alert">
            {error}
          </p>
        ) : (
          <div className="ob-skeleton" style={{ height: 240 }} />
        )}
      </div>
    );
  }

  const set = (patch: Partial<Template>) => setDraft((d) => (d ? { ...d, ...patch } : d));
  const setSections = (f: (s: TemplateSection[]) => TemplateSection[]) =>
    setDraft((d) => (d ? { ...d, sections: f(d.sections) } : d));
  const setSection = (i: number, patch: Partial<TemplateSection>) =>
    setSections((ss) => ss.map((s, j) => (j === i ? { ...s, ...patch } : s)));

  async function save() {
    if (!draft) return;
    setBusy(true);
    setError(null);
    try {
      const v = await engine.saveTemplate(clean(draft));
      const wasNew = !saved;
      setDraft(withUids(v.template));
      setSaved(clean(v.template));
      setStandards(v.template.standards ?? []);
      setBuiltin(v.builtin);
      app.toast(t("Template saved, version {v}", { v: v.template.version }));
      onSaved();
      if (wasNew) onOpen(v.template.id);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  async function remove() {
    if (!saved) return;
    try {
      await engine.deleteTemplate(saved.id);
      app.toast(t("Template deleted"));
      onClose();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }

  async function dropStandard(s: Standard) {
    if (!saved) return;
    try {
      const v = await engine.removeStandard(saved.id, s.id);
      setStandards(v.template.standards ?? []);
      app.toast(t("Standard removed"));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }

  function tryTemplate() {
    if (!saved || !tryOn) return;
    if (saved.id !== app.settings.template) app.setSettings({ template: saved.id });
    app.navigate({ screen: "review", id: tryOn, template: saved.id });
  }

  const standardsFor = (heading: string) => standards.filter((s) => s.heading === heading);
  const wholeNote = standardsFor("");

  return (
    <div className="screen builder">
      <header className="screen-head">
        <div className="builder-title">
          {confirm === "leave" ? (
            <span className="inline-confirm">
              <span className="small">{t("Discard unsaved changes?")}</span>
              <Button onClick={onClose}>{t("Discard changes")}</Button>
              <Button variant="ghost" onClick={() => setConfirm(null)}>
                {t("Keep editing")}
              </Button>
            </span>
          ) : (
            <Button variant="ghost" onClick={() => (dirty && !readOnly ? setConfirm("leave") : onClose())}>
              <ArrowLeft size={20} aria-hidden />
              {t("Templates")}
            </Button>
          )}
          <h1>{draft.name || t("New template")}</h1>
          {saved && <span className="mono faint">{t("version {v}", { v: saved.version })}</span>}
        </div>
        <div className="head-actions">
          {saved && (
            <Button onClick={() => onOpen(null, copyOf(draft, t("{name} copy", { name: draft.name })))}>
              <CopySimple size={20} aria-hidden />
              {t("Duplicate")}
            </Button>
          )}
          {saved && !readOnly &&
            (confirm === "delete" ? (
              <span className="inline-confirm">
                <Button onClick={remove}>{t("Delete for good")}</Button>
                <Button variant="ghost" onClick={() => setConfirm(null)}>
                  {t("Keep")}
                </Button>
              </span>
            ) : (
              <Button variant="ghost" onClick={() => setConfirm("delete")}>
                <Trash size={20} aria-hidden />
                {t("Delete")}
              </Button>
            ))}
          {!readOnly && (
            <Button variant="primary" onClick={save} disabled={busy || !dirty || problem != null}>
              {t("Save template")}
            </Button>
          )}
        </div>
      </header>

      {readOnly && (
        <p className="ob-notice">
          {t(
            "{name} is built in and can't be changed. Duplicate it to make your team's version. Standards saved while reviewing notes still apply to it, and you can remove them here.",
            { name: draft.name },
          )}
        </p>
      )}
      {error && (
        <p className="ob-notice" data-tone="danger" role="alert">
          {error}
        </p>
      )}

      <div className="builder-meta">
        <label className="ob-field">
          <span className="label">{t("Name")}</span>
          <input
            ref={nameRef}
            className="ob-input"
            value={draft.name}
            disabled={readOnly}
            placeholder={t("Dental procedure")}
            onChange={(e) => set({ name: e.target.value })}
          />
        </label>
        <label className="ob-field">
          <span className="label">{t("Description")}</span>
          <input
            className="ob-input"
            value={draft.description ?? ""}
            disabled={readOnly}
            placeholder={t("Who uses it, and when")}
            onChange={(e) => set({ description: e.target.value })}
          />
        </label>
        <label className="ob-field">
          <span className="label">{t("Who it's for")}</span>
          <select
            className="ob-select"
            value={draft.document ?? "record"}
            disabled={readOnly}
            onChange={(e) => set({ document: e.target.value === "client" ? "client" : undefined })}
          >
            <option value="record">{t("The medical record (signed by a veterinarian)")}</option>
            <option value="client">{t("The client (follows the signed note; never signed)")}</option>
          </select>
        </label>
        <details className="builder-advanced">
          <summary className="label">{t("Instructions and rules for the whole note")}</summary>
          <label className="ob-field">
            <span className="label muted">{t("Instructions")}</span>
            <textarea
              className="ob-input tb-text"
              rows={4}
              disabled={readOnly}
              value={draft.instructions ?? ""}
              placeholder={t("Write the medical record for this consultation under the headings below. Leave empty for the default.")}
              onChange={(e) => set({ instructions: e.target.value || undefined })}
            />
          </label>
          <label className="ob-field">
            <span className="label muted">{t("Extra rules, one per line")}</span>
            <textarea
              className="ob-input tb-text"
              rows={3}
              disabled={readOnly}
              value={(draft.rules ?? []).join("\n")}
              placeholder={t("Record the anaesthetic monitoring interval when stated.")}
              onChange={(e) => set({ rules: e.target.value.split("\n") })}
            />
          </label>
          <p className="ob-help">
            {t(
              "Every template also follows the core rules: only what was said, numbers exactly as stated, and every line linked to the transcript.",
            )}
          </p>
          {wholeNote.length > 0 && <StandardList title={t("Standards for the whole note")} items={wholeNote} onRemove={dropStandard} />}
        </details>
      </div>

      <div className="builder-split">
        <section className="pane builder-structure" aria-label={t("Headings")}>
          <ol className="tb-list">
            {draft.sections.map((s, i) => (
              <HeadingRow
                key={(s as WithUid<TemplateSection>)._uid ?? i}
                section={s}
                index={i}
                count={draft.sections.length}
                readOnly={readOnly}
                standardsFor={standardsFor}
                onChange={(patch) => setSection(i, patch)}
                onMove={(to) => setSections((ss) => move(ss, i, to))}
                onDropFrom={(from) => setSections((ss) => move(ss, from, i))}
                onRemove={() => setSections((ss) => ss.filter((_, j) => j !== i))}
                onRemoveStandard={dropStandard}
              />
            ))}
          </ol>
          {!readOnly && (
            <Button
              onClick={() =>
                setSections((ss) => [...ss, { id: "", title: "", kind: "statements", _uid: uid() } as TemplateSection])
              }
            >
              <Plus size={20} aria-hidden />
              {t("Add heading")}
            </Button>
          )}
        </section>

        <section className="pane builder-preview" aria-label={t("Preview")}>
          <div className="ob-segmented" role="tablist">
            <button className="ob-segment" role="tab" aria-selected={tab === "note"} onClick={() => setTab("note")}>
              {t("Note")}
            </button>
            <button className="ob-segment" role="tab" aria-selected={tab === "prompt"} onClick={() => setTab("prompt")}>
              {t("What the model is told")}
            </button>
          </div>
          {problem && (
            <p className="ob-notice" role="status">
              {problem}
            </p>
          )}
          {tab === "note" ? (
            <TemplatePreview template={preview?.template ?? draft} />
          ) : (
            <pre className="prompt">{preview?.prompt ?? ""}</pre>
          )}
          <div className="try-on">
            <span className="label">{t("Try on an encounter")}</span>
            <div className="try-row">
              <select
                className="ob-select"
                aria-label={t("Encounter to draft")}
                value={tryOn}
                onChange={(e) => setTryOn(e.target.value)}
                disabled={encounters.length === 0}
              >
                {encounters.length === 0 && <option value="">{t("No encounters yet")}</option>}
                {encounters.map((r) => (
                  <option key={r.id} value={r.id}>
                    {r.created_at} {r.patient || r.source}
                  </option>
                ))}
              </select>
              <Button onClick={tryTemplate} disabled={!saved || dirty || !tryOn}>
                {t("Draft with this template")}
              </Button>
            </div>
            {(dirty || !saved) && !readOnly && <span className="ob-help">{t("Save the template first.")}</span>}
          </div>
        </section>
      </div>
    </div>
  );
}

interface HeadingRowProps {
  section: TemplateSection;
  index: number;
  count: number;
  readOnly: boolean;
  standardsFor: (heading: string) => Standard[];
  onChange: (patch: Partial<TemplateSection>) => void;
  onMove: (to: number) => void;
  /** Another heading was dragged onto this one. */
  onDropFrom: (from: number) => void;
  onRemove: () => void;
  onRemoveStandard: (s: Standard) => void;
}

/** Drag state shared by rows of one list (HTML drag and drop, handle only). */
let dragging: { list: string; index: number } | null = null;

function HeadingRow({
  section: s,
  index,
  count,
  readOnly,
  standardsFor,
  onChange,
  onMove,
  onDropFrom,
  onRemove,
  onRemoveStandard,
}: HeadingRowProps) {
  const t = useT();
  const kind: Kind = s.kind ?? "parts";
  const [armed, setArmed] = useState(false);
  const parts = s.parts ?? [];
  const setParts = (f: (p: TemplatePart[]) => TemplatePart[]) => onChange({ parts: f(parts) });
  const changeKind = (k: Kind) => {
    if (k === "parts") {
      onChange({
        kind: null,
        fields: [],
        parts: parts.length ? parts : [{ key: "", label: "", kind: "statements", _uid: uid() } as TemplatePart],
      });
    } else {
      onChange({ kind: k, parts: [], fields: k === "fields" && !(s.fields ?? []).length ? [{ key: "", label: "" }] : s.fields });
    }
  };
  const label = s.title || t("Heading {n}", { n: index + 1 });

  return (
    <li
      className="tb-heading"
      draggable={armed && !readOnly}
      onDragStart={(e) => {
        dragging = { list: "sections", index };
        e.dataTransfer.effectAllowed = "move";
      }}
      onDragEnd={() => {
        dragging = null;
        setArmed(false);
      }}
      onDragOver={(e) => dragging?.list === "sections" && e.preventDefault()}
      onDrop={(e) => {
        e.preventDefault();
        if (dragging?.list === "sections" && dragging.index !== index) onDropFrom(dragging.index);
        dragging = null;
      }}
    >
      <div className="tb-row">
        <span
          className="tb-handle"
          aria-hidden
          onPointerDown={() => setArmed(true)}
          onPointerUp={() => setArmed(false)}
          title={t("Drag to reorder")}
        >
          <DotsSixVertical size={20} />
        </span>
        <input
          className="ob-input tb-title"
          aria-label={t("Heading {n} title", { n: index + 1 })}
          value={s.title}
          disabled={readOnly}
          placeholder={t("Heading, for example Anaesthesia")}
          onChange={(e) => onChange({ title: e.target.value })}
        />
        <select
          className="ob-select"
          aria-label={t("What goes under {label}", { label })}
          value={kind}
          disabled={readOnly}
          onChange={(e) => changeKind(e.target.value as Kind)}
        >
          {KINDS.map((k) => (
            <option key={k.value} value={k.value}>
              {t(k.label)}
            </option>
          ))}
        </select>
        {!readOnly && (
          <RowTools label={label} index={index} count={count} onMove={onMove} onRemove={onRemove} />
        )}
      </div>
      <textarea
        className="ob-input tb-text"
        rows={2}
        aria-label={t("Instruction for {label}", { label })}
        disabled={readOnly}
        value={s.instruction ?? ""}
        placeholder={t("Instruction for the model (optional): what belongs here and how to write it")}
        onChange={(e) => onChange({ instruction: e.target.value || null })}
      />
      {kind === "fields" && (
        <FieldsEditor fields={s.fields ?? []} readOnly={readOnly} owner={label} onChange={(fields) => onChange({ fields })} />
      )}
      <StandardList title={t("Standards")} items={standardsFor(s.id)} onRemove={onRemoveStandard} />
      {kind === "parts" && (
        <div className="tb-parts">
          <ol className="tb-list">
            {parts.map((p, j) => (
              <PartRow
                key={(p as WithUid<TemplatePart>)._uid ?? j}
                part={p}
                index={j}
                count={parts.length}
                readOnly={readOnly}
                standards={standardsFor(`${s.id}/${p.key}`)}
                onChange={(patch) => setParts((ps) => ps.map((x, k) => (k === j ? { ...x, ...patch } : x)))}
                onMove={(to) => setParts((ps) => move(ps, j, to))}
                onRemove={() => setParts((ps) => ps.filter((_, k) => k !== j))}
                onRemoveStandard={onRemoveStandard}
              />
            ))}
          </ol>
          {!readOnly && (
            <Button
              variant="ghost"
              onClick={() => setParts((ps) => [...ps, { key: "", label: "", kind: "statements", _uid: uid() } as TemplatePart])}
            >
              <Plus size={16} aria-hidden />
              {t("Add subheading")}
            </Button>
          )}
        </div>
      )}
    </li>
  );
}

function RowTools({
  label,
  index,
  count,
  onMove,
  onRemove,
}: {
  label: string;
  index: number;
  count: number;
  onMove: (to: number) => void;
  onRemove: () => void;
}) {
  const t = useT();
  return (
    <span className="tb-tools">
      <button type="button" className="icon-btn" aria-label={t("Move {label} up", { label })} disabled={index === 0} onClick={() => onMove(index - 1)}>
        <CaretUp size={16} aria-hidden />
      </button>
      <button
        type="button"
        className="icon-btn"
        aria-label={t("Move {label} down", { label })}
        disabled={index === count - 1}
        onClick={() => onMove(index + 1)}
      >
        <CaretDown size={16} aria-hidden />
      </button>
      <button type="button" className="icon-btn" aria-label={t("Remove {label}", { label })} onClick={onRemove}>
        <Trash size={16} aria-hidden />
      </button>
    </span>
  );
}

interface PartRowProps {
  part: TemplatePart;
  index: number;
  count: number;
  readOnly: boolean;
  standards: Standard[];
  onChange: (patch: Partial<TemplatePart>) => void;
  onMove: (to: number) => void;
  onRemove: () => void;
  onRemoveStandard: (s: Standard) => void;
}

function PartRow({ part: p, index, count, readOnly, standards, onChange, onMove, onRemove, onRemoveStandard }: PartRowProps) {
  const t = useT();
  const label = p.label || t("Subheading {n}", { n: index + 1 });
  return (
    <li className="tb-part">
      <div className="tb-row">
        <input
          className="ob-input tb-title"
          aria-label={t("Subheading {n} title", { n: index + 1 })}
          value={p.label}
          disabled={readOnly}
          placeholder={t("Subheading")}
          onChange={(e) => onChange({ label: e.target.value })}
        />
        <select
          className="ob-select"
          aria-label={t("What goes under {label}", { label })}
          value={p.kind}
          disabled={readOnly}
          onChange={(e) => {
            const kind = e.target.value as ContentKind;
            onChange({ kind, fields: kind === "fields" && !(p.fields ?? []).length ? [{ key: "", label: "" }] : p.fields });
          }}
        >
          {KINDS.filter((k) => k.value !== "parts").map((k) => (
            <option key={k.value} value={k.value}>
              {t(k.label)}
            </option>
          ))}
        </select>
        {!readOnly && <RowTools label={label} index={index} count={count} onMove={onMove} onRemove={onRemove} />}
      </div>
      <textarea
        className="ob-input tb-text"
        rows={1}
        aria-label={t("Instruction for {label}", { label })}
        disabled={readOnly}
        value={p.instruction ?? ""}
        placeholder={t("Instruction (optional)")}
        onChange={(e) => onChange({ instruction: e.target.value || null })}
      />
      {p.kind === "fields" && (
        <FieldsEditor fields={p.fields ?? []} readOnly={readOnly} owner={label} onChange={(fields) => onChange({ fields })} />
      )}
      <StandardList title={t("Standards")} items={standards} onRemove={onRemoveStandard} />
    </li>
  );
}

function FieldsEditor({
  fields,
  readOnly,
  owner,
  onChange,
}: {
  fields: TemplateField[];
  readOnly: boolean;
  owner: string;
  onChange: (f: TemplateField[]) => void;
}) {
  const t = useT();
  return (
    <div className="tb-fields">
      <span className="label muted">{t("Measurements")}</span>
      <ul>
        {fields.map((f, i) => (
          <li key={i}>
            <input
              className="ob-input"
              aria-label={t("{owner} measurement {n}", { owner, n: i + 1 })}
              value={f.label}
              disabled={readOnly}
              placeholder={t("Weight")}
              onChange={(e) => onChange(fields.map((x, j) => (j === i ? { ...x, label: e.target.value } : x)))}
            />
            {!readOnly && (
              <button
                type="button"
                className="icon-btn"
                aria-label={t("Remove {label}", { label: f.label || t("measurement") })}
                onClick={() => onChange(fields.filter((_, j) => j !== i))}
              >
                <X size={16} aria-hidden />
              </button>
            )}
          </li>
        ))}
      </ul>
      {!readOnly && (
        <Button variant="ghost" onClick={() => onChange([...fields, { key: "", label: "" }])}>
          <Plus size={16} aria-hidden />
          {t("Add measurement")}
        </Button>
      )}
    </div>
  );
}

function StandardList({ title, items, onRemove }: { title: string; items: Standard[]; onRemove: (s: Standard) => void }) {
  const t = useT();
  if (items.length === 0) return null;
  return (
    <div className="tb-standards">
      <span className="label muted">{title}</span>
      <ul>
        {items.map((s) => (
          <li key={s.id}>
            <span className="ob-dot" data-tone="accent" aria-hidden />
            <span className="small">{s.text}</span>
            <span className="small faint">
              {s.added_by ? `${s.added_by}, ` : ""}
              {engine.fmtStamp(s.added_at)}
            </span>
            <button type="button" className="icon-btn" aria-label={t("Remove standard: {text}", { text: s.text })} onClick={() => onRemove(s)}>
              <X size={16} aria-hidden />
            </button>
          </li>
        ))}
      </ul>
    </div>
  );
}

const KIND_HINT: Record<ContentKind, string> = {
  statements: "Bullet points, each linked to where it was said",
  fields: "",
  medications: "Drug, dose, route, frequency, duration",
};

/** The note's shape: every heading, with what the model will put there. */
function TemplatePreview({ template }: { template: Template }) {
  const t = useT();
  const body = (kind: ContentKind, fields: TemplateField[] | undefined, instruction?: string | null) => (
    <>
      {kind === "fields" ? (
        <dl className="fields">
          {(fields ?? []).map((f, i) => (
            <div key={i}>
              <dt className="label muted">{f.label || t("Unnamed")}</dt>
              <dd className="not-discussed">{t("Not discussed")}</dd>
            </div>
          ))}
        </dl>
      ) : (
        <p className="not-discussed">{t(KIND_HINT[kind])}</p>
      )}
      {instruction && <p className="small faint preview-instruction">{instruction}</p>}
    </>
  );
  return (
    <article className="note preview-note" aria-label={t("Template preview")}>
      <h2 className="preview-name">{template.name || t("New template")}</h2>
      {template.sections.map((s, i) => (
        <section key={i} className="note-section">
          <header className="note-section-head">
            <h2>{s.title || t("Untitled heading")}</h2>
          </header>
          {s.kind
            ? body(s.kind, s.fields, s.instruction)
            : (
                <>
                  {s.instruction && <p className="small faint preview-instruction">{s.instruction}</p>}
                  {(s.parts ?? []).map((p, j) => (
                    <div className="note-part" key={j}>
                      <h3 className="label">{p.label || t("Untitled subheading")}</h3>
                      {body(p.kind, p.fields, p.instruction)}
                    </div>
                  ))}
                </>
              )}
        </section>
      ))}
    </article>
  );
}
