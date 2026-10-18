# Runbook: run OpenBark on Windows from your own account

These steps are for the reference PC (RTX 4080), run from the person's own Windows login. The desktop window needs an interactive desktop session, so it can't be launched from an SSH session.

## Option A: web mode (no build needed if someone already built it)

The browser UI is served from the engine running on this PC:

```powershell
cd C:\path\to\openbark
.\target\release\openbark.exe serve          # then open http://localhost:4317
```

**What to try:**
1. Click **New from recording…** and choose `packages\eval\fixtures\audio\feline-vomiting.wav`. Leave **Label speakers** ticked.
2. Watch the transcript appear. Click any line to hear that moment.
3. Click **Generate note**, then **Copy**.
4. Use **Recent sessions…** to reopen an earlier session.

Each session is saved under `%LOCALAPPDATA%\OpenBark\sessions\<date>_<name>\` as `audio.wav`, `transcript.jsonl`, `session.json` and one folder per note under `runs\`.

## Option B: the desktop app

**One-time setup:**
- Rust (rustup)
- Visual Studio 2022 Build Tools with the C++ workload
- LLVM: set `LIBCLANG_PATH` to its `bin` folder
- Node 24
- the CUDA toolkit 12.9, if you have an NVIDIA GPU

`scripts\dev-env.sh` (Git Bash) sets up the paths.

```bash
source scripts/dev-env.sh
pnpm install
cd apps/desktop && pnpm tauri dev --features cuda     # drop --features cuda without an NVIDIA GPU
```

**Models:** each Windows account has its own model folder (`%LOCALAPPDATA%\OpenBark\models`). Either pull the models (`openbark models pull <id>` for each id in `openbark models list`), or point at an existing folder:

```powershell
$env:OPENBARK_MODELS_DIR = "C:\Users\<other user>\AppData\Local\OpenBark\models"
```

## Notes

- Keep the GPU free while benchmarking. A game running on the same GPU halves LLM speed (see `docs/perf/M0.md`).
- The PC won't go to sleep while a recording is being transcribed or a note is being generated. OpenBark holds a sleep blocker until the job finishes.
