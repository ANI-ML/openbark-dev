import { createLevelSource } from "@openbark/ui";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { useState, type ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AppContext, loadSettings, type Ctx, type Settings, type ToastAction } from "../context";
import * as engine from "../engine";
import { Toast } from "./Toast";
import { newerVersion, UPDATE_POLL_MS, UpdateChecks, UpdateSettings, updatesOffered } from "./UpdateCheck";

vi.mock("../engine", async (importOriginal) => {
  const real = await importOriginal<typeof import("../engine")>();
  return { ...real, checkForUpdate: vi.fn(), updateStatus: vi.fn(), openReleaseLink: vi.fn() };
});

const mocked = vi.mocked(engine);

const DESKTOP = { version: "0.2.4", platform: "windows", profile: "desktop" } as unknown as engine.Status;
const WEB = { version: "0.2.4", platform: "linux", profile: "desktop" } as unknown as engine.Status;
const IPHONE = { version: "0.2.4", platform: "ios", profile: "ios" } as unknown as engine.Status;

const view = (over: Partial<engine.UpdateView> = {}): engine.UpdateView => ({
  current: "0.2.4",
  checked: true,
  latest: "0.2.4",
  newer: false,
  notes: "https://github.com/ANI-ML/openbark-releases/releases/tag/v0.2.4",
  download: "https://www.openbark.io/#download",
  checked_at: 1_793_491_200,
  ...over,
});

/** The settings as the app last rendered them. */
let saved: Settings;
const toast = vi.fn<(message: string, action?: ToastAction | ToastAction[], ms?: number) => void>();

/** The app's context with real, changing settings. */
function Shell({
  start,
  status = DESKTOP,
  children,
}: {
  start: Partial<Settings>;
  status?: engine.Status | null;
  children: ReactNode;
}) {
  const [settings, setState] = useState<Settings>(() => ({ ...loadSettings(), uiLanguage: "en", ...start }));
  saved = settings;
  const ctx: Ctx = {
    route: { screen: "record" },
    navigate: vi.fn(),
    status,
    refreshStatus: vi.fn(),
    settings,
    setSettings: (p) => setState((prev) => ({ ...prev, ...p })),
    toast,
    announce: vi.fn(),
    levels: createLevelSource(),
    user: null,
    setUser: vi.fn(),
    actor: "Sam",
    actorName: "Sam",
    can: { notes: true, sign: true, team: false, admin: false },
  };
  return <AppContext.Provider value={ctx}>{children}</AppContext.Provider>;
}

const checks = (start: Partial<Settings>, status: engine.Status | null = DESKTOP, ready = true) =>
  render(
    <Shell start={start} status={status}>
      <UpdateChecks ready={ready} />
    </Shell>,
  );

/** Let resolved engine calls land. */
const settle = () => act(async () => {});

const question = () => screen.queryByRole("region", { name: "Check for new versions?" });

/** Fake time passes, then what it set off lands. */
const tick = async (ms: number) => {
  await act(async () => {
    vi.advanceTimersByTime(ms);
  });
  await act(async () => {});
};

