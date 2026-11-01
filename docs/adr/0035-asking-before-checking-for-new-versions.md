# ADR-0035: Asking once before checking for new versions

- Status: accepted
- Date: 2026-10-31
- Deciders: product owner, engineering
- Relations: meets the conditions ADR-0020 set for an updater ("its own ADR, a docs/network.md row and an off switch"), but this is a version check, not an updater: nothing is downloaded or installed, so ADR-0020's signing rules and "no updater key" still hold. Adds row R16 to docs/network.md. Changes the "no update checks" line of docs/network.md, the README and the website: there is now one, off until the person says yes.

## Context

OpenBark has no way to tell anyone a new version is out. Clinics find out from the website or not at all, and a clinic running an old version misses its fixes.

The app goes online only when the person chooses: every run-time row in docs/network.md is something the person starts (a model download, a Hugging Face search, connecting Twilio, turning on sync). A check that runs by itself would be the first automatic connection, and the privacy page, the README and the About screen all say there is none.

The website already publishes what a check needs. `website/public/release.json` (written by `scripts/website-release.py` on each release) holds the version, whether it is published, and the release notes page; Vercel serves it from `https://www.openbark.io/release.json` with `Cache-Control: no-cache`. The bare `openbark.io` redirects there.

On 2026-10-31 the product owner decided: **ask once**. The automatic check stays off until the person says yes.

## Decision

### 1. Ask once, then respect the answer

- A one-time card on a computer: "Check for new versions? Once a day, OpenBark reads the version number from openbark.io. Nothing about you, your clinic or your patients is sent." with **Yes, check daily** and **No thanks**, given equal weight.
- It appears after first-run setup is finished: the consent notice accepted, with a team the person chosen, and on a computer the setup walkthrough (ADR-0034) finished or skipped, or not needed because every model was already here. It never appears over the walkthrough. Existing users see it once after updating, because they have no answer saved yet.
- It is a card in the corner, not a dialog, so it can be ignored. It is remembered as shown the moment it appears and isn't shown again; ignored, nothing turns on.
- The answer is the setting `updateCheck` (`"ask" | "on" | "off"`, `apps/desktop/src/context.ts`), the one source of truth. "ask" checks nothing. Anything else that asks the same question (the first-run setup, later) reuses `UpdateQuestion` and `updateAnswer()` from `components/UpdateCheck.tsx` and stores the same setting.
- Settings, About has the same choice as a switch, **Check for new versions once a day**, with the same explanation, beside the version number.
- Never on the iPhone, where the App Store and TestFlight handle updates: no card, no switch, no check (the interface hides it and the engine refuses on iOS).

### 2. The check

- **When:** only while the setting is on. At start, and hourly while OpenBark is open, the interface asks the engine; the engine reaches openbark.io only if 24 hours have passed since its last request. The time of the last request is kept in `<local data>/OpenBark/update-check.json` (with the last version found), so restarting doesn't check again, and in web mode a second tab doesn't either. The time is written before the request goes out, so a failing or slow answer still counts as the day's check.
- **What:** one HTTPS GET of `https://www.openbark.io/release.json` with `User-Agent: OpenBark/<version>`. No query string, no cookies (no cookie store), no id, no token. reqwest adds its standard `Accept: */*`, and HTTP needs `Host`; a test on a loopback server checks that these three are all that is sent.
- At most one redirect is followed, and only to that same address. Connect timeout 5 s, total 10 s. An answer other than 200, over 64 KB, not JSON, whose `published` isn't `true` or whose version isn't `MAJOR.MINOR.PATCH` announces nothing. Offline, the automatic check fails silently.
- **Where:** in Rust (`engine/os-models/src/update.rs`), never in the WebView, so the main window's Content-Security-Policy stays "this computer only". os-models already holds an HTTP client for model downloads, so no new crate gets one; `scripts/ci/check-network.sh` also fails if any other source file names the release address. When and whether to ask is `engine/os-app/src/update.rs`.
- **Web mode** (`openbark serve`): the server makes the request when the page asks (`POST /api/update/check`), never the browser, whose policy is unchanged. The setting lives in each browser, like the other settings; the 24-hour limit is per computer.
- Versions compare number by number (0.2.10 is newer than 0.2.9).

### 3. The notice

- For a newer published version: the app's toast, "OpenBark X.Y.Z is available", with **What's new** and **Download**. It stays 12 seconds and, like every toast, holds while the pointer or focus is on it.
- **What's new** opens the release notes page from `release.json`, only if it is an https page on www.openbark.io or in ANI-ML/openbark-releases on GitHub (otherwise the website's version history). **Download** opens `https://www.openbark.io/#download`, which offers the installer for the computer. On a computer the engine opens the address it checked itself (the WebView passes only "notes" or "download"); in web mode the page opens it in a new tab.
- Once per version: shown is seen, whether it was used, closed or timed out. It returns only for a newer version.
- Nothing is downloaded or installed automatically.

### 4. Check for updates

Settings, About has **Check for updates**, which always works, even with the daily check off or never answered: pressing it is the person's choice. It says "You have the latest version", or shows the same notice (again, if it was shown before), or that openbark.io couldn't be reached.

## Consequences

- OpenBark's first automatic connection, but only after a yes, and it carries nothing about the clinic. The website host (Vercel) sees the computer's internet address and the version in the User-Agent, as with any visit to the site.
- "Nothing leaves this device" in Settings, About becomes conditional: with the check on it says "Nothing about you, your clinic or your patients leaves this device. Once a day, OpenBark reads the latest version number from openbark.io." docs/network.md, the README, the features list and the privacy page say the same.
- `release.json` is now read by installed apps, not only by the website. Its `version`, `published` and `notes` fields must keep their meaning; `scripts/website-release.py` writes them. Setting `published` to false stops every notice.
- The e2e and screenshot scripts set `updateCheck: "off"` so the card doesn't cover what they click or capture.
- Later, if wanted: a signed in-app updater would need its own ADR (ADR-0020's updater key), and this ADR's ask-once setting would be its consent.

## Alternatives considered

| Option | Why not |
|---|---|
| Check by default, with an off switch | The product owner chose to ask first (2026-10-31): every other connection is the person's choice |
| Never check; rely on the website | Clinics on old versions miss fixes |
| Fetch from the WebView | Needs openbark.io in the main window's Content-Security-Policy, which today allows only this computer |
| Tauri's updater plugin | Downloads and installs, needs an updater key and signed update files (ADR-0020); more than telling people a version is out |
| The GitHub releases API | A second host, rate limits, and a larger answer than one small file the project already publishes |
| Show the question as a dialog | Blocks work; the person should be able to ignore it |
