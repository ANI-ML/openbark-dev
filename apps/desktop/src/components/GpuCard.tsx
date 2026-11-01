import { Button, StatusDot } from "@openbark/ui";
import { useEffect, useState } from "react";
import { fmtBytes, useApp } from "../context";
import * as engine from "../engine";
import { useT } from "../i18n";
import { ExternalLink, NVIDIA_LICENCE } from "./ExternalLink";
import { RemoveModelDialog } from "./RemoveModelDialog";

/**
 * Models: speech recognition on an NVIDIA graphics card (ADR-0014). Shown
 * only where it can work. NVIDIA's libraries come from NVIDIA, under its
 * licence, so the clinic accepts that first.
 */
export function GpuCard() {
  const app = useApp();
  const t = useT();
  const [status, setStatus] = useState<engine.GpuStatus | null>(null);
  const [accepted, setAccepted] = useState(false);
  const [busy, setBusy] = useState<"install" | "remove" | null>(null);
  const [removing, setRemoving] = useState(false);
  const [progress, setProgress] = useState({ done: 0, total: 0 });
  const [error, setError] = useState<string | null>(null);

  // While a download runs (started here or in another tab), keep the status fresh.
  const installing = busy === "install" || Boolean(status?.installing);
  useEffect(() => {
    if (!status?.installing || busy === "install") return;
    const timer = setInterval(() => engine.gpuStatus().then(setStatus, () => {}), 3000);
    return () => clearInterval(timer);
  }, [status?.installing, busy]);

  useEffect(() => {
    engine.gpuStatus().then(setStatus, () => {});
    const off = engine.on(
      "download",
      (d) => d.model === "gpu-speech-pack" && setProgress({ done: d.done, total: d.total }),
    );
    return () => void off.then((f) => f());
  }, []);

  if (!status?.supported) return null;

  async function install() {
    setError(null);
    setBusy("install");
    setProgress({ done: 0, total: status?.download_bytes ?? 0 });
    try {
      setStatus(await engine.installGpu(accepted));
      app.announce(t("GPU speech pack downloaded"));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(null);
    }
  }

  const ready = status.pack_installed && status.models_installed;
  const removeButton = (status.pack_installed || status.models_installed) && !installing && (
    <Button variant="danger" onClick={() => setRemoving(true)} disabled={busy !== null}>
      {t("Remove")}
    </Button>
  );
  // ADR-0025 §10: the card shows the space the pack uses.
  const onDisk = status.installed_bytes > 0 && (
    <span className="small faint">{t("{size} on disk", { size: fmtBytes(status.installed_bytes) })}</span>
  );
  return (
    <section className="group gpu-card">
      {/* §7.4: no icon beside the title. */}
      <h2>{t("Faster transcription on the graphics card")}</h2>
      <p className="small muted measure">
        {t(
          "Your {gpu} can do the speech recognition. Measured on an RTX 4080, an 8.5-minute consult was ready in 43 seconds instead of 2.5 minutes, because the number check keeps up instead of finishing afterwards.",
          { gpu: status.gpu ?? t("NVIDIA graphics card") },
        )}
      </p>
      {error && (
        <p className="ob-notice" data-tone="danger" role="alert">
          {error}
        </p>
      )}
      {status.check_failed && (
        <p className="ob-notice" data-tone="danger" role="status">
          {t(
            "The graphics card couldn't run speech recognition, so the processor is used. Updating the NVIDIA driver may help; then choose Try again.",
          )}
        </p>
      )}
      {status.active ? (
        <div className="gpu-row">
          <StatusDot tone="ok">{t("On: speech recognition runs on the graphics card.")}</StatusDot>
          {onDisk}
          {removeButton}
        </div>
      ) : status.restart_needed && !status.check_failed ? (
        <div className="gpu-row">
          <p className="ob-notice" data-tone="ok">
            {t("Downloaded. Restart OpenBark to start using the graphics card.")}
          </p>
          <Button
            variant="primary"
            disabled={busy !== null}
            onClick={async () => {
              setBusy("remove");
              try {
                await engine.restartApp();
                window.location.reload();
              } catch (e) {
                setError(e instanceof Error ? e.message : String(e));
                setBusy(null);
              }
            }}
          >
            {t("Restart now")}
          </Button>
          {onDisk}
          {removeButton}
        </div>
      ) : installing ? (
        <div className="processing">
          <div
            className="ob-progress"
            role="progressbar"
            aria-label={t("Downloading the GPU speech pack")}
            aria-valuenow={Math.round((progress.done / Math.max(1, progress.total)) * 100)}
          >
            <span style={{ transform: `scaleX(${progress.total ? progress.done / progress.total : 0})` }} />
          </div>
          <div className="processing-text small">
            <span>
              {t("Downloading: {done} of {total}", { done: fmtBytes(progress.done), total: fmtBytes(progress.total) })}
            </span>
            <span className="mono faint">{Math.round((progress.done / Math.max(1, progress.total)) * 100)}%</span>
          </div>
          <p className="small faint">{t("You can keep working. An interrupted download picks up where it stopped.")}</p>
        </div>
      ) : (
        <>
          <p className="small">
            {t(
              "Downloads {size}: full-precision versions of the two speech models, ONNX Runtime for CUDA, and NVIDIA's CUDA runtime, cuBLAS and cuDNN libraries, which come from NVIDIA.",
              { size: fmtBytes(status.download_bytes) },
            )}
          </p>
          <label className="check">
            <input type="checkbox" checked={accepted} onChange={(e) => setAccepted(e.target.checked)} />
            <span>
              {t("I accept NVIDIA's licence for its libraries")}{" "}
              <ExternalLink href={NVIDIA_LICENCE} page={{ page: "nvidia-licence" }}>
                {t("(read it)")}
              </ExternalLink>
            </span>
          </label>
          {onDisk && <p>{onDisk}</p>}
          <div className="ask-actions">
            <Button variant="primary" onClick={() => void install()} disabled={!accepted || busy !== null}>
              {status.check_failed ? t("Try again") : ready ? t("Set up") : t("Download")}
            </Button>
            {removeButton}
          </div>
        </>
      )}
      {removing && (
        <RemoveModelDialog
          target={{ kind: "gpu", bytes: status.installed_bytes }}
          noteModel={app.settings.llm}
          toast={app.toast}
          onClose={() => setRemoving(false)}
          onRemoved={(r) => {
            if (r.gpu) setStatus(r.gpu);
            else void engine.gpuStatus().then(setStatus, () => {});
          }}
        />
      )}
    </section>
  );
}
