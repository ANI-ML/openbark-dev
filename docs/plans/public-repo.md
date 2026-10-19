# Plan: the public source repository

Status: **prepared, not published.** Date: 2026-10-18. Last export: commit `17f2195` of `main`.

OpenBark's public source repository starts from **one clean commit**, built from this repository's committed tree by `scripts/public-export.py`. This repository and its history stay private. The public tree leaves out the AI working files, carries no co-author lines, names no individuals, and credits only "the ANI.ML Health team".

This file is private process documentation. The export leaves it out, together with the export script and its config.

## Outcome

- The export runs end to end and **the gate passes**: no forbidden words, no personal details, no "the repo is private" claims, and no secrets (gitleaks 8.30.1 found nothing, and the built-in secret patterns found nothing).
- **Three blockers remain**, so the export is not ready to publish (the script exits with code 3): the help videos, the public repo's name, and the public repo's role. See [Blockers](#blockers).
- 456 committed files were exported, 9 left out, 447 kept (382 text, 65 binary), 6.59 MB in total. `git init` and `git add -A` in the export folder stage all 447 files, ignore none, and there is no remote and no commit.

## How the export works

`scripts/public-export.py` (Python 3.11+, standard library only) does five things, in order:

1. **Archive.** `git archive <ref>` (default `HEAD`) into an empty folder, so untracked and ignored files are never exported. Line endings follow `.gitattributes`.
2. **Exclude.** Removes the files listed under `[[exclude]]` and under any `[[blocker]]` with `action = "exclude"`.
3. **Scrub.** Applies the reviewed `[[scrub]]` rules: exact phrases (or a few anchored regular expressions) replaced in the exported copy only. **Every rule must still match** (at least once, or exactly `expect` times). If the source changed and a rule no longer matches, the export stops with a configuration error, so the change gets reviewed instead of slipping through.
4. **Rebuild.** Re-runs the generators inside the export folder (`[[rebuild]]`: `scripts/website-build.py`), so `website/dist` and `CHANGELOG.md` match the scrubbed sources.
5. **Gate.** Scans every exported file, by name and content, text and binary, against the `[gate]` patterns, runs gitleaks when it is available plus the built-in secret patterns in every case, and checks that the files a public repo needs are present. Optionally (`--git-init`) runs `git init` and `git add -A` inside the export folder only.

Every rule lives in **`scripts/public-export.toml`**, with a reason for each. That config is the one file that spells out the forbidden words, because the gate has to search for them; it is private tooling and is excluded from the export.

**Safety.** The script never writes inside the repository. It refuses an output folder inside the repository or one that contains it, and it only replaces an existing output folder when `<folder>.report.json` beside it shows that a previous export made it. All of these were tested.

### Running it

```bash
python scripts/public-export.py --out ../openbark-public --git-init          # first time
python scripts/public-export.py --out ../openbark-public --git-init --force  # replace a previous export
python scripts/public-export.py --out ../openbark-public --ref <commit>      # export a specific commit
```

Exit codes: **0** gate passed and no blockers (ready to publish); **3** gate passed but blockers remain; **1** gate failed; **2** configuration error, including a scrub rule that no longer matches. Each run writes a full JSON report beside the folder (`../openbark-public.report.json`): the excluded files, every replacement by file, every gate finding, the blockers and the file statistics.

The gitleaks location is configured under `[gate] gitleaks` in the config; if it isn't found, `gitleaks` on `PATH` is used, and if neither exists the built-in secret patterns still run.

## What is excluded, and why

The exact names and patterns are under `[[exclude]]` and `[[blocker]]` in `scripts/public-export.toml`.

| What | Files in this export | Why |
|---|---|---|
| The AI coding assistant's working-notes file at the repository root | 1 | Internal working notes for the coding tool, and its name names the tool. |
| The original product brief at the repository root | 1 | Written as a prompt for the coding tool. Contains open decisions for the owner and a competitor feature and pricing comparison. |
| The seven in-app Help videos, `apps/desktop/public/help/*.webm` | 7 | **Blocked:** the recordings show local folder paths that include a Windows username. Left out until re-recorded. |
| AI assistant settings and instruction folders, scratch files, logs, editor and merge leftovers, Python caches, `.env` files and key material | 0 | Guards: nothing like this is committed today, but the rules stay in case something is. |
| This export tooling: `scripts/public-export.py`, `scripts/public-export.toml` and this file | 0 (not committed yet) | Private process documents; the config spells out the forbidden words. |

Leaving out the Help videos removes `apps/desktop/public` entirely, since it holds nothing else. The app still builds; the Help screen shows empty players until the videos are back.

## Text scrubs

249 rules made 295 replacements in 52 files. All of them are in the appendix, with what each one changes and why. In short:

- **AI working traces.** The original brief was called the "master prompt"; every mention becomes "product brief" (32 places, including two Rust doc comments). References to the excluded working-notes file, an instruction addressed to the coding tool by name in `docs/design/DESIGN.md`, mentions of AI design "skills" and "agents", an assistant speaking in the first person to the owner ("I'll install these", "the GitHub connector here isn't authorised", "paste it into the chat"), and verbatim quotes of the owner's requests to the coding tool are reworded or removed. The "Original decision notes" section of `docs/plans/M0.md` is removed: it was a conversation between the assistant and the owner, named third parties' personal GitHub accounts and held trademark research. The resolved table above it keeps every outcome.
- **Personal details.** The owner's personal email address (four places), the owner's iPhone, Mac, mobile number, accounts and real data, the owner's games and streaming during benchmarks, and owner to-do lists become neutral wording ("a test iPhone", "a development machine", "Setup still needed: ...").
- **Internal company matters.** Another ANI.ML product, the internal Supabase project identifier (11 places), budget and "new spend" lines, the GitHub billing problem, "spun out" wording and a planned trademark search.
- **Private-repo claims.** Every "the repo is private" line becomes neutral wording, and `website/README.md` no longer says the main repo stays private.
- **The website copy in the export.** `scripts/website-build.py` gets `SOURCE_URL = "https://github.com/ANI-ML/openbark"`, the landing-page template keeps only its "source public" text, the "source will be public" feature becomes "shipped", and the site is rebuilt. The public copy therefore says "open source" and links to its own source instead of "isn't public yet". The live site is switched separately (publish step 9).
- **A real bug.** `README.md` has an editing instruction pasted inside the build commands ("Add `pnpm --filter ...` before the serve line, with the comment: ..."). The export replaces it with the two commands from `CONTRIBUTING.md`. **Fix it in this repository too.**

Facts are never invented: rewordings keep every technical fact and drop only the personal or internal part. Where a sentence's only content was internal (a to-do item, a quote), it is removed.

## The final gate

Every exported file is checked, by name and content, in text and in binary form. Any match fails the export.

| Pattern | What it catches |
|---|---|
| assistant-vendor | The AI tool's name and its vendor's name, case-insensitive. **No technical context needs either word**: the app uses Gemma, Parakeet and Whisper, no dependency carries either name, and no allow rule is defined. |
| owner-surname, owner-first-name, owner-email | The owner's name and personal email address. |
| windows-username | The local Windows account name of the development machine. |
| co-author-trailer | Co-author commit trailers. |
| assistant-scratch | The assistant's scratch folder names. |
| assistant-brief | References to the AI brief or tool by its working names. |
| internal-projects | Other ANI.ML products and internal projects. |
| supabase-project-ref | The internal Supabase project identifier. |
| local-user-path | Real local user folders. Placeholders like `<other user>` and the invented `/home/vet` test path are allowed. |
| private-repo-claim | "The repo is private", "private until launch", "isn't public yet", "will be public on GitHub" and similar. |
| owner-personal | The owner's devices, to-do lists and private-conversation phrasing ("still to do by the owner", "your OK", "owner decision"). |
| billing | "Billing problem", "needs budget", "new spend", "already paid". |
| required files | `LICENSE`, `NOTICE`, `README.md`, `SECURITY.md`, `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`, `CHANGELOG.md`, `.gitignore` and `.gitattributes` must exist. |
| secrets | gitleaks (`gitleaks dir`, run from a temporary folder with inline allow comments ignored, and refused if the tree contains its own gitleaks config), plus built-in patterns for AWS, GitHub, Slack, Google, Stripe, SendGrid, Twilio, Hugging Face and OpenAI-style keys, JWTs, private-key blocks and literal credentials. |

Two warning patterns are listed for a human look but don't fail: "product owner" (31 remaining mentions, all of them the role in ADR deciders lines, approvals and requirements) and first-person assistant phrasing (none left). "Owner" on its own is not flagged, because it almost always means the animal's owner.

Third-party names that stay on purpose: the pyannote credit "CNRS (Herve Bredin)" in `NOTICE` (a licence attribution), upstream model and library repositories in `docs/versions.md`, and the invented patient, client and clinician names in the fixtures and UI examples.

## Blockers

The export leaves blocked files out, and lists the rest. While any blocker is in the config the script exits with code 3.

1. **Help videos (`help-videos`).** All seven Help videos show local folder paths that include a Windows username. Re-record them with `apps/desktop/e2e/help-videos.mjs` using neutral data folders (for example `OPENBARK_SESSIONS_DIR=C:\OpenBark-demo\visits` and `OPENBARK_MODELS_DIR=C:\OpenBark-demo\models`), scan every frame, commit them, then delete the blocker so they are exported again.
2. **The public repo's name (`public-repo-name`).** Owner decision: rename this repository (for example to `openbark-dev`) so the public one can be `ANI-ML/openbark`, or give the public one a new name. Links to `github.com/ANI-ML/openbark` appear in `SECURITY.md`, `.github/ISSUE_TEMPLATE`, `Cargo.toml`, `apps/desktop/src-tauri/tauri.conf.json`, `docs/runbooks/code-signing.md`, `apps/desktop/e2e/website.mjs` and the export's `SOURCE_URL`. If the name differs, add scrub rules for those links, or change them here first.
3. **The public repo's role (`public-repo-role`).** Owner decision: is the public repo a one-time snapshot, a periodic export from here, or the new home of development (issues, pull requests, CI and releases)? Several files assume releases are built from this private repo: `.github/workflows/release.yml` and `website-release.yml` (the `PUBLIC_RELEASES_TOKEN` secret and the releases repo), `docs/adr/0020-code-signing.md` and `docs/runbooks/code-signing.md` (signing secrets in an environment on `ANI-ML/openbark`), and `website/README.md` (Vercel imports `ANI-ML/openbark`). In a public repo without those secrets, the release workflows fail when a tag is pushed. Reword those files here once the answer is known.

## Owner decisions and follow-ups (not blocking)

- **Trademark search.** The plans said a formal trademark search on "OpenBark" should happen before the public launch. The export removes that internal to-do; the search itself is still open.
- **"Product brief" citations.** About 60 places (docs, `website/content/features.json` evidence, two code comments) cite sections of the product brief, which is not published. Leave them, publish a cleaned brief, or reword them here.
- **Commit hashes in the features list.** 15 `evidence` fields in `website/content/features.json` cite commits (for example `3888f20`) that won't exist in the public history. Consider citing files instead.
- **An out-of-date feature fact.** `website/content/features.json` still says there is "no delete-visit function", but deleting visits (phase 1) has shipped. Fix it here.
- **The README build block** (see Text scrubs). Fix it here.
- **Social image.** `website/public/og-image.png` still has the "free" wording. Re-render it with `SOURCE_PUBLIC=1 node e2e/brand-images.mjs --og` when the live site is switched.
- **The account portal (in progress, not committed at this export).** It puts the Supabase project URL and its publishable key into `website/src/portal.js`, `website/dist`, `scripts/website-build.py`, `docs/network.md` and `supabase/`. A publishable key is public by design, since every visitor's browser receives it, but once that work is committed the gate will flag the project identifier (and possibly the key as a JWT or token). Decide whether it is fine to publish, then add reviewed `[[gate.allow]]` rules for those files, and retire the scrub rules that hide the identifier in `docs/network.md`, ADR-0017 and M3 (they will stop matching, which stops the export until they are updated).
- **`SECURITY.md` response targets** (five and ten business days) are still to be confirmed.
- **Publisher name.** `apps/desktop/src-tauri/tauri.conf.json` says "ANI-ML", while `NOTICE` says "ANI.ML Health Inc.".
- **`scripts/bench.sh`** names its results folder after the computer's host name, which can put a contributor's machine name into the repo, and downloads fixture audio from a `fixtures-v1` release that a fresh public repo won't have unless it is recreated (or regenerated with `tools/fixture-tts`).
- **`.gitignore`** has no `__pycache__/` entry, although the repo has Python scripts.
- **Same fixes here.** Most scrubs are improvements this repository could take as well (owner to-dos, quotes, the private-repo lines). Applying them here makes the scrub list shorter and the export easier to keep up to date.

## Export size and file list

| Folder | Files | Size |
|---|---|---|
| (root) | 19 | 0.29 MB |
| `.github` | 9 | 0.02 MB |
| `apps` | 148 | 1.50 MB |
| `docs` | 75 | 0.89 MB |
| `engine` | 62 | 0.81 MB |
| `install` | 11 | 0.04 MB |
| `packages` | 89 | 2.39 MB |
| `scripts` | 11 | 0.05 MB |
| `tools` | 2 | 0.01 MB |
| `website` | 21 | 0.61 MB |
| **Total** | **447** (382 text, 65 binary) | **6.59 MB** |

The largest file is the CI smoke clip `packages/eval/fixtures/audio/smoke.wav` (1.6 MB). With the Help videos back, the tree grows by about 26 MB.

## Checks run on the exported tree

- **README** reads correctly for the public: "free, open-source", the ANI.ML Health team credit, beta caveats, and no "private" claims. The build block now has real commands.
- **`LICENSE`, `NOTICE`, `SECURITY.md`, `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`, `CHANGELOG.md`** are present. `NOTICE` credits "ANI.ML Health Inc. and OpenBark contributors" and "the ANI.ML Health team".
- **Code is unchanged except comments.** Compared with the archived commit, the only code changes are two Rust doc comments, comments in `scripts/bench.sh` (an echo line), `scripts/macos-dmg.sh`, `apps/desktop/e2e/brand-images.mjs` and `.github/workflows/release.yml`, and in `scripts/website-build.py` the `SOURCE_URL` value and a comment. The changed scripts pass `python -m py_compile`, `bash -n` and `node --check`, and `website/content/features.json` is valid JSON.
- **Website.** The rebuild in the export succeeded. Against this repository's `website/dist`, the only differences are the intended ones: "open-source" wording, the Source code links, and the scrubbed feature text. `CHANGELOG.md` is byte-identical.
- **`git init` and `git add -A`** inside the export folder stage all 447 files; `.gitignore` drops none, there is no remote, and nothing is committed.

## Publish steps (for later)

Do these only after the blockers are cleared and the owner says go. None of this has been done.

1. **Clear the blockers.** Re-record and commit the Help videos, decide the name and the role, update the config to match, and delete the three `[[blocker]]` entries.
2. **Name.** If the public repo is to be `ANI-ML/openbark`, rename this repository first (for example to `ANI-ML/openbark-dev`; GitHub redirects the old URL until a new repo takes the name). Update this repository's remote and any secrets or webhooks that name it (Vercel's Git connection, `PUBLIC_RELEASES_TOKEN`).
3. **Create the public repo** in the ANI-ML organisation, empty (no README, licence or `.gitignore` from GitHub), with **private** visibility for now.
4. **Turn on protections before the first push** (Settings, then Code security, or Advanced Security): secret scanning, **push protection**, private vulnerability reporting (`SECURITY.md` relies on it), Dependabot alerts and Dependabot security updates. `.github/dependabot.yml` is in the tree for version updates.
5. **Export and check.** `python scripts/public-export.py --out ../openbark-public --git-init --force` must end with `RESULT: gate passed` and exit code 0. Read through `README.md` and skim the JSON report.
6. **Make the single commit as the company.** In `../openbark-public` only:
   ```bash
   git config user.name "ANI.ML Health"
   git config user.email "we.care@animl.health"       # or the company's GitHub noreply address
   git commit -m "OpenBark: first public release"     # signed with a company key, if commits are signed
   git log --format='%an <%ae> | %cn <%ce>%n%B'        # check: only the company identity, no trailers
   git rev-list --count HEAD                           # check: 1
   ```
   This computer has no global git identity, so these local settings decide the author. To keep AI working files out of future commits without naming them in `.gitignore`, list them in `.git/info/exclude` of this clone.
