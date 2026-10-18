import { StatusDot } from "@openbark/ui";
import { useEffect, useState } from "react";
import { fmtTime, useApp } from "../context";
import * as engine from "../engine";
import { useT } from "../i18n";

/** DESIGN.md §6 Encounters list: a table, not cards. */
export function Encounters({ embedded = false }: { embedded?: boolean } = {}) {
  const app = useApp();
  const t = useT();
  const [rows, setRows] = useState<engine.SessionSummary[] | null>(null);

  useEffect(() => {
    engine.listSessions().then(setRows, () => setRows([]));
  }, []);

  const body = (
    <>
      {rows === null ? (
        <div className="ob-skeleton" style={{ height: 160 }} />
      ) : rows.length === 0 ? (
        <p className="empty muted">{t("No visits yet. Record a consult and it appears here.")}</p>
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
                  <td>{r.patient || <span className="faint">{r.source}</span>}</td>
                  <td className="reason-cell">{r.reason || <span className="faint">{t("Not entered")}</span>}</td>
                  <td>{r.template || <span className="faint">{t("No draft")}</span>}</td>
                  <td className="num">{fmtTime(r.audio_secs)}</td>
                  <td className="num">{r.runs}</td>
                  <td>
                    {r.signed ? (
                      <StatusDot tone="ok">{t("Signed")}</StatusDot>
                    ) : r.runs > 0 ? (
                      <StatusDot tone="warn">{t("Draft")}</StatusDot>
                    ) : (
                      <StatusDot>{t("Transcribed")}</StatusDot>
                    )}
                  </td>
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
