# Termdock: features

**English** · [Italiano](FEATURES.it.md)

Everything Termdock can do, in order of importance. The details are in the [README](README.md) and the [security model](SECURITY.md).

## 1. The core: connecting to servers

- **SSH with a full terminal**: xterm-256color, truecolor, Unicode, resizing.
- **Connection security**:
  - the host key is verified before logging in, with a SHA-256 fingerprint to confirm;
  - the connection is refused if the key changes;
  - `~/.ssh/known_hosts` is read too, including hashed entries, wildcards and `@revoked`.
- **Authentication**: SSH agent (ssh-agent, Windows OpenSSH agent, Pageant), private keys (encrypted too), password, keyboard-interactive / 2FA.
- **Passwords in the system keychain** (Windows Credential Manager, macOS Keychain, Secret Service), only if you ask; never on disk.
- **Connection manager**: groups, colors, search, star for favorites; *Duplicate* to start from a similar connection; one click opens every connection of a group, one tab each.
- **Favorites bar**: one click to connect, reorder by dragging, `Ctrl+Shift+1…9`.
- **Quick connect** `user@host:port`, with the history of the last 10 addresses (Up/Down and suggestions).
- **Local shells** in a real terminal: PowerShell 7, Windows PowerShell, cmd, WSL, Git Bash; bash, zsh, fish, pwsh on macOS and Linux.

## 2. Working on several servers at once

- **Multiple tabs**:
  - rename with a double click, reorder by dragging, duplicate;
  - close the other tabs or those to the right, with a single confirmation;
  - clear the terminal, from the menu or with a shortcut of your choice;
  - a dot when a hidden tab prints something, a bell when the terminal rings.
- **Split panes** like Windows Terminal: side by side or stacked, already open tabs can be put side by side without reconnecting, `Alt+arrows` to move between them.
- **Saved layouts**: the arrangement and connections of a tab; one can open at startup.
- **Automatic reconnection** when the network or the server drops:
  - tries again for about five minutes; Enter retries now, Esc stops;
  - the session log and the file browser resume when it is back.

## 3. Files and servers

- **SFTP file browser** on the same connection, without a second login:
  - browsing, downloads and uploads, whole folders too;
  - transfer queue with progress and Cancel;
  - drag from the computer to upload;
  - follows the terminal's folder.
- **Files as another user via sudo**: password asked only when needed, never saved.
- **Editing remote files** (`F4`) in a text editor, uploaded automatically on every save, with a check that the file has not changed on the server.
- **Jump hosts (ProxyJump)**, chained too; each hop verifies its own host key.
- **Port forwarding** saved with the connection: local `-L`, remote `-R`, SOCKS5 `-D`.
- **SSH keys**:
  - generate an Ed25519 key with a passphrase, never over an existing file;
  - install the public key on the server, like `ssh-copy-id`.

## 4. Productivity

- **Snippets**: saved commands, sent with or without Enter, can be limited to a group of connections, `1…9` shortcuts.
- **Search in the terminal** (`Ctrl+Shift+F`): highlighting, match case, regular expressions.
- **Customizable shortcuts** in Settings → Keys, with a warning for keys that programs in the terminal use.
- **PuTTY-style copy/paste** (select and right click), with a protected paste: control characters removed and a preview for several lines, invisible characters marked.
- **Copy on select** (optional, off by default): the selected text goes to the clipboard at once.
- **Session log** to a clean text file: off by default, readable only by you.
- **Import from `~/.ssh/config`** with a preview.

## 5. Comfort and configuration

- **Confirmation before closing** tabs, panes or the app with open SSH connections.
- **The window remembers its size and position.**
- **Configuration backup**: export and import connections, favorites, layouts, snippets and settings, never passwords or keys; a preview before importing, then add or replace.
- **Themes and fonts**:
  - dark, light or system theme;
  - 6 color schemes: Termdock Dark/Light, Solarized, Dracula, Nord, Gruvbox;
  - font, cursor and scrollback, with a live preview;
  - zoom with `Ctrl` + `+` / `-` / `0`.
- **English and Italian interface**, automatic or chosen, switching at once.

## 6. Updates and trust

- **Signed in-app updates**: notice at startup and once a day, installed only after you confirm, no tampered packages or older versions.
- **Downloads marked as coming from the Internet**: Mark of the Web on Windows, quarantine on macOS.
- **Technical strengths**:
  - Rust backend and pure-Rust SSH, no external `ssh` processes;
  - modern algorithms, post-quantum key exchange first, no SHA-1 or CBC;
  - Tauri 2 with minimal permissions;
  - tested in CI against a real OpenSSH.
- **Platforms**:
  - Windows: `.exe` and `.msi` installers;
  - macOS: one package for Intel and Apple Silicon;
  - Linux: AppImage, `.deb`, `.rpm` (Ubuntu 22.04, Debian 12 and later).
