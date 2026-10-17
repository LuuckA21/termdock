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

- La chiave del server viene verificata durante lo scambio chiavi, **prima** che venga inviata qualsiasi credenziale (coperto da un test end-to-end).
- Chiave sconosciuta: all'utente vengono mostrati algoritmo e impronta SHA-256; il pulsante predefinito è *Annulla*. Solo una conferma esplicita registra la chiave.
- Chiave cambiata o marcata `@revoked`: la connessione viene rifiutata. Il dialogo non permette di forzarla; l'utente deve rimuovere esplicitamente la vecchia chiave ("Dimentica host key"), operazione che agisce solo sul `known_hosts` di Termdock. `~/.ssh/known_hosts` viene letto ma mai modificato.
- I certificati host non sono accettati (non è configurata nessuna CA fidata), quindi un certificato non può aggirare la chiave registrata.

### Credenziali (`vault.rs`, `prompts.rs`)

- Password e passphrase delle chiavi vengono salvate solo se l'utente spunta "Ricorda", e solo nel portachiavi del sistema operativo (Windows Credential Manager, macOS Keychain, Secret Service). Se non c'è un portachiavi disponibile non viene salvato nulla.
- I segreti salvati non vengono mai restituiti all'interfaccia; i segreti viaggiano solo da interfaccia a backend.
- I buffer dei segreti sono `Zeroizing` e vengono azzerati dopo l'uso; anche il contenuto dei file di chiave privata viene azzerato.
- Una password ricordata viene eliminata quando cambiano host, porta, utente o metodo di autenticazione del profilo.
- Le connessioni rapide (senza profilo salvato) non salvano mai nulla.
- Una password o passphrase ricordata che viene rifiutata è eliminata subito, invece di essere riprovata a ogni accesso (cosa che potrebbe far scattare MaxAuthTries o fail2ban).
- Le richieste all'utente scadono dopo 3 minuti; una richiesta senza risposta vale come "no".

### Applicazione desktop (Tauri 2)

