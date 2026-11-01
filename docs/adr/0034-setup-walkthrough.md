# ADR-0034: A setup walkthrough that recommends the note model by speed, and measures it

- Status: proposed
- Date: 2026-10-31
- Deciders: product owner, engineering
- Amends: [ADR-0030](0030-drafting-on-weak-computers.md) decision 1 (the note model by memory only). Keeps ADR-0030's rule that an existing install is never switched or sent to download on its own.

## Context

- On first run, `SetupPanel` downloaded the "missing" default models with no explanation of what they are or why that note model.
- `os_app::policy::default_note_model` chose Gemma 4 E4B or E2B by memory only (`os_models::hub::fit`). A 16 GB laptop with no graphics card and four cores therefore got E4B, the slower model, although it writes notes on the processor.
- A clinic on a weak laptop waited more than 10 minutes per note, and a French-speaking clinic also found drafting slow.

What has been measured, and what hasn't:

| Measurement | Source | Result |
|---|---|---|
| E2B, long visit, 2 processor threads | `.github/workflows/drafting.yml`, run 40869023832 | SOAP note: prompt 7,024 tokens, wrote 3,503 tokens at 10.8 tokens/s, 537 s in all (about 9 minutes). The client summary took 381 s. The visit was 16.1 minutes of continuous synthetic speech: eight consults back to back. |
| E2B against E4B, same runner | `docs/eval/phone-models.md`, run 40196217458 (4 vCPU, no GPU) | E2B 7.8 tokens/s and 146 s per note; E4B 5.0 tokens/s and 256 s per note. That makes E2B 1.56 times as fast by writing speed and 1.75 times by time per note; ADR-0030 rounds this to "about 1.6". Facts found: 90% for E2B, 95% for E4B. Wrong numbers across 23 visits: 2 for E2B, 0 for E4B. |
| More than 2 to 4 processor cores | none | No CI runner has 8 cores. Nothing has been measured on a dedicated graphics card or Apple silicon since 0.2.3. |

## Decision

### 1. The note model by speed, not memory alone (`os_app::policy::recommend_note_model`)

Where the note model writes comes from the devices llama.cpp sees and `gpu_layers` (`policy::drafts_on`):
- **graphics card**: a dedicated GPU through CUDA or Vulkan;
- **Apple silicon**: through Metal;
- **processor**: no GPU, or integrated graphics, which ADR-0030 keeps off. Integrated graphics turned back on with `OPENBARK_GPU_LAYERS` also count as the processor, because their speed was never measured.

| E4B fits (`hub::fit`) | Notes written on | Physical cores | Recommended | Why (`policy::Why`) |
|---|---|---|---|---|
| Won't fit | any | any | Gemma 4 E2B | `memory` |
| Fits or tight | graphics card | any | Gemma 4 E4B | `graphics_card` |
| Fits or tight | Apple silicon | any | Gemma 4 E4B | `apple_silicon` |
| Fits or tight | processor | 8 or more | Gemma 4 E4B | `many_cores` |
| Fits or tight | processor | fewer than 8, or not reported | Gemma 4 E2B | `few_cores` |

What the rule gives for some typical computers:

| Computer | Recommended |
|---|---|
| 8 GB, processor only | E2B (memory) |
| 16 GB, processor only, 4 cores | E2B (few cores) |
| 16 GB, processor only, 8 cores | E4B |
| 16 GB, dedicated 8 GB GPU | E4B |
| Apple silicon, 16 GB | E4B |
| 32 GB, processor only | by cores: 6 gives E2B, 16 gives E4B |

The reasoning behind the threshold (`MIN_CORES_FOR_E4B_ON_PROCESSOR = 8`):
- E2B on two processor threads took about 9 minutes for a long visit's note.
- On the same kind of processor E4B is 1.56 to 1.75 times slower. That makes about 14 to 16 minutes for the same note. This figure is derived from the two measurements above, not measured itself.
- The clinic's complaint was 10 minutes. Giving E4B the same 9 minutes needs at least 1.75 times the throughput. If speed scaled with cores, 4 cores would be enough.
- Writing speed is limited by memory bandwidth as much as by cores, and doesn't scale with cores. So the threshold doubles that margin to 8.
- **Eight is a judgement, not a measurement.** The speed check (decision 4) is what measures each computer, and it says so when the larger model is slow there.
- Revisit the threshold when a 4-core or 8-core machine has been measured with the speed check.

