import { describe, expect, it } from "vitest";
import { CALL_ERRORS, callErrorMessage } from "./errors";
import { callLength, callTimer, formatPhone, toE164 } from "./format";

describe("phone numbers", () => {
  it("shows North American numbers in groups and leaves others as stored", () => {
    expect(formatPhone("+15555550142")).toBe("+1 555 555 0142");
    expect(formatPhone("+442079460958")).toBe("+442079460958");
    expect(formatPhone(null)).toBe("");
  });

  it("reads what was typed on the dial pad the way the engine does", () => {
    for (const typed of ["+1 (555) 555-0142", "555-555-0142", "15555550142", " 555.555.0142 "]) {
      expect(toE164(typed), typed).toBe("+15555550142");
    }
    expect(toE164("+44 20 7946 0958")).toBe("+442079460958");
    for (const bad of ["", "555-0142", "+0 555 555 0142", "call me", "+1 555+555"]) {
      expect(toE164(bad), bad).toBeNull();
    }
  });

  it("times calls", () => {
    expect(callTimer(134)).toBe("02:14");
    expect(callTimer(3725)).toBe("1:02:05");
    expect(callLength(252)).toBe("4:12");
  });
});

describe("call errors", () => {
  it("names the cause and where to fix it", () => {
    expect(callErrorMessage({ code: 20101, message: "AccessTokenInvalid" })).toBe(CALL_ERRORS.wrongKey);
    expect(callErrorMessage({ code: 21219 })).toBe(CALL_ERRORS.trial);
    expect(callErrorMessage({ code: 13227 })).toBe(CALL_ERRORS.geo);
    expect(callErrorMessage({ code: 31402 })).toBe(CALL_ERRORS.account);
    expect(callErrorMessage({ code: 53405 })).toBe(CALL_ERRORS.noAudio);
    expect(callErrorMessage({ code: 31005 })).toBe(CALL_ERRORS.offline);
    expect(callErrorMessage({ name: "NotAllowedError", message: "Permission denied" })).toBe(CALL_ERRORS.microphone);
  });

  it("passes the engine's own messages through", () => {
    expect(callErrorMessage(new Error("The phone is off on this computer."))).toBe("The phone is off on this computer.");
    expect(callErrorMessage("plain")).toBe("plain");
  });
});
