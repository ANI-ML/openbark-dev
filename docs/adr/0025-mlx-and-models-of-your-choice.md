# ADR-0025: MLX models on Apple silicon, and models of your choice

- Status: accepted (built and integrated 2026-10-19)
- Date: 2026-10-18
- Deciders: product owner (asked: "Allow users to load models of their choice", specifically MLX models browsed on Hugging Face, which can be listed but not downloaded; later, "that seems to only be for MLX models, all the rest download"; and, while this was being designed, "Allow users to uninstall models as well to save space or swap in / out those of choice"), engineering
- Relations:
  - **Amends ADR-0009.** Decision 1 (labels) stays. Decision 2 (measure before adding a runtime) is no longer a gate: MLX becomes an opt-in engine because the owner asked for it, and the measurement is still run, to inform which engine OpenBark recommends. Decision 3 (prefer `mlx-rs`) is replaced by decision 2 below. The rejected "Python sidecar" alternative is adopted, in a stricter form (no server, no port, no network).
  - **Amends ADR-0019** where it says "MLX only after measurement" (decision table, §4 platform matrix and §5). In §5, "Notes: Apple M3 GPU (MLX)" becomes a true label when an MLX model writes the note.
  - Follows ADR-0014 and ADR-0019 for packs: opt-in download, every file pinned by URL, size and SHA-256, a self-test in a child process, results kept in the data folder keyed by app version, hardware, backend and model.
  - Keeps ADR-0001 (llama.cpp and GGUF stay the default everywhere) and ADR-0006 (every draft is generated against a JSON Schema).
  - Uses the keychain pattern of ADR-0008 for the Hugging Face token.
  - Copy follows the brand guide (ADR-0016, `docs/brand/DESIGN.md` §8).
- Plan: [docs/plans/mlx-models.md](../plans/mlx-models.md)

## Context

### What was asked, and what was found

On a Mac, **Browse Hugging Face** lists MLX builds (ADR-0009) and labels them **MLX optimized**, but opening one says the MLX engine is planned, and nothing can be downloaded. The owner wants those models to download and write notes. Other Hub models download fine on the owner's Mac.

Testing the Hub browser against the live Hub on 2026-10-18 found four more problems:

| Problem | Example | Cause (checked in the code and on the live API) |
|---|---|---|
| Split GGUFs and GGUFs in sub-folders show no files | `unsloth/Qwen3-235B-A22B-Instruct-2507-GGUF`: the plain tree lists 23 entries, 20 of them folders, and no GGUF at the root. `?recursive=true` lists 99, including `Q4_K_M/...-00001-of-00003.gguf` | `hub::repo` lists the tree without `recursive=true` and drops every name containing `-of-0` |
| Draft-model and vision files are offered as models | `unsloth/gemma-4-E4B-it-GGUF` offers `mtp-gemma-4-E4B-it.gguf` (98 MB) | Only `mmproj` is filtered out |
| Gated repos can't be downloaded | `google/gemma-3-1b-it-qat-q4_0-gguf` | The tree answers 200 but masks LFS hashes as `****...`, which would be saved as the SHA-256. `resolve` answers 401 with `X-Error-Code: GatedRepo`. The app always passes no token. (meta-llama repos show 0 files because they hold no GGUF at all, not because of the token.) |
| Hub models may not download on Windows | any `hf:` id | User-added ids look like `hf:<repo>/<file>`, and `ModelStore::model_dir` uses the id as a folder name. A colon is not allowed in a Windows folder name (WinError 267, reproduced) |

And two requests: add a model from this computer (a `.gguf` file, or an MLX folder on a Mac), and remove models again, to save space or swap one for another. Today nothing can be removed from the Models screen except the GPU speech pack; a 7 GB model stays until someone deletes its folder by hand.

### What OpenBark needs from an MLX engine

- **Constrained output.** Every draft passes a JSON Schema (`GenerateRequest.json_schema`). llama.cpp enforces it with a grammar. An MLX engine must enforce it too, or report that it can't.
- **Streaming tokens** for the live draft.
- **Local only.** No server, no port, no network. Patient text never leaves the machine.
- **Behind `LlmBackend`.** The rest of the engine stays unchanged.
- **Testable without a Mac.** The team has no Mac. The dev box is Windows with WSL (Ubuntu 24.04).

### Runtimes checked (live, 2026-10-18)

