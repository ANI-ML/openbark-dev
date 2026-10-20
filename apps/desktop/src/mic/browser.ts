// Web mode's microphone: the browser captures it and streams PCM to the
// engine over a WebSocket. This file opens the microphone robustly and keeps
// a recording going when the microphone changes under it:
// - the worklet is a static file (public/mic-tap.js): the page's CSP blocks
//   a blob: module;
// - Firefox can't connect a microphone to an AudioContext running at another
//   rate (a 44.1 or 16 kHz headset when the context is 48 kHz), so the
//   context is made at the microphone's rate when that happens, and the
//   engine is told about any change of rate mid-recording;
// - Safari starts an AudioContext suspended until a user gesture: it is
//   resumed at once, again on the next click or key press, and whenever the
//   tab comes back to the front;
// - a microphone that ends (unplugged, Bluetooth dropped) is replaced by the
//   chosen one or the system default as soon as one is available; until
//   then the recording is paused (nothing is sent), never silently empty;
// - the watchdog warns about silence and audio that stops arriving.
import { MicError, micError, micUnavailable, MIC_ERRORS } from "./errors";
import type { MicHealth } from "./health";
import { MicWatchdog } from "./watchdog";

/**
 * Raw capture: no echo cancellation, noise suppression or automatic gain.
 * Speech recognition does better on the microphone's own signal, and this
 * matches the desktop app, which records the device as it is.
 * TODO(speakerphone): decide jointly with the speakerphone work (branch
 * feat/speakerphone) whether a phone on speaker needs autoGainControl here,
 * or whether the engine's leveller covers both desktop and web. Keep the
 * desktop and web choices the same.
 */
export const CAPTURE_CONSTRAINTS: MediaTrackConstraints = {
  echoCancellation: false,
  noiseSuppression: false,
  autoGainControl: false,
  channelCount: { ideal: 1 },
};

export type OnBlock = (samples: Float32Array<ArrayBuffer>, peak: number, sampleRate: number) => void;

/** An open microphone, tapped through the worklet. */
export interface MicInput {
  track: MediaStreamTrack;
  ctx: AudioContext;
  sampleRate: number;
  label: string;
  /** The device it is (from the track), for noticing a new default. */
  deviceId: string | null;
  groupId: string | null;
  /** The chosen microphone wasn't available, so this is the default. */
  fellBack: boolean;
  close(): void;
}

export interface BrowserMic {
  deviceId: string;
  label: string;
  groupId: string;
}

/** Microphones the browser offers (names appear once the microphone is allowed). */
export async function listBrowserMics(): Promise<BrowserMic[]> {
  if (!navigator.mediaDevices?.enumerateDevices) return [];
  const all = await navigator.mediaDevices.enumerateDevices();
  return all
    .filter((d) => d.kind === "audioinput" && d.deviceId && !["default", "communications"].includes(d.deviceId))
    .map((d) => ({ deviceId: d.deviceId, label: d.label, groupId: d.groupId }));
}

/** A promise that gives up after `ms` (an AudioContext's resume can wait forever for a gesture). */
function within<T>(p: Promise<T>, ms: number): Promise<T | undefined> {
  return Promise.race([p, new Promise<undefined>((r) => setTimeout(() => r(undefined), ms))]);
}

async function getStream(deviceId: string | null): Promise<{ stream: MediaStream; fellBack: boolean }> {
  const unavailable = micUnavailable();
  if (unavailable) throw unavailable;
  try {
    const audio = deviceId ? { ...CAPTURE_CONSTRAINTS, deviceId: { exact: deviceId } } : CAPTURE_CONSTRAINTS;
    return { stream: await navigator.mediaDevices.getUserMedia({ audio }), fellBack: false };
  } catch (e) {
    const name = (e as { name?: string }).name;
    // The chosen microphone isn't connected: the default will do, and the
    // screen says which one is in use.
    if (deviceId && (name === "OverconstrainedError" || name === "NotFoundError")) {
      try {
        return { stream: await navigator.mediaDevices.getUserMedia({ audio: CAPTURE_CONSTRAINTS }), fellBack: true };
      } catch (e2) {
        throw micError(e2);
      }
    }
    throw micError(e);
  }
}

/** An AudioContext the stream can feed: the default rate, or the stream's own where the browser insists (Firefox). */
function contextFor(stream: MediaStream): { ctx: AudioContext; source: MediaStreamAudioSourceNode } {
  let ctx = new AudioContext({ latencyHint: "interactive" });
  try {
    return { ctx, source: ctx.createMediaStreamSource(stream) };
  } catch (e) {
    void ctx.close();
    const rate = stream.getAudioTracks()[0]?.getSettings().sampleRate;
    if (!rate) throw micError(e);
    ctx = new AudioContext({ latencyHint: "interactive", sampleRate: rate });
    return { ctx, source: ctx.createMediaStreamSource(stream) };
  }
}

