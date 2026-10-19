import { createLevelSource } from "@openbark/ui";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AppContext, loadSettings, type Ctx } from "../context";
import * as engine from "../engine";
import { CompareSheet } from "./CompareSheet";
import { RefinePanel } from "./RefinePanel";
import { TeamSettings } from "./TeamSettings";

vi.mock("../engine", async (importOriginal) => {
  const real = await importOriginal<typeof import("../engine")>();
  return {
    ...real,
    compareTemplate: vi.fn(),
    listUsers: vi.fn(),
    teamSettings: vi.fn(),
    profileSummary: vi.fn(),
    removeUser: vi.fn(),
  };
});

const mocked = vi.mocked(engine);

function ctx(over: Partial<Ctx> = {}): Ctx {
  return {
    route: { screen: "settings" },
    navigate: vi.fn(),
    status: { team: true, templates: "personal" } as engine.Status,
    refreshStatus: vi.fn(),
    settings: { ...loadSettings(), uiLanguage: "en" },
    setSettings: vi.fn(),
    toast: vi.fn(),
    announce: vi.fn(),
    levels: createLevelSource(),
    user: { id: "u-vet", name: "Dr Rivera", role: "dvm", admin: true },
    setUser: vi.fn(),
    actor: "u-vet",
    actorName: "Dr Rivera",
    can: { notes: true, sign: true, team: true, admin: true },
    ...over,
  };
}

beforeEach(() => vi.clearAllMocks());
afterEach(cleanup);

describe("CompareSheet", () => {
  it("lists what changed in the original, by heading, then the whole note", async () => {
    mocked.compareTemplate.mockResolvedValue({
      headings: [
        { path: "pain_score", title: "Pain score", changes: [{ kind: "added" }] },
        { path: "plan", title: "Plan", changes: [{ kind: "instruction_changed", from: "Old words.", to: "New words." }] },
      ],
      note: [{ field: "detail", from: "standard", to: "concise" }],
    });
    const onKeep = vi.fn();
    render(
      <CompareSheet templateId="soap" hasBase originalChanged canKeep onKeep={onKeep} onUseOriginal={vi.fn()} onClose={vi.fn()} />,
    );
    expect(await screen.findByText("Pain score")).toBeTruthy();
    expect(screen.getByText("Added")).toBeTruthy();
    expect(screen.getByText("Old words.")).toBeTruthy();
    expect(screen.getByText("New words.")).toBeTruthy();
    expect(screen.getByText("Level of detail")).toBeTruthy();
    expect(mocked.compareTemplate).toHaveBeenCalledWith("soap", "base");
    fireEvent.click(screen.getByRole("tab", { name: "Yours and the original" }));
    await waitFor(() => expect(mocked.compareTemplate).toHaveBeenLastCalledWith("soap", "original"));
    fireEvent.click(screen.getByRole("button", { name: "Keep mine" }));
    expect(onKeep).toHaveBeenCalledTimes(1);
  });

  it("says when there are no differences", async () => {
    mocked.compareTemplate.mockResolvedValue({ headings: [], note: [] });
    render(<CompareSheet templateId="soap" hasBase={false} originalChanged={false} canKeep={false} onKeep={vi.fn()} onUseOriginal={vi.fn()} onClose={vi.fn()} />);
    expect(await screen.findByText("No differences.")).toBeTruthy();
  });
});

describe("RefinePanel in a team", () => {
  it("saves a standard for me, or for the hospital with an admin's PIN", () => {
    const onSubmit = vi.fn();
    render(
      <AppContext.Provider value={ctx()}>
        <RefinePanel
          title="Plan"
          templateName="SOAP note"
          standards={[]}
          canSave
          busy={false}
          board={null}
          scopes={{ personal: true, teamNeedsPin: true, me: { id: "u-vet", name: "Dr Rivera", role: "dvm", admin: true } }}
          onEditHeading={vi.fn()}
          onSubmit={onSubmit}
          onCancel={() => {}}
        />
      </AppContext.Provider>,
    );
    fireEvent.change(screen.getByLabelText("What should change under Plan?"), { target: { value: "Fluids with rate" } });
    fireEvent.click(screen.getByLabelText(/Save as a standard/));
    fireEvent.click(screen.getByRole("button", { name: "Redraft Plan" }));
    expect(onSubmit).toHaveBeenLastCalledWith("Fluids with rate", true, { scope: "personal", approver: null });
    fireEvent.click(screen.getByLabelText("For the hospital"));
    const redraft = screen.getByRole("button", { name: "Redraft Plan" }) as HTMLButtonElement;
    expect(redraft.disabled).toBe(true);
    fireEvent.change(screen.getByLabelText("Admin's PIN"), { target: { value: "2468" } });
    fireEvent.click(redraft);
    expect(onSubmit).toHaveBeenLastCalledWith("Fluids with rate", true, { scope: "team", approver: { id: "u-vet", pin: "2468" } });
    expect(screen.getByRole("button", { name: "Edit this heading in the template" })).toBeTruthy();
  });
});

describe("TeamSettings", () => {
  it("says how many versions go with a removed member", async () => {
    mocked.listUsers.mockResolvedValue([
      { id: "u-vet", name: "Dr Rivera", role: "dvm", admin: true },
      { id: "u-sam", name: "Sam", role: "rvt", admin: false },
    ]);
    mocked.teamSettings.mockResolvedValue({ templates: "personal", setup_notice: false });
    mocked.profileSummary.mockResolvedValue({ versions: 2, own_templates: 1, standards: 4 });
    render(
      <AppContext.Provider value={ctx()}>
        <TeamSettings />
      </AppContext.Provider>,
    );
    expect(await screen.findByText("Admin")).toBeTruthy();
    expect(screen.getByRole("radio", { name: "Each person's own" }).getAttribute("aria-checked")).toBe("true");
    fireEvent.click(screen.getByRole("button", { name: "Remove Sam" }));
    const alert = await screen.findByRole("alert");
    expect(
      within(alert).getByText("Sam has their own version of 3 templates. They're removed with Sam. Notes keep the version that drafted them."),
    ).toBeTruthy();
    expect(mocked.profileSummary).toHaveBeenCalledWith("u-sam");
  });

  it("asks for an admin's PIN to switch to the hospital's templates", async () => {
    mocked.listUsers.mockResolvedValue([{ id: "u-vet", name: "Dr Rivera", role: "dvm", admin: true }]);
    mocked.teamSettings.mockResolvedValue({ templates: "personal", setup_notice: false });
    render(
      <AppContext.Provider value={ctx()}>
        <TeamSettings />
      </AppContext.Provider>,
    );
    fireEvent.click(await screen.findByRole("radio", { name: "Shared by the hospital" }));
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByText("Dr Rivera, enter your PIN to approve.")).toBeTruthy();
    expect(within(dialog).getByLabelText("Admin's PIN")).toBeTruthy();
  });
});
