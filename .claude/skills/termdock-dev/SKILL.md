---
name: termdock-dev
description: Hands-on recipes for Termdock in a cloud session - SSH end-to-end tests, running and screenshotting the app headless, testing in-app updates locally, and keeping disk space under control. Use when you need to exercise the real app or the SSH stack, not just compile it.
---

# Termdock development recipes

The SessionStart hook (`.claude/hooks/session-start.sh`) installs the system
packages, npm modules and crates these recipes need.

## SSH end-to-end tests

```sh
eval "$(sudo scripts/e2e-sshd.sh)"     # throw-away sshd instances, prints env vars
cargo test -p termdock-core --test ssh_e2e
```

Covers host key TOFU and changed keys, passwords, keys with passphrase,
keyboard-interactive, jump hosts and -L/-R/-D forwarding.

## Run the app headless

Debug builds load the UI from the Vite dev server:

```sh
nohup npx vite --port 1420 --strictPort > /tmp/vite.log 2>&1 &
cargo build -p termdock
(pgrep Xvfb >/dev/null || nohup Xvfb :99 -screen 0 1280x800x24 >/dev/null 2>&1 &)
DISPLAY=:99 WEBKIT_DISABLE_DMABUF_RENDERER=1 nohup ./target/debug/termdock > /tmp/app.log 2>&1 &
sleep 12
DISPLAY=:99 xdotool key ctrl+shift+e          # drive it with keys/clicks
DISPLAY=:99 import -window root /tmp/shot.png # then Read the screenshot
```

- Vite reloads the UI on save; the app keeps running.
- UI preferences live in WebKit local storage, so they persist between runs.
- Stop processes by exact match: `pkill -x termdock`,
  `kill $(pgrep -f '^node .*vite --port')`. A plain `pkill -f <pattern>` also
  matches (and kills) the shell running the command.

## Test in-app updates locally

1. `npx tauri signer generate -w <scratch>/test.key -p testpass --ci` (a
   throw-away key; never commit it or use it for releases).
2. Build two AppImages with a config override, e.g.
   `{"version":"0.9.0","bundle":{"targets":["appimage"]},"plugins":{"updater":{"pubkey":"<test pub>","endpoints":["http://127.0.0.1:8765/latest.json"],"dangerousInsecureTransportProtocol":true}}}`
   passed as `npx tauri build --config <file>` with
   `TAURI_SIGNING_PRIVATE_KEY=<key path>` and `..._PASSWORD`.
3. Serve the newer AppImage, its signature and a `latest.json` with
   `python3 -m http.server 8765`, run the older one with
   `APPIMAGE_EXTRACT_AND_RUN=1` under Xvfb and accept the update.
4. Also try a tampered file and a manifest announcing another version: both
   must be rejected and leave the installed file unchanged.

`scripts/updater-manifest.mjs verify <file> <file.sig> [version]` checks a
signature against the public key in `tauri.conf.json`.

## Disk space

`target/` grows past 25 GB with release builds. When a build fails with "No
space left on device", run `cargo clean` (or delete `target/release`) and the
scratch build outputs; deletes work even when the disk is full.
