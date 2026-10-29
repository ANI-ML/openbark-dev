import { Button } from "@openbark/ui";
import { fmtBytes, useApp } from "../context";
import type * as engine from "../engine";
import { useT } from "../i18n";
import { isIos } from "../visits";

/**
 * The note model chosen on this computer may not fit its memory, and a
 * smaller one would (0.2.3, ADR-0030): say so, with the download size and
 * what the smaller model does less well, and offer the switch. Nothing
 * changes and nothing downloads until the person chooses.
 */
export function NoteModelAdvice({
  rows,
  onDownload,
  onOpenModels,
}: {
  /** The catalog rows (engine.models()). */
  rows: engine.ModelRow[] | null;
  /** Download the suggested model here (the Models screen). */
  onDownload?: (id: string) => void;
  /** Go to Models instead (the Record screen). */
  onOpenModels?: () => void;
}) {
  const app = useApp();
  const t = useT();
  const advice = noteModelAdvice(rows, app.settings.llm, app.status);
  if (!advice) return null;
  const { current, suggested } = advice;
  return (
    <div className="ob-notice note-model-advice" role="status">
      <p>
        {t(
          "{current} may not fit this computer's memory, so drafts can be very slow or stop. {suggested} is smaller and drafts faster.",
          { current: current.name, suggested: suggested.name },
        )}{" "}
        {t(
          "In OpenBark's tests it found slightly fewer facts (90% against 95%) and wrote two numbers wrong across 23 visits, so check doses and weights closely.",
        )}
      </p>
      <div className="ask-actions">
        {suggested.installed ? (
          <Button size="compact" onClick={() => app.setSettings({ llm: suggested.id })}>
            {t("Use {model}", { model: suggested.name })}
          </Button>
        ) : onDownload ? (
          <Button size="compact" onClick={() => onDownload(suggested.id)}>
            {t("Download {model} ({size})", { model: suggested.name, size: fmtBytes(suggested.size_bytes) })}
          </Button>
        ) : (
          onOpenModels && (
            <Button size="compact" variant="ghost" onClick={onOpenModels}>
              {t("{size} download. Open Models", { size: fmtBytes(suggested.size_bytes) })}
            </Button>
          )
        )}
      </div>
    </div>
  );
}

/**
 * The suggestion, if any: the chosen note model won't fit, and the default
 * for this computer is a different, smaller built-in one. Never on a phone
 * (its own tiers decide there).
 */
export function noteModelAdvice(
  rows: engine.ModelRow[] | null,
  chosen: string,
  status: engine.Status | null,
): { current: engine.ModelRow; suggested: engine.ModelRow } | null {
  if (!rows || !status || isIos(status)) return null;
  const current = rows.find((r) => r.id === chosen);
  const suggested = rows.find((r) => r.id === status.default_llm);
  if (!current || !suggested || current.id === suggested.id) return null;
  if (current.fit !== "wont_fit" || suggested.size_bytes >= current.size_bytes) return null;
  return { current, suggested };
}