7. **Push.** `git remote add origin https://github.com/ANI-ML/<name>.git`, then `git push -u origin main`. Never push this private repository's history there, and never make this repository public.
8. **Protect `main`** (a branch ruleset): require a pull request with one approval, require the CI status checks to pass, block force pushes and deletion, and require linear history (signed commits optional). Set the description, the homepage `https://www.openbark.io`, topics, and the social preview image (`docs/brand/images/github-social-preview.png`). Then switch the visibility to **public**.
9. **Switch the live website.** In this repository: set `SOURCE_URL` in `scripts/website-build.py` to the public repo's URL, run `python scripts/website-build.py`, re-render the social image (`cd apps/desktop && SOURCE_PUBLIC=1 node e2e/brand-images.mjs --og`), rebuild so `website/dist` gets it, commit and push to `main`. Vercel deploys it. Dropping the template's "source private" branches (as the export does) is worth doing at the same time.
10. **Check from outside.** Open the public repo signed out: the README renders, the licence is detected, and the Security tab offers "Report a vulnerability". Search the public repo with GitHub code search for the gate words. Confirm secret scanning shows no alerts, and that the website's Source code links open the repo.

### Later updates

If the public repo stays an export (blocker 3), update it by exporting again into a fresh folder, copying the result over a clone of the public repo (everything except `.git`), and committing as the company, as in step 6. Every scrub rule must still match, so source changes that touch a scrubbed line stop the export until the rule is reviewed. If development moves to the public repo instead, the export is used once and this repository is archived.