| Option | Finding |
|---|---|
| **mlx-lm** (Python, MIT) | 0.32.2 (2026-10-16), Python 3.11 or later. About 140 architectures, including `gemma3`, `gemma4`, `llama`, `qwen3`, `mistral3`, `phi3`. `stream_generate` takes `logits_processors`, a sampler and a seed. No torch. Its dependencies are `mlx` (macOS only), numpy, transformers, sentencepiece, protobuf, pyyaml and jinja2 |
| **mlx** | 0.32.4 (2026-10-14), MIT. macOS wheels only for `macosx_14_0_arm64` and later. The MLX docs say "MLX is only available on devices running macOS >= 14.0". A Linux CPU build (`mlx-cpu`, `manylinux_2_35_x86_64`) exists |
| **llguidance** (MIT) | 1.9.1 (2026-09-30), abi3 wheels for macOS arm64 and Linux x86_64, no Python dependencies. Builds a token mask from a JSON Schema in about 50 µs to 1.5 ms per token. Ships `llguidance.hf` (Hugging Face fast tokenizers), `llguidance.numpy` and `llguidance.mlx` (a Metal kernel) |
| outlines 1.3.3 | Adds pydantic, jsonschema, pillow and more. Too heavy |
| xgrammar 0.2.8 | Requires torch. Rejected |
| mlx-swift-lm 3.32.3 | Strong native option with guided generation, but needs Xcode to build its Metal shaders, a Swift sidecar, and can't be built or tested on Windows or WSL |
| mlx-rs 0.32.0 | Its LM crate runs only Llama and Qwen3, isn't published, and has no constrained decoding. Not viable (this is what ADR-0009 preferred) |
| mlx-c 0.7.0 | A C API with no models or tokenizers. Every architecture would need rewriting |

A prototype ran end to end on Linux CPU in WSL: `mlx-community/Qwen3-0.6B-4bit` drafted a veterinary note against a 5-field schema with `additionalProperties: false`. The output was valid JSON with every required key and no extra keys, finished on its end token, and the matcher ended in an accepting state (peak 0.5 GB). The llguidance tokenizer bridge also built working matchers for Gemma 4, Gemma 3, Llama 3.2 and Qwen3 MLX repos, and `packages/templates/soap/schema.json` compiled with no warnings. CPU speed was about 3 tokens per second: fine for tests, not for clinics.

## Decision

### 1. Who gets MLX, and the minimum macOS

- **MLX runs on a Mac with Apple silicon and macOS 14 Sonoma or later.** This is MLX's own floor (its wheels start at `macosx_14_0_arm64`). The app itself keeps its macOS 13 minimum: on macOS 13 and on Intel Macs, MLX repos are still listed, with a plain note saying what they need, and the GGUF build is suggested.
- **Windows and Linux** keep not searching for MLX builds (ADR-0009). If an MLX repo is opened anyway (a pasted id, a link), it shows the same note and the GGUF suggestion. Nothing is offered for download there.
- **GGUF on llama.cpp stays the default** on every platform, Macs included. MLX is opt-in, from the Models screen.
- **A developer switch**, `OPENBARK_MLX_DEV=1`, turns the MLX engine on for Linux x86_64 using the CPU build of MLX, so the whole path can be tested in WSL and on CI. It is never on in a release by default and the UI marks it "test mode".

### 2. Runtime: mlx-lm in a private Python worker, behind `LlmBackend`

- New backend `MlxBackend` in `os-llm`, implementing `LlmBackend`. The rest of the engine is unchanged. `LlmCapabilities.backend` is `"mlx"`.
- It starts **one worker process**: the pack's own Python running a single-file worker script. The script is compiled into the program (`include_str!`) and written to the pack's folder when it is missing or out of date: its bytes are compared with the compiled-in copy on every start.
- **The worker talks only over its standard input and output, one JSON object per line** (the protocol is in the plan). Patient text goes only over the child's stdin. It binds no port, opens no socket and starts no server.
- **The worker can't reach the network**, by four layers:
  1. it is started as `python3 -I -B -X utf8 -u <worker.py>` from an empty environment, with only `PATH=/usr/bin:/bin`, `HOME`, an empty `HF_HOME` inside the pack, `HF_HUB_OFFLINE=1`, `TRANSFORMERS_OFFLINE=1`, `HF_DATASETS_OFFLINE=1`, `HF_HUB_DISABLE_TELEMETRY=1`, `HF_HUB_DISABLE_IMPLICIT_TOKEN=1`, `DO_NOT_TRACK=1`, `TOKENIZERS_PARALLELISM=false` and `NO_PROXY=*`. No proxy variables, no user site-packages, no `PYTHON*` variables, and the user's own `~/.cache/huggingface/token` is never read;
  2. its first lines replace `socket.socket.connect`, `socket.create_connection` and `socket.getaddrinfo` with functions that raise, before mlx-lm or transformers are imported;
  3. it is only ever given an absolute local folder to load, never a repo id;
  4. on macOS it runs under `sandbox-exec` with a profile that denies all network access (`OPENBARK_MLX_SANDBOX=0` turns this off, for support only).
