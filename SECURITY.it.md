# Sicurezza

[English](SECURITY.md) · **Italiano**

## Segnalare una vulnerabilità

Segnala le vulnerabilità in privato tramite "Report a vulnerability" (Security advisories) di GitHub su questo repository, non in una issue pubblica.

## Modello delle minacce

Termdock custodisce le credenziali per macchine remote ed esegue shell locali, quindi il progetto assume che:

1. **La rete sia ostile.** Chiunque si trovi tra Termdock e il server può provare a impersonarlo o a degradare la connessione.
2. **Il disco della macchina possa essere letto** da altri utenti o dai backup. Niente di ciò che c'è sopra deve servire per accedere altrove.
3. **La web view vada trattata come la parte meno affidabile** dell'app. L'output dei server, i banner e i nomi dei profili sono dati non fidati mostrati al suo interno.

## Controlli

### Trasporto SSH (`crates/termdock-core/src/ssh.rs`)

| | Offerti (in ordine) | Mai offerti |
| --- | --- | --- |
| Scambio chiavi | `mlkem768x25519-sha256` (ibrido post-quantistico), `curve25519-sha256`, ECDH NIST P-256/384/521, DH group 16/18 (SHA-512), group 14 (SHA-256) | qualsiasi cosa con SHA-1, group 1 |
| Host key | Ed25519, ECDSA, RSA con SHA-512/SHA-256 | `ssh-rsa` (SHA-1), DSA, certificati |
| Cifrari | `chacha20-poly1305`, AES-256/128-GCM, AES-CTR | CBC, 3DES, `none` |
| MAC | HMAC-SHA-512/256 ETM, poi non-ETM | HMAC-SHA-1, `none` |
| Compressione | nessuna | zlib (evita side channel legati alla compressione) |

Viene annunciato `kex-strict-c-v00@openssh.com`, che mitiga l'attacco Terrapin (CVE-2023-48795). Un test unitario fallisce se un algoritmo debole viene aggiunto a queste liste.

### Autenticazione dell'host (`hostkeys.rs`)

- La chiave del server viene verificata durante lo scambio chiavi, **prima** che venga inviata qualsiasi credenziale (coperto da un test end-to-end). Lo scambio, domanda compresa, deve finire entro 4 minuti: una porta che accetta la connessione e poi resta in silenzio non può bloccare una scheda.
- Chiave sconosciuta: all'utente vengono mostrati algoritmo e impronta SHA-256; il pulsante predefinito è *Annulla*. Solo una conferma esplicita registra la chiave.
- Chiave cambiata o marcata `@revoked`: la connessione viene rifiutata. Il dialogo non permette di forzarla; l'utente deve rimuovere esplicitamente la vecchia chiave ("Dimentica host key"), operazione che agisce solo sul `known_hosts` di Termdock. `~/.ssh/known_hosts` viene letto ma mai modificato.
- Anche una chiave di tipo diverso da quella registrata vale come cambiata: una connessione offre per primi i tipi di chiave registrati, così un server che ne ha uno lo mostra sempre, e un man-in-the-middle che offre un altro tipo non può trasformare un host noto in una domanda da "host sconosciuto".
- I certificati host non sono accettati (non è configurata nessuna CA fidata), quindi un certificato non può aggirare la chiave registrata.

### Credenziali (`vault.rs`, `prompts.rs`)

