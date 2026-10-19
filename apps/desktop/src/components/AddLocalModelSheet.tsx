import { Button, Sheet } from "@openbark/ui";
import { FileUp, FolderOpen, X } from "lucide-react";
import { useEffect, useState } from "react";
import { fmtBytes, useApp } from "../context";
import * as engine from "../engine";
import { useT } from "../i18n";

type Step = "choose" | "checking" | "preview" | "adding";

/** Files listed in the preview before "and n more". */
const SHOWN_FILES = 6;

/**
 * Add a model from this computer (ADR-0025 §9): a .gguf file, or an MLX
 * folder on a Mac. Copied into OpenBark's models folder by default, or used
 * where it is. Nothing changes until the clinic has seen what will be added.
 */
export function AddLocalModelSheet({
  mlx,
  onClose,
  onAdded,
}: {
  /** Offer MLX folders (a Mac, or the developer test mode). */
  mlx: boolean;
  onClose: () => void;
  onAdded: (row: engine.ModelRow) => void;
}) {
  const app = useApp();
  const t = useT();
  const [step, setStep] = useState<Step>("choose");
  const [path, setPath] = useState("");
  const [preview, setPreview] = useState<engine.ImportPreview | null>(null);
  const [mode, setMode] = useState<engine.ImportMode>("copy");
  const [copied, setCopied] = useState({ done: 0, total: 0 });
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (step !== "adding" || !preview) return;
    const id = preview.id;
    const off = engine.on("download", (d) => d.model === id && setCopied({ done: d.done, total: d.total }));
    return () => void off.then((f) => f());
  }, [step, preview]);

  async function check(p: string) {
    const chosen = p.trim();
    if (!chosen) return;
    setError(null);
    setPath(chosen);
    setStep("checking");
    try {
      const pv = await engine.inspectImport(chosen);
      setPreview(pv);
      setMode("copy");
      setStep("preview");
    } catch (e) {
      setError(engine.modelMessage(t, e));
      setStep("choose");
    }
  }

  async function pick(kind: "file" | "folder") {
    setError(null);
    try {
      const p = kind === "file" ? await engine.pickModelFile() : await engine.pickModelFolder();
      if (p) await check(p);
    } catch (e) {
      setError(engine.modelMessage(t, e));
    }
  }

  async function add() {
    if (!preview) return;
    setError(null);
    setCopied({ done: 0, total: preview.total_bytes });
    setStep("adding");
    try {
      const row = await engine.importModel(path, mode);
      app.toast(t("Model added"));
      onAdded(row);
    } catch (e) {
      setError(engine.modelMessage(t, e));
      setStep("preview");
    }
  }

  const busy = step === "adding" || step === "checking";
  const title = t("Add from this computer");
  const pct = Math.round((copied.done / Math.max(1, copied.total)) * 100);

  return (
    <Sheet label={title} onClose={onClose} dismissible={!busy}>
      {(close) => (
        <>
          <header className="sheet-head">
            <h2>{title}</h2>
            <button type="button" className="icon-btn" aria-label={t("Close")} disabled={busy} onClick={close}>
              <X size={20} aria-hidden />
            </button>
          </header>
          <div className="sheet-body">
            {error && (
              <p className="ob-notice" data-tone="danger" role="alert">
                {error}
              </p>
            )}

            {(step === "choose" || step === "checking") &&
              (engine.isDesktop ? (
                <div className="ask-actions">
                  <Button variant="primary" disabled={busy} onClick={() => void pick("file")}>
                    <FileUp size={20} aria-hidden />
                    {t("Choose a .gguf file")}
                  </Button>
                  {mlx && (
                    <Button disabled={busy} onClick={() => void pick("folder")}>
                      <FolderOpen size={20} aria-hidden />
                      {t("Choose an MLX folder")}
                    </Button>
                  )}
                </div>
              ) : (
                <form
                  className="search"
                  onSubmit={(e) => {
                    e.preventDefault();
                    void check(path);
                  }}
                >
                  <label className="ob-field">
                    <span className="label">{t("Path on the computer running OpenBark")}</span>
                    <input
                      className="ob-input mono"
                      value={path}
                      disabled={busy}
                      onChange={(e) => setPath(e.target.value)}
                    />
                  </label>
                  <Button type="submit" disabled={busy || !path.trim()}>
                    {t("Check")}
                  </Button>
                </form>
              ))}
            {step === "checking" && <div className="ob-skeleton" style={{ height: 120 }} />}

            {(step === "preview" || step === "adding") && preview && (
              <>
                <div className="import-preview">
                  <span className="import-name">
                    {preview.name}
                    {preview.kind === "mlx" && <span className="ob-chip mlx-chip">MLX</span>}
                  </span>
                  <span className="small muted">
                    {fmtBytes(preview.total_bytes)}
                    {preview.model_type && ` · ${preview.model_type}`}
                    {preview.files.length > 1 &&
                      ` · ${
                        preview.kind === "gguf"
                          ? t("{n} parts", { n: preview.files.length })
                          : t("{n} files", { n: preview.files.length })
                      }`}
                  </span>
                  <span className="mono small faint import-path">{path}</span>
                  {preview.files.length > 1 && (
                    <ul className="import-files mono small faint">
                      {preview.files.slice(0, SHOWN_FILES).map((f) => (
                        <li key={f.path}>{f.path}</li>
                      ))}
                      {preview.files.length > SHOWN_FILES && <li>…</li>}
                    </ul>
                  )}
                </div>
                <fieldset className="folder-modes">
                  <label className="check folder-mode">
                    <input
                      type="radio"
                      name="import-mode"
                      checked={mode === "copy"}
                      disabled={step === "adding"}
                      onChange={() => setMode("copy")}
                    />
                    <span>
                      <span className="folder-mode-title">{t("Copy into OpenBark's models folder (recommended)")}</span>
                      <span className="small faint">
                        {t(
                          "Copying keeps the model working if the original moves. On the same drive, OpenBark links it instead, so it takes no extra space.",
                        )}
                      </span>
                      {preview.free_bytes != null && (
                        <span className="small faint">
                          {t("{free} free on that drive", { free: fmtBytes(preview.free_bytes) })}
                        </span>
                      )}
                    </span>
                  </label>
                  <label className="check folder-mode">
                    <input
                      type="radio"
                      name="import-mode"
                      checked={mode === "in_place"}
                      disabled={step === "adding"}
                      onChange={() => setMode("in_place")}
                    />
                    <span>
                      <span className="folder-mode-title">{t("Use it where it is, don't copy")}</span>
                      <span className="small faint">
                        {t("Used where it is, OpenBark never changes or deletes the file. If it moves, add it again.")}
                      </span>
                    </span>
                  </label>
                </fieldset>
                {step === "adding" ? (
                  <div className="processing">
                    <div className="ob-progress" role="progressbar" aria-label={title} aria-valuenow={pct}>
                      <span
                        style={{ transform: `scaleX(${copied.total ? Math.min(1, copied.done / copied.total) : 0})` }}
                      />
                    </div>
                    <div className="processing-text small">
                      <span>
                        {mode === "copy"
                          ? t("Copying {progress}", {
                              progress: `${fmtBytes(copied.done)} / ${fmtBytes(copied.total)}`,
                            })
                          : t("Checking")}
                      </span>
                      <span className="mono faint">{pct}%</span>
                    </div>
                  </div>
                ) : (
                  <div className="ask-actions">
                    <Button variant="primary" onClick={() => void add()}>
                      {t("Add")}
                    </Button>
                    <Button
                      variant="ghost"
                      onClick={() => {
                        setPreview(null);
                        setStep("choose");
                      }}
                    >
                      {t("Back")}
                    </Button>
                  </div>
                )}
              </>
            )}
          </div>
        </>
      )}
    </Sheet>
  );
}
