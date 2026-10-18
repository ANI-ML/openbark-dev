# Plan: dental chart

Status: **proposed**, with [ADR-0021](../adr/0021-dental-chart.md). Date: 2026-10-18. Delivers M2 item 7.

## Goal

A tooth chart for dogs and cats, in modified Triadan numbering. Gemma fills it from the consult, and every finding links to the transcript line it came from. Teeth that weren't mentioned stay blank, never "normal". The vet edits it by clicking a tooth. It shows in the note, in every text copy and in the PDF.

## Acceptance

From M2 item 7:
1. The chart fills from a scripted dental fixture.
2. Every finding cites a transcript line.
3. Clicks edit it as the vet's own entries.

Added by this plan:
4. No tooth is charted that wasn't said, and no tooth is "normal" unless normal was said for it (eval: 0 invented teeth, 0 invented normals).
5. A tooth said only by name, a range or a group passes the faithfulness pass. A tooth said nowhere blocks signing.
6. "108, upper left fourth premolar" is charted as 108 and flagged as a name conflict.
7. The chart reads in light and dark mode, at desktop and phone width, by keyboard alone, and is drawn the same in the app and the PDF.
8. Templates without a chart are unchanged: same schema, same prompt, same SOAP snapshot.

## Where each step is tested

| Tag | Meaning |
|---|---|
| UNIT | `cargo test -p os-engine` (and `-p os-app`, `-p os-cli` where named); `pnpm -C apps/desktop test` (vitest) and `pnpm -C apps/desktop typecheck` |
| GOLDEN | Unit tests with fixed inputs and exact expected outputs (tooth reader tables, schema snapshots, Markdown and copy text, PDF operators) |
| EVAL | `openbark eval` on the Windows dev box with the CUDA build and Gemma 4, on the scripted fixtures (audio from `fixture-tts --kokoro`) |
| DEMO | A demo server with scratch data only: `OPENBARK_SESSIONS_DIR`, `OPENBARK_TEMPLATES_DIR`, `OPENBARK_USERS_FILE` and `OPENBARK_DATA_KEY` set to scratch paths, on port 4321. **Never the clinic copy on 4317, never real data** |

## Steps

Order: data and engine first (steps 1-8), then the app (9-12), then the PDF and translations (13-14), then the eval, end-to-end tests, screenshots and docs (15-18). Each step leaves the build green.

### Step 1: the shared chart data file (UNIT, GOLDEN)

New file `packages/dental/chart.json`:

| Key | Content |
|---|---|
| `teeth` | One entry per Triadan id: `id` ("108"), `jaw` (`upper`, `lower`), `side` (`right`, `left`), `deciduous` (bool), `type` (`incisor`, `canine`, `premolar`, `molar`), `ordinal` (1-4), `shape` (`incisor`, `canine`, `premolar`, `carnassial`, `molar`), `dentitions` (`["canine", "feline"]` or `["canine"]`) |
| `rows` | Per dentition, the drawing order of each row, patient's right on the viewer's left: `upper` 110…101, 201…210; `lower` 411…401, 301…311; `upper_deciduous`; `lower_deciduous` |
| `shapes` | Five outlines as SVG path strings in a 20 x 40 box, crown at the top, using only absolute `M`, `L`, `C` and `Z` (so the PDF can parse them). Lower teeth are drawn mirrored |
| `findings` | Per finding: allowed `details` (each with its AVDC code), `grade` range or none, `mm` allowed or not, the code pattern (`PD{grade}`, `M{grade}`, `F{grade}`, `TR{grade}{sub}`, `T{grade}`), the glyph (`circle`, `x`, `zigzag`, `notch`, `neck`, `dot`, `none`) and the cue words the faithfulness pass looks for |

Rust: `engine/os-engine/src/dental.rs` loads it once (`include_str!` and a `LazyLock`). App: `apps/desktop/src/dental.ts` imports it and exports typed helpers.

Tests (GOLDEN, both sides):
- Dog permanent 42, dog deciduous 28, cat permanent 30, cat deciduous 26 (checked against the AVDC table).
- Every cat id is a dog id.
- Every x04 is a canine; every x09 is a first molar; 108, 208, 309, 409 use the carnassial shape.
- No cat 105, 205, 305, 306, 405 or 406. No x05 in any deciduous set. No 706 or 806 for a cat.
- Each row lists exactly its dentition's teeth for that jaw, once each.
- Every shape path parses with the Rust parser.
- Every `details` entry of every finding has a code, and no code is "PD" alone.

Done when: both test suites read the same file and agree on the counts.

### Step 2: the `teeth` content kind in the engine (UNIT, GOLDEN)

`engine/os-engine/src/template.rs`:
- `PartKind::Teeth` (serialised `teeth`).
- `Dentition { Canine, Feline }` and `dentition: Option<Dentition>` on `Section` and `Part` (skipped when none). `normalize` refuses a dentition on a non-chart heading, and a second chart heading ("A template can have one tooth chart.").
- `kind_schema(Teeth)`: `{ "type": "array", "items": { "$ref": "#/$defs/tooth_finding" }, "maxItems": 96 }`.
- `generate_schema` adds `$defs/tooth_finding` only when the template has a chart. Property order: `tooth`, `text`, `finding`, `detail`, `grade`, `mm`, `planned`, `evidence`. All required, no extra properties. `tooth` is an enum of the dentition's ids (dog set when none); `grade` is null or an integer 0-5; `mm` is null or a number of at least 0.
- `empty_note`: `[]` for a chart.
- `for_species`: set the chart's dentition from `species_key` when it is `canine` or `feline` and the heading has none. Always rebuild the template when the dentition changed, even if no heading was removed (remove the early return for that case).
- `LoadedTemplate::with_dentition(heading, Dentition)` for the edit command (step 8).
- `edit.rs` `editable_schema`: add `/tooth_finding/properties` to the pointers that allow `entered_by`.

