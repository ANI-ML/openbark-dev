import { Button } from "@openbark/ui";
import { Play, Plus, X } from "lucide-react";
import { useState, type KeyboardEvent } from "react";
import {
  CHART,
  asItems,
  findingDef,
  findingText,
  findingWords,
  hasTooth,
  lowerFirst,
  misfit,
  newItem,
  tooth,
  toothName,
  triadanOrder,
  useLowerInner,
  type Dentition,
  type FindingDef,
  type ToothFinding,
} from "../dental";
import { useT } from "../i18n";
import { ToothDiagram } from "./DentalChart";
import "./DentalChart.css";

/**
 * Edit a tooth chart by clicking (ADR-0021 §8, plan step 11). The chart
 * above, a panel for the selected teeth below. Everything changed or added
 * becomes the vet's own entry when saved (the engine marks it); findings from
 * the recording keep their link to it.
 *
 * | Input | Result |
 * |---|---|
 * | Click a tooth, or Enter on it | Selects only that tooth |
 * | Ctrl or Cmd plus click, or Space | Adds the tooth to the selection, or removes it |
 * | Select several (a toggle, for touch) | Each click adds or removes a tooth |
 * | Arrows, Home, End | Move along the row and between rows (the diagram) |
 * | Escape | Closes the finding form; a second Escape clears the selection |
 * | Mark missing, Mark normal | Only for selected teeth with no findings yet |
 * | Add finding | Opens the form for all selected teeth; Add adds one item per tooth |
 * | Change, Remove, Play | On each finding of the selected teeth |
 * | Dog or cat? | Only when the chart has no dentition; Save waits for it |
 */
export interface DentalChartEditorProps {
  title: string;
  /** The chart's dentition; null asks "Dog or cat?" before saving. */
  dentition: Dentition | null;
  /** The chart heading's current value. */
  value: unknown;
  busy: boolean;
  /** The tooth clicked to open the editor. */
  initialSelection?: string[];
  /** The whole chart, and the animal chosen here (null when the chart already had one). */
  onSave: (items: ToothFinding[], dentition: Dentition | null) => void;
  onCancel: () => void;
  onPlay?: (ids: number[]) => void;
  onEvidence?: (ids: number[] | null) => void;
  /** A transcript line's words, shown beside a finding from the recording. */
  lineText?: (id: number) => string | undefined;
}

const GROUPS: [string, string][] = [
  ["presence", "Normal or missing"],
  ["fracture", "Fracture"],
  ["periodontal", "Periodontal"],
  ["resorption", "Resorption"],
  ["development", "Eruption"],
  ["condition", "Tooth condition"],
  ["treatment", "Treatment"],
  ["other", "Other"],
];
/** What the type control is called for each finding. */
const DETAIL_LABEL: Record<string, string> = { fracture: "Type", wear: "Type", malposition: "Version", extraction: "Technique" };
const GRADE_LABEL: Record<string, string> = {
  periodontal_stage: "Stage",
  resorption: "Stage",
  resorption_type: "Type",
  mobility: "Grade",
  furcation: "Grade",
};

interface FormValues {
  finding: string;
  detail: string;
  /** "" for not said; "4b" for a resorption sub-stage. */
  grade: string;
  mm: string;
  planned: boolean;
  text: string;
}

const EMPTY: FormValues = { finding: "", detail: "", grade: "", mm: "", planned: false, text: "" };

function valuesFor(finding: string): FormValues {
  const f = findingDef(finding);
  return { ...EMPTY, finding, grade: f?.grade?.required ? String(f.grade.min) : "" };
}

function valuesOf(item: ToothFinding): FormValues {
  const f = findingDef(item.finding);
  const sub = f?.details.find((d) => d.id === item.detail && d.with_grade != null);
  const plainDetail = f?.details.find((d) => d.id === item.detail && d.with_grade == null);
  return {
    finding: item.finding,
    detail: plainDetail?.id ?? "",
    grade: item.grade == null ? "" : `${item.grade}${sub ? sub.code : ""}`,
    mm: item.mm == null ? "" : String(item.mm),
    planned: !!item.planned,
    text: f?.text_required ? (item.text ?? "") : "",
  };
}

function fieldsOf(v: FormValues): Pick<ToothFinding, "finding" | "detail" | "grade" | "mm" | "planned" | "text"> {
  const f = findingDef(v.finding);
  let grade: number | null = v.grade === "" ? null : Number.parseInt(v.grade, 10);
  let detail: string | null = v.detail || null;
  const sub = /^(\d)([a-z])$/.exec(v.grade);
  if (sub && f) {
    grade = Number(sub[1]);
    detail = f.details.find((d) => d.code === sub[2] && d.with_grade === grade)?.id ?? null;
  }
  const mmText = v.mm.trim();
  return {
    finding: v.finding,
    detail,
    grade,
    mm: mmText === "" ? null : Number(mmText),
    planned: f?.plannable ? v.planned : false,
    text: v.text.trim(),
  };
}

