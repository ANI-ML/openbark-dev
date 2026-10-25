import { MarkMono, Skeleton, StatusDot } from "@openbark/ui";
import { ChevronRight, Search } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { fmtTime, useApp } from "../context";
import * as engine from "../engine";
import { sourceLabel, useT } from "../i18n";
import { PHONE, useMediaQuery } from "../useMediaQuery";
import {
  animalFamily,
  firstWhen,
  inWhen,
  isIos,
  needsYou,
  recordedOn,
  visitStatus,
  whenLabel,
  type When,
} from "../visits";

const WHEN: { id: When; label: string }[] = [
  { id: "today", label: "Today" },
  { id: "week", label: "This week" },
  { id: "earlier", label: "Earlier" },
];

/**
 * Visits (ADR-0027): its own destination, grouped by what needs you (not
 * signed) and what's done (signed). Times as people say them, the length in
 * mono, one status per visit. A table on desktop that scrolls inside its
 * box (DESIGN.md §9); inset grouped rows on phones. Searching looks through
 * every visit, whatever the time filter says.
 */
export function Visits() {
  const app = useApp();
  const t = useT();
  const phone = useMediaQuery(PHONE);
  const [rows, setRows] = useState<engine.SessionSummary[] | null>(null);
  const [q, setQ] = useState("");
  const [when, setWhen] = useState<When | null>(null);
  const now = useMemo(() => new Date(), []);

  useEffect(() => {
    engine.listSessions().then(
      (r) => {
        setRows(r);
        setWhen((w) => w ?? firstWhen(r, new Date()));
      },
      () => setRows([]),
    );
  }, []);

  const needle = q.trim().toLowerCase();
  const shown = (rows ?? []).filter((r) =>
    needle
      ? `${r.animal ?? ""} ${r.family ?? ""} ${r.patient} ${r.reason}`.toLowerCase().includes(needle)
      : inWhen(r.created_at, when ?? "today", now),
  );
  const groups = [
    { id: "needs", title: t("Needs you"), rows: shown.filter(needsYou) },
    { id: "done", title: t("Done"), rows: shown.filter((r) => !needsYou(r)) },
  ].filter((g) => g.rows.length > 0);

  const open = (r: engine.SessionSummary) => app.navigate({ screen: "review", id: r.id });
  const name = (r: engine.SessionSummary) => animalFamily(r) || t("No patient");
  const reason = (r: engine.SessionSummary) => r.reason || (animalFamily(r) ? "" : sourceLabel(t, r.source));
  const syncWords: Record<engine.SyncVisitState, string> = {
    synced: t("Synced"),
    pending: t("Waiting to sync"),
    stopped: t("Not syncing"),
  };
  const pill = (r: engine.SessionSummary) => {
    const s = visitStatus(r, app.status, t, !(isIos(app.status) && !app.settings.writeNotesHere));
    return <StatusDot tone={s.tone}>{s.text}</StatusDot>;
  };

  return (
    <div className="screen visits">
      <header className="screen-head">
        <h1>{t("Visits")}</h1>
        <div className="visits-tools">
          <label className="patient-filter visits-search">
            <Search size={18} aria-hidden />
            <input
              className="ob-input"
              type="search"
              value={q}
              placeholder={t("Search by animal or family")}
              aria-label={t("Search visits")}
              onChange={(e) => setQ(e.target.value)}
            />
          </label>
          <div className="ob-segmented visits-when" role="tablist" aria-label={t("When")}>
            {WHEN.map((w) => (
              <button
                key={w.id}
                type="button"
                role="tab"
                className="ob-segment"
                aria-selected={!needle && when === w.id}
                disabled={Boolean(needle)}
                onClick={() => setWhen(w.id)}
              >
                {t(w.label)}
              </button>
            ))}
          </div>
        </div>
      </header>

      {rows === null ? (
        <Skeleton rows={3} label={t("Loading")} />
      ) : rows.length === 0 ? (
        <div className="empty-state">
          <MarkMono size={48} className="faint" />
          <h3>{t("No visits yet")}</h3>
          <p className="muted">{t("Record a consult and it appears here.")}</p>
        </div>
      ) : groups.length === 0 ? (
        <p className="empty muted">
          {needle ? t("No visit matches that name.") : t("No visits in this period.")}
        </p>
      ) : phone ? (
        groups.map((g) => (
          <section key={g.id} className="inset-group" aria-label={g.title}>
            <h2 className="inset-title">{g.title}</h2>
            <ul className="inset-list">
              {g.rows.map((r) => (
                <li key={r.id}>
                  <button type="button" className="inset-row visit-row row-link" onClick={() => open(r)}>
                    <span className="inset-text">
                      <span className="inset-name">{name(r)}</span>
                      <span className="small muted inset-sub">
                        {[reason(r), whenLabel(r.created_at, now, app.settings.uiLanguage, t), fmtTime(r.audio_secs)]
                          .filter(Boolean)
                          .join(", ")}
                      </span>
                    </span>
                    {pill(r)}
                    <ChevronRight size={18} aria-hidden className="inset-chevron" />
                  </button>
                </li>
              ))}
            </ul>
          </section>
        ))
      ) : (
        <div className="table-wrap">
          <table className="ob-table visits-table">
            <thead>
              <tr>
                <th>{t("Animal and family")}</th>
                <th>{t("Reason")}</th>
                <th>{t("Time")}</th>
                <th className="num">{t("Length")}</th>
                <th>{t("Recorded on")}</th>
                <th>{t("Status")}</th>
              </tr>
            </thead>
            {groups.map((g) => (
              <tbody key={g.id}>
                <tr className="group-row">
                  <th colSpan={6} scope="colgroup">
                    {g.title}
                  </th>
                </tr>
                {g.rows.map((r) => (
                  <tr key={r.id} className="row-link" onClick={() => open(r)}>
                    <td className="visit-name-cell">
                      <button type="button" className="cell-link" onClick={(e) => (e.stopPropagation(), open(r))}>
                        {name(r)}
                      </button>
                    </td>
                    <td className="reason-cell muted">{reason(r) || <span className="faint">{t("Not entered")}</span>}</td>
                    <td className="nowrap">{whenLabel(r.created_at, now, app.settings.uiLanguage, t)}</td>
                    <td className="num mono">{fmtTime(r.audio_secs)}</td>
                    <td className="muted">
                      <span className="visit-status">
                        <span>{recordedOn(r, app.status, t)}</span>
                        {r.sync && r.sync !== "synced" && <span className="small faint">{syncWords[r.sync]}</span>}
                      </span>
                    </td>
                    <td>{pill(r)}</td>
                  </tr>
                ))}
              </tbody>
            ))}
          </table>
        </div>
      )}
    </div>
  );
}
