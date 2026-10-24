import { Button, Sheet, StatusDot } from "@openbark/ui";
import { Copy, X } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useApp } from "../context";
import * as engine from "../engine";
import { useT } from "../i18n";
import { formatPhone } from "../phone/format";

const DEFAULT_ANNOUNCEMENT =
  "This call is recorded for your pet's medical record. The recording stays at the clinic. Is that all right with you?";

/**
 * Settings > Phone calls (ADR-0023, docs/plans/telephony.md "UI flows and
 * copy"): connect the clinic's own Twilio account, turn the phone on, and
 * set the announcement. Only a veterinarian changes these; anyone can call.
 */
export function PhoneSettings() {
  const app = useApp();
  const t = useT();
  const [view, setView] = useState<engine.PhoneView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [connecting, setConnecting] = useState(false);
  const [checks, setChecks] = useState<engine.SetupCheck[] | null>(null);
  const [checking, setChecking] = useState(false);
  const [confirmOff, setConfirmOff] = useState(false);
  const [script, setScript] = useState("");
  const vet = app.can.team;

  const load = useCallback(() => {
    engine.phone().then(
      (v) => {
        setView(v);
        setScript(v.prefs.announcement);
      },
      (e) => setError(String(e?.message ?? e)),
    );
  }, []);
  useEffect(load, [load]);

  async function savePrefs(prefs: engine.PhonePrefs) {
    setError(null);
    try {
      const v = await engine.phoneSetPrefs(prefs);
      setView(v);
      setScript(v.prefs.announcement);
      app.toast(t("Phone settings saved"));
    } catch (e) {
      setError(t(e instanceof Error ? e.message : String(e)));
    }
  }

  async function check() {
    setChecking(true);
    setError(null);
    try {
      setChecks(await engine.phoneCheck());
    } catch (e) {
      setError(t(e instanceof Error ? e.message : String(e)));
    } finally {
      setChecking(false);
    }
  }

  async function disconnect() {
    setConfirmOff(false);
    try {
      const out = await engine.phoneDisconnect();
      setChecks(null);
      load();
      app.toast(t(out.note));
    } catch (e) {
      setError(t(e instanceof Error ? e.message : String(e)));
    }
  }

  const tw = view?.twilio;
  const prefs = view?.prefs;
  return (
    <section className="group phone-settings" aria-labelledby="phone-settings-title">
      <h2 id="phone-settings-title">{t("Phone calls")}</h2>
      <p className="small muted measure">
        {t(
          "Call clients from OpenBark through your own Twilio account. You pay Twilio directly. ANI.ML Health never sees your keys or calls.",
        )}
      </p>
      {error && (
        <p className="ob-notice" data-tone="danger" role="alert">
          {error}
        </p>
      )}
      {view?.linux && engine.isDesktop && (
        <p className="ob-notice">{t("Calls work in web mode on Linux. Run `openbark serve`, then open OpenBark in Chrome or Firefox.")}</p>
      )}
      {view === null ? (
        <div className="ob-skeleton" style={{ height: 80 }} />
      ) : !view.connected ? (
        <div className="phone-card">
          <h3 className="label">{t("Phone calls (Twilio)")}</h3>
          <p className="small muted measure">
            {t("Make calls here. Recorded calls are recorded on this computer and become visits.")}
          </p>
          {vet ? (
            <div>
              <Button variant="primary" onClick={() => setConnecting(true)}>
                {t("Connect Twilio")}
              </Button>
            </div>
          ) : (
            <p className="small faint">{t("Only a veterinarian can connect accounts.")}</p>
          )}
          <p className="small faint measure">{t("Email through your own SendGrid account comes in a later release.")}</p>
        </div>
      ) : (
        <>
          <div className="phone-card">
            <StatusDot tone="ok">{t("Twilio · connected")}</StatusDot>
            <dl className="phone-facts">
              <div>
                <dt className="small muted">{t("Caller ID")}</dt>
                <dd className="mono">{formatPhone(tw?.caller_id)}</dd>
              </div>
              <div>
                <dt className="small muted">{t("This computer")}</dt>
                <dd className="mono">{tw?.computer_name}</dd>
              </div>
              <div>
                <dt className="small muted">{t("Key")}</dt>
                <dd className="mono">{tw?.api_key_hint}</dd>
              </div>
            </dl>
            {view.secret?.location === "sealed_file" && (
              <p className="small muted measure">
                {t(
                  "This computer's keychain isn't available, so the key secret is kept in a file sealed with OpenBark's encryption key.",
                )}
              </p>
            )}
          </div>
          <label className="check">
            <input
              type="checkbox"
              checked={Boolean(prefs?.enabled)}
              disabled={!vet}
              onChange={(e) => prefs && void savePrefs({ ...prefs, enabled: e.target.checked })}
            />
            <span>{t("Turn on the phone on this computer")}</span>
          </label>
          <p className="small muted measure phone-helper">
            {t("While it's on, this computer can make calls, and Twilio's calling service is connected during each call.")}
          </p>
          <label className="check">
            <input
              type="checkbox"
              checked={Boolean(prefs?.record_calls)}
              disabled={!vet}
              onChange={(e) => prefs && void savePrefs({ ...prefs, record_calls: e.target.checked })}
            />
            <span>{t("Record calls")}</span>
          </label>
          <p className="small muted measure phone-helper">
            {t("Recorded calls start with an announcement and become visits. You can choose not to record a call.")}
          </p>
          <label className="ob-field">
            <span className="label">{t("Announcement")}</span>
            <textarea
              className="ob-input tb-text measure"
              rows={3}
              value={script}
              disabled={!vet}
              onChange={(e) => setScript(e.target.value)}
            />
            <span className="ob-help">
              {t("On calls you make, you read the announcement yourself. It's shown on screen when the client answers.")}
            </span>
          </label>
          {vet && prefs && (
            <div className="phone-row">
              <Button size="compact" disabled={script === prefs.announcement} onClick={() => void savePrefs({ ...prefs, announcement: script })}>
                {t("Save announcement")}
              </Button>
              {script !== DEFAULT_ANNOUNCEMENT && (
                <button type="button" className="link small" onClick={() => setScript(DEFAULT_ANNOUNCEMENT)}>
                  {t("Use the default wording")}
                </button>
              )}
            </div>
          )}
          <ul className="phone-notes small muted measure">
            <li>
              {t(
                "We announce every recorded call, whatever your clinic's consent rule, because a client may be somewhere that needs everyone's agreement.",
              )}
            </li>
            <li>
              {t(
                "Twilio carries your calls, like any phone company, and keeps call logs (numbers, times, durations) in your account. Twilio has no Canadian data centre, so those logs are kept in the United States. It doesn't keep the audio, because OpenBark never asks it to record.",
              )}
            </li>
            <li>{t("During a call, Twilio's calling software sends call-quality figures (no audio) to your Twilio account.")}</li>
          </ul>
          {checks && (
            <ul className="phone-checks" aria-label={t("Check setup")}>
              {checks.map((c) => (
                <li key={c.id}>
                  <StatusDot tone={c.ok === null ? "neutral" : c.ok ? "ok" : "danger"}>{t(c.message)}</StatusDot>
                </li>
              ))}
            </ul>
          )}
          {vet && (
            <div className="phone-row">
              <Button onClick={() => void check()} disabled={checking}>
                {checking ? t("Checking your Twilio account…") : t("Check setup")}
              </Button>
              <Button variant="ghost" onClick={() => setConnecting(true)}>
                {t("Change")}
              </Button>
              <Button variant="danger" onClick={() => setConfirmOff(true)}>
                {t("Disconnect")}
              </Button>
            </div>
          )}
          {confirmOff && (
            <div className="ob-notice" role="alertdialog" aria-label={t("Disconnect")}>
              <span>
                {t(
                  "Disconnect Twilio? This removes the key from this computer. Also delete it in your Twilio console, so it can't be used anywhere else.",
                )}
              </span>
              <span className="notice-actions">
                <Button size="compact" variant="danger" onClick={() => void disconnect()}>
                  {t("Disconnect")}
                </Button>
                <Button size="compact" variant="ghost" onClick={() => setConfirmOff(false)}>
                  {t("Cancel")}
                </Button>
              </span>
            </div>
          )}
        </>
      )}
      {connecting && (
        <ConnectTwilioSheet
          current={view}
          onClose={() => setConnecting(false)}
          onConnected={(v) => {
            setView(v);
            setScript(v.prefs.announcement);
            setChecks(null);
            app.toast(t("Connected to Twilio. Calls show {number}.", { number: formatPhone(v.twilio?.caller_id) }));
          }}
        />
      )}
    </section>
  );
}

