import { createLevelSource } from "@openbark/ui";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AppContext, loadSettings, type Ctx } from "../context";
import * as engine from "../engine";
import { Templates } from "./Templates";

vi.mock("../engine", async (importOriginal) => {
  const real = await importOriginal<typeof import("../engine")>();
  return {
    ...real,
    listTemplates: vi.fn(),
    template: vi.fn(),
    previewTemplate: vi.fn(),
    saveTemplate: vi.fn(),
    resetTemplate: vi.fn(),
    undoResetTemplate: vi.fn(),
    shareTemplate: vi.fn(),
    listSessions: vi.fn().mockResolvedValue([]),
    listUsers: vi.fn().mockResolvedValue([]),
    teamSettings: vi.fn().mockResolvedValue({ templates: "personal", setup_notice: false }),
  };
});

const mocked = vi.mocked(engine);

const CORE = [
  "Only what was said. No findings, diagnoses, vitals, doses or plans that weren't said.",
  "Anything not discussed shows as not discussed. Never \"normal\" unless it was said.",
  "Numbers are copied exactly as said, with their units.",
  "Patient details only if someone said them. The species is never guessed.",
  "Every fact points to the line where it was said.",
  "Speech-recognition hints never add a drug on their own.",
  "Every line links to the recording.",
  "One clinical fact or action per line.",
];

const soap: engine.Template = {
  id: "soap",
  version: "0.1.1",
  name: "SOAP note",
  instructions: "Write the medical record as a SOAP note.",
  fixed_rules: ["Differentials only if the veterinarian voiced them."],
  rules: ["Record diagnostics the client declined or deferred."],
  sections: [{ id: "plan", title: "Plan", kind: "statements", instruction: "Treatment and follow-up." }],
};

const editable: engine.TemplateCan = {
  edit: true,
  save_to: "personal",
  save_needs_pin: false,
  save_for_hospital: false,
  reset: false,
  share: false,
  reset_team: false,
  delete: false,
  delete_needs_pin: false,
  keep: false,
  keep_needs_pin: false,
  duplicate: true,
  change_document: false,
  standards_personal: true,
  standards_team_needs_pin: true,
  admin: false,
};

function view(over: Partial<engine.TemplateView> = {}): engine.TemplateView {
  return {
    template: soap,
    builtin: true,
    prompt: "Rules that always apply:\n- Use only information stated in the transcript.",
    layer: "builtin",
    can: editable,
    always_applied: { core: CORE.map((summary, i) => ({ text: `core ${i}`, summary })), fixed: soap.fixed_rules ?? [], checks: ["A note stays a draft until a veterinarian signs it."] },
    lint: [],
    mode: "personal",
    ...over,
  };
}

function ctx(over: Partial<Ctx> = {}): Ctx {
  return {
    route: { screen: "templates" },
    navigate: vi.fn(),
    status: { team: true, templates: "personal" } as engine.Status,
    refreshStatus: vi.fn(),
    settings: { ...loadSettings(), uiLanguage: "en" },
    setSettings: vi.fn(),
    toast: vi.fn(),
    announce: vi.fn(),
    levels: createLevelSource(),
    user: { id: "u-sam", name: "Sam", role: "rvt", admin: false },
    setUser: vi.fn(),
    actor: "u-sam",
    actorName: "Sam",
    can: { notes: true, sign: false, team: false, admin: false },
    ...over,
  };
}

function open(c: Ctx, v: engine.TemplateView, preview?: engine.TemplateView) {
  mocked.template.mockResolvedValue(v);
  mocked.previewTemplate.mockResolvedValue(preview ?? v);
  return render(
    <AppContext.Provider value={c}>
      <Templates initialOpen="soap" />
    </AppContext.Provider>,
  );
}

beforeEach(() => vi.clearAllMocks());
afterEach(cleanup);

