# Security

**English** · [Italiano](SECURITY.it.md)

## Reporting a vulnerability

Please report vulnerabilities privately through GitHub's "Report a vulnerability" (Security advisories) on this repository rather than in a public issue.

## Threat model

Termdock holds credentials for remote machines and runs local shells, so the design assumes:

1. **The network is hostile.** Anyone between Termdock and the server may try to impersonate it or downgrade the connection.
2. **The machine's disk may be read** by other users or by backups. Nothing there should be usable to log in elsewhere.
3. **The web view must be treated as the least trusted part** of the app. Server output, banners and profile names are untrusted data shown in it.

## Controls

### SSH transport (`crates/termdock-core/src/ssh.rs`)

| | Offered (in order) | Never offered |
| --- | --- | --- |
| Key exchange | `mlkem768x25519-sha256` (hybrid post-quantum), `curve25519-sha256`, ECDH NIST P-256/384/521, DH group 16/18 (SHA-512), group 14 (SHA-256) | anything SHA-1, group 1 |
| Host keys | Ed25519, ECDSA, RSA with SHA-512/SHA-256 | `ssh-rsa` (SHA-1), DSA, certificates |
| Ciphers | `chacha20-poly1305`, AES-256/128-GCM, AES-CTR | CBC, 3DES, `none` |
| MACs | HMAC-SHA-512/256 ETM, then non-ETM | HMAC-SHA-1, `none` |
| Compression | none | zlib (avoids compression side channels) |

`kex-strict-c-v00@openssh.com` is advertised, which mitigates the Terrapin attack (CVE-2023-48795). A unit test fails if a weak algorithm is ever added to these lists.

### Host authentication (`hostkeys.rs`)

- The server key is checked during key exchange, **before** any credential is sent (covered by an end-to-end test).
- Unknown key: the user is shown the algorithm and SHA-256 fingerprint; the default button is *Cancel*. Only an explicit confirmation records the key.
- Changed key or key marked `@revoked`: the connection is refused. The dialog offers no override; the user must remove the old key explicitly ("Forget host key"), which only affects Termdock's own `known_hosts`. `~/.ssh/known_hosts` is read but never modified.
- Host certificates are not accepted (no CA trust is configured), so a certificate cannot be used to bypass the pinned key.

### Credentials (`vault.rs`, `prompts.rs`)

- Passwords and key passphrases are stored only when the user ticks "Remember", and only in the OS vault (Windows Credential Manager, macOS Keychain, Secret Service). If no vault is available, nothing is stored.
- Stored secrets are never returned to the UI; secrets only travel UI → backend.
- Secret buffers are `Zeroizing` and wiped after use; private key file contents are zeroized too.
- A remembered password is deleted when the profile's host, port, user or authentication method changes.
- Quick-connect sessions (no saved profile) never store anything.
- A remembered password or passphrase that is rejected is deleted at once instead of being replayed on every login (which could trip MaxAuthTries or fail2ban).
- Prompts expire after 3 minutes; an unanswered prompt is treated as "no".

### Desktop shell (Tauri 2)

- Strict Content-Security-Policy: scripts only from the app bundle, no `eval`, no remote origins, no frames/objects/workers.
- Every IPC command must be allowed explicitly in `src-tauri/capabilities/default.json` (generated per-command permissions). No file-system, shell, HTTP or opener plugins are included. The UI may use the clipboard plugin (read/write text); the updater plugin is driven by the backend only and none of its own commands is allowed.
- The UI builds DOM nodes with `textContent` only; profile colours are validated (`#rrggbb`) both in the backend and before use.
- Text that is not session output but is shown in the terminal (server banner, exit signal, error and status messages) is stripped of control characters, so a server cannot inject escape sequences through it before the user has even authenticated.
- Local shells can only be started from the detected list (by id) or from a saved profile.
- `freezePrototype` is disabled because it breaks xterm.js; the UI does not merge untrusted objects, so prototype pollution has no entry point.

### Jump hosts and port forwarding (`ssh.rs`, `forward.rs`)

