import { Button, Dialog } from "@openbark/ui";
import { useContext, useState } from "react";
import { AppContext } from "../context";
import { useT } from "../i18n";
import { isIos } from "../visits";

export interface ConsentNoticeProps {
  onAccept: () => void;
}

/**
 * What the clinic is responsible for before recording, shared by this notice
 * and the iPhone walkthrough (so it is written, translated and shown once).
 */
export function ConsentText() {
  const t = useT();
  // The device's own word (ADR-0027): "this iPhone" on the iPhone profile.
  const ios = isIos(useContext(AppContext)?.status);
  return (
    <>
      <p>
        {t(
          "OpenBark records conversations so it can write the medical record. Recording laws differ by place, and many require everyone in the conversation to agree.",
        )}
      </p>
      <p>
        {t(
          "You are responsible for telling clients that you're recording and getting their consent before you start, as your local rules require.",
        )}
      </p>
      <p className="small muted">
        {ios
          ? t(
              "Recordings and notes stay on this iPhone. You can record how each client consented by turning on the consent step in Settings.",
            )
          : t(
              "Recordings and notes stay on this computer. You can record how each client consented by turning on the consent step in Settings.",
            )}
      </p>
    </>
  );
}

/** The agreement both the notice and the walkthrough ask for. */
export function ConsentCheck({ checked, onChange }: { checked: boolean; onChange: (on: boolean) => void }) {
  const t = useT();
  return (
    <label className="check">
      <input type="checkbox" checked={checked} onChange={(e) => onChange(e.target.checked)} />
      <span>{t("I understand, and I'll get consent from clients before recording.")}</span>
    </label>
  );
}

/**
 * Shown once per device before first use: the clinic is responsible for
 * getting the client's consent to record. Accepting it is remembered; the
 * per-recording consent step stays available in Settings. On the iPhone the
 * first-run walkthrough asks for it instead.
 */
export function ConsentNotice({ onAccept }: ConsentNoticeProps) {
  const t = useT();
  const [checked, setChecked] = useState(false);
  return (
    <Dialog labelledBy="consent-notice-title" onClose={onAccept} dismissible={false}>
      {(close) => (
        <>
          <h2 id="consent-notice-title">{t("Before you record")}</h2>
          <ConsentText />
          <ConsentCheck checked={checked} onChange={setChecked} />
          <div className="ask-actions">
            <Button variant="primary" disabled={!checked} onClick={close}>
              {t("Accept and continue")}
            </Button>
          </div>
        </>
      )}
    </Dialog>
  );
}
