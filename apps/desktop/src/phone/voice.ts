// Twilio's Voice SDK in the phone window (ADR-0023 section 4). Bundled from
// npm (@twilio/voice-sdk, pinned in docs/versions.md), never from a CDN, and
// only in this page: the main window's bundle has no Twilio code.
//
// Options OpenBark sets:
// - every sound is a local file (public/phone-sounds), so the SDK never
//   fetches from sdk.twilio.com;
// - Opus first: the SDK's default puts PCMU first, which sounds like a phone
//   line even on the clinic's side;
// - the two AudioProcessors that tap both sides for the local recording.
//
// The end-to-end test swaps the SDK for a fake that plays fixture audio
// (window.__openbarkFakeVoice, set by Playwright before the page loads). That
// hook exists only in builds made with `vite build --mode e2e`; production
// builds don't contain it (scripts/ci/check-phone-bundle.mjs checks).
import type { AudioProcessor } from "./callAudio";

/** The parts of a Twilio `Call` the phone window uses. */
export interface VoiceCall {
  on(event: "ringing" | "accept" | "disconnect" | "cancel" | "reject", cb: () => void): void;
  on(event: "error", cb: (e: { code?: number; message?: string }) => void): void;
  on(event: "reconnecting" | "reconnected", cb: () => void): void;
  on(event: "warning" | "warning-cleared", cb: (name: string) => void): void;
  disconnect(): void;
  mute(on?: boolean): void;
  sendDigits(digits: string): void;
  parameters: Record<string, string>;
}

/** The parts of a Twilio `Device` the phone window uses. */
export interface VoiceDevice {
  on(event: "error", cb: (e: { code?: number; message?: string }) => void): void;
  on(event: "tokenWillExpire", cb: () => void): void;
  updateToken(token: string): void;
  connect(options: { params: Record<string, string> }): Promise<VoiceCall>;
  destroy(): void;
  audio?: { addProcessor(p: AudioProcessor, isRemote?: boolean): Promise<void> } | null;
}

interface VoiceModule {
  Device: new (token: string, options: Record<string, unknown>) => VoiceDevice;
}

const DTMF = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"] as const;

/** Every sound the SDK can play, as local files: it fetches none from Twilio. */
export const SOUNDS: Record<string, string> = {
  incoming: "/phone-sounds/incoming.wav",
  outgoing: "/phone-sounds/outgoing.wav",
  disconnect: "/phone-sounds/disconnect.wav",
  dtmfs: "/phone-sounds/dtmf-star.wav",
  dtmfh: "/phone-sounds/dtmf-hash.wav",
  ...Object.fromEntries(DTMF.map((d) => [`dtmf${d}`, `/phone-sounds/dtmf-${d}.wav`])),
};

/** The Device's options: local sounds, Opus first, a quiet log. */
export const DEVICE_OPTIONS = {
  codecPreferences: ["opus", "pcmu"],
  sounds: SOUNDS,
  // Asks before closing the window during a call.
  closeProtection: true,
  logLevel: "warn",
};

/**
 * The end-to-end test's stand-in for the SDK. Only in builds made with
 * `--mode e2e`: in a production build `import.meta.env.MODE` is
 * "production", so this is always undefined and the bundler drops it.
 */
function fakeVoice(): VoiceModule | undefined {
  if (import.meta.env.MODE !== "e2e") return undefined;
  return (window as unknown as { __openbarkFakeVoice?: VoiceModule }).__openbarkFakeVoice;
}

async function voiceModule(): Promise<VoiceModule> {
  return fakeVoice() ?? ((await import("@twilio/voice-sdk")) as unknown as VoiceModule);
}

/** Whether this browser or WebView can make calls at all (Linux WebKitGTK can't). */
export function canCall(): boolean {
  return typeof RTCPeerConnection !== "undefined" || Boolean(fakeVoice());
}

/**
 * A Device for this computer, with both taps added and the token refreshed
 * when the SDK warns it will expire.
 */
export async function createDevice(
  token: string,
  taps: { local: AudioProcessor; remote: AudioProcessor },
  refresh: () => Promise<string>,
): Promise<VoiceDevice> {
  const { Device } = await voiceModule();
  const device = new Device(token, DEVICE_OPTIONS);
  device.on("tokenWillExpire", () => {
    refresh().then((t) => device.updateToken(t), () => {});
  });
  await device.audio?.addProcessor(taps.local, false);
  await device.audio?.addProcessor(taps.remote, true);
  return device;
}
