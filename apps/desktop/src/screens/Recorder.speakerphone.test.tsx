import { createLevelSource } from "@openbark/ui";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AppContext, loadSettings, type Ctx, type Settings } from "../context";
import * as engine from "../engine";
import { Recorder } from "./Recorder";

vi.mock("../engine", async (importOriginal) => {
  const real = await importOriginal<typeof import("../engine")>();
  return {
    ...real,
    on: vi.fn().mockResolvedValue(() => {}),
    listTemplates: vi.fn().mockResolvedValue([]),
    listPatients: vi.fn().mockResolvedValue([]),
    searchPatients: vi.fn().mockResolvedValue([]),
    startRecording: vi.fn(),
  };
});

const mocked = vi.mocked(engine);

function ctx(settings: Partial<Settings>): Ctx {
  return {
    route: { screen: "record" },
    navigate: vi.fn(),
    status: null,
    refreshStatus: vi.fn(),
    settings: { ...loadSettings(), uiLanguage: "en", ...settings },
    setSettings: vi.fn(),
    toast: vi.fn(),
    announce: vi.fn(),
    levels: createLevelSource(),
    user: null,
    setUser: vi.fn(),
    actor: "Sam",
    actorName: "Sam",
    can: { notes: true, sign: true, team: true, admin: true },
  };
}

function show(c: Ctx) {
  render(
    <AppContext.Provider value={c}>
      <Recorder />
    </AppContext.Provider>,
  );
  return c;
}

const HINT =
  "Picks up the quieter voice of a client on a phone on speaker. Put the phone between you and the computer, volume up.";

beforeEach(() => vi.clearAllMocks());
afterEach(cleanup);

describe("Record, Phone on speaker", () => {
  it("is off by default, with no hint", () => {
    expect(loadSettings().phoneOnSpeaker).toBe(false);
    show(ctx({}));
    const box = screen.getByRole("checkbox", { name: "Phone on speaker" }) as HTMLInputElement;
    expect(box.checked).toBe(false);
    expect(screen.queryByText(HINT)).toBeNull();
  });

  it("ticking it saves the setting", () => {
    const c = show(ctx({}));
    fireEvent.click(screen.getByRole("checkbox", { name: "Phone on speaker" }));
    expect(c.setSettings).toHaveBeenCalledWith({ phoneOnSpeaker: true });
  });

  it("when on, shows the hint and records with it", async () => {
    mocked.startRecording.mockResolvedValue({ stop: vi.fn() });
    show(ctx({ phoneOnSpeaker: true, consentEachTime: false }));
    const box = screen.getByRole("checkbox", { name: "Phone on speaker" });
    expect(screen.getByText(HINT)).toBeTruthy();
    expect(box.getAttribute("aria-describedby")).toBe("phone-on-speaker-hint");
    fireEvent.click(screen.getByRole("button", { name: "Start recording" }));
    await waitFor(() => expect(mocked.startRecording).toHaveBeenCalled());
    const [speakers, speakerphone] = mocked.startRecording.mock.calls[0]!;
    expect(speakers).toBe(true);
    expect(speakerphone).toBe(true);
  });
});
