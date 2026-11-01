import { createLevelSource } from "@openbark/ui";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AppContext, loadSettings, type Ctx, type Settings } from "../context";
import * as engine from "../engine";
import { E2B, E4B } from "../setup";
import { SetupCheckCard, SetupPrompt, useSpeedLine } from "./SetupCheckCard";
import { SetupWalkthrough } from "./SetupWalkthrough";

const downloaded = new Set<string>();
const order: string[] = [];

const SIZES: Record<string, [engine.Slot, number]> = {
  "silero-vad": ["vad", 643_854],
  "parakeet-tdt-0.6b-v3-int8": ["stt", 670_478_772],
  [E2B]: ["llm", 3_349_516_256],
  [E4B]: ["llm", 5_154_941_280],
};

vi.mock("../engine", async (importOriginal) => {
  const real = await importOriginal<typeof import("../engine")>();
  return {
    ...real,
    on: vi.fn().mockResolvedValue(() => {}),
    models: vi.fn(),
    storage: vi.fn(),
    gpuStatus: vi.fn(),
    downloadModel: vi.fn(),
    installGpu: vi.fn(),
    speedCheck: vi.fn(),
    cancelDraft: vi.fn(),
  };
});

const mocked = vi.mocked(engine);

function rows(): engine.ModelRow[] {
  return Object.entries(SIZES).map(
    ([id, [slot, size]]) => ({ id, name: id, slot, size_bytes: size, installed: downloaded.has(id), fit: "fits" }) as engine.ModelRow,
  );
}

/** A 16 GB laptop with four cores and no graphics card: the smaller model is recommended. */
function laptop(installed: boolean): engine.Status {
  return {
    profile: "desktop",
    platform: "windows",
    models: [
      { id: "silero-vad", name: "Silero VAD", installed },
      { id: "parakeet-tdt-0.6b-v3-int8", name: "Parakeet TDT 0.6B v3", installed },
      { id: E2B, name: "Gemma 4 E2B instruct (QAT Q4_0)", installed },
    ],
    hardware: {
      gpu: null,
      gpu_memory_bytes: null,
      ram_bytes: 16e9,
      summary: "16 GB memory, no supported graphics card",
      cpu_cores: 4,
      cpu_threads: 8,
      drafts_on: "processor",
    },
    default_llm: E2B,
    recommendation: { model: E2B, why: "few_cores", drafts_on: "processor", cores: 4, e4b_fit: "tight" },
    team: false,
  } as unknown as engine.Status;
}

const check = (model: string, secs: number): engine.SpeedCheck => ({
  model,
  load_ms: 3000,
  read_tokens: 520,
  read_ms: 16000,
  read_tokens_per_second: 32.5,
  written_tokens: 160,
  write_tokens_per_second: 6.4,
  long_visit_note_secs: secs,
  drafts_on: "processor",
});

