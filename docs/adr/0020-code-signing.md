# ADR-0020: Code signing for the installers and the iPhone app

- Status: proposed
- Date: 2026-10-18
- Deciders: product owner (asked on 2026-10-18 that engineering work out code signing), engineering
- Closes: ADR-0012 decision 6 (code signing was left open). Builds on ADR-0018 section 8 (iPhone signing on GitHub's macOS runners).
- Runbook: [docs/runbooks/code-signing.md](../runbooks/code-signing.md)

## Context

### What users see today

| Platform | What happens on first install | Why |
|---|---|---|
| Windows | "Windows protected your PC" with **Unknown publisher**. The user clicks More info, then Run anyway. On a Windows 11 PC with Smart App Control on, the app can be blocked outright | Nothing is signed. The installer, the app, `uninstall.exe` and the four sherpa-onnx and ONNX Runtime DLLs all report NotSigned |
| macOS | Gatekeeper blocks the first launch. The user goes to System Settings > Privacy & Security > **Open Anyway** (docs/runbooks/macos.md step 5) | Ad-hoc signature (`signingIdentity: "-"`) and no notarization. The uninstaller is signed ad-hoc with `\|\| true`, so it can ship unsigned without anyone noticing |
| Linux | No prompt | Linux doesn't check downloaded packages. Users can compare `SHA256SUMS.txt`, but nothing proves who made that file |
| iPhone | Not built yet | TestFlight and the App Store need Apple signing (ADR-0018) |

Clinic staff are trained not to run software from an unknown publisher. Some clinics' IT policies block it. "Unknown publisher" on a clinical tool costs trust before the app has even opened.

### Facts that shape the decision

Checked live on 2026-10-18 against Microsoft Learn, the Azure retail price list, Apple Developer documentation, the Tauri 2 docs and tauri-bundler 2.10.3 source. Anything not confirmed is marked "to confirm" or "believed".

**Windows**
- Microsoft's signing service is now called **Artifact Signing** (formerly Trusted Signing). The Azure price list still files it under the old name.

  | Tier | Price | Included |
  |---|---|---|
  | Basic | US$9.99 a month (CA$14.15) | 5,000 signatures a month, one certificate profile of each type |
  | Premium | US$99.99 a month (CA$141.64) | 100,000 signatures a month, ten profiles of each type |
  | Extra signatures | US$0.005 each | |

- Billing starts when the signing account is created and isn't pro-rated. Free, trial and sponsored Azure subscriptions are refused, so a pay-as-you-go subscription is needed.
- One OpenBark release needs about 10 to 15 signatures. Basic is plenty.
- Public Trust certificates are open to organisations in Canada (and the US, EU, UK and others). Individual developers must be in the US or Canada.
- **Business age, to confirm.** The official pages state no minimum business age. During the 2024-2025 preview, Microsoft staff answers on Q&A said organisations under 3 years old couldn't be onboarded. We don't know whether that still applies. A small UK company reported a successful organisation validation in July 2026.
- An organisation's certificate carries only the validated legal name, city, province and country. A custom name isn't allowed. An individual's certificate carries the person's legal name and town.
- Identity validation is done only in the Azure portal. It takes 1 to 20 business days and can't be expedited. Any documents must be issued within the last 12 months, and there are three upload attempts. Renewal opens 60 days before expiry and repeats the full review. How long a validation lasts is shown in the portal (term to confirm).
- Certificates last three days and rotate daily, so every signature must carry a timestamp. Keys stay in Microsoft's hardware security modules (FIPS 140-3 Level 3). Only file digests leave the build machine.
- Microsoft's SmartScreen page (2026-05-04) says: "EV certificates no longer bypass SmartScreen." With any valid signature, the first downloads still show an "unrecognized app" warning, but with the verified publisher name. Reputation builds with downloads, "several weeks and hundreds of clean installs", and carries over to new versions signed with the same identity. Smart App Control blocks unsigned files that have no reputation, and its checks apply to all executable files, DLLs included.
- Since 1 March 2026, certificates from commercial certificate authorities last at most 460 days, and their keys must be on hardware or in a cloud HSM. The cheapest workable route found is SSL.com OV (US$129 a year) with eSigner cloud signing (US$180 a year for 240 signings, then US$1 each). That is about US$309 a year.
- **Tauri signs file by file** through `bundle.windows.signCommand`. It signs the app's .exe, the four DLLs in `bundle.resources`, the NSIS plugins, the uninstaller (through the `!uninstfinalize` hook that our `installer.nsi` keeps) and finally the installer. Signing only the finished installer afterwards would leave the app and uninstaller inside it unsigned.

**macOS**
- Apps distributed outside the App Store need a **Developer ID Application** certificate, the hardened runtime, a secure timestamp and notarization by Apple.
- Only the Account Holder can create Developer ID certificates, up to five. The validity period isn't stated on Apple's pages; it is believed to be five years and shows in the portal. If a certificate expires, apps already signed with it keep working.
- The owner already pays for the Apple Developer Program (US$99 a year, charged in local currency). **Whether the membership is individual or organisation is not confirmed.**
  - An individual membership signs as the person's legal name and lists the person as the App Store seller.
  - An organisation membership uses the legal entity name. It needs a D-U-N-S number: free, up to 5 business days from Dun & Bradstreet, then up to 2 business days for Apple to receive it.
  - Apple converts an individual membership to an organisation on request ("please contact us"). It doesn't say how long that takes or whether the team ID is kept (to confirm).
- Tauri 2 reads these variables:
  - signing: `APPLE_SIGNING_IDENTITY`, `APPLE_CERTIFICATE` (base64 `.p12`) and `APPLE_CERTIFICATE_PASSWORD`;
  - notarization: `APPLE_API_KEY`, `APPLE_API_ISSUER` and `APPLE_API_KEY_PATH`, or `APPLE_ID`, `APPLE_PASSWORD` and `APPLE_TEAM_ID`.

  The `APPLE_SIGNING_IDENTITY` variable overrides the committed `"-"`, so local builds stay ad-hoc. Tauri signs the app inside-out, notarizes it and staples the ticket.
- Tauri doesn't notarize our disk image. `scripts/macos-dmg.sh` builds the DMG itself with create-dmg, so the script has to sign, notarize and staple it.
- Tauri turns the hardened runtime on by default. Under the hardened runtime, recording from the microphone needs the `com.apple.security.device.audio-input` entitlement, and we don't set it. So today's ad-hoc build may already fail to record on a real Mac. No one has tested on one (believed).
- The uninstaller's main executable is a bash script. Apple's developer forums report notarization and stapling trouble with script executables (believed).

**Linux**
- apt checks the signature of a repository, not of a single `.deb`.
- Tauri can embed a signature in the AppImage, but its docs say "AppImage does not validate the signature".
- Tauri signs `.rpm` files when `TAURI_SIGNING_RPM_KEY` and `TAURI_SIGNING_RPM_KEY_PASSPHRASE` are set. This is in the tauri-bundler 2.10.3 source but not in Tauri's docs.

**iPhone**
- TestFlight needs an **Apple Distribution** certificate (Account Holder or Admin) and an App Store Connect provisioning profile for `health.animl.openbark`.
- Tauri's iOS guide signs automatically with `APPLE_API_ISSUER`, `APPLE_API_KEY` and `APPLE_API_KEY_PATH`, using a key with Admin access. Manual signing uses `IOS_CERTIFICATE`, `IOS_CERTIFICATE_PASSWORD` and `IOS_MOBILE_PROVISION`, all base64.

**GitHub**
- The main repo (ANI-ML/openbark) is private. On a private repo:
  - environments, environment secrets and tag rules need GitHub Pro, Team or Enterprise;
  - required reviewers need Enterprise.

  Which plan the ANI-ML organisation is on is to confirm.
- GitHub's sign-in token for Azure (OIDC) is valid for about 5 minutes (Azure/login issue 441). So the Azure login must come just before signing, not before a 30-minute Rust build.

**Constraints**
- Public material names ANI.ML Health Inc. or the ANI.ML Health team, never an individual. A code signature is public: anyone can read it in a file's properties.
- Local-first: signing happens on CI at build time. The app itself never contacts a signing service.
- Local builds and manual workflow runs must keep working with no secrets.

## Decision

### 1. One identity everywhere: ANI.ML Health Inc.

Every signature names the company. We don't use individual validation at Microsoft. We don't sign with an individual Apple membership. If the Apple membership turns out to be individual, it is converted to an organisation before any Developer ID certificate is made.

### 2. Windows: Microsoft Artifact Signing, Basic tier

| | |
|---|---|
| What we sign | Everything Tauri builds into the setup wizard: the app's .exe, `sherpa-onnx-c-api.dll`, `sherpa-onnx-cxx-api.dll`, `onnxruntime.dll`, `onnxruntime_providers_shared.dll`, the NSIS plugins, `uninstall.exe` and the installer. Later, the `openbark` CLI and our own `ggml-cuda.dll` (ADR-0019), signed before their SHA-256 pins are computed |
| How | Tauri's `bundle.windows.signCommand` runs `signtool` with Microsoft's Artifact Signing dlib, SHA-256 digests and the timestamp server `http://timestamp.acs.microsoft.com`. The command lives in a config overlay that only the release workflow passes (`--config`), so local builds stay unsigned |
| Identity | Organisation, Public Trust, validated as ANI.ML Health Inc. Certificate profile without street address or postal code |
| Region | East US (`https://eus.codesigning.azure.net`). There is no Canadian region, and only file digests are sent |
| Login from CI | GitHub OIDC with an Entra app registration whose federated credential trusts only `repo:ANI-ML/openbark:environment:release`. The app holds one role, Artifact Signing Certificate Profile Signer. No long-lived Azure secret is stored |
| Build order | `tauri build --no-bundle`, then `azure/login`, then `tauri bundle --bundles nsis` with the signing overlay. The login lands inside the 5-minute window |
| Cost | US$9.99 a month (CA$14.15), about US$120 (CA$170) a year before tax. Azure pay-as-you-go subscription, no other fixed fee |
| Secrets | `AZURE_CLIENT_ID`, `AZURE_TENANT_ID`, `AZURE_SUBSCRIPTION_ID` (secrets). `AZURE_SIGNING_ENDPOINT`, `AZURE_SIGNING_ACCOUNT`, `AZURE_SIGNING_PROFILE` (variables) |

We don't sign the NVIDIA GPU pack's files (ADR-0014). They are NVIDIA's and sherpa-onnx's, pinned by URL and SHA-256. Re-signing would change their hashes and mean re-hosting them, and NVIDIA's licence terms for modified files are unchecked. Installing the pack replaces the signed `onnxruntime.dll` with sherpa-onnx's unsigned CUDA build. See Consequences.

### 3. macOS: Developer ID, hardened runtime, notarized and stapled

| | |
|---|---|
| What we sign | `OpenBark.app` (Tauri signs nested code first), `Uninstall OpenBark.app` and the disk image |
| Certificate | Developer ID Application, team ANI.ML Health Inc. Made by the owner as Account Holder from a certificate request generated with OpenSSL on Windows. No Mac needed |
| Hardened runtime | On (Tauri's default). A new `Entitlements.plist` grants only `com.apple.security.device.audio-input`. No JIT, unsigned-memory or library-validation exceptions: sherpa-onnx is linked statically on macOS, and nothing loads third-party dylibs (believed; checked again on the first signed build) |
| Notarization | App Store Connect API team key, role Developer (believed enough for notarytool, to confirm). Tauri notarizes and staples the app. `macos-dmg.sh` notarizes and staples the uninstaller, then signs, notarizes and staples the disk image. Stapling lets the first launch work on a clinic Mac that is offline |
| Uninstaller | Gets a small compiled launcher as its main executable, which runs the existing script from `Contents/Resources`. Signed with the Developer ID and the hardened runtime. No more `\|\| true` in signed builds |
| Cost | Nothing new. The Apple Developer Program (US$99 a year in local currency) is already paid. Notarization waits add a few macOS runner minutes per release, under US$1 at US$0.062 a minute (ADR-0018) |
| Secrets | `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_API_KEY`, `APPLE_API_ISSUER`, `APPLE_API_PRIVATE_KEY` (the `.p8` text, written to a temporary file for `APPLE_API_KEY_PATH`). `APPLE_TEAM_ID` as a variable |

### 4. Linux: a signed checksum file, signed RPMs

| | |
|---|---|
| What we sign | `SHA256SUMS.txt`, with a detached signature `SHA256SUMS.txt.asc` made in the `public-draft` job after every other platform has signed. It covers the Windows and macOS downloads too. The `.rpm` files are signed through Tauri's `TAURI_SIGNING_RPM_KEY` (to confirm on the first run). The `.deb` and AppImage stay as they are |
| Key | One OpenPGP key, "OpenBark Releases (ANI.ML Health Inc.)", RSA 4096 for the widest `rpm` and `gpg` support, signing only, expiring after 3 years and extended before then. The public key and its fingerprint go on www.openbark.io and in ANI-ML/openbark-releases |
| Cost | Free |
| Secrets | `OPENBARK_RELEASE_GPG_KEY` (armoured private key), `OPENBARK_RELEASE_GPG_PASSPHRASE`. `OPENBARK_RELEASE_GPG_FINGERPRINT` as a variable |

A signed apt or rpm repository, or Flathub, can come later if clinics ask. A repository adds a host that machines contact on every update, so it needs its own docs/network.md row.

### 5. iPhone: the same Apple team, an Apple Distribution certificate

| | |
|---|---|
| What we sign | The iOS app for TestFlight and, later, the App Store (ADR-0018 section 8) |
| Certificate | Apple Distribution, same team. Validity believed to be one year (to confirm). An App Store Connect provisioning profile for `health.animl.openbark` |
| Key | A second App Store Connect team key with Admin access, as Tauri's iOS guide asks, used only in the tag-gated iOS job. Keeping it apart from the notarization key limits what a leaked desktop key could do |
| Cost | Nothing new |
| Secrets | `IOS_CERTIFICATE`, `IOS_CERTIFICATE_PASSWORD`, `IOS_MOBILE_PROVISION` (if manual signing), `IOS_API_KEY`, `IOS_API_ISSUER`, `IOS_API_PRIVATE_KEY`. The iOS job maps the last three to Tauri's `APPLE_API_*` names |

The App Store seller name comes from the membership. An organisation membership shows ANI.ML Health Inc. Don't register the App ID or create the App Store Connect app record until the membership is an organisation, in case Apple asks for a new enrolment rather than converting the existing one.

### 6. Where secrets live

- **GitHub:** encrypted Actions secrets in an environment named `release` on ANI-ML/openbark. The environment accepts only tags matching `v*`. If the plan has no environments for private repos, they become repository secrets instead (decision 8).
- **Originals and backups:** the `.p12`, the certificate request key, the `.p8` files, the OpenPGP private key and its revocation certificate, and every password go into the company password manager (which one is to confirm). Nothing goes in the repo or on a shared drive.
- **Windows keys** never leave Microsoft's HSMs. No Azure secret exists to leak.
- **Who holds what:** the owner holds the Azure Identity Verifier role and the Apple Account Holder role. Engineering doesn't need either to maintain the workflow.

### 7. When signing runs

- Only for version tags (`refs/tags/v*`). Manual runs and local builds stay unsigned on Windows and ad-hoc on macOS, and need no secrets.
- A tag ruleset lets only repository admins create, move or delete `v*` tags, because a tag is what unlocks the signing secrets.
- After signing, CI checks every signature and fails the release if a file is unsigned, untimestamped or not notarized:
  - Windows: `signtool verify /pa` on a silent install;
  - macOS: `codesign --verify --strict`, `spctl` and `stapler validate`;
  - Linux and checksums: `gpg --verify` and `rpm -K`.

### 8. Fallbacks

| If | Then | Cost |
|---|---|---|
| Microsoft refuses the organisation (for example on business age) | An OV certificate from SSL.com with eSigner cloud signing, run through the same `signCommand` hook. Validity is at most 460 days, so it is re-validated about every 15 months | About US$309 a year |
| The GitHub plan has no environments for private repos | Repository secrets, signing gated by the tag condition alone, and Tauri's documented `artifact-signing-cli` with an Entra client secret (`AZURE_CLIENT_SECRET`, 12-month expiry, diarised) | Free |
| Converting the Apple membership takes weeks | macOS stays ad-hoc signed, as today, until it's done. We don't sign with a personal name in the meantime | Free |
| Apple requires a new organisation enrolment instead of a conversion | Enrol ANI.ML Health Inc. as a new organisation and let the individual membership lapse | US$99 a year more until the old one lapses |

### 9. What users will see

| Platform | After this ADR |
|---|---|
| Windows | The first downloads of a new release may still show "Windows protected your PC", now with **Publisher: ANI.ML Health Inc.** The prompt fades as reputation builds over weeks. Smart App Control should no longer block the app, because it accepts validly signed files (the GPU pack is the exception, see Consequences) |
| macOS | The standard "downloaded from the Internet. Are you sure?" question with an **Open** button. No trip to Privacy & Security |
| Linux | No change on install. Anyone can check `SHA256SUMS.txt.asc` against the published key, and `rpm -K` passes |
| iPhone | Installs from TestFlight, then the App Store |

The install guide and the website say this plainly, including that a SmartScreen prompt on the first downloads is expected.

### Cost summary

| Item | Cost | New spend? |
|---|---|---|
| Microsoft Artifact Signing, Basic | US$9.99 a month (CA$14.15), about US$120 (CA$170) a year before tax | Yes |
| Apple Developer Program | US$99 a year (CAD price shown at renewal) | No, already paid |
| D-U-N-S number | Free | No |
| OpenPGP key | Free | No |
| Extra macOS runner minutes | Under US$1 per release | Marginal |
| **Total new spend** | **About US$120 a year** | |
| Windows fallback (SSL.com OV + eSigner) | About US$309 a year, instead of Artifact Signing | Only if needed |

## Consequences

**Easier**
- Windows shows ANI.ML Health Inc. instead of Unknown publisher, and reputation can build. Unsigned releases would start from zero every time.
- macOS opens like any other app. The Open Anyway step leaves the install guide and docs/runbooks/macos.md.
- Anyone can check that a download came from us, on every platform, from the public key and fingerprint on www.openbark.io.
- The iPhone work reuses the same Apple team, the same OpenSSL routine and the same key handling.

**Harder**
- The Windows release job splits into build, Azure login and bundle steps, and it needs `signtool`, the .NET 8 runtime and a pinned `Microsoft.ArtifactSigning.Client` package (1.0.128, 2026-03-30).
- The macOS job gains a temporary keychain, two extra notarization submissions (uninstaller and disk image) and their waits.
- **Renewals to diarise:**
  - Microsoft identity validation: start 60 days before expiry; it takes 1 to 20 business days. If it lapses, signing stops.
  - The Apple membership: yearly.
  - The Developer ID certificate: believed every five years.
  - The Apple Distribution certificate: believed yearly.
  - The OpenPGP key: extend within 3 years.
- Build-time network hosts are added to docs/network.md:
  - Windows: `login.microsoftonline.com`, `eus.codesigning.azure.net`, `timestamp.acs.microsoft.com` and `www.nuget.org`;
  - macOS: Apple's timestamp and notary services (`timestamp.apple.com` and `appstoreconnect.apple.com`, believed).

  The app gains no run-time hosts. Windows and macOS may check certificates or notarization online on their own. That is the operating system, not OpenBark, and network.md says so.
- **The publisher name changes** from "ANI-ML" to "ANI.ML Health Inc." in `tauri.conf.json`, to match the certificate. The NSIS template keys a registry entry by publisher (`Software\<publisher>\OpenBark`), so installing over a beta build needs a test. The install folder is fixed, so data isn't at risk, and the old key is left behind, harmless.

**To watch**
- **Microsoft eligibility.** If ANI.ML Health Inc. is under 3 years old and Microsoft still applies that rule, validation fails and we take the SSL.com fallback. That costs a month of Artifact Signing (no pro-rating) plus the CA's validation time.
- **The GPU pack and Smart App Control.** On a PC with Smart App Control on, the pack's unsigned DLLs may be blocked, and the pack replaces our signed `onnxruntime.dll` with an unsigned one. To test on such a PC. If it bites, OpenBark says why the GPU check failed, and we consider re-hosting signed copies of the Apache-2.0 and MIT DLLs (not NVIDIA's).
- **The microphone entitlement.** Add it even before signing, because the ad-hoc build may already be unable to record. Confirm on a real Mac as soon as one is available.
- **Apple agreements.** When Apple updates its program agreements, notarization can fail until the Account Holder accepts them (believed).
- **Name spelling.** Microsoft, Apple and D&B must all show exactly the registry's legal name. A missing full stop in one record shows up in the signature.
- **Updater.** OpenBark has no updater, so there is no updater key. Adding one needs its own ADR, a docs/network.md row and an off switch. Its files would still need these signatures.

## Alternatives considered

| Option | Why not |
|---|---|
| EV certificate (SSL.com eSigner EV from US$100 a month, or DigiCert) | Microsoft says EV no longer bypasses SmartScreen. It costs more for nothing we need |
| OV certificate from SSL.com with eSigner | Kept as the Windows fallback. About 2.5 times the price, re-validation every 15 months, and a per-signing quota |
| DigiCert or Sectigo OV | DigiCert lists US$44 to US$62 a month per certificate, and its cloud-signing price isn't published. We found only reseller prices for Sectigo. Both cost more than SSL.com |
| SignPath Foundation (free for open source) | The publisher would read "SignPath Foundation", not ANI.ML Health Inc., and it needs a public repository the team owns. Ours is private |
| Microsoft individual validation, or an individual Apple membership | Puts a person's legal name (and on Windows their town) in every signature, and on the App Store as seller. Breaks the rule against naming individuals |
| Signing the finished installer with `azure/artifact-signing-action` after the build | Leaves the app's .exe, the DLLs and `uninstall.exe` inside the installer unsigned. Kept for files outside the installer, such as the planned CLI and `ggml-cuda.dll` |
| `artifact-signing-cli` with a client secret as the main route (Tauri's documented way) | Needs a long-lived Azure secret that expires and must be rotated, and it downloads Microsoft's client from NuGet as it runs. Kept as the fallback for a GitHub plan without environments |
| Microsoft Store (MSIX) | No SmartScreen prompt at all, but Tauri doesn't build MSIX, and the Store adds review and a second distribution channel. Later, perhaps |
| Apple ID and app-specific password for notarization | Tied to one person's Apple Account and its two-factor login. An API team key belongs to the team and can be revoked on its own |
| Apple cloud-managed Developer ID certificates | Apple documents them only for Xcode's Organizer, which a Tauri build on a CI runner can't use |
| Embedded AppImage signature | AppImage doesn't check it. The signed checksum file covers the AppImage |
| Signed apt and rpm repositories, or Flathub | More to run, and a new host that clinics' machines contact on every update. Later, if clinics ask |
| GitHub artifact attestations | Need GitHub Enterprise Cloud for a private repository |
| Keyless cosign | Works from a private repo, but writes the workflow identity, including the private repo's name, to the public Rekor log. The OpenPGP signature does the job without that |
| Not signing | Every Windows release starts with zero reputation and Unknown publisher, Smart App Control can block it, and macOS needs Open Anyway. Clinics' IT may refuse to install it |

## Implementation notes (2026-10-18)

The release pipeline is built on branch `ci/signing`, before any certificate exists. The owner decided that 0.1.1 ships unsigned. Signing switches itself on per platform when the secrets appear. The runbook's "CI changes" section marks each item and gives the secret-to-platform table.

- **When signing runs.** Only for a pushed `v*` tag (decision 7), and per platform only when all of that platform's secrets are set. With none set, the platform builds unsigned exactly as before (macOS ad-hoc) and the log carries a notice. With only some set, the job fails and names the missing ones. `scripts/ci/signing-plan.sh` makes that decision from true/false flags. The secret values reach only the steps that sign.
- **Manual runs never publish.** `draft-release` and `public-draft` require `github.event_name == 'push'` as well as a `v*` ref, so a `workflow_dispatch` on a tag can't create a release either. The installers job takes the `release` environment only for a tag push. The first manual run confirmed that an empty environment name means no environment.
- **Windows** follows decision 2: `tauri build --no-bundle`, then `azure/login` (OIDC), then `tauri bundle` with a CI-only `signCommand` overlay (signtool plus the Artifact Signing dlib, client 1.0.128 checked by SHA-256). Tauri 2.12.3's bundler source confirms that the overlay signs the app's .exe, the four DLLs in `bundle.resources`, the NSIS plugins, `uninstall.exe` (`!uninstfinalize`) and the installer. The job then installs the setup silently and checks every installed .exe and .dll for a valid, timestamped signature from ANI.ML Health Inc.
- **macOS** follows decision 3, with two additions:
  - the build compiles first with no signing values in the environment, then bundles and signs;
  - in signed builds the uninstaller gets a compiled launcher and an Apple Events entitlement, so under the hardened runtime it can still ask OpenBark to quit.

  The app keeps a single entitlement, the microphone (`com.apple.security.device.audio-input`). llama.cpp's Metal shaders compile through Metal's compiler service, so no JIT exception is needed.
- **Linux** (decision 4 changed): the `.rpm` is signed with `rpmsign` after the build instead of through Tauri's undocumented `TAURI_SIGNING_RPM_KEY`. `SHA256SUMS.txt.asc` and `openbark-release-key.asc` (exported in CI from the imported key) go on the public release.
- **Publisher metadata** now reads ANI.ML Health Inc. (Toronto, Ontario, Canada) in `tauri.conf.json` (publisher, copyright), the setup file's CompanyName, the `.deb` Maintainer, NOTICE and the website footer.
- **Tested without accounts.** The `Signing self-test` workflow runs the same scripts with throwaway credentials on macOS (self-signed certificate, no notarization), Windows (self-signed certificate through the same `signCommand` path) and Linux (a fresh OpenPGP key). The workflow files pass actionlint 1.7.12 with shellcheck 0.11.0.
- **Dry runs (2026-10-19), both green.** A manual release run on `ci/signing` (actions run 39246618797) built all five installers unsigned, macOS ad-hoc, and skipped both publishing jobs. The self-test (run 39246612725) signed and verified on all three platforms. It confirmed the publisher on Windows (CompanyName and LegalCopyright on the setup file and the app, Publisher in Apps & features) and the copyright and microphone entitlement on macOS. On macOS the app holds one Mach-O file, because sherpa-onnx and ONNX Runtime are linked statically. Two fixes came out of the first attempt:
  - Tauri read an empty `APPLE_SIGNING_IDENTITY` as the identity `""`, so `macos-dmg.sh` now unsets the signing variables when it isn't signing.
  - Removing a trusted certificate from a runner's System keychain waits for an authorization prompt, so the self-test leaves it on the single-use runner. Jobs and cleanup steps now have time limits.
- **Not verifiable until the accounts exist:**
  - Microsoft's service itself: OIDC sign-in, the role assignment, the dlib's token use and the certificate subject;
  - Apple notarization and stapling, and whether the API key's Developer role is enough;
  - Gatekeeper's verdict on a real Mac, including how macOS asks the uninstaller for Apple Events and App Management;
  - SmartScreen reputation;
  - installing over a beta now that the publisher has changed.
