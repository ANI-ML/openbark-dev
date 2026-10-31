# ADR-0032: A spoken-language setting, and measuring recognisers in French first

- Status: accepted
- Date: 2026-10-31
- Deciders: engineering, after a French-speaking clinic's report (0.2.4)
- Relations: amends ADR-0031 rule 8 (the second listen follows the app's language). It adds a setting for that language. It uses the second opinion of M1 item 4 and the hint policy of ADR-0031: hints stay hints, and the transcript is never rewritten. The French eval is `docs/eval/french.md`.

## Context

A French-speaking clinic mixes French with some English. Its transcript flipped between French and invented English: "Um being", "See that?", "Strong?". Four things were behind it:

- **Parakeet can't be told the language.** Parakeet TDT 0.6B v3, the main recogniser, detects the language of each chunk itself, and greedy decoding has no language option.
- **Whisper only followed the interface.** `models::set_speech_language` told Whisper, the second listen, which language to expect, but that language came from the interface. A clinic using the app in English while speaking French had Whisper locked to English.
- **No French fixtures.** Every eval fixture was English, so nobody knew how OpenBark handles French. We made no claims.
- **No French drug cues.** ADR-0031's drug-context cues were English only ("twice a day", "mg", "give"), so French talk about medication didn't trigger drug hints, or a second listen.

### Does sherpa-onnx 1.13.8 support Canary with a source language?

Yes. This is the version OpenBark pins.

- `Cargo.lock` pins `sherpa-onnx` 1.13.8 and `sherpa-onnx-sys` 1.13.8.
- The Rust bindings define `OfflineCanaryModelConfig { encoder, decoder, src_lang, tgt_lang, use_pnc }` (`sherpa-onnx-1.13.8/src/offline_asr.rs`, line 121). `OfflineModelConfig` has a `canary` field (line 463). The raw struct with the same fields is in `sherpa-onnx-sys-1.13.8/src/offline_asr.rs`, line 41.
- The v1.13.8 C API header used by the prebuilt library has `SherpaOnnxOfflineCanaryModelConfig` with `src_lang` and `tgt_lang` (`sherpa-onnx/c-api/c-api.h`, lines 884 to 896), and a `canary` member in `SherpaOnnxOfflineModelConfig` (lines 1104 to 1105).
- The upstream C example `c-api-examples/nemo-canary-c-api.c` uses exactly the export added here: `sherpa-onnx-nemo-canary-180m-flash-en-es-de-fr-int8`.
- Upstream's implementation (`offline-recognizer-canary-impl.h`) decodes with `<|notimestamp|>`, so Canary returns no token times. When `src_lang` is empty or unknown, it falls back to English. Canary can't detect a language itself.

No sherpa-onnx bump was needed. Setting `tgt_lang` equal to `src_lang` makes Canary transcribe rather than translate.

## Decision

### 1. A French eval, before any change of recogniser

`docs/eval/french.md` describes it. It uses six synthetic consults in standard French, voiced by Piper's French (France) voices:

- a cat with conjunctivitis;
- a pug with allergies;
- a vaccination visit;
- a lame dog;
- one consult that mixes French and English, with English drug names;
- one where the vet dictates the exam system by system.

The eval reports the following, in `.github/workflows/french-eval.yml`:

- WER per recogniser;
- the share of words heard in the wrong language;
- the real-time factor on two threads;
- facts found in notes drafted in French.

The fixtures are synthetic standard French, and every page that shows the numbers says so. **OpenBark makes no claim about Quebec French.**

### 2. Settings, Recording, Spoken language

The choices are **Auto** (the default), English, Français, Español, Deutsch and Português: the languages Whisper is offered in among the app's.

- **Separate from the interface language.** A clinic can use a French interface and speak a mix.
- **Per device.** It is kept with the other settings on the device and sent to the engine. On a computer that is the `set_spoken_language` command; in web mode it is `POST /api/spoken-language`.
- **Hidden on the iPhone.** The iPhone has no second listen, and its bundled models don't change.
- **Auto is 0.2.4.** Parakeet detects the language of each sentence, and Whisper expects the app's language. A device that never touches the setting behaves exactly as before.
- **A language that is set** is what every recogniser that takes a language is told. That means Whisper's second listen, and Canary when Canary is the main recogniser (point 4). It replaces the interface language as the source of Whisper's language. Parakeet takes no language.

### 3. Canary 180M Flash as an optional recogniser

The model is `canary-180m-flash-int8`: English, French, Spanish and German, 207 MB, CC-BY-4.0. It is in the catalogue with its checksums, in `docs/network.md` (R1), `NOTICE` and `docs/versions.md`.

The engine runs it like any other recogniser (`AsrModel::Canary`, `Layout::AsrCanary`), told the language. `models::LOCKED_MAIN` lists the languages for which it, or any other candidate, becomes the main recogniser. Settings, Models shows Canary only when some language uses it, so nobody downloads a model that does nothing. When a language in that list is set, a new install downloads it as one of the models it needs (`required_status_with`). If Canary is missing, Parakeet is used.

### 4. Measure before switching: the bar, set before the results

A candidate becomes the main recogniser for a set language only if, on that language's fixtures, against Parakeet:

- its WER is at least 20% lower (relative), pooled over the fixtures, the mixed consult included;
- it has no more wrong-language words, and strictly fewer when Parakeet has any;
- its real-time factor on two threads is at most 1.5 times Parakeet's, measured in the same job on the same runner.

French is judged on the French fixtures, and English on the English ones. Spanish, German and Portuguese have no fixtures, so they never switch. `packages/eval/score_language.py --baseline parakeet` applies the bar.

**The result** is in `docs/eval/french.md` and in this ADR's amendment below. While `LOCKED_MAIN` is empty, Parakeet stays the main recogniser for every language, and the setting locks only Whisper's second listen.

### 5. Drug talk in French

`vocab.rs` adds French cues:

- dose and form words: "mg", "comprimé", "gélule", "gouttes", "onguent", "pommade", "appliquer", "donnez", "médicament", "ordonnance";
- frequencies: "par jour", "chaque soir";
- phrases: "matin et soir", "pendant N jours".

The words are listed with and without accents. A drug name heard after a French elided article ("l'apple") is found as "apple". French drug talk therefore gets drug hints, and a second listen, as English does. Hints stay hints; the transcript is never rewritten.

### 6. What this doesn't do

- **No Quebec claims.** It claims nothing for Quebec French, accented speech or real clinic audio.
- **Alignment stays English-only.** The word-level faithfulness check runs only on notes written in English (`note.rs`). `align.rs` now understands French (accents, elisions, stopwords, negations), but turning it on for French notes needs its own evidence.
- **No language for Parakeet.** It can't be given one. Auto stays the right choice for consultations that really mix languages.

## Consequences

- A French-speaking clinic can stop the second listen from hearing French as English. Choosing Français is the fix for transcripts that flip into invented English *in the second listen*. If the main recogniser itself flips, the eval says whether a locked recogniser would help, and by how much.
- **More second listens.** More segments are re-heard in French consults, because French drug talk now counts. A recording takes a little longer to finish on a slow computer.
- **No ADR-0030 splitting with Canary.** If Canary ever becomes the main recogniser, it reports no token times, so an utterance holding two voices is no longer split where the voice changes. Speaker labels then come from whole utterances, as before ADR-0030.
- **The decision is one line.** It lives in `LOCKED_MAIN`. Changing it needs a new eval run that passes the same bar, written down here.
- **The eval costs runner hours,** so it runs only by hand or with `[fr-eval]` in a commit message.

## Alternatives considered

| Option | Why not |
|---|---|
| Whisper large-v3-turbo as the main recogniser when a language is set | Expected to fail the speed bar on the processor, and the clinic already found it slow. Measured anyway in `docs/eval/french.md` |
| Tie the spoken language to the interface language | A clinic can work in a French interface and speak a mix, or the other way round. 0.2.4 did this, and it is the bug's second half |
| Bump sherpa-onnx for Canary | Not needed: 1.13.8 already has `src_lang` (evidence above) |
| Canary 1B Flash, or Parakeet TDT 0.6B v3 with a language prompt | 1B is five times the download and slower on the processor. Parakeet has no language input |
| A language-identification model in front of Parakeet | Parakeet still can't be told the result, so it would only re-route chunks. A locked recogniser is the simpler test of the same idea |
| Rewrite English words in a French transcript | The transcript keeps what was heard (ADR-0031 rule 1) |