## Appendix: every scrub rule

Rules run in the order they appear in the config, global rules first; they are grouped by file here. "Changes" is the number of replacements in the last export. The exact text of each rule is in `scripts/public-export.toml` under the same id.

**Global**

| Rule | Changes | Why |
|---|---|---|
| `global-master-prompt` | 32 (docs/adr/0001-inference-libraries.md: 2, docs/adr/0004-monorepo-and-tooling.md: 1, docs/adr/0005-optional-cloud-sync.md: 2, docs/adr/0018-iphone-app.md: 2, docs/network.md: 1, docs/perf/M0.md: 2, docs/plans/M0.md: 9, docs/spikes/S1-mobile-audio-and-inference.md: 4, docs/spikes/S2-native-linking.md: 4, docs/versions.md: 2, engine/os-engine/src/faithfulness.rs: 1, engine/os-llm/src/lib.rs: 1, website/content/features.json: 1) | The 'master prompt' is the original product brief written for the AI coding assistant (excluded above). Neutral name for the same document. |
| `global-master-prompt-capital` | 1 (docs/plans/M0.md: 1) | As global-master-prompt, sentence case. |

**.github/workflows/release.yml**

| Rule | Changes | Why |
|---|---|---|
| `release-yml-private` | 1 | Private-repo claim in a comment. |

