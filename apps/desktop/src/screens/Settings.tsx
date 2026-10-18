import { ThemeToggle } from "@openbark/ui";
import { useEffect, useState } from "react";
import { DEFAULT_CONSENT_SCRIPT, useApp } from "../context";
import * as engine from "../engine";
import { NOTE_LANGUAGES, UI_LANGUAGES, useT } from "../i18n";

/** DESIGN.md §6 Settings: hairline-separated groups. */
export function SettingsScreen() {
  const app = useApp();
  const t = useT();
  const s = app.settings;
  const [devices, setDevices] = useState<string[]>([]);

  useEffect(() => {
    if (engine.isDesktop) engine.inputDevices().then(setDevices, () => {});
  }, []);

  return (
    <div className="screen settings">
      <header className="screen-head">
        <h1>{t("Settings")}</h1>
      </header>

      <section className="group">
        <h2>{t("You")}</h2>
        <label className="ob-field narrow">
          <span className="label">{t("Name for signing notes")}</span>
          <input
            className="ob-input"
            value={s.signer}
            onChange={(e) => app.setSettings({ signer: e.target.value })}
            placeholder="Dr. A. Patel"
            autoComplete="name"
          />
          <span className="ob-help">{t("Saved with each note you sign.")}</span>
        </label>
      </section>

      <section className="group">
        <h2>{t("Language")}</h2>
        <label className="ob-field narrow">
          <span className="label">{t("App language")}</span>
          <select className="ob-select" value={s.uiLanguage} onChange={(e) => app.setSettings({ uiLanguage: e.target.value })}>
            {UI_LANGUAGES.map((l) => (
              <option key={l.code} value={l.code}>
                {l.name}
              </option>
            ))}
          </select>
        </label>
        <label className="ob-field narrow">
          <span className="label">{t("Write notes in")}</span>
          <select className="ob-select" value={s.noteLanguage} onChange={(e) => app.setSettings({ noteLanguage: e.target.value })}>
            {NOTE_LANGUAGES.map((l) => (
              <option key={l.code} value={l.code}>
                {l.name}
              </option>
            ))}
          </select>
          <span className="ob-help">
            {t("Consultations can be in English or most European languages; notes are written in the language you choose here.")}
          </span>
        </label>
        <p className="small faint measure">
          {t("Translations of the app are new; tell us if something reads oddly. Number checks are most reliable when the consultation is in English.")}
        </p>
      </section>

      <section className="group">
        <h2>{t("Appearance")}</h2>
        <ThemeToggle translate={t} />
      </section>

      <section className="group">
        <h2>{t("Recording")}</h2>
        {engine.isDesktop ? (
          <label className="ob-field narrow">
            <span className="label">{t("Microphone")}</span>
            <select className="ob-select" value={s.device ?? ""} onChange={(e) => app.setSettings({ device: e.target.value || null })}>
              <option value="">{t("System default")}</option>
              {devices.map((d) => (
                <option key={d} value={d}>
                  {d}
                </option>
              ))}
            </select>
          </label>
        ) : (
          <p className="small muted">{t("In the browser, the microphone is the one you allow when you first record.")}</p>
        )}
        <label className="check">
          <input type="checkbox" checked={s.labelSpeakers} onChange={(e) => app.setSettings({ labelSpeakers: e.target.checked })} />
          <span>{t("Label speakers")}</span>
        </label>
        <label className="check">
          <input type="checkbox" checked={s.draftOnStop} onChange={(e) => app.setSettings({ draftOnStop: e.target.checked })} />
          <span>{t("Draft the note when I stop recording")}</span>
        </label>
      </section>

      <section className="group">
        <h2>{t("Consent to record")}</h2>
        <p className="small muted measure">
          {s.consentAcknowledgedAt
            ? t("You accepted the consent notice on {date}: you get clients' consent before recording.", {
                date: engine.fmtStamp(s.consentAcknowledgedAt),
              })
            : t("You haven't accepted the consent notice on this device yet.")}{" "}
          <button type="button" className="link small" onClick={() => app.setSettings({ consentAcknowledgedAt: null })}>
            {t("Show the notice again")}
          </button>
        </p>
        <label className="check">
          <input type="checkbox" checked={s.consentEachTime} onChange={(e) => app.setSettings({ consentEachTime: e.target.checked })} />
          <span>{t("Record how the client consented, each time I record")}</span>
        </label>
        {s.consentEachTime && (
          <>
            <fieldset className="consent-methods narrow">
              <legend className="label">{t("Your clinic's rule")}</legend>
              <label className="check">
                <input type="radio" name="consent-rule" checked={s.consentRule === "all_parties"} onChange={() => app.setSettings({ consentRule: "all_parties" })} />
                <span>{t("Everyone on the recording agrees (recommended)")}</span>
              </label>
              <label className="check">
                <input type="radio" name="consent-rule" checked={s.consentRule === "one_party"} onChange={() => app.setSettings({ consentRule: "one_party" })} />
                <span>{t("One party to the conversation may consent")}</span>
              </label>
            </fieldset>
            <label className="ob-field">
              <span className="label">{t("What to read to the client")}</span>
              <textarea className="ob-input tb-text measure" rows={3} value={s.consentScript} onChange={(e) => app.setSettings({ consentScript: e.target.value })} />
            </label>
            {s.consentScript !== DEFAULT_CONSENT_SCRIPT && (
              <button type="button" className="link small" onClick={() => app.setSettings({ consentScript: DEFAULT_CONSENT_SCRIPT })}>
                {t("Use the default wording")}
              </button>
            )}
            <p className="small faint measure">{t("The consent is saved with each encounter and shown in Review.")}</p>
          </>
        )}
      </section>

      <section className="group">
        <h2>{t("PDFs and copying")}</h2>
        <label className="ob-field narrow">
          <span className="label">{t("Clinic name (letterhead)")}</span>
          <input className="ob-input" value={s.clinicName} onChange={(e) => app.setSettings({ clinicName: e.target.value })} placeholder="Maple Street Animal Hospital" />
        </label>
        <label className="ob-field narrow">
          <span className="label">{t("Address, phone and email, one per line")}</span>
          <textarea className="ob-input tb-text" rows={3} value={s.clinicLines} onChange={(e) => app.setSettings({ clinicLines: e.target.value })} />
        </label>
        <label className="ob-field narrow">
          <span className="label">{t("Paper size")}</span>
          <select className="ob-select" value={s.pageSize} onChange={(e) => app.setSettings({ pageSize: e.target.value === "a4" ? "a4" : "letter" })}>
            <option value="letter">{t("Letter")}</option>
            <option value="a4">A4</option>
          </select>
        </label>
        <label className="ob-field narrow">
          <span className="label">{t("Copy note as")}</span>
          <select
            className="ob-select"
            value={s.copyFormat}
            onChange={(e) => app.setSettings({ copyFormat: e.target.value as typeof s.copyFormat })}
          >
            <option value="plain">{t("Plain text (most PIMS)")}</option>
            <option value="formatted">{t("Formatted text (PIMS with rich text editors)")}</option>
            <option value="markdown">Markdown</option>
          </select>
        </label>
        <label className="check">
          <input type="checkbox" checked={s.copyCitations} onChange={(e) => app.setSettings({ copyCitations: e.target.checked })} />
          <span>{t("Include transcript line numbers when copying")}</span>
        </label>
      </section>

      <section className="group">
        <h2>{t("Storage")}</h2>
        <dl className="slots">
          <div>
            <dt className="label muted">{t("Encounters")}</dt>
            <dd className="mono">{app.status?.sessions_dir}</dd>
          </div>
          <div>
            <dt className="label muted">{t("Models")}</dt>
            <dd className="mono">{app.status?.models_dir}</dd>
          </div>
        </dl>
        <p className="small faint">{t("Recordings and notes stay on this computer. They aren't encrypted yet; that arrives in the next release.")}</p>
      </section>

      <section className="group">
        <h2>{t("About")}</h2>
        <p className="small muted">
          OpenBark <span className="mono">{app.status?.version}</span>. {t("Notes are drafts until a veterinarian signs them.")}
        </p>
      </section>
    </div>
  );
}