describe("Templates: the list", () => {
  it("shows a badge for each layer and reloads for another person", async () => {
    mocked.listTemplates.mockResolvedValue([
      { id: "soap", name: "SOAP note", description: "", version: "1", builtin: true, document: "record", headings: 18, standards: 0, layer: "personal", edited: true, shared: true },
      { id: "dental", name: "Dental procedure", description: "", version: "0.1.1", builtin: true, document: "record", headings: 9, standards: 0, layer: "builtin", edited: false, shared: true, original_changed: null },
      { id: "sams", name: "Sam's recheck", description: "", version: "1", builtin: false, document: "record", headings: 3, standards: 0, layer: "personal", edited: false, shared: false },
    ]);
    const c = ctx();
    const { rerender } = render(
      <AppContext.Provider value={c}>
        <Templates />
      </AppContext.Provider>,
    );
    expect(await screen.findByText("Your version")).toBeTruthy();
    const only = screen.getByRole("region", { name: "Only you" });
    expect(within(only).getByText("Sam's recheck")).toBeTruthy();
    expect(within(only).getAllByText("Only you").length).toBeGreaterThan(0);
    expect(mocked.listTemplates).toHaveBeenCalledTimes(1);
    rerender(
      <AppContext.Provider value={ctx({ user: { id: "u-vet", name: "Dr Rivera", role: "dvm", admin: true }, actor: "u-vet" })}>
        <Templates />
      </AppContext.Provider>,
    );
    await waitFor(() => expect(mocked.listTemplates).toHaveBeenCalledTimes(2));
  });

  it("offers reception no New template", async () => {
    mocked.listTemplates.mockResolvedValue([]);
    render(
      <AppContext.Provider value={ctx({ user: { id: "u-lee", name: "Lee", role: "reception" }, can: { notes: false, sign: false, team: false, admin: false } })}>
        <Templates />
      </AppContext.Provider>,
    );
    await waitFor(() => expect(mocked.listTemplates).toHaveBeenCalled());
    expect(screen.queryByRole("button", { name: "New template" })).toBeNull();
  });
});

