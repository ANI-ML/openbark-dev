import { createLevelSource } from "@openbark/ui";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AppContext, loadSettings, type Ctx } from "../context";
import * as engine from "../engine";
import { CallButton } from "./CallButton";
import { PhoneSettings } from "./PhoneSettings";

vi.mock("../engine", async (importOriginal) => {
  const real = await importOriginal<typeof import("../engine")>();
  return {
    ...real,
    phone: vi.fn(),
    phoneBin: vi.fn(),
    phoneConnect: vi.fn(),
    phoneDisconnect: vi.fn(),
    phoneSetPrefs: vi.fn(),
    phoneCheck: vi.fn(),
    openPhone: vi.fn(),
  };
});

const mocked = vi.mocked(engine);
const PREFS: engine.PhonePrefs = {
  enabled: false,
  record_calls: true,
  announcement:
    "This call is recorded for your pet's medical record. The recording stays at the clinic. Is that all right with you?",
};
const OFF: engine.PhoneView = { connected: false, twilio: null, secret: null, prefs: PREFS, ready: false, linux: false, outgoing_bin: null };
const ON: engine.PhoneView = {
  connected: true,
  twilio: {
    account_sid: "AC-made-up",
    api_key_hint: "SK…cdef",
    api_key_sid: "SK-made-up",
    computer_name: "front-desk",
    identity: "openbark_front_desk",
    caller_id: "+15555550100",
    outgoing_bin_url: "https://handler.twilio.com/twiml/EH-made-up",
    twiml_app_sid: "AP-made-up",
  },
  secret: { location: "keychain", hint: "****0042" },
  prefs: { ...PREFS, enabled: true },
  ready: true,
  linux: false,
  outgoing_bin: "<Response/>",
};

function ctx(team = true): Ctx {
  return {
    route: { screen: "settings" },
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
    actor: "Dr Lee",
    actorName: "Dr Lee",
    can: { notes: true, sign: team, team, admin: team },
  };
}

function show(node: React.ReactNode, c = ctx()) {
  render(<AppContext.Provider value={c}>{node}</AppContext.Provider>);
  return c;
}

beforeEach(() => vi.clearAllMocks());
afterEach(cleanup);

