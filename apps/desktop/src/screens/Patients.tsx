import { Button, MarkMono, Skeleton } from "@openbark/ui";
import { CallButton } from "../components/CallButton";
import { Search } from "lucide-react";
import { AnimatePresence, m } from "motion/react";
import { useEffect, useState } from "react";
import { useApp } from "../context";
import { ENTER_Y, EXIT, FADE_IN, LAYOUT_LIST_LIMIT, SPRING } from "../motion";
import * as engine from "../engine";
import { templateName, useT } from "../i18n";
import { speciesName } from "../species";
import { SpeciesIcon, TemplateIcon } from "../templateIcons";
import { whenLabel } from "../visits";

/**
 * Patients (ADR-0011): one row per animal, most recently seen first, with a
 * name filter. The visit list is its own destination, Visits (ADR-0027).
 */
export function Patients() {
  const app = useApp();
  const t = useT();
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
        <div className="head-actions">
          <CallButton variant="ghost" label={t("Call a number")} />
        </div>
      </header>
      <div className="patients-bar">
        <label className="patient-filter">
          <Search size={18} aria-hidden />
          <input
            className="ob-input"
            type="search"
            value={q}
            placeholder={t("Find a patient")}
            aria-label={t("Find a patient")}
            onChange={(e) => setQ(e.target.value)}
          />
        </label>
      </div>
      {error && (
        <p className="ob-notice" data-tone="danger" role="alert">
          {error}
        </p>
      )}
      {rows === null ? (
        <Skeleton rows={4} label={t("Loading")} />
      ) : rows.length === 0 ? (
        <div className="empty-state">
          <MarkMono size={48} className="faint" />
          <h3>{t("No patients yet")}</h3>
          <p className="muted">{t("Record or import a visit and its patient appears here.")}</p>
          <Button variant="primary" onClick={() => app.navigate({ screen: "record" })}>
            {t("Record")}
          </Button>
        </div>
      ) : shown.length === 0 ? (
        <p className="empty muted">{t("No patient matches that name.")}</p>
      ) : (
        // Filtering: rows that stay glide into place, rows that go fade out,
        // rows that come back fade in (Motion layout and presence). Very long
        // lists skip the layout animation so typing stays instant.
        <ul className="patient-list">
          <AnimatePresence initial={false} mode="popLayout">
            {shown.map((r) => (
              <m.li
                key={r.patient.id}
                layout={shown.length <= LAYOUT_LIST_LIMIT ? "position" : false}
                initial={{ opacity: 0, y: ENTER_Y }}
                animate={{ opacity: 1, y: 0, transition: FADE_IN }}
                exit={{ opacity: 0, transition: EXIT }}
                transition={SPRING}
              >
                <button
                  type="button"
                  className="patient-row row-link"
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
                        <span className="small faint">{whenLabel(r.last_visit, new Date(), app.settings.uiLanguage, t)}</span>
                      </span>
                    </span>
                  )}
                </button>
              </m.li>
            ))}
          </AnimatePresence>
        </ul>
      )}
    </div>
  );
}
