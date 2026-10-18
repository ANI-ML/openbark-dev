import { Button } from "@openbark/ui";
import { CaretDown, CaretUp, Plus, X } from "@phosphor-icons/react";
import { useState } from "react";
import type { ContentKind, TemplateField } from "../engine";

type Item = Record<string, unknown> & { evidence?: number[]; entered_by?: string };

export interface HeadingEditorProps {
  title: string;
  kind: ContentKind;
  fields?: TemplateField[];
  /** The heading's current content from the draft. */
  value: unknown;
  busy: boolean;
  onSave: (value: unknown) => void;
  onCancel: () => void;
}

const MED_KEYS = ["drug", "dose", "route", "frequency", "duration"] as const;

/**
 * Edit one heading by hand. Lines keep their citations; anything changed or
 * added is saved as the veterinarian's own statement, in a new draft.
 */
export function HeadingEditor({ title, kind, fields, value, busy, onSave, onCancel }: HeadingEditorProps) {
  const [items, setItems] = useState<Item[]>(() => (Array.isArray(value) ? (structuredClone(value) as Item[]) : []));
  const [fieldValues, setFieldValues] = useState<Record<string, Item | null>>(() =>
    value && typeof value === "object" && !Array.isArray(value) ? (structuredClone(value) as Record<string, Item | null>) : {},
  );

  const setItem = (i: number, patch: Item) => setItems((xs) => xs.map((x, j) => (j === i ? { ...x, ...patch } : x)));
  const moveItem = (i: number, to: number) =>
    setItems((xs) => {
      if (to < 0 || to >= xs.length) return xs;
      const next = xs.slice();
      next.splice(to, 0, ...next.splice(i, 1));
      return next;
    });

  function result(): unknown {
    if (kind === "fields") return fieldValues;
    if (kind === "statements")
      return items.filter((s) => String(s.text ?? "").trim()).map((s) => ({ ...s, text: String(s.text).trim() }));
    return items
      .filter((m) => String(m.drug ?? "").trim())
      .map((m) => {
        const out: Item = { ...m, drug: String(m.drug).trim() };
        for (const k of MED_KEYS.slice(1)) {
          const v = typeof m[k] === "string" ? (m[k] as string).trim() : "";
          out[k] = v ? v : null;
        }
        return out;
      });
  }

  const tools = (i: number, label: string) => (
    <span className="tb-tools">
      <button type="button" className="icon-btn" aria-label={`Move ${label} up`} disabled={i === 0} onClick={() => moveItem(i, i - 1)}>
        <CaretUp size={16} aria-hidden />
      </button>
      <button
        type="button"
        className="icon-btn"
        aria-label={`Move ${label} down`}
        disabled={i === items.length - 1}
        onClick={() => moveItem(i, i + 1)}
      >
        <CaretDown size={16} aria-hidden />
      </button>
      <button type="button" className="icon-btn" aria-label={`Remove ${label}`} onClick={() => setItems((xs) => xs.filter((_, j) => j !== i))}>
        <X size={16} aria-hidden />
      </button>
    </span>
  );

  return (
    <form
      className="edit-heading"
      aria-label={`Edit ${title}`}
      onSubmit={(e) => {
        e.preventDefault();
        onSave(result());
      }}
      onKeyDown={(e) => e.key === "Escape" && !busy && onCancel()}
    >
      {kind === "statements" && (
        <ol className="edit-lines">
          {items.map((s, i) => (
            <li key={i}>
              <textarea
                className="ob-input tb-text"
                rows={Math.max(1, Math.ceil(String(s.text ?? "").length / 70))}
                aria-label={`Line ${i + 1}`}
                value={String(s.text ?? "")}
                disabled={busy}
                autoFocus={i === 0}
                onChange={(e) => setItem(i, { text: e.target.value })}
              />
              {tools(i, `line ${i + 1}`)}
            </li>
          ))}
        </ol>
      )}

      {kind === "medications" && (
        <ol className="edit-lines">
          {items.map((m, i) => (
            <li key={i} className="edit-med">
              <div className="edit-med-grid">
                {MED_KEYS.map((k) => (
                  <label key={k} className="ob-field">
                    <span className="label muted">{k.charAt(0).toUpperCase() + k.slice(1)}</span>
                    <input
                      className="ob-input"
                      value={typeof m[k] === "string" ? (m[k] as string) : ""}
                      disabled={busy}
                      onChange={(e) => setItem(i, { [k]: e.target.value })}
                    />
                  </label>
                ))}
              </div>
              {tools(i, String(m.drug || `medication ${i + 1}`))}
            </li>
          ))}
        </ol>
      )}

      {kind === "fields" && (
        <div className="edit-fields">
          {(fields ?? []).map((f) => {
            const cur = fieldValues[f.key];
            const val = cur?.value == null ? "" : String(cur.value);
            const unit = typeof cur?.unit === "string" ? cur.unit : "";
            const set = (v: string, u: string) =>
              setFieldValues((fv) => ({
                ...fv,
                [f.key]: v.trim() ? { ...(cur ?? { evidence: [] }), value: v, unit: u.trim() || null } : null,
              }));
            return (
              <div key={f.key} className="edit-field">
                <span className="label muted">{f.label}</span>
                <input className="ob-input" aria-label={f.label} value={val} disabled={busy} placeholder="Not discussed" onChange={(e) => set(e.target.value, unit)} />
                <input
                  className="ob-input edit-unit"
                  aria-label={`${f.label} unit`}
                  value={unit}
                  disabled={busy || !val}
                  placeholder="unit"
                  onChange={(e) => set(val, e.target.value)}
                />
              </div>
            );
          })}
        </div>
      )}

      {kind !== "fields" && (
        <Button
          variant="ghost"
          disabled={busy}
          onClick={() =>
            setItems((xs) => [
              ...xs,
              kind === "statements"
                ? { text: "", evidence: [] }
                : { drug: "", dose: null, route: null, frequency: null, duration: null, evidence: [] },
            ])
          }
        >
          <Plus size={16} aria-hidden />
          {kind === "statements" ? "Add line" : "Add medication"}
        </Button>
      )}
      <p className="small faint">
        Saved as a new draft. Lines you change or add are marked as yours and keep their link to the recording if they
        had one; the earlier draft is kept.
      </p>
      <div className="ask-actions">
        <Button type="submit" variant="primary" disabled={busy}>
          {busy ? "Saving" : "Save changes"}
        </Button>
        <Button variant="ghost" onClick={onCancel} disabled={busy}>
          Cancel
        </Button>
      </div>
    </form>
  );
}
