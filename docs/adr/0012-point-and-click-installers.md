# ADR-0012: Point-and-click installers and uninstallers on every desktop platform

- Status: accepted (Windows built and tested; macOS and Linux build in the release workflow, and the macOS disk image needs the runbook check)
- Date: 2026-10-18
- Deciders: product owner (asked for GUI installers and uninstallers that are easy to understand), engineering

## Context

The first installers were scripts (`install/`): double-click a `.cmd` or `.command`, and a terminal window walks through the steps and asks "type DELETE" to remove data. They install the browser version (`openbark serve`). Clinic staff expect an installer that looks like every other app's, and a terminal window is not that.

Tauri's bundler already makes each platform's usual installer for the desktop app. What it doesn't do by default:
- **Windows:** a per-user install goes to `%LOCALAPPDATA%\<product>`. For OpenBark that is `%LOCALAPPDATA%\OpenBark`, the folder holding the clinic's encounters, models and key. Uninstalling or updating the program from there would put the data at risk.
- **Windows:** the stock uninstaller's "Delete the application data" box removes only the WebView's folders, not OpenBark's data or its key in Credential Manager.
- **macOS:** a disk image has no uninstaller. Dragging to the Trash keeps data, which is right, but there's no clear way to remove the data when that's what the clinic wants.

## Decision

1. **Windows: the NSIS setup wizard, per-user, no administrator rights.** Tauri's template with one change: install to `%LOCALAPPDATA%\Programs\OpenBark`, the usual place for per-user apps, so the program and the data never share a folder. Five languages, the licence page, OpenBark artwork, and **Uninstall OpenBark** in the Start menu folder beside the app. The WebView2 bootstrapper downloads only if WebView2 is missing (docs/network.md).
2. **Uninstalling keeps data unless the user chooses otherwise, twice.** On Windows, the uninstaller's box is unticked by default, and ticking it brings up a second Yes/No (default No) before `%LOCALAPPDATA%\OpenBark` and the Credential Manager key are deleted. A silent or update uninstall never deletes data.
3. **macOS: a disk image with Uninstall OpenBark beside the app.** `scripts/macos-dmg.sh` builds the app with Tauri, then lays out the image with create-dmg: OpenBark, an Applications shortcut, and a small **Uninstall OpenBark** app that uses ordinary macOS dialogs. It follows the same rules as Windows: keep by default, delete only after a second confirmation, nothing changed until every question is answered. The app is ad-hoc signed (`signingIdentity: "-"`), so macOS shows **Open Anyway** rather than calling the app damaged.
4. **Linux: .deb, .rpm and AppImage,** installed and removed through the Software app. Packages can't touch home folders, so removal keeps data, and the README gives the one command to delete it.
5. **The scripts stay, for the browser version and building from source.** The README puts them under "Advanced".
6. **Not decided here: code signing.** A Windows code-signing certificate and an Apple Developer ID (with notarization) remove the SmartScreen and Gatekeeper prompts. Buying them is the product owner's decision. The release workflow can sign once they exist.

## Consequences

- The Windows wizard is tested on the dev box: a silent install puts the app and its four sherpa-onnx DLLs in `%LOCALAPPDATA%\Programs\OpenBark`; installing over it updates in place; a silent uninstall removes the program, its shortcuts and its Apps entry, and leaves a 13.9 GB data folder byte-for-byte unchanged. The delete-data path is **not** run on the dev box, which holds real data. It needs a VM or a separate Windows account.
- The macOS uninstaller's decisions are tested with stand-in dialogs (seven paths, including cancelling at each step). The real dialogs, the disk image layout and the delete path in a separate macOS account are in the Mac runbook, step 5.
- `installer.nsi` is a copy of tauri-cli's template, so it has to be refreshed when tauri-cli changes its template (docs/versions.md).
- The Linux packages and the macOS image build only on the release workflow (tags or a manual run), so a broken Linux or macOS build shows up there, not on the dev box.
