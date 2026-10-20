# Runbook: the iPhone app's signing, app record and TestFlight

This runbook is for the Account Holder of ANI.ML Health's Apple Developer account. It sets up everything GitHub's macOS runners need to build OpenBark for iPhone, sign it and upload it to TestFlight, with no Mac. Everything here is done on Windows, in a browser and in PowerShell or Git Bash.

The design is in [ADR-0018](../adr/0018-iphone-app.md) (section 8) and [docs/plans/M3.md](../plans/M3.md). The workflow that uses these secrets is `.github/workflows/ios-testflight.yml`. Without them it skips cleanly with a notice, so nothing breaks while this is being done.

## Checklist

- [ ] **1. Membership type.** Check whether the Apple Developer membership is Individual or Organization, and note the Team ID.
- [ ] **2. App ID.** Register the bundle id `health.animl.openbark`.
- [ ] **3. App record.** Create the app in App Store Connect.
- [ ] **4. Distribution certificate.** Make the certificate request and the `.p12` with OpenSSL on Windows.
- [ ] **5. API key.** Create an App Store Connect API key for the build.
- [ ] **6. GitHub secrets.** Add the six secrets to the `testflight` environment.
- [ ] **7. First build.** Run the TestFlight job and answer the export compliance question.
- [ ] **8. Install.** Add yourself as an internal tester and install from TestFlight.

Rules that apply throughout:
- Use the company's exact legal name, ANI.ML Health Inc., and an address on the company's own mail domain. Never a personal name or address.
- Every file, key and password goes into the company password manager. Delete the loose copies once step 6 is done.
- Paste values into GitHub only, never into chat, email or a ticket.

## 1. Membership type and Team ID (5 minutes)

1. Sign in at [developer.apple.com/account](https://developer.apple.com/account) and open **Membership details**.
2. Note:
   - **Entity type**: Individual or Organization.
   - **Team ID**: 10 characters, for example `AB12CD34EF` (that one is made up).
3. If it says **Individual**, the App Store would show a person's name as the seller. TestFlight builds work either way, so you can carry on now. Convert to Organization before the first public release: [code-signing.md, Part A1](code-signing.md#a1-if-the-membership-is-individual-convert-it-to-organization) has the steps (D-U-N-S number, then Apple's conversion).
4. Tell engineering which it is (ADR-0018 section 9 records it).

## 2. Register the App ID (5 minutes)

