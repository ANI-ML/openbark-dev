# ADR-0021: Dental chart as a template content kind

- Status: proposed
- Date: 2026-10-18
- Deciders: product owner (said "Implement dental chart."; M2 item 7), engineering
- Builds on: ADR-0006 (templates are headings; the schema is generated), ADR-0016 (the brand guide wins on visuals)
- Plan: [docs/plans/dental-chart.md](../plans/dental-chart.md)

## Context

### What was asked

M2 item 7 asks for a clickable tooth chart for dogs and cats, with modified Triadan numbering. The model fills it from the consult (missing, fractured, mobility, periodontal stage, extraction, resorption). The vet edits it by clicking. It shows in the note and in the PDF.

Acceptance in `docs/plans/M2.md`:
- it fills from a scripted dental fixture;
- every finding cites a transcript line;
- clicks edit it as the vet's own entries.

### What exists today

- `packages/templates/dental/template.yaml` (dental@0.1.0) records teeth as bullet points under "Findings by tooth", plus a feline-only "Tooth resorption" heading. There is no chart anywhere in the app.
- Its instructions give the example `108, "upper left fourth premolar"`. That is wrong: 108 is the **right** maxillary fourth premolar. The scripted fixture (`packages/eval/fixtures/scripts/dental.yaml`) repeats the error, in a consult whose nerve block is on the left.
- `packages/eval/fixtures/example-notes/dental.txt` says "307: resorptive lesion type 1" and then "Root replacement 307 confirmed". Replacement resorption is type 2, so the example contradicts itself.

### Tooth numbering, checked against AVDC

Checked live against the AVDC Triadan table (based on the modified Triadan system, J Vet Dent 8(4):19-20, 1991) and the AVDC nomenclature page.

| Quadrant | Permanent | Deciduous |
|---|---|---|
| Right maxilla | 100 | 500 |
| Left maxilla | 200 | 600 |
| Left mandible | 300 | 700 |
| Right mandible | 400 | 800 |

| Dentition | Formula | Teeth | Ids |
|---|---|---|---|
| Dog, permanent | 2 x (I 3/3, C 1/1, P 4/4, M 2/3) | 42 | 101-110, 201-210, 301-311, 401-411 |
| Dog, deciduous | 2 x (i 3/3, c 1/1, p 3/3) | 28 | 501-504 and 506-508, the same in 600; 701-704 and 706-708, the same in 800 |
| Cat, permanent | 2 x (I 3/3, C 1/1, P 3/2, M 1/1) | 30 | 101-104 and 106-109, the same in 200; 301-304 and 307-309, the same in 400 |
| Cat, deciduous | 2 x (i 3/3, c 1/1, p 3/2) | 26 | 501-504 and 506-508, the same in 600; 701-704 and 707-708, the same in 800 |

Facts the design relies on:
- The canine is always x04 and the first molar always x09.
- Gaps are left where teeth are absent, so the cat's first upper premolar is 106, not 105.
- Every cat id is also a dog id. A tooth's name maps to the same number in both species; only the set of teeth differs.
- The carnassials are the upper fourth premolars (108, 208) and the lower first molars (309, 409).

### Codes, checked against AVDC

The AVDC case-log abbreviation list says other abbreviations are not permitted; the whole word is written instead. Relevant codes include T/FX with EI, EF, UCF, CCF, UCRF, CCRF and RF; PD0-PD4; TR; GR; DT/P; T/U; RTR; CA; AB; AT; MAL1 with BV, DV, LABV, LV, MV, PV; X, XS, XSS; CR/A. The nomenclature page adds the stages and indices: PD0-PD4, the mobility index M0-M3, the furcation index F1-F3, tooth resorption stages TR1-TR5 (with 4a, 4b, 4c) and types T1-T3.

Three traps:

