import { createLevelSource } from "@openbark/ui";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AppContext, loadSettings, type Ctx } from "../context";
import * as engine from "../engine";
import { MlxCard } from "./MlxCard";

type DownloadEvent = { model: string; done: number; total: number };
const listeners: ((d: DownloadEvent) => void)[] = [];

vi.mock("../engine", async (importOriginal) => {
  const real = await importOriginal<typeof import("../engine")>();
  return {
    ...real,
    mlxStatus: vi.fn(),
    installMlx: vi.fn(),
    checkMlx: vi.fn(),
    removeMlx: vi.fn(),
    on: vi.fn(async (_kind: string, cb: (d: DownloadEvent) => void) => {
      listeners.push(cb);
      return () => {};
    }),
  };
});
const mocked = vi.mocked(engine);

const status = (over: Partial<engine.MlxStatus> = {}): engine.MlxStatus => ({
  support: "yes",
  macos_version: "15.4",
  pack_installed: false,
  installing: false,
  check: "not_run",
  check_message: null,
  metal: true,
  download_bytes: 106_000_000,
  installed_bytes: 0,
  versions: { mlx: "0.32.3" },
  in_use: false,
  ...over,
});

function ctx(): Ctx {
  return {
    route: { screen: "models" } as Ctx["route"],
    navigate: vi.fn(),
    status: null,
    refreshStatus: vi.fn(),
    settings: { ...loadSettings(), uiLanguage: "en", llm: "hf:mlx-community/Qwen3-0.6B-4bit/@mlx" },
    setSettings: vi.fn(),
    toast: vi.fn(),
    announce: vi.fn(),
    levels: createLevelSource(),
    user: null,
    setUser: vi.fn(),
    actor: "Sam",
    actorName: "Sam",
    can: { notes: true, sign: true, team: true },
  };
}

function show(s: engine.MlxStatus) {
  mocked.mlxStatus.mockResolvedValue(s);
  const c = ctx();
  const view = render(
    <AppContext.Provider value={c}>
      <MlxCard />
    </AppContext.Provider>,
  );
  return { c, view };
}

const emit = (d: DownloadEvent) => act(() => listeners.forEach((l) => l(d)));

beforeEach(() => {
  vi.clearAllMocks();
  listeners.length = 0;
});
afterEach(cleanup);

describe("MlxCard", () => {
  it("is hidden where MLX can't run and it isn't a Mac", async () => {
    const { view } = show(status({ support: "not_mac" }));
    await waitFor(() => expect(mocked.mlxStatus).toHaveBeenCalled());
    expect(view.container.textContent).toBe("");
  });

  it("explains an Intel Mac", async () => {
    show(status({ support: "needs_apple_silicon" }));
    expect(
      await screen.findByText("MLX needs a Mac with Apple silicon (M1 or later). GGUF models work on this Mac."),
    ).toBeTruthy();
    expect(screen.queryByRole("button")).toBeNull();
  });

  it("explains macOS 13, with the version", async () => {
    show(status({ support: "needs_macos14", macos_version: "13.6" }));
    expect(
      await screen.findByText(
        "MLX needs macOS 14 Sonoma or later. This Mac has macOS 13.6. GGUF models work on this Mac.",
      ),
    ).toBeTruthy();
  });

  it("downloads, shows progress, then setting up, then ready", async () => {
    let finish: (s: engine.MlxStatus) => void = () => {};
    mocked.installMlx.mockReturnValue(new Promise((r) => (finish = r)));
    show(status());
    expect(
      await screen.findByText(
        "Runs MLX models from Hugging Face on this Mac's graphics. GGUF models keep using the built-in engine.",
      ),
    ).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Download (106 MB)" }));
    expect(mocked.installMlx).toHaveBeenCalledTimes(1);
    emit({ model: "something-else", done: 1, total: 2 });
    emit({ model: "mlx-engine-pack", done: 53_000_000, total: 106_000_000 });
    expect(screen.getByText("Downloading 53 MB / 106 MB")).toBeTruthy();
    expect(screen.getByText("50%")).toBeTruthy();
    emit({ model: "mlx-engine-pack", done: 106_000_000, total: 106_000_000 });
    expect(screen.getByText("Setting up")).toBeTruthy();
    await act(async () => finish(status({ pack_installed: true, check: "passed", installed_bytes: 366_000_000 })));
    expect(screen.getByText("Ready")).toBeTruthy();
    expect(screen.getByText("366 MB on disk")).toBeTruthy();
  });

  it("check failed: says so and tries again", async () => {
    mocked.checkMlx.mockResolvedValue(status({ pack_installed: true, check: "passed", installed_bytes: 366_000_000 }));
    show(status({ pack_installed: true, check: "failed", check_message: "Metal isn't available." }));
    expect(
      await screen.findByText(
        "The MLX engine didn't pass its check on this Mac, so MLX models can't write notes. GGUF models still work.",
      ),
    ).toBeTruthy();
    expect(screen.getByText("Metal isn't available.")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Try again" }));
    await waitFor(() => expect(screen.getByText("Ready")).toBeTruthy());
    expect(mocked.checkMlx).toHaveBeenCalledTimes(1);
  });

  it("installed but not checked on this version: checks once by itself", async () => {
    let finish: (s: engine.MlxStatus) => void = () => {};
    mocked.checkMlx.mockReturnValue(new Promise((r) => (finish = r)));
    show(status({ pack_installed: true, check: "not_run", installed_bytes: 1 }));
    expect(await screen.findByText("Checking")).toBeTruthy();
    await act(async () => finish(status({ pack_installed: true, check: "passed", installed_bytes: 1 })));
    expect(screen.getByText("Ready")).toBeTruthy();
    expect(mocked.checkMlx).toHaveBeenCalledTimes(1);
  });

  it("removes through the dialog, sending the note model", async () => {
    mocked.removeMlx.mockResolvedValue({
      id: "mlx-engine-pack",
      freed_bytes: 366_000_000,
      files_removed: 40,
      forgotten: false,
      kept_in_place: false,
    });
    const { c } = show(status({ pack_installed: true, check: "passed", installed_bytes: 366_000_000 }));
    fireEvent.click(await screen.findByRole("button", { name: "Remove" }));
    expect(screen.getByRole("heading", { name: "Remove the MLX engine?" })).toBeTruthy();
    mocked.mlxStatus.mockResolvedValue(status());
    fireEvent.click(screen.getAllByRole("button", { name: "Remove" }).at(-1)!);
    await waitFor(() => expect(c.toast).toHaveBeenCalledWith("Removed. 366 MB freed."));
    expect(mocked.removeMlx).toHaveBeenCalledWith("hf:mlx-community/Qwen3-0.6B-4bit/@mlx");
    expect(await screen.findByRole("button", { name: "Download (106 MB)" })).toBeTruthy();
  });

  it("marks test mode", async () => {
    show(status({ support: "test_mode", metal: false }));
    expect(await screen.findByText("Test mode")).toBeTruthy();
    expect(screen.getByText("MLX on the processor, for testing only. It's slow.")).toBeTruthy();
  });

  it("shows why the download failed", async () => {
    mocked.installMlx.mockRejectedValue(new Error("Only a veterinarian can change the models."));
    show(status());
    fireEvent.click(await screen.findByRole("button", { name: "Download (106 MB)" }));
    expect((await screen.findByRole("alert")).textContent).toBe("Only a veterinarian can change the models.");
  });
});
