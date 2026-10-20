import { Button } from "@openbark/ui";
import { Mic, Square } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { useApp } from "../context";
import * as engine from "../engine";
import { useT } from "../i18n";
import { listBrowserMics, testBrowserMic, type BrowserMic } from "../mic/browser";
import { micNotice, micQuality, type MicQuality } from "../mic/health";

/** A browser test stops by itself after this long (the desktop engine's after a minute). */
const TEST_LIMIT_MS = 30_000;
/** A level above this means someone is speaking (about -45 dBFS). */
const VOICE_LEVEL = 0.25;

interface Testing {
  device: string;
  sampleRate: number;
  quality: MicQuality;
  fellBack: boolean;
}

/**
 * Choose the microphone and try it before recording: a live level meter and
 * a plain verdict ("Hearing you", "No sound yet", "sending silence"), plus a
 * warning for a Bluetooth headset in its low-quality hands-free mode.
 * Desktop: the engine opens the microphone natively. Web: the browser does.
 */
export function MicCheck({ disabled = false, id = "mic-check" }: { disabled?: boolean; id?: string }) {
  const app = useApp();
  const t = useT();
  const desktop = engine.isDesktop;
  const [devices, setDevices] = useState<string[]>([]);
  const [browserMics, setBrowserMics] = useState<BrowserMic[]>([]);
  const [testing, setTesting] = useState<Testing | null>(null);
  const [level, setLevel] = useState(0);
  const [silent, setSilent] = useState(false);
  const [heardAt, setHeardAt] = useState<number | null>(null);
  const [startedAt, setStartedAt] = useState(0);
  const [now, setNow] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const stopRef = useRef<(() => void) | null>(null);

  const refresh = useCallback(() => {
    if (desktop) engine.inputDevices().then(setDevices, () => {});
    else listBrowserMics().then(setBrowserMics, () => {});
  }, [desktop]);

  useEffect(() => {
    refresh();
    if (desktop || !navigator.mediaDevices?.addEventListener) return;
    navigator.mediaDevices.addEventListener("devicechange", refresh);
    return () => navigator.mediaDevices.removeEventListener("devicechange", refresh);
  }, [desktop, refresh]);

  const stop = useCallback(() => {
    stopRef.current?.();
    stopRef.current = null;
    setTesting(null);
    setLevel(0);
    setSilent(false);
  }, []);

  // Stop when the screen closes, or when a recording starts.
  useEffect(() => stop, [stop]);
  useEffect(() => {
    if (disabled) stop();
  }, [disabled, stop]);

  // The verdict changes with time ("No sound yet" after a few seconds).
  useEffect(() => {
    if (!testing) return;
    const timer = setInterval(() => {
      const n = performance.now();
      setNow(n);
      if (!desktop && n - startedAt > TEST_LIMIT_MS) stop();
    }, 250);
    return () => clearInterval(timer);
  }, [testing, desktop, startedAt, stop]);

  const heard = (l: number) => {
    setLevel(l);
    if (l >= VOICE_LEVEL) setHeardAt(performance.now());
  };

  async function start() {
    setError(null);
    setHeardAt(null);
    setSilent(false);
    const t0 = performance.now();
    setStartedAt(t0);
    setNow(t0);
    try {
      if (desktop) {
        const off = await engine.on("micTest", (m) => {
          heard(m.level);
          setSilent(m.silent);
          setTesting((cur) =>
            cur ? { ...cur, device: m.device, sampleRate: m.sample_rate, quality: m.quality } : cur,
          );
          if (m.done) {
            stopRef.current?.();
            stopRef.current = null;
            setTesting(null);
            setLevel(0);
          }
        });
        try {
          const started = await engine.micTestStart(app.settings.device);
          stopRef.current = () => {
            off();
            void engine.micTestStop().catch(() => {});
          };
          setTesting({
            device: started.device,
            sampleRate: started.sample_rate,
            quality: started.quality,
            fellBack: Boolean(started.replaced),
          });
        } catch (e) {
          off();
          throw e;
        }
      } else {
        const test = await testBrowserMic(app.settings.browserMic, (l, s) => {
          heard(l);
          setSilent(s);
        });
        stopRef.current = test.stop;
        setTesting({
          device: test.label,
          sampleRate: test.sampleRate,
          quality: micQuality(test.label, test.sampleRate),
          fellBack: test.fellBack,
        });
        // Names appear once the microphone is allowed.
        refresh();
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setTesting(null);
    }
  }

  function choose(value: string) {
    stop();
    if (desktop) app.setSettings({ device: value || null });
    else app.setSettings({ browserMic: value || null });
  }

  const value = (desktop ? app.settings.device : app.settings.browserMic) ?? "";
  const options = desktop
    ? devices.map((d) => ({ value: d, label: d }))
    : browserMics.map((d, i) => ({ value: d.deviceId, label: d.label || t("Microphone {n}", { n: i + 1 }) }));
  // The chosen one isn't connected: keep it listed, marked.
  if (value && !options.some((o) => o.value === value)) {
    options.push({ value, label: desktop ? t("{name} (not connected)", { name: value }) : t("Chosen microphone (not connected)") });
  }

  const verdict = (() => {
    if (!testing) return null;
    if (silent) {
      return {
        tone: "danger" as const,
        text:
          micNotice(
            t,
            { state: "silent", device: testing.device, sample_rate: testing.sampleRate, quality: testing.quality, switched_from: null, seconds: 0 },
            !desktop,
          )?.text ?? "",
      };
    }
    if (heardAt !== null && now - heardAt < 1500) return { tone: "ok" as const, text: t("Hearing you.") };
    if (heardAt === null && now - startedAt > 4000) {
      return { tone: "warn" as const, text: t("No speech heard yet. Speak normally, or check the microphone isn't muted.") };
    }
    return { tone: "info" as const, text: t("Speak normally: the bar should move.") };
  })();
  const quality = testing
    ? micNotice(
        t,
        { state: "ok", device: testing.device, sample_rate: testing.sampleRate, quality: testing.quality, switched_from: null, seconds: 0 },
        !desktop,
      )
    : null;

  return (
    <div className="mic-check">
      <label className="ob-field" htmlFor={id}>
        <span className="label">{t("Microphone")}</span>
        <select id={id} className="ob-select" value={value} onChange={(e) => choose(e.target.value)} disabled={disabled}>
          <option value="">{t("System default")}</option>
          {options.map((o) => (
            <option key={o.value} value={o.value}>
              {o.label}
            </option>
          ))}
        </select>
      </label>
      <div className="mic-check-row">
        <Button size="compact" onClick={testing ? stop : () => void start()} disabled={disabled}>
          {testing ? <Square size={16} aria-hidden /> : <Mic size={16} aria-hidden />}
          {testing ? t("Stop test") : t("Test microphone")}
        </Button>
        <div
          className="mic-meter"
          role="meter"
          aria-label={t("Microphone level")}
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={Math.round(level * 100)}
          data-active={testing ? "true" : undefined}
        >
          <span style={{ transform: `scaleX(${testing ? level : 0})` }} />
        </div>
      </div>
      {testing && (
        <p className="small muted mic-check-device">
          {testing.device || t("the microphone")}
          {testing.sampleRate ? `, ${Math.round(testing.sampleRate / 100) / 10} kHz` : ""}
        </p>
      )}
      {testing?.fellBack && (
        <p className="small muted" role="status">
          {t("The chosen microphone isn't connected, so this is the system default.")}
        </p>
      )}
      {verdict && (
        <p className="small mic-check-verdict" data-tone={verdict.tone} role="status" aria-live="polite">
          {verdict.text}
        </p>
      )}
      {quality && <p className="small muted">{quality.text}</p>}
      {error && (
        <p className="ob-notice" data-tone="danger" role="alert">
          {t(error)}
        </p>
      )}
    </div>
  );
}
