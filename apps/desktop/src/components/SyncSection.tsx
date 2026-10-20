import { Button, Dialog, Sheet } from "@openbark/ui";
import { RefreshCw, X } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { useApp } from "../context";
import * as engine from "../engine";
import { useT } from "../i18n";

/** The accepted public wording (ADR-0017 owner decisions). */
export const SYNC_PROMISE =
  "Recordings and notes stay on your devices unless you turn on encrypted sync. Even then, only encrypted copies leave, and our servers can't read them.";

const BACKFILL: { days: number | null; label: string }[] = [
  { days: 30, label: "The last 30 days (recommended)" },
  { days: 7, label: "The last 7 days" },
  { days: 90, label: "The last 90 days" },
  { days: null, label: "All visits on this computer" },
  { days: 0, label: "Only visits from now on" },
];

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

/**
 * Settings > Sync (ADR-0017, M3 phase 2). Off by default. Turning it on signs
 * in with an emailed code; a new account shows its Recovery Kit, a computer
 * joining an account shows a 6-word code to type on a computer that already
 * syncs. Every network call is made by OpenBark's engine, never this screen.
 */
export function SyncSection() {
  const app = useApp();
  const t = useT();
  const [s, setS] = useState<engine.SyncStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [sheet, setSheet] = useState(false);
  const [busy, setBusy] = useState(false);
  const [removing, setRemoving] = useState<engine.SyncDevice | null>(null);
  const [deleting, setDeleting] = useState(false);
  const [adding, setAdding] = useState(false);

  const refresh = useCallback(() => engine.syncStatus().then(setS, (e) => setError(message(e))), []);
  useEffect(() => {
    void refresh();
    const timer = setInterval(() => void refresh(), 5000);
    return () => clearInterval(timer);
  }, [refresh]);

  if (!s) {
    return (
      <section className="group">
        <h2>{t("Encrypted sync")}</h2>
        <div className="ob-skeleton" style={{ height: 80 }} />
      </section>
    );
  }
  if (!s.available) return null;

  const run = async (f: () => Promise<unknown>, done?: string) => {
    setError(null);
    setBusy(true);
    try {
      await f();
      if (done) app.toast(done);
    } catch (e) {
      setError(message(e));
    } finally {
      setBusy(false);
      void refresh();
    }
  };

  const phase = s.phase;
  const joining = phase === "pending" || phase === "kit_check" || phase === "code_sent";
  return (
    <section className="group sync-section">
      <h2>
        {t("Encrypted sync")} <span className="ob-tag">{t("Preview")}</span>
      </h2>
      <p className="small measure">{t(SYNC_PROMISE)}</p>
      <p className="small muted measure">
        {t(
          "Sync carries transcripts, notes and patient details between your devices. Recordings never leave the device that recorded them.",
        )}
      </p>
      {error && (
        <p className="ob-notice" data-tone="danger" role="alert">
          {error}
        </p>
      )}

      {(phase === "off" || phase === "revoked" || phase === "signed_out") && (
        <>
          {phase === "revoked" && (
            <p className="ob-notice" data-tone="warn">
              {t(
                "This computer was removed from your sync account. Its synced copies were erased; its own visits and recordings stay.",
              )}
            </p>
          )}
          {phase === "signed_out" && (
            <p className="ob-notice" data-tone="warn">
              {t("Signed out of sync. Sign in again to keep syncing.")}
            </p>
          )}
          <p className="small faint measure">
            {t("Preview: use test data only until the independent security review is finished.")}
          </p>
          <div className="ask-actions">
            <Button onClick={() => setSheet(true)}>
              {phase === "signed_out" ? t("Sign in again") : t("Turn on encrypted sync")}
            </Button>
          </div>
          {phase === "off" && <ServerChoice s={s} onChanged={refresh} />}
        </>
      )}

      {joining && (
        <div className="ask-actions">
          <Button onClick={() => setSheet(true)}>{t("Continue setting up sync")}</Button>
          <Button variant="ghost" disabled={busy} onClick={() => void run(() => engine.syncOff())}>
            {t("Cancel")}
          </Button>
        </div>
      )}

      {phase === "active" && (
        <>
          <p className="small measure">
            <span className="ob-dot" data-tone={s.last_error ? "warn" : "ok"} aria-hidden />{" "}
            {s.busy
              ? t("Syncing now.")
              : s.last_sync_at
                ? t("Syncing as {email}. Last synced {when}.", {
                    email: s.email ?? "",
                    when: engine.fmtStamp(s.last_sync_at),
                  })
                : t("Syncing as {email}.", { email: s.email ?? "" })}{" "}
            {(s.pending_uploads ?? 0) > 0 && t("{n} changes waiting to upload.", { n: s.pending_uploads ?? 0 })}
          </p>
          {s.last_error && (
            <p className="ob-notice" data-tone="warn">
              {s.last_error}
            </p>
          )}
          <div className="ask-actions">
            <Button variant="ghost" disabled={busy || s.busy} onClick={() => void run(() => engine.syncNow())}>
              <RefreshCw size={16} aria-hidden />
              {t("Sync now")}
            </Button>
            <Button variant="ghost" onClick={() => setAdding(true)}>
              {t("Add a device")}
            </Button>
          </div>

          <h3 className="label">{t("Devices")}</h3>
          <ul className="sync-devices">
            {(s.devices ?? [])
              .filter((d) => d.status === "approved")
              .map((d) => (
                <li key={d.id} className="storage-row">
                  <div className="storage-text">
                    <span className="label">
                      {d.name || t("Unnamed device")} {d.this_device && <span className="ob-tag">{t("This computer")}</span>}
                    </span>
                    <span className="small faint">
                      {platformName(t, d.platform)}
                      {d.last_seen_at ? `, ${t("last seen {when}", { when: engine.fmtStamp(d.last_seen_at) })}` : ""}
                    </span>
                  </div>
                  <Button variant="ghost" onClick={() => setRemoving(d)}>
                    {d.this_device ? t("Remove this computer") : t("Remove")}
                  </Button>
                </li>
              ))}
          </ul>
          {(s.devices ?? []).some((d) => d.status === "revoked") && (
            <p className="small faint">
              {t("Removed devices: {names}", {
                names: (s.devices ?? [])
                  .filter((d) => d.status === "revoked")
                  .map((d) => d.name || t("Unnamed device"))
                  .join(", "),
              })}
            </p>
          )}

          {(s.conflicts ?? []).length > 0 && (
            <>
              <h3 className="label">{t("Changed on two devices at once")}</h3>
              <p className="small muted measure">
                {t("The later change was kept. The other value is listed here so nothing is lost.")}
              </p>
              <ul className="sync-conflicts small">
                {(s.conflicts ?? []).slice(0, 5).map((c, i) => (
                  <li key={`${c.visit}-${c.field}-${i}`}>
                    {t("{field}: kept the change from {kept}; the change from {other} was {value}.", {
                      field: fieldName(t, c.field),
                      kept: c.kept_from === "this device" ? t("this device") : c.kept_from,
                      other: c.other_from === "this device" ? t("this device") : c.other_from,
                      value: summary(c.other),
                    })}
                  </li>
                ))}
              </ul>
            </>
          )}
          {(s.warnings ?? []).length > 0 && (
            <div className="ob-notice" data-tone="warn">
              <p>{t("Something the sync server sent didn't check out and was set aside. Your visits are unchanged.")}</p>
              <ul className="small">
                {(s.warnings ?? []).slice(-3).map((w) => (
                  <li key={w}>{w}</li>
                ))}
              </ul>
            </div>
          )}

          <div className="ask-actions">
            <Button variant="ghost" disabled={busy} onClick={() => void run(() => engine.syncOff(), t("Sync is off on this computer"))}>
              {t("Turn off sync on this computer")}
            </Button>
            <Button variant="danger" onClick={() => setDeleting(true)}>
              {t("Delete my sync account")}
            </Button>
          </div>
          <p className="small faint measure">
            {t("Turning sync off keeps every visit on this computer. The sync keys are kept in {where}.", {
              where: s.keys_in === "keychain" ? t("this computer's keychain") : t("a protected file on this computer"),
            })}
          </p>
        </>
      )}

      {sheet && (
        <SyncSheet
          status={s}
          onClose={() => {
            setSheet(false);
            void refresh();
          }}
        />
      )}
      {adding && <AddDeviceSheet onClose={() => (setAdding(false), void refresh())} />}
      {removing && (
        <Dialog labelledBy="sync-remove-title" onClose={() => setRemoving(null)} dismissible={!busy}>
          {(close) => (
            <>
              <h2 id="sync-remove-title">
                {removing.this_device
                  ? t("Remove this computer from sync?")
                  : t("Remove {name}?", { name: removing.name || t("Unnamed device") })}
              </h2>
              <p>
                {t(
                  "It's locked out at once, and the keys change so it can't open anything synced from now on. When it next connects, it erases its synced copies. Its own visits and recordings stay on it.",
                )}
              </p>
              <div className="ask-actions">
                <Button
                  variant="danger"
                  disabled={busy}
                  onClick={() => void run(() => engine.syncRemoveDevice(removing.id), t("Device removed")).then(close)}
                >
                  {t("Remove")}
                </Button>
                <Button variant="ghost" onClick={close}>
                  {t("Cancel")}
                </Button>
              </div>
            </>
          )}
        </Dialog>
      )}
      {deleting && <DeleteAccountDialog email={s.email ?? ""} onClose={() => (setDeleting(false), void refresh())} />}
    </section>
  );
}

