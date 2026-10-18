import { describe, expect, it } from "vitest";
import type { Template } from "./engine";
import { examplesForTitle, headingExamples } from "./promptExamples";

const dental: Template = {
  id: "dental",
  version: "0.1.0",
  name: "Dental procedure",
  sections: [
    { id: "findings_by_tooth", title: "Findings by tooth", kind: "statements", examples: ["Use the Triadan number for every tooth"] },
    { id: "plan", title: "Plan", parts: [{ key: "medications", label: "Medications", kind: "medications" }] },
    { id: "misc", title: "Miscellany", kind: "statements" },
  ],
};

describe("prompt examples", () => {
  it("uses a heading's own examples first", () => {
    expect(headingExamples(dental, "findings_by_tooth")).toEqual(["Use the Triadan number for every tooth"]);
  });

  it("falls back to examples matched on the title, for subheadings too", () => {
    expect(headingExamples(dental, "plan/medications")[0]).toMatch(/tablet strength/);
  });

  it("offers nothing for a heading it can't place", () => {
    expect(headingExamples(dental, "misc")).toEqual([]);
    expect(headingExamples(dental, "missing")).toEqual([]);
    expect(headingExamples(null, "plan")).toEqual([]);
  });

  it("prefers specific matches over general ones", () => {
    expect(examplesForTitle("Dental exam")[0]).toMatch(/Triadan/);
    expect(examplesForTitle("Lameness exam")[0]).toMatch(/lameness grade/);
    expect(examplesForTitle("Diagnostic results")[0]).toMatch(/unit/);
  });
});
