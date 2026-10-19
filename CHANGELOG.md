# Changelog

**English** · [Italiano](CHANGELOG.it.md)

All notable changes to Termdock are documented here.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses [Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.19.0] - 2026-10-19

### Added

- Connection manager: *Duplicate* copies a connection (as "name (copy)") to start a similar one, and the bolt next to a group opens all of its connections, one tab each (asking first above five).
- Tab menu: *Clear terminal* (a shortcut can be assigned in Settings → Keys; none by default, since `Ctrl+L` belongs to the shell), *Close other tabs* and *Close tabs to the right*, with one confirmation for all the SSH connections they close.
- Pasting several lines shows them in a preview before sending, with invisible characters marked as `<U+XXXX>`.
- *Copy on select* (Settings → Appearance, off by default): the selection is copied at once and right click always pastes.

## [0.18.0] - 2026-10-19

### Added

- Tabs can be renamed (double click or tab menu), reordered by dragging and duplicated (tab menu → *Duplicate tab*).
- A hidden tab shows a dot when its terminal prints something, and a bell when the program rings one (for example at the end of a long command).
- Closing a tab, a pane or Termdock with open SSH connections asks first; it can be turned off in Settings → General.
- The window opens again with the size and position it had (only on a monitor that is still there).
- Quick connect remembers the last 10 addresses that connected: `Up` / `Down` in its box, or the suggestions; the history can be cleared in Settings → General.

## [0.17.0] - 2026-10-19

### Added

- SSH keys: *Generate…* next to a connection's key path creates an Ed25519 key with a passphrase, saved in `~/.ssh` or wherever you choose (an existing file is never replaced), shows the public key to copy and fills in the connection. *Install public key…* in the tab menu of a connected terminal adds a public key to `~/.ssh/authorized_keys` on that server, like `ssh-copy-id`.

## [0.16.0] - 2026-10-18

### Added

- Automatic reconnection: when the network or the server drops an SSH connection, the terminal tries again by itself a few times over about five minutes (`Enter`: now, `Esc`: stop), with the usual host-key and password checks. Its session log goes on (with a note) and its file browser reopens in the same folder. It stops at once on anything that needs you (host key, authentication) and can be turned off in Settings → General.

### Fixed

- Session logs no longer get an extra space where a long command line wraps on screen.

### Security

- Downloaded files get the quarantine flag on macOS, so Gatekeeper checks them before they run (as SmartScreen already does on Windows).

## [0.15.0] - 2026-10-18

### Added

- Backup of the configuration: Settings → General → *Export…* saves connections, favorites, layouts, snippets and settings in one JSON file, and *Import…* shows what a backup holds, then adds only what is new or replaces the current configuration. Passwords, keys and host keys are never included.
- Session log: tab menu → *Log to file…* writes what the terminal shows from then on to a text file of your choice, without colours or other escape sequences, until *Stop logging* or the end of the session; a red dot on the tab shows that it is on.

### Changed

- Dates and times in the file browser follow the interface language instead of the system's.

### Security

- Backup files and session logs are created readable by their owner only (Unix); importing a backup checks it as a whole and asks before changing anything, and *Replace* deletes the saved passwords of connections that go away or point elsewhere.

## [0.14.0] - 2026-10-18

### Added

- Snippets: saved commands or blocks of text typed into the active terminal from the new *Snippets* menu (top bar, tab menu, `Ctrl+Shift+P`), optionally followed by Enter; each can be limited to the connections of one group, and the first nine can get `1…9` shortcuts. Managed in the new Settings → Snippets tab.

### Changed

- Linux packages are built in Ubuntu 22.04, so they also run on distributions with glibc 2.35 (Ubuntu 22.04, Debian 12 and newer); CI builds them on every change and fails if a newer glibc is needed.

## [0.13.0] - 2026-10-18

### Added

- Whole folders can be uploaded (Upload → Upload folders…, or dragged onto the file browser) and downloaded (right click → Download), with their subfolders.
- A transfer queue under the files: uploads and downloads wait their turn, each with its progress, a Cancel button and how it ended (copied, kept, left out, failed); the browser stays usable meanwhile.
- When files already exist at the destination, one question for all of them: replace existing or keep existing.

### Security

- Folder transfers never follow links or special files on either side and take at most 10,000 items and 32 levels; remote names are cleaned before becoming local names, local and remote folders are only merged into when they are real folders, and switching user waits for running transfers.

## [0.12.1] - 2026-10-18

### Fixed

- A slow keychain (or one waiting for its unlock dialog), a slow disk or a passphrase-protected key no longer holds up the other sessions while a connection logs in; opening a file for editing, choosing the editor and installing an update no longer block the app either.
- After switching language, open terminals and file browsers show their buttons, tooltips and item count in the new language.
- A tab whose connection waits for a host-key or password answer shows a lock mark until you open it.

## [0.12.0] - 2026-10-18

### Security

