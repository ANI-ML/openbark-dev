import { Button } from "@openbark/ui";
import { ClipboardCopy } from "lucide-react";
import { useState } from "react";
import { useApp } from "../context";
import * as engine from "../engine";
import { useT } from "../i18n";

/**
 * Copy diagnostics (Settings, About; 0.2.3): the app version, the computer,
 * the note model, the last draft's timings and the end of the log, for the
 * clinic to paste into an email when drafting is slow or stops. Nothing from
 * any visit is in it: the log never holds names, transcripts or notes.
 */
export function CopyDiagnostics() {
  const app = useApp();
  const t = useT();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function copy() {
    setBusy(true);
    setError(null);
    try {
      const { text } = await engine.diagnostics(app.settings.llm || null);
      await navigator.clipboard.writeText(text);
      app.toast(t("Diagnostics copied"));
    } catch {
      setError(t("The clipboard isn't available here. Select the text and copy it instead."));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="copy-diagnostics">
      <p className="small muted">
        {t(
          "If drafting is slow or stops, copy the diagnostics and send them to us: this computer's details, the note model, how long the last draft took, and the end of the log. Nothing from any visit is included.",
        )}
      </p>
      <Button size="compact" onClick={() => void copy()} disabled={busy}>
        <ClipboardCopy size={16} aria-hidden />
        {t("Copy diagnostics")}
      </Button>
      {error && (
        <p className="ob-notice" data-tone="danger" role="alert">
          {error}
        </p>
      )}
    </div>
  );
}