- **The worker never logs prompts or generated text.** Its stderr goes to OpenBark's log, truncated, and carries only metadata (timings, sizes, versions, error kinds).
- The worker exits when its stdin closes, so it dies with the app; `MlxBackend` also kills it when dropped. One request at a time; a cancel message stops generation between tokens.
- On a CPU device (WSL, CI) the worker upcasts to float32, because quantised bf16 matmul is about 10 times slower on CPU.

### 3. Constrained output on MLX: llguidance

- The worker turns `json_schema` into an llguidance matcher (`LLMatcher.grammar_from_json_schema`, `whitespace_flexible: false`) over the model's own tokenizer (`llguidance.hf.from_tokenizer`, with **all** of the model's end tokens, since Gemma ends turns with `<end_of_turn>`, id 106, not only `<eos>`), and applies it as an mlx-lm logits processor. On Metal it applies the mask with `llguidance.mlx.apply_token_bitmask`; on CPU it unpacks the mask with plain MLX operations.
- **`json_schema_constrained` is true only when it is enforced**: the worker reported that llguidance and a fast tokenizer loaded for this model, and the model passed its self-test (decision 6). Otherwise `MlxBackend::generate` with a schema returns `LlmError::Grammar` without generating, and OpenBark does not offer that model for drafting. There is no unconstrained fallback for notes.
- llguidance is not llama.cpp's GBNF converter, so schema edge cases can differ. A golden test compiles every built-in template's schema and the from-example and edit schemas in llguidance, with no errors.

### 4. The MLX engine pack