beforeEach(() => {
  vi.resetAllMocks();
  mocked.checkForUpdate.mockResolvedValue(view());
  mocked.updateStatus.mockResolvedValue(view({ checked: false, latest: null, checked_at: null }));
  mocked.openReleaseLink.mockResolvedValue();
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

describe("asking once", () => {
  it("asks after setup and consent, and stores yes", async () => {
    checks({});
    const card = question();
    expect(card).not.toBeNull();
    expect(card!.textContent).toContain(
      "Once a day, OpenBark reads the version number from openbark.io. Nothing about you, your clinic or your patients is sent.",
    );
    expect(saved.updateAskedAt).not.toBeNull();
    expect(saved.updateCheck).toBe("ask");
    expect(mocked.checkForUpdate).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Yes, check daily" }));
    expect(saved.updateCheck).toBe("on");
    expect(question()).toBeNull();
    await settle();
    // Yes turns the daily check on, which checks now.
    expect(mocked.checkForUpdate).toHaveBeenCalledWith(false);
  });

  it("stores no, and then checks nothing", async () => {
    vi.useFakeTimers();
    checks({});
    fireEvent.click(screen.getByRole("button", { name: "No thanks" }));
    expect(saved.updateCheck).toBe("off");
    expect(question()).toBeNull();
    await tick(3 * UPDATE_POLL_MS);
    expect(mocked.checkForUpdate).not.toHaveBeenCalled();
  });

  it("shows once: ignored, it doesn't come back and nothing turns on", async () => {
    const first = checks({});
    expect(question()).not.toBeNull();
    const after = saved;
    first.unmount();
    // The next start, with what was saved.
    checks(after);
    expect(question()).toBeNull();
    expect(saved.updateCheck).toBe("ask");
    await settle();
    expect(mocked.checkForUpdate).not.toHaveBeenCalled();
  });

  it("waits for first-run setup and consent", () => {
    checks({}, DESKTOP, false);
    expect(question()).toBeNull();
    expect(saved.updateAskedAt).toBeNull();
  });

  it("asks existing users once after they update (no answer saved yet)", () => {
    checks({ consentAcknowledgedAt: "2026-10-01T09:00:00Z" });
    expect(question()).not.toBeNull();
  });

  it("waits until the status has loaded", () => {
    checks({}, null);
    expect(question()).toBeNull();
    expect(saved.updateAskedAt).toBeNull();
  });

  it("is asked in the browser version too", () => {
    checks({}, WEB);
    expect(question()).not.toBeNull();
  });

  it("never appears on the iPhone, and the iPhone never checks", async () => {
    checks({}, IPHONE);
    expect(question()).toBeNull();
    expect(saved.updateAskedAt).toBeNull();
    cleanup();
    checks({ updateCheck: "on" }, IPHONE);
    await settle();
    expect(mocked.checkForUpdate).not.toHaveBeenCalled();
    expect(updatesOffered(IPHONE)).toBe(false);
    expect(updatesOffered({ ...DESKTOP, platform: "android" })).toBe(false);
    expect(updatesOffered(DESKTOP)).toBe(true);
  });
});

describe("the daily check", () => {
  it("fetches nothing while the setting is ask or off", async () => {
    vi.useFakeTimers();
    for (const updateCheck of ["ask", "off"] as const) {
      checks({ updateCheck, updateAskedAt: "2026-10-31T09:00:00Z" });
      await tick(48 * UPDATE_POLL_MS);
      cleanup();
    }
    expect(mocked.checkForUpdate).not.toHaveBeenCalled();
    expect(mocked.updateStatus).not.toHaveBeenCalled();
  });

  it("asks the engine at start and then hourly while on (the engine keeps it to once a day)", async () => {
    vi.useFakeTimers();
    checks({ updateCheck: "on", updateAskedAt: "2026-10-31T09:00:00Z" });
    await act(async () => {});
    expect(mocked.checkForUpdate).toHaveBeenCalledTimes(1);
    expect(mocked.checkForUpdate).toHaveBeenLastCalledWith(false);
    await tick(UPDATE_POLL_MS);
    expect(mocked.checkForUpdate).toHaveBeenCalledTimes(2);
    expect(mocked.checkForUpdate.mock.calls.every(([manual]) => manual === false)).toBe(true);
  });

  it("stops when the setting is turned off", async () => {
    vi.useFakeTimers();
    render(
      <Shell start={{ updateCheck: "on", updateAskedAt: "2026-10-31T09:00:00Z" }}>
        <UpdateChecks ready />
        <UpdateSettings />
      </Shell>,
    );
    await act(async () => {});
    const calls = mocked.checkForUpdate.mock.calls.length;
    fireEvent.click(screen.getByRole("checkbox", { name: /Check for new versions once a day/ }));
    expect(saved.updateCheck).toBe("off");
    await tick(5 * UPDATE_POLL_MS);
    expect(mocked.checkForUpdate.mock.calls.length).toBe(calls);
  });

  it("fails quietly offline", async () => {
    mocked.checkForUpdate.mockRejectedValue(new Error("OpenBark couldn't reach openbark.io."));
    checks({ updateCheck: "on", updateAskedAt: "2026-10-31T09:00:00Z" });
    await settle();
    expect(toast).not.toHaveBeenCalled();
    expect(screen.queryByRole("alert")).toBeNull();
  });
});

describe("the notice", () => {
  const on = { updateCheck: "on" as const, updateAskedAt: "2026-10-31T09:00:00Z" };

  it("shows for a newer version, with What's new and Download", async () => {
    const newer = view({ latest: "0.2.10", newer: true });
    mocked.checkForUpdate.mockResolvedValue(newer);
    checks(on);
    await settle();
    expect(toast).toHaveBeenCalledTimes(1);
    const [message, actions, ms] = toast.mock.calls[0]!;
    expect(message).toBe("OpenBark 0.2.10 is available");
    expect(Array.isArray(actions) && actions.map((a) => a.label)).toEqual(["What's new", "Download"]);
    expect(ms).toBeGreaterThanOrEqual(10_000);
    expect(saved.updateNotified).toBe("0.2.10");
    const [notes, download] = actions as ToastAction[];
    notes!.run();
    download!.run();
    expect(mocked.openReleaseLink).toHaveBeenCalledWith("notes", newer);
    expect(mocked.openReleaseLink).toHaveBeenCalledWith("download", newer);
  });

  it("doesn't show when the version running is the latest", async () => {
    checks(on);
    await settle();
    expect(mocked.checkForUpdate).toHaveBeenCalled();
    expect(toast).not.toHaveBeenCalled();
    expect(saved.updateNotified).toBeNull();
  });

  it("shows once per version: dismissed, it waits for a newer one", async () => {
    vi.useFakeTimers();
    mocked.checkForUpdate.mockResolvedValue(view({ latest: "0.2.5", newer: true, checked: false }));
    checks(on);
    await act(async () => {});
    expect(toast).toHaveBeenCalledTimes(1);
    // Hourly, and after a restart: the same version doesn't show again.
    await tick(3 * UPDATE_POLL_MS);
    const after = saved;
    cleanup();
    checks(after);
    await act(async () => {});
    expect(toast).toHaveBeenCalledTimes(1);
    // A newer one does.
    mocked.checkForUpdate.mockResolvedValue(view({ latest: "0.2.6", newer: true }));
    await tick(UPDATE_POLL_MS);
    expect(toast).toHaveBeenCalledTimes(2);
    expect(toast.mock.calls[1]![0]).toBe("OpenBark 0.2.6 is available");
    expect(saved.updateNotified).toBe("0.2.6");
  });

  it("offers both buttons in the toast, and one press closes it", () => {
    const what = vi.fn();
    const download = vi.fn();
    const onClose = vi.fn();
    render(
      <Toast
        message="OpenBark 0.2.10 is available"
        action={[
          { label: "What's new", run: what },
          { label: "Download", run: download },
        ]}
        onClose={onClose}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "What's new" }));
    fireEvent.click(screen.getByRole("button", { name: "Download" }));
    expect(what).toHaveBeenCalledTimes(1);
    expect(download).not.toHaveBeenCalled();
    expect(onClose).toHaveBeenCalled();
  });
});

