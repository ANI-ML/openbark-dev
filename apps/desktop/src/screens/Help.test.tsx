import { createLevelSource } from "@openbark/ui";
import { act, cleanup, fireEvent, render } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { AppContext, loadSettings, type Ctx } from "../context";
import { Help } from "./Help";

function ctx(): Ctx {
  return {
    route: { screen: "help" },
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
  } as Ctx;
}

function systemDark(dark: boolean) {
  vi.stubGlobal(
    "matchMedia",
    vi.fn((query: string) => ({
      matches: dark && query.includes("dark"),
      media: query,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    })),
  );
}

function videoSrc(container: HTMLElement) {
  return container.querySelector("video")?.getAttribute("src");
}

function show() {
  return render(
    <AppContext.Provider value={ctx()}>
      <Help />
    </AppContext.Provider>,
  );
}

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  document.documentElement.removeAttribute("data-theme");
});

describe("Help videos follow the theme", () => {
  it("plays the light recording in the light theme", () => {
    systemDark(false);
    const { container } = show();
    expect(videoSrc(container)).toBe("help/getting-started.webm");
  });

  it("plays the dark recording when the system is dark and no theme is chosen", () => {
    systemDark(true);
    const { container } = show();
    expect(videoSrc(container)).toBe("help/getting-started-dark.webm");
  });

  it("an explicit choice on <html> wins over the system, and changes follow it", async () => {
    systemDark(true);
    document.documentElement.setAttribute("data-theme", "light");
    const { container } = show();
    expect(videoSrc(container)).toBe("help/getting-started.webm");
    await act(async () => {
      document.documentElement.setAttribute("data-theme", "dark");
      await Promise.resolve();
    });
    expect(videoSrc(container)).toBe("help/getting-started-dark.webm");
  });

  it("falls back to the light recording if the dark one can't load", () => {
    systemDark(false);
    document.documentElement.setAttribute("data-theme", "dark");
    const { container } = show();
    const video = container.querySelector("video");
    expect(video?.getAttribute("src")).toBe("help/getting-started-dark.webm");
    fireEvent.error(video as HTMLVideoElement);
    expect(videoSrc(container)).toBe("help/getting-started.webm");
  });
});
