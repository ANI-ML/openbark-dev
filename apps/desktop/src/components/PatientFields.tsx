import type { Patient } from "../engine";
import { useT } from "../i18n";

/** Patient name, last name and reason for visit (DESIGN.md §5 Inputs: label above). */
export function PatientFields({
  value,
  onChange,
  disabled,
  idPrefix = "patient",
}: {
  value: Patient;
  onChange: (p: Patient) => void;
  disabled?: boolean;
  idPrefix?: string;
}) {
  const t = useT();
  const set = (k: keyof Patient) => (e: React.ChangeEvent<HTMLInputElement | HTMLTextAreaElement>) =>
    onChange({ ...value, [k]: e.target.value });
  return (
    <div className="patient-fields">
      <label className="ob-field" htmlFor={`${idPrefix}-name`}>
        <span className="label">{t("Patient name")}</span>
        <input
          id={`${idPrefix}-name`}
          className="ob-input"
          value={value.name}
          onChange={set("name")}
          disabled={disabled}
          placeholder={t("Maple")}
          autoComplete="off"
        />
      </label>
      {(value.others ?? []).map((other, i) => (
        <div key={i} className="ob-field other-pet">
          <label className="label" htmlFor={`${idPrefix}-other-${i}`}>
            {t("Another animal in this visit")}
          </label>
          <div className="other-pet-row">
            <input
              id={`${idPrefix}-other-${i}`}
              className="ob-input"
              value={other}
              disabled={disabled}
              autoComplete="off"
              onChange={(e) => onChange({ ...value, others: (value.others ?? []).map((o, j) => (j === i ? e.target.value : o)) })}
            />
            <button
              type="button"
              className="icon-btn"
              disabled={disabled}
              aria-label={t("Remove {name}", { name: other || t("Another animal in this visit") })}
              onClick={() => onChange({ ...value, others: (value.others ?? []).filter((_, j) => j !== i) })}
            >
              ×
            </button>
          </div>
        </div>
      ))}
      <button
        type="button"
        className="link small add-pet"
        disabled={disabled}
        onClick={() => onChange({ ...value, others: [...(value.others ?? []), ""] })}
      >
        {t("Add another animal")}
      </button>
      <label className="ob-field" htmlFor={`${idPrefix}-last`}>
        <span className="label">{t("Last name")}</span>
        <input
          id={`${idPrefix}-last`}
          className="ob-input"
          value={value.last_name}
          onChange={set("last_name")}
          disabled={disabled}
          placeholder={t("Okonkwo")}
          autoComplete="off"
        />
      </label>
      <label className="ob-field" htmlFor={`${idPrefix}-reason`}>
        <span className="label">{t("Reason for visit")}</span>
        <textarea
          id={`${idPrefix}-reason`}
          className="ob-input ob-textarea"
          value={value.reason}
          onChange={set("reason")}
          disabled={disabled}
          placeholder={t("Limping on the right hind leg for two weeks")}
          rows={3}
        />
      </label>
    </div>
  );
}
