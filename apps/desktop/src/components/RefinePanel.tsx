import { Button } from "@openbark/ui";
import { type ReactNode, useEffect, useRef, useState } from "react";
import type { Approver, Scope, Standard, User } from "../engine";
import * as engine from "../engine";
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
  /**
   * In a team (ADR-0024): who a saved standard is for. `personal` false when
   * templates are shared by the hospital; a standard for everyone needs an
   * admin's PIN. Absent working alone, where every standard is everyone's.
   */
  scopes?: { personal: boolean; teamNeedsPin: boolean; me?: User | null };
  /** "Edit this heading in the template": opens the editor at this heading. */
  onEditHeading?: () => void;
  onSubmit: (request: string, saveAsStandard: boolean, choice?: { scope: Scope; approver: Approver | null }) => void;
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
  scopes,
  onEditHeading,
  onSubmit,
  onCancel,
}: RefinePanelProps) {
  const t = useT();
  const [request, setRequest] = useState("");
  const [save, setSave] = useState(false);
  const [scope, setScope] = useState<Scope>(scopes?.personal === false ? "team" : "personal");
  const [admins, setAdmins] = useState<User[]>([]);
  const [admin, setAdmin] = useState(scopes?.me?.admin ? scopes.me.id : "");
  const [pin, setPin] = useState("");
  const needsPin = Boolean(scopes && save && scope === "team" && scopes.teamNeedsPin);
  useEffect(() => {
    if (!needsPin || scopes?.me?.admin) return;
    engine.listUsers().then((u) => setAdmins(u.filter((x) => x.admin)), () => {});
  }, [needsPin, scopes?.me?.admin]);
  const submit = (text: string, keep: boolean) => {
    if (!scopes) return onSubmit(text, keep);
    onSubmit(text, keep, { scope, approver: keep && needsPin ? { id: admin, pin } : null });
  };
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
        if (request.trim()) submit(request.trim(), save && !alreadyStandard);
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
            submit(request.trim(), save && !alreadyStandard);
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
              : scopes
                ? t("Save as a standard: later {template} drafts follow it under this heading.", { template: templateName })
                : t("Save as a standard: every {template} draft follows it under this heading.", {
                    template: templateName,
                  })}
          </span>
        </label>
      )}
      {canSave && scopes && save && !alreadyStandard && (
        <fieldset className="ask-scope">
          <legend className="sr-only">{t("Who the standard is for")}</legend>
          {scopes.personal && (
            <label className="check small">
              <input type="radio" name="standard-scope" checked={scope === "personal"} onChange={() => setScope("personal")} />
              <span>{t("For me")}</span>
            </label>
          )}
          <label className="check small">
            <input type="radio" name="standard-scope" checked={scope === "team"} onChange={() => setScope("team")} />
            <span>{t("For the hospital")}</span>
          </label>
          {needsPin && (
            <div className="ask-approve">
              <span className="small muted">{t("An admin approves standards for the hospital.")}</span>
              {!scopes.me?.admin && (
                <select className="ob-select" aria-label={t("Admin approving")} value={admin} onChange={(e) => setAdmin(e.target.value)}>
                  <option value="">{t("Choose an admin")}</option>
                  {admins.map((a) => (
                    <option key={a.id} value={a.id}>
                      {a.name}
                    </option>
                  ))}
                </select>
              )}
              <input
                className="ob-input mono"
                type="password"
                inputMode="numeric"
                autoComplete="off"
                maxLength={8}
                aria-label={t("Admin's PIN")}
                placeholder={t("Admin's PIN")}
                value={pin}
                onChange={(e) => setPin(e.target.value.replace(/\D/g, ""))}
              />
            </div>
          )}
        </fieldset>
      )}
      <p className="small faint">
        {t(
          "The request steers the model; it isn't evidence. Anything not said in the recording stays out, and numbers are checked again.",
        )}
      </p>
      {onEditHeading && (
        <button type="button" className="link small ask-edit-heading" onClick={onEditHeading}>
          {t("Edit this heading in the template")}
        </button>
      )}
      {busy && board}
      <div className="ask-actions">
        <Button type="submit" variant="primary" disabled={busy || !request.trim() || (needsPin && (!admin || pin.length < 4))}>
          {busy ? t("Redrafting") : t("Redraft {heading}", { heading: title.split(" > ").pop() ?? "" })}
        </Button>
        <Button variant="ghost" onClick={onCancel} disabled={busy}>
          {t("Cancel")}
        </Button>
      </div>
    </form>
  );
}
