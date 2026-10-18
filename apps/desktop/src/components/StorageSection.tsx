import { Button } from "@openbark/ui";
import { FolderOpen } from "lucide-react";
import { useEffect, useState } from "react";
import { fmtBytes } from "../context";
import * as engine from "../engine";
import { useT } from "../i18n";
import { ChangeFolderSheet } from "./ChangeFolderSheet";

/**
 * Settings, Storage (ADR-0013): where the models and the visits are kept,
 * how much room they take, and a way to put each wherever the clinic wants.
 */
export function StorageSection() {
  const t = useT();
  const [view, setView] = useState<engine.StorageView | null>(null);
  const [changing, setChanging] = useState<{ kind: engine.FolderKind; path?: string } | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = () => engine.storage().then(setView, (e) => setError(String(e?.message ?? e)));
  useEffect(() => {
    void refresh();
  }, []);

  async function change(kind: engine.FolderKind) {
    setError(null);
    if (engine.isDesktop) {
      // The system's own folder picker, then the check in the sheet.
      const path = await engine.pickFolder(view?.[kind].path).catch(() => null);
      if (path) setChanging({ kind, path });
    } else {
      setChanging({ kind });
    }
  }

  const row = (f: engine.FolderView, label: string, what: string) => (
    <div className="storage-row" key={f.kind}>
      <div className="storage-text">
        <span className="label">{label}</span>
        <span className="small muted">{what}</span>
        <span className="mono small storage-path">{f.path}</span>
        <span className="small faint">
          {f.free_bytes != null
            ? t("{used} used, {free} free on this drive", {
                used: fmtBytes(f.used_bytes),
                free: fmtBytes(f.free_bytes),
              })
            : t("{used} used", { used: fmtBytes(f.used_bytes) })}
        </span>
        {f.fixed_by && (
          <span className="small faint">{t("Set by {by}, so it can't be changed here.", { by: f.fixed_by })}</span>
        )}
      </div>
      <Button variant="ghost" onClick={() => void change(f.kind)} disabled={Boolean(f.fixed_by)}>
        <FolderOpen size={18} aria-hidden />
        {t("Change folder")}
      </Button>
    </div>
  );

  return (
    <section className="group">
      <h2>{t("Storage")}</h2>
      {error && (
        <p className="ob-notice" data-tone="danger" role="alert">
          {error}
        </p>
      )}
      {view === null ? (
        <div className="ob-skeleton" style={{ height: 120 }} />
      ) : (
        <div className="storage-rows">
          {row(view.visits, t("Visits"), t("Recordings, transcripts, notes and the patient list"))}
          {row(view.models, t("Models"), t("Speech and note models: large, and can be downloaded again"))}
        </div>
      )}
      <p className="small faint">{t("Recordings and notes stay on this computer, or on the drive you choose.")}</p>
      {changing && view && (
        <ChangeFolderSheet
          kind={changing.kind}
          current={view[changing.kind]}
          initialPath={changing.path}
          onClose={() => setChanging(null)}
          onChanged={() => void refresh()}
        />
      )}
    </section>
  );
}
