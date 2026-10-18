# Drafting in the template's heading order (2026-10-18)

## What changed

The note model writes JSON constrained by a schema generated from the template, and llama.cpp's grammar makes it write the properties in the schema's order. `serde_json` sorted object keys alphabetically, so every draft was written Assessment, Objective, Patient, Plan, Subjective (and, inside sections, alphabetically too: Differentials before Problems). The model reached its assessment before it had written the history or the exam.

Turning on `serde_json`'s `preserve_order` feature keeps the template's order: Patient, Subjective, Objective, Assessment, Plan. A test (`schema_keeps_the_templates_heading_order` in `engine/os-engine/src/template.rs`) keeps it that way. The drafting board in Review ticks headings off in this order, so it now matches what's being written.

## Measurement

`openbark eval` on all 13 fixtures with cached transcripts, Gemma 4 E4B (Q4_0) on an RTX 4080, the same build except for the one feature. Each draft is scored for facts captured, invented numbers, statements whose wording isn't supported by the transcript lines they cite ("word flags"), and citations to the wrong lines.

| Fixture | Facts, alphabetical | Facts, template order | Word flags | Citation errors |
|---|---|---|---|---|
| canine-wellness | 9/10 | 9/10 | 10 → 4 | 0 → 0 |
| dental | 10/10 | 10/10 | 0 → 0 | 0 → 0 |
| dermatology | 8/9 | 8/9 | 1 → 0 | 1 → 0 |
| emergency | 13/13 | 13/13 | 16 → 1 | 5 → 0 |
| equine | 11/11 | 11/11 | 1 → 0 | 0 → 0 |
| euthanasia | 6/7 | 5/7 | 4 → 0 | 0 → 0 |
| exotics | 10/10 | 9/10 | 1 → 1 | 0 → 0 |
| feline-vomiting | 17/19 | 16/19 | 5 → 0 | 2 → 1 |
| multipet | 21/23 | 23/23 | 3 → 8 | 0 → 0 |
| oncology | 10/11 | 9/11 | 0 → 6 | 0 → 0 |
| production-animal | 9/11 | 9/11 | 2 → 0 | 2 → 1 |
| smoke | 5/5 | 4/5 | 1 → 1 | 0 → 0 |
| surgery | 14/16 | 15/16 | 1 → 2 | 2 → 1 |
| **Total** | **143/155 (92%)** | **141/155 (91%)** | **45 → 23** | **12 → 3** |

Invented numbers: 0 in both.

## Reading it

- **Recall is level.** Template order missed 6 facts that alphabetical order caught (a time of death, a declined test, two doses, a hydration estimate, a diagnosis word) and caught 4 it had missed (two multi-pet mentions, a dose, a suture). Single facts move with small wording changes, so a two-fact difference is within that noise.
- **Grounding is much better.** Unsupported wording halved (45 to 23) and wrong citations fell from 12 to 3. A draft written history first, exam second and assessment last cites the transcript more accurately. That matters most here: the evidence link is how a vet checks a draft before signing.
- **Decision:** keep template order. Two fixtures (multipet, oncology) gained word flags and are worth a look when the prompts are next tuned.

Reports: `order-ab/alpha.json` and `order-ab/ordered.json` from the run (not committed; rerun with `openbark eval --json`).
