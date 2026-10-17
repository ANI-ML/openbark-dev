# Runbook: build and benchmark OpenBark on a Mac

For the product owner's Apple Silicon Mac (M0 acceptance tests M0-1, M0-3 and M0-7 on macOS). It takes about 30–45 minutes, mostly unattended: the first build and the model downloads.

## 1. One-time setup

```bash
xcode-select --install                     # Apple command-line tools (clang, git)
brew install cmake gh                      # cmake for llama.cpp; gh to fetch test audio
curl https://sh.rustup.rs -sSf | sh -s -- -y --profile minimal
source "$HOME/.cargo/env"
brew install node@24 && corepack enable    # only needed for the desktop app (step 4)
gh auth login                              # the repo is private
```

## 2. Get the code and build the CLI

```bash
gh repo clone ANI-ML/openbark && cd openbark
cargo build --release -p os-cli --features metal     # first build ≈ 10–15 min
./target/release/openbark probe                       # should list a Metal device
```

`rust-toolchain.toml` pins Rust 1.99.1, and rustup installs it automatically on first build.

## 3. Benchmark (M0-3: a 10-minute consult drafted in ≤ 60 s)

```bash
scripts/bench.sh            # pulls models (~13 GB), runs E4B and 12B, saves JSON
```

Each run prints a one-line summary: audio length, speech-to-text time, LLM speed, total wall time, and whether the faithfulness check passed. The results land in `docs/perf/results/<your-mac>-<date>/`. To send them back, commit them on a branch:

```bash
git switch -c perf/$(hostname -s) && git add docs/perf/results && git commit -m "perf: M0 bench on $(sysctl -n machdep.cpu.brand_string)" && git push -u origin HEAD
```

Or just paste the summary lines into the chat.

## 4. Desktop app (M0-7)

```bash
pnpm install
pnpm desktop                # opens the OpenBark window (dev mode)
```

Click **Open recording…** and choose `packages/eval/fixtures/audio/feline-vomiting.wav`. Then click **Transcribe**, then **Generate note**. Check four things:
- the transcript streams in as it's recognised;
- the note fills the right-hand pane;
- **Copy** puts the note on the clipboard;
- the light/dark/system toggle works.

## If something fails

- **`cmake not found`**: run `brew install cmake` and open a new terminal.
- **No Metal device in `probe`**: the binary was built without `--features metal`. Rebuild.
- **Out of memory with 12B on a 16 GB Mac**: that's a data point, not a bug to work around. Note it, and run `scripts/bench.sh gemma-4-e4b-it-q4_0`.
