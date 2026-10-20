# ADR-0026: Phone on speaker

- Status: accepted (built 2026-10-20, for 0.1.3)
- Date: 2026-10-20
- Deciders: product owner (reported: "When I use my phone on speaker, the transcription is maybe less sensitive? It picks up my voice no problem but has difficulty transcribing when the owner is talking on the phone"; then "ensure phone speech is properly documented and sensitive enough"), engineering
- Relations: keeps ADR-0001 (every audio model through sherpa-onnx; no new dependency). Complements ADR-0023 (calls through the clinic's own Twilio account record each side on its own channel and remain the better way to chart a phone conversation). Measurements: [docs/eval/M2.md](../eval/M2.md), "Speakerphone".

## Context

A vet records a consult on the normal Record screen while a client talks on a phone on speaker nearby. The client's voice reaches the computer's microphone three ways at once: quiet (15 to 25 dB below the vet), narrowband (a phone line carries 300 to 3400 Hz, companded, often at 8 kHz) and reverberant (the loudspeaker is across the room). The owner's recording on a Mac (0.1.1, native capture) showed every line as "Speaker 1", and the client's turns as clipped fragments with the first words missing.

Four synthetic speakerphone consults (`packages/eval/fixtures/scripts/speaker-*.yaml`, voiced by Kokoro, the client rendered through a simulated phone line, loudspeaker and room by `tools/fixture-tts`, 15, 18, 22 and 25 dB below the vet) reproduce both symptoms. Scored per speaker (`packages/eval/score_speakers.py`) on the 0.1.2 engine:

- **Voice detection drops the client.** 7% of the client's speech time was inside any segment; 43 of 43 client turns were dropped and 2% of the client's words reached the transcript. The vet: 96% of words, no turn dropped.
- **Lowering the detector's threshold doesn't fix it.** At 0.35 instead of 0.5, coverage rose to 16% and 95% of client words were still lost. Silero needs the voice at a reasonable absolute level, not only a reasonable probability threshold.
- **Every segment carried one speaker label.** The few client fragments heard were under a second long, and the live tracker never starts a new speaker from an utterance shorter than a second.

## Decision

1. **A "Phone on speaker" option** on the Record screen (also used for imports), off by default and remembered per computer. In-room consults keep the 0.1.2 behaviour exactly. Tauri, the HTTP API (`speakerphone` on `/api/live`, `/api/record/start` and `/api/sessions`) and the CLI (`--speakerphone` on `record` and `transcribe`) carry it to `LiveOptions::speakerphone`.
2. **Voice detection hears a levelled copy** (`os_asr::level::Leveller`): a streaming gain control that lifts quiet speech towards -20 dBFS (at most +30 dB, fast attack, about 100 ms release) but never lifts the background noise above -50 dBFS, so pauses stay pauses. Utterances still carry the original samples; the saved recording is untouched. Sweeping the noise ceiling: -65 dBFS heard 13% of the client, -60 33%, -55 65%, -50 96%.
3. **Cut where a louder voice starts or stops.** Lifting the quiet voice also lifts its echo, which bridged the pause before the vet answered: client and vet turns merged into one utterance (0.55 segments per vet line, labels 17% right). A second detector runs on the original audio, and a levelled utterance is cut where a voice at least 8 dB louder than the speech before it starts, or stops with quieter speech after it (`split_at_level_steps`). Where the original detector also merged the turns (a client only 15 dB down is loud enough for it), a pause between two stretches whose levels differ by 8 dB or more is a cut too (`split_at_level_dips`).
4. **Each utterance reaches the models at a normal level** (`os_asr::level::normalise`: one gain per utterance, the louder half of its frames to -20 dBFS, never lowered, soft-limited).
5. **Phone-band speech goes to the second recogniser.** An utterance with almost nothing above 4 kHz (`os_diar::high_band_db` below -24 dB) is transcribed by Whisper large-v3-turbo, the number cross-check recogniser, when it's installed; it heard the phone voice better than Parakeet (client words 82% against 69% with everything else equal). Those utterances skip the cross-check, which would compare Whisper with itself. Without the second recogniser, Parakeet transcribes everything.
6. **The live speaker tracker keeps a high-band level per speaker** and never merges a phone voice with a room voice (`band_apart`: one side below -24 dB and at least 10 dB apart, 16 dB for utterances under a second). This is always on: voices in an ordinary consult are all full band (synthetic fixtures: -20 to 0 dB per line; phone voices -40 to -30 dB), so it never splits them.

## Consequences

- On the speakerphone fixtures, with the option on (live path, as recordings and imports run): client words captured 85.0% (was 2.2%), the vet's 95.7% (was 96.4%); client turns dropped 1 of 43 (was 43); client words under their own speaker label 99% (was 0%), the vet's 100%. Details and the remaining errors are in docs/eval/M2.md.
- With the option on, the 19 ordinary fixtures transcribe as before within noise (vet words captured 93.8% against 94.1%, client 97.7% against 97.8%, labels unchanged), so leaving it on by mistake costs little on synthetic audio. In a real clinic, lifting quiet sound can also pick up voices from the corridor, which is why it is an option.
- With the second recogniser, phone sentences take longer to transcribe; on this computer the transcript was still ready within 1.3 to 4.4 s of the end of a 2.5-minute import.
- The batch `openbark transcribe --diarize` path (CLI only; the app's imports use the live path) gets the levelled detection and the level cuts but keeps Parakeet and the full-recording diarizer, which still labels the speakerphone fixtures as one speaker (client words 72.3%).
- Synthetic audio isn't a real phone: Kokoro voices are clean and distinct, the room is a simple reverberator. The thresholds (-50 dBFS ceiling, 8 dB steps, -24 dB phone band) need checking against real speakerphone recordings when the owner can share measurements.

## Alternatives considered

| Option | Why not |
|---|---|
| Lower the detector threshold | 0.35 heard 16% of the client; the problem is level, not threshold |
| Levelling always on | Equal on synthetic consults, but lifts distant voices in a real clinic; kept behind the option |
| Cut only at the original detector's boundaries | Missed merges where the client is loud enough for the original detector too (15 dB fixture); the pause-and-level rule covers them |
| Whisper for every utterance in speakerphone mode | Its vet lines were worse than Parakeet's (one vet turn lost on the dental fixture), and it's slower |
| Padding around utterances (0.15 s before, 0.1 s after) | No change in client capture (37.8% against 38.1% WER at the time); left at zero |
| Browser getUserMedia changes | Echo cancellation and noise suppression are already off in web mode; automatic gain is left on (unmeasured either way) |
