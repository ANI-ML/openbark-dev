# Drug names in context (ADR-0031)

A clinic found "Apoquel" transcribed as "apple" (an accented veterinarian, a laptop's microphone). This page measures the changes of ADR-0031 three ways:

1. **Text only, labelled lines** (`packages/eval/fixtures/vocab/hints.tsv`, scored by the `labelled_hint_set` test in `vocab.rs`): what recognisers write for a drug name they don't know, and ordinary sentences that must get no drug hint.
2. **Text only, what was really said**: the hint matcher run over every line of every consult's reference transcript (`packages/eval/fixtures/audio/*.ref.json`, 453 lines). Those lines are exactly what was said, so every hint on them is a false hint.
3. **Audio, end to end** (`.github/workflows/vocab-eval.yml`): every consult script and 12 brand-name snippets (`packages/eval/fixtures/vocab/scripts`) rendered with Kokoro, transcribed (Parakeet, with Whisper as the second recogniser) and drafted with Gemma 4 E4B on the CPU, by three builds: `main` (before), `noreg` (this change with the register names off) and `branch` (as shipped).

"main" below is the matcher before this change: the 296-term list and no context rules.

## 1. Labelled lines

37 lines where a drug should be hinted (real words like "apple", names split in two like "cyto point", near spellings like "incurine"), and 28 ordinary sentences, many with words that look like a drug name ("certainly", "library", "rabbits", "convenient", "clinic").

| Matcher | Drug hinted | Ordinary sentences with no hint | False hints |
|---|---|---|---|
| main | 4 / 37 | 20 / 28 | 8 |
| this change, register names off | 32 / 37 | 28 / 28 | 1 |
| **this change** | **37 / 37** | **28 / 28** | **0** |

The set was written alongside the rules, so it shows what the rules do, not how often clinics will see each slip; the audio run and clinic use are the real test. main's false hints were "Bravo" (Bravecto), "rabbits" (rabies), "entice" (Entyce), "lactate" (lactulose), "Flexion" (fluoxetine), "clinic" twice (clindamycin) and "romaine" (romifidine). With the register names off, "forthiron" is hinted as Fortekor; with them on, as Forthyron.

## 2. Reference transcripts (false hints on what was really said)

| Matcher | Hints on 453 correct lines |
|---|---|
| main | 13 |
| this change, register names off | 1 |
| **this change** | **1** |

main hinted "Extracting" (extraction) four times, "Border" Collie (Bordetella), "lactate", "Flexion", "lethargic", "clinic" twice, "romaine", "rabbit" (rabies) and "subcutaneously". The one left is "subcutaneously" (likely "subcutaneous", a route), harmless and left as it is.

The 3,218 register names add no false hint here: names a test consult's word would be mistaken for are left out when the list is built (120 of them), and the rest need drug context.

## 3. Audio, end to end

Numbers from the CI run are below (`vocab-eval-summary` artifact of the Vocabulary eval workflow).

Pending: filled in from the first full run.

## How the list was built

`scripts/vocab-import.py` (run by hand; downloads listed in `docs/network.md`, B10):

| Source | Products read | Terms it contributes |
|---|---|---|
| US FDA Green Book (`us-fda-gb`) | 1,968 trade names | 878 |
| UK VMD (`uk-vmd`) | 5,300 products | 1,396 |
| Health Canada DPD (`ca-dpd`) | 929 veterinary products | 462 |
| APVMA PubCRIS (`au-apvma`) | 3,406 veterinary products | 1,396 |
| EMA (`eu-ema`, brand names only) | 309 veterinary medicines | 179 |

After normalising (brand word only, salts dropped from ingredients, organisms and ordinary English words left out) and removing duplicates: **3,218 terms** (2,728 brand words, 490 ingredients; 3,054 drugs, 164 vaccines). 153 were already in the hand-written list and one is on the deny list. A term counts for every source that has it, so the per-source counts add up to more than the total.
