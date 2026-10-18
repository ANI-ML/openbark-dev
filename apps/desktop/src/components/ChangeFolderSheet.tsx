import { Button, Sheet } from "@openbark/ui";
import { ArrowUp, Folder, HardDrives, X } from "@phosphor-icons/react";
import { useEffect, useState } from "react";
import { fmtBytes, useApp } from "../context";
import * as engine from "../engine";
import { useT } from "../i18n";

type Step = "choose" | "review" | "moving" | "done";

/**
 * Put the models or the visits in another folder (ADR-0013). Nothing changes
 * until the clinic has seen where the files will go, how much room there is,
 * and chosen to move what's there or start empty.
 */
export function ChangeFolderSheet({
  kind,
  current,
  initialPath,
  onClose,
  onChanged,
}: {
  kind: engine.FolderKind;
  current: engine.FolderView;
  /** Already picked (desktop folder picker); web mode browses here. */
  initialPath?: string;
  onClose: () => void;
  onChanged: () => void;
}) {
  const app = useApp();
  const t = useT();
  const [step, setStep] = useState<Step>(initialPath ? "review" : "choose");
  const [check, setCheck] = useState<engine.FolderCheck | null>(null);
  const [mode, setMode] = useState<engine.ChangeMode>("move");
  const [moved, setMoved] = useState({ done: 0, total: 0 });
  const [error, setError] = useState<string | null>(null);
  const title = kind === "visits" ? t("Where visits are kept") : t("Where models are kept");
  // Whole sentences per kind, so each language can agree them properly.
  const visits = kind === "visits";
  const words = {
    size: (size: string) => (visits ? t("{size} of visits", { size }) : t("{size} of models", { size })),
    keepIn: visits ? t("OpenBark will keep the visits in") : t("OpenBark will keep the models in"),
    question: visits ? t("What happens to the visits you have now") : t("What happens to the models you have now"),
    addTo: visits ? t("Add them to the visits already there") : t("Add them to the models already there"),
    useThere: visits ? t("Use the visits already there") : t("Use the models already there"),
    stayBehind: visits
      ? t("The visits you have now stay in the old folder, and don't show until you switch back.")
      : t("The models you have now stay in the old folder, and don't show until you switch back."),
    nowIn: (path: string) =>
      visits ? t("The visits are now kept in {path}.", { path }) : t("The models are now kept in {path}.", { path }),
  };

  async function review(path: string) {
    setError(null);
    setStep("review");
    setCheck(null);
    try {
      const c = await engine.checkFolder(kind, path);
      setCheck(c);
      // Bring everything along unless the new folder already has its own, or there's no room.
      setMode(c.has_data || !c.can_move || current.used_bytes === 0 ? "switch" : "move");
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }

  useEffect(() => {
    if (initialPath) void review(initialPath);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    if (step !== "moving") return;
    const off = engine.on("moving", setMoved);
    return () => void off.then((f) => f());
  }, [step]);

  async function apply() {
    if (!check) return;
    setError(null);
    setMoved({ done: 0, total: check.needed_bytes });
    setStep("moving");
    try {
      await engine.changeFolder(kind, check.chosen, mode);
      setStep("done");
      onChanged();
      app.refreshStatus();
      app.announce(t("Folder changed"));
      if (engine.isDesktop) setTimeout(() => void engine.restartApp(), 1800);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setStep("review");
    }
  }

  const busy = step === "moving";
  return (
    <Sheet label={title} onClose={onClose} dismissible={!busy && !(step === "done" && engine.isDesktop)}>
      {(close) => (
        <>
          <header className="sheet-head">
            <h2>{title}</h2>
            <button type="button" className="icon-btn" aria-label={t("Close")} disabled={busy} onClick={close}>
              <X size={20} aria-hidden />
            </button>
          </header>
          <div className="sheet-body folder-sheet">
            <p className="small muted">
              {t("Now: {path}", { path: current.path })}
              <br />
              {words.size(fmtBytes(current.used_bytes))}
            </p>
            {error && (
              <p className="ob-notice" data-tone="danger" role="alert">
                {error}
              </p>
            )}

            {step === "choose" && <FolderBrowser start={current.path} onChoose={(p) => void review(p)} />}

            {step === "review" && !check && !error && <div className="ob-skeleton" style={{ height: 140 }} />}

            {step === "review" && check && (
              <>
                <div className="folder-target">
                  <span className="label">{words.keepIn}</span>
                  <span className="mono storage-path">{check.target}</span>
                  {check.target !== check.chosen && (
                    <span className="small faint">
                      {t("A new folder inside the one you chose, because it already holds other files.")}
                    </span>
                  )}
                  {check.free_bytes != null && (
                    <span className="small faint">
                      {t("{free} free on that drive", { free: fmtBytes(check.free_bytes) })}
                    </span>
                  )}
                </div>
                {check.problem ? (
                  <p className="ob-notice" data-tone="danger" role="alert">
                    {check.problem}
                  </p>
                ) : (
                  <>
                    {check.notices.map((n) => (
                      <p className="ob-notice" key={n}>
                        {n}
                      </p>
                    ))}
                    <fieldset className="folder-modes">
                      <legend className="label">{words.question}</legend>
                      <label className="check folder-mode">
                        <input
                          type="radio"
                          name="mode"
                          checked={mode === "move"}
                          disabled={!check.can_move || current.used_bytes === 0}
                          onChange={() => setMode("move")}
                        />
                        <span>
                          <span className="folder-mode-title">
                            {check.has_data
                              ? words.addTo
                              : t("Move them there ({size})", { size: fmtBytes(check.needed_bytes) })}
                          </span>
                          <span className="small faint">
                            {!check.can_move
                              ? t("There isn't room on that drive.")
                              : check.same_disk
                                ? t("Same drive, so this takes a moment.")
                                : t("Copied, checked, then removed from the old folder.")}
                          </span>
                        </span>
                      </label>
                      <label className="check folder-mode">
                        <input
                          type="radio"
                          name="mode"
                          checked={mode === "switch"}
                          onChange={() => setMode("switch")}
                        />
                        <span>
                          <span className="folder-mode-title">
                            {check.has_data ? words.useThere : t("Start with an empty folder")}
                          </span>
                          <span className="small faint">
                            {current.used_bytes > 0 ? words.stayBehind : t("Nothing to move.")}
                          </span>
                        </span>
                      </label>
                    </fieldset>
                    {kind === "models" && mode === "switch" && !check.has_data && current.used_bytes > 0 && (
                      <p className="small faint">
                        {t("OpenBark offers to download the models again into the new folder.")}
                      </p>
                    )}
                  </>
                )}
                <div className="ask-actions">
                  <Button variant="primary" onClick={() => void apply()} disabled={Boolean(check.problem)}>
                    {mode === "move" ? t("Move and use this folder") : t("Use this folder")}
                  </Button>
                  {!engine.isDesktop && (
                    <Button variant="ghost" onClick={() => setStep("choose")}>
                      {t("Choose another folder")}
                    </Button>
                  )}
                </div>
              </>
            )}

            {step === "moving" && (
              <div className="processing">
                <div
                  className="ob-progress"
                  role="progressbar"
                  aria-label={t("Moving")}
                  aria-valuenow={Math.round((moved.done / Math.max(1, moved.total)) * 100)}
                >
                  <span style={{ transform: `scaleX(${moved.total ? moved.done / moved.total : 0})` }} />
                </div>
                <div className="processing-text small">
                  <span>
                    {mode === "move"
                      ? t("Moving: {done} of {total}", { done: fmtBytes(moved.done), total: fmtBytes(moved.total) })
                      : t("Setting up the folder")}
                  </span>
                  <span className="mono faint">{Math.round((moved.done / Math.max(1, moved.total)) * 100)}%</span>
                </div>
                <p className="small faint">{t("Keep OpenBark open until this finishes.")}</p>
              </div>
            )}

            {step === "done" && check && (
              <>
                <p className="ob-notice" data-tone="ok" role="status">
                  {words.nowIn(check.target)}
                </p>
                {engine.isDesktop ? (
                  <p className="small muted">{t("OpenBark restarts to finish.")}</p>
                ) : (
                  <div className="ask-actions">
                    <Button variant="primary" onClick={close}>
                      {t("Done")}
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

/** Web mode has no system folder picker: browse this computer's folders. */
function FolderBrowser({ start, onChoose }: { start: string; onChoose: (path: string) => void }) {
  const t = useT();
  const [listing, setListing] = useState<engine.FolderListing | null>(null);
  const [path, setPath] = useState("");
  const [error, setError] = useState<string | null>(null);

  async function go(p: string) {
    setError(null);
    try {
      const l = await engine.listFolders(p);
      setListing(l);
      setPath(l.path);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }

  useEffect(() => {
    // Start beside the current folder; a path typed meanwhile stays.
    const parent = start.replace(/[\\/][^\\/]*$/, "");
    const first = (l: engine.FolderListing) => {
      setListing(l);
      setPath((typed) => typed || l.path);
    };
    engine.listFolders(parent).then(first, () => engine.listFolders("").then(first, () => {}));
  }, [start]);

  return (
    <div className="folder-browser">
      <label className="ob-field">
        <span className="label">{t("Folder")}</span>
        <input
          className="ob-input mono"
          value={path}
          placeholder={t("Choose a place below, or type a folder path")}
          onChange={(e) => setPath(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && void go(path)}
        />
      </label>
      {error && <p className="small danger">{error}</p>}
      <ul className="folder-list" aria-label={t("Folders")}>
        {listing?.parent != null && (
          <li>
            <button type="button" className="folder-row" onClick={() => void go(listing.parent ?? "")}>
              <ArrowUp size={18} aria-hidden />
              {listing.parent ? t("Up one level") : t("Drives and usual places")}
            </button>
          </li>
        )}
        {listing?.folders.map((f) => (
          <li key={f.path}>
            <button type="button" className="folder-row" onClick={() => void go(f.path)}>
              {listing.path ? <Folder size={18} aria-hidden /> : <HardDrives size={18} aria-hidden />}
              <span className="folder-name">{f.name}</span>
            </button>
          </li>
        ))}
        {listing && listing.path && listing.folders.length === 0 && (
          <li className="small faint folder-empty">{t("No folders inside. You can choose this one.")}</li>
        )}
      </ul>
      <div className="ask-actions">
        <Button variant="primary" onClick={() => onChoose(path)} disabled={!path.trim()}>
          {t("Choose this folder")}
        </Button>
      </div>
    </div>
  );
}
