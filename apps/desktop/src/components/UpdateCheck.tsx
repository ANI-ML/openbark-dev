import { Button } from "@openbark/ui";
import { useEffect, useRef, useState } from "react";
import { useApp, type Settings } from "../context";
import * as engine from "../engine";
import { useT } from "../i18n";
import { isIos } from "../visits";

/**
 * The version check (ADR-0035, docs/network.md R16). OpenBark asks once
 * whether to check openbark.io for new versions; the automatic check stays
 * off until the person says yes. The engine makes the request (never this
 * page) and reaches openbark.io at most once a day. Nothing is downloaded
 * or installed: the notice links to the release notes and the website's
 * download section. Never on the iPhone, where the App Store and
 * TestFlight handle updates.
 */

/** The automatic check asks the engine this often; the engine itself waits 24 hours between requests. */
export const UPDATE_POLL_MS = 60 * 60 * 1000;
/** How long the "is available" notice stays. */
export const UPDATE_TOAST_MS = 12_000;

/** Updates are offered here: a computer (desktop or web mode), never a phone. */
export function updatesOffered(status: engine.Status | null): boolean {
  return status !== null && !isIos(status) && !engine.isMobilePlatform(status.platform);
}

/**
 * The settings an answer to the question stores. For the card here and for
 * any other place that asks the same question (setup): `updateCheck` is the
 * one source of truth.
 */
export function updateAnswer(on: boolean): Partial<Settings> {
  return { updateCheck: on ? "on" : "off", updateAskedAt: new Date().toISOString() };
}

/** `a` is a newer MAJOR.MINOR.PATCH than `b` (number by number: 0.2.10 is newer than 0.2.9). */
export function newerVersion(a: string, b: string | null): boolean {
  const parse = (v: string) => {
    const m = /^v?(\d{1,9})\.(\d{1,9})\.(\d{1,9})$/.exec(v.trim());
    return m ? [Number(m[1]), Number(m[2]), Number(m[3])] : null;
  };
  const x = parse(a);
  if (!x) return false;
  const y = b === null ? null : parse(b);
  if (!y) return true;
  for (let i = 0; i < 3; i++) {
    if (x[i]! !== y[i]!) return x[i]! > y[i]!;
  }
  return false;
}

/**
 * The question, worded once. `onAnswer(true)`: "Yes, check daily". Both
 * buttons have the same weight: either answer is fine.
 */
export function UpdateQuestion({ onAnswer, titleId }: { onAnswer: (on: boolean) => void; titleId?: string }) {
  const t = useT();
  return (
    <>
      <h2 id={titleId}>{t("Check for new versions?")}</h2>
      <p className="small muted">
        {t(
          "Once a day, OpenBark reads the version number from openbark.io. Nothing about you, your clinic or your patients is sent.",
        )}
      </p>
      <div className="ask-actions">
        <Button onClick={() => onAnswer(true)}>{t("Yes, check daily")}</Button>
        <Button onClick={() => onAnswer(false)}>{t("No thanks")}</Button>
      </div>
    </>
  );
}

/**
 * Show the "is available" notice for a newer version: once per version
 * (shown is seen, whether it was used, closed or left to time out), unless
 * `again` (the person pressed Check for updates).
 */
export function useUpdateNotice() {
  const app = useApp();
  const t = useT();
  // The latest settings, for checks that finish after a re-render.
  const latest = useRef(app);
  latest.current = app;
  return (view: engine.UpdateView, again = false) => {
    const { settings, setSettings, toast } = latest.current;
    if (!view.newer || !view.latest) return;
    if (!again && !newerVersion(view.latest, settings.updateNotified)) return;
    if (newerVersion(view.latest, settings.updateNotified)) setSettings({ updateNotified: view.latest });
    toast(
      t("OpenBark {version} is available", { version: view.latest }),
      [
        { label: t("What's new"), run: () => void engine.openReleaseLink("notes", view).catch(() => {}) },
        { label: t("Download"), run: () => void engine.openReleaseLink("download", view).catch(() => {}) },
      ],
      UPDATE_TOAST_MS,
    );
  };
}

/**
 * The question card (once, after setup and consent: `ready`) and the
 * automatic check while the setting is on. Rendered once by the app shell.
 */