/** Open a microphone (the chosen one, else the default) and tap it. */
export async function openBrowserMic(deviceId: string | null, onBlock: OnBlock): Promise<MicInput> {
  const { stream, fellBack } = await getStream(deviceId);
  const track = stream.getAudioTracks()[0];
  if (!track) {
    stream.getTracks().forEach((t) => t.stop());
    throw new MicError("notFound");
  }
  let ctx: AudioContext;
  let source: MediaStreamAudioSourceNode;
  try {
    ({ ctx, source } = contextFor(stream));
  } catch (e) {
    track.stop();
    throw e instanceof MicError ? e : micError(e);
  }
  try {
    await ctx.audioWorklet.addModule("/mic-tap.js");
  } catch (e) {
    track.stop();
    void ctx.close();
    throw new MicError("worklet", e);
  }
  const tap = new AudioWorkletNode(ctx, "openbark-mic-tap", { numberOfInputs: 1, numberOfOutputs: 1 });
  const rate = ctx.sampleRate;
  tap.port.onmessage = (e: MessageEvent<{ samples: Float32Array<ArrayBuffer>; peak: number }>) =>
    onBlock(e.data.samples, e.data.peak, rate);
  // WebKit only runs nodes that lead to the speakers: route the tap there,
  // silenced.
  const mute = ctx.createGain();
  mute.gain.value = 0;
  source.connect(tap);
  tap.connect(mute);
  mute.connect(ctx.destination);
  if (ctx.state !== "running") await within(ctx.resume(), 1000);
  const settings = track.getSettings();
  return {
    track,
    ctx,
    sampleRate: rate,
    label: track.label,
    deviceId: settings.deviceId ?? null,
    groupId: settings.groupId ?? null,
    fellBack,
    close() {
      tap.port.onmessage = null;
      track.stop();
      void ctx.close();
    },
  };
}

/** The recorder's level scale: -60..0 dBFS to 0..1. */
export function levelOf(samples: Float32Array): number {
  let sum = 0;
  for (const v of samples) sum += v * v;
  const rms = Math.sqrt(sum / Math.max(1, samples.length));
  const db = 20 * Math.log10(Math.max(1e-6, rms));
  return Math.min(1, Math.max(0, (db + 60) / 60));
}

export interface BrowserRecordingOptions {
  /** The microphone chosen on this device; null follows the system default. */
  deviceId: string | null;
  /** Open the engine's socket for a recording at this rate. */
  connect(sampleRate: number): WebSocket;
  onLevel(level: number, seconds: number): void;
  onHealth(health: MicHealth): void;
  /** The recording ended on its own (the engine went away). */
  onEnded(message: string): void;
}

export interface BrowserRecording {
  stop(): Promise<unknown>;
  /** The microphone opened at the start. */
  mic: { label: string; sampleRate: number; fellBack: boolean };
}