| Trap | What it means for the chart |
|---|---|
| No AVDC code for "missing" or "normal" | Print the words "Missing" and "Normal" |
| "PD" means periodontal disease stage in AVDC, but many charts use it for pocket depth | Pocket depth is printed as "probing depth, mm", never "PD" |
| In the abbreviation list "M1" is a molar and "F" is a flap | Mobility and furcation codes are always printed next to their word ("Mobility 2 (M2)") and named in the legend |

The periodontal stage is per tooth ("The degree of severity of periodontal disease (PD) relates to a single tooth"). A whole-mouth stage said in the consult is a different, as-said summary.

The AAHA 2019 and WSAVA 2020 dental guidelines could not be read (access refused). Nothing here relies on them.

### What the engine allows

| Area | Today | Effect on a chart |
|---|---|---|
| Content kinds | `statements`, `fields`, `medications`, `text` (`template.rs` `PartKind`) | A chart needs a fifth kind |
| Drafting | llama.cpp with a grammar built from the template's JSON Schema; enums, `anyOf` with null, `$ref` and `minItems` already work | Fixed enums for tooth ids and findings are safe. Dynamic keys (`patternProperties`) are not |
| Faithfulness | Every number in an item must be in its cited lines; a number absent from the whole transcript is a hard fail and blocks signing | A tooth said only by name ("the lower right canine", charted 404) would hard-fail. So would ranges ("101 to 103") |
| Edits | One heading at a time (`edit::apply_edit`); changed items get `entered_by` and are listed, not checked | A chart edit is a new array for one heading. No new engine path |
| Signing | Signed runs never change; later changes are free-text addenda | The chart is read-only once signed |
| Species | Entered by the clinic, never taken from the recording. `for_species` removes other species' headings, but returns the template unchanged when nothing was removed | It can't yet specialise the tooth list by species |
| Run snapshot | Each run saves the template it was drafted with; edits, re-prompts and the PDF reload it | The snapshot is the natural place to keep the dentition |
| PDF | Hand-written writer; only text and rules | Drawing needs a path operator |

### Why "blank" matters

Two competing scribes advertise charts drawn from dictation. Their public material doesn't say whether teeth that weren't mentioned are shown blank or normal. A chart that looks complete when only four teeth were discussed is a false record. OpenBark's rule elsewhere is "Not discussed", never "normal". The chart must keep that rule.

## Decision

### 1. A fifth content kind: `teeth`

A heading can hold a tooth chart: `kind: teeth`, shown as "Tooth chart" in the template builder. A template has at most one. It may be a section or a subheading. It is offered for dogs and cats only.

### 2. Data model: one item per finding

The chart's value is a flat array. Each item is one finding on one tooth, with its own evidence. Teeth that weren't mentioned have no items. An empty array means "Not discussed".

| Key | Type | Meaning |
|---|---|---|
| `tooth` | string, enum of the dentition's Triadan ids | "108" |
| `text` | string | The finding in the words said, short: "slab fracture with pulp exposure" |
| `finding` | string, enum | `normal`, `missing`, `fracture`, `periodontal_stage`, `mobility`, `furcation`, `probing_depth`, `gingival_recession`, `resorption`, `resorption_type`, `persistent_deciduous`, `unerupted`, `retained_root`, `caries`, `wear`, `discoloured`, `malposition`, `extraction`, `crown_amputation`, `other` |
| `detail` | null or string, enum | The type, when said. Fracture: `enamel_infraction`, `enamel_fracture`, `uncomplicated_crown`, `complicated_crown`, `uncomplicated_crown_root`, `complicated_crown_root`, `root`. Extraction: `closed`, `closed_sectioned`, `open`. Resorption stage 4: `crown_and_root_equal`, `crown_worse`, `root_worse`. Wear: `abrasion`, `attrition`. Malposition: `buccoversion`, `distoversion`, `labioversion`, `linguoversion`, `mesioversion`, `palatoversion` |
| `grade` | null or integer 0-5 | Stage, grade or type number as said |
| `mm` | null or number | Probing depth or recession, in millimetres, as said |
| `planned` | boolean | true for "needs to come out"; false for done or found |
| `evidence` | array of segment ids | As every other item |
| `entered_by` | string, edits only | Set by `apply_edit` |

