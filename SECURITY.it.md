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
- Ogni comando IPC deve essere autorizzato esplicitamente in `src-tauri/capabilities/default.json` (permessi generati per comando). Non sono inclusi plugin per file system, shell, HTTP o apertura di URL; l'unico plugin è quello degli appunti (lettura/scrittura testo).
- L'interfaccia costruisce i nodi DOM solo con `textContent`; i colori dei profili sono validati (`#rrggbb`) sia nel backend sia prima dell'uso.
- Il testo mostrato nel terminale che non è output della sessione (banner del server, segnale di uscita, messaggi di errore e di stato) viene ripulito dai caratteri di controllo, così un server non può iniettare sequenze di escape attraverso di esso, nemmeno prima che l'utente si sia autenticato.
- Le shell locali si possono avviare solo dall'elenco rilevato (per id) o da un profilo salvato.
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

### Archiviazione

- `profiles.json` e `known_hosts` non contengono segreti; su Unix sono creati con permessi `0600` in una cartella `0700` e scritti in modo atomico (file temporaneo + rename), così un crash non può lasciare un file troncato.
- Gli input vengono validati a ogni salvataggio (lunghezze, caratteri di controllo, sintassi dell'host, porte), e un file di profili scritto con uno schema più recente viene rifiutato invece di essere riscritto in silenzio.

## Dipendenze

- `cargo audit` e `npm audit` girano in CI; Dependabot propone aggiornamenti ogni settimana.
- Gli avvisi esaminati e accettati sono elencati con la loro motivazione in `.cargo/audit.toml`. Attualmente: RUSTSEC-2023-0071 (attacco di timing Marvin nella crate `rsa`). Termdock non decifra mai con RSA e firma una sola challenge di autenticazione per accesso con la chiave dell'utente, quindi un attaccante remoto non ottiene un oracolo di timing utilizzabile. Preferisci chiavi Ed25519, che non usano quel codice.

## Limiti noti

- Le build di release non sono firmate finché non vengono configurati i certificati di firma nel workflow di release; verifica i download di conseguenza.
- Agent forwarding e X11 forwarding non sono implementati (e quindi non possono essere sfruttati).
- I segreti digitati passano per la memoria della web view prima di arrivare al backend; vengono cancellati dal campo subito dopo l'invio.
