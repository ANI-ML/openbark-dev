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

const select = () => screen.getByRole("combobox", { name: "Speakers in the room" }) as HTMLSelectElement;

beforeEach(() => vi.clearAllMocks());
afterEach(cleanup);

describe("Record, Speakers in the room", () => {
  it("is Auto by default, with 2, 3 and 4 to choose from", () => {
    expect(loadSettings().roomSpeakers).toBeNull();
    show(ctx({}));
    expect(select().value).toBe("auto");
    expect(Array.from(select().options).map((o) => o.text)).toEqual(["Auto", "2", "3", "4"]);
  });

  it("choosing a number saves it, and Auto clears it", () => {
    const c = show(ctx({}));
    fireEvent.change(select(), { target: { value: "3" } });
    expect(c.setSettings).toHaveBeenCalledWith({ roomSpeakers: 3 });
    cleanup();
    const d = show(ctx({ roomSpeakers: 2 }));
    expect(select().value).toBe("2");
    fireEvent.change(select(), { target: { value: "auto" } });
    expect(d.setSettings).toHaveBeenCalledWith({ roomSpeakers: null });
  });

  it("records with the number chosen", async () => {
    mocked.startRecording.mockResolvedValue({ stop: vi.fn() });
    show(ctx({ roomSpeakers: 3, consentEachTime: false }));
    fireEvent.click(screen.getByRole("button", { name: "Start recording" }));
    await waitFor(() => expect(mocked.startRecording).toHaveBeenCalled());
    const [speakers, , roomSpeakers] = mocked.startRecording.mock.calls[0]!;
    expect(speakers).toBe(true);
    expect(roomSpeakers).toBe(3);
  });

  it("is off while speakers aren't labelled", () => {
    show(ctx({ labelSpeakers: false }));
    expect(select().disabled).toBe(true);
  });
});
