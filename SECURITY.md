# Security

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
- Prompts expire after 3 minutes; an unanswered prompt is treated as "no".

### Desktop shell (Tauri 2)

- Strict Content-Security-Policy: scripts only from the app bundle, no `eval`, no remote origins, no frames/objects/workers.
- Every IPC command must be allowed explicitly in `src-tauri/capabilities/default.json` (generated per-command permissions). No file-system, shell, HTTP or opener plugins are included; the only plugin is the clipboard (read/write text).
- The UI builds DOM nodes with `textContent` only; profile colours are validated (`#rrggbb`) both in the backend and before use.
- Local shells can only be started from the detected list (by id) or from a saved profile.
- `freezePrototype` is disabled because it breaks xterm.js; the UI does not merge untrusted objects, so prototype pollution has no entry point.

### Jump hosts and port forwarding (`ssh.rs`, `forward.rs`)

- Every hop of a ProxyJump chain is a full SSH session: its host key is verified against `known_hosts` and it authenticates with its own credentials. A bastion only sees encrypted traffic for the next hop; nothing is forwarded to it (no agent forwarding).
- Forwards configured on a profile used as bastion are not opened when it is only a hop.
- Local (`-L`) and SOCKS (`-D`) listeners bind to `127.0.0.1` unless the profile says otherwise; the editor warns when a non-loopback address is used. As with OpenSSH, other local users and processes can connect to a loopback listener while the session is open.
- The SOCKS5 proxy implements CONNECT only (no BIND/UDP), without authentication, and validates requested names.
- Remote (`-R`) connections are only accepted for forwards Termdock requested; any other `forwarded-tcpip` channel the server opens is refused.
- Listeners are closed when the terminal session ends.
- Chains are limited to 8 hops and loops are rejected; a profile used as jump host cannot be deleted while referenced.

### `~/.ssh/config` import (`sshconfig.rs`)

- The backend reads only `~/.ssh/config`; the UI cannot choose another file.
- `ProxyCommand`, `Match`, `Include`, unknown tokens and Unix-socket forwards are reported, never executed or guessed.
- Imported profiles go through the same validation as manual ones.

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
