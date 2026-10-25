import { createLevelSource } from "@openbark/ui";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AppContext, loadSettings, type Ctx } from "../context";
import * as engine from "../engine";
import { Models, plainModelName } from "./Models";

vi.mock("../engine", async (importOriginal) => {
  const real = await importOriginal<typeof import("../engine")>();
  return {
    ...real,
    models: vi.fn(),
    downloadModel: vi.fn(),
    removeModel: vi.fn(),
    gpuStatus: vi.fn(),
    mlxStatus: vi.fn(),
    hubSearch: vi.fn(),
    hubRepo: vi.fn(),
    hubAdd: vi.fn(),
    hubAlternatives: vi.fn(),
    hfTokenStatus: vi.fn(),
    setHfToken: vi.fn(),
    on: vi.fn(async () => () => {}),
  };
});
const mocked = vi.mocked(engine);

const NOTE_MODEL = "gemma-4-e4b-it-q4_0";

const row = (over: Partial<engine.ModelRow>): engine.ModelRow => ({
  id: "x",
  name: "X",
  publisher: "google",
  slot: "llm",
  size_bytes: 5_150_000_000,
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
  ...over,
});

const ROWS: engine.ModelRow[] = [
  row({ id: NOTE_MODEL, name: "Gemma 4 E4B", installed: true, disk_bytes: 5_150_000_000 }),
  row({ id: "gemma-4-12b", name: "Gemma 4 12B", installed: true, disk_bytes: 6_980_000_000 }),
  row({ id: "gemma-4-e2b", name: "Gemma 4 E2B", partial_bytes: 1_200_000_000 }),
  row({
    id: "hf:unsloth/Qwen3-235B/Qwen3-235B-Q4_K_M",
    name: "Qwen3 235B Q4_K_M",
    publisher: "unsloth",
    origin: "hub",
    builtin: false,
    parts: 3,
    fit: "wont_fit",
  }),
  row({
    id: "hf:mlx-community/Qwen3-0.6B-4bit/@mlx",
    name: "Qwen3 0.6B 4bit",
    publisher: "mlx-community",
    origin: "hub",
    builtin: false,
    engine: "mlx",
    parts: 7,
    installed: true,
    disk_bytes: 346_000_000,
  }),
  row({
    id: "local/llama-3.2-3b",
    name: "llama-3.2-3b",
    publisher: "This computer",
    origin: "local_in_place",
    remove: "forget_only",
    installed: true,
    path: "D:\\Models\\llama-3.2-3b.gguf",
    builtin: false,
  }),
  row({
    id: "local/phi-4",
    name: "phi-4",
    publisher: "This computer",
    origin: "local_in_place",
    remove: "forget_only",
    installed: false,
    missing: true,
    path: "E:\\phi-4.gguf",
    builtin: false,
  }),
  row({
    id: "local/Qwen3-1.7B-4bit",
    name: "Qwen3-1.7B-4bit",
    publisher: "This computer",
    origin: "local_copy",
    remove: "delete_and_forget",
    engine: "mlx",
    parts: 6,
    installed: true,
    disk_bytes: 980_000_000,
    builtin: false,
  }),
  row({
    id: "parakeet",
    name: "Parakeet TDT 0.6B v3",
    slot: "stt",
    installed: true,
    disk_bytes: 670_000_000,
    remove: "needed",
  }),
  row({ id: "parakeet-fp32", name: "Parakeet full precision", slot: "stt", installed: true, remove: "with_gpu_pack" }),
];

const MLX_READY: engine.MlxStatus = {
  support: "yes",
  macos_version: "15.4",
  pack_installed: true,
  installing: false,
  check: "passed",
  check_message: null,
  metal: true,
  download_bytes: 0,
  installed_bytes: 366_000_000,
  versions: {},
  in_use: false,
};

