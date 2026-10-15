# Changelog

All notable changes to Termdock are documented here.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses [Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.1.0] - 2026-10-15

### Added

- Favorites bar: one-click connections, drag & drop ordering, `Ctrl+Shift+1…9`.
- SSH terminal (russh) with agent, private key, password and keyboard-interactive / 2FA authentication.
- Local shells through a real PTY: PowerShell, Windows PowerShell, cmd, WSL, Git Bash, bash, zsh, fish.
- Connection manager with groups, colors, search; quick connect `user@host:port`.
- Host key verification against Termdock's and OpenSSH's `known_hosts` (trust on first use, changed or revoked keys refused).
- Passwords and passphrases stored only in the OS keychain, on request.
- Installers for Windows (MSI, NSIS), macOS (universal DMG) and Linux (AppImage, deb, rpm).

[Unreleased]: https://github.com/LuuckA21/termdock/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/LuuckA21/termdock/releases/tag/v0.1.0
