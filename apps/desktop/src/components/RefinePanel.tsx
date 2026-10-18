import { Button } from "@openbark/ui";
import { type ReactNode, useEffect, useRef, useState } from "react";
import type { Standard } from "../engine";
import { useT } from "../i18n";

export interface RefinePanelProps {
  /** "Plan > Medications". */
  title: string;
  templateName: string;
  /** Example requests for this heading and kind of visit (e.g. Triadan numbers under dental findings). */
  examples?: string[];
  /** Standards already saved for this heading (shown as reusable requests). */
  standards: Standard[];
  /** False when the template can't take standards (deleted since drafting). */
  canSave: boolean;
  busy: boolean;
  /** The drafting board shown while the heading is redrafted. */
  board: ReactNode;
  onSubmit: (request: string, saveAsStandard: boolean) => void;
  onCancel: () => void;
}

/** Suggestions that suit any heading; teams' own requests become standards. Translated where shown. */
const SUGGESTIONS = [
  "Add any details that were said but left out",
  "Give every value with its unit",
  "One finding per line",
];

/**
 * Re-prompt one heading: say what should change, optionally keep the
 * request as a standard for this heading in the template. The redrafting
 * board inside carries the screen's one accent, so saved standards are
 * marked with a neutral dot (docs/brand/DESIGN.md §3.5).
 */
export function RefinePanel({
  title,
  templateName,
  examples = [],
  standards,
  canSave,
  busy,
  board,
  onSubmit,
  onCancel,
}: RefinePanelProps) {
  const t = useT();
  const [request, setRequest] = useState("");
  const [save, setSave] = useState(false);
  const box = useRef<HTMLTextAreaElement>(null);
  const id = `ask-${title.replace(/\W+/g, "-").toLowerCase()}`;

  useEffect(() => {
    box.current?.focus();
  }, []);

  const saved = new Set(standards.map((s) => s.text.toLowerCase()));
  const general = SUGGESTIONS.map((s) => t(s)).filter((s) => !saved.has(s.toLowerCase()));
  const own = examples.filter((e) => !saved.has(e.toLowerCase()));
  const chips = [...standards.map((s) => s.text), ...(own.length ? [] : general)];
  const alreadyStandard = saved.has(request.trim().toLowerCase());

  return (
    <form
      className="ask"
      aria-label={t("Ask for changes to {heading}", { heading: title })}
      onSubmit={(e) => {
        e.preventDefault();
        if (request.trim()) onSubmit(request.trim(), save && !alreadyStandard);
      }}
      onKeyDown={(e) => e.key === "Escape" && !busy && onCancel()}
    >
      <label className="label" htmlFor={id}>
        {t("What should change under {title}?", { title })}
      </label>
      <textarea
        id={id}
        ref={box}
        className="ob-input ask-text"
        rows={2}
        value={request}
        disabled={busy}
        placeholder={
          own[0]
            ? t("For example: {example}", { example: own[0] })
            : t("For example: list every fluid with its volume and rate")
        }
        onChange={(e) => setRequest(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && (e.metaKey || e.ctrlKey) && request.trim()) {
            e.preventDefault();
            onSubmit(request.trim(), save && !alreadyStandard);
          }
        }}
      />
      {own.length > 0 && (
        <div className="ask-examples">
          <span className="small muted">{t("Examples for {template}", { template: templateName })}</span>
          <div className="ask-chips">
            {own.map((c) => (
              <button
                key={c}
                type="button"
                className="ob-chip ask-chip"
                disabled={busy}
                onClick={() => {
                  setRequest(c);
                  box.current?.focus();
                }}
              >
                {c}
              </button>
            ))}
          </div>
        </div>
      )}
      <div className="ask-chips" aria-label={t("Reuse a request")}>
        {chips.map((c) => (
          <button
            key={c}
            type="button"
            className="ob-chip ask-chip"
            disabled={busy}
            onClick={() => {
              setRequest(c);
              box.current?.focus();
            }}
          >
            {saved.has(c.toLowerCase()) && <span className="ob-dot" aria-label={t("Standard")} />}
            {c}
          </button>
        ))}
      </div>
      {canSave && (
        <label className="check small">
          <input
            type="checkbox"
            checked={save || alreadyStandard}
            disabled={busy || alreadyStandard}
            onChange={(e) => setSave(e.target.checked)}
          />
          <span>
            {alreadyStandard
              ? t("Already a standard in {template}.", { template: templateName })
              : t("Save as a standard: every {template} draft follows it under this heading.", {
                  template: templateName,
                })}
          </span>
        </label>
      )}
      <p className="small faint">
        {t(
          "The request steers the model; it isn't evidence. Anything not said in the recording stays out, and numbers are checked again.",
        )}
      </p>
      {busy && board}
      <div className="ask-actions">
        <Button type="submit" variant="primary" disabled={busy || !request.trim()}>
          {busy ? t("Redrafting") : t("Redraft {heading}", { heading: title.split(" > ").pop() ?? "" })}
        </Button>
        <Button variant="ghost" onClick={onCancel} disabled={busy}>
          {t("Cancel")}
        </Button>
      </div>
    </form>
  );
}
