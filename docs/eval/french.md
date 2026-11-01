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

**Synthetic standard French: not Quebec French, and not real clinics.**

These numbers come from run 41249174129 of `.github/workflows/french-eval.yml` (2026-10-31, a GitHub Linux runner). That run compared Parakeet, Canary and Whisper on the same runner. The ship decision is in ADR-0033.

**Runs vary.** An earlier run on the same fixtures (41248209807, without Canary) found different numbers for Parakeet:

| Parakeet, French fixtures | Earlier run (41248209807) | This run (41249174129) |
|---|---|---|
| WER | 9.5% | 11.0% |
| Facts found | 75% | 82% |
| Real-time factor | 0.035 | 0.065 (a slower runner) |

So read differences of a few points as noise. The comparisons below are within one run, on one runner.

### Recognisers, French fixtures

| Recogniser | Fixtures | Script words | WER | Wrong-language words | Wrong-language share | Real-time factor (2 threads) |
|---|---|---|---|---|---|---|
| Parakeet TDT 0.6B v3 (detects the language) | 6 | 1,459 | 11.0% | 9 | 0.6% | 0.065 |
| Canary 180M Flash, told French | 6 | 1,459 | 18.5% | 2 | 0.1% | 0.088 |
| Whisper large-v3-turbo, told French | 6 | 1,459 | 13.4% | 4 | 0.3% | 0.529 |

WER by subset:

| Subset | Parakeet | Canary (French) | Whisper (French) |
|---|---|---|---|
| French only (5 fixtures, 1,249 words) | 10.4% | 12.1% | 10.6% |
| French mixed with English (1 fixture, 210 words) | 14.3% | 56.7% | 30.0% |

WER and wrong-language words per fixture:

| Fixture | Parakeet | Canary (French) | Whisper (French) |
|---|---|---|---|
| fr-carlin-allergies | 4.9%, 0 | 7.0%, 0 | 7.0%, 0 |
| fr-chat-conjonctivite | 9.1%, 3 | 13.8%, 1 | 11.4%, 0 |
| fr-chien-boiterie | 9.1%, 0 | 11.6%, 1 | 10.0%, 0 |
| fr-dictee-examen | 19.5%, 0 | 14.7%, 0 | 16.3%, 0 |
| fr-mixte-gastro | 14.3%, 5 | 56.7%, 0 | 30.0%, 4 |
| fr-vaccination-chiot | 12.7%, 1 | 14.3%, 0 | 9.7%, 0 |

The wrong-language words each recogniser wrote:
- **Parakeet:** "doctor", "for", "that", "thirty", "nine", "point", "one", "but", "yeah". So Parakeet does produce English words in synthetic French, though rarely.
- **Canary:** "doctor", "p".
- **Whisper:** "thirty", "nine", "point", "one".

Told French, Canary and Whisper both turn much of the mixed consult's English into French. That costs them WER there, but doesn't count as wrong-language words.

### The bar, French fixtures (all six)

| Recogniser | WER at least 20% lower | Fewer wrong-language words | Real-time factor at most 1.5x | Passes |
|---|---|---|---|---|
| Canary (French) | no: 69% higher | yes: 2 against 9 | yes: 1.34x | **no** |
| Whisper (French) | no: 22% higher | yes: 4 against 9 | no: 8.08x | **no** |

### Recognisers, English fixtures (regression check)

| Recogniser | Fixtures | Script words | WER | Wrong-language words | Real-time factor (2 threads) |
|---|---|---|---|---|---|
| Parakeet TDT 0.6B v3 | 6 | 2,119 | 5.0% | 1 | 0.066 |
| Canary 180M Flash, told English | 6 | 2,119 | 4.6% | 0 | 0.102 |
| Whisper large-v3-turbo, told English | 6 | 2,119 | 4.4% | 0 | 0.457 |

| Fixture | Parakeet | Canary (English) | Whisper (English) |
|---|---|---|---|
| canine-wellness | 0.5% | 1.6% | 1.6% |
| dermatology | 6.7% | 3.6% | 2.1% |
| emergency | 12.4% | 11.9% | 10.3% |
| feline-vomiting | 3.3% | 2.6% | 2.8% |
| oncology | 7.2% | 8.4% | 7.2% |
| surgery | 13.6% | 12.6% | 13.1% |

**Against the bar, English:** both fail.
- **Canary** is 8% better, not 20%, and 1.53 times slower.
- **Whisper** is 13% better and 6.89 times slower.

### Notes drafted in French

SOAP template, Gemma 4 E4B on the processor, notes in French, the second listen told to expect French. "Reference" drafts from the script itself, with no recognition errors.

| Transcript | Drafts | Facts found | Invented numbers | Failed drafts |
|---|---|---|---|---|
| Reference (the script) | 6 | 64/71 (90%) | 1 | 0 |
| Parakeet | 6 | 58/71 (82%) | 0 | 0 |
| Whisper (French) | 6 | 60/71 (85%) | 0 | 0 |
| Canary (French) | 6 | 49/71 (69%) | 2 | 0 |

| Subset | Reference | Parakeet | Whisper (French) | Canary (French) |
|---|---|---|---|---|
| French only (5 fixtures) | 54/61 (89%) | 51/61 (84%) | 52/61 (85%) | 44/61 (72%) |
| French mixed with English | 10/10 | 7/10 | 8/10 | 5/10 |

| Fixture | Reference | Parakeet | Whisper (French) | Canary (French) |
|---|---|---|---|---|
| fr-carlin-allergies | 11/13 | 12/13 | 12/13 | 9/13 |
| fr-chat-conjonctivite | 9/10 | 7/10 | 8/10 | 8/10 (1 invented) |
| fr-chien-boiterie | 11/12 | 11/12 | 12/12 | 9/12 |
| fr-dictee-examen | 14/15 (1 invented) | 13/15 | 13/15 | 12/15 (1 invented) |
| fr-mixte-gastro | 10/10 | 7/10 | 8/10 | 5/10 |
| fr-vaccination-chiot | 9/11 | 8/11 | 7/11 | 6/11 |

**What the missed facts were:**
- **From the script itself:** mostly a number dropped from a medication line, such as the durations of Apoquel, Maxitrol and Metacam.
- **The script's one invented number:** the dictated heart rate of "cent quatre-vingts" (180) was written as 190. That is a note-model error.
- **From recognisers:** misheard terms go missing too ("fluorescéine", "Breathalyzer", "hyperhémie", "Milbemax").
- **From Canary:** it also lost drug names and doses ("Apoquel 5.4", "Cerenia", "60 mg").

**Outcome.** No recogniser passes the bar, for French or English. Parakeet stays the main recogniser. The Spoken language setting locks only Whisper's second listen.