Tests:
- A YAML template with `kind: teeth` loads; its empty note validates.
- The SOAP schema snapshot test still passes unchanged (no `tooth_finding` in its `$defs`).
- New snapshots `packages/templates/dental/schema.canine.json` and `schema.feline.json`, checked like the SOAP one.
- The canine enum has 70 ids, the feline 56, the unknown 70.
- `for_species(Some("Cat"))` on the dental template sets `feline` and changes the schema even though no heading is removed; `for_species(None)` leaves it unset.
- The template snapshot round-trips with its dentition (`to_yaml`, `from_yaml`).
- Two chart headings are refused; a dentition on a statements heading is refused.
- `heading_schema("dental_chart")` stands alone and validates `[]`.
- The dental schema converts to a llama.cpp grammar (`llama_cpp_2::json_schema_to_grammar`) in an `os-llm` test. If integer `minimum`/`maximum` is rejected, use `enum: [0,1,2,3,4,5]` instead. Grammar build stays under 50 ms.
- Update `heading_examples_load_but_stay_out_of_the_prompt` to the new heading id.

Done when: the dental template's schema has the chart and every other built-in template's schema is byte-identical to before.

### Step 3: the tooth reader and code table (UNIT, GOLDEN)

`dental.rs` adds:
- `teeth_in(text) -> Vec<Mention>`: each mention has the teeth it covers, how they were said (`Number`, `Name`, `Range`, `Group`) and its word span.
- Numbers: digits ("108"), digit by digit ("one oh eight", "one o eight", "one zero eight"), "three ten", "four eleven", "one hundred and eight". Only valid Triadan ids count. This reader is local to `dental.rs`, so `numbers.rs` behaves as before for doses and vitals.
- Names: side (right, left); jaw (upper, maxillary, maxilla, top; lower, mandibular, mandible, bottom); tooth (central, intermediate or middle, corner or lateral incisor; first to third incisor; canine, fang, cuspid; first to fourth premolar; first to third molar; P1-P4, PM1-PM4, M1-M3, "P four"; upper carnassial = x08, lower carnassial = x09); deciduous words (deciduous, baby, milk, primary). "Retained" alone is not deciduous.
- Ranges, in one jaw only: the teeth between the two ends along the arcade, crossing the midline if needed ("101 through 103", "103 to 203"). Ranges across jaws aren't expanded.
- Groups: "upper incisors", "lower incisors", "upper right incisors", "canines", "upper canines", "lower premolars" and similar. Groups use the dog set; the dentition enum already excludes a cat's absent teeth.
- `name_of(id) -> parts` for the app and PDF ("right", "maxillary", "fourth premolar", deciduous).
- `code_of(item) -> (words key, code)` and `fits(item) -> Result<(), Misfit>` from the finding table.

Tests (GOLDEN tables):
- Every id of every dentition: `name_of(id)` read back by `teeth_in` gives the same id.
- Spoken forms: "one oh eight", "1 08", "upper right fourth premolar", "right maxillary P4", "the right upper carnassial" → 108. "lower left first molar", "left mandibular carnassial" → 309. "lower right canine" → 404. "baby upper right canine", "deciduous right maxillary canine" → 504. "baby lower left canine" → 704. "three ten" → 310. "four eleven" → 411.
- Ranges: "101 through 103" → 101, 102, 103. "one oh three to two oh three" → 103, 102, 101, 201, 202, 203. "108 to 308" → nothing expanded.
- Groups: "all the lower incisors" → 301-303, 401-403. "upper canines" → 104, 204.
- Ambiguous: "the canine", "a premolar", "the left side" → no tooth.
- Not teeth: "three oh nine pm", "a 108 mg tablet" → not read as teeth (unit or time follows).
- `code_of`: periodontal stage 3 → PD3; mobility 2 → M2; fracture + complicated_crown → T/FX/CCF; fracture + null → T/FX; resorption 4 + crown_worse → TR4b; resorption_type 2 → T2; extraction + open → XSS; extraction planned + null → "Planned: extraction"; probing depth 7 → "Probing depth 7 mm" with no code; missing → "Missing".
- `fits`: mobility with grade 4 fails; fracture with detail `open` fails; probing depth with no mm fails; resorption with `crown_worse` and grade 3 fails.

Done when: the tables pass and the reader never returns a tooth for an ambiguous phrase.

### Step 4: faithfulness for teeth (UNIT, GOLDEN)

`faithfulness.rs`:
- In `check_statement`, an item with `tooth` and `finding` keys calls `dental::check_item`. The generic number loop skips the `tooth` key; `text`, `grade` and `mm` still go through it.
- `check_item`, for items not entered by the vet:

| Case | Report |
|---|---|
| Tooth in the cited lines (number, name, range or group) | Passes |
| Tooth said elsewhere only | `citation_mismatches` entry, `number` = the tooth, `found_in` = those lines |
| Tooth said nowhere | `unsupported_numbers` entry (hard, blocks signing) |
| A number and a name within six words of each other in a cited line mean different teeth | `tooth_flags` `name_conflict`, with `other` = the tooth the name means |
| Tooth said by number and the second recogniser heard another number there | `disputed_numbers` entry, as for other numbers |
| No cue word for the finding or its detail in the cited lines | `tooth_flags` `cue_missing`, with the words looked for |
| `fits` fails | `tooth_flags` `shape` |
| Same tooth, finding, detail, grade, mm and planned as an earlier item | `tooth_flags` `duplicate` |

