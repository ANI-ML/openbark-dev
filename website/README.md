# The OpenBark website

A static site: a landing page with downloads, a features page generated from `content/features.json`, a version history from `content/history.json`, and an account portal (`/portal`). It's served by Vercel from `website/dist`, which is committed, so Vercel has nothing to build.

## Edit and rebuild

- Landing page: `src/index.html` (the page body; header and footer come from the build script). Styles: `src/site.css`. Behaviour (the evidence-link demo, platform detection, downloads): `src/site.js`.
- Features page: `content/features.json`. Each item has a name, benefit, detail, evidence (where it lives in the code or evaluation) and status (`shipped`, `testing` or `coming`).
- Account portal: `src/portal.html` (the page body) and `src/portal.js` (sign-in, profile, downloads, delete). See "Account portal" below.
- Product name, tagline and credit: the top of `scripts/website-build.py`. A rename is one line there plus `public/release.json`.

Rebuild after any change, then commit `website/dist` with the source:

```bash
python scripts/website-build.py
```

To look at it locally, serve `website/dist` with any static server, for example `python -m http.server -d website/dist 8080`.

## Publish on Vercel (first time)

1. In Vercel, **Add New > Project**, and import the development repo, **ANI-ML/openbark-dev** (see "Repositories" below). Not the public source copy: it changes only once per release.
2. Set **Root Directory** to `website` (recommended) or leave it blank: both work. `website/vercel.json` and the root `vercel.json` set no install or build step and point at the committed `website/dist`. Leave **Framework Preset** as **Other** and the build settings empty.
3. Deploy. Vercel gives the site a `*.vercel.app` address.
4. When the domain is bought: **Project > Settings > Domains**, add it, and follow Vercel's DNS instructions at the registrar.

Every push to the repo's default branch redeploys the site.

## Downloads

The download buttons read `public/release.json`. Until a release is published they show "Coming with the public beta", and the iPhone card shows "App Store: coming soon" until `app_store` is set to the App Store link.

Installers are built in the development repo and published to the public repo **ANI-ML/openbark-releases**. One-time setup:

1. Create the public repo `ANI-ML/openbark-releases` (it can be empty).
2. Create a fine-grained GitHub token with **Contents: read and write** on that repo, and add it to `ANI-ML/openbark-dev` as the Actions secret `PUBLIC_RELEASES_TOKEN`.

Then, for each version:

1. Tag the development repo (`git tag v0.1.0 && git push --tags`). The **Release installers** workflow builds Windows, macOS and Linux installers and creates a *draft* release, with checksums, in the public repo.
2. Check the draft on GitHub and publish it.
3. Run the **Website release** workflow (Actions tab) with the tag. It points `release.json` at the published files, with their sizes, rebuilds the site and commits; Vercel redeploys.

## Repositories

- **ANI-ML/openbark-dev** (private; it was named ANI-ML/openbark until the public copy took that name): where development happens. CI, the release and website workflows, the signing secrets and the Vercel connection all live here.
- **ANI-ML/openbark** (public): a published copy of the source, refreshed once per release with one commit authored as ANI.ML Health. It has no release secrets and builds no releases, so don't push tags to it. Pull requests sent there are reviewed there and ported to the development repo by hand (`CONTRIBUTING.md`).
- **ANI-ML/openbark-releases** (public): the installers and their checksums.

## Account portal

`/portal` lets a visitor sign in with a 6-digit code emailed by Supabase Auth (no passwords, no social logins), keep a short profile (name, clinic, role, country, update emails), find the downloads for their computer, sign out and delete the account. It never holds clinical data. The OpenBark app doesn't use it.

- The Supabase project URL and publishable key are in `src/portal.js`. Both are public by design; row level security decides what a signed-in person can read and change.
- The database table, its security rules and the delete-account function live in `supabase/`. **`supabase/README.md` has the owner's one-time setup**: run the migration, deploy the function, set the Site URL and redirect URLs, put the code in the email templates, and connect SendGrid. Until SendGrid is connected, Supabase's built-in sender only reaches members of the Supabase team.
- supabase-js is loaded from jsDelivr, pinned to one version with Subresource Integrity (`SUPABASE_JS` and `SUPABASE_JS_SRI` in `scripts/website-build.py`). To upgrade it, change those two lines and the matching `script-src` entry in `vercel.json` and `website/vercel.json`; `apps/desktop/e2e/website.mjs` fails if they disagree.

## Privacy

The site has no analytics or cookies. It loads two fonts from Google Fonts. The portal page alone loads one third-party script, supabase-js from jsDelivr, checked by its SRI hash, and talks to Supabase only when the visitor signs in or acts; the session stays in the browser's localStorage. `vercel.json` sets a strict Content-Security-Policy that allows exactly that script and the one Supabase project. Every network call the website makes is listed in `docs/network.md`.
