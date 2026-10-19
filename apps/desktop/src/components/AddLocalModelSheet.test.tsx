import { createLevelSource } from "@openbark/ui";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AppContext, loadSettings, type Ctx } from "../context";
import * as engine from "../engine";
import { AddLocalModelSheet } from "./AddLocalModelSheet";

type DownloadEvent = { model: string; done: number; total: number };
const h = vi.hoisted(() => ({ desktop: false, listeners: [] as ((d: DownloadEvent) => void)[] }));

vi.mock("../engine", async (importOriginal) => {
  const real = await importOriginal<typeof import("../engine")>();
  return {
    ...real,
    get isDesktop() {
      return h.desktop;
    },
    inspectImport: vi.fn(),
    importModel: vi.fn(),
    pickModelFile: vi.fn(),
    pickModelFolder: vi.fn(),
    on: vi.fn(async (_kind: string, cb: (d: DownloadEvent) => void) => {
      h.listeners.push(cb);
      return () => {};
    }),
  };
});
const mocked = vi.mocked(engine);

const PREVIEW: engine.ImportPreview = {
  kind: "gguf",
  name: "Qwen3-4B-Q4_K_M",
  id: "local/Qwen3-4B-Q4_K_M",
  files: [{ path: "Qwen3-4B-Q4_K_M.gguf", size: 2_500_000_000 }],
  total_bytes: 2_500_000_000,
  model_type: null,
  same_drive: true,
  free_bytes: 120_000_000_000,
};
const ROW = { id: "local/Qwen3-4B-Q4_K_M", name: "Qwen3-4B-Q4_K_M" } as engine.ModelRow;

function ctx(): Ctx {
  return {
    route: { screen: "models" } as Ctx["route"],
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
    actor: "Sam",
    actorName: "Sam",
    can: { notes: true, sign: true, team: true },
  };
}

function show(mlx = false) {
  const c = ctx();
  const onAdded = vi.fn();
  render(
    <AppContext.Provider value={c}>
      <AddLocalModelSheet mlx={mlx} onClose={() => {}} onAdded={onAdded} />
    </AppContext.Provider>,
  );
  return { c, onAdded };
}

async function checkPath(path: string) {
  fireEvent.change(screen.getByLabelText("Path on the computer running OpenBark"), { target: { value: path } });
  fireEvent.click(screen.getByRole("button", { name: "Check" }));
  await screen.findByText("Qwen3-4B-Q4_K_M");
}

beforeEach(() => {
  vi.clearAllMocks();
  h.desktop = false;
  h.listeners.length = 0;
});
afterEach(cleanup);