- Content-Security-Policy restrittiva: script solo dal bundle dell'app, niente `eval`, nessuna origine remota, niente frame/object/worker.
- Ogni comando IPC deve essere autorizzato esplicitamente in `src-tauri/capabilities/default.json` (permessi generati per comando). Non sono inclusi plugin per file system, shell, HTTP o apertura di URL. L'interfaccia può usare il plugin degli appunti (lettura/scrittura testo); i plugin di aggiornamento e delle finestre di dialogo sono usati solo dal backend e nessuno dei loro comandi è autorizzato.
- L'interfaccia costruisce i nodi DOM solo con `textContent`; i colori dei profili sono validati (`#rrggbb`) sia nel backend sia prima dell'uso.
- Il testo mostrato nel terminale che non è output della sessione (banner del server, segnale di uscita, messaggi di errore e di stato) viene ripulito dai caratteri di controllo, così un server non può iniettare sequenze di escape attraverso di esso, nemmeno prima che l'utente si sia autenticato.
- Le shell locali si possono avviare solo dall'elenco rilevato (per id) o da un profilo salvato.
- Le preferenze dell'interfaccia (lingua, aspetto, controllo aggiornamenti) e i layout salvati stanno nel local storage della web view e non contengono segreti: un layout memorizza il suo nome e, per ogni pannello, l'id di una connessione o di una shell oppure un indirizzo di connessione rapida e il titolo del pannello; aprirlo (anche automaticamente all'avvio, se scelto) passa dagli stessi controlli di host key e autenticazione di qualsiasi connessione. Sono validate a ogni lettura; il nome del carattere è ridotto a un solo nome di famiglia prima di arrivare al CSS o a xterm.js.
- `freezePrototype` è disattivato perché rompe xterm.js; l'interfaccia non unisce oggetti non fidati, quindi la prototype pollution non ha punti di ingresso.

### Jump host e port forwarding (`ssh.rs`, `forward.rs`)

- Ogni salto di una catena ProxyJump è una sessione SSH completa: la sua host key viene verificata con `known_hosts` e si autentica con le proprie credenziali. Un bastion vede solo traffico cifrato per il salto successivo; non gli viene inoltrato nulla (niente agent forwarding).
- I forwarding configurati su un profilo usato come bastion non vengono aperti quando è solo un salto.
- I listener locali (`-L`) e SOCKS (`-D`) ascoltano su `127.0.0.1` salvo diversa indicazione nel profilo; l'editor avvisa quando si usa un indirizzo non di loopback. Come con OpenSSH, altri utenti e processi locali possono collegarsi a un listener di loopback mentre la sessione è aperta.
- Il proxy SOCKS5 implementa solo CONNECT (niente BIND/UDP), senza autenticazione, e valida i nomi richiesti. I client devono completare l'handshake entro 10 s.
- Ogni listener locale serve al massimo 256 connessioni contemporanee; in caso di errore di accept attende invece di girare a vuoto.
- Le connessioni remote (`-R`) sono accettate solo per i forwarding richiesti da Termdock; qualsiasi altro canale `forwarded-tcpip` aperto dal server viene rifiutato.
- I listener vengono chiusi alla fine della sessione del terminale.
- Le catene sono limitate a 8 salti e i cicli vengono rifiutati; un profilo usato come jump host non può essere eliminato finché è referenziato.

### Import da `~/.ssh/config` (`sshconfig.rs`)

- Il backend legge solo `~/.ssh/config`; l'interfaccia non può scegliere un altro file.
- `ProxyCommand`, `Match`, `Include`, token sconosciuti e forwarding su socket Unix vengono segnalati, mai eseguiti o indovinati.
- I profili importati passano per la stessa validazione di quelli creati a mano.

### Trasferimento file (`sftp.rs`, `src-tauri/src/sftp.rs`, `src/files.ts`)

Il server SSH non è considerato fidato: controlla nomi, elenchi, dimensioni, contenuti e messaggi di errore.

- SFTP usa nuovi canali della stessa connessione SSH del terminale: stessa host key verificata, nessuna nuova credenziale, niente salvato. I file browser terminano con la loro sessione di terminale.
- L'interfaccia non indica mai un file locale. I download sono salvati dove l'utente sceglie in una finestra di salvataggio nativa aperta dal backend; gli upload accettano solo token monouso (validi 10 minuti) per file scelti dall'utente in una finestra nativa, aperti in quel momento così da non poter essere sostituiti dopo. Le finestre hanno i titoli del sistema (l'interfaccia non può formularli) e i comandi IPC del plugin delle finestre di dialogo non sono autorizzati, quindi non possono allargare alcun accesso al file system.
- I nomi remoti sono mostrati con i caratteri di controllo, invisibili e di inversione del testo sostituiti (niente inganni tipo `evil<U+202E>gpj.exe`) e non diventano mai percorsi locali; il nome proposto nella finestra di salvataggio è ripulito da separatori, nomi riservati di Windows e caratteri invisibili. I nomi che il server invia in un'altra codifica sono elencati ma non toccati.
- I nuovi nomi (cartella, rinomina, upload) devono essere un solo componente: niente `/`, NUL, caratteri di controllo o invisibili, `.` o `..`, al massimo 255 byte.
- Gli elenchi sono letti a blocchi e si fermano a 20.000 voci o dopo 30 secondi, qualunque cosa invii il server; la libreria SFTP limita anche i pacchetti (256 KiB) e applica un timeout a ogni richiesta.
- I download accettano solo file regolari (verificato sul file aperto, quindi link a dispositivi o FIFO sono rifiutati) e leggono al massimo la dimensione indicata all'apertura. Sono scritti in un file temporaneo accanto alla destinazione e spostati al loro posto solo a trasferimento completato; un trasferimento fallito o annullato non lascia nulla. I file salvati sono privati (`0600`) su Unix; su Windows ricevono il Mark of the Web (`Zone.Identifier`), così SmartScreen li controlla prima dell'esecuzione.
- Gli upload vanno in un nome nascosto casuale creato in esclusiva con permessi `0600` (un file o un link piazzato da altri non può deviare la scrittura). Solo a trasferimento completato, e per un nome esistente solo dopo la conferma dell'utente, il file prende il posto della destinazione: in modo atomico con `posix-rename@openssh.com`, altrimenti mettendo da parte il vecchio file e ripristinandolo in caso di errore. Un file sostituito mantiene i suoi permessi e, se il server lo consente, proprietario e gruppo (hard link, ACL e attributi estesi non sono mantenuti); uno nuovo prende i permessi del file locale. Caricare su un link scrive nel file a cui punta, che deve essere un file regolare. La dimensione deve corrispondere a quella annunciata, altrimenti non si sostituisce nulla.
- L'annullamento è immediato; chiudere un file browser o la sua scheda annulla i trasferimenti e li lascia ripulire prima che la connessione si chiuda.
- L'eliminazione non è mai ricorsiva (solo file, link e cartelle vuote) e la conferma ha *Annulla* come pulsante predefinito.
- **File trascinati**: i file trascinati dal sistema sulla finestra arrivano al backend dal sistema operativo (l'interfaccia non invia mai percorsi), vengono aperti subito se sono file regolari (le cartelle sono saltate, al massimo 200 in attesa alla volta) e passati all'interfaccia come gli stessi token monouso della finestra di apertura; un rilascio che non cade su un file browser li libera. Su Linux un rilascio è accettato solo sopra un file browser; su Windows e macOS qualunque rilascio sulla finestra diventa token, liberati se manca il file browser. Gli eventi di trascinamento di Tauri comunicano comunque alla web view i percorsi trascinati (solo i nomi: non può leggere i file). La web view non apre mai da sé un file o un link rilasciato.
- **Cartella del terminale**: la cartella comunicata dalla shell (OSC 7, o un titolo come `utente@host: ~/dir`) serve solo a navigare nel file browser, con un clic o con *Segui* attivo; arriva dal server e non apre nulla in locale.
- **Modifica**: un file remoto (regolare, di testo, al massimo 5 MB, limite applicato durante la lettura qualunque dimensione dichiari il server) è copiato in una cartella temporanea privata (`0700`, file `0600`; Mark of the Web su Windows) e aperto solo nell'*editor di testo* di sistema, mai eseguito né aperto in base alla sua estensione: Notepad dal suo percorso completo su Windows, `open -t` su macOS e su Linux l'applicazione `text/plain` del desktop avviata con `gtk-launch` o, in mancanza, `gio open` su un nome `.txt`; se nessuno dei due è disponibile Termdock rifiuta e suggerisce il download. Un link è modificato nel file a cui punta. Un salvataggio è caricato quando il file ha smesso di cambiare, con la stessa sostituzione sicura degli upload. Prima di ogni caricamento la copia sul server è confrontata con il contenuto letto o salvato l'ultima volta; se è diversa, l'utente viene interpellato prima di sovrascriverla. Le copie restano fino all'uscita da Termdock (chiudere il file browser smette di sorvegliarle) e poi sono cancellate; quelle lasciate da un crash vengono rimosse a un avvio successivo quando hanno più di 7 giorni (solo quelle di questo utente).
- **I file come altro utente** avviano `sftp-server` tramite `sudo -u <utente>` su nuovi canali della stessa connessione; decide la policy sudo del server, Termdock non aggiunge privilegi. Il nome utente deve essere un semplice nome Unix (lettere, cifre, `_ . -`, che inizia con una lettera o `_`, al massimo 32 caratteri) e il comando è composto da parti fisse: si eseguono solo percorsi di `sftp-server` da un elenco interno, mai uno indicato dal server. sudo è avviato con un marcatore di prompt casuale per ogni canale, e la password è inviata **solo dopo che compare esattamente quel prompt**; se sudo non ne ha bisogno (NOPASSWD, un modulo PAM che non la chiede), non viene mai inviata. La password è chiesta in una finestra che indica di chi è la password richiesta da sudo (`%p`: l'utente di login, oppure root o l'utente di destinazione con `rootpw`/`targetpw`), passata al sudo in attesa e scartata subito dopo; non è mai salvata né registrata. Una password errata torna allo stesso sudo, che concede i suoi soliti tre tentativi. Il cambio è sempre un clic esplicito e per root serve una conferma: il titolo del terminale (`utente@host`), controllato dal server, serve solo a suggerirlo. Quando si agisce come `root` l'utente è mostrato in rosso.

### Aggiornamenti (`src-tauri/src/updates.rs`, `scripts/updater-manifest.mjs`)

- L'app cerca aggiornamenti solo all'indirizzo HTTPS fissato in `tauri.conf.json` (il `latest.json` dell'ultima release GitHub *pubblicata*; le bozze non vengono mai proposte). Le build di release rifiutano un indirizzo HTTP in chiaro (l'URL di download arriva dal manifest ed è protetto dalla firma del pacchetto qualunque sia il protocollo; i manifest di Termdock contengono solo URL GitHub https) e l'interfaccia non può cambiare indirizzo, chiave o regole sulla versione: può solo chiedere "c'è un aggiornamento?" e "installa quello trovato".
- Ogni pacchetto è verificato con minisign (Ed25519) usando la chiave pubblica inclusa nell'app, dopo il download e di nuovo subito prima dell'installazione. Un pacchetto che non supera il controllo non viene mai scritto al posto dell'app installata.
- La firma copre anche la versione per cui è stata fatta (`requireSignedVersion`): un manifest che annuncia la versione X deve puntare a un pacchetto firmato per X, quindi un manifest manomesso non può spacciare una release più vecchia, firmata regolarmente, per una nuova. Sono accettate solo versioni più recenti (niente downgrade).
- L'interfaccia scarica e installa solo dopo la conferma dell'utente (il pulsante predefinito è *Più tardi*). Anche una web view compromessa potrebbe al massimo avviare l'installazione di una release più recente e firmata regolarmente, con un riavvio. Le note di rilascio arrivano dal manifest, che non è firmato: sono mostrate come testo semplice e troncate.
- Le installazioni deb e rpm non vengono aggiornate sul posto (servirebbe root, che il plugin otterrebbe passando la password dell'utente a `sudo`); l'app si limita ad annunciare la nuova versione.
- La chiave privata esiste solo come secret dell'environment `release` del repository, utilizzabile solo dai workflow eseguiti su `main` e sui tag `v*`, e nel backup offline del proprietario. Il workflow di release si rifiuta di creare il tag se la chiave manca o non corrisponde alla chiave pubblica dell'app, e verifica ogni pacchetto di aggiornamento (installer Windows, archivio dell'app macOS, AppImage) prima di allegare `latest.json`. Le action che esegue sono bloccate su commit SHA. La chiave è per forza nell'ambiente dello step di build (il bundler di Tauri firma durante la creazione dei pacchetti), quindi gli strumenti di build e le dipendenze girano accanto a essa: le dipendenze sono bloccate (`Cargo.lock`, `package-lock.json`), controllate in CI e aggiornate tramite PR revisionate.
- Privacy: un controllo è una richiesta HTTPS a GitHub all'avvio e una volta al giorno (i download arrivano dalla CDN di GitHub; GitHub vede l'indirizzo IP); si può disattivare nelle Impostazioni.
- Rischio residuo: chi possiede la chiave privata e può pubblicare release su questo repository può distribuire un aggiornamento. Se la chiave va persa, le copie installate non possono più aggiornarsi da sole e vanno reinstallate a mano una volta.

### Archiviazione

- `profiles.json` e `known_hosts` non contengono segreti; su Unix sono creati con permessi `0600` in una cartella `0700` e scritti in modo atomico (file temporaneo + rename), così un crash non può lasciare un file troncato.
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
