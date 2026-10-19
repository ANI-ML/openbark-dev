import { describe, expect, it } from "vitest";
import type { TemplateSummary, TemplateView } from "./engine";
import { translate } from "./i18n";
import { badges, draftLabel, editorNotice, originalNotice } from "./templateLabels";

const t = (text: string, vars?: Record<string, string | number>) => translate("en", text, vars);
const fr = (text: string, vars?: Record<string, string | number>) => translate("fr", text, vars);

function summary(over: Partial<TemplateSummary>): TemplateSummary {
  return {
    id: "soap",
    name: "SOAP note",
    description: "",
    version: "1",
    builtin: true,
    document: "record",
    headings: 18,
    standards: 0,
    layer: "builtin",
    edited: false,
    shared: true,
    original_changed: null,
    ...over,
  };
}

const text = (s: TemplateSummary, mode: "alone" | "personal" | "hospital") => badges(t, s, mode).map((b) => b.text);

describe("template list badges (ADR-0024 section 14.3)", () => {
  it("names each layer, and a newer original", () => {
    expect(text(summary({}), "personal")).toEqual([]);
    expect(text(summary({ layer: "personal", edited: true }), "personal")).toEqual(["Your version"]);
    expect(text(summary({ id: "mine", builtin: false, layer: "personal", edited: false, shared: false }), "personal")).toEqual([
      "Only you",
    ]);
    expect(text(summary({ layer: "team", edited: true }), "personal")).toEqual(["Edited"]);
    expect(text(summary({ layer: "team", edited: true }), "alone")).toEqual(["Edited"]);
    expect(text(summary({ id: "dental-x", builtin: false, layer: "team" }), "personal")).toEqual(["Hospital"]);
    expect(text(summary({ id: "dental-x", builtin: false, layer: "team" }), "alone")).toEqual(["Yours"]);
    const newer = summary({
      layer: "personal",
      edited: true,
      original_changed: { reason: "newer_builtin", name: "SOAP note", version: "0.2.0", fingerprint: "f" },
    });
    expect(badges(t, newer, "personal")).toEqual([
      { text: "Your version", tone: "neutral" },
      { text: "Newer original", tone: "warn" },
    ]);
    // A deleted original isn't "newer".
    expect(text(summary({ layer: "personal", edited: true, original_changed: { reason: "team_deleted", name: "x", version: "", fingerprint: "" } }), "personal")).toEqual([
      "Your version",
    ]);
  });
});

describe("which version drafted a note (ADR-0024 section 10)", () => {
  const tpl = (origin: object | null) => ({ id: "soap", version: "3", name: "SOAP note", sections: [], origin: origin as never });
  it("reads as the plan says", () => {
    expect(draftLabel(t, tpl({ layer: "personal", version: "3", fingerprint: "", owner: { id: "me", name: "Sam" } }), "me", "personal")).toBe(
      "SOAP note, your version 3",
    );
    expect(draftLabel(t, tpl({ layer: "personal", version: "2", fingerprint: "", owner: { id: "x", name: "Sam" } }), "me", "personal")).toBe(
      "SOAP note, Sam's version 2",
    );
    expect(draftLabel(t, tpl({ layer: "team", version: "4", fingerprint: "" }), "me", "personal")).toBe("SOAP note, the hospital's version 4");
    expect(
      draftLabel(t, tpl({ layer: "team", version: "1", fingerprint: "", based_on: { layer: "builtin", id: "soap", version: "0.1.1", fingerprint: "" } }), null, "alone"),
    ).toBe("SOAP note, edited version 1");
    expect(draftLabel(t, tpl({ layer: "builtin", version: "0.1.1", fingerprint: "" }), null, "alone")).toBe("SOAP note 0.1.1");
    // Runs saved before this change show as before: no label.
    expect(draftLabel(t, tpl(null), null, "alone")).toBeNull();
  });

  it("translates a built-in's name", () => {
    expect(draftLabel(fr, tpl({ layer: "builtin", version: "0.1.1", fingerprint: "" }), null, "alone")).not.toContain("SOAP note 0");
  });
});

function view(over: Partial<TemplateView>): TemplateView {
  return {
    template: { id: "soap", version: "0.1.1", name: "SOAP note", sections: [] },
    builtin: true,
    prompt: "",
    layer: "builtin",
    can: {
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
    },
    always_applied: { core: [], fixed: [], checks: [] },
    lint: [],
    mode: "personal",
    ...over,
  };
}

describe("the editor's notice (ADR-0024 section 14.2)", () => {
  it("says where changes go, for each state", () => {
    expect(editorNotice(t, view({}), false)).toBe("Changes are saved as your version. Everyone else keeps the original.");
    expect(editorNotice(t, view({ mode: "alone", can: { ...view({}).can, save_to: "team" } }), false)).toBe(
      "Changes are saved as your version. Reset to original brings back the built-in at any time.",
    );
    expect(
      editorNotice(
        t,
        view({
          layer: "personal",
          template: { id: "soap", version: "3", name: "SOAP note", sections: [] },
          original: { layer: "builtin", id: "soap", name: "SOAP note", version: "0.1.1", fingerprint: "" },
          origin: { layer: "personal", version: "3", fingerprint: "", based_on: { layer: "builtin", id: "soap", version: "0.1.0", fingerprint: "" } },
        }),
        false,
      ),
    ).toBe("Your version 3, from SOAP note 0.1.0. Only you draft with it.");
    expect(editorNotice(t, view({ layer: "personal", builtin: false }), false)).toBe("Only you draft with this template.");
    expect(editorNotice(t, view({ can: { ...view({}).can, edit: false } }), false)).toBe(
      "Templates are changed by the people who draft notes. You can read every template and what the model is told.",
    );
    expect(editorNotice(t, view({ mode: "hospital", can: { ...view({}).can, edit: false } }), false)).toContain("shared by the hospital");
    expect(editorNotice(t, view({}), true)).toBeNull();
  });

  it("says what happened to the original", () => {
    const changed = (reason: "newer_builtin" | "team_changed" | "team_reset" | "team_deleted" | "no_longer_builtin") =>
      originalNotice(t, view({ original_changed: { reason, name: "SOAP note", version: "0.2.0", fingerprint: "f" } }));
    expect(changed("newer_builtin")).toBe("A newer SOAP note is built in (version 0.2.0). Your version is unchanged.");
    expect(changed("team_changed")).toBe("The hospital changed SOAP note (hospital version 0.2.0). Your version is unchanged.");
    expect(changed("team_deleted")).toBe("The hospital template this came from was deleted. Your version still works.");
    expect(changed("no_longer_builtin")).toBe("This template is no longer built in. Your version still works.");
  });
});
