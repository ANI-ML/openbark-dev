import type { Template } from "./engine";

/**
 * Example instructions, so a vet sees what a request can look like for their
 * own kind of work. Built-in templates carry examples per heading
 * (`examples:` in the YAML); headings a team made get examples matched on
 * their title. Examples are suggestions only; they reach the model when the
 * vet uses one.
 */

/** Examples for headings a team wrote, by what the title says (specific matches first). */
const BY_TITLE: { match: RegExp; examples: string[] }[] = [
  { match: /dental|teeth|tooth|oral/i, examples: ["Use the Triadan number for every tooth", "One tooth per line, in numerical order"] },
  { match: /lameness|gait|limb/i, examples: ["Give the lameness grade per limb", "List nerve blocks in order with the response"] },
  { match: /anaesth|anesth|sedation|monitor/i, examples: ["Separate premedication, induction and maintenance", "Note the lowest blood pressure and when it happened"] },
  { match: /surg|procedure|operat/i, examples: ["Write the report as numbered steps in the order described"] },
  { match: /husbandry|enclosure|habitat/i, examples: ["List basking and cool-end temperatures separately"] },
  { match: /withdrawal|herd|flock/i, examples: ["Give meat and milk withdrawal in days for each drug"] },
  { match: /vaccin/i, examples: ["List each vaccine with its site", "Note vaccines declined and why"] },
  { match: /medication|drug|prescri|treatment given|discharge med/i, examples: ["Give the tablet strength and number of tablets per dose", "Put drugs given in clinic before drugs to go home", "Include the duration in days for each drug"] },
  { match: /vital|tpr/i, examples: ["Give every value with its unit", "Give weight in kg and lb if both were said"] },
  { match: /physical|exam|finding/i, examples: ["Group findings by body system", "Only list systems the vet actually commented on"] },
  { match: /history|subjective|complaint|presenting/i, examples: ["Start with onset and duration", "Keep the owner's own words for the main concern"] },
  { match: /assessment|problem|diagnosis|diagnoses/i, examples: ["Number the problems in order of priority as the vet said them"] },
  { match: /differential/i, examples: ["Keep the vet's order, most likely first"] },
  { match: /plan|recommend/i, examples: ["Separate what was done today from what was recommended", "Note anything the client declined"] },
  { match: /lab|diagnostic|result|bloodwork|test/i, examples: ["Give each result with its unit", "Separate tests run today from tests declined"] },
  { match: /follow|recheck|next/i, examples: ["Give each recheck date or interval as said"] },
  { match: /client|communication|advice|owner/i, examples: ["Record the exact warning signs the vet gave", "Note the costs discussed, as said"] },
];

/** Whole-note instructions, by kind of practice (template builder). */
export const NOTE_EXAMPLES: { useCase: string; text: string }[] = [
  { useCase: "Small-animal general practice", text: "Chart as a small-animal GP would, using SOAP order and standard abbreviations (BID, PO, SID)." },
  { useCase: "Emergency and critical care", text: "Put the time said in front of every treatment and event, and keep the triage findings first." },
  { useCase: "Feline-only clinic", text: "Use cat-friendly handling notes when the team mentioned them, and record stress or FAS scores if said." },
  { useCase: "Equine ambulatory", text: "Write for an equine field record: lameness by limb, management advice, and competition withdrawal times when given." },
  { useCase: "Exotics", text: "Record husbandry exactly as the owner described it, and weights in grams." },
  { useCase: "Farm and production animals", text: "Identify animals by ear tag or pen, and give meat and milk withdrawal times for every drug when said." },
  { useCase: "Shelter medicine", text: "Identify the animal by its shelter ID and kennel, and note intake findings separately from today's exam." },
  { useCase: "Referral specialty", text: "Write for the referring veterinarian: summarise prior workup first, then today's findings and recommendations." },
];

function findHeading(template: Template, heading: string) {
  const [sec, part] = heading.split("/");
  const s = template.sections.find((x) => x.id === sec);
  if (!s) return null;
  const p = part ? s.parts?.find((x) => x.key === part) : undefined;
  return { title: p?.label ?? s.title, examples: p ? (p.examples ?? []) : (s.examples ?? []) };
}

/** Examples for a heading's title (for headings without their own). */
export function examplesForTitle(title: string): string[] {
  return BY_TITLE.find((x) => x.match.test(title))?.examples ?? [];
}

/** Example requests for one heading of a template ("plan/medications"). */
export function headingExamples(template: Template | null | undefined, heading: string): string[] {
  if (!template) return [];
  const h = findHeading(template, heading);
  if (!h) return [];
  return h.examples.length ? h.examples : examplesForTitle(h.title);
}
