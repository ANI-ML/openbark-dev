# French eval (synthetic standard French)

**Read this first.** Every number on this page comes from six synthetic consults written in standard French and voiced by synthetic French (France) voices. They are not recordings of people, not real clinic audio, and not Quebec French. Nothing here is a result for Quebec French, for accented speech, or for any real clinic. We make no claim about how well OpenBark handles Quebec French.

## Why

A French-speaking clinic, mixing French and some English, got a transcript that flipped between French and invented English ("Um being", "See that?", "Strong?"). Parakeet TDT 0.6B v3, the main recogniser, detects the language of each chunk itself and can't be told which language to expect. Until this eval, every OpenBark fixture was English, so nobody knew how OpenBark handles French.

## The consults

Six scripts in `packages/eval/fixtures/fr/scripts`, in the same format as the English ones (a fact checklist under `expected:`). They live apart from the English set so the English evals are unchanged.

| Fixture | What it tests |
|---|---|
| `fr-chat-conjonctivite` | A cat with conjunctivitis of the left eye for 5 to 6 days, rinsed with water, a little pus, fluorescein negative; Maxitrol 0.5 cm three times a day for 10 days; gingivitis and Breathalyzer gel |
| `fr-carlin-allergies` | A pug with atopic dermatitis, a yeast otitis of the right ear and fold dermatitis; Apoquel 5.4 mg, Easotic, chlorhexidine wipes; an allergy test put off |
| `fr-vaccination-chiot` | A puppy's vaccination visit: boosters, first leptospirosis injection, rabies next visit, dewormer, Bravecto, microchip read |
| `fr-chien-boiterie` | A lame dog: grade 3/5 left hind lameness, painful stifle, suspected cranial cruciate ligament injury, radiographs, Metacam, strict rest |
| `fr-mixte-gastro` | **French mixed with English**, as in a bilingual clinic: whole English sentences and English drug talk ("Cerenia tablets, sixty milligrams once a day") inside a French consult. Its English lines are tagged and voiced with English voices |
| `fr-dictee-examen` | **The vet dictates the exam aloud, system by system** ("Yeux : hyperhémie conjonctivale à gauche…"), with vitals, a heart murmur, small irregular kidneys and a plan |

Names, patients and numbers are invented.

### Voices and their licences

`tools/fixture-tts` renders the scripts on the CI runner with sherpa-onnx, from the k2-fsa `tts-models` release (each archive checked against its SHA-256 in `.github/workflows/french-eval.yml`). The rendered audio is only used inside the CI job and is never published.

| Voice (`speakers:` name) | Speaker | Licence (dataset) |
|---|---|---|
| `piper:fr_FR-siwis-medium` | One woman (SIWIS French speech synthesis database) | CC BY 4.0 |
| `piper:fr_FR-gilles-low` | One man | CC0 |
| `piper:fr_FR-upmc-medium#1` and `#0` | Pierre (a man) and Jessica (a woman) (UPMC) | CC BY-SA 4.0 |
| Kokoro `af_sarah`, `am_michael` | The English lines of the mixed consult | Apache-2.0 (Kokoro-82M) |

Not used: Kokoro's multilingual model has only one French voice (`ff_siwis`, from the same SIWIS data), and Piper's `fr_FR-tom` voice is AGPLv3. Piper reads text through espeak-ng, so an English drug name inside a French sentence is said with French phonetics, much as a French speaker might say it. Synthetic voices are cleaner and more regular than people, so real speech will do worse than these numbers.

## How it runs

`.github/workflows/french-eval.yml`, by hand from the Actions tab or on a push whose head commit message contains `[fr-eval]`. Linux CPU runners.

The recognisers compared: Parakeet TDT 0.6B v3 (int8, detecting the language itself, as today), Canary 180M Flash (int8) told the language (`canary-fr`, and `canary-en` on the English fixtures) and Whisper large-v3-turbo (int8) told the language (`whisper-fr`, `whisper-en`).