**README.md**

| Rule | Changes | Why |
|---|---|---|
| `readme-build-line` | 1 | An editing instruction was left inside the build commands instead of being applied. Replaced with the two commands from CONTRIBUTING.md's Building section. Fix the private README too. |
| `readme-history-commits` | 1 | CHANGELOG.md lists no commits, and the public repo has fresh history, so private commit hashes mean nothing there. |

**apps/desktop/e2e/brand-images.mjs**

| Rule | Changes | Why |
|---|---|---|
| `brand-images-comment` | 1 | Comment says the code is not public. |

**docs/adr/0006-templates-from-headings.md**

| Rule | Changes | Why |
|---|---|---|
| `adr0006-deciders` | 1 | Verbatim quote of the owner's request to the coding assistant; paraphrased. |

**docs/adr/0007-phone-calls.md**

| Rule | Changes | Why |
|---|---|---|
| `adr0007-quote` | 1 | Verbatim quote of the owner's request to the coding assistant; paraphrased. |
| `adr0007-local-first-ref` | 1 | Cites the excluded assistant working-notes file. |
| `adr0007-open-questions` | 1 | Owner-addressed heading. |
| `adr0007-i-suggest` | 1 | Assistant first-person voice addressed to the owner. |

**docs/adr/0008-encryption-at-rest.md**

| Rule | Changes | Why |
|---|---|---|
| `adr0008-owner-asked` | 1 | Private-conversation phrasing; the requirement is kept. |

**docs/adr/0009-mlx-on-mac.md**

| Rule | Changes | Why |
|---|---|---|
| `adr0009-deciders` | 1 | Private-conversation phrasing in the deciders line. |

**docs/adr/0010-apple-style-refresh.md**

| Rule | Changes | Why |
|---|---|---|
| `adr0010-deciders` | 1 | Private-conversation phrasing; the requirement is kept. |
| `adr0010-owners-design-system` | 1 | Attributes the design system to an individual. |
| `adr0010-owner-asked` | 1 | Private-conversation phrasing. |
| `adr0010-alt` | 1 | Private-conversation phrasing. |

