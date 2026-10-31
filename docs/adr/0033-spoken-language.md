# ADR-0033: A spoken-language setting, and measuring recognisers in French first

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

- `Cargo.lock` pins `sherpa-onnx` 1.13.8 and `sherpa-onnx-sys` 1.13.8 (the latter built from `vendor/sherpa-onnx-sys` since #41, with the same bindings: `vendor/sherpa-onnx-sys/src/offline_asr.rs` line 41).
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

### 3. Canary 180M Flash, measured as a candidate

The model is `canary-180m-flash-int8`: English, French, Spanish and German, 207 MB, CC-BY-4.0. The engine runs it like any other recogniser (`AsrModel::Canary`, `Layout::AsrCanary`), told the language.

`models::LOCKED_MAIN` lists the languages for which a candidate becomes the main recogniser. For a language in that list, a new install would download the candidate as one of the models it needs (`required_status_with`), and Settings, Models would show it. If it were missing, Parakeet would be used.

The list is empty (point 4), so Canary stays in the catalogue only for the French eval to measure it (`openbark models pull`, on CI, as `docs/network.md` R1 says). Because it isn't offered, it is not in `NOTICE`; its CC-BY-4.0 credit is in the catalogue entry. The app never shows or downloads it:

- `models::offered` keeps it out of Settings, Models;
- `download_model` refuses it;
- no setting asks for it.

A test checks all three.

### 4. Measure before switching: the bar, set before the results

A candidate becomes the main recogniser for a set language only if, on that language's fixtures, against Parakeet:

- its WER is at least 20% lower (relative), pooled over the fixtures, the mixed consult included;
- it has no more wrong-language words, and strictly fewer when Parakeet has any;
- its real-time factor on two threads is at most 1.5 times Parakeet's, measured in the same job on the same runner.

French is judged on the French fixtures, and English on the English ones. Spanish, German and Portuguese have no fixtures, so they never switch. `packages/eval/score_language.py --baseline parakeet` applies the bar.

**The result** (run 41249174129, 2026-10-31; synthetic standard French, see `docs/eval/french.md`): no candidate passes, for French or for English.

| French fixtures, all six | WER | Wrong-language words | Real-time factor (2 threads) | Against the bar |
|---|---|---|---|---|
| Parakeet (detects the language) | 11.0% | 9 | 0.065 | baseline |
| Canary 180M Flash, told French | 18.5% | 2 | 0.088 (1.34x) | **fails**: WER 69% higher, not 20% lower |
| Whisper large-v3-turbo, told French | 13.4% | 4 | 0.529 (8.08x) | **fails**: WER 22% higher, and 8 times slower |

- **Canary** has fewer wrong-language words than Parakeet. But told French, it turns the English half of the mixed consult into French (56.7% WER on that consult, against Parakeet's 14.3%). It is also worse on four of the five French-only consults.
- **English fixtures:** Canary told English (4.6%) and Whisper told English (4.4%) are only 8% and 13% better than Parakeet (5.0%), short of 20%. Both are also more than 1.5 times slower (1.53x and 6.89x).
- **Notes drafted from each transcript:** facts found were 90% from the script, 82% from Parakeet, 85% from Whisper and 69% from Canary.

**Decision:** `LOCKED_MAIN` stays empty. Parakeet stays the main recogniser for every language. The Spoken language setting ships anyway: it locks Whisper's second listen to the chosen language. Canary is not offered to anyone (point 3).

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

- A French-speaking clinic can stop the second listen from hearing French as English. The main recogniser still detects the language of each sentence. On synthetic French it wrote a handful of English words per consult (9 in 1,459). No locked recogniser we measured did better overall, so a clinic whose transcripts still flip should try a closer microphone, and send us an example if it can.
- **More second listens.** More segments are re-heard in French consults, because French drug talk now counts. A recording takes a little longer to finish on a slow computer.
- **No ADR-0030 splitting with Canary.** Canary reports no token times. If it ever became the main recogniser, an utterance holding two voices would no longer be split where the voice changes, and speaker labels would come from whole utterances, as before ADR-0030. That is another reason it would need to clear the bar clearly.
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
