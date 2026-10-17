import { ThemeToggle } from "@openbark/ui";
import { useEffect, useState } from "react";
import { useApp } from "../context";
import * as engine from "../engine";

/** DESIGN.md §6 Settings: hairline-separated groups. */
export function SettingsScreen() {
  const app = useApp();
  const [devices, setDevices] = useState<string[]>([]);

  useEffect(() => {
    if (engine.isDesktop) engine.inputDevices().then(setDevices, () => {});
  }, []);

  return (
    <div className="screen settings">
      <header className="screen-head">
        <h1>Settings</h1>
      </header>

      <section className="group">
        <h2>You</h2>
        <label className="ob-field narrow">
          <span className="label">Name for signing notes</span>
          <input
            className="ob-input"
            value={app.settings.signer}
            onChange={(e) => app.setSettings({ signer: e.target.value })}
            placeholder="Dr. A. Patel"
            autoComplete="name"
          />
          <span className="ob-help">Saved with each note you sign.</span>
        </label>
      </section>

      <section className="group">
        <h2>Appearance</h2>
        <ThemeToggle />
      </section>

      <section className="group">
        <h2>Recording</h2>
        {engine.isDesktop ? (
          <label className="ob-field narrow">
            <span className="label">Microphone</span>
            <select
              className="ob-select"
              value={app.settings.device ?? ""}
              onChange={(e) => app.setSettings({ device: e.target.value || null })}
            >
              <option value="">System default</option>
              {devices.map((d) => (
                <option key={d} value={d}>
                  {d}
                </option>
              ))}
            </select>
          </label>
        ) : (
          <p className="small muted">In the browser, the microphone is the one you allow when you first record.</p>
        )}
        <label className="check">
          <input
            type="checkbox"
            checked={app.settings.labelSpeakers}
            onChange={(e) => app.setSettings({ labelSpeakers: e.target.checked })}
          />
          <span>Label speakers</span>
        </label>
        <label className="check">
          <input
            type="checkbox"
            checked={app.settings.draftOnStop}
            onChange={(e) => app.setSettings({ draftOnStop: e.target.checked })}
          />
          <span>Draft the note when I stop recording</span>
        </label>
      </section>

      <section className="group">
        <h2>Storage</h2>
        <dl className="slots">
          <div>
            <dt className="label muted">Encounters</dt>
            <dd className="mono">{app.status?.sessions_dir}</dd>
          </div>
          <div>
            <dt className="label muted">Models</dt>
            <dd className="mono">{app.status?.models_dir}</dd>
          </div>
        </dl>
        <p className="small faint">Recordings and notes stay on this computer. They aren't encrypted yet; that arrives in the next release.</p>
      </section>

      <section className="group">
        <h2>About</h2>
        <p className="small muted">
          OpenBark <span className="mono">{app.status?.version}</span>, pre-release. Notes are drafts until a veterinarian signs them.
        </p>
      </section>
    </div>
  );
}
