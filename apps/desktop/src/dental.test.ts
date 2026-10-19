import { describe, expect, it } from "vitest";
import {
  CHART,
  findingText,
  glyphMarks,
  hasTooth,
  legendLines,
  misfit,
  printed,
  rowsOf,
  teethOf,
  toothName,
  type ToothFinding,
} from "./dental";

const t = (s: string, vars?: Record<string, string | number>) => s.replace(/\{(\w+)\}/g, (m, k: string) => (vars && k in vars ? String(vars[k]) : m));
const item = (tooth: string, finding: string, extra: Partial<ToothFinding> = {}): ToothFinding => ({
  tooth,
  text: "as said",
  finding,
  detail: null,
  grade: null,
  mm: null,
  planned: false,
  evidence: [1],
  ...extra,
});

describe("the shared chart data (checked against the AVDC Triadan table)", () => {
  const count = (d: "canine" | "feline", deciduous: boolean) => teethOf(d).filter((x) => x.deciduous === deciduous).length;

  it("has the AVDC tooth counts", () => {
    expect(count("canine", false)).toBe(42);
    expect(count("canine", true)).toBe(28);
    expect(count("feline", false)).toBe(30);
    expect(count("feline", true)).toBe(26);
    expect(teethOf(null)).toEqual(teethOf("canine"));
  });

  it("keeps every cat tooth a dog tooth, with the same number", () => {
    const dog = new Set(teethOf("canine").map((x) => x.id));
    for (const x of teethOf("feline")) expect(dog.has(x.id), x.id).toBe(true);
    for (const absent of ["105", "205", "305", "306", "405", "406", "706", "806"]) expect(hasTooth("feline", absent), absent).toBe(false);
    expect(CHART.teeth.some((x) => x.deciduous && x.id.endsWith("05"))).toBe(false);
  });

  it("follows the Triadan rules: x04 canine, x09 first molar, four carnassials", () => {
    for (const x of CHART.teeth) {
      if (x.id.endsWith("04")) expect(x.type, x.id).toBe("canine");
      if (x.id.endsWith("09")) expect([x.type, x.ordinal], x.id).toEqual(["molar", 1]);
    }
    expect(CHART.teeth.filter((x) => x.shape === "carnassial").map((x) => x.id)).toEqual(["108", "208", "309", "409"]);
  });

  it("lists each row's teeth once, patient's right first", () => {
    for (const d of ["canine", "feline"] as const) {
      for (const row of rowsOf(d, true)) {
        expect(new Set(row.teeth).size, `${d} ${row.name}`).toBe(row.teeth.length);
        const half = row.teeth.length / 2;
        expect(row.teeth[half - 1]?.endsWith("01") && row.teeth[half]?.endsWith("01"), `${d} ${row.name}`).toBe(true);
      }
      const drawn = rowsOf(d, true).flatMap((r) => r.teeth).sort();
      expect(drawn).toEqual(teethOf(d).map((x) => x.id).sort());
    }
    expect(rowsOf("canine", false).map((r) => r.teeth.length)).toEqual([20, 22]);
  });

  it("gives every type a code and never prints PD alone", () => {
    for (const f of CHART.findings) {
      for (const d of f.details) expect(d.code, `${f.id}/${d.id}`).not.toBe("");
      for (const code of [f.code, f.code_grade, f.code_detail]) expect(code).not.toBe("PD");
    }
    expect(CHART.findings).toHaveLength(20);
  });
});

describe("printed words and AVDC codes", () => {
  it("derives codes from the table", () => {
    expect(findingText(t, item("309", "periodontal_stage", { grade: 3 }))).toBe("Periodontal disease stage 3 (PD3)");
    expect(findingText(t, item("309", "mobility", { grade: 2 }))).toBe("Mobility 2 (M2)");
    expect(findingText(t, item("108", "fracture", { detail: "complicated_crown" }))).toBe("Complicated crown fracture (T/FX/CCF)");
    expect(findingText(t, item("304", "fracture"))).toBe("Fracture (T/FX)");
    expect(findingText(t, item("307", "resorption", { grade: 4, detail: "crown_worse" }))).toBe("Tooth resorption stage 4b (TR4b)");
    expect(findingText(t, item("307", "resorption_type", { grade: 2 }))).toBe("Resorption type 2 (T2)");
    expect(findingText(t, item("108", "extraction", { detail: "open" }))).toBe("Open extraction (XSS)");
    expect(findingText(t, item("304", "extraction", { planned: true }))).toBe("Planned: extraction");
    expect(findingText(t, item("304", "extraction", { planned: true }), false)).toBe("Planned: Extraction");
    const depth = printed(item("309", "probing_depth", { mm: 7 }));
    expect([findingText(t, item("309", "probing_depth", { mm: 7 })), depth.code]).toEqual(["Probing depth 7 mm", null]);
    expect(findingText(t, item("109", "gingival_recession", { mm: 2.5 }))).toBe("Gingival recession 2.5 mm (GR)");
    expect(findingText(t, item("209", "missing"))).toBe("Missing");
    expect(findingText(t, item("104", "other", { text: "Enamel hypoplasia" }))).toBe("Enamel hypoplasia");
    // Fields that don't fit print as said, with no code.
    expect(findingText(t, item("108", "fracture", { detail: "open" }))).toBe("as said");
  });

  it("refuses fields that don't fit their finding", () => {
    expect(misfit(item("309", "mobility", { grade: 4 }))).toBe("Mobility takes a number from 0 to 3.");
    expect(misfit(item("108", "fracture", { detail: "open" }))).toMatch(/can't be "open"/);
    expect(misfit(item("309", "probing_depth"))).toMatch(/millimetres/);
    expect(misfit(item("307", "resorption", { grade: 3, detail: "crown_worse" }))).toMatch(/only for stage 4/);
    expect(misfit(item("108", "fracture", { planned: true }))).toMatch(/can't be planned/);
    expect(misfit(item("309", "mobility", { grade: 3 }))).toBeNull();
  });

  it("names teeth from short parts", () => {
    expect(toothName(t, "108")).toBe("right maxillary fourth premolar");
    expect(toothName(t, "309")).toBe("left mandibular first molar");
    expect(toothName(t, "704")).toBe("left mandibular deciduous canine");
    expect(toothName(t, "999")).toBe("999");
  });

  it("lists only the codes used in the legend", () => {
    const lines = legendLines(t, [
      item("108", "fracture", { detail: "complicated_crown" }),
      item("309", "mobility", { grade: 2 }),
      item("309", "mobility", { grade: 1 }),
      item("209", "missing"),
      item("109", "gingival_recession", { mm: 2 }),
    ]);
    expect(lines).toEqual([
      "T/FX/CCF: complicated crown fracture",
      "M0-M3: tooth mobility index (AVDC). Grades as said.",
      "GR: gingival recession",
    ]);
  });

  it("draws a mark for every glyph but normal", () => {
    for (const g of ["circle", "x", "zigzag", "notch", "neck", "dot"] as const) expect(glyphMarks(g).length, g).toBeGreaterThan(0);
    expect(glyphMarks("none")).toEqual([]);
  });
});
