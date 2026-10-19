import { Button, MarkMono } from "@openbark/ui";
import { Delete, Grid3x3, Mic, MicOff, Pause, Phone, PhoneOff, Play, Trash2, X } from "lucide-react";
import { useCallback, useEffect, useReducer, useRef, useState } from "react";
import { loadSettings } from "../context";
import type { CallTarget } from "../engine";
import { translate, type Vars } from "../i18n";
import * as api from "./api";
import { createTaps, type CallTaps } from "./callAudio";
import { callReducer, endLine, initialCall, statusLine } from "./callState";
import { CALL_ERRORS, callErrorMessage } from "./errors";
import { callLength, callTimer, formatPhone, toE164 } from "./format";
import { canCall, createDevice, type VoiceCall, type VoiceDevice } from "./voice";

const KEYS = ["1", "2", "3", "4", "5", "6", "7", "8", "9", "*", "0", "#"];
/** SDK warnings that mean the call connected but no audio gets through. */
const NO_AUDIO_WARNINGS = new Set(["constant-audio-output-level", "ice-connectivity-lost"]);

async function closeWindow() {
  if (api.isDesktop) {
    const { getCurrentWindow } = await import("@tauri-apps/api/window");
    await getCurrentWindow().close();
  } else {
    window.close();
  }
}

/**
 * The phone window (ADR-0023 sections 5 and 7): the call the main window
 * prepared, or the dial pad; the announcement and "Did the client agree?";
 * Mute, Hold, Keypad, Hang up; and the recording, made on this computer.
 */
