# Security policy

OpenBark records veterinary consults and keeps recordings, transcripts and notes encrypted on the clinic's own computer. We take reports about its security seriously and are grateful for them.

## Reporting a vulnerability

Please report security problems privately. **Do not open a public issue, discussion or pull request** for a vulnerability.

Use either of these:

1. **GitHub private vulnerability reporting.** On the repository page, open **Security**, then **Report a vulnerability**. This creates a private advisory that only the maintainers can see.
2. **Email** [we.care@animl.health](mailto:we.care@animl.health) with a subject line starting `Security:`.

Please include:

- what the problem is and what an attacker could do with it;
- the steps to reproduce it, or a proof of concept;
- the OpenBark version (the installer file name, such as `OpenBark_0.0.1_x64-setup.exe`, or the commit you built from) and your operating system;
- whether you think it is already being exploited.

**Never include real patient or client data** in a report: no real recordings, transcripts, notes, names or clinic details. Use the synthetic consults in `packages/eval/fixtures` or make up your own.

## What happens next

- We aim to acknowledge your report within five business days and to give you an assessment within ten.
- We will keep you updated while we work on a fix, and agree a disclosure date with you. We prefer to publish an advisory once a fixed build is available.
- If you would like, we will credit you in the advisory and the changelog.

We will not take legal action against anyone who reports in good faith, gives us reasonable time to fix the problem before disclosing it, and avoids accessing or changing other people's data.

## Supported versions

OpenBark has not had a stable release yet. Security fixes go into the `main` branch and the next build made from it. Older builds are not patched: please update to the newest one.

| Version | Supported |
|---|---|
| `main` and the newest pre-release build | Yes |
| Anything older | No |

## Scope

OpenBark is local-first. The app has no server component run by ANI.ML Health: no accounts, no cloud storage and no telemetry. Everything runs on the clinic's computer. Reports are most useful in these areas:

- **Encryption at rest:** recordings, transcripts, notes and the patient list (XChaCha20-Poly1305), and how the key and recovery key are stored ([ADR-0008](docs/adr/0008-encryption-at-rest.md)).
- **The browser version's local web server:** it should listen on 127.0.0.1 only and refuse requests from other sites.
- **Network calls:** OpenBark should make only the connections listed in [docs/network.md](docs/network.md). Anything else is a bug we want to hear about.
- **Downloads:** model files and the optional GPU speech pack are checked against recorded SHA-256 hashes before use.
- **Installers, install scripts and the release pipeline** in `install/`, `scripts/` and `.github/workflows/`.
- **Signing and roles:** a way to sign a note without the veterinarian role, or to change a signed note without an addendum.

Optional accounts and encrypted sync are planned but not built yet. They will get their own section here when they ship.

Not in scope: the content of a draft note. Drafts can be wrong, which is why a veterinarian reviews and signs every note. Please report wrong or invented content as a normal [bug report](https://github.com/ANI-ML/openbark/issues/new/choose), with synthetic examples only.
