import { Lockup, Mark, createLevelSource } from "@openbark/ui";
import {
  Box,
  CircleHelp,
  ClipboardList,
  Mic,
  NotebookText,
  PawPrint,
  Settings as SettingsIcon,
} from "lucide-react";
import { AnimatePresence, LazyMotion, MotionConfig, m } from "motion/react";
import { useCallback, useEffect, useMemo, useState } from "react";
import * as engine from "./engine";
import { ConsentNotice } from "./components/ConsentNotice";
import { PhoneWalkthrough, walkthroughDue } from "./components/PhoneWalkthrough";
import { SetupWalkthrough } from "./components/SetupWalkthrough";
import { Toast } from "./components/Toast";
import { UpdateChecks } from "./components/UpdateCheck";
import { ROLE_LABEL, UserPicker } from "./components/UserPicker";
import {
  AppContext,
  loadSettings,
  permissions,
  saveSettings,
  type Settings,
  type ToastAction,
  type ToastOptions,
} from "./context";
import { translate } from "./i18n";
import { loadMotionFeatures, SPRING } from "./motion";
import { hashToRoute, routeToHash, type Route } from "./routes";
import { PatientDetail } from "./screens/PatientDetail";
import { Patients } from "./screens/Patients";
import { Models } from "./screens/Models";
import { Recorder } from "./screens/Recorder";
import { Review } from "./screens/Review";
import { SettingsScreen } from "./screens/Settings";
import { Templates } from "./screens/Templates";
import { Help } from "./screens/Help";
import { Visits } from "./screens/Visits";
import { setupDone, setupMode } from "./setup";
import { isIos, needsYou } from "./visits";

export type { Route } from "./routes";

/**
 * The left rail (DESIGN.md §9: 240 px) and, on phones, the tab bar
 * (ADR-0027). The rail lists Record, Visits, Patients and Templates, then
 * Models, Settings and Help as a quieter group at the bottom. The phone tab
 * bar shows Record, Visits, Patients and Settings (`phone`); Templates,
 * Models and Help are rows in Settings there. e2e scripts address items by
 * their `data-nav` name.
 */
const NAV = [
  { screen: "record", label: "Record", icon: Mic, phone: true, group: "primary" },
  { screen: "visits", label: "Visits", icon: ClipboardList, phone: true, group: "primary" },
  { screen: "patients", label: "Patients", icon: PawPrint, phone: true, group: "primary" },
  { screen: "templates", label: "Templates", icon: NotebookText, phone: false, group: "primary" },
  { screen: "models", label: "Models", icon: Box, phone: false, group: "secondary" },
  { screen: "settings", label: "Settings", icon: SettingsIcon, phone: true, group: "secondary" },
  { screen: "help", label: "Help", icon: CircleHelp, phone: false, group: "secondary" },
] as const;

/** The destination a screen belongs to, for the rail's marker. */
function activeOf(route: Route): string {
  if (route.screen === "review") return "visits";
  if (route.screen === "patient") return "patients";
  return route.screen;
}

