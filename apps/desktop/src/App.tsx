import { Lockup, Mark, createLevelSource } from "@openbark/ui";
import { Box, ChevronRight, CircleHelp, Mic, NotebookText, PawPrint, Settings as SettingsIcon } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import * as engine from "./engine";
import { ConsentNotice } from "./components/ConsentNotice";
import { Toast } from "./components/Toast";
import { ROLE_LABEL, UserPicker } from "./components/UserPicker";
import { AppContext, loadSettings, permissions, saveSettings, type Settings, type ToastAction } from "./context";
import { translate } from "./i18n";
import { PatientDetail } from "./screens/PatientDetail";
import { Patients } from "./screens/Patients";
import { Models } from "./screens/Models";
import { Recorder } from "./screens/Recorder";
import { Review } from "./screens/Review";
import { SettingsScreen } from "./screens/Settings";
import { Templates } from "./screens/Templates";
import { Help } from "./screens/Help";

export type Route =
  | { screen: "record"; patient?: string }
  | { screen: "patients" }
  | { screen: "patient"; id: string }
  | { screen: "review"; id: string; draft?: boolean; template?: string; pet?: string }
  | { screen: "templates" }
  | { screen: "help" }
  | { screen: "models" }
  | { screen: "settings" };

/**
 * The left rail (DESIGN.md §9: 240 px) and, on phones, the 64 px tab bar.
 * The tab bar shows the four primary destinations (Record, Patients,
 * Templates, Settings); Models and Help stay in the rail and are reached
 * from Settings on a phone (`phone` false hides them from the tab bar).
 * The order is fixed: the e2e scripts address items by position.
 */
const NAV = [
  { screen: "record", label: "Record", icon: Mic, phone: true },
  { screen: "patients", label: "Patients", icon: PawPrint, phone: true },
  { screen: "templates", label: "Templates", icon: NotebookText, phone: true },
  { screen: "models", label: "Models", icon: Box, phone: false },
  { screen: "settings", label: "Settings", icon: SettingsIcon, phone: true },
  { screen: "help", label: "Help", icon: CircleHelp, phone: false },
] as const;

/** Destinations left out of the phone tab bar, listed under Settings there. */
const PHONE_MORE = NAV.filter((n) => !n.phone);

export function App() {
  const [route, setRoute] = useState<Route>({ screen: "record" });
  const [status, setStatus] = useState<engine.Status | null>(null);
  const [settings, setSettingsState] = useState<Settings>(loadSettings);
  const [toast, setToastState] = useState<{ n: number; message: string; action?: ToastAction } | null>(null);
  const showToast = useCallback(
    (message: string, action?: ToastAction) => setToastState((prev) => ({ n: (prev?.n ?? 0) + 1, message, action })),
    [],
  );
  const closeToast = useCallback(() => setToastState(null), []);
  const [announce, setAnnounce] = useState("");
  const levels = useMemo(() => createLevelSource(), []);

  // Each destination opens at its top, not at the previous screen's scroll position.
  useEffect(() => {
    window.scrollTo(0, 0);
  }, [route]);

  const refreshStatus = useCallback(() => {
    engine.status().then(setStatus, () => setStatus(null));
  }, []);

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

  // A remembered team member who has since been removed must choose again.
  useEffect(() => {
    if (!team || !user) return;
    engine.listUsers().then(
      (list) => {
        if (!list.some((u) => u.id === user.id)) setUser(null);
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
    can: permissions(team ? (user?.role ?? "reception") : null),
    route,
    navigate: setRoute,
    status,
    refreshStatus,
    settings,
    setSettings,
    toast: showToast,
    announce: setAnnounce,
    levels,
  };
  const active = route.screen === "review" || route.screen === "patient" ? "patients" : route.screen;
  const t = (text: string) => translate(settings.uiLanguage, text);

  useEffect(() => {
    document.documentElement.lang = settings.uiLanguage;
  }, [settings.uiLanguage]);

  return (
    <AppContext.Provider value={ctx}>
      <div className="frame">
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
            {NAV.map(({ screen, label: english, icon: Icon, phone }) => {
              const label = t(english);
              return (
                <li key={screen} className={phone ? undefined : "nav-secondary"}>
                  <button
                    type="button"
                    className="nav-item"
                    aria-label={label}
                    title={label}
                    aria-current={active === screen ? "page" : undefined}
                    // Phones: Models and Help open from Settings, so its tab stays marked (§9).
                    data-parent-current={screen === "settings" && (active === "models" || active === "help") ? "" : undefined}
                    onClick={() => setRoute({ screen } as Route)}
                  >
                    <Icon size={20} aria-hidden />
                    <span className="nav-label">{label}</span>
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
            <span className="nav-label">{t(engine.isDesktop ? "This device" : "Web, this computer only")}</span>
          </div>
        </nav>
        <main className="content">
          {route.screen === "record" && <Recorder key={route.patient ?? ""} patientId={route.patient} />}
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
          {route.screen === "templates" && <Templates />}
          {route.screen === "help" && <Help />}
          {route.screen === "models" && <Models />}
          {route.screen === "settings" && <SettingsScreen />}
          {route.screen === "settings" && (
            <nav className="phone-more" aria-label={t("More")}>
              <ul>
                {PHONE_MORE.map(({ screen, label: english, icon: Icon }) => (
                  <li key={screen}>
                    <button type="button" className="phone-more-row" onClick={() => setRoute({ screen } as Route)}>
                      <Icon size={20} aria-hidden />
                      <span>{t(english)}</span>
                      <ChevronRight size={18} aria-hidden className="phone-more-chevron" />
                    </button>
                  </li>
                ))}
              </ul>
            </nav>
          )}
        </main>
      </div>
      {!settings.consentAcknowledgedAt && (
        <ConsentNotice onAccept={() => setSettings({ consentAcknowledgedAt: new Date().toISOString() })} />
      )}
      {settings.consentAcknowledgedAt && team && !user && (
        <UserPicker
          onChosen={(u) => {
            setUser(u);
            refreshStatus();
          }}
        />
      )}
      {toast && <Toast key={toast.n} message={toast.message} action={toast.action} onClose={closeToast} />}
      <div className="sr-only" aria-live="polite">
        {announce}
      </div>
    </AppContext.Provider>
  );
}
