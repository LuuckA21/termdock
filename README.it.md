# Termdock

[English](README.md) · **Italiano**

Terminale e gestore di connessioni multipiattaforma (Windows, macOS, Linux), nello spirito di PuTTY / Windows Terminal, con una **barra dei preferiti** per aprire le connessioni che usi più spesso con un clic.

![Termdock](docs/screenshot.png)

## Funzionalità

- **Barra preferiti** sempre visibile: un clic apre la connessione in una nuova scheda, trascina per riordinare, tasto destro per modificare o togliere dai preferiti, `Ctrl+Shift+1…9` apre i primi nove.
- **SSH** con terminale completo (xterm-256color, truecolor, Unicode, ridimensionamento).
- **Jump host (ProxyJump)**: una connessione può passare da un bastion, anche a catena; ogni salto verifica la propria host key e si autentica separatamente.
- **Port forwarding** salvato nella connessione: locale (`-L`), remoto (`-R`) e dinamico SOCKS5 (`-D`); lo stato di ogni tunnel compare nel terminale.
- **Import da `~/.ssh/config`**: anteprima con avvisi, poi importa Host, HostName, User, Port, IdentityFile, ProxyJump e i forwarding.
- **Shell locali** in PTY reale: PowerShell 7, Windows PowerShell, cmd, WSL e Git Bash su Windows (ConPTY); bash, zsh, fish, pwsh… su macOS e Linux.
- **Gestore connessioni**: gruppi, colori, ricerca, stella per i preferiti.
- **Connessione rapida**: scrivi `utente@host:porta` nella barra e premi Invio.
- Schede multiple, riconnessione con Invio a sessione terminata, copia/incolla stile PuTTY (selezione + tasto destro).
- Interfaccia in italiano o inglese in base alla lingua del sistema.

### Scorciatoie

| Tasti | Azione |
| --- | --- |
| `Ctrl+Shift+T` | nuova shell locale (predefinita) |
| `Ctrl+Shift+W` | chiudi scheda |
| `Ctrl+Tab` / `Ctrl+Shift+Tab` | scheda successiva / precedente |
| `Alt+1…9` | vai alla scheda N |
| `Ctrl+Shift+1…9` | apri il preferito N |
| `Ctrl+Shift+K` | connessione rapida |
| `Ctrl+Shift+M` | gestore connessioni |
| `Ctrl+Shift+C` / `Ctrl+Shift+V` | copia / incolla |
| tasto destro nel terminale | copia la selezione, altrimenti incolla |

## Sicurezza

Le scelte principali (dettagli in [SECURITY.it.md](SECURITY.it.md)):

