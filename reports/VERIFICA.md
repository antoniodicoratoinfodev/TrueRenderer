# TrueRenderer — verifica del prototipo R0

Registro storico delle esecuzioni. Codice e correzioni pubblicati in `67146e3` il 14 settembre 2026; le note «nessun commit» fotografano il momento delle prove. Gli audit negativi restano conservati accanto alle correzioni successive. Ogni risultato vale per sorgenti e binari identificati dal proprio rapporto; la revisione dei Markdown non costituisce una nuova prova applicativa. Stato corrente in [avanzamento](../docs/avanzamento.md), navigazione nell'[indice](../docs/README.md).

## Immagini grandi, pressione e RAW Mac — 15 settembre 2026

**Funzionalità verificata, gate memoria fisica aperto.** 240 azioni PNG 12/24/45 MP, 20 con pressione renderer iniettata, 40 RAW; tutti i controlli 1:1 esatti. 120 sviluppi su 30 D750 nei quattro motori dopo la correzione dello stack LibRaw/XPC, originali invariati; nove confronti centrali registrati e 32 Bayer sintetici. Passati invalidazione 45 MP, cambio motore UI/pixel, 75 test Rust, fmt/Clippy, release e XPC.

A 4 GiB configurati, i quattro casi 45 MP Full superano quota nella somma RSS: massimo **4.816.601.088 byte**, footprint **4.296.512.936**. La pressione renderer a 1536 MiB passa entro quota campionata ma con copertura transitoria parziale; il rifiuto a 512 MiB restituisce i crediti. Non sono pressione OS reale, decode RAW ridotto, qualifica colore o p95/p99.

[Riepilogo, hash e limiti](large-pressure-raw-macos.json), campagne [immagini grandi](large-navigation-macos.json), [pressione](renderer-pressure-macos.json), [viewer RAW](raw-navigation-macos.json), [protocollo](../docs/verifica-grandi-raw-macos.md). App aggiornata `dist/TrueRenderer-restyle.app`, precedente `var/large-pressure-raw/before.app`. Fotografie e ritagli restano privati; screenshot del layout ancora rappresentativi.

## Copertura del ritorno a Fit — 15 settembre 2026

Conservato un frame più ampio compatibile entro quota; corregge anche il caso Full da SSD con provider distinti per la stessa revisione. Prima della correzione il controllo completo falliva (minimo storico 40,5%); anche la prima implementazione ha fallito su Full/SSD e l'evidenza negativa è conservata.

Release finale: **24 processi, 240 azioni, 1.553 ridisegni con copertura completa**, 993 riproiezioni, 287 usi della riserva ampia; 24 controlli 1:1 esatti, altri pixel entro un livello sRGB8, zero fallback. Picco facoltativo: un frame, 38.263.752 byte globali / 12.754.584 GPU contabilizzati. Fmt, Clippy, build release, 46 test desktop e 5 renderer passati; sei integrazioni desktop non ripetute. Invalidazione sorgente, recupero grafico e arresto alla seconda perdita, firma/XPC passati sullo stesso binario consegnato in `dist/TrueRenderer-restyle.app`.

[Riepilogo e hash](fit-coverage-macos.json), tracce [Standard](fit-coverage-standard-macos.json) e [Full](fit-coverage-full-macos.json). Prove sul corpus piccolo e sui draw UI, non continuità di ogni refresh, RAW ridotti, pressione fisica o p95/p99. La riserva può mancare o essere espulsa; nessuna garanzia universale né tetto RSS. Screenshot del README ancora rappresentativi: nessun cambiamento al layout in questo incremento.

## Transizioni del viewer — 15 settembre 2026

Estesa la traccia con zoom 300%, pan deterministico, Fit, override Standard/Full, 1:1 e ritorno Standard/Fit. Il presenter diagnostico distingue draw esatti, riproiettati e assenti; zero copertura della stessa sorgente fa fallire la prova. Regressione separata su sovrapposizione, assenza e lane di revisione diversa. Passati fmt/Clippy workspace, build debug/release, **46 test desktop e 2 renderer**; 6 integrazioni desktop ignorate. Firma e XPC passati sul bundle diagnostico `var/navigation-transitions/TrueRenderer-release.app`.

Release: **24 processi, 240 azioni**, tre ripetizioni CPU/GPU per qualità iniziale Standard/Full e stato cache. Tutti i pixel finali entro un livello sRGB8; **24 controlli fisici 1:1 esatti**, zero fallback. Durante le transizioni: **1.568 ridisegni, 1.007 riproiezioni, zero draw completamente vuoti**; copertura provvisoria minima **40,5%** al ritorno dal ritaglio a Fit. Questo incremento osserva il comportamento esistente e non rende completa la copertura. [Riepilogo e hash](navigation-transitions-macos.json), campioni [Standard](navigation-transitions-standard-macos.json) e [Full](navigation-transitions-full-macos.json).

Le osservazioni riguardano i draw dei passaggi UI e i pixel degli endpoint, non ogni refresh del display. Azioni sintetiche, due PNG sotto 2048 pixel, cache OS non svuotata e nessuna qualifica RAW ridotta/pressione/p95/p99. Resta aperto mantenere una copertura più ampia della stessa revisione entro quota; non usare il vecchio ritaglio per inventare regioni mancanti. Le copie ordinarie in `dist/`, le qualifiche Windows/release e i limiti LibRaw restano invariati.

## Probe comando→superficie — 15 settembre 2026

Implementati `--navigation-smoke` e orchestratore `scripts/test-navigation-surface.py`: due PNG generati, tre selezioni con ritorno, CPU/GPU, Standard/Full. Nessuna richiesta immagine precede il primo comando; una nuova istanza verifica il riuso SSD e il ritorno successivo verifica RAM. Le catture richiedono il raster esatto corrente e vengono confrontate con CPU dopo il timestamp del readback, entro un livello sRGB8.

Passati fmt/Clippy workspace, build debug/release, **46 test desktop** (6 integrazioni ignorate in questa corsa), regressione di geometria/pixel, controllo dei quantili e soppressione dei piccoli campioni. Prova debug: 4 processi; release: **24 processi e 72 selezioni**, errore massimo un livello, zero fallback, stessa geometria CPU/GPU. Firma e XPC passati sul bundle diagnostico `var/navigation-probe/TrueRenderer-release.app`. [Rapporto e hash](navigation-surface-macos.json), campioni [Standard](navigation-surface-standard-macos.json) e [Full](navigation-surface-full-macos.json).

Questa è la validazione funzionale del probe: il readback include cattura GPU e consegna eventi, con repaint a 10 ms. Non è un timestamp del monitor né una qualifica p95/p99; tre ripetizioni per scenario non bastano. Cache OS non svuotata, carico della macchina non isolato, nessun confronto con la baseline storica. Il piccolo corpus non esercita riduzione RAW, zoom, pan, cambio qualità o pressione. Le normali copie `dist/` restano quelle dell'incremento cache; gate R0–R4 e avvisi LibRaw invariati.

## Cache e suite Mac — 15 settembre 2026

Corretto il rilievo sugli hard link: rimozione e sostituzione ora ricontrollano i collegamenti; lettura e riuso di record validi rimangono disponibili. Passati 25 test cache, inclusi due nuovi casi e il fallimento storico rafforzato. Eliminati i warning Rust Mac compilando il decoder portabile per Windows e test. Suite completa: **106 test ordinari + 10 integrazioni**, fmt/Clippy, build debug/release, 2 Python, 6 IPC e 24 casi numerici. Due prove fotografiche private esplicitamente escluse perché `TR_RAW_SAMPLE` non impostato.