- New `ToothFlag { path, tooth, kind, other, expected }` and `FaithfulnessReport.tooth_flags` (default empty, so old `note.json` files still load). `flagged_paths` includes them. `passed` is unchanged: only invalid evidence and unsupported numbers fail.
- Cue words (from the data file): normal: normal, fine, healthy, good, unremarkable, intact; missing: missing, absent, not present, gone; fracture: fracture, fractured, broken, slab, chip, chipped; pulp exposure details: pulp, exposed, exposure; root details: root; resorption: resorption, resorptive, FORL, lesion; mobility: mobile, mobility, loose; furcation: furcation; probing depth: pocket, probing, probe, depth; recession: recession, receded; extraction: extract, extracting, extracted, extraction, out, remove; closed: simple, closed, no sectioning; closed_sectioned: section, sectioned, split; open: surgical, surgically, flap, open; crown amputation: amputation, amputate; persistent deciduous: persistent, retained, baby, deciduous; unerupted: unerupted, not erupted, hasn't erupted. Words are folded (case, plurals, simple stems) as in `align.rs`.
- `engine.ts` `Faithfulness` gains `tooth_flags?`.

Tests (GOLDEN, small transcripts):
- "Four oh four, the lower right canine, looks fine" with 404 normal → passes, no flags.
- "The upper right canine is normal" with 104 normal → passes (name only).
- 105 charted on a line about 104 → unsupported, hard; `passed` false.
- 309 charted citing line 2 when it was said on line 5 → citation mismatch.
- "One oh eight, upper left fourth premolar, has a slab fracture" with 108 → passes the tooth check and gets `name_conflict` with `other` = "208".
- "All the lower incisors are mobile, grade three" with six mobility items → all pass.
- "Two oh nine is missing" with 209 normal → `cue_missing`.
- "Three oh nine is fractured" with detail null → no flag; with `complicated_crown` → `cue_missing` (no pulp word).
- A vet-entered item is listed in `vet_entered` and not checked.
- `grade: 3` with only "stage two" in the line → unsupported number, as today.
- Report JSON without `tooth_flags` deserialises.

Done when: the fixture conflict is soft-flagged, name-only teeth pass, and an invented tooth blocks signing.

### Step 5: prompt rules and drafting budget (UNIT, EVAL)

`note.rs`:
- `guidance_line` for `Teeth`: `- dental_chart: Dental chart (a tooth chart: one item per finding on one tooth; teeth not mentioned are left out)`, plus the heading's instruction and standards.
- `CHART_RULES`: the ten rules in ADR-0021 decision 5, added by `rules()` whenever the template has a chart, so drafting and re-prompting both get them, and no team template can remove them.
- `JSON_FORMATS` gains a tooth line only for templates with a chart. Its example is not from any fixture: `{"tooth": "407", "text": "3 mm of gingival recession", "finding": "gingival_recession", "detail": null, "grade": null, "mm": 3, "planned": false, "evidence": [12]}`.
- A short Triadan reminder inside the rules: "100 right upper, 200 left upper, 300 left lower, 400 right lower; 500-800 the same for deciduous teeth; the canine is x04 and the first molar x09."
- `NoteOptions.max_tokens` is 8192 when the template has a chart (`os-app` `note_options` and `os-cli` `note_options`).

Tests:
- `system_prompt` of the dental template contains every chart rule and the tooth format line. The SOAP prompt is byte-identical to before.
- `build_refine_messages` for `dental_chart` contains the chart rules.
- The canine dental prompt doesn't mention any fixture tooth example.

Done when: prompts change only for templates with a chart.

### Step 6: dental template 0.2.0 (UNIT, EVAL)

`packages/templates/dental/template.yaml`:
- `version: 0.2.0`.
- Instructions: fix the example to `108, "upper right fourth premolar"`, and say teeth are charted under Dental chart.
- Replace "Findings by tooth" and "Tooth resorption" with:
  ```yaml
  - title: Dental chart
    kind: teeth
    presence: required
    when: { species: [canine, feline] }
    instruction: Every tooth named with a finding, one item per finding. Teeth not mentioned are left out.
    examples:
      - "Chart every tooth that was named, by its Triadan number"
      - "Add the extraction technique for each extracted tooth"
      - "Include probing depths in mm when they were called out"
      - "Split ranges such as 101 to 103 into one item per tooth"
  ```
- Rename "Overall periodontal status" to "Whole-mouth findings", with the instruction: "Findings about the whole mouth as said: overall periodontal stage, calculus, gingivitis, occlusion, 'no resorptive lesions'. Never per tooth."
- Keep "Treatment performed" for scaling, polishing, radiographs, flaps and closures; extractions may also appear there as said.
- Rules: drop the per-tooth statement rule; keep the nerve-block rule.
- `apps/desktop/src/builtinText.ts`: add "Dental chart" and "Whole-mouth findings" (the built-in template text test requires them).
- `packages/eval/fixtures/example-notes/dental.txt`: change "307: resorptive lesion type 1, stage 3" to "type 2, stage 3", so it agrees with "Root replacement 307 confirmed".

Tests:
- Every built-in template loads and its empty note validates (existing test).
- The PDF test of every built-in template still prints every heading (existing test).
- Old runs: a session saved with the 0.1.0 snapshot renders, edits and exports as before.

Done when: the dental template drafts a chart for dogs and cats and nothing else changes for other templates.

### Step 7: Markdown and copy text (UNIT, GOLDEN)

