import { createLevelSource } from "@openbark/ui";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AppContext, loadSettings, type Ctx, type Settings } from "../context";
import * as engine from "../engine";
import { PhoneWalkthrough, walkthroughDue } from "./PhoneWalkthrough";

vi.mock("../engine", async (importOriginal) => {
  const real = await importOriginal<typeof import("../engine")>();
  return {
    ...real,
    isDesktop: true,
    on: vi.fn().mockResolvedValue(() => {}),
    models: vi.fn().mockResolvedValue([]),
    downloadModel: vi.fn(),
    micTestStart: vi.fn(),
    micTestStop: vi.fn(),
  };
});

const mocked = vi.mocked(engine);

const ios = {
  profile: "ios",
  mobile_tier: 8,
  default_llm: "gemma-4-e2b-it-q4_0",
  models: [
    { id: "silero-vad", name: "Silero VAD", installed: true, bundled: true },
    { id: "parakeet-tdt-0.6b-v3-int8", name: "Parakeet", installed: true, bundled: true },
  ],
} as unknown as engine.Status;

function ctx(over: Partial<Settings> = {}): Ctx {
  return {
    route: { screen: "record" },
    navigate: vi.fn(),
    status: ios,
    refreshStatus: vi.fn(),
    settings: { ...loadSettings(), uiLanguage: "en", ...over },
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
      <PhoneWalkthrough />
    </AppContext.Provider>,
  );
  return c;
}

const next = (name: string) => fireEvent.click(screen.getByRole("button", { name }));

beforeEach(() => vi.clearAllMocks());
afterEach(cleanup);

describe("walkthroughDue", () => {
  const s = (over: Partial<Settings>) => ({ ...loadSettings(), ...over });
  it("shows on an iPhone's first run only", () => {
    expect(walkthroughDue(ios, s({}))).toBe(true);
    expect(walkthroughDue(ios, s({ walkthroughDoneAt: "2026-10-25T10:00:00Z" }))).toBe(false);
    // Someone who already accepted the consent notice isn't on a first run.
    expect(walkthroughDue(ios, s({ consentAcknowledgedAt: "2026-10-20T10:00:00Z" }))).toBe(false);
  });

  it("shows again when asked from Settings, and never on a computer", () => {
    expect(walkthroughDue(ios, s({ walkthroughDoneAt: "2026-10-25T10:00:00Z", showWalkthrough: true }))).toBe(true);
    const desktop = { ...ios, profile: "desktop" } as engine.Status;
    expect(walkthroughDue(desktop, s({}))).toBe(false);
    expect(walkthroughDue(null, s({}))).toBe(false);
  });
});

