// Why a browser microphone couldn't start, in plain words with the next step.
// The texts are English keys: the app translates them where they're shown
// (i18n/messages.ts), so each is a fixed sentence.

export const MIC_ERRORS = {
  insecure:
    "The browser only allows the microphone on a secure page. Open OpenBark at http://localhost on this computer (not by its network address), or use the desktop app.",
  unsupported: "This browser can't record audio. Use a current version of Chrome, Edge, Firefox or Safari.",
  denied:
    "Microphone access was blocked. Allow the microphone for this page (the icon in the address bar), then press Record again. If the browser itself is blocked, allow it in the system's privacy settings: on a Mac, System Settings, Privacy & Security, Microphone; on Windows, Settings, Privacy & security, Microphone.",
  dismissed: "The browser asked to use the microphone and the question was closed. Press Record again and choose Allow.",
  notFound:
    "No microphone was found. Plug one in or switch it on, then press Record again. A Bluetooth headset must be connected first.",
  busy: "The microphone couldn't be started: another app may be using it (a call in Teams or Zoom, for example), or the system blocked it. Close the other app, then press Record again.",
  worklet:
    "OpenBark couldn't start its audio processing in this browser. Reload the page and press Record again; if that doesn't help, use another browser or the desktop app.",
  engine: "Couldn't reach the OpenBark engine. Check that `openbark serve` is running.",
  connection:
    "The connection to the OpenBark engine was lost, so recording stopped. What was recorded until then is saved: find it under Patients.",
} as const;

export class MicError extends Error {
  constructor(
    public kind: keyof typeof MIC_ERRORS,
    public cause?: unknown,
  ) {
    super(MIC_ERRORS[kind]);
    this.name = "MicError";
  }
}

/** The error getUserMedia (or what follows it) threw, as a MicError. */
export function micError(e: unknown): MicError {
  if (e instanceof MicError) return e;
  const name = (e as { name?: string } | null)?.name ?? "";
  const message = String((e as { message?: string } | null)?.message ?? e ?? "");
  switch (name) {
    case "NotAllowedError":
    case "PermissionDeniedError":
      // Chrome: "Permission dismissed" when the prompt was closed without an answer.
      return new MicError(/dismiss/i.test(message) ? "dismissed" : "denied", e);
    case "SecurityError":
      return new MicError("denied", e);
    case "NotFoundError":
    case "DevicesNotFoundError":
    case "OverconstrainedError":
      return new MicError("notFound", e);
    case "NotSupportedError":
      // A browser (or a headless one) that can't ask for the microphone at all.
      return new MicError("unsupported", e);
    case "NotReadableError":
    case "TrackStartError":
    case "AbortError":
      return new MicError("busy", e);
    default:
      return new MicError("busy", e);
  }
}

/** Why the browser can't offer a microphone at all, or null if it can. */
export function micUnavailable(): MicError | null {
  if (typeof window !== "undefined" && window.isSecureContext === false) return new MicError("insecure");
  if (typeof navigator === "undefined" || !navigator.mediaDevices?.getUserMedia) return new MicError("unsupported");
  return null;
}
