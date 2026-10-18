import { Button, Sheet } from "@openbark/ui";
import { Copy, FileDown, RotateCw, X } from "lucide-react";
import type { ReactNode } from "react";
import type { DocumentView } from "../engine";
import { NOTE_LANGUAGES, templateName, useT } from "../i18n";
import { NoteView } from "./NoteView";

export interface DocumentSheetProps {
  doc: DocumentView;
  busy: boolean;
  /** The drafting board shown while the document is written. */
  board: ReactNode;
  /** Copy the whole document in the chosen copy format. */
  onCopy: (doc: DocumentView) => void;
  onPdf: () => void;
  onRedraft: () => void;
  /** The client's language for this document (drafts again when changed). */
  language: string;
  onLanguage: (code: string) => void;
  onClose: () => void;
  onEvidence: (ids: number[] | null) => void;
  onPlay: (ids: number[]) => void;
}

/** A client document (discharge instructions, a summary) in a sheet (DESIGN.md §5). */
export function DocumentSheet({
  doc,
  busy,
  board,
  onCopy,
  onPdf,
  onRedraft,
  language,
  onLanguage,
  onClose,
  onEvidence,
  onPlay,
}: DocumentSheetProps) {
  const t = useT();

  const ready = Boolean(doc.run && doc.template);
  const name = templateName(t, doc.name);
  return (
    <Sheet label={name} onClose={onClose}>
      {(close) => (
        <>
          <header className="sheet-head">
            <div>
              <h2>{name}</h2>
              <span className="small faint mono">{doc.created_at}</span>
            </div>
            <label className="doc-language">
              <span className="small muted">{t("Client's language")}</span>
              <select
                className="ob-select"
                value={language}
                disabled={busy}
                onChange={(e) => onLanguage(e.target.value)}
              >
                {NOTE_LANGUAGES.map((l) => (
                  <option key={l.code} value={l.code}>
                    {l.name}
                  </option>
                ))}
              </select>
            </label>
            <button type="button" className="icon-btn" aria-label={t("Close")} onClick={close}>
              <X size={20} aria-hidden />
            </button>
          </header>
          <div className="sheet-body">
            <p className="ob-notice">
              {t("For the client. Read it through before handing it over: it's a draft written from the visit.")}
            </p>
            {busy && board}
            {doc.run && doc.template ? (
              <NoteView
                template={doc.template}
                run={doc.run}
                onEvidence={onEvidence}
                onPlay={onPlay}
                onCopySection={() => onCopy(doc)}
              />
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
              <FileDown size={20} aria-hidden />
              {t("Save PDF")}
            </Button>
            <Button variant="ghost" onClick={onRedraft} disabled={busy}>
              <RotateCw size={20} aria-hidden />
              {t("Draft again")}
            </Button>
          </footer>
        </>
      )}
    </Sheet>
  );
}
