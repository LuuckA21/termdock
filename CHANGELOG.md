# Changelog

**English** · [Italiano](CHANGELOG.it.md)

All notable changes to Termdock are documented here.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses [Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.8.0] - 2026-10-17

### Added

- Find in terminal (`Ctrl+Shift+F`, or the tab menu): highlighted matches in the scrollback, next/previous, match case, regular expressions and a result count.
- Saved layouts: save a tab's panes (connections, shells, split directions and sizes) with `Ctrl+Shift+L` or the tab menu, reopen them from the new *Layouts* menu, and choose one to open at startup. Connections deleted since then open as a local terminal, with a notice.

## [0.7.0] - 2026-10-17

### Added

- Split panes like Windows Terminal: splitting asks what opens in the new pane (the same connection, a local shell, any saved connection or a `user@host` address), so server A and server B can sit side by side. Enter keeps the old behaviour (same connection).
- Tab menu → *Move next to*: puts an open tab beside another one, keeping its sessions.
- Favourites menu: *Open on the right* / *Open below*.
- File browser as another user: after `sudo -iu user` in the terminal it offers to open the files as that user (through sudo on the server; the sudo password is asked only when needed and never saved). Any user can also be chosen from the browser's user menu; back to the login user in one click.

### Changed

- The file browser's path moves to its own row in narrow panes.

## [0.6.0] - 2026-10-17

### Added

- File browser (SFTP): `Ctrl+Shift+S` (or the tab's right-click menu) opens the server's files next to its SSH terminal, on the same connection without logging in again. Browse folders, download, upload (asking before replacing a file), create folders, rename and delete, with progress and cancel.

### Changed

- Confirmations of destructive actions (delete a connection, a remote file) now default to *Cancel*.

### Security

- File transfers treat the server as untrusted: local files are only chosen in native dialogs opened by the backend; remote names are shown without invisible or reordering characters and never become local paths; listings are bounded; only regular files are downloaded; uploads use exclusive temporary files and replace the target atomically, keeping its permissions; downloads on Windows carry the Mark of the Web.

## [0.5.1] - 2026-10-17

### Fixed

- The active tab was unreadable when the terminal colours and the app theme differ in brightness (for example the light theme with Dracula); it now uses the terminal's own text colour.
- Clearing a number in Settings (font size, line height, scrollback) and saving set it to the minimum, for example scrollback 0; the current value is now kept.
- Shortcuts no longer act on the tabs behind an open dialog, and holding one down no longer opens or closes a burst of panes.
- `Alt+arrows` move between panes only from a terminal, so text fields keep word movement.
- Dragging a divider reacts only to the left mouse button; the tab's × tooltip no longer suggests `Ctrl+Shift+W`, which now closes a pane.

## [0.5.0] - 2026-10-17

### Added

- Split panes: `Ctrl+Shift+E` splits the active pane to the right, `Ctrl+Shift+O` downwards (also from the tab's right-click menu). The new pane opens the same connection or shell; dividers can be dragged and `Alt+arrows` move between panes.
- Appearance settings: dark, light or system theme, terminal colour schemes (Termdock Dark/Light, Solarized Dark/Light, Dracula, Nord, Gruvbox Dark), font, size, line height, cursor style and blinking, scrollback, with a live preview.
- `Ctrl` + `+` / `-` / `0` makes the terminal font larger, smaller or resets it.

### Changed

- `Ctrl+Shift+W` closes the active pane; a tab closes with its last pane (the × on the tab still closes the whole tab).

## [0.4.1] - 2026-10-17

### Changed

- The update dialog shows the release notes as headings and bullet lists instead of raw text.

## [0.4.0] - 2026-10-16

### Added

- Automatic updates: Termdock looks for a new version at start and once a day and, after you confirm, downloads it, verifies it and restarts. Settings has a switch to turn the check off, a "Check now" button and the current version. Copies installed from deb/rpm packages are only told that a new version exists. Earlier versions have no updater: install 0.4.0 by hand once.

### Security

- Updates are accepted only if signed with the key built into the app and signed for the version they announce, so neither a tampered package nor an older release can be installed through the updater.

## [0.3.0] - 2026-10-16

### Added

- Language selection in the new Settings dialog (gear button or `Ctrl+,`): Automatic (system language), English or Italiano. The change applies at once, without closing open sessions, and is remembered.
- Translations are organised one file per language (`src/locales/`), so new languages can be added easily.
- Documentation in English and Italian.

## [0.2.0] - 2026-10-15

### Added

- Jump hosts (ProxyJump): connect through one or more saved SSH connections; each hop verifies its own host key and authenticates separately.
- Port forwarding saved in the connection: local (`-L`), remote (`-R`) and dynamic SOCKS5 (`-D`), with their status shown in the terminal.
- Import connections from `~/.ssh/config` with a preview: Host, HostName, User, Port, IdentityFile, ProxyJump, LocalForward, RemoteForward, DynamicForward, ServerAliveInterval.

### Changed

- A connection used as jump host cannot be deleted, or turned into a local shell, while other connections go through it; missing or looping jump hosts are rejected when saving.

### Fixed

- Raw input from xterm.js (some mouse reports) was UTF-8 encoded instead of sent as bytes.
- Closing a tab while a host key or password dialog was open left the dialog on screen.

### Security

- Server banners, exit signals and error messages are stripped of control characters before being shown in the terminal (no escape sequence injection).
- A saved password or passphrase that is rejected is deleted instead of being retried on every login.
- SOCKS5 handshake timeout, per-listener connection limit and accept-error backoff for port forwarding.

## [0.1.0] - 2026-10-15

### Added

- Favorites bar: one-click connections, drag & drop ordering, `Ctrl+Shift+1…9`.
- SSH terminal (russh) with agent, private key, password and keyboard-interactive / 2FA authentication.
- Local shells through a real PTY: PowerShell, Windows PowerShell, cmd, WSL, Git Bash, bash, zsh, fish.
- Connection manager with groups, colors, search; quick connect `user@host:port`.
- Host key verification against Termdock's and OpenSSH's `known_hosts` (trust on first use, changed or revoked keys refused).
- Passwords and passphrases stored only in the OS keychain, on request.
- Installers for Windows (MSI, NSIS), macOS (universal DMG) and Linux (AppImage, deb, rpm).

[Unreleased]: https://github.com/LuuckA21/termdock/compare/v0.8.0...HEAD
[0.8.0]: https://github.com/LuuckA21/termdock/compare/v0.7.0...v0.8.0
[0.7.0]: https://github.com/LuuckA21/termdock/compare/v0.6.0...v0.7.0
[0.6.0]: https://github.com/LuuckA21/termdock/compare/v0.5.1...v0.6.0
[0.5.1]: https://github.com/LuuckA21/termdock/compare/v0.5.0...v0.5.1
[0.5.0]: https://github.com/LuuckA21/termdock/compare/v0.4.1...v0.5.0
[0.4.1]: https://github.com/LuuckA21/termdock/compare/v0.4.0...v0.4.1
[0.4.0]: https://github.com/LuuckA21/termdock/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/LuuckA21/termdock/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/LuuckA21/termdock/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/LuuckA21/termdock/releases/tag/v0.1.0