The recommendation is the computer's default note model (`App::default_llm`, `Status.recommendation`). An install that already has a built-in note model keeps it (`App::required_llm`, unchanged from ADR-0030). A note model chosen in setup is never swapped by the app (`Settings.noteModelChosenAt`).

### 2. "Set up OpenBark" on a computer's first run

It replaces the bare `SetupPanel` on computers. The iPhone keeps `SetupPanel` and its own walkthrough.

**When it opens.** It opens after the consent notice and, with a team, after someone has chosen who they are. It opens by itself while models are missing and setup hasn't been finished or skipped. It opens again from Models ("Run setup again"), Help and Settings → About.

**The steps.**

1. **This computer.** Memory, physical cores and threads, graphics, free space in the models folder (`storage()`), and the system, with what this means in plain words.
2. **The note model.**
   - The recommendation, with its reason in one or two sentences tied to this computer.
   - Both choices, with download size, a Recommended badge, and what each does better or worse, all from the eval above.
   - A model that won't fit is labelled.
   - The person can choose the other model.
3. **Speech models.** What each one does, its size, and that they run on this computer. The GPU speech pack (ADR-0014) is offered only where it can work (`gpu_status().supported`: Windows with an NVIDIA card). Its reason is the RTX 4080 measurement already in the GPU card, and NVIDIA's licence must be accepted first.
4. **Download.**
   - Smallest first.
   - One step per model, each with its own clock.
   - The speed measured, the time left, the projected total and the expected finish on the clock.
   - A warning when the free space is less than what's left to download.
5. **Speed check.** Optional and skippable (decision 4).
6. **Recording tips.**
   - A USB or clip-on microphone, or the iPhone close by.
   - "Speakers in the room": 2 for a vet and a client.
   - Review every draft before signing.

"Skip setup" closes it; Record then offers "Set up OpenBark" while models are missing.

