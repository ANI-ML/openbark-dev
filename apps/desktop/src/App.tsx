import { createLevelSource } from "@openbark/ui";
import { Cube, Gear, Microphone, Notebook, PawPrint, Question } from "@phosphor-icons/react";
import { useCallback, useEffect, useMemo, useState } from "react";
import * as engine from "./engine";
import { ConsentNotice } from "./components/ConsentNotice";
import { ROLE_LABEL, UserPicker } from "./components/UserPicker";
import { AppContext, loadSettings, permissions, saveSettings, type Settings } from "./context";
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

const NAV = [
  { screen: "record", label: "Record", icon: Microphone },
  { screen: "patients", label: "Patients", icon: PawPrint },
  { screen: "templates", label: "Templates", icon: Notebook },
  { screen: "models", label: "Models", icon: Cube },
  { screen: "settings", label: "Settings", icon: Gear },
  { screen: "help", label: "Help", icon: Question },
] as const;

export function App() {
  const [route, setRoute] = useState<Route>({ screen: "record" });
  const [status, setStatus] = useState<engine.Status | null>(null);
  const [settings, setSettingsState] = useState<Settings>(loadSettings);
  const [toast, setToast] = useState<string | null>(null);
  const [announce, setAnnounce] = useState("");
  const levels = useMemo(() => createLevelSource(), []);

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

  useEffect(() => {
    if (!toast) return;
    const t = setTimeout(() => setToast(null), 4000);
    return () => clearTimeout(t);
  }, [toast]);

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
    toast: setToast,
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
          <div className="brand">
            <svg className="brand-mark" viewBox="0 0 1024 1024" aria-hidden="true">
              <rect className="brand-mark-tile" width="1024" height="1024" rx="224" />
              <g className="brand-mark-paw">
                <ellipse cx="512" cy="640" rx="190" ry="160" />
                <ellipse cx="300" cy="430" rx="78" ry="100" transform="rotate(-18 300 430)" />
                <ellipse cx="430" cy="300" rx="80" ry="108" transform="rotate(-6 430 300)" />
                <ellipse cx="594" cy="300" rx="80" ry="108" transform="rotate(6 594 300)" />
                <ellipse cx="724" cy="430" rx="78" ry="100" transform="rotate(18 724 430)" />
              </g>
            </svg>
            <span className="brand-name">OpenBark</span>
          </div>
          <ul>
            {NAV.map(({ screen, label: english, icon: Icon }) => {
              const label = t(english);
              return (
                <li key={screen}>
                  <button
                    type="button"
                    className="nav-item"
                    aria-label={label}
                    title={label}
                    aria-current={active === screen ? "page" : undefined}
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
      {toast && (
        <div className="ob-toast" role="status">
          {toast}
        </div>
      )}
      <div className="sr-only" aria-live="polite">
        {announce}
      </div>
    </AppContext.Provider>
  );
}
