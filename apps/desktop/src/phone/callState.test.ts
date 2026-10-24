import { describe, expect, it } from "vitest";
import { DROPPED, NOT_RECORDED_DECLINED, NO_ANSWER, callReducer, endLine, initialCall, statusLine, type CallEvent } from "./callState";
import { callLength, callTimer } from "./format";

const run = (...events: CallEvent[]) => events.reduce(callReducer, initialCall);
const t = (text: string, vars?: Record<string, string | number>) =>
  text.replace(/\{(\w+)\}/g, (_, k: string) => String(vars?.[k] ?? ""));

describe("a recorded call", () => {
  it("shows the announcement first, then asks whether the client agreed", () => {
    let s = run({ type: "dial", number: "+15555550142", record: true }, { type: "ringing" });
    expect(s.phase).toBe("ringing");
    s = callReducer(s, { type: "accepted", at: 1000 });
    expect(s.consent).toBe("read");
    // "Did the client agree?" can't come before the announcement was read.
    expect(callReducer(s, { type: "agreed" }).consent).toBe("read");
    s = callReducer(callReducer(s, { type: "recording" }), { type: "read" });
    expect(s.consent).toBe("ask");
    expect(s.announcedAt).toBe(0);
    s = callReducer(s, { type: "agreed" });
    expect(s.consent).toBe("agreed");
    expect(statusLine(s, 135_000, t, callTimer)).toBe("Recording · 02:14");
    s = callReducer(callReducer(s, { type: "hangup" }), { type: "disconnected", at: 253_000 });
    expect(s.phase).toBe("ended");
    expect(s.endedSecs).toBe(252);
    s = callReducer(s, { type: "saved" });
    expect(endLine(s, true, t, callLength)).toBe("Call ended · 4:12. Writing note…");
    expect(endLine(s, false, t, callLength)).toBe("Call ended · 4:12. Saved as a visit.");
  });

  it("stops recording when the client says no, and the call goes on", () => {
    let s = run(
      { type: "dial", number: "+15555550142", record: true },
      { type: "accepted", at: 0 },
      { type: "recording" },
      { type: "read" },
      { type: "declined" },
    );
    expect(s.phase).toBe("connected");
    expect(s.recording).toBe(false);
    expect(s.consent).toBe("declined");
    expect(s.message).toBe(NOT_RECORDED_DECLINED);
    expect(statusLine(s, 9_000, t, callTimer)).toBe("On call · 00:09 · Not recorded");
    // Recording can't come back on this call.
    expect(callReducer(s, { type: "recording" }).recording).toBe(false);
    s = callReducer(callReducer(s, { type: "hangup" }), { type: "disconnected", at: 60_000 });
    expect(endLine(callReducer(s, { type: "saved" }), true, t, callLength)).toBe("Call ended · 1:00. Not recorded.");
  });

  it("can be stopped and deleted at any time, even before the announcement is read", () => {
    const s = run({ type: "dial", number: "+1", record: true }, { type: "accepted", at: 0 }, { type: "recording" }, { type: "declined" });
    expect(s.consent).toBe("declined");
    expect(s.recording).toBe(false);
  });
});

describe("a recording that can't start", () => {
  it("lets the call go on unrecorded, with no consent step", () => {
    const s = run({ type: "dial", number: "+1", record: true }, { type: "accepted", at: 0 }, { type: "recording" }, { type: "unrecorded" });
    expect(s.recording).toBe(false);
    expect(s.consent).toBe("none");
    expect(s.message).toBeNull();
  });
});

describe("a call that isn't recorded", () => {
  it("skips the announcement and never records", () => {
    const s = run({ type: "dial", number: "+1", record: false }, { type: "accepted", at: 0 }, { type: "recording" });
    expect(s.consent).toBe("none");
    expect(s.recording).toBe(false);
    expect(callReducer(s, { type: "declined" })).toBe(s);
  });
});

describe("calls that don't go to plan", () => {
  it("says no answer when it ends before the client answers", () => {
    const s = run({ type: "dial", number: "+1", record: true }, { type: "ringing" }, { type: "disconnected", at: 30_000 });
    expect(s.message).toBe(NO_ANSWER);
    expect(endLine(s, true, t, callLength)).toBe("");
  });

  it("says the call dropped and keeps what was recorded", () => {
    const s = run(
      { type: "dial", number: "+1", record: true },
      { type: "accepted", at: 0 },
      { type: "recording" },
      { type: "reconnecting", on: true },
      { type: "disconnected", at: 5_000 },
    );
    expect(s.message).toBe(DROPPED);
  });

  it("keeps an error on screen", () => {
    const s = run({ type: "dial", number: "+1", record: true }, { type: "failed", message: "Can't reach Twilio." });
    expect(s.phase).toBe("ended");
    expect(s.message).toBe("Can't reach Twilio.");
  });
});
