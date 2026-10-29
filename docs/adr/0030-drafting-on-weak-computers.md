# ADR-0030: Drafting on weak computers: the note model by memory, no integrated graphics, Stop and bounds

- Status: accepted (2026-10-28, OpenBark 0.2.3)
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
5. **Bounded drafts.** The drafting grammar caps every list and free string (`maxItems`, `maxLength`; `os_engine::template::bounded_for_drafting`), set from the longest measured drafts scaled to a 60-minute visit, with the source of each number beside it. The saved schema is unchanged, so the vet's own edits aren't limited. Client documents draft with 2,048 tokens (`CLIENT_MAX_TOKENS`). A draft that starts repeating itself stops early with a plain message, and a draft cut off at its budget says so; neither is ever saved.
6. **A log and Copy diagnostics.** The desktop app logs to `<data>/OpenBark/logs/openbark.log` (2 MB, one older file kept), panics included. Only counts, timings, stages, devices, model ids and error kinds are logged, never names, transcripts, prompts, notes or tokens, and visit folder names (which carry the patient's name) are cut back to their date. Settings, About, **Copy diagnostics** puts the version, the computer, the note model, the last draft's timings and the end of the log on the clipboard.

## Consequences

- New computers with less than about 13 GB of memory and no dedicated graphics card start with E2B: faster, and more numbers to check. The suggestion on Models and Record says so.
- Laptops with integrated graphics draft on the processor. Some integrated GPUs may have been faster; ADR-0019's measurements decide whether that changes, and the override is there meanwhile.
- A note of a very long visit that needs more than the list caps loses the rest of that heading; the caps are several times the longest measured headings, and the long-visit CI job (`.github/workflows/drafting.yml`) watches them.
- The Whisper second-opinion pass finishes before a stopped recording is saved, so it never overlaps drafting.

## Alternatives considered

| Option | Why not |
|---|---|
| Switch existing installs to E2B automatically | It would change the notes a clinic gets and start a 3.4 GB download without asking |
| Qwen3 4B Instruct as the small computer default (the 8 and 12 GB iPhone default) | Fewer wrong numbers in the phone eval, but slower on the processor (231 s against 146 s per note), and speed is the problem on these computers. Left as a choice for the owner |
| A repetition penalty in the sampler | Changes every note's wording and can push the model off legitimate repeats (evidence ids, units); detection stops only real loops |
| A fixed timeout for every draft | A long visit on a slow processor legitimately takes many minutes to read; Stop, progress and the stuck notice while writing give the person the decision |
