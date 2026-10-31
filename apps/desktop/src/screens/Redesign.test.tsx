import { createLevelSource } from "@openbark/ui";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AppContext, loadSettings, type Ctx } from "../context";
import * as engine from "../engine";
import { Recorder } from "./Recorder";
import { SettingsScreen } from "./Settings";
import { Visits } from "./Visits";

vi.mock("../engine", async (importOriginal) => {
  const real = await importOriginal<typeof import("../engine")>();
  return {
    ...real,
    on: vi.fn().mockResolvedValue(() => {}),
    listTemplates: vi.fn().mockResolvedValue([]),
    listPatients: vi.fn().mockResolvedValue([]),
    searchPatients: vi.fn().mockResolvedValue([]),
    listSessions: vi.fn().mockResolvedValue([]),
    syncStatus: vi.fn().mockResolvedValue({ available: true, phase: "off" }),
    startRecording: vi.fn(),
    prewarmSpeech: vi.fn().mockResolvedValue(undefined),
    releaseSpeech: vi.fn().mockResolvedValue(undefined),
  };
});

const mocked = vi.mocked(engine);

function ctx(over: Partial<Ctx> = {}): Ctx {
  return {
    route: { screen: "record" },
    navigate: vi.fn(),
    status: null,
    refreshStatus: vi.fn(),
    settings: { ...loadSettings(), uiLanguage: "en", consentEachTime: false },
    setSettings: vi.fn(),
    toast: vi.fn(),
    announce: vi.fn(),
    levels: createLevelSource(),
    user: null,
    setUser: vi.fn(),
    actor: "Sam",
    actorName: "Sam",
    can: { notes: true, sign: true, team: true, admin: true },
    ...over,
  };
}

const status = (profile: "desktop" | "ios") =>
  ({ profile, mobile_tier: profile === "ios" ? 8 : null, models: [], encryption: { enabled: true, source: "keychain", warning: null }, team: false, version: "0.1.3" }) as unknown as engine.Status;

function phoneWidth(on: boolean) {
  vi.stubGlobal("matchMedia", (q: string) => ({
    matches: on && q === "(max-width: 640px)",
    media: q,
    addEventListener() {},
    removeEventListener() {},
  }));
}

function show(node: React.ReactNode, c: Ctx) {
  render(<AppContext.Provider value={c}>{node}</AppContext.Provider>);
  return c;
}

beforeEach(() => vi.clearAllMocks());
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

