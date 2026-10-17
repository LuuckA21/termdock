# Changelog

[English](CHANGELOG.md) · **Italiano**

Tutte le modifiche rilevanti di Termdock sono documentate qui.
Il formato segue [Keep a Changelog](https://keepachangelog.com/it-IT/1.1.0/) e il progetto usa il [Semantic Versioning](https://semver.org/lang/it/).

## [Unreleased]

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

[Unreleased]: https://github.com/LuuckA21/termdock/compare/v0.4.1...HEAD
[0.4.1]: https://github.com/LuuckA21/termdock/compare/v0.4.0...v0.4.1
[0.4.0]: https://github.com/LuuckA21/termdock/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/LuuckA21/termdock/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/LuuckA21/termdock/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/LuuckA21/termdock/releases/tag/v0.1.0
