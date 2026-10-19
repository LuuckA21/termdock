# Changelog

[English](CHANGELOG.md) · **Italiano**

Tutte le modifiche rilevanti di Termdock sono documentate qui.
Il formato segue [Keep a Changelog](https://keepachangelog.com/it-IT/1.1.0/) e il progetto usa il [Semantic Versioning](https://semver.org/lang/it/).

## [Unreleased]

## [0.18.0] - 2026-10-19

### Aggiunto

- Le schede si possono rinominare (doppio clic o menu della scheda), riordinare trascinandole e duplicare (menu della scheda → *Duplica scheda*).
- Una scheda nascosta mostra un punto quando il suo terminale stampa qualcosa, e una campanella quando il programma la fa suonare (per esempio alla fine di un comando lungo).
- Chiudere una scheda, un pannello o Termdock con connessioni SSH aperte chiede conferma; si può disattivare in Impostazioni → Generale.
- La finestra si riapre con la dimensione e la posizione che aveva (solo su un monitor ancora presente).
- La connessione rapida ricorda gli ultimi 10 indirizzi che si sono collegati: `Su` / `Giù` nel suo campo, oppure i suggerimenti; la cronologia si può cancellare in Impostazioni → Generale.

## [0.17.0] - 2026-10-19

### Aggiunto

- Chiavi SSH: *Genera…* accanto al percorso della chiave di una connessione crea una chiave Ed25519 con passphrase, salvata in `~/.ssh` o dove scegli (un file esistente non viene mai sostituito), mostra la chiave pubblica da copiare e compila la connessione. *Installa chiave pubblica…* nel menu della scheda di un terminale connesso aggiunge una chiave pubblica a `~/.ssh/authorized_keys` su quel server, come `ssh-copy-id`.

## [0.16.0] - 2026-10-18

### Aggiunto

- Riconnessione automatica: quando la rete o il server fanno cadere una connessione SSH, il terminale riprova da solo alcune volte nell'arco di circa cinque minuti (`Invio`: subito, `Esc`: basta), con i soliti controlli di host key e password. La registrazione della sessione continua (con una nota) e il browser dei file si riapre nella stessa cartella. Si ferma subito per tutto ciò che richiede te (host key, autenticazione) e si può disattivare in Impostazioni → Generale.

### Corretto

- Nei log di sessione non compare più uno spazio in più dove una riga di comando lunga va a capo sullo schermo.

### Sicurezza

- I file scaricati ricevono il flag di quarantena su macOS, così Gatekeeper li controlla prima dell'esecuzione (come già fa SmartScreen su Windows).

## [0.15.0] - 2026-10-18

### Aggiunto

- Backup della configurazione: Impostazioni → Generale → *Esporta…* salva connessioni, preferiti, layout, snippet e impostazioni in un unico file JSON, e *Importa…* mostra cosa contiene un backup, poi aggiunge solo ciò che è nuovo oppure sostituisce la configurazione attuale. Password, chiavi e host key non vengono mai inclusi.
- Registrazione della sessione: menu della scheda → *Registra su file…* scrive ciò che il terminale mostra da quel momento in un file di testo a scelta, senza colori né altre sequenze di escape, fino a *Ferma registrazione* o alla fine della sessione; un punto rosso sulla scheda indica che è attiva.

### Modificato

- Date e ore nel browser dei file seguono la lingua dell'interfaccia invece di quella del sistema.

### Sicurezza

- I file di backup e i log di sessione vengono creati leggibili solo dal proprietario (Unix); l'importazione di un backup lo controlla per intero e chiede prima di cambiare qualcosa, e *Sostituisci* elimina le password salvate delle connessioni che spariscono o puntano altrove.

## [0.14.0] - 2026-10-18

### Aggiunto

- Snippet: comandi o blocchi di testo salvati, scritti nel terminale attivo dal nuovo menu *Snippet* (barra in alto, menu della scheda, `Ctrl+Shift+P`), volendo seguiti da Invio; ognuno può essere limitato alle connessioni di un gruppo, e i primi nove possono avere scorciatoie `1…9`. Si gestiscono nella nuova scheda Impostazioni → Snippet.

### Modificato

- I pacchetti Linux sono compilati in Ubuntu 22.04, quindi funzionano anche sulle distribuzioni con glibc 2.35 (Ubuntu 22.04, Debian 12 e successive); la CI li compila a ogni modifica e fallisce se serve una glibc più recente.

## [0.13.0] - 2026-10-18

### Aggiunto

- Si possono caricare cartelle intere (Carica → Carica cartelle…, oppure trascinandole sul file browser) e scaricarle (clic destro → Scarica), con le loro sottocartelle.
- Una coda dei trasferimenti sotto i file: upload e download aspettano il loro turno, ognuno con il suo avanzamento, un pulsante Annulla e il risultato (copiati, mantenuti, esclusi, non riusciti); intanto il file browser resta utilizzabile.
- Quando dei file esistono già nella destinazione, una sola domanda per tutti: sostituisci esistenti o mantieni esistenti.

### Sicurezza

- I trasferimenti di cartelle non seguono mai link o file speciali, da entrambe le parti, e prendono al massimo 10.000 elementi e 32 livelli; i nomi remoti vengono ripuliti prima di diventare nomi locali, le cartelle locali e remote vengono unite solo se sono vere cartelle, e il cambio utente aspetta i trasferimenti in corso.

## [0.12.1] - 2026-10-18

### Corretto

- Un portachiavi lento (o in attesa della sua finestra di sblocco), un disco lento o una chiave protetta da passphrase non rallentano più le altre sessioni mentre una connessione esegue il login; anche aprire un file per modificarlo, scegliere l'editor e installare un aggiornamento non bloccano più l'app.
- Dopo il cambio lingua, i terminali e i browser di file già aperti mostrano pulsanti, suggerimenti e numero di elementi nella nuova lingua.
- Una scheda la cui connessione aspetta una risposta (host key o password) mostra un lucchetto finché non la apri.

## [0.12.0] - 2026-10-18

### Sicurezza

- Un host registrato con una chiave di un tipo non riceve più la domanda da "host sconosciuto" quando mostra una chiave di un altro tipo: vale come chiave cambiata e la connessione viene rifiutata, e i tipi di chiave registrati vengono offerti per primi.
- I link mandati dal server nel terminale (OSC 8) non vengono più aperti.
- Al testo incollato vengono tolti Escape e gli altri caratteri di controllo, così non può chiudere in anticipo il bracketed paste; incollare più righe in un programma senza bracketed paste chiede prima conferma.
- Una domanda di password o di host key da una scheda in secondo piano aspetta che quella scheda venga mostrata, e non prende la tastiera mentre scrivi in un altro pannello.
- I canali X11, agent, session, direct-tcpip e socket Unix aperti dal server vengono rifiutati.
- Le chiavi RSA non firmano mai con SHA-1; a un server che accetta solo firme SHA-1 non vengono offerte.
- Cambiare host, porta, utente o autenticazione di una connessione ora rimuove la password salvata prima di salvare la modifica; se non riesce, non cambia nulla. Un salvataggio non riuscito non lascia più la modifica in memoria.
- Caricare su un link scrive nel file a cui punta solo se entrambi appartengono allo stesso utente; shell e interpreti non si possono più scegliere come editor.
- Gli elenchi di file sono limitati anche nella dimensione dei nomi; l'attesa di 5 minuti per la password di sudo viene rispettata.
- Le release si compilano solo da `main` (nessun trigger sui tag), i checkout della CI non conservano credenziali e i file della chiave di firma sono ignorati da git.

### Corretto

- Connettersi a una porta che accetta e resta in silenzio ora va in timeout invece di bloccarsi.
- L'import da `~/.ssh/config` non fallisce più quando un host è elencato prima del suo bastion ProxyJump; i cicli di ProxyJump vengono segnalati.
- Due forwarding remoti sullo stesso indirizzo non si rompono più a vicenda.
- Una password rifiutata via keyboard-interactive non viene richiesta di nuovo in un secondo giro (meno tentativi falliti sul server).
- I file `known_hosts` con byte non UTF-8 (es. un commento Latin-1) non bloccano più tutte le connessioni.
- Il tasto destro incolla di nuovo quando non c'è selezione (prima selezionava e copiava la parola sotto il puntatore).
- Su macOS, Option scrive di nuovo i caratteri (@ # [ ]…) nel terminale, e *Vai alla scheda 1…9* usa di default Cmd+1…9.
- Invio su una connessione nel gestore non arriva più anche al terminale dietro.
- Chiudere un terminale chiude anche il suo file browser; Elimina, Rinomina e Nuova cartella agiscono sulla cartella mostrata quando sono stati avviati, anche con Segui attivo.
- *File (SFTP)* non viene più offerto per le shell locali salvate; il titolo della finestra segue il pannello attivo; i menu restituiscono il focus della tastiera.
- Una modifica il cui file browser si è chiuso ora dice che i salvataggi non vengono più caricati; un salvataggio non riuscito non viene più segnalato come conflitto.
- Uscire durante un trasferimento rimuove i suoi file temporanei; una shell che esce subito non lascia più una sessione morta.
- Le richieste di login mostrano il testo del server con i caratteri invisibili resi visibili; "Passphrase" è tradotto.
- Documentazione: le build locali usano `npm run tauri build -- --no-sign`; archiviazione, algoritmi e architettura sono descritti correttamente.

## [0.11.0] - 2026-10-17

### Aggiunto

- Impostazioni → *Tasti*: cambia, rimuovi o ripristina ogni scorciatoia da tastiera (fai clic e premi i nuovi tasti), e scegli i modificatori di *Vai alla scheda 1…9* e *Apri preferito 1…9* o disattivali. Le combinazioni già usate vengono spostate solo dopo conferma; per Ctrl+lettera, Alt+lettera e Ctrl+Alt (AltGr su Windows) compare un avviso. Suggerimenti, menu ed elenco delle scorciatoie seguono i tasti scelti.

## [0.10.1] - 2026-10-17

### Modificato

- Le Impostazioni sono più grandi e divise in schede (Generale, Aspetto, File, Aggiornamenti), con un'anteprima del terminale più ampia; si riaprono sull'ultima scheda usata.

## [0.10.0] - 2026-10-17

### Aggiunto

- Impostazioni → *Modifica dei file remoti*: scegli il programma con cui *Modifica* (`F4`) apre i file, selezionandolo nella finestra di scelta file del sistema; *Usa l'editor di sistema* torna a quello predefinito.

### Corretto

- File browser: *Modifica* falliva su Windows 11 con "impossibile trovare il file specificato" quando è installata solo la versione dello Store di Notepad; ora trova anche quel Notepad.

## [0.9.1] - 2026-10-17

### Aggiunto

- File browser: trascina file dal computer per caricarli (le cartelle sono saltate).
- File browser: *Vai alla cartella del terminale* e *Segui* (si sposta insieme ai `cd` nel terminale), da OSC 7 o dal solito titolo `utente@host: dir`.
- File browser: *Modifica* (`F4`) apre un file di testo remoto nell'editor di testo di sistema; ogni salvataggio viene ricaricato sul server, chiedendo prima se il file è cambiato sul server nel frattempo.

### Modificato

- I preferiti si riordinano trascinandoli con il puntatore (la finestra ora riceve i file rilasciati dal sistema).
- Caricare su un file esistente ne mantiene anche proprietario e gruppo se il server lo consente, e caricare su un link sostituisce il file a cui punta invece del link.

### Corretto

- Release: la bozza della release è creata una volta sola prima delle build, così i pacchetti di ogni piattaforma finiscono nella stessa bozza (la 0.9.0 non è stata pubblicata per questo motivo).

## [0.9.0] - 2026-10-17

Non pubblicata: la build della release ha messo i pacchetti macOS in una seconda bozza. Le sue modifiche sono nella 0.9.1.

## [0.8.0] - 2026-10-17

### Aggiunto

- Ricerca nel terminale (`Ctrl+Shift+F`, o dal menu della scheda): risultati evidenziati nello storico, successivo/precedente, maiuscole/minuscole, espressioni regolari e conteggio dei risultati.
- Layout salvati: salva i pannelli di una scheda (connessioni, shell, direzione degli split e dimensioni) con `Ctrl+Shift+L` o dal menu della scheda, riaprili dal nuovo menu *Layout* e scegline uno da aprire all'avvio. Le connessioni eliminate nel frattempo si aprono come terminale locale, con un avviso.

## [0.7.0] - 2026-10-17

### Aggiunto

- Pannelli divisi come Windows Terminal: dividendo si sceglie cosa aprire nel nuovo pannello (la stessa connessione, una shell locale, qualsiasi connessione salvata o un indirizzo `utente@host`), così server A e server B possono stare affiancati. Invio mantiene il comportamento precedente (stessa connessione).
- Menu della scheda → *Affianca a*: mette una scheda aperta accanto a un'altra, mantenendo le sue sessioni.
- Menu dei preferiti: *Apri a destra* / *Apri sotto*.
- File browser come altro utente: dopo `sudo -iu utente` nel terminale propone di aprire i file come quell'utente (tramite sudo sul server; la password di sudo è chiesta solo se serve e mai salvata). Si può anche scegliere qualsiasi utente dal menu utente del file browser; si torna all'utente di login con un clic.

### Modificato

- Nei pannelli stretti il percorso del file browser va su una riga a sé.

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

[Unreleased]: https://github.com/LuuckA21/termdock/compare/v0.18.0...HEAD
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
