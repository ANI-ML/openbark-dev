import { Button, Dialog, Sheet } from "@openbark/ui";
import { RefreshCw, X } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useApp } from "../context";
import * as engine from "../engine";
import { useT } from "../i18n";
import { isIos } from "../visits";

/** The accepted public wording (ADR-0017 owner decisions). */
export const SYNC_PROMISE =
  "Recordings and notes stay on your devices unless you turn on encrypted sync. Even then, only encrypted copies go to your clinic's hub, and nothing comes to us.";

const BACKFILL: { days: number | null; label: string }[] = [
  { days: 30, label: "The last 30 days (recommended)" },
  { days: 7, label: "The last 7 days" },
  { days: 90, label: "The last 90 days" },
  { days: null, label: "All visits on this computer" },
  { days: 0, label: "Only visits from now on" },
];

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

/**
 * Settings > Sync (ADR-0017, ADR-0029). Off by default. Turning it on joins
 * the clinic hub with a join code shown on the hub computer; a person's first
 * device makes their account and shows its Recovery Kit, a further device
 * shows a 6-word code to type on a device that already syncs. Every network
 * call is made by OpenBark's engine, never this screen.
 */
export function SyncSection() {
  const app = useApp();
  const t = useT();
  const ios = isIos(app.status);
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
  if (!s.available) {
    return (
      <section className="group sync-section">
        <h2>{t("Encrypted sync")}</h2>
        {s.reason === "failed" ? (
          <p className="ob-notice" data-tone="warn" role="alert">
            {t(
              "Encrypted sync couldn't start on this device. Restart OpenBark; if it still doesn't start, send this to the OpenBark team: {detail}",
              { detail: s.detail ?? "" },
            )}
          </p>
        ) : (
          <p className="small measure">
            {t(
              "Encrypted sync isn't in this version of OpenBark yet. It's in testing and arrives once the independent security review is finished. Until then, everything stays on this device.",
            )}
          </p>
        )}
      </section>
    );
  }

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
          <p className="small muted measure">
            {t(
              "Your devices sync through your clinic's hub: a computer at the clinic that keeps encrypted copies. Ask whoever runs it for a join code.",
            )}
          </p>
          <p className="small faint measure">
            {t("Preview: use test data only until the independent security review is finished.")}
          </p>
          <div className="ask-actions">
            <Button onClick={() => setSheet(true)}>{t("Join your clinic hub")}</Button>
          </div>
          {s.hub && <p className="small faint">{t("Clinic hub: {address}.", { address: s.hub.address })}</p>}
        </>
      )}

      {joining && (
        <div className="ask-actions">
          <Button onClick={() => setSheet(true)}>{t("Continue setting up sync")}</Button>
          {phase === "pending" && s.hub ? (
            <Button variant="ghost" disabled={busy} onClick={() => void run(() => engine.syncStartOver())}>
              {t("Start over")}
            </Button>
          ) : (
            <Button variant="ghost" disabled={busy} onClick={() => void run(() => engine.syncOff())}>
              {t("Cancel")}
            </Button>
          )}
        </div>
      )}

      {phase === "confirm" && (
        <ConfirmJoin
          status={s}
          onConfirmed={() => {
            app.toast(t("Encrypted sync is on"));
            void refresh();
          }}
          onStartOver={() => void refresh()}
        />
      )}

      {phase === "active" && (
        <>
          <p className="small measure">
            <span className="ob-dot" data-tone={s.last_error ? "warn" : "ok"} aria-hidden />{" "}
            {s.busy
              ? t("Syncing now.")
              : s.hub
                ? s.last_sync_at
                  ? t("Syncing with the clinic hub at {address}. Last synced {when}.", {
                      address: s.hub.address,
                      when: engine.fmtStamp(s.last_sync_at),
                    })
                  : t("Syncing with the clinic hub at {address}.", { address: s.hub.address })
                : s.last_sync_at
                  ? t("Syncing as {email}. Last synced {when}.", {
                      email: s.email ?? "",
                      when: engine.fmtStamp(s.last_sync_at),
                    })
                  : t("Syncing as {email}.", { email: s.email ?? "" })}{" "}
            {(s.pending_uploads ?? 0) > 0 && t("{n} changes waiting to upload.", { n: s.pending_uploads ?? 0 })}
          </p>
          {s.hub && (
            <p className="small faint">
              {t("Hub certificate")}: <span className="mono">{s.hub.fingerprint}</span>
            </p>
          )}
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
                    {d.this_device ? (ios ? t("Remove this iPhone") : t("Remove this computer")) : t("Remove")}
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
            <Button
              variant="ghost"
              disabled={busy}
              onClick={() =>
                void run(() => engine.syncOff(), ios ? t("Sync is off on this iPhone") : t("Sync is off on this computer"))
              }
            >
              {ios ? t("Turn off sync on this iPhone") : t("Turn off sync on this computer")}
            </Button>
            <Button variant="danger" onClick={() => setDeleting(true)}>
              {t("Delete my sync account")}
            </Button>
          </div>
          <p className="small faint measure">
            {ios
              ? t("Turning sync off keeps every visit on this iPhone. The sync keys stay in its protected storage.")
              : t("Turning sync off keeps every visit on this computer. The sync keys are kept in {where}.", {
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
      {adding && <AddDeviceSheet hub={!!s.hub} onClose={() => (setAdding(false), void refresh())} />}
      {removing && (
        <Dialog labelledBy="sync-remove-title" onClose={() => setRemoving(null)} dismissible={!busy}>
          {(close) => (
            <>
              <h2 id="sync-remove-title">
                {removing.this_device
                  ? ios
                    ? t("Remove this iPhone from sync?")
                    : t("Remove this computer from sync?")
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
      {deleting && (
        <DeleteAccountDialog
          confirmWord={s.hub ? "delete" : (s.email ?? "")}
          onClose={() => (setDeleting(false), void refresh())}
        />
      )}
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

type Step = "hub" | "kit" | "check" | "pair" | "confirm" | "recover" | "done";

/** The text a hub's QR code holds, pasted or scanned into the code field. */
export const isHubLink = (text: string) => text.trim().startsWith("openbark-hub:");

/**
 * Joined through a clinic hub and approved: nothing syncs until the person
 * types the code the approving device showed. It proves which account this
 * device ended up in; the devices of that account are listed so the person
 * can see whose it is. Start over leaves that account.
 */
function ConfirmJoin({
  status,
  onConfirmed,
  onStartOver,
}: {
  status: engine.SyncStatus;
  onConfirmed: () => void;
  onStartOver: () => void;
}) {
  const t = useT();
  const [code, setCode] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const names = (status.devices ?? [])
    .filter((d) => d.status === "approved" && !d.this_device)
    .map((d) => d.name || t("Unnamed device"));
  const act = async (f: () => Promise<unknown>, after: () => void) => {
    setError(null);
    setBusy(true);
    try {
      await f();
      after();
    } catch (e) {
      setError(message(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <form
      className="sync-confirm"
      onSubmit={(e) => {
        e.preventDefault();
        void act(() => engine.syncConfirmPairing(code.trim()), onConfirmed);
      }}
    >
      <p className="small measure">
        {t("A device added this one to its account. Before anything syncs, type the code that device showed.")}
      </p>
      {names.length > 0 && (
        <p className="small measure">
          {t("This device was added to the account with: {names}.", { names: names.join(", ") })}
        </p>
      )}
      {error && (
        <p className="ob-notice" data-tone="danger" role="alert">
          {error}
        </p>
      )}
      <label className="ob-field narrow">
        <span className="label">{t("Code from the device that added this one")}</span>
        <input
          className="ob-input mono"
          autoCapitalize="characters"
          autoComplete="off"
          spellCheck={false}
          placeholder="XXXX-XXXX"
          value={code}
          onChange={(e) => setCode(e.target.value)}
        />
      </label>
      <div className="ask-actions">
        <Button type="submit" variant="primary" disabled={busy || code.replace(/[\s-]/g, "").length !== 8}>
          {t("Confirm and start syncing")}
        </Button>
      </div>
      <p className="small muted measure">
        {t(
          "None of your devices added this one, or you don't know these devices? Start over: this device leaves that account and nothing is synced.",
        )}
      </p>
      <div className="ask-actions">
        <Button variant="ghost" disabled={busy} onClick={() => void act(() => engine.syncStartOver(), onStartOver)}>
          {t("Start over")}
        </Button>
      </div>
    </form>
  );
}

/** Turning sync on, step by step. */
function SyncSheet({ status, onClose }: { status: engine.SyncStatus; onClose: () => void }) {
  const t = useT();
  const app = useApp();
  const ios = isIos(app.status);
  const initial: Step =
    status.phase === "kit_check"
      ? "kit"
      : status.phase === "pending"
        ? "pair"
        : status.phase === "confirm"
          ? "confirm"
          : "hub";
  const [step, setStep] = useState<Step>(initial);
  const [latest, setLatest] = useState<engine.SyncStatus>(status);
  const [name, setName] = useState(status.device_name ?? "");
  const [backfill, setBackfill] = useState<number | null>(30);
  const [code, setCode] = useState("");
  const [address, setAddress] = useState(status.hub?.address ?? "");
  const [check, setCheck] = useState("");
  const [existing, setExisting] = useState(false);
  const [found, setFound] = useState<engine.FoundHub[] | null>(null);
  const [searching, setSearching] = useState(false);
  const [kit, setKit] = useState<engine.SyncKit | null>(status.kit ?? null);
  const [pairing, setPairing] = useState<engine.SyncPairing | null>(status.pairing ?? null);
  const [answers, setAnswers] = useState<[string, string]>(["", ""]);
  const [kitWords, setKitWords] = useState("");
  const [computerKey, setComputerKey] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  // While waiting to be added, check every few seconds.
  useEffect(() => {
    if (step !== "pair") return;
    const id = setInterval(() => {
      void engine
        .syncNow()
        .then((s) => {
          setLatest(s);
          if (s.phase === "active") setStep("done");
          // Approved through a hub: the code from the approving device first.
          if (s.phase === "confirm") setStep("confirm");
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

  const link = isHubLink(code);
  const codeReady = link || (code.replace(/[\s-]/g, "").length === 8 && address.trim() && check.replace(/[\s-]/g, "").length === 10);

  const findHubs = async () => {
    setError(null);
    setSearching(true);
    try {
      const hubs = await engine.syncFindHubs();
      setFound(hubs);
      if (hubs.length === 1 && hubs[0]) setAddress(`${hubs[0].host}:${hubs[0].port}`);
    } catch (e) {
      setError(message(e));
    } finally {
      setSearching(false);
    }
  };

  const join = () =>
    act(async () => {
      const next = await engine.syncJoinHub(
        code.trim(),
        link ? "" : address.trim(),
        link ? "" : check.trim(),
        name,
        backfill,
        existing,
      );
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
      : step === "pair" || step === "recover" || step === "confirm"
        ? t("Add this computer")
        : step === "done"
          ? t("Encrypted sync is on")
          : t("Join your clinic hub");

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

            {step === "hub" && (
              <form
                onSubmit={(e) => {
                  e.preventDefault();
                  void join();
                }}
              >
                <p className="small measure">
                  {t(
                    "On the hub computer, open Settings, Clinic hub. It shows a join code, its address and a certificate check. A code works once, for 30 minutes.",
                  )}
                </p>
                <label className="ob-field">
                  <span className="label">{t("Join code")}</span>
                  <input
                    className="ob-input mono"
                    autoCapitalize="characters"
                    autoComplete="off"
                    spellCheck={false}
                    placeholder="ABCD-EFGH"
                    value={code}
                    onChange={(e) => setCode(e.target.value)}
                  />
                  <span className="ob-help">{t("Or paste the hub's link, from its QR code.")}</span>
                </label>
                {!link && (
                  <>
                    <div className="ob-field">
                      <label className="label" htmlFor="hub-address">
                        {t("Hub address")}
                      </label>
                      <input
                        id="hub-address"
                        className="ob-input mono"
                        autoCapitalize="none"
                        autoComplete="off"
                        spellCheck={false}
                        placeholder="192.168.1.20:47110"
                        value={address}
                        onChange={(e) => setAddress(e.target.value)}
                      />
                      <div className="ask-actions">
                        <Button variant="ghost" disabled={busy || searching} onClick={() => void findHubs()}>
                          {searching ? t("Looking on this network") : t("Find on this network")}
                        </Button>
                      </div>
                      {found && found.length === 0 && (
                        <span className="ob-help">
                          {t("No hub answered. Some networks block this; enter the address shown on the hub.")}
                        </span>
                      )}
                      {found && found.length > 0 && (
                        <>
                          <ul className="hub-found">
                            {found.map((h) => (
                              <li key={`${h.name}-${h.host}`}>
                                <label className="check">
                                  <input
                                    type="radio"
                                    name="hub-found"
                                    checked={address === `${h.host}:${h.port}`}
                                    onChange={() => setAddress(`${h.host}:${h.port}`)}
                                  />
                                  <span>
                                    {h.name} <span className="mono small faint">{`${h.host}:${h.port}`}</span>
                                  </span>
                                </label>
                              </li>
                            ))}
                          </ul>
                          <span className="ob-help">
                            {t("A hub found here is only a hint. The join code and the certificate check decide which hub this device trusts.")}
                          </span>
                        </>
                      )}
                    </div>
                    <label className="ob-field narrow">
                      <span className="label">{t("Certificate check for this code")}</span>
                      <input
                        className="ob-input mono"
                        autoCapitalize="characters"
                        autoComplete="off"
                        spellCheck={false}
                        placeholder="XXXXX-XXXXX"
                        value={check}
                        onChange={(e) => setCheck(e.target.value)}
                      />
                      <span className="ob-help">
                        {t("Shown on the hub beside this join code. It changes with every code.")}
                      </span>
                    </label>
                  </>
                )}
                <fieldset className="ob-field hub-who">
                  <legend className="label">{t("Is this your first device?")}</legend>
                  <label className="check">
                    <input type="radio" name="hub-existing" checked={!existing} onChange={() => setExisting(false)} />
                    <span>{t("This is my first device")}</span>
                  </label>
                  <label className="check">
                    <input type="radio" name="hub-existing" checked={existing} onChange={() => setExisting(true)} />
                    <span>{t("I already use OpenBark on another device")}</span>
                  </label>
                  <span className="ob-help">
                    {existing
                      ? t("That device approves this one: it shows 6 words to type there.")
                      : t("This device makes your account and its Recovery Kit.")}
                  </span>
                </fieldset>
                <label className="ob-field">
                  <span className="label">{ios ? t("This iPhone's name") : t("This computer's name")}</span>
                  <input
                    className="ob-input"
                    value={name}
                    placeholder={ios ? t("Dr Rivera's iPhone") : t("Front desk PC")}
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
                  <Button type="submit" variant="primary" disabled={busy || !codeReady}>
                    {t("Join the hub")}
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
                  <Button
                    variant="ghost"
                    disabled={busy}
                    onClick={() =>
                      void act(async () => {
                        await engine.syncStartOver();
                        close();
                      })
                    }
                  >
                    {t("Start over")}
                  </Button>
                </div>
              </>
            )}

            {step === "confirm" && (
              <ConfirmJoin status={latest} onConfirmed={() => setStep("done")} onStartOver={close} />
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
function AddDeviceSheet({ hub, onClose }: { hub: boolean; onClose: () => void }) {
  const t = useT();
  const app = useApp();
  const [words, setWords] = useState("");
  const [found, setFound] = useState<engine.SyncPending | null>(null);
  /** Added through a hub: the code to type on the new device. */
  const [confirm, setConfirm] = useState<string | null>(null);
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
              {hub
                ? t(
                    "On the new device, join the clinic hub with a new join code and choose I already use OpenBark on another device. It shows 6 words. Type them here: OpenBark checks they match that device before giving it the keys.",
                  )
                : t(
                    "On the new device, turn on encrypted sync and sign in with the same email. It shows 6 words. Type them here: OpenBark checks they match that device before giving it the keys.",
                  )}
            </p>
            {error && (
              <p className="ob-notice" data-tone="danger" role="alert">
                {error}
              </p>
            )}
            {confirm ? (
              <>
                <p className="small measure">
                  {t("{name} was added. Type this code on the new device:", { name: found?.name ?? "" })}
                </p>
                <p className="pair-words mono" aria-label={t("Code for the new device")}>
                  {confirm}
                </p>
                <p className="small muted measure">
                  {t(
                    "It shows the new device which account it joined. Until it is typed there, the new device syncs nothing.",
                  )}
                </p>
                <div className="ask-actions">
                  <Button variant="primary" onClick={close}>
                    {t("Done")}
                  </Button>
                </div>
              </>
            ) : !found ? (
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
                        const added = await engine.syncPair(words, true);
                        if (added.confirm) {
                          setConfirm(added.confirm);
                        } else {
                          app.toast(t("{name} added", { name: found.name }));
                          close();
                        }
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

/** `confirmWord`: the account's email, or on a clinic hub the word "delete". */
function DeleteAccountDialog({ confirmWord, onClose }: { confirmWord: string; onClose: () => void }) {
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
            <span className="label">
              {confirmWord === "delete" ? t("Type delete to confirm") : t("Type {email} to confirm", { email: confirmWord })}
            </span>
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
              disabled={busy || typed.trim().toLowerCase() !== confirmWord.toLowerCase()}
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
