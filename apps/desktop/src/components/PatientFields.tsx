import type { Patient } from "../engine";

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
  const set = (k: keyof Patient) => (e: React.ChangeEvent<HTMLInputElement | HTMLTextAreaElement>) =>
    onChange({ ...value, [k]: e.target.value });
  return (
    <div className="patient-fields">
      <label className="ob-field" htmlFor={`${idPrefix}-name`}>
        <span className="label">Patient name</span>
        <input
          id={`${idPrefix}-name`}
          className="ob-input"
          value={value.name}
          onChange={set("name")}
          disabled={disabled}
          placeholder="Maple"
          autoComplete="off"
        />
      </label>
      <label className="ob-field" htmlFor={`${idPrefix}-last`}>
        <span className="label">Last name</span>
        <input
          id={`${idPrefix}-last`}
          className="ob-input"
          value={value.last_name}
          onChange={set("last_name")}
          disabled={disabled}
          placeholder="Okonkwo"
          autoComplete="off"
        />
      </label>
      <label className="ob-field" htmlFor={`${idPrefix}-reason`}>
        <span className="label">Reason for visit</span>
        <textarea
          id={`${idPrefix}-reason`}
          className="ob-input ob-textarea"
          value={value.reason}
          onChange={set("reason")}
          disabled={disabled}
          placeholder="Limping on the right hind leg for two weeks"
          rows={3}
        />
      </label>
    </div>
  );
}
