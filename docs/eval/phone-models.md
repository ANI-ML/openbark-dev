# Note models for the iPhone (phone model package, 2026-10-25)

Which small model writes notes by default on each iPhone memory tier (ADR-0018 section 10). The owner asked for "a package of phone-friendly models (ie nemotron or others)", and later said Nemotron isn't required if another model does better. So every candidate drafts the same consults and the measurements pick the defaults.

## Method

- **Same transcript for every model.** `openbark eval --from-ref` builds each fixture's transcript from its reference (`packages/eval/fixtures/audio/<id>.ref.json`): the scripted lines, speakers numbered in the order they first speak (as the app numbers them after diarization), and the timings the synthetic audio has. No audio and no speech recognition are involved, so only the note model differs. Real transcripts have recognition errors, so these scores are a ceiling for each model, not what a clinic sees.
- **All 23 fixtures**, synthetic consults written for OpenBark's tests (not real patients): the SOAP template for fixtures that name none, each fixture's own template otherwise (dental, surgery, emergency, equine, exotics, dermatology, oncology, euthanasia, production animal). The patient details a script declares go into the prompt, as the app sends them.
- **The app's drafting path**: `os_app::open_llm` with the GGUF file, llama.cpp from the pinned `llama-cpp-2` 0.1.160, output constrained to the template's schema, then the faithfulness pass. The app's sampling: temperature 0.1 with a fixed seed (42), and at most 4,096 new tokens per note. A run is repeatable on the same build and processor; it isn't greedy decoding, and a draft that doesn't finish within 4,096 tokens fails in the app too, so the eval counts every check of that fixture as missed.
- **Checks** (as in [M2](M2.md)): facts found (vitals, medications with their numbers, phrases that must and must not appear); **invented numbers** (numbers in the note that aren't in the transcript: a hard fail); citation errors (evidence that points at the wrong or a missing line); word flags (statements whose words aren't in the lines they cite); tooth-chart recall and precision, invented teeth and wrong values on the dental fixtures; species from a breed; cross-contamination in the multi-pet visit.
- **Runner**: GitHub's `ubuntu-24.04` runners, CPU only, one job per model and quarter of the fixtures (`.github/workflows/phone-models-eval.yml`). Seconds per note are CPU seconds on those runners, **not iPhone timings**: an A17 Pro or A18 with Metal is several times faster than the runner's CPU on these models. Peak memory is the process's maximum resident set (`/usr/bin/time -v`), which counts the memory-mapped weights; it is not iOS's `phys_footprint`, where most mapped weight pages don't count.

## Candidates

Every file is pinned by revision, size and SHA-256 in `packages/eval/phone-models.json` (checked live on 2026-10-25). Only models from the vendor's own repository or a well-known converter (Unsloth). Each must fit the tier it is compared for (`os_models::platform::check_gguf`: the tier's byte budget, parameter cap and a phone quantisation).

