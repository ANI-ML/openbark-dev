import { Button } from "@openbark/ui";
import { useState } from "react";
import { useT } from "../i18n";

export interface ConsentNoticeProps {
  onAccept: () => void;
}

/**
 * Shown once per device before first use: the clinic is responsible for
 * getting the client's consent to record. Accepting it is remembered; the
 * per-recording consent step stays available in Settings.
 */
export function ConsentNotice({ onAccept }: ConsentNoticeProps) {
  const t = useT();
  const [checked, setChecked] = useState(false);
  return (
    <>
      <div className="ob-backdrop" />
      <div className="dialog" role="dialog" aria-modal="true" aria-labelledby="consent-notice-title">
        <h2 id="consent-notice-title">{t("Before you record")}</h2>
        <p>{t("OpenBark records conversations so it can write the medical record. Recording laws differ by place, and many require everyone in the conversation to agree.")}</p>
        <p>{t("You are responsible for telling clients that you're recording and getting their consent before you start, as your local rules require.")}</p>
        <p className="small muted">
          {t("Recordings and notes stay on this computer. You can record how each client consented by turning on the consent step in Settings.")}
        </p>
        <label className="check">
          <input type="checkbox" checked={checked} onChange={(e) => setChecked(e.target.checked)} />
          <span>{t("I understand, and I'll get consent from clients before recording.")}</span>
        </label>
        <div className="ask-actions">
          <Button variant="primary" disabled={!checked} onClick={onAccept}>
            {t("Accept and continue")}
          </Button>
        </div>
      </div>
    </>
  );
}