Nuovo `dist/TrueRenderer-restyle.app`: firma e XPC passati; tre sorgenti cache generate con 15 riusi bit-exact e zero nuovi decode a caldo; 8 schermate native, 11 regioni entro un livello sRGB8, 77 canali differenti, 1:1 esatto. [Rapporto con hash e limiti](cache-integrity-macos.json). Evidenze locali in `var/cache-integrity/`, backup del bundle precedente in `before.app` nella stessa cartella. Nessuna prova Windows o campagna fotografica ampia ripetuta. Warning di deployment target LibRaw, latenze evento→frame, colore/display e gate R0–R4 restano aperti. I fallimenti e warning Rust delle campagne sottostanti descrivono lo stato precedente alla correzione.

## Restyling desktop — 15 settembre 2026

Revisione della barra e consegna completate: strumenti e motore in alto, cartella/conteggi in basso, selettori Globale e Solo questa foto bidirezionali, Settings coerente e bozze preservate. Corretto anche Esc nei popup, con regressione riprodotta prima della correzione. Bundle finale: fmt/Clippy/build/firma, 7 test UI, 12 schermate preferenze, 8 rendering e XPC passati; 11 regioni entro 1 livello sRGB8, 77 canali differenti, 1:1 esatto. Suite desktop: 42 passati, il medesimo fallimento cache e 6 ignorati. Prova diretta del viewer e apertura finale confermate. [Rapporto con hash e limiti](toolbar-macos.json), [viewer](toolbar-viewer.png). Le prove della prima iterazione seguono sotto.

Implementazione locale con copia release separata `dist/TrueRenderer-restyle.app`; precedente bundle e launcher intatti. Passati fmt, Clippy desktop, 4 regressioni UI, 4 test app/renderer, 6 integrazioni desktop e 2 Python. Preferenze: 12 schermate native nelle due lingue, incluse finestra minima e UI 200%. Rendering: 8 schermate, 11 regioni entro la soglia di 1 livello sRGB8 (58 canali differenti), 1:1 esatto; 24 casi numerici passati. Firma ad hoc e suite XPC su due processi passate sull'hash finale. [Rapporto](restyle-macos.json), [progetto e riproduzione](../docs/progetto-restyling.md).

La suite desktop ordinaria resta a 39 passati, 1 fallimento cache Unix sugli hard link e 6 ignorati; il fallimento è già riprodotto sulla baseline della [localizzazione](localization-macos.json). Restano warning worker Mac e warning di deployment target LibRaw nel link, matrice motori/RAW Mac estesa, UI Windows e gate di accessibilità/display/sandbox/release. Nessun gate R0–R4 chiuso, nessun commit o pubblicazione.

## Correzioni gamma/orientamento — 14 settembre 2026

**Due P2 corretti e verificati su Windows:** curva PNG con sola gAMA, orientamento per IFD dell'anteprima RAW, cache v5. Passati 99 test ordinari + 8 integrazioni, debug/release, fmt/Clippy, 18 casi sintetici per worker, IPC/Python/ricampionamento e inventari. [Dettagli](../docs/revisione-aggiuntiva-2026-09-14.md#correzioni), [rapporto con hash](gamma-orientation-fixes-windows.json). Nessun commit/staging; Mac/XPC rinviato, GUI/Nikon non ripetuti.

## Revisione aggiuntiva — 14 settembre 2026

**Due ulteriori P2 aperti:** curva PNG con sola gAMA e orientamento dell'anteprima RAW contaminato da una IFD secondaria. [Rilievi](../docs/revisione-aggiuntiva-2026-09-14.md), [evidenze debug/release](additional-review-windows.json). 18 nuovi casi su ciascun worker, input invariati, sorgenti e binari identici all'ultima suite: i 103 test precedenti non sono stati rieseguiti. Nessuna modifica applicativa, staging o commit; Mac/XPC ancora rinviato.

## Tre correzioni del ricontrollo — 14 settembre 2026

