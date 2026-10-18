import { Button, Dialog } from "@openbark/ui";
import { Trash2 } from "lucide-react";
import { useState } from "react";
import { useT } from "../i18n";

/**
 * "Delete this recording? The note stays." (brand guide §8, ADR-0022
 * section 4). Only `audio.wav` goes, at once and for good; the transcript,
 * drafts, signed notes and addenda stay. A dialog, because it can't be undone.
 */
export function DeleteRecordingDialog({
  signed,
  multiPet,
  importedFile,
  onConfirm,
  onClose,
}: {
  /** Any note in the visit is signed. */
  signed: boolean;
  multiPet: boolean;
  /** The imported file's name; null for a live recording. */
  importedFile: string | null;
  /** Resolves when the recording is deleted; rejects (with a message to show) to keep the dialog open. */
  onConfirm: () => Promise<void>;
  onClose: () => void;
}) {
  const t = useT();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  return (
    <Dialog labelledBy="delete-recording-title" onClose={onClose} dismissible={!busy} className="delete-dialog">
      {(close) => (
        <>
          <h2 id="delete-recording-title">{t("Delete this recording? The note stays.")}</h2>
          <p>
            {t(
              "The transcript, drafts and signed notes are kept. You won't be able to play this visit back or transcribe it again.",
            )}
          </p>
          {!signed && <p>{t("The note isn't signed yet. Listen back to anything you want to check first.")}</p>}
          {multiPet && <p>{t("The recording covers every animal in this visit.")}</p>}
          {importedFile && (
            <p className="small muted">
              {t("The file you imported, {file}, is still where it was. Delete it there too if you need to.", {
                file: importedFile,
              })}
            </p>
          )}
          {error && (
            <p className="ob-notice" data-tone="danger" role="alert">
              {error}
            </p>
          )}
          <div className="ask-actions">
            <Button
              variant="danger"
              disabled={busy}
              onClick={async () => {
                setBusy(true);
                setError(null);
                try {
                  await onConfirm();
                  close();
                } catch (e) {
                  setError(e instanceof Error ? e.message : String(e));
                  setBusy(false);
                }
              }}
            >
              <Trash2 size={18} aria-hidden />
              {t("Delete recording")}
            </Button>
            <Button variant="ghost" disabled={busy} onClick={close} autoFocus>
              {t("Keep recording")}
            </Button>
          </div>
        </>
      )}
    </Dialog>
  );
}