function platformName(t: (s: string) => string, p: string): string {
  const names: Record<string, string> = {
    windows: "Windows",
    macos: "Mac",
    linux: "Linux",
    ios: "iPhone",
    android: "Android",
  };
  return names[p] ?? t("Other");
}

function fieldName(t: (s: string) => string, f: string): string {
  const names: Record<string, string> = {
    patient: "Patient details",
    consent: "Consent",
    recorded_by: "Recorded by",
    audio: "Recording details",
    audio_deleted: "Recording deleted",
  };
  return t(names[f] ?? f);
}

function summary(v: unknown): string {
  if (v && typeof v === "object") {
    const o = v as Record<string, unknown>;
    const parts = ["name", "last_name", "reason", "species"].map((k) => o[k]).filter((x) => typeof x === "string" && x);
    if (parts.length) return parts.join(", ");
  }
  return v == null ? "empty" : JSON.stringify(v).slice(0, 80);
}

/** Before signing in: the hosted service, or the clinic's own Supabase. */
function ServerChoice({ s, onChanged }: { s: engine.SyncStatus; onChanged: () => void }) {
  const t = useT();
  const [open, setOpen] = useState(false);
  const [url, setUrl] = useState("");
  const [key, setKey] = useState("");
  const [error, setError] = useState<string | null>(null);
  if (!open) {
    return (
      <p className="small faint">
        {t("Server: {host}.", { host: s.server ?? "" })}{" "}
        <button type="button" className="link small" onClick={() => setOpen(true)}>
          {t("Use my clinic's own server")}
        </button>
      </p>
    );
  }
  return (
    <form
      className="sync-server"
      onSubmit={(e) => {
        e.preventDefault();
        setError(null);
        engine.syncSetServer(url, key).then(
          () => (setOpen(false), onChanged()),
          (err) => setError(message(err)),
        );
      }}
    >
      <label className="ob-field narrow">
        <span className="label">{t("Server address")}</span>
        <input className="ob-input" value={url} placeholder="https://example.supabase.co" onChange={(e) => setUrl(e.target.value)} />
      </label>
      <label className="ob-field narrow">
        <span className="label">{t("Publishable key")}</span>
        <input className="ob-input mono" value={key} onChange={(e) => setKey(e.target.value)} />
        <span className="ob-help">{t("Leave both empty to use the OpenBark service.")}</span>
      </label>
      {error && (
        <p className="ob-notice" data-tone="danger" role="alert">
          {error}
        </p>
      )}
      <div className="ask-actions">
        <Button type="submit">{t("Save")}</Button>
        <Button variant="ghost" onClick={() => setOpen(false)}>
          {t("Cancel")}
        </Button>
      </div>
    </form>
  );
}

