# ADR-0031: Drug names in context, register names and the clinic's own words

- Status: accepted
- Date: 2026-10-28
- Deciders: engineering, after a clinic's report (0.2.4)
- Relations: extends the hint policy of M1 (`docs/eval/M1.md`, findings 3 and 4: visible hints, never a rewrite; hotwords rejected) and the second opinion of M1 item 4. Uses the admin approval of ADR-0024 for the clinic's list. Changes no prompt rule: the note model already treats hints as "use the likely spelling only when it fits; never add a drug because of a hint alone" (`template.rs`).

## Context

A clinic using OpenBark on a laptop with an accented veterinarian found frequent transcription errors. The clearest: "Apoquel" written as "apple". Nothing caught it:

- Parakeet (the main recogniser) decodes greedily with no word list. Hotwords made it worse in the M1 spike.
- The hint matcher compares spelling and sound. "apple" scores 0.761 against "apoquel": under the drug bar (0.87) and under the spelling floor (0.75 on spelling alone: 0.741).
- The second recogniser (Whisper turbo) only re-heard segments with a number, a hint or a word ending like a drug name, and guessed the language of each segment, so an accented English sentence could come back in another language. Even when it heard "Apoquel", the hint needed a word in the main transcript within 0.80 of it.
- The hand-written list had 296 terms and few brand names (no Metacam, Cerenia, Convenia, Librela, Rimadyl, Vetmedin).
- A clinic could add terms only by editing a file by hand.

The opposite risk is real too. M1 measured that adding hints to the prompt once cut facts found from 82% to 64%, and a loose match hints ordinary speech. The scan for this ADR found that the existing matcher already hinted some everyday words: "rabbits" as rabies, "clinic" as clindamycin, "lactate" as lactulose, "flexion" as fluoxetine.

## Decision

1. **Hints stay hints.** The transcript is never changed. Every new mechanism produces the same visible hint (`[heard "apple", likely apoquel]`), which the note model already reads with the rule above.

2. **Drug context.** A word is "in drug context" when, within eight words either side, there is a dose or unit (mg, millilitres, tablet, chew, injection...), a prescribing word (give, start, prescribe, dispense, refill...), a frequency or indication pair ("a day", "per kilo", "him on", "send home", "for itch"...) or another drug's name. It is computed on the transcript alone (`vocab.rs`, `drug_context`).

3. **Known mishearings** (`packages/vocab/mishearings.tsv`): real words and split forms that spelling can't reach ("apple", "a pickle", "site of point", "siren ya"...). They are hinted for drugs only in drug context. "She ate an apple" gets no hint.

4. **Split names**: two plain words that make one drug name of seven letters or more ("cyto point", "meta cam") are joined, in drug context only, at a stricter bar (0.92).

5. **Register names** (`packages/vocab/regulatory.tsv`, 3,218 terms): brand and ingredient words from the US FDA Green Book, the UK VMD, Health Canada's DPD, the APVMA and EMA, built by `scripts/vocab-import.py`. They are matched only in drug context, at a stricter bar (0.90, spelling 0.80), through an index by first sound. A register name that fits better than a hand-written term wins in drug context ("forthiron": Forthyron, not Fortekor). Heard exactly, a register name is a known term: no hint.

6. **Everyday look-alikes** (`packages/vocab/everyday.tsv`, generated): an everyday English word (SCOWL, size 35) that merely resembles a term is never hinted by spelling, whatever the context. A real confusion goes in the mishearings list instead.

7. **The hand-written list** gains 42 brand names said in clinics.

8. **Second opinion**: segments in drug context are also re-heard. When Whisper hears a known name where Parakeet wrote something unlike it, the hint is attached to the word standing in the same place, found by a neighbouring word both readings share. Whisper is told to expect the app's language (English until the app says otherwise; it detects the language itself only for a language the app doesn't offer).

9. **The clinic's own words**: Settings, Words and drug names lists, adds and removes terms (term, kind, optional "heard as" words) in the existing `vocab.tsv`. Rows keep two columns unless "heard as" words exist, so older versions still read them. In a team, each change needs an admin's PIN, as the hospital's templates do (ADR-0024).

10. **Attribution ships in the app**: `THIRD_PARTY_NOTICES`, copied to `licenses/vocabulary-data.txt`, SCOWL's notice in `licenses/scowl-copyright.txt`, and the text in Settings, About.

## Consequences

- More drug names are caught, and an ordinary word is hinted less often than before (rules 5 and 6).
- Clinics can fix a recurring slip themselves, without a release.
- More segments are re-heard by Whisper on computers that have it, so a recording takes a little longer to finish on a weak computer. The eval reports how many.
- The register list must be rebuilt by hand (`scripts/vocab-import.py`) and reviewed as a diff. It is not reviewed term by term, which is why it needs drug context and a stricter bar.
- `OPENBARK_VOCAB_REGULATORY=off` turns the register names off, for comparison.
- The numbers are in `docs/eval/vocabulary.md`. If false hints or facts found regress there, the register list is turned off by default and this ADR says so.

## Alternatives considered

| Option | Why not |
|---|---|
| Rewrite the transcript when confident | A wrong correction swaps one drug for another, silently. The project's rule is that the transcript keeps what was heard |
| Hotwords / contextual biasing in Parakeet | Tried in M1: it dropped the drug name and wrote "1 5 inch". A properly scored word list is still possible, but it changes recognition for every word, and greedy decoding is fixed in the current setup |
| A lower bar for every hint | "apple" is 0.761 from apoquel, and so are hundreds of ordinary words from some drug. The context rule and the mishearing list target the slip without hinting ordinary speech |
| Register names everywhere, without context | Thousands of unreviewed names would hint ordinary words, and each hint lengthens the prompt (M1: facts found fell from 82% to 64% when hints were added) |
| Whisper as the main recogniser | Slower on the processor, and it was kept as a second opinion for that reason (M1). It still helps through rule 8 |