export function UpdateChecks({ ready }: { ready: boolean }) {
  const app = useApp();
  const s = app.settings;
  const offered = updatesOffered(app.status);
  const notify = useUpdateNotice();
  const notifyRef = useRef(notify);
  notifyRef.current = notify;

  // Shown once: remembered the moment it appears. Ignored, it leaves the
  // check at "ask", which checks nothing.
  const [asking, setAsking] = useState(false);
  const due = offered && ready && s.updateCheck === "ask" && !s.updateAskedAt;
  useEffect(() => {
    if (!due) return;
    setAsking(true);
    app.setSettings({ updateAskedAt: new Date().toISOString() });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [due]);

  // The automatic check: at start and then hourly while open, only while on.
  // The engine asks openbark.io only when 24 hours have passed since its
  // last request, and fails quietly offline.
  const on = offered && s.updateCheck === "on";
  useEffect(() => {
    if (!on) return;
    let live = true;
    const check = () =>
      engine.checkForUpdate(false).then(
        (view) => {
          if (live) notifyRef.current(view);
        },
        () => {},
      );
    void check();
    const timer = window.setInterval(() => void check(), UPDATE_POLL_MS);
    return () => {
      live = false;
      window.clearInterval(timer);
    };
  }, [on]);

  if (!(asking && offered && s.updateCheck === "ask")) return null;
  return (
    <section className="update-ask" role="region" aria-labelledby="update-ask-title">
      <UpdateQuestion titleId="update-ask-title" onAnswer={(yes) => app.setSettings(updateAnswer(yes))} />
    </section>
  );
}

/** Settings, About: the version, Check for updates, and the daily switch. */
export function UpdateSettings() {
  const app = useApp();
  const t = useT();
  const s = app.settings;
  const notify = useUpdateNotice();
  const [checking, setChecking] = useState(false);
  const [found, setFound] = useState<engine.UpdateView | null>(null);
  const [failed, setFailed] = useState(false);
  const offered = updatesOffered(app.status);

  // What the last check found; no request.
  useEffect(() => {
    if (!offered) return;
    engine.updateStatus().then(
      (v) => setFound((prev) => prev ?? v),
      () => {},
    );
  }, [offered]);

  if (!offered) return null;

  async function checkNow() {
    setChecking(true);
    setFailed(false);
    try {
      const view = await engine.checkForUpdate(true);
      setFound(view);
      notify(view, true);
    } catch {
      setFailed(true);
    } finally {
      setChecking(false);
    }
  }

  const when = found?.checked_at ? engine.fmtStamp(new Date(found.checked_at * 1000).toISOString()) : null;
  return (
    <div className="update-settings">
      <h3>{t("Updates")}</h3>
      <div className="opt-list">
        <label className="opt-row">
          <span className="opt-text">
            <span>{t("Check for new versions once a day")}</span>
            <span className="small muted">
              {t(
                "Once a day, OpenBark reads the version number from openbark.io. Nothing about you, your clinic or your patients is sent.",
              )}
            </span>
          </span>
          <input
            type="checkbox"
            className="opt-switch"
            checked={s.updateCheck === "on"}
            onChange={(e) => app.setSettings(updateAnswer(e.target.checked))}
          />
        </label>
      </div>
      <div className="ask-actions">
        <Button onClick={() => void checkNow()} disabled={checking}>
          {checking ? t("Checking…") : t("Check for updates")}
        </Button>
      </div>
      {failed ? (
        <p className="ob-notice" data-tone="danger" role="alert">
          {t("OpenBark couldn't reach openbark.io. Check the internet connection and try again.")}
        </p>
      ) : found?.newer && found.latest ? (
        <div className="ob-notice" role="status">
          <span>{t("OpenBark {version} is available", { version: found.latest })}</span>
          <span className="notice-actions">
            <Button size="compact" variant="ghost" onClick={() => void engine.openReleaseLink("notes", found)}>
              {t("What's new")}
            </Button>
            <Button size="compact" variant="ghost" onClick={() => void engine.openReleaseLink("download", found)}>
              {t("Download")}
            </Button>
          </span>
        </div>
      ) : found?.checked ? (
        <p className="ob-notice" data-tone="ok" role="status">
          {t("You have the latest version")}
        </p>
      ) : null}
      {when && <p className="small faint">{t("Last checked {date}.", { date: when })}</p>}
    </div>
  );
}
