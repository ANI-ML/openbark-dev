import { Button, Sheet } from "@openbark/ui";
import { X } from "lucide-react";
import { useEffect, useState } from "react";
import * as engine from "../engine";
import { useT } from "../i18n";

const KIND_LABEL: Record<string, string> = {
  statements: "Bullet points",
  text: "Text box",
  fields: "Measurements",
  medications: "Medications",
  subheadings: "Subheadings",
};

const FIELD_LABEL: Record<string, string> = {
  name: "Name",
  description: "Description",
  icon: "Icon",
  detail: "Level of detail",
  document: "Who it's for",
  instructions: "Instructions",
  rules: "Extra rules",
  fixed_rules: "Rules that always apply",
};

function show(value: unknown): string {
  if (value == null || value === "") return "";
  if (Array.isArray(value)) return value.join("\n");
  return String(value);
}

/** One change, in words: the old and new text side by side on desktop, stacked on phones. */
function Change({ change, left, right }: { change: engine.HeadingChange; left: string; right: string }) {
  const t = useT();
  switch (change.kind) {
    case "added":
      return <p className="small">{t("Added")}</p>;
    case "removed":
      return <p className="small">{t("Removed")}</p>;
    case "renamed":
      return <p className="small">{t("Renamed from {from}", { from: change.from })}</p>;
    case "moved":
      return <p className="small">{t("Moved")}</p>;
    case "kind_changed":
      return (
        <p className="small">
          {t("What goes under it: {from} to {to}", { from: t(KIND_LABEL[change.from] ?? change.from), to: t(KIND_LABEL[change.to] ?? change.to) })}
        </p>
      );
    case "instruction_changed":
      return (
        <div className="compare-pair">
          <span className="label muted">{t("Instruction changed")}</span>
          <div className="compare-sides">
            <div>
              <span className="small faint">{left}</span>
              <p className="small compare-text">{change.from ?? t("No instruction")}</p>
            </div>
            <div>
              <span className="small faint">{right}</span>
              <p className="small compare-text">{change.to ?? t("No instruction")}</p>
            </div>
          </div>
        </div>
      );
    case "measurements_changed":
      return (
        <p className="small">
          {[
            change.added.length ? t("Measurements added: {list}", { list: change.added.join(", ") }) : "",
            change.removed.length ? t("Measurements removed: {list}", { list: change.removed.join(", ") }) : "",
            ...change.renamed.map(([a, b]) => t("{from} renamed {to}", { from: a, to: b })),
          ]
            .filter(Boolean)
            .join(". ")}
        </p>
      );
    case "show_changed":
      return <p className="small">{t("Which species it's shown for changed")}</p>;
    case "examples_changed":
      return <p className="small">{t("Example requests changed")}</p>;
  }
}

/**
 * Compare with the original (ADR-0024 section 14.4): what changed in the
 * original since this version was made, or how this version differs from
 * the original now. Changes are grouped by heading, then the whole note.
 */
export function CompareSheet({
  templateId,
  hasBase,
  originalChanged,
  canKeep,
  onKeep,
  onUseOriginal,
  onClose,
}: {
  templateId: string;
  /** A copy of the original from when the version was made exists. */
  hasBase: boolean;
  originalChanged: boolean;
  canKeep: boolean;
  onKeep: () => void;
  onUseOriginal: () => void;
  onClose: () => void;
}) {
  const t = useT();
  const [view, setView] = useState<"base" | "original">(hasBase && originalChanged ? "base" : "original");
  const [diff, setDiff] = useState<engine.TemplateDiff | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [left, right] = view === "base" ? [t("When yours was made"), t("Now")] : [t("Original"), t("Yours")];

  useEffect(() => {
    setDiff(null);
    setError(null);
    engine.compareTemplate(templateId, view).then(setDiff, (e) => setError(String(e?.message ?? e)));
  }, [templateId, view]);

  return (
    <Sheet label={t("Compare with the original")} onClose={onClose} className="compare-sheet">
      {(close) => (
        <>
          <header className="sheet-head">
            <h2>{t("Compare with the original")}</h2>
            <button type="button" className="icon-btn" aria-label={t("Close")} onClick={close}>
              <X size={20} aria-hidden />
            </button>
          </header>
          <div className="sheet-body">
            {hasBase && (
              <div className="ob-segmented" role="tablist">
                <button className="ob-segment" role="tab" aria-selected={view === "base"} onClick={() => setView("base")}>
                  {t("What changed in the original")}
                </button>
                <button className="ob-segment" role="tab" aria-selected={view === "original"} onClick={() => setView("original")}>
                  {t("Yours and the original")}
                </button>
              </div>
            )}
            {error && (
              <p className="ob-notice" data-tone="danger" role="alert">
                {t(error)}
              </p>
            )}
            {!diff && !error && <div className="ob-skeleton" style={{ height: 120 }} />}
            {diff && diff.headings.length === 0 && diff.note.length === 0 && <p className="muted">{t("No differences.")}</p>}
            {diff && diff.headings.length > 0 && (
              <ul className="compare-list" aria-label={t("Headings")}>
                {diff.headings.map((h) => (
                  <li key={h.path}>
                    <span className="compare-title">{h.title}</span>
                    {h.changes.map((c, i) => (
                      <Change key={i} change={c} left={left} right={right} />
                    ))}
                  </li>
                ))}
              </ul>
            )}
            {diff && diff.note.length > 0 && (
              <ul className="compare-list" aria-label={t("The whole note")}>
                {diff.note.map((c) => (
                  <li key={c.field}>
                    <span className="compare-title">{t(FIELD_LABEL[c.field] ?? c.field)}</span>
                    <div className="compare-sides">
                      <div>
                        <span className="small faint">{left}</span>
                        <p className="small compare-text">{show(c.from) || t("Empty")}</p>
                      </div>
                      <div>
                        <span className="small faint">{right}</span>
                        <p className="small compare-text">{show(c.to) || t("Empty")}</p>
                      </div>
                    </div>
                  </li>
                ))}
              </ul>
            )}
            {(canKeep || originalChanged) && (
              <div className="dialog-actions">
                {canKeep && originalChanged && (
                  <Button
                    onClick={() => {
                      onKeep();
                      close();
                    }}
                  >
                    {t("Keep mine")}
                  </Button>
                )}
                <Button
                  variant="ghost"
                  onClick={() => {
                    onUseOriginal();
                    close();
                  }}
                >
                  {t("Use the original")}
                </Button>
              </div>
            )}
          </div>
        </>
      )}
    </Sheet>
  );
}
