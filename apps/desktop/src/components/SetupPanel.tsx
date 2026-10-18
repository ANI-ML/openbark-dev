import { Button, StatusDot } from "@openbark/ui";
import { Download } from "lucide-react";
import { useContext, useEffect, useMemo, useState } from "react";
import { AppContext, fmtBytes } from "../context";
import * as engine from "../engine";
import { useT } from "../i18n";

export interface SetupPanelProps {
  /** Required models (from status) that aren't on this computer yet. */
  missing: { id: string; name: string }[];
  onReady: () => void;
}

/**
 * First run (M1 item 8): download the default models from the app, smallest
 * first, with one determinate progress rule for the whole set.
 */
export function SetupPanel({ missing, onReady }: SetupPanelProps) {
  const t = useT();
  // Optional: the panel also renders outside the app shell (tests).
  const app = useContext(AppContext);
  const [rows, setRows] = useState<engine.ModelRow[]>([]);
  const [current, setCurrent] = useState<string | null>(null);
  const [done, setDone] = useState<Record<string, number>>({});
  const [finished, setFinished] = useState<Set<string>>(new Set());
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    engine.models().then(setRows, () => {});
    const off = engine.on("download", (d) => setDone((p) => ({ ...p, [d.model]: d.done })));
    return () => void off.then((f) => f());
  }, []);

  const queue = useMemo(() => {
    const size = (id: string) => rows.find((r) => r.id === id)?.size_bytes ?? 0;
    return [...missing].sort((a, b) => size(a.id) - size(b.id)).map((m) => ({ ...m, size: size(m.id) }));
  }, [missing, rows]);
  const total = queue.reduce((n, m) => n + m.size, 0);
  const got = queue.reduce((n, m) => n + (finished.has(m.id) ? m.size : Math.min(m.size, done[m.id] ?? 0)), 0);
  const llm = rows.find((r) => r.slot === "llm" && missing.some((m) => m.id === r.id));
  const fits = llm?.fit;

  async function downloadAll() {
    setError(null);
    for (const m of queue) {
      if (finished.has(m.id)) continue;
      setCurrent(m.id);
      try {
        await engine.downloadModel(m.id);
        setFinished((s) => new Set(s).add(m.id));
      } catch (e) {
        setError(
          t("{model} didn't finish downloading: {error} Try again to resume where it stopped.", {
            model: m.name,
            error: e instanceof Error ? e.message : String(e),
          }),
        );
        setCurrent(null);
        return;
      }
    }
    setCurrent(null);
    onReady();
  }

  return (
    <section className="setup" aria-label={t("Set up OpenBark")}>
      <h2>{t("Download the models to start")}</h2>
      {llm && (
        <p className="measure">
          {t("No model loaded. Download {model} ({size}) to start, or load one from Hugging Face.", {
            model: llm.name,
            size: fmtBytes(llm.size_bytes),
          })}
        </p>
      )}
      <p className="small muted measure">
        {t("OpenBark transcribes and writes notes on this computer, so it needs its models once:")}{" "}
        <span className="mono">{fmtBytes(total)}</span>{" "}
        {t("in all. After that it works offline. The computer stays awake while they download.")}
      </p>
      <ul className="setup-list">
        {queue.map((m) => (
          <li key={m.id}>
            <span>{m.name}</span>
            <span className="mono faint">{fmtBytes(m.size)}</span>
            {finished.has(m.id) ? (
              <StatusDot tone="ok">{t("Ready")}</StatusDot>
            ) : current === m.id ? (
              <StatusDot tone="warn">{t("Downloading")}</StatusDot>
            ) : (
              <StatusDot>{t("Waiting")}</StatusDot>
            )}
          </li>
        ))}
      </ul>
      {fits === "wont_fit" && (
        <p className="ob-notice">
          {t("The note model may not fit this computer's memory. You can still download it, or choose a smaller one in Models.")}
        </p>
      )}
      {current && (
        <div className="setup-progress">
          <div className="ob-progress" role="progressbar" aria-label={t("Downloading models")} aria-valuenow={Math.round((got / Math.max(1, total)) * 100)}>
            <span style={{ transform: `scaleX(${got / Math.max(1, total)})` }} />
          </div>
          <span className="mono faint small">{t("{size} left", { size: fmtBytes(Math.max(0, total - got)) })}</span>
        </div>
      )}
      {error && (
        <p className="ob-notice" data-tone="danger" role="alert">
          {error}
        </p>
      )}
      <div className="ask-actions">
        <Button variant="primary" onClick={downloadAll} disabled={current != null || queue.length === 0}>
          <Download size={20} aria-hidden />
          {error ? t("Try again") : t("Download everything ({size})", { size: fmtBytes(total) })}
        </Button>
        {app && llm && (
          <Button variant="ghost" onClick={() => app.navigate({ screen: "models" })} disabled={current != null}>
            {t("Browse Hugging Face")}
          </Button>
        )}
      </div>
    </section>
  );
}
