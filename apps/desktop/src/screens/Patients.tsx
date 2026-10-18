import { Button } from "@openbark/ui";
import { MagnifyingGlass, Microphone } from "@phosphor-icons/react";
import { useEffect, useState } from "react";
import { useApp } from "../context";
import * as engine from "../engine";
import { templateName, useT } from "../i18n";
import { speciesName } from "../species";
import { SpeciesIcon, TemplateIcon } from "../templateIcons";
import { Encounters } from "./Encounters";

/**
 * Patients and their visits (ADR-0011): one row per animal, most recently
 * seen first, with "Recent visits" one tap away for the day's list.
 */
export function Patients() {
  const app = useApp();
  const t = useT();
  const [view, setView] = useState<"patients" | "visits">("patients");
  const [rows, setRows] = useState<engine.PatientSummary[] | null>(null);
  const [q, setQ] = useState("");
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    engine.listPatients().then(setRows, (e) => {
      setError(String(e?.message ?? e));
      setRows([]);
    });
  }, []);

  const needle = q.trim().toLowerCase();
  const shown = (rows ?? []).filter(
    (r) => !needle || `${r.patient.name} ${r.patient.last_name}`.toLowerCase().includes(needle),
  );

  return (
    <div className="screen patients">
      <header className="screen-head">
        <h1>{t("Patients")}</h1>
        <Button variant="primary" onClick={() => app.navigate({ screen: "record" })}>
          <Microphone size={20} aria-hidden />
          {t("New visit")}
        </Button>
      </header>
      <div className="patients-bar">
        <div className="ob-segmented" role="tablist" aria-label={t("Show")}>
          <button
            type="button"
            role="tab"
            className="ob-segment"
            aria-selected={view === "patients"}
            onClick={() => setView("patients")}
          >
            {t("Patients")}
          </button>
          <button
            type="button"
            role="tab"
            className="ob-segment"
            aria-selected={view === "visits"}
            onClick={() => setView("visits")}
          >
            {t("Recent visits")}
          </button>
        </div>
        {view === "patients" && (
          <label className="patient-filter">
            <MagnifyingGlass size={18} aria-hidden />
            <input
              className="ob-input"
              type="search"
              value={q}
              placeholder={t("Find a patient")}
              aria-label={t("Find a patient")}
              onChange={(e) => setQ(e.target.value)}
            />
          </label>
        )}
      </div>
      {error && (
        <p className="ob-notice" data-tone="danger" role="alert">
          {error}
        </p>
      )}
      {view === "visits" ? (
        <Encounters embedded />
      ) : rows === null ? (
        <div className="ob-skeleton" style={{ height: 200 }} />
      ) : rows.length === 0 ? (
        <p className="empty muted">{t("No patients yet. Record or import a visit and its patient appears here.")}</p>
      ) : shown.length === 0 ? (
        <p className="empty muted">{t("No patient matches that name.")}</p>
      ) : (
        <ul className="patient-list">
          {shown.map((r) => (
            <li key={r.patient.id}>
              <button
                type="button"
                className="patient-row"
                onClick={() => app.navigate({ screen: "patient", id: r.patient.id })}
              >
                <span className="patient-avatar" aria-hidden>
                  <SpeciesIcon species={r.patient.species} size={22} />
                </span>
                <span className="patient-text">
                  <span className="patient-name">
                    {r.patient.name}
                    {r.patient.last_name && <span className="muted"> {r.patient.last_name}</span>}
                  </span>
                  <span className="small muted">
                    {[
                      r.patient.species ? t(speciesName(r.patient.species)) : "",
                      r.visits === 1 ? t("1 visit") : t("{n} visits", { n: r.visits }),
                    ]
                      .filter(Boolean)
                      .join(", ")}
                  </span>
                </span>
                {r.last_visit && (
                  <span className="patient-last">
                    <span className="type-icon small-icon">
                      <TemplateIcon template={{ icon: r.last_icon }} size={16} />
                    </span>
                    <span className="patient-last-text">
                      <span className="small">{r.last_type ? templateName(t, r.last_type) : t("No draft")}</span>
                      <span className="small faint mono">{r.last_visit}</span>
                    </span>
                  </span>
                )}
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