- Every hop of a ProxyJump chain is a full SSH session: its host key is verified against `known_hosts` and it authenticates with its own credentials. A bastion only sees encrypted traffic for the next hop; nothing is forwarded to it (no agent forwarding).
- Forwards configured on a profile used as bastion are not opened when it is only a hop.
- Local (`-L`) and SOCKS (`-D`) listeners bind to `127.0.0.1` unless the profile says otherwise; the editor warns when a non-loopback address is used. As with OpenSSH, other local users and processes can connect to a loopback listener while the session is open.
- The SOCKS5 proxy implements CONNECT only (no BIND/UDP), without authentication, and validates requested names. Clients must complete the handshake within 10 s.
- Each local listener serves at most 256 connections at a time; accept errors back off instead of spinning.
- Remote (`-R`) connections are only accepted for forwards Termdock requested; any other `forwarded-tcpip` channel the server opens is refused.
- Listeners are closed when the terminal session ends.
- Chains are limited to 8 hops and loops are rejected; a profile used as jump host cannot be deleted while referenced.

### `~/.ssh/config` import (`sshconfig.rs`)

- The backend reads only `~/.ssh/config`; the UI cannot choose another file.
- `ProxyCommand`, `Match`, `Include`, unknown tokens and Unix-socket forwards are reported, never executed or guessed.
- Imported profiles go through the same validation as manual ones.

### Updates (`src-tauri/src/updates.rs`, `scripts/updater-manifest.mjs`)

- The app looks for updates only at the HTTPS endpoint fixed in `tauri.conf.json` (`latest.json` of the latest *published* GitHub release; drafts are never offered). Release builds refuse a plain-HTTP endpoint (the download URL comes from the manifest and is protected by the package signature whatever its scheme; Termdock's manifests only list https GitHub URLs), and the UI cannot change the endpoint, the key or the version policy: it can only ask "is there an update?" and "install the one you found".
- Every package is verified with minisign (Ed25519) against the public key built into the app, after the download and again right before installing. A package that fails the check is never written over the installed app.
- The signature also covers the version it was made for (`requireSignedVersion`): a manifest announcing version X must point to a package signed for X, so a tampered manifest cannot pass off an older, validly signed release as a new one. Only newer versions are accepted (no downgrades).
- The UI downloads and installs only after the user confirms (the default button is *Later*). Even a compromised web view could at most start installing a newer, validly signed release, with a restart. Release notes come from the manifest, which is not signed: they are shown as plain text and truncated.
- deb and rpm installs are not updated in place (it would need root, which the updater plugin obtains by piping the user's password to `sudo`); the app only announces the new version.
- The private key exists only as a secret of the repository's `release` environment, which only workflow runs on `main` and `v*` tags can use, and in the owner's offline backup. The release workflow refuses to tag a release if the key is missing or does not match the public key in the app, and verifies every updater package (Windows installers, macOS app archive, AppImage) before attaching `latest.json`. The actions it runs are pinned to commit SHAs. The key is necessarily in the environment of the build step (the Tauri bundler signs while packaging), so the build tools and dependencies run next to it: dependencies are locked (`Cargo.lock`, `package-lock.json`), audited in CI and updated through reviewed PRs.
- Privacy: a check is one HTTPS request to GitHub at start and once a day (downloads come from GitHub's CDN; GitHub sees the IP address); it can be turned off in Settings.
- Residual risk: whoever holds the private key and can publish releases on this repository can ship an update. If the key is lost, installed copies cannot update themselves any more and must be reinstalled by hand once.

### Storage

- `profiles.json` and `known_hosts` contain no secrets; on Unix they are created `0600` in a `0700` directory and written atomically (temp file + rename) so a crash cannot leave a truncated file.
- Inputs are validated on every save (lengths, control characters, host syntax, ports), and a profile file written by a newer schema is refused rather than silently rewritten.

## Dependencies

- `cargo audit` and `npm audit` run in CI; Dependabot proposes updates weekly.
- Reviewed and accepted advisories are listed with their justification in `.cargo/audit.toml`. Currently: RUSTSEC-2023-0071 (Marvin timing attack in the `rsa` crate). Termdock never decrypts with RSA and signs one authentication challenge per login with the user's own key, so a remote attacker gets no practical timing oracle. Prefer Ed25519 keys, which do not use that code.

## Known limitations

- Release builds are not code-signed until signing certificates are configured in the release workflow; verify downloads accordingly.
- Agent forwarding and X11 forwarding are not implemented (and therefore cannot be abused).
- Typed secrets transit the web view's memory before reaching the backend; they are cleared from the input immediately after submission.
