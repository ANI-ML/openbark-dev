import { Button } from "@openbark/ui";
import { ClipboardCheck, Download, Gauge, Lock, Mic, Square, UsersRound } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { fmtBytes, useApp } from "../context";
import * as engine from "../engine";
import { useT } from "../i18n";
import type { Route } from "../routes";
import {
  E2B,
  E4B,
  GPU_PACK,
  NOTE_MODELS,
  type QueueItem,
  type SetupMode,
  clockTime,
  downloadQueue,
  noteMinutes,
  savedSpeed,
  shortName,
  suggestSmaller,
  systemName,
  transferEta,
} from "../setup";
import { type BoardStep, ProgressBoard, type StepState } from "./ProgressBoard";

type T = ReturnType<typeof useT>;
type StepKey = "computer" | "model" | "speech" | "download" | "speed" | "tips";

const NVIDIA_LICENCE = "https://docs.nvidia.com/deeplearning/cudnn/latest/reference/eula.html";

/** The speed check writes for at most this long (os-app speed::WRITE_BUDGET). */
const WRITE_BUDGET_SECS = 15;

const fmtSecs = (s: number) => (s < 60 ? `${s} s` : `${Math.floor(s / 60)} min ${String(s % 60).padStart(2, "0")} s`);
const since = (from: number, to: number) => Math.max(0, Math.round((to - from) / 1000));

/** "about 3 min", "about 40 s": an estimate, never to the second. */
function aboutTime(t: T, secs: number): string {
  return secs >= 90
    ? t("about {n} min", { n: Math.round(secs / 60) })
    : t("about {n} s", { n: Math.max(5, Math.round(secs / 5) * 5) });
}

/** What each speech model does, by slot. */
function speechRole(t: T, slot: engine.Slot | undefined): string {
  switch (slot) {
    case "vad":
      return t("Finds where someone is speaking, so silence is skipped.");
    case "stt":
      return t("Turns speech into the transcript.");
    case "diarization":
      return t("Finds where one speaker stops and the next starts.");
    case "speaker_embedding":
      return t("Tells the voices apart, so each line gets a speaker label.");
    default:
      return "";
  }
}

/** Why this note model suits this computer, in one or two sentences. */
export function reasonText(t: T, rec: engine.Recommendation, hw: engine.Hardware | undefined): string {
  const gb = Math.round((hw?.ram_bytes ?? 0) / 1e9);
  switch (rec.why) {
    case "memory":
      return t(
        "{gb} GB of memory: Gemma 4 E4B needs more than this computer can spare, so Gemma 4 E2B is the one that fits. It finds slightly fewer details (90% of facts against 95% in OpenBark's tests).",
        { gb },
      );
    case "graphics_card":
      return t(
        "Notes are written on your graphics card ({gpu}), not the processor alone, so the larger model is the better choice: it finds a few more details (95% of facts against 90% in OpenBark's tests).",
        { gpu: hw?.gpu ?? t("NVIDIA graphics card") },
      );
    case "apple_silicon":
      return t(
        "Notes are written on this Mac's graphics (Apple silicon), not the processor alone, so the larger model is the better choice: it finds a few more details (95% of facts against 90% in OpenBark's tests).",
      );
    case "few_cores":
      return rec.cores
        ? t(
            "{gb} GB of memory but no graphics card, and {cores} processor cores: the smaller model writes notes about 1.6 times as fast on a processor, and finds slightly fewer details (90% of facts against 95% in OpenBark's tests).",
            { gb, cores: rec.cores },
          )
        : t(
            "{gb} GB of memory but no graphics card: the smaller model writes notes about 1.6 times as fast on a processor, and finds slightly fewer details (90% of facts against 95% in OpenBark's tests).",
            { gb },
          );
    case "many_cores":
      return t(
        "No graphics card, but {cores} processor cores and {gb} GB of memory: the larger model finds a few more details (95% of facts against 90% in OpenBark's tests). Notes take some minutes; the speed check measures how many.",
        { gb, cores: rec.cores ?? "" },
      );
  }
}

/** What one download is doing. */
interface Fetch {
  state: StepState;
  startedAt: number;
  endedAt: number;
  done: number;
}

/** Where the speed check is. */
interface Measure {
  state: "idle" | "running" | "done" | "stopped" | "failed";
  /** When each step started, and when the check ended. */
  at: { start: number; load?: number; read?: number; write?: number; end?: number };
  prompt: { done: number; total: number } | null;
  writing: { tokens: number; rate: number } | null;
  result: engine.SpeedCheck | null;
  error: string | null;
}