describe("Record (ADR-0027)", () => {
  it("has one optional patient row, one template choice and the options list on a phone", async () => {
    phoneWidth(true);
    show(<Recorder />, ctx({ status: status("ios") }));
    expect(screen.getByRole("button", { name: /Optional. Add now or after the visit/ })).toBeTruthy();
    expect(screen.getAllByRole("button", { name: /Template/ })).toHaveLength(1);
    expect(screen.getByRole("checkbox", { name: "Phone on speaker" })).toBeTruthy();
    expect(screen.getByRole("checkbox", { name: "Label speakers" })).toBeTruthy();
    expect(screen.getByRole("checkbox", { name: "Draft the note when I stop" })).toBeTruthy();
    expect(screen.getByText("Consent: you get it before recording.")).toBeTruthy();
    expect(screen.getByText("On this iPhone")).toBeTruthy();
  });

  it("opens a full recording screen on a phone: the time as a timer, Stop and save, no Pause", async () => {
    phoneWidth(true);
    let level: ((level: number, seconds: number) => void) | undefined;
    mocked.startRecording.mockImplementation(async (...args) => {
      const hooks = args[6] as { onLevel?: (level: number, seconds: number) => void } | undefined;
      level = hooks?.onLevel;
      return { stop: vi.fn() };
    });
    show(<Recorder />, ctx({ status: status("ios") }));
    fireEvent.click(screen.getByRole("button", { name: "Start recording" }));
    await waitFor(() => expect(mocked.startRecording).toHaveBeenCalled());
    act(() => level?.(0.4, 134));
    expect(await screen.findByRole("timer")).toBeTruthy();
    expect(screen.getByRole("timer").textContent).toBe("Recording · 02:14");
    expect(screen.getByRole("button", { name: "Stop and save" })).toBeTruthy();
    expect(screen.queryByRole("button", { name: /Pause/ })).toBeNull();
    expect(screen.getByText("Transcribing here")).toBeTruthy();
    expect(document.querySelector(".recording-screen")).not.toBeNull();
  });

  it("keeps recording in place on a computer, with the template as a select", () => {
    phoneWidth(false);
    show(<Recorder />, ctx({ status: status("desktop") }));
    expect(screen.getByRole("combobox", { name: "Template" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Import a recording" })).toBeTruthy();
    expect(document.querySelector(".ob-record-wrap[data-size='large']")).not.toBeNull();
  });
});

describe("Visits (ADR-0027)", () => {
  const today = new Date();
  const stamp = `${today.getFullYear()}-${String(today.getMonth() + 1).padStart(2, "0")}-${String(today.getDate()).padStart(2, "0")} 10:42`;
  const row = (over: Partial<engine.SessionSummary>): engine.SessionSummary => ({
    id: "v1",
    created_at: stamp,
    source: "Recorded",
    audio_secs: 728,
    runs: 0,
    signed: false,
    patient: "Maple Okonkwo",
    animal: "Maple",
    family: "Okonkwo",
    reason: "Annual wellness",
    template: "",
    ...over,
  });

  it("groups visits into Needs you and Done, with plain times and one status each", async () => {
    phoneWidth(false);
    mocked.listSessions.mockResolvedValue([
      row({ id: "a", runs: 1 }),
      row({ id: "b", animal: "Juniper", family: "Haddad", signed: true, signed_by: "Dr Rivera", runs: 1 }),
    ]);
    show(<Visits />, ctx({ status: status("desktop") }));
    expect(await screen.findByText("Maple · Okonkwo")).toBeTruthy();
    expect(screen.getByRole("columnheader", { name: "Needs you" })).toBeTruthy();
    expect(screen.getByRole("columnheader", { name: "Done" })).toBeTruthy();
    expect(screen.getByText("Check and sign")).toBeTruthy();
    expect(screen.getByText("Signed by Dr Rivera")).toBeTruthy();
    expect(screen.getAllByText("10:42")).toHaveLength(2);
    expect(screen.getAllByText("12:08")).toHaveLength(2);
    expect(screen.queryByRole("button", { name: "New visit" })).toBeNull();
  });

  it("opens a visit in Review", async () => {
    phoneWidth(true);
    mocked.listSessions.mockResolvedValue([row({ id: "a" })]);
    const c = show(<Visits />, ctx());
    fireEvent.click(await screen.findByRole("button", { name: /Maple · Okonkwo/ }));
    expect(c.navigate).toHaveBeenCalledWith({ screen: "review", id: "a" });
  });
});

describe("Settings as sections (ADR-0027)", () => {
  it("shows one section at a time on a computer, every section listed", () => {
    phoneWidth(false);
    show(<SettingsScreen section="recording" />, ctx({ status: status("desktop") }));
    expect(screen.getByRole("heading", { name: "Recording and consent", level: 2 })).toBeTruthy();
    const nav = screen.getByRole("navigation", { name: "Settings sections" });
    for (const name of ["You and your team", "Phone calls", "PDFs and copying", "Encrypted sync", "Privacy and storage", "Deleted visits", "About"])
      expect(nav.textContent).toContain(name);
  });

  it("has a Spoken language choice on a computer, Auto by default, and none on the iPhone (ADR-0033)", () => {
    phoneWidth(false);
    const c = show(<SettingsScreen section="recording" />, ctx({ status: status("desktop") }));
    const select = screen.getByRole("combobox", { name: /^Spoken language/ }) as HTMLSelectElement;
    expect(select.value).toBe("");
    expect(Array.from(select.options).map((o) => o.textContent)).toEqual([
      "Auto",
      "English",
      "Français",
      "Español",
      "Deutsch",
      "Português",
    ]);
    fireEvent.change(select, { target: { value: "fr" } });
    expect(c.setSettings).toHaveBeenCalledWith({ spokenLanguage: "fr" });
    fireEvent.change(select, { target: { value: "" } });
    expect(c.setSettings).toHaveBeenLastCalledWith({ spokenLanguage: null });
    cleanup();
    show(<SettingsScreen section="recording" />, ctx({ status: status("ios") }));
    expect(screen.queryByRole("combobox", { name: /^Spoken language/ })).toBeNull();
  });

  it("is a list of rows on a phone; the iPhone leaves out phone calls", async () => {
    phoneWidth(true);
    show(<SettingsScreen />, ctx({ status: status("ios") }));
    expect(screen.getByRole("button", { name: /Templates/ })).toBeTruthy();
    expect(screen.getByRole("button", { name: /Models and storage/ })).toBeTruthy();
    expect(screen.getByRole("button", { name: /Guides and videos/ })).toBeTruthy();
    expect(screen.getByRole("checkbox", { name: "Write notes on this iPhone" })).toBeTruthy();
    expect(screen.queryByRole("button", { name: /Phone calls/ })).toBeNull();
  });

  it("keeps phone calls for a narrow window on a computer", () => {
    phoneWidth(true);
    show(<SettingsScreen />, ctx({ status: status("desktop") }));
    expect(screen.getByRole("button", { name: /Phone calls/ })).toBeTruthy();
    expect(screen.queryByRole("checkbox", { name: "Write notes on this iPhone" })).toBeNull();
  });

  it("opens a section on a phone with a way back to Settings", () => {
    phoneWidth(true);
    const c = show(<SettingsScreen section="privacy" />, ctx({ status: status("ios") }));
    fireEvent.click(screen.getByRole("button", { name: "Settings" }));
    expect(c.navigate).toHaveBeenCalledWith({ screen: "settings" });
    // The iPhone never mentions the key setting or folders.
    expect(screen.queryByText(/OPENBARK_DATA_KEY/)).toBeNull();
    expect(screen.getByText(/encrypted on this iPhone/)).toBeTruthy();
  });
});
