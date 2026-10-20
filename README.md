# Termdock

**English** · [Italiano](README.it.md)

Cross-platform terminal and connection manager (Windows, macOS, Linux), in the spirit of PuTTY / Windows Terminal, with a **favorites bar** to open the connections you use most with one click.

![Termdock](docs/screenshot.png)

## Features

A short list in order of importance is in [FEATURES.md](FEATURES.md).

- **Favorites bar**, always visible: one click opens the connection in a new tab, drag to reorder, right click to edit or unpin, `Ctrl+Shift+1…9` opens the first nine.
- **SSH** with a full terminal (xterm-256color, truecolor, Unicode, resizing).
- **Jump hosts (ProxyJump)**: a connection can go through a bastion, even a chain of them; every hop verifies its own host key and authenticates separately.
- **File browser (SFTP)**: `Ctrl+Shift+S` opens the files of the server next to its terminal, on the same authenticated connection (no second login): browse, download, upload, new folder, rename, delete. Whole folders can be uploaded and downloaded too; transfers wait in a queue under the files, each with its progress and a Cancel button, and existing files are replaced or kept with one choice for all. After `sudo -iu user` in the terminal it offers to open the files as that user too (through sudo on the server, with the sudo password asked only when needed and never saved), or pick any user from its user menu. Files and folders can be dragged from the computer onto the browser to upload them; the browser can jump to (or follow) the terminal's current folder; and *Edit* (`F4`) opens a remote text file in the system text editor (or the one chosen in Settings) and uploads it back each time it is saved, asking first if the server's copy changed meanwhile.
- **Port forwarding** saved in the connection: local (`-L`), remote (`-R`) and dynamic SOCKS5 (`-D`); the status of each tunnel is shown in the terminal.
- **Import from `~/.ssh/config`**: preview with warnings, then imports Host, HostName, User, Port, IdentityFile, ProxyJump and the forwards.
- **Local shells** in a real PTY: PowerShell 7, Windows PowerShell, cmd, WSL and Git Bash on Windows (ConPTY); bash, zsh, fish, pwsh… on macOS and Linux.
- **Connection manager**: groups, colors, search, star to pin to the favorites bar. *Duplicate* copies a connection as a starting point for a similar one, and the bolt next to a group opens all of its connections, one tab each. A connection can have a **startup command** (one line typed into the shell once connected, e.g. `cd /var/www`) and can be marked as a **production server**: its tabs get a red *PROD* badge and its terminals a red border, so it is not mistaken for a test machine.
- **Quick connect**: type `user@host:port` in the bar and press Enter.
- Multiple tabs, reconnect with Enter after a session ends, PuTTY-style copy/paste (select + right click; with *Copy on select* in Settings → Appearance the selection is copied at once and right click always pastes). Pasting several lines shows them first, invisible characters marked, unless the program uses bracketed paste. Tabs can be renamed (double click), reordered by dragging and duplicated (tab menu), which also has *Clear terminal* (shortcut assignable in Settings → Keys), *Close other tabs* and *Close tabs to the right*; a hidden tab shows a dot when its terminal prints something and a bell when it rings one, and a system notification can tell you about that bell while you are elsewhere (Settings → General, off by default). Web links in the output open in the browser with `Ctrl`+click (`Cmd`+click on macOS), after a confirmation that shows where they really go. Closing a tab, pane or Termdock with open SSH connections asks first (can be turned off in Settings → General), and the window opens again where and as large as it was left.
- Quick connect remembers the last addresses that connected: `Up` / `Down` in its box, or pick one from the suggestions.
- **Automatic reconnection**: when the network or the server drops an SSH connection (noticed through the keepalive set in the connection), the terminal tries again by itself a few times over about five minutes, with the usual host-key and password checks; `Enter` tries at once, `Esc` stops. Its session log and file browser carry on once it is back. Can be turned off in Settings → General.
- **Split panes**, like Windows Terminal: divide a tab side by side or top/bottom as many times as needed and choose what opens in the new pane (the same connection, a local shell, any saved connection or a `user@host` address), e.g. server A on the left and server B on the right. Already open tabs can be put side by side without reconnecting (tab menu → *Move next to*), and favourites open directly on the right or below. Drag the dividers to resize, move between panes with `Alt+arrows`.
- **Saved layouts**: save a tab's arrangement (which connection or shell is in each pane, and the sizes) with `Ctrl+Shift+L` and reopen it from the *Layouts* menu with one click; one layout can open automatically at startup (its connections then start right away, with the usual host-key and password prompts).
- **Snippets**: saved texts (a command, a block of lines) typed into the active terminal from the *Snippets* menu or `Ctrl+Shift+P`, optionally with Enter so they run at once; each can be offered in every terminal or only for connections of one group, and the first nine can get `1…9` shortcuts in Settings → Keys. Managed in Settings → Snippets; they go in like a paste (control characters removed, several lines confirmed). They are plain text: never put passwords in them.
- **Backup of the configuration**: Settings → General → *Export…* saves connections, favorites, layouts, snippets and settings (appearance, language, keys) in one JSON file; *Import…* shows what a backup holds and then either adds only what is new or replaces the current configuration, e.g. to move to another computer. Passwords, keys and host keys are never included.
- **Session log**: tab menu → *Log to file…* writes what a terminal shows from then on to a text file of your choice (colours and other escape sequences removed), until *Stop logging* or the end of the session; a red dot on the tab shows it is on. Off unless started, since a log keeps everything printed on screen.
- **SSH keys**: *Generate…* next to the key path of a connection creates an Ed25519 key with a passphrase (saved in `~/.ssh` or wherever you choose, never over an existing file) and shows the public key to copy; *Install public key…* in the tab menu of a connected terminal adds a `.pub` to `~/.ssh/authorized_keys` on that server, like `ssh-copy-id`.
- **Command palette**: `Ctrl+Shift+A` searches everything Termdock can do right now (actions with their shortcuts, open tabs, connections, local shells, layouts and the active terminal's snippets) and runs the one you pick; typing an address offers a quick connect.
- **Find in terminal**: `Ctrl+Shift+F` searches the scrollback with highlighted matches, next/previous (`Enter` / `Shift+Enter`), match case and regular expressions.
- **Themes and fonts**: dark, light or system theme; terminal colour schemes (Termdock Dark/Light, Solarized, Dracula, Nord, Gruvbox); font, size, line height, cursor style and scrollback, with a live preview in Settings; `Ctrl` + `+` / `-` / `0` changes the font size.
- **Automatic updates**: a new version is announced at start (and once a day), installed only after you confirm, and verified against the signing key built into the app; it can be turned off in Settings.
- **Multilingual interface** (English, Italiano): choose it in Settings (`Ctrl+,`) or leave *Automatic* to follow the system language; the switch is immediate and open sessions keep running.

### Shortcuts

| Keys | Action |
| --- | --- |
| `Ctrl+Shift+T` | new local shell (default one) |
| `Ctrl+Shift+E` / `Ctrl+Shift+O` | split the pane right / down (then choose what to open; Enter = same connection) |
| `Ctrl+Shift+S` | file browser (SFTP) for the SSH terminal |
| `Ctrl+Shift+F` | find in the terminal |
| `Ctrl+Shift+P` | snippets menu |
| `Ctrl+Shift+L` | save the tab as a layout |
| `Alt+←↑→↓` | move to the next pane (with several panes) |
| `Ctrl+Shift+W` | close the pane (the tab with its last pane) |
| `Ctrl+Tab` / `Ctrl+Shift+Tab` | next / previous tab |
| `Alt+1…9` (macOS: `Cmd+1…9`) | go to tab N |
| `Ctrl+Shift+1…9` | open favorite N |
| `Ctrl+Shift+K` | quick connect |
| `Ctrl+Shift+M` | connection manager |
| `Ctrl+Shift+A` | command palette |
| `Ctrl+Shift+C` / `Ctrl+Shift+V` | copy / paste |
| `Ctrl` + `+` / `-` / `0` | larger / smaller / default font size |
| `Ctrl+,` | settings (language, appearance, keys, snippets, files, updates) |
| right click in the terminal | copy the selection, otherwise paste (with *Copy on select*: always paste) |
| `Ctrl`+click on a link (macOS: `Cmd`+click) | open it in the browser, after a confirmation |

These are the defaults (`Cmd` works in place of `Ctrl` on macOS). Every shortcut can be changed or turned off in **Settings → Keys**: click it and press the new keys; a combination already in use is moved only after confirmation, and keys that terminal programs or AltGr typing need get a warning first.

## Security

The main choices (details in [SECURITY.md](SECURITY.md)):

- **Rust backend** (memory safe), SSH implemented in pure Rust with [russh](https://github.com/Eugeny/russh): no external `ssh` process, no intermediate shell.
- **Modern algorithms**: hybrid post-quantum key exchange `mlkem768x25519-sha256` first, then `curve25519-sha256` (NIST ECDH and large DH groups only as fallback); AEAD ciphers preferred (`chacha20-poly1305`, `aes-gcm`), then AES-CTR; SHA-2 MACs, encrypt-then-MAC first; *strict KEX* (Terrapin mitigation). SHA-1, CBC, DSA, `ssh-rsa` and compression are excluded.
- **Host key verification** before any authentication: on first connection the SHA-256 fingerprint is shown for confirmation (default: *Cancel*); if the key changes the connection is **refused** and the dialog offers no way to force it. Supports the OpenSSH format, including hashed entries, wildcards and `@revoked`, and also reads `~/.ssh/known_hosts`.
- **No secrets on disk**: passwords and passphrases are stored only if you ask, and only in the operating system keychain (Windows Credential Manager, macOS Keychain, Secret Service on Linux). They are wiped from memory after use (`zeroize`), never sent back to the UI, and deleted if the server rejects them.
- Changing a connection's host, port, user or authentication method deletes its saved password, so it is never sent to a different server.
- Authentication: SSH agent (ssh-agent, Windows OpenSSH agent, Pageant), private keys (also encrypted), password, keyboard-interactive / 2FA.
- Text coming from the server outside the session (banner, exit messages) is shown without control characters: no escape sequence injection.
- **Least-privilege Tauri 2**: strict Content-Security-Policy (no inline or remote scripts), every IPC command explicitly allowed in [`capabilities/default.json`](src-tauri/capabilities/default.json), no file system or shell plugins. The UI builds the DOM with `textContent` only (no `innerHTML`).
- Configuration files with `0600` permissions (`0700` folder) on Unix, atomic writes.
- **Signed updates**: every update package is verified (Ed25519/minisign) against the public key built into the app and must be signed for the version it claims to be, so neither a tampered package nor an older release can be installed. Details in [SECURITY.md](SECURITY.md#updates-src-taurisrcupdatesrs-scriptsupdater-manifestmjs).
- CI with `cargo audit`, `npm audit`, clippy with `-D warnings`, end-to-end tests against OpenSSH and weekly Dependabot.

> Tip: use **Ed25519** keys (`ssh-keygen -t ed25519`) protected by a passphrase, ideally loaded in an SSH agent.

## Architecture

```
crates/termdock-core   UI-independent logic, tested in isolation
  ├─ profile.rs         connection model, validation, "user@host:port" parser, jump chains
  ├─ store.rs           profiles.json (atomic, 0600) and favorites order
  ├─ vault.rs           OS keychain (keyring)
  ├─ hostkeys.rs        known_hosts: TOFU, changed/revoked keys
  ├─ ssh.rs             SSH client (russh): algorithms, authentication, jump hosts
  ├─ session.rs         session events and output shared by SSH and local shells
  ├─ forward.rs         port forwarding -L / -R / -D (SOCKS5)
  ├─ sftp.rs            SFTP on the terminal's connection (file browser)
  ├─ sshconfig.rs       ~/.ssh/config import
  ├─ local.rs           local shells through a PTY (portable-pty / ConPTY)
  ├─ shells.rs          detection of the available shells
  └─ error.rs           error type shared by the crate
src-tauri/             Tauri 2 desktop app
  ├─ commands.rs        IPC commands for profiles and sessions
  ├─ prompts.rs         questions for the user (host key, password, OTP)
  ├─ sftp.rs            file browser commands, native dialogs, dropped files
  ├─ transfer.rs        uploads and downloads of files and folders: plan, then run
  ├─ edit.rs            Edit: private copy, watching and upload on save
  ├─ editor.rs          which text editor opens it (system or chosen in Settings)
  ├─ desktop.rs         web links to the browser, system notifications
  └─ updates.rs         signed in-app updates
src/                   TypeScript + xterm.js UI (no framework)
```

Terminal data travels on binary Tauri IPC channels; questions for the user (host key, password, OTP) use a separate channel and expire after 3 minutes.

Data is stored in:

| System | Path |
| --- | --- |
| Windows | `%APPDATA%\dev.termdock.app\` |
| macOS | `~/Library/Application Support/dev.termdock.app/` |
| Linux | `~/.config/dev.termdock.app/` |

(`profiles.json`, `known_hosts` and `editor.json`; passwords live only in the keychain, and a connection's startup command is plain text in `profiles.json`. Interface preferences such as language, appearance and keyboard shortcuts, saved layouts, snippets and the quick-connect history are kept in the web view's local storage; the window's size and position in `.window-state.json`. Backups and session logs are only written where you choose.)

## Development

Requirements: stable [Rust](https://rustup.rs) (≥ 1.90), Node.js ≥ 22 and the [Tauri prerequisites](https://tauri.app/start/prerequisites/) for your system (on Linux: `libwebkit2gtk-4.1-dev`, `libgtk-3-dev`, `librsvg2-dev`, `libdbus-1-dev`). The released Linux packages are built in Ubuntu 22.04 and need glibc 2.35 or newer (Ubuntu 22.04, Debian 12, Fedora 36 and later).

```sh
npm ci
npm run tauri dev        # run the app in development mode
npm run tauri build -- --no-sign   # build the installers (MSI/NSIS, DMG, AppImage/deb/rpm) without the updater key
```

Tests and checks:

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
npm run typecheck
npm run version:check

# end-to-end tests against a real OpenSSH (Linux, needs root and openssh-server;
# creates throw-away users and sudo rules, so use a VM or CI runner)
eval "$(sudo scripts/e2e-sshd.sh)"
cargo test -p termdock-core --test ssh_e2e
```

The end-to-end tests cover: unknown host key refused, trust on first use, wrong password and retries, password remembered in the keychain and forgotten when rejected, encrypted private key with passphrase, keyboard-interactive, jump host with `-L` / `-R` / `-D` forwards, SFTP (upload, overwrite, download, cancel, rename, delete, rejected names), SFTP as another user through sudo (without password, password only after the prompt, wrong then right, three tries, refused user, unsafe names), session close and **changed host key (MITM simulation)**.

Claude Code cloud sessions are prepared by `.claude/hooks/session-start.sh`, and the project skills in `.claude/skills/` describe the PR/release procedure and how to run the app headless.

Code, comments and commit messages are in English; documentation is kept in English and Italian (`*.md` and `*.it.md`).

UI strings live in [`src/locales/`](src/locales), one file per language. To add a language, copy `en.ts` to `<code>.ts`, translate it (the `Dict` type makes the build fail if a key is missing) and register it in `LANGUAGES` in [`src/i18n.ts`](src/i18n.ts); it then appears in Settings.

Versions and releases are automated: every PR updates [CHANGELOG.md](CHANGELOG.md) and [CHANGELOG.it.md](CHANGELOG.it.md); a release is prepared with `npm run version:set -- X.Y.Z`, which updates the version in every file (`npm run version:check` verifies it in CI). When the PR lands on `main`, the [`release.yml`](.github/workflows/release.yml) workflow creates the `vX.Y.Z` tag and builds the installers for every platform into a draft release, with the changelog section as notes. Distributing on macOS/Windows without system warnings requires signing certificates (see the comments in the workflow).

In-app updates need the updater signing key, created once by the owner:

```sh
npm run tauri signer generate -- -w ~/.tauri/termdock.key
```

Without Node.js, [minisign](https://jedisct1.github.io/minisign/) produces compatible keys: run `minisign -G -p ~/.tauri/termdock.key.pub -s ~/.tauri/termdock.key` (it asks for the password; keep the keys outside the repository), then use the **Base64 encoding** of each file where the steps below mention its content (PowerShell: `[Convert]::ToBase64String([IO.File]::ReadAllBytes("$HOME\.tauri\termdock.key"))`; macOS/Linux: `base64 < ~/.tauri/termdock.key | tr -d '\n'`).

The public key (`~/.tauri/termdock.key.pub`) goes into `plugins.updater.pubkey` in `src-tauri/tauri.conf.json`. The private key goes into an offline backup (without it, installed copies can no longer be updated) and into GitHub: *Settings → Environments → New environment* `release`, *Deployment branches and tags* limited to the branch `main` (no tags: a tag can point at an unreviewed commit), then the environment secrets `TAURI_SIGNING_PRIVATE_KEY` (content of `~/.tauri/termdock.key`) and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. Do not store them as repository secrets: those can be read by workflows on any branch.

The release workflow checks the key before tagging, signs every package, verifies them with `scripts/updater-manifest.mjs` and attaches `latest.json`, which installed apps read once the release is published. **Publish a draft only if it has `latest.json`**: without it, installed copies stop finding updates. deb/rpm installs are only notified (the package manager updates them).

## License

[MIT](LICENSE)
