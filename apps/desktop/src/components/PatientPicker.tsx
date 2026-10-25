import { useEffect, useId, useState } from "react";
import * as engine from "../engine";
import { useT } from "../i18n";
import { speciesName } from "../species";
import { SpeciesIcon } from "../templateIcons";

/**
 * Choose an existing patient for the visit (ADR-0011), or leave it empty and
 * type a new one into the fields below. Choosing fills the animal's name,
 * family name and species and links the visit to that patient.
 */
export function PatientPicker({
  value,
  onChange,
  disabled,
  onNew,
}: {
  value: engine.Patient;
  onChange: (p: engine.Patient) => void;
  disabled?: boolean;
  /** No match: offer the typed name as a new patient (Record's compact strip, ADR-0027). */
  onNew?: (name: string) => void;
}) {
  const t = useT();
  const id = useId();
  const [all, setAll] = useState<engine.PatientSummary[] | null>(null);
  const [q, setQ] = useState("");

  useEffect(() => {
    engine.listPatients().then(setAll, () => setAll([]));
  }, []);

  const chosen = value.patient_id ? all?.find((p) => p.patient.id === value.patient_id) : undefined;
  if (value.patient_id) {
    return (
      <div className="picked-patient">
        <span className="patient-avatar" aria-hidden>
          <SpeciesIcon species={value.species} size={20} />
        </span>
        <span className="picked-text">
          <span className="picked-name">{`${value.name} ${value.last_name}`.trim()}</span>
          <span className="small muted">
            {!chosen
              ? t("Existing patient")
              : chosen.visits === 1
                ? t("Existing patient, 1 visit")
                : t("Existing patient, {n} visits", { n: chosen.visits })}
          </span>
        </span>
        <button
          type="button"
          className="link small"
          disabled={disabled}
          onClick={() => onChange({ ...engine.emptyPatient(), reason: value.reason })}
        >
          {t("Choose another")}
        </button>
      </div>
    );
  }

  const needle = q.trim().toLowerCase();
  const matches = needle
    ? (all ?? []).filter((p) => `${p.patient.name} ${p.patient.last_name}`.toLowerCase().includes(needle)).slice(0, 6)
    : [];
  return (
    <div className="patient-search">
      <label className="ob-field" htmlFor={`${id}-q`}>
        <span className="label">{t("Find a patient")}</span>
        <input
          id={`${id}-q`}
          className="ob-input"
          role="combobox"
          aria-expanded={matches.length > 0}
          aria-controls={`${id}-list`}
          aria-autocomplete="list"
          autoComplete="off"
          value={q}
          disabled={disabled || all === null}
          placeholder={t("Name or family name")}
          onChange={(e) => setQ(e.target.value)}
        />
      </label>
      {matches.length > 0 && (
        <ul className="patient-matches" id={`${id}-list`} role="listbox" aria-label={t("Patients")}>
          {matches.map((p) => (
            <li key={p.patient.id} role="option" aria-selected={false}>
              <button
                type="button"
                className="patient-match"
                onClick={() => {
                  setQ("");
                  onChange({
                    ...value,
                    name: p.patient.name,
                    last_name: p.patient.last_name,
                    species: p.patient.species,
                    patient_id: p.patient.id,
                  });
                }}
              >
                <span className="patient-avatar" aria-hidden>
                  <SpeciesIcon species={p.patient.species} size={18} />
                </span>
                <span className="picked-text">
                  <span className="picked-name">{`${p.patient.name} ${p.patient.last_name}`.trim()}</span>
                  <span className="small muted">
                    {[
                      p.patient.species ? t(speciesName(p.patient.species)) : "",
                      p.visits === 1 ? t("1 visit") : t("{n} visits", { n: p.visits }),
                    ]
                      .filter(Boolean)
                      .join(", ")}
                  </span>
                </span>
              </button>
            </li>
          ))}
        </ul>
      )}
      {needle && matches.length === 0 && all !== null && onNew ? (
        <button
          type="button"
          className="link small"
          onClick={() => {
            onNew(q.trim());
            setQ("");
          }}
        >
          {t("Add {name} as a new patient", { name: q.trim() })}
        </button>
      ) : (
        needle &&
        matches.length === 0 &&
        all !== null && (
          <p className="small muted">{t("No patient by that name yet: fill in the details below to add one.")}</p>
        )
      )}
    </div>
  );
}