1. **Recognisers alone** (`scripts/ci/french-eval.sh`): each recogniser transcribes every fixture with `openbark transcribe` (voice detection and one recogniser: no speaker labels, no second listen) on two threads (`OPENBARK_ASR_THREADS=2`), one after another on the same runner so their speeds compare. Six English fixtures run the same way, as a regression check. Scored by `packages/eval/score_language.py`.
2. **Notes** (`openbark eval`): each transcript is drafted as the app would with a French interface: the default template (SOAP), Gemma 4 E4B on the processor, notes written in French (`--note-language fr`, Settings, Language, Write notes in), and Whisper's second listen told to expect French (`--spoken-language fr`, what OpenBark 0.2.4 does with a French interface). The `reference` arm drafts from the script itself, with no recognition errors, as a control. Summarised by `scripts/ci/french-eval-summary.py`.

### What is measured

- **WER**: word errors (substitutions, deletions, insertions) over the script's words, pooled over fixtures. Both sides are normalised the same way: lower case, no punctuation, hyphens and apostrophes as spaces, "œ" as "oe", numbers written with digits said as words in the language around them ("38,6" is "trente huit virgule six"), "mg", "ml", "kg", "cm" and "%" spelled out. Accents are kept, so a missing accent is an error.
- **Wrong-language share**: the share of the recogniser's words that belong to the other language (English in a French consult) and are not in the script. A word counts as English when it is in the English word list and not in the French one. The lists come from the scripts themselves (the English fixtures, and the French fixtures' French and English lines) plus a few hundred common words of each language. It is a heuristic, applied the same way to every recogniser.
- **Real-time factor**: recognition time over audio time, for the recogniser alone, on two threads of the runner's processor.
- **Facts found**: the share of each script's checklist found in the drafted note (`openbark eval`: vitals, medications with their numbers, phrases that must and must not appear). French phrases match without regard to accents ("oeil gauche" finds "œil gauche"), and numbers match whether said or written ("trois semaines" finds "3 semaines").
- **Invented numbers**: numbers in the note that aren't in the transcript (the faithfulness check). **Failed drafts**: drafts that didn't produce a valid note, scored as every fact missed.

### French support added for this eval

- The number check reads French numbers said in words ("trente-huit virgule six" is 38.6, "cent quatre-vingts" is 180) and the decimal comma ("0,5 cm" is 0.5), so a French note that writes digits isn't flagged as inventing them. English is unchanged: words that are numbers only in French start one, "1,250" still groups thousands, and the existing tests pass unchanged.
- The word check (`align.rs`) folds accents, drops French elisions ("l'œil" is "œil"), ignores French stopwords and counts "pas", "aucun", "ni", "sans", "jamais" and "rien" as negations. French stopwords that are also English words ("pour", "plus", "son", "car", "comment", "par") are left out, so English statements keep them; the English tests pass unchanged. The app still runs the word check only on notes written in English (`note.rs`).
- `openbark eval` takes `--stt`, `--spoken-language` and `--note-language`; `openbark transcribe` takes `--language`.

## The bar, set before any result

Written before the first run, and checked by `score_language.py --baseline parakeet`. A recogniser becomes OpenBark's main recogniser when a spoken language is set only if, on that language's fixtures, against Parakeet TDT 0.6B v3:

1. its WER is at least **20% lower** (relative), pooled over all the fixtures of that language, the mixed consult included;
2. it has **no more wrong-language words**, and strictly fewer when Parakeet has any;
3. its real-time factor on two threads is at most **1.5 times** Parakeet's, measured in the same job on the same runner.

The bar is per language: French is judged on the French fixtures, English on the English ones. Spanish, German and Portuguese have no fixtures, so no recogniser replaces Parakeet for them. Whisper large-v3-turbo is expected to fail the speed bar, which matters: the clinic already found it slow.

## Results

**Synthetic standard French, not Quebec French and not real clinics.** These numbers come from run 41248209807 of `.github/workflows/french-eval.yml` (2026-10-31), on a GitHub Linux runner. That run compared Parakeet with Whisper. Canary 180M Flash and the ship decision are in ADR-0033 and the next version of this page. Runs vary: a second run on the same fixtures put Parakeet's French WER at 11.0% (this run: 9.5%) and its facts found at 82% (this run: 75%). Read differences of a few points as noise.

