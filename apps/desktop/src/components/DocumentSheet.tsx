import { Button } from "@openbark/ui";
import { ArrowClockwise, Copy, FilePdf, X } from "@phosphor-icons/react";
import { useEffect } from "react";
import type { DocumentView } from "../engine";
import { templateName, useT } from "../i18n";
import { NoteView } from "./NoteView";

export interface DocumentSheetProps {
  doc: DocumentView;
  busy: boolean;
  progress: number;
  /** Copy the whole document in the chosen copy format. */
  onCopy: (doc: DocumentView) => void;
  onPdf: () => void;
  onRedraft: () => void;
  onClose: () => void;
  onEvidence: (ids: number[] | null) => void;
  onPlay: (ids: number[]) => void;
}

/** A client document (discharge instructions, a summary) in a sheet (DESIGN.md §5). */
export function DocumentSheet({ doc, busy, progress, onCopy, onPdf, onRedraft, onClose, onEvidence, onPlay }: DocumentSheetProps) {
  const t = useT();
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const ready = Boolean(doc.run && doc.template);
  const name = templateName(t, doc.name);
  return (
    <>
      <div className="ob-backdrop" onClick={onClose} />
      <aside className="ob-sheet" role="dialog" aria-modal="true" aria-label={name}>
        <header className="sheet-head">
          <div>
            <h2>{name}</h2>
            <span className="small faint mono">{doc.created_at}</span>
          </div>
          <button type="button" className="icon-btn" aria-label={t("Close")} onClick={onClose}>
            <X size={20} aria-hidden />
          </button>
        </header>
        <div className="sheet-body">
          <p className="ob-notice">{t("For the client. Read it through before handing it over: it's a draft written from the visit.")}</p>
          {busy && (
            <div className="ob-progress" role="progressbar" aria-label={t("Drafting {name}", { name })}>
              <span style={{ transform: `scaleX(${progress})` }} />
            </div>
          )}
          {doc.run && doc.template ? (
            <NoteView template={doc.template} run={doc.run} onEvidence={onEvidence} onPlay={onPlay} onCopySection={() => onCopy(doc)} />
          ) : (
            <p className="small muted">{t("Writing the draft.")}</p>
          )}
        </div>
        <footer className="output-row">
          <Button variant="primary" onClick={() => onCopy(doc)} disabled={!ready}>
            <Copy size={20} aria-hidden />
            {t("Copy")}
          </Button>
          <Button variant="ghost" onClick={onPdf} disabled={!ready || busy}>
            <FilePdf size={20} aria-hidden />
            {t("Save PDF")}
          </Button>
          <Button variant="ghost" onClick={onRedraft} disabled={busy}>
            <ArrowClockwise size={20} aria-hidden />
            {t("Draft again")}
          </Button>
        </footer>
      </aside>
    </>
  );
}
