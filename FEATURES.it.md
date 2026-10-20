# Termdock: funzionalità

[English](FEATURES.md) · **Italiano**

Tutto ciò che Termdock sa fare, in ordine di importanza. I dettagli sono nel [README](README.it.md) e nel [modello di sicurezza](SECURITY.it.md).

## 1. Il cuore: connettersi ai server

- **SSH con terminale completo**: xterm-256color, truecolor, Unicode, ridimensionamento.
- **Sicurezza della connessione**:
  - verifica della host key prima dell'accesso, con impronta SHA-256 da confermare;
  - connessione rifiutata se la chiave cambia;
  - legge anche `~/.ssh/known_hosts`, comprese voci hashate, wildcard e `@revoked`.
- **Autenticazione**: SSH agent (ssh-agent, agent OpenSSH di Windows, Pageant), chiavi private anche cifrate, password, keyboard-interactive / 2FA.
- **Password nel portachiavi del sistema** (Windows Credential Manager, macOS Keychain, Secret Service), solo se lo chiedi; mai su disco.
- **Gestore connessioni**: gruppi, colori, ricerca, stella per i preferiti; *Duplica* per partire da una connessione simile; un clic apre tutte le connessioni di un gruppo, una scheda ciascuna.
- **Barra dei preferiti**: un clic per connettersi, riordino trascinando, `Ctrl+Shift+1…9`.
- **Connessione rapida** `utente@host:porta`, con la cronologia degli ultimi 10 indirizzi (Su/Giù e suggerimenti).
- **Shell locali** in un vero terminale: PowerShell 7, Windows PowerShell, cmd, WSL, Git Bash; bash, zsh, fish, pwsh su macOS e Linux.

## 2. Lavorare su più server insieme

- **Schede multiple**:
  - rinomina con doppio clic, riordino trascinandole, duplica;
  - chiudi le altre schede o quelle a destra, con un'unica conferma;
  - pulisci terminale, dal menu o con una scorciatoia a scelta;
  - un punto se una scheda nascosta stampa qualcosa, una campanella se suona il campanello del terminale.
- **Pannelli divisi** come Windows Terminal: affiancati o sovrapposti, schede già aperte affiancabili senza riconnettersi, `Alt+frecce` per spostarsi.
- **Layout salvati**: disposizione e connessioni di una scheda; uno può aprirsi all'avvio.
- **Riconnessione automatica** quando la rete o il server cadono:
  - riprova per circa cinque minuti; Invio riprova subito, Esc smette;
  - log e browser dei file riprendono quando torna.

## 3. File e server

- **File browser SFTP** sulla stessa connessione, senza un secondo login:
  - navigazione, download e upload anche di cartelle intere;
  - coda dei trasferimenti con avanzamento e Annulla;
  - trascinamento dal computer per caricare;
  - segue la cartella del terminale.
- **File come altro utente tramite sudo**: password chiesta solo se serve, mai salvata.
- **Modifica dei file remoti** (`F4`) nell'editor di testo, con caricamento automatico a ogni salvataggio e controllo se il file è cambiato sul server.
- **Jump host (ProxyJump)**, anche a catena; ogni salto verifica la propria host key.
- **Port forwarding** salvato nella connessione: locale `-L`, remoto `-R`, SOCKS5 `-D`.
- **Chiavi SSH**:
  - genera una chiave Ed25519 con passphrase, senza mai sovrascrivere file esistenti;
  - installa la chiave pubblica sul server, come `ssh-copy-id`.

## 4. Produttività

- **Snippet**: comandi salvati, inviati con o senza Invio, limitabili a un gruppo di connessioni, scorciatoie `1…9`.
- **Ricerca nel terminale** (`Ctrl+Shift+F`): evidenziazione, maiuscole/minuscole, espressioni regolari.
- **Scorciatoie personalizzabili** in Impostazioni → Tasti, con avviso per i tasti che usano i programmi nel terminale.
- **Copia/incolla stile PuTTY** (selezione e tasto destro), con incolla protetto: caratteri di controllo rimossi e anteprima per più righe, con i caratteri invisibili segnalati.
- **Copia alla selezione** (opzionale, spenta di default): il testo selezionato va subito negli appunti.
- **Registrazione della sessione** su file di testo pulito: spenta di default, file leggibile solo da te.
- **Import da `~/.ssh/config`** con anteprima.

## 5. Comodità e configurazione

- **Conferma prima di chiudere** schede, pannelli o l'app con connessioni SSH aperte.
- **La finestra ricorda dimensione e posizione.**
- **Backup della configurazione**: esporta e importa connessioni, preferiti, layout, snippet e impostazioni, mai password o chiavi; anteprima prima di importare, poi aggiungi o sostituisci.
- **Temi e caratteri**:
  - tema scuro, chiaro o di sistema;
  - 6 schemi di colori: Termdock Dark/Light, Solarized, Dracula, Nord, Gruvbox;
  - carattere, cursore e scrollback, con anteprima dal vivo;
  - zoom con `Ctrl` + `+` / `-` / `0`.
- **Interfaccia in inglese e italiano**, automatica o a scelta, con cambio immediato.

## 6. Aggiornamenti e fiducia

- **Aggiornamenti in-app firmati**: avviso all'avvio e una volta al giorno, installazione solo dopo conferma, niente pacchetti manomessi né versioni più vecchie.
- **Download marcati come provenienti da Internet**: Mark of the Web su Windows, quarantena su macOS.
- **Punti di forza tecnici**:
  - backend in Rust e SSH in Rust puro, senza processi `ssh` esterni;
  - algoritmi moderni, con scambio chiavi post-quantistico come prima scelta e senza SHA-1 né CBC;
  - Tauri 2 con permessi minimi;
  - testato in CI contro un vero OpenSSH.
- **Piattaforme**:
  - Windows: installer `.exe` e `.msi`;
  - macOS: un solo pacchetto per Intel e Apple Silicon;
  - Linux: AppImage, `.deb`, `.rpm` (Ubuntu 22.04, Debian 12 e successive).
