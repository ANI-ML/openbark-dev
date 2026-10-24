import { Button, RecordButton, Sheet, StatusDot, Waveform, formatElapsed } from "@openbark/ui";
import { Check, ChevronRight, ClipboardList, Mic, PawPrint, Phone, Square, Upload, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { useApp } from "../context";
import * as engine from "../engine";
import { templateName, useT } from "../i18n";
import { ConsentStep } from "../components/ConsentStep";
import { micNotice, switchNote, type MicHealth } from "../mic/health";
import { PatientFields } from "../components/PatientFields";
import { PatientPicker } from "../components/PatientPicker";
import { ProcessingBar } from "../components/ProcessingBar";
import { SetupPanel } from "../components/SetupPanel";
import { TranscriptList } from "../components/TranscriptList";
import { PHONE, useMediaQuery } from "../useMediaQuery";
import { animalFamily, inWhen, isIos, needsYou } from "../visits";

type State = "idle" | "consent" | "starting" | "recording" | "stopping" | "importing";

/** The consent line under the Record button, from the consent settings. */
export function consentLine(t: (s: string) => string, eachTime: boolean, rule: "all_parties" | "one_party"): string {
  if (!eachTime) return t("Consent: you get it before recording.");
  return rule === "one_party" ? t("Consent: asked each time, one party.") : t("Consent: asked each time, everyone present.");
}

/**
 * Record (ADR-0027): the 96 px Record button in the main area (on phones in
 * thumb reach above the options), one optional patient row, one template
 * choice and a short list of options. Under the button: the consent line and
 * the microphone, each with Change (the full microphone test lives in
 * Settings). DESIGN.md §7.1: "Record" when idle, "Recording · 02:14" while
 * recording, "Writing note…" while a stopped or imported recording is
 * processed. Phones get a full recording screen with the time, a small
 * waveform, the latest lines of the transcript and Stop and save.
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
  const ios = isIos(app.status);
  /** Phones: the patient form and the template picker open in sheets. */
  const [patientSheet, setPatientSheet] = useState(false);
  const [templateSheet, setTemplateSheet] = useState(false);
  /** Desktop: species, more animals and the rest of the patient form. */
  const [moreDetails, setMoreDetails] = useState(false);
  /** Visits waiting for someone today (desktop side column). */
  const [waiting, setWaiting] = useState<engine.SessionSummary[]>([]);
  const setupRef = useRef<HTMLDivElement>(null);

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
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Each person's own versions and templates (ADR-0024): reload when someone else uses the app.
  useEffect(() => {
    engine.listTemplates().then(setTemplates, () => {});
  }, [app.user?.id]);

  // What needs someone today, beside the recorder on a computer.
  useEffect(() => {
    if (phone) return;
    const now = new Date();
    engine.listSessions().then(
      (rows) => setWaiting(rows.filter((r) => needsYou(r) && inWhen(r.created_at, "today", now)).slice(0, 5)),
      () => {},
    );
  }, [phone]);

  const missing = app.status?.models.filter((m) => !m.installed) ?? [];

  // Load the speech models while the screen is open, so Record starts at
  // once; give the memory back when it closes.
  const ready = missing.length === 0 && Boolean(app.status);
  useEffect(() => {
    if (!ready) return;
    // A moment's wait, so toggling the options doesn't start a load per click.
    const timer = setTimeout(
      () => void engine.prewarmSpeech(app.settings.labelSpeakers, app.settings.phoneOnSpeaker),
      800,
    );
    return () => clearTimeout(timer);
  }, [ready, app.settings.labelSpeakers, app.settings.phoneOnSpeaker]);
  useEffect(() => () => void engine.releaseSpeech(), []);
  /** The template chosen on this device is one this person can draft with. */
  const templateAvailable = templates.length === 0 || templates.some((x) => x.id === app.settings.template);
  const noteTemplates = templates.filter((x) => x.document !== "client");
  const templateId = templateAvailable ? app.settings.template : "soap";
  const currentTemplate = templates.find((x) => x.id === templateId);
  const currentTemplateName = templateName(t, currentTemplate?.name ?? "SOAP note");

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

  /** Draft after stopping, unless this iPhone leaves drafting to a computer. */
  const draftAfter = app.settings.draftOnStop && !(ios && !app.settings.writeNotesHere);

  async function finish() {
    try {
      const view = await recording.current!.stop();
      recording.current = null;
      app.announce(draftAfter ? t("Recording stopped. Drafting the note.") : t("Recording stopped"));
      app.navigate({ screen: "review", id: view.id, draft: draftAfter });
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
        app.settings.phoneOnSpeaker,
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
        app.settings.phoneOnSpeaker,
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
      if (view) app.navigate({ screen: "review", id: view.id, draft: draftAfter });
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
  /** Said under the button when it isn't simply ready. */
  const statusLine =
    state === "recording" && mic?.state === "lost"
      ? t("Paused: no microphone")
      : state === "consent"
        ? t("Confirm consent to record.")
        : state === "starting"
          ? t("Starting the microphone")
          : state === "stopping"
            ? t("Finishing the transcript")
            : state === "importing"
              ? t("Transcribing the recording")
              : missing.length
                ? t("Download the models to start recording.")
                : null;

  const patientName = animalFamily({ animal: patient.name, family: patient.last_name });
  const micName = !engine.isDesktop
    ? t("Browser microphone")
    : app.settings.device
      ? app.settings.device
      : ios
        ? t("iPhone microphone")
        : t("System default microphone");
  const toSettings = () => app.navigate({ screen: "settings", section: "recording" });
  const locked = state !== "idle";

  const recordButton = (
    <RecordButton
      className="recorder-record"
      size="large"
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
  );

  /** Under the button: consent and the microphone, each with Change. */
  const stageLines = (
    <div className="stage-lines">
      {statusLine && (
        <span className="small muted" role="status">
          {statusLine}
        </span>
      )}
      {!live && (
        <span className="small muted">
          {consentLine(t, app.settings.consentEachTime, app.settings.consentRule)}{" "}
          <button type="button" className="link small" onClick={toSettings} disabled={locked}>
            {t("Change")}
          </button>
        </span>
      )}
      <span className="stage-mic small muted">
        <Mic size={16} aria-hidden />
        <span className="stage-mic-name">{micName}</span>
        <Waveform levels={app.levels} bars={5} active={live} className={`mic-bars ${live ? "wave-live" : "wave-idle"}`} />
        {!live && (
          <button type="button" className="link small" onClick={toSettings} disabled={locked}>
            {t("Change")}
          </button>
        )}
      </span>
    </div>
  );

  const speakerHint = app.settings.phoneOnSpeaker && (
    <span className="small muted opt-hint" id="phone-on-speaker-hint">
      {t(
        "Picks up the quieter voice of a client on a phone on speaker. Put the phone between you and the computer, volume up.",
      )}
    </span>
  );

  const checkRow = (
    label: string,
    checked: boolean,
    onChange: (v: boolean) => void,
    opts: { icon?: React.ReactNode; hint?: React.ReactNode; describedBy?: string; disabled?: boolean } = {},
  ) => (
    <div className="opt-item">
      <label className="opt-row">
        {opts.icon}
        <span className="opt-text">{label}</span>
        <input
          type="checkbox"
          className="opt-switch"
          checked={checked}
          onChange={(e) => onChange(e.target.checked)}
          disabled={opts.disabled}
          aria-describedby={opts.describedBy}
        />
      </label>
      {opts.hint}
    </div>
  );

  const options = (
    <div className="opt-list" role="group" aria-label={t("Recording options")}>
      {phone ? (
        <button type="button" className="opt-row" onClick={() => setTemplateSheet(true)} disabled={locked}>
          <ClipboardList size={20} aria-hidden className="opt-icon" />
          <span className="opt-text">{t("Template")}</span>
          <span className="opt-value">{currentTemplateName}</span>
          <ChevronRight size={18} aria-hidden className="opt-chevron" />
        </button>
      ) : (
        <label className="opt-row">
          <span className="opt-text">{t("Template")}</span>
          <select
            className="ob-select opt-select"
            value={templateId}
            onChange={(e) => app.setSettings({ template: e.target.value })}
            disabled={locked}
          >
            {(noteTemplates.length ? noteTemplates : [{ id: "soap", name: "SOAP note" }]).map((x) => (
              <option key={x.id} value={x.id}>
                {templateName(t, x.name)}
              </option>
            ))}
          </select>
        </label>
      )}
      {checkRow(t("Phone on speaker"), app.settings.phoneOnSpeaker, (v) => app.setSettings({ phoneOnSpeaker: v }), {
        icon: phone ? <Phone size={20} aria-hidden className="opt-icon" /> : undefined,
        hint: speakerHint,
        describedBy: app.settings.phoneOnSpeaker ? "phone-on-speaker-hint" : undefined,
        disabled: locked,
      })}
      {checkRow(t("Label speakers"), app.settings.labelSpeakers, (v) => app.setSettings({ labelSpeakers: v }), {
        disabled: locked,
      })}
      {checkRow(t("Draft the note when I stop"), app.settings.draftOnStop, (v) => app.setSettings({ draftOnStop: v }))}
    </div>
  );

  const templateNote = !templateAvailable && (
    // Another person's own template was chosen on this computer (ADR-0024 D6).
    <p className="small muted" role="status">
      {t("The visit type chosen last is someone else's own template. Drafting with {default}.", {
        default: templateName(t, templates.find((x) => x.id === "soap")?.name ?? "SOAP note"),
      })}
    </p>
  );

  const privacy = (
    <p className="small faint">
      {ios
        ? t("The iPhone stays awake while recording. Recordings are saved on this iPhone only.")
        : t("The machine stays awake while recording and drafting. Recordings are saved on this computer only.")}
    </p>
  );

  const setup = missing.length > 0 && state === "idle" && (
    <div ref={setupRef}>
      <SetupPanel
        missing={missing}
        onReady={() => {
          app.refreshStatus();
          app.toast(t("Models ready"));
          app.announce(t("Models downloaded. Ready to record."));
        }}
      />
    </div>
  );

  const consentStep = state === "consent" && (
    <ConsentStep
      rule={app.settings.consentRule}
      script={app.settings.consentScript}
      signer={app.actorName}
      purpose={consentFor}
      onConfirm={confirmConsent}
      onDecline={declineConsent}
      onCancel={() => setState("idle")}
    />
  );

  const alerts = (
    <>
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
    </>
  );

  const importButton = (
    <Button
      variant="ghost"
      onClick={importFile}
      disabled={state !== "idle" || missing.length > 0}
      className={phone ? "icon-only" : undefined}
      aria-label={phone ? t("Import a recording") : undefined}
      title={phone ? t("Import a recording") : undefined}
    >
      <Upload size={20} aria-hidden />
      {!phone && t("Import a recording")}
    </Button>
  );

  // Phones: a full recording screen while recording and finishing (ADR-0027).
  if (phone && (state === "starting" || state === "recording" || state === "stopping")) {
    const tail = segments.slice(-3);
    return (
      <div className="recording-screen" tabIndex={-1} onKeyDown={onKeyDown}>
        <header className="rec-head">
          <span className="rec-who">
            <span className="small muted">{patientName || t("No patient")}</span>
            <span className="rec-reason">{patient.reason || t("Visit")}</span>
          </span>
          <StatusDot tone="ok">{ios ? t("Transcribing here") : t("Transcribing on this computer")}</StatusDot>
        </header>
        {alerts}
        <div className="rec-stage">
          <span className="rec-indicator" data-state={state} aria-hidden>
            {state === "recording" && <span className="ob-record-pulse" />}
            <span className="rec-disc">
              <span className="rec-dot" />
            </span>
          </span>
          <p className="rec-time mono" role="timer">
            {state === "stopping" ? t("Writing note…") : `${t("Recording")} · ${formatElapsed(elapsed)}`}
          </p>
          <span className="sr-only" aria-live="polite">
            {state === "recording" ? t("Recording") : state === "stopping" ? t("Finishing the transcript") : ""}
          </span>
          <Waveform levels={app.levels} bars={12} active={state === "recording"} className="rec-wave wave-live" />
        </div>
        <section className="rec-transcript" aria-label={t("Live transcript")}>
          {tail.length === 0 ? (
            <p className="small muted">{t("Each sentence appears here once it's finished.")}</p>
          ) : (
            <TranscriptList segments={tail} follow />
          )}
        </section>
        <footer className="rec-bar">
          {state === "stopping" ? (
            <ProcessingBar progress={progress} mode="finish" />
          ) : (
            <Button
              variant="primary"
              size="prominent"
              className="rec-stop"
              onClick={() => void toggle()}
              disabled={state !== "recording"}
            >
              <Square size={20} aria-hidden />
              {t("Stop and save")}
            </Button>
          )}
        </footer>
      </div>
    );
  }

  if (phone) {
    return (
      <div className="screen recorder recorder-phone" tabIndex={-1} onKeyDown={onKeyDown}>
        <header className="screen-head">
          <h1>{t("Record")}</h1>
          <div className="head-actions">
            <StatusDot tone="ok">{ios ? t("On this iPhone") : t("On this computer")}</StatusDot>
            {importButton}
          </div>
        </header>
        {alerts}
        {missing.length > 0 && state === "idle" && (
          <p className="ob-notice setup-notice">
            <span>{t("Download the models to start recording.")}</span>
            <button
              type="button"
              className="link small"
              onClick={() => setupRef.current?.scrollIntoView({ behavior: "smooth", block: "start" })}
            >
              {t("Download")}
            </button>
          </p>
        )}
        {consentStep}
        <button type="button" className="patient-row-button" onClick={() => setPatientSheet(true)} disabled={locked}>
          <span className="patient-avatar" aria-hidden>
            <PawPrint size={20} />
          </span>
          <span className="opt-text">
            <span className="patient-row-title">{patientName || t("Patient")}</span>
            <span className="small muted">
              {patientName
                ? patient.reason || t("Add a reason for the visit")
                : t("Optional. Add now or after the visit")}
            </span>
          </span>
          <ChevronRight size={18} aria-hidden className="opt-chevron" />
        </button>
        <section className={`record-stage${processing ? " is-processing" : ""}`} aria-label={t("Recorder")}>
          {recordButton}
          {stageLines}
          {processing && <ProcessingBar progress={progress} mode={state === "importing" ? "import" : "finish"} />}
        </section>
        {options}
        {templateNote}
        {setup}
        {patientSheet && (
          <Sheet label={t("Patient")} onClose={() => setPatientSheet(false)}>
            {(close) => (
              <>
                <header className="sheet-head">
                  <h2>{t("Patient")}</h2>
                  <button type="button" className="icon-btn" aria-label={t("Close")} onClick={close}>
                    <X size={20} aria-hidden />
                  </button>
                </header>
                <div className="sheet-body">
                  <PatientPicker value={patient} onChange={setPatient} />
                  <PatientFields value={patient} onChange={editPatient} />
                  <p className="small faint">
                    {t("Saved with the visit and time-stamped. Optional: you can add or change these later.")}
                  </p>
                </div>
                <footer className="sheet-foot">
                  <Button variant="primary" onClick={close}>
                    {t("Done")}
                  </Button>
                </footer>
              </>
            )}
          </Sheet>
        )}
        {templateSheet && (
          <Sheet label={t("Template")} onClose={() => setTemplateSheet(false)}>
            {(close) => (
              <>
                <header className="sheet-head">
                  <h2>{t("Template")}</h2>
                  <button type="button" className="icon-btn" aria-label={t("Close")} onClick={close}>
                    <X size={20} aria-hidden />
                  </button>
                </header>
                <div className="sheet-body">
                  <ul className="inset-list" role="radiogroup" aria-label={t("Template")}>
                    {noteTemplates.map((x) => (
                      <li key={x.id}>
                        <button
                          type="button"
                          role="radio"
                          aria-checked={x.id === templateId}
                          className="inset-row"
                          onClick={() => {
                            app.setSettings({ template: x.id });
                            close();
                          }}
                        >
                          <span className="inset-text">
                            <span className="inset-name">{templateName(t, x.name)}</span>
                          </span>
                          {x.id === templateId && <Check size={18} aria-hidden />}
                        </button>
                      </li>
                    ))}
                  </ul>
                  <button
                    type="button"
                    className="link small"
                    onClick={() => {
                      close();
                      app.navigate({ screen: "templates" });
                    }}
                  >
                    {t("Edit templates")}
                  </button>
                </div>
              </>
            )}
          </Sheet>
        )}
      </div>
    );
  }

  return (
    <div className="screen recorder" tabIndex={-1} onKeyDown={onKeyDown}>
      <header className="screen-head">
        <h1>{t("Record")}</h1>
        {importButton}
      </header>
      {alerts}
      <div className="record-layout">
        <section className="record-main" aria-label={t("Recorder")}>
          <div className="patient-strip" role="group" aria-label={t("Patient")}>
            <div className="patient-strip-find">
              {patient.name && !patient.patient_id && !moreDetails ? (
                <div className="ob-field">
                  <span className="label">{t("Patient")}</span>
                  <span className="patient-strip-new">
                    <span className="picked-name">{patientName}</span>
                    <button type="button" className="link small" onClick={() => setMoreDetails(true)} disabled={locked}>
                      {t("Edit")}
                    </button>
                  </span>
                </div>
              ) : (
                <PatientPicker
                  value={patient}
                  onChange={setPatient}
                  disabled={locked}
                  onNew={(name) => {
                    setPatient((p) => ({ ...p, name, patient_id: null }));
                    setMoreDetails(true);
                  }}
                />
              )}
            </div>
            {!moreDetails && (
              <label className="ob-field patient-strip-reason" htmlFor="strip-reason">
                <span className="label">{t("Reason for visit")}</span>
                <input
                  id="strip-reason"
                  className="ob-input"
                  value={patient.reason}
                  placeholder={t("Optional")}
                  disabled={locked}
                  onChange={(e) => setPatient({ ...patient, reason: e.target.value })}
                />
              </label>
            )}
            <p className="small muted patient-strip-help">
              {t("Optional. You can add species, more animals or change these after the visit.")}{" "}
              <button type="button" className="link small" onClick={() => setMoreDetails((o) => !o)} aria-expanded={moreDetails}>
                {moreDetails ? t("Fewer details") : t("More details")}
              </button>
            </p>
            {moreDetails && (
              <div className="patient-strip-more">
                <PatientFields value={patient} onChange={editPatient} disabled={locked} idPrefix="strip-patient" />
              </div>
            )}
          </div>
          {consentStep}
          <div className={`record-stage${processing ? " is-processing" : ""}`}>
            {recordButton}
            {stageLines}
            {live && (
              <Waveform levels={app.levels} bars={48} active={live} className="stage-wave wave-live" />
            )}
            {processing && <ProcessingBar progress={progress} mode={state === "importing" ? "import" : "finish"} />}
          </div>
          {(live || segments.length > 0) && (
            <div className="recorder-transcript" aria-label={t("Live transcript")}>
              {segments.length === 0 ? (
                <p className="small muted">{t("Each sentence appears here once it's finished.")}</p>
              ) : (
                <TranscriptList segments={segments} follow />
              )}
            </div>
          )}
        </section>
        <aside className="record-side" aria-label={t("Before you record")}>
          {setup}
          {options}
          {templateNote}
          {waiting.length > 0 && (
            <section className="needs-today" aria-label={t("Needs you today")}>
              <h2 className="inset-title">{t("Needs you today")}</h2>
              <ul className="needs-list">
                {waiting.map((r) => (
                  <li key={r.id}>
                    <button
                      type="button"
                      className="needs-row"
                      onClick={() => app.navigate({ screen: "review", id: r.id })}
                    >
                      <span className="inset-name">{animalFamily(r) || t("No patient")}</span>
                      <span className="small muted">
                        {r.runs > 0
                          ? t("Note drafted, ready to check and sign")
                          : r.audio_on
                            ? t("Recorded on {device}, transcribed", { device: r.audio_on })
                            : t("Transcribed, ready to draft")}
                      </span>
                    </button>
                  </li>
                ))}
              </ul>
            </section>
          )}
          {privacy}
        </aside>
      </div>
    </div>
  );
}