1. Go to [Certificates, Identifiers & Profiles](https://developer.apple.com/account/resources/identifiers/list), **Identifiers**, then **+**.
2. Choose **App IDs**, then **App**, then **Continue**.
3. Description: `OpenBark`. Bundle ID: **Explicit**, `health.animl.openbark`. It can't be changed once the first build is uploaded.
4. Capabilities: leave the defaults for now. When engineering turns on drafting on the phone (ADR-0018 section 10), come back and tick **Increased Memory Limit**.
5. **Continue**, then **Register**.

## 3. Create the app record (10 minutes)

1. Go to [App Store Connect](https://appstoreconnect.apple.com/apps), **Apps**, then **+** and **New App**.
2. Fill in:
   - Platforms: **iOS**.
   - Name: `OpenBark`. If it's taken on the App Store, use `OpenBark Scribe` and tell engineering. This name is what the store shows, and can be changed until the first public release.
   - Primary language: English (Canada) or English (U.S.).
   - Bundle ID: `health.animl.openbark` (from step 2).
   - SKU: `openbark-ios`.
   - User access: **Full Access**.
3. **Create**. Nothing else on the app page is needed for TestFlight.

## 4. The distribution certificate, made on Windows (20 minutes)

Apple's own steps assume a Mac's Keychain Access. OpenSSL does the same on Windows. Git for Windows includes it: in **Git Bash**, `openssl version` should print OpenSSL 3.x. Work in a new folder that only you can read, for example `C:\Users\<you>\openbark-signing`.

1. **Make a private key and a certificate request.** In Git Bash, in that folder (on one line each; replace the address with a company one):
   ```bash
   openssl genrsa -out openbark-distribution.key 2048
   openssl req -new -key openbark-distribution.key -out openbark-distribution.csr -subj "/emailAddress=apple@animl.health/CN=ANI.ML Health Inc./C=CA"
   ```
   If Git Bash rewrites the `/emailAddress...` argument into a Windows path, start it with two slashes (`-subj "//emailAddress=...\CN=..."`), or run the same commands in PowerShell with `& "C:\Program Files\Git\usr\bin\openssl.exe"`.
2. **Ask Apple for the certificate.** Go to [Certificates](https://developer.apple.com/account/resources/certificates/list), **+**, choose **Apple Distribution**, **Continue**, upload `openbark-distribution.csr`, then **Download**. You get `distribution.cer`.
3. **Get Apple's intermediate certificate** (Worldwide Developer Relations, G3): download [AppleWWDRCAG3.cer](https://www.apple.com/certificateauthority/AppleWWDRCAG3.cer) into the same folder.
4. **Convert both to PEM:**
   ```bash
   openssl x509 -in distribution.cer -inform DER -out distribution.pem
   openssl x509 -in AppleWWDRCAG3.cer -inform DER -out AppleWWDRCAG3.pem
   ```
5. **Bundle the key and the certificates into a `.p12`.** Choose a long password and keep it in the password manager. The `-keypbe`, `-certpbe` and `-macalg` options use the older encryption that macOS's `security import` reads reliably:
   ```bash
   openssl pkcs12 -export -inkey openbark-distribution.key -in distribution.pem -certfile AppleWWDRCAG3.pem \
     -name "Apple Distribution: ANI.ML Health Inc." -keypbe PBE-SHA1-3DES -certpbe PBE-SHA1-3DES -macalg sha1 \
     -out openbark-distribution.p12
   ```
6. **Check it:** `openssl pkcs12 -in openbark-distribution.p12 -noout -info` asks for the password and should list one certificate chain and one key, with no error.
7. **Base64 for GitHub.** In PowerShell, in that folder:
   ```powershell
   [Convert]::ToBase64String([IO.File]::ReadAllBytes("$PWD\openbark-distribution.p12")) | Set-Clipboard
   ```
   The clipboard now holds the value for `IOS_CERTIFICATE` (step 6). Paste it straight into GitHub.

The certificate lasts one year. Apple emails before it expires; repeat this step then and replace the two certificate secrets.

## 5. App Store Connect API key (5 minutes)

The build uses it to fetch the App Store provisioning profile, and the upload uses it to send the build to TestFlight.

1. In App Store Connect, open **Users and Access**, then **Integrations**, **App Store Connect API**, **Team Keys**, then **+** (Generate API Key).
2. Name: `OpenBark TestFlight (GitHub)`. Access: **Admin**. Xcode needs it to create and download the provisioning profile by itself; a key with less access can upload but not sign.
3. **Generate**, then download the `.p8` file. Apple lets you download it **once**.
4. Note the **Key ID** (10 characters, beside the key) and the **Issuer ID** (at the top of the page, a UUID).
5. Base64 the `.p8` in PowerShell for `APPLE_API_KEY_P8`:
   ```powershell
   [Convert]::ToBase64String([IO.File]::ReadAllBytes("$PWD\AuthKey_<KEYID>.p8")) | Set-Clipboard
   ```

If the key ever leaks, revoke it on the same page and make a new one. Keys don't expire.

## 6. GitHub secrets (10 minutes)

1. Go to the development repository's **Settings**, **Environments**, **New environment**, and name it `testflight`.
2. Under **Deployment branches and tags**, choose **Selected branches and tags** and add the rules `main` and `v*` (tags). Builds from other branches can't use the signing secrets then.
3. Add these **environment secrets**:

| Secret | Value |
|---|---|
| `IOS_CERTIFICATE` | The base64 of `openbark-distribution.p12` (step 4.7) |
| `IOS_CERTIFICATE_PASSWORD` | The `.p12`'s password |
| `APPLE_DEVELOPMENT_TEAM` | The Team ID (step 1) |
| `APPLE_API_ISSUER` | The Issuer ID (step 5) |
| `APPLE_API_KEY` | The Key ID (step 5) |
| `APPLE_API_KEY_P8` | The base64 of the `.p8` file (step 5.5) |

`IOS_MOBILE_PROVISION` is optional: without it, Xcode fetches the profile with the API key.

The names follow Tauri's iOS signing guide (`IOS_CERTIFICATE`, `IOS_CERTIFICATE_PASSWORD`, `IOS_MOBILE_PROVISION`, `APPLE_API_ISSUER`, `APPLE_API_KEY`, with `APPLE_API_KEY_PATH` set by the workflow), checked on 2026-10-19: https://v2.tauri.app/distribute/sign/ios/.

4. Delete the loose files from step 4 and 5 once they are in the password manager.

## 7. The first TestFlight build (40 minutes, mostly waiting)

1. Open **Actions**, **iOS TestFlight**, **Run workflow**, on `main`. (A version tag runs it too.)
2. It takes 30 to 40 minutes. macOS minutes count ten times on a private repository, so it runs only by hand or on tags.
3. If it says "TestFlight skipped: signing secrets not set", a secret's name is wrong or missing. The notice lists which.
4. When it finishes, the build appears in App Store Connect under **TestFlight** after Apple's processing (5 to 30 minutes).
5. **Export compliance.** The first build shows "Missing Compliance". OpenBark encrypts visits on the phone with standard algorithms (XChaCha20-Poly1305, and TLS for downloads), and uses no proprietary encryption. Answer the questions that way, and send engineering a copy of the answers: ADR-0018 section 9 records them and sets `ITSAppUsesNonExemptEncryption` to match, so later builds don't ask again. Whether an annual self-classification report is due is checked before any external release.

## 8. Install from TestFlight (10 minutes)

1. In App Store Connect, **TestFlight**, **Internal Testing**, **+**: make a group called `OpenBark team` and add yourself. Internal testers must be users of the App Store Connect team (up to 100), and their builds need no review.
2. On the iPhone, install **TestFlight** from the App Store, and accept the invitation from the email (or open TestFlight; the build is listed).
3. Install **OpenBark**. TestFlight builds expire after 90 days.
4. Check that it opens, shows the Record screen, and that **Settings**, **Storage** says recordings stay in OpenBark's storage on the phone. Send engineering a screenshot of anything odd. The scripted audio checklist (S1, M3 phase 1) is added here once that build exists.

## If something goes wrong

| What you see | What to do |
|---|---|
| The build fails with "No profiles for 'health.animl.openbark' were found" | The API key needs **Admin** access (step 5), and the App ID must exist (step 2) |
| "MAC verification failed" or "Unable to import" when the certificate is imported | Make the `.p12` again with the options in step 4.5, and check the password secret |
| "No signing certificate 'iOS Distribution' found" | The `.p12` must contain the **Apple Distribution** certificate made from your own request, with its private key (step 4) |
| The upload says the bundle id doesn't match | The app record (step 3) must use `health.animl.openbark` |
| A tester can't see the build | They must be in the internal group, and the export compliance answer must be given (step 7.5) |

## For engineering

- The workflow imports the certificate through the Tauri CLI (`IOS_CERTIFICATE`), writes the `.p8` to `~/.appstoreconnect/private_keys/AuthKey_<id>.p8` (one of the folders `altool` searches) and sets `APPLE_API_KEY_PATH`, builds with `tauri ios build --export-method app-store-connect --build-number <run number>`, uploads with `xcrun altool --upload-app --type ios --apiKey --apiIssuer`, and removes the key at the end.
- The simulator check (`.github/workflows/ios.yml`) needs no secrets.
- `apps/desktop/src-tauri/gen/apple` is committed (made by the iOS init job). Regenerate it with the init job when Tauri or the app's iOS settings change, and commit the artifact.