All keys are required and `additionalProperties` is false, like the other item kinds. The schema definition (`$defs/tooth_finding`) is added only to templates that have a chart, so other templates' prompts and grammars don't change.

A table in the shared data file (decision 4) says which `detail`, `grade` range and `mm` each finding may use. For example, `mobility` takes `grade` 0-3 and nothing else.

Stage and type of resorption are separate items, because they are separate AVDC attributes and are often said on different lines (stage at the exam, type on the radiograph).

Whole-mouth findings stay out of the chart: overall periodontal stage, calculus and gingivitis, occlusion class, "no resorptive lesions", "everything else is normal". They go in a "Whole-mouth findings" heading as statements.

### 3. The model writes words; the code writes AVDC codes

The model never writes a code string. The renderer derives the printed words and code from `finding`, `detail`, `grade` and `mm`, from one table.

| Finding | Printed |
|---|---|
| `normal`, `missing` | Normal; Missing (no code exists) |
| `fracture` | Fracture (T/FX); with a type: Complicated crown fracture (T/FX/CCF), and so on |
| `periodontal_stage` | Periodontal disease stage 3 (PD3) |
| `mobility` | Mobility 2 (M2) |
| `furcation` | Furcation 2 (F2) |
| `probing_depth` | Probing depth 7 mm |
| `gingival_recession` | Gingival recession 2 mm (GR) |
| `resorption`, `resorption_type` | Tooth resorption stage 4b (TR4b); Resorption type 2 (T2) |
| `persistent_deciduous`, `unerupted`, `retained_root`, `caries` | DT/P, T/U, RTR, CA, each with its word |
| `wear` | Wear; Abrasion (AB); Attrition (AT) |
| `discoloured` | Discoloured (no code; T/NV means non-vital, which is a different claim) |
| `malposition` | Malpositioned; with a version: Linguoversion (MAL1/LV), and so on |
| `extraction` | Extraction; Closed extraction (X); Closed extraction, sectioned (XS); Open extraction (XSS). Planned: "Planned: open extraction (XSS)" |
| `crown_amputation` | Crown amputation (CR/A) |
| `other` | The as-said text |

An item whose fields don't fit its finding (say `detail: open` on a fracture) prints its as-said text instead of a code, and is flagged (decision 7).

### 4. Species and dentition

- The chart heading carries an optional `dentition`: `canine` or `feline`.
- `for_species` sets it from the patient's species when that is a dog or a cat, and always regenerates the schema. Today it returns early when no heading was removed; that early return goes.
- The tooth enum follows the dentition: 70 ids for a dog (42 permanent and 28 deciduous), 56 for a cat (30 and 26). With no dentition, the dog set is used, because it contains every cat id. The grammar can't then emit a tooth the animal doesn't have.
- The dentition lives in the run's template snapshot. Edits, re-prompts and the PDF all reload that snapshot, so a later change to the patient's species never redraws a signed chart with different teeth. No new field on `RunInfo` is needed.
- When the dentition is unknown (no species entered, or a second animal in a multi-pet visit), the note view lists the findings but draws no diagram. It says "Dentition not recorded". The chart editor asks "Dog or cat?" before it saves, and the edit command writes that choice into the new run's snapshot. It refuses teeth the chosen animal doesn't have.
- One data file, `packages/dental/chart.json`, holds the tooth lists, the drawing order of each row, five schematic tooth outlines (incisor, canine, premolar, carnassial, molar) as simple SVG paths, and the finding table of decisions 2 and 3. Rust loads it with `include_str!`; the app imports it. The UI and the PDF draw from the same numbers.

### 5. How the model fills the chart

Dental template 0.2.0 replaces "Findings by tooth" and the feline "Tooth resorption" heading with one "Dental chart" heading (`kind: teeth`, shown for dogs and cats). "Overall periodontal status" becomes "Whole-mouth findings". The other headings stay. The instruction example is corrected to `108, "upper right fourth premolar"`. Old notes keep their 0.1.0 snapshot and render as before.

