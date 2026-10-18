import { Button } from "@openbark/ui";
import { useEffect, useRef, useState } from "react";
import type { Standard } from "../engine";

export interface RefinePanelProps {
  /** "Plan > Medications". */
  title: string;
  templateName: string;
  /** Standards already saved for this heading (shown as reusable requests). */
  standards: Standard[];
  /** False when the template can't take standards (deleted since drafting). */
  canSave: boolean;
  busy: boolean;
  progress: number;
  onSubmit: (request: string, saveAsStandard: boolean) => void;
  onCancel: () => void;
}

/** Suggestions that suit any heading; teams' own requests become standards. */
const SUGGESTIONS = [
  "Add any details that were said but left out",
  "Give every value with its unit",
  "One finding per line",
];

/**
 * Re-prompt one heading: say what should change, optionally keep the
 * request as a standard for this heading in the template.
 */
export function RefinePanel({
  title,
  templateName,
  standards,
  canSave,
  busy,
  progress,
  onSubmit,
  onCancel,
}: RefinePanelProps) {
  const [request, setRequest] = useState("");
  const [save, setSave] = useState(false);
  const box = useRef<HTMLTextAreaElement>(null);
  const id = `ask-${title.replace(/\W+/g, "-").toLowerCase()}`;

  useEffect(() => {
    box.current?.focus();
  }, []);

  const saved = new Set(standards.map((s) => s.text.toLowerCase()));
  const chips = [...standards.map((s) => s.text), ...SUGGESTIONS.filter((s) => !saved.has(s.toLowerCase()))];
  const alreadyStandard = saved.has(request.trim().toLowerCase());

  return (
    <form
      className="ask"
      aria-label={`Ask for changes to ${title}`}
      onSubmit={(e) => {
        e.preventDefault();
        if (request.trim()) onSubmit(request.trim(), save && !alreadyStandard);
      }}
      onKeyDown={(e) => e.key === "Escape" && !busy && onCancel()}
    >
      <label className="label" htmlFor={id}>
        What should change under {title}?
      </label>
      <textarea
        id={id}
        ref={box}
        className="ob-input ask-text"
        rows={2}
        value={request}
        disabled={busy}
        placeholder="For example: list every fluid with its volume and rate"
        onChange={(e) => setRequest(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && (e.metaKey || e.ctrlKey) && request.trim()) {
            e.preventDefault();
            onSubmit(request.trim(), save && !alreadyStandard);
          }
        }}
      />
      <div className="ask-chips" aria-label="Reuse a request">
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
            {saved.has(c.toLowerCase()) && <span className="ob-dot" data-tone="accent" aria-label="Standard" />}
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
              ? `Already a standard in ${templateName}.`
              : `Save as a standard: every ${templateName} draft follows it under this heading.`}
          </span>
        </label>
      )}
      <p className="small faint">
        The request steers the model; it isn't evidence. Anything not said in the recording stays out, and numbers are
        checked again.
      </p>
      {busy && (
        <div className="ob-progress" role="progressbar" aria-label={`Redrafting ${title}`}>
          <span style={{ transform: `scaleX(${progress})` }} />
        </div>
      )}
      <div className="ask-actions">
        <Button type="submit" variant="primary" disabled={busy || !request.trim()}>
          {busy ? "Redrafting" : `Redraft ${title.split(" > ").pop()}`}
        </Button>
        <Button variant="ghost" onClick={onCancel} disabled={busy}>
          Cancel
        </Button>
      </div>
    </form>
  );
}
