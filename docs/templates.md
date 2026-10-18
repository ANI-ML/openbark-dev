# Note templates for veterinary teams

OpenBark writes the note to your team's headings: SOAP, a dental procedure note, a surgical report, a callback note, or anything else your clinic uses. The design is in ADR-0006.

## Making a template

Open **Templates**, then **New template** or **Start from SOAP**.

- **Headings.** Each heading holds either:
  - **Bullet points:** findings, history, plans. One fact per line, each linked to where it was said.
  - **Measurements:** named values, such as Weight, Temperature or Teeth extracted, each with its unit.
  - **Medications:** drug, dose, route, frequency and duration.
  - **Subheadings:** each of these holds one of the three kinds above.
- **Instructions** (optional, per heading): plain language, for example "Injections and fluids given during the visit." The model follows them only where the recording supports them.
- **Instructions and rules for the whole note** (optional, under the disclosure). Every template also follows OpenBark's core rules: only what was said, numbers exactly as stated, every line linked to the transcript. You can't remove these.

The right-hand pane shows the note's shape and, under **What the model is told**, the exact prompt. **Try on an encounter** drafts a saved consult with the template, so you can compare it with the original note.

Saving a changed template increases its version. Every note keeps a copy of the template it was written with (`runs/NNN/template.yaml` in the encounter folder), so older notes keep their own headings.

## Asking for changes, and standards

In Review, every heading and subheading has **Ask**. Describe what should change, for example:
- "List every fluid with its volume and rate";
- "One line per follow-up: when, why, and what will be reviewed".

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
