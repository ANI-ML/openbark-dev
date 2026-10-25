import { ThemeToggle } from "@openbark/ui";
import {
  Box,
  ChevronLeft,
  ChevronRight,
  CircleHelp,
  FileText,
  Info,
  Languages,
  Lock,
  Mic,
  NotebookText,
  Phone,
  RefreshCw,
  Trash2,
  Users,
} from "lucide-react";
import { useEffect, useState, type ReactNode } from "react";
import { DEFAULT_CONSENT_SCRIPT, useApp } from "../context";
import * as engine from "../engine";
import { NOTE_LANGUAGES, UI_LANGUAGES, useT } from "../i18n";
import { DeletedVisits } from "../components/DeletedVisits";
import { MicCheck } from "../components/MicCheck";
import { PhoneDrafting } from "../components/PhoneDrafting";
import { PhoneSettings } from "../components/PhoneSettings";
import { StorageSection } from "../components/StorageSection";
import { SyncSection } from "../components/SyncSection";
import { TeamSettings } from "../components/TeamSettings";
import type { Route, SettingsSection } from "../routes";
import { PHONE, useMediaQuery } from "../useMediaQuery";
import { isIos } from "../visits";

/** Section titles; rendered through t() (scripts/i18n-keys.py INDIRECT). */
const TITLE: Record<SettingsSection, string> = {
  you: "You and your team",
  recording: "Recording and consent",
  language: "Language and appearance",
  phone: "Phone calls",
  pdf: "PDFs and copying",
  sync: "Encrypted sync",
  privacy: "Privacy and storage",
  deleted: "Deleted visits",
  about: "About",
};

/**
 * Settings as sections (ADR-0027). Desktop: the section list on the left and
 * one section at a time on the right, 720 px at most. Phones: a grouped list
 * of short rows, each opening its own screen with a back link. The iPhone
 * profile leaves out what only a computer does: phone calls through Twilio,
 * PDFs and letterhead, folders, and the key from a setting.
 */
export function SettingsScreen({ section }: { section?: SettingsSection } = {}) {
  const app = useApp();
  const t = useT();
  const phone = useMediaQuery(PHONE);
  const ios = isIos(app.status);
  const sections = (Object.keys(TITLE) as SettingsSection[]).filter(
    (s) => !(ios && s === "phone") && !(s === "deleted" && !app.can.notes),
  );
  const title = (s: SettingsSection) => (ios && s === "pdf" ? t("Copying") : t(TITLE[s]));

  if (phone && !section) return <PhoneList title={title} />;
  const current = section && sections.includes(section) ? section : "you";

  if (phone) {
    return (
      <div className="screen settings settings-sub">
        <header className="screen-head sub-head">
          <button type="button" className="back-top" onClick={() => app.navigate({ screen: "settings" })}>
            <ChevronLeft size={22} aria-hidden />
            {t("Settings")}
          </button>
          <h1>{title(current)}</h1>
        </header>
        <div className="settings-pane" data-section={current}>
          <SectionBody section={current} />
        </div>
      </div>
    );
  }

  return (
    <div className="screen settings">
      <header className="screen-head">
        <h1>{t("Settings")}</h1>
      </header>
      <div className="settings-layout">
        <nav className="settings-nav" aria-label={t("Settings sections")}>
          <ul>
            {sections.map((s) => (
              <li key={s}>
                <button
                  type="button"
                  className="settings-nav-item"
                  aria-current={s === current ? "page" : undefined}
                  onClick={() => app.navigate({ screen: "settings", section: s })}
                >
                  {title(s)}
                </button>
              </li>
            ))}
          </ul>
        </nav>
        <section className="settings-pane" data-section={current} aria-labelledby="settings-section-title">
          <h2 id="settings-section-title" className="settings-title">
            {title(current)}
          </h2>
          <SectionBody section={current} />
        </section>
      </div>
    </div>
  );
}

/** One row of the phone list: an icon, a label, an optional value or subtitle, a chevron. */
function Row({
  icon,
  label,
  value,
  sub,
  to,
}: {
  icon: ReactNode;
  label: string;
  value?: string;
  sub?: string;
  to: Route;
}) {
  const app = useApp();
  return (
    <li>
      <button type="button" className="inset-row settings-row" onClick={() => app.navigate(to)}>
        <span className="inset-icon" aria-hidden>
          {icon}
        </span>
        <span className="inset-text">
          <span>{label}</span>
          {sub && <span className="small muted">{sub}</span>}
        </span>
        {value && <span className="inset-value">{value}</span>}
        <ChevronRight size={18} aria-hidden className="inset-chevron" />
      </button>
    </li>
  );
}

