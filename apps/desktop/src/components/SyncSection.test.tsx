import { createLevelSource } from "@openbark/ui";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AppContext, loadSettings, type Ctx } from "../context";
import * as engine from "../engine";
import { SYNC_PROMISE, SyncSection } from "./SyncSection";

vi.mock("../engine", async (importOriginal) => {
  const real = await importOriginal<typeof import("../engine")>();
  return {
    ...real,
    syncStatus: vi.fn(),
    syncSendCode: vi.fn(),
    syncVerify: vi.fn(),
    syncConfirmKit: vi.fn(),
    syncPair: vi.fn(),
    syncRemoveDevice: vi.fn(),
    syncNow: vi.fn(),
    recoveryKey: vi.fn(),
  };
});

const mocked = vi.mocked(engine);

function ctx(lang = "en"): Ctx {
  return {
    route: { screen: "settings" },
    navigate: vi.fn(),
    status: null,
    refreshStatus: vi.fn(),
    settings: { ...loadSettings(), uiLanguage: lang },
    setSettings: vi.fn(),
    toast: vi.fn(),
    announce: vi.fn(),
    levels: createLevelSource(),
    user: null,
    setUser: vi.fn(),
    actor: "Sam",
    actorName: "Sam",
    can: { notes: true, sign: true, team: false, admin: false },
  };
}

const show = (c = ctx()) =>
  render(
    <AppContext.Provider value={c}>
      <SyncSection />
    </AppContext.Provider>,
  );

const OFF: engine.SyncStatus = { available: true, phase: "off", server: "nmecovtmijnfnjuadicp.supabase.co" };
const KIT = {
  words: Array.from({ length: 24 }, (_, i) => `word${i + 1}`),
  qr_svg: "<svg></svg>",
  check: [3, 17] as [number, number],
};

describe("Settings, Encrypted sync", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    mocked.recoveryKey.mockResolvedValue("computer-key");
  });
  afterEach(cleanup);

  it("is off by default, says what syncs and what never leaves, and calls nothing until asked", async () => {
    mocked.syncStatus.mockResolvedValue(OFF);
    show();
    await screen.findByText(SYNC_PROMISE);
    expect(screen.getByText(/Recordings never leave the device that recorded them/)).toBeTruthy();
    expect(screen.getByText(/use test data only/)).toBeTruthy();
    expect(screen.getByRole("button", { name: "Turn on encrypted sync" })).toBeTruthy();
    expect(mocked.syncSendCode).not.toHaveBeenCalled();
  });

  it("turns on with an emailed code, shows the kit and checks two of its words", async () => {
    mocked.syncStatus.mockResolvedValue(OFF);
    mocked.syncSendCode.mockResolvedValue({ ...OFF, phase: "code_sent" });
    mocked.syncVerify.mockResolvedValue({ next: "kit", ...KIT });
    mocked.syncConfirmKit.mockResolvedValue({ ...OFF, phase: "active" });
    show();
    fireEvent.click(await screen.findByRole("button", { name: "Turn on encrypted sync" }));
    fireEvent.change(screen.getByLabelText(/Your email address/), { target: { value: "vet@example.org" } });
    fireEvent.click(screen.getByRole("button", { name: "Email me a code" }));
    await waitFor(() => expect(mocked.syncSendCode).toHaveBeenCalledWith("vet@example.org"));
    fireEvent.change(await screen.findByLabelText(/6-digit code/), { target: { value: "123456" } });
    fireEvent.click(screen.getByRole("button", { name: "Continue" }));
    await waitFor(() => expect(mocked.syncVerify).toHaveBeenCalledWith("123456", "", 30));
    expect(await screen.findByText("word24")).toBeTruthy();
    // The computer's own key arrives separately from the kit: wait for it.
    expect(await screen.findByText("computer-key")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "I've kept it safe" }));
    fireEvent.change(screen.getByLabelText("Word 3"), { target: { value: "word3" } });
    fireEvent.change(screen.getByLabelText("Word 17"), { target: { value: "word17" } });
    fireEvent.click(screen.getByRole("button", { name: "Check and start syncing" }));
    await waitFor(() =>
      expect(mocked.syncConfirmKit).toHaveBeenCalledWith([
        [3, "word3"],
        [17, "word17"],
      ]),
    );
    expect(await screen.findByText("Encrypted sync is on")).toBeTruthy();
  });

  it("lists devices and adds one by its 6 words after checking them", async () => {
    mocked.syncStatus.mockResolvedValue({
      ...OFF,
      phase: "active",
      email: "vet@example.org",
      last_sync_at: "2026-10-20T09:00:00Z",
      devices: [
        { id: "a", name: "Front desk PC", platform: "windows", status: "approved", this_device: true, last_seen_at: null, approved_at: null },
        { id: "b", name: "Exam room laptop", platform: "macos", status: "approved", this_device: false, last_seen_at: null, approved_at: null },
      ],
    });
    mocked.syncPair.mockResolvedValue({ device_id: "c", name: "Home laptop", platform: "linux" });
    show();
    expect(await screen.findByText("Exam room laptop")).toBeTruthy();
    expect(screen.getByText(/Syncing as vet@example.org/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Add a device" }));
    fireEvent.change(screen.getByLabelText("The 6 words shown on the new device"), {
      target: { value: "apple banana cherry dolphin eagle falcon" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Check" }));
    expect(await screen.findByText(/Add Home laptop \(Linux\) to your sync account\?/)).toBeTruthy();
    expect(mocked.syncPair).toHaveBeenCalledWith("apple banana cherry dolphin eagle falcon", false);
    fireEvent.click(screen.getByRole("button", { name: "Add device" }));
    await waitFor(() => expect(mocked.syncPair).toHaveBeenCalledWith("apple banana cherry dolphin eagle falcon", true));
  });

  it("speaks the app's language", async () => {
    mocked.syncStatus.mockResolvedValue(OFF);
    show(ctx("fr"));
    expect(await screen.findByRole("button", { name: "Activer la synchronisation chiffrée" })).toBeTruthy();
  });
});
