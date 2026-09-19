# Isolamento dei decoder e formati esterni

Contratti di esecuzione macOS/Windows, ammissione degli input esterni e vincoli di pubblicazione. Date e raccordi distinguono le baseline storiche. Stato e verifiche correnti sono in [STATO.md](../STATO.md).

- [XPC macOS — ADR 0002](#adr-0002)
- [File esterni e pubblicazione — ADR 0004](#adr-0004)
- [LPAC Windows e contratto decoder — ADR 0009](#adr-0009)

<a id="adr-0002"></a>

## ADR 0002 — decoder XPC e supervisione R0 su macOS

Data: 6 settembre 2026. Stato: adottata per il prototipo interno 0.1.1; gate di rilascio aperto.

### Raccordo con la 0.1.5 — 9 settembre 2026

Le quote 32 MiB/8 Mi pixel, la sola allowlist e le misure sotto descrivono la 0.1.1. [ADR 0004](isolamento-decoder-e-formati.md#adr-0004) estende il bundle macOS agli esterni come Anteprima (256 MiB/64 Mi pixel, supervisione esterni 2 GiB/45 s); i binari su pipe conservano la allowlist. [ADR 0006](progetto-anteprime-cache-prestazioni.md#adr-0006) aggiunge budget globale stimato, code indipendenti e P0–P6. La revoca del dominio continua a riciclare i servizi; l’abbandono di una vista interrompe soltanto i passaggi cancellabili e lascia terminare le chiamate native già iniziate. Firma di release e gate sandbox completi restano aperti.

### Correzione stack LibRaw — 15 settembre 2026

La prova dei motori su D750 ha riprodotto `Thread stack size exceeded` nel probe LibRaw eseguito dal thread dispatch XPC. I quattro oggetti LibRaw locali del bridge sono ora posseduti sullo heap tramite `std::unique_ptr`: stessa durata ed eccezioni gestite, senza occupare lo stack limitato del thread. Passati i 120 sviluppi dei 30 NEF autorizzati sui quattro motori dopo la correzione. Non cambiano ricette, capability, entitlement o garanzie di rilascio. Protocolli e limiti nella [campagna immagini grandi/RAW](../STATO.md#campagne-raw-concluse).

### Decisione

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

### Autorità e protocollo

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

### Timeout, memoria e limiti osservati

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

### Prove e limiti del gate

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

### Fonti e riproduzione

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

<a id="adr-0004"></a>

## ADR 0004 — formati esterni macOS e repository pubblico proprietario

Data: 7 settembre 2026. Stato: implementato nella 0.1.3, qualifica limitata alle prove registrate.

### Raccordo con la 0.1.5 — 9 settembre 2026

Le quote fisse di cache e l’assenza di budget globale sotto descrivono la 0.1.3. La 0.1.5 applica le impostazioni e l’ammissione per fasi di [ADR 0006](progetto-anteprime-cache-prestazioni.md#adr-0006). Oltre al DNG sintetico sono stati verificati 30 NEF Nikon D750 alla risoluzione nativa, inclusi due accessi simultanei entro 2 GiB. Il riconoscimento RAW nei contenitori TIFF è stato corretto. I report RAW pubblicati contengono risultati numerici; fotografie e copie di lavoro restano private. Le altre fotocamere e il gate sandbox completo rimangono da qualificare. [Ultime verifiche](../reports/preview-navigation-continuation-macos.json).

### Richiesta e modifica dello scopo

Il titolare ha richiesto un repository GitHub pubblico, README, licenza **proprietaria** e apertura effettiva di JPEG, PNG, RAW, TIFF e altri formati. La 0.1.2 ammetteva soltanto i dodici PNG del corpus. Questo incremento anticipa una parte dei formati R1/R3 attraverso i decoder Apple, conservando la modalità **Anteprima**. È una modifica esplicita del perimetro di sviluppo rispetto all'attesa del gate R0 completo nella proposta originale; non chiude quel gate e non sostituisce la futura matrice RAW/ICC multipiattaforma.

### Confine di esecuzione

Solo il bundle macOS abilita i file esterni: due servizi XPC separati con App Sandbox e senza entitlement di rete o accesso generale ai file. Il broker legge gli originali in sola lettura, produce una copia privata, ne calcola SHA-256 e trasferisce soltanto i byte nel protocollo bounded. Nessun percorso viene inviato al decoder. Il worker su pipe continua a rifiutare tutto ciò che non appartiene al corpus compilato. Un errore XPC non attiva un decoder alternativo nel processo UI.

Il riconoscimento usa la firma del contenuto, oltre all'elenco di estensioni per la scansione. Un file rifiutato con risposta IPC valida lascia il servizio disponibile per il job successivo; errori di framing, crash, cancellazione e timeout comportano il riciclo. Le generazioni di cartella conservano la revoca del decoder inattivo. Il digest mostrato nell'ispezione appartiene ai byte decodificati; il token stat dell'indice resta una osservazione best-effort, non una revisione coerente garantita sotto writer concorrente.

Restano i limiti di ADR 0002: il broker verifica il CDHash del servizio, mentre il servizio verifica l'identificatore dell'host; non è il requisito reciproco di un firmatario di release. Firma ad hoc, libproc SPI/audit token, ritardo di launchd e suite avversaria incompleta mantengono aperto il gate di rilascio.

### Decoder e colore

- JPEG, PNG, TIFF, GIF, BMP, HEIC/HEIF e WebP: ImageIO crea il bitmap, Core Image/ColorSync lo converte in Rec.2020 lineare esteso fp32 con alpha premoltiplicata. L'orientamento EXIF viene applicato una volta. PNG e TIFF a 16 bit non transitano in un buffer sRGB8. Per file multipagina/animati viene mostrata solo la prima pagina o il primo fotogramma, dichiarandolo nella provenienza.
- RAW: `CIRAWFilter.outputImage`, scala 1 e draft disattivato, con dimensioni native controllate. Nessun fallback alla preview JPEG. Ricetta **TR-linear-v1**: WB e baseline exposure dai metadati/decoder Apple; esposizione aggiunta zero, boost/gamut mapping/lens correction disattivati, sharpening, contrast/detail, noise reduction, moiré e local tone mapping a zero dove supportati. Demosaicing e trasformazioni indispensabili restano quelli Apple; non si promette una ricetta identica a LibRaw o ad altri sviluppatori.
- Provenienza: decoder/versione OS o RAW, ricetta, profilo rilevato/assunto, profondità disponibile, orientamento, SHA-256 e stadi di presentazione. Profondità RAW non riportata = non dichiarata, mai inventata.
- Il corpus continua sul percorso analitico `image/png` precedente. Il sistema di campionamento di ADR 0003 resta condiviso da tutte le viste. Non si aggiunge un ricampionamento sRGB o della UI.

Il supporto RAW è condizionato al modello e al decoder installato: elencare NEF/CR2/CR3/ARW/RAF ecc. non certifica ogni variante. La prova positiva riguarda un DNG Bayer RGGB sintetico 1024×768 senza preview incorporata. Non è una qualifica di fotocamere reali. Little CMS, ICC v2/v4 e CMYK/YCCK completi, precedenza metadati e monitor restano lavoro R1; LibRaw/matrice CFA e gigapixel restano R3. HEIC/WebP anticipano un sottoinsieme che la proposta collocava dopo v1.

### Quote del prototipo

| Risorsa | Quota corrente |
|---|---|
| File sorgente | 256 MiB, controllata prima e durante la lettura |
| Raster sorgente orientato | 67.108.864 pixel, massimo 32.768 per lato nel decoder nativo |
| Pagine/fotogrammi | massimo 256 nel contenitore; si decodifica il primo |
| Render fisico di una vista | 8.388.608 pixel |
| Worker esterno | supervisione a 2 GiB ogni 25 ms; timeout assoluto 45 s |
| Worker corpus | supervisione a 384 MiB; timeout 12 s |
| Cache sorgenti/piramidi | massimo 64 voci e 1.536 MiB |
| Cache presentazione | massimo 64 voci e 128 MiB |

Il campionamento memoria non è un tetto rigido. Cache, job in corso, copie, staging e GPU non hanno ancora un budget globale coordinato. La piramide prende possesso del raster, evitando la precedente copia integrale. Non c'è una promessa di immagini gigapixel: sorgenti troppo grandi vengono rifiutate.

### Evidenze e riproduzione

`scripts/generate-format-fixtures.py` crea i file locali usando formule proprie, ImageIO e `cwebp` per la sola fixture WebP. Nessuna fotografia viene scaricata. `--verify-formats` passa 19 casi: otto famiglie di formato incluso DNG, PNG/TIFF 16 bit, EXIF 1–8 e JPEG 12 MP. Confronta dimensioni e quadranti con un riferimento sRGB analitico; per RAW verifica output completo non costante senza preview. Sei controlli aggiuntivi coprono tre input malformati con recupero sullo stesso PID, firma contro estensione, quota sorgente e osservazione obsoleta. La precisione 16 bit/alpha ha anche una prova Rust separata. Questi controlli non equivalgono a fuzzing o a una matrice fotografica completa.

`--formats-smoke` verifica griglia, DNG completo e crop 1:1 del JPEG 12 MP con tre screenshot. I report sono `reports/formats-macos.json` e `reports/formats-smoke-macos.json`. Le prove di ricampionamento del corpus e la suite XPC vengono ripetute sul pacchetto finale; esiti aggiornati in `STATO.md` (registro e collegamenti ai rapporti JSON).

### Repository e licenza

Repository: [antoniodicoratoinfodev/TrueRenderer](https://github.com/antoniodicoratoinfodev/TrueRenderer). Il file `LICENSE` riserva i diritti sul materiale originale ad Antonio Dicorato, preserva i diritti previsti dai termini GitHub e quelli delle dipendenze. La consultabilità pubblica non concede una licenza open source. Nessun database, backup, immagine personale o toolchain viene pubblicato. I report pubblici e i loro screenshot riguardano solo contenuti sintetici del progetto.

Fonti primarie: [Apple CIRAWFilter](https://developer.apple.com/documentation/coreimage/cirawfilter), [dimensioni native RAW](https://developer.apple.com/documentation/coreimage/cirawfilter/nativesize), [ImageIO](https://developer.apple.com/documentation/imageio), [termini GitHub, contenuti degli utenti](https://docs.github.com/en/site-policy/github-terms/github-terms-of-service#d-user-generated-content). Le API sono state controllate anche negli header dell'SDK locale; le fonti descrivono i contratti, i report misurano l'implementazione.

<a id="adr-0009"></a>

## ADR 0009 — confinamento del worker Windows dopo l'audit

Data: 13 settembre 2026. Stato: implementato e verificato localmente su Windows, [rapporto](../reports/pre-main-fixes-windows.json); nessuna qualifica di rilascio o dei gate R0–R4.

### Problema e decisione

L'audit pre-main ha dimostrato che rimuovere i privilegi dal token e applicare un Job Object non impediva al decoder di leggere e scrivere file privati non concessi. Il nuovo avvio Windows richiede un **Less Privileged AppContainer (LPAC) senza capacità**, oltre al token ristretto, al Job Object e alla lista esplicita degli handle ereditati. Il worker interroga il proprio token per LPAC/AppContainer e zero capacità, e verifica i limiti effettivi del Job prima di abilitare input esterni. Un fallimento dell'avvio o della verifica chiude il percorso esterno.

Il Job ammette un solo processo, limita memoria impegnata per processo e complessiva e termina i processi alla chiusura. Il broker ricrea il worker quando cambia la fiducia della sorgente: corpus a 384 MiB, esterni a 2 GiB; la quota non resta quella del primo lavoro.

### Autorità concesse e durata

Ogni avvio crea un profilo AppContainer univoco con l'API Windows e una copia privata dell'eseguibile. Le ACL aggiungono soltanto lettura/esecuzione sul codice e sulla sua cartella. Un handle aperto ne impedisce modifica e cancellazione durante l'uso. Nessuna ACL viene aggiunta alle fotografie, alla cartella d'installazione o ai dati del catalogo. Gli input arrivano come byte sulle pipe; il codice non riceve un handle al file originale.

Il percorso dell'applicazione è passato esplicitamente a Windows in UTF-16. Il runtime C/C++ è collegato staticamente per x86-64 MSVC; rimangono le DLL di sistema. L'ambiente proviene da `CreateEnvironmentBlock` senza ereditarietà ed è filtrato alle sole variabili di percorso di sistema/profilo. PATH, opzioni shell e variabili arbitrarie del processo host sono escluse. Windows reindirizza le cartelle temporanee al profilo isolato, che può contenere dati temporanei propri.

Alla terminazione vengono chiusi processo/handle, rimossa la copia privata e richiesto a Windows di cancellare il profilo creato da quell'istanza. Un arresto brusco dell'host può lasciare residui temporanei: questa non è una garanzia di cancellazione sicura. Non vengono riutilizzati o cancellati profili preesistenti, né puliti dati durevoli dell'utente.

### Prove e limiti

La verifica LPAC legge `WIN://NOALLAPPPKG` tramite `TokenSecurityAttributes`: buffer limitato a 64 KiB, versione/tipo/numero di valori controllati e ogni puntatore verificato entro l'allocazione restituita dal kernel. La query scalare `TokenIsLessPrivilegedAppContainer` restituisce parametro/classe non validi su questo host; il controllo alternativo di membership non distingue le due modalità e non viene usato come gate. Il confronto negativo con AppContainer ordinario restituisce attributo falso; LPAC restituisce vero. Riferimento primario per questa semantica: [Google Project Zero, proprietà LowPrivilegeAppContainer](https://github.com/googleprojectzero/sandbox-attacksurface-analysis-tools/blob/main/NtCoreLib/NtToken.cs).

La regressione riproducibile è `scripts/cargo-local.sh run --offline -p tr-platform --example windows-isolation-probe`. Su Windows va eseguita fuori dalla sandbox del terminale. Usa una sentinella temporanea con percorso assoluto, controlli positivi nell'host, pipe, lettura del codice, tentativo di scrittura nella cartella del codice e tentativo di creare un secondo processo.

Nella prova locale lettura e apertura in scrittura della sentinella, e creazione di un file accanto al codice, restituiscono Windows 5 (accesso negato); lettura del codice e pipe funzionano; il secondo processo è rifiutato. Il controllo host accede alla sentinella e al listener loopback. Nel worker LPAC Winsock non si inizializza (10107 nella prova osservata): **non è una misura di WSAEACCES sulla connessione**, né una prova esaustiva delle interfacce di rete. La prima variante AppContainer ordinaria permetteva il loopback su questo PC, motivo per cui non è stata mantenuta. Non sono state cambiate regole firewall o esenzioni del PC.

LPAC esclude l'autorità generica di All Application Packages. Le risorse di sistema con ACL compatibili, il profilo temporaneo proprio e gli handle concessi rimangono accessibili. Queste prove non qualificano attacchi al kernel, tutti gli oggetti IPC, tutti i driver, altri Windows o un ambiente di installazione pulito. Il nome `kernel_enforced` nell'API continua a indicare il tetto di memoria del Job, non una garanzia universale di sandbox.

macOS mantiene il proprio percorso XPC/App Sandbox. Il titolare ha rinviato esplicitamente la nuova verifica nativa al Mac; le prove Windows non la sostituiscono. Il primo port è riassunto in [STATO.md](../STATO.md#prototipo-e-port); questa decisione descrive il confine successivo dell'esperimento autorizzato.

Fonti primarie: [Microsoft: avvio AppContainer e LPAC](https://learn.microsoft.com/en-us/windows/win32/secauthz/implementing-an-appcontainer), [attributi di creazione](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-updateprocthreadattribute), [ambiente del token](https://learn.microsoft.com/en-us/windows/win32/api/userenv/nf-userenv-createenvironmentblock).

### Contratto decoder e avvio conservato dal primo port

`tr_core::decoder::Decoder` espone `name`, `probe` e `decode`; identità e provenienza devono essere coerenti con ricetta e cache. I raster condividono lo spazio lineare Rec.2020 fp32 premoltiplicato. `Trust::Controlled` richiede il digest nel corpus compilato; soltanto il trasporto isolato può concedere `External`, senza fallback al decoder del corpus. La provenienza distingue colore dichiarato e applicato, spazio assunto e sole primarie assunte: dichiarazioni non supportate devono essere rifiutate, non ignorate. Le regole PNG iniziali del port sono superate dalle correzioni gamma/cICP registrate in STATO.md.

Le allocazioni passate a `UpdateProcThreadAttribute` (Job e lista degli handle) restano vive fino alla creazione del processo: l'API conserva puntatori. Il percorso eseguibile usa UTF-16 senza conversioni lossy. CIRAWFilter e LibRaw non sono riferimenti bit-exact reciproci; la tolleranza GPU/CPU non qualifica il confronto fra sviluppatori RAW. Restano da qualificare ICC, percorsi lunghi, pressione memoria e installazione pulita.