- **Backend in Rust** (memory-safe), SSH implementato in Rust puro con [russh](https://github.com/Eugeny/russh): nessun processo `ssh` esterno, nessuna shell intermedia.
- **Solo algoritmi moderni**: scambio chiavi ibrido post-quantistico `mlkem768x25519-sha256` come prima scelta, poi `curve25519-sha256`; cifrari AEAD (`chacha20-poly1305`, `aes-gcm`); MAC SHA-2 encrypt-then-MAC; *strict KEX* (mitigazione Terrapin). Esclusi SHA-1, CBC, DSA, `ssh-rsa` e compressione.
- **Verifica host key** prima di qualsiasi autenticazione: al primo collegamento viene mostrata l'impronta SHA-256 da confermare (predefinito: *Annulla*); se la chiave cambia la connessione viene **rifiutata** senza possibilità di forzarla dal dialogo. Supporta il formato OpenSSH, incluse voci hashate, wildcard e `@revoked`, e legge anche `~/.ssh/known_hosts`.
- **Nessun segreto su disco**: password e passphrase sono salvate solo se lo chiedi, e solo nel portachiavi del sistema operativo (Windows Credential Manager, macOS Keychain, Secret Service su Linux). Vengono azzerate in memoria dopo l'uso (`zeroize`), non tornano mai all'interfaccia e vengono eliminate se il server le rifiuta.
- Se cambi host, porta, utente o metodo di autenticazione di una connessione, la password salvata viene eliminata, così non viene mai inviata a un server diverso.
- Autenticazione: SSH agent (ssh-agent, agent OpenSSH di Windows, Pageant), chiavi private (anche cifrate), password, keyboard-interactive / 2FA.
- Il testo che arriva dal server fuori dalla sessione (banner, messaggi di uscita) viene mostrato senza caratteri di controllo: nessuna iniezione di sequenze di escape.
- **Tauri 2 con privilegi minimi**: Content-Security-Policy restrittiva (niente script inline o remoti), ogni comando IPC autorizzato esplicitamente in [`capabilities/default.json`](src-tauri/capabilities/default.json), nessun plugin per file system o shell. L'interfaccia costruisce il DOM solo con `textContent` (niente `innerHTML`).
- File di configurazione con permessi `0600` (cartella `0700`) su Unix, scritture atomiche.
- CI con `cargo audit`, `npm audit`, clippy con `-D warnings`, test end-to-end contro OpenSSH e Dependabot settimanale.

> Consiglio: usa chiavi **Ed25519** (`ssh-keygen -t ed25519`) protette da passphrase, meglio se caricate in un SSH agent.

## Architettura

```
crates/termdock-core   logica senza interfaccia, testata in isolamento
  ├─ profile.rs         modello connessioni, validazione, parser "utente@host:porta", catene di jump
  ├─ store.rs           profiles.json (atomico, 0600) e ordine dei preferiti
  ├─ vault.rs           portachiavi di sistema (keyring)
  ├─ hostkeys.rs        known_hosts: TOFU, chiavi cambiate/revocate
  ├─ ssh.rs             client SSH (russh): algoritmi, autenticazione, jump host
  ├─ forward.rs         port forwarding -L / -R / -D (SOCKS5)
  ├─ sshconfig.rs       import da ~/.ssh/config
  ├─ local.rs           shell locali via PTY (portable-pty / ConPTY)
  └─ shells.rs          rilevamento delle shell disponibili
src-tauri/             app desktop Tauri 2: comandi IPC, prompt, stato
src/                   interfaccia TypeScript + xterm.js (nessun framework)
```

I dati del terminale viaggiano su canali IPC binari di Tauri; le domande per l'utente (host key, password, OTP) passano da un canale separato e scadono dopo 3 minuti.

I dati sono salvati in:

| Sistema | Percorso |
| --- | --- |
| Windows | `%APPDATA%\dev.termdock.app\` |
| macOS | `~/Library/Application Support/dev.termdock.app/` |
| Linux | `~/.config/dev.termdock.app/` |

(`profiles.json` e `known_hosts`; le password stanno solo nel portachiavi.)

## Sviluppo

Requisiti: [Rust](https://rustup.rs) stabile (≥ 1.90), Node.js ≥ 22 e i [prerequisiti di Tauri](https://tauri.app/start/prerequisites/) per il tuo sistema (su Linux: `libwebkit2gtk-4.1-dev`, `libgtk-3-dev`, `librsvg2-dev`, `libdbus-1-dev`).

```sh
npm ci
npm run tauri dev        # avvia l'app in modalità sviluppo
npm run tauri build      # crea gli installer (MSI/NSIS, DMG, AppImage/deb/rpm)
```

Test e controlli:

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
npm run typecheck
npm run version:check

# test end-to-end contro un vero OpenSSH (Linux, richiede root e openssh-server)
eval "$(sudo scripts/e2e-sshd.sh)"
cargo test -p termdock-core --test ssh_e2e
```

I test end-to-end coprono: rifiuto della host key sconosciuta, trust-on-first-use, password errata e ritentativi, password ricordata nel portachiavi ed eliminata se rifiutata, chiave privata cifrata con passphrase, keyboard-interactive, jump host con forwarding `-L` / `-R` / `-D`, chiusura sessione e **host key cambiata (simulazione MITM)**.

Codice, commenti e messaggi di commit sono in inglese; la documentazione è mantenuta in inglese e in italiano (`*.md` e `*.it.md`); i testi dell'interfaccia stanno in [`src/i18n.ts`](src/i18n.ts) in entrambe le lingue.

Versioni e release sono automatiche: ogni PR aggiorna [CHANGELOG.md](CHANGELOG.md) e [CHANGELOG.it.md](CHANGELOG.it.md); per rilasciare si usa `npm run version:set -- X.Y.Z`, che aggiorna la versione in tutti i file (`npm run version:check` lo verifica in CI). Quando la PR arriva su `main`, il workflow [`release.yml`](.github/workflows/release.yml) crea il tag `vX.Y.Z` e compila gli installer per tutte le piattaforme in una release in bozza, con le note prese dal changelog. Per distribuire su macOS/Windows senza avvisi del sistema servono certificati di firma (vedi i commenti nel workflow).

## Licenza

[MIT](LICENSE)
