# Installing and uninstalling OpenBark

OpenBark installs like any other app, for your own user account. On Windows no administrator rights are needed. On macOS one exception is noted below. On Linux the .deb and .rpm packages ask for your password; the AppImage doesn't. Installing or uninstalling never touches your encounters, models or settings unless you ask it to.

| | Install | Uninstall |
|---|---|---|
| **Windows** | Double-click **OpenBark_<version>_x64-setup.exe** and follow the wizard | **Settings > Apps > Installed apps > OpenBark > Uninstall**, or Start menu > OpenBark > **Uninstall OpenBark** |
| **macOS** | Open **OpenBark_<version>_aarch64.dmg** and drag **OpenBark** onto **Applications** | Double-click **Uninstall OpenBark** in the same disk image, or drag OpenBark from Applications to the Trash |
| **Linux** | Double-click the **.deb** (Ubuntu, Debian, Mint) or **.rpm** (Fedora, openSUSE). It opens in your Software app: choose **Install**. Or make the **AppImage** executable and run it | Remove OpenBark in your Software app; for the AppImage, delete the file |

The installers come from the release page. The release workflow (`.github/workflows/release.yml`) builds all of them from a version tag.

## Windows: the setup wizard

1. **Language.** English, French, Spanish, German or Portuguese.
2. **Licence.** OpenBark is open source under the Apache 2.0 licence.
3. **Where to install.** The default, `%LOCALAPPDATA%\Programs\OpenBark`, is the usual place for per-user apps. Your data lives separately, in `%LOCALAPPDATA%\OpenBark`, so reinstalling or updating never touches it.
4. **Start menu folder**, then **Install**. The last page offers to open OpenBark.

If this Windows computer doesn't have Microsoft Edge WebView2 (Windows 10 or 11 almost always do), the wizard downloads it from Microsoft. Running a newer setup file over an older one updates OpenBark in place.

**Uninstalling** opens a wizard with one choice: **Delete the application data**. It's unticked, which keeps your encounters, recordings, notes, models, templates and team members. If you tick it, Windows asks once more, warning that encounters copied elsewhere can then only be opened with your recovery key. Only a **Yes** deletes `%LOCALAPPDATA%\OpenBark` and this computer's encryption key. A silent uninstall (`uninstall.exe /S`, used by IT tools) always keeps the data.

## macOS: the disk image

The disk image shows three icons: **OpenBark**, an **Applications** shortcut, and **Uninstall OpenBark**.

- **Install:** drag OpenBark onto Applications, then open it from Applications or Launchpad.
- **The first time you open it,** macOS may say it can't check OpenBark for malicious software, because this build isn't signed with an Apple Developer ID yet. Open **System Settings > Privacy & Security**, scroll down to the message about OpenBark, and choose **Open Anyway**. macOS only asks once.
- **Uninstall:** double-click **Uninstall OpenBark**. It asks first whether to go ahead, then whether to **Keep my data** (the default) or **Delete my data**. Deleting asks once more. The app is only removed after every question is answered. If OpenBark is in the main Applications folder and your account can't change it, macOS asks for an administrator's password.

Dragging OpenBark to the Trash also uninstalls it, and always keeps your data in `~/Library/Application Support/OpenBark`.

## Linux: .deb, .rpm and AppImage

The .deb and .rpm install OpenBark in your app menu and appear in your Software app, which also removes them. Removing the package keeps your data in `~/.local/share/OpenBark`. To delete it as well, close OpenBark, then run `rm -rf ~/.local/share/OpenBark` and remove the **OpenBark** entry from your keyring (Passwords and Keys, or KWallet). The AppImage runs from wherever you save it and installs nothing.

## Your data, and the recovery key

Your data is your encounters, recordings, notes, models, templates and team members:
- Windows: `%LOCALAPPDATA%\OpenBark`
- macOS: `~/Library/Application Support/OpenBark`
- Linux: `~/.local/share/OpenBark`

