# Changelog

[English](CHANGELOG.md) · **Italiano**

Tutte le modifiche rilevanti di Termdock sono documentate qui.
Il formato segue [Keep a Changelog](https://keepachangelog.com/it-IT/1.1.0/) e il progetto usa il [Semantic Versioning](https://semver.org/lang/it/).

## [Unreleased]

## [0.6.0] - 2026-10-17

### Aggiunto

- File browser (SFTP): `Ctrl+Shift+S` (o il menu col tasto destro sulla scheda) apre i file del server accanto al suo terminale SSH, sulla stessa connessione senza un nuovo login. Navigazione nelle cartelle, download, upload (con richiesta prima di sostituire un file), nuove cartelle, rinomina ed eliminazione, con avanzamento e annullamento.

### Modificato

- Le conferme delle azioni distruttive (eliminare una connessione o un file remoto) hanno ora *Annulla* come pulsante predefinito.

### Sicurezza

- I trasferimenti considerano il server non fidato: i file locali si scelgono solo in finestre native aperte dal backend; i nomi remoti sono mostrati senza caratteri invisibili o di inversione e non diventano mai percorsi locali; gli elenchi sono limitati; si scaricano solo file regolari; gli upload usano file temporanei esclusivi e sostituiscono la destinazione in modo atomico mantenendone i permessi; su Windows i download ricevono il Mark of the Web.

## [0.5.1] - 2026-10-17

### Corretto

- La scheda attiva era illeggibile quando i colori del terminale e il tema dell'app hanno luminosità diverse (per esempio tema chiaro con Dracula); ora usa il colore del testo del terminale.
- Svuotando un numero nelle Impostazioni (dimensione del carattere, interlinea, scrollback) e salvando, veniva impostato il minimo, per esempio scrollback 0; ora resta il valore attuale.
- Le scorciatoie non agiscono più sulle schede dietro una finestra di dialogo aperta, e tenerne premuta una non apre o chiude più una raffica di pannelli.
- `Alt+frecce` sposta tra i pannelli solo da un terminale, così i campi di testo mantengono lo spostamento per parole.
- Il trascinamento di un divisore risponde solo al tasto sinistro del mouse; il suggerimento sulla × della scheda non indica più `Ctrl+Shift+W`, che ora chiude un pannello.

## [0.5.0] - 2026-10-17

### Aggiunto

- Pannelli divisi: `Ctrl+Shift+E` divide il pannello attivo a destra, `Ctrl+Shift+O` in basso (anche dal menu col tasto destro sulla scheda). Il nuovo pannello apre la stessa connessione o shell; i divisori si trascinano e `Alt+frecce` sposta tra i pannelli.
- Impostazioni di aspetto: tema scuro, chiaro o di sistema, schemi di colori del terminale (Termdock Dark/Light, Solarized Dark/Light, Dracula, Nord, Gruvbox Dark), carattere, dimensione, interlinea, stile e lampeggio del cursore, scrollback, con anteprima dal vivo.
- `Ctrl` + `+` / `-` / `0` ingrandisce, rimpicciolisce o ripristina il carattere del terminale.

### Modificato

- `Ctrl+Shift+W` chiude il pannello attivo; una scheda si chiude con il suo ultimo pannello (la × sulla scheda chiude ancora l'intera scheda).

## [0.4.1] - 2026-10-17

### Modificato

- Il dialogo di aggiornamento mostra le note di rilascio con titoli ed elenchi puntati invece del testo grezzo.

## [0.4.0] - 2026-10-16

### Aggiunto

- Aggiornamenti automatici: Termdock cerca una nuova versione all'avvio e una volta al giorno e, dopo la tua conferma, la scarica, la verifica e si riavvia. Nelle Impostazioni ci sono l'interruttore per disattivare il controllo, il pulsante "Controlla ora" e la versione attuale. Le copie installate da pacchetti deb/rpm ricevono solo l'avviso della nuova versione. Le versioni precedenti non hanno l'updater: la 0.4.0 va installata a mano una volta.

### Sicurezza

- Gli aggiornamenti sono accettati solo se firmati con la chiave inclusa nell'app e firmati per la versione che annunciano, quindi tramite l'updater non si possono installare né un pacchetto manomesso né una release più vecchia.

## [0.3.0] - 2026-10-16

### Aggiunto

- Scelta della lingua nella nuova finestra Impostazioni (pulsante ingranaggio o `Ctrl+,`): Automatica (lingua del sistema), English o Italiano. Il cambio vale subito, senza chiudere le sessioni aperte, e viene ricordato.
- Traduzioni organizzate con un file per lingua (`src/locales/`), così è semplice aggiungere nuove lingue.
- Documentazione in inglese e in italiano.

## [0.2.0] - 2026-10-15

### Aggiunto

- Jump host (ProxyJump): connessione attraverso una o più connessioni SSH salvate; ogni salto verifica la propria host key e si autentica separatamente.
- Port forwarding salvato nella connessione: locale (`-L`), remoto (`-R`) e dinamico SOCKS5 (`-D`), con lo stato mostrato nel terminale.
- Import delle connessioni da `~/.ssh/config` con anteprima: Host, HostName, User, Port, IdentityFile, ProxyJump, LocalForward, RemoteForward, DynamicForward, ServerAliveInterval.

### Modificato

- Una connessione usata come jump host non può essere eliminata, né trasformata in shell locale, finché altre connessioni passano da essa; i jump host mancanti o in ciclo vengono rifiutati al salvataggio.

### Corretto

- L'input binario di xterm.js (alcuni report del mouse) veniva codificato in UTF-8 invece di essere inviato come byte.
- Chiudendo una scheda con il dialogo della host key o della password aperto, il dialogo restava a schermo.

### Sicurezza

- I banner del server, i segnali di uscita e i messaggi di errore vengono ripuliti dai caratteri di controllo prima di essere mostrati nel terminale (nessuna iniezione di sequenze di escape).
- Una password o passphrase salvata che viene rifiutata è eliminata invece di essere riprovata a ogni accesso.
- Timeout dell'handshake SOCKS5, limite di connessioni per listener e attesa in caso di errori di accept nel port forwarding.

## [0.1.0] - 2026-10-15

### Aggiunto

- Barra dei preferiti: connessioni con un clic, ordinamento con drag & drop, `Ctrl+Shift+1…9`.
- Terminale SSH (russh) con autenticazione tramite agent, chiave privata, password e keyboard-interactive / 2FA.
- Shell locali in un vero PTY: PowerShell, Windows PowerShell, cmd, WSL, Git Bash, bash, zsh, fish.
- Gestore connessioni con gruppi, colori e ricerca; connessione rapida `utente@host:porta`.
- Verifica delle host key con il `known_hosts` di Termdock e quello di OpenSSH (trust on first use, chiavi cambiate o revocate rifiutate).
- Password e passphrase salvate solo nel portachiavi del sistema, su richiesta.
- Installer per Windows (MSI, NSIS), macOS (DMG universale) e Linux (AppImage, deb, rpm).

[Unreleased]: https://github.com/LuuckA21/termdock/compare/v0.6.0...HEAD
[0.6.0]: https://github.com/LuuckA21/termdock/compare/v0.5.1...v0.6.0
[0.5.1]: https://github.com/LuuckA21/termdock/compare/v0.5.0...v0.5.1
[0.5.0]: https://github.com/LuuckA21/termdock/compare/v0.4.1...v0.5.0
[0.4.1]: https://github.com/LuuckA21/termdock/compare/v0.4.0...v0.4.1
[0.4.0]: https://github.com/LuuckA21/termdock/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/LuuckA21/termdock/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/LuuckA21/termdock/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/LuuckA21/termdock/releases/tag/v0.1.0