describe("The iPhone walkthrough", () => {
  it("goes through four screens, asks for the microphone only on the second, and lands on Record", async () => {
    mocked.micTestStart.mockResolvedValue({ test: 7 } as engine.MicTestStarted);
    mocked.micTestStop.mockResolvedValue(undefined);
    const c = show(ctx());

    // Started: it stays until finished, even once consent is accepted.
    expect(c.setSettings).toHaveBeenCalledWith({ showWalkthrough: true });
    expect(walkthroughDue(ios, { ...loadSettings(), consentAcknowledgedAt: "2026-10-25T10:00:00Z", showWalkthrough: true })).toBe(true);

    // 1. Welcome: the promise, and no permission asked.
    expect(screen.getByRole("heading", { name: "Welcome to OpenBark" })).toBeTruthy();
    expect(screen.getByText("Recordings stay on this iPhone.")).toBeTruthy();
    expect(screen.getByText("OpenBark records the consult and writes the note, here on this iPhone.")).toBeTruthy();
    expect(screen.getByText("Step 1 of 4")).toBeTruthy();
    expect(mocked.micTestStart).not.toHaveBeenCalled();
    next("Get started");

    // 2. Recording and consent: Continue waits for the agreement.
    expect(screen.getByRole("heading", { name: "Recording and consent" })).toBeTruthy();
    expect(mocked.micTestStart).not.toHaveBeenCalled();
    expect((screen.getByRole("button", { name: "Continue" }) as HTMLButtonElement).disabled).toBe(true);
    next("Allow microphone");
    await waitFor(() => expect(mocked.micTestStart).toHaveBeenCalledWith(null));
    expect(await screen.findByText("Microphone ready", {}, { timeout: 3000 })).toBeTruthy();
    expect(mocked.micTestStop).toHaveBeenCalledWith(7);
    fireEvent.click(screen.getByRole("checkbox", { name: "I understand, and I'll get consent from clients before recording." }));
    next("Continue");
    expect(c.setSettings).toHaveBeenCalledWith(expect.objectContaining({ consentAcknowledgedAt: expect.any(String) }));

    // 3. Notes on this iPhone: the speech models are built in.
    expect(screen.getByRole("heading", { name: "Notes on this iPhone" })).toBeTruthy();
    expect(screen.getByText(/speech models are built in, so recording and transcribing work offline/)).toBeTruthy();
    // Offered, and off until the person turns it on (drafting on the phone is opt-in).
    expect((screen.getByRole("checkbox", { name: "Write notes on this iPhone" }) as HTMLInputElement).checked).toBe(false);
    next("Continue");

    // 4. The clinic computer, then Record; finishing is remembered.
    expect(screen.getByRole("heading", { name: "Use it with your clinic computer" })).toBeTruthy();
    next("Later");
    expect(c.setSettings).toHaveBeenCalledWith({ walkthroughDoneAt: expect.any(String), showWalkthrough: false });
    expect(c.navigate).toHaveBeenCalledWith({ screen: "record" });
  });

  it("can be skipped, and the skip is remembered", () => {
    const c = show(ctx());
    next("Skip");
    expect(c.setSettings).toHaveBeenCalledWith({ walkthroughDoneAt: expect.any(String), showWalkthrough: false });
    expect(c.navigate).toHaveBeenCalledWith({ screen: "record" });
    expect(mocked.micTestStart).not.toHaveBeenCalled();
    // Consent wasn't accepted, so the notice still asks for it afterwards.
    expect(c.setSettings).not.toHaveBeenCalledWith(expect.objectContaining({ consentAcknowledgedAt: expect.any(String) }));
  });

  it("offers no drafting on a 6 GB iPhone", () => {
    const c = ctx();
    c.status = { ...ios, mobile_tier: 6, default_llm: "" } as unknown as engine.Status;
    show(c);
    // Screen 1 says what this iPhone does: no notes written here.
    expect(screen.getByText("OpenBark records and transcribes the consult here on this iPhone. Your computer writes the note.")).toBeTruthy();
    next("Get started");
    fireEvent.click(screen.getByRole("checkbox", { name: "I understand, and I'll get consent from clients before recording." }));
    next("Continue");
    expect(
      screen.getByText("Writing notes on the phone needs an iPhone with 8 GB of memory or more. Record here, and draft the note on your computer."),
    ).toBeTruthy();
    expect(screen.queryByRole("checkbox", { name: "Write notes on this iPhone" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Not now" })).toBeNull();
  });

  it("says plainly when the microphone is blocked", async () => {
    mocked.micTestStart.mockRejectedValue(new Error("denied"));
    show(ctx());
    next("Get started");
    next("Allow microphone");
    expect(
      await screen.findByText("Microphone access is off. Turn it on in the iPhone's Settings, OpenBark, Microphone."),
    ).toBeTruthy();
  });

  it("opens Encrypted sync from the last screen, and Not now leaves notes to a computer", () => {
    const c = show(ctx({ consentAcknowledgedAt: "2026-10-20T10:00:00Z", showWalkthrough: true }));
    next("Get started");
    // Seen again: the agreement already given.
    next("Continue");
    next("Not now");
    expect(c.setSettings).toHaveBeenCalledWith({ writeNotesHere: false });
    next("Open Encrypted sync");
    expect(c.navigate).toHaveBeenCalledWith({ screen: "settings", section: "sync" });
  });
});

describe("The walkthrough in a build that can't draft", () => {
  it("says the computer writes the note, even on a 12 GB iPhone", () => {
    const c = ctx();
    c.status = { ...ios, mobile_tier: 12, can_draft: false } as unknown as engine.Status;
    show(c);
    expect(screen.getByText("OpenBark records and transcribes the consult here on this iPhone. Your computer writes the note.")).toBeTruthy();
  });
});
