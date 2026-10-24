// The phone window's call, step by step (ADR-0023 sections 5 and 7). Pure, so
// the order of the consent steps is tested without a browser or Twilio.
//
//   idle → calling → ringing → connected → ended
//
// When the client answers a recorded call: the caller reads the announcement
// ("read"), then says whether the client agreed ("ask"): yes keeps recording,
// no deletes what was recorded and the call goes on unrecorded. Recording
// without the announcement isn't possible (ADR-0023 decision 1).

export type Phase = "idle" | "calling" | "ringing" | "connected" | "ended";
export type ConsentStep = "none" | "read" | "ask" | "agreed" | "declined";

export interface CallState {
  phase: Phase;
  number: string;
  /** Chosen before the call: record it (unless "Don't record this call"). */
  record: boolean;
  /** The recording is running now. */
  recording: boolean;
  consent: ConsentStep;
  /** When the client answered (ms), for the timer and the visit's length. */
  connectedAt: number | null;
  /** Seconds from answer to the announcement, for the visit. */
  announcedAt: number | null;
  muted: boolean;
  held: boolean;
  /** The call's length once it ended. */
  endedSecs: number | null;
  /** The recorded call was saved as a visit. */
  saved: boolean;
  /** A problem or an outcome to show (an English key for t()). */
  message: string | null;
  /** The clinic hung up (rather than the client or the network). */
  hungUp: boolean;
  /** The connection dropped and was trying to come back. */
  reconnecting: boolean;
}

export type CallEvent =
  | { type: "dial"; number: string; record: boolean }
  | { type: "ringing" }
  | { type: "accepted"; at: number }
  | { type: "recording" }
  | { type: "read" }
  | { type: "agreed" }
  | { type: "declined" }
  | { type: "unrecorded" }
  | { type: "deleteFailed"; message: string }
  | { type: "mute"; on: boolean }
  | { type: "hold"; on: boolean }
  | { type: "reconnecting"; on: boolean }
  | { type: "hangup" }
  | { type: "disconnected"; at: number }
  | { type: "saved" }
  | { type: "failed"; message: string }
  | { type: "reset" };

export const NOT_RECORDED_DECLINED = "Not recorded. The client declined.";
export const NO_ANSWER = "No answer.";
export const DROPPED = "The call dropped. What was recorded is saved.";

export const initialCall: CallState = {
  phase: "idle",
  number: "",
  record: true,
  recording: false,
  consent: "none",
  connectedAt: null,
  announcedAt: null,
  muted: false,
  held: false,
  endedSecs: null,
  saved: false,
  message: null,
  hungUp: false,
  reconnecting: false,
};

export function callReducer(s: CallState, e: CallEvent): CallState {
  switch (e.type) {
    case "dial":
      return { ...initialCall, phase: "calling", number: e.number, record: e.record };
    case "ringing":
      return s.phase === "calling" ? { ...s, phase: "ringing" } : s;
    case "accepted":
      if (s.phase !== "calling" && s.phase !== "ringing") return s;
      // A recorded call shows the announcement first.
      return { ...s, phase: "connected", connectedAt: e.at, consent: s.record ? "read" : "none" };
    case "recording":
      return s.phase === "connected" && s.record && s.consent !== "declined" ? { ...s, recording: true } : s;
    case "read":
      if (s.consent !== "read") return s;
      // The announcement is on screen from the answer, when recording starts: 0 s in.
      return { ...s, consent: "ask", announcedAt: 0 };
    case "agreed":
      return s.consent === "ask" ? { ...s, consent: "agreed" } : s;
    case "declined":
      // "No, stop recording", or "Stop and delete the recording" at any time.
      if (!s.record || s.consent === "declined" || s.phase !== "connected") return s;
      return { ...s, consent: "declined", recording: false, message: NOT_RECORDED_DECLINED };
    case "unrecorded":
      // The recording couldn't start: the call goes on, unrecorded, with no consent step.
      return { ...s, record: false, recording: false, consent: "none" };
    case "deleteFailed":
      // The engine couldn't delete what was recorded: say so, never "Not recorded".
      return { ...s, message: e.message };
    case "mute":
      return { ...s, muted: e.on };
    case "hold":
      return { ...s, held: e.on };
    case "reconnecting":
      return { ...s, reconnecting: e.on || s.reconnecting };
    case "hangup":
      return { ...s, hungUp: true };
    case "disconnected": {
      if (s.phase === "ended" || s.phase === "idle") return s;
      const secs = s.connectedAt === null ? 0 : Math.max(0, (e.at - s.connectedAt) / 1000);
      let message = s.message;
      if (s.connectedAt === null && !s.hungUp && !message) message = NO_ANSWER;
      else if (s.connectedAt !== null && !s.hungUp && s.reconnecting) message = s.recording ? DROPPED : null;
      return { ...s, phase: "ended", endedSecs: secs, recording: false, message, muted: false, held: false };
    }
    case "saved":
      return { ...s, saved: true };
    case "failed":
      return { ...s, message: e.message, phase: s.phase === "connected" ? s.phase : "ended", endedSecs: s.endedSecs ?? 0 };
    case "reset":
      return initialCall;
  }
}

/** The status line under the number: what's happening now. */
export function statusLine(
  s: CallState,
  now: number,
  t: (text: string, vars?: Record<string, string | number>) => string,
  fmt: (secs: number) => string,
): string {
  const secs = s.connectedAt === null ? 0 : (now - s.connectedAt) / 1000;
  switch (s.phase) {
    case "calling":
      return t("Calling…");
    case "ringing":
      return t("Ringing…");
    case "connected":
      return s.recording
        ? t("Recording · {time}", { time: fmt(secs) })
        : t("On call · {time} · Not recorded", { time: fmt(secs) });
    default:
      return "";
  }
}

/** What the window says once the call is over. */
export function endLine(
  s: CallState,
  draftOnStop: boolean,
  t: (text: string, vars?: Record<string, string | number>) => string,
  fmt: (secs: number) => string,
): string {
  const time = fmt(s.endedSecs ?? 0);
  if (s.connectedAt === null) return "";
  if (s.saved && s.consent !== "declined") {
    return draftOnStop
      ? t("Call ended · {time}. Writing note…", { time })
      : t("Call ended · {time}. Saved as a visit.", { time });
  }
  return t("Call ended · {time}. Not recorded.", { time });
}
