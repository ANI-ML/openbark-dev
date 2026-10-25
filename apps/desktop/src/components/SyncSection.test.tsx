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
    syncJoinHub: vi.fn(),
    syncFindHubs: vi.fn(),
    syncConfirmKit: vi.fn(),
    syncPair: vi.fn(),
    syncRemoveDevice: vi.fn(),
    syncNow: vi.fn(),
    syncConfirmPairing: vi.fn(),
    syncStartOver: vi.fn(),
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
    expect(screen.getByRole("button", { name: "Join your clinic hub" })).toBeTruthy();
    // The email sign-in is gone (ADR-0029).
    expect(screen.queryByText(/email/i)).toBeNull();
    expect(mocked.syncJoinHub).not.toHaveBeenCalled();
    expect(mocked.syncSendCode).not.toHaveBeenCalled();
  });

  it("joins the hub with a code, its address and certificate check, then shows the kit and checks two words", async () => {
    mocked.syncStatus.mockResolvedValue(OFF);
    mocked.syncFindHubs.mockResolvedValue([
      {
        name: "OpenBark hub 3f9a01",
        host: "192.168.1.20",
        port: 47110,
        addresses: ["192.168.1.20"],
        fingerprint_prefix: "3f9a01",
      },
    ]);
    mocked.syncJoinHub.mockResolvedValue({ next: "kit", ...KIT });
    mocked.syncConfirmKit.mockResolvedValue({ ...OFF, phase: "active" });
    show();
    fireEvent.click(await screen.findByRole("button", { name: "Join your clinic hub" }));
    const join = screen.getByRole("button", { name: "Join the hub" }) as HTMLButtonElement;
    expect(join.disabled).toBe(true);
    fireEvent.click(screen.getByRole("button", { name: "Find on this network" }));
    expect(await screen.findByText("OpenBark hub 3f9a01")).toBeTruthy();
    expect((screen.getByLabelText("Hub address") as HTMLInputElement).value).toBe("192.168.1.20:47110");
    fireEvent.change(screen.getByLabelText(/Join code/), { target: { value: "abcd-efgh" } });
    fireEvent.change(screen.getByLabelText(/Certificate check/), { target: { value: "7K3QD-9XAPR" } });
    fireEvent.click(join);
    await waitFor(() =>
      expect(mocked.syncJoinHub).toHaveBeenCalledWith("abcd-efgh", "192.168.1.20:47110", "7K3QD-9XAPR", "", 30, false),
    );
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

  it("joins with the hub's link alone, as another device of the same person", async () => {
    mocked.syncStatus.mockResolvedValue(OFF);
    mocked.syncJoinHub.mockResolvedValue({
      next: "pair",
      words: ["a", "b", "c", "d", "e", "f"],
      qr_svg: "<svg></svg>",
      expires_at: null,
    });
    show();
    fireEvent.click(await screen.findByRole("button", { name: "Join your clinic hub" }));
    const link = "openbark-hub:1?h=192.168.1.20&p=47110&f=00&c=ABCDEFGH";
    fireEvent.change(screen.getByLabelText(/Join code/), { target: { value: link } });
    expect(screen.queryByLabelText("Hub address")).toBeNull();
    fireEvent.click(screen.getByLabelText("I already use OpenBark on another device"));
    fireEvent.click(screen.getByRole("button", { name: "Join the hub" }));
    await waitFor(() => expect(mocked.syncJoinHub).toHaveBeenCalledWith(link, "", "", "", 30, true));
    expect(await screen.findByText("a b c d e f")).toBeTruthy();
  });

  it("shows the hub it syncs with and asks for the word delete before deleting the account", async () => {
    mocked.syncStatus.mockResolvedValue({
      ...OFF,
      phase: "active",
      hub: { address: "192.168.1.20:47110", fingerprint: "ab12 cd34 ef56 7890" },
      last_error: "Hub not reachable: syncs when you're back on the clinic network.",
      devices: [],
    });
    show();
    expect(await screen.findByText(/Syncing with the clinic hub at 192.168.1.20:47110/)).toBeTruthy();
    expect(screen.getByText("ab12 cd34 ef56 7890")).toBeTruthy();
    expect(screen.getByText(/Hub not reachable/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Delete my sync account" }));
    expect(await screen.findByText("Type delete to confirm")).toBeTruthy();
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

  it("after approval through a hub, shows whose account it joined and syncs only once the code is typed", async () => {
    const confirm: engine.SyncStatus = {
      ...OFF,
      phase: "confirm",
      hub: { address: "192.168.1.20:47110", fingerprint: "ab12 cd34 ef56 7890" },
      devices: [
        { id: "a", name: "Front desk PC", platform: "windows", status: "approved", this_device: false, last_seen_at: null, approved_at: null },
        { id: "b", name: "Exam room laptop", platform: "macos", status: "approved", this_device: true, last_seen_at: null, approved_at: null },
      ],
    };
    mocked.syncStatus.mockResolvedValue(confirm);
    mocked.syncConfirmPairing
      .mockRejectedValueOnce(new Error("That code doesn't match."))
      .mockResolvedValueOnce({ ...confirm, phase: "active" });
    const c = ctx();
    show(c);
    expect(await screen.findByText(/This device was added to the account with: Front desk PC\./)).toBeTruthy();
    // No way around the check: no Turn off here.
    expect(screen.queryByRole("button", { name: /Turn off sync/ })).toBeNull();
    const box = screen.getByLabelText("Code from the device that added this one");
    const ok = screen.getByRole("button", { name: "Confirm and start syncing" }) as HTMLButtonElement;
    expect(ok.disabled).toBe(true);
    fireEvent.change(box, { target: { value: "wxyz-2345" } });
    fireEvent.click(ok);
    expect(await screen.findByText("That code doesn't match.")).toBeTruthy();
    fireEvent.change(box, { target: { value: "ABCD-2345" } });
    fireEvent.click(ok);
    await waitFor(() => expect(mocked.syncConfirmPairing).toHaveBeenLastCalledWith("ABCD-2345"));
    await waitFor(() => expect(c.toast).toHaveBeenCalledWith("Encrypted sync is on"));
  });

  it("starts over when none of the person's devices added this one", async () => {
    mocked.syncStatus.mockResolvedValue({ ...OFF, phase: "confirm", devices: [] });
    mocked.syncStartOver.mockResolvedValue(OFF);
    show();
    fireEvent.click(await screen.findByRole("button", { name: "Start over" }));
    await waitFor(() => expect(mocked.syncStartOver).toHaveBeenCalled());
  });

  it("shows the code to type on the new device after adding it through a hub", async () => {
    mocked.syncStatus.mockResolvedValue({
      ...OFF,
      phase: "active",
      hub: { address: "192.168.1.20:47110", fingerprint: "ab12 cd34 ef56 7890" },
      devices: [],
    });
    mocked.syncPair
      .mockResolvedValueOnce({ device_id: "c", name: "Home laptop", platform: "linux" })
      .mockResolvedValueOnce({ device_id: "c", name: "Home laptop", platform: "linux", confirm: "K7M2-QX9P" });
    const c = ctx();
    show(c);
    fireEvent.click(await screen.findByRole("button", { name: "Add a device" }));
    fireEvent.change(screen.getByLabelText("The 6 words shown on the new device"), {
      target: { value: "apple banana cherry dolphin eagle falcon" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Check" }));
    fireEvent.click(await screen.findByRole("button", { name: "Add device" }));
    expect(await screen.findByText("K7M2-QX9P")).toBeTruthy();
    expect(screen.getByText(/Type this code on the new device/)).toBeTruthy();
    expect(c.toast).not.toHaveBeenCalled();
  });

  it("speaks the app's language", async () => {
    mocked.syncStatus.mockResolvedValue(OFF);
    show(ctx("fr"));
    expect(await screen.findByRole("button", { name: "Rejoindre le hub de votre clinique" })).toBeTruthy();
  });
});
