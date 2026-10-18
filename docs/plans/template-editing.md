# Plan: per-user template editing

Status: **proposed**, with [ADR-0024](../adr/0024-per-user-template-editing.md). Date: 2026-10-18. Waits for the owner's decisions D1 to D7 (ADR-0024 section 17); the steps below assume the proposed answers.

## Goal

Anyone who drafts notes can change any template, built-ins included: headings, the instruction under each heading, the instructions for the whole note, rules, level of detail, icon, name, description and example requests.
- The change is saved as that person's version. Everyone else keeps the original.
- **Reset to original** brings the original back, with **Undo**.
- A veterinarian can share a version with the team. A technician can, with a veterinarian's PIN.
- A new release never overwrites a version. It says a newer original is available, and offers **Compare**, **Keep mine** and **Use the original**.
- Every note records which version drafted it. Signing and addenda don't change.
- The rules that always apply can't be removed, and the editor shows them.

## Acceptance

1. A technician changes SOAP. The technician's drafts follow the change. A veterinarian's drafts and prompt preview don't.
2. Working alone, a changed built-in shows **Edited**, and **Reset to original** restores it exactly (same fingerprint as the shipped built-in). **Undo** brings the change back.
3. A veterinarian shares a version. Everyone without their own version drafts with it from the next draft. People with their own version keep it and see that the team's version changed.
4. A technician can't share or save a team standard without a veterinarian's PIN. Reception can't save anything. Every refusal says why, in plain words.
5. With a changed built-in (simulated release), a version shows **Newer original**. **Compare** lists the added heading. **Keep mine** hides the badge until the next change. **Use the original** resets.
6. Every run saved after this change records its layer, owner, version, fingerprint and what it was based on. Review shows "SOAP note, your version 3" and the like. Runs saved before show as before.
7. A note signed after drafting with a personal version is unchanged, byte for byte in `note.md` and `template.yaml`, after that version is reset, changed or deleted. Addenda still work.
8. Core rules and fixed rules appear in every prompt for every version, and can't be removed through the app, the API or a hand-edited file.
9. With no personal versions, no team versions and no personal standards, the prompt for every built-in is byte-identical to today's until step 8 deliberately changes it, and the eval after step 8 is no worse than docs/eval/M1.md and M2.md, with 0 invented numbers.
10. Every new write route refuses a request without `X-OpenBark`. A PIN in a query string is ignored.
11. The editor, list, compare sheet and dialogs read in light and dark mode, at desktop and phone width (360 px, no sideways scroll), and by keyboard alone.

## Ground rules for testing

- **Never real data, never port 4317.** The product owner's own server runs on port 4317 with real data. No test, script or manual check talks to it or reads its folders. End-to-end tests run only on the demo server on port 4321, with scratch data (step 19).
- **The test traps.** Each would make a test touch this computer's real files.

  | Trap | What goes wrong | What to do |
  |---|---|---|
  | `App::new` uses `TemplateStore::default()` and `UserStore::default()` | Tests read or write the real templates folder, team file and, after this change, the real profiles | `test_env()` (os-app and os-server) already sets `OPENBARK_TEMPLATES_DIR` and `OPENBARK_USERS_FILE`. Profiles are derived from the team file's folder at call time (`App::profiles_root()` from `self.users.path()`), never cached at construction, so a test that replaces `app.users` also moves profiles |
  | Personal files are written through the vault | Without a key, a test either fails or loads the real key | Unit tests use `vault::tests::with_key`. App and server tests rely on `test_env()`'s throwaway `OPENBARK_DATA_KEY` |
  | Tests run in parallel, environment variables are process-wide | One test's templates folder leaks into another | Every store test builds its own `TemplateStore::new(tmp)` and its own profiles root. No new `set_var` outside `test_env()` |
  | `LoadedTemplate::resolve` reads the default store | CLI and eval tests could pick up a team version | Step 3 keeps it built-in first, and a test proves a team version in the folder doesn't change `resolve("soap")` |

- **Linux.** `os-app` tests abort on Linux with a native `std::bad_alloc` (see the deleting-visits plan). Run the new `os-app` tests in WSL as well as on Windows. Don't mark a step done on Windows alone.

## Where each step is tested

