// Web-mode capture against a fake browser: getUserMedia, AudioContext, the
// worklet and the engine's WebSocket. Covers the failure modes real
// browsers showed or are known for: the chosen microphone gone, Firefox's
// sample-rate mismatch, a worklet that can't load (the page CSP), and a
// microphone unplugged mid-recording.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { MIC_ERRORS } from "./errors";
import type { MicHealth } from "./health";
import { openBrowserMic, recordInBrowser } from "./browser";

class FakeTrack extends EventTarget {
  stopped = false;
  constructor(
    public label: string,
    private settings: { deviceId: string; groupId: string; sampleRate: number },
  ) {
    super();
  }
  getSettings() {
    // Firefox doesn't report a microphone's sample rate.
    return hideRate ? { ...this.settings, sampleRate: undefined } : this.settings;
  }
  /** The rate it really runs at, whether or not it's reported. */
  get realRate() {
    return this.settings.sampleRate;
  }
  stop() {
    this.stopped = true;
  }
  /** The device went away. */
  end() {
    this.dispatchEvent(new Event("ended"));
  }
}

interface Device {
  deviceId: string;
  groupId: string;
  label: string;
  rate: number;
}

let devices: Device[] = [];
let defaultId = "";
let tracks: FakeTrack[] = [];
let taps: FakeTap[] = [];
let contexts: FakeContext[] = [];
let workletFails = false;
let hideRate = false;
/** Rates a context may be made at (an unusual device: none fit). */
let contextRates: number[] | null = null;
/** Firefox: a stream can't feed a context at another rate. */
let strictRates = false;
const deviceListeners = new Set<() => void>();

function gum(constraints: { audio: { deviceId?: { exact: string } } }) {
  const want = constraints.audio.deviceId?.exact;
  const d = want ? devices.find((x) => x.deviceId === want) : devices.find((x) => x.deviceId === defaultId);
  if (!d) {
    const name = want ? "OverconstrainedError" : "NotFoundError";
    return Promise.reject(Object.assign(new Error(name), { name }));
  }
  const track = new FakeTrack(d.label, { deviceId: d.deviceId, groupId: d.groupId, sampleRate: d.rate });
  tracks.push(track);
  return Promise.resolve({ getAudioTracks: () => [track], getTracks: () => [track] });
}

class FakeTap {
  port: { onmessage: ((e: { data: unknown }) => void) | null } = { onmessage: null };
  constructor(public ctx: FakeContext) {
    taps.push(this);
  }
  connect() {}
  /** The worklet posts a block. */
  post(samples: number[], peak = 0.2) {
    this.port.onmessage?.({ data: { samples: new Float32Array(samples), peak } });
  }
}

class FakeContext {
  sampleRate: number;
  state = "running";
  closed = false;
  destination = {};
  audioWorklet = {
    addModule: (url: string) => {
      expect(url).toBe("/mic-tap.js");
      return workletFails ? Promise.reject(new Error("blocked by CSP")) : Promise.resolve();
    },
  };
  constructor(opts?: { sampleRate?: number }) {
    this.sampleRate = opts?.sampleRate ?? 48000;
    contexts.push(this);
  }
  createMediaStreamSource(stream: { getAudioTracks(): FakeTrack[] }) {
    const rate = stream.getAudioTracks()[0]!.realRate;
    if ((strictRates && rate !== this.sampleRate) || (contextRates && !contextRates.includes(this.sampleRate))) {
      throw Object.assign(new Error("Connecting AudioNodes from AudioContexts with different sample-rate"), {
        name: "NotSupportedError",
      });
    }
    return { connect() {} };
  }
  createGain() {
    return { gain: { value: 1 }, connect() {} };
  }
  resume() {
    this.state = "running";
    return Promise.resolve();
  }
  close() {
    this.closed = true;
    return Promise.resolve();
  }
}

class FakeSocket {
  static OPEN = 1;
  readyState = 1;
  binaryType = "";
  sent: unknown[] = [];
  onmessage: ((e: { data: string }) => void) | null = null;
  onerror: (() => void) | null = null;
  onclose: (() => void) | null = null;
  constructor(public rate: number) {
    // The engine says it started, a moment later.
    setTimeout(() => this.onmessage?.({ data: JSON.stringify({ started: "visit-1" }) }), 0);
  }
  send(x: unknown) {
    this.sent.push(x);
    if (x === "stop") setTimeout(() => this.onmessage?.({ data: JSON.stringify({ session: { id: "visit-1" } }) }), 0);
  }
  close() {
    this.readyState = 3;
  }
}

