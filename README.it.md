# Termdock

[English](README.md) · **Italiano**

Terminale e gestore di connessioni multipiattaforma (Windows, macOS, Linux), nello spirito di PuTTY / Windows Terminal, con una **barra dei preferiti** per aprire le connessioni che usi più spesso con un clic.

![Termdock](docs/screenshot.png)

## Funzionalità

- **Barra preferiti** sempre visibile: un clic apre la connessione in una nuova scheda, trascina per riordinare, tasto destro per modificare o togliere dai preferiti, `Ctrl+Shift+1…9` apre i primi nove.
- **SSH** con terminale completo (xterm-256color, truecolor, Unicode, ridimensionamento).
- **Jump host (ProxyJump)**: una connessione può passare da un bastion, anche a catena; ogni salto verifica la propria host key e si autentica separatamente.
- **File browser (SFTP)**: `Ctrl+Shift+S` apre i file del server accanto al suo terminale, sulla stessa connessione già autenticata (nessun secondo login): navigazione, download, upload, nuova cartella, rinomina, eliminazione. Si possono caricare e scaricare anche cartelle intere; i trasferimenti aspettano in una coda sotto i file, ognuno con il suo avanzamento e un pulsante Annulla, e i file già presenti si sostituiscono o si mantengono con una sola scelta per tutti. Dopo `sudo -iu utente` nel terminale propone di aprire i file anche come quell'utente (tramite sudo sul server, con la password di sudo chiesta solo se serve e mai salvata), oppure scegli qualsiasi utente dal suo menu utente. Si possono trascinare file e cartelle dal computer sul file browser per caricarli; il file browser può andare alla cartella del terminale (o seguirla); e *Modifica* (`F4`) apre un file di testo remoto nell'editor di testo di sistema (o in quello scelto nelle Impostazioni) e lo ricarica sul server a ogni salvataggio, chiedendo prima se nel frattempo è cambiato sul server.
- **Port forwarding** salvato nella connessione: locale (`-L`), remoto (`-R`) e dinamico SOCKS5 (`-D`); lo stato di ogni tunnel compare nel terminale.
- **Import da `~/.ssh/config`**: anteprima con avvisi, poi importa Host, HostName, User, Port, IdentityFile, ProxyJump e i forwarding.
- **Shell locali** in PTY reale: PowerShell 7, Windows PowerShell, cmd, WSL e Git Bash su Windows (ConPTY); bash, zsh, fish, pwsh… su macOS e Linux.
- **Gestore connessioni**: gruppi, colori, ricerca, stella per i preferiti.
- **Connessione rapida**: scrivi `utente@host:porta` nella barra e premi Invio.
- Schede multiple, riconnessione con Invio a sessione terminata, copia/incolla stile PuTTY (selezione + tasto destro).
- **Riconnessione automatica**: quando la rete o il server fanno cadere una connessione SSH (rilevato tramite il keepalive impostato nella connessione), il terminale riprova da solo alcune volte nell'arco di circa cinque minuti, con i soliti controlli di host key e password; `Invio` riprova subito, `Esc` smette. Registrazione della sessione e browser dei file riprendono quando torna. Si può disattivare in Impostazioni → Generale.
- **Pannelli divisi**, come Windows Terminal: dividi una scheda affiancando o sovrapponendo i pannelli quante volte serve e scegli cosa aprire nel nuovo pannello (la stessa connessione, una shell locale, qualsiasi connessione salvata o un indirizzo `utente@host`), per esempio server A a sinistra e server B a destra. Le schede già aperte si possono affiancare senza riconnettersi (menu della scheda → *Affianca a*) e i preferiti si aprono direttamente a destra o sotto. Trascina i divisori per ridimensionarli, spostati tra i pannelli con `Alt+frecce`.
- **Layout salvati**: salva la disposizione di una scheda (quale connessione o shell c'è in ogni pannello e le dimensioni) con `Ctrl+Shift+L` e riaprila dal menu *Layout* con un clic; un layout può aprirsi automaticamente all'avvio (le sue connessioni partono subito, con le solite richieste di host key e password).
- **Snippet**: testi salvati (un comando, un blocco di righe) scritti nel terminale attivo dal menu *Snippet* o con `Ctrl+Shift+P`, volendo con Invio così partono subito; ognuno può essere proposto in ogni terminale o solo per le connessioni di un gruppo, e i primi nove possono avere scorciatoie `1…9` in Impostazioni → Tasti. Si gestiscono in Impostazioni → Snippet; entrano come un incolla (caratteri di controllo rimossi, più righe confermate). Sono testo in chiaro: non metterci mai password.
- **Backup della configurazione**: Impostazioni → Generale → *Esporta…* salva connessioni, preferiti, layout, snippet e impostazioni (aspetto, lingua, tasti) in un unico file JSON; *Importa…* mostra cosa contiene un backup e poi aggiunge solo ciò che è nuovo oppure sostituisce la configurazione attuale, per esempio per passare a un altro computer. Password, chiavi e host key non vengono mai inclusi.
- **Registrazione della sessione**: menu della scheda → *Registra su file…* scrive ciò che un terminale mostra da quel momento in un file di testo a scelta (colori e altre sequenze di escape rimossi), fino a *Ferma registrazione* o alla fine della sessione; un punto rosso sulla scheda indica che è attiva. È spenta finché non la si avvia, perché un log conserva tutto ciò che compare a schermo.
- **Chiavi SSH**: *Genera…* accanto al percorso della chiave di una connessione crea una chiave Ed25519 con passphrase (salvata in `~/.ssh` o dove scegli, mai sopra un file esistente) e mostra la chiave pubblica da copiare; *Installa chiave pubblica…* nel menu della scheda di un terminale connesso aggiunge un `.pub` a `~/.ssh/authorized_keys` su quel server, come `ssh-copy-id`.
- **Ricerca nel terminale**: `Ctrl+Shift+F` cerca nello storico con i risultati evidenziati, successivo/precedente (`Invio` / `Maiusc+Invio`), maiuscole/minuscole ed espressioni regolari.
- **Temi e caratteri**: tema scuro, chiaro o di sistema; schemi di colori del terminale (Termdock Dark/Light, Solarized, Dracula, Nord, Gruvbox); carattere, dimensione, interlinea, stile del cursore e scrollback, con anteprima dal vivo nelle Impostazioni; `Ctrl` + `+` / `-` / `0` cambia la dimensione del carattere.
- **Aggiornamenti automatici**: una nuova versione viene segnalata all'avvio (e una volta al giorno), installata solo dopo la tua conferma e verificata con la chiave di firma inclusa nell'app; si può disattivare nelle Impostazioni.
- **Interfaccia multilingua** (English, Italiano): si sceglie nelle Impostazioni (`Ctrl+,`) oppure si lascia *Automatica* per seguire la lingua del sistema; il cambio è immediato e le sessioni aperte restano attive.

### Scorciatoie

| Tasti | Azione |
| --- | --- |
| `Ctrl+Shift+T` | nuova shell locale (predefinita) |
| `Ctrl+Shift+E` / `Ctrl+Shift+O` | dividi il pannello a destra / in basso (poi scegli cosa aprire; Invio = stessa connessione) |
| `Ctrl+Shift+S` | file browser (SFTP) del terminale SSH |
| `Ctrl+Shift+F` | cerca nel terminale |
| `Ctrl+Shift+P` | menu degli snippet |
| `Ctrl+Shift+L` | salva la scheda come layout |
| `Alt+←↑→↓` | passa al pannello vicino (con più pannelli) |
| `Ctrl+Shift+W` | chiudi il pannello (la scheda con l'ultimo pannello) |
| `Ctrl+Tab` / `Ctrl+Shift+Tab` | scheda successiva / precedente |
| `Alt+1…9` (macOS: `Cmd+1…9`) | vai alla scheda N |
| `Ctrl+Shift+1…9` | apri il preferito N |
| `Ctrl+Shift+K` | connessione rapida |
| `Ctrl+Shift+M` | gestore connessioni |
| `Ctrl+Shift+C` / `Ctrl+Shift+V` | copia / incolla |
| `Ctrl` + `+` / `-` / `0` | carattere più grande / più piccolo / predefinito |
| `Ctrl+,` | impostazioni (lingua, aspetto, tasti, snippet, file, aggiornamenti) |
| tasto destro nel terminale | copia la selezione, altrimenti incolla |

Questi sono i tasti predefiniti (su macOS `Cmd` funziona al posto di `Ctrl`). Ogni scorciatoia si può cambiare o disattivare in **Impostazioni → Tasti**: fai clic e premi i nuovi tasti; una combinazione già usata viene spostata solo dopo conferma, e per i tasti che servono ai programmi nel terminale o alla digitazione con AltGr compare prima un avviso.

## Sicurezza

Le scelte principali (dettagli in [SECURITY.it.md](SECURITY.it.md)):

- **Backend in Rust** (memory-safe), SSH implementato in Rust puro con [russh](https://github.com/Eugeny/russh): nessun processo `ssh` esterno, nessuna shell intermedia.
- **Algoritmi moderni**: scambio chiavi ibrido post-quantistico `mlkem768x25519-sha256` come prima scelta, poi `curve25519-sha256` (ECDH NIST e gruppi DH grandi solo come ripiego); cifrari AEAD preferiti (`chacha20-poly1305`, `aes-gcm`), poi AES-CTR; MAC SHA-2, prima encrypt-then-MAC; *strict KEX* (mitigazione Terrapin). Esclusi SHA-1, CBC, DSA, `ssh-rsa` e compressione.
- **Verifica host key** prima di qualsiasi autenticazione: al primo collegamento viene mostrata l'impronta SHA-256 da confermare (predefinito: *Annulla*); se la chiave cambia la connessione viene **rifiutata** senza possibilità di forzarla dal dialogo. Supporta il formato OpenSSH, incluse voci hashate, wildcard e `@revoked`, e legge anche `~/.ssh/known_hosts`.
- **Nessun segreto su disco**: password e passphrase sono salvate solo se lo chiedi, e solo nel portachiavi del sistema operativo (Windows Credential Manager, macOS Keychain, Secret Service su Linux). Vengono azzerate in memoria dopo l'uso (`zeroize`), non tornano mai all'interfaccia e vengono eliminate se il server le rifiuta.
- Se cambi host, porta, utente o metodo di autenticazione di una connessione, la password salvata viene eliminata, così non viene mai inviata a un server diverso.
- Autenticazione: SSH agent (ssh-agent, agent OpenSSH di Windows, Pageant), chiavi private (anche cifrate), password, keyboard-interactive / 2FA.
- Il testo che arriva dal server fuori dalla sessione (banner, messaggi di uscita) viene mostrato senza caratteri di controllo: nessuna iniezione di sequenze di escape.
- **Tauri 2 con privilegi minimi**: Content-Security-Policy restrittiva (niente script inline o remoti), ogni comando IPC autorizzato esplicitamente in [`capabilities/default.json`](src-tauri/capabilities/default.json), nessun plugin per file system o shell. L'interfaccia costruisce il DOM solo con `textContent` (niente `innerHTML`).
- File di configurazione con permessi `0600` (cartella `0700`) su Unix, scritture atomiche.
- **Aggiornamenti firmati**: ogni pacchetto di aggiornamento viene verificato (Ed25519/minisign) con la chiave pubblica inclusa nell'app e deve essere firmato per la versione che dichiara, quindi non si possono installare né un pacchetto manomesso né una release più vecchia. Dettagli in [SECURITY.it.md](SECURITY.it.md#aggiornamenti-src-taurisrcupdatesrs-scriptsupdater-manifestmjs).
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
  ├─ session.rs         eventi e output delle sessioni, comuni a SSH e shell locali
  ├─ forward.rs         port forwarding -L / -R / -D (SOCKS5)
  ├─ sftp.rs            SFTP sulla connessione del terminale (file browser)
  ├─ sshconfig.rs       import da ~/.ssh/config
  ├─ local.rs           shell locali via PTY (portable-pty / ConPTY)
  ├─ shells.rs          rilevamento delle shell disponibili
  └─ error.rs           tipo di errore comune al crate
src-tauri/             app desktop Tauri 2
  ├─ commands.rs        comandi IPC per profili e sessioni
  ├─ prompts.rs         domande all'utente (host key, password, OTP)
  ├─ sftp.rs            comandi del file browser, finestre native, file trascinati
  ├─ transfer.rs        upload e download di file e cartelle: piano, poi esecuzione
  ├─ edit.rs            Modifica: copia privata, sorveglianza e caricamento al salvataggio
  ├─ editor.rs          quale editor di testo la apre (di sistema o scelto nelle Impostazioni)
  └─ updates.rs         aggiornamenti firmati dall'app
src/                   interfaccia TypeScript + xterm.js (nessun framework)
```

I dati del terminale viaggiano su canali IPC binari di Tauri; le domande per l'utente (host key, password, OTP) passano da un canale separato e scadono dopo 3 minuti.

I dati sono salvati in:

| Sistema | Percorso |
| --- | --- |
| Windows | `%APPDATA%\dev.termdock.app\` |
| macOS | `~/Library/Application Support/dev.termdock.app/` |
| Linux | `~/.config/dev.termdock.app/` |

(`profiles.json`, `known_hosts` e `editor.json`; le password stanno solo nel portachiavi. Le preferenze dell'interfaccia, come lingua, aspetto e scorciatoie da tastiera, i layout salvati e gli snippet sono nel local storage della web view. Backup e log di sessione vengono scritti solo dove scegli tu.)

## Sviluppo

Requisiti: [Rust](https://rustup.rs) stabile (≥ 1.90), Node.js ≥ 22 e i [prerequisiti di Tauri](https://tauri.app/start/prerequisites/) per il tuo sistema (su Linux: `libwebkit2gtk-4.1-dev`, `libgtk-3-dev`, `librsvg2-dev`, `libdbus-1-dev`). I pacchetti Linux rilasciati sono compilati in Ubuntu 22.04 e richiedono glibc 2.35 o successiva (Ubuntu 22.04, Debian 12, Fedora 36 e successive).

```sh
npm ci
npm run tauri dev        # avvia l'app in modalità sviluppo
npm run tauri build -- --no-sign   # crea gli installer (MSI/NSIS, DMG, AppImage/deb/rpm) senza la chiave degli aggiornamenti
```

Test e controlli:

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
npm run typecheck
npm run version:check

# test end-to-end contro un vero OpenSSH (Linux, richiede root e openssh-server;
# crea utenti e regole sudo usa e getta, quindi usa una VM o il runner della CI)
eval "$(sudo scripts/e2e-sshd.sh)"
cargo test -p termdock-core --test ssh_e2e
```

I test end-to-end coprono: rifiuto della host key sconosciuta, trust-on-first-use, password errata e ritentativi, password ricordata nel portachiavi ed eliminata se rifiutata, chiave privata cifrata con passphrase, keyboard-interactive, jump host con forwarding `-L` / `-R` / `-D`, SFTP (upload, sovrascrittura, download, annullamento, rinomina, eliminazione, nomi rifiutati), SFTP come altro utente tramite sudo (senza password, password solo dopo il prompt, errata e poi giusta, tre tentativi, utente non consentito, nomi non sicuri), chiusura sessione e **host key cambiata (simulazione MITM)**.

Le sessioni cloud di Claude Code vengono preparate da `.claude/hooks/session-start.sh`, e le skill del progetto in `.claude/skills/` descrivono la procedura di PR/release e come avviare l'app senza interfaccia grafica.

Codice, commenti e messaggi di commit sono in inglese; la documentazione è mantenuta in inglese e in italiano (`*.md` e `*.it.md`).

I testi dell'interfaccia stanno in [`src/locales/`](src/locales), un file per lingua. Per aggiungere una lingua copia `en.ts` in `<codice>.ts`, traducilo (il tipo `Dict` fa fallire la build se manca una chiave) e registralo in `LANGUAGES` in [`src/i18n.ts`](src/i18n.ts); comparirà nelle Impostazioni.

Versioni e release sono automatiche: ogni PR aggiorna [CHANGELOG.md](CHANGELOG.md) e [CHANGELOG.it.md](CHANGELOG.it.md); per rilasciare si usa `npm run version:set -- X.Y.Z`, che aggiorna la versione in tutti i file (`npm run version:check` lo verifica in CI). Quando la PR arriva su `main`, il workflow [`release.yml`](.github/workflows/release.yml) crea il tag `vX.Y.Z` e compila gli installer per tutte le piattaforme in una release in bozza, con le note prese dal changelog. Per distribuire su macOS/Windows senza avvisi del sistema servono certificati di firma (vedi i commenti nel workflow).

Gli aggiornamenti dall'app richiedono la chiave di firma degli aggiornamenti, creata una volta dal proprietario:

```sh
npm run tauri signer generate -- -w ~/.tauri/termdock.key
```

Senza Node.js, [minisign](https://jedisct1.github.io/minisign/) genera chiavi compatibili: esegui `minisign -G -p ~/.tauri/termdock.key.pub -s ~/.tauri/termdock.key` (chiede la password; tieni le chiavi fuori dal repository), poi usa la **codifica Base64** di ciascun file dove i passi seguenti parlano del suo contenuto (PowerShell: `[Convert]::ToBase64String([IO.File]::ReadAllBytes("$HOME\.tauri\termdock.key"))`; macOS/Linux: `base64 < ~/.tauri/termdock.key | tr -d '\n'`).

La chiave pubblica (`~/.tauri/termdock.key.pub`) va in `plugins.updater.pubkey` in `src-tauri/tauri.conf.json`. La chiave privata va in un backup offline (senza, le copie installate non si possono più aggiornare) e su GitHub: *Settings → Environments → New environment* `release`, *Deployment branches and tags* limitato al branch `main` (nessun tag: un tag può puntare a un commit non revisionato), poi i secret dell'environment `TAURI_SIGNING_PRIVATE_KEY` (contenuto di `~/.tauri/termdock.key`) e `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. Non salvarli come secret del repository: quelli sono leggibili dai workflow di qualsiasi branch.

Il workflow di release controlla la chiave prima di creare il tag, firma ogni pacchetto, li verifica con `scripts/updater-manifest.mjs` e allega `latest.json`, che le app installate leggono quando la release viene pubblicata. **Pubblica una bozza solo se contiene `latest.json`**: senza, le copie installate smettono di trovare aggiornamenti. Le installazioni deb/rpm ricevono solo l'avviso (le aggiorna il gestore di pacchetti).

## Licenza

[MIT](LICENSE)