- An **opt-in download**, from a new **MLX engine** card on the Models screen (like the GPU speech card), shown on a Mac. About 106 MB to download and 366 MB on disk.
- Contents: a standalone CPython 3.12 build for `aarch64-apple-darwin` from [python-build-standalone](https://github.com/astral-sh/python-build-standalone) (Python under the PSF licence, build scripts MPL-2.0), and the wheels for mlx, mlx-metal, mlx-lm, llguidance, transformers, tokenizers, numpy and their dependencies (35 wheels, all permissive licences, no torch, no source builds).
- **Every file is pinned** by URL, size and SHA-256 in a lock file generated by `scripts/mlx-pack-lock.py` (uv's cross-platform resolver, `--only-binary :all:`, `MACOSX_DEPLOYMENT_TARGET=14.0`, Python 3.12), checked against the PyPI JSON API and the release's `SHA256SUMS` and GitHub digest. The lock is compiled into the program. Downloads go through `os_models::download` (resumable, verified), as for the GPU pack.
- **Release age:** the lock generator uses `--exclude-newer` 7 days before the day it runs, and records the cut-off in the lock. Code the pack downloads runs outside Cargo's and pnpm's lockfiles, so it gets a longer cool-off than pnpm's 24 hours. Checked on 2026-10-18, that resolves to python-build-standalone `20261009` (CPython 3.12.15), mlx and mlx-metal 0.32.3, mlx-lm 0.32.0, llguidance 1.9.1 and transformers 5.19.0. The exact list is whatever the script writes. It is recorded in `docs/mlx-pack-pins.md` and summarised in `docs/versions.md`.
- **No pip, no index.** Python's archive is unpacked with `tar` and `flate2` (rejecting absolute paths, `..` and symlinks that point outside the folder). Each wheel is unzipped straight into `site-packages`, `.dist-info` included (transformers checks package versions); wheels with a `.data/` folder are refused by the lock script. Then `python -I -m compileall` precompiles the packages.
- **Where it lives:** `<local data>/OpenBark/engines/mlx/<lock id>/` (overridable with `OPENBARK_ENGINES_DIR`), the data folder, never the app bundle (read-only on macOS). Downloads go to `<models>/mlx-engine-pack/` and are deleted after unpacking. A folder for an older lock is removed at start-up. **Remove** deletes it.
- The Linux CPU lock (`x86_64-manylinux_2_35`, with `mlx-cpu`) is generated the same way and used only under `OPENBARK_MLX_DEV=1`.

### 5. MLX models from the Hub

- An MLX repo is offered as **one row: the whole model folder**, with its total size, fit and file count. It is chosen with `file = "@mlx"` and gets the id `hf:<repo>/@mlx`, so it never shares a folder with a GGUF from the same repo. The catalog gets a new layout, `LlmMlx { config: "config.json" }`; the model folder is the parent of `config.json`.
- **Which files:** an allow-list at the repo root: `config.json`, `generation_config.json`, `model*.safetensors`, `weights*.safetensors`, `model.safetensors.index.json`, the tokenizer files (`tokenizer.json`, `tokenizer_config.json`, `special_tokens_map.json`, `added_tokens.json`, `vocab.json`, `merges.txt`, `vocab.txt`, `tokenizer.model`, `*.tiktoken`), `chat_template.jinja`, `chat_template.json`, and small optional files (`processor_config.json`, `preprocessor_config.json`, `LICENSE*`). **Never `*.py`** (remote code), sub-folders, or other weight formats.
- **Runnable only if** `config.json` is there, at least one safetensors file is, every file the index names is selected (or a `-NNNNN-of-NNNNN` set is complete), and there is a tokenizer. Otherwise the repo shows why.
- **Every file is verified.** LFS files (the weights, sometimes `tokenizer.json`) by the SHA-256 the Hub reports (`lfs.oid`). Small plain files have no SHA-256 in the listing, only their git blob SHA-1, so `FileSpec` gains an optional `git_sha1`, checked as `sha1("blob <len>\0" + bytes)`. `sha256` may be empty only when `git_sha1` is set; the built-in catalog keeps requiring SHA-256. New crate: `sha1` 0.11.0 (MIT OR Apache-2.0).
- **The architecture is checked before download.** `config.json` is read (a small pinned GET) for `model_type`, and compared with the list of architectures in the pinned mlx-lm (written into the lock by the script). An unsupported one is shown, not offered.
- **GGUF fallback.** When an MLX repo is opened, OpenBark asks the Hub for GGUF builds of the same base model (`filter=base_model:quantized:<base>&filter=gguf`, taken from the repo's `base_model`) and lists up to five, well-known publishers first. Checked live: for `mlx-community/Qwen3-0.6B-4bit` this returns `Qwen/Qwen3-0.6B-GGUF` and others.

### 6. Self-tests

- **Pack check** (after install, and from **Try again**): the worker imports mlx, mlx-lm, transformers and llguidance, confirms that the socket guard blocks a connection, confirms Metal is available (on a Mac), runs a small matrix multiply, and checks that applying a known mask to random logits gives minus infinity exactly where expected. A failure means MLX models aren't offered for drafting, and the card says so.
- **Model check** (first load of each model): generate at most 32 tokens under `{"type":"object","properties":{"ok":{"type":"boolean"}},"required":["ok"]}`; the output must parse. Only then is `json_schema_constrained` true.
- Results go in `<engines>/mlx/checks.json`, keyed by app version, lock id, macOS version, chip and model id and revision (ADR-0019 decision on where results live).

### 7. Split GGUFs, sub-folders, and files that aren't models

- The Hub tree is listed with `?recursive=true&expand=false`, following `Link: rel="next"` pages only on `https://huggingface.co/api/models/`, up to 20 pages.
- Shards named `<prefix>-NNNNN-of-NNNNN.gguf` are grouped by folder, prefix and count, and offered as **one row** ("3 parts", summed size) only when every part is there. All parts download side by side at their repo paths; llama.cpp is given part 1 and finds the rest itself (`llama-model-loader.cpp`, same folder, same prefix).
- Ids of grouped models are shortened to `hf:<repo>/<prefix basename>`, to keep Windows paths short.
- Left out, case-insensitively: names containing `mmproj`; names starting with `mtp-`, `imatrix` or `ggml-vocab`; anything under a folder called `MTP`; names containing `-draft`, `lora` or `adapter`.
- Repo paths are checked before anything is written: no absolute paths, no `..`, `\`, `:` or NUL, no empty components. Download folders for nested paths are created as needed.
- **Model folders get safe names**: a colon in an id becomes `_` on disk (`hf_publisher/...`). An existing folder under the old name is still used, so Mac and Linux installs keep their downloads. The Models folder check (ADR-0013) recognises these folders, the `local/` folder and the MLX pack's download folder as OpenBark's.

### 8. Gated repos and the Hugging Face token

- A repo is gated when the Hub says `gated: "auto"` or `"manual"`. A hash that isn't exactly 64 hex characters counts as masked, and a masked file is never added to the catalog.
- **States:** no token saved → "This model is gated. Request access on Hugging Face, then add your token." Token saved but hashes still masked → "Hugging Face hasn't given your token access yet. Check your request on the model's page." (plus "The publisher reviews requests by hand." for `manual`). A 401 or 403 with `X-Error-Code: GatedRepo` during download becomes the same message, not "HTTP 401". A 401 for a repo that isn't gated is "not found or private", which is what the Hub means by it.
- **The token is optional, saved on request, and sent only where needed:** to `huggingface.co` only, only for a gated repo's file list and downloads, and once to `/api/whoami-v2` when it is saved (to check it and show whose it is). Search stays tokenless. The Hub's redirects to its storage hosts don't carry it (reqwest drops `Authorization` when the host changes).
- **Storage:** the operating system's keychain (service `OpenBark`, account `huggingface-token`, the ADR-0008 pattern). Without a keychain, a vault-encrypted file in the data folder, only when encryption at rest is on; otherwise it is kept in memory for the session. Never in plain text, never returned to the interface, never logged. On servers `OPENBARK_HF_TOKEN` (or `HF_TOKEN`) overrides it, and the UI says it is set by the server. In web mode with a team set up, saving or removing it needs a veterinarian, as changing the team does.
- Catalog entries remember that their repo is gated (`gated: true`), so downloads send the token only for those.

### 9. Adding a model from this computer

- **Copied into the model store by default.** The store survives folder moves (ADR-0013); a file on a USB stick or in Downloads doesn't. SHA-256 is computed while copying. A hard link is tried first on the same drive (no extra space). The same 1 GiB free-space margin as downloads applies.
- **Or used where it is, on request** ("Use it where it is, don't copy"). Large models are often already on disk for another app (LM Studio, Ollama exports), and a second 20 GB copy defeats the point of saving space. OpenBark then only remembers the absolute path. It never writes to, moves or deletes that file or folder. If it disappears, the row says **Missing** and drafting with it is refused. Hashes are computed once at import and recorded, so a changed file is noticed by `models verify`.
- **A `.gguf` file:** the file must start with `GGUF` and version 2 or 3. A split file brings all its parts from the same folder, or the import names the missing ones. `mmproj` and `mtp-` files are refused with a plain reason.
- **An MLX folder (Mac, or the developer switch):** the same runnable checks as the Hub (`config.json` with a `model_type`, every indexed safetensors file present and starting with a valid header, a tokenizer). Only allow-listed files are copied, or, in place, used.
- Entries get `source: local` (with the path when used in place) and ids `local/<name>`. They show "This computer" as the publisher, and can't be re-downloaded. The desktop app uses the system file picker; web mode asks for a path on the computer running OpenBark, and needs a veterinarian when a team is set up.

### 10. Removing models

Every downloaded or imported model has a **Remove** action on the Models screen, with the space it uses and, afterwards, the space freed ("Removed. 4.9 GB freed.").

| What | What Remove does | Catalog row afterwards |
|---|---|---|
| Built-in model (catalog) | Deletes its files from the model store | Stays, with **Download** (one click to get it back) |
| Model added from the Hub (GGUF, split GGUF or MLX) | Deletes its files | Stays, with **Download**. A second action, **Remove from list**, also drops it from the user catalog (`user-catalog.json`) |
| Copied from this computer | Deletes the copy | Dropped (it can't be downloaded again). The original file is untouched |
| Used where it is | Deletes nothing. OpenBark forgets it | Dropped. The file stays where it is |
| A download that stopped part-way | Deletes the `.part` files too | Stays |
| MLX engine pack | Deletes `<engines>/mlx/<lock id>/` and any leftover downloads in `<models>/mlx-engine-pack/` | The MLX engine card shows **Download** again. MLX models stay on disk, but can't write notes until the pack is back |
| GPU speech pack (ADR-0014, already exists) | As today; the card now also shows the space it uses | As today |

**Refusals.** Remove is refused, with a plain reason, when:
- the model is the one chosen to write notes on this device: "This model writes your notes. Choose another one under In use first." (The choice is a per-device setting, so the interface sends it with the request; the engine checks it.) The same applies to the MLX engine pack when the chosen note model is an MLX model;
- the model is loaded and a draft, refinement or template-from-example is using it: "A note is being written with this model. Remove it when that's finished." **A running draft is never cancelled.** A loaded model that is idle is unloaded first, then removed;
- the model is downloading: "This model is still downloading.";
- it is a speech model OpenBark needs to transcribe visits (the default recogniser, the second recogniser, voice detection, speaker separation and speaker voices): "OpenBark needs this to transcribe visits." The full-precision GPU recognisers are removed with the GPU speech pack, not one by one;
- OpenBark is changing where it keeps its files (the existing folder-change gate).

**Never outside the model store.**
- Only the files listed in the model's catalog entry (and their `.part` partial downloads) are deleted, never a whole folder blindly. Empty folders left behind are removed, up to but never including the models folder itself. Anything else someone put in the folder stays.
- Every path is built from the catalog entry, checked as a safe relative path (decision 7), then canonicalised; it must sit strictly inside the canonical models folder (or, for the MLX pack, the canonical engines folder), and must not be a symbolic link or junction. Otherwise nothing is deleted and the error says so.
- A model used in place is only forgotten; its path is never passed to any delete call.
- In web mode with a team set up, removing models or packs needs a veterinarian (as changing the team does).

Removal makes no network calls. Removal logic is tested only in temporary folders, never against a real models folder.

### 11. Minimum macOS for MLX

**macOS 14 Sonoma on Apple silicon** (M1 or later). That is the lowest platform tag of the mlx and mlx-metal wheels (`macosx_14_0_arm64`), and the MLX install guide says the same. OpenBark itself keeps its macOS 13 minimum for everything else.

## Privacy and network

- **New run-time row R6** in `docs/network.md`: only when the user chooses **Download** on the MLX engine card, GETs to `github.com` (redirecting to `release-assets.githubusercontent.com`) for the Python archive and to `files.pythonhosted.org` for the pinned wheels, with `User-Agent: OpenBark/<version>`. Nothing about the clinic is sent.
- **R1** gains the Hub's current storage hosts (`us.aws.cdn.hf.co`, seen live today) and the `/api/resolve-cache/` redirect on `huggingface.co`, MLX model files, and the gated-only token rule.
- **R4** gains the recursive, paged file listing, the small pinned `config.json` and index reads for MLX repos, the GGUF-alternatives query, the token for gated repos only, and `whoami-v2` once when a token is saved.
- **The worker makes no network calls and listens on nothing.** The CI source check (`scripts/ci/check-network.sh`) keeps passing: `os-llm` gains no HTTP client, because pack downloads go through `os-models`.
- **Patient text** reaches the worker only through its stdin pipe, and is never written to disk by it or logged.
- **Removing models, importing from this computer and saving the token's keychain entry** make no network calls. Checking a token is the single `whoami-v2` call above.
- Developer and CI test runs download the Linux CPU lock and a small test model (new build-time row B7).

## How it is tested without a Mac

1. **Unit tests, any platform:** Hub parsing and grouping on saved live responses (MLX trees, the 99-entry recursive listing, a masked gated tree), git blob SHA-1, safe paths, folder names, shard grouping, exclusions, local import checks, the lock file's shape (https only, `files.pythonhosted.org` and `github.com` only, 64-hex hashes, unique names), the worker protocol types, and the UI states with a mocked engine. **Removal** is tested only in temporary folders: listed files and `.part` files go, a stranger's file in the same folder stays, a symbolic link or junction pointing outside is refused, a model used in place is never touched, and the refusals (note model, busy, downloading, needed speech models) hold.
2. **Python worker tests in WSL:** a pinned virtual environment from the Linux lock; protocol framing, cancel, errors, the socket guard and the schema processor.
3. **Linux CPU end to end in WSL and on CI** (`OPENBARK_MLX_DEV=1`): install the Linux pack through OpenBark's own downloader, run the pack check, download `mlx-community/Qwen3-0.6B-4bit` (335 MB) through the Hub path, draft a note from a fixture under `unshare -rn` (no network at all), and check it validates against the template's schema. Also a demo server in WSL on port 4327 with `OPENBARK_PLATFORM=macos`, to click through the Mac screens.
4. **Golden schema test:** every built-in template's schema, plus the from-example and edit schemas, compile in llguidance.
5. **macOS CI smoke** (GitHub `macos-15`, arm64): install the real macOS pack from the lock, run the pack check under `sandbox-exec`, and draft with the 0.6B model. GitHub's Mac runners may expose no usable Metal device (ADR-0019); then the smoke runs on MLX's CPU device (`OPENBARK_MLX_DEVICE=cpu`, a developer switch) and records that Metal was not available, so it proves packaging, signing, the sandbox and the protocol, not Metal speed.
6. **On a real Mac (later, a volunteer or pilot):** Metal speed and memory for Gemma 4 E4B against llama.cpp Metal on the eval fixtures, the comparison ADR-0009 asked for. Until then the UI quotes no speeds for MLX.

## Consequences

- Mac users with Apple silicon and macOS 14 can download MLX models from the Hub or from their own disk and write notes with them, constrained to the template's schema like any other model.
- A second inference stack to maintain: a Python runtime and 35 wheels, refreshed by re-running the lock script. Bumps follow the pinning policy in `docs/versions.md`.
- About 366 MB on disk for the pack, in the data folder, removable.
- The first load of each MLX model takes a few seconds longer (the model check). The worker's start-up (importing transformers) adds a few seconds when the model is first loaded.
- The llguidance mask forces a sync every token, so MLX may be slower than its unconstrained benchmarks. Not measured on Metal yet.
- Split GGUFs, GGUFs in sub-folders and gated repos become usable everywhere. `mtp-` and other helper files stop being offered.
- Hub downloads on Windows work again (safe folder names). Existing Mac and Linux downloads keep their folders.
- Clinics can bring their own GGUF or MLX model without the Hub, copied or used where it is.
- Clinics can remove any model they downloaded or added, see the space it frees, and get a catalog model back in one click. The models needed to transcribe, and the model writing notes, can't be removed by mistake.
- A model used in place can go missing (a USB stick unplugged); the row says so and drafting refuses it, rather than failing mid-draft.
- `docs/network.md` (R1, R4, new R6 and B7), `docs/versions.md`, `NOTICE` and the user guide change.

## Alternatives considered

| Option | Why not |
|---|---|
| Keep labelling only (ADR-0009 as written) | The owner asked for MLX models to run |
| `mlx-rs` native binding (ADR-0009's preference) | Only Llama and Qwen3, unpublished, no constrained decoding, macOS only |
| mlx-swift-lm as a Swift sidecar | Needs Xcode builds, a second language and toolchain, and can't be built or tested on our Windows and WSL machines. Kept as the strongest native option if Python becomes a burden |
| mlx-c with our own model code | Every architecture and tokenizer rewritten by us |
| `mlx_lm.server` on 127.0.0.1 | A listening port, an HTTP API to secure, and constraints still unverified. Stdio is simpler and has no port |
| outlines or xgrammar for constraints | outlines pulls in pydantic, jsonschema and pillow; xgrammar requires torch |
| Unconstrained MLX generation, validated afterwards | Breaks ADR-0006's guarantee that drafts are always valid JSON |
| pip install from PyPI at run time | An index, resolver and `pip.conf` on the clinic's machine, and versions that change under us. Pinned wheels unzipped by Rust are reproducible |
| Rely on the Mac's own Python or Homebrew | Usually absent or the wrong version, and outside our pins |
| Ship the pack inside the app | Adds 106 MB to every Mac download for an opt-in feature, and the app bundle is read-only and signed |
| Only ever reference imported model files where they are | Breaks when a USB stick is removed or a folder moved; the store is the one place OpenBark manages (ADR-0013). Kept as an opt-in ("Use it where it is") for large files already on disk |
| Only ever copy imported models | Doubles the disk use of a 20 GB model that another app already keeps, which works against removing models to save space |
| Remove a model by deleting its whole folder | Would take anything else someone put there, and a symbolic link could point outside the store. Deleting only the catalog's listed files is exact |
| Drop built-in rows when removed | Getting a model back would mean finding it again. The row stays with Download |
| Cancel a running draft to remove its model | Loses the clinician's work. Removal waits; the draft is never interrupted |
| Save the Hugging Face token in a settings file | Plain text on disk. The keychain is already used for the data key |
| Send the token on every Hub request | It isn't needed for public repos, and R4 promises tokenless search |