type Step = "email" | "code" | "kit" | "check" | "pair" | "recover" | "done";

/** Turning sync on, step by step. */
function SyncSheet({ status, onClose }: { status: engine.SyncStatus; onClose: () => void }) {
  const t = useT();
  const initial: Step =
    status.phase === "kit_check" ? "kit" : status.phase === "pending" ? "pair" : status.phase === "code_sent" ? "code" : "email";
  const [step, setStep] = useState<Step>(initial);
  const [email, setEmail] = useState(status.email ?? "");
  const [name, setName] = useState(status.device_name ?? "");
  const [backfill, setBackfill] = useState<number | null>(30);
  const [code, setCode] = useState("");
  const [kit, setKit] = useState<engine.SyncKit | null>(status.kit ?? null);
  const [pairing, setPairing] = useState<engine.SyncPairing | null>(status.pairing ?? null);
  const [answers, setAnswers] = useState<[string, string]>(["", ""]);
  const [kitWords, setKitWords] = useState("");
  const [computerKey, setComputerKey] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [resendIn, setResendIn] = useState(0);
  const timer = useRef<ReturnType<typeof setInterval> | null>(null);

  useEffect(() => () => void (timer.current && clearInterval(timer.current)), []);
  useEffect(() => {
    if (resendIn <= 0) return;
    const id = setTimeout(() => setResendIn((n) => n - 1), 1000);
    return () => clearTimeout(id);
  }, [resendIn]);
  // While waiting to be added, check every few seconds.
  useEffect(() => {
    if (step !== "pair") return;
    const id = setInterval(() => {
      void engine
        .syncNow()
        .then((s) => {
          if (s.phase === "active") setStep("done");
          if (s.pairing) setPairing(s.pairing);
        })
        .catch(() => {});
    }, 3000);
    return () => clearInterval(id);
  }, [step]);
  useEffect(() => {
    if (step === "kit") engine.recoveryKey().then(setComputerKey, () => setComputerKey(null));
  }, [step]);

  const act = async (f: () => Promise<void>) => {
    setError(null);
    setBusy(true);
    try {
      await f();
    } catch (e) {
      setError(message(e));
    } finally {
      setBusy(false);
    }
  };

  const sendCode = () =>
    act(async () => {
      await engine.syncSendCode(email);
      setResendIn(60);
      setStep("code");
    });

  const verify = () =>
    act(async () => {
      const next = await engine.syncVerify(code, name, backfill);
      if (next.next === "kit") {
        setKit({ words: next.words, qr_svg: next.qr_svg, check: next.check });
        setStep("kit");
      } else if (next.next === "pair") {
        setPairing({ words: next.words, qr_svg: next.qr_svg, expires_at: next.expires_at });
        setStep("pair");
      } else {
        setStep("done");
      }
    });

  const title =
    step === "kit" || step === "check"
      ? t("Your Recovery Kit")
      : step === "pair" || step === "recover"
        ? t("Add this computer")
        : step === "done"
          ? t("Encrypted sync is on")
          : t("Turn on encrypted sync");

  return (
    <Sheet label={title} onClose={onClose} dismissible={!busy}>
      {(close) => (
        <>
          <header className="sheet-head">
            <h2>{title}</h2>
            <button type="button" className="icon-btn" aria-label={t("Close")} disabled={busy} onClick={close}>
              <X size={20} aria-hidden />
            </button>
          </header>
          <div className="sheet-body sync-sheet">
            {error && (
              <p className="ob-notice" data-tone="danger" role="alert">
                {error}
              </p>
            )}

            {step === "email" && (
              <form
                onSubmit={(e) => {
                  e.preventDefault();
                  void sendCode();
                }}
              >
                <p className="small measure">{t(SYNC_PROMISE)}</p>
                <label className="ob-field">
                  <span className="label">{t("Your email address")}</span>
                  <input
                    className="ob-input"
                    type="email"
                    autoComplete="email"
                    required
                    value={email}
                    onChange={(e) => setEmail(e.target.value)}
                  />
                  <span className="ob-help">{t("We email you a 6-digit code. There's no password.")}</span>
                </label>
                <label className="ob-field">
                  <span className="label">{t("This computer's name")}</span>
                  <input
                    className="ob-input"
                    value={name}
                    placeholder={t("Front desk PC")}
                    onChange={(e) => setName(e.target.value)}
                  />
                  <span className="ob-help">{t("Shown in your list of devices. Only your devices can read it.")}</span>
                </label>
                <label className="ob-field">
                  <span className="label">{t("Upload visits from")}</span>
                  <select
                    className="ob-select"
                    value={backfill === null ? "all" : String(backfill)}
                    onChange={(e) => setBackfill(e.target.value === "all" ? null : Number(e.target.value))}
                  >
                    {BACKFILL.map((b) => (
                      <option key={String(b.days)} value={b.days === null ? "all" : String(b.days)}>
                        {t(b.label)}
                      </option>
                    ))}
                  </select>
                </label>
                <div className="ask-actions">
                  <Button type="submit" variant="primary" disabled={busy || !email.includes("@")}>
                    {t("Email me a code")}
                  </Button>
                </div>
              </form>
            )}

            {step === "code" && (
              <form
                onSubmit={(e) => {
                  e.preventDefault();
                  void verify();
                }}
              >
                <label className="ob-field">
                  <span className="label">{t("Enter the 6-digit code we emailed to {email}", { email })}</span>
                  <input
                    className="ob-input mono sync-code"
                    inputMode="numeric"
                    autoComplete="one-time-code"
                    maxLength={8}
                    autoFocus
                    value={code}
                    onChange={(e) => setCode(e.target.value.replace(/\D/g, ""))}
                  />
                </label>
                <div className="ask-actions">
                  <Button type="submit" variant="primary" disabled={busy || code.length !== 6}>
                    {t("Continue")}
                  </Button>
                  <Button variant="ghost" disabled={busy || resendIn > 0} onClick={() => void sendCode()}>
                    {resendIn > 0 ? t("Send a new code in {n} s", { n: resendIn }) : t("Send a new code")}
                  </Button>
                </div>
              </form>
            )}

            {step === "kit" && kit && (
              <>
                <p className="small measure">
                  {t(
                    "These 24 words are your Recovery Kit. With them, a new device can open your synced visits even if every other device is lost. Print this page or keep it in your clinic's password manager. Nobody else has a copy, including us.",
                  )}
                </p>
                <div className="print-kit">
                  <h3 className="label">{t("OpenBark Recovery Kit (restores your synced visits)")}</h3>
                  <ol className="kit-words mono">
                    {kit.words.map((w, i) => (
                      <li key={i}>{w}</li>
                    ))}
                  </ol>
                  {/* The QR comes from the engine as SVG (qrcode crate). */}
                  <div className="kit-qr" aria-hidden dangerouslySetInnerHTML={{ __html: kit.qr_svg }} />
                  {computerKey && (
                    <>
                      <h3 className="label">{t("Computer recovery key (opens this computer's own visits)")}</h3>
                      <p className="mono small kit-computer">{computerKey}</p>
                    </>
                  )}
                </div>
                <div className="ask-actions">
                  <Button variant="primary" onClick={() => setStep("check")}>
                    {t("I've kept it safe")}
                  </Button>
                  <Button variant="ghost" onClick={() => window.print()}>
                    {t("Print")}
                  </Button>
                </div>
              </>
            )}

            {step === "check" && kit && (
              <form
                onSubmit={(e) => {
                  e.preventDefault();
                  void act(async () => {
                    await engine.syncConfirmKit([
                      [kit.check[0], answers[0]],
                      [kit.check[1], answers[1]],
                    ]);
                    setStep("done");
                  });
                }}
              >
                <p className="small measure">
                  {t("To be sure the kit is kept, type two of its words. Syncing starts after this.")}
                </p>
                {kit.check.map((pos, i) => (
                  <label className="ob-field narrow" key={pos}>
                    <span className="label">{t("Word {n}", { n: pos })}</span>
                    <input
                      className="ob-input mono"
                      autoCapitalize="none"
                      autoComplete="off"
                      value={answers[i]}
                      onChange={(e) =>
                        setAnswers((a) => (i === 0 ? [e.target.value, a[1]] : [a[0], e.target.value]) as [string, string])
                      }
                    />
                  </label>
                ))}
                <div className="ask-actions">
                  <Button type="submit" variant="primary" disabled={busy || !answers[0] || !answers[1]}>
                    {t("Check and start syncing")}
                  </Button>
                  <Button variant="ghost" onClick={() => setStep("kit")}>
                    {t("Show the words again")}
                  </Button>
                </div>
              </form>
            )}

            {step === "pair" && pairing && (
              <>
                <p className="small measure">
                  {t(
                    "Your account already syncs on another device. On a computer that syncs, open Settings, Encrypted sync, Add a device, and type these 6 words. The code works for 10 minutes.",
                  )}
                </p>
                <p className="pair-words mono" aria-label={t("Pairing code")}>
                  {pairing.words.join(" ")}
                </p>
                <div className="kit-qr" aria-hidden dangerouslySetInnerHTML={{ __html: pairing.qr_svg }} />
                <p className="small faint">{t("A phone can scan the code instead.")}</p>
                <p className="small muted">
                  <span className="ob-dot" data-tone="warn" aria-hidden /> {t("Waiting to be added.")}
                </p>
                <div className="ask-actions">
                  <Button variant="ghost" disabled={busy} onClick={() => void act(async () => setPairing(await engine.syncNewCode()))}>
                    {t("New code")}
                  </Button>
                  <Button variant="ghost" onClick={() => setStep("recover")}>
                    {t("Use my Recovery Kit instead")}
                  </Button>
                </div>
              </>
            )}

            {step === "recover" && (
              <form
                onSubmit={(e) => {
                  e.preventDefault();
                  void act(async () => {
                    await engine.syncRecover(kitWords);
                    setStep("done");
                  });
                }}
              >
                <label className="ob-field">
                  <span className="label">{t("The 24 words of your Recovery Kit")}</span>
                  <textarea
                    className="ob-input tb-text mono"
                    rows={4}
                    autoCapitalize="none"
                    autoComplete="off"
                    spellCheck={false}
                    value={kitWords}
                    onChange={(e) => setKitWords(e.target.value)}
                  />
                  <span className="ob-help">{t("The words stay on this computer; they're never sent anywhere.")}</span>
                </label>
                <div className="ask-actions">
                  <Button type="submit" variant="primary" disabled={busy || kitWords.trim().split(/\s+/).length < 24}>
                    {t("Add this computer")}
                  </Button>
                  <Button variant="ghost" onClick={() => setStep("pair")}>
                    {t("Back")}
                  </Button>
                </div>
              </form>
            )}

            {step === "done" && (
              <>
                <p className="measure">
                  {t(
                    "This computer now syncs transcripts, notes and patient details with your other devices, every 30 seconds while OpenBark is open. Recordings stay where they were made.",
                  )}
                </p>
                <div className="ask-actions">
                  <Button variant="primary" onClick={close}>
                    {t("Done")}
                  </Button>
                </div>
              </>
            )}
          </div>
        </>
      )}
    </Sheet>
  );
}