**docs/adr/0011-patients-and-visits.md**

| Rule | Changes | Why |
|---|---|---|
| `adr0011-deciders` | 1 | Verbatim quote of the owner's request to the coding assistant. |
| `adr0011-owner-asked` | 1 | Private-conversation phrasing. |

**docs/adr/0012-point-and-click-installers.md**

| Rule | Changes | Why |
|---|---|---|
| `adr0012-deciders` | 1 | Private-conversation phrasing in the deciders line. |
| `adr0012-buying` | 1 | An internal spending decision; ADR-0020 is where signing is designed. |
| `adr0012-real-data` | 1 | Says a developer machine holds real clinic data. |

**docs/adr/0013-choose-where-data-is-kept.md**

| Rule | Changes | Why |
|---|---|---|
| `adr0013-deciders` | 1 | Private-conversation phrasing in the deciders line. |

**docs/adr/0014-gpu-speech-recognition.md**

| Rule | Changes | Why |
|---|---|---|
| `adr0014-status` | 1 | Owner to-do. |
| `adr0014-deciders` | 1 | Private-conversation phrasing in the deciders line. |

**docs/adr/0015-geist-and-animl-orange.md**

| Rule | Changes | Why |
|---|---|---|
| `adr0015-deciders` | 1 | 'Design skills' are AI-assistant skills. |
| `adr0015-skills-review` | 1 | Names AI-assistant skills. |
| `adr0015-both-skills` | 1 | Refers to AI-assistant skills. |
| `adr0015-skills-layout` | 1 | Refers to AI-assistant skills. |
| `adr0015-skills-suggest` | 1 | Refers to AI-assistant skills. |
| `adr0015-spin-out` | 1 | Internal company matter. |

**docs/adr/0016-brand-guide.md**

| Rule | Changes | Why |
|---|---|---|
| `adr0016-deciders` | 1 | Verbatim quote of the owner's message to the coding assistant. |
| `adr0016-taste-skill` | 1 | Refers to an AI-assistant skill and to the owner personally. |
| `adr0016-alt-brand` | 1 | Owner-personal phrasing. |
| `adr0016-alt-icons` | 1 | Owner-personal phrasing. |

**docs/adr/0017-accounts-and-encrypted-sync.md**

| Rule | Changes | Why |
|---|---|---|
| `adr0017-status` | 1 | Owner-personal phrasing. |
| `adr0017-deciders` | 1 | Private-conversation phrasing; the requirement is restated in Context. |
| `adr0017-decisions-heading` | 1 | Owner-personal phrasing. |
| `adr0017-decision1` | 1 | Owner's personal email, owner hardware, billing status and owner errands. |
| `adr0017-decision4` | 1 | Another ANI.ML product, and an owner errand. |
| `adr0017-decision5` | 1 | Internal project identifier, owner errand and a spending plan. |
| `adr0017-accepted-default` | 1 | Owner-personal phrasing. |
| `adr0017-owner-todo` | 1 | Owner to-do list (the personal membership item is dropped). |
| `adr0017-quote` | 1 | Verbatim quote of the owner's request to the coding assistant; paraphrased. |
| `adr0017-owner-decided` | 1 | Owner-personal phrasing. |
| `adr0017-service-ref` | 1 | Internal project identifier. |
| `adr0017-email-row` | 1 | Another ANI.ML product, and an owner errand. |
| `adr0017-owner-decision-212` | 1 | Owner-personal phrasing. |
| `adr0017-projects` | 1 | Internal project identifier, owner errand and a spending plan. |
| `adr0017-email-setup` | 1 | Owner errand. |
| `adr0017-cost` | 1 | Company spending plan and another ANI.ML product. |
| `adr0017-housekeeping` | 1 | Concerns the excluded assistant working-notes file only. |
| `adr0017-alt-audio` | 1 | Owner-personal phrasing. |

**docs/adr/0018-iphone-app.md**

| Rule | Changes | Why |
|---|---|---|
| `adr0018-status` | 1 | Owner hardware. |
| `adr0018-deciders` | 1 | Verbatim quote of the owner's request to the coding assistant. |
| `adr0018-owner-decisions` | 1 | Owner hardware. |
| `adr0018-context-facts` | 1 | Billing status, private-repo claim, owner hardware and the owner's personal email. |
| `adr0018-s1-gates` | 1 | Owner hardware. |
| `adr0018-decision-154` | 1 | Billing status and owner hardware. |
| `adr0018-s1-row` | 1 | Owner errand. |
| `adr0018-cost` | 1 | Private-repo billing detail. |
| `adr0018-secrets` | 1 | Owner errand. |
| `adr0018-openssl` | 1 | Owner errand. |
| `adr0018-seller` | 1 | The owner's personal account. |
| `adr0018-bottleneck` | 1 | Owner errand and billing. |
| `adr0018-alt-record-only` | 1 | Owner-personal phrasing. |
| `adr0018-alt-mac` | 1 | Owner hardware. |
| `adr0018-owner-decision-paren` | 2 | Owner-personal phrasing (two places). |

**docs/adr/0019-acceleration-on-every-platform.md**

| Rule | Changes | Why |
|---|---|---|
| `adr0019-deciders` | 1 | Private-conversation phrasing; the requirement is kept. |
| `adr0019-owner-4080` | 1 | Owner hardware. |
| `adr0019-owner-case` | 1 | Owner-personal phrasing. |
| `adr0019-owner-problem` | 1 | Owner-personal phrasing. |

