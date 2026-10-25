# Languages

OpenBark separates three languages:

| What | Languages | Where to set it |
|---|---|---|
| The consultation (what's said) | English plus 24 European languages, recognised by Parakeet TDT v3 with no setting needed | Automatic |
| The note (what the model writes) | English, French, Spanish, German, Portuguese, Italian, Dutch | Settings → Language → **Write notes in** |
| The app (menus, buttons, messages) | English, French, Spanish, German, Portuguese | Settings → Language → **App language** (starts in the browser's language when available) |

Notes:
- **Note language.** The model is told to write every statement in the chosen language, whatever language the consultation was in, and to keep drug names, doses, units and numbers exactly as said. Headings come from the template, so a team writing in French should name its template headings in French.
- **PDFs** use the note language for their own words (Patient, Signed by, Not discussed, Page 1 of 2).
- **Number checks** compare the note's numbers with the transcript. Spoken numbers ("thirty-nine point two") are normalised in English only, so in other languages a number said in words can be flagged for checking even when it's right. Vocabulary hints are English-only too.
- **App translations** were generated, and need review by native-speaking veterinary staff before release. They're keyed by the English text. A string without a translation shows in English. The newest, for deleting visits and recordings (ADR-0022, 2026-10-18), haven't been reviewed by anyone yet. Nor have those for MLX, gated models, adding models from this computer and removing models (ADR-0025, 2026-10-19). Nor have those for the redesigned screens (ADR-0027, 2026-10-24).

## Updating translations

1. `python scripts/i18n-keys.py > target/i18n-keys.json` lists every interface string.
2. Translate the new strings in `packages/i18n/i18n-<code>.json` (one JSON object per language, English → translation), keeping `{placeholders}`.
3. `python scripts/i18n-build.py packages/i18n/i18n-*.json` checks the placeholders and writes `apps/desktop/src/i18n/messages.ts`.

Adding a language means adding it to `UI_LANGUAGES` in `apps/desktop/src/i18n.ts`, to `Labels` in `engine/os-engine/src/pdf.rs`, and, for notes, to `NOTE_LANGUAGES` in both `i18n.ts` and `engine/os-engine/src/note.rs`.