- A host recorded with a key of one type no longer gets the "unknown host" question when it shows a key of another type: that counts as a changed key and the connection is refused, and the recorded key types are offered first.
- Links sent by the server in the terminal (OSC 8) are no longer opened.
- Pasted text loses Escape and other control characters, so it cannot end bracketed paste early; pasting several lines into a program without bracketed paste asks first.
- A password or host-key question from a background tab waits until that tab is shown, and does not take the keyboard while you type in another pane.
- Server-opened X11, agent, session, direct-tcpip and Unix-socket channels are refused.
- RSA keys never sign with SHA-1; a server that only accepts SHA-1 signatures is not offered them.
- Changing a connection's host, port, user or authentication now removes its saved password before the change is saved; if that fails, nothing is changed. A failed save no longer leaves the change in memory.
- Uploading over a link only writes the file it points to when both belong to the same user; shells and interpreters can no longer be chosen as editor.
- File listings are limited in name size as well; the 5-minute sudo password wait is enforced.
- Releases are built only from `main` (no tag trigger), checkouts in CI keep no credentials, and the signing-key files are ignored by git.

### Fixed

- Connecting to a port that accepts and stays silent now times out instead of hanging.
- Importing `~/.ssh/config` no longer fails when a host is listed before its ProxyJump bastion; ProxyJump loops are reported.
- Two remote forwards on the same address no longer break each other.
- A password refused over keyboard-interactive is not asked again in a second round (fewer failed attempts on the server).
- `known_hosts` files with non-UTF-8 bytes (e.g. a Latin-1 comment) no longer block every connection.
- Right click pastes again when nothing is selected (it used to select and copy the word under the pointer).
- On macOS, Option types characters (@ # [ ]…) in the terminal again, and *Go to tab 1…9* defaults to Cmd+1…9.
- Enter on a connection in the manager no longer also reaches the terminal behind it.
- Closing a terminal ends its file browser too; Delete, Rename and New folder act on the folder shown when they were started, even with Follow on.
- *Files (SFTP)* is no longer offered for saved local shells; the window title follows the pane in focus; menus give the keyboard focus back.
- An edit whose file browser closed now says that saves are no longer uploaded; a failed save is no longer reported as a conflict.
- Quitting during a transfer removes its temporary files; a shell that exits at once no longer leaves a dead session behind.
- Login prompts show the server's text with invisible characters made visible; "Passphrase" is translated.
- Documentation: local builds use `npm run tauri build -- --no-sign`; storage, algorithms and architecture are described accurately.

## [0.11.0] - 2026-10-17

### Added

- Settings → *Keys*: change, remove or reset every keyboard shortcut (click it and press the new keys), and choose the modifiers of *Go to tab 1…9* and *Open favorite 1…9* or turn them off. Combinations already in use are moved only after confirmation; Ctrl+letter, Alt+letter and Ctrl+Alt (AltGr on Windows) combinations get a warning. Tooltips, menus and the shortcut list follow the chosen keys.

## [0.10.1] - 2026-10-17

### Changed

- Settings is larger and split into tabs (General, Appearance, Files, Updates), with a bigger terminal preview; it reopens on the last tab used.

## [0.10.0] - 2026-10-17

### Added

- Settings → *Editing remote files*: choose the program *Edit* (`F4`) opens files with, picked in the system's file dialog; *Use the system editor* goes back to the default.

### Fixed

- File browser: *Edit* failed on Windows 11 with "cannot find the file specified" where only the Store version of Notepad is installed; it now finds that Notepad too.

## [0.9.1] - 2026-10-17

### Added

- File browser: drag files from the computer onto it to upload them (folders are skipped).
- File browser: *Go to the terminal's folder* and *Follow* (it moves along with `cd` in the terminal), from OSC 7 or the usual `user@host: dir` title.
- File browser: *Edit* (`F4`) opens a remote text file in the system text editor; each save there is uploaded back, asking first if the file changed on the server meanwhile.

### Changed

- Favourites are reordered by dragging with the pointer (the window now takes file drops from the system).
- Uploading over an existing file also keeps its owner and group where the server allows it, and uploading over a link replaces the file it points to instead of the link.

### Fixed

- Releases: the draft release is created once before the builds, so every platform's packages land in the same draft (0.9.0 was not published because of this).

## [0.9.0] - 2026-10-17

Not published: the release build put the macOS packages in a second draft. Its changes ship in 0.9.1.

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

[Unreleased]: https://github.com/LuuckA21/termdock/compare/v0.19.0...HEAD
[0.19.0]: https://github.com/LuuckA21/termdock/compare/v0.18.0...v0.19.0
[0.18.0]: https://github.com/LuuckA21/termdock/compare/v0.17.0...v0.18.0
[0.17.0]: https://github.com/LuuckA21/termdock/compare/v0.16.0...v0.17.0
[0.16.0]: https://github.com/LuuckA21/termdock/compare/v0.15.0...v0.16.0
[0.15.0]: https://github.com/LuuckA21/termdock/compare/v0.14.0...v0.15.0
[0.14.0]: https://github.com/LuuckA21/termdock/compare/v0.13.0...v0.14.0
[0.13.0]: https://github.com/LuuckA21/termdock/compare/v0.12.1...v0.13.0
[0.12.1]: https://github.com/LuuckA21/termdock/compare/v0.12.0...v0.12.1
[0.12.0]: https://github.com/LuuckA21/termdock/compare/v0.11.0...v0.12.0
[0.11.0]: https://github.com/LuuckA21/termdock/compare/v0.10.1...v0.11.0
[0.10.1]: https://github.com/LuuckA21/termdock/compare/v0.10.0...v0.10.1
[0.10.0]: https://github.com/LuuckA21/termdock/compare/v0.9.1...v0.10.0
[0.9.1]: https://github.com/LuuckA21/termdock/compare/v0.8.0...v0.9.1
[0.9.0]: https://github.com/LuuckA21/termdock/compare/v0.8.0...v0.9.0
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