When a template has a chart, the engine adds chart rules to the prompt, in both drafting and re-prompting. Teams can't remove them, as with the core rules:

1. Chart a tooth only when it was named, by number or by name, and a finding was said about it.
2. One item per finding. A tooth with a fracture and an extraction has two items.
3. Use the tooth number as said. When only a name was said, use its Triadan number (quadrants, canine x04, first molar x09, deciduous 500-800).
4. Never chart a tooth as normal unless normal, fine, healthy or the like was said about that tooth.
5. Chart a tooth as missing only when it was said to be missing or absent. Never infer missing from an extraction or from silence.
6. Whole-mouth statements ("stage two across the mouth", "no resorptive lesions", "everything else looks normal") never go on teeth.
7. A range or group ("101 to 103", "all the lower incisors") becomes one item per tooth, only when every tooth in it is clear.
8. `grade` and `mm` only when said. `detail` is null when the type wasn't said: "fractured" alone is never a guessed complicated fracture.
9. `planned` is true for a plan. A treatment planned and then done in the same recording is charted once, as done.
10. If a number and a name disagree ("108, upper left fourth premolar"), chart the number as said and keep both in `text`.

The prompt gets one tooth example in its JSON formats, only for templates with a chart. The example is not taken from any eval fixture. A finding costs about 50 tokens, so the drafting budget for a template with a chart rises from 4096 to 8192 tokens (the context limit is 32768). The array allows at most 96 findings.

### 6. Never invent findings

| Rule | Where it is enforced |
|---|---|
| Teeth not mentioned stay blank, not normal | Schema (an array of findings, not a key per tooth), prompt rule 1, the diagram (blank outline) and the legend ("Blank: not charted in this recording. It doesn't mean normal.") |
| "Normal" only when said for that tooth | Prompt rule 4; a cue-word check flags "normal" without a normal word in its cited line |
| "Missing" only when said | Prompt rule 5; cue-word check |
| No tooth the animal doesn't have | The tooth enum per dentition, in the grammar and in the editable schema |
| No tooth that wasn't said | The tooth check (decision 7): a hard fail that blocks signing |
| No guessed types, stages or sizes | Prompt rule 8; `grade` and `mm` go through the existing number check |
| No findings from earlier visits | The chart is never pre-filled. A past chart may be shown later as reference only |
| No "mark the rest normal" | There is no such button. A vet who examined other teeth selects them and marks them, as their own entries |

### 7. Faithfulness checks for teeth

A new module, `engine/os-engine/src/dental.rs`, reads teeth from transcript text. It knows:
- numbers said as digits or spoken ("one oh eight", "three ten", "four eleven");
- names: side, upper or maxillary, lower or mandibular, incisor (central, intermediate, corner), canine, premolar, molar, carnassial, ordinals, "P4" and "M1" forms, and deciduous words (deciduous, baby, milk);
- ranges in one jaw ("101 through 103", "103 to 203" across the midline);
- groups ("the lower incisors", "upper canines").

Names map to the same numbers for dogs and cats, so this needs no species.

The faithfulness pass recognises a tooth item by its `tooth` and `finding` keys. For each one not entered by the vet:

| Check | Result |
|---|---|
| The tooth is in the cited lines, by number, name, range or group | Passes |
| The tooth is said elsewhere in the transcript, but not in the cited lines | Citation mismatch (soft), as for numbers today |
| The tooth is said nowhere | Unsupported number (hard). Blocks signing until the vet corrects it |
| A number and an adjacent name in the cited line mean different teeth | Name conflict (soft): "Said as 108 and as upper left fourth premolar (208). Check which tooth." Never corrected automatically |
| The second recogniser heard a different number | Disputed number (soft), as today |
| The cited line has no cue word for the finding (for example "missing" without missing, absent or gone) | Cue missing (soft) |
| `detail`, `grade` or `mm` don't fit the finding | Shape (soft) |
| `grade`, `mm` and numbers in `text` | The existing number check |