These are the usual places. If you chose other folders for visits or models (Settings > Storage), no uninstaller ever deletes those folders: delete them yourself if you no longer need them.

Every uninstaller keeps it unless you choose otherwise. Deleting your data also deletes this computer's encryption key. **Save your recovery key first** (Settings > Encryption > Show recovery key) if you keep copies of encounters anywhere else; without it they can't be opened.

## Signing

The Windows wizard and the macOS app aren't signed with a publisher certificate yet, so Windows SmartScreen may show **More info > Run anyway**, and macOS asks as described above. Buying a Windows code-signing certificate and an Apple Developer ID is the product owner's decision. The release workflow can sign once they exist.

## Advanced: install scripts (browser version, or building from source)

The scripts in this folder install the **browser version** of OpenBark (`openbark serve`, reachable only from this computer at `127.0.0.1`). Use them to build from source, or on a computer where the desktop app can't run.

| | Install | Uninstall |
|---|---|---|
| **Windows** | Double-click **Install OpenBark.cmd** | Start menu > OpenBark > **Uninstall OpenBark**, or double-click **Uninstall OpenBark.cmd** |
| **macOS** | Double-click **Install OpenBark.command** (if macOS blocks it: right-click > Open) | Double-click **Uninstall OpenBark.command** |
| **Linux** | `./install/install.sh` | `~/.local/opt/openbark/uninstall.sh` |

What the install script does:
1. **Finds the program.** From an unzipped release (the scripts sit next to `openbark` and `ui/`), from a folder you name (`-From` / `--from`), or by building it from this source code. Building needs the one-time setup in [docs/runbooks](../docs/runbooks), and takes 10 to 20 minutes the first time. It picks graphics support automatically: Metal on Apple silicon, CUDA with an NVIDIA toolkit, Vulkan with the Vulkan SDK, otherwise the processor.
2. **Installs it** to:
   - Windows: `%LOCALAPPDATA%\Programs\OpenBark`, with Start menu and desktop shortcuts and `openbark` on your PATH;
   - macOS: `~/Applications/OpenBark.app`;
   - Linux: `~/.local/opt/openbark`, with an app-menu entry;
   - and, on macOS and Linux, a `~/.local/bin/openbark` link for the terminal.
3. **Optionally downloads the models** (about 6 GB): add `-Models` (Windows) or `--models`. Otherwise OpenBark offers to download them the first time you open it.

Opening OpenBark starts it in the background and opens your browser. **Stop OpenBark** (Start menu or app menu) stops it. Running the installer again updates it in place.

The uninstall script stops OpenBark and removes the program, its shortcuts and its PATH entry. It then asks whether to delete your data too: press Enter to keep it, or type `DELETE` to remove it. For scripts: `uninstall.ps1 -Yes` (Windows) or `uninstall.sh --keep-data` keeps data without asking, and `-RemoveData -Yes` or `--remove-data --yes` deletes it without asking.

| Windows (`install.ps1`) | macOS / Linux (`install.sh`) | |
|---|---|---|
| `-From <folder>` | `--from <folder>` | Install from a release folder |
| `-Models` | `--models` | Download the models now |
| `-Gpu cuda\|vulkan\|cpu` | `--gpu metal\|cuda\|vulkan\|cpu` | Graphics support when building from source |
| `-NoDesktopShortcut`, `-NoPath` | | Skip the desktop shortcut or PATH entry |

The desktop app and the browser version use the same data folder, so you can switch between them.

## Building the installers

- **Windows:** `cd apps/desktop && pnpm tauri build --bundles nsis` (add `--features vulkan` or `--features cuda` for graphics). The wizard's template, uninstall hooks and artwork are in `apps/desktop/src-tauri/windows/`; `scripts/installer-art.py` redraws the artwork.
- **macOS:** `scripts/macos-dmg.sh` builds the app and the disk image with Uninstall OpenBark beside it ([runbook](../docs/runbooks/macos.md), step 5).
- **Linux:** `cd apps/desktop && pnpm tauri build --features vulkan` builds the .deb, .rpm and AppImage.
