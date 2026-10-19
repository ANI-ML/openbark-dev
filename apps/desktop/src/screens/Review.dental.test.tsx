import { createLevelSource } from "@openbark/ui";
import { cleanup, render, screen } from "@testing-library/react";
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
  id: "dental",
  version: "0.2.0",
  name: "Dental procedure",
  sections: [{ id: "dental_chart", title: "Dental chart", kind: "teeth", dentition: "canine", parts: [] }],
};

const item = (tooth: string, finding: string) => ({
  tooth,
  text: `${tooth} ${finding}`,
  finding,
  detail: null,
  grade: null,
  mm: null,
  planned: false,
  evidence: [0],
});

function view(faithfulness: Partial<engine.NoteRun["faithfulness"]>): engine.SessionView {
  const run: engine.NoteRun = {
    status: "draft",
    template: "dental@0.2.0",
    model: "gemma",
    note: { dental_chart: [item("108", "fracture"), item("105", "missing")] },
    faithfulness: {
      passed: true,
      statements_checked: 2,
      numbers_checked: 2,
      unsupported_numbers: [],
      citation_mismatches: [],
      invalid_evidence: [],
      ...faithfulness,
    },
    stats: { prompt_tokens: 1, completion_tokens: 1, total_ms: 1, tokens_per_second: 1, time_to_first_token_ms: 1 },
  };
  return {
    id: "2026-10-19_090000_juno",
    dir: "",
    audio_path: "",
    created_at: "2026-10-19 09:00",
    source: "Recorded",
    transcript: { segments: [{ id: 0, start: 0, end: 1, speaker: 0, text: "One oh eight, upper left fourth premolar, is fractured." }] },
    stats: null,
    latest_run: run,
    latest_markdown: "",
    signed_at: null,
    signed_by: null,
    patient: null,
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
  } as unknown as engine.SessionView;
}

function ctx(): Ctx {
  return {
    route: { screen: "review", id: "v" },
    navigate: vi.fn(),
    status: null,
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

function show(v: engine.SessionView) {
  mocked.openSession.mockResolvedValue(v);
  render(
    <AppContext.Provider value={ctx()}>
      <Review id={v.id} autoDraft={false} />
    </AppContext.Provider>,
  );
}

beforeEach(() => vi.clearAllMocks());
afterEach(cleanup);

describe("Review: tooth chart flags in the summary", () => {
  it("counts a tooth flag as an item to check, even when it is the only flag", async () => {
    show(view({ tooth_flags: [{ path: "/dental_chart/0", tooth: "108", kind: "name_conflict", other: "208" }] }));
    expect(await screen.findByText(/1 item needs checking against the transcript\./)).toBeTruthy();
    expect((screen.getByRole("button", { name: /Sign/ }) as HTMLButtonElement).disabled).toBe(false);
  });

  it("counts an item with two tooth flags once", async () => {
    show(
      view({
        tooth_flags: [
          { path: "/dental_chart/0", tooth: "108", kind: "name_conflict", other: "208" },
          { path: "/dental_chart/0", tooth: "108", kind: "cue_missing", expected: ["fracture"] },
        ],
      }),
    );
    expect(await screen.findByText(/1 item needs checking against the transcript\./)).toBeTruthy();
  });

  it("blocks signing for a tooth that needs dog or cat chosen, and says why", async () => {
    show(view({ passed: false, tooth_flags: [{ path: "/dental_chart/1", tooth: "105", kind: "choose_dentition" }] }));
    expect(await screen.findByText(/1 tooth on the chart blocks signing until it's corrected\./)).toBeTruthy();
    expect((screen.getByRole("button", { name: /Sign/ }) as HTMLButtonElement).disabled).toBe(true);
  });
});