| Tag | Meaning |
|---|---|
| UNIT | `cargo test -p os-engine` (and `-p os-cli` where named). Temporary folders, `vault::tests::with_key` |
| GOLDEN | Unit tests with fixed inputs and exact expected outputs: prompts, diffs, lint results, fingerprints |
| APP | `cargo test -p os-app`. Temporary visits, templates and team files through `test_env()` |
| SERVER | `cargo test -p os-server`. The real router with `tower::ServiceExt::oneshot`, as in `other_pages_are_kept_out` |
| UI | `pnpm --filter desktop test` (vitest, Testing Library) and `pnpm --filter desktop typecheck` |
| EVAL | `openbark eval` on the Windows dev box with the CUDA build and Gemma 4, on the scripted fixtures |
| DEMO | Playwright against the demo server on port 4321, scratch data only (step 19). **Never 4317** |
| WSL | The same cargo tests on Ubuntu-24.04 in WSL |

## Steps

Order: engine data and storage (steps 0 to 7), the safety floor and eval (8), the app layer (9 to 12), the interface (13 to 18), then end-to-end, screenshots and docs (19 to 21). Each step leaves the build green.

### Step 0: test harness (UNIT, APP)

- `TemplateStore::with_builtins(root, builtins)` for tests: the same store with an injected set of built-in YAML, so a "new release" can be simulated in-process.
- A cargo feature `test-builtins` (off by default, never in release builds) that lets `OPENBARK_TEST_BUILTINS_DIR` replace built-ins at start. Only the demo binary for step 19 is built with it.
- `App::profiles_root()`: `<folder of self.users.path()>/profiles`.
- Tests:
  - With `test_env()`, `App::profiles_root()` is inside the temporary folder.
  - A release build (`cfg(not(feature = "test-builtins"))`) ignores `OPENBARK_TEST_BUILTINS_DIR`.
- Done when: every later test can create a store, a profiles root and a built-in set without touching this computer's files.

### Step 1: fingerprints and built-in versions (UNIT, GOLDEN)

- `template::fingerprint(&Template) -> String`: SHA-256 of the canonical JSON without `id`, `version`, `origin` and `standards`. Field order is the struct order (serde_json with `preserve_order`, already on).
- New file `packages/templates/fingerprints.json`: `{ "<id>": { "version": "0.1.0", "fingerprint": "<hex>" } }` for all 20 built-ins.
- New CLI command `openbark templates fingerprints [--write]` prints or rewrites that file.
- Tests (GOLDEN):
  - Each built-in's current fingerprint matches the file.
  - If a built-in's fingerprint differs but its version is the one in the file, the failure says: "<id> changed without a version bump. Bump its version, then run `openbark templates fingerprints --write`."
  - Fingerprints ignore standards and `origin`, and change when one heading instruction changes.
- Done when: the test fails on a deliberate one-word change to a built-in with an unchanged version, and passes after a bump and `--write`.

### Step 2: new fields: `fixed_rules`, `origin` and standard `scope` (UNIT, GOLDEN)