export function PhoneApp() {
  const settings = loadSettings();
  const t = useCallback((text: string, vars?: Vars) => translate(settings.uiLanguage, text, vars), [settings.uiLanguage]);
  const [target, setTarget] = useState<CallTarget | null | undefined>(undefined);
  const [problem, setProblem] = useState<string | null>(null);
  const [typed, setTyped] = useState("");
  const [skipRecording, setSkipRecording] = useState(false);
  const [call, dispatch] = useReducer(callReducer, initialCall);
  const [now, setNow] = useState(() => Date.now());
  const [keypad, setKeypad] = useState(false);
  const [askDelete, setAskDelete] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const device = useRef<VoiceDevice | null>(null);
  const active = useRef<VoiceCall | null>(null);
  const taps = useRef<CallTaps | null>(null);
  const recording = useRef<api.CallRecording | null>(null);
  /** The recording being started: the engine loads its speech models first, which takes a moment. */
  const starting = useRef<Promise<api.CallRecording | null> | null>(null);
  /** Audio from the answer until the recording is ready, so the announcement is in it. */
  const buffered = useRef<Float32Array[]>([]);
  /** Decisions made before the recording was ready, applied once it is. */
  const decision = useRef<"agreed" | "declined" | null>(null);
  const connectedAt = useRef<number | null>(null);
  const finished = useRef(false);
  const phase = useRef(call.phase);
  phase.current = call.phase;

  /** Reads the call the main window chose. `replace` puts its number on the dial pad even over what was typed. */
  const loadTarget = useCallback((replace = true) => {
    api.pending().then(
      (p) => {
        setTarget(p);
        setTyped((typed) => (replace || !typed.trim() ? (p?.number ?? "") : typed));
      },
      (e) => {
        setTarget(null);
        setProblem(callErrorMessage(e));
      },
    );
  }, []);

  useEffect(() => {
    if (!canCall()) {
      setProblem(api.isDesktop ? CALL_ERRORS.linux : CALL_ERRORS.noWebrtc);
      setTarget(null);
      return;
    }
    loadTarget();
    // The main window chose someone else to call: show them, unless a call is on.
    let off: (() => void) | undefined;
    if (api.isDesktop) {
      void import("@tauri-apps/api/event").then(({ listen }) =>
        listen("phone-target", () => phase.current === "idle" && loadTarget()).then((f) => (off = f)),
      );
    } else {
      try {
        const channel = new BroadcastChannel("openbark-phone");
        channel.onmessage = (e: MessageEvent<{ type?: string }>) => {
          if (e.data?.type === "target" && phase.current === "idle") loadTarget();
        };
        off = () => channel.close();
      } catch {
        // Old browsers: the call is read when the page opens.
      }
    }
    return () => off?.();
  }, [loadTarget]);

  useEffect(() => {
    if (call.phase !== "connected") return;
    const id = window.setInterval(() => setNow(Date.now()), 500);
    return () => window.clearInterval(id);
  }, [call.phase]);

  const cleanUp = useCallback(async () => {
    device.current?.destroy();
    device.current = null;
    active.current = null;
    await taps.current?.close();
    taps.current = null;
  }, []);

  /** The call ended, however: save the visit (or the declined visit's length). */
  const finish = useCallback(async () => {
    if (finished.current) return;
    finished.current = true;
    const at = Date.now();
    dispatch({ type: "disconnected", at });
    taps.current?.setRecording(false);
    const secs = connectedAt.current === null ? 0 : (at - connectedAt.current) / 1000;
    const sid = active.current?.parameters?.CallSid ?? null;
    await cleanUp();
    // A hang-up while the recording was still starting waits for it.
    const rec = recording.current ?? (await starting.current) ?? null;
    recording.current = null;
    starting.current = null;
    if (!rec) return;
    try {
      const view = await rec.end(secs, sid);
      if (view) {
        dispatch({ type: "saved" });
        api.announceSaved(view.id);
      }
    } catch (e) {
      setNotice(callErrorMessage(e));
    }
  }, [cleanUp]);

  async function dial() {
    setProblem(null);
    setNotice(null);
    const number = toE164(typed);
    if (!number) {
      setProblem(t("{number} isn't a full phone number. Write it with + and the country code, for example +15555550142.", { number: typed || "?" }));
      return;
    }
    const record = Boolean(target?.record_calls) && !skipRecording;
    finished.current = false;
    connectedAt.current = null;
    dispatch({ type: "dial", number, record });
    try {
      const token = await api.token();
      buffered.current = [];
      decision.current = null;
      starting.current = null;
      taps.current = await createTaps((frames) => {
        if (recording.current) recording.current.push(frames);
        // About a minute at most, while the engine gets ready.
        else if (buffered.current.length < 600) buffered.current.push(frames);
      });
      device.current = await createDevice(token.token, taps.current, async () => (await api.token()).token);
      device.current.on("error", (e) => setNotice(callErrorMessage(e)));
      const c = await device.current.connect({ params: { To: number } });
      active.current = c;
      c.on("ringing", () => dispatch({ type: "ringing" }));
      c.on("accept", () => {
        const at = Date.now();
        connectedAt.current = at;
        setNow(at);
        dispatch({ type: "accepted", at });
        if (record) startRecording(number, c);
      });
      c.on("disconnect", () => void finish());
      c.on("cancel", () => void finish());
      c.on("reject", () => void finish());
      c.on("reconnecting", () => dispatch({ type: "reconnecting", on: true }));
      c.on("error", (e) => {
        const message = callErrorMessage(e);
        if (connectedAt.current === null) dispatch({ type: "failed", message });
        else setNotice(message);
      });
      c.on("warning", (name) => NO_AUDIO_WARNINGS.has(name) && setNotice(CALL_ERRORS.noAudio));
      c.on("warning-cleared", (name) => NO_AUDIO_WARNINGS.has(name) && setNotice(null));
    } catch (e) {
      dispatch({ type: "failed", message: callErrorMessage(e) });
      await cleanUp();
    }
  }

  /**
   * The client answered: record both sides on this computer, from this
   * moment. Audio is kept here until the engine is ready, so the
   * announcement is the first thing in the recording.
   */
  function startRecording(number: string, c: VoiceCall) {
    const t0 = taps.current;
    if (!t0) return;
    t0.setRecording(true);
    dispatch({ type: "recording" });
    starting.current = (async () => {
      try {
        const rec = await api.startRecording({
          number,
          patient_id: target?.patient_id ?? null,
          call_sid: c.parameters?.CallSid ?? null,
          sample_rate: t0.sampleRate,
        });
        for (const frames of buffered.current) rec.push(frames);
        buffered.current = [];
        recording.current = rec;
        if (decision.current) await sendDecision(rec, decision.current);
        return rec;
      } catch (e) {
        // The call goes on; it just isn't recorded.
        t0.setRecording(false);
        buffered.current = [];
        setNotice(`${t("This call isn't being recorded.")} ${callErrorMessage(e)}`);
        dispatch({ type: "unrecorded" });
        return null;
      }
    })();
  }

  async function sendDecision(rec: api.CallRecording, outcome: "agreed" | "declined") {
    await rec.consent(outcome, outcome === "agreed" ? 0 : null).catch((e: unknown) => setNotice(callErrorMessage(e)));
  }

  async function answer(agreed: boolean) {
    if (!agreed) return stopRecording();
    dispatch({ type: "agreed" });
    if (recording.current) await sendDecision(recording.current, "agreed");
    else decision.current = "agreed";
  }

  /** "No, stop recording" or "Stop and delete the recording": the call goes on. */
  function stopRecording() {
    setAskDelete(false);
    taps.current?.setRecording(false);
    buffered.current = [];
    dispatch({ type: "declined" });
    if (recording.current) void sendDecision(recording.current, "declined");
    else decision.current = "declined";
  }

  function hangUp() {
    dispatch({ type: "hangup" });
    if (active.current) active.current.disconnect();
    else void finish();
  }

  function mute() {
    active.current?.mute(!call.muted);
    dispatch({ type: "mute", on: !call.muted });
  }

  function hold() {
    const on = !call.held;
    active.current?.mute(on || call.muted);
    taps.current?.setHeld(on);
    dispatch({ type: "hold", on });
  }

  const who = target && (target.last_name || target.patient_name) ? [target.last_name, target.patient_name].filter(Boolean) : [];
  const title = who.length ? who.join(" · ") : formatPhone(call.number || toE164(typed) || typed) || t("New call");

  if (target === undefined) {
    return (
      <main className="phone">
        <div className="ob-skeleton" style={{ height: 120 }} />
      </main>
    );
  }

  return (
    <main className="phone" aria-label={t("Phone")}>
      <header className="phone-head">
        <MarkMono size={20} aria-hidden />
        <span className="small muted">{t("Phone, through your clinic's Twilio account")}</span>
      </header>

      <section className="phone-who" aria-live="polite">
        <h1>{title}</h1>
        {who.length > 0 && (call.number || target?.number) && (
          <span className="mono muted">{formatPhone(call.number || target?.number)}</span>
        )}
        {call.phase !== "idle" && call.phase !== "ended" && (
          <p className="phone-status mono">
            {call.recording && <span className="phone-rec-dot" aria-hidden />}
            {statusLine(call, now, t, callTimer)}
          </p>
        )}
      </section>

      {problem && (
        <p className="ob-notice" data-tone="danger" role="alert">
          {t(problem)}
        </p>
      )}
      {notice && (
        <p className="ob-notice" role="status">
          {t(notice)}
        </p>
      )}

      {call.phase === "idle" && (
        <section className="phone-dial">
          {target && !target.number && target.patient_id && (
            <p className="small muted">{t("No phone number for this client. Add one, or type it on the dial pad.")}</p>
          )}
          <label className="ob-field">
            <span className="label">{t("Number")}</span>
            <input
              className="ob-input mono"
              inputMode="tel"
              autoComplete="off"
              value={typed}
              onChange={(e) => setTyped(e.target.value)}
              placeholder="+1 555 555 0142"
            />
          </label>
          <DialPad onKey={(k) => setTyped((v) => v + k)} onBack={() => setTyped((v) => v.slice(0, -1))} t={t} />
          {target?.record_calls ? (
            <div className="phone-precall">
              {!skipRecording && <p className="small">{t("This call will be recorded and announced.")}</p>}
              <label className="check">
                <input type="checkbox" checked={skipRecording} onChange={(e) => setSkipRecording(e.target.checked)} />
                <span>{t("Don't record this call")}</span>
              </label>
            </div>
          ) : (
            target && <p className="small muted">{t("Calls aren't recorded on this computer.")}</p>
          )}
          <Button variant="primary" size="prominent" onClick={() => void dial()} disabled={!target || !typed.trim()}>
            <Phone size={20} aria-hidden />
            {t("Call")}
          </Button>
        </section>
      )}

      {call.phase === "connected" && call.message && <p className="small muted phone-outcome">{t(call.message)}</p>}

      {call.phase === "connected" && call.consent === "read" && target && (
        <section className="phone-script" aria-label={t("Announcement")}>
          <p className="label">{t("Read this to the client now:")}</p>
          <blockquote className="phone-script-text">{target.announcement}</blockquote>
          <Button variant="primary" onClick={() => dispatch({ type: "read" })}>
            {t("I've read it")}
          </Button>
        </section>
      )}

      {call.phase === "connected" && call.consent === "ask" && (
        <section className="phone-script" aria-label={t("Did the client agree?")}>
          <p className="label">{t("Did the client agree?")}</p>
          <div className="phone-answer">
            <Button variant="primary" onClick={() => void answer(true)}>
              {t("Yes, keep recording")}
            </Button>
            <Button onClick={() => void answer(false)}>{t("No, stop recording")}</Button>
          </div>
          <p className="small muted">{t("Keep recording for now. Answer when you know.")}</p>
        </section>
      )}

      {call.phase === "connected" && (
        <section className="phone-controls" aria-label={t("Call controls")}>
          <div className="phone-grid">
            <button type="button" className="phone-key" aria-pressed={call.muted} onClick={mute}>
              {call.muted ? <MicOff size={22} aria-hidden /> : <Mic size={22} aria-hidden />}
              <span>{call.muted ? t("Unmute") : t("Mute")}</span>
            </button>
            <button type="button" className="phone-key" aria-pressed={call.held} onClick={hold}>
              {call.held ? <Play size={22} aria-hidden /> : <Pause size={22} aria-hidden />}
              <span>{call.held ? t("Resume") : t("Hold")}</span>
            </button>
            <button type="button" className="phone-key" aria-pressed={keypad} onClick={() => setKeypad(!keypad)}>
              <Grid3x3 size={22} aria-hidden />
              <span>{t("Keypad")}</span>
            </button>
          </div>
          {keypad && <DialPad onKey={(k) => active.current?.sendDigits(k)} t={t} />}
          {call.recording &&
            (askDelete ? (
              <div className="phone-confirm" role="alertdialog" aria-label={t("Stop and delete the recording")}>
                <p className="small">{t("Delete the recording so far? The call goes on, unrecorded.")}</p>
                <div className="phone-answer">
                  <Button variant="danger" onClick={stopRecording}>
                    {t("Delete the recording")}
                  </Button>
                  <Button variant="ghost" onClick={() => setAskDelete(false)}>
                    {t("Keep recording")}
                  </Button>
                </div>
              </div>
            ) : (
              <Button variant="ghost" size="compact" onClick={() => setAskDelete(true)}>
                <Trash2 size={16} aria-hidden />
                {t("Stop and delete the recording")}
              </Button>
            ))}
        </section>
      )}

      {(call.phase === "calling" || call.phase === "ringing" || call.phase === "connected") && (
        <Button variant="danger" size="prominent" className="phone-hangup" onClick={hangUp}>
          <PhoneOff size={20} aria-hidden />
          {t("Hang up")}
        </Button>
      )}

      {call.phase === "ended" && (
        <section className="phone-ended" aria-live="polite">
          {call.message && <p>{t(call.message)}</p>}
          {endLine(call, settings.draftOnStop, t, callLength) && (
            <p className="mono">{endLine(call, settings.draftOnStop, t, callLength)}</p>
          )}
          <div className="phone-answer">
            <Button
              variant="primary"
              onClick={() => {
                dispatch({ type: "reset" });
                setNotice(null);
                setTyped("");
                loadTarget(false);
              }}
            >
              <Phone size={18} aria-hidden />
              {t("New call")}
            </Button>
            <Button variant="ghost" onClick={() => void closeWindow()}>
              <X size={18} aria-hidden />
              {t("Close")}
            </Button>
          </div>
        </section>
      )}

      <footer className="phone-foot small faint">
        {t("Twilio carries the call. The recording is made and kept on this computer.")}
      </footer>
    </main>
  );
}

function DialPad({
  onKey,
  onBack,
  t,
}: {
  onKey: (key: string) => void;
  onBack?: () => void;
  t: (text: string) => string;
}) {
  return (
    <div className="phone-pad" role="group" aria-label={t("Keypad")}>
      {KEYS.map((k) => (
        <button key={k} type="button" className="phone-pad-key mono" onClick={() => onKey(k)}>
          {k}
        </button>
      ))}
      {onBack && (
        <button type="button" className="phone-pad-key phone-pad-back" aria-label={t("Delete a digit")} onClick={onBack}>
          <Delete size={20} aria-hidden />
        </button>
      )}
    </div>
  );
}
