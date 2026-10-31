import { createLevelSource } from "@openbark/ui";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AppContext, loadSettings, type Ctx } from "../context";
import * as engine from "../engine";
import { Review } from "./Review";

vi.mock("../engine", async (importOriginal) => {
  const real = await importOriginal<typeof import("../engine")>();
  return {
    ...real,
    openSession: vi.fn(),
    settleDefaults: vi.fn(),
    signNote: vi.fn(),
    listTemplates: vi.fn().mockResolvedValue([]),
    template: vi.fn().mockRejectedValue(new Error("none")),
    on: vi.fn().mockResolvedValue(() => {}),
  };
});

const mocked = vi.mocked(engine);

const template: engine.Template = {
  id: "examen",
  version: "1",
  name: "Examen",
  sections: [
    { id: "motif", title: "Motif", kind: "statements", parts: [] },
    {
      id: "examen_physique",
      title: "Examen physique",
      kind: "statements",
      parts: [],
      defaults: [
        { label: "Yeux", text: "Normal, aucun écoulement" },
        { label: "Cœur", text: "Rythme régulier" },
      ],
    },
  ],
};

function view(defaults: engine.PendingDefault[], signedAt: string | null = null): engine.SessionView {
  const run: engine.NoteRun = {
    status: "draft",
    template: "examen@1",
    model: "gemma",
    note: { motif: [{ text: "Se gratte l'oreille", evidence: [0] }], examen_physique: [] },
    faithfulness: { passed: true, statements_checked: 1, numbers_checked: 0, unsupported_numbers: [], citation_mismatches: [], invalid_evidence: [] },
    stats: { prompt_tokens: 1, completion_tokens: 1, total_ms: 1, tokens_per_second: 1, time_to_first_token_ms: 1 },
    template_defaults: defaults,
  };
  return {
    id: "2026-10-31_090000_mina",
    dir: "",
    audio_path: "",
    created_at: "2026-10-31 09:00",
    source: "Recorded",
    transcript: { segments: [{ id: 0, start: 0, end: 1, speaker: 0, text: "Elle se gratte l'oreille." }] },
    stats: null,
    latest_run: run,
    latest_markdown: "",
    signed_at: signedAt,
    signed_by: signedAt ? "Dr Martin" : null,
    patient: null,
    latest_template: template,
    addenda: [],
    consent: null,
    documents: [],
    recorded_by: null,
    pets: [],
    pet: null,
    any_signed: Boolean(signedAt),
    has_audio: false,
    audio_deleted: null,
  } as unknown as engine.SessionView;
}

const waiting: engine.PendingDefault[] = [
  { heading: "examen_physique", label: "Yeux", text: "Normal, aucun écoulement" },
  { heading: "examen_physique", label: "Cœur", text: "Rythme régulier" },
];

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
    actor: "Dr Martin",
    actorName: "Dr Martin",
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

describe("Review: normal findings from the template (ADR-0032)", () => {
  it("marks them, says why Sign is unavailable, and confirms them as the vet's own", async () => {
    show(view(waiting));
    expect(await screen.findByText("Template default, not from the recording")).toBeTruthy();
    expect(screen.getByText("Yeux: Normal, aucun écoulement")).toBeTruthy();
    expect(screen.getByText(/2 normal findings from the template weren't said in the recording\. Signing waits/)).toBeTruthy();
    expect((screen.getByRole("button", { name: /Sign note/ }) as HTMLButtonElement).disabled).toBe(true);

    // Confirm one; the draft reloads with the other still waiting.
    mocked.settleDefaults.mockResolvedValue({ markdown: "", run: view([waiting[1]!]).latest_run! });
    mocked.openSession.mockResolvedValue(view([waiting[1]!]));
    fireEvent.click(screen.getByRole("button", { name: "Confirm Yeux" }));
    await waitFor(() =>
      expect(mocked.settleDefaults).toHaveBeenCalledWith(
        "2026-10-31_090000_mina",
        { heading: "examen_physique", labels: ["Yeux"], confirm: true, text: null },
        "Dr Martin",
        null,
      ),
    );
    expect(await screen.findByText(/1 normal finding from the template wasn't said in the recording/)).toBeTruthy();
    expect((screen.getByRole("button", { name: /Sign note/ }) as HTMLButtonElement).disabled).toBe(true);

    // Removed: nothing waits, and Sign is available.
    mocked.settleDefaults.mockResolvedValue({ markdown: "", run: view([]).latest_run! });
    mocked.openSession.mockResolvedValue(view([]));
    fireEvent.click(screen.getByRole("button", { name: "Remove Cœur" }));
    await waitFor(() => expect(screen.queryByText("Template default, not from the recording")).toBeNull());
    expect(mocked.settleDefaults).toHaveBeenLastCalledWith(
      "2026-10-31_090000_mina",
      { heading: "examen_physique", labels: ["Cœur"], confirm: false, text: null },
      "Dr Martin",
      null,
    );
    expect((screen.getByRole("button", { name: /Sign note/ }) as HTMLButtonElement).disabled).toBe(false);
  });

  it("never shows them on a signed note", async () => {
    show(view(waiting, "2026-10-31 10:00"));
    expect(await screen.findByText("Se gratte l'oreille")).toBeTruthy();
    expect(screen.queryByText("Template default, not from the recording")).toBeNull();
    expect(screen.queryByText(/Normal, aucun écoulement/)).toBeNull();
  });
});
