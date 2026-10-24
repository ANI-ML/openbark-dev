import { createLevelSource } from "@openbark/ui";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AppContext, loadSettings, type Ctx } from "../context";
import * as engine from "../engine";
import { Review } from "./Review";

vi.mock("../engine", async (importOriginal) => {
  const real = await importOriginal<typeof import("../engine")>();
  return {
    ...real,
    openSession: vi.fn(),
    listTemplates: vi.fn().mockResolvedValue([]),
    template: vi.fn().mockRejectedValue(new Error("none")),
    on: vi.fn().mockResolvedValue(() => {}),
  };
});

const mocked = vi.mocked(engine);

const template: engine.Template = {
  id: "soap",
  version: "0.1.1",
  name: "SOAP note",
  sections: [{ id: "objective", title: "Objective", kind: "statements", parts: [] }],
} as unknown as engine.Template;

function view(over: Partial<engine.SessionView> = {}): engine.SessionView {
  const run = {
    status: "draft",
    template: "soap@0.1.1",
    model: "gemma-4-e2b-it-q4_0",
    note: { objective: [{ text: "Weight 24.6 kg.", evidence: [0] }] },
    faithfulness: {
      passed: true,
      statements_checked: 1,
      numbers_checked: 1,
      unsupported_numbers: [],
      citation_mismatches: [],
      invalid_evidence: [],
    },
    stats: { prompt_tokens: 1, completion_tokens: 1, total_ms: 1, tokens_per_second: 1, time_to_first_token_ms: 1 },
  };
  return {
    id: "2026-10-24_104200_maple",
    dir: "",
    audio_path: "",
    created_at: "2026-10-24 10:42",
    source: "Recorded",
    transcript: { segments: [{ id: 0, start: 192, end: 196, speaker: 0, text: "Weight today is 24.6 kilos." }] },
    stats: null,
    latest_run: run,
    latest_markdown: "",
    signed_at: null,
    signed_by: null,
    patient: { name: "Maple", last_name: "Okonkwo", reason: "Annual wellness" },
    latest_template: template,
    addenda: [],
    consent: null,
    documents: [],
    recorded_by: null,
    pets: [],
    pet: null,
    any_signed: false,
    has_audio: false,
    audio_deleted: null,
    ...over,
  } as unknown as engine.SessionView;
}

function ctx(profile: "desktop" | "ios"): Ctx {
  return {
    route: { screen: "review", id: "v" },
    navigate: vi.fn(),
    status: { profile, models: [], team: false } as unknown as engine.Status,
    refreshStatus: vi.fn(),
    settings: { ...loadSettings(), uiLanguage: "en" },
    setSettings: vi.fn(),
    toast: vi.fn(),
    announce: vi.fn(),
    levels: createLevelSource(),
    user: null,
    setUser: vi.fn(),
    actor: "Dr Rivera",
    actorName: "Dr Rivera",
    can: { notes: true, sign: true, team: true, admin: true },
  };
}

function phoneWidth(on: boolean) {
  vi.stubGlobal("matchMedia", (q: string) => ({
    matches: on && q === "(max-width: 640px)",
    media: q,
    addEventListener() {},
    removeEventListener() {},
  }));
}

function show(v: engine.SessionView, c: Ctx) {
  mocked.openSession.mockResolvedValue(v);
  render(
    <AppContext.Provider value={c}>
      <Review id={v.id} autoDraft={false} />
    </AppContext.Provider>,
  );
}

beforeEach(() => vi.clearAllMocks());
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

describe("Review: no signing on the iPhone during the pilot (ADR-0027)", () => {
  it("hides Sign note on the iPhone and says where to sign", async () => {
    phoneWidth(true);
    show(view(), ctx("ios"));
    expect(await screen.findByText("Sign it on your computer.")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Sign note" })).toBeNull();
    // The phone's bottom bar: Ask for changes and Copy note.
    expect(screen.getByRole("button", { name: "Ask for changes" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Copy note" })).toBeTruthy();
  });

  it("still says it on an iPhone held sideways, wider than the phone layout", async () => {
    phoneWidth(false);
    show(view(), ctx("ios"));
    expect(await screen.findByText("Sign it on your computer.")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Sign note" })).toBeNull();
  });

  it("offers no addendum on a signed visit on the iPhone", async () => {
    phoneWidth(true);
    show(view({ signed_at: "2026-10-24 11:00", signed_by: "Dr Rivera", any_signed: true }), ctx("ios"));
    await screen.findByRole("heading", { name: "Maple · Okonkwo" });
    expect(screen.queryByRole("button", { name: "Add addendum" })).toBeNull();
  });

  it("keeps Sign note on a computer, phone width or not", async () => {
    phoneWidth(true);
    show(view(), ctx("desktop"));
    expect(await screen.findByRole("button", { name: "Sign note" })).toBeTruthy();
    expect(screen.queryByText("Sign it on your computer.")).toBeNull();
  });

  it("shows where a tapped sentence was said, with Play this moment", async () => {
    phoneWidth(true);
    show(view({ has_audio: true }), ctx("ios"));
    await screen.findByText("Sign it on your computer.");
    const sentence = document.querySelector(".stmt");
    expect(sentence).not.toBeNull();
    fireEvent.click(sentence!);
    expect(screen.getByText("Speaker 1, at 03:12")).toBeTruthy();
    expect(screen.getByText("Weight today is 24.6 kilos.", { selector: "q" })).toBeTruthy();
  });
});
