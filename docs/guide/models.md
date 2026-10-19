# Models

OpenBark writes notes and transcribes with models that run on your computer. The **Models** screen (in **Settings** on a phone) lists them, with their size, licence and whether they fit your computer's memory. Decisions behind this page: [ADR-0025](../adr/0025-mlx-and-models-of-your-choice.md).

## Choosing a note model

Under **In use**, **Notes** is the model that writes your notes on this device. Only downloaded models are offered there. **What the model names mean**, at the top of the screen, explains labels such as E4B, Q4_K_M and "it", and suggests a size for your computer. In short:

- older laptops: Gemma 4 E2B (about 3 GB);
- most laptops: Gemma 4 E4B (about 5 GB), the everyday choice;
- a graphics card with 10 GB or more, or a Mac with 24 GB or more: Gemma 4 12B (about 7 GB).

The speech models are chosen for you. OpenBark needs them to transcribe visits, so they can't be removed.

## Downloading a model

In **Catalog**, choose **Download** beside a model. The row shows the progress, and an interrupted download picks up where it stopped. A row says how much is **partly downloaded** when a download stopped part-way: choose **Download** to finish it, or **Remove** to delete what came so far.

To find other note-writing models, choose **Browse Hugging Face**. Search, open a model, and choose **Download** beside the file you want. Some models come in several parts (for example "3 parts"); OpenBark downloads every part and shows them as one row. Files that aren't models on their own, such as vision helpers and draft files (`mmproj`, `mtp-`), aren't offered.

Searching and downloading contact huggingface.co only. Nothing about your patients or clinic is sent ([network.md](../network.md), R1 and R4).

## MLX models on a Mac

On a Mac with Apple silicon (M1 or later) and macOS 14 Sonoma or later, OpenBark can also run models converted for Apple's MLX framework, marked **MLX** in the Hugging Face browser.

1. On the **Models** screen, find the **MLX engine** card and choose **Download**. It's about 100 MB to download, and takes about 370 MB on disk. OpenBark sets it up and checks that it works on this Mac, then the card says **Ready**.
2. In **Browse Hugging Face**, open an MLX model. It shows as one row, **Whole model folder**, with its size and number of files. Choose **Download**.
3. Choose it under **In use** like any other model.

MLX notes follow your template's format exactly, as GGUF notes do. If a model can't be held to the format, OpenBark says so and won't use it for notes.

If an MLX model can't run, the browser says why: the engine isn't downloaded yet (**Get the MLX engine** takes you to the card), the kind of model isn't supported yet, or files are missing from the repository. It also lists **GGUF builds of this model**, which run with the built-in engine on any computer.

On an Intel Mac, or a Mac with macOS 13, the card explains what MLX needs. GGUF models work on every Mac. Windows and Linux use GGUF models only.

If the card says the engine **didn't pass its check**, choose **Try again**. GGUF models keep working either way.

## Gated models and your Hugging Face token

Some publishers ask you to request access before you download their models ("gated" models). When you open one, OpenBark says what to do:

1. Choose **Request access** and accept the publisher's terms on Hugging Face. Some publishers review requests by hand, which can take a while.
2. Choose **Get a token** and create a token on Hugging Face. A Read token is enough.
3. Paste it into **Hugging Face token** and choose **Save token**. OpenBark checks it with Hugging Face once and shows whose it is.

The token is kept in this computer's keychain (or, without one, encrypted on this computer, or only until OpenBark closes). It's never shown again, and it's sent only to huggingface.co and only for gated models. To change or remove it, use the **Hugging Face token** link at the bottom of the Hugging Face browser. If a server sets the token (`OPENBARK_HF_TOKEN`), the app says **Set by the server**.

## Adding a model from this computer

Choose **Add from this computer** on the Models screen.

- In the desktop app, choose **Choose a .gguf file**, or on a Mac, **Choose an MLX folder**. For a model in parts, choose any part: OpenBark brings the others from the same folder.
- In the browser version, type the full path of the file or folder on the computer running OpenBark, and choose **Check**.

OpenBark shows the name, size and files before anything changes. Then choose how to add it:

- **Copy into OpenBark's models folder (recommended).** The model keeps working if the original moves. On the same drive, OpenBark links it instead, so it takes no extra space.
- **Use it where it is, don't copy.** Useful for a large model another app already keeps. OpenBark never changes or deletes the file. If it moves, the row says **Missing**: add it again.

Choose **Add**. Added models show **This computer** as their publisher. Adding a model makes no network calls. When a team is set up, only a veterinarian can add or remove models, or change the token.

## Removing a model and getting it back

Choose **Remove** beside a model to free its space. The row shows the space it uses ("on disk"), and OpenBark says how much was freed afterwards.

| What | What Remove does | Getting it back |
|---|---|---|
| A built-in model | Deletes its files | The row stays: choose **Download** |
| A model from Hugging Face | Deletes its files. Tick **Also remove it from the list** to drop the row too | Choose **Download**, or find it again in **Browse Hugging Face** |
| A copy added from this computer | Deletes OpenBark's copy. The original stays where it was | **Add from this computer** again |
| A model used where it is | **Forget**: OpenBark stops using it and deletes nothing | **Add from this computer** again |
| A download that stopped part-way | Deletes what was downloaded so far | Choose **Download** |
| The MLX engine | Deletes the engine. MLX models stay, but can't write notes until it's back | **Download** on the MLX engine card |
| The GPU speech pack | Deletes the pack and its two full-precision models | **Download** on its card |

OpenBark only deletes the files it downloaded or copied for that model, inside its models folder. Remove is refused, with the reason, when:

- the model writes your notes on this device: choose another one under **In use** first;
- a note is being written with it: remove it when that's finished (a draft is never interrupted);
- it's still downloading;
- OpenBark needs it to transcribe visits.

Removing makes no network calls.

## From the command line

Whoever looks after the computer can do the same with the `openbark` command:

| Command | What it does |
|---|---|
| `openbark models list` | Every model, and whether it's on this computer |
| `openbark models add-hf <repo> <file>` | Add a GGUF from a Hugging Face repo (part 1 of a split GGUF), or `@mlx` for an MLX model folder. Then `openbark models pull <id>` downloads it |
| `openbark models import <path> [--in-place]` | Add a `.gguf` file or an MLX folder from this computer, copied or used where it is |
| `openbark models rm <id> [--forget]` | Remove a model, as **Remove** does. `--forget` also drops a Hugging Face model from the list |
| `openbark mlx install`, `check`, `remove`, `status` | The MLX engine, on a Mac with Apple silicon and macOS 14 or later |

`--llm <id>` on `openbark note`, `run` and `rerun` takes any of these models, MLX ones included. The command line doesn't know which note model each device chose, so `models rm` only refuses the speech models OpenBark needs.
