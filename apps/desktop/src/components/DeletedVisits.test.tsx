import { createLevelSource } from "@openbark/ui";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AppContext, loadSettings, type Ctx } from "../context";
import * as engine from "../engine";
import { DeletedVisits } from "./DeletedVisits";

vi.mock("../engine", async (importOriginal) => {
  const real = await importOriginal<typeof import("../engine")>();
  return { ...real, listDeleted: vi.fn(), restoreVisit: vi.fn(), purgeDeleted: vi.fn() };
});

const mocked = vi.mocked(engine);

const ROW: engine.DeletedSummary = {
  trash_id: "0199a7f0-1c2d-7e3f-8a9b-0c1d2e3f4a5b",
  patient: "Olive Haddad",
  patient_ids: ["p-olive", "p-pickles"],
  pets: ["Olive", "Pickles"],
  created_at: "2026-10-18 10:15",
  deleted_at: "2026-10-18 11:02",
  by: "Sam",
  reason: { kind: "mistake", note: "" },
  signed: false,
  purge_after: "2026-11-17 11:02",
  kept_until: null,
  bytes: 120_000,
  recovered: false,
};

function ctx(): Ctx {
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
    actor: "Sam",
    actorName: "Sam",
    can: { notes: true, sign: false, team: false },
  };
}

function show(c = ctx()) {
  render(
    <AppContext.Provider value={c}>
      <DeletedVisits />
    </AppContext.Provider>,
  );
  return c;
}

beforeEach(() => vi.clearAllMocks());
afterEach(cleanup);

describe("Settings, Deleted visits", () => {
  it("says when there are none", async () => {
    mocked.listDeleted.mockResolvedValue([]);
    show();
    expect(await screen.findByText("No deleted visits.")).toBeTruthy();
    expect(screen.getByText("Deleted visits are kept here for 30 days, then removed for good.")).toBeTruthy();
    expect(mocked.listDeleted).toHaveBeenCalledWith("Sam");
  });

  it("shows every field of a row", async () => {
    mocked.listDeleted.mockResolvedValue([ROW, { ...ROW, trash_id: "0199b", patient: "", pets: [], signed: true }]);
    show();
    expect(await screen.findByText("Olive Haddad")).toBeTruthy();
    expect(screen.getByText("Olive and Pickles")).toBeTruthy();
    expect(screen.getAllByText("Visit of 2026-10-18 10:15")).toHaveLength(2);
    expect(screen.getAllByText("Deleted 2026-10-18 11:02 by Sam")).toHaveLength(2);
    expect(screen.getAllByText("Recorded by mistake")).toHaveLength(2);
    expect(screen.getAllByText("Removed for good on 2026-11-17")).toHaveLength(2);
    expect(screen.getByText("No patient")).toBeTruthy();
    expect(screen.getByText("Signed")).toBeTruthy();
    expect(screen.getAllByRole("button", { name: "Restore" })).toHaveLength(2);
    const now = screen.getAllByRole("button", { name: "Delete now" });
    expect(now[0]?.getAttribute("data-variant")).toBe("danger");
  });

  it("restores a visit", async () => {
    mocked.listDeleted.mockResolvedValueOnce([ROW]).mockResolvedValueOnce([]);
    mocked.restoreVisit.mockResolvedValue({ id: "2026-10-18_101500_olive-haddad", patient_id: "p-olive" });
    const c = show();
    fireEvent.click(await screen.findByRole("button", { name: "Restore" }));
    await waitFor(() => expect(mocked.restoreVisit).toHaveBeenCalledWith(ROW.trash_id, "Sam"));
    await waitFor(() => expect(c.toast).toHaveBeenCalledWith("Visit restored"));
    expect(await screen.findByText("No deleted visits.")).toBeTruthy();
  });

  it("asks before deleting now, then removes it for good", async () => {
    mocked.listDeleted.mockResolvedValueOnce([ROW]).mockResolvedValueOnce([]);
    mocked.purgeDeleted.mockResolvedValue(undefined);
    const c = show();
    fireEvent.click(await screen.findByRole("button", { name: "Delete now" }));
    expect(screen.getByRole("heading", { name: "Delete this visit now?" })).toBeTruthy();
    expect(
      screen.getByText(
        "OpenBark removes its files now, and it can't be restored. Backups of this computer may still hold encrypted copies, which the recovery key can open.",
      ),
    ).toBeTruthy();
    expect(mocked.purgeDeleted).not.toHaveBeenCalled();
    const dialog = screen.getByRole("dialog");
    fireEvent.click(dialog.querySelector("[data-variant=danger]")!);
    await waitFor(() => expect(mocked.purgeDeleted).toHaveBeenCalledWith(ROW.trash_id, "Sam"));
    await waitFor(() => expect(c.toast).toHaveBeenCalledWith("Visit deleted for good"));
  });

  it("Keep visit closes the dialog without deleting", async () => {
    mocked.listDeleted.mockResolvedValue([ROW]);
    show();
    fireEvent.click(await screen.findByRole("button", { name: "Delete now" }));
    fireEvent.click(screen.getByRole("button", { name: "Keep visit" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(mocked.purgeDeleted).not.toHaveBeenCalled();
  });
});