/** Settings on a phone: grouped rows (ADR-0027). Every section stays reachable. */
function PhoneList({ title }: { title: (s: SettingsSection) => string }) {
  const app = useApp();
  const t = useT();
  const ios = isIos(app.status);
  const [sync, setSync] = useState<engine.SyncStatus | null>(null);
  useEffect(() => {
    engine.syncStatus().then(setSync, () => {});
  }, []);
  const noteLanguage = NOTE_LANGUAGES.find((l) => l.code === app.settings.noteLanguage)?.name ?? "";
  const syncSub =
    sync?.phase === "active"
      ? t("On")
      : ios
        ? t("Off. Visits stay on this iPhone")
        : t("Off. Visits stay on this computer");
  const s = (section: SettingsSection): Route => ({ screen: "settings", section });

  return (
    <div className="screen settings settings-list">
      <header className="screen-head">
        <h1>{t("Settings")}</h1>
      </header>
      <section className="inset-group" aria-labelledby="set-notes">
        <h2 className="inset-title" id="set-notes">
          {t("Notes")}
        </h2>
        <ul className="inset-list">
          <Row icon={<NotebookText size={20} />} label={t("Templates")} to={{ screen: "templates" }} />
          <Row icon={<Languages size={20} />} label={t("Note language")} value={noteLanguage} to={s("language")} />
          {ios && <PhoneDrafting />}
        </ul>
      </section>
      <section className="inset-group" aria-labelledby="set-device">
        <h2 className="inset-title" id="set-device">
          {ios ? t("On this iPhone") : t("On this computer")}
        </h2>
        <ul className="inset-list">
          <Row icon={<Box size={20} />} label={t("Models and storage")} to={{ screen: "models" }} />
          <Row icon={<Mic size={20} />} label={t("Microphone and consent")} to={s("recording")} />
          <Row icon={<RefreshCw size={20} />} label={t("Encrypted sync")} sub={syncSub} to={s("sync")} />
          <Row icon={<Lock size={20} />} label={title("privacy")} to={s("privacy")} />
        </ul>
      </section>
      <section className="inset-group" aria-labelledby="set-clinic">
        <h2 className="inset-title" id="set-clinic">
          {t("You and the clinic")}
        </h2>
        <ul className="inset-list">
          <Row icon={<Users size={20} />} label={title("you")} to={s("you")} />
          <Row icon={<Languages size={20} />} label={title("language")} to={s("language")} />
          {!ios && <Row icon={<Phone size={20} />} label={title("phone")} to={s("phone")} />}
          <Row icon={<FileText size={20} />} label={title("pdf")} to={s("pdf")} />
          {app.can.notes && <Row icon={<Trash2 size={20} />} label={title("deleted")} to={s("deleted")} />}
        </ul>
      </section>
      <section className="inset-group" aria-labelledby="set-help">
        <h2 className="inset-title" id="set-help">
          {t("Help")}
        </h2>
        <ul className="inset-list">
          <Row icon={<CircleHelp size={20} />} label={t("Guides and videos")} to={{ screen: "help" }} />
          <Row icon={<Info size={20} />} label={t("About OpenBark")} value={app.status?.version} to={s("about")} />
        </ul>
      </section>
    </div>
  );
}