- `render.rs`: a `teeth` branch. One line per tooth in Triadan order, findings joined with "; ", each with its citation or "[entered by …]" and its flag: `- 108: complicated crown fracture (T/FX/CCF) [4] [check against recording]; open extraction (XSS) [8]`. Empty: "Not discussed." English, as the rest of the Markdown.
- `copyFormats.ts`: a `teeth` branch in `lines()`, same layout, words in the interface language. `CopyOptions` gains `t` (the translator) so the words translate.
- `NoteView` heading Copy: for a chart heading, copy the text form, not `innerText` (which would pick up the SVG's tooth numbers).

Tests (GOLDEN): Markdown and plain, Markdown and HTML copy of a fixed chart, with and without citations; a vet-entered item; a flagged item; an empty chart.

Done when: every text form shows the chart in words, and nothing copies stray tooth numbers.

### Step 8: the edit path (UNIT)

- `edit::apply_edit`: for a chart heading, every changed or added item must pass `dental::fits` and use a tooth of the dentition; otherwise `EngineError::Template` with a plain message ("Mobility takes a grade from 0 to 3.", "105 isn't a cat's tooth."). The model's own items are never refused here; they are flagged in step 4.
- `os-app` `edit_note` gains `dentition: Option<&str>`. When the chart heading has no dentition and one is given, the new run's snapshot is `template.with_dentition(...)`. Refuse when the heading already has a different one, or when a charted tooth isn't in it.
- Tauri `edit_note` command and the HTTP edit route (`POST /api/sessions/{id}/edit`) take the optional `dentition`. `engine.ts` `editNote(..., dentition?)`.
- Order the saved array by tooth, then by the finding table's order. Moving unchanged items doesn't mark them (existing behaviour).

Tests:
- A chart edit through `apply_edit`: a changed item gets `entered_by`, an added item needs no evidence, an unchanged item keeps its evidence, a removed item is counted, and the run still signs.
- An edit that adds 105 to a feline chart is refused. An edit with mobility grade 4 is refused.
- `edit_note` with `dentition: "feline"` on an unset chart writes it into the new run's snapshot; a second edit with `"canine"` is refused.
- The earlier draft is still on disk after a chart edit.

Done when: hand edits can't save a malformed chart and always become the vet's own entries.

### Step 9: template from example and the builder (UNIT)

- `from_example.rs`: add `teeth` to the kinds, described as "a tooth chart for dogs and cats: findings by Triadan number". Map it in `content()`. A pasted "DENTAL CHART" heading becomes a chart.
- `packages/eval/fixtures/example-notes/dental.headings` stays the same list; the from-example test also checks that "Dental chart" gets `kind: teeth`.
- `Templates.tsx`: `KINDS` gains "Tooth chart"; `KIND_HINT.teeth`: "Teeth by Triadan number, for dogs and cats. Filled from what was said; teeth not mentioned stay blank." The builder offers it only when the template has no chart yet. The preview shows an empty chart.
- `engine.ts`: `ContentKind` gains `"teeth"`; `TemplateSection` and `TemplatePart` gain `dentition?: "canine" | "feline" | null`.

Tests: the dental example note proposes a chart heading; the builder hides "Tooth chart" when one exists; a saved team template with a chart drafts.

### Step 10: the chart view component (UNIT)

New `apps/desktop/src/components/ToothChart.tsx`, hand-written SVG, no new dependency.

Props: `dentition` (or null), `items`, `faithfulness`, `path`, `onEvidence`, `onPlay`, `selected`, `onToothActivate`, `showDeciduous`.

Drawing (all sizes in CSS px at 1x; the SVG scales by `viewBox`):

| Element | Look |
|---|---|
| Tooth slot | 32 wide; outline 20 x 40 from `chart.json`; upper teeth crown down toward the bite line, lower teeth mirrored |
| Uncharted tooth | 1 px outline, `--ob-text-3`, no mark |
| Charted tooth | 1.5 px outline, `--ob-text` (ink) |
| Missing | Circle around the tooth, ink |
| Extraction | X through the tooth; dashed when planned |
| Fracture | Zigzag across the crown |
| Resorption | Notch at the neck |
| Crown amputation | Line across the neck |
| Any other finding | 4 px filled dot under the tooth |
| Faithfulness flag | "!" beside the tooth number; the message is in the list |
| Tooth number | JetBrains Mono 12 px, `--ob-text-2`; above upper teeth, below lower teeth |
| Midline | Hairline, `--ob-surface-3` |
| Captions | "Patient's right" over the left half, "Patient's left" over the right half, caption style |
| Selected tooth | 2 px `--ob-accent` outline while the chart has focus; 2 px ink outline otherwise |
| Focus | 2 px accent ring, 2 px offset (brand guide §10) |

- Rows: permanent upper, then lower. Deciduous rows sit between them when shown. They show automatically when a deciduous tooth is charted.
- Width: at 768 px and wider, the whole mouth in one view (22 slots fit the 720 px reading width). Narrower, two blocks: patient's right (quadrants 100 and 400, deciduous 500 and 800), then patient's left. Each tooth stays at least 24 px wide (WCAG 2.2 target size). The page never scrolls sideways.
- Under the diagram: the per-tooth list, one line per tooth in Triadan order. The tooth number is in mono. Each finding is an evidence-linked statement, reusing NoteView's `statement` (hover lights its lines, click plays, flags show as today, "Edited by" for the vet's items).
- Then the legend: only the codes used, in AVDC wording, plus "Blank: not charted in this recording. It doesn't mean normal." and "Right and left are the patient's." Mobility, furcation and resorption lines name their AVDC index.
- No dentition: no diagram. The list, then "Dentition not recorded. Edit the chart to choose dog or cat."
- Empty chart: "Not discussed", no diagram.
- All colours are CSS tokens or `currentColor`, so dark mode follows the brand tokens with no extra code.

