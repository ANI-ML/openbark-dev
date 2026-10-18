import { describe, expect, it } from "vitest";
import { SHOW_FOR, SPECIES, speciesPlural, speciesValue } from "./species";

describe("species", () => {
  it("offers every species on its own, and no choice twice", () => {
    for (const s of SPECIES) expect(SHOW_FOR.some((o) => speciesValue(o.species) === s.key)).toBe(true);
    const values = SHOW_FOR.map((o) => speciesValue(o.species));
    expect(new Set(values).size).toBe(values.length);
  });

  it("compares sets of species in any order", () => {
    expect(speciesValue(["feline", "canine"])).toBe(speciesValue(["canine", "feline"]));
    expect(speciesValue(undefined)).toBe("");
  });

  it("names species for sentences, and keeps unknown keys", () => {
    expect(speciesPlural("small_ruminant")).toBe("sheep and goats");
    expect(speciesPlural("axolotl")).toBe("axolotl");
  });
});
