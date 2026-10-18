import { Button, RecordButton, Waveform } from "@openbark/ui";
import { VisitTypePicker } from "../components/VisitTypePicker";
import { UploadSimple } from "@phosphor-icons/react";
import { useEffect, useRef, useState } from "react";
import { fmtTime, useApp } from "../context";
import * as engine from "../engine";
import { templateName, useT } from "../i18n";
import { ConsentStep } from "../components/ConsentStep";
import { PatientFields } from "../components/PatientFields";
import { PatientPicker } from "../components/PatientPicker";
import { ProcessingBar } from "../components/ProcessingBar";
import { SetupPanel } from "../components/SetupPanel";
import { TranscriptList } from "../components/TranscriptList";

type State = "idle" | "consent" | "starting" | "recording" | "stopping" | "importing";

/** DESIGN.md §6 Recorder (home): Record button, waveform, elapsed, live transcript. */
export function Recorder({ patientId }: { patientId?: string } = {}) {
  const app = useApp();
  const t = useT();
  const [state, setState] = useState<State>("idle");
  const [elapsed, setElapsed] = useState(0);
  const [segments, setSegments] = useState<engine.Segment[]>([]);
  /** How far an import, or a stopped recording, has got. */
  const [progress, setProgress] = useState<engine.ProcessingProgress | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [patient, setPatient] = useState<engine.Patient>(engine.emptyPatient);
  // Opened from a patient's page: start with that patient (ADR-0011).
  useEffect(() => {
    if (!patientId) return;
    engine.patientView(patientId).then(
      (v) =>
        setPatient((p) => ({
          ...p,
          name: v.patient.name,
          last_name: v.patient.last_name,
          species: v.patient.species,
          patient_id: v.patient.id,
        })),
      () => {},
    );
  }, [patientId]);
  /** Typing a different name or family name makes it a new patient. */
  const editPatient = (next: engine.Patient) =>
    setPatient((p) =>
      p.patient_id && (next.name !== p.name || next.last_name !== p.last_name) ? { ...next, patient_id: null } : next,
    );
  const [templates, setTemplates] = useState<engine.TemplateSummary[]>([]);
  /** What the consent step is for, while it's open. */
  const [consentFor, setConsentFor] = useState<"record" | "import">("record");
  const recording = useRef<engine.Recording | null>(null);
  const live = state === "recording" || state === "stopping" || state === "importing";

  useEffect(() => {
    const offs = [
      engine.on("segment", (s) => setSegments((prev) => (prev.some((p) => p.id === s.id) ? prev : [...prev, s]))),
      engine.on("level", (l) => setElapsed(l.seconds)),
      engine.on("progress", setProgress),
    ];
    engine.listTemplates().then(setTemplates, () => {});
    return () => offs.forEach((p) => void p.then((f) => f()));
  }, []);

  const missing = app.status?.models.filter((m) => !m.installed) ?? [];

  async function toggle() {
    setError(null);
    if (state === "recording") {
      setProgress(null);
      setState("stopping");
      try {
        const view = await recording.current!.stop();
        recording.current = null;
        app.announce(t("Recording stopped. Drafting the note."));
        app.navigate({ screen: "review", id: view.id, draft: app.settings.draftOnStop });
      } catch (e) {
        setError(String(e instanceof Error ? e.message : e));
        setState("idle");
      }
      return;
    }
    // Consent is the clinic's responsibility (accepted once); the
    // per-recording step is optional (Settings).
    if (app.settings.consentEachTime) {
      setConsentFor("record");
      setState("consent");
    } else {
      void begin(null);
    }
  }

  async function begin(consent: engine.RecordingConsent | null) {
    setState("starting");
    setSegments([]);
    setElapsed(0);
    try {
      recording.current = await engine.startRecording(
        app.settings.labelSpeakers,
        app.settings.device,
        patient,
        consent,
        (level, seconds) => {
          app.levels.push(level);
          setElapsed(seconds);
        },
      );
      setState("recording");
      app.announce(t("Recording started"));
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e));
      setState("idle");
    }
  }

  function importFile() {
    setError(null);
    if (app.settings.consentEachTime) {
      setConsentFor("import");
      setState("consent");
    } else {
      void importWith(null);
    }
  }

  async function importWith(consent: engine.RecordingConsent | null) {
    setState("idle");
    try {
      const view = await engine.importRecording(
        app.settings.labelSpeakers,
        patient,
        consent,
        () => {
          setSegments([]);
          setProgress(null);
          setState("importing");
        },
        // The browser's last upload event can land after the engine's first
        // step (they travel separately); once the engine has started, it leads.
        (next) =>
          setProgress((prev) =>
            next.stage === "upload" && prev && ["read", "transcribe", "check", "save"].includes(prev.stage)
              ? prev
              : next,
          ),
      );
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
    if (!app.settings.signer.trim() && !app.user) app.setSettings({ signer });
    if (consentFor === "record") void begin(consent);
    else void importWith(consent);
  }

  function declineConsent() {
    setState("idle");
    app.toast(t("Not recording"));
    app.announce(t("The client declined. Not recording."));
  }

  const statusLine =
    state === "recording"
      ? t("Recording. Audio is saved as you go.")
      : state === "consent"
        ? t("Confirm consent to record.")
        : state === "starting"
          ? t("Starting the microphone")
          : state === "stopping"
            ? t("Finishing the transcript")
            : state === "importing"
              ? t("Transcribing the recording")
              : missing.length
                ? t("Download the models above to start recording.")
                : t("Press the button or Space to start recording.");

  return (
    <div className="screen recorder" tabIndex={-1} onKeyDown={onKeyDown}>
      <header className="screen-head">
        <h1>{t("Record")}</h1>
        <Button variant="ghost" onClick={importFile} disabled={state !== "idle" || missing.length > 0}>
          <UploadSimple size={20} aria-hidden />
          {t("Import a recording")}
        </Button>
      </header>

      {error && (
        <p className="ob-notice" data-tone="danger" role="alert">
          {error}
        </p>
      )}

      <div className="recorder-grid">
        <aside className="recorder-patient" aria-label={t("Patient")}>
          <h2>{t("Patient")}</h2>
          <PatientPicker value={patient} onChange={setPatient} disabled={state !== "idle"} />
          <PatientFields value={patient} onChange={editPatient} disabled={state !== "idle"} />
          <p className="small faint">
            {t("Saved with the visit and time-stamped. Optional: you can add or change these later.")}
          </p>
        </aside>
        <section
          className={`recorder-center${state === "importing" || state === "stopping" ? " is-processing" : ""}`}
          aria-label={t("Recorder")}
        >
          {missing.length > 0 && state === "idle" && (
            <SetupPanel
              missing={missing}
              onReady={() => {
                app.refreshStatus();
                app.toast(t("Models ready"));
                app.announce(t("Models downloaded. Ready to record."));
              }}
            />
          )}
          {state === "consent" && (
            <ConsentStep
              rule={app.settings.consentRule}
              script={app.settings.consentScript}
              signer={app.actorName}
              purpose={consentFor}
              onConfirm={confirmConsent}
              onDecline={declineConsent}
              onCancel={() => setState("idle")}
            />
          )}
          <VisitTypePicker
            templates={templates}
            value={app.settings.template}
            onChange={(id) => app.setSettings({ template: id })}
            disabled={state !== "idle"}
          />
          <div className="recorder-stage">
            <RecordButton
              recording={state === "recording"}
              busy={state === "starting" || state === "stopping"}
              disabled={missing.length > 0 || state === "importing"}
              onToggle={toggle}
              levels={app.levels}
              labels={{ start: t("Start recording"), stop: t("Stop recording") }}
            />
            <div className="recorder-meta">
              <span className="mono elapsed" aria-label={t("Elapsed time")}>
                {fmtTime(elapsed)}
              </span>
              <span className="small muted">{statusLine}</span>
            </div>
            <Waveform levels={app.levels} bars={48} className={`stage-wave ${live ? "wave-live" : "wave-idle"}`} />
            {(state === "importing" || state === "stopping") && <ProcessingBar progress={progress} />}
          </div>
          <div className="recorder-transcript">
            {segments.length === 0 ? (
              <p className="empty small muted">
                {live
                  ? t("Listening. Each sentence appears here once it's finished.")
                  : t("The live transcript appears here.")}
              </p>
            ) : (
              <TranscriptList segments={segments} follow />
            )}
          </div>
        </section>

        <aside className="recorder-rail" aria-label={t("Recording options")}>
          <button type="button" className="link small" onClick={() => app.navigate({ screen: "templates" })}>
            {t("Edit templates")}
          </button>
          <label className="check">
            <input
              type="checkbox"
              checked={app.settings.labelSpeakers}
              onChange={(e) => app.setSettings({ labelSpeakers: e.target.checked })}
              disabled={state !== "idle"}
            />
            <span>{t("Label speakers")}</span>
          </label>
          <label className="check">
            <input
              type="checkbox"
              checked={app.settings.draftOnStop}
              onChange={(e) => app.setSettings({ draftOnStop: e.target.checked })}
            />
            <span>{t("Draft the note when I stop")}</span>
          </label>
          <p className="small faint">
            {t("The machine stays awake while recording and drafting. Recordings are saved on this computer only.")}
          </p>
          <p className="small faint">{t("Get the client's consent before recording.")}</p>
        </aside>
      </div>
    </div>
  );
}
