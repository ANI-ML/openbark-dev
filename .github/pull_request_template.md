## What this changes

<!-- A sentence or two: what changes, and why. -->

Closes #

## How it was tested

<!-- Commands you ran and what you checked by hand. For UI changes, add screenshots in light and dark themes and at phone width. For drafting changes, paste the openbark eval results before and after. -->

## Checklist

- [ ] No real patient or client data, recordings, transcripts, notes or secrets anywhere in this change, including screenshots, videos and test fixtures.
- [ ] `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` pass.
- [ ] `pnpm -r run typecheck` and `pnpm -r run test` pass.
- [ ] New behaviour has tests (written first for the engine pipeline, templates, faithfulness checks and connectors).
- [ ] Drafts still stay unsigned until a veterinarian signs, and nothing is invented: anything not discussed shows as "Not discussed".
- [ ] Any new network connection is listed in `docs/network.md`.
- [ ] Any new or changed dependency is checked live and recorded in `docs/versions.md`.
- [ ] Any architectural decision has a decision record in `docs/adr`.
- [ ] UI changes follow the brand guide (`docs/brand/DESIGN.md`), and user-facing text is translated in `packages/i18n`.
- [ ] Changes that matter to a clinic have an entry in `website/content/history.json`.
- [ ] Every commit is signed off (`git commit -s`), as described in CONTRIBUTING.md.

<!-- Not applicable? Tick it and add "n/a". -->