The generic number check skips the `tooth` key, because the tooth check replaces it. Soft flags go in a new `tooth_flags` list in the faithfulness report. Hard and mismatch results reuse the existing lists, so the existing red callout and dotted underline work unchanged.

Name matching ships for English. In another consultation language, a tooth said only by name shows as unsupported until the vet edits it. Name tables for French, Spanish, German and Portuguese follow in the plan, checked by native-speaking vets.

### 8. Editing by clicking a tooth

- In an unsigned draft, clicking a tooth (or Enter on it) opens the chart editor in place, with that tooth selected. The heading's Edit button does the same with nothing selected.
- The editor shows the chart and a panel for the selected teeth. The panel lists each finding with its source line and play, and offers Change and Remove. "Add finding" offers only valid choices for the finding picked, with "Not said" for an unknown type. "Mark missing" and "Mark normal" act on teeth with no findings.
- Ctrl or Cmd plus click, or Space on the focused tooth, selects several teeth. One action then applies to all of them.
- Save sends the whole array through the existing edit command. Changed and added items become the vet's own (`entered_by`, "Edited by"), need no evidence and never block signing. Unchanged items keep their links. Removed items are counted. The earlier draft is kept.
- Hand edits must fit the finding table; the edit command refuses a mismatch with a plain message.
- Ask (re-prompt) works on the chart heading, as on any heading.

The plan gives the exact keyboard and pointer behaviour.

### 9. Rendering