beforeEach(() => {
  devices = [
    { deviceId: "builtin", groupId: "g-builtin", label: "MacBook Pro Microphone", rate: 48000 },
    { deviceId: "usb", groupId: "g-usb", label: "USB mic", rate: 44100 },
  ];
  defaultId = "builtin";
  tracks = [];
  taps = [];
  contexts = [];
  workletFails = false;
  strictRates = false;
  hideRate = false;
  contextRates = null;
  deviceListeners.clear();
  vi.stubGlobal("AudioContext", FakeContext);
  vi.stubGlobal(
    "AudioWorkletNode",
    class {
      constructor(ctx: FakeContext) {
        return new FakeTap(ctx);
      }
    },
  );
  vi.stubGlobal("WebSocket", FakeSocket);
  Object.defineProperty(window, "isSecureContext", { value: true, configurable: true });
  Object.defineProperty(navigator, "mediaDevices", {
    configurable: true,
    value: {
      getUserMedia: gum,
      enumerateDevices: () =>
        Promise.resolve([
          ...devices.map((d) => ({ kind: "audioinput", deviceId: d.deviceId, groupId: d.groupId, label: d.label })),
          {
            kind: "audioinput",
            deviceId: "default",
            groupId: devices.find((d) => d.deviceId === defaultId)?.groupId ?? "",
            label: "Default",
          },
        ]),
      addEventListener: (_: string, f: () => void) => deviceListeners.add(f),
      removeEventListener: (_: string, f: () => void) => deviceListeners.delete(f),
    },
  });
});

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

const noop = () => {};

describe("openBrowserMic", () => {
  it("opens the chosen microphone at its own rate", async () => {
    const mic = await openBrowserMic("usb", noop);
    expect(mic.label).toBe("USB mic");
    expect(mic.fellBack).toBe(false);
    // Chrome resamples into a 48 kHz context; the engine gets 48 kHz.
    expect(mic.sampleRate).toBe(48000);
  });

  it("falls back to the default when the chosen microphone isn't connected", async () => {
    const mic = await openBrowserMic("gone", noop);
    expect(mic.label).toBe("MacBook Pro Microphone");
    expect(mic.fellBack).toBe(true);
  });

  it("makes the context at the microphone's rate where the browser insists (Firefox)", async () => {
    strictRates = true;
    const mic = await openBrowserMic("usb", noop);
    expect(mic.sampleRate).toBe(44100);
    expect(contexts[0]!.closed).toBe(true);
  });

  it("finds the microphone's rate when the browser insists on it but won't say it (Firefox)", async () => {
    strictRates = true;
    hideRate = true;
    const mic = await openBrowserMic("usb", noop);
    expect(mic.sampleRate).toBe(44100);
    // Tried the default (48 kHz), then each rate down to the one that fits.
    expect(contexts.map((c) => c.sampleRate)).toEqual([48000, 48000, 44100]);
    expect(contexts.filter((c) => !c.closed)).toHaveLength(1);
  });

  it("a 16 kHz headset in Firefox is found too", async () => {
    strictRates = true;
    hideRate = true;
    devices.push({ deviceId: "headset", groupId: "g-h", label: "Headset (AirPods Hands-Free)", rate: 16000 });
    const mic = await openBrowserMic("headset", noop);
    expect(mic.sampleRate).toBe(16000);
  });

  it("explains when no rate works, rather than calling the browser unsupported", async () => {
    contextRates = [];
    await expect(openBrowserMic(null, noop)).rejects.toThrow(MIC_ERRORS.rate);
    expect(contexts.every((c) => c.closed)).toBe(true);
  });

  it("explains a worklet that can't load, and lets the microphone go", async () => {
    workletFails = true;
    await expect(openBrowserMic(null, noop)).rejects.toThrow(MIC_ERRORS.worklet);
    expect(tracks[0]!.stopped).toBe(true);
  });

  it("explains no microphone at all", async () => {
    devices = [];
    await expect(openBrowserMic(null, noop)).rejects.toThrow(MIC_ERRORS.notFound);
  });

  it("explains an insecure page (opened by its network address)", async () => {
    Object.defineProperty(window, "isSecureContext", { value: false, configurable: true });
    await expect(openBrowserMic(null, noop)).rejects.toThrow(MIC_ERRORS.insecure);
  });
});