Accessibility:
- The diagram is one tab stop, with a roving `tabindex` across the teeth. Each tooth has `role="button"`.
- Each tooth's `aria-label`: "108, right maxillary fourth premolar: complicated crown fracture; open extraction", or "110, right maxillary second molar: not charted".
- Arrow keys move focus (step 11 table). Hover or focus lights the evidence of all the tooth's findings through `onEvidence`.

Tests (vitest and Testing Library):
- A canine chart draws 42 permanent teeth; a feline one 30, with no 105 or 305; deciduous rows appear when 504 is charted.
- Uncharted teeth have no mark; 209 missing has the circle; a planned extraction has the dashed X.
- Hovering 309 calls `onEvidence` with the union of its findings' evidence.
- Every tooth has an accessible name; a hard-flagged tooth shows "!" and the list shows the callout text.
- No dentition: no `<svg>`, and the notice text.
- Empty chart: "Not discussed" and no `<svg>`.
- Phone width (mocked `useMediaQuery`): two blocks.

### Step 11: the chart editor and its exact interaction (UNIT)

New `apps/desktop/src/components/ChartEditor.tsx`. `HeadingEditor` renders it for `kind: teeth`. It keeps the heading editor's Save and Cancel and the accent gutter bar while editing.

Layout: the chart (from step 10, all rows interactive) above a panel for the selection.

| Input | Result |
|---|---|
| Click a tooth (view mode, unsigned draft) | Opens the editor in place with that tooth selected |
| Click a tooth (editor) | Selects only that tooth |
| Ctrl or Cmd plus click | Adds the tooth to the selection, or removes it |
| Enter on the focused tooth | Selects only that tooth (same as click) |
| Space on the focused tooth | Adds it to the selection, or removes it |
| Left and Right arrows | Move focus along the row in drawing order, across the midline |
| Up and Down arrows | Move to the nearest slot in the next row (upper, upper deciduous, lower deciduous, lower) |
| Home and End | First and last tooth of the row |
| Escape | Closes the add form; a second Escape clears the selection |
| Mark missing, Mark normal | Add that item to each selected tooth that has no findings. Disabled, with a hint, when a selected tooth already has findings |
| Add finding | Opens the form for all selected teeth |
| Form: finding | A grouped choice: Normal, Missing; Fracture; Periodontal stage, Mobility, Furcation, Probing depth, Gingival recession; Tooth resorption stage, Resorption type; Persistent deciduous, Unerupted, Retained root; Caries, Wear, Discoloured, Malpositioned; Extraction, Crown amputation; Other |
| Form: dependent controls | Only what the finding uses (from `chart.json`): fracture type (seven types and "Not said"); stage 0-4; mobility 0-3; furcation 1-3; millimetres (number, step 0.5); resorption stage 1-5, with 4a, 4b, 4c when 4; type 1-3; extraction technique (closed, closed sectioned, open, "Not said") and "Planned, not done yet"; wear (abrasion, attrition, "Not said"); malposition version; Other needs text |
| Form: Add | Adds one item per selected tooth. `text` is the finding's words in the interface language; `evidence` is empty |
| Row: Change | Opens the form filled in; Save replaces that item |
| Row: Remove | Removes the item (`aria-label`: "Remove {finding} from {tooth}") |
| Row: Play | Plays the cited audio (rows from the recording only) |
| Deciduous teeth switch | Shows or hides the deciduous rows |
| Dog or cat choice | Shown only when the chart has no dentition. Save is disabled until chosen. Choosing Cat with dog-only teeth charted shows "{teeth} aren't in a cat's mouth. Remove them or choose Dog." |
| Save | Sends the whole array (and the dentition, if chosen) through `editNote`. One new run |
| Cancel | Discards every change |

Panel text: "108, right maxillary fourth premolar" for one tooth; "3 teeth selected: 101, 102, 103" for several.

Tests:
- Click 309, Add finding, Mobility, 2, Add: the saved array gains `{tooth: "309", finding: "mobility", grade: 2, detail: null, mm: null, planned: false, evidence: []}`.
- Ctrl-click 101, 102, 103, Mark missing: three items.
- Change a model item's fracture type: the item is replaced; others are unchanged in the payload.
- Remove an item: it's gone from the payload.
- The finding form offers only valid controls (no millimetres for mobility).
- Keyboard only: focus the chart, arrows to 208, Space, arrows to 209, Space, Mark missing, Save.
- No dentition: Save disabled until Dog or Cat; choosing Cat with 105 charted shows the message.
- Cancel sends nothing.

### Step 12: wiring into Review (UNIT, DEMO)

- `NoteView.tsx`: a `teeth` branch in `content()` that renders `ToothChart`. Unknown kinds render "This heading can't be shown in this version of OpenBark." instead of falling through to medications.
- Clicking a tooth in an unsigned draft calls `onEdit(heading, { tooth })`; `onEdit` gains that optional second argument, and `renderEdit` passes the tooth on to `ChartEditor` as its first selection. Signed: a click only selects and highlights the tooth's list lines.
- `flagsFor` gains the soft messages for `tooth_flags`:
  - name conflict: "Said as {number} and as {name} ({other}). Check which tooth before signing."
  - cue missing: "The cited line doesn't mention this finding. Check before signing."
  - shape: "This finding's details don't fit together. Check before signing."
  - duplicate: "This finding is listed twice."
