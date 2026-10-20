// Web mode's microphone watchdog: the browser's version of the engine's
// HealthTracker (os-engine mic_health.rs), with the same thresholds.
// - silent: blocks arrive, but at digital silence for 4 s (muted, or blocked
//   by the system's privacy settings, which hand the browser zeros);
// - stalled: no blocks for 3 s (the audio context was suspended, or the tab
//   was frozen in the background);
// - lost: the microphone track ended (unplugged, Bluetooth dropped).
import { micQuality, type MicHealth, type MicState } from "./health";

/** Peak below this (about -80 dBFS) is digital silence; a quiet room is louder. */
export const SILENT_PEAK = 1e-4;

export interface WatchdogTiming {
  silentAfterMs: number;
  stalledAfterMs: number;
}

export const DEFAULT_TIMING: WatchdogTiming = { silentAfterMs: 4000, stalledAfterMs: 3000 };

export class MicWatchdog {
  private device: string;
  private rate: number;
  private switchedFrom: string | null = null;
  private lostFlag = false;
  private lastBlock: number;
  private silentSince: number | null = null;
  private lastSent: MicHealth | null = null;

  constructor(
    device: string,
    rate: number,
    now: number,
    private timing: WatchdogTiming = DEFAULT_TIMING,
  ) {
    this.device = device;
    this.rate = rate;
    this.lastBlock = now;
  }

  /** Recording from a (possibly different) microphone now. */
  opened(device: string, rate: number, replaced: string | null, now: number) {
    if (replaced && replaced !== device) this.switchedFrom = replaced;
    this.device = device;
    this.rate = rate;
    this.lostFlag = false;
    this.lastBlock = now;
    this.silentSince = null;
  }

  lost() {
    this.lostFlag = true;
  }

  /** A block arrived with this peak level. */
  block(peak: number, now: number) {
    if (now - this.lastBlock >= this.timing.stalledAfterMs) this.silentSince = null;
    this.lastBlock = now;
    if (peak < SILENT_PEAK) this.silentSince ??= now;
    else this.silentSince = null;
  }

  state(now: number): MicState {
    if (this.lostFlag) return "lost";
    if (now - this.lastBlock >= this.timing.stalledAfterMs) return "stalled";
    if (this.silentSince !== null && now - this.silentSince >= this.timing.silentAfterMs) return "silent";
    return "ok";
  }

  /** The health now, if it differs from the last returned (ignoring the seconds). */
  tick(now: number, seconds: number): MicHealth | null {
    const h: MicHealth = {
      state: this.state(now),
      device: this.device,
      sample_rate: this.rate,
      quality: micQuality(this.device, this.rate),
      switched_from: this.switchedFrom,
      seconds,
    };
    const prev = this.lastSent;
    if (
      prev &&
      prev.state === h.state &&
      prev.device === h.device &&
      prev.sample_rate === h.sample_rate &&
      prev.quality === h.quality &&
      prev.switched_from === h.switched_from
    ) {
      return null;
    }
    this.lastSent = h;
    return h;
  }
}
