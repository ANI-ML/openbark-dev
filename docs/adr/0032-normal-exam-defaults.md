# ADR-0032: Normal-exam defaults, a warning for pasted findings, and no "null" in notes

- Status: accepted
- Date: 2026-10-31
- Deciders: product owner, engineering
- Relations: bends the core rule "Never write \"normal\" or \"within normal limits\" unless that was said" (`template.rs`, `CORE_RULES`) only under the safeguards below; the rule itself is unchanged and still in every prompt. Uses the hand-edit mechanism of M1 item 6 (`entered_by`), the sign gate of ADR-0021 decision 10, and the lint of ADR-0024 section 4.4.

## Context

A French-speaking clinic moving from Covet pasted its whole normal physical exam into the instruction box of its template's "Examen physique" heading:

```text
-Yeux : Normal, aucun écoulement… -Tartre : -T: P: R: …
```

OpenBark sends a heading's instruction to the note model as guidance (`note.rs`, `guidance`). The small model copied the normals into the note as findings, and wrote "Tartre : null" and "T: null" for the blanks. The note said "Yeux : Normal" for a cat seen for an eye problem. The faithfulness pass (`faithfulness.rs`) checks numbers and cited words, not whether a system was examined, so the copied "Normal" lines went through.

What the clinic wanted is common in practice management systems: a normal exam by system, which the vet changes only where something was found. What it got was the model asserting findings nobody said.

On 2026-10-31, after this report, the product owner approved normal-exam defaults as an opt-in per heading, on the condition that they can never pass for something said in the recording.

## Decision

### 1. Normal findings are a field of their own, never sent to the model

- A heading of bullet points (`statements`) in a medical-record template may have **default lines**, `defaults: [{label, text}]`, kept apart from its `instruction`. Other kinds, and client documents (never signed, so never gated), refuse them with a plain message. Labels are unique per heading; at most 40 lines.
- The template editor shows **Normal findings (used only when not discussed)** under such a heading: one "System: finding" per line.
- **The default text never reaches the model.** The prompt names the systems of the heading at most ("Systems to report on, only if they were discussed: Yeux, Oreilles…"), with the usual evidence rule. The rule "never write normal unless that was said" stays in the prompt, word for word. A test builds every message and grammar of a draft and a re-prompt and checks the text appears in none of them.

### 2. Code, not the model, decides which defaults apply

