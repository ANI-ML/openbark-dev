import { Button } from "@openbark/ui";
import { ArrowClockwise, Copy, X } from "@phosphor-icons/react";
import { useEffect } from "react";
import type { DocumentView } from "../engine";
import { NoteView } from "./NoteView";

export interface DocumentSheetProps {
  doc: DocumentView;
  busy: boolean;
  progress: number;
  onCopy: (text: string) => void;
  onRedraft: () => void;
  onClose: () => void;
  onEvidence: (ids: number[] | null) => void;
  onPlay: (ids: number[]) => void;
}

/** A client document (discharge instructions, a summary) in a sheet (DESIGN.md §5). */
export function DocumentSheet({ doc, busy, progress, onCopy, onRedraft, onClose, onEvidence, onPlay }: DocumentSheetProps) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  return (
    <>
      <div className="ob-backdrop" onClick={onClose} />
      <aside className="ob-sheet" role="dialog" aria-modal="true" aria-label={doc.name}>
        <header className="sheet-head">
          <div>
            <h2>{doc.name}</h2>
            <span className="small faint mono">{doc.created_at}</span>
          </div>
          <button type="button" className="icon-btn" aria-label="Close" onClick={onClose}>
            <X size={20} aria-hidden />
          </button>
        </header>
        <div className="sheet-body">
          <p className="ob-notice">For the client. Read it through before handing it over: it's a draft written from the visit.</p>
          {busy && (
            <div className="ob-progress" role="progressbar" aria-label={`Drafting ${doc.name}`}>
              <span style={{ transform: `scaleX(${progress})` }} />
            </div>
          )}
          {doc.run && doc.template ? (
            <NoteView template={doc.template} run={doc.run} onEvidence={onEvidence} onPlay={onPlay} onCopySection={(t, x) => onCopy(`${t}\n${x}`)} />
          ) : (
            <p className="small muted">Writing the draft.</p>
          )}
        </div>
        <footer className="output-row">
          <Button variant="primary" onClick={() => onCopy(doc.markdown ?? "")} disabled={!doc.markdown}>
            <Copy size={20} aria-hidden />
            Copy {doc.name.toLowerCase()}
          </Button>
          <Button variant="ghost" onClick={onRedraft} disabled={busy}>
            <ArrowClockwise size={20} aria-hidden />
            Draft again
          </Button>
        </footer>
      </aside>
    </>
  );
}
