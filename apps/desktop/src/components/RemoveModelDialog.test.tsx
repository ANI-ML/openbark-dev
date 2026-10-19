import { createLevelSource } from "@openbark/ui";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AppContext, loadSettings, type Ctx } from "../context";
import * as engine from "../engine";
import { RemoveModelDialog, type RemoveTarget } from "./RemoveModelDialog";

vi.mock("../engine", async (importOriginal) => {
  const real = await importOriginal<typeof import("../engine")>();
  return { ...real, removeModel: vi.fn(), removeMlx: vi.fn(), removeGpu: vi.fn() };
});
const mocked = vi.mocked(engine);

const NOTE_MODEL = "gemma-4-e4b-it-q4_0";

const row = (over: Partial<engine.ModelRow>): engine.ModelRow => ({
  id: "gemma-4-12b",
  name: "Gemma 4 12B",
  publisher: "google",
  slot: "llm",
  size_bytes: 6_980_000_000,
  license: "Apache-2.0",
  installed: true,
  fit: "fits",
  builtin: true,
  disk_bytes: 6_980_000_000,
  partial_bytes: 0,
  origin: "builtin",
  engine: "llama",
  parts: 1,
  remove: "delete",
  missing: false,
  gated: false,
  path: null,
  ...over,
});

const removal = (over: Partial<engine.Removal> = {}): engine.Removal => ({
  id: "gemma-4-12b",
  freed_bytes: 6_980_000_000,
  files_removed: 1,
  forgotten: false,
  kept_in_place: false,
  ...over,
});

