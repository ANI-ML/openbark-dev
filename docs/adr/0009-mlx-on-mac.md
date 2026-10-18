# ADR-0009: MLX models on the Mac: label them now, add a runtime only if it measures faster

- Status: proposed (labelling is built; the runtime needs a decision and Mac hardware)
- Date: 2026-10-18
- Deciders: product owner (asked that the Mac label which Hugging Face models are MLX optimized), engineering

## Context

On Apple silicon, many models on Hugging Face are published as **MLX** builds (Apple's array framework; the `mlx-community` organisation alone converts most popular models). Mac users look for them because MLX is tuned for the unified memory and GPU of M-series chips.

OpenBark's note models run through llama.cpp (ADR-0001), which reads **GGUF** files and already runs on the Mac's GPU through Metal. It can't read MLX weights (safetensors laid out for MLX). So "MLX optimized" is a different file format, not a setting: running one needs a second inference engine.

What OpenBark needs from an engine, beyond speed:
- **Constrained output.** Notes are generated against a JSON Schema built from the template (ADR-0006); llama.cpp enforces it with a grammar, which is part of why drafts are always valid.
- **Local and packaged.** One app, no Python install, nothing leaving the computer.
- **Streaming tokens** for the live draft.

## Decision

1. **Built now: labels.** On a Mac, **Browse Hugging Face** also searches MLX builds and marks each with an **MLX optimized** chip, with a filter to show only those. Opening one explains that it needs the MLX engine, and points to the GGUF build of the same model, which runs on the Mac's GPU today. Windows and Linux never list MLX builds, since they can't run them. (`HubModel.mlx`, `Status.platform`; network row R4.)
2. **Before adding a runtime, measure.** On an M-series Mac, compare llama.cpp (Metal) with MLX for Gemma 4 E4B and 12B on the eval fixtures: time to draft, memory, and output validity. Add an MLX engine only if it's clearly faster for our workload.
3. **If it is, the preferred route is a native binding**, `mlx-rs` (MIT/Apache-2.0, Rust bindings to Apple's `mlx-c`), behind the existing `LlmBackend` trait, so the rest of the app doesn't change. Constrained output would need a schema-to-token mask on top, which is the main cost.

## Consequences

- Mac users can already see which models are MLX optimized, and aren't misled into downloading files the app can't run.
- Adding MLX later is contained to one backend crate plus the catalog (a model's format decides its engine).
- Until then, Macs use GGUF on Metal, which is what the M0 benchmarks plan covered.

## Alternatives considered

| Option | Why not (for now) |
|---|---|
| Run `mlx_lm.server` (MIT, Python) as a local sidecar on 127.0.0.1 | Needs a Python runtime shipped or installed on every Mac. Constrained JSON output would have to be verified separately. Possible as an opt-in for power users later. |
| Show MLX builds on every platform | They can't run on Windows or Linux; listing them there would only confuse. |
| Convert MLX weights to GGUF on the Mac | Loses whatever MLX-specific quantisation made them attractive, and the GGUF build is usually already published. |
| Do nothing | The product owner asked for MLX models to be identified on the Mac. |