**Spoken language.** TODO: there is no spoken-language step yet. The Spoken language setting (PR #44, ADR-0033) wasn't on main when this was written. When it lands, add a step between Speech and Download:
- default Auto;
- suggest the interface language's spoken language when it isn't English;
- reuse that setting.

**Update check.** No update-check question here. If the opt-in update notice (ADR-0035) lands first, an optional step may reuse its setting.

### 3. Existing installs

A computer set up before this version has every model, and setup has never run on it. It gets a one-time, dismissible card on Record: "Check my setup".
- The card runs steps 1, 2 and 5 only.
- It only suggests: the note model changes only when the person picks another one, or presses the speed check's suggestion. Picking a model that isn't downloaded downloads it first, with the same progress board.
- "Not now" hides the card for good.

### 4. The speed check (`App::speed_check`, `POST /api/speed-check`, Tauri `speed_check`)

**What it does.**
1. Loads the chosen note model as a draft would, with the same threads and graphics layers.
2. Reads an invented consult. It is about 500 tokens, has no real animal, owner or clinic, and nothing from any visit is used.
3. Writes a JSON-constrained summary, the way drafts are constrained. It stops 15 s after the first token, or at 256 tokens.

**Speeds.**
- Reading: prompt tokens divided by the time to the first token.
- Writing: llama.cpp's tokens per second.

**Progress.** Progress comes as a draft's: the model loading, the prompt batches read, and the writing heartbeat. The board shows loading, reading and writing, each with its own clock. The time left uses the speed measured in this check, plus the fixed writing budget. Before the model has loaded there is no estimate, and the board says so instead of guessing. "Stop" is the drafting Stop (`cancel_draft`).

**Logging.** Only counts and timings are logged.

**The estimate.**

> note seconds = (7,024 / reading tokens per second + 3,503 / writing tokens per second) × `LONG_CONTEXT_SLOWDOWN`

- The token counts are the CI long visit's (`policy::LONG_VISIT_PROMPT_TOKENS`, `LONG_VISIT_NOTE_TOKENS`).
- The slowdown factor allows for a draft carrying the whole long prompt in its context, unlike the short check. It is set from the drafting job: `drafting.yml` now runs the speed check before the SOAP draft on the same runner and prints the estimate beside the time the note really took.
- **Calibration** (run 41257289162, the same runner, Gemma 4 E2B on two threads):

  | | Reading | Writing | Time |
  |---|---|---|---|
  | Speed check (454 tokens read, 249 written) | 44.9 tokens/s | 18.7 tokens/s | |
  | The SOAP note that followed (6,949 read, 4,063 written) | about 39 tokens/s | 13.1 tokens/s | 489 s |
  | That note at the check's speeds | | | 372 s |

  The note took 1.31 times as long as the check's speeds predict, mostly because writing slows with the long prompt in context (18.7 against 13.1 tokens/s). `LONG_CONTEXT_SLOWDOWN` is 1.3. This is one measurement on one kind of processor. On a graphics card the slowdown may differ, and the factor should be re-measured as more runs come in.

**How the result is worded.** The walkthrough says "On this computer, a note for a long visit will take about N minutes", then states what that is based on: OpenBark's long test visit, 16 minutes of non-stop talking, with shorter visits taking less.
- It doesn't say "a 30-minute visit". No visit of that length was measured, and how much talking a real 30-minute consult holds varies.
- If the estimate is over 5 minutes and the person chose E4B, it suggests E2B with the reason ("about 1.6 times as fast on the same processor", and 90% against 95% of facts).
- The result is kept on the device (`Settings.speedCheck`). Models (under In use) and Record show it for the note model it measured.

### 5. The iPhone

PhoneWalkthrough gets a third screen, "What this iPhone can do", which says one of three things:
- **8 GB or more**, in a build with llama.cpp: this iPhone can write notes. That needs a note model in memory beside the speech models, so it's offered only from 8 GB.
- **Less than 8 GB:** this iPhone records and transcribes, and the computer writes the note. Every candidate in the phone eval wrote doses that weren't said at that size (`docs/eval/phone-models.md`).
- **8 GB or more, but a build without llama.cpp:** this version doesn't write notes on the iPhone yet.

The iPhone's bundled models are unchanged.

## Consequences

- A new 16 GB laptop with four cores and no graphics card now starts with E2B. Its notes come faster, and it has more numbers to check: 2 wrong numbers against none across the 23 eval visits. The model screen says so before anything downloads.
- Existing installs keep their model. The card and the speed check only suggest.
- The default for CI runners (4 vCPU, processor only) becomes E2B. Workflows that need a particular model already pull it by id.
- The 8-core threshold rests on reasoning, not a measurement. Speed checks from real computers, and a measured 4- or 8-core runner, should confirm it or move it.
- The estimate uses one long visit's token counts. Templates with more headings, client documents and other languages write different amounts.
- e2e scripts seed `setupDoneAt` so that their screens stay as they were. The Design screens workflow photographs every setup step on the desktop, with a real speed check, and also the card.

## Alternatives considered

| Option | Why not |
|---|---|
| Keep memory-only selection and rely on the speed check | Most people skip optional steps, and the default should already suit the computer |
| Pick the model from a speed check before downloading anything | The check needs the model on disk: 3 to 5 GB downloaded just to measure |
| A benchmark table by processor model | Not measured, and it would go out of date; the speed check measures the real computer |
| Switch existing installs to the recommended model | It would change a clinic's notes and start a download without asking (ADR-0030) |
| Say "a 30-minute visit" in the estimate | The measured visit was 16 minutes of continuous speech; the mapping to consult length isn't measured |
| "About twice as fast" for E2B | The eval measured 1.56 to 1.75 times; "about 1.6" is what was measured |
