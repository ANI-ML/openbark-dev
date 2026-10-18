# The OpenBark website

A static site: a landing page with downloads, and a features page generated from `content/features.json`. It's served by Vercel from `website/dist`, which is committed, so Vercel has nothing to build.

## Edit and rebuild

- Landing page: `src/index.html` (the page body; header and footer come from the build script). Styles: `src/site.css`. Behaviour (the evidence-link demo, platform detection, downloads): `src/site.js`.
- Features page: `content/features.json`. Each item has a name, benefit, detail, evidence (where it lives in the code or evaluation) and status (`shipped`, `testing` or `coming`).
- Product name, tagline and credit: the top of `scripts/website-build.py`. A rename is one line there plus `public/release.json`.

Rebuild after any change, then commit `website/dist` with the source:

```bash
python scripts/website-build.py
```

To look at it locally, serve `website/dist` with any static server, for example `python -m http.server -d website/dist 8080`.

## Publish on Vercel (first time)

1. In Vercel, **Add New > Project**, and import the GitHub repo **ANI-ML/openbark**.
2. Set **Root Directory** to `website` (recommended) or leave it blank: both work. `website/vercel.json` and the root `vercel.json` set no install or build step and point at the committed `website/dist`. Leave **Framework Preset** as **Other** and the build settings empty.
3. Deploy. Vercel gives the site a `*.vercel.app` address.
4. When the domain is bought: **Project > Settings > Domains**, add it, and follow Vercel's DNS instructions at the registrar.

Every push to the repo's default branch redeploys the site.

## Downloads

The download buttons read `public/release.json`. Until a release is published they show "Coming with the public beta", and the iPhone card shows "App Store: coming soon" until `app_store` is set to the App Store link.

Installers are published from the public repo **ANI-ML/openbark-releases**. The main repo stays private. One-time setup:

1. Create the public repo `ANI-ML/openbark-releases` (it can be empty).
2. Create a fine-grained GitHub token with **Contents: read and write** on that repo, and add it to `ANI-ML/openbark` as the Actions secret `PUBLIC_RELEASES_TOKEN`.

Then, for each version:

1. Tag the main repo (`git tag v0.1.0 && git push --tags`). The **Release installers** workflow builds Windows, macOS and Linux installers and creates a *draft* release, with checksums, in the public repo.
2. Check the draft on GitHub and publish it.
3. Run the **Website release** workflow (Actions tab) with the tag. It points `release.json` at the published files, with their sizes, rebuilds the site and commits; Vercel redeploys.

## Privacy

The site has no analytics, cookies or third-party scripts. It loads two fonts from Google Fonts. `vercel.json` sets a strict Content-Security-Policy.