describe("Settings, About", () => {
  const about = (start: Partial<Settings>, status: engine.Status | null = DESKTOP) =>
    render(
      <Shell start={start} status={status}>
        <UpdateSettings />
      </Shell>,
    );

  it("Check for updates works while the daily check is off, and says when it's the latest", async () => {
    about({ updateCheck: "off" });
    fireEvent.click(screen.getByRole("button", { name: "Check for updates" }));
    expect(mocked.checkForUpdate).toHaveBeenCalledWith(true);
    expect((await screen.findByRole("status")).textContent).toBe("You have the latest version");
    expect(saved.updateCheck).toBe("off");
    expect(toast).not.toHaveBeenCalled();
  });

  it("shows the same notice for a newer version, even one already announced", async () => {
    const newer = view({ latest: "0.3.0", newer: true });
    mocked.checkForUpdate.mockResolvedValue(newer);
    about({ updateCheck: "ask", updateNotified: "0.3.0" });
    fireEvent.click(screen.getByRole("button", { name: "Check for updates" }));
    expect((await screen.findByRole("status")).textContent).toContain("OpenBark 0.3.0 is available");
    expect(toast).toHaveBeenCalledTimes(1);
    expect(toast.mock.calls[0]![0]).toBe("OpenBark 0.3.0 is available");
    fireEvent.click(screen.getByRole("button", { name: "What's new" }));
    expect(mocked.openReleaseLink).toHaveBeenCalledWith("notes", newer);
  });

  it("says so when openbark.io can't be reached", async () => {
    mocked.checkForUpdate.mockRejectedValue(new Error("offline"));
    about({ updateCheck: "off" });
    fireEvent.click(screen.getByRole("button", { name: "Check for updates" }));
    expect((await screen.findByRole("alert")).textContent).toContain("couldn't reach openbark.io");
  });

  it("has the daily switch with the same explanation, and it stores the answer", () => {
    about({ updateCheck: "ask" });
    const sw = screen.getByRole("checkbox", { name: /Check for new versions once a day/ }) as HTMLInputElement;
    expect(sw.checked).toBe(false);
    expect(sw.closest("label")!.textContent).toContain("Nothing about you, your clinic or your patients is sent.");
    fireEvent.click(sw);
    expect(saved.updateCheck).toBe("on");
    expect(saved.updateAskedAt).not.toBeNull();
    fireEvent.click(sw);
    expect(saved.updateCheck).toBe("off");
  });

  it("isn't there on the iPhone", () => {
    about({}, IPHONE);
    expect(screen.queryByRole("button", { name: "Check for updates" })).toBeNull();
    expect(mocked.updateStatus).not.toHaveBeenCalled();
  });
});

describe("versions", () => {
  it("compares number by number", () => {
    expect(newerVersion("0.2.10", "0.2.9")).toBe(true);
    expect(newerVersion("0.2.9", "0.2.10")).toBe(false);
    expect(newerVersion("0.2.4", "0.2.4")).toBe(false);
    expect(newerVersion("1.0.0", "0.99.99")).toBe(true);
    expect(newerVersion("0.2.5", null)).toBe(true);
    expect(newerVersion("not a version", null)).toBe(false);
    expect(newerVersion("0.3.0-beta.1", "0.2.4")).toBe(false);
  });
});
