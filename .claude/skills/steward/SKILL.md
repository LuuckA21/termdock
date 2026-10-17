---
name: steward
description: How to drive a Termdock pull request from push to release - checks, review fixes, merge policy, versioning and release verification. Use when opening, updating, merging or releasing a PR in this repository.
---

# Driving a Termdock PR

The owner decides what ships. The agent owns code quality, CI, versioning and
release verification. CLAUDE.md is the rulebook; this is the procedure.

## Before every push

Run exactly what CI runs and push only when all of it is clean:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
npm run typecheck
npm run version:check
node scripts/updater-manifest.mjs check-config
npm run build
```

SSH changes also need the end-to-end suite (`skill termdock-dev`).
UI changes are checked in the running app, not only by the type checker
(`skill termdock-dev`, "Run the app headless").

## Opening the PR

- Branch from the latest `origin/main`, never commit to `main`.
- English for code, commits and the PR body. User-visible changes get lines
  under `## [Unreleased]` in both CHANGELOG.md and CHANGELOG.it.md; README and
  SECURITY pairs change together.
- A PR meant to ship runs `npm run version:set -- X.Y.Z` (features -> minor,
  fixes only -> patch before 1.0). Never edit versions by hand.
- For non-trivial changes, ask an independent reviewer agent for a read-only
  review of the diff and fix what holds up before asking for the merge.

## CI and reviews

- Red CI on our PR is always ours to fix now; root-cause it, no re-run
  "to see", never skip or weaken a test.
- Small, clearly correct review asks: fix and push. Design-level changes:
  propose them to the owner first.

## Merging

- Merge only after the owner's explicit OK in the conversation, with a green
  head and no conflicts. Squash merge, passing the expected head SHA.
- After merging, the Release workflow runs on `main`.

## Release verification (every version bump)

1. Watch the Release run: `prepare` (updater key check, tag), three `build`
   legs, then `updater`.
2. The draft release must contain installers, `.sig` files and `latest.json`.
   A draft without `latest.json` (a failed leg) must never be published: fix,
   bump the patch version, release again.
3. Tell the owner the draft is ready; only the owner publishes. Older
   superseded drafts can be deleted by the owner.

## Never

- Push tags (the session proxy refuses them; the workflow creates them).
- Commit a private key, relax `plugins.updater`, change the updater public key
  or unpin a workflow action from its commit SHA.
