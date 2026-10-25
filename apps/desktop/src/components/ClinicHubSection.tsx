import { Button, Dialog } from "@openbark/ui";
import { useCallback, useEffect, useState } from "react";
import { useApp } from "../context";
import * as engine from "../engine";
import { useT } from "../i18n";

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

type T = (text: string, vars?: Record<string, string | number>) => string;

function platformName(t: T, p: string): string {
  const names: Record<string, string> = { windows: "Windows", macos: "Mac", linux: "Linux", ios: "iPhone", android: "Android" };
  return names[p] ?? t("Other");
}

function statusName(t: T, s: engine.ClinicHubDevice["status"]): string {
  return s === "approved" ? t("Joined") : s === "pending" ? t("Waiting to be approved") : t("Removed");
}

/** "one per line, or separated by commas" into a list. */
export const parseRanges = (text: string) =>
  text
    .split(/[\n,]/)
    .map((s) => s.trim())
    .filter(Boolean);

/**
 * Settings > Clinic hub (ADR-0029): this computer as the clinic's sync hub.
 * Off by default. The hub keeps encrypted copies for the clinic's other
 * devices and shows the join codes they need. It knows no device names:
 * those are sealed in each person's device list.
 */
export function ClinicHubSection() {
  const app = useApp();
  const t = useT();
  const [s, setS] = useState<engine.ClinicHubStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [removing, setRemoving] = useState<engine.ClinicHubDevice | null>(null);
  const [port, setPort] = useState("");
  const [ranges, setRanges] = useState("");
  const [quotaGb, setQuotaGb] = useState("");
  const [existing, setExisting] = useState(false);

  const refresh = useCallback(() => engine.clinicHubStatus().then(setS, (e) => setError(message(e))), []);
  useEffect(() => {
    void refresh();
    const timer = setInterval(() => void refresh(), 5000);
    return () => clearInterval(timer);
  }, [refresh]);
  // The form starts from the saved settings (and again after each save).
  const savedPort = s?.port;
  const savedRanges = (s?.allowed ?? []).join("\n");
  const savedQuota = s?.quota_mb;
  useEffect(() => {
    if (savedPort !== undefined) setPort(String(savedPort));
    setRanges(savedRanges);
    if (savedQuota !== undefined) setQuotaGb(String(Math.round((savedQuota / 1024) * 10) / 10));
  }, [savedPort, savedRanges, savedQuota]);

  if (!s) {
    return (
      <section className="group">
        <h2>{t("Clinic hub")}</h2>
        <div className="ob-skeleton" style={{ height: 80 }} />
      </section>
    );
  }
  if (!s.available) return null;

  const run = async (f: () => Promise<engine.ClinicHubStatus | void>, done?: string) => {
    setError(null);
    setBusy(true);
    try {
      const next = await f();
      if (next) setS(next);
      if (done) app.toast(done);
    } catch (e) {
      setError(message(e));
    } finally {
      setBusy(false);
      void refresh();
    }
  };

  const join = s.join;
  const live = join && !join.used && new Date(join.expires_at).getTime() > Date.now();
  const devices = s.devices ?? [];
  const saveSettings = () => {
    const gb = Number(quotaGb.replace(",", "."));
    return run(
      () =>
        engine.clinicHubSettings({
          port: Number(port),
          allowed: parseRanges(ranges),
          quotaMb: Number.isFinite(gb) ? Math.max(1, Math.round(gb * 1024)) : undefined,
        }),
      t("Hub settings saved"),
    );
  };

  return (
    <section className="group hub-section">
      <h2>
        {t("Clinic hub")} <span className="ob-tag">{t("Preview")}</span>
      </h2>
      <p className="small measure">
        {t(
          "The clinic's other devices sync through this computer. It keeps encrypted copies it can't read, except for the accounts of people who use this computer themselves.",
        )}
      </p>
      <p className="small muted measure">
        {t(
          "Use a staff-only computer with full-disk encryption, and leave it on: devices sync only while it's on and they're on the clinic network.",
        )}
      </p>
      {error && (
        <p className="ob-notice" data-tone="danger" role="alert">
          {error}
        </p>
      )}
      <label className="opt-row">
        <span className="opt-text">
          <span>{t("Make this computer the clinic hub")}</span>
          <span className="small muted">{t("Off by default. Nothing listens on the network while it's off.")}</span>
        </span>
        <input
          type="checkbox"
          className="opt-switch"
          checked={s.on}
          disabled={busy}
          onChange={(e) =>
            void run(
              () => engine.clinicHubSettings({ on: e.target.checked }),
              e.target.checked ? t("Hub mode is on") : t("Hub mode is off"),
            )
          }
        />
      </label>

      {s.on && (
        <>
          <p className="small measure">
            <span className="ob-dot" data-tone={s.listening ? "ok" : "warn"} aria-hidden />{" "}
            {s.listening
              ? t("Listening on port {port}.", { port: s.port ?? 0 })
              : t("Not listening.")}{" "}
            {(s.addresses ?? []).length > 0 &&
              t("This computer's address: {addresses}.", { addresses: (s.addresses ?? []).join(", ") })}
          </p>
          {s.error && (
            <p className="ob-notice" data-tone="warn">
              {s.error}
            </p>
          )}

          <h3 className="label">{t("Join code")}</h3>
          {live && join ? (
            <div className="hub-join">
              {/* The QR comes from the engine as SVG (qrcode crate). */}
              <div className="kit-qr" aria-hidden dangerouslySetInnerHTML={{ __html: join.qr_svg }} />
              <div className="hub-join-text">
                <p className="pair-words mono" aria-label={t("Join code")}>
                  {join.code}
                </p>
                <p className="small">
                  {t("Address")}: <span className="mono">{join.address}</span>
                </p>
                <p className="small">
                  {t("Certificate check")}: <span className="mono">{join.check}</span>
                </p>
                <p className="small faint">
                  {t("Works once, until {time}.", { time: engine.fmtStamp(join.expires_at) })}
                </p>
              </div>
            </div>
          ) : (
            <p className="small muted measure">
              {join?.used
                ? t("The last join code was used. Make a new one for the next device.")
                : t(
                    "A device joins with a join code from this screen: scan the QR code, or type the code, the address and the certificate check.",
                  )}
            </p>
          )}
          <div className="ask-actions">
            <Button variant="primary" disabled={busy || !s.listening} onClick={() => void run(() => engine.clinicHubCode())}>
              {t("New join code")}
            </Button>
          </div>

          <h3 className="label">{t("Connected devices")}</h3>
          {devices.length === 0 ? (
            <p className="small muted">{t("No device has joined yet.")}</p>
          ) : (
            <ul className="sync-devices">
              {devices.map((d) => (
                <li key={d.id} className="storage-row">
                  <div className="storage-text">
                    <span className="label">
                      {platformName(t, d.platform)} <span className="mono small faint">{d.id.slice(0, 8)}</span>
                    </span>
                    <span className="small faint">
                      {statusName(t, d.status)}
                      {d.account ? `, ${t("account {id}", { id: d.account })}` : ""}
                      {d.last_seen_at ? `, ${t("last seen {when}", { when: engine.fmtStamp(d.last_seen_at) })}` : ""}
                    </span>
                  </div>
                  {d.status !== "revoked" && (
                    <Button variant="ghost" onClick={() => setRemoving(d)}>
                      {t("Remove")}
                    </Button>
                  )}
                </li>
              ))}
            </ul>
          )}
          {s.accounts !== undefined && (
            <p className="small faint">
              {t("{n} accounts, {mb} MB of encrypted copies.", { n: s.accounts, mb: s.used_mb ?? 0 })}
            </p>
          )}

          <h3 className="label">{t("Use this computer with its hub")}</h3>
          <p className="small muted measure">
            {t("This computer can sync through its own hub, like any other device.")}
          </p>
          <fieldset className="ob-field hub-who">
            <legend className="label">{t("Is this your first device?")}</legend>
            <label className="check">
              <input type="radio" name="hub-self" checked={!existing} onChange={() => setExisting(false)} />
              <span>{t("This is my first device")}</span>
            </label>
            <label className="check">
              <input type="radio" name="hub-self" checked={existing} onChange={() => setExisting(true)} />
              <span>{t("I already use OpenBark on another device")}</span>
            </label>
          </fieldset>
          <div className="ask-actions">
            <Button
              variant="ghost"
              disabled={busy || !s.listening}
              onClick={() =>
                void run(async () => {
                  await engine.clinicHubJoinSelf("", existing);
                  app.toast(t("Joined. Finish in Encrypted sync."));
                  app.navigate({ screen: "settings", section: "sync" });
                })
              }
            >
              {t("Sync this computer through its hub")}
            </Button>
          </div>

          <h3 className="label">{t("Hub settings")}</h3>
          <form
            className="hub-settings"
            onSubmit={(e) => {
              e.preventDefault();
              void saveSettings();
            }}
          >
            <label className="ob-field narrow">
              <span className="label">{t("Port")}</span>
              <input
                className="ob-input mono"
                inputMode="numeric"
                value={port}
                onChange={(e) => setPort(e.target.value.replace(/\D/g, ""))}
              />
            </label>
            <label className="ob-field">
              <span className="label">{t("Allowed addresses")}</span>
              <textarea
                className="ob-input tb-text mono"
                rows={4}
                spellCheck={false}
                value={ranges}
                onChange={(e) => setRanges(e.target.value)}
              />
              <span className="ob-help">
                {t(
                  "Address ranges that may connect, one per line. The default allows private networks only, never the internet.",
                )}
              </span>
            </label>
            <label className="ob-field narrow">
              <span className="label">{t("Storage per person (GB)")}</span>
              <input
                className="ob-input mono"
                inputMode="decimal"
                value={quotaGb}
                onChange={(e) => setQuotaGb(e.target.value)}
              />
            </label>
            <div className="ask-actions">
              <Button type="submit" variant="ghost" disabled={busy || !port || parseRanges(ranges).length === 0}>
                {t("Save hub settings")}
              </Button>
            </div>
          </form>
        </>
      )}

      {removing && (
        <Dialog labelledBy="hub-remove-title" onClose={() => setRemoving(null)} dismissible={!busy}>
          {(close) => (
            <>
              <h2 id="hub-remove-title">{t("Remove this device from the hub?")}</h2>
              <p>
                {t(
                  "It's refused at once. Its account's keys change the next time another of that person's devices syncs. Its own visits stay on it.",
                )}
              </p>
              <div className="ask-actions">
                <Button
                  variant="danger"
                  disabled={busy}
                  onClick={() =>
                    void run(() => engine.clinicHubRemoveDevice(removing.id), t("Device removed")).then(close)
                  }
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
    </section>
  );
}