/** On a computer that syncs: type the new device's 6 words, check, add. */
function AddDeviceSheet({ onClose }: { onClose: () => void }) {
  const t = useT();
  const app = useApp();
  const [words, setWords] = useState("");
  const [found, setFound] = useState<engine.SyncPending | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const act = async (f: () => Promise<void>) => {
    setError(null);
    setBusy(true);
    try {
      await f();
    } catch (e) {
      setError(message(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <Sheet label={t("Add a device")} onClose={onClose} dismissible={!busy}>
      {(close) => (
        <>
          <header className="sheet-head">
            <h2>{t("Add a device")}</h2>
            <button type="button" className="icon-btn" aria-label={t("Close")} disabled={busy} onClick={close}>
              <X size={20} aria-hidden />
            </button>
          </header>
          <div className="sheet-body sync-sheet">
            <p className="small measure">
              {t(
                "On the new device, turn on encrypted sync and sign in with the same email. It shows 6 words. Type them here: OpenBark checks they match that device before giving it the keys.",
              )}
            </p>
            {error && (
              <p className="ob-notice" data-tone="danger" role="alert">
                {error}
              </p>
            )}
            {!found ? (
              <form
                onSubmit={(e) => {
                  e.preventDefault();
                  void act(async () => setFound(await engine.syncPair(words, false)));
                }}
              >
                <label className="ob-field">
                  <span className="label">{t("The 6 words shown on the new device")}</span>
                  <input
                    className="ob-input mono"
                    autoCapitalize="none"
                    autoComplete="off"
                    spellCheck={false}
                    autoFocus
                    value={words}
                    onChange={(e) => setWords(e.target.value)}
                  />
                </label>
                <div className="ask-actions">
                  <Button type="submit" variant="primary" disabled={busy || words.trim().split(/[\s-]+/).length < 6}>
                    {t("Check")}
                  </Button>
                </div>
              </form>
            ) : (
              <>
                <p>
                  {t("Add {name} ({platform}) to your sync account?", {
                    name: found.name,
                    platform: platformName(t, found.platform),
                  })}
                </p>
                <p className="small muted measure">
                  {t("It can then open everything you sync. You can remove it at any time.")}
                </p>
                <div className="ask-actions">
                  <Button
                    variant="primary"
                    disabled={busy}
                    onClick={() =>
                      void act(async () => {
                        await engine.syncPair(words, true);
                        app.toast(t("{name} added", { name: found.name }));
                        close();
                      })
                    }
                  >
                    {t("Add device")}
                  </Button>
                  <Button variant="ghost" onClick={() => setFound(null)}>
                    {t("Back")}
                  </Button>
                </div>
              </>
            )}
          </div>
        </>
      )}
    </Sheet>
  );
}

function DeleteAccountDialog({ email, onClose }: { email: string; onClose: () => void }) {
  const t = useT();
  const app = useApp();
  const [typed, setTyped] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  return (
    <Dialog labelledBy="sync-delete-title" onClose={onClose} dismissible={!busy}>
      {(close) => (
        <>
          <h2 id="sync-delete-title">{t("Delete your sync account?")}</h2>
          <p>
            {t(
              "Every encrypted copy, device and key on the sync server is deleted. The visits on each of your computers stay where they are.",
            )}
          </p>
          <label className="ob-field">
            <span className="label">{t("Type {email} to confirm", { email })}</span>
            <input className="ob-input" autoComplete="off" value={typed} onChange={(e) => setTyped(e.target.value)} />
          </label>
          {error && (
            <p className="ob-notice" data-tone="danger" role="alert">
              {error}
            </p>
          )}
          <div className="ask-actions">
            <Button
              variant="danger"
              disabled={busy || typed.trim().toLowerCase() !== email.toLowerCase()}
              onClick={async () => {
                setBusy(true);
                setError(null);
                try {
                  await engine.syncDeleteAccount(typed);
                  app.toast(t("Sync account deleted"));
                  close();
                } catch (e) {
                  setError(message(e));
                } finally {
                  setBusy(false);
                }
              }}
            >
              {t("Delete account")}
            </Button>
            <Button variant="ghost" onClick={close}>
              {t("Cancel")}
            </Button>
          </div>
        </>
      )}
    </Dialog>
  );
}
