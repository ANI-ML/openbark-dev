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

Each run prints a one-line summary: audio length, speech-to-text time, LLM speed, total wall time, and whether the faithfulness check passed. The results land in `docs/perf/results/<label>-<date>/` (set `BENCH_NAME` first, for example `BENCH_NAME=m3-pro-36gb`). To send them back, commit them on a branch:

```bash
git switch -c "perf/$BENCH_NAME" && git add docs/perf/results && git commit -m "perf: M0 bench on $(sysctl -n machdep.cpu.brand_string)" && git push -u origin HEAD
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

## 5. The disk image and the uninstaller

```bash
scripts/macos-dmg.sh        # builds the app, then target/release/bundle/dmg/OpenBark_<version>_aarch64.dmg
open target/release/bundle/dmg/*.dmg
```

Check, in this order:
1. The window shows **OpenBark**, an **Applications** shortcut and **Uninstall OpenBark**, with nothing overlapping.
2. Drag OpenBark onto Applications and open it. If macOS blocks it, go to **System Settings > Privacy & Security > Open Anyway**. Then quit OpenBark.
3. Double-click **Uninstall OpenBark** and choose **Continue**, then **Keep my data**. Expect the app to be gone from Applications, `~/Library/Application Support/OpenBark` to still be there, and a final message that says so.
4. **Test "Delete my data" only in a separate macOS user account that has no real data** (System Settings > Users & Groups > Add User). Install OpenBark there, open it once, then uninstall with **Delete my data**, then **Delete**. Expect its `Application Support/OpenBark` folder and its keychain item **OpenBark** (Keychain Access) to be gone. Never try this in the account that holds the clinic's encounters.

Note anything that looks wrong (dialog wording, icon positions, a second password prompt) in the chat.

## 6. Quit and replay (the two 0.1.1 bugs)

0.1.1 crashed on Cmd-Q after a note had been drafted (a "quit unexpectedly" report naming `ggml_abort` and `ggml_metal_rsets_free`), and recordings showed an error instead of playing in Review. Both are checked on GitHub's Macs by the **Quit and replay checks** workflow (`.github/workflows/macos-repro.yml`). On your Mac, with a build that has the fix:

1. Open OpenBark and draft a note for any visit, so the note model is loaded.
2. Press Cmd-Q while the model is loaded, and once more in the middle of a draft. Expect OpenBark to close within a few seconds with no "quit unexpectedly" dialog.
3. Open the visit again. In Review, play the recording, drag the slider near the end, and let it play out. Try a long visit (over 30 minutes) and an imported file too.
4. If either fails, send the crash report from **Console > Crash Reports** (or `~/Library/Logs/DiagnosticReports/OpenBark-*.ips`) and say which visit and how long its recording is.

## If something fails

- **`cmake not found`**: run `brew install cmake` and open a new terminal.
- **No Metal device in `probe`**: the binary was built without `--features metal`. Rebuild.
- **The disk image has no icon layout**: the Finder step timed out, and the script built it without positions. Run `scripts/macos-dmg.sh --skip-build` again with Finder idle.
- **Out of memory with 12B on a 16 GB Mac**: that's a data point, not a bug to work around. Note it, and run `scripts/bench.sh gemma-4-e4b-it-q4_0`.
