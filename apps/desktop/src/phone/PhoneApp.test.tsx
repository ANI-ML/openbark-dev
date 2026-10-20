import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { CallTarget } from "../engine";
import * as api from "./api";
import { PhoneApp } from "./PhoneApp";
import * as voice from "./voice";

vi.mock("./api", () => ({
  isDesktop: false,
  pending: vi.fn(),
  token: vi.fn(),
  startRecording: vi.fn(),
  announceSaved: vi.fn(),
}));
/** The taps' frame callback, so a test can play audio into the call. */
let feedFrames: (frames: Float32Array) => void = () => {};
vi.mock("./callAudio", () => ({
  createTaps: vi.fn(async (onFrames: (frames: Float32Array) => void) => ({
    ...((feedFrames = onFrames), {}),
    local: {},
    remote: {},
    sampleRate: 48000,
    setRecording: vi.fn(),
    setHeld: vi.fn(),
    close: vi.fn(async () => {}),
  })),
}));
vi.mock("./voice", () => ({ canCall: () => true, createDevice: vi.fn() }));

const mApi = vi.mocked(api);
const mVoice = vi.mocked(voice);

const BELLA: CallTarget = {
  patient_id: "p-bella",
  patient_name: "Bella",
  last_name: "Patel",
  number: "+15555550142",
  record_calls: true,
  announcement: "This call is recorded for your pet's medical record. Is that all right with you?",
  by: "Dr Lee",
  clinic_label: "Vet",
};

/** A stand-in for Twilio's Call: the test fires its events. */
function fakeCall() {
  const handlers: Record<string, ((...a: unknown[]) => void)[]> = {};
  return {
    parameters: { CallSid: "CA-made-up" } as Record<string, string>,
    on(ev: string, cb: (...a: unknown[]) => void) {
      (handlers[ev] ??= []).push(cb);
    },
    emit(ev: string, ...a: unknown[]) {
      handlers[ev]?.forEach((h) => h(...a));
    },
    disconnect: vi.fn(function (this: { emit: (e: string) => void }) {
      this.emit("disconnect");
    }),
    mute: vi.fn(),
    sendDigits: vi.fn(),
  };
}

function recorder() {
  return {
    push: vi.fn(),
    announced: vi.fn(async (_at: number) => {}),
    consent: vi.fn(async (_outcome: string, _at: number | null) => null),
    end: vi.fn(async (_secs: number, _sid: string | null) => ({ id: "2026-10-19_101500_call" }) as never),
  };
}

let call: ReturnType<typeof fakeCall>;
let rec: ReturnType<typeof recorder>;

beforeEach(() => {
  vi.clearAllMocks();
  call = fakeCall();
  rec = recorder();
  mApi.pending.mockResolvedValue(BELLA);
  mApi.token.mockResolvedValue({ token: "token", identity: "openbark_front_desk", expires_at: 0 });
  mApi.startRecording.mockResolvedValue(rec);
  mVoice.createDevice.mockResolvedValue({
    on: vi.fn(),
    updateToken: vi.fn(),
    connect: vi.fn(async () => call),
    destroy: vi.fn(),
  } as never);
});
afterEach(cleanup);

