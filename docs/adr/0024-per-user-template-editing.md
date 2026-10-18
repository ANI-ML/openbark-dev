# ADR-0024: Per-user template editing

- Status: accepted (owner decisions 2026-10-18; see section 17)
- Date: 2026-10-18
- Deciders: product owner (asked on 2026-10-18: "There are plenty of templates, but as a user, I can't seem to edit them. Please allow me to edit them, the prompt, etc. Per user of course."), engineering
- Relations:
  - Amends ADR-0006 decision 4 ("Built-in templates are read-only (duplicate one to change it)") and decision 7 (every standard applies to the whole team). Keeps decisions 1 to 3, 5 and 6: templates are headings, the schema is generated, `CORE_RULES` can't be removed, every run keeps its template snapshot, and re-prompting works as before.
  - Follows the principle of ADR-0011 decision 4: changing something later never changes past notes.
  - Uses the team roles and PINs of M1 item 11 (`engine/os-engine/src/users.rs`) and the PIN re-entry pattern of ADR-0022.
  - Uses ADR-0008 for encryption at rest. Fills in the "User templates and standards" row of ADR-0017 section 6.
  - Copy follows the brand guide (ADR-0016, `docs/brand/DESIGN.md` section 8). Interaction follows `docs/design/DESIGN.md` section 5.
- Plan: [docs/plans/template-editing.md](../plans/template-editing.md)

## Context

### What was asked

The owner wants every person to be able to change any template, including the built-in ones: the headings, the instruction under each heading, the instructions for the whole note, and everything else a template controls in "what the model is told". The change must be per person. One clinician's preferences must not change a colleague's notes.

### What exists today (checked in the code on 2026-10-18)

| Area | Today | Where |
|---|---|---|
| Built-in templates | 20, compiled into the program from `packages/templates/*/template.yaml`. The builder disables every field and says the template "is built in and can't be changed. Duplicate it to make your team's version." The engine refuses to save or delete a built-in id | `template.rs` (`BUILTIN`), `template_store.rs` (`save`, `delete`), `Templates.tsx` (`readOnly = builtin`) |
| Changing a built-in | Only by copying it. **Duplicate** or **Start from SOAP** makes a new team template with a new id (`soap-2`) and a new name ("SOAP note copy"). The copy is a different visit type, so Record tiles, client documents and the per-heading example requests treat it as unrelated | `copyOf()` in `Templates.tsx` |
| Team templates | Plain YAML in `<templates>/<id>/template.yaml` (`OPENBARK_TEMPLATES_DIR`). Everyone on the computer sees them. Saving a change bumps the version | `template_store.rs` |
| Who may change templates | Anyone. Saving, deleting and changing standards carry no `by` and check no role. The Templates screen isn't gated, so reception can edit and delete what the whole team drafts with | `os-app` `save_template`, `delete_template`, `add_standard`, `remove_standard`; `App.tsx` |
| Standards | Stored per template id in `<templates>/<id>/standards.yaml`, built-ins included. Every standard applies to everyone, as "Team standard: ..." in the prompt | `template_store.rs`, `note.rs` (`guidance_line`, `rules`) |
| The prompt | Role line, then the template's `instructions`, then the rules (`CORE_RULES`, then the template's `rules`, then whole-note standards, then the level-of-detail line), then one guidance line per heading with its instruction and standards, then the JSON formats | `note.rs` (`system_prompt`) |
| Fixed rules | The 8 `CORE_RULES` are code. A template adds rules but can't remove these. The builder mentions them in one line of help text and doesn't list them. Per-template safety rules are ordinary `rules`: oncology "Never calculate a dose or body surface area", production animal "leave a withdrawal time null if it was not said. Never supply one", estimate "Never add up, round or estimate a cost" | `template.rs` (`CORE_RULES`), `packages/templates/*` |
| Note runs | Each run saves `runs/NNN/template.yaml` (the template with its standards) and `RunInfo.template` = `id@version`. Ask and Edit use that snapshot | `session.rs` (`save_run`, `run_template`) |
| Built-in versions | All 20 are `0.1.0`. Nothing forces a version bump when a built-in's content changes | `packages/templates/*/template.yaml` |
| Example note | Not stored. **From an example note** proposes a new template from pasted text. The text is never saved, and patient details aren't copied | `ExampleSheet.tsx`, `from_example.rs` (M2 item 5) |
| Translations | In the builder, built-in names, headings, measurement labels and example requests show in the app language, read-only. The model is instructed in English. Built-in headings in notes stay in English (M2 item 6) | `i18n.ts` (`builtinText`), `builtinText.ts` |
| CLI and eval | `LoadedTemplate::resolve` returns the shipped built-in for a built-in id, then a team template | `template.rs`, `os-cli` (`eval.rs`, `main.rs`) |