- `Review.tsx`: pass the dentition through to `editNote` from the chart editor.

Tests: `NoteView.test.tsx` renders a dental run with a chart; an unknown kind shows the notice; a name conflict shows the message on the 108 line.

### Step 13: the PDF (UNIT, GOLDEN)

`engine/os-engine/src/pdf.rs`:
- `Op::Path { cmds, stroke: Option<grey>, fill: Option<grey>, width, dash }` with `PathCmd::{Move, Line, Curve, Close}`. Written as `q … m … l … c … h … S/f Q`, with `[2 1.5] 0 d` for dashes. Circles as four Bézier curves.
- A parser for the shape strings in `chart.json` (absolute `M`, `L`, `C`, `Z`), scaled and flipped for PDF's upward y axis.
- `content()` takes the heading's dentition. For a chart:
  - Empty: "Not discussed", no diagram.
  - No dentition: the list, then "Dentition not recorded.", no diagram.
  - Otherwise: the diagram across the content width (about 22 pt per slot on Letter, 21 pt on A4), greys only: uncharted outline grey 0.6 at 0.5 pt, charted outline black at 0.9 pt, marks black at 0.9 pt, planned marks dashed. Tooth numbers Helvetica 6.5 pt grey 0.35. "Patient's right" and "Patient's left" captions.
  - Then the per-tooth list in words with codes, then the legend. No citations.
  - `l.ensure(diagram height + 3 lines)` keeps the diagram with the start of its list.
- `Labels` gains a `dental: &'static DentalWords` for each of the five PDF languages: finding words, detail words, tooth-name parts, captions and legend lines. AVDC codes are not translated.

Tests (GOLDEN):
- A chart run's PDF contains path operators (` m `, ` c `, ` S`), "Dental chart", "T/FX/CCF", "Missing" and "Blank: not charted in this recording".
- An empty chart prints "Not discussed" and no chart path operators.
- No dentition prints "Dentition not recorded." and no diagram.
- Letter and A4 both keep the diagram inside the margins.
- A long chart (60 findings) flows onto a second page with the diagram on the first.
- Every PDF language has every dental word (no empty strings).
- The "every built-in template renders a PDF" test passes.

### Step 14: translations (UNIT)

App keys (English text is the key; add to `packages/i18n/i18n-{de,es,fr,pt}.json`, then run `scripts/i18n-build.py`):

| Group | Keys |
|---|---|
| Builder | "Tooth chart"; "Teeth by Triadan number, for dogs and cats. Filled from what was said; teeth not mentioned stay blank."; "A template can have one tooth chart." |
| Headings (via `builtinText.ts`) | "Dental chart"; "Whole-mouth findings" |
| Chart | "Patient's right"; "Patient's left"; "Deciduous teeth"; "Blank: not charted in this recording. It doesn't mean normal."; "Right and left are the patient's."; "Dentition not recorded. Edit the chart to choose dog or cat."; "Not charted" |
| Tooth names | "Right maxillary {tooth}"; "Left maxillary {tooth}"; "Right mandibular {tooth}"; "Left mandibular {tooth}"; "deciduous {tooth}"; "first incisor"; "second incisor"; "third incisor"; "canine"; "first premolar"; "second premolar"; "third premolar"; "fourth premolar"; "first molar"; "second molar"; "third molar"; "{tooth}, {name}: {findings}"; "{tooth}, {name}: not charted" |
| Findings | "Normal"; "Missing"; "Fracture"; "Enamel infraction"; "Enamel fracture"; "Uncomplicated crown fracture"; "Complicated crown fracture"; "Uncomplicated crown-root fracture"; "Complicated crown-root fracture"; "Root fracture"; "Periodontal disease stage {n}"; "Mobility {n}"; "Furcation {n}"; "Probing depth {mm} mm"; "Gingival recession"; "Gingival recession {mm} mm"; "Tooth resorption stage {stage}"; "Resorption type {n}"; "Persistent deciduous tooth"; "Unerupted"; "Retained root"; "Caries"; "Wear"; "Abrasion"; "Attrition"; "Discoloured"; "Malpositioned"; "Buccoversion"; "Distoversion"; "Labioversion"; "Linguoversion"; "Mesioversion"; "Palatoversion"; "Extraction"; "Closed extraction"; "Closed extraction, sectioned"; "Open extraction"; "Planned: {finding}"; "Crown amputation"; "Other" |
| Legend | "PD0-PD4: periodontal disease stage (AVDC)"; "M0-M3: tooth mobility index (AVDC). Grades as said."; "F1-F3: furcation index (AVDC)"; "TR1-TR5: tooth resorption stage (AVDC). T1-T3: resorption type."; "Probing depth is in millimetres." |
| Editor | "Add finding"; "Mark missing"; "Mark normal"; "Change"; "Remove {finding} from {tooth}"; "Not said"; "Planned, not done yet"; "What was found"; "Type"; "Stage"; "Grade"; "Millimetres"; "Technique"; "Version"; "{n} teeth selected: {teeth}"; "Dog or cat?"; "Choose dog or cat before saving."; "{teeth} aren't in a cat's mouth. Remove them or choose Dog."; "Only for teeth with no findings yet." |
| Flags | the four messages in step 12; "This heading can't be shown in this version of OpenBark." |

