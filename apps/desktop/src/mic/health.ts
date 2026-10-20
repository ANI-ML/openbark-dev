// The microphone's health during a recording, and what to tell the clinic
// about it. The engine sends the same shape (os-engine mic_health.rs) for the
// desktop's own microphone; in web mode the browser works it out itself
// (watchdog.ts) and the engine adds what it sees of the stream.

export type MicState = "ok" | "silent" | "stalled" | "lost";
export type MicQuality = "good" | "hands_free" | "narrowband";

export interface MicHealth {
  state: MicState;
  /** The microphone's name; empty when not known. */
  device: string;
  sample_rate: number;
  quality: MicQuality;
  /** After a switch: the microphone recording moved away from. */
  switched_from: string | null;
  /** Seconds of audio recorded so far. */
  seconds: number;
}

/** Bluetooth headsets in call mode, as Windows, macOS and Linux name them. */
const HANDS_FREE = ["hands-free", "handsfree", "headset", "airpods", "bluetooth", "beats", "buds", "hfp"];

/** Same rules as the engine: below 16 kHz is telephone quality; a headset at 24 kHz or less is in hands-free mode. */
export function micQuality(device: string, sampleRate: number): MicQuality {
  if (sampleRate > 0 && sampleRate < 16000) return "narrowband";
  const name = device.toLowerCase();
  if (sampleRate > 0 && sampleRate <= 24000 && HANDS_FREE.some((n) => name.includes(n))) return "hands_free";
  return "good";
}

type Translate = (text: string, vars?: Record<string, string | number>) => string;

export interface MicNotice {
  /** "danger": nothing useful is being recorded; "warn": recording, but check. */
  tone: "danger" | "warn";
  text: string;
}

/** What to tell the clinic, in the app language, or null when all is well. */
export function micNotice(t: Translate, h: MicHealth | null, web: boolean): MicNotice | null {
  if (!h) return null;
  const device = h.device || t("the microphone");
  switch (h.state) {
    case "lost":
      return {
        tone: "danger",
        text: t(
          "The microphone was disconnected, so recording is paused. Plug it back in or connect another one: recording carries on by itself. Or press Stop to keep what was recorded.",
        ),
      };
    case "stalled":
      return {
        tone: "danger",
        text: web
          ? t(
              "No sound is reaching OpenBark. Keep this tab open and in front while recording, and check that the microphone is still connected.",
            )
          : t(
              "No sound is reaching OpenBark from {device}. OpenBark is reopening it. If this doesn't clear in a few seconds, check the microphone is connected, or choose another one in Settings.",
              { device },
            ),
      };
    case "silent":
      return {
        tone: "danger",
        text: web
          ? t(
              "{device} is sending silence. Check it isn't muted (a mute switch or key), and that this browser is allowed to use it in the system's privacy settings.",
              { device },
            )
          : t(
              "{device} is sending silence. Check it isn't muted (a mute switch or key), and that OpenBark is allowed to use the microphone: on a Mac, System Settings, Privacy & Security, Microphone; on Windows, Settings, Privacy & security, Microphone.",
              { device },
            ),
      };
    case "ok":
      break;
  }
  if (h.quality === "narrowband") {
    return {
      tone: "warn",
      text: t(
        "{device} is sending telephone-quality audio ({rate} kHz), so some words may be missed. A Bluetooth headset does this while it's in call mode: for a consult, use the computer's own microphone or a wired one.",
        { device, rate: Math.round(h.sample_rate / 100) / 10 },
      ),
    };
  }
  if (h.quality === "hands_free") {
    return {
      tone: "warn",
      text: t(
        "{device} is in its hands-free mode ({rate} kHz), which is less clear than the computer's own microphone. Recording carries on.",
        { device, rate: Math.round(h.sample_rate / 100) / 10 },
      ),
    };
  }
  return null;
}

/** "Now recording from AirPods Pro." after a switch, for a toast. */
export function switchNote(t: Translate, prev: MicHealth | null, next: MicHealth): string | null {
  if (!next.switched_from || next.state !== "ok" || !next.device) return null;
  if (prev && prev.device === next.device && prev.state === "ok") return null;
  return t("Now recording from {device}.", { device: next.device });
}