function ctx(platform = "macos"): Ctx {
  return {
    route: { screen: "models" } as Ctx["route"],
    navigate: vi.fn(),
    status: { platform, hardware: null } as unknown as engine.Status,
    refreshStatus: vi.fn(),
    settings: { ...loadSettings(), uiLanguage: "en", llm: NOTE_MODEL },
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

function show(c = ctx()) {
  render(
    <AppContext.Provider value={c}>
      <Models />
    </AppContext.Provider>,
  );
  return c;
}

const rowOf = (name: string) => screen.getByText(name, { selector: "td > div" }).closest("tr") as HTMLElement;

beforeEach(() => {
  vi.clearAllMocks();
  mocked.models.mockResolvedValue(ROWS);
  mocked.gpuStatus.mockResolvedValue({ supported: false } as engine.GpuStatus);
  mocked.mlxStatus.mockResolvedValue(MLX_READY);
  mocked.hubSearch.mockResolvedValue([]);
  mocked.hubAlternatives.mockResolvedValue([]);
  mocked.hfTokenStatus.mockResolvedValue({ saved: false, source: null, user: null });
});
afterEach(cleanup);

describe("Models catalog", () => {
  it("shows space on disk, stopped downloads, parts, MLX and models used where they are", async () => {
    show();
    await screen.findByText("Gemma 4 12B", { selector: "td > div" });
    expect(within(rowOf("Gemma 4 12B")).getByText("7.0 GB on disk")).toBeTruthy();
    expect(within(rowOf("Gemma 4 E2B")).getByText("1.2 GB partly downloaded")).toBeTruthy();
    expect(within(rowOf("Qwen3 235B Q4_K_M")).getByText("3 parts")).toBeTruthy();
    const mlx = rowOf("Qwen3 0.6B 4bit");
    expect(within(mlx).getByText("MLX")).toBeTruthy();
    expect(within(mlx).getByText("7 files")).toBeTruthy();
    const inPlace = rowOf("llama-3.2-3b");
    expect(within(inPlace).getByText("This computer")).toBeTruthy();
    expect(within(inPlace).getByText("Used where it is")).toBeTruthy();
    expect(within(inPlace).getByRole("button", { name: "Forget" })).toBeTruthy();
    expect(within(inPlace).queryByRole("button", { name: /Download/ })).toBeNull();
    const copied = rowOf("Qwen3-1.7B-4bit");
    expect(within(copied).getByText("MLX")).toBeTruthy();
    expect(within(copied).getByText("6 files")).toBeTruthy();
    expect(within(copied).getByText("This computer")).toBeTruthy();
    expect(within(copied).queryByText("Used where it is")).toBeNull();
    expect(within(copied).getByRole("button", { name: "Remove" })).toBeTruthy();
    const missing = rowOf("phi-4");
    expect(within(missing).getByText("Not found")).toBeTruthy();
    expect(within(missing).queryByRole("button", { name: /Download/ })).toBeNull();
  });

  it("protects the speech models, hides the GPU pack's, and offers Remove on the rest", async () => {
    show();
    await screen.findByText("Gemma 4 12B", { selector: "td > div" });
    const stt = rowOf("Parakeet TDT 0.6B v3");
    expect(within(stt).getByText("Needed to transcribe visits")).toBeTruthy();
    expect(within(stt).queryByRole("button", { name: "Remove" })).toBeNull();
    expect(screen.queryByText("Parakeet full precision")).toBeNull();
    expect(within(rowOf("Gemma 4 12B")).getByRole("button", { name: "Remove" })).toBeTruthy();
    expect(within(rowOf("Gemma 4 E2B")).getByRole("button", { name: "Remove" })).toBeTruthy();
    // A Hugging Face model with nothing downloaded: only the list entry to drop.
    const hub = rowOf("Qwen3 235B Q4_K_M");
    expect(within(hub).queryByRole("button", { name: "Remove" })).toBeNull();
    expect(within(hub).getByRole("button", { name: "Remove from list" })).toBeTruthy();
  });

  it("can't remove the model that writes notes on this device", async () => {
    show();
    await screen.findByText("Gemma 4 12B", { selector: "td > div" });
    const note = rowOf("Gemma 4 E4B");
    const remove = within(note).getByRole("button", { name: "Remove" }) as HTMLButtonElement;
    expect(remove.disabled).toBe(true);
    const help = within(note).getByText("Writes your notes. Choose another model first.");
    expect(remove.getAttribute("aria-describedby")).toBe(help.id);
  });

  it("removes through the dialog with the note model, then reloads", async () => {
    mocked.removeModel.mockResolvedValue({
      id: "gemma-4-12b",
      freed_bytes: 6_980_000_000,
      files_removed: 1,
      forgotten: false,
      kept_in_place: false,
    });
    const c = show();
    await screen.findByText("Gemma 4 12B", { selector: "td > div" });
    fireEvent.click(within(rowOf("Gemma 4 12B")).getByRole("button", { name: "Remove" }));
    const dialog = screen.getByRole("dialog");
    expect(within(dialog).getByRole("heading").textContent).toBe("Remove Gemma 4 12B?");
    fireEvent.click(within(dialog).getByRole("button", { name: "Remove" }));
    await waitFor(() => expect(c.toast).toHaveBeenCalledWith("Removed. 7.0 GB freed."));
    expect(mocked.removeModel).toHaveBeenCalledWith("gemma-4-12b", NOTE_MODEL, false);
    await waitFor(() => expect(mocked.models).toHaveBeenCalledTimes(2));
  });

  it("drops a Hugging Face model that has nothing on disk from the list", async () => {
    mocked.removeModel.mockResolvedValue({
      id: "hf:unsloth/Qwen3-235B/Qwen3-235B-Q4_K_M",
      freed_bytes: 0,
      files_removed: 0,
      forgotten: true,
      kept_in_place: false,
    });
    const c = show();
    await screen.findByText("Gemma 4 12B", { selector: "td > div" });
    fireEvent.click(within(rowOf("Qwen3 235B Q4_K_M")).getByRole("button", { name: "Remove from list" }));
    await waitFor(() => expect(c.toast).toHaveBeenCalledWith("Removed from the list."));
    expect(mocked.removeModel).toHaveBeenCalledWith("hf:unsloth/Qwen3-235B/Qwen3-235B-Q4_K_M", NOTE_MODEL, true);
  });

  it("shows download progress on its row, but not a copy from this computer's (the add sheet shows that)", async () => {
    let send: ((d: { model: string; done: number; total: number }) => void) | null = null;
    mocked.on.mockImplementation((async (_name: string, cb: (d: never) => void) => {
      send = cb as unknown as typeof send;
      return () => {};
    }) as unknown as typeof engine.on);
    show();
    await screen.findByText("Gemma 4 E2B", { selector: "td > div" });
    await waitFor(() => expect(send).not.toBeNull());
    send!({ model: "local/llama-3.2-3b", done: 10, total: 100 });
    send!({ model: "gemma-4-e2b", done: 10, total: 100 });
    await waitFor(() => expect(within(rowOf("Gemma 4 E2B")).getByText("Downloading")).toBeTruthy());
    expect(within(rowOf("llama-3.2-3b")).queryByText("Downloading")).toBeNull();
  });

  it("says when a copy is linked to the original and takes no extra space", async () => {
    mocked.models.mockResolvedValue([
      ...ROWS,
      row({
        id: "local/linked",
        name: "linked-copy",
        origin: "local_copy",
        remove: "delete_and_forget",
        installed: true,
        disk_bytes: 0,
        shared_bytes: 2_000_000_000,
        builtin: false,
      }),
    ]);
    show();
    await screen.findByText("linked-copy", { selector: "td > div" });
    const linked = rowOf("linked-copy");
    expect(within(linked).getByText("Linked to another file, so it takes no extra space")).toBeTruthy();
    expect(within(linked).queryByText(/on disk/)).toBeNull();
  });

  it("doesn't offer MLX models for notes where the MLX engine can't run", async () => {
    mocked.mlxStatus.mockResolvedValue({ ...MLX_READY, support: "not_mac", pack_installed: false, check: "not_run" });
    show(ctx("windows"));
    await screen.findByText("Gemma 4 12B", { selector: "td > div" });
    const select = screen.getByRole("combobox", { name: "Model that writes notes" });
    await waitFor(() => {
      const mlx = within(select).getByRole("option", { name: "Qwen3 0.6B 4bit (needs the MLX engine)" });
      expect((mlx as HTMLOptionElement).disabled).toBe(true);
    });
    expect((within(select).getByRole("option", { name: "Gemma 4 12B" }) as HTMLOptionElement).disabled).toBe(false);
  });

  it("offers MLX models for notes once the engine is installed", async () => {
    show();
    await screen.findByText("Gemma 4 12B", { selector: "td > div" });
    const select = screen.getByRole("combobox", { name: "Model that writes notes" });
    await waitFor(() => {
      const mlx = within(select).getByRole("option", { name: "Qwen3 0.6B 4bit" });
      expect((mlx as HTMLOptionElement).disabled).toBe(false);
    });
  });

  it("a download's last event, arriving late, doesn't leave the row downloading", async () => {
    let send: ((d: { model: string; done: number; total: number }) => void) | null = null;
    mocked.on.mockImplementation((async (_name: string, cb: (d: never) => void) => {
      send = cb as unknown as typeof send;
      return () => {};
    }) as unknown as typeof engine.on);
    show();
    await screen.findByText("Gemma 4 E2B", { selector: "td > div" });
    await waitFor(() => expect(send).not.toBeNull());
    send!({ model: "gemma-4-e2b", done: 10, total: 100 });
    await waitFor(() => expect(within(rowOf("Gemma 4 E2B")).getByText("Downloading")).toBeTruthy());
    send!({ model: "gemma-4-e2b", done: 100, total: 100 });
    await waitFor(() => expect(within(rowOf("Gemma 4 E2B")).queryByText("Downloading")).toBeNull());
  });

  it("has a header button to add a model from this computer", async () => {
    show();
    fireEvent.click(await screen.findByRole("button", { name: "Add from this computer" }));
    expect(await screen.findByRole("heading", { name: "Add from this computer" })).toBeTruthy();
  });
});

// ---- the Hugging Face sheet ------------------------------------------------------

const model = (id: string, over: Partial<engine.HubModel> = {}): engine.HubModel => ({
  id,
  publisher: id.split("/")[0] ?? "",
  downloads: 1000,
  likes: 1,
  license: "apache-2.0",
  license_class: "permissive",
  gated: false,
  last_modified: null,
  mlx: false,
  ...over,
});

const mlxView = (over: Partial<engine.HubMlxView> = {}): engine.HubMlxView => ({
  size_bytes: 346_000_000,
  files: 7,
  fit: "fits",
  model_type: "qwen3",
  quantization: "4-bit",
  problem: null,
  arch_supported: true,
  support: "yes",
  pack_installed: true,
  ...over,
});

const MLX_REPO = "mlx-community/Qwen3-0.6B-4bit";

function repoView(over: {
  id?: string;
  mlx?: engine.HubMlxView | null;
  files?: engine.HubRepoView["files"];
  access?: engine.HubAccess;
  gated_kind?: "auto" | "manual" | null;
  base_model?: string | null;
}): engine.HubRepoView {
  const id = over.id ?? MLX_REPO;
  return {
    repo: {
      model: model(id, { mlx: over.mlx !== undefined && over.mlx !== null }),
      revision: "abc",
      access: over.access ?? "open",
      gated_kind: over.gated_kind ?? null,
      base_model: over.base_model ?? null,
    },
    files: over.files ?? [],
    mlx: over.mlx ?? null,
  };
}

async function openRepo(view: engine.HubRepoView, c = ctx()) {
  mocked.hubSearch.mockResolvedValue([view.repo.model]);
  mocked.hubRepo.mockResolvedValue(view);
  show(c);
  fireEvent.click(await screen.findByRole("button", { name: "Browse Hugging Face" }));
  fireEvent.click(await screen.findByRole("button", { name: new RegExp(view.repo.model.id) }));
  await screen.findByRole("heading", { name: view.repo.model.id });
  return c;
}

describe("Hugging Face sheet", () => {
  it("offers an MLX repo as one row: the whole model folder", async () => {
    mocked.hubAdd.mockResolvedValue(row({ id: `hf:${MLX_REPO}/@mlx` }));
    mocked.downloadModel.mockResolvedValue(undefined);
    await openRepo(repoView({ mlx: mlxView() }));
    const folder = screen.getByText("Whole model folder").closest("tr") as HTMLElement;
    expect(within(folder).getByText("7 files")).toBeTruthy();
    expect(within(folder).getByText("346 MB")).toBeTruthy();
    expect(within(folder).getByText("Fits")).toBeTruthy();
    fireEvent.click(within(folder).getByRole("button", { name: "Download" }));
    await waitFor(() => expect(mocked.hubAdd).toHaveBeenCalledWith(MLX_REPO, "@mlx", false));
    await waitFor(() => expect(mocked.downloadModel).toHaveBeenCalledWith(`hf:${MLX_REPO}/@mlx`));
  });

  it("asks for the MLX engine first, with a way to it", async () => {
    await openRepo(repoView({ mlx: mlxView({ pack_installed: false }) }));
    expect(screen.queryByText("Whole model folder")).toBeNull();
    expect(screen.getByText("Download the MLX engine first. It's on the Models screen.")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Get the MLX engine" }));
    await waitFor(() => expect(screen.queryByText("Whole model folder")).toBeNull());
  });

  it.each([
    [{ support: "needs_macos14" as const }, "MLX models run on a Mac with Apple silicon and macOS 14 or later."],
    [{ support: "not_mac" as const }, "MLX models run on a Mac with Apple silicon and macOS 14 or later."],
    [
      { arch_supported: false, model_type: "deepseek_v4" },
      "OpenBark's MLX engine can't run this kind of model yet (deepseek_v4).",
    ],
    [{ problem: { kind: "no_config" } as engine.MlxProblem }, "This repository has no config.json, so it can't run."],
    [{ problem: { kind: "bad_config" } as engine.MlxProblem }, "This repository's config.json can't be read."],
    [{ problem: { kind: "no_weights" } as engine.MlxProblem }, "This repository has no safetensors weights."],
    [
      { problem: { kind: "incomplete_shards", missing: ["model-00002-of-00002.safetensors"] } as engine.MlxProblem },
      "Some weight files are missing: model-00002-of-00002.safetensors.",
    ],
    [{ problem: { kind: "no_tokenizer" } as engine.MlxProblem }, "This repository has no tokenizer files."],
  ])("explains why an MLX repo can't run here (%o)", async (over, text) => {
    await openRepo(repoView({ mlx: mlxView(over) }));
    expect(screen.getByText(text)).toBeTruthy();
    expect(screen.queryByText("Whole model folder")).toBeNull();
    expect(screen.queryByRole("button", { name: "Get the MLX engine" })).toBeNull();
  });

  it("lists GGUF builds of an MLX repo's base model, and opens one", async () => {
    mocked.hubAlternatives.mockResolvedValue([model("Qwen/Qwen3-0.6B-GGUF")]);
    await openRepo(repoView({ mlx: mlxView({ support: "needs_apple_silicon" }), base_model: "Qwen/Qwen3-0.6B" }));
    expect(mocked.hubAlternatives).toHaveBeenCalledWith("Qwen/Qwen3-0.6B");
    expect(await screen.findByText("GGUF builds of this model")).toBeTruthy();
    expect(screen.getByText("They run with the built-in engine on any computer.")).toBeTruthy();
    mocked.hubRepo.mockResolvedValue(
      repoView({
        id: "Qwen/Qwen3-0.6B-GGUF",
        files: [{ path: "Qwen3-0.6B-Q8_0.gguf", size_bytes: 1, fit: "fits", parts: 1 }],
      }),
    );
    fireEvent.click(screen.getByRole("button", { name: /Qwen\/Qwen3-0\.6B-GGUF/ }));
    expect(await screen.findByRole("heading", { name: "Qwen/Qwen3-0.6B-GGUF" })).toBeTruthy();
    expect(mocked.hubRepo).toHaveBeenLastCalledWith("Qwen/Qwen3-0.6B-GGUF");
  });

  it("shows a split GGUF as one row with its parts", async () => {
    await openRepo(
      repoView({
        id: "unsloth/Qwen3-235B-A22B-Instruct-2507-GGUF",
        files: [
          {
            path: "Q4_K_M/Qwen3-235B-A22B-Instruct-2507-Q4_K_M-00001-of-00003.gguf",
            size_bytes: 142_000_000_000,
            fit: "wont_fit",
            parts: 3,
          },
        ],
      }),
    );
    const split = screen.getByText(/Q4_K_M-00001-of-00003\.gguf/).closest("tr") as HTMLElement;
    expect(within(split).getByText("3 parts")).toBeTruthy();
  });

  it("says when nothing in a repo can run", async () => {
    await openRepo(repoView({ id: "meta-llama/Llama-3.2-3B-Instruct", files: [] }));
    expect(screen.getByText("No GGUF or MLX files OpenBark can run in this repository.")).toBeTruthy();
  });

  it("gated, no token: what to do, and the token field", async () => {
    await openRepo(
      repoView({
        id: "google/gemma-3-1b-it-qat-q4_0-gguf",
        access: "needs_token",
        gated_kind: "auto",
        files: [{ path: "gemma-3-1b-it-q4_0.gguf", size_bytes: 1_000_000_000, fit: "fits", parts: 1 }],
      }),
    );
    expect(screen.getByText("This model is gated. Request access on Hugging Face, then add your token.")).toBeTruthy();
    expect(screen.queryByText(/The publisher reviews requests by hand\./)).toBeNull();
    expect(screen.getByRole("link", { name: "Request access" }).getAttribute("href")).toBe(
      "https://huggingface.co/google/gemma-3-1b-it-qat-q4_0-gguf",
    );
    expect(screen.getByRole("link", { name: "Get a token" }).getAttribute("href")).toBe(
      "https://huggingface.co/settings/tokens",
    );
    expect(await screen.findByLabelText("Hugging Face token")).toBeTruthy();
    expect((screen.getByRole("button", { name: "Download" }) as HTMLButtonElement).disabled).toBe(true);
    expect(screen.queryByText("No GGUF or MLX files OpenBark can run in this repository.")).toBeNull();
  });

  it("gated, token without access yet, reviewed by hand", async () => {
    mocked.hfTokenStatus.mockResolvedValue({ saved: true, source: "keychain", user: null });
    await openRepo(
      repoView({
        id: "google/medgemma-1.5-4b-it-GGUF",
        access: "no_access_yet",
        gated_kind: "manual",
        files: [{ path: "medgemma-1.5-4b-it-Q4_K_M.gguf", size_bytes: 2_500_000_000, fit: "fits", parts: 1 }],
      }),
    );
    expect(
      screen.getByText(
        "Hugging Face hasn't given your token access yet. Check your request on the model's page. The publisher reviews requests by hand.",
      ),
    ).toBeTruthy();
    expect(await screen.findByText("Saved in this computer's keychain.")).toBeTruthy();
  });

  it("opens the repo again once a token is saved", async () => {
    const view = repoView({
      id: "google/gemma-3-1b-it-qat-q4_0-gguf",
      access: "needs_token",
      gated_kind: "auto",
      files: [{ path: "gemma-3-1b-it-q4_0.gguf", size_bytes: 1_000_000_000, fit: "fits", parts: 1 }],
    });
    await openRepo(view);
    mocked.setHfToken.mockResolvedValue({ saved: true, source: "keychain", user: "sam" });
    mocked.hubRepo.mockResolvedValue({ ...view, repo: { ...view.repo, access: "open" } });
    fireEvent.change(await screen.findByLabelText("Hugging Face token"), { target: { value: "hf_abcdefghijk" } });
    fireEvent.click(screen.getByRole("button", { name: "Save token" }));
    await waitFor(() => expect(mocked.hubRepo).toHaveBeenCalledTimes(2));
  });

  // meta-llama/*: gated, and holding only safetensors. Asking for access
  // and a token would lead nowhere: say there's nothing to run, and offer
  // GGUF builds of it instead.
  it("a gated repo with nothing OpenBark can run says so, without asking for a token", async () => {
    mocked.hubAlternatives.mockResolvedValue([model("bartowski/Llama-3.2-1B-Instruct-GGUF")]);
    await openRepo(
      repoView({ id: "meta-llama/Llama-3.2-1B-Instruct", access: "needs_token", gated_kind: "manual", files: [] }),
    );
    expect(screen.getByText("No GGUF or MLX files OpenBark can run in this repository.")).toBeTruthy();
    expect(screen.queryByText("This model is gated. Request access on Hugging Face, then add your token.")).toBeNull();
    expect(screen.queryByRole("link", { name: "Request access" })).toBeNull();
    expect(screen.queryByLabelText("Hugging Face token")).toBeNull();
    expect(mocked.hubAlternatives).toHaveBeenCalledWith("meta-llama/Llama-3.2-1B-Instruct");
    expect(await screen.findByText("GGUF builds of this model")).toBeTruthy();
    expect(screen.getByRole("button", { name: /bartowski\/Llama-3\.2-1B-Instruct-GGUF/ })).toBeTruthy();
  });

  it("drops a slow list of GGUF builds for a repo that's no longer open", async () => {
    const A = "mlx-community/Llama-3.2-1B-Instruct-4bit";
    const viewA = repoView({ id: A, mlx: mlxView(), base_model: "meta-llama/Llama-3.2-1B-Instruct" });
    const viewB = repoView({ id: MLX_REPO, mlx: mlxView(), base_model: "Qwen/Qwen3-0.6B" });
    let answerA: (m: engine.HubModel[]) => void = () => {};
    mocked.hubAlternatives.mockImplementation((base: string) =>
      base === "meta-llama/Llama-3.2-1B-Instruct"
        ? new Promise<engine.HubModel[]>((resolve) => {
            answerA = resolve;
          })
        : Promise.resolve([]),
    );
    mocked.hubSearch.mockResolvedValue([viewA.repo.model, viewB.repo.model]);
    mocked.hubRepo.mockImplementation(async (id: string) => (id === A ? viewA : viewB));
    show();
    fireEvent.click(await screen.findByRole("button", { name: "Browse Hugging Face" }));
    fireEvent.click(await screen.findByRole("button", { name: /Llama-3\.2-1B-Instruct-4bit/ }));
    await screen.findByRole("heading", { name: A });
    fireEvent.click(screen.getByRole("button", { name: "Back to results" }));
    fireEvent.click(await screen.findByRole("button", { name: /Qwen3-0\.6B-4bit/ }));
    await screen.findByRole("heading", { name: MLX_REPO });
    answerA([model("bartowski/Llama-3.2-1B-Instruct-GGUF")]);
    await new Promise((r) => setTimeout(r, 20));
    expect(screen.queryByText("GGUF builds of this model")).toBeNull();
    expect(screen.queryByText("bartowski/Llama-3.2-1B-Instruct-GGUF")).toBeNull();
  });

  it("reaches the token from the sheet's footer", async () => {
    show();
    fireEvent.click(await screen.findByRole("button", { name: "Browse Hugging Face" }));
    fireEvent.click(await screen.findByRole("button", { name: "Hugging Face token" }));
    expect(await screen.findByLabelText("Hugging Face token")).toBeTruthy();
  });
});

// ---- the phone profile (ADR-0018 section 10) ------------------------------------

const PHONE_ROWS: engine.ModelRow[] = [
  row({ id: "gemma-4-e2b-it-q4_0", name: "Gemma 4 E2B", size_bytes: 3_349_516_256 }),
  row({ id: "qwen3-1.7b-q4_k_m", name: "Qwen3 1.7B", size_bytes: 1_107_409_472, publisher: "unsloth" }),
  row({
    id: "gemma-4-e4b-it-q4_0",
    name: "Gemma 4 E4B",
    fit: "wont_fit",
    phone_note: "Too large for this iPhone (8 GB): 5.2 GB. Models up to 3.5 GB fit.",
  }),
  row({ id: "parakeet", name: "Parakeet TDT 0.6B v3", slot: "stt", installed: true, remove: "needed" }),
];

function phoneCtx(tier: number | null = 8): Ctx {
  const c = ctx("ios");
  c.status = {
    platform: "ios",
    profile: "ios",
    mobile_tier: tier,
    default_llm: tier ? "gemma-4-e2b-it-q4_0" : "",
    hardware: null,
  } as unknown as engine.Status;
  c.settings = { ...c.settings, llm: "gemma-4-e2b-it-q4_0" };
  return c;
}

describe("Models on a phone", () => {
  beforeEach(() => {
    mocked.models.mockResolvedValue(PHONE_ROWS);
  });

  it("shows the phone's tier and hides the MLX, GPU and add-from-computer parts", async () => {
    show(phoneCtx());
    await screen.findByText("Gemma 4 E2B", { selector: "td > div" });
    expect(screen.getByTestId("phone-tier").textContent).toContain("(8 GB)");
    expect(screen.queryByRole("button", { name: "Add from this computer" })).toBeNull();
    expect(screen.queryByText("What the model names mean")).toBeNull();
    expect(mocked.mlxStatus).not.toHaveBeenCalled();
    expect(mocked.gpuStatus).not.toHaveBeenCalled();
    expect(screen.getByRole("columnheader", { name: "On this phone" })).toBeTruthy();
  });

  it("says a model is too large for this iPhone and won't download it", async () => {
    show(phoneCtx());
    await screen.findByText("Gemma 4 E4B", { selector: "td > div" });
    const e4b = rowOf("Gemma 4 E4B");
    expect(within(e4b).getByText("Too large for this iPhone (8 GB): 5.2 GB. Models up to 3.5 GB fit.")).toBeTruthy();
    const dl = within(e4b).getByRole("button", { name: /Download/ }) as HTMLButtonElement;
    expect(dl.disabled).toBe(true);
    const e2b = within(rowOf("Gemma 4 E2B")).getByRole("button", { name: /Download/ }) as HTMLButtonElement;
    expect(e2b.disabled).toBe(false);
  });

  it("shows the speech models that come with the app as built in, with their credits", async () => {
    mocked.models.mockResolvedValue([
      ...PHONE_ROWS.filter((r) => r.id !== "parakeet"),
      row({
        id: "parakeet",
        name: "Parakeet TDT 0.6B v3",
        slot: "stt",
        license: "CC-BY-4.0",
        installed: true,
        bundled: true,
        remove: "needed",
        attribution: "Parakeet TDT 0.6B v3 by NVIDIA, CC-BY-4.0; ONNX export by the k2-fsa/sherpa-onnx project",
      }),
    ]);
    show(phoneCtx());
    await screen.findByText("Parakeet TDT 0.6B v3", { selector: "td > div" });
    const parakeet = rowOf("Parakeet TDT 0.6B v3");
    expect(within(parakeet).getByText("Built in")).toBeTruthy();
    expect(within(parakeet).getByText("Comes with the app")).toBeTruthy();
    expect(within(parakeet).getByText(/by NVIDIA, CC-BY-4.0/)).toBeTruthy();
    expect(within(parakeet).queryByRole("button")).toBeNull();
  });

  it("says a phone under 6 GB can't write notes", async () => {
    show(phoneCtx(null));
    await screen.findByText("Gemma 4 E2B", { selector: "td > div" });
    expect(screen.getByTestId("phone-tier").textContent).toBe(
      "Writing notes on the phone needs an iPhone with 8 GB of memory or more. Record here, and draft the note on your computer.",
    );
  });

  it("browses phone-sized GGUFs only, and lists what's held back with the reason", async () => {
    const model: engine.HubModel = {
      id: "unsloth/gemma-4-E4B-it-GGUF",
      publisher: "unsloth",
      downloads: 10,
      likes: 0,
      license: "apache-2.0",
      license_class: "permissive",
      gated: false,
      last_modified: null,
      mlx: false,
    } as engine.HubModel;
    mocked.hubSearch.mockResolvedValue([model]);
    mocked.hubRepo.mockResolvedValue({
      repo: { model, revision: "a".repeat(40), access: "open", gated_kind: null, base_model: null },
      files: [],
      mlx: null,
      held: [
        {
          path: "gemma-4-E4B-it-Q4_K_M.gguf",
          size_bytes: 4_977_171_584,
          reason: "Too large for this iPhone (8 GB): 5.0 GB. Models up to 3.5 GB fit.",
        },
        {
          path: "gemma-4-E4B-it-BF16.gguf",
          size_bytes: 15_053_097_856,
          reason: "Not offered on this iPhone (8 GB): phones need a 3 to 5-bit build (Q4_K_M or Q4_0, for example).",
        },
      ],
      phone_note:
        "No file in this repository fits this iPhone (8 GB). Models up to 3.5 GB and 4.5B parameters, in a 3 to 5-bit build, fit.",
    });
    show(phoneCtx());
    fireEvent.click(await screen.findByRole("button", { name: "Browse Hugging Face" }));
    expect(
      await screen.findByText(
        "Small note-writing models in GGUF format that fit this iPhone. Searching contacts huggingface.co; nothing else leaves this iPhone.",
      ),
    ).toBeTruthy();
    expect(screen.queryByText("Only show MLX-optimized models")).toBeNull();
    expect(screen.getByRole("button", { name: "Qwen3 1.7B" })).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Gemma 4 12B" })).toBeNull();
    fireEvent.click(await screen.findByText("unsloth/gemma-4-E4B-it-GGUF", { selector: ".hub-name" }));
    expect((await screen.findByTestId("phone-note")).textContent).toContain("No file in this repository fits");
    expect(screen.getByText("Not offered on this iPhone (2)")).toBeTruthy();
    expect(screen.getByText("Too large for this iPhone (8 GB): 5.0 GB. Models up to 3.5 GB fit.")).toBeTruthy();
    // Held files can't be downloaded, and no "GGUF builds of this model" are fetched for them.
    expect(screen.queryByRole("button", { name: "Download" })).toBeNull();
    expect(mocked.hubAlternatives).not.toHaveBeenCalled();
  });
});

// ---- phone width (ADR-0027): stacked rows, plain names ---------------------------

describe("plainModelName", () => {
  it("keeps the name a person picks a model by, and the build details apart", () => {
    expect(plainModelName("Gemma 4 E4B instruct (QAT Q4_0)")).toEqual({ name: "Gemma 4 E4B", detail: "QAT Q4_0" });
    expect(plainModelName("Qwen3 235B Q4_K_M")).toEqual({ name: "Qwen3 235B", detail: "Q4_K_M" });
    expect(plainModelName("Qwen3 0.6B 4bit")).toEqual({ name: "Qwen3 0.6B", detail: "4bit" });
    expect(plainModelName("Qwen3-1.7B-4bit")).toEqual({ name: "Qwen3-1.7B", detail: "4bit" });
    expect(plainModelName("Parakeet TDT 0.6B v3 (int8)")).toEqual({ name: "Parakeet TDT 0.6B v3", detail: "int8" });
    expect(plainModelName("llama-3.2-3b")).toEqual({ name: "llama-3.2-3b", detail: "" });
    expect(plainModelName("Silero VAD")).toEqual({ name: "Silero VAD", detail: "" });
    expect(plainModelName("Q4_K_M")).toEqual({ name: "Q4_K_M", detail: "" });
  });
});

describe("Models at phone width", () => {
  beforeEach(() => {
    vi.stubGlobal("matchMedia", (q: string) => ({
      matches: q === "(max-width: 640px)",
      media: q,
      addEventListener() {},
      removeEventListener() {},
    }));
    mocked.models.mockResolvedValue([
      ...PHONE_ROWS,
      row({ id: "qwen3-q4km", name: "Qwen3 4B instruct (Q4_K_M)", size_bytes: 2_500_000_000, publisher: "unsloth" }),
    ]);
  });
  afterEach(() => vi.unstubAllGlobals());

  it("stacks the catalog with plain names and sizes, no table", async () => {
    show(phoneCtx());
    const name = await screen.findByText("Qwen3 4B", { selector: ".model-item-name" });
    expect(document.querySelector(".ob-table")).toBeNull();
    const item = name.closest("li") as HTMLElement;
    expect(within(item).getByText("Q4_K_M")).toBeTruthy();
    expect(within(item).getByText("2.5 GB")).toBeTruthy();
    expect(within(item).getByRole("button", { name: "Download (2.5 GB)" })).toBeTruthy();
    // A too-large model still says why, and still can't download.
    const e4b = screen.getByText("Gemma 4 E4B", { selector: ".model-item-name" }).closest("li") as HTMLElement;
    expect(within(e4b).getByText("Too large for this iPhone (8 GB): 5.2 GB. Models up to 3.5 GB fit.")).toBeTruthy();
    expect((within(e4b).getByRole("button", { name: /Download/ }) as HTMLButtonElement).disabled).toBe(true);
  });

  it("makes Browse Hugging Face a secondary button", async () => {
    show(phoneCtx());
    const browse = await screen.findByRole("button", { name: "Browse Hugging Face" });
    expect(browse.getAttribute("data-variant")).toBe("default");
  });
});
