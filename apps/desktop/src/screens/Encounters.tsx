import { Button, MarkMono, Skeleton, StatusDot } from "@openbark/ui";
import { useEffect, useState } from "react";
import { fmtTime, useApp } from "../context";
import * as engine from "../engine";
import { sourceLabel, useT } from "../i18n";
import { PHONE, useMediaQuery } from "../useMediaQuery";

/**
 * Recent visits: a table, not cards; it scrolls inside its box, never the page (DESIGN.md §9).
 * On phones the same visits are stacked rows, like a patient's visit list.
 */
export function Encounters({ embedded = false }: { embedded?: boolean } = {}) {
  const app = useApp();
  const t = useT();
  const [rows, setRows] = useState<engine.SessionSummary[] | null>(null);
  const phone = useMediaQuery(PHONE);

  useEffect(() => {
    engine.listSessions().then(setRows, () => setRows([]));
  }, []);

  const syncWords: Record<engine.SyncVisitState, string> = {
    synced: t("Synced"),
    pending: t("Waiting to sync"),
    stopped: t("Not syncing"),
  };
  const status = (r: engine.SessionSummary) => (
    <span className="visit-status">
      {r.signed ? (
        <StatusDot tone="ok">{t("Signed")}</StatusDot>
      ) : r.runs > 0 ? (
        <StatusDot tone="warn">{t("Draft")}</StatusDot>
      ) : (
        <StatusDot>{t("Transcribed")}</StatusDot>
      )}
      {r.sync && <span className="small faint sync-state">{syncWords[r.sync]}</span>}
    </span>
  );
  // No patient entered: say so, with where the visit came from (a file name or a live recording) after it.
  const noPatient = (source: string) => (
    <span className="faint">
      {t("No patient")}
      <span className="small"> · {sourceLabel(t, source)}</span>
    </span>
  );

  const body = (
    <>
      {rows === null ? (
        <Skeleton rows={3} label={t("Loading")} />
      ) : rows.length === 0 ? (
        <div className="empty-state">
          <MarkMono size={48} className="faint" />
          <h3>{t("No visits yet")}</h3>
          <p className="muted">{t("Record a consult and it appears here.")}</p>
          <Button variant="primary" onClick={() => app.navigate({ screen: "record" })}>
            {t("New visit")}
          </Button>
        </div>
      ) : phone ? (
        // Phones: the stacked rows the Patients tab uses, not a seven-column table (§9).
        <ul className="visit-list">
          {rows.map((r) => (
            <li key={r.id}>
              <button
                type="button"
                className="visit-row row-link"
                onClick={() => app.navigate({ screen: "review", id: r.id })}
              >
                <span className="visit-text">
                  <span className="visit-type">{r.patient || noPatient(r.source)}</span>
                  <span className="small muted visit-reason">
                    {r.reason || r.template || t("Not entered")}
                  </span>
                </span>
                <span className="visit-meta">
                  <span className="small faint mono">{r.created_at}</span>
                  <span className="small faint mono">{fmtTime(r.audio_secs)}</span>
                  {status(r)}
                </span>
              </button>
            </li>
          ))}
        </ul>
      ) : (
        <div className="table-wrap">
          <table className="ob-table">
            <thead>
              <tr>
                <th>{t("Time")}</th>
                <th>{t("Patient")}</th>
                <th>{t("Reason for visit")}</th>
                <th>{t("Template")}</th>
                <th className="num">{t("Length")}</th>
                <th className="num">{t("Drafts")}</th>
                <th>{t("Status")}</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((r) => (
                <tr
                  key={r.id}
                  tabIndex={0}
                  className="row-link"
                  onClick={() => app.navigate({ screen: "review", id: r.id })}
                  onKeyDown={(e) => e.key === "Enter" && app.navigate({ screen: "review", id: r.id })}
                >
                  <td className="mono">{r.created_at}</td>
                  <td>{r.patient || noPatient(r.source)}</td>
                  <td className="reason-cell">{r.reason || <span className="faint">{t("Not entered")}</span>}</td>
                  <td>{r.template || <span className="faint">{t("No draft")}</span>}</td>
                  <td className="num">{fmtTime(r.audio_secs)}</td>
                  <td className="num">{r.runs}</td>
                  <td>{status(r)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </>
  );
  if (embedded) return body;
  return (
    <div className="screen">
      <header className="screen-head">
        <h1>{t("Recent visits")}</h1>
      </header>
      {body}
    </div>
  );
}
