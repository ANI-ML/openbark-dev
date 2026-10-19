# Plan: the public source repository

Status: **ready, not published.** Date: 2026-10-19. Last export: branch `chore/public-export` (v0.1.1, `44e9269`, plus the repo-role wording in this repo's docs).

OpenBark's public source repository, `ANI-ML/openbark`, is a **published copy**: one clean commit per release, built from this repository's committed tree by `scripts/public-export.py` and authored as "ANI.ML Health". Development, CI, releases and their secrets stay in this repository, which is renamed `ANI-ML/openbark-dev`. Outside pull requests are reviewed on the public repo and ported here by hand. The public tree leaves out the AI working files, carries no co-author lines, names no individuals, and credits only "the ANI.ML Health team".

This file is private process documentation. The export leaves it out, together with the export script and its config.

## Outcome

- The export runs end to end and **the gate passes with exit code 0**: no forbidden words, no personal details, no "the repo is private" claims, and no secrets. gitleaks 8.30.1 reports 14 findings, every one a test value, a hash or the portal's publishable key, each reviewed and listed by location in the config; the built-in secret patterns find nothing outside six reviewed exceptions (see [The final gate](#the-final-gate)).
- **No blockers remain.** The three earlier blockers were resolved by owner decisions on 2026-10-19 (see [Decisions that resolved the blockers](#decisions-that-resolved-the-blockers)).
- 590 committed files were exported, 7 left out, 583 kept (504 text, 79 binary), 114.80 MB in total, of which 103 MB are the 14 Help videos. `git init` and `git add -A` in the export folder stage all 583 files, ignore none, and there is no remote and no commit.

## How the export works

`scripts/public-export.py` (Python 3.11+, standard library only) does five things, in order:

1. **Archive.** `git archive <ref>` (default `HEAD`) into an empty folder, so untracked and ignored files are never exported. Line endings follow `.gitattributes`.
2. **Exclude.** Removes the files listed under `[[exclude]]` and under any `[[blocker]]` with `action = "exclude"` (there are none now).
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

The gitleaks location is configured under `[gate] gitleaks` in the config; if it isn't found, `gitleaks` on `PATH` is used, and if neither exists the built-in secret patterns still run. Reviewed gitleaks findings are listed as `<path in the export>:<rule>:<line>` under `gitleaks_allow_fingerprints`, so the list doesn't depend on where the export is written; a finding that moves to another line is flagged again.

## What is excluded, and why

The exact names and patterns are under `[[exclude]]` in `scripts/public-export.toml`.

| What | Files in this export | Why |
|---|---|---|
| The AI coding assistant's working-notes file at the repository root | 1 | Internal working notes for the coding tool, and its name names the tool. |
| The original product brief at the repository root | 1 | Written as a prompt for the coding tool. Contains open decisions for the owner and a competitor feature and pricing comparison. |
| The daily download-stats workflow and its script (`.github/workflows/download-stats.yml`, `scripts/download-stats.py`) | 2 | It runs only in the development repo (`docs/network.md` B7 says so). In the public copy its schedule would run too and push a public `stats` branch. |
| AI assistant settings and instruction folders, scratch files, logs, editor and merge leftovers, Python caches, `.env` files and key material | 0 | Guards: nothing like this is committed today, but the rules stay in case something is. |
| This export tooling: `scripts/public-export.py`, `scripts/public-export.toml` and this file | 3 | Private process documents; the config spells out the forbidden words. |

The 14 Help videos (`apps/desktop/public/help/*.webm`, light and dark) are exported. They were re-recorded on a demo with neutral `C:\OpenBark-demo` folders; a frame every 3 seconds of each was checked for user names, AppData, Temp, scratch folders and personal names, and none appear. The demo names on screen (Dr. Demo, Dr. Laurent, Okonkwo, Tanaka) are invented.

## Text scrubs

270 rules made 316 replacements in 60 files. All of them are in the appendix, with what each one changes and why. In short:

- **AI working traces.** The original brief was called the "master prompt"; every mention becomes "product brief" (32 places, including two Rust doc comments). References to the excluded working-notes file, an instruction addressed to the coding tool by name in `docs/design/DESIGN.md`, mentions of AI design "skills", "agents" and an agent "track brief", an assistant speaking in the first person to the owner ("I'll install these", "the GitHub connector here isn't authorised", "paste it into the chat"), and verbatim quotes of the owner's requests to the coding tool are reworded or removed. The "Original decision notes" section of `docs/plans/M0.md` is removed: it was a conversation between the assistant and the owner, named third parties' personal GitHub accounts and held trademark research. The resolved table above it keeps every outcome. One entry of the help-video frame-leak guard named the assistant's scratch folder; the AppData, Temp and Users entries beside it still catch such paths.
- **Personal details.** The owner's personal email address (four places), the owner's iPhone, Mac, mobile number, accounts and real data, the owner's games and streaming during benchmarks, owner to-do lists and "the owner decided" phrasing become neutral wording ("a test iPhone", "a development machine", "Setup still needed: ...", "it was decided").
- **Internal company matters.** Another ANI.ML product, the internal Supabase project identifier where it appears in plans (ADR-0017, M3 and the sync line of `docs/network.md`), budget and "new spend" lines, the GitHub billing problem, "spun out" wording and a planned trademark search.
- **Private-repo claims.** Lines that said "the repo is private" become neutral wording. The docs that describe the repos (`CONTRIBUTING.md`, `website/README.md`, ADR-0020, the code-signing runbook, `docs/network.md`, `release.yml`) were reworded in this repository on 2026-10-19 to describe the development repo and the public copy accurately, so they need no scrubs.
- **The website copy in the export.** `scripts/website-build.py` gets `SOURCE_URL = "https://github.com/ANI-ML/openbark"`, the landing-page template keeps only its "source public" text, the "source will be public" feature becomes "shipped", and the site is rebuilt. The public copy therefore says "open source" and links to its own source. The live site is switched separately: branch `chore/source-public` (publish step 9).
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

**Reviewed exceptions** (`[[gate.allow]]`, each limited to named files and exact text; none of the patterns was changed):

| Pattern | Files | Why |
|---|---|---|
| supabase-project-ref | `website/src/portal.js`, `website/dist/**`, `scripts/website-build.py`, both `vercel.json`, `apps/desktop/e2e/website.mjs`, `docs/network.md`, `supabase/README.md`, the delete-account function | The account portal's project URL and session key. They are in the live website's code that every visitor downloads, the CSP must name the host, and `docs/network.md` must list every host. The key in `portal.js` is the **publishable** key (`sb_publishable_...`); no `sb_secret_` or service-role key is anywhere in the tree. |
| twilio-key | `engine/os-connect/**` | Test placeholders: sequential-hex or all-f SIDs. |
| jwt | `engine/os-connect/fixtures/twilio-node-access-tokens.json` | Test vectors made by twilio-node from those placeholders and the secret `fixture-secret-not-a-real-key-000`; their HMAC signatures were re-checked with that secret. |
| openai-hf-keys, assigned-secret | two test files | A made-up Hugging Face token (the alphabet). |
| internal-projects | `apps/desktop/public/help/*.webm` | Three bytes, "ARC", inside compressed video data in three of the videos. The gate's binary scan reports only the first match per file, so the audit scanned every binary file for every internal name and found nothing else. |

The 14 gitleaks findings are the same Twilio test values and fixture tokens, a git blob hash in `scripts/mlx-test-env.sh`, and the publishable key in `website/src/portal.js` and `website/dist/portal.js`.

Two warning patterns are listed for a human look but don't fail: "product owner" (32 remaining mentions, all of them the role in ADR deciders lines, approvals and requirements) and first-person assistant phrasing (none left). "Owner" on its own is not flagged, because it almost always means the animal's owner.

Third-party names that stay on purpose: the pyannote credit "CNRS (Herve Bredin)" in `NOTICE` (a licence attribution), upstream model and library repositories in `docs/versions.md`, and the invented patient, client and clinician names in the fixtures, UI examples and Help videos.

## Decisions that resolved the blockers

Owner decisions, 2026-10-19:

1. **Help videos (`help-videos`).** Re-recorded on a demo with neutral `C:\OpenBark-demo` folders and checked frame by frame; exported again.
2. **The public repo's name (`public-repo-name`).** The public repo is `ANI-ML/openbark`. This repository is renamed `ANI-ML/openbark-dev` first, so every `github.com/ANI-ML/openbark` link in the tree (`SECURITY.md`, `.github/ISSUE_TEMPLATE`, `CONTRIBUTING.md`, `Cargo.toml`, the website's Source links) points at the public repo.
3. **The public repo's role (`public-repo-role`).** A published copy, refreshed each release with one clean commit authored as "ANI.ML Health". Development stays private; outside pull requests are ported by hand. `CONTRIBUTING.md` explains this to contributors, and `website/README.md`, ADR-0020, the code-signing runbook, `docs/network.md` and `release.yml` say that releases, signing secrets, the Azure federated credential, the releases token and the Vercel connection belong to `ANI-ML/openbark-dev`.

## Owner decisions and follow-ups (not blocking)

- **Credit for ported pull requests.** One commit per release, authored as ANI.ML Health, leaves an outside contributor's name out of the public history. Decide whether to thank them in the release notes or the changelog entry, or with a `Co-authored-by` line in the release commit (that line is in the commit message, which the gate doesn't scan, but the publish check in step 6 expects no trailers; change one or the other).
- **Tags and workflows on the public copy.** `release.yml` runs on any pushed `v*` tag: on the public copy it would build unsigned installers, create a draft release there and fail at the releases repo. Don't push tags there, or, once the rename is done, add `if: github.repository == 'ANI-ML/openbark-dev'` to the release and website-release jobs (not before: today this repo is still `ANI-ML/openbark`). CI on pull requests to the public repo is fine and useful.
- **Dependabot on the public copy.** `.github/dependabot.yml` will open version-update pull requests on the public repo too. They can't be merged there; turn version updates off on the public repo (keep alerts and security updates), or port them like any other pull request.
- **Repository size.** The Help videos add 103 MB, and each release that re-records them adds that much again to the public history. Consider serving them from the website or a release instead.
- **The Supabase project identifier in plans.** ADR-0017, M3 and the sync line of `docs/network.md` still hide it, although the portal files now publish it. Fine as is; drop those scrubs if the plans should name the project.
- **Trademark search.** The plans said a formal trademark search on "OpenBark" should happen before the public launch. The export removes that internal to-do; the search itself is still open.
- **"Product brief" citations.** About 60 places (docs, `website/content/features.json` evidence, two code comments) cite sections of the product brief, which is not published. Leave them, publish a cleaned brief, or reword them here.
- **Commit hashes in the features list.** About 15 `evidence` fields in `website/content/features.json` cite commits (for example `3888f20`) that won't exist in the public history. Consider citing files instead.
- **The README build block** (see Text scrubs). Fix it here.
- **Social image.** `website/public/og-image.png` in this export still has the "free" wording. `chore/source-public` re-renders it; the next export after that branch is merged carries the new one.
- **`SECURITY.md` response targets** (five and ten business days) are still to be confirmed.
- **`scripts/bench.sh`** names its results folder after the computer's host name, which can put a contributor's machine name into the repo, and downloads fixture audio from a `fixtures-v1` release that a fresh public repo won't have unless it is recreated (or regenerated with `tools/fixture-tts`).
- **`.gitignore`** has no `__pycache__/` entry, although the repo has Python scripts.
- **Same fixes here.** Most scrubs are improvements this repository could take as well (owner to-dos, quotes, "the owner decided"). Applying them here makes the scrub list shorter and the export easier to keep up to date. "Dev box" references to the reference Windows machine are kept: they describe test hardware, not a person.

## Export size and file list

| Folder | Files | Size |
|---|---|---|
| (root) | 19 | 0.29 MB |
| `.github` | 10 | 0.06 MB |
| `apps` | 194 | 107.40 MB |
| `docs` | 79 | 1.14 MB |
| `engine` | 115 | 2.39 MB |
| `install` | 13 | 0.04 MB |
| `packages` | 98 | 2.62 MB |
| `scripts` | 23 | 0.13 MB |
| `supabase` | 5 | 0.02 MB |
| `tools` | 2 | 0.01 MB |
| `website` | 25 | 0.71 MB |
| **Total** | **583** (504 text, 79 binary) | **114.80 MB** |

The largest files are the Help videos (`record.webm` 15.5 MB, `record-dark.webm` 15.3 MB, `review-dark.webm` 12.3 MB); every file is under GitHub's 100 MB limit.

## Checks run on the exported tree

- **Independent audit** beyond the gate, case-insensitive: no AI tool or vendor name, no owner name or personal email (only we.care@animl.health and `example.com` addresses), no Windows user name, AppData only as the product's own install and data paths, no scratch folders, no other ANI.ML product, no budget or spending notes (only public list prices and token "budgets"), no TODOs naming people, no absolute local paths except invented test paths (`/Users/sam`, `/Users/x`, `/home/vet`, `<other user>`). Every remaining "owner" is the animal's owner or the product-owner role. ADR-0017, ADR-0018 and M3 carry no name, email, ARC or budget.
- **No key material**: no `.p12`, `.p8`, `.key`, `.pem`, `.pfx`, `.env` or `.asc` files, and no Supabase secret or service-role key.
- **Help videos**: a frame every 3 seconds of all 14 was checked: only `C:\OpenBark-demo` paths and invented names.
- **`LICENSE`, `NOTICE`, `SECURITY.md`, `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`, `CHANGELOG.md`** are present. `NOTICE` credits "ANI.ML Health Inc. and OpenBark contributors" and "the ANI.ML Health team".
- **Code is unchanged except comments and one string.** The only code changes are doc comments (two Rust files from the "product brief" rule, `twiml.rs`, two template files), comments in `scripts/bench.sh`, `scripts/macos-dmg.sh`, `brand-images.mjs` and the delete-account function, the `SOURCE_URL` value and a comment in `scripts/website-build.py`, and one entry removed from the help-video leak guard. The changed scripts pass `node --check`, `python -m py_compile` and `bash -n`, and `website/content/features.json` is valid JSON.
- **Website.** The rebuild in the export succeeded.
- **`git init` and `git add -A`** inside the export folder stage all 583 files; `.gitignore` drops none, there is no remote, and nothing is committed.

## Publish steps (for later)

Do these only when the owner says go. None of this has been done.

1. **Blockers.** Cleared (see above).
2. **Rename.** Rename this repository to `ANI-ML/openbark-dev` (GitHub redirects the old URL until a new repo takes the name). Update local remotes. Check what names the repo: Vercel's Git connection (it follows a rename, but confirm the next deploy), the Azure federated credential's subject if signing is set up (runbook B8: it must become `repo:ANI-ML/openbark-dev:environment:release`), and any webhooks. `PUBLIC_RELEASES_TOKEN` is a secret on this repo and moves with it.
3. **Create the public repo** `ANI-ML/openbark` in the ANI-ML organisation, empty (no README, licence or `.gitignore` from GitHub), with **private** visibility for now.
4. **Turn on protections before the first push** (Settings, then Code security, or Advanced Security): secret scanning, **push protection**, private vulnerability reporting (`SECURITY.md` relies on it), Dependabot alerts and Dependabot security updates (and decide on version updates, see follow-ups).
5. **Export and check.** `python scripts/public-export.py --out ../openbark-public --git-init --force` must end with `RESULT: gate passed` and exit code 0. Read through `README.md` and skim the JSON report.
6. **Make the single commit as the company.** In `../openbark-public` only:
   ```bash
   git config user.name "ANI.ML Health"
   git config user.email "we.care@animl.health"       # or the company's GitHub noreply address
   git commit -m "OpenBark 0.1.1"                     # signed with a company key, if commits are signed
   git log --format='%an <%ae> | %cn <%ce>%n%B'        # check: only the company identity, no trailers
   git rev-list --count HEAD                           # check: 1
   ```
   The local settings decide the author, whatever the computer's global identity is. To keep AI working files out of future commits without naming them in `.gitignore`, list them in `.git/info/exclude` of this clone.
7. **Push.** `git remote add origin https://github.com/ANI-ML/openbark.git`, then `git push -u origin main`. Never push this private repository's history there, and never make this repository public.
8. **Protect `main`** (a branch ruleset): block force pushes and deletion, and require linear history. Pull-request rules matter less, since outside pull requests are ported rather than merged. Set the description, the homepage `https://www.openbark.io`, topics, and the social preview image (`docs/brand/images/github-social-preview.png`). Then switch the visibility to **public**.
9. **Switch the live website.** Merge branch `chore/source-public` into `main` here: it sets `SOURCE_URL`, rebuilds `website/dist` and re-renders the social image. Vercel deploys it.
10. **Check from outside.** Open the public repo signed out: the README renders, the licence is detected, and the Security tab offers "Report a vulnerability". Search the public repo with GitHub code search for the gate words. Confirm secret scanning shows no alerts, and that the website's Source code links open the repo.

### Later updates

For each release: export again into a fresh folder, copy the result over a clone of the public repo (everything except `.git`, deleting files that are gone), and commit once as the company, as in step 6 (for example "OpenBark 0.1.2"). Every scrub rule must still match, so source changes that touch a scrubbed line stop the export until the rule is reviewed. Outside pull requests on the public repo are reviewed there, ported to this repository by hand, and closed with a note naming the release that includes them.

## Appendix: every scrub rule

Rules run in the order they appear in the config, global rules first; they are grouped by file here. "Changes" is the number of replacements in the last export. The exact text of each rule is in `scripts/public-export.toml` under the same id.

**Global**

| Rule | Changes | Why |
|---|---|---|
| `global-master-prompt` | 32 (docs/adr/0001-inference-libraries.md: 2, docs/adr/0004-monorepo-and-tooling.md: 1, docs/adr/0005-optional-cloud-sync.md: 2, docs/adr/0018-iphone-app.md: 2, docs/network.md: 1, docs/perf/M0.md: 2, docs/plans/M0.md: 9, docs/spikes/S1-mobile-audio-and-inference.md: 4, docs/spikes/S2-native-linking.md: 4, docs/versions.md: 2, engine/os-engine/src/faithfulness.rs: 1, engine/os-llm/src/lib.rs: 1, website/content/features.json: 1) | The 'master prompt' is the original product brief written for the AI coding assistant (excluded above). Neutral name for the same document. |
| `global-master-prompt-capital` | 1 (docs/plans/M0.md: 1) | As global-master-prompt, sentence case. |

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
| `adr0020-private-repo` | 1 | An internal account question (which GitHub plan). The paragraph now describes the repos accurately, so only that line goes. |
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
| `adr0020-alt-attestations` | 1 | Private-repo claim. |
| `adr0020-alt-cosign` | 1 | Private-repo claim. |
| `adr0020-unsigned-decision` | 1 | Owner-personal phrasing. |

**docs/adr/0021-dental-chart.md**

| Rule | Changes | Why |
|---|---|---|
| `adr0021-deciders` | 1 | Verbatim quote of the owner's instruction to the coding assistant. |
| `adr0021-access-refused` | 1 | Reads as an automated fetch-tool error. |
| `adr0021-status-approval` | 1 | Owner-personal phrasing. |
| `adr0021-chart-decision` | 1 | Owner-personal phrasing. |

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
| `adr0024-decisions-built` | 1 | Owner-personal phrasing. |

**docs/adr/0025-mlx-and-models-of-your-choice.md**

| Rule | Changes | Why |
|---|---|---|
| `adr0025-deciders` | 1 | Verbatim quotes of the owner's requests to the coding assistant; paraphrased. |
| `adr0025-amends` | 1 | Owner-personal phrasing. |
| `adr0025-what-was-asked` | 1 | Private-conversation phrasing. |
| `adr0025-owner-wants` | 1 | Owner-personal phrasing and owner hardware. |
| `adr0025-dev-box` | 1 | Owner hardware. |
| `adr0025-alt-labels` | 1 | Owner-personal phrasing. |

**docs/design/DESIGN.md**

| Rule | Changes | Why |
|---|---|---|
| `design-provided-by` | 1 | Owner-personal phrasing. |
| `design-amended` | 1 | Owner-personal phrasing. |
| `design-assistant-instruction` | 1 | Instruction addressed to the AI coding assistant by name. |

**docs/eval/M2.md**

| Rule | Changes | Why |
|---|---|---|
| `eval-m2-owner-decide` | 1 | Owner-personal phrasing. |
| `eval-m2-chart-decision` | 1 | Owner-personal phrasing. |

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

**docs/plans/mlx-models.md**

| Rule | Changes | Why |
|---|---|---|
| `mlx-never-real-folder` | 1 | Owner's machine and real data. |
| `mlx-removal-real-folder` | 1 | Owner's machine. |
| `mlx-track-brief-probes` | 1 | Points at an AI-agent working brief and research files outside the repository. |
| `mlx-track-brief-prototype` | 1 | The same. |

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
| `tpl-status` | 1 | Owner-personal phrasing in the plan's status line. |
| `tpl-never-4317` | 1 | Owner's machine and real data. |
| `tpl-eval-row` | 1 | Owner hardware. |
| `tpl-owner-decides` | 1 | Owner-personal phrasing. |

**docs/runbooks/code-signing.md**

| Rule | Changes | Why |
|---|---|---|
| `sign-audience` | 1 | Owner-personal audience. |
| `sign-still-to-do` | 1 | Owner errand heading. |
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

**scripts/website-build.py**

| Rule | Changes | Why |
|---|---|---|
| `site-source-url` | 1 | The public copy is the public source, so the site it generates links to it instead of saying the code isn't public. Change the URL here if the public repo gets another name. |
| `site-preview-comment` | 1 | Names an AI-assistant preview feature. |

**apps/desktop/e2e/help-videos.mjs**

| Rule | Changes | Why |
|---|---|---|
| `help-videos-leak-list` | 1 | One entry of the frame-leak guard names the assistant's scratch folder; the AppData, Temp and Users entries still catch such paths. |

**engine/os-connect/src/twiml.rs, engine/os-app/src/templates.rs, engine/os-app/src/tests/templates.rs**

| Rule | Changes | Why |
|---|---|---|
| `twiml-owner-decision` | 1 | Owner-personal phrasing in a doc comment; the decision is ADR-0023's. |
| `templates-rs-owner` | 1 | Owner-personal phrasing in a doc comment. |
| `templates-tests-owner` | 1 | Owner-personal phrasing in a doc comment. |

**supabase/**

| Rule | Changes | Why |
|---|---|---|
| `supabase-readme-owner` | 1 | Owner errand. |
| `supabase-readme-chat` | 1 | Refers to a private chat with the coding assistant. |
| `supabase-function-owner` | 1 | Owner errand, in a comment. |

**website/README.md**

| Rule | Changes | Why |
|---|---|---|
| `website-readme-domain` | 1 | Internal purchase step (the domain is live). |
| `website-readme-supabase-owner` | 1 | Owner errand. |

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