| Model | GGUF | Bytes | Parameters | Architecture | Licence | Tiers |
|---|---|---|---|---|---|---|
| Qwen3 1.7B Q4_K_M (current 6 GB default) | `unsloth/Qwen3-1.7B-GGUF` | 1,107,409,472 | 1.72B | qwen3 | Apache-2.0 | 6, 8, 12 GB |
| Granite 4.0 1B Q4_K_M | `ibm-granite/granite-4.0-1b-GGUF` | 1,023,645,440 | 1.63B | granite | Apache-2.0 | 6, 8, 12 GB |
| Gemma 3 1B instruct Q4_K_M | `unsloth/gemma-3-1b-it-GGUF` | 806,058,272 | 1.00B | gemma3 | Gemma Terms of Use (not permissive) | 6, 8, 12 GB |
| Gemma 4 E2B QAT Q4_0 (current 8 and 12 GB default) | `google/gemma-4-E2B-it-qat-q4_0-gguf` | 3,349,516,256 | 2B effective | gemma4 | Apache-2.0 | 8, 12 GB |
| Nemotron 3 Nano 4B Q4_K_M | `nvidia/NVIDIA-Nemotron-3-Nano-4B-GGUF` | 2,837,072,864 | 3.97B | nemotron_h (hybrid Mamba-2 and Transformer) | NVIDIA Nemotron Open Model License | 8, 12 GB |
| Qwen3 4B Instruct 2507 Q4_K_M | `unsloth/Qwen3-4B-Instruct-2507-GGUF` | 2,497,281,120 | 4.02B | qwen3 | Apache-2.0 | 8, 12 GB |
| Phi-4-mini-instruct Q4_K_M | `unsloth/Phi-4-mini-instruct-GGUF` | 2,491,874,272 | 3.84B | phi3 | MIT | 8, 12 GB |
| SmolLM3 3B Q4_K_M | `unsloth/SmolLM3-3B-GGUF` | 1,915,306,528 | 3.08B | smollm3 | Apache-2.0 | 8, 12 GB |
| Granite 4.0 H Micro Q4_K_M | `ibm-granite/granite-4.0-h-micro-GGUF` | 1,942,564,512 | 3.19B | granitehybrid (hybrid Mamba-2 and Transformer) | Apache-2.0 | 8, 12 GB |
| Gemma 4 E4B QAT Q4_0 (computers' default, the reference) | `google/gemma-4-E4B-it-qat-q4_0-gguf` | 5,154,941,280 | 4B effective | gemma4 | Apache-2.0 | 12 GB |

"Qwen3 4B" is the Instruct 2507 build: it doesn't think before answering, which suits output constrained from the first token. The 6 GB tier's budget is 1.3 GB and 2B parameters; the 8 GB tier's 3.5 GB and 4.5B; the 12 GB tier's 5.3 GB and 8.5B.

## How a default is chosen (written before the results)

1. **Zero invented numbers across all fixtures.** Any invented number disqualifies a model as a default, however good its other scores.
2. **It ran every fixture**: the pinned llama.cpp loads it, its chat template renders, and it passes OpenBark's constrained-output check.
3. **Facts found**, as a share of all checks, decides among the rest.
4. Within 3 points of facts: fewer citation errors and word flags, then fewer invented teeth and wrong values on the dental charts, then speed.
5. **Licence.** Apache-2.0 or MIT wins a tie. A model under a licence that isn't permissive (Gemma Terms of Use, the NVIDIA Nemotron Open Model License, which adds an indemnity clause) becomes a default only if it leads the best permissive model by 5 points of facts or more with the same gates passed, and this page says so.
6. It must fit the tier's byte budget and parameter cap.
7. If no candidate in a tier passes the gates, the current default stays and this page says plainly that drafting on that tier doesn't meet the bar yet.

## Results (2026-10-25)

Runs: [phone models eval 40196217458](https://github.com/ANI-ML/openbark-dev/actions/runs/40196217458) for every candidate, and [40200848219](https://github.com/ANI-ML/openbark-dev/actions/runs/40200848219) again for Granite 4.0 1B, Gemma 3 1B, SmolLM3 and Nemotron, whose first run stopped at their first failed draft (the eval now scores a failed draft and goes on). Each model's numbers come from one run only. Runners: GitHub `ubuntu-24.04`, 4 vCPUs (AMD EPYC and Intel Xeon of several generations) and 15 GB of memory, no GPU. Peak RSS includes the memory-mapped weights.

**In short.** Every model, the computers' default included, had at least one number flagged by the check. Three of the flags are gaps in the check on reference transcripts (below). With those set aside, the same way for every model, two models wrote no number that wasn't said: Qwen3 4B Instruct 2507 and Gemma 4 E4B, the computers' default. **Gemma 4 E2B, the 8 and 12 GB default until now, wrote two wrong numbers**: acepromazine 0.2 mg/kg where 0.02 was said (tenfold), and a weight of 32 kg where 30.2 was said.

| Model | Tiers | Size | Licence | Fixtures (drafts failed) | Facts found | Invented numbers (check) | Invented after the check's gaps | Citation errors | Word flags | Chart recall | Chart precision | Invented teeth | s per note (mean / median) | tokens/s | Peak RSS | Runs |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| Qwen3 1.7B Q4_K_M | 6, 8, 12 GB | 1.11 GB | Apache-2.0 | 23 (0) | 206/232 (89%) | 36 | 36 | 38 | 64 | 31% | 55% | 0 | 118 / 86 | 9.3 | 3.0 GB | 4/4 |
| Granite 4.0 1B Q4_K_M | 6, 8, 12 GB | 1.02 GB | Apache-2.0 | 23 (2) | 162/229 (71%) | 12 | 11 | 44 | 166 | 22% | 38% | 0 | 110 / 90 | 9.6 | 2.6 GB | 4/4 |
| Gemma 3 1B instruct Q4_K_M | 6, 8, 12 GB | 0.81 GB | Gemma Terms of Use (not permissive) | 23 (2) | 114/234 (49%) | 36 | 36 | 265 | 388 | 5% | 17% | 2 | 82 / 70 | 14.3 | 1.4 GB | 4/4 |
| Gemma 4 E2B instruct QAT Q4_0 | 8, 12 GB | 3.35 GB | Apache-2.0 | 23 (0) | 208/232 (90%) | 3 | 2 | 3 | 22 | 45% | 85% | 0 | 146 / 114 | 7.8 | 4.7 GB | 4/4 |
| Nemotron 3 Nano 4B Q4_K_M | 8, 12 GB | 2.84 GB | NVIDIA Nemotron Open Model License | 23 (1) | 178/229 (78%) | 16 | 16 | 40 | 26 | 43% | 75% | 0 | 203 / 201 | 4.7 | 3.6 GB | 4/4 |
| Qwen3 4B Instruct 2507 Q4_K_M | 8, 12 GB | 2.50 GB | Apache-2.0 | 23 (0) | 219/232 (94%) | 6 | 0 | 2 | 60 | 85% | 94% | 0 | 231 / 198 | 4.5 | 5.7 GB | 4/4 |
| Phi-4-mini-instruct Q4_K_M | 8, 12 GB | 2.49 GB | MIT | 23 (0) | 208/232 (90%) | 7 | 7 | 25 | 88 | 34% | 57% | 0 | 182 / 150 | 5.1 | 5.1 GB | 4/4 |
| SmolLM3 3B Q4_K_M | 8, 12 GB | 1.92 GB | Apache-2.0 | 23 (2) | 190/232 (82%) | 5 | 4 | 19 | 129 | 24% | 50% | 0 | 198 / 160 | 5.4 | 4.0 GB | 4/4 |
| Granite 4.0 H Micro Q4_K_M | 8, 12 GB | 1.94 GB | Apache-2.0 | 23 (0) | 216/232 (93%) | 10 | 5 | 77 | 170 | 50% | 77% | 0 | 147 / 140 | 6.9 | 3.5 GB | 4/4 |
| Gemma 4 E4B instruct QAT Q4_0 (computers' default, reference) | 12 GB | 5.15 GB | Apache-2.0 | 23 (0) | 220/232 (95%) | 1 | 0 | 1 | 35 | 88% | 99% | 0 | 256 / 223 | 5.0 | 7.8 GB | 4/4 |

Facts found count a failed draft's checks as missed, so a model with failed drafts has a few more checks in total (vitals of a template without that field are otherwise skipped). Chart recall and precision are over the dental fixtures with an expected chart (88 expected findings). Seconds per note are CPU seconds on the runner, several times what an iPhone with Metal takes; compare them only with each other.

Other measures:

| Model | Species read from a breed (right and unflagged) | Tooth findings with a wrong type, grade, mm or plan | Another animal's value in a multi-pet note |
|---|---|---|---|
| Qwen3 1.7B | 0 of 5 | 20 | 0 |
| Granite 4.0 1B | 0 of 5 | 26 | 0 |
| Gemma 3 1B | 1 of 4 | 1 (4 found) | 0 |
| Gemma 4 E2B | 3 of 5 | 6 | 0 |
| Nemotron 3 Nano 4B | 5 of 5 | 7 | 0 |
| Qwen3 4B Instruct 2507 | 0 of 5 | 4 | 0 |
| Phi-4-mini-instruct | 0 of 5 | 19 | 0 |
| SmolLM3 3B | 0 of 5 | 14 | 1 |
| Granite 4.0 H Micro | 0 of 5 | 6 | 0 |
| Gemma 4 E4B (reference) | 5 of 5 | 0 | 0 |

### The check's gaps on reference transcripts

The reference transcripts spell numbers the way the scripts say them, and three ways of saying a number aren't read by the number check yet (`os_engine::numbers::normalize_spoken`), so a correct number in the note is flagged:

1. **"one sixty"** for 160: a vital said the clinical way. The emergency consult says "Heart rate one sixty", and its own expected vitals are `heart_rate: "160"`. Six models were flagged for writing 160.
2. **"two oh PDS"**, written 2-0 or two-0 in the note, flagged as an invented 0 (suture sizes in the surgery consult).
3. **"two and a quarter cups"**, written 2.25 (the wellness consult).

`scripts/ci/phone-eval-summary.py` (`checker_gap`) applies exactly these three patterns to every flagged number of every model; anything else counts as invented. The rule was written **after** the first results, because the gate as first written disqualified every model, the computers' default included. Both counts are in the table, and every flagged number is listed below with its verdict. The faithfulness pass itself is unchanged in this work: reading these forms is a follow-up (the "one sixty" form is ambiguous with times such as "three forty", which the check deliberately reads as two numbers). With real recordings Parakeet usually writes such numbers as digits, which is why the audio eval ([M2](M2.md)) didn't meet them.

### Every flagged number

- Flagged numbers, Qwen3 1.7B Q4_K_M: exotics /vitals/temperature 37.8 (invented); dental-feline-multi-tr /anaesthesia_and_nerve_blocks/0 0.05 (invented); dental-feline-multi-tr /anaesthesia_and_nerve_blocks/1 0.05 (invented); dental-feline-multi-tr /anaesthesia_and_nerve_blocks/2 0.05 (invented); dental-feline-multi-tr /anaesthesia_and_nerve_blocks/3 1.5 (invented); dental-feline-multi-tr /anaesthetic_monitoring/0 30 (invented); dental-feline-multi-tr /anaesthetic_monitoring/1 39.2 (invented); dental-feline-multi-tr /anaesthetic_monitoring/2 100 (invented); smoke /plan/medications/0 1 (invented); speaker-rabbit-history /plan/medications/0 1 (invented); speaker-rabbit-history /plan/medications/1 1 (invented); dental-chart /anaesthesia_and_nerve_blocks/0 1 (invented); dental-chart /anaesthesia_and_nerve_blocks/1 1 (invented); dental-perio-dog /anaesthesia_and_nerve_blocks/1 0.05 (invented); dental-perio-dog /anaesthesia_and_nerve_blocks/3 1.5 (invented); speaker-dental-estimate /plan/medications/0 12 (invented); speaker-dental-estimate /plan/medications/1 12 (invented); dental-extractions /anaesthesia_and_nerve_blocks/0 0.1 (invented); dental-extractions /anaesthesia_and_nerve_blocks/1 0.05 (invented); dental-extractions /anaesthesia_and_nerve_blocks/2 0.1 (invented); dental-extractions /anaesthesia_and_nerve_blocks/3 1.5 (invented); dental-extractions /anaesthetic_monitoring/0 120 (invented); dental-extractions /anaesthetic_monitoring/1 120 (invented); dental-extractions /anaesthetic_monitoring/1 80 (invented); dental-extractions /anaesthetic_monitoring/2 98 (invented); dental-extractions /medications_to_go_home/0 0.1 (invented); dental-extractions /medications_to_go_home/1 0.05 (invented); dental-extractions /medications_to_go_home/2 0.1 (invented); dental-extractions /medications_to_go_home/3 1.5 (invented); dental-young-dog /anaesthesia_and_nerve_blocks/0 1 (invented); dental-young-dog /medications_to_go_home/0 1 (invented); oncology /treatment_given/0 0.07 (invented); speaker-diabetic-callback /objective/vitals/heart_rate 120 (invented); speaker-diabetic-callback /objective/vitals/capillary_refill_time 10 (invented); speaker-diabetic-callback /objective/diagnostic_results/0 300 (invented); speaker-diabetic-callback /objective/diagnostic_results/0 60 (invented)
- Drafts that failed, Granite 4.0 1B Q4_K_M: canine-wellness: model output is not valid JSON: EOF while parsing an object at line 529 column 7 (output hit max_tokens); multipet: model output is not valid JSON: EOF while parsing a string at line 502 column 54 (output hit max_tokens)
- Flagged numbers, Granite 4.0 1B Q4_K_M: emergency /presenting_complaint/3 160 (hundreds); dental-chart /dental_chart/8 301 (invented); dental-chart /dental_chart/9 302 (invented); dental-chart /dental_chart/10 303 (invented); dental-chart /dental_chart/11 401 (invented); dental-chart /dental_chart/12 402 (invented); dental-chart /dental_chart/13 403 (invented); equine /vitals/digital_pulses 1 (invented); surgery /anaesthetic_protocol/1 0.2 (invented); oncology /vitals/body_surface_area 2 (invented); oncology /treatment_options_discussed/0 2 (invented); oncology /treatment_given/0 2 (invented)
- Drafts that failed, Gemma 3 1B instruct Q4_K_M: dental-chart: model output is not valid JSON: EOF while parsing a string at line 286 column 33 (output hit max_tokens); dental-extractions: model output is not valid JSON: EOF while parsing a value at line 181 column 37 (output hit max_tokens)
- Flagged numbers, Gemma 3 1B instruct Q4_K_M: canine-wellness /objective/vitals/capillary_refill_time 3.5 (invented); dental-feline /anaesthesia_and_nerve_blocks/1 0.5 (invented); dental-feline /dental_chart/0 407 (invented); dermatology /medications/0 1 (invented); exotics /vitals/weight 301.1 (invented); exotics /vitals/temperature 32.5 (invented); exotics /vitals/heart_rate 72 (invented); exotics /vitals/respiratory_rate 160 (invented); dental /anaesthetic_monitoring/0 92 (invented); dental /dental_chart/0 407 (invented); smoke /patient/age 3 (invented); smoke /objective/vitals/heart_rate 72 (invented); smoke /plan/medications/0 1 (invented); speaker-rabbit-history /plan/medications/0 1 (invented); speaker-rabbit-history /plan/medications/1 10 (invented); dental-perio-dog /anaesthesia_and_nerve_blocks/1 0.5 (invented); dental-perio-dog /anaesthesia_and_nerve_blocks/2 10 (invented); dental-perio-dog /medications_to_go_home/1 0.5 (invented); equine /treatment_given/0 1 (invented); equine /medications_to_continue/0 1 (invented); multipet Olive: /subjective/history/0 3 (invented); speaker-dental-estimate /objective/physical_exam/0 3 (invented); surgery /pre_anaesthetic_assessment/temperature 38.2 (invented); dental-young-dog /anaesthesia_and_nerve_blocks/0 1 (invented); dental-young-dog /anaesthesia_and_nerve_blocks/1 1 (invented); dental-young-dog /dental_chart/0 3 (invented); dental-young-dog /dental_chart/0 3 (invented); dental-young-dog /dental_chart/1 3 (invented); dental-young-dog /dental_chart/1 3 (invented); dental-young-dog /medications_to_go_home/0 1 (invented); oncology /vitals/weight 3.2 (invented); oncology /vitals/temperature 38.2 (invented); oncology /vitals/heart_rate 72 (invented); oncology /vitals/respiratory_rate 16 (invented); speaker-diabetic-callback /objective/diagnostic_results/0 301.1 (invented); speaker-diabetic-callback /objective/diagnostic_results/2 94 (invented)
- Flagged numbers, Gemma 4 E2B instruct QAT Q4_0: emergency /vitals/heart_rate 160 (hundreds); surgery /anaesthetic_protocol/1 0.2 (invented); oncology /vitals/weight 32 (invented)
- Drafts that failed, Nemotron 3 Nano 4B Q4_K_M: multipet: model output is not valid JSON: EOF while parsing a value at line 96 column 103 (output hit max_tokens)
- Flagged numbers, Nemotron 3 Nano 4B Q4_K_M: production-animal /vitals/temperature 4.1 (invented); speaker-rabbit-history /plan/medications/0 1 (invented); speaker-rabbit-history /plan/medications/0 3 (invented); speaker-dental-estimate /plan/medications/1 0.5 (invented); surgery /anaesthetic_protocol/1 0.2 (invented); dental-extractions /anaesthesia_and_nerve_blocks/0 1 (invented); dental-extractions /anaesthesia_and_nerve_blocks/1 0.05 (invented); dental-extractions /anaesthesia_and_nerve_blocks/2 0.05 (invented); dental-extractions /dental_chart/0 3 (invented); dental-extractions /dental_chart/0 3 (invented); dental-extractions /dental_chart/1 3 (invented); dental-extractions /dental_chart/1 3 (invented); dental-extractions /dental_chart/2 3 (invented); dental-extractions /dental_chart/2 3 (invented); dental-extractions /dental_chart/3 3 (invented); dental-extractions /dental_chart/3 3 (invented)
- Flagged numbers, Qwen3 4B Instruct 2507 Q4_K_M: canine-wellness /plan/client_communication/0 2.25 (quarter); emergency /vitals/heart_rate 160 (hundreds); surgery /surgical_report/1 0 (suture); surgery /surgical_report/2 0 (suture); surgery /surgical_report/3 0 (suture); surgery /surgical_report/4 0 (suture)
- Flagged numbers, Phi-4-mini-instruct Q4_K_M: canine-wellness /subjective/history/4 31.1 (invented); speaker-rabbit-history /objective/vitals/weight 9.1 (invented); speaker-rabbit-history /plan/medications/0 1 (invented); surgery /pre_anaesthetic_assessment/temperature 38 (invented); surgery /anaesthetic_protocol/1 0.2 (invented); dental-extractions /medications_to_go_home/0 1 (invented); oncology /treatment_given/0 0.007 (invented)
- Drafts that failed, SmolLM3 3B Q4_K_M: canine-wellness: model output is not valid JSON: EOF while parsing a string at line 705 column 9 (output hit max_tokens); speaker-rabbit-history: model output is not valid JSON: EOF while parsing a string at line 419 column 10 (output hit max_tokens)
- Flagged numbers, SmolLM3 3B Q4_K_M: emergency /vitals/heart_rate 160 (hundreds); smoke /subjective/history/1 3 (invented); multipet Olive: /assessment/problems/2 100 (invented); multipet Pickles: /objective/physical_exam/2 100 (invented); multipet Pickles: /assessment/problems/2 100 (invented)
- Flagged numbers, Granite 4.0 H Micro Q4_K_M: canine-wellness /plan/procedures_and_treatments/0 2.25 (quarter); emergency /vitals/heart_rate 160 (hundreds); speaker-rabbit-history /plan/medications/0 1 (invented); surgery /surgical_report/0 0 (suture); surgery /surgical_report/0 0 (suture); surgery /surgical_report/0 0 (suture); dental-young-dog /conscious_oral_exam/2 404 (invented); dental-young-dog /dental_radiographs/0 3 (invented); dental-young-dog /treatment_performed/0 5 (invented); dental-young-dog /treatment_performed/0 6 (invented)
- Flagged numbers, Gemma 4 E4B instruct QAT Q4_0 (computers' default, reference): emergency /vitals/heart_rate 160 (hundreds)

## Tier defaults

| Tier | Before | From now | Why |
|---|---|---|---|
| 6 GB (1.3 GB, 2B) | Qwen3 1.7B Q4_K_M | **Qwen3 1.7B Q4_K_M, unchanged** | No candidate passes. Qwen3 1.7B wrote 36 numbers that weren't said, most of them anaesthetic doses in the dental consults; Granite 4.0 1B wrote 11 (charted teeth that weren't mentioned, a tenfold acepromazine dose) and ran past the token limit twice; Gemma 3 1B wrote 36, ran past the limit twice and found half the facts. Rule 7 applies: the default stays, and **drafting on 6 GB iPhones doesn't meet the bar yet**. Recording and transcribing on those phones are unaffected |
| 8 GB (3.5 GB, 4.5B) | Gemma 4 E2B QAT Q4_0 | **Qwen3 4B Instruct 2507 Q4_K_M** | The only candidate with no invented number after the check's gaps, and every draft finished. It found the most facts (219 of 232, 94%), had the fewest citation errors (2) and by far the best tooth charts (85% recall, 94% precision, 4 wrong values). Apache-2.0; 2,497,281,120 bytes and 4.02B parameters, inside the tier's 3.5 GB and 4.5B. Gemma 4 E2B (two wrong numbers), Phi-4-mini (7, including a tenfold acepromazine dose and an invented maropitant dose), Granite 4.0 H Micro (5, including tooth 404 for 304), SmolLM3 (4, plus two failed drafts) and Nemotron 3 Nano 4B (16, including invented anaesthetic doses, plus a failed draft) don't pass |
| 12 GB (5.3 GB, 8.5B) | Gemma 4 E2B QAT Q4_0 | **Qwen3 4B Instruct 2507 Q4_K_M** | As for 8 GB. Gemma 4 E4B, the computers' default, was in the table as the reference for this tier and also passes; by rule 4 it would edge Qwen3 4B (95% against 94% of facts, 36 against 62 citation errors and word flags). It isn't made the default: the task set it as the desktop reference rather than a phone candidate, its 5,154,941,280-byte file leaves the least margin of any default under the tier's 5.3 GB budget, and it needed the increased-memory entitlements to load under LiteRT-LM (ADR-0018 section 10). It stays selectable on 12 GB phones, and the first TestFlight build with drafting is the place to measure it |

What Qwen3 4B Instruct 2507 does less well: it left the species empty or flagged in all five breed-only consults (E4B and Nemotron got all five), and it is the slowest phone candidate on the runner's CPU (231 s per note on average against 146 s for Gemma 4 E2B). Its word flags (60) are higher than Gemma's (22 for E2B, 35 for E4B): statements whose words aren't all in the cited line, shown to the vet as "check" marks in Review.

**Licences.** Both new defaults are Apache-2.0, so rule 5 didn't come into play. Nemotron 3 Nano 4B (NVIDIA Nemotron Open Model License) stays in the catalog for 8 and 12 GB phones as a choice, with NVIDIA's statement and a copy of the licence; Gemma 3 1B (Gemma Terms of Use) is eval-only and not offered.

**Nemotron 3 Nano 4B on the pinned llama.cpp.** `llama-cpp-2` 0.1.160 (llama.cpp d812350) loads the `nemotron_h` GGUF and drafts with it: every fixture ran, and the Linux CI job drafts the smoke consult from the catalog entry (status draft, faithfulness passed, 620 tokens in 169 s on the CPU). No llama.cpp change was needed. Granite 4.0 H Micro (`granitehybrid`), the other hybrid Mamba-2 model, also loaded and ran every fixture.

## Limits of this comparison

- Synthetic consults written for OpenBark's tests, 23 of them; not real patients, and the scripts favour clearly spoken numbers.
- Reference transcripts, not recognised speech: real transcripts have recognition errors, so every score here is a ceiling.
- CPU timings on shared runners of mixed hardware, not iPhone timings, and not comparable across runs.
- One run per model at temperature 0.1 with a fixed seed: a different processor can change a token here and there.
- Memory on the phone isn't measured here. For Qwen3 4B the KV cache is about 144 KiB per token at 16-bit, so 0.9 to 1.3 GB for the fixtures' contexts, on top of the mapped weights; the first TestFlight build with `inference` measures `phys_footprint` (ADR-0018 section 10).

## Reproduce

Push a commit whose message contains `[phone-eval]` (every candidate) or `[phone-eval: id,id]` (some), or run the Phone models eval workflow by hand once it is on `main`. The candidates and their pinned files are in `packages/eval/phone-models.json`; the summary is `python3 scripts/ci/phone-eval-summary.py <folder of the downloaded phone-eval-* artifacts>`. Locally, with a GGUF on disk: `openbark eval --from-ref --llm <file.gguf> --json report.json`.
