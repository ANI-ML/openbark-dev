import { Button } from "@openbark/ui";
import { Download, PenLine } from "lucide-react";
import { useEffect, useState } from "react";
import { fmtBytes, useApp } from "../context";
import * as engine from "../engine";
import { useT } from "../i18n";

/**
 * "Write notes on this iPhone" (Settings on the iPhone, ADR-0018 section 10).
 * Drafting on the phone is opt-in: turning it on offers this tier's default
 * note model, with its size, and downloads it with the same progress the
 * Models screen shows. The other models that fit stay in Models.
 */
export function PhoneDrafting() {
  const app = useApp();
  const t = useT();
  const on = app.settings.writeNotesHere;
  const tier = app.status?.mobile_tier ?? null;
  const wanted = app.status?.default_llm ?? "";
  const [row, setRow] = useState<engine.ModelRow | null>(null);
  const [progress, setProgress] = useState<{ done: number; total: number } | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!on || !wanted) return;
    let live = true;
    engine.models().then(
      (rows) => live && setRow(rows.find((r) => r.id === wanted) ?? null),
      () => {},
    );
    return () => {
      live = false;
    };
  }, [on, wanted]);

  useEffect(() => {
    if (!on || !wanted) return;
    // Only while this screen's download runs (the last event can arrive after it returned).
    const off = engine.on("download", (d) => {
      if (d.model === wanted && d.total > 0) setProgress((p) => (p ? { done: d.done, total: d.total } : p));
    });
    return () => void off.then((f) => f());
  }, [on, wanted]);

  async function download() {
    if (!row) return;
    setError(null);
    setProgress({ done: 0, total: row.size_bytes });
    try {
      await engine.downloadModel(row.id);
      app.setSettings({ llm: row.id });
      app.toast(t("Model downloaded"));
      app.refreshStatus();
      setRow({ ...row, installed: true });
    } catch (e) {
      setError(engine.modelMessage(t, e));
    } finally {
      setProgress(null);
    }
  }

  const name = row?.name.replace(/\s*\([^)]*\)/g, "") ?? "";
  const size = row ? fmtBytes(row.size_bytes) : "";
  const offer = on && row && !row.installed;

  return (
    <li className="phone-drafting">
      <label className="inset-row settings-row">
        <span className="inset-icon" aria-hidden>
          <PenLine size={20} />
        </span>
        <span className="inset-text">{t("Write notes on this iPhone")}</span>
        <input
          type="checkbox"
          className="opt-switch"
          checked={on}
          onChange={(e) => app.setSettings({ writeNotesHere: e.target.checked })}
        />
      </label>
      {on && !tier && (
        <p className="small muted phone-drafting-body" role="status">
          {t("OpenBark needs an iPhone with 6 GB of memory or more to write notes on the phone.")}
        </p>
      )}
      {offer && (
        <div className="phone-drafting-body" data-testid="phone-drafting-offer">
          <p className="small">
            {t("Notes on this iPhone are written by {name}, a {size} download. Wi-Fi is recommended.", { name, size })}
          </p>
          {progress ? (
            <div className="dl">
              <div
                className="ob-progress"
                role="progressbar"
                aria-label={t("Downloading {name}", { name })}
                aria-valuenow={Math.round((progress.done / Math.max(1, progress.total)) * 100)}
              >
                <span style={{ transform: `scaleX(${progress.done / Math.max(1, progress.total)})` }} />
              </div>
              <span className="small mono faint">
                {t("{size} left", { size: fmtBytes(Math.max(0, progress.total - progress.done)) })}
              </span>
            </div>
          ) : (
            <div className="phone-drafting-actions">
              <Button onClick={() => void download()}>
                <Download size={20} aria-hidden />
                {t("Download ({size})", { size })}
              </Button>
              <Button variant="ghost" onClick={() => app.navigate({ screen: "models" })}>
                {t("Other models")}
              </Button>
            </div>
          )}
          {error && (
            <p className="small ob-notice" data-tone="danger" role="alert">
              {error}
            </p>
          )}
        </div>
      )}
    </li>
  );
}