**Tre P2 corretti e verificati su Windows; nessun commit/staging.** [Dettagli e limiti](../docs/revisione-generale-2026-09-14.md#correzioni), [rapporto con hash](general-review-fixes-windows.json). Passati 96 test ordinari + 7 integrazioni, build debug/release, fmt/Clippy, 36 casi sintetici su ciascun worker debug/release, sei controlli IPC, due Python, 24 segnali/1:1 e inventari. TIFF con colorimetria non supportata ora rifiutato esplicitamente, fallback RAW coerente col probe, timestamp cache aggiornato senza scrivere hard link. XPC non avviabile su Windows senza codesign, verifica Mac rinviata. GUI e campagna Nikon non ripetute.

## Ricontrollo generale — 14 settembre 2026

**Quattro correzioni confermate nei casi Windows verificati; tre nuovi P2 aperti.** [Rilievi](../docs/revisione-generale-2026-09-14.md), [rapporto e casi sintetici](recheck-windows-2026-09-14.json). Rieseguiti fmt/Clippy, 86 test ordinari e 6 integrazioni: tutti passati. Riprodotti con 36 casi sintetici e un controllo isolato degli handle: colorimetria TIFF ignorata, fallback RAW incompatibile col probe e mtime cache Windows non aggiornato. Verificati LibRaw, Cargo.lock e 348 notice. Nessuna modifica applicativa, staging o commit. Release/GUI/fotografie restano evidenze delle campagne precedenti; Mac/XPC e le altre qualifiche aperte non sono dichiarati passati.

## Correzioni della revisione serale — 14 settembre 2026

**Verifiche Windows concluse; nessun commit/staging.** [Rapporto e hash](review-followup-fixes-windows.json), [correzioni e limiti](../docs/revisione-continuata-2026-09-13.md#correzioni). Ripresa la sessione dopo l'applicazione dei quattro fix: 86 test Rust ordinari recuperati + 6 integrazioni completate, 21 richieste bitmap LPAC, debug/release, fmt/Clippy, sei IPC, due Python, 24 segnali/1:1, LibRaw e inventario Windows (205 package/348 notice). Match con entrambe le varianti compilato nella prova minima; Mac/XPC nativi restano rinviati.

GUI release: seconda foto e zoom/centro conservati attraverso tre motori e ritorno al primo, usando l'azione completa condivisa col pulsante. 577.296 pixel GPU entro un livello sRGB8; due database privati integri, sorgente e copie invariate. Una persistenza saltata per contesa del lock Windows 33 e nuovo decode del dettaglio al ritorno: la prova non attesta cache integralmente riutilizzata. La precedente campagna di 156 sviluppi non è stata ripetuta. Le sezioni sotto conservano gli audit precedenti alle rispettive correzioni.

## Revisione ripresa — 13 settembre 2026 sera

**Quattro nuovi difetti confermati e non corretti in questo audit.** [Rilievi](../docs/revisione-continuata-2026-09-13.md), [rapporto con hash](review-continuation-windows.json). Recuperati dalla sessione interrotta 81 test ordinari + 6 integrazioni, build debug/fmt/Clippy, sei controlli IPC, LPAC e 18 risposte bitmap; completati build release, 24 segnali e 1:1, due regressioni Python e verifica manifest/notice. Confermato `E0005` con una riproduzione minima del pattern dei test Mac: nessuna build nativa Mac dichiarata. Il percorso completo del pulsante «Applica e salva» perde selezione/zoom; lo smoke precedente invocava direttamente `apply_settings()`. PNG con `cICP` e TIFF con alpha associata producono pixel errati. Nessuna modifica applicativa, commit o staging; le campagne Nikon precedenti non sono state ripetute.

## Correzioni dell'audit — 13 settembre 2026

**Verificate localmente su Windows; nessun commit/staging.** [Rapporto senza fotografie](pre-main-fixes-windows.json), [risoluzione dei rilievi](../docs/revisione-pre-main.md), [confine LPAC](../docs/adr/0009-isolamento-worker-windows.md). Passati 89 test Rust (81 ordinari + 8 integrazioni), fmt/Clippy, build debug/release, 6 prove IPC, 2 Python, 24 segnali e 1:1 esatto. LibRaw 0.22.2: 106 hash upstream/79 unità compilate; inventario Windows di 205 package e 348 notice verificati, con byte preservati da Git. Import release privi delle DLL VC redistributable; installazione pulita ancora da provare.

**Nikon e UI:** 90/90 sviluppi D750 + 66/66 D40, risoluzioni rispettivamente 6032×4032 e 3039×2014 (o orientate); hash originali invariati rispetto all'audit. GUI D40: tre motori e ritorno al bilineare, cambio durante scansione, selezione preservata e riuso cache, quattro stadi riusciti. Confrontati 260.796 pixel fotografici della superficie con errore massimo zero; esclusi compositor e monitor. La prova native macOS/XPC è rinviata dal titolare; colore misurato, qualità generale, prestazioni p95 e R0–R4 restano aperti. La sezione seguente conserva l'audit iniziale, prima delle correzioni.

## Audit prima di main — 13 settembre 2026

**Promozione dell'intero incremento locale: non raccomandata finché i difetti restano aperti.** Verificati tutti i file non committati inventariati; codice applicativo e vendor invariati durante l'audit, nessun commit/staging. [Rilievi e riproduzioni](../docs/revisione-pre-main.md), [riepilogo senza fotografie](pre-main-review-windows.json).

Passati build debug/release, fmt/Clippy, 75 test Rust ordinari + 7 integrazioni, 6 prove IPC, 2 Python, 24 casi di ricampionamento, 90 sviluppi release D750 e 66 D40 con originali invariati. GUI release D40: cambio motore, selezione e cache al ritorno passati; 260.796 pixel della superficie e ulteriori 180.056 pixel in regione fotografica a errore massimo zero. Queste prove non qualificano colore fisico, superiorità del demosaic, p95 o il nuovo Mac/XPC.

Il laboratorio ha riprodotto accesso del worker a file non concessi, PNG a gamma dichiarata interpretati come sRGB, TIFF con miniatura scambiati per RAW, panic su preview troncata, quota errata nei riusi corpus→RAW, deroga hard link non esclusiva e token Windows invariato dopo modifica in-place con mtime ripristinato. Confermate correzioni upstream mancanti nella LibRaw 0.21.1 vendorizzata. Sono rilievi **non corretti** da questa revisione. Log e input sintetici privati in `var/pre-main-review/`; il tentativo Mac si ferma prima dei test perché manca `/usr/bin/codesign`.

## Esperimento locale Windows — 12 settembre 2026, nessun commit

**Revisione successiva D40:** [risultati](raw-engines-d40-windows.json) e [dettagli](../docs/verifica-motori-d40.md). Prima 22 rifiuti del motore proprio per modello escluso; dopo l'estensione controllata, 66/66 sviluppi completi sui 22 NEF con i tre motori, originali invariati. Corretto il cambio motore durante scansione e provato nella UI; 260.796 pixel della superficie identici a CPU (ricampionamento CPU per questa geometria), ritorno alla cache senza nuovo sviluppo. Regressione D750: tre sviluppi passati e nove ritagli 1:1 identici alla consegna sotto. Passati 82 test Rust, fmt/Clippy/build, due regressioni Python e sei controlli IPC. Questa revisione non qualifica altri ISO (tutti i D40 provati dichiarano ISO 200), colore misurato o Mac/XPC; nessuna fotografia pubblicata.

Implementati motori RAW selezionabili, AHD e TrueRenderer direzionale fp32. [Progetto e limiti](../docs/progetto-motori-raw.md), [riepilogo numerico senza fotografie](raw-engines-windows.json). Il lavoro Windows preesistente e questo incremento restano locali, non da committare. Le sezioni Mac sotto descrivono la baseline precedente.

- **Codice:** 82 test Rust (75 ordinari e 7 integrazioni esplicite), fmt/Clippy/build, 2 regressioni Python e 6 prove IPC passati. 24 segnali di ricampionamento e 1:1 esatto passati con report in `var/raw-engine-check/resampling/reports/`; il nome storico `resampling-macos.json` dentro quella cartella privata non cambia il target Windows della prova.
- **RAW:** 30 NEF D750 × 3 motori, 90 sviluppi completi nel worker confinato, originali invariati. Mediane bilineare/AHD/proprio 2,67/4,61/6,10 s, build debug e cache OS non svuotata. fp32 proprio conserva −0,188…2,298. Non sono tempi evento→frame o una qualifica della memoria totale.
- **Dettaglio sintetico:** 24 casi migliorano sul bilineare matematico, quattro rampe sono numericamente equivalenti e quattro trame a crominanze indipendenti peggiorano. Nessuna superiorità fotografica generale, ΔE00 misurato o confronto Apple qui.
- **UI:** tre motori e ritorno al precedente, selezione conservata, livelli Adatta riusati in cache; quattro schermate private. Corrette le acquisizioni ripetute e rimosso il secondo dithering egui dal raster già quantizzato. Il confronto di 558.144 pixel del viewport con CPU passa entro un livello sRGB8; prima prova negativa a due livelli conservata in `var/raw-engine-ui-pixels-before-presentation.json`.

**Mac aperto:** build Apple+LibRaw e collegamento libc++ dei due XPC predisposti, ma nuovo bundle non costruito né provato. La suite XPC invocata su questo PC non procede perché manca `/usr/bin/codesign`. Da ripetere anche campionamento della superficie e selettore sul Mac. R0–R4, matrice camere, ICC/display, firma e sandbox completi restano aperti. I risultati fotografici dettagliati e gli originali non sono pubblicati.

Baseline del 9 settembre 2026. Build interna **0.1.5** installata e verificata su macOS arm64; qualifica integrata del progetto aperta.

**Stato repository controllato il 9 settembre:** codice e report di queste prove sono ora inclusi in `10123b5`, verificato su `main` remoto. Gli hash nel riepilogo corrispondono ancora al codice e al bundle installato. Le basi Git e gli stati «non pubblicato» nelle sezioni/JSON storici descrivono il momento della misura. Questa revisione aggiorna i Markdown; non è una nuova esecuzione dei benchmark.

## Continuazione della navigazione del 9 settembre 2026

Corretto il lavoro che proseguiva dopo l’abbandono di una foto: snapshot/hash/cache e attesa dell’ammissione verificano ora la domanda corrente. Probe e decode nativi già iniziati terminano mantenendo processo e lease; il cambio qualità della stessa sorgente conserva lo sviluppo condivisibile. Prima della consegna si escludono i consumatori abbandonati e si liberano anche le richieste originarie dal registro pending. Un annullamento rimane riprovabile, senza errore permanente in UI.

**Codice:** 71 test Rust (61 ordinari + 10 integrazioni), 2 regressioni Python, fmt/Clippy, 6 prove IPC e 24 sinusoidi passati. Le quattro nuove regressioni coprono annullamento prima dell’ammissione, lookup già bloccato sui crediti, cambio foto durante probe e cambio qualità sulla stessa sorgente; verificano il numero di sviluppi, il processo ancora vivo e i crediti restituiti. La prima esecuzione nel sandbox non aveva accesso a Core Image; la suite nella sessione macOS nativa è passata. Log locale `var/continuation-navigation-20260909-143737/verify-native.log`.

**RAW e memoria:** [30 NEF autorizzati](preview-real-raw-macos.json) passati entro 2 GiB: 60 coppie Standard/Piena fredde/calde bit-exact, 30 riaperture senza decode, tre dettagli e due richieste fredde simultanee. Originali invariati, 1.815.038.714 byte di picco prenotato e zero crediti residui. [Misura isolata](preview-navigation-memory-real-raw-macos.json): footprint massimo 2.018.970.672 byte, RSS 1.965.047.808 byte, 7.042 campioni completi, intervallo massimo 54,63 ms. La misura comprende tutti i processi del bundle isolato e non qualifica driver indipendenti, picchi fra campioni o altri RAW; non è un confronto A/B né una latenza evento→frame.

**Recupero:** [tre prove native](preview-review-native-macos.json) passate sul candidato: sorgente modificata/rimossa/ripristinata, prima perdita del device recuperata e seconda perdita con arresto atteso; database integri. I quattro binari corrispondono a quelli della misura RAW.

**Bundle finale:** `dist/TrueRenderer.app`, precedente in `var/package-history/TrueRenderer-0.1.5-before-navigation-20260909-144830.app`. Suite completa installata passata: XPC, Finder, formati/cache, impostazioni, copia autonoma con database integri e firma/hash. Le 14 regioni di screenshot passano entro la soglia prestabilita di un livello sRGB8 (massimo osservato 1, con 36 canali differenti nella vista Adatta); 1:1 resta esatto. I quattro binari coincidono con quelli delle prove RAW e recupero. [Riepilogo e hash dei report](preview-navigation-continuation-macos.json); log `var/continuation-navigation-20260909-143737/installed.log`.

Restano aperti corpus reale 1.000 RAW e matrici 12/24/45 MP, pressione fisica, p95/p99 evento→frame, A/B integrato, driver/display/altri OS e gate R0–R4. Le evidenze della continuazione precedente sono conservate separatamente e riportate sotto.

## Prima continuazione verificata del 9 settembre 2026 (storico)

Al momento della prova: base Git `e609310`, modifiche ancora locali; successivamente incluse in `10123b5`. Corretti rilascio dei buffer compressi, ammissione per fasi, geometria dei mip e contesa dei decode pesanti; massimo due snapshot pronti. Default 2 GiB e preferenze utente invariati. Corrette le verifiche affinché errori, interruzioni o carichi falliti non conservino un precedente successo come esito corrente.

**Controlli del codice passati:** 67 test Rust (60 ordinari + 7 integrazioni), 2 regressioni Python dei report, rustfmt, Clippy, 6 prove IPC e 24 casi sinusoidali con 1:1 esatto. Log locale `var/preview-review/verify-concurrent.log`. Passati anche cache v2, integrazione dei due XPC e rifiuto del comando di fault injection. Le [tre prove native](preview-native-before-navigation-macos.json) verificano cambio/rimozione/ripristino sorgente, una ricreazione del device e arresto atteso alla seconda perdita; database integri. Non sono reset fisici del driver o OOM.

| Prova reale, limite esplicito 2 GiB | Esito finale isolato |
|---|---|
| 30 NEF Nikon D750, 6016×4016 o orientamento equivalente | Passati; copie private, originali invariati |
| Miniature Standard/Piena a freddo e caldo | 60 coppie, fp32 bit-exact |
| Riapertura cache da nuovo pool | 30 hit, zero nuovi decode |
| Dettaglio | Standard 2048 e Piena nativa su tre file |
| Due richieste fredde visibili simultanee | Passate |
| Picco crediti prenotati | 1.815.038.714 byte; zero residui allo shutdown |
| Picco RSS aggregato host/XPC | 2.066.481.152 byte |
| Picco physical footprint aggregato | 2.100.284.608 byte (circa 1,96 GiB) |
| Campionamento | 7.245 campioni, zero incompleti, massimo tre processi; intervallo massimo 355 ms |

Evidenze: [carico RAW finale](preview-real-raw-before-navigation-macos.json), [memoria finale](preview-memory-real-raw-macos.json), [ambiente e identità dei binari](preview-review-environment-macos.json). Ogni misura ora avvia una copia univoca del bundle, conteggiando tutti i suoi processi XPC. I tempi includono snapshot/hash/cache o decode fino agli artefatti CPU; escludono la presentazione GUI. Cache OS non svuotata, una prova per caso, nessun p95/p99 o A/B integrato. RSS e footprint sono campionati e possono duplicare pagine condivise: non misurano separatamente driver/sistema o picchi fra campioni.

**Esiti negativi conservati:** la [precedente prova estesa](preview-memory-overlap-failure-macos.json) registrava 2.591.724.536 byte e cinque processi, a fronte di un [carico funzionalmente passato](preview-real-raw-overlap-functional-macos.json). La successiva diagnosi per PID ha rilevato un'altra istanza host già aperta dallo stesso percorso con un suo decoder. Quel totale era contaminato e non prova il consumo del solo carico; la nuova prova isolata lo sostituisce senza cancellarne l'evidenza. La modifica sperimentale alla terminazione XPC è stata rimossa. Separatamente, il [test a 512 MiB](preview-memory-low-budget-macos.json) registra correttamente un [rifiuto di ammissione](preview-real-raw-failure-macos.json), originali invariati e zero crediti residui: l'host di questa regressione precede l'ultima estensione della concorrenza, come mostrano gli hash, mentre i decoder coincidono.

**Pacchetto finale:** `dist/TrueRenderer.app` aggiornato; precedente in `var/package-history/TrueRenderer-0.1.5-before-continuation-20260909-142049.app`. Suite installata completamente passata: Finder, formati/cache, 14 regioni di screenshot, impostazioni, XPC, rilocazione con database integri e firma/hash. Log `var/preview-review/installed-final-continuation.log`; report `package-macos.json` e `relocation-check.json`. I quattro binari installati sono identici a quelli della misura RAW/memoria e delle tre prove native.

**Qualifica ancora aperta:** 1.000 RAW reali, altri modelli e 12/24/45 MP, carichi misti/tutte le viste, pressione fisica, p95/p99 evento→frame, A/B integrato, energia/termica, driver/display e altri OS. Il margine osservato sotto 2 GiB è limitato e non autorizza una garanzia universale. Le qualità Anteprima Standard/Piena non abilitano Standard/Riferimento; R0–R4 restano aperti.

## Revisione locale del 9 settembre 2026 — sessione precedente, prima della continuazione

Al momento della prova le modifiche erano nel working tree, base `e609310`; la successiva pubblicazione è inclusa in `10123b5`. Bundle aggiornato in `dist/TrueRenderer.app`, versione interna 0.1.5. I risultati dell’8 settembre nella sezione successiva restano storici.

Corretti tre problemi: invalidazione delle sorgenti richieste/residenti modificate, rimosse o ripristinate; ricreazione grafica limitata a un tentativo con conservazione di selezione, undo e salvataggi; riconoscimento dei NEF che ImageIO presentava come TIFF, evitando di usare la miniatura 160×120 al posto del RAW nativo. Il fingerprint `raw-detection-v2` rende obsolete le precedenti derivazioni. La profondità del sensore rimane sconosciuta quando non è disponibile: non viene dedotta dagli 8 bit della miniatura.

**Verifiche passate nella sessione precedente:** 64 test Rust (57 ordinari + 7 integrazioni esplicite), rustfmt, Clippy, 6 controlli IPC e 24 casi sinusoidali con 1:1 esatto. Suite completa del bundle installato: XPC, cache, 19 fixture formati e controlli aggiuntivi, Finder, screenshot/campionamento, impostazioni, copia autonoma, integrità database e firma/hash. Cache v2: 6 casi freddi, 30 hit caldi, zero decode RAW a caldo, fp32 bit-exact e zero crediti residui. Log locali: `var/preview-review/verify-final.log`, `installed-final.log`, `native-review-final.log`, `preview-cache-final.log`.

Le tre prove native della revisione passano sul bundle installato: modifica/rimozione/ripristino della sorgente, perdita del device con recupero, seconda perdita con arresto atteso (exit 1). Database integri in tutti i casi. Sono callback reali di `device.destroy`, non una qualifica di reset fisici del driver o OOM. Se il backend finestra ha già propagato un panic, si termina con diagnostica e completamento dei salvataggi accettati, senza tentare di riusare winit. Evidenza: [prove native della revisione](preview-review-before-phases-native-macos.json).

**RAW reali autorizzati:** 30 NEF distinti Nikon D750, 6016×4016 o equivalente orientato. Lavorazione su copie private temporanee, originali verificati invariati; nessuna fotografia, percorso o digest delle foto pubblicato. Il test usa esplicitamente **3072 MiB**, senza modificare le preferenze dell’utente.

| Prova | Esito |
|---|---|
| Miniature Standard, mediana fredda / calda | 2,917 s / 34,36 ms |
| Miniature Piena, mediana fredda / calda | 2,898 s / 36,53 ms |
| Standard dopo nuovo pool | 39,31 ms, 30 hit, zero nuovi decode |
| Residenza miniature | 2.016.048 byte per qualità |
| Dettaglio su tre foto, Standard / Piena | 32.216.368 / 515.421.488 byte residenti |
| Picco crediti prenotati, limite 3 GiB | 2.163.241.030 byte; zero crediti residui |
| Picco footprint aggregato host/XPC | 2.058.652.816 byte; zero campioni incompleti |

Report: [RAW reali](preview-real-raw-before-phases-macos.json), [memoria](preview-memory-before-phases-macos.json), [ambiente e hash dei binari](preview-review-before-phases-environment-macos.json). Tempi esplorativi: cache OS non svuotata, alcune attività di build/regressione sovrapposte, nessun A/B isolato né p95 evento→frame. Il bundle misurato precede soltanto le ultime correzioni grafiche e dell’isolamento dei dati diagnostici; worker e decoder XPC sono identici nei byte al bundle finale. Il campionamento non include allocazioni indipendenti dei driver o picchi fra campioni e può contare due volte pagine condivise.

**Fallimento storico a 2 GiB, prima della correzione per fasi:** un NEF da 24 MP viene rifiutato correttamente dall’ammissione conservativa, prima di superare il budget. Il [report di fallimento funzionale](preview-real-raw-before-phases-failure-macos.json) conserva `passed: false`, originali invariati e zero crediti residui. Questa prova verifica il rifiuto sicuro, non il funzionamento del carico entro il budget predefinito. Serve ridurre/qualificare i temporanei o un percorso RAW ridotto/regionale; non abbassare artificialmente le stime.

Restano aperti 1.000 RAW reali e matrice 12/24/45 MP, p95/p99 e A/B evento→frame, pressione fisica e memoria driver, reset/OOM reali, display e altri OS. Specifica e ADR sono integrati nell’appendice E delle due architetture; piano e registro aggiornati. Per riprendere: [consegna della sessione](../docs/ripresa-codex.md).

## Anteprime 0.1.5 — prove dell'8 settembre

`scripts/verify.sh` ha superato **62 test Rust** (55 ordinari + 7 integrazioni esplicite), rustfmt, Clippy con `-D warnings`, sei controlli IPC e 24 casi sinusoidali con 1:1 esatto. Le regressioni includono lease concorrenti, livelli autonomi, CPU scalare/NEON/parallela bit-exact, migrazione preferenze, cache v1/v2 con quota comune, corruzione/link/contesa, cancellazione fra batch, precedenza lettori, P0–P6/FIFO/promozioni durante lookup, prefetch e pressione macOS. Esecuzione nativa fuori dal sandbox aggiuntivo del terminale; XPC dell'app resta App Sandbox. Una regressione di arresto invia 40 salvataggi senza consumare il canale di 16 risultati: tutti risultano committati, con WAL chiusi e database integri quando Service viene rilasciato. Evidenza: `verification.log`.

Il controllo del filtro e dell'uscita SDR GPU passa su 1.593.672 canali lineari entro `1e-5 + 1e-4*abs(cpu)`, errore massimo 0,00006103515625; altrettanti canali sRGB8 entro un livello. Copre dimensioni dispari, alpha, crop, bordi, 1:1 e valori firmati fino a 1000. Cancellazione prima del submit e crediti fino al completamento verificati. Non è qualifica ICC/ΔE00/display. Evidenza: `preview-quality-gpu-macos.json`.

Contesto Core Image richiesto CPU riutilizzato bit-exact su 17 fixture. Richiesta separata Metal conforme nei casi provati, senza prova di backend effettivo diverso o beneficio generale: il decode produttivo resta CPU. Evidenza: `preview-native-compute-macos.json`.

| Stadio renderer, stessa immagine/qualità | CPU scalare | CPU parallela | GPU | IC 95% parallela/scalare | IC 95% GPU/parallela |
|---|---:|---:|---:|---:|---:|
| 256×171 | 3.934 ms | 3.039 ms | 1.350 ms | 0.771–0.774 | 0.437–0.450 |
| 1200×800 | 70.392 ms | 60.857 ms | 8.219 ms | 0.863–0.866 | 0.134–0.136 |

100 terne con ordine ruotato per caso e 1.000 bootstrap appaiati. Criterio IC superiore < 0,95 passato per entrambe le accelerazioni nei due casi. CPU include filtro e codifica SDR; GPU submit e completamento, input residenti dopo il primo upload riportato separatamente. Esclusi sorgente/hash/cache/decode ed effettiva presentazione egui. **Non è un guadagno end-to-end dell'app né un p95 evento→frame**. Cache OS non svuotata. Dati grezzi, intervalli e hash del binario misurato: `preview-compute-performance-macos.json`. Misura precedente alla sola correzione della chiusura Service/catalogo; renderer e harness invariati.

La preparazione di **1.000 DNG sintetici distinti 512×384**, senza preview incorporata, termina in **345.507 s** inclusa persistenza. I 106 ritorni con nuovo pool hanno mediana **9.483 ms**, zero nuovi decode, hash sorgente verificato e zero crediti di lavoro residui. Pulizia iniziale separata dalle espulsioni della corsa. La misura precede la sola correzione della chiusura Service/catalogo: questo harness usa direttamente DecodePool e non istanzia Service; gli hash dei binari misurati sono inclusi nel report. Evidenza: `preview-navigation-macos.json`; misura precedente conservata in `preview-navigation-before-batching-macos.json`. Le prove coprono consegna CPU, non GUI né la matrice di RAW reali 12/24/45 MP. Il generatore iniziale 256×192 era rifiutato da ImageIO; i dati usati nelle misure completate sono 512×384.

Footprint aggregato host/XPC massimo **1,109,149,976 byte** nel test cache e **915,212,544 byte** nel viewer, entro 2 GiB con zero campioni incompleti. Intervalli massimi 39.15/38.10 ms. Include memoria GPU attribuita ai processi, non driver indipendenti o picchi fra campioni; le somme possono duplicare pagine condivise. Report `preview-memory-cache-macos.json` e `preview-memory-viewer-macos.json`.

Il crash intermittente wgpu durante `Surface::configure` è stato riprodotto e corretto eliminando submit concorrenti dal worker: encoding asincrono, submit UI, qualifica iniziale prima del loop. Passati **20 avvii/transizioni** consecutivi sul bundle finale, senza retry dei fallimenti (`preview-lifecycle-macos.json`). Non è fault injection di device loss.

`scripts/test-installed-macos.py` passa sul bundle finale: cache, 19 fixture e 6 controlli formati, XPC con due PID e rifiuto della fault injection nel servizio normale, Finder, screenshot, impostazioni, copia autonoma con database integri e binari identici, firma ad hoc/arm64. Evidenze: `installed-verification.log`, `package-macos.json`, report nativi. Ambiente e hash in `preview-environment-macos.json`. Inventario **212 package/330 notice** verificato contro il lockfile.

Restano recovery del device, osservazione delle sorgenti residenti modificate/rimosse, corpus reale autorizzato, p95/p99 integrati, pressione fisica/matrice completa dei profili, driver/display/altri OS. Il progetto anteprime e R0–R4 rimangono aperti secondo PLAN/ADR 0006; il file della proposta è conservato. Baseline 0.1.4 separata in `preview-baseline-macos.json`, con solo il percorso temporaneo anonimizzato.

I paragrafi seguenti sono lo storico delle versioni precedenti. I report con nomi condivisi vengono rigenerati dalla suite corrente; i confronti storici dedicati restano conservati.

## Ottimizzazioni dopo il primo push — pacchetto finale 0.1.4

Il commit cache `2dad0b2` è stato pubblicato prima di profilare e modificare ulteriormente le prestazioni; il secondo incremento verificato è pubblicato in `a901942`. Attivati SHA-256 hardware con rilevamento CPU/fallback software e riuso dello snapshot privato fra cache lookup e decode. Sono passati **40 test Rust** (35 ordinari + 5 integrazioni esplicite), fmt/Clippy, 6 controlli IPC, 19 fixture e 6 controlli aggiuntivi dei formati, 24 sinusoidi, XPC, Finder, copia autonoma e firma/hash del pacchetto. Tutte le 14 regioni di screenshot continuano a dare zero differenze di canale. Evidenze aggiornate: `verification.log`, `installed-verification.log`, `package-macos.json` e report nativi. Il pannello `13-cache-settings.png` è stato controllato visivamente.

| Sorgente generata | Mediana calda prima | Mediana calda finale | Guadagno caldo | Fredda finale, scrittura inclusa |
|---|---:|---:|---:|---:|
| JPEG 12 MP | 0,9631 s | 0,1374 s | 7,0× | 0,6223 s |
| DNG Bayer | 0,06928 s | 0,00972 s | 7,1× | 0,1727 s |
| PNG 16 bit piccolo | 0,000304 s | 0,000119 s | 2,6× | 0,01024 s |

Una corsa a cache applicativa fredda e cinque calde per sorgente, stesso Apple M4/macOS 26.6.2, cache OS non svuotata. Il confronto non isola il contributo di ciascuna modifica e non è un p95. Ogni bit fp32 di ogni livello e l'istogramma sono identici, senza nuovi job decoder a caldo; originali invariati e pulizia passata. `cache-performance-comparison.json` riporta valori non arrotondati, hash dei report e conteggi della profilazione: 1.737/2.153 campioni del thread in SHA-256, non dell'app intera. Tre nuovi test verificano vettori SHA noti e chunk, policy del broker dopo passaggio di snapshot e lettura della copia catturata anche quando il file sorgente viene sostituito.

Le scritture restano sincrone nel thread decoder; dimensione fp32, quote locali e gate v1 rimangono quelli di ADR 0005. Inventario aggiornato a 207 package e 320 notice, compresa la licenza MIT di `sha2-asm 0.6.4`.

## Cache per cartella 0.1.4 — prima pubblicazione

Passati **37 test Rust** (33 ordinari + 4 integrazioni esplicite), 6 controlli IPC e tutta la suite nativa del pacchetto. I cinque nuovi test cache coprono precisione bit esatti, corruzione/troncamento, chiavi obsolete, cancellazione, quote, scadenza/LRU, temporanei abbandonati, impostazioni, link e lock. Tutte le 19 fixture dei formati, le 24 sinusoidi e le 14 regioni di screenshot restano passate, con zero differenze di canale nelle regioni confrontate. Finder, XPC, copia autonoma e integrità dei database passati; screenshot delle impostazioni in `13-cache-settings.png`.

Misura della prima implementazione (`cache-before-performance.json`), prima delle altre ottimizzazioni richieste:

| Sorgente generata | Cache applicativa fredda, scrittura inclusa | Mediana di 5 riaperture calde | Rapporto |
|---|---:|---:|---:|
| 12mp-jpeg.jpg | 1.8610 s | 0.9631 s | 1.9× |
| 07-bayer.dng | 0.3966 s | 0.0693 s | 5.7× |
| 16bit.png | 0.0170 s | 0.0003 s | 56.1× |

Sono prove locali su Apple M4, con cache del sistema operativo non svuotata; non sono p95. Tutti i bit di ogni livello fp32 e l'istogramma sono identici; nei riusi non viene avviato alcun nuovo job decoder e i file originali rimangono invariati. La cache è riapribile da una nuova istanza del gestore e la pulizia elimina i derivati. La cartella è `.truerenderer-cache` dentro quella delle immagini; default 4 GiB complessivi, temporaneo massimo 2 GiB, scadenza 30 giorni, riserva libera 512 MiB.

Il README pubblico è in inglese. Le ulteriori ottimizzazioni sono state eseguite dopo il commit/push di questo incremento, come richiesto; vedere la sezione precedente. Il cache full-frame rimane Anteprima, senza qualifica tile/gigapixel, budget globale o filesystem remoti.

## Risultati precedenti e regressioni

| Controllo | Esito | Evidenza |
|---|---|---|
| Workspace Rust, debug e release | Passato | `verification.log`, `build-macos.log` |
| rustfmt e Clippy con `-D warnings` | Passato | `verification.log`, incluse le ultime modifiche UI |
| Test Rust | 32 passati | 28 normali + 4 integrazioni eseguite esplicitamente dopo la build |
| IPC avversario sul binario worker | 6 controlli passati | `worker-protocol.json` |
| Timeout del processo | Passato | worker fermo interrotto entro il limite del test |
| Crash e riciclo del worker reale | Passato | decodifica, kill, errore contenuto, nuova decodifica |
| Gate nel worker su pipe | Passato | input fuori allowlist rifiutato prima del codec; il successivo job del corpus passa. Esterni ammessi solo nel bundle XPC (ADR 0004). |
| Salvataggio durante decoder bloccato | Passato | test del servizio: commit delle annotazioni e shutdown entro i limiti |
| Cambio dominio con decoder inattivo | Passato | worker reale riciclato senza inviare un nuovo job |
| Libreria, revisioni e undo | Passato | conflitto di revisione, rollback, undo con incremento, servizio asincrono |
| Backup/restore | Passato | Online Backup API, controllo integrità e restore verificato in database separato |
| Eliminazione dell'indice | Passato | rating e UUID conservati dopo ricostruzione dell'indice |
| Native UI nel bundle 0.1.3 | Passato | Metal/Apple M4, 12 immagini, 8 screenshot controllati, incluse frequenze radiali e griglia a tre dimensioni |
| Calcolo WGSL vs CPU | Passato per il solo stadio provato | 4.096 campioni, massimo errore assoluto 9,536743e-7; soglia 1e-4 |
| Bundle `.app`, servizi XPC, plist, arm64 e firma ad hoc | Passato | `package-macos.json`: firma strict, entitlements dei servizi, hash dei quattro binari |
| Avvio 0.1.3 via Finder/LaunchServices | Passato dopo il consenso macOS | `finder-macos.json`: 12 immagini, zero errori, Metal, due PID XPC |
| Copia indipendente del runtime 0.1.3 | Passato | `relocation-check.json`, `relocation-macos.json`: bundle/corpus copiati, XPC e database integri nella nuova cartella |
| Laboratorio XPC con App Sandbox | Controlli del laboratorio passati | `xpc-sandbox-macos.json`: file/rete negati, FD readonly, versione errata e crash contenuti |
| Laboratorio XPC con limite processi | Controlli del laboratorio passati | `xpc-sandbox-limited-macos.json`: `RLIMIT_NPROC` 0 blocca figli e non può essere rialzato |
| Controllo del processo tramite task/audit token | Prova SPI locale passata | `xpc-control-macos.json`; `task_terminate` non è utilizzabile per questo processo BSD |
| Decoder XPC integrati | Passato | `xpc-integration-macos.json`: due processi, pixel identici, 12 immagini, timeout e recupero indipendente |
| Memoria in un servizio di fault injection separato | Passato per la traiettoria provata | `xpc-memory-growth-macos.json`: soglia superata, servizio terminato e sostituito |
| Esclusione della fault injection dal bundle normale | Passato | `xpc-qualification-macos.json`: comando rifiutato, hash confrontati con il pacchetto installato |

## Formati e pacchetto 0.1.3

`installed-verification.log` registra l'esecuzione completa di `scripts/test-installed-macos.py` sul bundle installato, dopo la suite Rust. Il comando ripete decodifica, ricampionamento, XPC, Finder, confronto screenshot, formati nella UI, copia autonoma, integrità dei database di test e confronto firma/hash del pacchetto.

| Verifica nuova | Esito | Evidenza |
|---|---|---|
| Apertura esterna JPEG, PNG, TIFF, GIF, BMP, HEIC, WebP e DNG | 19 fixture passate | `formats-macos.json`, byte propri rigenerati tramite gli script consegnati |
| Campioni 16 bit e alpha | Passato | Test Rust Core Image su PNG RGBA16: due codici adiacenti rimangono distinti e alpha zero è zero premoltiplicato; PNG/TIFF 16 bit nei casi XPC |
| EXIF 1–8 | Dimensioni/quadranti passati | Otto TIFF con riferimento analitico indipendente; nessuna rotazione duplicata |
| RAW completo | Passato sul DNG sintetico | Bayer RGGB 1024×768, nessuna preview incorporata, `Apple RAW 8.dng · full · TR-linear-v1` |
| Fotografia per dimensioni | JPEG 12 MP passato | 4000×3000 sorgente, crop 1:1 nella UI; contenuto sintetico |
| Input malformati e recupero | Tre rifiuti passati | JPEG sconosciuto, PNG e TIFF troncati; successivo PNG valido con pixel identici e stesso PID |
| Firma/estensione, quota, sorgente obsoleta | Tre controlli passati | PNG rinominato JPG riconosciuto come PNG, file oltre 256 MiB e token stat obsoleto rifiutati |
| UI esterna | 19 immagini, zero errori | `formats-smoke-macos.json`: `10-external-grid`, `11-raw-full`, `12-jpeg12mp-1to1`, controllati visivamente |
| Regressioni di campionamento | Passate sulla 0.1.3 | 24 sinusoidi, 8 screenshot corpus, 14 regioni con zero differenze di canale |
| Firma, Finder e copia autonoma | Passati sulla 0.1.3 | `package-macos.json`, `finder-macos.json`, `relocation-check.json`; originali e libreria ordinaria preservati |
| Repository/licenza | Pubblico, proprietaria | LICENSE, README, NOTICE; sorgenti originali riservati, dipendenze con proprie licenze |

I quadranti bitmap hanno soglia di errore assoluto lineare 0,025 per includere la compressione: JPEG massimo 0,01592201, HEIC 0,00600189, casi lossless circa 0,00002821. Non è una misura ΔE00 né una qualifica ICC/CMYK universale. Per RAW si verifica lo sviluppo completo non costante; non un riferimento cromatico di fotocamere reali.

La compatibilità RAW resta legata a fotocamera/OS. Core Image e ColorSync sono forniti da macOS; nessun LibRaw è incorporato nella 0.1.3. Quote e ricetta sono in ADR 0004. Il picco del worker nella prova formati è una misura singola, disponibile nel report; i 2 GiB supervisionati non sono un tetto kernel né il budget globale dell'app. Nessun nuovo test di crescita artificiale è stato dichiarato eseguito sulla 0.1.3.

La costruzione delle fixture è stata ripetuta con `scripts/generate-format-fixtures.py`. Le schermate pubblicate mostrano solo queste immagini o il corpus numerico. Le verifiche Rust sono eseguite fuori dal sandbox aggiuntivo del terminale per consentire Core Image; i decoder dell'app restano nei servizi con App Sandbox. Durante il controllo visivo è stato corretto anche il testo lungo dell'inspector, che ora va a capo senza tagliare il pannello.

## Correzione del ricampionamento 0.1.2

Il problema della 0.1.1 è stato osservato nella finestra nativa e conservato in `00-grid-before-0.1.1.png`. Il corpus e il generatore sono invariati. Viewer e miniature ora producono il raster alla dimensione fisica, con filtro lineare condiviso e 1:1 senza filtro. La riduzione usa la variante Lanczos3 con margine di banda descritta in ADR 0003; alpha usa pesi non negativi. Non è stata allargata l’allowlist.

| Prova mirata | Esito | Evidenza e perimetro |
|---|---|---|
| 1:1/crop, RGB non clampato, costanti/dispari/1×N, media lineare, alpha e quote | 4 nuovi test Rust passati | Inclusi nel totale 30; il bianco/nero alternato ridotto restituisce 188 sRGB, non 128 |
| 24 casi sinusoidali | Soglie rispettate | `resampling-macos.json`: 8 stopband, 5 passband, 11 transizione soltanto osservati |
| Attenuazione fuori banda | RMS massimo 0,003726606 ≤ 0,02 | Frequenze ≥1,5× Nyquist di uscita, escluso bordo di 12 pixel; non è una misura completa di alias 2D della zone plate |
| Conservazione contrasto passante | Rapporti entro 0,95–1,05 | Frequenze ≤0,25× Nyquist di uscita; riferimento sinusoidale analitico indipendente |
| Corpus radiale a 1:1 | Pixel fp32 identici a LOD 0 | Digest sorgente registrato, PNG a cinque scale in `resampling/`; i file `legacy-nearest-*` modellano il vecchio Adatta, non sono screenshot |
| Presentazione fisica effettiva | **14 regioni, zero differenze di canale** | `sampling-presentation-macos.json`, soglia 1 su 255; confronto del raster CPU con i pixel effettivi degli screenshot della superficie egui/wgpu |
| Screenshot finali | 8 acquisiti e controllati | `01-grid`–`08-grid-large`: griglia, preview, confronto, radiale 1:1, Adatta, 37%, griglia piccola e grande |

Il confronto fisico include miniature di griglia 402×268, 309×206 e 557×372, inspector 514×343, filmstrip 188×125, viewer 1200×800 a 1:1, 1625×1083 in Adatta e 444×296 al 37%. In questo layout Retina 2×, **Adatta è un ingrandimento**: cambia il numero di pixel rappresentati rispetto a 1:1. Il test confronta anche le ripetizioni dell’inspector/filmstrip nei diversi stati.

La finestra installata 0.1.3 è stata anche aperta con `--open` sul DNG sintetico e acquisita dal sistema in `09-installed-window.png`, quindi controllata visivamente; l’app è rimasta aperta per l’uso. Questa cattura del compositore è distinta dagli otto screenshot della superficie usati nei test.

Le schermate a tutto frame, se visualizzate ridotte in un altro programma, possono introdurre nuovi motivi di moiré: l’esito quantitativo è sui pixel nativi, senza ridimensionare i PNG. Non dimostra assenza universale di alias/ringing, fedeltà del compositore o calibrazione del pannello. Restano i test completi di §19.3 elencati nell’ADR. Il renderer può mostrare brevemente lo sfondo mentre prepara un nuovo raster; il budget «mai viewport vuoto» non è ancora soddisfatto.

La richiesta del Desktop è stata osservata nei log TCC (`AUTHREQ_PROMPTING` per `it.truerenderer.prototype`), anche dopo il cambiamento del codice firmato ad hoc; i successivi avvii sono terminati con esito positivo. Il progetto conserva corpus e libreria sul Desktop come richiesto. Il consenso non è stato dato automaticamente e le impostazioni privacy non sono state modificate. Il messaggio dell'app è dichiarato in `NSDesktopFolderUsageDescription`.

Fonti e spiegazione del comportamento: [documentazione Apple della chiave Desktop](https://developer.apple.com/documentation/bundleresources/information-property-list/nsdesktopfolderusagedescription), [controllo degli accessi ai file macOS](https://support.apple.com/guide/security/controlling-app-access-to-files-secddd1d86a6/web).

## Mac di prova

- macOS 26.6.2, build 25G83; architettura arm64.
- Adattatore wgpu: Apple M4, backend Metal; superficie Bgra8Unorm.
- Rust 1.98.1; egui/eframe 0.36.1; wgpu 30.0.1; SQLite bundled 3.53.2.
- Corpus sintetico generato nel repository: 12 PNG, 8/16 bit e trasparenza.

I tempi registrati nello smoke sono misure di una singola esecuzione e non sono benchmark p95.

## Misure XPC e provenienza delle prove

La build release 0.1.3 ha usato due PID distinti. Un decoder sospeso è stato fermato in **211 ms**
nel test con timeout da 200 ms; l’altro ha continuato a decodificare. Il riavvio è riuscito
con un nuovo processo. Il giro completo dura circa 10,93 s e include il ritardo di launchd.

La misura di crescita conservata in `xpc-memory-growth-macos.json` appartiene alla 0.1.1 e non è stata ripetuta come fault di crescita nella 0.1.2/0.1.3. Il bundle normale 0.1.3 è stato nuovamente verificato e rifiuta quel comando.

Il servizio avversario separato alloca 512 MiB a passi nel singolo comando di prova e ignora
gli errori di disconnessione XPC. Il broker campiona ogni 25 ms, con soglia 384 MiB:
picco osservato **408.731.648 byte**, superamento **6.078.464 byte**, circa **5,8 MiB**.
La terminazione è avvenuta 1.099 ms dopo l’inizio della crescita; questo tempo non misura
la latenza a partire dall’attraversamento della soglia. Il decoder successivo ha funzionato.
La fault injection è compilata soltanto nel bundle di prova sotto `var/`; il servizio normale
rifiuta quel comando. Una traiettoria non stabilisce il massimo overshoot possibile.

Il broker verifica il CDHash del servizio installato e usa il task del kernel per la misura;
la terminazione impiega un audit token e libproc SPI, senza fallback a un segnale al solo PID.
Le impronte dei binari sottoposti alla suite XPC coincidono con quelle del pacchetto installato.
Lettura aggiuntiva dei database rilocati: file chiusi senza WAL, controllo di integrità readonly
con SQLite di sistema 3.51.0; l’app e i test di persistenza usano sempre la versione bundled 3.53.2.

## Cosa questi risultati non dimostrano

Il decoder è ora integrato con XPC, ma il gate completo della sandbox resta aperto. La firma
ad hoc non autentica un distributore e il servizio verifica soltanto l’identificatore dell’host;
il requisito reciproco del firmatario di release rimane da qualificare. La terminazione usa una
SPI, provata su questo Mac: non è una promessa di supporto per tutte le versioni macOS.
Mancano corpus avversario esteso, handle residui/duplicati, quote end-to-end, bootstrap ostile
e pressione del sistema. La supervisione memoria non è un tetto rigido imposto dal kernel.
App Sandbox da sola consente il figlio `/usr/bin/true`; il limite aggiuntivo è stato provato
soltanto sul Mac indicato. I due PID dell’app derivano da due servizi distinti; due connessioni
allo stesso servizio nel laboratorio condividevano un PID. Dettagli in `docs/adr/0002-xpc-decoder-r0.md`.

Non sono prove dell'accessibilità con VoiceOver/NVDA, di Windows, del recupero da device loss, della gestione ICC o della fedeltà del display. Il test GPU non misura ΔE00, LUT, gamut o filtro Lanczos3. Non qualificano le matrici complete di fotocamere RAW e sottotipi JPEG/TIFF, né XMP o gigapixel. Il sottoinsieme bitmap/RAW effettivamente provato nella 0.1.3 è registrato sopra. La rilocazione riguarda i file necessari al runtime; non prova una ricompilazione della toolchain dopo spostamento. I render rimangono Anteprima e R0 resta aperto.

## Riproduzione

`scripts/verify.sh` esegue build/lint/test, protocollo e i 24 casi di campionamento. `scripts/verify.sh --gui` aggiunge lo
smoke esteso del binario fuori bundle, che usa pipe, e il confronto dei pixel. `scripts/build-macos.sh` crea il bundle con XPC;
`python3 scripts/test-xpc-integration.py` ne verifica decoder, timeout e rifiuto della fault injection.
Il test LaunchServices è `open -n -W dist/TrueRenderer.app --args --sampling-smoke`. Dopo lo smoke, eseguire
`./dist/TrueRenderer.app/Contents/MacOS/TrueRenderer --verify-sampling-screenshots`; la suite numerica è
`./dist/TrueRenderer.app/Contents/MacOS/TrueRenderer --verify-resampling`.
L’avvio del bundle può richiedere di consentire l’accesso al Desktop nella finestra di macOS.
`python3 scripts/verify-package-macos.py` controlla firma, entitlements e hash dopo che i report
Finder e rilocazione della versione corrente sono stati salvati. Le copie per la rilocazione
devono contenere un proprio `corpus/`, il bundle in `dist/` e nessun symlink verso gli originali.

Il laboratorio XPC usa `python3 scripts/build-xpc-probe.py` e `python3 scripts/test-xpc-probe.py`;
ripetere entrambi con `--limited` per la seconda variante. La prova di crescita si riproduce con
i comandi in ADR 0002. Le suite XPC sono separate dai 30 test Rust e dalle 6 prove IPC del worker.