### Recognisers, French fixtures

| Recogniser | Fixtures | Script words | WER | Wrong-language words | Wrong-language share | Real-time factor (2 threads) |
|---|---|---|---|---|---|---|
| Parakeet TDT 0.6B v3 (detects the language) | 6 | 1,459 | 9.5% | 8 | 0.6% | 0.035 |
| Whisper large-v3-turbo, told French | 6 | 1,459 | 14.6% | 9 | 0.6% | 0.297 |

| Subset | Parakeet WER | Whisper (French) WER |
|---|---|---|
| French only (5 fixtures, 1,249 words) | 9.0% | 11.4% |
| French mixed with English (1 fixture, 210 words) | 12.9% | 33.3% |

| Fixture | Parakeet: WER, wrong-language words | Whisper (French): WER, wrong-language words |
|---|---|---|
| fr-carlin-allergies | 7.0%, 0 | 7.0%, 1 |
| fr-chat-conjonctivite | 10.4%, 2 | 14.5%, 0 |
| fr-chien-boiterie | 9.1%, 0 | 7.5%, 0 |
| fr-dictee-examen | 8.9%, 0 | 20.5%, 4 |
| fr-mixte-gastro | 12.9%, 4 | 33.3%, 4 |
| fr-vaccination-chiot | 9.3%, 2 | 9.7%, 0 |

Parakeet's wrong-language words were "max", "definitely", "thirty", "nine", "point", "one", "kilo" and "yeah". So Parakeet does produce English words in synthetic French, though rarely. Told French, Whisper turns much of the mixed consult's English into French, which is why its WER on that consult is high.

**Against the bar:** Whisper fails all three tests. Its WER is 53% higher, not 20% lower. It has 9 wrong-language words against Parakeet's 8. It is 8.5 times slower.

### Recognisers, English fixtures (regression check)

| Recogniser | Fixtures | Script words | WER | Wrong-language words | Real-time factor (2 threads) |
|---|---|---|---|---|---|
| Parakeet TDT 0.6B v3 | 6 | 2,119 | 5.6% | 1 | 0.034 |
| Whisper large-v3-turbo, told English | 6 | 2,119 | 4.7% | 0 | 0.265 |

**Against the bar:** Whisper fails for English. Its WER is 16% lower, short of 20%, and it is 7.8 times slower.

### Notes drafted in French

SOAP template, Gemma 4 E4B on the processor, notes in French, with the second listen told to expect French. "Reference" drafts from the script itself, with no recognition errors.

| Transcript | Drafts | Facts found | Invented numbers | Failed drafts |
|---|---|---|---|---|
| Reference (the script) | 6 | 64/71 (90%) | 1 | 0 |
| Parakeet | 6 | 53/71 (75%) | 0 | 0 |
| Whisper (French) | 6 | 60/71 (85%) | 0 | 0 |

| Fixture | Reference | Parakeet | Whisper (French) |
|---|---|---|---|
| fr-carlin-allergies | 11/13 | 8/13 | 11/13 |
| fr-chat-conjonctivite | 9/10 | 7/10 | 7/10 |
| fr-chien-boiterie | 11/12 | 10/12 | 11/12 |
| fr-dictee-examen | 14/15 (1 invented) | 13/15 | 13/15 |
| fr-mixte-gastro | 10/10 | 7/10 | 8/10 |
| fr-vaccination-chiot | 9/11 | 8/11 | 10/11 |

- **Misses with the script itself** are mostly a number dropped from a medication line, such as the duration of Apoquel, Maxitrol or Metacam. With a recogniser, terms it misheard go missing too: "fluorescéine", "Breathalyzer", "hyperhémie", "Milbemax".
- **The one invented number** is most likely the dictated heart rate of "cent quatre-vingts" (180) was written as 190, from the script itself. That is a note-model error.
- **No draft failed.**
