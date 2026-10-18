import type { TemplateSummary } from "../engine";
import { templateName, useT } from "../i18n";
import { COMMON_TYPES, TemplateIcon } from "../templateIcons";

/**
 * What kind of visit this is, as icon tiles: the common visit types and the
 * team's own templates, with every other record template one step away
 * under "More types". Each type drafts with its own template.
 */
export function VisitTypePicker({
  templates,
  value,
  onChange,
  disabled,
}: {
  templates: TemplateSummary[];
  value: string;
  onChange: (id: string) => void;
  disabled?: boolean;
}) {
  const t = useT();
  const records = templates.filter((x) => x.document !== "client");
  const common = COMMON_TYPES.map((id) => records.find((x) => x.id === id)).filter((x): x is TemplateSummary => !!x);
  const team = records.filter((x) => !x.builtin);
  const tiles = [...common, ...team];
  const selected = records.find((x) => x.id === value);
  if (selected && !tiles.includes(selected)) tiles.push(selected);
  const others = records.filter((x) => x.builtin && !COMMON_TYPES.includes(x.id));

  return (
    <div className="visit-types">
      <div className="type-tiles" role="radiogroup" aria-label={t("Type of visit")}>
        {tiles.map((x) => (
          <button
            key={x.id}
            type="button"
            role="radio"
            aria-checked={x.id === value}
            className="type-tile"
            disabled={disabled}
            onClick={() => onChange(x.id)}
          >
            <span className="type-icon">
              <TemplateIcon template={x} size={20} />
            </span>
            <span className="type-name">{templateName(t, x.name)}</span>
          </button>
        ))}
      </div>
      {others.length > 0 && (
        <select
          className="ob-select type-more"
          aria-label={t("More types")}
          disabled={disabled}
          value={others.some((x) => x.id === value) ? value : ""}
          onChange={(e) => e.target.value && onChange(e.target.value)}
        >
          <option value="">{t("More types")}</option>
          {others.map((x) => (
            <option key={x.id} value={x.id}>
              {templateName(t, x.name)}
            </option>
          ))}
        </select>
      )}
    </div>
  );
}
