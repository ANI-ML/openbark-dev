# Installing and uninstalling OpenBark

These scripts install OpenBark for your own user account. They don't need administrator rights, and they never touch your encounters, models or settings. OpenBark opens in your web browser and is only reachable from this computer (`127.0.0.1`).

| | Install | Uninstall |
|---|---|---|
| **Windows** | Double-click **Install OpenBark.cmd** | Start menu > OpenBark > **Uninstall OpenBark**, or double-click **Uninstall OpenBark.cmd** |
| **macOS** | Double-click **Install OpenBark.command** (if macOS blocks it: right-click > Open) | Double-click **Uninstall OpenBark.command** |
| **Linux** | `./install/install.sh` | `~/.local/opt/openbark/uninstall.sh` |

## What the installer does

1. **Finds the program.** From an unzipped release (the scripts sit next to `openbark` and `ui/`), from a folder you name (`-From` / `--from`), or by building it from this source code. Building needs the one-time setup in [docs/runbooks](../docs/runbooks), and takes 10 to 20 minutes the first time. It picks graphics support automatically: Metal on Apple silicon, CUDA with an NVIDIA toolkit, Vulkan with the Vulkan SDK, otherwise the processor.
2. **Installs it** to:
   - Windows: `%LOCALAPPDATA%\Programs\OpenBark`, with Start menu and desktop shortcuts and `openbark` on your PATH;
   - macOS: `~/Applications/OpenBark.app`;
   - Linux: `~/.local/opt/openbark`, with an app-menu entry;
   - and, on macOS and Linux, a `~/.local/bin/openbark` link for the terminal.
3. **Optionally downloads the models** (about 6 GB): add `-Models` (Windows) or `--models`. Otherwise OpenBark offers to download them the first time you open it.

Opening OpenBark starts it in the background and opens your browser. **Stop OpenBark** (Start menu or app menu) stops it.

Running the installer again updates OpenBark in place.

## What the uninstaller does

It stops OpenBark and removes the program, its shortcuts and its PATH entry.

It then asks whether to **delete your data** too. Keeping it is the default: press Enter to keep it, or type `DELETE` to remove it. Your data is your encounters, recordings, notes, models, templates and team members:
- Windows: `%LOCALAPPDATA%\OpenBark`
- macOS: `~/Library/Application Support/OpenBark`
- Linux: `~/.local/share/OpenBark`

Deleting data also deletes this computer's encryption key, from the keychain and the key file. **Save your recovery key first** (Settings > Encryption > Show recovery key) if you keep copies of encounters anywhere else; without it they can't be opened.

For scripts: `uninstall.ps1 -Yes` (Windows) or `uninstall.sh --keep-data` keeps data without asking. `-RemoveData -Yes` or `--remove-data --yes` deletes it without asking.

## Options

| Windows (`install.ps1`) | macOS / Linux (`install.sh`) | |
|---|---|---|
| `-From <folder>` | `--from <folder>` | Install from a release folder |
| `-Models` | `--models` | Download the models now |
| `-Gpu cuda\|vulkan\|cpu` | `--gpu metal\|cuda\|vulkan\|cpu` | Graphics support when building from source |
| `-NoDesktopShortcut`, `-NoPath` | | Skip the desktop shortcut or PATH entry |

The desktop app (a native window, not the browser) is built separately with `pnpm tauri build`, which produces each platform's usual installer (MSI or EXE, DMG, DEB or AppImage) with its own uninstaller.