**docs/adr/0020-code-signing.md**

| Rule | Changes | Why |
|---|---|---|
| `adr0020-deciders` | 1 | Private-conversation phrasing. |
| `adr0020-apple-membership` | 1 | The owner's personal account and spending. |
| `adr0020-private-repo` | 1 | Private-repo claim and an internal account question; the GitHub facts are kept. |
| `adr0020-individual` | 1 | The owner's personal account status. |
| `adr0020-account-holder` | 1 | Owner errand. |
| `adr0020-mac-cost` | 1 | Company spending status. |
| `adr0020-ios-cost` | 1 | Company spending status. |
| `adr0020-app-record` | 1 | The owner's personal account status. |
| `adr0020-env-private` | 1 | Private-repo plan detail. |
| `adr0020-password-manager` | 1 | Internal open item. |
| `adr0020-who-holds` | 1 | Owner-personal role assignment. |
| `adr0020-risk-envs` | 1 | Private-repo plan detail. |
| `adr0020-risk-convert` | 1 | The owner's personal account status. |
| `adr0020-risk-enrol` | 1 | The owner's personal account. |
| `adr0020-cost-summary` | 1 | Company spending status ('new spend', 'already paid'); public list prices are kept. |
| `adr0020-eligibility` | 1 | Internal company detail. |
| `adr0020-alt-signpath` | 1 | Private-repo claim. |
| `adr0020-alt-attestations` | 1 | Private-repo claim. |
| `adr0020-alt-cosign` | 1 | Private-repo claim. |

**docs/adr/0021-dental-chart.md**

| Rule | Changes | Why |
|---|---|---|
| `adr0021-deciders` | 1 | Verbatim quote of the owner's instruction to the coding assistant. |
| `adr0021-access-refused` | 1 | Reads as an automated fetch-tool error. |

**docs/adr/0022-deleting-visits.md**

| Rule | Changes | Why |
|---|---|---|
| `adr0022-status` | 1 | Owner-personal phrasing. |
| `adr0022-deciders` | 1 | Verbatim quote of the owner's instruction to the coding assistant. |

**docs/adr/0023-bring-your-own-telephony.md**

| Rule | Changes | Why |
|---|---|---|
| `adr0023-status` | 1 | Owner-personal phrasing. |
| `adr0023-deciders` | 1 | Verbatim quote of the owner's request to the coding assistant; the requirement is restated in Context. |
| `adr0023-context` | 1 | Owner-personal phrasing. |
| `adr0023-context-sendgrid` | 1 | Owner-personal phrasing. |
| `adr0023-voiceprint` | 1 | Owner-addressed phrasing. |
| `adr0023-dev-box` | 1 | Owner hardware. |
| `adr0023-open-questions` | 1 | Owner-addressed heading (the section number is cited elsewhere and is kept). |
| `adr0023-owner-decisions` | 1 | Owner-personal phrasing. |
| `adr0023-dev-account` | 1 | Owner errand. |
| `adr0023-own-account` | 1 | The owner's personal account. |
| `adr0023-alt-relay` | 1 | Owner-personal phrasing; the requirement is stated in Context. |
| `adr0023-alt-smtp` | 1 | Owner-personal phrasing. |

**docs/adr/0024-per-user-template-editing.md**

| Rule | Changes | Why |
|---|---|---|
| `adr0024-status` | 1 | Owner-personal phrasing. |
| `adr0024-deciders` | 1 | Verbatim quote of the owner's request to the coding assistant; the requirement is restated in Context. |
| `adr0024-what-was-asked` | 1 | Private-conversation phrasing. |
| `adr0024-owner-wants` | 1 | Owner-personal phrasing. |
| `adr0024-standing-rules` | 1 | Refers to the excluded assistant working-notes file. |
| `adr0024-decisions-heading` | 1 | Owner-addressed heading (section number kept; it is cited). |
| `adr0024-owner-decisions` | 1 | Owner-personal phrasing. |

**docs/design/DESIGN.md**

| Rule | Changes | Why |
|---|---|---|
| `design-provided-by` | 1 | Owner-personal phrasing. |
| `design-amended` | 1 | Owner-personal phrasing. |
| `design-assistant-instruction` | 1 | Instruction addressed to the AI coding assistant by name. |

**docs/network.md**

| Rule | Changes | Why |
|---|---|---|
| `network-supabase-ref` | 1 | Internal project identifier for a feature not built yet. When sync ships, the real host must be listed here. |

**docs/perf/M0.md**

| Rule | Changes | Why |
|---|---|---|
| `perf-shared-gpu` | 1 | Personal software use on the owner's machine. |
| `perf-owner-mac` | 1 | Owner hardware. |

**docs/plans/M0.md**