/** One section's settings: the same on a computer and a phone. */
function SectionBody({ section }: { section: SettingsSection }) {
  const app = useApp();
  const t = useT();
  const s = app.settings;
  const ios = isIos(app.status);
  const [key, setKey] = useState<string | null>(null);
  const [encrypting, setEncrypting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [syncOn, setSyncOn] = useState(false);
  const enc = app.status?.encryption;

  useEffect(() => {
    if (section !== "about") return;
    engine.syncStatus().then((st) => setSyncOn(st.phase === "active"), () => {});
  }, [section]);

  async function encryptNow() {
    setError(null);
    setEncrypting(true);
    try {
      const n = await engine.encryptExisting();
      app.toast(t("{n} visits encrypted", { n }));
      app.refreshStatus();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setEncrypting(false);
    }
  }

  async function showKey() {
    setError(null);
    try {
      setKey(await engine.recoveryKey());
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }

  const toggle = (label: string, help: string | null, checked: boolean, onChange: (v: boolean) => void) => (
    <label className="opt-row">
      <span className="opt-text">
        <span>{label}</span>
        {help && <span className="small muted">{help}</span>}
      </span>
      <input type="checkbox" className="opt-switch" checked={checked} onChange={(e) => onChange(e.target.checked)} />
    </label>
  );

  switch (section) {
    case "you":
      return (
        <>
          {!app.status?.team && (
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
          )}
          <TeamSettings />
        </>
      );

    case "recording":
      return (
        <>
          <section className="group" aria-label={t("Microphone")}>
            <div className="narrow">
              <MicCheck id="settings-mic" />
            </div>
            {!engine.isDesktop && (
              <p className="small muted">
                {t("In the browser, microphone names appear once you allow the microphone (press Test microphone).")}
              </p>
            )}
          </section>
          <div className="opt-list">
            {toggle(
              t("Label speakers"),
              null,
              s.labelSpeakers,
              (v) => app.setSettings({ labelSpeakers: v }),
            )}
            {toggle(
              t("Draft the note when I stop recording"),
              ios ? t("The iPhone stays awake while it writes.") : t("This computer stays awake while it writes."),
              s.draftOnStop,
              (v) => app.setSettings({ draftOnStop: v }),
            )}
            {toggle(
              t("Record how the client consented, each time I record"),
              t("Asks before each recording and saves the answer with the visit."),
              s.consentEachTime,
              (v) => app.setSettings({ consentEachTime: v }),
            )}
          </div>
          {s.consentEachTime && (
            <section className="group">
              <h2>{t("Consent to record")}</h2>
              <fieldset className="consent-methods narrow">
                <legend className="label">{t("Your clinic's rule")}</legend>
                <label className="check">
                  <input
                    type="radio"
                    name="consent-rule"
                    checked={s.consentRule === "all_parties"}
                    onChange={() => app.setSettings({ consentRule: "all_parties" })}
                  />
                  <span>{t("Everyone on the recording agrees (recommended)")}</span>
                </label>
                <label className="check">
                  <input
                    type="radio"
                    name="consent-rule"
                    checked={s.consentRule === "one_party"}
                    onChange={() => app.setSettings({ consentRule: "one_party" })}
                  />
                  <span>{t("One party to the conversation may consent")}</span>
                </label>
              </fieldset>
              <label className="ob-field">
                <span className="label">{t("What to read to the client")}</span>
                <textarea
                  className="ob-input tb-text measure"
                  rows={3}
                  value={s.consentScript}
                  onChange={(e) => app.setSettings({ consentScript: e.target.value })}
                />
              </label>
              {s.consentScript !== DEFAULT_CONSENT_SCRIPT && (
                <button
                  type="button"
                  className="link small"
                  onClick={() => app.setSettings({ consentScript: DEFAULT_CONSENT_SCRIPT })}
                >
                  {t("Use the default wording")}
                </button>
              )}
              <p className="small faint measure">{t("The consent is saved with each visit and shown in Review.")}</p>
            </section>
          )}
          <p className="small muted measure">
            {ios ? t("Recordings stay on this iPhone.") : t("Recordings stay on this computer.")}{" "}
            {s.consentAcknowledgedAt
              ? t("You accepted the consent notice on {date}: you get clients' consent before recording.", {
                  date: engine.fmtStamp(s.consentAcknowledgedAt),
                })
              : t("You haven't accepted the consent notice on this device yet.")}{" "}
            <button type="button" className="link small" onClick={() => app.setSettings({ consentAcknowledgedAt: null })}>
              {t("Show the notice again")}
            </button>
          </p>
        </>
      );

    case "language":
      return (
        <>
          <section className="group">
            <h2>{t("Language")}</h2>
            <label className="ob-field narrow">
              <span className="label">{t("App language")}</span>
              <select
                className="ob-select"
                value={s.uiLanguage}
                onChange={(e) => app.setSettings({ uiLanguage: e.target.value })}
              >
                {UI_LANGUAGES.map((l) => (
                  <option key={l.code} value={l.code}>
                    {l.name}
                  </option>
                ))}
              </select>
            </label>
            <label className="ob-field narrow">
              <span className="label">{t("Write notes in")}</span>
              <select
                className="ob-select"
                value={s.noteLanguage}
                onChange={(e) => app.setSettings({ noteLanguage: e.target.value })}
              >
                {NOTE_LANGUAGES.map((l) => (
                  <option key={l.code} value={l.code}>
                    {l.name}
                  </option>
                ))}
              </select>
              <span className="ob-help">
                {t(
                  "Consultations can be in English or most European languages; notes are written in the language you choose here.",
                )}
              </span>
            </label>
            <p className="small faint measure">
              {t(
                "Translations of the app are new; tell us if something reads oddly. Number checks are most reliable when the consultation is in English.",
              )}
            </p>
          </section>
          <section className="group">
            <h2>{t("Appearance")}</h2>
            <ThemeToggle translate={t} />
          </section>
        </>
      );

    case "phone":
      return <PhoneSettings />;

    case "pdf":
      return (
        <section className="group">
          <h2>{ios ? t("Copying") : t("PDFs and copying")}</h2>
          {!ios && (
            <>
              <label className="ob-field narrow">
                <span className="label">{t("Clinic name (letterhead)")}</span>
                <input
                  className="ob-input"
                  value={s.clinicName}
                  onChange={(e) => app.setSettings({ clinicName: e.target.value })}
                  placeholder="Maple Street Animal Hospital"
                />
              </label>
              <label className="ob-field narrow">
                <span className="label">{t("Address, phone and email, one per line")}</span>
                <textarea
                  className="ob-input tb-text"
                  rows={3}
                  value={s.clinicLines}
                  onChange={(e) => app.setSettings({ clinicLines: e.target.value })}
                />
              </label>
              <label className="ob-field narrow">
                <span className="label">{t("Paper size")}</span>
                <select
                  className="ob-select"
                  value={s.pageSize}
                  onChange={(e) => app.setSettings({ pageSize: e.target.value === "a4" ? "a4" : "letter" })}
                >
                  <option value="letter">{t("Letter")}</option>
                  <option value="a4">A4</option>
                </select>
              </label>
            </>
          )}
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
            <input
              type="checkbox"
              checked={s.copyCitations}
              onChange={(e) => app.setSettings({ copyCitations: e.target.checked })}
            />
            <span>{t("Include transcript line numbers when copying")}</span>
          </label>
        </section>
      );

    case "sync":
      return <SyncSection />;

    case "privacy":
      return (
        <>
          <section className="group">
            <h2>{t("Encryption")}</h2>
            {error && (
              <p className="ob-notice" data-tone="danger" role="alert">
                {error}
              </p>
            )}
            {enc?.enabled ? (
              <>
                <p className="small measure">
                  <span className="ob-dot" data-tone="ok" aria-hidden />{" "}
                  {ios
                    ? t("Recordings, transcripts and notes are encrypted on this iPhone.")
                    : t("Recordings, transcripts and notes are encrypted on this computer.")}{" "}
                  {ios
                    ? t("The key is kept in this iPhone's protected storage.")
                    : enc.source === "keychain"
                      ? t("The key is kept in this computer's keychain.")
                      : enc.source === "environment"
                        ? t("The key comes from the OPENBARK_DATA_KEY setting.")
                        : t(
                            "The key is kept in a file in your user folder, because this computer's keychain isn't available.",
                          )}
                </p>
                {(app.status?.plain_encounters ?? 0) > 0 && (
                  <p className="ob-notice">
                    {t("{n} visits were saved before encryption and are still unencrypted.", {
                      n: app.status?.plain_encounters ?? 0,
                    })}{" "}
                    <button type="button" className="link" onClick={() => void encryptNow()} disabled={encrypting}>
                      {encrypting ? t("Encrypting") : t("Encrypt them now")}
                    </button>
                  </p>
                )}
                <div className="ob-field">
                  <span className="label">{t("Recovery key")}</span>
                  <p className="small muted measure">
                    {ios
                      ? t(
                          "Without this key, encrypted visits can't be opened on another device or after reinstalling. Keep it in your clinic's password manager.",
                        )
                      : t(
                          "Without this key, encrypted visits can't be opened on another computer or after reinstalling. Print it or keep it in your clinic's password manager.",
                        )}
                  </p>
                  {key ? (
                    <div className="recovery-key">
                      <code>{key}</code>
                      <button
                        type="button"
                        className="link small"
                        onClick={() =>
                          void navigator.clipboard.writeText(key).then(() => app.toast(t("Recovery key copied")))
                        }
                      >
                        {t("Copy")}
                      </button>
                      <button type="button" className="link small" onClick={() => setKey(null)}>
                        {t("Hide")}
                      </button>
                    </div>
                  ) : (
                    <button type="button" className="link small" onClick={() => void showKey()}>
                      {t("Show the recovery key")}
                    </button>
                  )}
                </div>
              </>
            ) : (
              <p className="ob-notice" data-tone="danger">
                {t(
                  "Encryption isn't running, so new visits are saved unencrypted. Restart OpenBark; if this stays, check the log.",
                )}
              </p>
            )}
          </section>
          {!ios && <StorageSection />}
        </>
      );

    case "deleted":
      return app.can.notes ? <DeletedVisits /> : null;

    case "about":
      return (
        <section className="group">
          <h2>{t("About")}</h2>
          <p className="small muted">
            OpenBark <span className="mono">{app.status?.version}</span>.{" "}
            {t("Notes are drafts until a veterinarian signs them.")}
          </p>
          {/* §8 offline notice: where OpenBark states its privacy. */}
          <p className="ob-notice" data-tone="ok">
            {syncOn
              ? t("Running locally. Encrypted sync is on: only encrypted copies of transcripts and notes leave this device, never recordings.")
              : t("Running locally. Nothing leaves this device.")}
          </p>
        </section>
      );
  }
}
