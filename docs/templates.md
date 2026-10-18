# Note templates for veterinary teams

OpenBark writes the note to your team's headings: SOAP, a dental procedure note, a surgical report, a callback note, or anything else your clinic uses. The design is in ADR-0006.

## Built-in templates

Each template is a type of visit with its own icon. On **Record**, the common types are tiles (New consult, Recheck, Annual exam, Phone call, Results, Clinician rounds), followed by your team's own templates; every other type is under **More types**.

| Template | Icon | Who it's for |
|---|---|---|
| New consult (a new problem or a new patient) | stethoscope | The medical record |
| Recheck | arrows | The medical record |
| Annual exam (preventive care, vaccines, parasite control) | calendar | The medical record |
| Phone call (any call with a client, either way) | phone | The medical record |
| Results (results reviewed or given, what they mean, what changes) | flask | The medical record |
| Clinician rounds (hospitalised patient handover: since last rounds, treatments, plan, to do) | people | The medical record |
| SOAP note | clipboard | The medical record |
| Emergency and triage (with treatment times and disposition) | siren | The medical record |
| Dental procedure (findings by tooth, Triadan numbers as said) | tooth | The medical record |
| Surgery and anaesthesia (protocol, monitoring, surgical report) | scissors | The medical record |
| Dermatology consult | hand | The medical record |
| Oncology visit (doses copied as said, never calculated) | microscope | The medical record |
| Exotics consult (avian, reptile, small mammal; husbandry in full) | bird | The medical record |
| Equine visit (lameness by limb) | horse | The medical record |
| Production animal visit (ear tags, withdrawal times as said) | cow | The medical record |
| Euthanasia and quality of life | flower | The medical record |
| Discharge instructions | house | The client |
| Client summary | speech bubble | The client |
| Estimate explanation (prices copied exactly, never added up) | receipt | The client |
| Euthanasia aftercare | hand and heart | The client |

Each specialty template has its own rules on top of the core ones. For example, the production-animal template leaves a withdrawal time empty unless one was said, and the oncology template never calculates a dose or body surface area. Each one was checked on a scripted consult of its kind (docs/eval/M2.md).

**Medical-record** templates are signed by a veterinarian and then locked. **Client** documents are written in plain language and are never signed:
- Open them from the buttons under the note in Review. They open in a side sheet with **Copy**.
- They can be drafted after the note is signed.
- They follow the vet's note where it differs from the recording. For example, a dose the vet corrected is used as corrected, and it counts as supported in the number check.

