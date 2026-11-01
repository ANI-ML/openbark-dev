import { describe, expect, it } from "vitest";
import { loadSettings, type Settings } from "./context";
import type * as engine from "./engine";
import {
  E2B,
  E4B,
  GPU_PACK,
  checkCardDue,
  downloadQueue,
  noteMinutes,
  savedSpeed,
  setupMode,
  suggestSmaller,
  transferEta,
} from "./setup";

const s = (over: Partial<Settings> = {}): Settings => ({
  ...loadSettings(),
  consentAcknowledgedAt: "2026-10-31T10:00:00Z",
  ...over,
});

const status = (installed: boolean, profile: "desktop" | "ios" = "desktop") =>
  ({
    profile,
    models: [
      { id: "silero-vad", name: "Silero VAD", installed },
      { id: "parakeet-tdt-0.6b-v3-int8", name: "Parakeet", installed },
      { id: E2B, name: "Gemma 4 E2B", installed },
    ],
  }) as unknown as engine.Status;

const row = (id: string, slot: engine.Slot, size: number, installed = false) =>
  ({ id, name: id, slot, size_bytes: size, installed }) as engine.ModelRow;

const rows = [
  row(E4B, "llm", 5_154_941_280),
  row(E2B, "llm", 3_349_516_256),
  row("parakeet-tdt-0.6b-v3-int8", "stt", 670_478_772),
  row("silero-vad", "vad", 643_854),
];

describe("when setup opens (ADR-0034)", () => {
  it("opens by itself on a computer's first run, after the consent notice", () => {
    expect(setupMode(status(false), s(), false)).toBe("full");
    expect(setupMode(status(false), s({ consentAcknowledgedAt: null }), false)).toBeNull();
    // With a team, once someone has said who they are.
    expect(setupMode(status(false), s(), true)).toBeNull();
    // Finished or skipped: not again by itself, even with models missing.
    expect(setupMode(status(false), s({ setupDoneAt: "2026-10-31T10:05:00Z" }), false)).toBeNull();
    // Everything downloaded: no walkthrough (the card asks instead).
    expect(setupMode(status(true), s(), false)).toBeNull();
  });

  it("opens again when asked, as the whole walkthrough or the check", () => {
    const done = { setupDoneAt: "2026-10-31T10:05:00Z" };
    expect(setupMode(status(true), s({ ...done, showSetup: "full" }), false)).toBe("full");
    expect(setupMode(status(true), s({ ...done, showSetup: "check" }), false)).toBe("check");
  });

  it("never opens on an iPhone, which has its own walkthrough", () => {
    expect(setupMode(status(false, "ios"), s(), false)).toBeNull();
    expect(setupMode(status(false, "ios"), s({ showSetup: "full" }), false)).toBeNull();
    expect(setupMode(null, s(), false)).toBeNull();
  });
});

describe("the existing-install card", () => {
  it("shows once on a computer set up before this version", () => {
    expect(checkCardDue(status(true), s())).toBe(true);
    expect(checkCardDue(status(true), s({ setupCardDismissedAt: "2026-10-31T10:00:00Z" }))).toBe(false);
    expect(checkCardDue(status(true), s({ setupDoneAt: "2026-10-31T10:00:00Z" }))).toBe(false);
    // Not with models still to download (setup opens instead), nor on an iPhone.
    expect(checkCardDue(status(false), s())).toBe(false);
    expect(checkCardDue(status(true, "ios"), s())).toBe(false);
    // Not while the check itself is open.
    expect(checkCardDue(status(true), s({ showSetup: "check" }))).toBe(false);
  });
});

describe("what setup downloads", () => {
  it("takes the speech models and the chosen note model, smallest first", () => {
    const q = downloadQueue(status(false).models, rows, E4B, null);
    // The engine's list names E2B (the default); the person chose E4B.
    expect(q.map((i) => i.id)).toEqual(["silero-vad", "parakeet-tdt-0.6b-v3-int8", E4B]);
    expect(q[2]?.name).toBe("Gemma 4 E4B");
  });

  it("leaves out what's already here and adds the GPU pack when asked", () => {
    const installed = rows.map((r) => (r.id === "silero-vad" ? { ...r, installed: true } : r));
    const required = status(false).models.map((m) => (m.id === "silero-vad" ? { ...m, installed: true } : m));
    const q = downloadQueue(required, installed, E2B, 2_000_000_000);
    expect(q.map((i) => i.id)).toEqual(["parakeet-tdt-0.6b-v3-int8", GPU_PACK, E2B]);
    expect(q.find((i) => i.id === GPU_PACK)?.kind).toBe("gpu");
  });

  it("downloads nothing when everything is here", () => {
    const all = rows.map((r) => ({ ...r, installed: true }));
    expect(downloadQueue(status(true).models, all, E2B, null)).toEqual([]);
  });
});

describe("download time left", () => {
  it("waits for something to go on, then works from the speed measured", () => {
    expect(transferEta(0, 2_000, 1_000_000, 10_000_000)).toBeNull();
    expect(transferEta(0, 10_000, 0, 10_000_000)).toBeNull();
    // 10 MB in 10 s: 1 MB a second, 90 MB to go.
    expect(transferEta(0, 10_000, 10_000_000, 100_000_000)).toEqual({ bytesPerSecond: 1_000_000, secsLeft: 90 });
  });
});

describe("the speed check's estimate", () => {
  const check = (secs: number | null, model = E4B) =>
    ({
      model,
      load_ms: 4000,
      read_tokens: 520,
      read_ms: 16000,
      read_tokens_per_second: 32.5,
      written_tokens: 160,
      write_tokens_per_second: 10.7,
      long_visit_note_secs: secs,
      drafts_on: "processor",
    }) as engine.SpeedCheck;

  it("reads in whole minutes, at least one", () => {
    expect(noteMinutes(537)).toBe(9);
    expect(noteMinutes(20)).toBe(1);
    expect(noteMinutes(89)).toBe(1);
    expect(noteMinutes(91)).toBe(2);
  });

  it("suggests the smaller model only for the larger one, past five minutes", () => {
    expect(suggestSmaller(check(301), E4B)).toBe(true);
    expect(suggestSmaller(check(300), E4B)).toBe(false);
    expect(suggestSmaller(check(900, E2B), E2B)).toBe(false);
    expect(suggestSmaller(check(null), E4B)).toBe(false);
    expect(suggestSmaller(null, E4B)).toBe(false);
  });

  it("is kept for Models and Record", () => {
    const at = new Date("2026-10-31T10:00:00Z");
    expect(savedSpeed(check(537), at)).toEqual({ model: E4B, read: 32.5, write: 10.7, noteSecs: 537, at: at.toISOString() });
  });
});
