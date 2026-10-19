import { createLevelSource } from "@openbark/ui";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AppContext, loadSettings, type Ctx } from "../context";
import * as engine from "../engine";
import { Review } from "./Review";

vi.mock("../engine", async (importOriginal) => {
  const real = await importOriginal<typeof import("../engine")>();
  return {
    ...real,
    openSession: vi.fn(),
    listTemplates: vi.fn().mockResolvedValue([]),
    template: vi.fn().mockRejectedValue(new Error("none")),
    deleteVisit: vi.fn(),
    restoreVisit: vi.fn(),
    deleteRecording: vi.fn(),
    on: vi.fn().mockResolvedValue(() => {}),
  };
});

const mocked = vi.mocked(engine);

function sessionView(over: Partial<engine.SessionView> = {}): engine.SessionView {
  return {
    id: "2026-10-18_101500_olive-haddad",
    dir: "",
    audio_path: "",
    created_at: "2026-10-18 10:15",
    source: "Recorded",
    transcript: { segments: [{ id: 0, start: 0, end: 1, speaker: 0, text: "Both ears look red" }] },
    stats: null,
    latest_run: null,
    latest_markdown: null,
    signed_at: null,
    signed_by: null,
    patient: {
      name: "Olive",
      last_name: "Haddad",
      reason: "Ear check",
      others: ["Pickles"],
      patient_id: "p-olive",
      other_ids: ["p-pickles"],
    },
    latest_template: null,
    addenda: [],
    consent: null,
    documents: [],
    recorded_by: null,
    pets: ["Olive", "Pickles"],
    pet: "Pickles",
    any_signed: false,
    has_audio: true,
    audio_deleted: null,
    ...over,
  };
}

function ctx(over: Partial<Ctx> = {}): Ctx {
  return {
    route: { screen: "review", id: "v" },
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
    actor: "Dr Rivera",
    actorName: "Dr Rivera",
    can: { notes: true, sign: true, team: true, admin: true },
    ...over,
  };
}

function show(view: engine.SessionView, c: Ctx, pet = "Pickles") {
  mocked.openSession.mockResolvedValue(view);
  render(
    <AppContext.Provider value={c}>
      <Review id={view.id} autoDraft={false} initialPet={pet} />
    </AppContext.Provider>,
  );
}

beforeEach(() => {
  vi.clearAllMocks();
  // jsdom has no media playback; the player is stopped before files move.
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
});
afterEach(cleanup);

