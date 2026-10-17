# Termdock: notes for contributors and agents

Cross-platform terminal & SSH connection manager. Rust backend
(`crates/termdock-core`: no UI; `src-tauri`: Tauri 2 shell), TypeScript UI
without framework in `src/`. See README.md for architecture and SECURITY.md
for the threat model; keep both up to date when behaviour changes.

## Workflow

- `main` is protected: work on a branch, open a PR, wait for CI and for the
  owner's explicit OK before merging (squash only).
- Before pushing run the same checks as CI:
  `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace`, `npm run typecheck`, `npm run version:check`,
  `node scripts/updater-manifest.mjs check-config`.
  SSH changes: also the end-to-end suite (`scripts/e2e-sshd.sh`, see README).
- Languages: code, comments, identifiers, log/error messages, commit messages
  and PR descriptions are in **English only**. Documentation is kept in
  **English and Italian**: `README.md` / `README.it.md`, `SECURITY.md` /
  `SECURITY.it.md`, `CHANGELOG.md` / `CHANGELOG.it.md`; update both files
  of a pair in the same PR. UI strings go through `t()` from `src/i18n.ts`,
  with one dictionary per language in `src/locales/` (every key in every
  locale).

## Agent sessions

- `.claude/hooks/session-start.sh` prepares cloud sessions (system packages,
  npm modules, crates). Skills: `steward` (PR, merge and release procedure)
  and `termdock-dev` (SSH e2e, headless app runs, local update tests).

## Versioning and releases (owned by the agent, not the user)

- Semantic Versioning. Before 1.0: new features -> minor (0.2.0 -> 0.3.0),
  fixes only -> patch (0.2.0 -> 0.2.1). Breaking changes to stored data must
  keep a migration path (see `store.rs` schema version).
- Every PR with user-visible changes adds lines under `## [Unreleased]` in
  CHANGELOG.md (Added / Changed / Fixed / Security) and the same lines in
  CHANGELOG.it.md (Aggiunto / Modificato / Corretto / Sicurezza).
- A PR that should ship runs `npm run version:set -- X.Y.Z`: it updates
  package.json, package-lock.json, Cargo.toml, Cargo.lock, tauri.conf.json and
  turns `[Unreleased]` into the `[X.Y.Z]` section in both changelogs. Do not
  edit versions by hand.
- When such a PR lands on main, `.github/workflows/release.yml` creates the
  `vX.Y.Z` tag and builds a **draft** release whose notes are the English
  changelog section followed by the Italian one; the owner tests and
  publishes it.
- Never push tags from an agent session (the session proxy refuses them);
  the release workflow creates them.
- In-app updates: packages are signed with the updater key (secrets
  `TAURI_SIGNING_PRIVATE_KEY` / `_PASSWORD` of the `release` environment,
  public key in `plugins.updater.pubkey`). Never commit a private key, never
  relax `plugins.updater` (https only, `requireSignedVersion`, no downgrades),
  keep the actions in workflows pinned to commit SHAs, and do not change the
  public key: installed copies only trust the key they were built with, and the
  release workflow verifies against the key in `tauri.conf.json`, so rotating
  it needs a dedicated, planned procedure. A draft release without
  `latest.json` (a build leg failed) must not be published.
  `scripts/updater-manifest.mjs check-config` guards the configuration.