- The drafting grammar (only the grammar: the template's saved schema is unchanged) gives every statement under such a heading a `system`: one of the heading's labels, or null (`defaults.rs`, `tag_systems`). The model therefore says which system each finding is about, through the schema, instead of code guessing from the words.
- After drafting, `defaults::undiscussed` takes the tags out of the note and keeps a default only when nothing was said about its system. A default is dropped, whole, when:
  - a statement under its heading is tagged with its system;
  - anything in the note mentions its system (a model that tagged wrongly, or a backend without the grammar), with accents folded and plurals and word starts matched ("oreille" for "Oreilles", "coeur" for "Cœur");
  - the system was named anywhere in the recording, even if the draft left it out (the cat with the eye problem).
- When in doubt, the default is left out. A finding replaces its default and is never merged into it.
- Re-prompting a heading drops a waiting default whose system the redraft now covers; a hand edit drops one whose system the vet wrote about. Neither adds one back.

### 3. Defaults are stored beside the note, not in it

- The defaults that apply are saved in the run's `note.json` as `template_defaults: [{heading, label, text}]`, a sibling of `note`, never inside it.
- **Older versions ignore them.** 0.2.4 reads only `note` from `note.json` (`Session::latest_note`, the app's NoteView and Copy, the PDF) and `note.md` (iPhones show it through the clinic hub), and neither holds a default. Its `Template` has no `defaults` field and no `deny_unknown_fields`, so a 0.2.5 template snapshot loads there without them, with the same schema; the note has no `system` tag, so it still validates and can be edited there. A test reads a 0.2.5 run through that path.
- A key inside `note` was rejected: the root schema has `additionalProperties: false`, so a 0.2.4 hand edit would fail, and re-prompting sends a heading's content back to the model. A field on the run in `session.json` was rejected: a 0.2.4 device that re-saves `session.json` drops fields it doesn't know.
- Because the defaults are outside the note, the faithfulness pass and the eval (`os-cli eval` scores `result.note`) never see them: never flagged, never counted as supported or as facts found. Copy, the PDF, `note.md` and client documents drafted before signing (which follow `note.md`) leave them out.

### 4. Review shows them apart, and a veterinarian confirms or removes each one

- Under its heading, Review shows waiting defaults in a dashed box in sans type, labelled **Template default, not from the recording**, with **Confirm**, **Edit** and **Remove** per line and **Confirm all** per heading.
- **Confirm** adds the line to the heading as the vet's own, through the existing hand-edit path: `entered_by` the vet, no evidence, listed by the faithfulness pass rather than checked, shown as "Edited by". **Edit** confirms the vet's wording instead. **Remove** drops it. Each is a new draft; the earlier one is kept (`defaults::settle`, `App::settle_defaults`, the `settle_defaults` command, `POST /api/sessions/{id}/defaults`).
- With a team, only a veterinarian confirms; removing needs someone who may edit notes. On the iPhone during the pilot, as with signing, defaults are confirmed on a computer.

### 5. The engine refuses to sign while one is waiting

`Session::sign_latest_by` reads `note.json` and refuses while `template_defaults` is not empty, naming the systems, whatever the client: the app, web mode or the API. Review keeps **Sign note** off and says why above the note. A note signed on a 0.2.4 device is signed without its defaults (which were never part of it), and 0.2.5 never shows defaults on a signed note.

### 6. A warning for findings pasted into an instruction, in any language

- `template_lint.rs` warns (`pasted_findings`) when an instruction (the whole note's, a heading's or a subheading's) reads like a filled-in exam or form: three or more "Label : value" or "-Label:" lines or segments, at least two of them a normal ("Normal", "normale", "normaux", "WNL", "aucun", "absent", "sin", "keine", "unauffällig"…) or a blank ("T: P: R:"), or five or more of any kind. It reads the shape of the text, so it works whatever the language (`blanks.rs`, `label_values`).
- The warning says plainly that instructions are told to the model, and that findings pasted there can end up in the note as if they were said.
- Under a heading of bullet points it offers **Move these lines into Normal findings**: the lines become default lines (blank ones such as "T:" left out), they leave the instruction, and the instruction says "List the findings the veterinarian states, by system."

### 7. "null" never shows in a note

- After drafting and re-prompting, `blanks::scrub_note` turns values that are "null", "n/a", "nil", "undefined", a dash or empty into real nulls (a measurement whose whole value is "none" too), and drops statements that say nothing ("null", "Tartre : null", "T: null P: null"); "T: 38.5 P: null" reads "T: 38.5". This runs before the defaults are decided, so a blank never counts as a finding.
- Review, Copy, the PDF, `note.md` and client documents skip such values in notes saved before, so "null" is never printed.
- "Vomiting: none" is kept: a statement with a label and "none" can be a negative that was said.

## Consequences

- Clinics that chart a normal exam by system can keep doing so without the model ever asserting a finding: every default line that reaches a signed note was confirmed by a named veterinarian and is marked as theirs.
- Defaults are conservative by design: a system named anywhere in the recording gets no default, even when nothing was found. A clinic will see fewer defaults than a template that fills every system, and must chart what it examined.
- A label that is a common word in conversation ("Général", "Comportement") rarely gets its default. Short, specific labels work best; the guide says so.
- The drafting grammar is a little larger for a heading with defaults (one enum per statement). Templates without defaults draft with byte-identical prompts and grammars; their fingerprints don't change, since the key isn't written when empty.
- A 0.2.4 device computes a template's fingerprint without `defaults`, so a template with defaults has a different fingerprint there than on 0.2.5. That can show a spurious "Newer original" notice on a 0.2.4 device until it updates.
- A template saved on a 0.2.4 device (a personal version, or the hospital's by an admin there) is saved without its normal findings, since 0.2.4 doesn't know the field. Clinics using them should update every device to 0.2.5.
- The comparison of template versions (`template_diff.rs`) doesn't list a change to a heading's normal findings yet.
- `note.md` files written before 0.2.5 still hold any "null" the model wrote; Review, Copy and the PDF read the note itself and skip it, but a draft from another device offered as an addendum (which starts from that run's `note.md`) can show it.
- The MLX golden check now also compiles a drafting schema with defaults (`examples/write_schemas.rs`), so llguidance covers the tagged statements.

## Alternatives considered

| Option | Why not |
|---|---|
| Keep normals in the instruction and tell the model to use them only if not discussed | That is what failed: a small model copies them as findings. The default text must never reach the model |
| Let the model write the defaults, marked | The model would decide what was discussed and could still merge a normal into a finding. Code decides, from tags the grammar forces |
| Fuzzy matching of findings to systems without tags | Unreliable across languages and phrasing. Kept only as an extra reason to drop a default, never to add one |
| Store defaults as ordinary statements with a flag | 0.2.4 phones would show them as findings, and the faithfulness pass and eval would count them |
| Store them inside `note` under a new key | 0.2.4's schema rejects it on edit, and re-prompting would send them to the model |
| Fill defaults silently and let signing stand for review | Signing a whole note doesn't show that each default was looked at; a per-line confirmation does |
