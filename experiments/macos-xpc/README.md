# Laboratorio XPC macOS — R0

Questa prova è separata dall’app e non decodifica immagini. Usa un servizio XPC incorporato,
firmato ad hoc, con il solo entitlement `com.apple.security.app-sandbox`. Il processo host
non è sandboxato. I percorsi nel protocollo sono esclusivamente bersagli sintetici dei test
negativi: **non sono il contratto IPC del decoder**.

## Riproduzione

Richiede macOS e Command Line Tools, nella normale sessione desktop:

```sh
python3 scripts/build-xpc-probe.py
python3 scripts/test-xpc-probe.py
python3 scripts/build-xpc-probe.py --limited
python3 scripts/test-xpc-probe.py --limited
```

Le build sono in `var/sandbox-probe/`, i report in `reports/xpc-sandbox*-macos.json`.
Il test crea solo fixture proprie sotto `var/`, apre un listener su `127.0.0.1`, passa un
descrittore `O_RDONLY` via XPC e tenta le operazioni dal servizio. Il controllo preliminare
conferma che file e listener siano accessibili all’host. Le fixture vengono rimosse al termine;
gli originali e la libreria dell’app non vengono toccati. App Sandbox può creare i propri
container gestiti da macOS in `~/Library/Containers/`.

Lo script verifica firma ed entitlement prima dell’esperimento; l’host valida versione/request ID
e copia un piccolo albero di primitive nella risposta. Ogni risposta ha un timeout di 15 s;
il wrapper ferma il solo host dopo 100 s. Il servizio chiude richieste con versione, campi,
tipi o conteggio non ammessi. Le identità XPC sono controllate per bundle identifier: questa
firma ad hoc di laboratorio **non qualifica l’autenticazione del firmatario di una release**.

## Risultati del 6 settembre 2026

Su macOS 26.6.2 arm64, entrambe le varianti hanno superato i controlli del laboratorio:

| Prova | Solo App Sandbox | App Sandbox + limite processi |
|---|---|---|
| Apertura di file esterno in lettura/scrittura | `EPERM` | `EPERM` |
| Creazione di file fuori dal container | `EPERM` | `EPERM` |
| Connessione e ascolto TCP loopback | `EPERM` | `EPERM` |
| Lettura del descrittore concesso | Contenuto esatto | Contenuto esatto |
| Scrittura sul descrittore `O_RDONLY` | `EBADF` | `EBADF` |
| `posix_spawn` di `/usr/bin/true` | Consentito, uscita 0 | Bloccato, `EAGAIN` |
| Aumento del limite processi da parte del servizio | Non provato | `EPERM` |
| Due connessioni allo stesso servizio | Stesso PID | Stesso PID |
| Versione sconosciuta | Connessione chiusa; altra connessione viva | Uguale |
| Uscita improvvisa e richiesta successiva | Host vivo, nuovo PID | Uguale |

La variante `--limited` imposta `RLIMIT_NPROC` soft/hard a zero nel `main` del solo servizio,
prima di accettare messaggi; un errore impedisce l’avvio. Non modifica limiti della shell,
dell’host o del sistema. Nel JSON, `child_spawn.denied` indica specificamente un diniego
`EPERM/EACCES`; per il blocco da quota `EAGAIN` si legge `nproc_policy_checks_passed`.

Sono stati toccati 16 MiB di memoria: RSS circa 8,2 MB prima e 25,0 MB dopo nella variante
limitata. È una singola osservazione, non un tetto imposto dal kernel né un test di overshoot.
Le prove complete durano circa 10 s includendo il riavvio XPC: non sono benchmark di decode
o di startup a freddo. Due connessioni non equivalgono a due isolati indipendenti.

## Decisione e lavoro necessario

La primitiva XPC/App Sandbox è concretamente utilizzabile per il confine macOS. La prova
fornisce anche una restrizione candidata per i figli. Il gate completo rimane **aperto** e
`full_sandbox_gate_passed` resta `false` in entrambi i report. Il decoder dell’app mantiene
la propria allowlist. L’incremento 0.1.1 ha ora integrato due servizi XPC distinti nel bundle;
il trasporto su pipe rimane nei binari di sviluppo fuori bundle.

Il decoder senza percorsi, la copia privata, due PID distinti e le prove di timeout/memoria
sono descritti in `docs/adr/0002-xpc-decoder-r0.md` e nei report `xpc-integration-macos.json`
e `xpc-memory-growth-macos.json`. Restano da qualificare il firmatario, l’API di terminazione,
le quote end-to-end, gli handle residui, il corpus avversario esteso e Windows reale.
Il limite processi non limita heap/mmap/thread e la sandbox conserva l’autorità sul proprio
container e sulle risorse di sistema consentite dal profilo.

Il laboratorio aggiuntivo del riferimento kernel è in `control-host.m` e `control-worker.m`.
Si costruisce con `python3 scripts/build-xpc-control-probe.py` e si esegue con
`var/control-probe/ControlProbe.app/Contents/MacOS/ControlProbe`; stampa un JSON su stdout.
`reports/xpc-control-macos.json` conserva la prova di misura, sospensione e segnale tramite
audit token. Il risultato 5 di `task_terminate` è una limitazione osservata, non un metodo
usato per terminare i decoder dell’app. Compatibilità della SPI ancora da qualificare.

## Riferimenti

- [Apple: struttura e ciclo di vita dei servizi XPC](https://developer.apple.com/library/archive/documentation/MacOSX/Conceptual/BPSystemStartup/Chapters/CreatingXPCServices.html).
- [Apple: App Sandbox](https://developer.apple.com/documentation/security/protecting-user-data-with-app-sandbox).
- Header dell’SDK macOS locale: `xpc/connection.h`, `xpc/xpc.h`, `sys/resource.h`;
  manuale locale `setrlimit(2)` per la semantica dei limiti per processo.

Le fonti descrivono il meccanismo; i risultati sopra provengono dalle esecuzioni nei report.
