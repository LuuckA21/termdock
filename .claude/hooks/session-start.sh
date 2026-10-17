#!/bin/bash
# SessionStart hook for Claude Code cloud sessions: installs what the CI
# checks, the SSH end-to-end tests and the headless app runs need, so every
# session can lint, test and build right away. Idempotent; does nothing on a
# developer machine (CLAUDE_CODE_REMOTE unset).
set -euo pipefail

if [ "${CLAUDE_CODE_REMOTE:-}" != "true" ]; then
  exit 0
fi

cd "${CLAUDE_PROJECT_DIR:-$(dirname "$0")/../..}"

# System packages: Tauri/WebKitGTK build deps (same list as CI), openssh-server
# for scripts/e2e-sshd.sh, Xvfb + xdotool + ImageMagick to drive the app headless.
PACKAGES=(
  libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev libayatana-appindicator3-dev libdbus-1-dev
  openssh-server xvfb xdotool imagemagick
)
missing=()
for pkg in "${PACKAGES[@]}"; do
  dpkg -s "$pkg" >/dev/null 2>&1 || missing+=("$pkg")
done
if [ "${#missing[@]}" -gt 0 ]; then
  SUDO=""
  [ "$(id -u)" -ne 0 ] && SUDO="sudo"
  $SUDO apt-get update -qq
  DEBIAN_FRONTEND=noninteractive $SUDO apt-get install -y -qq --no-install-recommends "${missing[@]}"
fi

# Rust: stable toolchain with the components CI uses.
if command -v rustup >/dev/null 2>&1; then
  rustup component add clippy rustfmt >/dev/null 2>&1 || true
fi

# Node dependencies (package-lock.json is the source of truth).
npm install --no-audit --no-fund

# Crate sources, so builds and tests work offline from here on.
cargo fetch --locked