function ctx(status: engine.Status, over: Partial<Settings> = {}): Ctx {
  return {
    route: { screen: "record" },
    navigate: vi.fn(),
    status,
    refreshStatus: vi.fn(),
    settings: { ...loadSettings(), uiLanguage: "en", consentAcknowledgedAt: "2026-10-31T09:00:00Z", ...over },
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

function show(node: React.ReactNode, c: Ctx) {
  render(<AppContext.Provider value={c}>{node}</AppContext.Provider>);
  return c;
}

const foot = () => within(document.querySelector(".setup-walk-foot") as HTMLElement);
const press = (name: string | RegExp) => fireEvent.click(foot().getByRole("button", { name }));
const title = () => screen.getByRole("heading", { level: 1 }).textContent;
/** The model rows have loaded: each choice shows its size or Downloaded. */
const sized = () => screen.findAllByText(/^(\d+\.\d GB download|Downloaded)$/);

beforeEach(() => {
  vi.clearAllMocks();
  downloaded.clear();
  order.length = 0;
  mocked.models.mockImplementation(async () => rows());
  mocked.storage.mockResolvedValue({ models: { free_bytes: 120e9 } } as unknown as engine.StorageView);
  mocked.gpuStatus.mockResolvedValue({ supported: false } as engine.GpuStatus);
  mocked.downloadModel.mockImplementation(async (id: string) => {
    order.push(id);
    downloaded.add(id);
  });
  mocked.cancelDraft.mockResolvedValue(true);
});
afterEach(cleanup);

describe("Set up OpenBark (ADR-0034)", () => {
  it("1. says what this computer has and what it means", async () => {
    const c = show(<SetupWalkthrough mode="full" />, ctx(laptop(false)));
    // Opened on a first run, it stays open until finished or skipped.
    expect(c.setSettings).toHaveBeenCalledWith({ showSetup: "full" });
    expect(title()).toBe("This computer");
    expect(screen.getByText("Step 1 of 6")).toBeTruthy();
    expect(screen.getByText("16 GB")).toBeTruthy();
    expect(screen.getByText("4 physical cores (8 threads)")).toBeTruthy();
    expect(screen.getByText("No supported graphics card")).toBeTruthy();
    expect(await screen.findByText("120.0 GB free where models are kept")).toBeTruthy();
    expect(screen.getByText("Windows")).toBeTruthy();
    expect(screen.getByText(/Notes are written on the processor/)).toBeTruthy();
  });

  it("2. recommends a note model with the reason for this computer, and the other can be chosen", async () => {
    const c = show(<SetupWalkthrough mode="full" />, ctx(laptop(false)));
    press("Continue");
    expect(title()).toBe("The note model");
    expect(screen.getByTestId("setup-reason").textContent).toContain(
      "16 GB of memory but no graphics card, and 4 processor cores: the smaller model writes notes about 1.6 times as fast on a processor",
    );
    const e2b = screen.getByRole("radio", { name: /Gemma 4 E2B/ });
    const e4b = screen.getByRole("radio", { name: /Gemma 4 E4B/ });
    expect((e2b as HTMLInputElement).checked).toBe(true);
    // The badge is on the recommended one only, with each download size.
    expect(within(e2b.closest("label") as HTMLElement).getByText("Recommended")).toBeTruthy();
    expect(within(e4b.closest("label") as HTMLElement).queryByText("Recommended")).toBeNull();
    expect(await screen.findByText("3.3 GB download")).toBeTruthy();
    expect(screen.getByText("5.2 GB download")).toBeTruthy();
    expect(screen.getByText(/Less good: slower, and needs more memory/)).toBeTruthy();
    // Choosing the other one.
    fireEvent.click(e4b);
    press("Continue");
    expect(c.setSettings).toHaveBeenCalledWith({ llm: E4B, noteModelChosenAt: expect.any(String) });
    expect(title()).toBe("Speech models");
  });

  it("3. explains the speech models, and offers no GPU pack without an NVIDIA card on Windows", async () => {
    show(<SetupWalkthrough mode="full" />, ctx(laptop(false)));
    press("Continue");
    await screen.findByText("3.3 GB download");
    press("Continue");
    expect(screen.getByText(/They run on this computer, and recordings are never sent anywhere/)).toBeTruthy();
    expect(screen.getByText("Turns speech into the transcript.")).toBeTruthy();
    expect(screen.getByText("Finds where someone is speaking, so silence is skipped.")).toBeTruthy();
    expect(screen.queryByText(/GPU speech pack/)).toBeNull();
  });

  it("3. offers the GPU speech pack on an NVIDIA card, after the licence, and downloads it with the rest", async () => {
    mocked.gpuStatus.mockResolvedValue({
      supported: true,
      gpu: "NVIDIA GeForce RTX 3060",
      pack_installed: false,
      models_installed: false,
      download_bytes: 2e9,
    } as engine.GpuStatus);
    mocked.installGpu.mockImplementation(async () => {
      order.push("gpu-speech-pack");
      return { supported: true } as engine.GpuStatus;
    });
    show(<SetupWalkthrough mode="full" />, ctx(laptop(false)));
    press("Continue");
    await screen.findByText("3.3 GB download");
    press("Continue");
    expect(await screen.findByText(/Your NVIDIA GeForce RTX 3060 can do the speech recognition/)).toBeTruthy();
    fireEvent.click(screen.getByRole("checkbox", { name: "Also download the GPU speech pack (2.0 GB)" }));
    expect((foot().getByRole("button", { name: "Continue" }) as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(screen.getByRole("checkbox", { name: /I accept NVIDIA's licence/ }));
    press("Continue");
    press(/Download everything/);
    await waitFor(() => expect(order).toEqual(["silero-vad", "parakeet-tdt-0.6b-v3-int8", "gpu-speech-pack", E2B]));
    expect(mocked.installGpu).toHaveBeenCalledWith(true);
    expect(await screen.findByText("Downloaded. Restart OpenBark to start using the graphics card.")).toBeTruthy();
  });

  it("5. downloads smallest first, each step timed, then measures the speed and suggests the smaller model when notes are slow", async () => {
    mocked.speedCheck.mockResolvedValue(check(E4B, 840));
    const c = show(<SetupWalkthrough mode="full" />, ctx(laptop(false)));
    press("Continue");
    await sized();
    fireEvent.click(screen.getByRole("radio", { name: /Gemma 4 E4B/ }));
    press("Continue");
    press("Continue");
    expect(title()).toBe("Download");
    expect(screen.getByText(/Smallest first/)).toBeTruthy();
    // The chosen model takes the default's place: 0.6 MB + 670 MB + 5.15 GB.
    press("Download everything (5.8 GB)");
    await waitFor(() => expect(order).toEqual(["silero-vad", "parakeet-tdt-0.6b-v3-int8", E4B]));
    expect(await screen.findByText("Models ready")).toBeTruthy();
    expect(screen.getAllByText(/^Done in /)).toHaveLength(3);
    expect(c.refreshStatus).toHaveBeenCalled();
    press("Continue");
    expect(title()).toBe("Speed check");
    expect(await screen.findByText(/Optional\. Gemma 4 E4B writes a short practice note from an invented consult/)).toBeTruthy();
    press("Check the speed");
    await waitFor(() => expect(mocked.speedCheck).toHaveBeenCalledWith(E4B));
    expect(
      await screen.findByText("On this computer, a note for a long visit will take about 14 minutes with Gemma 4 E4B."),
    ).toBeTruthy();
    expect(c.setSettings).toHaveBeenCalledWith({
      speedCheck: { model: E4B, read: 32.5, write: 6.4, noteSecs: 840, at: expect.any(String) },
    });
    // Over five minutes with the larger model: the smaller one, with the reason.
    expect(screen.getByText(/That's a long wait\. Gemma 4 E2B writes notes about 1\.6 times as fast/)).toBeTruthy();
    expect(screen.getByRole("button", { name: "Download Gemma 4 E2B (3.3 GB)" })).toBeTruthy();
    press("Continue");
    // 6. Recording tips, then Record; finishing is remembered.
    expect(title()).toBe("Recording tips");
    expect(screen.getByText("Speakers in the room: choose 2 for a vet and a client. It's on the Record screen.")).toBeTruthy();
    expect(screen.getByText(/USB or clip-on microphone/)).toBeTruthy();
    expect(screen.getByText(/Review every draft before you sign it/)).toBeTruthy();
    press("Finish");
    expect(c.setSettings).toHaveBeenCalledWith(
      expect.objectContaining({ setupDoneAt: expect.any(String), showSetup: null }),
    );
    expect(c.navigate).toHaveBeenCalledWith({ screen: "record" });
  });

  it("makes no suggestion when the recommended model is quick enough", async () => {
    downloaded.add("silero-vad").add("parakeet-tdt-0.6b-v3-int8").add(E2B);
    mocked.speedCheck.mockResolvedValue(check(E2B, 200));
    show(<SetupWalkthrough mode="full" />, ctx(laptop(true)));
    press("Continue");
    await screen.findAllByText("Downloaded");
    press("Continue");
    press("Continue");
    expect(screen.getByText("Everything is already on this computer.")).toBeTruthy();
    press("Continue");
    press("Check the speed");
    expect(
      await screen.findByText("On this computer, a note for a long visit will take about 3 minutes with Gemma 4 E2B."),
    ).toBeTruthy();
    expect(screen.queryByText(/That's a long wait/)).toBeNull();
  });

  it("can skip the speed check, and Stop ends one that's running", async () => {
    downloaded.add("silero-vad").add("parakeet-tdt-0.6b-v3-int8").add(E2B);
    let reject: (e: Error) => void = () => {};
    mocked.speedCheck.mockImplementation(() => new Promise((_, r) => (reject = r)));
    mocked.cancelDraft.mockImplementation(async () => {
      reject(new Error(engine.DRAFT_STOPPED));
      return true;
    });
    show(<SetupWalkthrough mode="full" />, ctx(laptop(true)));
    press("Continue");
    await screen.findAllByText("Downloaded");
    press("Continue");
    press("Continue");
    press("Continue");
    press("Check the speed");
    expect(await screen.findByText("Checking the speed of Gemma 4 E2B")).toBeTruthy();
    expect(screen.getAllByText("Loading the note model").length).toBeGreaterThan(0);
    expect(screen.getByText("Reading a practice consult")).toBeTruthy();
    expect(screen.getByText("Writing a practice note")).toBeTruthy();
    // Closing is held back while it runs.
    expect((screen.getByRole("button", { name: "Skip setup" }) as HTMLButtonElement).disabled).toBe(true);
    press("Stop");
    expect(mocked.cancelDraft).toHaveBeenCalled();
    expect(await screen.findByText(/Speed check stopped/)).toBeTruthy();
    press("Continue");
    expect(title()).toBe("Recording tips");
  });

  it("goes straight to the tips when the speed check is skipped", async () => {
    downloaded.add("silero-vad").add("parakeet-tdt-0.6b-v3-int8").add(E2B);
    show(<SetupWalkthrough mode="full" />, ctx(laptop(true)));
    press("Continue");
    await screen.findAllByText("Downloaded");
    press("Continue");
    press("Continue");
    press("Continue");
    press("Skip the speed check");
    expect(mocked.speedCheck).not.toHaveBeenCalled();
    expect(title()).toBe("Recording tips");
  });

  it("can be skipped from the first step, and the skip is remembered", () => {
    const c = show(<SetupWalkthrough mode="full" />, ctx(laptop(false)));
    fireEvent.click(screen.getByRole("button", { name: "Skip setup" }));
    expect(c.setSettings).toHaveBeenCalledWith(expect.objectContaining({ setupDoneAt: expect.any(String) }));
    expect(mocked.downloadModel).not.toHaveBeenCalled();
  });
});

describe("Check my setup (an existing install)", () => {
  const existing = () => {
    downloaded.add("silero-vad").add("parakeet-tdt-0.6b-v3-int8").add(E2B).add(E4B);
    return ctx(laptop(true), { llm: E4B, showSetup: "check" });
  };

  it("runs the computer, the note model and the speed check, and changes nothing by itself", async () => {
    mocked.speedCheck.mockResolvedValue(check(E4B, 420));
    const c = show(<SetupWalkthrough mode="check" />, existing());
    expect(screen.getByText("Check my setup")).toBeTruthy();
    expect(screen.getByText("Step 1 of 3")).toBeTruthy();
    press("Continue");
    await sized();
    // The model in use stays chosen; the recommendation is shown beside it.
    expect((screen.getByRole("radio", { name: /Gemma 4 E4B/ }) as HTMLInputElement).checked).toBe(true);
    expect(within(screen.getByRole("radio", { name: /Gemma 4 E2B/ }).closest("label") as HTMLElement).getByText("Recommended")).toBeTruthy();
    press("Continue");
    expect(title()).toBe("Speed check");
    expect(c.setSettings).not.toHaveBeenCalledWith(expect.objectContaining({ llm: expect.anything() }));
    press("Check the speed");
    expect(await screen.findByText(/about 7 minutes with Gemma 4 E4B/)).toBeTruthy();
    // A suggestion only: the model changes when the person presses it.
    expect(screen.getByText(/That's a long wait/)).toBeTruthy();
    expect(c.setSettings).not.toHaveBeenCalledWith(expect.objectContaining({ llm: expect.anything() }));
    fireEvent.click(screen.getByRole("button", { name: "Use Gemma 4 E2B" }));
    expect(c.setSettings).toHaveBeenCalledWith({ llm: E2B, noteModelChosenAt: expect.any(String) });
  });

  it("switches only to a model the person picks", async () => {
    const c = show(<SetupWalkthrough mode="check" />, existing());
    press("Continue");
    await sized();
    fireEvent.click(screen.getByRole("radio", { name: /Gemma 4 E2B/ }));
    expect(c.setSettings).not.toHaveBeenCalledWith(expect.objectContaining({ llm: expect.anything() }));
    press("Continue");
    expect(c.setSettings).toHaveBeenCalledWith({ llm: E2B, noteModelChosenAt: expect.any(String) });
    press("Skip the speed check");
    expect(c.setSettings).toHaveBeenCalledWith(
      expect.objectContaining({ setupDoneAt: expect.any(String), setupCardDismissedAt: expect.any(String) }),
    );
  });

  it("downloads the other model first when it isn't on this computer", async () => {
    downloaded.add("silero-vad").add("parakeet-tdt-0.6b-v3-int8").add(E4B);
    const c = ctx(laptop(true), { llm: E4B, showSetup: "check" });
    show(<SetupWalkthrough mode="check" />, c);
    press("Continue");
    await sized();
    fireEvent.click(screen.getByRole("radio", { name: /Gemma 4 E2B/ }));
    press("Download Gemma 4 E2B (3.3 GB)");
    expect(title()).toBe("Download");
    expect(screen.getByText("Step 3 of 4")).toBeTruthy();
    press("Download everything (3.3 GB)");
    await waitFor(() => expect(order).toEqual([E2B]));
    await waitFor(() => expect(c.setSettings).toHaveBeenCalledWith({ llm: E2B, noteModelChosenAt: expect.any(String) }));
  });
});

describe("The card for existing installs", () => {
  it("offers Check my setup once, and Not now dismisses it for good", () => {
    const c = show(<SetupCheckCard />, ctx(laptop(true)));
    expect(screen.getByText(/New in this version: OpenBark can check this computer/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Check my setup" }));
    expect(c.setSettings).toHaveBeenCalledWith({ showSetup: "check" });
    fireEvent.click(screen.getByRole("button", { name: "Not now" }));
    expect(c.setSettings).toHaveBeenCalledWith({ setupCardDismissedAt: expect.any(String) });
  });

  it("isn't shown once setup was done or the card dismissed", () => {
    show(<SetupCheckCard />, ctx(laptop(true), { setupCardDismissedAt: "2026-10-31T10:00:00Z" }));
    expect(screen.queryByRole("button", { name: "Check my setup" })).toBeNull();
    cleanup();
    show(<SetupCheckCard />, ctx(laptop(true), { setupDoneAt: "2026-10-31T10:00:00Z" }));
    expect(screen.queryByRole("button", { name: "Check my setup" })).toBeNull();
  });

  it("Record without models leads back into setup", () => {
    const c = show(<SetupPrompt />, ctx(laptop(false), { setupDoneAt: "2026-10-31T10:00:00Z" }));
    fireEvent.click(screen.getByRole("button", { name: "Set up OpenBark" }));
    expect(c.setSettings).toHaveBeenCalledWith({ showSetup: "full" });
  });
});

function SpeedLine() {
  return <p data-testid="speed">{useSpeedLine()}</p>;
}

describe("The speed check, afterwards", () => {
  const saved = { model: E2B, read: 33, write: 10.8, noteSecs: 537, at: "2026-10-31T10:00:00Z" };

  it("is shown on Models and Record for the note model it measured", () => {
    show(<SpeedLine />, ctx(laptop(true), { llm: E2B, speedCheck: saved }));
    expect(screen.getByTestId("speed").textContent).toBe(
      "On this computer, a long visit's note takes about 9 minutes (speed check).",
    );
  });

  it("isn't shown for another note model", () => {
    show(<SpeedLine />, ctx(laptop(true), { llm: E4B, speedCheck: saved }));
    expect(screen.getByTestId("speed").textContent).toBe("");
  });
});
