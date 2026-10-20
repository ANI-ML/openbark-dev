import { Button, MarkMono, RecordButton, Waveform } from "@openbark/ui";
import { VisitTypePicker } from "../components/VisitTypePicker";
import { ChevronDown, Upload } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { useApp } from "../context";
import * as engine from "../engine";
import { templateName, useT } from "../i18n";
import { ConsentStep } from "../components/ConsentStep";
import { MicCheck } from "../components/MicCheck";
import { micNotice, switchNote, type MicHealth } from "../mic/health";
import { PatientFields } from "../components/PatientFields";
import { PatientPicker } from "../components/PatientPicker";
import { ProcessingBar } from "../components/ProcessingBar";
import { SetupPanel } from "../components/SetupPanel";
import { TranscriptList } from "../components/TranscriptList";
import { PHONE, useMediaQuery } from "../useMediaQuery";

type State = "idle" | "consent" | "starting" | "recording" | "stopping" | "importing";

/**
 * Recorder (home): Record button, waveform, live transcript.
 * docs/brand/DESIGN.md §7.1: the button reads "Record" when idle,
 * "Recording · 02:14" while recording and "Writing note…" while a stopped
 * or imported recording is processed. The live transcript is in the
 * transcript type (§7.5); the empty transcript follows §7.7 with the mono
 * mark (the Record button is its one action).
 */
