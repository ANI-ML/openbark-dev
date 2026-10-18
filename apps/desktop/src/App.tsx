import { createLevelSource } from "@openbark/ui";
import { Cube, Gear, ListBullets, Microphone, Notebook } from "@phosphor-icons/react";
import { useCallback, useEffect, useMemo, useState } from "react";
import * as engine from "./engine";
import { AppContext, loadSettings, saveSettings, type Settings } from "./context";
import { Encounters } from "./screens/Encounters";
import { Models } from "./screens/Models";
import { Recorder } from "./screens/Recorder";
import { Review } from "./screens/Review";
import { SettingsScreen } from "./screens/Settings";
import { Templates } from "./screens/Templates";

export type Route =
  | { screen: "record" }
  | { screen: "encounters" }
  | { screen: "review"; id: string; draft?: boolean; template?: string }
  | { screen: "templates" }
  | { screen: "models" }
  | { screen: "settings" };

const NAV = [
  { screen: "record", label: "Record", icon: Microphone },
  { screen: "encounters", label: "Encounters", icon: ListBullets },
  { screen: "templates", label: "Templates", icon: Notebook },
  { screen: "models", label: "Models", icon: Cube },
  { screen: "settings", label: "Settings", icon: Gear },
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

  const ctx = {
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
  const active = route.screen === "review" ? "encounters" : route.screen;

  return (
    <AppContext.Provider value={ctx}>
      <div className="frame">
        <nav className="sidebar" aria-label="Main">
          <div className="brand">
            <span className="brand-mark" aria-hidden />
            <span className="brand-name">OpenBark</span>
          </div>
          <ul>
            {NAV.map(({ screen, label, icon: Icon }) => (
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
            ))}
          </ul>
          <div className="sidebar-foot small faint">
            <span className="nav-label">{engine.isDesktop ? "This device" : "Web, this computer only"}</span>
          </div>
        </nav>
        <main className="content">
          {route.screen === "record" && <Recorder />}
          {route.screen === "encounters" && <Encounters />}
          {route.screen === "review" && (
            <Review key={`${route.id}:${route.template ?? ""}`} id={route.id} autoDraft={route.draft ?? false} draftTemplate={route.template} />
          )}
          {route.screen === "templates" && <Templates />}
          {route.screen === "models" && <Models />}
          {route.screen === "settings" && <SettingsScreen />}
        </main>
      </div>
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
