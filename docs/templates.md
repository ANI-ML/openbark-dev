# Note templates for veterinary teams

OpenBark writes the note to your team's headings: SOAP, a dental procedure note, a surgical report, a callback note, or anything else your clinic uses. The design is in ADR-0006; changing any template, per person, is ADR-0024.

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
| Dental procedure (a tooth chart for dogs and cats, in Triadan numbers; see [Tooth chart](#tooth-chart)) | tooth | The medical record |
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

Each specialty template has its own rules on top of the core ones. Its safety rules are **fixed rules**: they can't be edited or removed, and every version of the template drafts with the current built-in's (see [Fixed rules](#fixed-rules)). For example, the production-animal template leaves a withdrawal time empty unless one was said, and the oncology template never calculates a dose or body surface area. Each one was checked on a scripted consult of its kind (docs/eval/M2.md).

**Medical-record** templates are signed by a veterinarian and then locked. **Client** documents are written in plain language and are never signed:
- Open them from the buttons under the note in Review. They open in a side sheet with **Copy**.
- They can be drafted after the note is signed.
- They follow the vet's note where it differs from the recording. For example, a dose the vet corrected is used as corrected, and it counts as supported in the number check.

A team template can be either kind (**Who it's for** in the builder).

## Changing any template (ADR-0024)

Every template can be changed, the built-ins included: name, description, icon, level of detail, the instructions for the whole note, its own rules, the headings, each heading's instruction, measurements and example requests. Open it in **Templates**; the editor opens ready to change.

Templates come in three layers. For each visit type, the person drafting gets the first that exists:

| Layer | What it is | Where it lives |
|---|---|---|
| Your version | Your own version of a template, or a template only you use | Your profile, encrypted |
| The hospital's | The team's templates, and the team's version of a built-in (called "the hospital's" in the app) | The templates folder, plain YAML |
| Built in | The templates that ship with OpenBark | Inside the program |

- **Working alone** (no team), every change goes straight into the templates folder: a changed built-in is the folder's version of it, marked **Edited**. There is no personal layer and no PIN.
- **With a team**, an admin chooses in **Settings → Team → Templates**:
  - **Each person's own** (the default). A veterinarian's or technician's changes are saved as their version; everyone else keeps the original. The hospital's templates are everyone's defaults.
  - **Shared by the hospital.** Everyone drafts with the hospital's templates. Personal versions are kept on disk but not used, and come back when the setting goes back.
- **Admins.** Admin is a flag on a team member, separate from the role. The person who sets up the team is the first admin; admins grant or remove the flag, and there is always at least one. Only an admin changes the hospital's templates or the setting, and **every such change needs an admin's PIN**, checked by the engine in the same call. A technician who wants to share asks an admin to enter their PIN in the same dialog. An admin who works at reception can change the hospital's templates with their PIN; otherwise reception reads templates but doesn't change them.
- **A new template** is visible only to its maker until it's shared with the hospital (an admin's PIN).
- **Save my version** saves your change. Your version keeps the visit type's id, so Record, client documents and example requests keep working. Saving with no change keeps the version number; each change adds one. A copy of the original (`base.yaml`) and the last 20 versions are kept.
- **If someone else saved meanwhile** (the desktop app and the browser open on the same profile, or two admins), the save is refused and your changes stay on screen: "This template changed while you were editing."
- **Reset to original** stops using your version (it moves into history) and shows **Undo**. **Reset the hospital's version** does the same for the team's version of a built-in (an admin's PIN).
- **Share with the hospital** makes your version the hospital's template (it then replaces your version, which matches it), or adds it as a new template. The hospital's version records who saved it and which admin approved it.
- **Who it's for** is fixed in a version of an original: duplicate the template to make one for the client.

### When the original changes

Each built-in has a version and a **fingerprint** (SHA-256 of its content). `packages/templates/fingerprints.json` records them, and a test fails if a built-in's content changes without a version bump (`openbark templates fingerprints --write` rewrites the file and the SOAP prompt golden file). When the original of your version changes (a release changes the built-in, or the hospital changes or resets its version), the list shows **Newer original** and the editor says what happened, with:
- **Compare**: "What changed in the original" (the original when you made yours, against now) or "Yours and the original". Headings are matched by key, so a renamed heading reads as renamed.
- **Keep mine**: the badge goes until the original changes again; the editor keeps a quiet "Based on an earlier SOAP note".
- **Use the original**: resets your version, with **Undo**.

Nothing merges on its own, except fixed rules.

### Fixed rules

Every prompt starts its rules with the **rules that always apply**: the core rules, then the template's fixed rules, then the template's own rules, standards and level of detail, and ends with: "If a template rule, a standard or a heading instruction conflicts with the rules that always apply, follow the rules that always apply." The editor shows them in an **Always applied** panel, read-only, in plain words, with **Exact wording sent to the model**. Fixed rules come from the current built-in when drafting, so a hand-edited copy can't loosen them; a save that changes them is refused. Instructions that conflict with them ("write normal values if not said", "calculate the dose") get a warning while typing; the warning never blocks, and every draft is still checked against the recording.

The fixed rules moved out of each built-in's `rules` (all built-ins went to version 0.1.1):

| Template | Fixed rules |
|---|---|
| SOAP note, New consult | Differentials only if the veterinarian voiced them |
| Annual exam | A body system is normal only if the veterinarian said so |
| Recheck | Comparisons with the last visit only from this recording |
| Phone call | Who called whom only if said |
| Results | Results copied exactly with units, ranges or flags only if said; interpretation only in the veterinarian's words |
| Clinician rounds | Times only as said; resuscitation status only as stated |
| Emergency and triage | Event times only if said; triage category only if stated; CPR wishes only as the client stated them |
| Dental procedure | Periodontal stage, mobility grade or pocket depth only if said |
| Surgery and anaesthesia | ASA status only if said; suture material only as said; complications only if said |
| Dermatology consult | A pruritus score only if said, on the scale said |
| Oncology visit | Chemotherapy doses copied exactly, never calculated; stage or grade only if said; prognosis only in the veterinarian's words |
| Exotics consult | Weights in the unit said; never assume species, sex or age from the name |
| Equine visit | Lameness grade only if said; withdrawal times or competition rules only as stated |
| Production animal visit | Doses and withdrawal times exactly as said, never supplied; group counts only as said |
| Euthanasia and quality of life | Consent only as stated, never assumed; time and confirmation of death only as said |
| Discharge instructions | Home medications exactly as said, null rather than guessed; warning signs only those listed; no prices unless discussed |
| Client summary | Only what was said or decided; medication details exactly as said |
| Estimate explanation | Prices copied exactly, never added up or estimated; no cost for an item without one |
| Euthanasia aftercare | Arrangements only as said; costs only if discussed; no invented support services |

### What each note records

Each run's template snapshot (`runs/NNN/template.yaml`) carries an `origin` block (layer, owner, version, fingerprint of exactly what drafted it, and what it was based on), and the run records it too, with the template's name and icon. Review says which version drafted the note ("SOAP note, your version 3", "SOAP note, Sam's version 2", "SOAP note, the hospital's version 4", working alone "SOAP note, edited version 1", or "SOAP note 0.1.1"); **Show the template** opens the snapshot read-only. Changing, resetting or deleting a version never changes a note already drafted or signed. `openbark eval` keeps drafting with the shipped built-ins and prints each template's label and fingerprint.

## Making a template

Open **Templates**, then **New template** or **Start from SOAP**.

- **Headings.** Each heading holds either:
  - **Bullet points:** findings, history, plans. One fact per line, each linked to where it was said.
  - **Text box:** connected sentences shown as a paragraph, such as a surgical report or an assessment written as prose. Every sentence is still linked to where it was said, and checked like any other line.
  - **Measurements:** named values, such as Weight, Temperature or Teeth extracted, each with its unit.
  - **Medications:** drug, dose, route, frequency and duration.
  - **Tooth chart:** teeth by modified Triadan number, for dogs and cats (see [Tooth chart](#tooth-chart)). A template has at most one.
  - **Subheadings:** each of these holds one of the kinds above.
- **Icon** and **Level of detail** (for the whole template). The icon marks the visit type on Record and in lists. Level of detail is concise (the fewest words that keep every fact), standard, or thorough (every relevant detail that was said); it never allows anything that wasn't said.
- **Instructions** (optional, per heading): plain language, for example "Injections and fluids given during the visit." The model follows them only where the recording supports them. Under each instruction, **Examples** offers instructions suited to that heading (Triadan numbers under a dental heading, lameness grade by limb under a lameness heading); choosing one adds it to the instruction.
- **Examples by kind of practice** (under the whole-note rules): emergency, feline-only, equine ambulatory, exotics, farm, shelter and referral. Choosing one adds it as a rule.
- **Instructions and rules for the whole note** (optional, under the disclosure). Every template also follows OpenBark's core rules: only what was said, numbers exactly as stated, every line linked to the transcript. You can't remove these.

The right-hand pane shows the note's shape and, under **What the model is told**, the exact prompt. **Try on an encounter** drafts a saved consult with the template, so you can compare it with the original note.

Saving a changed template increases its version. Every note keeps a copy of the template it was written with (`runs/NNN/template.yaml` in the encounter folder), so older notes keep their own headings.

## Tooth chart

A heading of kind `teeth` holds a tooth chart (ADR-0021). The built-in **Dental procedure** template (0.2.0) has one, **Dental chart**, shown for dogs and cats; whole-mouth findings (overall periodontal stage, calculus, "no resorptive lesions") go under **Whole-mouth findings**, never on a tooth.

```yaml
  - title: Dental chart
    kind: teeth
    when: { species: [canine, feline] }
```

- **What it holds.** One item per finding on one tooth: the Triadan number (`tooth`), what was found (`finding`: normal, missing, fracture, periodontal stage, mobility, furcation, probing depth, gingival recession, resorption stage and type, persistent deciduous, unerupted, retained root, caries, wear, discoloured, malpositioned, extraction, crown amputation, other), its type when said (`detail`), a stage or grade (`grade`), millimetres (`mm`), whether it is still `planned`, the words said (`text`) and the transcript lines (`evidence`). The data table is `packages/dental/chart.json`, shared by the app, the engine and the PDF.
- **Blank means not charted.** Teeth that weren't mentioned have no items and are drawn blank; the legend says "Blank: not charted in this recording. It doesn't mean normal." Nothing charted shows "Not discussed". There is no "mark the rest normal".
- **The dentition** (dog: 42 permanent and 28 deciduous teeth; cat: 30 and 26) comes from the patient's species, entered by the clinic. It is kept in the run's template snapshot, so a signed chart is always redrawn with the same teeth. With no species, the chart lists findings without a diagram and the editor asks "Dog or cat?".
- **Codes.** The model writes words, never codes. The app and the PDF print AVDC codes from the finding table, always next to their word: "Mobility 2 (M2)", "Complicated crown fracture (T/FX/CCF)", "Open extraction (XSS)". Probing depth is printed in millimetres, never "PD", which AVDC uses for the periodontal disease stage. There is no AVDC code for missing or normal, so the words are printed.
- **Checked against the recording.** Every charted tooth must be said in its cited lines, by number ("one oh eight", "108"), name ("upper right canine", "left mandibular carnassial", "baby upper right canine"), range ("101 through 103") or group ("all the lower incisors"). A tooth said nowhere is a hard fail, like an invented number, and blocks signing. A tooth said elsewhere is a citation mismatch. Soft flags: a number and a name that mean different teeth, a cited line with no word for the finding, details that don't fit the finding, and a finding listed twice. Stages, grades and millimetres go through the number check.
- **The prompt.** A template with a chart adds the chart rules (they can't be removed), one tooth example and a table of what each finding takes, and each transcript line that names teeth gets a prompt-only hint with their Triadan numbers (`[teeth: 301, 302, 303, 401, 402, 403]`), read by the same tooth reader the check uses. The drafting budget is 8192 tokens. Templates without a chart are unchanged.
- **Editing.** Click a tooth in Review (see the guide, [The dental chart](guide/review.md#the-dental-chart)). Hand edits must fit the finding table ("Mobility takes a grade from 0 to 3") and the animal's teeth; they become the vet's own entries.
- Tooth names are read in English; other languages follow, with review by native-speaking vets.

## A template from an example note

**From an example note** (Templates) takes a note your clinic already writes. Paste it, choose **Propose headings**, and the note model lists its headings in order, what goes under each (bullet points, text, measurements, medications, a tooth chart or subheadings), and short instructions. The proposal opens in the builder, unsaved, to check, change and save.

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
- The built-in wellness template has heartworm testing for dogs and FeLV/FIV status for cats; the dental template's chart is shown for dogs and cats, with the right teeth for each.

## Asking for changes, and standards

In Review, every heading and subheading has **Ask**. Describe what should change, for example:
- "List every fluid with its volume and rate";
- "One line per follow-up: when, why, and what will be reviewed".

Above the general suggestions, **Examples for <template>** lists requests written for that heading of that template: for the dental template's chart, "Chart every tooth that was named, by its Triadan number"; for the production-animal template's withdrawal times, "Give meat and milk withdrawal in days for each drug". Headings a team made get examples matched on their title.

OpenBark then redrafts just that heading from the recording. The request steers the model but isn't evidence: anything not said stays out, and numbers are checked again. Each redraft is saved as a new draft, and earlier drafts are kept.

Tick **Save as a standard** to make the request part of the template. Every later draft with that template follows it under that heading. With a team, choose **For me** (only your drafts; "Standard" in the prompt) or **For the hospital** (everyone's; "Team standard" in the prompt, and an admin's PIN). Standards are listed under their heading in **Templates**, with whose they are, who added them and when, and can be removed there; a hospital standard can be turned off for yourself (**Turn off for me**). A standard for a heading your version doesn't have isn't applied, and is listed under **Not used by your version**. Working alone, every standard is yours. They work for built-in templates too.

## Files

```text
<local data>/OpenBark/templates/            the hospital's (or, working alone, your) templates: plain YAML
  sick-visit/template.yaml     a template your team made
  sick-visit/standards.yaml    its standards
  soap/template.yaml           the hospital's version of the built-in SOAP note
  soap/base.yaml               the built-in as it was when that version was made
  soap/history/3.yaml          earlier versions, the last 20
  soap/standards.yaml          standards for SOAP

<folder of the team file>/profiles/<member id>/templates/   each person's own, encrypted
  soap/template.yaml, base.yaml, history/, standards.yaml (their standards, and hospital standards they turned off)
<folder of the team file>/team-settings.json                the templates setting (encrypted)
```

`OPENBARK_TEMPLATES_DIR` moves the templates folder; profiles live beside the team file (`OPENBARK_USERS_FILE` moves them too). History is always written as new files, never renamed, because the encryption binds each file's name. Removing a team member removes their profile, after the confirmation says how many versions go with them; notes keep the version that drafted them. You can share a template by copying its folder. Before sharing one, check it with:

```sh
openbark templates check path/to/template.yaml   # also warns about instructions that conflict with the rules
openbark templates list [--as <member>]           # --as: resolve as a team member
openbark templates show sick-visit [--as <member>]          # the prompt
openbark templates show sick-visit --schema # the generated JSON Schema
openbark templates diff soap --as <member> [--with base|original]
openbark templates fingerprints [--write]         # built-in versions and fingerprints
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