export function DentalChartEditor({
  title,
  dentition,
  value,
  busy,
  initialSelection = [],
  onSave,
  onCancel,
  onPlay,
  onEvidence,
  lineText,
}: DentalChartEditorProps) {
  const t = useT();
  const lowerInner = useLowerInner();
  const [items, setItems] = useState<ToothFinding[]>(() => structuredClone(asItems(value)));
  const [selection, setSelection] = useState<string[]>(initialSelection);
  const [choice, setChoice] = useState<Dentition | null>(null);
  const [deciduous, setDeciduous] = useState(() =>
    [...asItems(value).map((i) => i.tooth), ...initialSelection].some((id) => tooth(id)?.deciduous),
  );
  /** null: closed; -1: adding; otherwise the index of the item being changed. */
  const [form, setForm] = useState<number | null>(null);
  /** Clicks add to the selection (touch screens have no Ctrl or Cmd). */
  const [several, setSeveral] = useState(false);
  const [values, setValues] = useState<FormValues>(EMPTY);

  const drawn: Dentition = dentition ?? choice ?? "canine";
  const sorted = [...selection].sort(triadanOrder);
  const findingsOf = (id: string) => items.filter((i) => i.tooth === id);
  const anyCharted = selection.some((id) => findingsOf(id).length > 0);
  const notInCat = choice === "feline" ? [...new Set(items.map((i) => i.tooth).filter((id) => !hasTooth("feline", id)))].sort(triadanOrder) : [];
  const needsChoice = dentition == null && choice == null;
  const def: FindingDef | undefined = findingDef(values.finding);
  const fields = fieldsOf(values);
  // The teeth the form applies to: every selected tooth when adding, the
  // item's own tooth when changing. The first reason any of them doesn't fit.
  const formTeeth = form === -1 ? sorted : form != null && items[form] ? [items[form].tooth] : [];
  const problem = def
    ? (formTeeth.length ? formTeeth : [undefined]).map((tooth) => misfit({ ...fields, tooth }, t)).find((m) => m) ?? ""
    : "";

  const activate = (id: string, { additive }: { additive: boolean }) =>
    setSelection((sel) => (additive || several ? (sel.includes(id) ? sel.filter((x) => x !== id) : [...sel, id]) : [id]));

  const mark = (finding: "missing" | "normal") => {
    const added = sorted
      .filter((id) => findingsOf(id).length === 0)
      .map((id) => newItem(t, id, { finding, detail: null, grade: null, mm: null, planned: false }, lowerInner));
    setItems((xs) => [...xs, ...added]);
  };

  const openAdd = () => {
    setValues(EMPTY);
    setForm(-1);
  };

  const openChange = (index: number) => {
    const item = items[index];
    if (!item) return;
    setValues(valuesOf(item));
    setForm(index);
  };

  const applyForm = () => {
    if (!def || problem) return;
    if (form === -1) {
      setItems((xs) => [...xs, ...sorted.map((id) => newItem(t, id, fields, lowerInner))]);
    } else if (form != null) {
      setItems((xs) =>
        xs.map((x, i) => {
          if (i !== form) return x;
          const fresh = newItem(t, x.tooth, fields, lowerInner);
          // The same finding, changed: it keeps its link to the recording.
          return { ...x, ...fresh, evidence: x.evidence ?? [] };
        }),
      );
    }
    setForm(null);
  };

  const onKeyDown = (e: KeyboardEvent) => {
    if (e.key !== "Escape" || busy) return;
    e.stopPropagation();
    if (form != null) setForm(null);
    else if (selection.length) setSelection([]);
  };

  const selectionText =
    sorted.length === 0
      ? t("Select teeth on the chart to add findings.")
      : sorted.length === 1
        ? `${sorted[0]}, ${toothName(t, sorted[0] ?? "")}`
        : t("{n} teeth selected: {teeth}", { n: sorted.length, teeth: sorted.join(", ") });

  const rows = items
    .map((item, index) => ({ item, index }))
    .filter(({ item }) => selection.includes(item.tooth))
    .sort((a, b) => triadanOrder(a.item.tooth, b.item.tooth) || a.index - b.index);

  const gradeOptions = (f: FindingDef): { value: string; label: string }[] => {
    if (!f.grade) return [];
    const out = f.grade.required ? [] : [{ value: "", label: t("Not said") }];
    for (let g = f.grade.min; g <= f.grade.max; g++) {
      out.push({ value: String(g), label: String(g) });
      for (const d of f.details.filter((x) => x.with_grade === g)) out.push({ value: `${g}${d.code}`, label: `${g}${d.code}` });
    }
    return out;
  };

  const plainDetails = def?.details.filter((d) => d.with_grade == null) ?? [];
  const set = (patch: Partial<FormValues>) => setValues((v) => ({ ...v, ...patch }));

  return (
    <form
      className="edit-heading dce"
      aria-label={t("Edit {heading}", { heading: title })}
      onSubmit={(e) => {
        e.preventDefault();
        if (!needsChoice && notInCat.length === 0) onSave(items, dentition == null ? choice : null);
      }}
      onKeyDown={onKeyDown}
    >
      <div className="dce-top">
        {dentition == null && (
          <div className="ob-field">
            <span className="label" id="dce-animal">
              {t("Dog or cat?")}
            </span>
            <div className="ob-segmented" role="radiogroup" aria-labelledby="dce-animal">
              {(["canine", "feline"] as const).map((d) => (
                <button
                  key={d}
                  type="button"
                  role="radio"
                  className="ob-segment"
                  aria-checked={choice === d}
                  disabled={busy}
                  onClick={() => setChoice(d)}
                >
                  {d === "canine" ? t("Dog") : t("Cat")}
                </button>
              ))}
            </div>
          </div>
        )}
        <label className="dce-check">
          <input type="checkbox" checked={deciduous} disabled={busy} onChange={(e) => setDeciduous(e.target.checked)} />
          {t("Deciduous teeth")}
        </label>
        <Button variant="ghost" size="compact" aria-pressed={several} disabled={busy} onClick={() => setSeveral((s) => !s)}>
          {t("Select several")}
        </Button>
      </div>

      <ToothDiagram
        dentition={drawn}
        items={items}
        showDeciduous={deciduous}
        selected={selection}
        onToothActivate={busy ? undefined : activate}
        onEvidence={onEvidence}
        toggles
      />

      <section className="dce-panel" aria-label={t("Selected teeth")}>
        <p className="dce-selection" aria-live="polite">
          {selectionText}
        </p>
        {rows.length > 0 && (
          <ul className="dce-rows">
            {rows.map(({ item, index }) => {
              const words = findingWords(t, item, lowerInner);
              const ev = Array.isArray(item.evidence) ? item.evidence : [];
              const source = ev.map((id) => lineText?.(id)).filter((s): s is string => !!s);
              return (
                <li key={index} className="dce-row">
                  <span className="dce-row-text">
                    <span className="dce-row-tooth">{item.tooth}</span>
                    {findingText(t, item, lowerInner)}
                    {source.length > 0 && <span className="dce-row-source">{`“${source.join(" ")}”`}</span>}
                  </span>
                  {ev.length > 0 && onPlay && (
                    <button type="button" className="icon-btn" aria-label={t("Play the supporting audio")} onClick={() => onPlay(ev)}>
                      <Play size={16} aria-hidden />
                    </button>
                  )}
                  <Button
                    variant="ghost"
                    size="compact"
                    disabled={busy}
                    aria-label={t("Change {finding} on {tooth}", { finding: lowerInner ? lowerFirst(words) : words, tooth: item.tooth })}
                    onClick={() => openChange(index)}
                  >
                    {t("Change")}
                  </Button>
                  <button
                    type="button"
                    className="icon-btn"
                    disabled={busy}
                    aria-label={t("Remove {finding} from {tooth}", { finding: lowerInner ? lowerFirst(words) : words, tooth: item.tooth })}
                    onClick={() => {
                      setItems((xs) => xs.filter((_, i) => i !== index));
                      setForm(null); // indexes shift; an open form would point at another finding
                    }}
                  >
                    <X size={16} aria-hidden />
                  </button>
                </li>
              );
            })}
          </ul>
        )}
        <div className="dce-actions">
          <Button
            disabled={busy || selection.length === 0 || anyCharted}
            aria-describedby={anyCharted ? "dce-mark-hint" : undefined}
            onClick={() => mark("missing")}
          >
            {t("Mark missing")}
          </Button>
          <Button
            disabled={busy || selection.length === 0 || anyCharted}
            aria-describedby={anyCharted ? "dce-mark-hint" : undefined}
            onClick={() => mark("normal")}
          >
            {t("Mark normal")}
          </Button>
          <Button variant="ghost" disabled={busy || selection.length === 0} onClick={openAdd} aria-expanded={form === -1}>
            <Plus size={16} aria-hidden />
            {t("Add finding")}
          </Button>
        </div>
        {anyCharted && (
          <p className="small faint" id="dce-mark-hint">
            {`${t("Mark missing")}, ${t("Mark normal")}: ${lowerFirst(t("Only for teeth with no findings yet."))}`}
          </p>
        )}

        {form != null && (
          <fieldset
            className="dce-form"
            onKeyDown={(e) => {
              if (e.key === "Enter" && (e.target as HTMLElement).tagName === "INPUT") {
                e.preventDefault();
                applyForm();
              }
            }}
          >
            <div className="dce-fields">
              <label className="ob-field">
                <span className="label">{t("What was found")}</span>
                <select
                  className="ob-select"
                  value={values.finding}
                  disabled={busy}
                  autoFocus
                  onChange={(e) => setValues(valuesFor(e.target.value))}
                >
                  <option value="" disabled>
                    {t("Choose a finding")}
                  </option>
                  {GROUPS.map(([group, label]) => (
                    <optgroup key={group} label={t(label)}>
                      {CHART.findings
                        .filter((f) => f.group === group)
                        .map((f) => (
                          <option key={f.id} value={f.id}>
                            {t(f.label)}
                          </option>
                        ))}
                    </optgroup>
                  ))}
                </select>
              </label>
              {def && plainDetails.length > 0 && (
                <label className="ob-field">
                  <span className="label">{t(DETAIL_LABEL[def.id] ?? "Type")}</span>
                  <select className="ob-select" value={values.detail} disabled={busy} onChange={(e) => set({ detail: e.target.value })}>
                    <option value="">{t("Not said")}</option>
                    {plainDetails.map((d) => (
                      <option key={d.id} value={d.id}>
                        {t(d.words)}
                      </option>
                    ))}
                  </select>
                </label>
              )}
              {def?.grade && (
                <label className="ob-field">
                  <span className="label">{t(GRADE_LABEL[def.id] ?? "Grade")}</span>
                  <select className="ob-select" value={values.grade} disabled={busy} onChange={(e) => set({ grade: e.target.value })}>
                    {gradeOptions(def).map((o) => (
                      <option key={o.value} value={o.value}>
                        {o.label}
                      </option>
                    ))}
                  </select>
                </label>
              )}
              {def && def.mm !== "none" && (
                <label className="ob-field">
                  <span className="label">{t("Millimetres")}</span>
                  <input
                    className="ob-input"
                    type="number"
                    inputMode="decimal"
                    min={0}
                    step={0.5}
                    value={values.mm}
                    disabled={busy}
                    placeholder={def.mm === "optional" ? t("Not said") : undefined}
                    onChange={(e) => set({ mm: e.target.value })}
                  />
                </label>
              )}
              {def?.text_required && (
                <label className="ob-field">
                  <span className="label">{t("Describe the finding")}</span>
                  <input className="ob-input" value={values.text} disabled={busy} onChange={(e) => set({ text: e.target.value })} />
                </label>
              )}
            </div>
            {def?.plannable && (
              <label className="dce-check">
                <input type="checkbox" checked={values.planned} disabled={busy} onChange={(e) => set({ planned: e.target.checked })} />
                {t("Planned, not done yet")}
              </label>
            )}
            <p id="dce-form-problem" className="small dce-warning" aria-live="polite">
              {problem}
            </p>
            <div className="dce-actions">
              <Button
                size="compact"
                disabled={busy || !def || !!problem}
                aria-describedby={problem ? "dce-form-problem" : undefined}
                onClick={applyForm}
              >
                {form === -1 ? t("Add") : t("Change")}
              </Button>
              <Button variant="ghost" size="compact" disabled={busy} onClick={() => setForm(null)}>
                {t("Close")}
              </Button>
            </div>
          </fieldset>
        )}
      </section>

      {notInCat.length > 0 && (
        <p className="small dce-warning" role="alert">
          {t("{teeth} aren't in a cat's mouth. Remove them or choose Dog.", { teeth: notInCat.join(", ") })}
        </p>
      )}
      {needsChoice && <p className="small faint">{t("Choose dog or cat before saving.")}</p>}
      <p className="small faint">
        {t(
          "Saved as a new draft. Lines you change or add are marked as yours and keep their link to the recording if they had one; the earlier draft is kept.",
        )}
      </p>
      <div className="ask-actions">
        <Button type="submit" variant="primary" disabled={busy || needsChoice || notInCat.length > 0}>
          {busy ? t("Saving") : t("Save changes")}
        </Button>
        <Button variant="ghost" onClick={onCancel} disabled={busy}>
          {t("Cancel")}
        </Button>
      </div>
    </form>
  );
}
