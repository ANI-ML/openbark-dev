import { describe, expect, it } from "vitest";
import type * as engine from "../engine";
import { draftErrorText, readingSecondsLeft } from "./DraftProgress";
import { noteModelAdvice } from "./NoteModelAdvice";

const t = (s: string) => s;

describe("reading estimate", () => {
  it("waits for a measured rate before estimating", () => {
    expect(readingSecondsLeft({ prompt: null, promptStartedAt: 0, promptAt: 0 })).toBeNull();
    // One batch after two seconds: too early to say.
    expect(readingSecondsLeft({ prompt: { done: 512, total: 8192 }, promptStartedAt: 0, promptAt: 2000 })).toBeNull();
    // 1,024 of 8,192 in 20 s: 140 s left at that rate.
    expect(readingSecondsLeft({ prompt: { done: 1024, total: 8192 }, promptStartedAt: 0, promptAt: 20_000 })).toBe(140);
    expect(readingSecondsLeft({ prompt: { done: 8192, total: 8192 }, promptStartedAt: 0, promptAt: 60_000 })).toBeNull();
  });
});

describe("draft errors", () => {
  it("passes the engine's plain messages through the translator", () => {
    expect(draftErrorText(t, "Drafting stopped. Nothing was saved.")).toBe("Drafting stopped. Nothing was saved.");
    expect(draftErrorText(t, new Error("model output is not valid JSON"))).toBe("model output is not valid JSON");
  });
});

const row = (id: string, size: number, fit: engine.Fit, installed = true) =>
  ({ id, name: id, size_bytes: size, fit, installed, slot: "llm" }) as unknown as engine.ModelRow;

const status = (defaultLlm: string) => ({ profile: "desktop", default_llm: defaultLlm }) as unknown as engine.Status;

describe("note model advice", () => {
  const e4b = row("gemma-4-e4b-it-q4_0", 5.15e9, "wont_fit");
  const e2b = row("gemma-4-e2b-it-q4_0", 3.35e9, "fits", false);

  it("suggests the smaller default when the chosen model won't fit", () => {
    const advice = noteModelAdvice([e4b, e2b], e4b.id, status(e2b.id));
    expect(advice?.suggested.id).toBe(e2b.id);
  });

  it("stays quiet when the chosen model fits, is the default, or nothing smaller is offered", () => {
    expect(noteModelAdvice([{ ...e4b, fit: "tight" }, e2b], e4b.id, status(e2b.id))).toBeNull();
    expect(noteModelAdvice([e4b, e2b], e2b.id, status(e2b.id))).toBeNull();
    expect(noteModelAdvice([e4b, e2b], e4b.id, status(e4b.id))).toBeNull();
    expect(noteModelAdvice(null, e4b.id, status(e2b.id))).toBeNull();
    const phone = { profile: "ios", platform: "ios", default_llm: e2b.id } as unknown as engine.Status;
    expect(noteModelAdvice([e4b, e2b], e4b.id, phone)).toBeNull();
  });
});