describe("AddLocalModelSheet", () => {
  it("web: checks a path, copies by default, shows progress and adds", async () => {
    let finish: (r: engine.ModelRow) => void = () => {};
    mocked.inspectImport.mockResolvedValue(PREVIEW);
    mocked.importModel.mockReturnValue(new Promise((r) => (finish = r)));
    const { c, onAdded } = show();
    expect(screen.getByRole("heading", { name: "Add from this computer" })).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Choose a .gguf file" })).toBeNull();
    await checkPath("D:\\Models\\Qwen3-4B-Q4_K_M.gguf");
    expect(mocked.inspectImport).toHaveBeenCalledWith("D:\\Models\\Qwen3-4B-Q4_K_M.gguf");
    expect(screen.getByText("2.5 GB")).toBeTruthy();
    expect(screen.getByText("120.0 GB free on that drive")).toBeTruthy();
    const copy = screen.getByLabelText(/Copy into OpenBark's models folder \(recommended\)/) as HTMLInputElement;
    expect(copy.checked).toBe(true);
    expect(
      screen.getByText(
        "Copying keeps the model working if the original moves. On the same drive, OpenBark links it instead, so it takes no extra space.",
      ),
    ).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Add" }));
    expect(mocked.importModel).toHaveBeenCalledWith("D:\\Models\\Qwen3-4B-Q4_K_M.gguf", "copy");
    act(() => h.listeners.forEach((l) => l({ model: "local/other", done: 1, total: 9 })));
    act(() => h.listeners.forEach((l) => l({ model: PREVIEW.id, done: 1_000_000_000, total: 2_500_000_000 })));
    expect(screen.getByText("Copying 1.0 GB / 2.5 GB")).toBeTruthy();
    await act(async () => finish(ROW));
    expect(c.toast).toHaveBeenCalledWith("Model added");
    expect(onAdded).toHaveBeenCalledWith(ROW);
  });

  it("uses a model where it is when asked", async () => {
    mocked.inspectImport.mockResolvedValue(PREVIEW);
    mocked.importModel.mockResolvedValue(ROW);
    const { onAdded } = show();
    await checkPath("/Users/sam/models/Qwen3-4B-Q4_K_M.gguf");
    fireEvent.click(screen.getByLabelText(/Use it where it is, don't copy/));
    expect(
      screen.getByText("Used where it is, OpenBark never changes or deletes the file. If it moves, add it again."),
    ).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Add" }));
    await waitFor(() => expect(onAdded).toHaveBeenCalledWith(ROW));
    expect(mocked.importModel).toHaveBeenCalledWith("/Users/sam/models/Qwen3-4B-Q4_K_M.gguf", "in_place");
  });

  it("shows why a path can't be added", async () => {
    mocked.inspectImport.mockRejectedValue(
      new Error("That's a helper file (mmproj-gemma.gguf), not a model. Choose the main model file."),
    );
    show();
    fireEvent.change(screen.getByLabelText("Path on the computer running OpenBark"), {
      target: { value: "D:\\mmproj-gemma.gguf" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Check" }));
    expect((await screen.findByRole("alert")).textContent).toBe(
      "That's a helper file (mmproj-gemma.gguf), not a model. Choose the main model file.",
    );
    expect(screen.getByLabelText("Path on the computer running OpenBark")).toBeTruthy();
  });

  it("shows a refused import and keeps the preview", async () => {
    mocked.inspectImport.mockResolvedValue(PREVIEW);
    mocked.importModel.mockRejectedValue(new Error("Only a veterinarian can change the models."));
    show();
    await checkPath("D:\\m.gguf");
    fireEvent.click(screen.getByRole("button", { name: "Add" }));
    expect((await screen.findByRole("alert")).textContent).toBe("Only a veterinarian can change the models.");
    expect(screen.getByRole("button", { name: "Add" })).toBeTruthy();
  });

  it("desktop: picks a file, and an MLX folder only on a Mac or in test mode", async () => {
    h.desktop = true;
    mocked.pickModelFile.mockResolvedValue(null);
    show(false);
    expect(screen.queryByLabelText("Path on the computer running OpenBark")).toBeNull();
    expect(screen.queryByRole("button", { name: "Choose an MLX folder" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Choose a .gguf file" }));
    await waitFor(() => expect(mocked.pickModelFile).toHaveBeenCalledTimes(1));
    // Cancelled: nothing checked.
    expect(mocked.inspectImport).not.toHaveBeenCalled();
    cleanup();

    mocked.pickModelFolder.mockResolvedValue("/Users/sam/models/Qwen3-0.6B-4bit");
    mocked.inspectImport.mockResolvedValue({
      ...PREVIEW,
      kind: "mlx",
      name: "Qwen3-0.6B-4bit",
      id: "local/Qwen3-0.6B-4bit",
      model_type: "qwen3",
      files: [
        { path: "config.json", size: 1000 },
        { path: "model.safetensors", size: 335_000_000 },
        { path: "tokenizer.json", size: 11_000_000 },
      ],
      total_bytes: 346_000_000,
    });
    show(true);
    fireEvent.click(screen.getByRole("button", { name: "Choose an MLX folder" }));
    expect(await screen.findByText("Qwen3-0.6B-4bit")).toBeTruthy();
    expect(mocked.inspectImport).toHaveBeenCalledWith("/Users/sam/models/Qwen3-0.6B-4bit");
    expect(screen.getByText("346 MB · qwen3 · 3 files")).toBeTruthy();
    expect(screen.getByText("model.safetensors")).toBeTruthy();
  });
});