- Reuse existing keys where they exist ("Dog", "Cat", "Save", "Cancel", "Not discussed", "Play the supporting audio").
- Tooth-name parts compose without gender problems in the shipped languages (French nouns here are all feminine; Spanish, Portuguese and German all masculine). The whole phrases still go to a native-speaking vet for review.
- PDF words live in `pdf.rs` (step 13).
- Faithfulness name tables for French, Spanish, German and Portuguese: a follow-up inside this plan, after the English eval passes. Each gets the step 3 tables in its language and a native-speaker check.

Tests: the existing i18n tests (placeholders kept, every language has a catalog, every built-in heading has text); a new test that every key used in `ToothChart.tsx`, `ChartEditor.tsx` and `dental.ts` exists in each catalog.

### Step 15: eval fixtures and scoring (EVAL)

`engine/os-cli/src/eval.rs`: `Expected` gains:
- `chart`: expected items (`tooth`, `finding`, optional `detail`, `grade`, `mm`, `planned`). Each is one fact. An item matches when tooth and finding match and every given field matches.
- `chart_flags`: expected tooth flags (`tooth`, `kind`).

The existing `must_mention` checks keep working, because the Markdown lists the chart in words ("6 mm" appears as "Probing depth 6 mm"). Check this on the `dental` fixture before changing any of its expectations.

Scores:
- chart facts found;
- invented teeth: charted teeth not in any expected item (must be 0);
- invented normals: `normal` items not expected (must be 0);
- items without evidence (must be 0);
- expected flags present.

Fixtures (synthetic, written for the test suite; Kokoro voices; audio from `cargo run -p fixture-tts -- --kokoro <dir>`):

**`dental` (existing, unchanged audio): the name-conflict test.** Expected chart: 108 fracture `complicated_crown`; 108 extraction `open`, planned false (planned then done, charted once); 209 missing; 309 periodontal_stage 3; 309 probing_depth 6; 309 mobility 2; 309 extraction `closed`; 404 normal. Expected flag: 108 `name_conflict`. Not allowed: resorption on any tooth; a periodontal stage on any tooth other than 309.

**`dental-chart` (new, canine, Labrador "Juno", 9 years).** Lines:

| Speaker | Line |
|---|---|
| dvm | This is Juno, a nine year old spayed Labrador, in for her dental under anaesthesia. |
| tech | She weighs thirty-one kilos. Premed was methadone and dexmedetomidine, propofol to induce, and she's on isoflurane. |
| dvm | Charting now. One oh one through one oh three are missing. |
| dvm | The upper right canine is normal. |
| dvm | Two oh eight has a slab fracture with pulp exposure. |
| dvm | Two oh four has an uncomplicated crown fracture. |
| dvm | Three oh four is fractured as well. I can't tell how deep yet. |
| dvm | Three oh nine has stage three periodontal disease, a seven millimetre pocket, grade two mobility and a class two furcation. |
| dvm | All the lower incisors are mobile, grade three. |
| dvm | Overall I'd call her stage two across the mouth, with heavy calculus. |
| dvm | Radiographs show no tooth resorption. |
| dvm | Extracting two oh eight surgically with a flap. Three oh nine is a simple extraction. |
| dvm | The lower incisors come out closed, no sectioning. |
| dvm | Three oh four may need to come out. We'll radiograph it again at the recheck. |
| tech | Scaling and polishing done. |
| dvm | She goes home on carprofen, seventy-five milligrams by mouth twice a day for five days. Soft food for two weeks. |
| dvm | Recheck in ten days. |

Expected chart (26 facts): 101, 102, 103 missing (range); 104 normal (name only); 208 fracture `complicated_crown`; 208 extraction `open`; 204 fracture `uncomplicated_crown`; 304 fracture, detail null; 304 extraction, detail null, planned true; 309 periodontal_stage 3, probing_depth 7, mobility 2, furcation 2, extraction `closed`; 301, 302, 303, 401, 402, 403 mobility 3 and extraction `closed` (group). Also: weight 31, carprofen 75. Not allowed: any other tooth; any resorption; any periodontal stage except 309's; any normal except 104.

**`dental-feline` (new, cat "Pepper", 12 years).** Lines:

| Speaker | Line |
|---|---|
| dvm | This is Pepper, a twelve year old neutered male domestic shorthair, here for his dental. |
| tech | He's four point eight kilos. Buprenorphine and alfaxalone, and he's on isoflurane. |
| dvm | Left caudal mandibular block, point two mils of bupivacaine. |
| dvm | Three oh seven has tooth resorption, stage four b. |
| dvm | On the radiograph, three oh seven is type two, replacement resorption. |
| dvm | Four oh eight has a stage two resorptive lesion, type one. |
| dvm | Two oh six is missing, and the upper left canine looks healthy. |
| dvm | One oh nine has two millimetres of gingival recession. |
| dvm | No other resorptive lesions on the full-mouth films. |
| dvm | Three oh seven gets a crown amputation, since it's type two. Four oh eight is extracted surgically. |
| dvm | Buprenorphine at home, point zero two milligrams per kilo in the mouth every eight hours for three days. |
| dvm | Recheck in two weeks. |

Expected chart (9 facts): 307 resorption 4 `crown_worse`; 307 resorption_type 2 (cites the radiograph line); 307 crown_amputation; 408 resorption 2; 408 resorption_type 1; 408 extraction `open`; 206 missing; 204 normal (name only); 109 gingival_recession 2 mm. Not allowed: any other tooth; resorption on any other tooth.

**`dental-young-dog` (new, Yorkshire terrier "Coco", 7 months).** Lines:

