# ADR 0002 — decoder XPC e supervisione R0 su macOS

Data: 6 settembre 2026. Stato: adottata per il prototipo interno 0.1.1; gate di rilascio aperto.

## Decisione

Il bundle macOS usa due servizi distinti, `it.truerenderer.prototype.decoder.0` e `.1`, ciascuno
con il solo entitlement App Sandbox. Il decoder Rust diventa una libreria riutilizzata dal
worker CLI e dai servizi XPC. Il broker nel bundle richiede XPC e non ripiega sul processo
senza sandbox in caso di errore. Il binario di sviluppo fuori da un bundle conserva il
trasporto CLI per test del core e target non ancora qualificati.

L’app usa due thread di decodifica, una coda finita di 64 descrittori con precedenza alle
anteprime complete e un writer separato per SQLite. Cambiare generazione annulla i job
obsoleti e ricicla gli isolati anche se la nuova cartella non produce richieste decodificabili.
Una coda piena differisce la
richiesta e la UI riprova; non crea thread o buffer pixel senza limite. Shutdown cancella
i job e attende i decoder prima di confermare la chiusura del servizio.

La lista dei digest ammessi è condivisa in `tr-core::corpus` e verificata sia dal broker sia
dal decoder. Non vengono ammesse immagini esterne in questo incremento.

## Autorità e protocollo

Il bootstrap XPC accetta soltanto versione/id/operazione, un riferimento Mach e due
descrittori di pipe con direzione verificata. Nessun percorso arriva al servizio. Sorgente,
controllo e raster continuano a usare il protocollo R0 bounded: controllo ≤64 KiB,
sorgente ≤32 MiB, raster ≤8.388.608 pixel fp32. La copia in memoria del broker rimane privata;
non vengono creati riferimenti Rust a regioni modificabili dal worker.

Il broker verifica il bundle del servizio e impone `identifier` e CDHash della versione
installata a ogni messaggio XPC. Il servizio verifica l’identificatore dell’app nel namespace
XPC incorporato. **Non è ancora autenticazione di un firmatario Developer ID.** La firma ad
hoc non prova l’identità del distributore. In questa sandbox leggere il pacchetto padre per
derivarne il CDHash ha restituito `EPERM`; anche la verifica dinamica dal messaggio ha trovato
quel limite. Non sono state aggiunte eccezioni di filesystem. Questo punto rimane aperto
prima di una release e dell’abilitazione degli archivi esterni.

## Timeout, memoria e limiti osservati

Il servizio concede al broker un riferimento al proprio task. Il broker lo confronta con
il peer XPC autenticato e acquisisce l’audit token tramite `task_info`: il token non proviene
da campi di risposta dichiarati dal worker. Misura RSS e physical footprint attraverso il
riferimento del kernel. La soglia del prototipo è 384 MiB per isolato, campionata ogni 25 ms
durante i job e nell’attesa della coda; è supervisione con terminazione, **non un tetto rigido**.
Anche startup e I/O rientrano nel timeout di 12 s; una connessione XPC in avvio può completare
l’attesa bounded prima di osservare una cancellazione della UI.

`task_get_special_port(TASK_KERNEL_PORT)` ha restituito 53 sul Mac di prova. Il trasferimento
diretto del riferimento `mach_task_self()` consente misura e sospensione. `task_terminate`
restituisce 5 per questo processo BSD: non viene usato per fermare i decoder.

La terminazione usa **`proc_signal_with_audittoken` di libproc**, risolta dinamicamente e
invocata soltanto con il token del task verificato. È un’interfaccia SPI presente nell’SDK,
non una garanzia di compatibilità pubblica per tutte le versioni macOS. Il prototipo la prova
su macOS 26.6.2 arm64; la scelta deve essere riesaminata e qualificata per il rilascio.
Non esiste fallback a `kill(pid)`. Dopo SIGKILL viene verificata la scomparsa del task entro
500 ms; un errore rende lo slot inutilizzabile fino al riavvio, evitando nuovi processi
mentre la terminazione rimane incerta.

Il servizio imposta inoltre limiti hard/soft propri: nessun figlio, nessun core dump,
128 descrittori e 60 s di CPU per vita del processo. Questi limiti non sono limiti di heap,
mmap o numero di thread. Il riciclo avviene anche dopo 32 job o errore. Il riavvio XPC dopo
una terminazione può subire un ritardo di circa 10 s di launchd, incluso nelle prove.

## Prove e limiti del gate

`--verify-xpc` prova due PID distinti, uguaglianza dei pixel, decoder sospeso interrotto,
sopravvivenza dell’altro isolato, nuovo processo e intervento della soglia memoria.
`--verify-xpc-growth` richiede un servizio di fault injection compilato a parte sotto `var/`:
alloca 512 MiB a passi nel singolo comando di prova, ignora gli errori di disconnessione XPC e misura il superamento
osservato della soglia. Quel ramo non viene compilato nei servizi del bundle utente.

I report reali sono in `reports/xpc-*.json`. Il test di soglia abbassata prova l’intervento del
controllore, mentre il test di crescita misura una traiettoria concreta: nessuno dei due
dimostra un limite universale di overshoot. Restano firma di distribuzione, corpus ostile
esteso, confini/allocazioni del bootstrap XPC, quote end-to-end, comportamento sotto pressione
del sistema, Windows reale, display e accessibilità. R0 e il gate completo sandbox restano
aperti; ogni render rimane Anteprima.

Sul Mac di prova: due PID distinti, timeout di un worker sospeso contenuto in 216 ms nel test
da 200 ms, nuovo processo e 12 immagini del corpus passati. La crescita ha raggiunto
408.731.648 byte osservati, 6.078.464 byte oltre soglia (circa 5,8 MiB); il tempo registrato
di 1.099 ms parte dall’avvio dell’allocazione, non dall’attraversamento della soglia.
Un test con worker reale verifica inoltre il riciclo a coda vuota al cambio generazione.

## Fonti e riproduzione

- Header dell’SDK macOS: `xpc/xpc.h`, `xpc/connection.h`, `mach/task_info.h`, `libproc.h`;
  il commento di `libproc.h` qualifica le interfacce come private e soggette a cambiamenti.
- [Apple: API e struttura dei servizi XPC](https://developer.apple.com/library/archive/documentation/MacOSX/Conceptual/BPSystemStartup/Chapters/CreatingXPCServices.html).
- [Apple XNU: implementazione di task_terminate](https://github.com/apple-oss-distributions/xnu/blob/main/osfmk/kern/task.c).
- [Apple XNU: segnali tramite audit token](https://github.com/apple-oss-distributions/xnu/blob/main/libsyscall/wrappers/libproc/libproc.c).

`scripts/verify.sh` verifica il workspace. `scripts/build-macos.sh` prepara il bundle normale.
`python3 scripts/test-xpc-integration.py` esegue le prove native e registra anche le impronte
del pacchetto, compreso il rifiuto del comando di fault injection.
Per la variante avversaria, dopo la build debug, usare
`python3 scripts/package-macos.py --profile debug --fault-injection --bundle var/xpc-fault/TrueRenderer.app`,
poi `python3 scripts/test-xpc-integration.py --fault-bundle var/xpc-fault/TrueRenderer.app`.