describe("Review: deleting", () => {
  it("deletes from the reason menu, goes back to the animal's page, and Undo restores", async () => {
    const c = ctx();
    const view = sessionView();
    mocked.deleteVisit.mockResolvedValue({
      trash_id: "0199a7f0-1c2d-7e3f-8a9b-0c1d2e3f4a5b",
      pets: ["Olive", "Pickles"],
      signed: false,
      purge_after: "2026-11-17 10:20",
      patient_id: "p-olive",
    });
    mocked.restoreVisit.mockResolvedValue({ id: view.id, patient_id: "p-olive" });
    show(view, c);
    fireEvent.click(await screen.findByRole("button", { name: "Delete visit" }));
    expect(screen.getByText("This visit also has notes for Olive. They're deleted with it.")).toBeTruthy();
    await waitFor(() => expect(document.querySelector("audio")).not.toBeNull());
    const player = document.querySelector("audio")!;
    expect(player.getAttribute("src")).toBe(`/api/sessions/${view.id}/audio`);
    mocked.deleteVisit.mockImplementationOnce(async () => {
      // The player stopped streaming the recording before the files moved.
      expect(player.getAttribute("src")).toBeNull();
      return {
        trash_id: "0199a7f0-1c2d-7e3f-8a9b-0c1d2e3f4a5b",
        pets: ["Olive", "Pickles"],
        signed: false,
        purge_after: "2026-11-17 10:20",
        patient_id: "p-olive",
      };
    });
    fireEvent.click(screen.getByRole("menuitem", { name: "Recorded by mistake" }));
    await waitFor(() => expect(mocked.deleteVisit).toHaveBeenCalledWith(view.id, "Dr Rivera", { kind: "mistake" }));
    expect(HTMLMediaElement.prototype.pause).toHaveBeenCalled();
    await waitFor(() => expect(c.navigate).toHaveBeenCalledWith({ screen: "patient", id: "p-pickles" }));
    const [message, action] = vi.mocked(c.toast).mock.calls[0]!;
    expect(message).toBe("Visit deleted");
    expect(action?.label).toBe("Undo");
    action?.run();
    await waitFor(() =>
      expect(mocked.restoreVisit).toHaveBeenCalledWith("0199a7f0-1c2d-7e3f-8a9b-0c1d2e3f4a5b", "Dr Rivera"),
    );
    await waitFor(() => expect(c.toast).toHaveBeenCalledWith("Visit restored"));
    expect(c.navigate).toHaveBeenLastCalledWith({ screen: "review", id: view.id, pet: "Pickles" });
  });

  it("shows no Delete visit or Delete recording to reception", async () => {
    show(sessionView(), ctx({ can: { notes: false, sign: false, team: false, admin: false } }));
    await screen.findByText("Ear check");
    expect(screen.queryByRole("button", { name: "Delete visit" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Delete recording" })).toBeNull();
  });

  it("disables Delete visit on a signed visit, with the reason", async () => {
    show(sessionView({ any_signed: true }), ctx());
    const button = (await screen.findByRole("button", { name: "Delete visit" })) as HTMLButtonElement;
    expect(button.disabled).toBe(true);
    expect(button.closest("[title]")?.getAttribute("title")).toBe(
      "Signed visits can't be deleted yet. Add an addendum to correct the note.",
    );
  });

  it("offers Delete recording under the player; a technician can't on a signed visit", async () => {
    show(sessionView({ any_signed: true }), ctx({ can: { notes: true, sign: false, team: false, admin: false } }));
    const button = (await screen.findByRole("button", { name: "Delete recording" })) as HTMLButtonElement;
    expect(button.disabled).toBe(true);
    expect(button.closest("[title]")?.getAttribute("title")).toBe(
      "Only a veterinarian can delete the recording of a signed visit.",
    );
  });

  it("deletes the recording after the dialog, and shows the line in the player's place", async () => {
    const c = ctx();
    const before = sessionView({ pets: [], pet: null, patient: { name: "Maple", last_name: "Okonkwo", reason: "Limping" } });
    show(before, c, "");
    fireEvent.click(await screen.findByRole("button", { name: "Delete recording" }));
    expect(screen.getByRole("heading", { name: "Delete this recording? The note stays." })).toBeTruthy();
    const at = "2026-10-18T14:05:00+00:00";
    mocked.deleteRecording.mockResolvedValue(before);
    mocked.openSession.mockResolvedValue({ ...before, has_audio: false, audio_deleted: { at, by: { name: "Sam" } } });
    fireEvent.click(screen.getAllByRole("button", { name: "Delete recording" }).at(-1)!);
    await waitFor(() => expect(mocked.deleteRecording).toHaveBeenCalledWith(before.id, "Dr Rivera"));
    await waitFor(() => expect(c.toast).toHaveBeenCalledWith("Recording deleted"));
    expect(
      await screen.findByText(
        `Recording deleted ${engine.fmtStamp(at)} by Sam. The transcript and notes are kept.`,
      ),
    ).toBeTruthy();
    expect(document.querySelector("audio")).toBeNull();
    // The dialog closes with its exit motion; then no Delete recording is left.
    await waitFor(() => expect(screen.queryByRole("button", { name: "Delete recording" })).toBeNull());
  });

  it("with the recording already deleted there's no player", async () => {
    const at = "2026-10-18T09:00:00+00:00";
    show(sessionView({ has_audio: false, audio_deleted: { at, by: { name: "Dr Rivera" } } }), ctx());
    expect(
      await screen.findByText(
        `Recording deleted ${engine.fmtStamp(at)} by Dr Rivera. The transcript and notes are kept.`,
      ),
    ).toBeTruthy();
    expect(document.querySelector("audio")).toBeNull();
  });
});
