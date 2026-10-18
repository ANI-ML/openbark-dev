# Runbook: set up code signing for OpenBark

This runbook is for the product owner. It sets up signing so that:
- Windows shows **ANI.ML Health Inc.** instead of "Unknown publisher";
- macOS opens OpenBark without the Open Anyway step;
- anyone can check that a download really came from us.

The decision and the reasons behind it are in [ADR-0020](../adr/0020-code-signing.md). The last section, "CI changes", is for engineering.

## At a glance

| | Windows | macOS | Linux and checksums | iPhone (later) |
|---|---|---|---|---|
| Service | Microsoft Artifact Signing, Basic | Apple Developer ID and notarization | An OpenPGP key | Apple Distribution certificate |
| Cost | US$9.99 a month (CA$14.15) | Already paid (US$99 a year) | Free | Already paid |
| Your hands-on time | About 2 hours | About 1 hour (more if the membership must be converted) | About 20 minutes | About 1 hour |
| Waiting time | 1 to 20 business days (Microsoft's review) | Minutes, or 1 to 3 weeks if a conversion is needed (to confirm) | None | None |

**New spending: about US$120 a year** (CA$170 before tax). If Microsoft refuses the company, the fallback costs about US$309 a year (Part H).

**Order of work.** Start the two waits first: Microsoft's review (Part B) and, if needed, Apple's conversion (Part A). Do Parts C and D while you wait.

**Rules that apply throughout:**
- Always use the company's exact legal name, as the corporate registry shows it. Never use a personal name or a nickname.
- Use addresses on the company's own email domain, never a personal address.
- Every file, key and password you create goes into the company password manager. Delete the loose copies once Part D is done.
- Paste values into GitHub only, never into chat, email or a ticket.

## Part 0: four facts to find first (30 minutes)

Write these down before you start. Every form below asks for them.

| Fact | Where to find it | Why it matters |
|---|---|---|
| 1. Exact legal name, corporation number, registered address and **date of incorporation** of ANI.ML Health Inc. | Corporations Canada or the provincial registry where it's incorporated. Have the CRA Business Number at hand too | Microsoft and Apple check these against public records. Microsoft may refuse companies under 3 years old (to confirm) |
| 2. Apple membership type | [developer.apple.com/account](https://developer.apple.com/account), then Membership details. It shows Individual or Organization, and the 10-character Team ID | An individual membership would put your personal name in every Mac signature |
| 3. GitHub organisation plan (Free, Team or Enterprise) | github.com/organizations/ANI-ML/settings/billing | Team or higher lets us lock the signing secrets to release tags (Part D) |
| 4. The company website and mail domain | Check that the website is live and names ANI.ML Health Inc., and that the domain is registered to the company | Microsoft and Apple both check the website. Microsoft may ask for the domain's registration invoice |

Make sure the registry shows the company's current address. If it is out of date, fix it first. Both reviews compare against it.

## Part A: Apple (macOS now, iPhone later)

### A1. If the membership is Individual, convert it to Organization

Skip to A2 if it already says Organization.

1. Look up the company's D-U-N-S number with Apple's free tool: [developer.apple.com/enroll/duns-lookup](https://developer.apple.com/enroll/duns-lookup/). If there isn't one, request it there. It's free. Allow up to 5 business days from Dun & Bradstreet, then up to 2 business days before Apple can see it.
2. Check that D&B's record spells the legal name exactly as the registry does, punctuation included. Apple uses D&B's spelling in the certificate.
3. Contact Apple Developer Support ([developer.apple.com/contact](https://developer.apple.com/contact/), Membership and Account) and ask to convert the membership to an organisation for ANI.ML Health Inc., quoting the D-U-N-S number. Ask two questions:
   - Will the Team ID stay the same?
   - How long will it take?
4. Apple may ask for business documents, sometimes notarised, and will check that you can sign legal agreements for the company.
5. **Until the conversion is done:** don't create a Developer ID certificate, and don't register `health.animl.openbark` or create the iPhone app record in App Store Connect.

If Apple says it can't convert and you must enrol again, enrol ANI.ML Health Inc. as a new organisation (US$99 a year) and let the individual membership lapse. Tell engineering, because the Team ID will change.

### A2. Accept any pending agreements (5 minutes)

Sign in to [developer.apple.com/account](https://developer.apple.com/account) and [App Store Connect](https://appstoreconnect.apple.com) as the Account Holder. Accept any agreement shown in a banner or under Business. Notarization can fail while an agreement is waiting (believed).

### A3. Create the notarization key (10 minutes)

1. In App Store Connect, open Users and Access, then Integrations, then App Store Connect API.
2. If you see **Request Access**, click it, tick the terms and submit. Only the Account Holder can do this, and Apple reviews it case by case. You'll get an email.
3. Under **Team Keys**, click Generate API Key (or the + button).
   - Name: `OpenBark notarize`
   - Access: **Developer**
4. Click **Download API Key**. You get a file named `AuthKey_XXXXXXXXXX.p8`. **It can be downloaded only once.** Put it in the password manager straight away.
5. Note two values from the same page:
   - **Key ID:** the 10 characters in the key's row.
   - **Issuer ID:** the long value shown above the table.

If notarization later fails with a 401 error even with the right Issuer ID, create a second key with **App Manager** access and replace this one (to confirm: Apple doesn't publish the minimum role).

### A4. Make the certificate request on Windows (10 minutes)

You don't need a Mac. Open **Git Bash** (it comes with Git for Windows and includes OpenSSL). Paste these lines one block at a time. Replace `you@company-domain` with your work address.

```bash
mkdir -p ~/openbark-signing && cd ~/openbark-signing
MSYS_NO_PATHCONV=1 openssl req -new -newkey rsa:2048 -nodes \
  -keyout developer-id.key -out developer-id.certSigningRequest \
  -subj "/emailAddress=you@company-domain/CN=ANI.ML Health Inc./C=CA"
```

`MSYS_NO_PATHCONV=1` stops Git Bash from mangling the `/CN=` part. You now have two files:
- `developer-id.key` is the private key. It's secret, so put it in the password manager.
- `developer-id.certSigningRequest` is the request you upload to Apple.

### A5. Create the Developer ID Application certificate (5 minutes)

Only the Account Holder can do this.

1. Go to [Certificates, Identifiers & Profiles](https://developer.apple.com/account/resources/certificates/list) and click **+**.
2. Under Software, choose **Developer ID**, then **Developer ID Application**, then Continue.
3. If asked to choose a profile type, choose **G2 Sub-CA**. The older one is being retired (believed).
4. Upload `developer-id.certSigningRequest`, click Continue, then **Download**. You get `developerID_application.cer`. Move it into `~/openbark-signing`.
5. Note the expiry date shown in the portal and put a reminder 3 months before it.

### A6. Turn it into the file GitHub needs (10 minutes)

In Git Bash, in the same folder:

```bash
cd ~/openbark-signing
curl -fsSLO https://www.apple.com/certificateauthority/DeveloperIDG2CA.cer
openssl x509 -inform DER -in developerID_application.cer -out developer-id.pem
openssl x509 -inform DER -in DeveloperIDG2CA.cer -out developer-id-g2-ca.pem
openssl x509 -in developer-id.pem -noout -subject
```

The last line prints something like:

`subject=UID=ABCDE12345, CN=Developer ID Application: ANI.ML Health Inc. (ABCDE12345), OU=ABCDE12345, O=ANI.ML Health Inc., C=CA`

- The text after `CN=` and before `, OU=` is your **signing identity**, for example `Developer ID Application: ANI.ML Health Inc. (ABCDE12345)`. Copy it exactly.
- The 10 characters in brackets are your **Team ID**.
- **Stop here if the name shows a person instead of the company.** The membership is still individual, so go back to A1.

Now make the `.p12` file. It asks for a password twice. Generate a long random one in the password manager.

```bash
openssl pkcs12 -export -legacy -inkey developer-id.key -in developer-id.pem \
  -certfile developer-id-g2-ca.pem -name "OpenBark Developer ID" -out developer-id.p12
openssl base64 -A -in developer-id.p12 -out developer-id.p12.base64.txt
```

`-legacy` makes a file that macOS's keychain tool accepts (believed necessary with OpenSSL 3). `developer-id.p12.base64.txt` is one long line of text. That line is what goes into GitHub.

Save to the password manager:
- `developer-id.key`;
- `developer-id.p12` and its password;
- `developerID_application.cer`;
- the signing identity text and the Team ID.

## Part B: Microsoft (Windows)

### B1. Create the Azure subscription (20 minutes)

1. Go to [portal.azure.com](https://portal.azure.com) and sign in with your **company** work account. If the company has no Microsoft 365 or Azure account yet, signing up creates one.
2. Create a **Pay-As-You-Go** subscription. Use the company name and a company card for billing. Free trials are refused by the signing service.
3. Name it `ANI.ML Health - OpenBark signing`.
4. In the portal, open Subscriptions, then that subscription, then Resource providers. Find **Microsoft.CodeSigning**, then click Register. Wait until it says Registered.

### B2. Create the signing account (10 minutes)

**Billing starts now,** for a full month, with no pro-rating. Do this step only when you're ready to do B3 and B4 the same day.

1. In the portal search box, type **Artifact Signing Accounts**, open it, then click Create.
2. Fill in:

   | Field | Enter |
   |---|---|
   | Subscription | The one from B1 |
   | Resource group | Create new: `openbark-signing` |
   | Account name | `openbarksigning` (3 to 24 letters and numbers, globally unique. If it's taken, add digits) |
   | Region | **East US** (there's no Canadian region; only file fingerprints are ever sent) |
   | Pricing | **Basic** |

3. Click Review + Create, then Create, then Go to resource.
4. Write down the account name and the endpoint, `https://eus.codesigning.azure.net`.

### B3. Give yourself the verifier role (5 minutes)

Even as subscription owner, you need this role to see the validation button.

1. In the signing account, open **Access control (IAM)**, then Add, then Add role assignment.
2. Choose the role **Artifact Signing Identity Verifier**. Older screens may say "Trusted Signing".
3. Under Members, select yourself, then Review + assign.

### B4. Submit the company's identity validation (20 minutes)

1. In the signing account, open **Identity validations**.
2. Choose **Organization**, then **New Identity**, then **Public**.
3. Fill in the form:

   | Field | Enter |
   |---|---|
   | Organization Name | The exact legal name from the registry. Check punctuation and "Inc." |
   | Website url | The company website (Part 0, fact 4) |
   | Primary Email | Your work address on the company domain. It must be a real mailbox you watch, not a group, and it must accept emails with links from outside senders |
   | Secondary Email | A different address **on the same domain**. A team alias is fine |
   | Business Identifier | The corporation number from the registry. Have the CRA Business Number and the D-U-N-S number ready in case they ask (which one Microsoft prefers is to confirm) |
   | Seller ID | Leave blank |
   | Street, City, Country/Region, State/Province, Postal code | The registered address, exactly as the registry shows it |
   | First Name, Last Name | Your names exactly as on your passport or driver's licence. They're used only for your ID check and don't appear on the certificate |

4. Click **Certificate subject preview**. Expect the company name, city, province and Canada, and no personal name.
5. Click Create. The status changes to In Progress.

### B5. Do your personal ID check (15 minutes, on your phone)

1. Watch the primary mailbox. **The links expire after 7 days.** If one expires, you must start a new request. Check the spam folder too.
2. When the request says **Action Required**, click your name in the portal and follow "Please complete your verification here". Use the same email address you entered.
3. The check runs through AU10TIX: you photograph your government ID and your face on your phone. Then you add a **Verified ID** to the **Microsoft Authenticator** app and share it back to the browser.
   - Use good light, no flash and no VPN.
   - Use the latest Authenticator app.
4. The status goes back to In Progress.

### B6. Wait, and answer any document request (1 to 20 business days)

- Microsoft emails if they need documents, for example articles of incorporation, a registry extract or a domain registration invoice. Upload them in the portal.
- **You get three attempts in total.** Documents must be issued within the last 12 months and stay valid for at least 2 more months.
- The review can't be sped up. Opening a second request doesn't help.
- At the end the status says **Completed** or **Failed**. If it fails, tell engineering and go to Part H. Delete the signing account so billing stops.
- When it says Completed, note the **expiry date** on the Identity validations page. Set a reminder 60 days before it. Renewal repeats the full review.

### B7. Create the certificate profile (5 minutes)

1. In the signing account, open **Certificate profiles**, then Create, then **Public Trust**.
2. Name: `openbarkpublic`.
3. Under Verified CN and O, choose the validation from B4.
4. **Leave "Include street address" and "Include postal code" unticked.**
5. Check the subject preview again, then click Create.

### B8. Let GitHub sign without a password (20 minutes)

This sets up a trust between GitHub and Azure, so no Azure password is ever stored in GitHub.

1. In the portal, open **Microsoft Entra ID**, then App registrations, then New registration.
   - Name: `openbark-release-signing`
   - Who can use it: accounts in this directory only
   - Click Register.
2. On its Overview page, copy:
   - **Application (client) ID**, which becomes `AZURE_CLIENT_ID`;
   - **Directory (tenant) ID**, which becomes `AZURE_TENANT_ID`.
3. Open Certificates & secrets, then **Federated credentials**, then Add credential.

   | Field | Enter |
   |---|---|
   | Scenario | GitHub Actions deploying Azure resources |
   | Organization | `ANI-ML` |
   | Repository | `openbark` |
   | Entity type | **Environment** |
   | GitHub environment name | `release` |
   | Name | `github-release` |

   The subject shown should read `repo:ANI-ML/openbark:environment:release`. Click Add.

   If your GitHub plan has no environments (Part 0, fact 3), skip this step and tell engineering. They'll use the fallback in Part H.
4. Go back to the signing account, open **Access control (IAM)**, then Add role assignment.
   - Role: **Artifact Signing Certificate Profile Signer**
   - Members: User, group or service principal, then `openbark-release-signing`
   - Click Review + assign.
5. Open Subscriptions and copy the **Subscription ID**, which becomes `AZURE_SUBSCRIPTION_ID`.

## Part C: the OpenPGP key for checksums and Linux (20 minutes)

This key signs the checksum file for every download and the Linux `.rpm` packages. In Git Bash, replace `releases@company-domain` with a team address on the company domain, not a personal one:

```bash
cd ~/openbark-signing
gpg --quick-generate-key "OpenBark Releases (ANI.ML Health Inc.) <releases@company-domain>" rsa4096 sign 3y
gpg -K --fingerprint
```

- It asks for a passphrase. Generate a long one in the password manager.
- The 40-character string under `sec` is the **fingerprint**. Copy it without spaces.

If Git Bash can't show the passphrase prompt, install Gpg4win and run the same commands in its command prompt.

Export the keys. Replace `FINGERPRINT` with yours:

```bash
gpg --armor --export FINGERPRINT > openbark-release-key.asc
gpg --armor --export-secret-keys FINGERPRINT > openbark-release-key.PRIVATE.asc
ls ~/.gnupg/openpgp-revocs.d/
```

| File | What to do with it |
|---|---|
| `openbark-release-key.asc` | Public. It goes on www.openbark.io and in the openbark-releases repo (engineering will do this) |
| `openbark-release-key.PRIVATE.asc` | Secret. Goes into GitHub (Part D) and the password manager |
| The `.rev` file in `openpgp-revocs.d` | Secret. The emergency "this key is no longer ours" certificate. Password manager only |

Set a reminder 33 months from today to extend the key's expiry.

## Part D: GitHub settings and secrets (20 minutes)

### D1. Create the `release` environment

1. Go to github.com/ANI-ML/openbark, then Settings, then **Environments**, then New environment. Name it `release`.
2. Under **Deployment branches and tags**, choose "Selected branches and tags". Add a rule with ref type **Tag** and pattern `v*`. Save.
3. Required reviewers aren't available on private repos below GitHub Enterprise, so the tag rule plus D2 are the protection.

If Settings has no Environments page, or it can't hold secrets, the plan is Free. Put everything below under Settings, then Secrets and variables, then Actions as **repository** secrets and variables instead, and tell engineering.

### D2. Protect release tags

1. Go to Settings, then Rules, then Rulesets, then New ruleset, then **New tag ruleset**.
2. Name: `Release tags`. Enforcement: Active.
3. Target tags: Include by pattern `v*`.
4. Rules: tick Restrict creations, Restrict updates and Restrict deletions.
5. Bypass list: add Repository admin.

Only admins can now push a `v*` tag, and a `v*` tag is what unlocks signing.

### D3. Add the secrets and variables

Add these in Settings, then Environments, then `release`. Use **Add secret** for secrets and **Add variable** for variables.

| Name | Type | Value | From |
|---|---|---|---|
| `AZURE_CLIENT_ID` | Secret | Application (client) ID | B8 |
| `AZURE_TENANT_ID` | Secret | Directory (tenant) ID | B8 |
| `AZURE_SUBSCRIPTION_ID` | Secret | Subscription ID | B8 |
| `AZURE_SIGNING_ENDPOINT` | Variable | `https://eus.codesigning.azure.net` | B2 |
| `AZURE_SIGNING_ACCOUNT` | Variable | The signing account name, for example `openbarksigning` | B2 |
| `AZURE_SIGNING_PROFILE` | Variable | `openbarkpublic` | B7 |
| `APPLE_CERTIFICATE` | Secret | The whole single line in `developer-id.p12.base64.txt` | A6 |
| `APPLE_CERTIFICATE_PASSWORD` | Secret | The `.p12` password | A6 |
| `APPLE_SIGNING_IDENTITY` | Secret | For example `Developer ID Application: ANI.ML Health Inc. (ABCDE12345)` | A6 |
| `APPLE_TEAM_ID` | Variable | The 10-character Team ID | A6 |
| `APPLE_API_KEY` | Secret | The Key ID | A3 |
| `APPLE_API_ISSUER` | Secret | The Issuer ID | A3 |
| `APPLE_API_PRIVATE_KEY` | Secret | The whole text of `AuthKey_XXXXXXXXXX.p8`, including the BEGIN and END lines (open it in Notepad) | A3 |
| `OPENBARK_RELEASE_GPG_KEY` | Secret | The whole text of `openbark-release-key.PRIVATE.asc` | C |
| `OPENBARK_RELEASE_GPG_PASSPHRASE` | Secret | The key's passphrase | C |
| `OPENBARK_RELEASE_GPG_FINGERPRINT` | Variable | The 40-character fingerprint | C |

`PUBLIC_RELEASES_TOKEN` stays where it is.

If you have the GitHub CLI, you can load files without copy and paste, for example:
`gh secret set APPLE_CERTIFICATE --repo ANI-ML/openbark --env release < developer-id.p12.base64.txt`

When all of this is in GitHub and in the password manager, delete the `~/openbark-signing` folder.

### D4. Tell engineering

Send engineering a short note: "Signing secrets are in the `release` environment" (or "in repository secrets, Free plan"). Then they make the changes in "CI changes" below and tag a test release.

## Part E: iPhone (later, when ADR-0018 reaches TestFlight)

Do this only after the membership is an organisation.

1. Register the App ID `health.animl.openbark` under Identifiers, and create the app record in App Store Connect (ADR-0018 section 8).
2. Make a second certificate request as in A4, with file names `distribution.key` and `distribution.certSigningRequest`.
3. Create an **Apple Distribution** certificate (Certificates, then +, then Apple Distribution) from that request. Make `distribution.p12` and its base64 text as in A6, without the `-certfile` part.
4. Create an **App Store Connect** provisioning profile for `health.animl.openbark` with that certificate, then download it. Make its base64 text: `openssl base64 -A -in profile.mobileprovision -out profile.base64.txt`.
5. Create a second team key, `OpenBark iOS`, with **Admin** access, as Tauri's iOS guide asks. Download its `.p8`.
6. Add the secrets to `release`:
   - `IOS_CERTIFICATE`
   - `IOS_CERTIFICATE_PASSWORD`
   - `IOS_MOBILE_PROVISION`
   - `IOS_API_KEY`
   - `IOS_API_ISSUER`
   - `IOS_API_PRIVATE_KEY`
7. Set a reminder for the Distribution certificate's expiry (believed yearly).

## Part F: check the first signed release

Engineering pushes a test tag. It creates **draft** releases in both repos; drafts aren't public. Download the files from the draft release pages. Delete the test drafts afterwards.

### Windows (any Windows PC, 10 minutes)

1. Right-click the setup file, then Properties, then **Digital Signatures**. Expect **ANI.ML Health Inc.** with a timestamp. Click Details: "This digital signature is OK."
2. Or in PowerShell:

   ```powershell
   Get-AuthenticodeSignature "$env:USERPROFILE\Downloads\OpenBark_*_x64-setup.exe" | Format-List Status, SignerCertificate, TimeStamperCertificate
   ```

   Expect `Status : Valid`, a signer subject starting `CN=ANI.ML Health Inc., O=ANI.ML Health Inc.`, and a time-stamper certificate that isn't empty.
3. Run the installer. For the first weeks a blue "Windows protected your PC" box may still appear. Click More info: it must say **Publisher: ANI.ML Health Inc.**, not Unknown publisher. That's expected and fades as people install.
4. After installing, check every program file:

   ```powershell
   Get-ChildItem "$env:LOCALAPPDATA\Programs\OpenBark" -Recurse -Include *.exe,*.dll | Get-AuthenticodeSignature | Format-Table Status, Path -AutoSize
   ```

   Every line must say `Valid`: the app, `uninstall.exe` and the four DLLs.

### macOS

The release workflow checks this on its Mac runner and fails if anything is wrong. If you or a tester can borrow a Mac, also do this hands-on check:

1. Download the disk image with Safari, open it, drag OpenBark to Applications and open it. Expect "OpenBark is an app downloaded from the Internet. Are you sure you want to open it?" with an **Open** button. No visit to Privacy & Security.
2. Start a recording. macOS must ask for the microphone with OpenBark's sentence, and the level meter must move.
3. Double-click **Uninstall OpenBark** on the disk image and click Cancel. It must open without a block.
4. In Terminal:

   ```bash
   spctl -a -vvv -t open --context context:primary-signature ~/Downloads/OpenBark_*.dmg
   xcrun stapler validate ~/Downloads/OpenBark_*.dmg
   spctl -a -vvv /Applications/OpenBark.app
   ```

   Expect `accepted`, `source=Notarized Developer ID` and `origin=Developer ID Application: ANI.ML Health Inc. (...)`, then "The validate action worked!"

### Linux and checksums (any computer with gpg, 5 minutes)

From the release page, download `SHA256SUMS.txt`, `SHA256SUMS.txt.asc`, `openbark-release-key.asc` and an installer. Then:

```bash
gpg --import openbark-release-key.asc
gpg --verify SHA256SUMS.txt.asc SHA256SUMS.txt
sha256sum --check --ignore-missing SHA256SUMS.txt
```

- `gpg --verify` must say `Good signature from "OpenBark Releases (ANI.ML Health Inc.) ..."`, and the fingerprint must match the one you noted.
- `sha256sum` must print `OK` for each file you downloaded.

The warning "This key is not certified with a trusted signature" is normal: it means you haven't personally vouched for the key.

On Fedora or openSUSE: `sudo rpm --import openbark-release-key.asc`, then `rpm -K OpenBark-*.rpm`. Expect `digests signatures OK`.

### iPhone (later)

The build appears in TestFlight and installs on your iPhone.

## Keeping it working

| What | How often | Start | Who | Time |
|---|---|---|---|---|
| Microsoft identity validation | Expiry shown on the Identity validations page (term to confirm) | 60 days before expiry | Owner (Identity Verifier) | 1 to 20 business days. **If it lapses, Windows signing stops** |
| Azure invoice | Monthly, US$9.99 | Check the card on file | Owner | None |
| Apple Developer Program | Yearly | Keep auto-renew on and the card current | Owner | None |
| Apple agreements | When Apple updates them | When a banner appears | Owner (Account Holder) | 5 minutes |
| Developer ID Application certificate | Believed every 5 years (date in the portal) | 3 months before | Owner (Account Holder), then repeat A4 to A6 and D3 | 1 hour |
| Apple Distribution certificate | Believed yearly | 1 month before | Owner or an Admin | 30 minutes |
| OpenPGP key | Expires after 3 years | 3 months before: extend it (`gpg --quick-set-expire FINGERPRINT 3y`), then re-export and update the secret and the website | Owner or engineering | 15 minutes |
| Registry and D&B records | When the address or name changes | At once | Owner | Varies |

**If a key or password leaks:**
- Apple certificate: revoke it in the portal and make a new one. Apps signed before the leak stop installing once it's revoked, so talk to engineering first.
- Apple API key: revoke it in App Store Connect.
- OpenPGP key: publish the `.rev` file and make a new key.
- Azure: remove the role from `openbark-release-signing`.

## If something goes wrong

| Problem | What to do |
|---|---|
| The New identity button is greyed out | You're missing the Artifact Signing Identity Verifier role (B3) |
| The Microsoft email link expired, or the email never came | Check spam and that the mailbox accepts outside links. Start a new identity request; the old link can't be re-sent |
| Microsoft validation failed | Delete the signing account to stop billing. Ask Azure Support whether business age was the reason. Tell engineering to switch to Part H |
| CI says 403 when signing | Check that the role in B8 step 4 is assigned, that the variables match the account and profile names, and that the endpoint is East US |
| CI says AADSTS700024 | The Azure login came too early in the job. An engineering fix (see CI changes) |
| CI says "certificate ... does not match provided identity" | `APPLE_SIGNING_IDENTITY` doesn't exactly match the certificate name. Copy it again from A6 |
| CI says "MAC verification failed" or the keychain import fails | Wrong `.p12` password, or the `.p12` was made without `-legacy` |
| Notarization says 401 | Wrong Issuer ID, an individual key instead of a team key, or a revoked key |
| Notarization says Invalid | Engineering reads the log the workflow prints. Also check for pending Apple agreements (A2) |
| The Mac app can't hear the microphone | The microphone entitlement is missing. An engineering fix |
| Windows still warns after signing | Expected for the first weeks. It must show ANI.ML Health Inc. as the publisher. If it says Unknown publisher, the file isn't signed: tell engineering |

## Part H: fallbacks

**Windows, if Microsoft refuses the company.** Buy an OV code signing certificate from SSL.com and their eSigner cloud signing:

| Item | Cost |
|---|---|
| OV certificate | US$129 a year |
| eSigner, tier 1 | US$180 a year for 240 signings |
| Extra signings | US$1.00 each |
| **Total** | **About US$309 a year** |

- SSL.com checks the company's registration and phone number. Their timing is to confirm.
- Since March 2026 these certificates last at most 460 days, so the check repeats about every 15 months.
- Engineering then swaps the signing command. The rest of this runbook stays the same, and the secret names come from SSL.com's instructions (to confirm).
- Don't buy EV. Microsoft says it no longer helps with SmartScreen.

**GitHub on the Free plan.** Skip the federated credential (B8 step 3).
1. In the app registration, open Certificates & secrets, then Client secrets, then New client secret, with a 12-month expiry.
2. Add it as the repository secret `AZURE_CLIENT_SECRET`.
3. Set a reminder 11 months out to replace it.

**Apple conversion is slow.** Nothing to do. Mac builds stay as they are today, ad-hoc signed with Open Anyway, until the organisation membership is ready. Don't sign with a personal name in the meantime.

## CI changes (for engineering)

Nothing below is done yet. Make these changes only after the secrets in D3 exist. Signing runs only when `startsWith(github.ref, 'refs/tags/v')`. Manual runs and local builds must keep working with no secrets, exactly as today.

### `.github/workflows/release.yml`

**Job `installers`**

1. **Permissions and environment.**
   - Add `permissions: { contents: read, id-token: write }` to the job.
   - Add `environment: ${{ startsWith(github.ref, 'refs/tags/v') && 'release' || '' }}`. Manual runs then don't touch the environment, whose tag rule would reject them.
   - Confirm on the first run that an empty name means "no environment". If it doesn't, split signed tag builds into their own job.
2. **Split the shared "Installers" step into one step per OS.** This keeps each signing sequence readable.
3. **Windows, unsigned (manual runs):** unchanged, `pnpm tauri build --features vulkan`.
4. **Windows, signed (tags), in this order:**
   1. `pnpm tauri build --features vulkan --no-bundle`. This compiles everything, which takes most of the job.
   2. **Signing tools:**
      - Download `Microsoft.ArtifactSigning.Client` **1.0.128** with `nuget install Microsoft.ArtifactSigning.Client -Version 1.0.128 -x -OutputDirectory $RUNNER_TEMP\asc`. Pin it exactly: 1.0.146 is from 2026-10-08, inside the 30-day rule in docs/versions.md.
      - Find the x64 `Azure.CodeSigning.Dlib.dll` inside the package (check the folder layout on the first run).
      - Locate the newest x64 `signtool.exe` under `C:\Program Files (x86)\Windows Kits\10\bin\`. It must be at least 10.0.2261.755.
      - Make sure the .NET 8 runtime is present, adding `actions/setup-dotnet` with `8.0.x` if the image lacks it. The dlib fails silently without it.
   3. **`metadata.json`** in `$RUNNER_TEMP`, from the variables. Keep only `AzureCliCredential`, so the dlib doesn't wait on managed-identity probes on a GitHub runner:

      ```json
      {
        "Endpoint": "${AZURE_SIGNING_ENDPOINT}",
        "CodeSigningAccountName": "${AZURE_SIGNING_ACCOUNT}",
        "CertificateProfileName": "${AZURE_SIGNING_PROFILE}",
        "CorrelationId": "${GITHUB_REF_NAME}-${GITHUB_RUN_ID}",
        "ExcludeCredentials": ["EnvironmentCredential", "WorkloadIdentityCredential", "ManagedIdentityCredential",
          "SharedTokenCacheCredential", "VisualStudioCredential", "VisualStudioCodeCredential",
          "AzurePowerShellCredential", "AzureDeveloperCliCredential", "InteractiveBrowserCredential"]
      }
      ```

   4. **A CI-only Tauri overlay**, `$RUNNER_TEMP/sign.windows.json`. Use the object form, because the string form splits on spaces and the signtool path has spaces:

      ```json
      { "bundle": { "windows": { "signCommand": {
          "cmd": "<full path to signtool.exe>",
          "args": ["sign", "/v", "/fd", "SHA256", "/tr", "http://timestamp.acs.microsoft.com", "/td", "SHA256",
                   "/dlib", "<path to Azure.CodeSigning.Dlib.dll>", "/dmdf", "<path to metadata.json>", "%1"] } } } }
      ```

   5. **Log in to Azure.** Run `azure/login@v3.1.0` (pin by tag or commit) with `client-id`, `tenant-id` and `subscription-id` from the secrets. Straight after it, run `az account get-access-token --resource https://codesigning.azure.net`. This caches the signing token while GitHub's 5-minute sign-in token is still valid. The resource URI is believed correct; check it on the first run.
      - If `azure/login` complains that no subscription is visible, assign the signer role at resource-group scope, or add `allow-no-subscriptions: true`.
   6. `pnpm tauri bundle --bundles nsis --features vulkan --config $RUNNER_TEMP/sign.windows.json`. Tauri signs the app, the four DLLs, the NSIS plugins, `uninstall.exe` and the installer.
   7. **Verify.**
      - Run `signtool verify /pa /v` on the installer.
      - Silently install it (`/S`) on the runner, then run `signtool verify /pa /v` on every `.exe` and `.dll` in `%LOCALAPPDATA%\Programs\OpenBark`.
      - Fail the job if any file isn't signed by ANI.ML Health Inc. or has no timestamp.
5. **macOS, signed (tags):**
   1. **Temporary keychain** (the pattern in Tauri's macOS guide):
      - decode `APPLE_CERTIFICATE` to a `.p12` in `$RUNNER_TEMP`;
      - create `build.keychain` with a random password generated in the step (no secret needed);
      - make it the default keychain and unlock it, with a lock timeout longer than the build (for example 21600 seconds);
      - `security import` the `.p12` with `-T /usr/bin/codesign`;
      - `security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k <password> build.keychain`;
      - delete the `.p12` file.

      One keychain then serves both Tauri and `macos-dmg.sh`. Don't also pass `APPLE_CERTIFICATE` to the build step: Tauri would import it a second time into its own short-lived keychain, which the script can't use.
   2. Write `APPLE_API_PRIVATE_KEY` to `$RUNNER_TEMP/private_keys/AuthKey_${APPLE_API_KEY}.p8`.
   3. Run `scripts/macos-dmg.sh --features metal` with these variables set:
      - `APPLE_SIGNING_IDENTITY`
      - `APPLE_API_KEY`
      - `APPLE_API_ISSUER`
      - `APPLE_API_KEY_PATH` (the path from step 2)
   4. **Verify.**
      - `codesign --verify --deep --strict --verbose=2` on both apps in the image;
      - `spctl -a -t exec -vv` on both apps;
      - `spctl -a -t open --context context:primary-signature -vv` on the DMG;
      - `xcrun stapler validate` on the app, the uninstaller and the DMG;
      - `codesign -d --entitlements -` on the app, which must show `com.apple.security.device.audio-input`.
   5. With `if: always()`, run `security delete-keychain build.keychain` and remove the `.p8`.
6. **Linux, signed (tags):** set `TAURI_SIGNING_RPM_KEY` from `OPENBARK_RELEASE_GPG_KEY` and `TAURI_SIGNING_RPM_KEY_PASSPHRASE` from `OPENBARK_RELEASE_GPG_PASSPHRASE` on the `tauri build` step. These names come from the tauri-bundler 2.10.3 source and aren't in Tauri's docs. Then check the result: import the public key into a throwaway rpm database and run `rpm -K` on the `.rpm`. Leave the AppImage unsigned.
7. Upload paths stay the same.

**Job `public-draft`**

8. Add `environment: release`. It already runs only on tags.
9. After the `sha256sum` line:
   - import `OPENBARK_RELEASE_GPG_KEY` into a temporary `GNUPGHOME`;
   - run `gpg --batch --pinentry-mode loopback --passphrase-fd 0 --local-user "$OPENBARK_RELEASE_GPG_FINGERPRINT" --armor --detach-sign --output SHA256SUMS.txt.asc SHA256SUMS.txt`;
   - export the public key as `openbark-release-key.asc`;
   - check with `gpg --verify`.

   Upload both new files with the installers.
10. Add one sentence to the release notes: "Windows installer signed by ANI.ML Health Inc.; macOS disk image notarized by Apple; checksums signed with the OpenBark Releases key, fingerprint <fingerprint> (see www.openbark.io)."

**Job `draft-release`**

11. No change needed. Optionally attach the same `.asc` files.

### `apps/desktop/src-tauri/tauri.conf.json` and its platform files

12. In `tauri.conf.json`:
    - `bundle.publisher`: "ANI-ML" becomes **"ANI.ML Health Inc."**, so Apps & features matches the certificate;
    - `bundle.copyright` becomes "Copyright 2026 ANI.ML Health Inc. Apache-2.0.".

    The NSIS template keys `Software\<publisher>\OpenBark` by publisher, so test installing over a beta build. The install folder is fixed, so the old key is just left behind.

    While there, consider pointing `bundle.homepage` at `https://www.openbark.io`. It currently points at the private repo, and package managers show it.
13. **Don't add `signCommand` to `tauri.conf.json` or `tauri.windows.conf.json`.** It lives only in the CI overlay, so local builds don't try to sign.
14. In `tauri.macos.conf.json`:
    - add `"entitlements": "Entitlements.plist"` under `bundle.macOS`;
    - keep `"signingIdentity": "-"`, because CI overrides it with `APPLE_SIGNING_IDENTITY`;
    - keep the hardened runtime at its default (on).
15. Add `apps/desktop/src-tauri/Entitlements.plist` with one key. This fixes recording even in ad-hoc builds, so it can land before signing does.

    ```xml
    <?xml version="1.0" encoding="UTF-8"?>
    <!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
    <plist version="1.0"><dict>
      <key>com.apple.security.device.audio-input</key><true/>
    </dict></plist>
    ```

### `scripts/macos-dmg.sh`

16. **Signing mode.**
    - The script signs when `APPLE_SIGNING_IDENTITY` is set and isn't `-`.
    - In signing mode it needs `APPLE_API_KEY`, `APPLE_API_ISSUER` and `APPLE_API_KEY_PATH`, and stops at once with a plain message if one is missing.
    - Without signing mode, everything stays as today (ad-hoc).
    - Update the header comment, and the `sed -n '2,11p'` range that `--help` prints.
17. **The app.** The build line stays the same. With the variables set, Tauri signs with the hardened runtime and the entitlements, then notarizes and staples. Afterwards run `xcrun stapler validate "$APP"`.
18. **The uninstaller.** In signing mode, replace `codesign --force --sign - "$UNINSTALLER" || true` with:
    1. `codesign --force --options runtime --timestamp --sign "$APPLE_SIGNING_IDENTITY" "$UNINSTALLER"`, with no `|| true`;
    2. `ditto -c -k --keepParent "$UNINSTALLER" "$WORK/uninstaller.zip"`;
    3. `xcrun notarytool submit "$WORK/uninstaller.zip" --key "$APPLE_API_KEY_PATH" --key-id "$APPLE_API_KEY" --issuer "$APPLE_API_ISSUER" --wait`;
    4. `xcrun stapler staple "$UNINSTALLER"`.

    Stapling has to happen before the disk image is made, because the image is read-only afterwards.

    **Prerequisite, outside this file:** give `install/macos/Uninstall OpenBark.app` a small compiled main executable that runs the existing script from `Contents/Resources`, for example a C or Swift launcher built on the runner. Then point `CFBundleExecutable` at it. A bash script as main executable is believed to cause notarization and stapling trouble.
19. **Copying the app.** Use `ditto "$APP" "$WORK/stage/OpenBark.app"` instead of `cp -R`, so the signature and the stapled ticket are copied exactly.
20. **The disk image.** After either create-dmg branch, including the no-layout retry, in signing mode:
    1. `codesign --force --timestamp --sign "$APPLE_SIGNING_IDENTITY" "$OUT"`;
    2. `xcrun notarytool submit "$OUT" ... --wait`;
    3. `xcrun stapler staple "$OUT"`;
    4. `xcrun stapler validate "$OUT"`.
21. **Failures.** If any notarization submission comes back Invalid, run `xcrun notarytool log <submission id> --key ... --key-id ... --issuer ...`, print the log and exit non-zero.

### Other follow-ups (separate changes)

| File | Change |
|---|---|
| `docs/network.md` | Build-time rows: Windows signing (`login.microsoftonline.com`, `eus.codesigning.azure.net`, `timestamp.acs.microsoft.com`, `www.nuget.org`) and macOS notarization (`timestamp.apple.com` and `appstoreconnect.apple.com`, believed). A note that Windows and macOS may check certificates or notarization online on their own, and that OpenBark itself makes no such call |
| `docs/versions.md` | Pins: `azure/login` v3.1.0 (2026-09-10), `Microsoft.ArtifactSigning.Client` 1.0.128 (2026-03-30), and `azure/artifact-signing-action` v2.0.0 (2026-05-14) when the CLI and `ggml-cuda.dll` are signed |
| `docs/runbooks/macos.md` step 5 | Signed builds no longer need Open Anyway. Ad-hoc local builds still do |
| `install/README.md` and www.openbark.io | What users see on first download (decision 9 of ADR-0020), and a "Check your download" page with the public key, its fingerprint and the commands in Part F |
| ADR-0012 | Point decision 6 at ADR-0020 once ADR-0020 is accepted |
| ADR-0019 work | Sign the `openbark` CLI and `ggml-cuda.dll` with `azure/artifact-signing-action` before computing their SHA-256 pins |