/** Start a recording from the browser's microphone. */
export async function recordInBrowser(o: BrowserRecordingOptions): Promise<BrowserRecording> {
  let ws: WebSocket | null = null;
  let sentRate = 0;
  let seconds = 0;
  let stopping = false;
  const now = () => performance.now();
  // Set once the microphone is open; blocks can arrive before that.
  let dog: MicWatchdog | null = null;

  const onBlock: OnBlock = (samples, peak, rate) => {
    if (stopping) return;
    if (ws && ws.readyState === WebSocket.OPEN) {
      if (rate !== sentRate) {
        // A microphone at another rate took over: say so before its first block.
        ws.send(JSON.stringify({ sample_rate: rate }));
        sentRate = rate;
      }
      ws.send(samples);
    }
    seconds += samples.length / rate;
    dog?.block(peak, now());
    o.onLevel(levelOf(samples), seconds);
  };

  let input: MicInput | null = await openBrowserMic(o.deviceId, onBlock);
  const watchdog = new MicWatchdog(input.label, input.sampleRate, now());
  dog = watchdog;
  let lastLabel: string | null = input.label;
  const watchdogFellBack = input.fellBack;
  sentRate = input.sampleRate;
  const socket = o.connect(input.sampleRate);
  ws = socket;
  socket.binaryType = "arraybuffer";
  const opened = new Promise<void>((resolve, reject) => {
    socket.onmessage = (e) => {
      const msg = JSON.parse(String(e.data)) as { started?: string; error?: string };
      if (msg.error) reject(new Error(msg.error));
      else resolve();
    };
    socket.onerror = () => reject(new MicError("engine"));
    socket.onclose = () => reject(new MicError("engine"));
  });
  try {
    await opened;
  } catch (e) {
    input.close();
    socket.close();
    throw e;
  }

  // ---- keeping it going ----------------------------------------------------
  let recovering = false;
  let retry: ReturnType<typeof setTimeout> | null = null;
  let stalledSince: number | null = null;

  const adopt = (next: MicInput, previous: string | null) => {
    input = next;
    next.track.addEventListener("ended", onLost);
    watchdog.opened(next.label, next.sampleRate, previous, now());
    stalledSince = null;
  };

  const recover = async () => {
    if (stopping || recovering || input) return;
    recovering = true;
    const previous = lastLabel;
    try {
      const next = await openBrowserMic(o.deviceId, onBlock);
      if (stopping) next.close();
      else adopt(next, previous);
    } catch {
      // Nothing to record from yet: try again shortly, and on the next device change.
      if (!stopping) retry = setTimeout(() => void recover(), 2000);
    } finally {
      recovering = false;
    }
  };

  function onLost() {
    if (stopping || !input) return;
    lastLabel = input.label;
    input.track.removeEventListener("ended", onLost);
    input.close();
    input = null;
    watchdog.lost();
    report();
    void recover();
  }
  input.track.addEventListener("ended", onLost);

  /** Switch to another microphone, opening it before letting the current one go. */
  const switchTo = async (deviceId: string | null) => {
    if (stopping || recovering || !input) return;
    recovering = true;
    try {
      const next = await openBrowserMic(deviceId, onBlock);
      const old = input;
      if (stopping || !old) {
        next.close();
        return;
      }
      old.track.removeEventListener("ended", onLost);
      lastLabel = old.label;
      adopt(next, old.label);
      old.close();
    } catch {
      // Keep recording on the current microphone.
    } finally {
      recovering = false;
    }
  };

  const onDeviceChange = async () => {
    if (stopping) return;
    if (!input) {
      void recover();
      return;
    }
    let devices: MediaDeviceInfo[] = [];
    try {
      devices = await navigator.mediaDevices.enumerateDevices();
    } catch {
      return;
    }
    if (o.deviceId === null) {
      // Following the system default (Chromium lists it as "default").
      const def = devices.find((d) => d.kind === "audioinput" && d.deviceId === "default");
      if (def && input.groupId && def.groupId && def.groupId !== input.groupId) void switchTo(null);
    } else if (input.deviceId !== o.deviceId && devices.some((d) => d.deviceId === o.deviceId)) {
      // The chosen microphone is back.
      void switchTo(o.deviceId);
    }
  };
  navigator.mediaDevices?.addEventListener?.("devicechange", onDeviceChange);

  const resume = () => {
    if (input && input.ctx.state !== "running") void input.ctx.resume();
  };
  const onVisible = () => {
    if (document.visibilityState === "visible") resume();
  };
  document.addEventListener("visibilitychange", onVisible);
  // Safari may keep the context suspended until the next gesture.
  document.addEventListener("pointerdown", resume);
  document.addEventListener("keydown", resume);

  function report() {
    const h = watchdog.tick(now(), seconds);
    if (h) o.onHealth(h);
  }
  const timer = setInterval(() => {
    const t = now();
    const state = watchdog.state(t);
    if (state === "stalled") {
      stalledSince ??= t;
      resume();
      // Still nothing after a while with a live track: the microphone died
      // without saying so. Reopen it.
      if (input && t - stalledSince > 6000) onLost();
    } else {
      stalledSince = null;
    }
    report();
  }, 500);

  const cleanup = () => {
    stopping = true;
    clearInterval(timer);
    if (retry) clearTimeout(retry);
    navigator.mediaDevices?.removeEventListener?.("devicechange", onDeviceChange);
    document.removeEventListener("visibilitychange", onVisible);
    document.removeEventListener("pointerdown", resume);
    document.removeEventListener("keydown", resume);
    if (input) {
      input.track.removeEventListener("ended", onLost);
      input.close();
      input = null;
    }
  };

  socket.onclose = () => {
    if (stopping) return;
    cleanup();
    o.onEnded(MIC_ERRORS.connection);
  };
  socket.onerror = null;

  return {
    mic: { label: lastLabel ?? "", sampleRate: sentRate, fellBack: watchdogFellBack },
    stop: () =>
      new Promise((resolve, reject) => {
        cleanup();
        if (socket.readyState !== WebSocket.OPEN) {
          reject(new MicError("connection"));
          return;
        }
        socket.onclose = () => reject(new MicError("connection"));
        socket.onmessage = (e) => {
          const msg = JSON.parse(String(e.data)) as { session?: unknown; error?: string };
          socket.onclose = null;
          if (msg.session) resolve(msg.session);
          else reject(new Error(msg.error ?? "The recording couldn't be saved."));
        };
        socket.send("stop");
      }),
  };
}

export interface MicTest {
  label: string;
  sampleRate: number;
  fellBack: boolean;
  stop(): void;
}

/** A quick microphone test: the level of the chosen (or default) microphone, until stopped. */
export async function testBrowserMic(
  deviceId: string | null,
  onLevel: (level: number, silent: boolean) => void,
): Promise<MicTest> {
  let silentSince: number | null = null;
  const input = await openBrowserMic(deviceId, (samples, peak) => {
    const t = performance.now();
    if (peak < 1e-4) silentSince ??= t;
    else silentSince = null;
    onLevel(levelOf(samples), silentSince !== null && t - silentSince > 2000);
  });
  return {
    label: input.label,
    sampleRate: input.sampleRate,
    fellBack: input.fellBack,
    stop: () => input.close(),
  };
}
