import { Button, RecordButton, Waveform } from "@openbark/ui";
import { UploadSimple } from "@phosphor-icons/react";
import { useEffect, useRef, useState } from "react";
import { fmtTime, useApp } from "../context";
import * as engine from "../engine";
import { ConsentStep } from "../components/ConsentStep";
import { PatientFields } from "../components/PatientFields";
import { SetupPanel } from "../components/SetupPanel";
import { TranscriptList } from "../components/TranscriptList";

type State = "idle" | "consent" | "starting" | "recording" | "stopping" | "importing";

/** DESIGN.md §6 Recorder (home): Record button, waveform, elapsed, live transcript. */
export function Recorder() {
  const app = useApp();
  const [state, setState] = useState<State>("idle");
  const [elapsed, setElapsed] = useState(0);
  const [segments, setSegments] = useState<engine.Segment[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [patient, setPatient] = useState<engine.Patient>(engine.emptyPatient);
  const [templates, setTemplates] = useState<engine.TemplateSummary[]>([]);
  /** What the consent step is for, while it's open. */
  const [consentFor, setConsentFor] = useState<"record" | "import">("record");
  const recording = useRef<engine.Recording | null>(null);
  const live = state === "recording" || state === "stopping" || state === "importing";

  useEffect(() => {
    const offs = [
      engine.on("segment", (s) => setSegments((prev) => (prev.some((p) => p.id === s.id) ? prev : [...prev, s]))),
      engine.on("level", (l) => setElapsed(l.seconds)),
    ];
    engine.listTemplates().then(setTemplates, () => {});
    return () => offs.forEach((p) => void p.then((f) => f()));
  }, []);

  const missing = app.status?.models.filter((m) => !m.installed) ?? [];

  async function toggle() {
    setError(null);
    if (state === "recording") {
      setState("stopping");
      try {
        const view = await recording.current!.stop();
        recording.current = null;
        app.announce("Recording stopped. Drafting the note.");
        app.navigate({ screen: "review", id: view.id, draft: app.settings.draftOnStop });
      } catch (e) {
        setError(String(e instanceof Error ? e.message : e));
        setState("idle");
      }
      return;
    }
    // Nothing records until consent is confirmed (M1 item 7).
    setConsentFor("record");
    setState("consent");
  }

  async function begin(consent: engine.RecordingConsent) {
    setState("starting");
    setSegments([]);
    setElapsed(0);
    try {
      recording.current = await engine.startRecording(app.settings.labelSpeakers, app.settings.device, patient, consent, (level, seconds) => {
        app.levels.push(level);
        setElapsed(seconds);
      });
      setState("recording");
      app.announce("Recording started");
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e));
      setState("idle");
    }
  }

  function importFile() {
    setError(null);
    setConsentFor("import");
    setState("consent");
  }

  async function importWith(consent: engine.RecordingConsent) {
    setState("idle");
    try {
      const view = await engine.importRecording(app.settings.labelSpeakers, patient, consent, () => {
        setSegments([]);
        setState("importing");
      });
      if (view) app.navigate({ screen: "review", id: view.id, draft: app.settings.draftOnStop });
      else setState("idle");
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e));
      setState("idle");
    }
  }

  // Space toggles recording when the recorder has focus (DESIGN.md §6).
  function onKeyDown(e: React.KeyboardEvent) {
    if (e.code === "Space" && e.target === e.currentTarget && (state === "idle" || state === "recording")) {
      e.preventDefault();
      void toggle();
    }
  }

  function confirmConsent(consent: engine.RecordingConsent, signer: string) {
    if (!app.settings.signer.trim()) app.setSettings({ signer });
    if (consentFor === "record") void begin(consent);
    else void importWith(consent);
  }

  function declineConsent() {
    setState("idle");
    app.toast("Not recording");
    app.announce("The client declined. Not recording.");
  }

  const statusLine =
    state === "recording"
      ? "Recording. Audio is saved as you go."
      : state === "consent"
        ? "Confirm consent to record."
        : state === "starting"
        ? "Starting the microphone"
        : state === "stopping"
          ? "Finishing the transcript"
          : state === "importing"
            ? "Transcribing the recording"
            : missing.length
              ? "Download the models above to start recording."
              : "Press the button or Space to start recording.";

  return (
    <div className="screen recorder" tabIndex={-1} onKeyDown={onKeyDown}>
      <header className="screen-head">
        <h1>Record</h1>
        <Button variant="ghost" onClick={importFile} disabled={state !== "idle" || missing.length > 0}>
          <UploadSimple size={20} aria-hidden />
          Import a recording
        </Button>
      </header>

      {error && (
        <p className="ob-notice" data-tone="danger" role="alert">
          {error}
        </p>
      )}

      <div className="recorder-grid">
        <aside className="recorder-patient" aria-label="Patient">
          <h2>Patient</h2>
          <PatientFields value={patient} onChange={setPatient} disabled={state !== "idle"} />
          <p className="small faint">Saved with the encounter and time-stamped. Optional: you can add or change these later.</p>
        </aside>
        <section className="recorder-center" aria-label="Recorder">
          {missing.length > 0 && state === "idle" && (
            <SetupPanel
              missing={missing}
              onReady={() => {
                app.refreshStatus();
                app.toast("Models ready");
                app.announce("Models downloaded. Ready to record.");
              }}
            />
          )}
          {state === "consent" && (
            <ConsentStep
              rule={app.settings.consentRule}
              script={app.settings.consentScript}
              signer={app.settings.signer}
              purpose={consentFor}
              onConfirm={confirmConsent}
              onDecline={declineConsent}
              onCancel={() => setState("idle")}
            />
          )}
          <div className="recorder-stage">
            <RecordButton
              recording={state === "recording"}
              busy={state === "starting" || state === "stopping"}
              disabled={missing.length > 0 || state === "importing"}
              onToggle={toggle}
              levels={app.levels}
            />
            <div className="recorder-meta">
              <span className="mono elapsed" aria-label="Elapsed time">
                {fmtTime(elapsed)}
              </span>
              <span className="small muted">{statusLine}</span>
            </div>
          </div>
          <Waveform levels={app.levels} bars={64} className={live ? "wave-live" : "wave-idle"} />
          <div className="recorder-transcript">
            {segments.length === 0 ? (
              <p className="empty small muted">
                {live ? "Listening. Each sentence appears here once it's finished." : "The live transcript appears here."}
              </p>
            ) : (
              <TranscriptList segments={segments} follow />
            )}
          </div>
        </section>

        <aside className="recorder-rail" aria-label="Recording options">
          <div className="ob-field">
            <span className="label">Template</span>
            <select
              className="ob-select"
              value={app.settings.template}
              aria-label="Template"
              onChange={(e) => app.setSettings({ template: e.target.value })}
            >
              {templates.length === 0 && <option value={app.settings.template}>SOAP note</option>}
              {templates.filter((t) => t.document !== "client").map((t) => (
                <option key={t.id} value={t.id}>
                  {t.name}
                </option>
              ))}
            </select>
            <button type="button" className="link small" onClick={() => app.navigate({ screen: "templates" })}>
              Edit templates
            </button>
          </div>
          <label className="check">
            <input
              type="checkbox"
              checked={app.settings.labelSpeakers}
              onChange={(e) => app.setSettings({ labelSpeakers: e.target.checked })}
              disabled={state !== "idle"}
            />
            <span>Label speakers</span>
          </label>
          <label className="check">
            <input
              type="checkbox"
              checked={app.settings.draftOnStop}
              onChange={(e) => app.setSettings({ draftOnStop: e.target.checked })}
            />
            <span>Draft the note when I stop</span>
          </label>
          <p className="small faint">
            The machine stays awake while recording and drafting. Recordings are saved on this computer only.
          </p>
        </aside>
      </div>
    </div>
  );
}
