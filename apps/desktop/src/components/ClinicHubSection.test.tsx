import { createLevelSource } from "@openbark/ui";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AppContext, loadSettings, type Ctx } from "../context";
import * as engine from "../engine";
import { ClinicHubSection, parseRanges } from "./ClinicHubSection";

vi.mock("../engine", async (importOriginal) => {
  const real = await importOriginal<typeof import("../engine")>();
  return {
    ...real,
    clinicHubStatus: vi.fn(),
    clinicHubSettings: vi.fn(),
    clinicHubCode: vi.fn(),
    clinicHubJoinSelf: vi.fn(),
    clinicHubRemoveDevice: vi.fn(),
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
      <ClinicHubSection />
    </AppContext.Provider>,
  );

const OFF: engine.ClinicHubStatus = {
  available: true,
  on: false,
  port: 47110,
  allowed: ["10.0.0.0/8", "192.168.0.0/16"],
  quota_mb: 20480,
  addresses: ["192.168.1.20"],
  join: null,
  devices: [],
};

const ON: engine.ClinicHubStatus = {
  ...OFF,
  on: true,
  listening: true,
  fingerprint: "ab".repeat(32),
  check: "7K3QD-9XAPR",
  accounts: 1,
  used_mb: 2.5,
  join: {
    code: "ABCD-EFGH",
    check: "7K3QD-9XAPR",
    address: "192.168.1.20:47110",
    expires_at: new Date(Date.now() + 20 * 60_000).toISOString(),
    used: false,
    link: "openbark-hub:1?h=192.168.1.20&p=47110&f=ab&c=ABCDEFGH",
    qr_svg: "<svg></svg>",
  },
  devices: [
    {
      id: "0192aaaa-0000-7000-8000-000000000001",
      account: "5f3c2a1b",
      platform: "ios",
      status: "approved",
      created_at: "2026-10-25T09:00:00Z",
      last_seen_at: null,
    },
  ],
};

describe("Settings, Clinic hub", () => {
  beforeEach(() => vi.resetAllMocks());
  afterEach(cleanup);

  it("is hidden where this computer can't be a hub", async () => {
    mocked.clinicHubStatus.mockResolvedValue({ available: false, on: false });
    const { container } = show();
    await waitFor(() => expect(mocked.clinicHubStatus).toHaveBeenCalled());
    await waitFor(() => expect(container.querySelector(".hub-section")).toBeNull());
    expect(screen.queryByText("Make this computer the clinic hub")).toBeNull();
  });

  it("is off by default and turns on with the switch", async () => {
    mocked.clinicHubStatus.mockResolvedValue(OFF);
    mocked.clinicHubSettings.mockResolvedValue(ON);
    show();
    const toggle = (await screen.findByLabelText(/Make this computer the clinic hub/)) as HTMLInputElement;
    expect(toggle.checked).toBe(false);
    expect(screen.queryByText("Join code")).toBeNull();
    fireEvent.click(toggle);
    await waitFor(() => expect(mocked.clinicHubSettings).toHaveBeenCalledWith({ on: true }));
  });

  it("shows the join code with its address and certificate check, and the devices", async () => {
    mocked.clinicHubStatus.mockResolvedValue(ON);
    mocked.clinicHubCode.mockResolvedValue(ON);
    mocked.clinicHubRemoveDevice.mockResolvedValue({ ...ON, devices: [] });
    show();
    expect(await screen.findByText("ABCD-EFGH")).toBeTruthy();
    expect(screen.getAllByText("192.168.1.20:47110").length).toBeGreaterThan(0);
    expect(screen.getAllByText("7K3QD-9XAPR").length).toBeGreaterThan(0);
    expect(screen.getByText(/Listening on port 47110/)).toBeTruthy();
    expect(screen.getByText("iPhone")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "New join code" }));
    await waitFor(() => expect(mocked.clinicHubCode).toHaveBeenCalled());
    fireEvent.click(screen.getByRole("button", { name: "Remove" }));
    expect(await screen.findByText("Remove this device from the hub?")).toBeTruthy();
    const buttons = screen.getAllByRole("button", { name: "Remove" });
    fireEvent.click(buttons[buttons.length - 1] as HTMLElement);
    await waitFor(() => expect(mocked.clinicHubRemoveDevice).toHaveBeenCalledWith("0192aaaa-0000-7000-8000-000000000001"));
  });

  it("saves the port, the allowed ranges and the storage limit", async () => {
    mocked.clinicHubStatus.mockResolvedValue(ON);
    mocked.clinicHubSettings.mockResolvedValue(ON);
    show();
    const ranges = (await screen.findByLabelText(/Allowed addresses/)) as HTMLTextAreaElement;
    // The form fills in once the status has loaded.
    await waitFor(() => expect(ranges.value).toBe("10.0.0.0/8\n192.168.0.0/16"));
    fireEvent.change(ranges, { target: { value: "192.168.1.0/24, 100.64.0.0/10" } });
    fireEvent.change(screen.getByLabelText("Storage per person (GB)"), { target: { value: "5" } });
    fireEvent.click(screen.getByRole("button", { name: "Save hub settings" }));
    await waitFor(() =>
      expect(mocked.clinicHubSettings).toHaveBeenCalledWith({
        port: 47110,
        allowed: ["192.168.1.0/24", "100.64.0.0/10"],
        quotaMb: 5120,
      }),
    );
  });

  it("reads address ranges one per line or separated by commas", () => {
    expect(parseRanges(" 10.0.0.0/8\n\n192.168.0.0/16 , fd00::/8 ")).toEqual(["10.0.0.0/8", "192.168.0.0/16", "fd00::/8"]);
  });
});
