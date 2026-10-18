/**
 * Species OpenBark recognises, for the patient's details and for headings
 * shown only for some species (M2 item 4). Keys match `SPECIES` in
 * engine/os-engine/src/template.rs. Labels are translated where shown.
 */
export const SPECIES: { key: string; one: string; many: string }[] = [
  { key: "canine", one: "Dog", many: "dogs" },
  { key: "feline", one: "Cat", many: "cats" },
  { key: "equine", one: "Horse", many: "horses" },
  { key: "bovine", one: "Cattle", many: "cattle" },
  { key: "small_ruminant", one: "Sheep or goat", many: "sheep and goats" },
  { key: "porcine", one: "Pig", many: "pigs" },
  { key: "avian", one: "Bird", many: "birds" },
  { key: "reptile", one: "Reptile", many: "reptiles" },
  { key: "rabbit", one: "Rabbit", many: "rabbits" },
  { key: "rodent", one: "Small rodent", many: "small rodents" },
  { key: "ferret", one: "Ferret", many: "ferrets" },
];

/** The "Show …" choices for a heading, as sentences. */
export const SHOW_FOR: { label: string; species: string[] }[] = [
  { label: "for every species", species: [] },
  ...SPECIES.map((s) => ({ label: `only for ${s.many}`, species: [s.key] })),
  { label: "only for dogs and cats", species: ["canine", "feline"] },
  { label: "only for farm animals", species: ["bovine", "small_ruminant", "porcine"] },
  { label: "only for exotics", species: ["avian", "reptile", "rabbit", "rodent", "ferret"] },
];

/** A stable value for a set of species ("canine,feline"). */
export const speciesValue = (species: string[] | undefined) => [...(species ?? [])].sort().join(",");

/** "cats", or the key itself for a species the app doesn't list. */
export const speciesPlural = (key: string) => SPECIES.find((s) => s.key === key)?.many ?? key;

/** "Cat" for the patient's details. */
export const speciesName = (key: string) => SPECIES.find((s) => s.key === key)?.one ?? key;