A team template can be either kind (**Who it's for** in the builder).

## Making a template

Open **Templates**, then **New template** or **Start from SOAP**.

- **Headings.** Each heading holds either:
  - **Bullet points:** findings, history, plans. One fact per line, each linked to where it was said.
  - **Text box:** connected sentences shown as a paragraph, such as a surgical report or an assessment written as prose. Every sentence is still linked to where it was said, and checked like any other line.
  - **Measurements:** named values, such as Weight, Temperature or Teeth extracted, each with its unit.
  - **Medications:** drug, dose, route, frequency and duration.
  - **Subheadings:** each of these holds one of the three kinds above.
- **Icon** and **Level of detail** (for the whole template). The icon marks the visit type on Record and in lists. Level of detail is concise (the fewest words that keep every fact), standard, or thorough (every relevant detail that was said); it never allows anything that wasn't said.
- **Instructions** (optional, per heading): plain language, for example "Injections and fluids given during the visit." The model follows them only where the recording supports them. Under each instruction, **Examples** offers instructions suited to that heading (Triadan numbers under a dental heading, lameness grade by limb under a lameness heading); choosing one adds it to the instruction.
- **Examples by kind of practice** (under the whole-note rules): emergency, feline-only, equine ambulatory, exotics, farm, shelter and referral. Choosing one adds it as a rule.
- **Instructions and rules for the whole note** (optional, under the disclosure). Every template also follows OpenBark's core rules: only what was said, numbers exactly as stated, every line linked to the transcript. You can't remove these.

The right-hand pane shows the note's shape and, under **What the model is told**, the exact prompt. **Try on an encounter** drafts a saved consult with the template, so you can compare it with the original note.

Saving a changed template increases its version. Every note keeps a copy of the template it was written with (`runs/NNN/template.yaml` in the encounter folder), so older notes keep their own headings.

## A template from an example note

**From an example note** (Templates) takes a note your clinic already writes. Paste it, choose **Propose headings**, and the note model lists its headings in order, what goes under each (bullet points, measurements, medications or subheadings), and short instructions. The proposal opens in the builder, unsaved, to check, change and save.

- The model works to a fixed structure, so the result is always a valid template.
- It keeps the clinic's own heading names, and decides whether the note is for the medical record or for the pet owner.
- Nothing about the patient is copied into the template, and the example itself isn't saved. It's processed on this computer.
- `openbark templates from-example note.txt` prints the proposed template as YAML; `--expect headings.txt` reports which expected headings it found.

## Headings for one species

Any heading or subheading can be shown only for some species: **Show** under the heading reads as a sentence, such as "Show only for cats" or "Show for every species". In the YAML:

```yaml
  - title: FeLV and FIV status
    when: { species: [feline] }
    kind: statements
```

Species are dogs, cats, horses, cattle, sheep and goats, pigs, birds, reptiles, rabbits, small rodents and ferrets. Everyday words work too (`[cat]`, `[guinea pig]`), and are saved as the species key (`feline`, `rodent`).

- The species comes from the patient's details (**Species**, entered by the clinic). It's never guessed from the recording.
- When drafting, headings for other species are removed before the model sees the template, so neither the prompt nor the schema asks for them. The note's saved template copy has the same headings, so Review shows only what applied.
- With no species entered, or for the other animals in a multi-pet visit (only the main animal has a species), every heading stays.
- The built-in wellness template has heartworm testing for dogs and FeLV/FIV status for cats; the dental template has tooth resorption for cats.

## Asking for changes, and standards

In Review, every heading and subheading has **Ask**. Describe what should change, for example:
- "List every fluid with its volume and rate";
- "One line per follow-up: when, why, and what will be reviewed".

Above the general suggestions, **Examples for <template>** lists requests written for that heading of that template: for the dental template's findings, "Use the Triadan number for every tooth"; for the production-animal template's withdrawal times, "Give meat and milk withdrawal in days for each drug". Headings a team made get examples matched on their title.

OpenBark then redrafts just that heading from the recording. The request steers the model but isn't evidence: anything not said stays out, and numbers are checked again. Each redraft is saved as a new draft, and earlier drafts are kept.

Tick **Save as a standard** to make the request part of the template. Every later draft with that template follows it under that heading, and it shows as "Team standard" in the prompt. Standards are listed under their heading in **Templates**, with who added them and when, and can be removed there. They work for built-in templates too.

## Files

```text
<local data>/OpenBark/templates/
  sick-visit/template.yaml     your template
  sick-visit/standards.yaml    its standards
  soap/standards.yaml          standards for the built-in SOAP note
```

`OPENBARK_TEMPLATES_DIR` moves this folder. You can share a template by copying its folder. Before sharing one, check it with:

```sh
openbark templates check path/to/template.yaml
openbark templates list
openbark templates show sick-visit          # the prompt
openbark templates show sick-visit --schema # the generated JSON Schema
```

A minimal `template.yaml`:

```yaml
name: Dental procedure
description: Prophylaxis and extractions under general anaesthesia.
sections:
  - title: Anaesthesia
    kind: medications
    instruction: Premedication, induction and maintenance as said.
  - title: Oral exam
    parts:
      - { title: Findings, kind: statements, instruction: By quadrant, using Triadan numbers when said. }
      - { title: Charting, kind: fields, fields: [{ label: Teeth extracted }, { label: Calculus grade }] }
  - title: Home care
    kind: statements
```

## Editing a draft yourself, and addenda

**Edit** on a heading opens it for typing. Change, reorder, remove or add lines, medications or measurements, then **Save changes**. The edit is saved as a new draft and the earlier one is kept. Lines you change or add are marked "Edited by" with your name (from Settings):
- They are your statements. OpenBark lists them instead of checking them against the recording, so correcting a mis-heard dose doesn't block signing.
- A line that came from the recording keeps its link to it.

Once a note is signed, it can't be edited, re-prompted or redrafted. **Add addendum** appends a dated addition, signed with your name. It appears under the note and in **Copy note**.