describe("Templates: the editor", () => {
  it("saves an unchanged built-in as your version once something changes", async () => {
    const c = ctx();
    open(c, view());
    const save = (await screen.findByRole("button", { name: "Save my version" })) as HTMLButtonElement;
    expect(save.disabled).toBe(true);
    expect(screen.getByText("Changes are saved as your version. Everyone else keeps the original.")).toBeTruthy();
    fireEvent.change(screen.getByLabelText("Instruction for Plan"), { target: { value: "Rechecks in days." } });
    await waitFor(() => expect((screen.getByRole("button", { name: "Save my version" }) as HTMLButtonElement).disabled).toBe(false));
    mocked.saveTemplate.mockResolvedValue(
      view({
        layer: "personal",
        template: { ...soap, version: "1" },
        original: { layer: "builtin", id: "soap", name: "SOAP note", version: "0.1.1", fingerprint: "f" },
        can: { ...editable, reset: true, share: true },
      }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Save my version" }));
    await waitFor(() => expect(c.toast).toHaveBeenCalledWith("Saved as your version 1"));
    const sent = mocked.saveTemplate.mock.calls[0]![0];
    expect(sent.base_version).toBe("0.1.1");
    expect(sent.base_layer).toBe("builtin");
    expect(sent.hospital).toBe(false);
    expect(await screen.findByRole("button", { name: "Reset to original" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Share with the hospital" })).toBeTruthy();
  });

  it("lists the rules that always apply, with no inputs", async () => {
    open(ctx(), view());
    fireEvent.click(await screen.findByText("Instructions and rules for the whole note"));
    const panel = screen.getByRole("region", { name: "Always applied" });
    for (const summary of CORE) expect(within(panel).getByText(summary)).toBeTruthy();
    expect(within(panel).getByText("Differentials only if the veterinarian voiced them.")).toBeTruthy();
    expect(panel.querySelectorAll("input, textarea, select").length).toBe(0);
  });

  it("warns about an instruction that conflicts, and still lets you save", async () => {
    const warned = view({ lint: [{ path: "headings/plan", text: "normal", reason: "unsaid_findings" }] });
    open(ctx(), view(), warned);
    fireEvent.change(await screen.findByLabelText("Instruction for Plan"), { target: { value: "Assume normal vitals." } });
    expect(await screen.findByText(/OpenBark won't do this/)).toBeTruthy();
    expect((screen.getByRole("button", { name: "Save my version" }) as HTMLButtonElement).disabled).toBe(false);
  });

  it("is read-only for reception", async () => {
    open(
      ctx({ user: { id: "u-lee", name: "Lee", role: "reception" }, can: { notes: false, sign: false, team: false, admin: false } }),
      view({ can: { ...editable, edit: false, save_to: null, duplicate: false } }),
    );
    const plan = (await screen.findByLabelText("Instruction for Plan")) as HTMLTextAreaElement;
    expect(plan.disabled).toBe(true);
    expect(screen.getByText("Templates are changed by the people who draft notes. You can read every template and what the model is told.")).toBeTruthy();
    expect(screen.queryByRole("button", { name: /Save/ })).toBeNull();
    expect(screen.queryByRole("button", { name: "Duplicate" })).toBeNull();
  });

  it("keeps your changes on screen when the template changed meanwhile", async () => {
    open(ctx(), view());
    fireEvent.change(await screen.findByLabelText("Instruction for Plan"), { target: { value: "Mine." } });
    mocked.saveTemplate.mockRejectedValue(new Error("This template changed while you were editing. Your changes are still here. Compare, then save again."));
    await waitFor(() => expect((screen.getByRole("button", { name: "Save my version" }) as HTMLButtonElement).disabled).toBe(false));
    fireEvent.click(screen.getByRole("button", { name: "Save my version" }));
    expect(await screen.findByText(/changed while you were editing/)).toBeTruthy();
    expect((screen.getByLabelText("Instruction for Plan") as HTMLTextAreaElement).value).toBe("Mine.");
    expect(screen.getByRole("button", { name: "Save over it" })).toBeTruthy();
  });

  it("resets to the original with an Undo that runs once", async () => {
    const mine = view({
      layer: "personal",
      template: { ...soap, version: "2" },
      original: { layer: "builtin", id: "soap", name: "SOAP note", version: "0.1.1", fingerprint: "f" },
      can: { ...editable, reset: true, share: true },
    });
    const c = ctx();
    open(c, mine);
    mocked.resetTemplate.mockResolvedValue({ view: view(), undo: "tok" });
    mocked.undoResetTemplate.mockResolvedValue(mine);
    fireEvent.click(await screen.findByRole("button", { name: "Reset to original" }));
    await waitFor(() => expect(c.toast).toHaveBeenCalled());
    const [message, action] = vi.mocked(c.toast).mock.calls[0]!;
    expect(message).toBe("Reset to the original");
    action?.run();
    await waitFor(() => expect(mocked.undoResetTemplate).toHaveBeenCalledTimes(1));
    expect(mocked.undoResetTemplate).toHaveBeenCalledWith("soap", "tok");
  });

  it("shares with an admin's PIN, and keeps the dialog open on a wrong one", async () => {
    mocked.listUsers.mockResolvedValue([{ id: "u-vet", name: "Dr Rivera", role: "dvm", admin: true }]);
    open(
      ctx(),
      view({
        layer: "personal",
        template: { ...soap, version: "1" },
        original: { layer: "builtin", id: "soap", name: "SOAP note", version: "0.1.1", fingerprint: "f" },
        can: { ...editable, reset: true, share: true },
      }),
    );
    fireEvent.click(await screen.findByRole("button", { name: "Share with the hospital" }));
    const dialog = await screen.findByRole("dialog");
    await waitFor(() => expect(within(dialog).getByRole("option", { name: "Dr Rivera" })).toBeTruthy());
    fireEvent.change(within(dialog).getByLabelText("Admin approving"), { target: { value: "u-vet" } });
    fireEvent.change(within(dialog).getByLabelText("Admin's PIN"), { target: { value: "0000" } });
    mocked.shareTemplate.mockRejectedValueOnce(new Error("That PIN isn't right."));
    fireEvent.click(within(dialog).getByRole("button", { name: "Share with the hospital" }));
    expect(await within(dialog).findByText("That PIN isn't right.")).toBeTruthy();
    expect(mocked.shareTemplate).toHaveBeenCalledWith("soap", "replace", null, { id: "u-vet", pin: "0000" });
    expect(screen.getByRole("dialog")).toBeTruthy();
  });
});

describe("Templates: a new template", () => {
  function startNew(c: Ctx = ctx()) {
    mocked.listTemplates.mockResolvedValue([]);
    render(
      <AppContext.Provider value={c}>
        <Templates />
      </AppContext.Provider>,
    );
    return screen.findByRole("button", { name: "New template" }).then((b) => fireEvent.click(b));
  }
  const input = (label: string) => screen.getByLabelText(label) as HTMLInputElement;

  it("can be typed in before it has a name, even when the check fails", async () => {
    mocked.previewTemplate.mockRejectedValue(new Error("Give the template a name."));
    await startNew();
    expect(input("Name").disabled).toBe(false);
    expect(document.activeElement).toBe(input("Name"));
    expect(input("Heading 1 title").disabled).toBe(false);
    expect(await screen.findByText("Give the template a name.")).toBeTruthy();
    fireEvent.change(input("Name"), { target: { value: "Dental procedure" } });
    expect(input("Name").value).toBe("Dental procedure");
    expect(input("Name").disabled).toBe(false);
  });

  it("adds, moves and removes headings while a heading has no title", async () => {
    mocked.previewTemplate.mockRejectedValue(new Error("A heading has no title."));
    await startNew();
    fireEvent.click(screen.getByRole("button", { name: "Add heading" }));
    fireEvent.change(input("Heading 2 title"), { target: { value: "Teeth" } });
    fireEvent.click(screen.getByRole("button", { name: "Move Teeth up" }));
    expect(input("Heading 1 title").value).toBe("Teeth");
    expect(input("Heading 2 title").value).toBe("");
    fireEvent.click(screen.getByRole("button", { name: "Remove Heading 2" }));
    expect(screen.queryByLabelText("Heading 2 title")).toBeNull();
    expect(input("Heading 1 title").value).toBe("Teeth");
    expect(await screen.findByText("A heading has no title.")).toBeTruthy();
    expect(input("Heading 1 title").disabled).toBe(false);
    expect(screen.getByRole("button", { name: "Add heading" })).toBeTruthy();
  });

  it("keeps Save going to the same place while the draft is incomplete", async () => {
    const hospital: engine.TemplateCan = { ...editable, save_to: "team", save_needs_pin: true, standards_personal: false, admin: true };
    // The engine answers every check with what this person may do, and says what's missing.
    mocked.previewTemplate.mockImplementation(async (t) =>
      view({
        template: t,
        builtin: false,
        layer: "team",
        prompt: "",
        mode: "hospital",
        can: hospital,
        problem: !t.name.trim() ? "Give the template a name." : t.sections.some((s) => !s.title.trim()) ? "A heading has no title." : null,
      }),
    );
    await startNew(
      ctx({
        status: { team: true, templates: "hospital" } as engine.Status,
        user: { id: "u-vet", name: "Dr Rivera", role: "dvm", admin: true },
        actor: "u-vet",
        can: { notes: true, sign: true, team: true, admin: true },
      }),
    );
    const save = () => screen.getByRole("button", { name: "Save for the hospital" }) as HTMLButtonElement;
    expect(await screen.findByText("Give the template a name.")).toBeTruthy();
    expect(save().disabled).toBe(true);
    fireEvent.change(input("Name"), { target: { value: "Dental procedure" } });
    fireEvent.change(input("Heading 1 title"), { target: { value: "Teeth" } });
    await waitFor(() => expect(save().disabled).toBe(false));
    fireEvent.click(screen.getByRole("button", { name: "Add heading" }));
    expect(await screen.findByText("A heading has no title.")).toBeTruthy();
    expect(save().disabled).toBe(true);
    expect(input("Heading 2 title").disabled).toBe(false);
    fireEvent.click(screen.getByRole("button", { name: "Remove Heading 2" }));
    await waitFor(() => expect(save().disabled).toBe(false));
    fireEvent.change(input("Name"), { target: { value: "" } });
    expect(await screen.findByText("Give the template a name.")).toBeTruthy();
    expect(input("Name").disabled).toBe(false);
    fireEvent.change(input("Name"), { target: { value: "Dental procedure" } });
    await waitFor(() => expect(save().disabled).toBe(false));
    // The hospital's templates: an admin's PIN before anything is saved.
    fireEvent.click(save());
    expect(await screen.findByRole("dialog")).toBeTruthy();
    expect(mocked.saveTemplate).not.toHaveBeenCalled();
  });
});
