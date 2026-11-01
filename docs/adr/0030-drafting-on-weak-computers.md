# ADR-0030: Drafting on weak computers: the note model by memory, no integrated graphics, Stop and bounds

- Status: accepted (2026-10-28, OpenBark 0.2.3); amended 2026-11-01 (OpenBark 0.2.5): the drafting budget grows with the visit (decision 5 and the amendment below)
- Amended by: [ADR-0034](0034-setup-walkthrough.md) (decision 1: the recommendation also weighs where notes are written and the processor's cores)
- Date: 2026-10-28
- Deciders: product owner, engineering
- Amends: the computers' default note model (Gemma 4 E4B for every computer, `os_engine::models::DEFAULT_LLM`); ADR-0019's open question on integrated graphics; ADR-0014 decision 1 (thread counts) for the note model only

## Context

A clinic ran OpenBark on a small Windows laptop. A 30 to 60 minute visit took more than ten minutes to draft, and drafting the client summary stopped at "Writing note… 87%" on its last heading, "What happens next", and never finished or showed an error. Reading the code found:

- Every computer got Gemma 4 E4B (5.15 GB), even where the app's own fit check (`os_models::hub::fit`) said it won't fit. Gemma 4 E2B (3.35 GB) was never suggested.
- The Windows and Linux installers build llama.cpp with Vulkan and put every layer on the first graphics device, which on a laptop is often integrated graphics sharing the processor's memory. That path was never tested for drafting (ADR-0019 is still proposed).
- llama.cpp ran with its default of four threads whatever the processor, and its threads spin while they wait.
- A draft was one generation with no way to stop it but quitting, no timeout, no limit on list lengths or string lengths, and the same 4,096-token budget for a client summary as for a full note. A small model stuck repeating a line would write until the budget ran out, at a few tokens a second.
- The board showed nothing while the model read a long transcript (minutes on a weak processor), then estimated progress by headings, so the whole last heading read 87%.
- The desktop app kept no log, so there was nothing for the clinic to send.

## Decision

1. **The note model by memory.** A computer starts with Gemma 4 E4B unless the fit check says it won't fit, then Gemma 4 E2B (`os_app::policy::default_note_model`). "Tight" still gets E4B: most 16 GB laptops report a little under 16 GB and land there, and E4B runs on them. An install that already has a built-in note model is never switched or sent to download the other one; Models and Record suggest switching, with the download size and what E2B does less well. In the phone model eval (docs/eval/phone-models.md) E2B found 90% of the facts against E4B's 95%, and wrote two numbers wrong across 23 visits where E4B wrote none; it was about 1.6 times as fast on the runner's processor.
2. **No integrated graphics by default.** With only integrated graphics under Vulkan, the note model runs on the processor (`gpu_layers = 0`, `os_app::policy::gpu_layers`), and the fit check counts the computer's memory. A dedicated card is used as before (llama.cpp itself skips integrated graphics when there is one), and Apple's Metal is unchanged. `OPENBARK_GPU_LAYERS=all` (or a number of layers) overrides it.
3. **Threads from the cores.** One thread per physical core when the processor has hyper-threading, else one fewer than the cores (at least one), for reading the prompt and for writing (`os_app::policy::llm_threads`). `OPENBARK_LLM_THREADS` overrides it.
4. **Stop, progress and a stuck notice.** A draft carries a control (`os_llm::Control`): llama.cpp reads the prompt in 512-token batches, reports each one and checks Stop between them, checks Stop at every token, and reports a writing heartbeat about once a second. Stop ends the draft, saves nothing and says "Drafting stopped. Nothing was saved." The board shows how much of the transcript has been read, with the time left at the speed measured in this draft (no estimate before there is one), then tokens and tokens per second. Only while writing, a minute without a token is called out as maybe stuck, with Stop beside it.
5. **Bounded drafts.** The drafting grammar caps every list and free string (`maxItems`, `maxLength`; `os_engine::template::bounded_for_drafting`), set from the longest measured drafts scaled to a 60-minute visit, with the source of each number beside it. The saved schema is unchanged, so the vet's own edits aren't limited. Client documents draft with 2,048 tokens (`CLIENT_MAX_TOKENS`); since 0.2.5 that is the budget of a short visit, and a longer visit gets more (see the amendment). A draft that starts repeating itself stops early with a plain message, and a draft cut off at its budget says so; neither is ever saved.
6. **A log and Copy diagnostics.** The desktop app logs to `<data>/OpenBark/logs/openbark.log` (2 MB, one older file kept), panics included. Only counts, timings, stages, devices, model ids and error kinds are logged, never names, transcripts, prompts, notes or tokens, and visit folder names (which carry the patient's name) are cut back to their date. Settings, About, **Copy diagnostics** puts the version, the computer, the note model, the last draft's timings and the end of the log on the clipboard.

## Amendment (2026-11-01, OpenBark 0.2.5): the budget grows with the visit

The long-visit CI job (16 minutes, eight synthetic consults back to back: 161 transcript lines, about 13,500 characters of speech, a 7,000-token prompt; Gemma 4 E2B on two threads) wrote SOAP notes of 2,841 to 4,063 tokens across runs of 0.2.3 to main, and twice ran past the 4,096-token budget; its client summaries ran to 1,201 to 1,749 tokens, and four times past 2,048. The changes between those versions made no measurable difference; this is run-to-run variance near a fixed limit. A cut-off client summary was no runaway (31 distinct statements, stopped in its last heading): eight consults back to back really are a lot to tell. A draft that reaches its budget is cut off and nothing is saved, so the fixed budgets already failed a 16-minute visit some of the time, and clinic visits run 30 to 60 minutes. The model writes pretty-printed JSON (about 40% of the characters are line breaks and indents); the grammar leaves that alone, since a single-space rule made Gemma write numbers wrong (`os-llm/src/llama.rs`).

1. **A budget from the transcript** (`os_engine::note::budget_for`). A note gets one token for every two characters of speech and a client document one for every three: about 0.3 and up to 0.17 tokens per character were measured. The old budgets stay as floors (4,096 for a note, 2,048 for a client document, 8,192 with a tooth chart), so visits under about 8,200 characters for a note and 6,100 for a client document (some ten and seven minutes of steady talk) draft as before. The CI visit gets 6,762 and 4,508, which puts the longest drafts measured (4,096 and 2,048, both cut off) at 61% and 45%. The ceilings are 12,288 for a note and 6,144 for a client document, reached at about 24,600 and 18,400 characters (some 30 and 22 minutes): a draft that runs on without repeating itself (the repeat guard stops one that does) still stops, after about 19 minutes at the runner's 11 tokens a second. Re-prompting a heading gets the same budget, since the heading may be most of the note. Line labelling for multi-pet visits and a template from an example keep their own budgets.
2. **Trimmed to the context.** The prompt and the budget share the context (32,768 tokens, `max_context`), and a 60-minute visit's prompt alone takes about 19,000 to 26,000. Both backends refuse a request that doesn't fit before reading the prompt, with the prompt's length; the draft is then asked again with the room that is left, as long as that is at least the floor (`generate_guarded`). With less room than the floor it fails as too long, as such a visit did before. The draft's stats are the second request's, so a trimmed draft cut off at its budget says so.
3. **A budget given by hand is kept.** `openbark note --max-tokens` uses the number as given, with the old chart and client document rules, and is never trimmed; without it the CLI and the eval draft with the app's budget.
4. **The CI job fails when the note or the client summary isn't saved** (it only warned before), and its summary shows each draft's budget.

## Consequences

- New computers with less than about 13 GB of memory and no dedicated graphics card start with E2B: faster, and more numbers to check. The suggestion on Models and Record says so.
- Laptops with integrated graphics draft on the processor. Some integrated GPUs may have been faster; ADR-0019's measurements decide whether that changes, and the override is there meanwhile.
- A note of a very long visit that needs more than the list caps loses the rest of that heading; the caps are several times the longest measured headings, and the long-visit CI job (`.github/workflows/drafting.yml`) watches them.
- The Whisper second-opinion pass finishes before a stopped recording is saved, so it never overlaps drafting.
- Since 0.2.5, a long visit's context is sized for its larger budget (prompt plus budget, at most 32,768 tokens), so drafting a long visit takes more memory than before. Visits too long for the context even at the floor still fail with the overflow error; raising `max_context` is a separate decision.

## Alternatives considered

| Option | Why not |
|---|---|
| Switch existing installs to E2B automatically | It would change the notes a clinic gets and start a 3.4 GB download without asking |
| Qwen3 4B Instruct as the small computer default (the 8 and 12 GB iPhone default) | Fewer wrong numbers in the phone eval, but slower on the processor (231 s against 146 s per note), and speed is the problem on these computers. Left as a choice for the owner |
| A repetition penalty in the sampler | Changes every note's wording and can push the model off legitimate repeats (evidence ids, units); detection stops only real loops |
| A fixed timeout for every draft | A long visit on a slow processor legitimately takes many minutes to read; Stop, progress and the stuck notice while writing give the person the decision |
