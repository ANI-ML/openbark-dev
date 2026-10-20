// The microphone test on Record and in Settings (web mode): one microphone
// open at a time, whatever the clicks, and none left open when Record is
// pressed or the screen closes while a test is still opening.
import { createLevelSource } from "@openbark/ui";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AppContext, loadSettings, type Ctx } from "../context";
import { MicCheck } from "./MicCheck";

const opened: { stop: ReturnType<typeof vi.fn>; resolve: () => void }[] = [];

vi.mock("../mic/browser", () => ({
  listBrowserMics: () => Promise.resolve([]),
  testBrowserMic: () =>
    new Promise((resolve) => {
      const stop = vi.fn();
      opened.push({
        stop,
        resolve: () => resolve({ label: "Fake mic", sampleRate: 48000, fellBack: false, stop }),
      });
    }),
}));

function ctx(): Ctx {
  return {
    route: { screen: "record" },
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
    actor: null,
    actorName: "",
    can: { notes: true, sign: true, team: true, admin: true },
  } as unknown as Ctx;
}

const view = (disabled = false) => (
  <AppContext.Provider value={ctx()}>
    <MicCheck disabled={disabled} />
  </AppContext.Provider>
);

beforeEach(() => {
  opened.length = 0;
});
afterEach(cleanup);

const testButton = () => screen.getByRole("button", { name: /Test microphone|Stop test/ });

describe("MicCheck", () => {
  it("a double click opens one microphone, and Stop closes it", async () => {
    render(view());
    await act(async () => {
      fireEvent.click(testButton());
      fireEvent.click(testButton());
    });
    expect(opened).toHaveLength(1);
    await act(async () => opened[0]!.resolve());
    expect(testButton().textContent).toMatch(/Stop test/);
    fireEvent.click(testButton());
    expect(opened[0]!.stop).toHaveBeenCalledTimes(1);
    expect(testButton().textContent).toMatch(/Test microphone/);
  });

  it("pressing Record while the test is opening closes the microphone it opens", async () => {
    const r = render(view());
    await act(async () => fireEvent.click(testButton()));
    // Record pressed: the recorder disables the check.
    r.rerender(view(true));
    await act(async () => opened[0]!.resolve());
    expect(opened[0]!.stop).toHaveBeenCalledTimes(1);
    expect(testButton().textContent).toMatch(/Test microphone/);
  });

  it("leaving the screen while the test is opening closes the microphone it opens", async () => {
    const r = render(view());
    await act(async () => fireEvent.click(testButton()));
    r.unmount();
    await act(async () => opened[0]!.resolve());
    expect(opened[0]!.stop).toHaveBeenCalledTimes(1);
  });
});
