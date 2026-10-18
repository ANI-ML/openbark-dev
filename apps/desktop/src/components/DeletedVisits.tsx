import { Button, Dialog, StatusDot } from "@openbark/ui";
import { RotateCcw, Trash2 } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useApp } from "../context";
import { listNames, reasonLabel } from "../deleting";
import * as engine from "../engine";
import { useT } from "../i18n";

/**
 * Settings, Deleted visits (ADR-0022 section 11): every deleted visit, kept
 * for 30 days, with Restore and Delete now. Shown to veterinarians and
 * technicians (and single-user), never to reception.
 */
export function DeletedVisits() {
  const app = useApp();
  const t = useT();
  const [items, setItems] = useState<engine.DeletedSummary[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [purging, setPurging] = useState<engine.DeletedSummary | null>(null);
  const by = app.actor.trim() || null;
  const fail = (e: unknown) => t(e instanceof Error ? e.message : String(e));

  const load = useCallback(
    () => engine.listDeleted(by).then(setItems, (e) => setError(fail(e))),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [by],
  );
  useEffect(() => void load(), [load]);

  async function restore(item: engine.DeletedSummary) {
    setError(null);
    setBusy(item.trash_id);
    try {
      await engine.restoreVisit(item.trash_id, by);
      app.toast(t("Visit restored"));
      await load();
    } catch (e) {
      setError(fail(e));
    } finally {
      setBusy(null);
    }
  }

  async function purge(item: engine.DeletedSummary) {
    try {
      await engine.purgeDeleted(item.trash_id, by);
    } catch (e) {
      throw new Error(fail(e));
    }
    app.toast(t("Visit deleted for good"));
    await load();
  }

  return (
    <section className="group deleted-visits" aria-labelledby="deleted-visits-title">
      <h2 id="deleted-visits-title">{t("Deleted visits")}</h2>
      <p className="small muted">{t("Deleted visits are kept here for 30 days, then removed for good.")}</p>
      {error && (
        <p className="ob-notice" data-tone="danger" role="alert">
          {error}
        </p>
      )}
      {items === null ? (
        <div className="ob-skeleton" style={{ height: 72 }} />
      ) : items.length === 0 ? (
        <p className="small faint deleted-empty">{t("No deleted visits.")}</p>
      ) : (
        <ul className="deleted-list">
          {items.map((item) => (
            <li key={item.trash_id} className="deleted-row">
              <div className="deleted-text">
                <span className="deleted-who">
                  <span className="label">{item.patient || t("No patient")}</span>
                  {item.pets.length > 1 && (
                    <span className="small muted">{listNames(app.settings.uiLanguage, item.pets)}</span>
                  )}
                  {item.signed && <StatusDot tone="ok">{t("Signed")}</StatusDot>}
                </span>
                <span className="small muted">{t("Visit of {date}", { date: item.created_at })}</span>
                <span className="small muted">
                  {t("Deleted {date} by {name}", { date: item.deleted_at, name: item.by })}
                </span>
                <span className="small muted">
                  {reasonLabel(t, item.reason.kind)}
                  {item.reason.note ? `: ${item.reason.note}` : ""}
                </span>
                <span className="small faint">
                  {item.kept_until
                    ? t("Kept until {date} by your clinic's rule", { date: item.kept_until.slice(0, 10) })
                    : t("Removed for good on {date}", { date: item.purge_after.slice(0, 10) })}
                </span>
              </div>
              <div className="deleted-actions">
                <Button size="compact" disabled={busy != null} onClick={() => void restore(item)}>
                  <RotateCcw size={16} aria-hidden />
                  {t("Restore")}
                </Button>
                <Button variant="danger" size="compact" disabled={busy != null} onClick={() => setPurging(item)}>
                  <Trash2 size={16} aria-hidden />
                  {t("Delete now")}
                </Button>
              </div>
            </li>
          ))}
        </ul>
      )}
      {purging && (
        <DeleteNowDialog item={purging} onConfirm={() => purge(purging)} onClose={() => setPurging(null)} />
      )}
    </section>
  );
}

/** "Delete this visit now?": removed for good, said plainly (ADR-0022 section 8). */
export function DeleteNowDialog({
  item,
  onConfirm,
  onClose,
}: {
  item: engine.DeletedSummary;
  /** Rejects (with a message to show) to keep the dialog open. */
  onConfirm: () => Promise<void>;
  onClose: () => void;
}) {
  const t = useT();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  return (
    <Dialog labelledBy="delete-now-title" onClose={onClose} dismissible={!busy} className="delete-dialog">
      {(close) => (
        <>
          <h2 id="delete-now-title">{t("Delete this visit now?")}</h2>
          <p>
            {t(
              "OpenBark removes its files now, and it can't be restored. Backups of this computer may still hold encrypted copies, which the recovery key can open.",
            )}
          </p>
          {item.signed && (
            <p>
              {t("This visit has a signed note. Only a veterinarian can remove it before {date}.", {
                date: (item.kept_until ?? item.purge_after).slice(0, 10),
              })}
            </p>
          )}
          {error && (
            <p className="ob-notice" data-tone="danger" role="alert">
              {error}
            </p>
          )}
          <div className="ask-actions">
            <Button
              variant="danger"
              disabled={busy}
              onClick={async () => {
                setBusy(true);
                setError(null);
                try {
                  await onConfirm();
                  close();
                } catch (e) {
                  setError(e instanceof Error ? e.message : String(e));
                  setBusy(false);
                }
              }}
            >
              <Trash2 size={18} aria-hidden />
              {t("Delete now")}
            </Button>
            <Button variant="ghost" disabled={busy} onClick={close} autoFocus>
              {t("Keep visit")}
            </Button>
          </div>
        </>
      )}
    </Dialog>
  );
}
