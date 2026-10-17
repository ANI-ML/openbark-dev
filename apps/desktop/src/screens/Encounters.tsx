import { StatusDot } from "@openbark/ui";
import { useEffect, useState } from "react";
import { fmtTime, useApp } from "../context";
import * as engine from "../engine";

/** DESIGN.md §6 Encounters list: a table, not cards. */
export function Encounters() {
  const app = useApp();
  const [rows, setRows] = useState<engine.SessionSummary[] | null>(null);

  useEffect(() => {
    engine.listSessions().then(setRows, () => setRows([]));
  }, []);

  return (
    <div className="screen">
      <header className="screen-head">
        <h1>Encounters</h1>
      </header>
      {rows === null ? (
        <div className="ob-skeleton" style={{ height: 160 }} />
      ) : rows.length === 0 ? (
        <p className="empty muted">No encounters yet. Record a consult and it appears here.</p>
      ) : (
        <div className="table-wrap">
          <table className="ob-table">
            <thead>
              <tr>
                <th>Time</th>
                <th>Patient</th>
                <th>Reason for visit</th>
                <th className="num">Length</th>
                <th className="num">Drafts</th>
                <th>Status</th>
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
                  <td className="reason-cell">{r.reason || <span className="faint">Not entered</span>}</td>
                  <td className="num">{fmtTime(r.audio_secs)}</td>
                  <td className="num">{r.runs}</td>
                  <td>
                    {r.signed ? (
                      <StatusDot tone="ok">Signed</StatusDot>
                    ) : r.runs > 0 ? (
                      <StatusDot tone="warn">Draft</StatusDot>
                    ) : (
                      <StatusDot>Transcribed</StatusDot>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}
