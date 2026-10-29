import { createLevelSource } from "@openbark/ui";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AppContext, loadSettings, type Ctx } from "../context";
import * as engine from "../engine";
import { heardWords, VocabularySection } from "./VocabularySection";

vi.mock("../engine", async (importOriginal) => {
  const real = await importOriginal<typeof import("../engine")>();
  return {
    ...real,
    vocabulary: vi.fn(),
    saveVocabularyTerm: vi.fn(),
    removeVocabularyTerm: vi.fn(),
    listUsers: vi.fn(),
  };
});

const mocked = vi.mocked(engine);

function ctx(user: engine.User | null = null): Ctx {
  return {
    route: { screen: "settings", section: "vocabulary" },
    navigate: vi.fn(),
    status: null,
    refreshStatus: vi.fn(),
    settings: loadSettings(),
    setSettings: vi.fn(),
    toast: vi.fn(),
    announce: vi.fn(),
    levels: createLevelSource(),
    user,
    setUser: vi.fn(),
    actor: user?.id ?? "Sam",
    actorName: user?.name ?? "Sam",
    can: { notes: true, sign: true, team: true, admin: user?.admin ?? true },
  };
}

const show = (c = ctx()) =>
  render(
    <AppContext.Provider value={c}>
      <VocabularySection />
    </AppContext.Provider>,
  );

const VIEW: engine.VocabularyView = {
  terms: [{ term: "zenrelia", kind: "drug", heard: ["zen really a"] }],
  builtin: 340,
  regulatory: 3218,
  file: "/data/OpenBark/vocab.tsv",
  needs_admin: false,
};

beforeEach(() => {
  mocked.vocabulary.mockResolvedValue(VIEW);
});
afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("Words and drug names", () => {
  it("splits heard-as words on commas", () => {
    expect(heardWords(" apple,  a   pickle ;; ")).toEqual(["apple", "a pickle"]);
  });

  it("lists the clinic's words and adds one with how it's heard", async () => {
    mocked.saveVocabularyTerm.mockResolvedValue({ ...VIEW, terms: [...VIEW.terms, { term: "librela", kind: "drug", heard: ["library"] }] });
    const c = ctx();
    show(c);
    expect(await screen.findByText("zenrelia")).toBeTruthy();
    expect(screen.getByText(/3218 drug names from public registers/)).toBeTruthy();
    fireEvent.change(screen.getByLabelText("Word or name"), { target: { value: "Librela" } });
    fireEvent.change(screen.getByLabelText("Heard as (optional)"), { target: { value: "library" } });
    fireEvent.click(screen.getByRole("button", { name: "Add" }));
    await waitFor(() =>
      expect(mocked.saveVocabularyTerm).toHaveBeenCalledWith({ term: "Librela", kind: "drug", heard: ["library"] }, null),
    );
    expect(await screen.findByText("librela")).toBeTruthy();
    expect(c.toast).toHaveBeenCalledWith("Librela added");
  });

  it("asks an admin to approve a change in a team", async () => {
    const admin: engine.User = { id: "u1", name: "Dr Rivera", role: "dvm", admin: true };
    mocked.vocabulary.mockResolvedValue({ ...VIEW, needs_admin: true });
    mocked.listUsers.mockResolvedValue([admin]);
    mocked.removeVocabularyTerm.mockResolvedValue({ ...VIEW, terms: [], needs_admin: true });
    show(ctx(admin));
    fireEvent.click(await screen.findByRole("button", { name: "Remove zenrelia" }));
    expect(mocked.removeVocabularyTerm).not.toHaveBeenCalled();
    fireEvent.change(await screen.findByLabelText("Admin's PIN"), { target: { value: "2468" } });
    fireEvent.click(screen.getByRole("button", { name: "Remove" }));
    await waitFor(() => expect(mocked.removeVocabularyTerm).toHaveBeenCalledWith("zenrelia", { id: "u1", pin: "2468" }));
    expect(await screen.findByText("No words added yet.")).toBeTruthy();
  });
});