export function Recorder({ patientId }: { patientId?: string } = {}) {
  const app = useApp();
  const t = useT();
  const [state, setState] = useState<State>("idle");
  const [elapsed, setElapsed] = useState(0);
  const [segments, setSegments] = useState<engine.Segment[]>([]);
  /** How far an import, or a stopped recording, has got. */
  const [progress, setProgress] = useState<engine.ProcessingProgress | null>(null);
  const [error, setError] = useState<string | null>(null);
  /** The recording's microphone: silent, stalled, lost, switched or low quality. */
  const [mic, setMic] = useState<MicHealth | null>(null);
  const [patient, setPatient] = useState<engine.Patient>(engine.emptyPatient);
  const phone = useMediaQuery(PHONE);
  /** Phones: the patient fold starts open only when the visit was started from a patient's page. */
  const [patientOpen, setPatientOpen] = useState(Boolean(patientId));
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
  /** Stop was pressed while the recording was still being handed back: stop as soon as it is. */
  const stopWhenReady = useRef(false);
  const live = state === "recording" || state === "stopping" || state === "importing";
  /** Stopped or imported: the recording is being turned into a transcript and note. */
  const processing = state === "stopping" || state === "importing";

  /** A new microphone health: toast a switch, and keep the latest for the notice. */
  const micRef = useRef<MicHealth | null>(null);
  const onMic = (next: MicHealth | null) => {
    const note = next ? switchNote(t, micRef.current, next) : null;
    micRef.current = next;
    setMic(next);
    if (note) {
      app.toast(note);
      app.announce(note);
    }
  };

  useEffect(() => {
    const offs = [
      engine.on("segment", (s) => setSegments((prev) => (prev.some((p) => p.id === s.id) ? prev : [...prev, s]))),
      engine.on("level", (l) => setElapsed(l.seconds)),
      // Desktop: the engine watches the microphone. Web mode: the browser does (onHealth below).
      engine.on("mic", (h) => {
        if (engine.isDesktop) onMic(h);
      }),
      // Transcription steps only: drafting (Review) reports on the same channel.
      engine.on("progress", (p) => {
        if (["upload", "convert", "read", "transcribe", "check", "save"].includes(p.stage)) setProgress(p);
      }),
    ];
    return () => offs.forEach((p) => void p.then((f) => f()));
  }, []);

  // Each person's own versions and templates (ADR-0024): reload when someone else uses the app.
  useEffect(() => {
    engine.listTemplates().then(setTemplates, () => {});
  }, [app.user?.id]);

  const missing = app.status?.models.filter((m) => !m.installed) ?? [];

  // Load the speech models while the screen is open, so Record starts at
  // once; give the memory back when it closes.
  const ready = missing.length === 0 && Boolean(app.status);
  useEffect(() => {
    if (ready) void engine.prewarmSpeech(app.settings.labelSpeakers);
  }, [ready, app.settings.labelSpeakers]);
  useEffect(() => () => void engine.releaseSpeech(), []);
  /** The visit type chosen on this device is one this person can draft with. */
  const templateAvailable = templates.length === 0 || templates.some((x) => x.id === app.settings.template);

  async function toggle() {
    setError(null);
    if (state === "recording") {
      setProgress(null);
      setState("stopping");
      // Audio can start flowing a moment before the recording is handed back;
      // begin() stops it as soon as it arrives.
      if (!recording.current) {
        stopWhenReady.current = true;
        return;
      }
      await finish();
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

  async function finish() {
    try {
      const view = await recording.current!.stop();
      recording.current = null;
      app.announce(t("Recording stopped. Drafting the note."));
      app.navigate({ screen: "review", id: view.id, draft: app.settings.draftOnStop });
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e));
      setState("idle");
    }
  }

  async function begin(consent: engine.RecordingConsent | null) {
    stopWhenReady.current = false;
    setState("starting");
    setSegments([]);
    setElapsed(0);
    onMic(null);
    try {
      recording.current = await engine.startRecording(
        app.settings.labelSpeakers,
        engine.isDesktop ? app.settings.device : app.settings.browserMic,
        patient,
        consent,
        {
          onLevel: (level, seconds) => {
            app.levels.push(level);
            setElapsed(seconds);
            // Audio is flowing: say so now, not when the start call returns.
            setState((s) => (s === "starting" ? "recording" : s));
          },
          onHealth: onMic,
          onEnded: (message) => {
            recording.current = null;
            onMic(null);
            setError(message);
            setState("idle");
          },
        },
      );
      const opened = recording.current.mic;
      if (opened?.fellBack) {
        app.toast(t("The chosen microphone isn't connected. Recording from {device}.", { device: opened.label }));
      }
      if (stopWhenReady.current) {
        stopWhenReady.current = false;
        await finish();
        return;
      }
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

  const notice = state === "recording" ? micNotice(t, mic, !engine.isDesktop) : null;
  const statusLine =
    state === "recording" && mic?.state === "lost"
      ? t("Paused: no microphone")
      : state === "recording"
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

  const patientForm = (
    <>
      <PatientPicker value={patient} onChange={setPatient} disabled={state !== "idle"} />
      <PatientFields value={patient} onChange={editPatient} disabled={state !== "idle"} />
      <p className="small faint">
        {t("Saved with the visit and time-stamped. Optional: you can add or change these later.")}
      </p>
    </>
  );
  const patientLabel = [patient.name, patient.last_name].filter((x) => x?.trim()).join(" ");

  return (
    <div className="screen recorder" tabIndex={-1} onKeyDown={onKeyDown}>
      <header className="screen-head">
        <h1>{t("Record")}</h1>
        <Button variant="ghost" onClick={importFile} disabled={state !== "idle" || missing.length > 0}>
          <Upload size={20} aria-hidden />
          {t("Import a recording")}
        </Button>
      </header>

      {error && (
        <p className="ob-notice" data-tone="danger" role="alert">
          {t(error)}
        </p>
      )}
      {notice && (
        <p className="ob-notice mic-notice" data-tone={notice.tone === "danger" ? "danger" : undefined} role="alert">
          {notice.text}
        </p>
      )}

      <div className="recorder-grid">
        {phone ? (
          // Phones (§9): the patient folds away so the visit type, transcript and Record come first.
          <details
            className="recorder-patient recorder-patient-fold"
            aria-label={t("Patient")}
            open={patientOpen}
            onToggle={(e) => setPatientOpen(e.currentTarget.open)}
          >
            <summary>
              <span className="label">{t("Patient")}</span>
              {patientLabel && <span className="muted recorder-patient-name">{patientLabel}</span>}
              <ChevronDown size={20} className="fold-chevron" aria-hidden />
            </summary>
            <div className="recorder-patient-body">{patientForm}</div>
          </details>
        ) : (
          <aside className="recorder-patient" aria-label={t("Patient")}>
            <h2>{t("Patient")}</h2>
            {patientForm}
          </aside>
        )}
        <section
          className={`recorder-center${processing ? " is-processing" : ""}`}
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
            value={templateAvailable ? app.settings.template : "soap"}
            onChange={(id) => app.setSettings({ template: id })}
            disabled={state !== "idle"}
          />
          {!templateAvailable && (
            // Another person's own template was chosen on this computer (ADR-0024 D6).
            <p className="small muted" role="status">
              {t("The visit type chosen last is someone else's own template. Drafting with {default}.", {
                default: templateName(t, templates.find((x) => x.id === "soap")?.name ?? "SOAP note"),
              })}
            </p>
          )}
          <div className="recorder-stage">
            <RecordButton
              className="recorder-record"
              recording={state === "recording"}
              busy={state === "starting"}
              processing={processing}
              disabled={missing.length > 0}
              onToggle={toggle}
              elapsed={elapsed}
              labels={{
                start: t("Start recording"),
                stop: t("Stop recording"),
                idle: t("Record"),
                recording: t("Recording"),
                processing: t("Writing note…"),
              }}
            />
            <div className="recorder-meta">
              <span className="small muted">{statusLine}</span>
            </div>
            <Waveform levels={app.levels} bars={48} active={live} className={`stage-wave ${live ? "wave-live" : "wave-idle"}`} />
            {processing && (
              <ProcessingBar progress={progress} mode={state === "importing" ? "import" : "finish"} />
            )}
          </div>
          <div className="recorder-transcript">
            {segments.length === 0 ? (
              <div className="empty-state">
                <MarkMono size={48} className="faint" />
                <h3>{live ? t("Listening") : t("No transcript yet")}</h3>
                <p className="muted">
                  {live
                    ? t("Each sentence appears here once it's finished.")
                    : t("Press record and each sentence appears here once it's finished, with its time.")}
                </p>
              </div>
            ) : (
              <TranscriptList segments={segments} follow />
            )}
          </div>
        </section>

        <aside className="recorder-rail" aria-label={t("Recording options")}>
          <MicCheck disabled={state !== "idle"} id="recorder-mic" />
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