export function App() {
  const [route, setRoute] = useState<Route>(() =>
    typeof window === "undefined" ? { screen: "record" } : hashToRoute(window.location.hash),
  );
  const [status, setStatus] = useState<engine.Status | null>(null);
  const [settings, setSettingsState] = useState<Settings>(loadSettings);
  const [toast, setToastState] = useState<
    { n: number; message: string; action?: ToastAction; options?: ToastOptions } | null
  >(null);
  const showToast = useCallback(
    (message: string, action?: ToastAction, options?: ToastOptions) =>
      setToastState((prev) => ({ n: (prev?.n ?? 0) + 1, message, action, options })),
    [],
  );
  const closeToast = useCallback(() => setToastState(null), []);
  const [announce, setAnnounce] = useState("");
  const levels = useMemo(() => createLevelSource(), []);
  /** Visits nobody has signed yet: the count beside Visits. */
  const [waiting, setWaiting] = useState(0);

  /** Go somewhere, with a history entry so Back returns here. */
  const navigate = useCallback((next: Route) => {
    setRoute(next);
    const hash = routeToHash(next);
    try {
      if (window.location.hash !== hash) window.history.pushState(null, "", hash);
    } catch {
      // History can be unavailable (sandboxed frames); the screen still changes.
    }
  }, []);

  // Back and Forward: the hash names the screen.
  useEffect(() => {
    const onPop = () => setRoute(hashToRoute(window.location.hash));
    window.addEventListener("popstate", onPop);
    return () => window.removeEventListener("popstate", onPop);
  }, []);

  // Each destination opens at its top, not at the previous screen's scroll position.
  useEffect(() => {
    window.scrollTo(0, 0);
  }, [route]);

  // The count beside Visits, fresh whenever the screen changes.
  useEffect(() => {
    engine.listSessions().then(
      (rows) => setWaiting(rows.filter(needsYou).length),
      () => {},
    );
  }, [route]);

  // Back in the foreground (the phone opened again on the clinic's Wi-Fi):
  // sync now rather than at the next poll (ADR-0029 section 6). Does nothing
  // while sync is off.
  useEffect(() => {
    const onVisible = () => {
      if (document.visibilityState === "visible") engine.syncNow().catch(() => {});
    };
    document.addEventListener("visibilitychange", onVisible);
    return () => document.removeEventListener("visibilitychange", onVisible);
  }, []);

  const refreshStatus = useCallback(() => {
    engine.status().then(setStatus, () => setStatus(null));
  }, []);

  // A phone writes notes with its tier's small model unless the person chose
  // one of the phone's own (ADR-0018 section 10).
  useEffect(() => {
    if (status?.profile !== "ios" || status.default_llm === undefined) return;
    const chosen = settings.llm;
    if (chosen === "gemma-4-e4b-it-q4_0" || chosen === "gemma-4-12b-it-q4_0" || chosen === "") {
      if (status.default_llm && status.default_llm !== chosen) setSettings({ llm: status.default_llm });
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [status?.profile, status?.default_llm]);

  // A computer starts with the note model that fits its memory (ADR-0030):
  // Gemma 4 E4B, or E2B where E4B won't fit. Only a setting still on the old
  // default whose model was never downloaded follows it; a computer that
  // already drafts with E4B keeps it (Models and Record suggest switching).
  useEffect(() => {
    if (!status || status.profile === "ios" || !status.default_llm) return;
    // A model chosen in setup (ADR-0034) is the person's choice: never swapped.
    if (settings.noteModelChosenAt) return;
    if (settings.llm !== "gemma-4-e4b-it-q4_0" || status.default_llm === settings.llm) return;
    const wanted = status.default_llm;
    engine.models().then(
      (rows) => {
        if (!rows.some((r) => r.id === "gemma-4-e4b-it-q4_0" && r.installed)) setSettings({ llm: wanted });
      },
      () => {},
    );
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [status?.profile, status?.default_llm]);

  useEffect(() => {
    refreshStatus();
    const off = engine.on("level", (l) => levels.push(l.level));
    return () => {
      void off.then((f) => f());
    };
  }, [refreshStatus, levels]);

  const setSettings = (s: Partial<Settings>) =>
    setSettingsState((prev) => {
      const next = { ...prev, ...s };
      saveSettings(next);
      return next;
    });

  // The team member using the app (M1 item 11), remembered on this device.
  const [user, setUserState] = useState<engine.User | null>(() => {
    try {
      return JSON.parse(localStorage.getItem("openbark.user") ?? "null") as engine.User | null;
    } catch {
      return null;
    }
  });
  const setUser = (u: engine.User | null) => {
    setUserState(u);
    try {
      if (u) localStorage.setItem("openbark.user", JSON.stringify(u));
      else localStorage.removeItem("openbark.user");
    } catch {
      // Non-fatal: they choose again next time.
    }
  };
  const team = Boolean(status?.team);
  const acting = team ? user : null;
  const actor = team ? (user?.id ?? "") : settings.signer;
  const actorName = team ? (user?.name ?? "") : settings.signer;
  engine.setActor(actor);

  // A remembered team member who has since been removed must choose again;
  // one whose admin flag changed gets the change.
  useEffect(() => {
    if (!team || !user) return;
    engine.listUsers().then(
      (list) => {
        const now = list.find((u) => u.id === user.id);
        if (!now) setUser(null);
        else if (Boolean(now.admin) !== Boolean(user.admin) || now.role !== user.role) setUser(now);
      },
      () => {},
    );
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [team, user?.id]);

  const ctx = {
    user: acting,
    setUser,
    actor,
    actorName,
    can: permissions(team ? (user?.role ?? "reception") : null, Boolean(team && user?.admin)),
    route,
    navigate,
    status,
    refreshStatus,
    settings,
    setSettings,
    toast: showToast,
    announce: setAnnounce,
    levels,
  };
  const active = activeOf(route);
  /** The computer's setup walkthrough (ADR-0034), once consent is given and, with a team, someone is chosen. */
  const setup = setupMode(status, settings, team && !user);
  const t = (text: string, vars?: Record<string, string | number>) => translate(settings.uiLanguage, text, vars);
  /** On a phone, Templates, Models and Help open from Settings, so its tab stays marked. */
  const underSettings = active === "templates" || active === "models" || active === "help";

  useEffect(() => {
    document.documentElement.lang = settings.uiLanguage;
    // The second speech recogniser expects the app's language (ADR-0031),
    // unless a spoken language is set (below).
    engine.setSpeechLanguage(settings.uiLanguage).catch(() => {});
  }, [settings.uiLanguage]);

  useEffect(() => {
    // Settings, Recording, Spoken language; null is Auto (ADR-0033).
    engine.setSpokenLanguage(settings.spokenLanguage).catch(() => {});
  }, [settings.spokenLanguage]);

  // A recorded call was saved in the phone window: open it, drafted with the
  // Phone call template when "Draft the note when I stop recording" is on.
  const draftOnStop = settings.draftOnStop;
  useEffect(
    () =>
      engine.onCallSaved((id) => {
        // Review picks the Phone call template for a call by itself.
        navigate({ screen: "review", id, draft: draftOnStop });
        setAnnounce(translate(settings.uiLanguage, "Call saved"));
      }),
    [draftOnStop, settings.uiLanguage, navigate],
  );

  return (
    // Motion (docs/plans/design-motion.md): the light `m` components, with
    // the animation and layout features (domMax) loaded in their own chunk;
    // reducedMotion="user" stops transform and layout animation when the
    // system asks for reduced motion.
    <LazyMotion features={loadMotionFeatures} strict>
    <MotionConfig reducedMotion="user">
    <AppContext.Provider value={ctx}>
      <div className="frame" data-profile={isIos(status) ? "ios" : undefined}>
        <nav className="sidebar" aria-label="Main">
          {/* The horizontal lockup in the full rail; the mark alone in the narrow rail (§2.3).
              Wrapped so app.css's .brand-full / .brand-compact switch hides the whole element. */}
          <div className="brand">
            <span className="brand-full">
              <Lockup size={28} />
            </span>
            <span className="brand-compact">
              <Mark size={28} title="OpenBark" />
            </span>
          </div>
          <ul>
            {NAV.map(({ screen, label: english, icon: Icon, phone, group }, i) => {
              const label = t(english);
              const count = screen === "visits" && waiting > 0 ? waiting : 0;
              const name = count ? `${label}, ${t("{n} need you", { n: count })}` : label;
              const first = group === "secondary" && NAV[i - 1]?.group === "primary";
              return (
                <li
                  key={screen}
                  data-phone={phone ? undefined : "no"}
                  className={[group === "secondary" ? "nav-secondary" : "", first ? "nav-group-start" : ""]
                    .filter(Boolean)
                    .join(" ") || undefined}
                >
                  <button
                    type="button"
                    className="nav-item"
                    data-nav={screen}
                    aria-label={name}
                    title={name}
                    aria-current={active === screen ? "page" : undefined}
                    data-parent-current={screen === "settings" && underSettings ? "" : undefined}
                    onClick={() => navigate({ screen } as Route)}
                  >
                    {/* One 2 px ink marker that glides to the new destination. */}
                    {active === screen && (
                      <m.span layoutId="nav-marker" className="nav-marker" transition={SPRING} aria-hidden />
                    )}
                    <Icon size={20} aria-hidden />
                    <span className="nav-label">{label}</span>
                    {count > 0 && (
                      <span className="nav-badge mono" aria-hidden>
                        {count}
                      </span>
                    )}
                  </button>
                </li>
              );
            })}
          </ul>
          <div className="sidebar-foot small faint">
            {acting && (
              <button
                type="button"
                className="link small who-now"
                onClick={() => setUser(null)}
                title={t("Switch user")}
              >
                <span className="nav-label">
                  {acting.name}, {t(ROLE_LABEL[acting.role])}. {t("Switch")}
                </span>
              </button>
            )}
            <span className="nav-label running-on">
              <span className="ob-dot" data-tone="ok" aria-hidden />
              {t(engine.isDesktop ? "This device" : "Web, this computer only")}
            </span>
          </div>
        </nav>
        <main className="content">
          {route.screen === "record" && <Recorder key={route.patient ?? ""} patientId={route.patient} />}
          {route.screen === "visits" && <Visits />}
          {route.screen === "patients" && <Patients />}
          {route.screen === "patient" && <PatientDetail key={route.id} id={route.id} />}
          {route.screen === "review" && (
            <Review
              key={`${route.id}:${route.template ?? ""}:${route.pet ?? ""}`}
              id={route.id}
              autoDraft={route.draft ?? false}
              draftTemplate={route.template}
              initialPet={route.pet}
            />
          )}
          {route.screen === "templates" && (
            <Templates key={`${route.open ?? ""}:${route.heading ?? ""}`} initialOpen={route.open} initialHeading={route.heading} />
          )}
          {route.screen === "help" && <Help />}
          {route.screen === "models" && <Models />}
          {route.screen === "settings" && <SettingsScreen key={route.section ?? ""} section={route.section} />}
        </main>
      </div>
      {/* The iPhone's first run is the walkthrough, which asks for the consent
          agreement itself; skipped before that step, the notice asks after it. */}
      {walkthroughDue(status, settings) ? (
        <PhoneWalkthrough />
      ) : (
        !settings.consentAcknowledgedAt && (
          <ConsentNotice onAccept={() => setSettings({ consentAcknowledgedAt: new Date().toISOString() })} />
        )
      )}
      {settings.consentAcknowledgedAt && team && !user && !walkthroughDue(status, settings) && (
        <UserPicker
          onChosen={(u) => {
            setUser(u);
            refreshStatus();
          }}
        />
      )}
      {setup && <SetupWalkthrough key={setup} mode={setup} />}
      {/* The version check (ADR-0035): asked once, after the consent notice
          (and choosing who is using the app) and once setup is done, so never
          over the setup walkthrough (ADR-0034); never on the iPhone; the daily
          check only once the person said yes. */}
      <UpdateChecks
        ready={
          Boolean(settings.consentAcknowledgedAt) &&
          !(team && !user) &&
          !walkthroughDue(status, settings) &&
          setupDone(status, settings, team && !user)
        }
      />
      {/* The toast arrives and leaves (it used to vanish); a new one waits for the old to go. */}
      <AnimatePresence mode="wait">
        {toast && (
          <Toast
            key={toast.n}
            message={toast.message}
            action={toast.action}
            more={toast.options?.more}
            ms={toast.options?.ms}
            onClose={closeToast}
          />
        )}
      </AnimatePresence>
      <div className="sr-only" aria-live="polite">
        {announce}
      </div>
    </AppContext.Provider>
    </MotionConfig>
    </LazyMotion>
  );
}