**Note view (SVG, hand-written React, no new dependency).**
- The AVDC table layout, seen from the front: patient's right on the viewer's left. Upper row 110-101 | 201-210; lower row 411-401 | 301-311; deciduous rows inside them when shown. A midline hairline, and "Patient's right" and "Patient's left" captions.
- Schematic outlines, not drawings. Tooth numbers in JetBrains Mono, caption size.
- Uncharted teeth: hairline outline in `--ob-text-3`, no mark. Charted teeth: ink outline.
- Marks are shapes in ink: a circle for missing, an X for extracted (dashed when planned), a zigzag across the crown for a fracture, a notch at the neck for resorption, a line across the neck for crown amputation, and a small dot for any other finding.
- The selected tooth gets the accent outline while the chart has focus, the chart's one active mark (brand guide §3.5). Focus uses the standard 2 px accent ring.
- Below the diagram: one line per tooth, in Triadan order, with each finding as an evidence-linked statement ("108  Complicated crown fracture (T/FX/CCF); open extraction (XSS)"). Then a text legend of only the codes used. Status is never shown by colour alone; the words and codes carry it.
- Hovering or focusing a tooth lights the transcript lines of all its findings. Each tooth has an accessible name such as "108, right maxillary fourth premolar: complicated crown fracture; open extraction".
- Colours are tokens and `currentColor`, so dark mode follows the brand guide's tokens.
- At 768 px and wider, the whole mouth shows in one view. Narrower, it shows as two blocks (patient's right, then left), so each tooth stays at least 24 px wide.
- An empty chart shows "Not discussed" and no diagram.

**PDF.** `pdf.rs` gains a path operator (move, line, curve, close, stroke, fill, dash; a few lines, no dependency). Under the chart heading it draws the same schematic in greys, then the per-tooth list in words with codes, then the legend. The diagram and the start of its list stay on one page. No citations, as for every PDF. Nothing charted prints "Not discussed" and no diagram, so a blank chart is never read as a full normal exam.

**Text forms.** The Markdown note (`render.rs`), Copy note (`copyFormats.ts`) and a heading's Copy all render the chart as the per-tooth list, for example `- 108: complicated crown fracture (T/FX/CCF) [4]; open extraction (XSS) [8]`. PIMS text boxes get the record in words. Client documents, which take the vet's note as reference, can describe extractions.

### 10. Signing and addenda

Signing is unchanged. A hard fail (a tooth not said anywhere) blocks it, like an invented number. Once signed, the chart is read-only: clicking a tooth only highlights its findings. Corrections are free-text addenda, as today. Structured chart addenda are left for later.

### 11. Translations

- Enum values in the schema stay English. The model writes `text` in the note language.
- AVDC codes stay as they are in every language.
- Finding words, tooth names, legend lines and editor labels are translated in the app (the four shipped interface languages) and, for the PDF, in the PDF label tables (the same five languages as today). Tooth names are built from short parts ("Right maxillary {tooth}", "fourth premolar").
- The Markdown note stays English, as today.
- Every translation is listed for review by a native-speaking vet before release.

### 12. What doesn't change

- No network calls: `docs/network.md` is unchanged.
- No new dependencies: `docs/versions.md` is unchanged.
- Equine, rabbit and rodent dentitions, radiograph images and six-site probing grids are out of scope.

## Consequences

- The M2 acceptance is met by the existing machinery: per-item evidence, per-heading edits and immutable signed runs. The new parts are the kind, the tooth reader and the drawing.
- The record stays honest: a tooth is blank unless something was said about it, and the legend says so on screen and on paper.
- A tooth said only by name no longer fails the number check, and ranges and groups are understood.
- Templates with a chart allow longer drafts (8192 tokens). Templates without one are unchanged, down to their schema and prompt.
- The dental template moves to 0.2.0. Other species lose the per-tooth bullet heading in that template. Clinics that need it can keep a team copy of 0.1.0.
- The cat-or-dog question appears when the species wasn't entered. That is one extra click, instead of a guess.
- Translations exist twice, in the app catalog and in the PDF label table, as for today's PDF labels. A test checks that every language has every word.
- The wrong-side example is fixed. The old fixture keeps its audio and becomes the name-conflict test; clean fixtures carry the main acceptance tests.
- The website's chart claim stays "planned" until this ships.

## Alternatives considered

| Option | Why not |
|---|---|
| A fixed object with one nullable key per tooth | The model would write every tooth on every draft, which wastes tokens and pushes it to fill teeth in |
| An object keyed by tooth number (`patternProperties`) | Poorly supported by the grammar converter |
| One item per tooth holding a list of findings | Findings said on different lines would share one citation, and edits would diff less cleanly |
| The model writes AVDC code strings | Invites invented or non-AVDC codes. Codes are derived in code instead |
| A discriminated union per finding in the schema (`anyOf`) | A larger grammar and more converter risk. The finding table, the shape flag and the editor cover it. Revisit if the eval shows mismatches |
| Keep teeth as bullet points and draw the chart by parsing the text | Parsing free text is fragile and gives no reliable codes |
| Fill unmentioned teeth as normal, or a "mark the rest normal" button | Invents findings. The vet can select teeth and mark them deliberately |
| Pre-fill from the last visit's chart | Carries old findings into a new record. Past charts may be shown later as reference only |
| Take the species from the recording | Breaks the rule that species is entered by the clinic |
| A `species` field on `RunInfo` | The template snapshot already travels with every run (edits, re-prompts, PDF). Keeping the dentition on the heading keeps one source |
| Render the chart in Rust as SVG for both the app and the PDF | The app needs focusable, hoverable teeth wired to the evidence link. A shared data file keeps both in step instead |
| Embed a PDF library or a drawing font | Not needed. A path operator is a few lines |
| Structured chart addenda after signing | Useful later. v1 addenda stay free text, as for every heading |
| Equine, rabbit and rodent dentitions now | Different numbering and anatomy. The chart is gated to dogs and cats in v1 |
| A six-site probing grid by voice | Too much to dictate reliably in v1. One probing depth per item, with the site kept in `text` |

## Implementation notes (2026-10-19)

Built on branch `feat/dental-chart`, on top of template editing (ADR-0024). The status stays proposed until the owner approves.

What shipped as decided:
- `PartKind::Teeth`, the `dentition` on the chart heading, `$defs/tooth_finding` only for templates with a chart (every other built-in's schema and prompt are byte-identical; SOAP's prompt file is unchanged), `for_species` setting the dentition, and `LoadedTemplate::with_dentition` for the edit command.
- `packages/dental/chart.json`, read by `engine/os-engine/src/dental.rs` and `apps/desktop/src/dental.ts`: tooth lists, rows, five outlines, the finding table with AVDC codes and cue words.
- The tooth reader (numbers, spoken numbers, names, ranges, groups), the tooth check in the faithfulness pass (hard fail for a tooth said nowhere; citation mismatch; soft `tooth_flags`: name conflict, cue missing, shape, duplicate) and the chart rules in drafting and re-prompting.
- Dental template 0.2.0: "Dental chart" (`kind: teeth`, dogs and cats) and "Whole-mouth findings"; the wrong-side example corrected. Fingerprints rewritten with `openbark templates fingerprints --write`.
- The app: `DentalChart` (diagram, per-tooth list with evidence links, legend) and `DentalChartEditor` (click, Ctrl or Cmd-click, keyboard, Mark missing or normal, Add finding with only the valid controls, the dog-or-cat choice). The plan called them `ToothChart` and `ChartEditor`. Edits go through the existing edit command, with `dentition` on the Tauri command and `POST /api/sessions/{id}/edit`; the earlier draft is kept; signed charts are read-only.
- Markdown, Copy note, a heading's Copy (words, never the diagram's numbers) and the PDF (`pdf.rs` path operator, greys, the same legend, translated words in the five PDF languages).
- Template from an example note proposes a `teeth` heading (at most one; a second becomes bullet points).

