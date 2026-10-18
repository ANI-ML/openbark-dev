import { Sheet } from "@openbark/ui";
import { RefreshCw, X } from "lucide-react";
import { useEffect, useState } from "react";
import * as engine from "../engine";
import { templateDescription, templateName, useT } from "../i18n";
import { COMMON_TYPES, TemplateIcon } from "../templateIcons";

/**
 * Draft a visit's note again from its recording: the same visit type (when a
 * draft went wrong) or another one. Earlier drafts are kept, so nothing is
 * lost by trying.
 */
export function ReprocessSheet({
  visit,
  onClose,
  onChoose,
}: {
  visit: engine.VisitSummary;
  onClose: () => void;
  onChoose: (templateId: string) => void;
}) {
  const t = useT();
  const [templates, setTemplates] = useState<engine.TemplateSummary[] | null>(null);

  useEffect(() => {
    engine.listTemplates().then(setTemplates, () => setTemplates([]));
  }, []);

  const records = (templates ?? []).filter((x) => x.document !== "client");
  const current = records.find((x) => x.id === visit.template_id);
  const order = (x: engine.TemplateSummary) => {
    const i = COMMON_TYPES.indexOf(x.id);
    return i >= 0 ? i : x.builtin ? 100 : 50;
  };
  const others = records
    .filter((x) => x !== current)
    .sort((a, b) => order(a) - order(b) || a.name.localeCompare(b.name));

  return (
    <Sheet label={t("Reprocess this visit")} onClose={onClose}>
      {(close) => (
        <>
          <header className="sheet-head">
            <h2>{t("Reprocess this visit")}</h2>
            <button type="button" className="icon-btn" aria-label={t("Close")} onClick={close}>
              <X size={20} aria-hidden />
            </button>
          </header>
          <div className="sheet-body reprocess">
            <p className="small muted">{t("Draft the note again from the recording. Earlier drafts are kept.")}</p>
            {templates === null ? (
              <div className="ob-skeleton" style={{ height: 160 }} />
            ) : (
              <>
                {current && (
                  <button type="button" className="reprocess-row again" onClick={() => onChoose(current.id)}>
                    <span className="type-icon">
                      <RefreshCw size={20} aria-hidden />
                    </span>
                    <span className="reprocess-text">
                      <span className="reprocess-name">{t("Draft again")}</span>
                      <span className="small muted">
                        {t("With the same type: {name}", { name: templateName(t, current.name) })}
                      </span>
                    </span>
                  </button>
                )}
                <h3 className="label">{t("Or as another type of visit")}</h3>
                <ul className="reprocess-list">
                  {others.map((x) => (
                    <li key={x.id}>
                      <button type="button" className="reprocess-row" onClick={() => onChoose(x.id)}>
                        <span className="type-icon">
                          <TemplateIcon template={x} size={20} />
                        </span>
                        <span className="reprocess-text">
                          <span className="reprocess-name">{templateName(t, x.name)}</span>
                          {x.description && <span className="small muted">{templateDescription(t, x)}</span>}
                        </span>
                      </button>
                    </li>
                  ))}
                </ul>
              </>
            )}
          </div>
        </>
      )}
    </Sheet>
  );
}
