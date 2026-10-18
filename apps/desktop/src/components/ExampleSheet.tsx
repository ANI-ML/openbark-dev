import { Button } from "@openbark/ui";
import { X } from "@phosphor-icons/react";
import { useEffect, useRef, useState } from "react";
import { useApp } from "../context";
import * as engine from "../engine";
import type { Template } from "../engine";
import { useT } from "../i18n";

/**
 * M2 item 5: paste a note the clinic already writes; the model proposes its
 * headings, and the proposal opens in the builder to edit before saving.
 * The example isn't saved, and patient details aren't copied into the template.
 */
export function ExampleSheet({ onClose, onProposed }: { onClose: () => void; onProposed: (t: Template) => void }) {
  const app = useApp();
  const t = useT();
  const [text, setText] = useState("");
  const [busy, setBusy] = useState(false);
  const [progress, setProgress] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const box = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    box.current?.focus();
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && !busy && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose, busy]);

  // No token stream for this task: an estimate that slows as it nears the end.
  useEffect(() => {
    if (!busy) return;
    const start = Date.now();
    const timer = setInterval(() => setProgress(1 - Math.exp(-(Date.now() - start) / 15000)), 250);
    return () => clearInterval(timer);
  }, [busy]);

  async function propose() {
    setError(null);
    setBusy(true);
    setProgress(0);
    try {
      const view = await engine.templateFromExample(app.settings.llm, text);
      onProposed(view.template);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <>
      <div className="ob-backdrop" onClick={() => !busy && onClose()} />
      <aside className="ob-sheet" role="dialog" aria-modal="true" aria-label={t("Template from an example note")}>
        <header className="sheet-head">
          <h2>{t("Template from an example note")}</h2>
          <button type="button" className="icon-btn" aria-label={t("Close")} disabled={busy} onClick={onClose}>
            <X size={20} aria-hidden />
          </button>
        </header>
        <div className="sheet-body example-sheet">
          <p className="small muted">
            {t(
              "Paste a note your clinic already writes. OpenBark proposes its headings, what goes under each, and short instructions, then opens them in the builder for you to check and save. The example stays on this computer and isn't saved, and patient details aren't copied into the template.",
            )}
          </p>
          <label className="ob-field">
            <span className="label">{t("Example note")}</span>
            <textarea
              ref={box}
              className="ob-input example-text"
              rows={14}
              value={text}
              disabled={busy}
              placeholder={t("SUBJECTIVE: ... OBJECTIVE: ... ASSESSMENT: ... PLAN: ...")}
              onChange={(e) => setText(e.target.value)}
            />
          </label>
          {error && (
            <p className="ob-notice" data-tone="danger" role="alert">
              {error}
            </p>
          )}
          {busy && (
            <div className="ob-progress" role="progressbar" aria-label={t("Reading the example")}>
              <span style={{ transform: `scaleX(${progress})` }} />
            </div>
          )}
          <div className="ask-actions">
            <Button variant="primary" onClick={() => void propose()} disabled={busy || text.trim().length < 20}>
              {busy ? t("Reading the example") : t("Propose headings")}
            </Button>
            <Button variant="ghost" onClick={onClose} disabled={busy}>
              {t("Cancel")}
            </Button>
          </div>
        </div>
      </aside>
    </>
  );
}