function ctx(lang: string): Ctx {
  return {
    route: { screen: "models" } as Ctx["route"],
    navigate: vi.fn(),
    status: null,
    refreshStatus: vi.fn(),
    settings: { ...loadSettings(), uiLanguage: lang as Ctx["settings"]["uiLanguage"], llm: NOTE_MODEL },
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

function show(target: RemoveTarget, lang = "en") {
  const onClose = vi.fn();
  const onRemoved = vi.fn();
  const toast = vi.fn();
  render(
    <AppContext.Provider value={ctx(lang)}>
      <RemoveModelDialog
        target={target}
        noteModel={NOTE_MODEL}
        onClose={onClose}
        onRemoved={onRemoved}
        toast={toast}
      />
    </AppContext.Provider>,
  );
  return { onClose, onRemoved, toast };
}

const confirm = (name: string) => fireEvent.click(screen.getByRole("button", { name }));

beforeEach(() => vi.clearAllMocks());
afterEach(cleanup);

describe("RemoveModelDialog", () => {
  it("built-in: frees its size, can come back, no list checkbox", async () => {
    mocked.removeModel.mockResolvedValue(removal());
    const { toast, onRemoved, onClose } = show({ kind: "model", row: row({}) });
    expect(screen.getByRole("heading", { name: "Remove Gemma 4 12B?" })).toBeTruthy();
    expect(
      screen.getByText("This frees 7.0 GB on this computer. You can download it again from the catalog."),
    ).toBeTruthy();
    expect(screen.queryByLabelText("Also remove it from the list")).toBeNull();
    confirm("Remove");
    await waitFor(() => expect(toast).toHaveBeenCalledWith("Removed. 7.0 GB freed."));
    expect(mocked.removeModel).toHaveBeenCalledWith("gemma-4-12b", NOTE_MODEL, false);
    expect(onRemoved).toHaveBeenCalledWith(expect.objectContaining({ message: "Removed. 7.0 GB freed." }));
    await waitFor(() => expect(onClose).toHaveBeenCalled());
  });

  it("Hugging Face: can also drop it from the list", async () => {
    mocked.removeModel.mockResolvedValue(removal({ id: "hf:a/b/c", freed_bytes: 4_900_000_000, forgotten: true }));
    const { toast } = show({
      kind: "model",
      row: row({ id: "hf:a/b/c", name: "B Q4_K_M", origin: "hub", builtin: false, disk_bytes: 4_900_000_000 }),
    });
    fireEvent.click(screen.getByLabelText("Also remove it from the list"));
    confirm("Remove");
    await waitFor(() => expect(toast).toHaveBeenCalledWith("Removed. 4.9 GB freed."));
    expect(mocked.removeModel).toHaveBeenCalledWith("hf:a/b/c", NOTE_MODEL, true);
  });

  it("Hugging Face with nothing freed: removed from the list", async () => {
    mocked.removeModel.mockResolvedValue(
      removal({ id: "hf:a/b/c", freed_bytes: 0, files_removed: 0, forgotten: true }),
    );
    const { toast } = show({
      kind: "model",
      row: row({
        id: "hf:a/b/c",
        name: "B",
        origin: "hub",
        builtin: false,
        installed: false,
        disk_bytes: 0,
        partial_bytes: 1_000_000,
      }),
    });
    fireEvent.click(screen.getByLabelText("Also remove it from the list"));
    confirm("Remove");
    await waitFor(() => expect(toast).toHaveBeenCalledWith("Removed from the list."));
  });

  it("copied from this computer: deletes the copy, the original stays", async () => {
    mocked.removeModel.mockResolvedValue(removal({ id: "local/m", freed_bytes: 2_000_000_000, forgotten: true }));
    const { toast } = show({
      kind: "model",
      row: row({
        id: "local/m",
        name: "m",
        origin: "local_copy",
        publisher: "This computer",
        builtin: false,
        remove: "delete_and_forget",
        disk_bytes: 2_000_000_000,
      }),
    });
    expect(screen.getByRole("heading", { name: "Remove m?" })).toBeTruthy();
    expect(
      screen.getByText("This deletes OpenBark's copy and frees 2.0 GB. The original file stays where it was."),
    ).toBeTruthy();
    expect(screen.queryByLabelText("Also remove it from the list")).toBeNull();
    confirm("Remove");
    await waitFor(() => expect(toast).toHaveBeenCalledWith("Removed. 2.0 GB freed."));
  });

  it("used where it is: forgets, deletes nothing", async () => {
    mocked.removeModel.mockResolvedValue(
      removal({ id: "local/llama", freed_bytes: 0, files_removed: 0, forgotten: true, kept_in_place: true }),
    );
    const { toast } = show({
      kind: "model",
      row: row({
        id: "local/llama",
        name: "llama",
        origin: "local_in_place",
        builtin: false,
        remove: "forget_only",
        disk_bytes: 0,
        path: "D:/models/llama.gguf",
      }),
    });
    expect(screen.getByRole("heading", { name: "Forget llama?" })).toBeTruthy();
    expect(
      screen.getByText("OpenBark will stop using this model. The file stays where it is, and nothing is deleted."),
    ).toBeTruthy();
    confirm("Forget");
    await waitFor(() => expect(toast).toHaveBeenCalledWith("Forgotten. Nothing was deleted."));
    expect(mocked.removeModel).toHaveBeenCalledWith("local/llama", NOTE_MODEL, false);
  });

  it("a stopped download: deletes what came so far", async () => {
    mocked.removeModel.mockResolvedValue(removal({ id: "gemma-4-e2b", freed_bytes: 1_200_000_000 }));
    const { toast } = show({
      kind: "model",
      row: row({
        id: "gemma-4-e2b",
        name: "Gemma 4 E2B",
        installed: false,
        disk_bytes: 0,
        partial_bytes: 1_200_000_000,
      }),
    });
    expect(screen.getByText("This deletes the 1.2 GB downloaded so far.")).toBeTruthy();
    confirm("Remove");
    await waitFor(() => expect(toast).toHaveBeenCalledWith("Removed. 1.2 GB freed."));
  });

  it("the MLX engine: MLX models stay but can't write notes", async () => {
    mocked.removeMlx.mockResolvedValue(removal({ id: "mlx-engine-pack", freed_bytes: 366_000_000 }));
    const { toast } = show({ kind: "mlx", bytes: 366_000_000 });
    expect(screen.getByRole("heading", { name: "Remove the MLX engine?" })).toBeTruthy();
    expect(
      screen.getByText(
        "This frees 366 MB. MLX models stay on this computer, but can't write notes until you download the engine again.",
      ),
    ).toBeTruthy();
    confirm("Remove");
    await waitFor(() => expect(toast).toHaveBeenCalledWith("Removed. 366 MB freed."));
    expect(mocked.removeMlx).toHaveBeenCalledWith(NOTE_MODEL);
  });

  it("the GPU speech pack: says what it frees", async () => {
    const gpu = { active: false } as unknown as engine.GpuStatus;
    mocked.removeGpu.mockResolvedValue(gpu);
    const { toast, onRemoved } = show({ kind: "gpu", bytes: 2_100_000_000 });
    expect(screen.getByText(/This frees 2\.1 GB\./)).toBeTruthy();
    confirm("Remove");
    await waitFor(() => expect(toast).toHaveBeenCalledWith("Removed. 2.1 GB freed."));
    expect(onRemoved).toHaveBeenCalledWith(expect.objectContaining({ gpu, removal: null }));
  });

  it.each([
    "This model writes your notes. Choose another one under In use first.",
    "A note is being written with this model. Remove it when that's finished.",
    "This model is still downloading.",
    "OpenBark needs this to transcribe visits.",
    "Only a veterinarian can change the models.",
    "OpenBark won't delete D:/elsewhere/m.gguf: it's outside the models folder.",
  ])("shows the engine's refusal inline and stays open: %s", async (message) => {
    mocked.removeModel.mockRejectedValue(new Error(message));
    const { toast, onClose, onRemoved } = show({ kind: "model", row: row({}) });
    confirm("Remove");
    expect((await screen.findByRole("alert")).textContent).toBe(message);
    expect(toast).not.toHaveBeenCalled();
    expect(onRemoved).not.toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();
    expect(screen.getByRole("heading", { name: "Remove Gemma 4 12B?" })).toBeTruthy();
    expect((screen.getByRole("button", { name: "Remove" }) as HTMLButtonElement).disabled).toBe(false);
  });

  it("the MLX engine's refusals show too", async () => {
    mocked.removeMlx.mockRejectedValue(
      new Error("Your note model uses the MLX engine. Choose another one under In use first."),
    );
    show({ kind: "mlx", bytes: 366_000_000 });
    confirm("Remove");
    expect((await screen.findByRole("alert")).textContent).toBe(
      "Your note model uses the MLX engine. Choose another one under In use first.",
    );
  });

  it("translates a refusal, keeping the path it names", async () => {
    mocked.removeModel.mockRejectedValue(
      new Error("OpenBark won't delete D:/elsewhere/m.gguf: it's a link to somewhere else."),
    );
    show({ kind: "model", row: row({}) }, "fr");
    expect(screen.getByRole("heading", { name: /^Supprimer Gemma 4 12B\s\?$/u })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Supprimer" }));
    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toContain("D:/elsewhere/m.gguf");
    expect(alert.textContent).not.toContain("it's a link");
  });
});