| Speaker | Line |
|---|---|
| dvm | Coco is a seven month old female Yorkshire terrier, in for her spay, and I'm checking her teeth. |
| dvm | She has persistent deciduous upper canines, five oh four and six oh four, beside the adult canines. |
| dvm | The adult upper canines, one oh four and two oh four, are fully erupted and look normal. |
| dvm | The baby lower left canine is still there too. |
| dvm | Extracting five oh four, six oh four and seven oh four, closed, no sectioning. |
| dvm | Three oh four hasn't erupted yet. We'll radiograph it at the recheck. |
| dvm | Recheck in four weeks. |

Expected chart (9 facts): 504, 604 persistent_deciduous; 704 persistent_deciduous (deciduous name only); 504, 604, 704 extraction `closed`; 104, 204 normal; 304 unerupted. Not allowed: any other tooth.

Targets, recorded in `docs/eval/M2.md`:
- At least 90% of chart facts across the four fixtures on Gemma 4 E4B, and record 12B too.
- 0 invented teeth, 0 invented normals, 0 items without evidence, 0 hard fails on the clean fixtures.
- The 108 name conflict flagged on `dental`.
- Draft time with and without the chart, and completion tokens used (so the 8192 budget is justified by numbers).
- Try the property order with `text` after `finding` once, and keep the better one (as in `docs/eval/heading-order.md`).

### Step 16: end-to-end on the demo server (DEMO)

Setup: a scratch folder with the four scratch variables set; the CUDA build of the CLI copied to its own path; `openbark serve --port 4321`. Seed visits by running `openbark run` on the fixture audio of `dental-chart`, `dental-feline` and `dental-young-dog` into the scratch sessions folder. Never touch 4317 or its data.

New script `apps/desktop/e2e/dental-chart.mjs` (Playwright, as the other e2e scripts; `OPENBARK_URL=http://localhost:4321`):

1. Open Juno's visit. Set species Dog. Choose Dental procedure. Draft.
2. The chart shows marks on exactly 101, 102, 103, 104, 204, 208, 301, 302, 303, 304, 309, 401, 402, 403. Every other tooth has no mark. The legend has the "Blank" line.
3. Hover 208: the transcript lines that say "two oh eight" are lit.
4. Click 304: the editor opens with 304 selected. Change the planned extraction to an open extraction, not planned. Save. The 304 line shows "Edited by".
5. Ctrl-click 105 and 106, Mark missing, Save. Two new vet-entered items; the draft is still signable.
6. Keyboard only: Tab to the chart, arrow to 110, then Right to 109, Enter opens the editor, Escape, Cancel.
7. Sign. Clicking a tooth now only highlights its lines; there is no Edit.
8. Export the PDF. Its bytes contain "Dental chart", "T/FX/CCF", "Blank: not charted" and path operators.
9. Open Pepper's visit with species Cat and draft: 30 permanent teeth are drawn, no 105.
10. Open Coco's visit with species Dog and draft: the deciduous rows are shown, with 504, 604 and 704 marked.
11. A visit drafted with no species: "Dentition not recorded"; Edit asks "Dog or cat?"; Save stays disabled until one is chosen.
12. At 390 px wide: two blocks, and `scrollWidth` is not wider than the window.
13. Dark mode: an uncharted tooth's stroke equals the computed `--ob-text-3`.
14. No page errors or console errors in any run.

### Step 17: screenshots (DEMO)

Add a dental section to `apps/desktop/e2e/screenshots.mjs` (output in `target/screenshots`):

| Shot | Variants |
|---|---|
| Juno's draft: chart, list and legend | desktop light, desktop dark |
| Chart editor with 309 selected and the add form open | desktop light, desktop dark |
| Signed note | desktop light |
| Pepper's chart (cat) | desktop light |
| Coco's chart (deciduous rows) | desktop light |
| Juno's chart at phone width | mobile light, mobile dark |
| PDF page with the chart | page 1 rendered to PNG in WSL with `pdftoppm -png -r 110` |

Check every frame before committing: synthetic patients only, no real names, no user names or local paths.

### Step 18: docs and release notes

- `docs/templates.md`: a "Tooth chart" section: what it holds, blank means not charted, how to edit, the dog-or-cat question, codes and legend.
- `docs/plans/M2.md`: item 7 status, with the eval numbers.
- `docs/eval/M2.md`: the step 15 results.
- `CHANGELOG.md`: the dental chart, the corrected tooth example, dental template 0.2.0.
- `website/content/features.json`: change the chart claim from planned to available only after release, worded from what shipped ("filled from what was said; teeth not mentioned stay blank").
- ADR-0021 status to accepted when the owner approves.
- No change to `docs/network.md` or `docs/versions.md` (no network calls, no new dependencies).

## Risks

| Risk | Watch for | Mitigation |
|---|---|---|
| The model fills in teeth that weren't said | Invented teeth in the eval | Grammar enum, chart rules, the hard tooth check, and the eval gate at 0 |
| The model puts whole-mouth statements on teeth | A periodontal stage on many teeth after "stage two across the mouth" | Rule 6, the "not allowed" checks in the fixtures |
| Speech recognition mishears tooth numbers ("one oh nine" for "one oh eight") | Disputed numbers on teeth | The second-recogniser check applies to tooth numbers |
| Long charts run out of tokens | `hit_max_tokens` on big mouths | 8192-token budget for chart templates, 96-item cap, measured in the eval |
| Tooth names in other languages hard-fail | Unsupported teeth in non-English consults | The vet's edit unblocks signing; language name tables follow, with native-speaker review |
| The grammar converter rejects part of the schema | Grammar errors on load | The `os-llm` conversion test in step 2 |
| Mobility codes read as molars, "PD" read as pocket depth | Confusion in the record | Codes always printed with their word; pocket depth never abbreviated; the legend names each index |
