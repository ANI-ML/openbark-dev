# Templates

Video: **Help → Templates** in the app. The full reference is [docs/templates.md](../templates.md).

A template is the headings of a note, and what goes under each one. OpenBark ships with these templates:

| Template | Who it's for |
|---|---|
| New consult, Recheck, Annual exam, Phone call, Results, Clinician rounds | The medical record |
| SOAP note | The medical record |
| Emergency and triage, Surgery and anaesthesia, Dental procedure | The medical record |
| Dermatology consult, Oncology visit | The medical record |
| Exotics consult, Equine visit, Production animal visit | The medical record |
| Euthanasia and quality of life | The medical record |
| Discharge instructions, Client summary, Estimate explanation, Euthanasia aftercare | The client |

On **Record**, choose the type of visit from the icon tiles (or **More types**) before recording, or use **Draft as** in Review. You can change any of them, make your own, or copy one.

## Change any template

Open **Templates** and choose a template: it opens ready to change. Change the headings, what goes under each, each heading's instruction, the instructions and rules for the whole note, the name, icon or level of detail, then choose **Save my version**.

- **With a team**, your change is saved as your version: your drafts follow it, and everyone else keeps the original. The list marks it **Your version**. A template you make is **Only you** until you share it.
- **Working alone**, the change goes straight into your templates, marked **Edited**.
- If your clinic's admin chose **Shared by the hospital** (Settings), everyone drafts with the hospital's templates, and only an admin changes them, with their PIN. Your own versions are kept and come back if the setting changes.
- Reception can read every template and what the model is told, but doesn't change them.

The **Always applied** panel, under the rules for the whole note, lists the rules no template can change: only what was said, numbers exactly as said, every line linked to the recording, and the template's own safety rules. If an instruction asks for something they forbid, such as "write normal values if not said", a warning appears under it. The rules that always apply win, and every draft is still checked against the recording.

## Your version and the original

- **Reset to original** goes back to the original. **Undo** in the message brings your version back.
- **Compare with original** shows how your version differs from the original, heading by heading.
- If someone saved the same template while you were editing (in another window), saving is refused and your changes stay on screen. Choose **Save over it** or **Load the saved version**.

## Share with the hospital

**Share with the hospital** makes your version the hospital's template, so everyone without their own version drafts with it, or adds it as a new template. An admin approves with their PIN in the same dialog. People with their own version keep it and are told the hospital's changed.

## When OpenBark updates a template

A release never overwrites your version. When the original changes, the list shows **Newer original** and the template says what changed, with **Compare**, **Keep mine** and **Use the original**. Safety rules from the new release apply to your version straight away.

Review says which version drafted each note, for example "Drafted with SOAP note, your version 3". **Show the template** shows exactly what the model was told. Changing a template later never changes a note already drafted or signed.

## Make a template

1. Open **Templates**. Choose **From an example note** to paste a note your clinic already writes (OpenBark proposes its headings for you to check), **Start from SOAP** to adapt SOAP, or **New template** for blank headings.
2. Name it, choose its **Icon**, its **Level of detail** (concise, standard or thorough), and **Who it's for**: the medical record, or the client.
3. For each heading, choose what goes under it:
   - **Bullet points** for findings, history and plans;
   - **Text box** for a paragraph, such as a surgical report (every sentence still linked to the recording);
   - **Measurements** for named values with units, such as weight or teeth extracted;
   - **Medications** for drug, dose, route, frequency and duration;
   - **Tooth chart** for teeth by Triadan number, for dogs and cats: filled from what was said, and teeth not mentioned stay blank (one per template; see [The dental chart](review.md#the-dental-chart));
   - **Subheadings**, each holding one of the above.
4. Add an instruction where it helps, in plain words, such as "Injections and fluids given during the visit." Not sure what to write? Choose one of the **Examples** under the heading; they suit what the heading is for, like Triadan numbers for teeth or lameness grades by limb. **Examples by kind of practice**, under the whole-note rules, adds a rule for an emergency, feline-only, equine, exotics, farm, shelter or referral practice.
5. If a heading only applies to some animals, choose who it's for under **Show**, such as "only for cats". It's left out of notes for other species, and kept when no species was entered.
6. Check the **Note** preview and **What the model is told**, then choose **Save**. Each change saves a new version, and notes keep the version they were drafted with.
7. Choose **Try on a visit** to draft a saved consult with the new template.

Every template follows OpenBark's core rules, and you can't remove them: only what was said, numbers exactly as stated, every line linked to the recording. They're listed under **Always applied**.

## Standards

When reviewing a note, **Ask** shows example requests made for that heading of that template (for the dental template's chart, "Split ranges such as 101 to 103 into one item per tooth"). Choose one, or write your own. **Ask** with **Save as a standard** adds your request to the template for that heading: with a team, **For me** (only your drafts) or **For the hospital** (everyone's, with an admin's PIN). Standards appear under their heading in the template, with who added them and when, and can be removed there.
