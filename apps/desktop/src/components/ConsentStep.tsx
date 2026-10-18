import { Button } from "@openbark/ui";
import { useEffect, useRef, useState } from "react";
import type { ConsentMethod, ConsentRule, RecordingConsent } from "../engine";
import { useT } from "../i18n";

export interface ConsentStepProps {
  rule: ConsentRule;
  script: string;
  /** Name from Settings; asked for here when empty. */
  signer: string;
  /** "record" before recording; "import" before importing a recording. */
  purpose: "record" | "import";
  onConfirm: (consent: RecordingConsent, signer: string) => void;
  onDecline: () => void;
  onCancel: () => void;
}

/** Labels are translated where shown. */
const METHODS: { value: ConsentMethod; label: string; rule?: ConsentRule }[] = [
  { value: "verbal", label: "Agreed out loud just now" },
  { value: "written", label: "Signed a consent form" },
  { value: "on_file", label: "Consent on file for this client" },
  { value: "clinician_only", label: "Only I consent (one-party rule)", rule: "one_party" },
];

/**
 * The optional per-recording consent step (Settings): records how the client
 * consented, saved with the encounter. Under an all-parties rule the client
 * must have agreed.
 */
export function ConsentStep({ rule, script, signer, purpose, onConfirm, onDecline, onCancel }: ConsentStepProps) {
  const t = useT();
  const methods = METHODS.filter((m) => !m.rule || m.rule === rule);
  const [method, setMethod] = useState<ConsentMethod>("verbal");
  const [name, setName] = useState(signer);
  const form = useRef<HTMLFormElement>(null);
  // On phones the step sits below the Record button; bring it into view.
  useEffect(() => {
    form.current?.scrollIntoView?.({ block: "nearest" });
  }, []);
  const clinicianOnly = method === "clinician_only";
  const confirm = () =>
    name.trim() &&
    onConfirm({ rule, method, confirmed_by: name.trim(), script: method === "verbal" ? script : null }, name.trim());

  return (
    <form
      ref={form}
      className="consent"
      aria-label={t("Consent to record")}
      onSubmit={(e) => {
        e.preventDefault();
        confirm();
      }}
      onKeyDown={(e) => e.key === "Escape" && onCancel()}
    >
      <h2>{purpose === "record" ? t("Before recording") : t("Consent for this recording")}</h2>
      {purpose === "record" && (
        <blockquote className="consent-script">
          <span className="label muted">{t("Read to the client")}</span>
          <p>{script}</p>
        </blockquote>
      )}
      <fieldset className="consent-methods">
        <legend className="label">{t("How did the client agree?")}</legend>
        {methods.map((m) => (
          <label key={m.value} className="check">
            <input
              type="radio"
              name="consent-method"
              value={m.value}
              checked={method === m.value}
              onChange={() => setMethod(m.value)}
            />
            <span>{t(m.label)}</span>
          </label>
        ))}
      </fieldset>
      {!signer.trim() && (
        <label className="ob-field narrow">
          <span className="label">{t("Your name")}</span>
          <input className="ob-input" value={name} onChange={(e) => setName(e.target.value)} placeholder={t("Dr. A. Patel")} autoComplete="name" />
          <span className="ob-help">{t("Saved in Settings and with the encounter.")}</span>
        </label>
      )}
      <p className="small faint">
        {rule === "all_parties"
          ? t("Your clinic's rule: everyone on the recording agrees. Change it in Settings if your local rules differ.")
          : t("Your clinic's rule: one party to the conversation may consent. Check that this matches your local rules.")}
      </p>
      <div className="ask-actions">
        <Button autoFocus type="submit" variant="primary" disabled={!name.trim()}>
          {clinicianOnly
            ? purpose === "record" ? t("Start recording") : t("Choose the recording")
            : purpose === "record" ? t("Client agreed, start recording") : t("Client agreed, choose the recording")}
        </Button>
        {!clinicianOnly && (
          <Button onClick={onDecline}>{t("Client declined")}</Button>
        )}
        <Button variant="ghost" onClick={onCancel}>
          {t("Cancel")}
        </Button>
      </div>
    </form>
  );
}