`HeadingEditor.tsx` edits a draft's content in Review. It doesn't touch templates.

In short: nobody can change a built-in. People can only make a renamed copy, which then appears for everyone. At the same time, anyone, in any role, can change or delete what the whole team drafts with.

### Constraints

- **Clinical safety** (the project's standing rules). A note stays a draft until a veterinarian signs it. Nothing is invented. Anything not discussed shows as "Not discussed". No template change may loosen this.
- **The schema stays generated** (ADR-0006). People edit headings, never JSON Schema.
- **Notes stay explainable.** Months later, anyone must be able to see which template text drafted a note.
- **Local-first.** No new network calls.
- **PINs are a convenience on a shared computer, not a security boundary** (`users.rs`, ADR-0022).
- **Built-in templates keep improving.** The dental template moves to 0.2.0 with the tooth chart (ADR-0021). A release must not wipe out anyone's changes, and must not silently change them either.

## Decision

### 1. Principles

1. **Anyone who drafts notes can change any template.** The change is saved as their own version. Nobody else's drafts change.
2. **The safety floor can't be edited away.** Core rules, a template's fixed rules, the generated schema with evidence on every line, "Not discussed", the checks after drafting and signing all stay. The editor shows them, read-only.
3. **Your version of a template is still that visit type.** Your version of SOAP keeps the id `soap`. Record, Draft as, client documents and example requests keep working.
4. **Every note records exactly which version drafted it.**
5. **A release never overwrites anyone's version.** It says a newer original is available, lets them compare, and lets them reset.
6. **Changing a template never changes a note already drafted, signed or amended.**

### 2. Three layers, one rule for which wins

| Layer | What it holds | Who sees it | Where it lives |
|---|---|---|---|
| Built in | The templates that ship with OpenBark | Everyone | Inside the program |
| Team | The team's own templates, and the team's version of a built-in | Everyone using this templates folder | `<templates>/<id>/`, plain YAML, as today |
| Personal | Your version of a built-in or team template, and templates only you use | Only you | Your profile, encrypted (section 11) |

**Resolution.** For each template id, the person drafting gets the personal layer first, then the team layer, then the built-in. "The person drafting" is the `by` that every drafting call already carries. The same rule applies to the template list, opening a template, the prompt preview, drafting, Draft as, and Try on a visit.

**The original.** The original of your version is what you would get without it: the team's version if there is one, otherwise the built-in.

**Ids.** Your version keeps the original's id. A new template gets an id that no built-in, no team template and no profile on this computer uses.

**The CLI and the eval keep using the shipped built-ins.** `LoadedTemplate::resolve` stays as it is, so `openbark eval` is never affected by anyone's changes. The layered rule is a new call, `TemplateStore::resolve_for(id, viewer)`, used by the app. The CLI gains `--as <member>` to resolve as a team member when asked.

### 3. What you can change

| Part of the template | In your version | Notes |
|---|---|---|
| Name and description | Yes | Shown in your template list and pickers |
| Icon (the visit type's icon) | Yes | |
| Level of detail | Yes | Concise, standard or thorough |
| Instructions for the whole note | Yes | Checked for conflicts with the fixed rules (section 4.4) |
| Extra rules | Yes | The template's own rules. Fixed rules are not editable |
| Headings and subheadings | Yes | Add, remove, rename, reorder, change what goes under them, and **Show** for some species. A renamed heading keeps its key (ADR-0006), so standards and examples still find it |
| The instruction under each heading (the prompt) | Yes | |
| Measurements | Yes | Add, rename, remove |
| Example requests under each heading | Yes | At most 8 per heading, as today |
| Who it's for (medical record or client) | **No** | Signing and client documents depend on it. To make a client version, **Duplicate** into a new template |
| Id | **No** | Your version is the same visit type |
| Schema | **No** | Always generated |
| Core rules and fixed rules | **No** | Shown read-only (section 4.6) |
| Standards | Your own, yes | Team standards can be turned off for yourself (section 8) |
| Example note | Not stored | See below |

**The example note.** OpenBark doesn't store example notes, and this ADR keeps it that way. A pasted note is usually a real note, with patient and client details. Team templates are plain files that clinics copy and share, and templates will sync (ADR-0017). Instead, the editor gains **Update from an example note**: paste a note, and the proposed headings replace the headings in your unsaved draft. Name, icon, level of detail and rules stay. You check and save as usual, and the example is discarded as today. Storing a sample note for the preview is decision D4.

### 4. The safety floor

#### 4.1 Core rules

`CORE_RULES` stay in code and apply to every template and every version. Nothing in a template can remove them.

#### 4.2 Fixed rules per template

`Template` gains `fixed_rules: Vec<String>`. In each built-in, the safety rules move from `rules` to `fixed_rules`. Examples:

| Template | Fixed rule (wording as today) |
|---|---|
| Oncology visit | Chemotherapy doses must be copied exactly as said, with units (mg/m², mg/kg, mg). Never calculate a dose or body surface area. |
| Production animal visit | Every drug used must keep its dose, route and the meat and milk withdrawal times exactly as said; leave a withdrawal time null if it was not said. Never supply one. |
| Estimate explanation | Copy every price, range and deposit exactly as said, with its currency. Never add up, round or estimate a cost. |
| Euthanasia and quality of life | Record consent only as it was stated in the recording; never assume consent was given. |
| Surgery and anaesthesia | Record complications only if they were said; never write "no complications" unless that was said. |

The full list is chosen in the plan, one built-in at a time.

When a version is drafted, its fixed rules come from **the current original**, not from the copy in the version. A safety fix in a new release therefore reaches everyone, including people with their own version. If the original no longer exists (a built-in was removed, or the team template was deleted), the fixed rules saved in the version are used.

Fixed rules are set only in built-ins. A team template made from scratch has the core rules and its own rules, as today.

#### 4.3 The rules always come first

The rules block of every prompt is split and ends with a precedence sentence:

```text
Rules that always apply:
- <each core rule>
- <each fixed rule of this template>
Template rules:
- <each of the template's own rules>
- Team standard: ... / Standard: ...
- Level of detail: ...
If a template rule, a standard or a heading instruction conflicts with the rules that always apply, follow the rules that always apply.
```

This changes every prompt, so the full eval is re-run before release (decision D5), as was done when `CORE_RULES` moved (ADR-0006).

#### 4.4 Warnings while typing

A new engine function, `lint_template`, checks instructions, rules and heading instructions for requests that conflict with the rules that always apply. It warns and never blocks, because the checks after drafting are the real guard. It looks for phrases such as:

| Pattern | Why it conflicts |
|---|---|
| "normal", "within normal limits", "WNL" with "assume", "if not mentioned", "default" | Findings that weren't said |
| "calculate", "work out", "estimate" near dose, BSA, price or withdrawal | Numbers that weren't said |
| "typical", "standard dose", "usual dose" | Doses that weren't said |
| "fill in", "complete every", "never leave empty" | Removes "Not discussed" |
| "guess", "infer the species", "make up" | Inventing |
| "no evidence needed", "skip citations" | Evidence links |

The same function backs `openbark templates check`. The patterns are a golden-tested list in the engine.

#### 4.5 The checks after drafting

Unchanged, and they apply to every version: the generated schema with evidence ids on every item, the number check, claim-level alignment (M2 item 1), unsupported numbers blocking **Sign note**, "Not discussed" rendering, and drafts staying unsigned until a veterinarian signs.

#### 4.6 Shown in the editor

Under **Instructions and rules for the whole note**, an **Always applied** panel lists, with a lock icon:
- the core rules, as plain-language sentences in the app language;
- this template's fixed rules;
- the checks after drafting, in one sentence each.

**Exact wording sent to the model** opens the English text. **What the model is told** marks the lines that can't be changed. The editor also says, honestly: "Built-in templates are tested on scripted consults. Your changes aren't, but every draft is still checked against the recording."

### 5. Your version: saving, history, reset and undo

- **First save.** Changing a built-in or team template and choosing **Save my version** creates your version. It stores the full template, an `origin` block (section 10), and `base.yaml`, a copy of the original as it was at that moment.
- **Versions.** Your version starts at 1. Each save that changes something adds one. Saving with no change keeps the number, as team templates do today.
- **Conflicts.** Every save sends the version it started from (`base_version`). If the stored version differs (the desktop app and `openbark serve` open on the same profile, or two people saving the team's version), the save is refused: "This template changed while you were editing. Your changes are still here. Compare, then save again."
- **History.** The last 20 versions of each template are kept, per layer.
- **Reset to original.** Your version moves into history and stops applying, so you draft with the original again. A toast offers **Undo**. Because it can be undone, there is no confirmation dialog (`docs/design/DESIGN.md` section 5).
- **Delete.** A template only you use can be deleted, with an inline confirmation ("Delete for good"). Notes drafted with it keep their snapshot.
- **Duplicate** still makes a new template, now one that only you use until you share it (decision D6).

### 6. Sharing with the team

From your version, **Share with the team** offers:

| Choice | What happens |
|---|---|
| **Make it the team's \<name\>** | Your version becomes the team's version of that id: the team's version of a built-in, or the next version of a team template. The previous team version goes into team history. Your personal version is removed, because it now matches the original |
| **Add it as a new template** | A new team template with its own name and id. Your version stays as it is |

A template only you use is shared as a team template with the same id.

After sharing:
- Everyone without their own version drafts with the team's version from their next draft. Drafts already made don't change.
- People with their own version of that id keep it, and see "The team's version changed" (section 9).

A veterinarian can also **Reset the team's version**. The team goes back to the built-in, the old team version goes into team history, and a toast offers **Undo**. Deleting a team template works as today, with the role rule below. People with a personal version based on it keep theirs.

### 7. Roles, approvals and single-user mode

| Action | Working alone | Veterinarian | Technician | Reception |
|---|---|---|---|---|
| Open any template, its prompt and the rules that always apply | Yes | Yes | Yes | Yes |
| Change any template, saved as your version | Yes (saved to the templates folder) | Yes | Yes | No (view only) |
| Make a new template | Yes (templates folder) | Yes, only you see it | Yes, only you see it | No |
| Reset your version, Undo, delete a template only you use | Yes | Yes | Yes | No |
| Save a standard for yourself | Not needed (all standards are yours) | Yes | Yes | No |
| Save a standard for the team | Yes | Yes | With a veterinarian's PIN | No |
| Share with the team | Not needed | Yes | With a veterinarian's PIN | No |
| Reset the team's version, delete a team template, remove a team standard | Yes | Yes | No | No |

Reasons:
- Reception can't draft notes (`Role::can_write_notes`), so a version of theirs would never be used. They can read everything. This is decision D3.
- Changing what the whole team drafts with is the same kind of decision as signing what the record says, so it rests with veterinarians. A technician who wants to share asks a veterinarian to enter their PIN in the same dialog. This is decision D2. Today any role can change team templates, and veterinarians and technicians can save team standards, so this tightens current behaviour.

**Approvals.** The veterinarian's PIN travels in the request body only, is checked inside the same engine call (`UserStore::verify`), and is never logged, as in ADR-0022. The team template's `origin` records both people: shared by the technician, approved by the veterinarian.

**Working alone (no team set up).** You are the team. Every change is saved to the templates folder: a change to a built-in becomes the folder's version of that built-in, and new templates go to the folder as today. There is no Share step and no personal layer. The files stay plain YAML you can copy to another computer. This is decision D1.

**Setting up a team.** When the first team member is added, everything in the templates folder becomes the team's. The first veterinarian sees one notice: "Templates you changed while working alone are now the team's. Everyone can make their own version."

**Removing a team member.** Their profile folder, and so their versions and personal standards, are removed with them. The confirmation says how many: "\<name\> has their own version of 3 templates. They're removed with \<name\>. Notes keep the version that drafted them." This is decision D7. A veterinarian resets someone's PIN in place (the engine already allows it, `App::set_user_pin`). The guide's "remove and re-add them" advice changes, so nobody loses their versions to a PIN reset.

**Switching person.** The template list, the editor and Record reload for the new person. Unsaved changes in the editor ask "Discard unsaved changes?" first.

### 8. Standards for you and for the team

- `Standard` gains `scope`: `team` (the default, so existing files load unchanged) or `personal`.
- **Ask** in Review: **Save as a standard** gains a choice, **For me** or **For the team**.
- Team standards stay in `<templates>/<id>/standards.yaml`. Personal standards live beside your version in your profile, in `<id>/standards.yaml`, with a `muted` list of team standards you turned off for yourself. You can hold personal standards without having your own version: they apply to whatever you draft that template with.
- **In the prompt**, team standards come first, as "Team standard: ...", then yours, as "Standard: ...". Muted team standards are left out of your prompt only.
- A standard for a heading your version doesn't have isn't applied. The editor lists it under **Not used by your version**.
- The run snapshot keeps each standard with its scope.

### 9. When the original changes

**Fingerprints.** A template's fingerprint is the SHA-256 of its canonical JSON without `id`, `version`, `origin` and `standards`. Built-in fingerprints are recorded in `packages/templates/fingerprints.json`. A test fails if a built-in's content changes but its version doesn't, and a release bumps the version of every built-in it changes.

**Detection.** On list and open, the engine compares the fingerprint your version was based on with the original's current fingerprint. A difference sets `original_changed`, with the original's name, version and layer.

**What you see:**
- In the list, the row shows **Newer original**.
- In the editor, a notice: "A newer SOAP note is built in (version 0.2.0). Your version is unchanged." or "The team changed SOAP note (team version 5). Your version is unchanged." It offers **Compare**, **Keep mine** and **Use the original**.

| Action | Effect |
|---|---|
| **Compare** | A sheet with two views. **What changed in the original** compares `base.yaml` with the original now. **Yours and the original** compares the original now with your version |
| **Keep mine** | Records the original's current fingerprint as seen. The badge goes away until the original changes again. The editor keeps a quiet line: "Based on an earlier SOAP note" |
| **Use the original** | The same as **Reset to original**, with **Undo** |

**The comparison is structured, not a text diff.** `template_diff(a, b)` matches headings by key and reports: heading added, removed, renamed, moved, kind changed, instruction changed, measurements changed, **Show** changed, examples changed, and for the whole note: name, description, icon, level of detail, instructions, rules and fixed rules. The same function backs `openbark templates diff`. Bringing in one change at a time is phase 2.

**Nothing merges on its own,** except fixed rules (section 4.2). A heading appearing in someone's notes without their choice would be a surprise in a clinical record. Fixed rules are different: they only ever keep notes to what was said.

| Case | What happens to your version |
|---|---|
| A release changes the built-in | Kept. "A newer original is built in" |
| The team makes or changes its version | Kept. "The team changed ..." |
| The team resets its version to the built-in | Kept. The notice compares with the built-in |
| The team template it came from is deleted | Kept, and still drafts. "The team template this came from was deleted" |
| A release removes the built-in | Kept, and still drafts. "This template is no longer built in" |
| An older OpenBark opens it (a downgrade, or later a synced copy from a newer release) | Works as a full copy. No comparison is offered for an id it doesn't know |

Example: when the dental template moves to 0.2.0 with the tooth chart (ADR-0021), a veterinarian with their own version of 0.1.0 keeps drafting with it, sees **Newer original**, compares, and chooses whether to reset.

### 10. What each note records

- The run snapshot (`runs/NNN/template.yaml`) gains the `origin` block:

  ```yaml
  origin:
    layer: personal            # builtin, team or personal
    owner: { id: 0192..., name: ..., role: rvt }   # personal only
    version: "3"
    fingerprint: 9b2e...       # of exactly what drafted the note
    based_on: { layer: builtin, id: soap, version: 0.1.0, fingerprint: 3f1c... }
  ```

- `RunInfo` gains `template_origin` (the same block), `template_name` and `template_icon`, so lists don't need to open each snapshot.
- Review's draft history says which version drafted each draft: "SOAP note, your version 3", "SOAP note, \<name\>'s version 2", "SOAP note, the team's version 4", or "SOAP note 0.1.0". Working alone, a changed built-in reads "SOAP note, edited version 1". **Show the template** opens the snapshot read-only.
- Visit lists show the name and icon saved with the note, falling back to today's lookup for older runs.
- **Ask** and **Edit** keep using the run's own snapshot, whoever's version it was.
- **Signing, addenda, client documents, PDFs and copy text don't change.** Signing locks a run, and the run already holds its snapshot. Resetting or deleting a version later changes nothing in signed notes. Client documents are still grouped by template id.
- Runs saved before this change have no `origin` and show as before.

### 11. Storage and encryption

```text
<templates>/                              team layer: plain YAML, shareable (OPENBARK_TEMPLATES_DIR)
  dental-procedure/template.yaml          a team template (unchanged)
  soap/template.yaml                      NEW: the team's version of the built-in SOAP note
  soap/base.yaml                          NEW: the built-in as it was when the team's version was made
  soap/standards.yaml                     team standards (unchanged)
  soap/history/4.yaml                     NEW: earlier team versions, the last 20

<folder of the team file>/profiles/       personal layer: encrypted with the local data key
  <member id>/templates/
    soap/template.yaml                    your version
    soap/base.yaml                        the original when you made your version
    soap/standards.yaml                   your standards and muted team standards
    soap/history/2.yaml                   your earlier versions, the last 20
    my-dental/template.yaml               a template only you use
```

- **Where profiles live.** Beside the team file: by default `<local data>/OpenBark/profiles/`. Setting `OPENBARK_USERS_FILE` moves it too, so tests and demo servers are isolated with no new variable. Profiles never live in the visits folder or the templates folder.
- **Why encrypted.** Templates are not clinical data, and must not hold patient details. Personal versions are still encrypted with the local data key, like `users.json` (ADR-0008), because they belong to a profile that only exists in that encrypted file, and free-text instructions are someone's own working notes. It costs nothing, and the Computer recovery key covers them. History entries are written as new files, never renamed, because the vault binds each file's name into its encryption.
- **Why the team layer stays plain.** Clinics share team templates by copying the folder or pointing `OPENBARK_TEMPLATES_DIR` at a shared drive, and check them with `openbark templates check` (ADR-0006).
- **Upgrading.** A `template.yaml` already sitting in a built-in's folder (only possible if someone copied one there by hand) is read as the team's version of that built-in, with no `base.yaml`. **Compare** then shows only "Yours and the original".
- **Sync-ready fields.** Each version's `origin` also carries `object_id` (UUIDv7), `saved_at` and `saved_by`, so sync (section 15) needs no migration.

### 12. Translations

- New interface strings go through the existing pipeline (`scripts/i18n-keys.py`, `packages/i18n/i18n-*.json`, `scripts/i18n-build.py`).
- **Built-in text in other languages.** `builtinText` changes its rule from "the template is built in" to "the template is built in or a version of one, and this text is unchanged from the built-in". Unchanged headings, labels and examples keep showing in the app language. Text you changed shows as written.
- **In the editor, inputs show the stored text**, which is what notes will show. When the app isn't in English and a heading is unchanged built-in text, a faint line under it reads "In French: \<translation\>" with **Use this**. **Use French headings** applies every available translation to your version at once. Keys don't change, so standards and examples still match.
- **Instructions** can be written in any language. The built-in ones are English, and the eval covers English instructions only. The existing line stays: "The model is instructed in English; notes are written in your chosen language."
- **The rules that always apply** show as translated plain-language sentences. **Exact wording sent to the model** shows the English. A test checks that every core rule has a summary, so a new core rule can't ship unexplained.

### 13. API

Reads take `by` in the query string. Writes take it in the body. Every write is a POST or DELETE, so `local_only` already demands the `X-OpenBark` header, and a test checks each new route. A PIN travels only in a body and is never logged. Each route has a matching Tauri command.

| Route | Body | Returns |
|---|---|---|
| `GET /api/templates?by=` | | `[TemplateSummary]`, now with `layer`, `edited`, `shared`, `original_changed` |
| `GET /api/templates/{id}?by=` | | `TemplateView`, now with `layer`, `origin`, `original`, `can`, `always_applied`, `lint` |
| `POST /api/templates/preview` | `{template, by}` | `TemplateView` |
| `POST /api/templates` | `{template, by, base_version?}` (was a bare `Template`) | `TemplateView`. Saves your version, a new template, or, working alone, the folder copy |
| `GET /api/templates/{id}/compare?by=&with=` | `with`: `original` or `base` | `TemplateDiff` |
| `POST /api/templates/{id}/reset` | `{by}` | `{view, undo}` |
| `POST /api/templates/{id}/reset/undo` | `{by, undo}` | `TemplateView` |
| `POST /api/templates/{id}/keep` | `{by, fingerprint}` | `TemplateView` |
| `POST /api/templates/{id}/share` | `{by, mode: replace or add, name?, approver?: {id, pin}}` | `TemplateView` |
| `POST /api/templates/{id}/team/reset` | `{by}` | `{view, undo}` |
| `DELETE /api/templates/{id}?by=` | | `{}` |
| `POST /api/templates/{id}/standards` | `{heading, text, by, scope, approver?}` | `Standard` |
| `DELETE /api/templates/{id}/standards/{sid}?by=` | | `TemplateView` |
| `POST /api/templates/{id}/standards/{sid}/mute` | `{by, muted}` | `TemplateView` |
| `POST /api/sessions/{id}/refine` | gains `standard_scope` and `approver?` | as today |

`POST /api/sessions/{id}/notes` already carries `by`, and now resolves the template with it. The UI and the engine are always built and updated together, so the changed body of `POST /api/templates` needs no compatibility shim.

### 14. Interface

#### 14.1 Where Edit appears

| Place | What it offers |
|---|---|
| **Templates** list | Every row opens the editor. Vets and technicians can change any template there, directly |
| Review, the template chip above the note | **Edit template** opens the editor on the template that drafted this note, and returns to the visit |
| Review, **Ask** on a heading | **Edit this heading in the template** opens the editor scrolled to that heading |
| Record | Nothing new. The visit-type tiles show your names and icons |

The editor opens editable, with no separate Edit mode. Nothing is saved until **Save my version**, and leaving with changes asks first, as today.

#### 14.2 The editor, by state

| State | Notice at the top | Primary button | Other actions |
|---|---|---|---|
| A built-in or team template you haven't changed | "Changes are saved as your version. Everyone else keeps the original." | **Save my version** (disabled until something changes) | Duplicate |
| Working alone, a built-in you haven't changed | "Changes are saved as your version. Reset to original brings back the built-in at any time." | **Save my version** | Duplicate |
| Your version | "Your version 3, from SOAP note 0.1.0. Only you draft with it." | **Save** | Compare with original, Reset to original, Share with the team, Duplicate |
| Your version, original changed | "A newer SOAP note is built in (version 0.2.0). Your version is unchanged." | **Save** | Compare, Keep mine, Use the original |
| A template only you use | "Only you draft with this template." | **Save** | Share with the team, Duplicate, Delete |
| The team's version, opened by a veterinarian | "The team's version 4 of SOAP note." | **Save my version** | Reset the team's version, Duplicate |
| Reception | "Templates are changed by the people who draft notes. You can read every template and what the model is told." | None | Duplicate is hidden |

One primary button per view. **Who it's for** shows read-only in a version, with "Fixed for this visit type. Duplicate the template to make one for the client." Warnings from `lint_template` show under the field they concern.

#### 14.3 Badges in the list

Each badge is a dot with text, never colour alone.

| Badge | Meaning |
|---|---|
| **Your version** | You have your own version of this template |
| **Only you** | A template you made and haven't shared |
| **Edited** | The team's version of a built-in (or, working alone, a built-in you changed) |
| **Team** | A template the team made |
| **Yours** | Working alone, a template you made (replaces **Team**, which means nothing without a team) |
| **Newer original** | The original of your version changed since you made or kept it |

Grouping stays as today ("Your team's templates", "Common visits", "Specialty and other notes", "For the client"), with **Only you** first in a team. A built-in with your version stays in its group with its badge.

#### 14.4 Compare, share and reset

- **Compare with the original** is a sheet: a segmented control for the two views, changes grouped by heading (Added, Removed, Renamed, Instruction changed with old and new text), then the whole-note changes. Original and yours sit side by side on desktop and stack on phones. Actions: **Keep mine** and **Use the original**. With nothing to show: "No differences."
- **Share with the team** is a dialog, because it changes other people's drafts. It names who is affected ("Everyone without their own version will draft with it."). For a technician it asks for a veterinarian and their PIN.
- **Reset to original** acts at once, with an **Undo** toast.

#### 14.5 Review

The draft history labels each draft with its version (section 10). **Save as a standard** shows **For me** and **For the team**. For a technician, **For the team** asks for a veterinarian's PIN in place. If the visit type chosen on Record isn't available to the person now drafting (another person's private template), drafting falls back to the default and says so: "\<template\> is only available to \<name\>. Drafting with New consult."

### 15. Future sync (ADR-0017)

ADR-0017 section 6 says user templates and standards sync later in M3, whole templates, last writer wins, old versions kept, built-ins never.

| Layer | How it syncs |
|---|---|
| Built in | Never. Each device has its own release's built-ins |
| Personal | With the team member's own account, once local members can link accounts (ADR-0017 section 3, "Shared computers"). One OBS1 object per template id per person: the template, `base.yaml` and personal standards. Last writer wins by hybrid logical clock. History is kept |
| Team, working alone | The templates folder syncs to the account of the person using it |
| Team, with a team | Stays on the computer or the shared folder until clinic spaces arrive (M4) |

- A synced version of a built-in carries `based_on`. Each device compares it with its own release's built-in, so "Newer original" is always worked out locally.
- An id collision between a synced private template and a different team template is resolved on arrival by giving the private one a new id. A test covers it in M3.
- The server sees what ADR-0017 section 8 already lists: an object kind, a padded size and timestamps. It can't read template names, headings or instructions.

### 16. What doesn't change

- The template file format, apart from the new optional `fixed_rules` and `origin` keys, and `scope` on standards. Existing files load as they are.
- The generated schema, and the SOAP schema snapshot test.
- The faithfulness pass, claim alignment, hard fails and "Not discussed".
- Signing, addenda, Ask and Edit on drafts, client documents, PDFs, copy formats.
- `openbark eval`, which keeps drafting with the shipped built-ins.
- The network list: no new calls.

### 17. Decisions for the owner

**Owner decisions (2026-10-18).** These override the proposals where they differ:

| # | Decision |
|---|---|
| D1 | Working alone: edits go straight into the templates (the team layer), as proposed. |
| D2 | **A clinic-wide toggle, set in Settings: "Templates: each person's own" or "Shared by the hospital".** In *each person's own*, every member keeps personal versions as designed, and the hospital's templates are the defaults. In *shared by the hospital*, everyone drafts with the hospital's templates; personal versions are switched off (kept on disk, not used, and restored if the toggle goes back). **Only admins can change hospital templates or the toggle, and every such change needs the admin's PIN.** "Admin" is a new flag on a team member, separate from the vet/tech/reception role. The person who sets up the team is the first admin, and admins grant or remove it. There must always be at least one admin. |
| D3 | Reception: view only, as proposed (admins excepted: an admin who is also reception can change hospital templates with their PIN). |
| D4 | Example notes are not stored, as proposed. |
| D5 | Yes: add the precedence sentence and move template safety rules into fixed rules, then re-run the full evaluation before release. |
| D6 | A new template is visible only to its maker until shared (and sharing to the hospital needs an admin PIN). |
| D7 | Removed member's versions: removed with them after a confirmation, as proposed. |

The original proposals follow for the record.

| # | Question | Proposed | Alternative |
|---|---|---|---|
| D1 | Working alone, where do changes go? | The templates folder, as the team layer. Plain, copyable files. When a team is set up, they become the team's | A personal profile, handed to the first veterinarian when a team is set up |
| D2 | Who may change what the whole team drafts with? | Veterinarians. Technicians with a veterinarian's PIN | Veterinarians and technicians (today's rule for standards), or anyone (today's rule for templates) |
| D3 | Can reception change templates? | No, view only. They don't draft, so their version would never be used | Let them keep versions anyway |
| D4 | Store an example note with a template? | No. Offer **Update from an example note**; the example is discarded | Store a sample note, shown in the preview only and never sent to the model. Risk: real patient details in shared files |
| D5 | Add the precedence sentence and split out fixed rules, with a full eval re-run? | Yes, before release | Ship without, and rely on the checks after drafting |
| D6 | In a team, who sees a new template? | Only its maker, until shared | Everyone at once (today) |
| D7 | What happens to a removed member's versions? | Removed with them, after the confirmation says how many. PINs are reset in place | Kept for a veterinarian to adopt or share |

## Consequences

- Each clinician can tune any note, including the built-ins, without changing a colleague's notes. "Per user" holds.
- Visit types stay stable. Your version of SOAP is still SOAP on Record, in client documents and in example requests.
- Old notes stay explainable. Each run says whose version drafted it and keeps the exact text.
- A safety fix in a new release reaches everyone, through `CORE_RULES` and fixed rules taken from the current original. Style and structure never change under anyone without their choice.
- The prompt changes for every template (section 4.3), so the eval must be re-run before release.
- Personal versions are not evaluated on the scripted consults. The checks after drafting still apply to every draft, and the editor says so.
- Notes across a clinic may drift apart. **Share with the team**, the team's version of a built-in, and **Newer original** pull them back together, but a clinic that wants one house style must use them.
- **Behaviour change for roles.** Today anyone can change or delete team templates, and technicians can save team standards. Under D2, team-wide changes need a veterinarian. The release notes and the guide must say so.
- **New states to explain.** Five badges and four editor states. The guide and the Help video for templates need updating.
- **Copying the templates folder no longer carries everything.** Personal versions stay in the profile, by design.
- Removing a team member removes their versions (D7). The guide stops recommending remove-and-re-add for a PIN reset.
- Built-in versions now matter. Every release that changes a built-in bumps its version, and a test enforces it.

## Alternatives considered

| Option | Why not |
|---|---|
| Keep built-ins read-only and improve **Duplicate** | A copy is a new visit type with a new id, so Record tiles, client documents and examples lose track of it, and everyone sees everyone's copies. It isn't per user |
| Edit built-ins in place, for the whole team only | Simple, but one person's change alters everyone's notes. It isn't per user. It survives as the team layer, behind a role |
| Personal copies with their own ids (`soap-<member>`) | Breaks visit-type continuity, client-document grouping and standards, and fills the list |
| Store personal versions as patches over the original | Small and update-friendly, but a patch can fail to apply after a release, and the template that drafts someone's notes would change without their choice |
| Merge new built-in releases into personal versions automatically | Silent changes to a clinical note's structure, and conflicts that are hard to explain. Fixed rules are the one part that updates on its own, because they only restrict |
| Personal versions as plain YAML in the templates folder | Anyone browsing the folder reads everyone's instructions, member ids mean nothing on another computer, and removing a member leaves orphans. The profile, encrypted, matches where members already live |
| Let people edit the core rules or the schema | Would let a template loosen faithfulness and drop evidence links, which ADR-0006 exists to prevent |
| Block instructions that conflict with the rules | Pattern lists can't be complete, and false blocks would frustrate. Warning while typing, rules first in the prompt, and checks after drafting are layered instead |
| Store example notes with templates | Real notes carry patient and client details into files that are shared and synced, and a model shown an example tends to copy its facts (D4) |
