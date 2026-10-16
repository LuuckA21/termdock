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
  `cargo test --workspace`, `npm run typecheck`, `npm run version:check`.
  SSH changes: also the end-to-end suite (`scripts/e2e-sshd.sh`, see README).
- UI strings go through `src/i18n.ts` (Italian and English).

## Versioning and releases (owned by the agent, not the user)

- Semantic Versioning. Before 1.0: new features -> minor (0.2.0 -> 0.3.0),
  fixes only -> patch (0.2.0 -> 0.2.1). Breaking changes to stored data must
  keep a migration path (see `store.rs` schema version).
- Every PR with user-visible changes adds lines under `## [Unreleased]` in
  CHANGELOG.md (Added / Changed / Fixed / Security).
- A PR that should ship runs `npm run version:set -- X.Y.Z`: it updates
  package.json, package-lock.json, Cargo.toml, Cargo.lock, tauri.conf.json and
  turns `[Unreleased]` into the `[X.Y.Z]` section. Do not edit versions by hand.
- When such a PR lands on main, `.github/workflows/release.yml` creates the
  `vX.Y.Z` tag and builds a **draft** release with the CHANGELOG section as
  notes; the owner tests and publishes it.
- Never push tags from an agent session (the session proxy refuses them);
  the release workflow creates them.