const idleMeasure = (): Measure => ({ state: "idle", at: { start: 0 }, prompt: null, writing: null, result: null, error: null });

/**
 * "Set up OpenBark" (ADR-0034): the desktop's first run, and "Check my
 * setup" for a computer set up before. Steps: this computer (memory,
 * processor, graphics, free space, system, and what that means), the note
 * model (a recommendation tied to this computer, both choices with what each
 * does better or worse), the built-in speech models (and the GPU speech
 * pack where it helps), the download (smallest first, every step timed, time
 * left and the projected finish), an optional speed check that measures
 * this computer, and recording tips. The check runs the computer, the note
 * model and the speed check only, and changes nothing unless the person
 * chooses. TODO(ADR-0034): a spoken-language step once that setting exists.
 */
export function SetupWalkthrough({ mode }: { mode: SetupMode }) {
  const app = useApp();
  const t = useT();
  const status = app.status;
  const hw = status?.hardware;
  const rec = status?.recommendation ?? null;
  const [rows, setRows] = useState<engine.ModelRow[] | null>(null);
  const [free, setFree] = useState<number | null>(null);
  const [gpu, setGpu] = useState<engine.GpuStatus | null>(null);
  const builtIn = (id: string) => NOTE_MODELS.some((m) => m.id === id);
  const [choice, setChoice] = useState<string>(() =>
    (app.settings.noteModelChosenAt || mode === "check") && builtIn(app.settings.llm)
      ? app.settings.llm
      : (rec?.model ?? status?.default_llm ?? E4B),
  );
  const [step, setStep] = useState<StepKey>("computer");
  const [gpuWanted, setGpuWanted] = useState(false);
  const [gpuAccepted, setGpuAccepted] = useState(false);
  /** The check: a model to download first (the other one was chosen). */
  const [checkDownload, setCheckDownload] = useState(false);
  const [plan, setPlan] = useState<QueueItem[]>([]);
  const [fetches, setFetches] = useState<Record<string, Fetch>>({});
  const [downloading, setDownloading] = useState(false);
  const [downloadError, setDownloadError] = useState<string | null>(null);
  const [downloadStart, setDownloadStart] = useState(0);
  const [measure, setMeasure] = useState<Measure>(idleMeasure);
  const measuring = useRef(false);
  const [now, setNow] = useState(() => Date.now());
  const heading = useRef<HTMLHeadingElement>(null);

  const loadRows = () => engine.models().then(setRows, () => setRows([]));
  useEffect(() => {
    void loadRows();
    engine.storage().then(
      (s) => setFree(s.models.free_bytes ?? null),
      () => {},
    );
    engine.gpuStatus().then(setGpu, () => {});
    const offs = [
      engine.on("download", (d) =>
        setFetches((f) => {
          const cur = f[d.model];
          return cur && cur.state === "active" ? { ...f, [d.model]: { ...cur, done: d.done } } : f;
        }),
      ),
      engine.on("progress", (p) => {
        if (!measuring.current) return;
        const key = p.stage === "load_model" ? "load" : p.stage === "read_transcript" ? "read" : p.stage === "write" ? "write" : null;
        if (key) setMeasure((m) => (m.at[key] ? m : { ...m, at: { ...m.at, [key]: Date.now() } }));
      }),
      engine.on("draft", (d) => {
        if (!measuring.current) return;
        if (d.phase === "prompt") setMeasure((m) => ({ ...m, prompt: { done: d.done, total: d.total } }));
        else if (d.phase === "writing")
          setMeasure((m) => ({ ...m, writing: { tokens: d.tokens, rate: d.tokens_per_second } }));
      }),
    ];
    return () => offs.forEach((o) => void o.then((f) => f()));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Once open, it stays until finished or skipped: downloading the models
  // would otherwise end the first run (and close it) halfway, and a restart
  // reopens it.
  useEffect(() => {
    if (app.settings.showSetup !== mode) app.setSettings({ showSetup: mode });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Each step's title takes focus, so a screen reader reads the new step.
  useEffect(() => {
    heading.current?.focus();
  }, [step]);

  // The clocks: once a second while something runs.
  const busy = downloading || measure.state === "running";
  useEffect(() => {
    if (!busy) return;
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, [busy]);

  const steps: StepKey[] =
    mode === "full"
      ? ["computer", "model", "speech", "download", "speed", "tips"]
      : ["computer", "model", ...(checkDownload ? (["download"] as const) : []), "speed"];
  const index = Math.max(0, steps.indexOf(step));
  const row = (id: string) => rows?.find((r) => r.id === id);
  const chosenRow = row(choice);
  const gpuOffer = Boolean(gpu?.supported && !(gpu.pack_installed && gpu.models_installed));
  const speech = (status?.models ?? []).filter((m) => row(m.id)?.slot !== "llm");

  function finish(route?: Route) {
    const at = new Date().toISOString();
    app.setSettings({
      setupDoneAt: app.settings.setupDoneAt ?? at,
      setupCardDismissedAt: app.settings.setupCardDismissedAt ?? at,
      showSetup: null,
    });
    app.refreshStatus();
    app.navigate(route ?? { screen: "record" });
  }

  function chooseModel() {
    const installed = Boolean(chosenRow?.installed);
    if (mode === "full") {
      app.setSettings({ llm: choice, noteModelChosenAt: new Date().toISOString() });
      setStep("speech");
      return;
    }
    // The check: only the person's own choice changes the model.
    if (choice !== app.settings.llm && installed) {
      app.setSettings({ llm: choice, noteModelChosenAt: new Date().toISOString() });
    }
    if (installed) {
      setStep("speed");
    } else {
      setCheckDownload(true);
      setStep("download");
    }
  }

  // The download step's list, fixed when the step opens (or the choice
  // changes): rows and status refreshed by the downloads don't redo it.
  const rowsReady = rows !== null;
  useEffect(() => {
    if (step !== "download" || downloading || !rows || !status) return;
    const items =
      mode === "full"
        ? downloadQueue(status.models, rows, choice, gpuWanted && gpuAccepted && gpu ? gpu.download_bytes : null)
        : downloadQueue([], rows, choice, null);
    setPlan(items);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [step, rowsReady, choice]);

  async function downloadAll() {
    setDownloadError(null);
    setDownloading(true);
    const started = Date.now();
    setDownloadStart(started);
    setNow(started);
    for (const item of plan) {
      if (fetches[item.id]?.state === "done") continue;
      const begun = Date.now();
      setFetches((f) => ({ ...f, [item.id]: { state: "active", startedAt: begun, endedAt: 0, done: 0 } }));
      try {
        if (item.kind === "gpu") await engine.installGpu(true);
        else await engine.downloadModel(item.id);
        setFetches((f) => ({ ...f, [item.id]: { state: "done", startedAt: begun, endedAt: Date.now(), done: item.size } }));
      } catch (e) {
        setFetches((f) => ({ ...f, [item.id]: { state: "pending", startedAt: 0, endedAt: 0, done: 0 } }));
        setDownloadError(
          t("{model} didn't finish downloading: {error} Try again to resume where it stopped.", {
            model: item.kind === "gpu" ? t("GPU speech pack") : item.name,
            error: e instanceof Error ? e.message : String(e),
          }),
        );
        setDownloading(false);
        return;
      }
    }
    setDownloading(false);
    if (mode === "check") app.setSettings({ llm: choice, noteModelChosenAt: new Date().toISOString() });
    app.refreshStatus();
    app.announce(t("Models downloaded. Ready to record."));
    void loadRows();
  }

  async function runSpeedCheck() {
    measuring.current = true;
    const start = Date.now();
    setNow(start);
    setMeasure({ ...idleMeasure(), state: "running", at: { start } });
    try {
      const result = await engine.speedCheck(choice);
      setMeasure((m) => ({ ...m, state: "done", result, at: { ...m.at, end: Date.now() } }));
      app.setSettings({ speedCheck: savedSpeed(result) });
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      setMeasure((m) => ({
        ...m,
        state: message === engine.DRAFT_STOPPED ? "stopped" : "failed",
        error: message,
        at: { ...m.at, end: Date.now() },
      }));
    } finally {
      measuring.current = false;
    }
  }

  /** The speed check found the larger model slow here: switch to the smaller one, downloading it first if need be. */
  function chooseSmaller() {
    setChoice(E2B);
    setMeasure(idleMeasure());
    if (row(E2B)?.installed) {
      app.setSettings({ llm: E2B, noteModelChosenAt: new Date().toISOString() });
    } else {
      if (mode === "check") setCheckDownload(true);
      setFetches({});
      setStep("download");
    }
  }

  const titles: Record<StepKey, string> = {
    computer: t("This computer"),
    model: t("The note model"),
    speech: t("Speech models"),
    download: t("Download"),
    speed: t("Speed check"),
    tips: t("Recording tips"),
  };

  return (
    <div className="setup-walk" role="dialog" aria-modal="true" aria-labelledby="setup-walk-title">
      <header className="setup-walk-top">
        <div className="setup-walk-titles">
          <p className="setup-walk-name">{mode === "full" ? t("Set up OpenBark") : t("Check my setup")}</p>
          <span className="small muted" aria-live="polite">
            {t("Step {n} of {total}", { n: index + 1, total: steps.length })}
          </span>
        </div>
        <Button variant="ghost" onClick={() => finish()} disabled={busy}>
          {mode === "full" ? t("Skip setup") : t("Close")}
        </Button>
      </header>

      <ol className="setup-walk-steps" aria-label={t("Steps")}>
        {steps.map((s, i) => (
          <li key={s} data-state={i < index ? "done" : i === index ? "active" : "pending"} aria-current={i === index ? "step" : undefined}>
            {titles[s]}
          </li>
        ))}
      </ol>

      <div className="setup-walk-body" data-step={step}>
        <h1 id="setup-walk-title" ref={heading} tabIndex={-1}>
          {titles[step]}
        </h1>

        {step === "computer" && <ComputerStep t={t} status={status} free={free} />}

        {step === "model" && (
          <>
            {rec && (
              <p className="setup-reason" data-testid="setup-reason">
                <strong>{t("Recommended: {model}.", { model: shortName(rec.model) })}</strong> {reasonText(t, rec, hw)}
              </p>
            )}
            <fieldset className="setup-choices">
              <legend className="sr-only">{t("Model that writes notes")}</legend>
              {NOTE_MODELS.map((m) => {
                const r = row(m.id);
                const recommended = rec?.model === m.id;
                return (
                  <label key={m.id} className="setup-choice" data-selected={choice === m.id || undefined}>
                    <input
                      type="radio"
                      name="setup-note-model"
                      value={m.id}
                      checked={choice === m.id}
                      onChange={() => setChoice(m.id)}
                    />
                    <span className="setup-choice-body">
                      <span className="setup-choice-head">
                        <span className="setup-choice-name">{m.name}</span>
                        {recommended && <span className="ob-chip">{t("Recommended")}</span>}
                        <span className="mono faint small setup-choice-size">
                          {r?.installed ? t("Downloaded") : r ? t("{size} download", { size: fmtBytes(r.size_bytes) }) : ""}
                        </span>
                      </span>
                      {m.id === E2B ? (
                        <>
                          <span className="small">{t("Better: about 1.6 times as fast on a processor, and needs less memory.")}</span>
                          <span className="small muted">
                            {t(
                              "Less good: finds slightly fewer details (90% of facts) and wrote two numbers wrong across 23 test visits, so check doses and weights closely.",
                            )}
                          </span>
                        </>
                      ) : (
                        <>
                          <span className="small">{t("Better: finds more details (95% of facts), with no wrong numbers across 23 test visits.")}</span>
                          <span className="small muted">{t("Less good: slower, and needs more memory.")}</span>
                        </>
                      )}
                      {r?.fit === "wont_fit" && (
                        <span className="small" data-tone="warn">
                          {t("May not fit this computer's memory, so drafts can be very slow or stop.")}
                        </span>
                      )}
                    </span>
                  </label>
                );
              })}
            </fieldset>
            <p className="small muted">{t("You can change it later in Models.")}</p>
          </>
        )}

        {step === "speech" && (
          <>
            <p>
              {t(
                "OpenBark turns speech into text with these built-in models. They run on this computer, and recordings are never sent anywhere.",
              )}
            </p>
            <ul className="setup-list setup-speech">
              {speech.map((m) => {
                const r = row(m.id);
                return (
                  <li key={m.id}>
                    <span className="setup-speech-text">
                      <span>{m.name}</span>
                      <span className="small muted">{speechRole(t, r?.slot)}</span>
                    </span>
                    <span className="mono faint">{r ? fmtBytes(r.size_bytes) : ""}</span>
                    <span className="small muted">{m.installed ? t("Downloaded") : t("To download")}</span>
                  </li>
                );
              })}
            </ul>
            {gpuOffer && gpu && (
              <div className="setup-gpu">
                <h2>{t("Faster transcription on the graphics card")}</h2>
                <p className="small muted">
                  {t(
                    "Your {gpu} can do the speech recognition. Measured on an RTX 4080, an 8.5-minute consult was ready in 43 seconds instead of 2.5 minutes, because the number check keeps up instead of finishing afterwards.",
                    { gpu: gpu.gpu ?? t("NVIDIA graphics card") },
                  )}
                </p>
                <label className="check">
                  <input type="checkbox" checked={gpuWanted} onChange={(e) => setGpuWanted(e.target.checked)} />
                  <span>{t("Also download the GPU speech pack ({size})", { size: fmtBytes(gpu.download_bytes) })}</span>
                </label>
                {gpuWanted && (
                  <label className="check">
                    <input type="checkbox" checked={gpuAccepted} onChange={(e) => setGpuAccepted(e.target.checked)} />
                    <span>
                      {t("I accept NVIDIA's licence for its libraries")}{" "}
                      <a href={NVIDIA_LICENCE} target="_blank" rel="noreferrer">
                        {t("(read it)")}
                      </a>
                    </span>
                  </label>
                )}
              </div>
            )}
          </>
        )}

        {step === "download" && (
          <DownloadStep
            t={t}
            plan={plan}
            fetches={fetches}
            now={now}
            start={downloadStart}
            running={downloading}
            error={downloadError}
            free={free}
          />
        )}

        {step === "speed" && (
          <SpeedStep
            t={t}
            model={shortName(choice, chosenRow?.name)}
            installed={Boolean(chosenRow?.installed)}
            measure={measure}
            now={now}
            suggest={suggestSmaller(measure.result, choice)}
            smallerInstalled={Boolean(row(E2B)?.installed)}
            smallerSize={row(E2B)?.size_bytes ?? 0}
            onUseSmaller={chooseSmaller}
          />
        )}

        {step === "tips" && (
          <ul className="setup-tips">
            <li>
              <Mic size={20} aria-hidden />
              <span>
                {t(
                  "Microphone: a USB or clip-on microphone close to the people talking works best, or the iPhone app placed near them. A laptop's own microphone far from the speakers picks up less.",
                )}
              </span>
            </li>
            <li>
              <UsersRound size={20} aria-hidden />
              <span>{t("Speakers in the room: choose 2 for a vet and a client. It's on the Record screen.")}</span>
            </li>
            <li>
              <ClipboardCheck size={20} aria-hidden />
              <span>
                {t("Review every draft before you sign it. OpenBark checks each number against the recording, but you sign the record.")}
              </span>
            </li>
          </ul>
        )}
      </div>

      <footer className="setup-walk-foot">
        {index > 0 && !busy && (
          <Button variant="ghost" onClick={() => setStep(steps[index - 1] ?? "computer")}>
            {t("Back")}
          </Button>
        )}
        {step === "computer" && (
          <Button variant="primary" onClick={() => setStep("model")}>
            {t("Continue")}
          </Button>
        )}
        {step === "model" && (
          <Button variant="primary" onClick={chooseModel} disabled={!rows}>
            {mode === "check" && !chosenRow?.installed
              ? t("Download {model} ({size})", { model: shortName(choice), size: fmtBytes(chosenRow?.size_bytes ?? 0) })
              : t("Continue")}
          </Button>
        )}
        {step === "speech" && (
          <Button variant="primary" onClick={() => setStep("download")} disabled={gpuWanted && !gpuAccepted}>
            {t("Continue")}
          </Button>
        )}
        {step === "download" &&
          (plan.length > 0 && plan.some((p) => fetches[p.id]?.state !== "done") ? (
            <Button variant="primary" onClick={() => void downloadAll()} disabled={downloading}>
              <Download size={20} aria-hidden />
              {downloadError
                ? t("Try again")
                : t("Download everything ({size})", { size: fmtBytes(plan.reduce((n, p) => n + p.size, 0)) })}
            </Button>
          ) : (
            <Button variant="primary" onClick={() => setStep("speed")}>
              {t("Continue")}
            </Button>
          ))}
        {step === "speed" && measure.state === "idle" && chosenRow?.installed && (
          <>
            <Button variant="primary" onClick={() => void runSpeedCheck()}>
              <Gauge size={20} aria-hidden />
              {t("Check the speed")}
            </Button>
            <Button onClick={() => (mode === "full" ? setStep("tips") : finish())}>{t("Skip the speed check")}</Button>
          </>
        )}
        {step === "speed" && measure.state === "running" && (
          <Button onClick={() => void engine.cancelDraft().catch(() => {})}>
            <Square size={16} aria-hidden />
            {t("Stop")}
          </Button>
        )}
        {step === "speed" && (measure.state !== "idle" || !chosenRow?.installed) && measure.state !== "running" && (
          <Button variant="primary" onClick={() => (mode === "full" ? setStep("tips") : finish())}>
            {mode === "full" ? t("Continue") : t("Finish")}
          </Button>
        )}
        {step === "tips" && (
          <>
            <Button variant="primary" onClick={() => finish()}>
              {t("Start recording")}
            </Button>
            <Button onClick={() => finish({ screen: "settings", section: "recording" })}>
              <Mic size={20} aria-hidden />
              {t("Test the microphone")}
            </Button>
          </>
        )}
      </footer>
    </div>
  );
}

/** Step 1: what this computer has, and what that means for OpenBark. */
function ComputerStep({ t, status, free }: { t: T; status: engine.Status | null; free: number | null }) {
  const hw = status?.hardware;
  if (!hw) return null;
  const gb = (b: number) => Math.round(b / 1e9);
  const on = hw.drafts_on ?? "processor";
  const graphics =
    on === "graphics_card"
      ? t("{gpu}, a graphics card with {gb} GB of its own memory", { gpu: hw.gpu ?? "", gb: gb(hw.gpu_memory_bytes ?? 0) })
      : on === "apple_silicon"
        ? t("Apple silicon: the graphics share the computer's memory")
        : hw.gpu && hw.gpu_integrated
          ? t("{gpu}, integrated graphics (sharing the processor's memory)", { gpu: hw.gpu })
          : t("No supported graphics card");
  const meaning =
    on === "processor"
      ? t(
          "Notes are written on the processor. That works on any computer, and takes some minutes per note; the next step picks the model that suits this one.",
        )
      : t("Notes are written on the graphics, not the processor alone. The speed check measures how long a note takes.");
  return (
    <>
      <dl className="setup-facts">
        <div>
          <dt className="muted">{t("Memory")}</dt>
          <dd>{t("{gb} GB", { gb: gb(hw.ram_bytes) })}</dd>
        </div>
        <div>
          <dt className="muted">{t("Processor")}</dt>
          <dd>
            {hw.cpu_cores
              ? t("{cores} physical cores ({threads} threads)", { cores: hw.cpu_cores, threads: hw.cpu_threads ?? hw.cpu_cores })
              : t("{threads} threads", { threads: hw.cpu_threads ?? 0 })}
          </dd>
        </div>
        <div>
          <dt className="muted">{t("Graphics")}</dt>
          <dd>{graphics}</dd>
        </div>
        <div>
          <dt className="muted">{t("Free disk space")}</dt>
          <dd>{free == null ? t("Not known") : t("{size} free where models are kept", { size: fmtBytes(free) })}</dd>
        </div>
        <div>
          <dt className="muted">{t("System")}</dt>
          <dd>{systemName(status?.platform)}</dd>
        </div>
      </dl>
      <p>{meaning}</p>
      <p className="setup-promise">
        <Lock size={20} aria-hidden />
        <span>{t("Everything runs on this computer. After the one-time download, OpenBark works offline.")}</span>
      </p>
    </>
  );
}

/** The download step: one timed step per model, the time left and the projected finish. */
function DownloadStep({
  t,
  plan,
  fetches,
  now,
  start,
  running,
  error,
  free,
}: {
  t: T;
  plan: QueueItem[];
  fetches: Record<string, Fetch>;
  now: number;
  start: number;
  running: boolean;
  error: string | null;
  free: number | null;
}) {
  const total = plan.reduce((n, p) => n + p.size, 0);
  const got = plan.reduce((n, p) => {
    const f = fetches[p.id];
    return n + (f?.state === "done" ? p.size : Math.min(p.size, f?.done ?? 0));
  }, 0);
  // Bytes that arrived in this run only, for the speed (finished items from an earlier try don't count).
  const arrived = plan.reduce((n, p) => {
    const f = fetches[p.id];
    if (!f || f.startedAt < start) return n;
    return n + (f.state === "done" ? p.size : Math.min(p.size, f.done));
  }, 0);
  const eta = running ? transferEta(start, now, arrived, Math.max(0, total - got) + arrived) : null;
  const name = (p: QueueItem) => (p.kind === "gpu" ? t("GPU speech pack") : p.name);
  const steps: BoardStep[] = plan.map((p) => {
    const f = fetches[p.id];
    const state: StepState = f?.state ?? "pending";
    const left = eta && state === "active" ? Math.round(Math.max(0, p.size - (f?.done ?? 0)) / eta.bytesPerSecond) : null;
    const detail =
      state === "done"
        ? f && f.endedAt
          ? t("Done in {time}", { time: fmtSecs(since(f.startedAt, f.endedAt)) })
          : t("Ready")
        : state === "active" && f
          ? [
              t("{done} of {total}", { done: fmtBytes(f.done), total: fmtBytes(p.size) }),
              t("{time} so far", { time: fmtSecs(since(f.startedAt, now)) }),
              eta ? t("{size} a second", { size: fmtBytes(eta.bytesPerSecond) }) : null,
              left != null ? t("{time} left", { time: aboutTime(t, left) }) : null,
            ]
              .filter(Boolean)
              .join(", ")
          : `${fmtBytes(p.size)}, ${t("Waiting")}`;
    return { key: p.id, label: name(p), detail, state };
  });
  const allDone = plan.length > 0 && plan.every((p) => fetches[p.id]?.state === "done");
  if (plan.length === 0) {
    return <p className="ob-notice" data-tone="ok">{t("Everything is already on this computer.")}</p>;
  }
  const pct = Math.round((got / Math.max(1, total)) * 100);
  const short = free != null && free < total - got;
  return (
    <>
      <p className="small muted">
        {t("Smallest first. {size} in all, once. After that OpenBark works offline, and the computer stays awake while they download.", {
          size: fmtBytes(total),
        })}
      </p>
      {short && (
        <p className="ob-notice" data-tone="danger" role="alert">
          {t("There's {free} free where models are kept, less than the {size} to download. Free some space first.", {
            free: fmtBytes(free ?? 0),
            size: fmtBytes(total - got),
          })}
        </p>
      )}
      {(running || allDone || got > 0) && (
        <ProgressBoard
          label={t("Downloading models")}
          title={allDone ? t("Models ready") : t("Downloading models")}
          steps={steps}
          fraction={got / Math.max(1, total)}
          status={
            allDone
              ? "100%"
              : eta
                ? `${pct}%, ${t("{time} left, done by {clock}", { time: aboutTime(t, eta.secsLeft), clock: clockTime(now + eta.secsLeft * 1000) })}`
                : running
                  ? `${pct}%, ${t("working out the time left")}`
                  : `${pct}%`
          }
          aside={
            running ? (
              <span>
                {t("{time} so far", { time: fmtSecs(since(start, now)) })}.{" "}
                {eta
                  ? t("Projected total {time}. Expected finish {clock}.", {
                      time: fmtSecs(since(start, now) + eta.secsLeft),
                      clock: clockTime(now + eta.secsLeft * 1000),
                    })
                  : t("The time left shows after a few seconds, from the speed measured.")}
              </span>
            ) : undefined
          }
        />
      )}
      {!running && !allDone && got === 0 && (
        <ul className="setup-list">
          {plan.map((p) => (
            <li key={p.id}>
              <span>{name(p)}</span>
              <span className="mono faint">{fmtBytes(p.size)}</span>
              <span className="small muted">{t("Waiting")}</span>
            </li>
          ))}
        </ul>
      )}
      {allDone && plan.some((p) => p.id === GPU_PACK) && (
        <p className="ob-notice" data-tone="ok">{t("Downloaded. Restart OpenBark to start using the graphics card.")}</p>
      )}
      {error && (
        <p className="ob-notice" data-tone="danger" role="alert">
          {error}
        </p>
      )}
    </>
  );
}

/** The speed check: optional, timed step by step, and plain about what it found. */
function SpeedStep({
  t,
  model,
  installed,
  measure,
  now,
  suggest,
  smallerInstalled,
  smallerSize,
  onUseSmaller,
}: {
  t: T;
  model: string;
  installed: boolean;
  measure: Measure;
  now: number;
  suggest: boolean;
  smallerInstalled: boolean;
  smallerSize: number;
  onUseSmaller: () => void;
}) {
  if (!installed) {
    return <p className="ob-notice">{t("{model} isn't downloaded yet, so its speed can't be checked. Run setup again later.", { model })}</p>;
  }
  if (measure.state === "idle") {
    return (
      <>
        <p>
          {t(
            "Optional. {model} writes a short practice note from an invented consult, for about half a minute, to measure how fast it is on this computer. Nothing from your visits is used.",
            { model },
          )}
        </p>
        <p className="small muted">{t("Then you'll know how long a note takes here, before your first consult.")}</p>
      </>
    );
  }
  if (measure.state === "running") return <SpeedBoard t={t} model={model} measure={measure} now={now} />;
  if (measure.state === "stopped") return <p className="ob-notice">{t("Speed check stopped. You can run it again from Models, Run setup again.")}</p>;
  if (measure.state === "failed") {
    return (
      <p className="ob-notice" data-tone="danger" role="alert">
        {t("The speed check didn't finish: {error}", { error: measure.error ?? "" })}
      </p>
    );
  }
  const r = measure.result;
  if (!r) return null;
  const secs = r.long_visit_note_secs;
  return (
    <>
      <p className="setup-result" role="status">
        {secs == null
          ? t("The speed check couldn't measure a speed on this computer.")
          : secs < 60
            ? t("On this computer, a note for a long visit will take less than a minute with {model}.", { model })
            : t("On this computer, a note for a long visit will take about {n} minutes with {model}.", {
                n: noteMinutes(secs),
                model,
              })}
      </p>
      <p className="small muted">
        {t(
          "Measured just now: it read {read} tokens a second and wrote {write} a second. The estimate is for OpenBark's long test visit, 16 minutes of non-stop talking; shorter visits take less.",
          { read: Math.round(r.read_tokens_per_second), write: r.write_tokens_per_second.toFixed(1) },
        )}
      </p>
      {suggest && (
        <div className="ob-notice setup-suggest">
          <p>
            {t(
              "That's a long wait. Gemma 4 E2B writes notes about 1.6 times as fast on the same processor. It finds slightly fewer details (90% of facts against 95% in OpenBark's tests), so check doses and weights closely.",
            )}
          </p>
          <div className="ask-actions">
            <Button size="compact" onClick={onUseSmaller}>
              {smallerInstalled
                ? t("Use {model}", { model: "Gemma 4 E2B" })
                : t("Download {model} ({size})", { model: "Gemma 4 E2B", size: fmtBytes(smallerSize) })}
            </Button>
          </div>
        </div>
      )}
    </>
  );
}

/** The speed check's board: loading, reading, writing, each with its clock, the time left and the finish. */
function SpeedBoard({ t, model, measure, now }: { t: T; model: string; measure: Measure; now: number }) {
  const { at, prompt, writing } = measure;
  const stage = at.write ? "write" : at.read ? "read" : "load";
  const readRate =
    prompt && at.read && prompt.done > 0 && now - at.read > 1000 ? prompt.done / ((now - at.read) / 1000) : null;
  const readLeft = prompt && readRate ? Math.round(Math.max(0, prompt.total - prompt.done) / readRate) : null;
  const writeLeft = at.write ? Math.max(0, WRITE_BUDGET_SECS - since(at.write, now)) : null;
  const left = stage === "write" ? writeLeft : stage === "read" && readLeft != null ? readLeft + WRITE_BUDGET_SECS : null;
  const loadState: StepState = stage === "load" ? "active" : "done";
  const steps: BoardStep[] = [
    {
      key: "load",
      label: t("Loading the note model"),
      detail:
        loadState === "active"
          ? `${t("{time} so far", { time: fmtSecs(since(at.load ?? at.start, now)) })}. ${t("The time left shows once it has loaded.")}`
          : at.load
            ? t("Done in {time}", { time: fmtSecs(since(at.load, at.read ?? now)) })
            : t("Already loaded"),
      state: loadState,
    },
    {
      key: "read",
      label: t("Reading a practice consult"),
      detail:
        stage === "read"
          ? [
              prompt ? t("{done} of {total}", { done: prompt.done.toLocaleString(), total: prompt.total.toLocaleString() }) : null,
              t("{time} so far", { time: fmtSecs(since(at.read ?? now, now)) }),
              readLeft != null ? t("{time} left", { time: aboutTime(t, readLeft) }) : null,
            ]
              .filter(Boolean)
              .join(", ")
          : stage === "write" && at.read
            ? t("Done in {time}", { time: fmtSecs(since(at.read, at.write ?? now)) })
            : undefined,
      state: stage === "load" ? "pending" : stage === "read" ? "active" : "done",
    },
    {
      key: "write",
      label: t("Writing a practice note"),
      detail:
        stage === "write"
          ? [
              writing ? t("{n} tokens, {rate} a second", { n: writing.tokens, rate: writing.rate.toFixed(1) }) : null,
              t("{time} so far", { time: fmtSecs(since(at.write ?? now, now)) }),
              writeLeft != null ? t("{time} left", { time: fmtSecs(writeLeft) }) : null,
            ]
              .filter(Boolean)
              .join(", ")
          : undefined,
      state: stage === "write" ? "active" : "pending",
    },
  ];
  const fraction = stage === "load" ? 0.05 : stage === "read" ? 0.1 + 0.4 * (prompt ? prompt.done / Math.max(1, prompt.total) : 0) : 0.5 + 0.5 * Math.min(1, since(at.write ?? now, now) / WRITE_BUDGET_SECS);
  return (
    <ProgressBoard
      label={t("Speed check")}
      title={t("Checking the speed of {model}", { model })}
      steps={steps}
      fraction={fraction}
      status={left != null ? t("{time} left, done by {clock}", { time: aboutTime(t, left), clock: clockTime(now + left * 1000) }) : t("working out the time left")}
      aside={
        <span>
          {t("{time} so far", { time: fmtSecs(since(at.start, now)) })}.{" "}
          {left != null
            ? t("Projected total {time}. Expected finish {clock}.", {
                time: fmtSecs(since(at.start, now) + left),
                clock: clockTime(now + left * 1000),
              })
            : t("The time left shows once the model has loaded.")}
        </span>
      }
    />
  );
}
