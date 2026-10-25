import { createLevelSource } from "@openbark/ui";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AppContext, loadSettings, type Ctx } from "../context";
import * as engine from "../engine";
import { PhoneDrafting } from "./PhoneDrafting";

vi.mock("../engine", async (importOriginal) => {
  const real = await importOriginal<typeof import("../engine")>();
  return { ...real, on: vi.fn().mockResolvedValue(() => {}), models: vi.fn(), downloadModel: vi.fn() };
});

const mocked = vi.mocked(engine);

const qwen = {
  id: "qwen3-1.7b-q4_k_m",
  name: "Qwen3 1.7B (Q4_K_M, for iPhones with 6 GB)",
  publisher: "unsloth",
  slot: "llm",
  size_bytes: 1_107_409_472,
  license: "Apache-2.0",
  installed: false,
  fit: "fits",
  builtin: true,
  disk_bytes: 0,
  shared_bytes: 0,
  partial_bytes: 0,
  origin: "builtin",
  engine: "llama",
  parts: 1,
  remove: "delete",
  missing: false,
  gated: false,
  path: null,
} as engine.ModelRow;

function ctx(on: boolean, tier: number | null): Ctx {
  return {
    route: { screen: "settings" },
    navigate: vi.fn(),
    status: { profile: "ios", mobile_tier: tier, default_llm: tier ? qwen.id : "", models: [] } as unknown as engine.Status,
    refreshStatus: vi.fn(),
    settings: { ...loadSettings(), uiLanguage: "en", writeNotesHere: on },
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
      <ul>
        <PhoneDrafting />
      </ul>
    </AppContext.Provider>,
  );
  return c;
}

beforeEach(() => vi.clearAllMocks());
afterEach(cleanup);

describe("Write notes on this iPhone (ADR-0018 section 10)", () => {
  it("offers nothing while it's off", () => {
    mocked.models.mockResolvedValue([qwen]);
    const c = show(ctx(false, 6));
    expect(screen.queryByTestId("phone-drafting-offer")).toBeNull();
    fireEvent.click(screen.getByRole("checkbox", { name: "Write notes on this iPhone" }));
    expect(c.setSettings).toHaveBeenCalledWith({ writeNotesHere: true });
  });

  it("offers the tier's model with its size, and downloads it", async () => {
    mocked.models.mockResolvedValue([qwen]);
    mocked.downloadModel.mockResolvedValue(undefined);
    const c = show(ctx(true, 6));
    expect(
      await screen.findByText("Notes on this iPhone are written by Qwen3 1.7B, a 1.1 GB download. Wi-Fi is recommended."),
    ).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Download (1.1 GB)" }));
    await waitFor(() => expect(mocked.downloadModel).toHaveBeenCalledWith(qwen.id));
    await waitFor(() => expect(c.setSettings).toHaveBeenCalledWith({ llm: qwen.id }));
    expect(c.toast).toHaveBeenCalledWith("Model downloaded");
    // The other models that fit stay in Models.
    await waitFor(() => expect(screen.queryByTestId("phone-drafting-offer")).toBeNull());
  });

  it("says nothing more once the model is on the phone", async () => {
    mocked.models.mockResolvedValue([{ ...qwen, installed: true }]);
    show(ctx(true, 6));
    await waitFor(() => expect(mocked.models).toHaveBeenCalled());
    expect(screen.queryByTestId("phone-drafting-offer")).toBeNull();
  });

  it("explains a phone too small to write notes", () => {
    mocked.models.mockResolvedValue([]);
    show(ctx(true, null));
    expect(screen.getByText("OpenBark needs an iPhone with 6 GB of memory or more to write notes on the phone.")).toBeTruthy();
  });
});