describe("Settings > Phone calls", () => {
  it("offers Connect Twilio to a veterinarian, and says who can to everyone else", async () => {
    mocked.phone.mockResolvedValue(OFF);
    show(<PhoneSettings />);
    expect(await screen.findByRole("button", { name: "Connect Twilio" })).toBeTruthy();
    cleanup();
    show(<PhoneSettings />, ctx(false));
    expect(await screen.findByText("Only a veterinarian can connect accounts.")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Connect Twilio" })).toBeNull();
  });

  it("connects with what was pasted, shows the Bin text, and never shows the secret again", async () => {
    mocked.phone.mockResolvedValue(OFF);
    mocked.phoneBin.mockResolvedValue('<Response>\n  <Dial callerId="+15555550100">');
    mocked.phoneConnect.mockResolvedValue({ phone: ON });
    const c = show(<PhoneSettings />);
    fireEvent.click(await screen.findByRole("button", { name: "Connect Twilio" }));
    const type = (label: string, value: string) => fireEvent.change(screen.getByLabelText(new RegExp(`^${label}`)), { target: { value } });
    type("Account SID", "AC-made-up");
    type("API key SID", "SK-made-up");
    type("API key secret", "pasted-secret");
    type("This computer's name", "Front desk");
    type("Caller ID", "+1 555 555 0100");
    fireEvent.click(screen.getByRole("button", { name: "Show the Twilio text" }));
    expect(await screen.findByText(/callerId="\+15555550100"/)).toBeTruthy();
    expect(mocked.phoneBin).toHaveBeenCalledWith("+1 555 555 0100");
    type("Outgoing Bin URL", "https://handler.twilio.com/twiml/EH-made-up");
    fireEvent.click(screen.getByRole("button", { name: "Connect" }));
    await waitFor(() =>
      expect(mocked.phoneConnect).toHaveBeenCalledWith({
        account_sid: "AC-made-up",
        api_key_sid: "SK-made-up",
        api_key_secret: "pasted-secret",
        computer_name: "Front desk",
        caller_id: "+1 555 555 0100",
        outgoing_bin_url: "https://handler.twilio.com/twiml/EH-made-up",
      }),
    );
    await waitFor(() => expect(c.toast).toHaveBeenCalledWith("Connected to Twilio. Calls show +1 555 555 0100."));
    expect(await screen.findByText("Twilio · connected")).toBeTruthy();
    expect(screen.getByText("SK…cdef")).toBeTruthy();
    expect(document.body.textContent).not.toContain("pasted-secret");
  });

  it("shows Twilio's refusal in plain words and keeps the form", async () => {
    mocked.phone.mockResolvedValue(OFF);
    mocked.phoneConnect.mockRejectedValue(
      new Error("Twilio didn't accept this key. Check the SID and secret, or make a new key in your Twilio console."),
    );
    show(<PhoneSettings />);
    fireEvent.click(await screen.findByRole("button", { name: "Connect Twilio" }));
    fireEvent.click(screen.getByRole("button", { name: "Connect" }));
    expect((await screen.findByRole("alert")).textContent).toContain("Twilio didn't accept this key");
    expect(screen.getByLabelText(/^Account SID/)).toBeTruthy();
  });

  it("turns the phone on, runs the checks, and disconnects with the reminder", async () => {
    mocked.phone.mockResolvedValue({ ...ON, prefs: PREFS, ready: false });
    mocked.phoneSetPrefs.mockResolvedValue(ON);
    mocked.phoneCheck.mockResolvedValue([
      { id: "key", ok: true, message: "Twilio accepts this computer's key." },
      { id: "trial", ok: null, message: "Real calls need an upgraded Twilio account. Trials can't connect calls through OpenBark." },
    ]);
    mocked.phoneDisconnect.mockResolvedValue({ note: "Also delete this key in your Twilio console, so it can't be used anywhere else." });
    const c = show(<PhoneSettings />);
    fireEvent.click(await screen.findByLabelText("Turn on the phone on this computer"));
    await waitFor(() => expect(mocked.phoneSetPrefs).toHaveBeenCalledWith({ ...PREFS, enabled: true }));
    fireEvent.click(screen.getByRole("button", { name: "Check setup" }));
    expect(await screen.findByText(/upgraded Twilio account/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Disconnect" }));
    expect(screen.getByRole("alertdialog").textContent).toContain("Also delete it in your Twilio console");
    fireEvent.click(screen.getAllByRole("button", { name: "Disconnect" })[1]!);
    await waitFor(() => expect(c.toast).toHaveBeenCalledWith(expect.stringContaining("Also delete this key")));
  });

  it("can't be changed by someone who isn't a veterinarian", async () => {
    mocked.phone.mockResolvedValue(ON);
    show(<PhoneSettings />, ctx(false));
    expect(((await screen.findByLabelText("Turn on the phone on this computer")) as HTMLInputElement).disabled).toBe(true);
    expect(screen.queryByRole("button", { name: "Disconnect" })).toBeNull();
  });
});

describe("Call", () => {
  it("appears only when the phone is ready, and opens the phone window for the patient", async () => {
    mocked.phone.mockResolvedValue(OFF);
    show(<CallButton patientId="p-bella" />);
    await waitFor(() => expect(mocked.phone).toHaveBeenCalled());
    expect(screen.queryByRole("button", { name: "Call" })).toBeNull();
    cleanup();
    mocked.phone.mockResolvedValue(ON);
    mocked.openPhone.mockResolvedValue(false);
    const c = show(<CallButton patientId="p-bella" />);
    fireEvent.click(await screen.findByRole("button", { name: "Call" }));
    await waitFor(() => expect(mocked.openPhone).toHaveBeenCalledWith({ patient_id: "p-bella" }));
    await waitFor(() => expect(c.toast).toHaveBeenCalledWith("Allow pop-ups for this page to make calls."));
  });
});