async function answered() {
  render(<PhoneApp />);
  expect(await screen.findByText("This call will be recorded and announced.")).toBeTruthy();
  expect(screen.getByText("Patel · Bella")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Call" }));
  await waitFor(() => expect(mVoice.createDevice).toHaveBeenCalled());
  await waitFor(() => expect(screen.getByText("Calling…")).toBeTruthy());
  call.emit("ringing");
  await screen.findByText("Ringing…");
  call.emit("accept");
  await waitFor(() => expect(mApi.startRecording).toHaveBeenCalled());
}

describe("the phone window", () => {
  it("records only after the client answers, shows the announcement, then asks", async () => {
    await answered();
    expect(mApi.startRecording).toHaveBeenCalledWith(
      expect.objectContaining({ number: "+15555550142", patient_id: "p-bella", call_sid: "CA-made-up", sample_rate: 48000 }),
    );
    expect(typeof mApi.startRecording.mock.calls[0]?.[0].since_answer).toBe("number");
    expect(screen.getByText("Read this to the client now:")).toBeTruthy();
    expect(screen.getByText(BELLA.announcement)).toBeTruthy();
    expect(await screen.findByText(/^Recording · /)).toBeTruthy();
    expect(rec.announced).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "I've read it" }));
    await waitFor(() => expect(rec.announced).toHaveBeenCalledWith(0));
    expect(screen.getByText("Did the client agree?")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Yes, keep recording" }));
    await waitFor(() => expect(rec.consent).toHaveBeenCalledWith("agreed", 0));
    fireEvent.click(screen.getByRole("button", { name: "Hang up" }));
    await waitFor(() => expect(rec.end).toHaveBeenCalled());
    expect(rec.end.mock.calls[0]?.[1]).toBe("CA-made-up");
    await waitFor(() => expect(mApi.announceSaved).toHaveBeenCalledWith("2026-10-19_101500_call"));
    expect(await screen.findByText(/^Call ended · 0:00\. /)).toBeTruthy();
  });

  it("deletes the recording when the client says no, and the call goes on", async () => {
    await answered();
    fireEvent.click(screen.getByRole("button", { name: "I've read it" }));
    fireEvent.click(screen.getByRole("button", { name: "No, stop recording" }));
    await waitFor(() => expect(rec.consent).toHaveBeenCalledWith("declined", null));
    expect(screen.getByText("Not recorded. The client declined.")).toBeTruthy();
    expect(screen.getByText(/^On call · .* · Not recorded$/)).toBeTruthy();
    expect(screen.getByRole("button", { name: "Hang up" })).toBeTruthy();
  });

  it("can stop and delete the recording at any time, after confirming", async () => {
    await answered();
    fireEvent.click(await screen.findByRole("button", { name: "Stop and delete the recording" }));
    expect(screen.getByText("Delete the recording so far? The call goes on, unrecorded.")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Delete the recording" }));
    await waitFor(() => expect(rec.consent).toHaveBeenCalledWith("declined", null));
  });

  it("doesn't record a call the caller chose not to record", async () => {
    render(<PhoneApp />);
    fireEvent.click(await screen.findByLabelText("Don't record this call"));
    fireEvent.click(screen.getByRole("button", { name: "Call" }));
    await waitFor(() => expect(mVoice.createDevice).toHaveBeenCalled());
    call.emit("accept");
    expect(await screen.findByText(/^On call · /)).toBeTruthy();
    expect(screen.queryByText("Read this to the client now:")).toBeNull();
    expect(mApi.startRecording).not.toHaveBeenCalled();
  });

  it("asks for the number when none is on file, and checks it", async () => {
    mApi.pending.mockResolvedValue({ ...BELLA, number: null });
    render(<PhoneApp />);
    expect(await screen.findByText("No phone number for this client. Add one, or type it on the dial pad.")).toBeTruthy();
    for (const k of ["5", "5", "5"]) fireEvent.click(screen.getByRole("button", { name: k }));
    fireEvent.click(screen.getByRole("button", { name: "Call" }));
    expect((await screen.findByRole("alert")).textContent).toContain("555 isn't a full phone number");
    expect(mVoice.createDevice).not.toHaveBeenCalled();
  });

  it("says why a call failed", async () => {
    mVoice.createDevice.mockRejectedValue({ code: 20101, message: "AccessTokenInvalid" });
    render(<PhoneApp />);
    fireEvent.click(await screen.findByRole("button", { name: "Call" }));
    expect(await screen.findByText(/Twilio didn't accept this key/)).toBeTruthy();
  });

  it("keeps the audio and the client's answer from before the engine was ready", async () => {
    let ready: (r: typeof rec) => void = () => {};
    mApi.startRecording.mockReturnValue(new Promise((r) => (ready = r)) as never);
    render(<PhoneApp />);
    fireEvent.click(await screen.findByRole("button", { name: "Call" }));
    await waitFor(() => expect(mVoice.createDevice).toHaveBeenCalled());
    call.emit("accept");
    await screen.findByText("Read this to the client now:");
    // The announcement, read while the engine loads its models.
    const announcement = new Float32Array([0.1, -0.1]);
    feedFrames(announcement);
    fireEvent.click(screen.getByRole("button", { name: "I've read it" }));
    fireEvent.click(screen.getByRole("button", { name: "No, stop recording" }));
    expect(rec.consent).not.toHaveBeenCalled();
    ready(rec);
    await waitFor(() => expect(rec.consent).toHaveBeenCalledWith("declined", null));
    // Frames after "No" aren't kept; the ones before it reached the engine first.
    expect(rec.push).not.toHaveBeenCalled();
  });

  it("sends what was recorded before the engine was ready, first", async () => {
    let ready: (r: typeof rec) => void = () => {};
    mApi.startRecording.mockReturnValue(new Promise((r) => (ready = r)) as never);
    render(<PhoneApp />);
    fireEvent.click(await screen.findByRole("button", { name: "Call" }));
    await waitFor(() => expect(mVoice.createDevice).toHaveBeenCalled());
    call.emit("accept");
    await screen.findByText("Read this to the client now:");
    const first = new Float32Array([0.5, 0.25]);
    feedFrames(first);
    ready(rec);
    await waitFor(() => expect(rec.push).toHaveBeenCalledWith(first));
    const later = new Float32Array([0.2, 0.1]);
    feedFrames(later);
    expect(rec.push).toHaveBeenLastCalledWith(later);
  });

  it("never confirms an announcement that wasn't read: hanging up first leaves the engine to delete it", async () => {
    await answered();
    fireEvent.click(screen.getByRole("button", { name: "Hang up" }));
    await waitFor(() => expect(rec.end).toHaveBeenCalled());
    expect(rec.announced).not.toHaveBeenCalled();
    expect(rec.consent).not.toHaveBeenCalled();
  });

  it("sends the announcement once the engine is ready, if it was read before", async () => {
    let ready: (r: typeof rec) => void = () => {};
    mApi.startRecording.mockReturnValue(new Promise((r) => (ready = r)) as never);
    render(<PhoneApp />);
    fireEvent.click(await screen.findByRole("button", { name: "Call" }));
    await waitFor(() => expect(mVoice.createDevice).toHaveBeenCalled());
    call.emit("accept");
    fireEvent.click(await screen.findByRole("button", { name: "I've read it" }));
    fireEvent.click(screen.getByRole("button", { name: "Yes, keep recording" }));
    ready(rec);
    await waitFor(() => expect(rec.consent).toHaveBeenCalledWith("agreed", 0));
    expect(rec.announced.mock.invocationCallOrder[0]).toBeLessThan(rec.consent.mock.invocationCallOrder[0]!);
  });

  it("says when a decline couldn't delete the recording, never 'Not recorded'", async () => {
    rec.consent.mockRejectedValue(new Error("Couldn't reach the OpenBark engine."));
    await answered();
    fireEvent.click(await screen.findByRole("button", { name: "Stop and delete the recording" }));
    fireEvent.click(screen.getByRole("button", { name: "Delete the recording" }));
    expect(await screen.findByText(/^Couldn't delete the recording\./)).toBeTruthy();
    expect(screen.queryByText("Not recorded. The client declined.")).toBeNull();
  });

  it("shows a new call chosen in the main window when the phone window gets focus", async () => {
    render(<PhoneApp />);
    expect(await screen.findByText("Patel · Bella")).toBeTruthy();
    mApi.pending.mockResolvedValue({ ...BELLA, patient_id: "p-max", patient_name: "Max", last_name: "Okafor", number: "+15555550177" });
    window.dispatchEvent(new Event("focus"));
    expect(await screen.findByText("Okafor · Max")).toBeTruthy();
    expect((screen.getByLabelText("Number") as HTMLInputElement).value).toBe("+15555550177");
  });

  it("says no answer when the call ends before the client answers", async () => {
    render(<PhoneApp />);
    fireEvent.click(await screen.findByRole("button", { name: "Call" }));
    await waitFor(() => expect(mVoice.createDevice).toHaveBeenCalled());
    await screen.findByText("Calling…");
    call.emit("disconnect");
    expect(await screen.findByText("No answer.")).toBeTruthy();
    expect(mApi.startRecording).not.toHaveBeenCalled();
  });
});
