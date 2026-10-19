import { Button, StatusDot } from "@openbark/ui";
import { Download, Trash2 } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { fmtBytes, useApp } from "../context";
import * as engine from "../engine";
import { useT } from "../i18n";
import { RemoveModelDialog } from "./RemoveModelDialog";

/** The card's element id, so "Get the MLX engine" in the Hub sheet can bring it into view. */
export const MLX_CARD_ID = "mlx-engine";

/**
 * Models: the MLX engine (ADR-0025 §4), an opt-in download that runs MLX
 * models on a Mac with Apple silicon and macOS 14 or later. Shown on a Mac
 * (with what it needs when it can't run) and in the developer test mode.
 */
export function MlxCard({ onChange }: { onChange?: (s: engine.MlxStatus) => void }) {
  const app = useApp();
  const t = useT();
  const [status, setStatusRaw] = useState<engine.MlxStatus | null>(null);
  const [busy, setBusy] = useState<"install" | "check" | null>(null);
  const [progress, setProgress] = useState({ done: 0, total: 0 });
  const [error, setError] = useState<string | null>(null);
  const [removing, setRemoving] = useState(false);
  const autoChecked = useRef(false);

  const setStatus = (s: engine.MlxStatus) => {
    setStatusRaw(s);
    onChange?.(s);
  };

  useEffect(() => {
    engine.mlxStatus().then(setStatus, () => {});
    const off = engine.on(
      "download",
      (d) => d.model === engine.MLX_PACK_ID && setProgress({ done: d.done, total: d.total }),
    );
    return () => void off.then((f) => f());
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // While another window installs, keep the status fresh.
  useEffect(() => {
    if (!status?.installing || busy === "install") return;
    const timer = setInterval(() => engine.mlxStatus().then(setStatus, () => {}), 3000);
    return () => clearInterval(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [status?.installing, busy]);

  // Installed but not checked on this version yet: check once (local only, no network).
  useEffect(() => {
    if (!status || autoChecked.current || busy) return;
    if (status.pack_installed && status.check === "not_run" && !status.installing && runs(status.support)) {
      autoChecked.current = true;
      void check();
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [status, busy]);

  if (!status || status.support === "not_mac") return null;

  async function install() {
    setError(null);
    setBusy("install");
    setProgress({ done: 0, total: status?.download_bytes ?? 0 });
    try {
      setStatus(await engine.installMlx());
      app.announce(t("MLX engine downloaded"));
    } catch (e) {
      setError(engine.modelMessage(t, e));
    } finally {
      setBusy(null);
    }
  }

  async function check() {
    setError(null);
    setBusy("check");
    try {
      setStatus(await engine.checkMlx());
    } catch (e) {
      setError(engine.modelMessage(t, e));
    } finally {
      setBusy(null);
    }
  }

  const testMode = status.support === "test_mode";
  const installing = busy === "install" || status.installing;
  const downloaded = progress.total > 0 && progress.done >= progress.total;
  const pct = Math.round((progress.done / Math.max(1, progress.total)) * 100);
  const removeButton = (
    <Button variant="ghost" onClick={() => setRemoving(true)} disabled={busy !== null}>
      <Trash2 size={18} aria-hidden />
      {t("Remove")}
    </Button>
  );

  return (
    <section className="group mlx-card" id={MLX_CARD_ID} tabIndex={-1}>
      <div className="mlx-card-head">
        {/* §7.4: no icon beside the title. */}
        <h2>{t("MLX engine")}</h2>
        {testMode && <span className="ob-chip">{t("Test mode")}</span>}
      </div>
      <p className="small muted measure">
        {t("Runs MLX models from Hugging Face on this Mac's graphics. GGUF models keep using the built-in engine.")}
      </p>
      {testMode && <p className="small faint">{t("MLX on the processor, for testing only. It's slow.")}</p>}

      {status.support === "needs_apple_silicon" ? (
        <p className="ob-notice">
          {t("MLX needs a Mac with Apple silicon (M1 or later). GGUF models work on this Mac.")}
        </p>
      ) : status.support === "needs_macos14" ? (
        <p className="ob-notice">
          {t("MLX needs macOS 14 Sonoma or later. This Mac has macOS {version}. GGUF models work on this Mac.", {
            version: status.macos_version ?? "?",
          })}
        </p>
      ) : (
        <>
          {error && (
            <p className="ob-notice" data-tone="danger" role="alert">
              {error}
            </p>
          )}
          {installing ? (
            <div className="processing">
              <div
                className="ob-progress"
                role="progressbar"
                aria-label={t("Downloading the MLX engine")}
                aria-valuenow={downloaded ? 100 : pct}
              >
                <span
                  style={{ transform: `scaleX(${progress.total ? Math.min(1, progress.done / progress.total) : 0})` }}
                />
              </div>
              <div className="processing-text small">
                <span>
                  {downloaded
                    ? t("Setting up")
                    : t("Downloading {progress}", {
                        progress: `${fmtBytes(progress.done)} / ${fmtBytes(progress.total)}`,
                      })}
                </span>
                {!downloaded && <span className="mono faint">{pct}%</span>}
              </div>
              <p className="small faint">
                {t("You can keep working. An interrupted download picks up where it stopped.")}
              </p>
            </div>
          ) : busy === "check" ? (
            <StatusDot tone="warn">{t("Checking")}</StatusDot>
          ) : !status.pack_installed ? (
            <div className="ask-actions">
              <Button variant="primary" onClick={() => void install()} disabled={busy !== null}>
                <Download size={20} aria-hidden />
                {t("Download ({size})", { size: fmtBytes(status.download_bytes) })}
              </Button>
            </div>
          ) : status.check === "failed" ? (
            <>
              <p className="ob-notice" data-tone="danger" role="status">
                {t(
                  "The MLX engine didn't pass its check on this Mac, so MLX models can't write notes. GGUF models still work.",
                )}
              </p>
              {status.check_message && <p className="small faint">{status.check_message}</p>}
              <div className="ask-actions">
                <Button variant="primary" onClick={() => void check()} disabled={busy !== null}>
                  {t("Try again")}
                </Button>
                {removeButton}
              </div>
            </>
          ) : (
            <div className="gpu-row">
              <span className="mlx-ready">
                {status.check === "passed" ? (
                  <StatusDot tone="ok">{t("Ready")}</StatusDot>
                ) : (
                  <StatusDot tone="warn">{t("Checking")}</StatusDot>
                )}
                <span className="small faint">{t("{size} on disk", { size: fmtBytes(status.installed_bytes) })}</span>
              </span>
              {removeButton}
            </div>
          )}
        </>
      )}

      {removing && (
        <RemoveModelDialog
          target={{ kind: "mlx", bytes: status.installed_bytes }}
          noteModel={app.settings.llm}
          toast={app.toast}
          onClose={() => setRemoving(false)}
          onRemoved={() => void engine.mlxStatus().then(setStatus, () => {})}
        />
      )}
    </section>
  );
}

const runs = (s: engine.MlxSupport) => s === "yes" || s === "test_mode";