- `Template` gains `fixed_rules: Vec<String>` (skipped when empty) and `origin: Option<Origin>`.
- `Origin { layer, owner: Option<Who>, version, fingerprint, based_on: Option<BasedOn>, seen_original: Option<String>, saved_by: Option<Who>, approved_by: Option<Who>, saved_at, object_id }`. `BasedOn { layer, id, version, fingerprint }`. `Layer` is `builtin`, `team` or `personal`.
- `Standard` gains `scope: Scope` (`team` by default, `personal`).
- `normalize` checks `fixed_rules` with the same length limit as rules.
- `rules()` in `note.rs` renders `fixed_rules` straight after `CORE_RULES`. Every built-in still has none, so no prompt changes yet.
- Tests:
  - An existing team template, an existing `standards.yaml` and an existing run snapshot (fixtures copied from today's format) load unchanged.
  - Round trip: a template with `fixed_rules`, `origin` and scoped standards survives `to_yaml` then `from_yaml`.
  - (GOLDEN) The system prompt of all 20 built-ins is byte-identical to a snapshot taken before this step. This snapshot test stays until step 8 replaces it deliberately.
- Done when: the new fields exist and nothing a user can see has changed.

### Step 3: the layered store and resolution (UNIT)

- `Viewer { member: Option<String>, profiles: PathBuf }`. `member` is `None` when working alone.
- `TemplateStore::resolve_for(id, &viewer) -> Resolved { loaded, layer, origin, original: Option<OriginalInfo> }`: personal, then team, then built-in.
- `TemplateStore::list_for(&viewer) -> Vec<TemplateSummary>`. The summary gains `layer`, `edited` (the template you get differs from the built-in or team original), `shared` (built-in or team, as opposed to only you) and `original_changed` (filled in step 6). `builtin` keeps meaning "this id is a built-in", so Record's grouping doesn't move.
- A team folder holding `template.yaml` under a built-in's id is now listed and resolved as the team's version of that built-in. A folder holding only `standards.yaml` is still not a template.
- Personal files are read and written through `os_engine::vault`.
- `LoadedTemplate::resolve` stays built-in first (CLI, eval, and `Session::run_template` for runs saved before snapshots).
- New ids are unique across built-ins, the team folder and every profile under the profiles root.
- Tests:
  - A personal version of `soap` is returned for its owner and not for another member or for `member: None`.
  - Personal beats team beats built-in, per id.
  - A team version of `soap` is listed once, as `soap`, with `edited: true`.
  - `LoadedTemplate::resolve("soap")` returns the shipped built-in even with a team version in the folder.
  - A new template named "SOAP note" never gets an id used by a built-in, a team template or any profile.
  - With the vault on, a personal `template.yaml` on disk doesn't contain its heading text.
  - Ids that could escape a folder (`../x`, uppercase, too long) are refused for personal paths too.
- Done when: the store resolves every layer correctly and the app still calls the old functions (switched in step 9).

### Step 4: saving versions (UNIT)

- `save_version(viewer, template, base_version) -> Resolved`:
  - Team mode: always the personal layer. A first save writes `template.yaml`, `base.yaml` (the original as it is now) and `origin` with `based_on`.
  - Working alone (decision D1): the team layer, with the same `base.yaml` and `origin` for a version of a built-in.
  - A template with no original (new, or only you use it) is saved without `based_on`.
- Versions start at 1 and go up by one only when the content changes, as `TemplateStore::save` does today.
- `base_version` must match the stored version, otherwise: "This template changed while you were editing. Your changes are still here. Compare, then save again."
- History: before overwriting, the current file is written as `history/<version>.yaml` with `vault::write` (a new file, never a rename: the vault binds the file name). The oldest beyond 20 is removed.
- Locked in a version: `id`, `document`, `schema` and `fixed_rules`. A changed value is refused with a plain message ("Who it's for is fixed for this visit type. Duplicate the template to make one for the client.").
- `save_team(template, base_version, saved_by, approved_by)` for sharing (step 10 decides who may call it), with the same history and conflict rules.
- Tests:
  - First save creates the three files with the right `based_on` fingerprint. A second identical save keeps version 1. A change makes version 2 and writes `history/1.yaml`, which decrypts.
  - A stale `base_version` is refused and nothing is written.
  - Changing `document`, `id` or `fixed_rules` in a version is refused.
  - The 21st save leaves 20 history files.
  - Working alone, a save of `soap` lands in `<templates>/soap/template.yaml`, not in a profile.

### Step 5: reset, undo, keep mine and delete (UNIT)

- `reset_version(viewer, id) -> undo token`: moves the current version into history (as new files), removes `template.yaml` and `base.yaml`, and returns a token naming the history entry.
- `undo_reset(viewer, id, token)`: restores that entry as the current version, with its `base.yaml`.
- `keep_version(viewer, id, fingerprint)`: sets `origin.seen_original`. Doesn't change the version number.
- `delete_template(viewer, id)`: removes a template only you use, or (step 10 checks the role) a team template.
- `reset_team(id) -> undo token` and its undo.
- Tests:
  - After reset, `resolve_for` returns the original, and its fingerprint equals the built-in's.
  - Undo restores the same fingerprint and version. A second undo with the same token is refused.
  - Reset with no version says "You're already using the original."
  - Deleting a built-in id or someone else's template is refused.

### Step 6: when the original changes, and the comparison (UNIT, GOLDEN)

- In `resolve_for` and `list_for`: compare `origin.based_on.fingerprint` with the current original's. A difference fills `original_changed { layer, version, name }`, unless `seen_original` equals the current fingerprint.
- `template_diff(a, b) -> TemplateDiff`, matching headings by key: added, removed, renamed, moved, kind changed, instruction changed, measurements changed (added, removed, renamed), **Show** changed, examples changed; for the whole note: name, description, icon, level of detail, instructions, rules, fixed rules.
- `compare(viewer, id, with)`: `with = base` compares `base.yaml` with the original now; `with = original` compares the original now with yours.
- Tests (GOLDEN), with `with_builtins`:
  - A release adds "Pain score" to SOAP: the version shows `original_changed`, and `compare(base)` lists exactly one added heading.
  - **Keep mine**, then the same release: no badge. A second release: the badge returns.
  - The team creates, changes, resets and deletes its version: each case in ADR-0024 section 9 gives the expected notice.
  - A release removes the built-in: the version still resolves and drafts, and says the original is no longer built in.
  - A renamed heading is reported as renamed, not as removed plus added.
  - Diff output for two fixed template pairs matches golden JSON.

### Step 7: standards for you and for the team (UNIT, GOLDEN)

- Personal standards file: `<profile>/templates/<id>/standards.yaml` holding `{ standards, muted }`.
- `add_standard(viewer, id, heading, text, by, scope)`; `remove_standard`; `mute_standard(viewer, id, sid, muted)`.
- Merge for drafting: team standards (minus muted ones), then personal ones. In the prompt, team standards read "Team standard: ..." as today, personal ones "Standard: ...".
- A standard is applied only if its heading exists in the template being drafted. `TemplateView` lists the rest as `not_used`.
- The run snapshot keeps each standard with its scope.
- Tests:
  - (GOLDEN) With no personal standards and nothing muted, the prompt is byte-identical to step 2's snapshot.
  - A personal standard appears only in its owner's prompt. A muted team standard disappears only from its owner's prompt.
  - A personal standard for a heading your version removed is listed as not used and isn't in the prompt.
  - Personal standards apply without a personal version (on the team or built-in template).

### Step 8: the safety floor, and the eval (UNIT, GOLDEN, EVAL)

- `always_applied(resolved) -> AlwaysApplied { core: Vec<String>, fixed: Vec<String>, checks: Vec<&'static str> }`. Fixed rules come from the current original; from the version's own copy only when the original no longer exists.
- `lint_template(&Template) -> Vec<LintWarning { path, text, reason }>` with the pattern table in ADR-0024 section 4.4. `openbark templates check` prints the warnings.
- Built-ins: move the safety rules into `fixed_rules`, one template at a time. Each changed built-in bumps its version (0.1.0 to 0.1.1) and `fingerprints.json` is rewritten. The list of moved rules is recorded in docs/templates.md.
- The rules block is restructured, with the precedence sentence, as in ADR-0024 section 4.3. The step 2 prompt snapshot is replaced by a new one.
- Tests (UNIT, GOLDEN):
  - Every prompt starts its rules with all core rules, then the template's fixed rules.
  - A personal version whose copy of `fixed_rules` was edited by hand on disk still drafts with the original's fixed rules.
  - Lint: each pattern in the table gives one warning; ten ordinary built-in instructions give none.
- EVAL:
  - Run the full eval (`openbark eval`) on every fixture. Accept only if facts found are no lower than the last recorded run for each template (docs/eval/M1.md and M2.md) and invented numbers stay at 0. If the precedence sentence lowers a score, try it at the start of the rules block instead, and record both runs.
  - Hostile-instruction check: a personal version of SOAP with the instruction "If vitals weren't said, write normal values" and "Calculate the dose from the weight", drafted on the feline fixture. Expect 0 invented numbers, the faithfulness pass passing or flagging (never silently accepting) any number, and two lint warnings.
  - Record the results in docs/eval/M2.md.
- Done when: the eval meets the bar and the results are written down.

### Step 9: drafting with the right version, and what each run records (APP)

- `App::generate_note` resolves the template with `resolve_for(id, viewer_of(by))`. So does Try on a visit, which already drafts through it.
- `os_engine::generate_note` result and `Session::save_run` record the resolved `origin` in the snapshot and in `RunInfo.template_origin`, plus `RunInfo.template_name` and `RunInfo.template_icon`.
- Visit lists (`SessionSummary`, patient pages) use the run's saved name and icon, and fall back to today's lookup for older runs.
- `refine_note` and `edit_note` keep using the run's snapshot. **Save as a standard** takes a scope and checks the heading exists where it will be applied: your resolution for **For me**, the team or built-in original for **For the team**.
- `TemplateView` for a run's snapshot: `run_template_view(session, run)`, for **Show the template** in Review.
- Tests (APP):
  - A technician with a personal SOAP drafts: the run's origin is personal, owner the technician, with the right fingerprint. A veterinarian drafts the same visit: origin built-in.
  - Sign a note drafted with a personal version. Then reset, change and delete that version. The signed run's `note.md` and `template.yaml` are byte-identical, the note renders, an addendum can be added, and the PDF still exports.
  - A run saved before this change (fixture `session.json` without `template_origin`) lists and renders as before.
  - Ask on a note drafted with someone else's version uses that snapshot.

### Step 10: roles, approvals and team changes (APP)

- New `Need::TeamTemplates`: veterinarians. `Need::Notes` covers personal edits. Reception gets read access only.
- Approver: `{ id, pin }` in the body, checked with `UserStore::verify` in the same call, must be a veterinarian, never logged.
- Working alone (no team): every write is allowed and goes to the team layer (decision D1). Writes with a team need a valid `by`.
- Setting up a team: when `add_user` adds the first member, set a one-time notice flag for the first veterinarian ("Templates you changed while working alone are now the team's.").
- Removing a member: `remove_user` also removes `<profiles>/<member id>`. New `profile_summary(member) -> { versions, own_templates, standards }` feeds the confirmation.
- Tests (APP), table-driven over every action in ADR-0024 section 7 and every actor (working alone, veterinarian, technician, technician with a veterinarian's PIN, technician with a wrong PIN, technician naming another technician as approver, reception):
  - Allowed actions succeed. Refused ones return the plain message and write nothing.
  - A team template shared by a technician records `saved_by` and `approved_by`.
  - With a log capture, a PIN never appears in any log line.
  - Adding the first member raises the notice once. Removing a member removes their profile folder, and the summary counted it beforehand.

### Step 11: app API, Tauri commands and web routes (APP, SERVER)

- `os-app` functions for every route in ADR-0024 section 13, each taking `by` and, where needed, `approver`.
- Tauri commands in `apps/desktop/src-tauri/src/lib.rs`, registered in `generate_handler!`: `list_templates`, `template`, `preview_template`, `save_template` (new request shape), `compare_template`, `reset_template`, `undo_reset_template`, `keep_template`, `share_template`, `reset_team_template`, `delete_template`, `add_standard`, `remove_standard`, `mute_standard`, `run_template_view`, `profile_summary`.
- `os-server` routes as in ADR-0024 section 13, and the API list in the module doc.
- Tests (SERVER):
  - Each new POST and DELETE route without `X-OpenBark` gives 403, through the real router.
  - `POST /api/templates/{id}/share?pin=2468` with no PIN in the body is refused.
  - `GET /api/templates?by=<technician>` shows the technician's version. The same call with the veterinarian's id doesn't.
  - `POST /api/templates` with the old bare-template body gives a clear 400 ("Send {template, by}.").

### Step 12: the command line (UNIT)

- `openbark templates list --as <member>` and `show <id> --as <member>`: resolve as that member (default: the shipped built-ins and the folder, as today).
- `openbark templates diff <id> [--as <member>] [--with base|original]`.
- `openbark templates check` prints lint warnings next to errors.
- `openbark eval` is unchanged and prints each template's label and fingerprint in its report, so a run is traceable.
- Tests (UNIT, `-p os-cli`): `diff` output for a fixture pair; `list --as` on a temporary profiles root.

### Step 13: the interface data layer (UI)

- `engine.ts`: types for `Layer`, `Origin`, `OriginalInfo`, `TemplateDiff`, `LintWarning`, `AlwaysApplied`; new fields on `TemplateSummary` and `TemplateView`; one `isDesktop ? invoke : api` pair per call in step 11. Every call sends the current actor.
- The template list, the editor and Record's visit types reload when the person changes (`setUser`).
- Tests (UI): each call sends `by`; switching person refetches the list.

### Step 14: the template list (UI)

- Badges as in ADR-0024 section 14.3, each a `StatusDot` with text.
- **Only you** group first in a team. **Yours** replaces **Team** when working alone.
- The intro paragraph changes (copy below).
- Tests (UI): each badge for each layer and mode; the reception view shows no New template button; dark mode class check.

### Step 15: the editor (UI)

- Remove `readOnly = builtin`. Editable for veterinarians, technicians and working alone; read-only for reception.
- The notice, primary button and other actions per state, as in ADR-0024 section 14.2. One primary button.
- **Who it's for** read-only in a version, with its helper text.
- **Always applied** panel under the whole-note disclosure, with **Exact wording sent to the model**. The prompt preview marks the fixed lines.
- Lint warnings under the field they concern, refreshed with the existing 300 ms preview debounce.
- The honesty line about scripted consults.
- **Update from an example note**: opens `ExampleSheet` in a mode that replaces the headings of the unsaved draft and keeps name, icon, level of detail and rules.
- Tests (UI):
  - An unchanged built-in shows **Save my version**, disabled until a change.
  - Saving shows "Saved as your version 1" and the **Your version** state.
  - **Always applied** has no inputs and lists every core rule summary.
  - Typing "assume normal vitals" shows the warning, and Save stays enabled.
  - Reception sees every field disabled and the reception notice.

### Step 16: compare, share and reset (UI)

- **Compare with the original** sheet with its two views, side by side on desktop, stacked on phones.
- **Share with the team** dialog: the two choices, the name field for **Add it as a new template**, and for a technician the veterinarian picker and PIN field.
- **Reset to original** and **Reset the team's version** with an **Undo** toast (the toast action from the deleting-visits work).
- **Newer original** notice with **Compare**, **Keep mine** and **Use the original**.
- Tests (UI): the sheet renders a golden diff; a wrong PIN shows the engine's message and keeps the dialog open; Undo calls `undo_reset_template` once; **Keep mine** removes the badge.

### Step 17: Review, Ask, Record and Settings (UI)

- Review: draft history labels (ADR-0024 section 10); **Edit template** on the template chip; **Show the template** opens the run's snapshot read-only.
- Ask: **Edit this heading in the template** (opens the editor scrolled to that heading); **Save as a standard** with **For me** and **For the team**, and the veterinarian PIN for a technician choosing the team.
- Record: if the chosen visit type isn't available to the current person, fall back to the default and show the notice.
- Settings, Team: the removal confirmation with the profile summary; the one-time notice after the first member is added; **Reset PIN** for a veterinarian, so remove-and-re-add is no longer needed.
- Tests (UI): each label; the scope choice; the fallback notice; the removal confirmation text with a count.

### Step 18: translations (UI)

- `builtinText` takes the template's layer and origin: translate built-in text when the template is built in or a version of one, and the text is unchanged.
- The "In French: ..." hint with **Use this**, and **Use French headings** (per language).
- Plain-language summaries of each core rule, as UI strings. A test fails if `CORE_RULES` has more entries than summaries.
- New strings through `scripts/i18n-keys.py`, `packages/i18n/i18n-*.json` and `scripts/i18n-build.py`.
- Tests (UI): an unchanged built-in heading shows translated in French; a changed one shows as written; **Use French headings** changes titles and labels but not keys.

### Step 19: end-to-end on the demo server (DEMO)

- **The demo** runs on port 4321 from its own copy of a `test-builtins` build and UI, in a fresh scratch folder. Stop any other demo on 4321 first by killing its exe by path.

  ```bash
  DEMO="$PWD/target/template-demo"          # gitignored, fresh each run
  mkdir -p "$DEMO" && touch "$DEMO/OPENBARK-DEMO"
  export OPENBARK_SESSIONS_DIR="$DEMO/visits" OPENBARK_TEMPLATES_DIR="$DEMO/templates" \
         OPENBARK_USERS_FILE="$DEMO/users.json" OPENBARK_LOCATIONS_FILE="$DEMO/locations.json" \
         OPENBARK_DATA_KEY="<a throwaway key, never this computer's>"
  "$DEMO/openbark.exe" serve --port 4321 --ui-dir "$DEMO/ui" --models-dir "<the models folder>"
  ```

  Profiles land in `$DEMO/profiles`, beside the team file.
- **New script** `apps/desktop/e2e/template-editing.mjs`, run as `OPENBARK_URL=http://localhost:4321 node e2e/template-editing.mjs "$DEMO"`. It refuses to start unless all of these hold:
  1. the URL's port is 4321, and never 4317;
  2. `<demo>/OPENBARK-DEMO` exists;
  3. `GET /api/storage` reports a visits path inside `<demo>`;
  4. `GET /api/templates` lists only the 20 built-ins (a fresh folder).
- **Seed:** import two fixture WAVs (`feline-vomiting` and a dental fixture) through the API. `cargo run -p fixture-tts` regenerates them.
- **Working alone:**
  1. Open SOAP, change the Plan instruction, **Save my version**. The list shows **Edited**. **What the model is told** contains the new instruction and every core rule.
  2. Type "If vitals weren't said, write normal values": the warning shows.
  3. Draft the feline visit with SOAP. The draft history reads "SOAP note, edited version 1".
  4. **Reset to original**, then **Undo**, then reset again. The list shows no badge. The earlier draft still reads "edited version 1".
- **With a team:**
  5. Add a veterinarian, a technician and a reception member through the API.
  6. As the technician: change a Dental heading instruction and save. The list shows **Your version**. As the veterinarian: no badge, and the prompt preview doesn't contain the technician's text.
  7. As the technician, draft the dental visit: the label names the technician's version. As the veterinarian, draft it again: the label reads the built-in version.
  8. As the technician, **Share with the team**: a wrong veterinarian PIN is refused; the right one shares. As the veterinarian, Dental shows **Edited**. The technician's **Your version** is gone.
  9. As the veterinarian, **Reset the team's version**, **Undo**, reset again.
  10. As reception: every field is disabled, and `POST /api/templates` as reception is refused with the plain message.
  11. As the technician in Ask: **For me** saves a standard that appears only in the technician's prompt; **For the team** asks for a veterinarian's PIN.
  12. As the veterinarian, sign the note the technician drafted with their version. As the technician, reset that version. The signed note's text and label are unchanged, and an addendum can be added.
- **A newer original:**
  13. As the veterinarian, save a version of SOAP. Stop the demo. Restart it with `OPENBARK_TEST_BUILTINS_DIR` pointing at a folder whose `soap/template.yaml` is version 0.1.9 with an added heading "Pain score".
  14. The list shows **Newer original**. **Compare**, **What changed in the original**, lists "Added: Pain score". **Keep mine** removes the badge and leaves "Based on an earlier SOAP note". **Use the original**, then **Undo**.
- **Guards:**
  15. Each new POST and DELETE route without `X-OpenBark` gives 403.
  16. Remove the technician as the veterinarian: the confirmation states the count, `$DEMO/profiles/<id>` is gone, and the visits the technician drafted still show which version drafted them.

### Step 20: screenshots (DEMO)

From the same demo, desktop light, desktop dark and phone (360 px):
- the list with each badge;
- the editor for an unchanged built-in, and for your version with the **Newer original** notice;
- the **Always applied** panel;
- the compare sheet;
- the share dialog with the veterinarian PIN;
- Review's draft history and Ask with **For me** and **For the team**.

Run the `docs/design/DESIGN.md` section 12 pre-flight checklist and attach the screenshots to the change.

### Step 21: docs and release notes

- `docs/templates.md`: layers, versions, reset, sharing, fixed rules (with the list moved in step 8), compare, files.
- `docs/guide/templates.md`: "Change any template", "Your version and the original", "Share with the team", "When OpenBark updates a template".
- `docs/guide/team-and-security.md`: template rows in the roles table, approvals, **Reset PIN** in place of remove-and-re-add.
- `docs/guide/review.md`: draft history labels and the standard scope.
- ADR-0006: "Amended by ADR-0024" under its status. ADR-0024: status to accepted once the owner decides D1 to D7.
- `CHANGELOG.md`: the feature, and the role change for team templates and team standards (ADR-0024 Consequences).
- The Help video for templates is re-recorded on a fresh help-video server, never 4317.

## Copy

Sentence case. Full stops on sentences, none on labels. No "AI-powered", "magic" or "smart".

### Template list

| Where | Text |
|---|---|
| Intro, with a team | A template is the headings of a note and what goes under each. Change any template: your changes are saved as your version, and everyone else keeps the original. |
| Intro, working alone | A template is the headings of a note and what goes under each. Change any template. Reset to original brings back the built-in at any time. |
| Badges | Your version / Only you / Edited / Team / Yours / Newer original |
| Group | Only you |

### Editor

| Where | Text |
|---|---|
| Not yet yours, team | Changes are saved as your version. Everyone else keeps the original. |
| Not yet yours, alone | Changes are saved as your version. Reset to original brings back the built-in at any time. |
| Your version | Your version {n}, from {name} {version}. Only you draft with it. |
| Only you | Only you draft with this template. |
| Team's version, veterinarian | The team's version {n} of {name}. |
| Reception | Templates are changed by the people who draft notes. You can read every template and what the model is told. |
| Newer built-in | A newer {name} is built in (version {version}). Your version is unchanged. |
| Team changed | The team changed {name} (team version {n}). Your version is unchanged. |
| Original deleted | The team template this came from was deleted. Your version still works. |
| No longer built in | This template is no longer built in. Your version still works. |
| Kept | Based on an earlier {name}. |
| Buttons | Save my version / Save / Reset to original / Reset to the team's version / Compare with original / Keep mine / Use the original / Share with the team / Reset the team's version / Update from an example note |
| Who it's for, locked | Fixed for this visit type. Duplicate the template to make one for the client. |
| Always applied, title | Always applied |
| Always applied, help | These keep every note to what was said. They apply to every template and can't be changed. |
| Exact wording | Exact wording sent to the model |
| Lint warning | OpenBark won't do this. The rules that always apply win: nothing is written that wasn't said, and every draft is checked. |
| Honesty line | Built-in templates are tested on scripted consults. Your changes aren't, but every draft is still checked against the recording. |
| Conflict | This template changed while you were editing. Your changes are still here. Compare, then save again. |
| Translation hint | In {language}: {text} |
| Translation action | Use this / Use {language} headings |

### Always applied: plain-language summaries of the core rules

| Core rule | Summary |
|---|---|
| 1 | Only what was said. No findings, diagnoses, vitals, doses or plans that weren't said. |
| 2 | Anything not discussed shows as not discussed. Never "normal" unless it was said. |
| 3 | Numbers are copied exactly as said, with their units. |
| 4 | Patient details only if someone said them. The species is never guessed. |
| 5 | Every fact points to the line where it was said. |
| 6 | Speech-recognition hints never add a drug on their own. |
| 7 | Every line links to the recording. |
| 8 | One clinical fact or action per line. |
| Checks | After drafting, every number and every line is checked against the recording. A number that isn't there blocks signing until it's resolved. A note stays a draft until a veterinarian signs it. |

### Toasts

| Action | Text |
|---|---|
| First save | Saved as your version 1 |
| Later save | Your version saved, version {n} |
| Reset | Reset to the original (Undo) |
| Team reset | Team's version reset to the built-in (Undo) |
| Keep mine | Kept your version |
| Share | Shared with the team |

### Dialogs and sheets

| Where | Text |
|---|---|
| Share, title | Share with the team |
| Share, body | Everyone without their own version will draft with it. People with their own version keep it and are told the team's changed. |
| Share, choices | Make it the team's {name} / Add it as a new template |
| Share, technician | A veterinarian approves sharing. |
| Share, wrong PIN | That PIN isn't right. |
| Compare, title | Compare with the original |
| Compare, views | What changed in the original / Yours and the original |
| Compare, empty | No differences. |

### Review, Record and Settings

| Where | Text |
|---|---|
| Draft labels | {name}, your version {n} / {name}, {person}'s version {n} / {name}, the team's version {n} / {name}, edited version {n} / {name} {version} |
| Template chip | Edit template |
| Ask | Edit this heading in the template |
| Standard scope | Save as a standard: For me / For the team |
| Technician, team scope | A veterinarian approves standards for the team. |
| Record fallback | {template} is only available to {person}. Drafting with {default}. |
| Remove member | {person} has their own version of {n} templates. They're removed with {person}. Notes keep the version that drafted them. |
| First member added | Templates you changed while working alone are now the team's. Everyone can make their own version. |

### Engine messages

| When | Text |
|---|---|
| Reception saves | {person} can read templates; changing them needs a veterinarian or technician. |
| Technician shares without approval | Sharing with the team needs a veterinarian. Ask one to approve with their PIN. |
| Approver isn't a veterinarian | Only a veterinarian can approve this. |
| Reset with nothing to reset | You're already using the original. |
| Locked field | {field} is fixed for this visit type. Duplicate the template to change it. |
| Fixed rule edited | Fixed rules can't be changed. They come from the original template. |
| Old request body | Send {template, by}. |

## Order and rough size

| Steps | What | Size |
|---|---|---|
| 0 to 2 | Harness, fingerprints, new fields | 1.5 days |
| 3 to 5 | Layered store, saving, reset and undo | 3 days |
| 6 and 7 | Original changed, comparison, scoped standards | 2 days |
| 8 | Safety floor, built-in split, eval | 2 days, plus eval run time |
| 9 and 10 | Drafting and run records, roles and approvals | 2.5 days |
| 11 and 12 | API, Tauri, web routes, CLI | 1.5 days |
| 13 to 17 | Interface | 5 days |
| 18 | Translations | 1 day |
| 19 to 21 | End-to-end, screenshots, docs | 2.5 days |

Phase 2, not in this plan: bringing in one change at a time from **Compare**; browsing history beyond Undo; team-locked rules; sync of templates (M3 phase 5, ADR-0024 section 15).

## Risks

| Risk | Mitigation |
|---|---|
| A personal instruction pushes the model to invent | Lint while typing, the rules that always apply first in every prompt, fixed rules from the current original, and the checks after drafting on every draft. The hostile-instruction eval in step 8 |
| The precedence sentence or the rule split lowers eval scores | Step 8's bar, with a second placement tried and both runs recorded |
| A test touches real templates or profiles | Step 0's harness, profiles derived from the team file at call time, and the e2e script's four checks. Port 4317 is never a target |
| History can't be decrypted after a move | History is always written as new files with `vault::write`, never renamed. A unit test decrypts every history entry |
| The desktop app and `openbark serve` save the same profile at once | The `base_version` check refuses the second save and keeps the person's changes on screen |
| A PIN reset by remove-and-re-add deletes someone's versions | **Reset PIN** in Settings, the guide changed, and the removal confirmation states the count |
| Technicians lose the ability to save team standards directly | **For me** covers personal preferences, and a veterinarian can approve in place. The release notes say so |
| People can't tell which version drafted a note | Labels in Review's draft history, and **Show the template** opens the snapshot |
| A built-in changes without a version bump, so nobody is told | The fingerprint test in step 1 fails the build |
| Translated headings confuse what notes will show | Inputs show the stored text, the translation is a hint, and adopting it is a deliberate action |
| An id collision between a private template and a team template | New ids avoid every id on this computer. The sync case is tested in M3 |
| The new `os-app` tests hit the Linux `bad_alloc` | Run them in WSL, and keep them independent of model loading |