| Rule | Changes | Why |
|---|---|---|
| `m0-status` | 1 | Private-repo claim. |
| `m0-your-ok` | 1 | Assistant addressing the owner. |
| `m0-notes-file-item` | 1 | Refers to the excluded assistant working-notes file. |
| `m0-day0` | 1 | Refers to the excluded assistant working-notes file. |
| `m0-this-box-day5` | 1 | Owner hardware as 'this box'. |
| `m0-dev-env` | 1 | Owner's machine inventory, install to-do list and assistant first-person voice ('I'll install these ... I'll ask'). Facts are kept. |
| `m0-this-box-m01` | 1 | Owner hardware as 'this box'. |
| `m0-signed-off` | 1 | Assistant addressing the owner. |
| `m0-this-box-risk` | 1 | Owner hardware as 'this box'. |
| `m0-name-risk` | 1 | Pre-launch internal to-do. |
| `m0-name-decision` | 1 | Internal trademark research and a legal to-do (listed as an owner item in docs/plans/public-repo.md). |
| `m0-apple-hardware` | 1 | Owner hardware. |
| `m0-github-private` | 1 | Private-repo claim. |
| `m0-asked` | 3 | Private-conversation phrasing (three rows). |
| `m0-original-notes` | 1 | Assistant-to-owner conversation (first-person questions, 'the GitHub connector here isn't authorised'), third-party personal GitHub accounts and trademark research. The resolved table above records every outcome. |

**docs/plans/M1.md**

| Rule | Changes | Why |
|---|---|---|
| `m1-status` | 1 | Verbatim quote of the owner's instruction to the coding assistant. |
| `m1-consent` | 1 | Private-conversation phrasing. |
| `m1-copy` | 1 | Private-conversation phrasing. |
| `m1-login` | 1 | Private-conversation phrasing. |
| `m1-mac-bench` | 1 | Owner hardware. |
| `m1-iphone` | 1 | Owner hardware. |

**docs/plans/M2.md**

| Rule | Changes | Why |
|---|---|---|
| `m2-keep-going` | 1 | Verbatim quote of the owner's instruction to the coding assistant. |
| `m2-decisions` | 1 | Owner-addressed heading. |

**docs/plans/M3.md**

| Rule | Changes | Why |
|---|---|---|
| `m3-status` | 1 | Owner-personal phrasing. |
| `m3-decisions-heading` | 1 | Owner-personal phrasing. |
| `m3-decision1` | 1 | Owner's personal email, owner hardware, billing status and owner errands. |
| `m3-decision4` | 1 | Another ANI.ML product, and an owner errand. |
| `m3-decision5` | 1 | Internal project identifier and a spending plan. |
| `m3-accepted-default` | 1 | Owner-personal phrasing. |
| `m3-owner-todo` | 1 | Owner to-do list (the personal membership item is dropped). |
| `m3-mac-row` | 1 | Owner hardware and private-repo billing. |
| `m3-apple-row` | 1 | The owner's personal email and account. |
| `m3-sbdev-row` | 1 | Internal project identifier. |
| `m3-sbprod-row` | 1 | Budget, internal project identifier and an owner errand. |
| `m3-gh-row` | 1 | Billing status. |
| `m3-notes-file` | 1 | Refers to the excluded assistant working-notes file. |
| `m3-no-owner-mac` | 1 | Owner hardware. |
| `m3-installs` | 1 | Owner hardware and errand. |
| `m3-prereq` | 1 | Owner errand. |
| `m3-real-server` | 1 | Internal project identifier. |
| `m3-service` | 1 | Internal project identifier. |
| `m3-smtp` | 1 | Owner errand. |
| `m3-owners-iphone` | 1 | Owner hardware. |
| `m3-testflight` | 1 | Owner-personal. |
| `m3-dropped` | 1 | Owner-personal phrasing. |
| `m3-not-in-m3` | 1 | Owner-personal phrasing. |
| `m3-phase1` | 1 | Owner errand. |
| `m3-bottleneck` | 1 | Private-repo billing, owner hardware and errands. |

**docs/plans/acceleration.md**

| Rule | Changes | Why |
|---|---|---|
| `accel-owner-case` | 1 | Owner-personal phrasing. |
| `accel-shown-owner` | 1 | Owner-personal phrasing. |

**docs/plans/deleting-visits.md**

| Rule | Changes | Why |
|---|---|---|
| `delete-real-data` | 1 | Owner's machine and real data. |

**docs/plans/dental-chart.md**

| Rule | Changes | Why |
|---|---|---|
| `dental-demo-row` | 1 | Points at the owner's real instance. |
| `dental-never-4317` | 1 | Points at the owner's real instance. |
| `dental-approves` | 1 | Owner-personal phrasing. |

**docs/plans/telephony.md**

| Rule | Changes | Why |
|---|---|---|
| `tel-win` | 1 | Owner hardware. |
| `tel-trial` | 1 | Owner errand. |
| `tel-upgraded` | 1 | The owner's personal account. |
| `tel-owners-mobile` | 4 | The owner's personal phone (several places). |
| `tel-owners-mobile-cap` | 1 | The owner's personal phone. |
| `tel-sg` | 1 | Owner errand. |
| `tel-order` | 1 | Owner-personal phrasing. |
| `tel-1b` | 1 | Owner errand. |
| `tel-go-back` | 1 | Owner-personal phrasing. |
| `tel-owners-number` | 1 | The owner's personal number. |
| `tel-dev-box-mic` | 1 | Owner hardware. |
| `tel-dev-box-second` | 1 | Owner hardware. |
| `tel-owners-address` | 1 | The owner's personal email. |
| `tel-incoming` | 1 | Owner-personal phrasing. |
| `tel-owner-checklist` | 1 | The owner's personal to-do list (own mobile, own spending, borrow a Mac). Kept as a neutral checklist with the same items. |

**docs/plans/template-editing.md**

| Rule | Changes | Why |
|---|---|---|
| `tpl-status` | 1 | Owner-personal phrasing. The ADR records the decisions; the plan predates them. |
| `tpl-never-4317` | 1 | Owner's machine and real data. |
| `tpl-eval-row` | 1 | Owner hardware. |
| `tpl-owner-decides` | 1 | Owner-personal phrasing. |

**docs/runbooks/code-signing.md**

| Rule | Changes | Why |
|---|---|---|
| `sign-audience` | 1 | Owner-personal audience. |
| `sign-cost-row` | 1 | Company spending status. |
| `sign-new-spending` | 1 | Company spending. |
| `sign-personal-name` | 1 | The owner's personal account. |
| `sign-gh-plan` | 1 | The organisation's billing page and a private-repo plan question. |
| `sign-lapse` | 1 | The owner's personal account. |
| `sign-env-skip` | 1 | Private-repo plan detail. |
| `sign-reviewers` | 1 | Private-repo claim. |
| `sign-free-plan` | 1 | Internal plan detail. |
| `sign-note-free` | 1 | Internal plan detail. |
| `sign-iphone` | 1 | Owner hardware. |
| `sign-keep-identity` | 1 | Owner-personal role. |
| `sign-keep-invoice` | 1 | Company billing errand. |
| `sign-keep-apple` | 1 | Company billing errand. |
| `sign-keep-holder` | 2 | Owner-personal role (two rows). |
| `sign-keep-admin` | 1 | Owner-personal role. |
| `sign-keep-gpg` | 1 | Owner-personal role. |
| `sign-keep-registry` | 1 | Owner-personal role. |
| `sign-free-fallback` | 1 | Internal plan detail. |
| `sign-personal-meantime` | 1 | The owner's personal account status. |
| `sign-homepage` | 1 | Private-repo claim. |

**docs/runbooks/macos.md**

| Rule | Changes | Why |
|---|---|---|
| `macos-owner-mac` | 1 | Owner hardware. |
| `macos-private` | 1 | Private-repo claim. |
| `macos-chat` | 1 | Refers to a private chat with the coding assistant. |
| `macos-chat2` | 1 | Refers to a private chat with the coding assistant. |

**docs/runbooks/windows.md**

| Rule | Changes | Why |
|---|---|---|
| `windows-reference-pc` | 1 | Owner hardware and remote-session phrasing. |

**docs/spikes/S1-mobile-audio-and-inference.md**

| Rule | Changes | Why |
|---|---|---|
| `s1-amended` | 1 | Owner hardware. |
| `s1-ios-row` | 1 | Owner hardware. |
| `s1-risk` | 1 | Owner errand. |

**docs/versions.md**

| Rule | Changes | Why |
|---|---|---|
| `global-the-prompt-repo` | 1 | Names third-party personal GitHub accounts and refers to the AI prompt. Not needed publicly. |
| `versions-verify-i` | 1 | Assistant first-person voice. |
| `versions-rust` | 1 | Owner machine state. |
| `versions-node` | 1 | Owner machine state. |
| `versions-pnpm` | 1 | Owner machine state and assistant first-person voice. |
| `versions-clang` | 1 | Owner machine state. |
| `versions-llvm` | 1 | Owner machine state. |
| `versions-pin-i` | 1 | Assistant first-person voice. |
| `versions-whisper-i` | 1 | Assistant first-person voice. |

**install/README.md**

| Rule | Changes | Why |
|---|---|---|
| `install-readme-signing` | 1 | Internal spending decision. |

**scripts/bench.sh**

| Rule | Changes | Why |
|---|---|---|
| `bench-chat` | 1 | Refers to a private chat with the coding assistant. |

**scripts/macos-dmg.sh**

| Rule | Changes | Why |
|---|---|---|
| `dmg-owner-mac` | 1 | Owner hardware. |
| `dmg-owner-call` | 1 | Internal spending decision. |

**scripts/website-build.py**

| Rule | Changes | Why |
|---|---|---|
| `site-source-url` | 1 | The public copy is the public source, so the site it generates links to it instead of saying the code isn't public. Change the URL here if the public repo gets another name. |
| `site-preview-comment` | 1 | Names an AI-assistant preview feature. |

**website/README.md**

| Rule | Changes | Why |
|---|---|---|
| `website-readme-private` | 1 | Private-repo claim. |
| `website-readme-domain` | 1 | Internal purchase step (the domain is live). |

**website/content/features.json**

| Rule | Changes | Why |
|---|---|---|
| `features-m2-decisions` | 1 | Follows the m2-decisions heading change. |
| `features-game` | 1 | Personal software use on the owner's machine. |
| `features-source-public` | 1 | In the public copy the source is public. |
| `features-agent-review` | 1 | AI-agent working detail. |
| `features-agent-review-detail` | 1 | AI-agent working detail. |
| `features-agent-review-evidence` | 1 | AI-agent working detail and a private commit hash; the files are those the other entries cite for the same fixes. |
| `features-real-data` | 1 | Says a developer machine holds real clinic data (same change as adr0012-real-data). |
| `features-trademark` | 1 | Internal legal to-do (listed as an owner item in docs/plans/public-repo.md). |

**website/src/index.html**

| Rule | Changes | Why |
|---|---|---|
| `site-template-private-branches` | 5 | Drops the 'code isn't public yet' variants from the template; the public variants are what the public source renders. |
| `site-template-public-branches` | 5 | Unwraps the 'source public' variants (the generator would do the same with SOURCE_URL set). |