/** Connect Twilio: the clinic's own account, key and Bin (ADR-0023 section 3). */
export function ConnectTwilioSheet({
  current,
  onClose,
  onConnected,
}: {
  current: engine.PhoneView | null;
  onClose: () => void;
  onConnected: (v: engine.PhoneView) => void;
}) {
  const t = useT();
  const tw = current?.twilio;
  const [form, setForm] = useState<engine.ConnectTwilio>({
    account_sid: tw?.account_sid ?? "",
    api_key_sid: tw?.api_key_sid ?? "",
    api_key_secret: "",
    computer_name: tw?.computer_name ?? "",
    caller_id: tw?.caller_id ?? "",
    outgoing_bin_url: tw?.outgoing_bin_url ?? "",
  });
  const [bin, setBin] = useState<string | null>(null);
  const [binError, setBinError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const set = (k: keyof engine.ConnectTwilio) => (e: React.ChangeEvent<HTMLInputElement>) =>
    setForm({ ...form, [k]: e.target.value });
  const keepSecret = Boolean(tw && tw.api_key_sid === form.api_key_sid.trim());

  async function showBin() {
    setBinError(null);
    try {
      setBin(await engine.phoneBin(form.caller_id));
    } catch (e) {
      setBin(null);
      setBinError(t(e instanceof Error ? e.message : String(e)));
    }
  }

  async function connect(close: () => void) {
    setBusy(true);
    setError(null);
    try {
      const out = await engine.phoneConnect(form);
      // The secret is with the engine now; the form forgets it.
      setForm((f) => ({ ...f, api_key_secret: "" }));
      onConnected(out.phone);
      close();
    } catch (e) {
      setError(t(e instanceof Error ? e.message : String(e)));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Sheet label={t("Connect Twilio")} onClose={onClose} dismissible={!busy}>
      {(close) => (
        <form
          className="sheet-form"
          onSubmit={(e) => {
            e.preventDefault();
            void connect(close);
          }}
        >
          <header className="sheet-head">
            <h2>{t("Connect Twilio")}</h2>
            <button type="button" className="icon-btn" aria-label={t("Close")} onClick={close} disabled={busy}>
              <X size={20} aria-hidden />
            </button>
          </header>
          <div className="sheet-body phone-connect">
            <p className="small muted">
              {t("About 15 minutes in your Twilio console. You pay Twilio directly; OpenBark only uses what you paste here.")}
            </p>
            <ol className="phone-steps small">
              <li>{t("Upgrade your Twilio account (add a payment method). A trial account can't connect calls through OpenBark.")}</li>
              <li>
                {t(
                  "Choose your caller ID: buy a Twilio number, or verify your clinic's number under Phone Numbers > Verified Caller IDs.",
                )}
              </li>
              <li>{t("Limit where calls can go: Voice > Settings > Geo permissions. Turn on only the countries you call.")}</li>
              <li>{t("Set a usage alert: Billing > Usage triggers.")}</li>
              <li>
                {t(
                  "Make an API key for this computer: Account > API keys & tokens > Create API key, in the United States (US1) region, type Standard. Copy the SID and the secret now: Twilio shows the secret only once.",
                )}
              </li>
              <li>
                {t(
                  "Make a TwiML Bin named OpenBark outgoing: TwiML Bins > Create. Paste the text from Show the Twilio text below, save, then copy the Bin's URL into this form.",
                )}
              </li>
            </ol>
            <label className="ob-field">
              <span className="label">{t("Account SID")}</span>
              <input className="ob-input mono" value={form.account_sid} onChange={set("account_sid")} autoComplete="off" spellCheck={false} />
              <span className="ob-help">{t("On your Twilio console's home page. Starts with AC.")}</span>
            </label>
            <label className="ob-field">
              <span className="label">{t("API key SID")}</span>
              <input className="ob-input mono" value={form.api_key_sid} onChange={set("api_key_sid")} autoComplete="off" spellCheck={false} />
              <span className="ob-help">{t("Starts with SK. Use a Standard key made for this computer.")}</span>
            </label>
            <label className="ob-field">
              <span className="label">{t("API key secret")}</span>
              <input
                className="ob-input mono"
                type="password"
                value={form.api_key_secret}
                onChange={set("api_key_secret")}
                autoComplete="off"
                placeholder={keepSecret ? current?.secret?.hint : undefined}
              />
              <span className="ob-help">
                {keepSecret
                  ? t("Leave empty to keep the saved secret.")
                  : t("Kept in this computer's keychain. It's only sent to Twilio.")}
              </span>
            </label>
            <label className="ob-field">
              <span className="label">{t("This computer's name")}</span>
              <input className="ob-input" value={form.computer_name} onChange={set("computer_name")} placeholder="front-desk" autoComplete="off" />
              <span className="ob-help">{t("Use a place, not a person: \"front-desk\".")}</span>
            </label>
            <label className="ob-field">
              <span className="label">{t("Caller ID")}</span>
              <input className="ob-input mono" value={form.caller_id} onChange={set("caller_id")} placeholder="+1 555 555 0100" inputMode="tel" autoComplete="off" />
              <span className="ob-help">{t("The number clients see: a number you bought from Twilio, or one you verified there.")}</span>
            </label>
            <div className="phone-row">
              <Button size="compact" onClick={() => void showBin()} disabled={!form.caller_id.trim()}>
                {t("Show the Twilio text")}
              </Button>
            </div>
            {binError && <p className="small" role="alert">{binError}</p>}
            {bin && (
              <div className="phone-bin">
                <p className="label">{t("Text for your TwiML Bin")}</p>
                <p className="small muted">
                  {t("Create a TwiML Bin in your Twilio console and paste this into it. Twilio has no way for OpenBark to do this for you.")}
                </p>
                <pre className="phone-bin-text mono">{bin}</pre>
                <div className="phone-row">
                  <Button size="compact" onClick={() => void navigator.clipboard.writeText(bin)}>
                    <Copy size={16} aria-hidden />
                    {t("Copy")}
                  </Button>
                </div>
                <p className="small faint">{t("Neither text asks Twilio to record. Recording happens on this computer.")}</p>
              </div>
            )}
            <label className="ob-field">
              <span className="label">{t("Outgoing Bin URL")}</span>
              <input
                className="ob-input mono"
                value={form.outgoing_bin_url}
                onChange={set("outgoing_bin_url")}
                placeholder="https://handler.twilio.com/twiml/EH…"
                autoComplete="off"
                spellCheck={false}
              />
            </label>
            {error && (
              <p className="ob-notice" data-tone="danger" role="alert">
                {error}
              </p>
            )}
          </div>
          <footer className="sheet-foot phone-row">
            <Button type="submit" variant="primary" disabled={busy}>
              {busy ? t("Checking your Twilio account…") : t("Connect")}
            </Button>
            <Button variant="ghost" onClick={close} disabled={busy}>
              {t("Cancel")}
            </Button>
          </footer>
        </form>
      )}
    </Sheet>
  );
}