Added during the build, beyond the decision:
- **Tooth hints in the prompt.** For a template with a chart, each transcript line that names teeth gets `[teeth: …]` with the Triadan numbers the tooth reader finds in it, limited to the dentition, and a chart rule says to go through every such line and never to copy the list into `text`. Only the prompt sees them; the saved transcript is unchanged; templates without a chart are untouched. It took chart recall on Gemma 4 E4B from 50% to 79%, with no invented tooth (docs/eval/M2.md). It works because the model then numbers names and groups the same way the faithfulness pass checks them.
- **A finding table in the prompt.** The JSON formats of a chart template list what each finding takes (its `detail` values with the words that signal them, `grade` range, `mm`, `planned`), generated from `chart.json`, since the grammar allows every detail on every finding.
- **The word check skips charted findings** except `other`. A charted finding prints its words from the finding table, and the cue-word check is its word check ("the lower incisors come out" supports an extraction, which the generic word check flagged).
- `os_llm::grammar_for_schema` exposes the grammar conversion, and a test converts the dental schema for a dog, a cat and no species.
- An os-app test covers the dog-or-cat choice: written into the new run's snapshot, a different second choice refused, a tooth the animal doesn't have refused.
- Eval keys: `chart`, `chart_flags`, `chart_not_allowed` (with `except`), and `cites` in an expected item (a word its cited line must hold).
- `apps/desktop/e2e/dental-chart.mjs`: imports the three new fixtures on a scratch demo, drafts them, and checks the chart in desktop light and dark and at 360 px (light and dark), the evidence link, editing by click and keyboard, the cat's teeth, the dog-or-cat question, the signed read-only chart and the PDF.

Not done yet, or different from the plan:
- Chart recall on Gemma 4 E4B is 79% against the plan's 90% target (12B: 88%). Invented teeth, invented normals and invented numbers are 0. Misses are blank teeth or missing findings, mostly in the cat consult.
- The property-order trial (`text` after `finding`) wasn't run.
- No schema snapshot files (`schema.canine.json`, `schema.feline.json`); the tests check the tooth enum counts and that other templates have no tooth definition instead.
- Dental shots are taken by the new e2e script, not by `screenshots.mjs`, and the PDF page isn't rendered to PNG.
- Tooth names are read in English only. French, Spanish, German and Portuguese name tables, and a native-speaking vet's review of every chart translation (app and PDF), are still to do.
