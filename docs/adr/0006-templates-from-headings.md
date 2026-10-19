# ADR-0006: Team templates are headings; the schema is generated; re-prompts can become standards

- Status: accepted; amended by ADR-0024 (built-ins can be changed, per person; template safety rules are fixed rules)
- Date: 2026-10-17
- Deciders: product owner (asked for "full customization of templates for veterinary teams that the model will fit based on heading", and "a layer that allows users to re-prompt the model for certain details and have that as a standard"), engineering

## Context

Until now the only template was SOAP: a YAML file with instructions, plus a JSON Schema written by hand. The schema matters because it constrains decoding (llama.cpp grammar) and shapes what the faithfulness pass can check. Veterinary teams won't write JSON Schema, though, and a hand-written schema could quietly drop the evidence citations that the safety checks depend on.

The M1 eval showed that the model follows headings well when it gets one guidance line per heading, and that E4B needs measurements as `{value, unit, evidence}` (docs/eval/M1.md, finding 2).

## Decision

1. **A template is headings.** Each heading either holds content itself or has subheadings. Content is one of three kinds:
   - bullet points (`statements`);
   - named measurements (`fields`);
   - medications.

   Every heading and subheading can have a plain-language instruction. JSON keys come from the titles (`"Take-home medications"` becomes `take_home_medications`). Once a key is assigned, it stays fixed when the heading is renamed.
2. **The schema is generated** from the headings (`os_engine::template::generate_schema`). It always uses the same building blocks: evidence ids on every item, `null` or empty meaning "not discussed", and values with separate units. SOAP uses the generator too. `packages/templates/soap/schema.json` is now a snapshot, and a test fails if the generated schema drifts from it.
3. **Core rules** (`CORE_RULES`) are added to every template ahead of its own rules: only what was said, numbers exactly as stated, evidence on every line, hints treated with care. A team template can add rules but can't remove these.
4. **Team templates are files:** `<local data>/OpenBark/templates/<id>/template.yaml`. A team can share them or keep them under version control. Saving a changed template increments its version. Built-in templates are read-only (duplicate one to change it). A new template never takes over an existing id.
5. **Every note run saves the template it was written with** (`runs/NNN/template.yaml`, standards included). Old notes still render under their own headings after the template changes, and a draft can be reproduced exactly.
6. **Re-prompting** (`refine_note`) redrafts one heading or subheading of the latest draft from the transcript, following the vet's request. The redraft:
   - is constrained by that heading's part of the schema;
   - leaves the rest of the note untouched;
   - is validated and faithfulness-checked as a whole note;
   - is saved as a new run, with `refinement {heading, request, from_run}`.

   The request steers the model, but it is not evidence. Signed notes can't be re-prompted.
7. **Standards:** while re-prompting, the vet can save the request against that heading. Standards are stored in `<templates>/<id>/standards.yaml`, which works for built-in templates as well. They go into every later draft's prompt as "Team standard: …" on that heading's guidance line (a standard with heading `""` applies to the whole note), and can be removed in the template builder.

## Consequences

- A team can build a dental, surgical, exotics or callback template with no code and no schema, and keep evidence links, "Not discussed", hard fails and second-opinion flags.
- Prompt changes now come from two places: the template and its standards. Both are snapshotted per run, so the eval stays comparable if the template id and standards are pinned.
- Moving the generic rules into `CORE_RULES` changed the order of rules in SOAP's prompt. The eval was re-run after the change (docs/eval/M1.md).
- In testing, one redraft dropped a fact from the earlier draft. The re-prompt rules now say to keep every supported fact unless the vet asked to remove it. Watch for this in review; a "what changed" view would help.
- Per-species conditional headings ("Show when species is feline", DESIGN.md §5) and building a template from an example note are not done yet.

## Alternatives considered

| Option | Why not |
|---|---|
| Teams write JSON Schema | Error-prone, and can drop the evidence fields the safety checks need. |
| Free-form Markdown templates filled as text | No grammar constraint and no per-line evidence, so the faithfulness pass can't work. |
| Store templates and standards in a database | Folder-of-files matches sessions and models, is easy to share and diff, and needs no migration before the M1 encrypted store. |
| Apply re-prompts as edits to the note text | Loses the transcript grounding. Redrafting from the transcript keeps every line cited and checked. |