describe("recordInBrowser", () => {
  async function start(deviceId: string | null = null) {
    const health: MicHealth[] = [];
    const levels: number[] = [];
    const sockets: FakeSocket[] = [];
    const ended: string[] = [];
    const rec = await recordInBrowser({
      deviceId,
      connect: (rate) => {
        const s = new FakeSocket(rate);
        sockets.push(s);
        return s as unknown as WebSocket;
      },
      onLevel: (l) => levels.push(l),
      onHealth: (h) => health.push(h),
      onEnded: (m) => ended.push(m),
    });
    return { rec, health, levels, socket: sockets[0]!, ended };
  }

  it("streams blocks and stops with the saved visit", async () => {
    const { rec, socket, levels } = await start();
    expect(socket.rate).toBe(48000);
    taps[0]!.post([0.1, -0.1, 0.2]);
    expect(socket.sent).toHaveLength(1);
    expect(levels).toHaveLength(1);
    await expect(rec.stop()).resolves.toEqual({ id: "visit-1" });
    expect(socket.sent.at(-1)).toBe("stop");
    expect(tracks[0]!.stopped).toBe(true);
  });

  it("carries on with the new default when the microphone is unplugged, telling the engine the new rate", async () => {
    strictRates = true; // as Firefox: the new microphone's rate reaches the engine
    devices.push({ deviceId: "airpods", groupId: "g-air", label: "AirPods Pro", rate: 16000 });
    const { rec, socket, health } = await start();
    taps[0]!.post([0.1]);
    // AirPods take over as the default, then the built-in microphone goes.
    defaultId = "airpods";
    devices = devices.filter((d) => d.deviceId !== "builtin");
    tracks[0]!.end();
    await vi.waitFor(() => expect(taps).toHaveLength(2));
    taps[1]!.post([0.3, 0.3]);
    const texts = socket.sent.filter((x) => typeof x === "string");
    expect(texts).toEqual([JSON.stringify({ sample_rate: 16000 })]);
    // The rate change goes before the new microphone's first block.
    const i = socket.sent.indexOf(texts[0]);
    expect(socket.sent[i + 1]).toBeInstanceOf(Float32Array);
    expect(health.map((h) => h.state)).toContain("lost");
    await vi.waitFor(() =>
      expect(health.at(-1)).toMatchObject({ state: "ok", device: "AirPods Pro", switched_from: "MacBook Pro Microphone" }),
    );
    expect(health.at(-1)!.quality).toBe("hands_free");
    await rec.stop();
  });

  it("pauses while there is no microphone and resumes when one is plugged in", async () => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "setInterval", "clearInterval"] });
    const started = start();
    await vi.advanceTimersByTimeAsync(1);
    const { rec, socket, health } = await started;
    devices = [];
    tracks[0]!.end();
    await vi.advanceTimersByTimeAsync(600);
    expect(health.at(-1)!.state).toBe("lost");
    const sentWhileLost = socket.sent.length;
    // Plugged back in: found on the next retry (every 2 s).
    devices = [{ deviceId: "usb", groupId: "g-usb", label: "USB mic", rate: 44100 }];
    defaultId = "usb";
    await vi.advanceTimersByTimeAsync(2600);
    expect(taps).toHaveLength(2);
    taps[1]!.post([0.2]);
    expect(socket.sent.length).toBe(sentWhileLost + 1);
    expect(health.at(-1)).toMatchObject({ state: "ok", device: "USB mic" });
    const stopped = rec.stop();
    await vi.advanceTimersByTimeAsync(1);
    await stopped;
  });

  it("follows a new system default (Chromium's \"default\" device)", async () => {
    devices.push({ deviceId: "headset", groupId: "g-headset", label: "Headset Microphone (USB)", rate: 48000 });
    const { rec, health } = await start(null);
    defaultId = "headset";
    for (const f of deviceListeners) f();
    await vi.waitFor(() => expect(taps).toHaveLength(2));
    expect(tracks[0]!.stopped).toBe(true);
    taps[1]!.post([0.2]);
    await vi.waitFor(() => expect(health.at(-1)).toMatchObject({ device: "Headset Microphone (USB)" }));
    await rec.stop();
  });

  it("reports the engine going away mid-recording", async () => {
    const { socket, ended } = await start();
    socket.onclose?.();
    expect(ended).toEqual([MIC_ERRORS.connection]);
    expect(tracks[0]!.stopped).toBe(true);
  });

  it("shows the engine's own start error and lets the microphone go", async () => {
    class Refusing {
      readyState = 1;
      binaryType = "";
      closed = false;
      onmessage: ((e: { data: string }) => void) | null = null;
      onerror: (() => void) | null = null;
      onclose: (() => void) | null = null;
      constructor() {
        setTimeout(() => this.onmessage?.({ data: JSON.stringify({ error: "A recording is already running. Stop it first." }) }), 0);
      }
      send() {}
      close() {
        this.closed = true;
      }
    }
    await expect(
      recordInBrowser({
        deviceId: null,
        connect: () => new Refusing() as unknown as WebSocket,
        onLevel: noop,
        onHealth: noop,
        onEnded: noop,
      }),
    ).rejects.toThrow("A recording is already running");
    expect(tracks[0]!.stopped).toBe(true);
  });
});