- Password e passphrase delle chiavi vengono salvate solo se l'utente spunta "Ricorda", e solo nel portachiavi del sistema operativo (Windows Credential Manager, macOS Keychain, Secret Service). Se non c'è un portachiavi disponibile non viene scritto nulla da nessuna parte: un segreto da ricordare resta in memoria (azzerata dopo l'uso) solo fino all'uscita da Termdock.
- I segreti salvati non vengono mai restituiti all'interfaccia; i segreti viaggiano solo da interfaccia a backend.
- I buffer dei segreti sono `Zeroizing` e vengono azzerati dopo l'uso; anche il contenuto dei file di chiave privata viene azzerato.
- Le chiavi utente RSA firmano solo con SHA-2 (SHA-512 se il server non dice cosa accetta); a un server che accetta solo firme `ssh-rsa` (SHA-1) la chiave non viene offerta. keyboard-interactive non viene ritentato dopo aver rifiutato una password, e un server può fare al massimo 10 giri di domande per tentativo.
- Una password ricordata viene eliminata quando cambiano host, porta, utente o metodo di autenticazione del profilo.
- Le connessioni rapide (senza profilo salvato) non salvano mai nulla.
- Una password o passphrase ricordata che viene rifiutata è eliminata subito, invece di essere riprovata a ogni accesso (cosa che potrebbe far scattare MaxAuthTries o fail2ban).
- Le richieste all'utente scadono dopo 3 minuti; una richiesta senza risposta vale come "no". Una domanda da una sessione in una scheda in secondo piano aspetta che quella scheda venga mostrata (intanto la scheda mostra un lucchetto), e un campo password non prende il focus mentre l'utente sta scrivendo in un altro pannello, così i tasti destinati a un server non possono finire a rispondere a un altro.

### Applicazione desktop (Tauri 2)

- Content-Security-Policy restrittiva: script solo dal bundle dell'app, niente `eval`, nessuna origine remota, niente frame/object/worker.
- Ogni comando IPC deve essere autorizzato esplicitamente in `src-tauri/capabilities/default.json` (permessi generati per comando). Non sono inclusi plugin per file system, shell, HTTP o apertura di URL. L'interfaccia può usare il plugin degli appunti (lettura/scrittura testo); i plugin di aggiornamento e delle finestre di dialogo sono usati solo dal backend e nessuno dei loro comandi è autorizzato.
- L'interfaccia costruisce i nodi DOM solo con `textContent`; i colori dei profili sono validati (`#rrggbb`) sia nel backend sia prima dell'uso.
- Il testo mostrato nel terminale che non è output della sessione (banner del server, segnale di uscita, messaggi di errore e di stato) viene ripulito dai caratteri di controllo, così un server non può iniettare sequenze di escape attraverso di esso, nemmeno prima che l'utente si sia autenticato.
- I link che il server manda nell'output del terminale (OSC 8) non vengono mai aperti: nessun browser e nessuna nuova finestra per un URL scelto dal server.
- Al testo incollato vengono tolti Escape e gli altri caratteri di controllo prima che arrivi alla sessione, così non può chiudere in anticipo il bracketed paste né inviare sequenze di tasti; un testo di più righe va confermato prima se il programma non usa il bracketed paste (ogni riga verrebbe eseguita subito). Anche l'incolla del sistema (Ctrl/Cmd+V) passa di qui.
- Le shell locali si possono avviare solo dall'elenco rilevato (per id) o da un profilo salvato.
- Le preferenze dell'interfaccia (lingua, aspetto, scorciatoie da tastiera, controllo aggiornamenti) e i layout salvati stanno nel local storage della web view e non contengono segreti: un layout memorizza il suo nome e, per ogni pannello, l'id di una connessione o di una shell oppure un indirizzo di connessione rapida e il titolo del pannello; aprirlo (anche automaticamente all'avvio, se scelto) passa dagli stessi controlli di host key e autenticazione di qualsiasi connessione. Anche gli snippet stanno lì, come testo in chiaro: servono per i comandi e le Impostazioni avvertono di non metterci mai password; inviarne uno passa dalla stessa pulizia dell'incolla (caratteri di controllo rimossi, più righe confermate quando il programma non usa il bracketed paste), e solo nel terminale attivo dopo un clic o una scorciatoia. Sono validate a ogni lettura; il nome del carattere è ridotto a un solo nome di famiglia prima di arrivare al CSS o a xterm.js.
- `freezePrototype` è disattivato perché rompe xterm.js; l'interfaccia non unisce oggetti non fidati, quindi la prototype pollution non ha punti di ingresso.

### Jump host e port forwarding (`ssh.rs`, `forward.rs`)

- Ogni salto di una catena ProxyJump è una sessione SSH completa: la sua host key viene verificata con `known_hosts` e si autentica con le proprie credenziali. Un bastion vede solo traffico cifrato per il salto successivo; non gli viene inoltrato nulla (niente agent forwarding).
- I forwarding configurati su un profilo usato come bastion non vengono aperti quando è solo un salto.
- I listener locali (`-L`) e SOCKS (`-D`) ascoltano su `127.0.0.1` salvo diversa indicazione nel profilo; l'editor avvisa quando si usa un indirizzo non di loopback. Come con OpenSSH, altri utenti e processi locali possono collegarsi a un listener di loopback mentre la sessione è aperta.
- Il proxy SOCKS5 implementa solo CONNECT (niente BIND/UDP), senza autenticazione, e valida i nomi richiesti. I client devono completare l'handshake entro 10 s.
- Ogni listener locale serve al massimo 256 connessioni contemporanee; in caso di errore di accept attende invece di girare a vuoto.
- Le connessioni remote (`-R`) sono accettate solo per i forwarding richiesti da Termdock; qualsiasi altro canale `forwarded-tcpip` aperto dal server viene rifiutato. Lo stesso vale per i canali X11, agent, session, `direct-tcpip` e socket Unix: un server non può aprirne nessuno verso il client.
- I listener vengono chiusi alla fine della sessione del terminale.
- Le catene sono limitate a 8 salti e i cicli vengono rifiutati; un profilo usato come jump host non può essere eliminato finché è referenziato.

### Import da `~/.ssh/config` (`sshconfig.rs`)

- Il backend legge solo `~/.ssh/config`; l'interfaccia non può scegliere un altro file.
- `ProxyCommand`, `Match`, `Include`, token sconosciuti e forwarding su socket Unix vengono segnalati, mai eseguiti o indovinati.
- I profili importati passano per la stessa validazione di quelli creati a mano.

### Trasferimento file (`sftp.rs`, `src-tauri/src/sftp.rs`, `src-tauri/src/transfer.rs`, `src/files.ts`)

Il server SSH non è considerato fidato: controlla nomi, elenchi, dimensioni, contenuti e messaggi di errore.

- SFTP usa nuovi canali della stessa connessione SSH del terminale: stessa host key verificata, nessuna nuova credenziale, niente salvato. I file browser terminano con la loro sessione di terminale.
- L'interfaccia non indica mai un file locale. I download sono salvati dove l'utente sceglie in una finestra di salvataggio nativa aperta dal backend; gli upload accettano solo token monouso (validi 10 minuti) per file o cartelle scelti dall'utente in una finestra nativa; i file sono aperti in quel momento così da non poter essere sostituiti dopo. Le finestre hanno i titoli del sistema (l'interfaccia non può formularli) e i comandi IPC del plugin delle finestre di dialogo non sono autorizzati, quindi non possono allargare alcun accesso al file system.
- I nomi remoti sono mostrati con i caratteri di controllo, invisibili e di inversione del testo sostituiti (niente inganni tipo `evil<U+202E>gpj.exe`) e non diventano mai percorsi locali; il nome proposto nella finestra di salvataggio è ripulito da separatori, nomi riservati di Windows e caratteri invisibili. I nomi che il server invia in un'altra codifica sono elencati ma non toccati.
- I nuovi nomi (cartella, rinomina, upload) devono essere un solo componente: niente `/`, NUL, caratteri di controllo o invisibili, `.` o `..`, al massimo 255 byte.
- Gli elenchi sono letti a blocchi e si fermano a 20.000 voci o dopo 30 secondi, qualunque cosa invii il server; la libreria SFTP limita anche i pacchetti (256 KiB) e applica un timeout a ogni richiesta. Anche nomi più lunghi di 4 KiB, o più di 8 MiB di nomi in tutto, chiudono l'elenco (che viene allora segnato come incompleto).
- I download accettano solo file regolari (verificato sul file aperto, quindi link a dispositivi o FIFO sono rifiutati) e leggono al massimo la dimensione indicata all'apertura. Sono scritti in un file temporaneo accanto alla destinazione e spostati al loro posto solo a trasferimento completato; un trasferimento fallito o annullato non lascia nulla. I file salvati sono privati (`0600`) su Unix; su Windows ricevono il Mark of the Web (`Zone.Identifier`), così SmartScreen li controlla prima dell'esecuzione.
- Gli upload vanno in un nome nascosto casuale creato in esclusiva con permessi `0600` (un file o un link piazzato da altri non può deviare la scrittura). Solo a trasferimento completato, e per un nome esistente solo dopo la conferma dell'utente, il file prende il posto della destinazione: in modo atomico con `posix-rename@openssh.com`, altrimenti mettendo da parte il vecchio file e ripristinandolo in caso di errore. Un file sostituito mantiene i suoi permessi e, se il server lo consente, proprietario e gruppo (hard link, ACL e attributi estesi non sono mantenuti); uno nuovo prende i permessi del file locale. Caricare su un link scrive nel file a cui punta, che deve essere un file regolare. La dimensione deve corrispondere a quella annunciata, altrimenti non si sostituisce nulla. Caricare su un link scrive nel file a cui punta solo se il link e quel file appartengono allo stesso utente (come protected_symlinks di Linux): un utente non può far puntare un link a un file di sistema e farlo sostituire da chi lavora in quella cartella come root.
- L'annullamento è immediato; chiudere un file browser o la sua scheda annulla i trasferimenti e li lascia ripulire prima che la connessione si chiuda.
- L'eliminazione non è mai ricorsiva (solo file, link e cartelle vuote) e la conferma ha *Annulla* come pulsante predefinito.
- **Cartelle e coda**: ogni upload o download viene prima pianificato (cosa verrà copiato, cosa esiste già) e parte solo dopo che l'utente ha scelto, con un'unica risposta per tutti, se sostituire o mantenere ciò che esiste. Dentro una cartella i link e i file speciali sono esclusi e mai seguiti, da entrambe le parti, così un trasferimento non può andare in ciclo né uscire dalla cartella; si prendono al massimo 10.000 elementi e 32 livelli. Ogni file di una cartella scelta viene ricontrollato come file regolare (non un link) quando arriva il suo turno. I nomi remoti diventano nomi locali solo dopo la stessa pulizia della finestra di salvataggio (niente separatori, caratteri di controllo o invisibili, né nomi riservati da Windows), mantenendo il punto iniziale; due nomi remoti che diventano uguali in locale non vengono scritti entrambi (il secondo è escluso). Una cartella esistente, locale o remota, viene unita solo se è una vera cartella, mai attraverso un link. Ogni file usa la stessa scrittura sicura dei trasferimenti singoli, quindi un elemento annullato o non riuscito non lascia nulla di scritto a metà. I trasferimenti vanno uno alla volta per file browser; il cambio utente aspetta che siano finiti o annullati, e chiudere il file browser li annulla. I trasferimenti pianificati e non avviati entro 10 minuti vengono scartati.
- **File trascinati**: i file trascinati dal sistema sulla finestra arrivano al backend dal sistema operativo (l'interfaccia non invia mai percorsi), vengono aperti subito se sono file regolari, mentre le cartelle sono lette quando l'upload viene pianificato (al massimo 200 in attesa alla volta) e passati all'interfaccia come gli stessi token monouso della finestra di apertura; un rilascio che non cade su un file browser li libera. Su Linux un rilascio è accettato solo sopra un file browser; su Windows e macOS qualunque rilascio sulla finestra diventa token, liberati se manca il file browser. Gli eventi di trascinamento di Tauri comunicano comunque alla web view i percorsi trascinati (solo i nomi: non può leggere i file). La web view non apre mai da sé un file o un link rilasciato.
- **Cartella del terminale**: la cartella comunicata dalla shell (OSC 7, o un titolo come `utente@host: ~/dir`) serve solo a navigare nel file browser, con un clic o con *Segui* attivo; arriva dal server e non apre nulla in locale.
- **Modifica**: un file remoto (regolare, di testo, al massimo 5 MB, limite applicato durante la lettura qualunque dimensione dichiari il server) è copiato in una cartella temporanea privata (`0700`, file `0600`; Mark of the Web su Windows) e aperto solo in un editor di testo, mai eseguito né aperto in base alla sua estensione: il programma scelto nelle Impostazioni o, altrimenti, l'*editor di testo* di sistema. Un programma si può scegliere solo nella finestra di scelta file del sistema (l'interfaccia non può indicarne uno); si accetta solo un `.exe` su Windows (niente script o collegamenti), un'`.app` o un eseguibile su macOS, un file eseguibile su Linux; la scelta è conservata dal backend in `editor.json` (nessun segreto), verificata di nuovo prima di ogni uso e avviata direttamente con il file come unico argomento (nessuna shell). L'editor di testo di sistema è Notepad dal suo percorso completo su Windows (nella cartella di Windows o, se è installata solo la versione dello Store, il suo alias nella cartella `WindowsApps` dell'utente; altrimenti Termdock rifiuta), `open -t` su macOS e su Linux l'applicazione `text/plain` del desktop avviata con `gtk-launch` o, in mancanza, `gio open` su un nome `.txt`; se nessuno dei due è disponibile Termdock rifiuta e suggerisce il download. Un link è modificato nel file a cui punta. Un salvataggio è caricato quando il file ha smesso di cambiare, con la stessa sostituzione sicura degli upload. Prima di ogni caricamento la copia sul server è confrontata con il contenuto letto o salvato l'ultima volta; se è diversa, l'utente viene interpellato prima di sovrascriverla. Le copie restano fino all'uscita da Termdock (chiudere il file browser smette di sorvegliarle) e poi sono cancellate; quelle lasciate da un crash vengono rimosse a un avvio successivo quando hanno più di 7 giorni (solo quelle di questo utente). Shell, interpreti di script e lanciatori (sh, bash, python, perl, node, PowerShell, cmd, wscript, mshta, Terminale…) sono rifiutati come editor, anche dietro un link, perché eseguirebbero il file invece di aprirlo. Quando il file browser si chiude o cambia utente, le sue modifiche si fermano e l'interfaccia lo dice. Un salvataggio non riuscito (rete, permessi) è segnalato come errore, non come conflitto.
- **I file come altro utente** avviano `sftp-server` tramite `sudo -u <utente>` su nuovi canali della stessa connessione; decide la policy sudo del server, Termdock non aggiunge privilegi. Il nome utente deve essere un semplice nome Unix (lettere, cifre, `_ . -`, che inizia con una lettera o `_`, al massimo 32 caratteri) e il comando è composto da parti fisse: si eseguono solo percorsi di `sftp-server` da un elenco interno, mai uno indicato dal server. sudo è avviato con un marcatore di prompt casuale per ogni canale, e la password è inviata **solo dopo che compare esattamente quel prompt**; se sudo non ne ha bisogno (NOPASSWD, un modulo PAM che non la chiede), non viene mai inviata. La password è chiesta in una finestra che indica di chi è la password richiesta da sudo (`%p`: l'utente di login, oppure root o l'utente di destinazione con `rootpw`/`targetpw`), passata al sudo in attesa e scartata subito dopo; non è mai salvata né registrata. Una password errata torna allo stesso sudo, che concede i suoi soliti tre tentativi. Il cambio è sempre un clic esplicito e per root serve una conferma: il titolo del terminale (`utente@host`), controllato dal server, serve solo a suggerirlo. Quando si agisce come `root` l'utente è mostrato in rosso. Un sudo in attesa della password viene chiuso dopo 5 minuti senza risposta.

### Aggiornamenti (`src-tauri/src/updates.rs`, `scripts/updater-manifest.mjs`)

- L'app cerca aggiornamenti solo all'indirizzo HTTPS fissato in `tauri.conf.json` (il `latest.json` dell'ultima release GitHub *pubblicata*; le bozze non vengono mai proposte). Le build di release rifiutano un indirizzo HTTP in chiaro (l'URL di download arriva dal manifest ed è protetto dalla firma del pacchetto qualunque sia il protocollo; i manifest di Termdock contengono solo URL GitHub https) e l'interfaccia non può cambiare indirizzo, chiave o regole sulla versione: può solo chiedere "c'è un aggiornamento?" e "installa quello trovato".
- Ogni pacchetto è verificato con minisign (Ed25519) usando la chiave pubblica inclusa nell'app, dopo il download e di nuovo subito prima dell'installazione. Un pacchetto che non supera il controllo non viene mai scritto al posto dell'app installata.
- La firma copre anche la versione per cui è stata fatta (`requireSignedVersion`): un manifest che annuncia la versione X deve puntare a un pacchetto firmato per X, quindi un manifest manomesso non può spacciare una release più vecchia, firmata regolarmente, per una nuova. Sono accettate solo versioni più recenti (niente downgrade).
- L'interfaccia scarica e installa solo dopo la conferma dell'utente (il pulsante predefinito è *Più tardi*). Anche una web view compromessa potrebbe al massimo avviare l'installazione di una release più recente e firmata regolarmente, con un riavvio. Le note di rilascio arrivano dal manifest, che non è firmato: sono mostrate come testo semplice e troncate.
- Le installazioni deb e rpm non vengono aggiornate sul posto (servirebbe root, che il plugin otterrebbe passando la password dell'utente a `sudo`); l'app si limita ad annunciare la nuova versione.
- La chiave privata esiste solo come secret dell'environment `release` del repository, utilizzabile solo dai workflow eseguiti su `main` (il workflow di release non parte dai tag: un tag può puntare a un commit non revisionato), e nel backup offline del proprietario. Il workflow di release si rifiuta di creare il tag se la chiave manca o non corrisponde alla chiave pubblica dell'app, e verifica ogni pacchetto di aggiornamento (installer Windows, archivio dell'app macOS, AppImage) prima di allegare `latest.json`. Le action che esegue sono bloccate su commit SHA. La chiave è per forza nell'ambiente dello step di build (il bundler di Tauri firma durante la creazione dei pacchetti), quindi gli strumenti di build e le dipendenze girano accanto a essa: le dipendenze sono bloccate (`Cargo.lock`, `package-lock.json`), controllate in CI e aggiornate tramite PR revisionate.
- Privacy: un controllo è una richiesta HTTPS a GitHub all'avvio e una volta al giorno (i download arrivano dalla CDN di GitHub; GitHub vede l'indirizzo IP); si può disattivare nelle Impostazioni.
- Rischio residuo: chi possiede la chiave privata e può pubblicare release su questo repository può distribuire un aggiornamento. Se la chiave va persa, le copie installate non possono più aggiornarsi da sole e vanno reinstallate a mano una volta.

### Backup e log di sessione (`src-tauri/src/backup.rs`, `src-tauri/src/sessionlog.rs`, `src/backup.ts`)

- Un backup contiene le connessioni (host, porte, nomi utente, percorsi dei file di chiave, jump host, forward), i preferiti e le impostazioni dell'interfaccia, i layout e gli snippet. Non contiene mai password, chiavi private, passphrase né host key note: restano nel portachiavi, su disco e in `known_hosts`. Descrive comunque i server dell'utente, quindi su Unix viene scritto con permessi `0600` (file temporaneo + rename) e l'interfaccia dice dove è stato salvato.
- I percorsi arrivano solo da finestre di dialogo native aperte dal backend; l'interfaccia non indica mai un file da leggere o scrivere.
- L'importazione avviene in due passi: il file viene letto (al massimo 16 MiB) e controllato per intero (formato e versione, ogni connessione validata come al salvataggio, id univoci, catene di jump risolvibili, al massimo 5.000 connessioni), poi un'anteprima mostra cosa contiene e quali connessioni sono nuove; nulla cambia finché l'utente non sceglie *Aggiungi* o *Sostituisci*, e l'anteprima scade dopo 10 minuti. L'interfaccia avverte di importare solo backup propri o di cui ci si fida, perché una connessione può portare port forwarding e un jump host (che agiscono comunque solo quando l'utente la apre, dopo i soliti passi di host key e autenticazione).
- *Sostituisci* elimina le password salvate delle connessioni che spariscono o di cui cambiano host, porta, utente o autenticazione, come fa la modifica di una connessione, così una password non può essere offerta a un altro server con un id riusato. *Aggiungi* non modifica mai le connessioni esistenti. La parte dell'interfaccia viene validata come le impostazioni salvate nel local storage.
- Un log di sessione è spento finché non viene avviato per un terminale, dopo un avviso che conterrà tutto ciò che compare a schermo, compresi i segreti stampati lì (l'input che il terminale non mostra, come le password a un prompt, non c'è). Su Unix viene creato con permessi `0600`, le sequenze di escape vengono rimosse così il file si può visualizzare con `cat` senza rischi, e si ferma alla fine della sessione, su richiesta o alla chiusura dell'app. Se il disco non tiene il passo, l'output in eccesso viene scartato (memoria limitata) e il log lo segnala.

### Archiviazione

- `profiles.json`, `known_hosts` e `editor.json` non contengono segreti; su Unix sono creati con permessi `0600` in una cartella `0700` e scritti in modo atomico (file temporaneo + rename), così un crash non può lasciare un file troncato.
- Gli input vengono validati a ogni salvataggio (lunghezze, caratteri di controllo, sintassi dell'host, porte), e un file di profili scritto con uno schema più recente viene rifiutato invece di essere riscritto in silenzio.

## Dipendenze

- `cargo audit` e `npm audit` girano in CI; Dependabot propone aggiornamenti ogni settimana.
- Gli avvisi esaminati e accettati sono elencati con la loro motivazione in `.cargo/audit.toml`. Attualmente: RUSTSEC-2023-0071 (attacco di timing Marvin nella crate `rsa`). Termdock non decifra mai con RSA e firma una sola challenge di autenticazione per accesso con la chiave dell'utente, quindi un attaccante remoto non ottiene un oracolo di timing utilizzabile. Preferisci chiavi Ed25519, che non usano quel codice.

## Limiti noti

- Le build di release non sono firmate finché non vengono configurati i certificati di firma nel workflow di release; verifica i download di conseguenza.
- I file scaricati sono affidabili quanto il server da cui provengono; su macOS non ricevono ancora il flag di quarantena.
- Con sudo la password attraversa la connessione SSH verso il server come se la si scrivesse nel terminale; è al sicuro quanto quel server. Ogni apertura si autentica con sudo due volte (una per canale SFTP), che i sistemi a due fattori possono mostrare come due richieste.
- Agent forwarding e X11 forwarding non sono implementati (e quindi non possono essere sfruttati).
- I segreti digitati passano per la memoria della web view prima di arrivare al backend; vengono cancellati dal campo subito dopo l'invio.
- Le risposte keyboard-interactive (una password ricordata o digitata, codici monouso) passano alla libreria SSH come stringhe normali, che lei non azzera dopo l'invio; le password inviate con il metodo `password` vengono azzerate.
