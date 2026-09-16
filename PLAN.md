# TrueRenderer — piano di sviluppo

Aggiornato: 16 settembre 2026. Specifica: [architettura](docs/TrueRenderer-Architettura.md); matrice di implementazione: [avanzamento](docs/avanzamento.md); [indice documentale](docs/README.md).

Codice Windows/RAW e correzioni pubblicati in `67146e3`. Ultima suite Windows: 99 test ordinari + 8 integrazioni, [rapporto con hash](reports/gamma-orientation-fixes-windows.json). Nuovo restyling locale Mac con prove UI/corpus/XPC; la campagna fotografica ampia resta quella del 9 settembre. Le misure delle singole campagne restano nei [rapporti](reports/VERIFICA.md).



## Progetto di incremento — navigatore filesystem commutabile, 15 settembre 2026

Richiesta del titolare: pianificare un navigatore di file/cartelle a sinistra, **commutabile con il pannello già presente**, da implementare più avanti. Specifica: [progetto Esplora / Libreria](docs/progetto-navigatore-filesystem.md). Questo aggiornamento è solo documentale e non cambia l'ordine delle attività applicative già aperte.

- [x] Esaminare codice e architettura e documentare layout, switch Libreria/Esplora, interazioni, concorrenza, persistenza, fasi e matrice di accettazione; nessuna implementazione applicativa in questa sessione.
- [ ] N0: verificare contratti, baseline e spike nativi/accessibilità, includendo la conservazione del pannello attuale.
- [ ] N1–N2: modello dell'albero, enumerazione asincrona limitata, coordinatore di navigazione e scansione progressiva separata dai salvataggi.
- [ ] N3: pannelli Libreria/Esplora commutabili, file/cartelle, breadcrumb e cronologia, mouse/tastiera e layout IT/EN; switch senza reset di selezione, filtri, zoom o qualità.
- [ ] N4–N5: preferiti durevoli, sessione, migrazione/backup, refresh, watcher, volumi e casi filesystem.
- [ ] N6–N7: eseguire la matrice di accettazione, verificare bundle/XPC e piattaforme dichiarate, registrare risultati e limiti e sincronizzare i documenti.

## Prosecuzione — cache e verifica Mac, 15 settembre 2026

- [x] Riesaminare i progetti attivi: il restyling e il bundle/corpus/XPC sono verificati; qualifica fotografica RAW, prestazioni integrate e gate R0–R4 restano aperti.
- [x] Correggere la rimozione di hard link nella cache v1/v2, mantenendo le letture valide e proteggendo anche link comparsi dopo una scansione; 25 test cache passati.
- [x] Completare suite workspace (106 ordinari + 10 integrazioni), build debug/release e prove native del nuovo bundle: cache, 8 schermate, 11 regioni e XPC passati; [hash e limiti](reports/cache-integrity-macos.json), due test fotografici privati esplicitamente saltati.
- [ ] Qualifica integrata evento→frame (§12 della specifica anteprime): primo probe comando→superficie implementato sotto; presentazione effettiva e campagna completa ancora aperte.

### Harness della superficie — 15 settembre 2026

- [x] Collegare un probe opt-in al viewer reale: comando, raster esatto aggiunto a egui, ricezione della superficie e confronto pixel successivo alla misura.
- [x] Preparare esecuzioni isolate CPU/GPU, cache applicativa fredda, SSD dopo riavvio e ritorno RAM; impedire caricamenti prima del primo comando.
- [x] Verificare release Standard/Full: 24 processi isolati, 72 selezioni e confronto pixel entro un livello sRGB8; CPU/GPU, cache fredda/SSD/RAM confermati. [Rapporto](reports/navigation-surface-macos.json). Il readback non è un timestamp del display; tre selezioni dipendenti non qualificano p95/p99.
- [ ] Completare scroll, transizioni di risoluzione RAW, pressione, baseline e campagna statistica con intervalli di confidenza prima di chiudere §12; zoom/pan e override sul corpus piccolo sono verificati sotto.

### Transizioni del viewer — 15 settembre 2026

- [x] Estendere il probe a zoom 300%, pan, Fit, override Standard/Full e 1:1 fisico usando gli stessi comandi del viewer.
- [x] Registrare copertura completa/parziale e riproiezione a ogni ridisegno della stessa sorgente; fallire se manca completamente il contenuto.
- [x] Verificare release Standard/Full e CPU/GPU: 24 processi, 240 azioni, 24 controlli 1:1 esatti, 1.568 ridisegni delle transizioni senza draw completamente vuoti; [rapporto](reports/navigation-transitions-macos.json). Copertura provvisoria minima 40,5%, non sempre completa.
- [x] Migliorare il ritorno a Fit conservando una rappresentazione con copertura più ampia della stessa sorgente/revisione entro quota; verificare invalidazione, rilascio memoria e continuità senza inventare pixel mancanti.

### Copertura del ritorno a Fit — 15 settembre 2026

- [x] Conservare facoltativamente un frame più ampio per vista, foto, digest, motore e dimensioni sorgente: massimo due complessivi, entro un quarto dei budget applicabile/GPU e 64 frame totali.
- [x] Abbandonare prima i frame facoltativi in caso di pressione, riduzione quota, ammissione renderer/decoder o invalidazione; mantenere i crediti fino al completamento previsto.
- [x] Riprodurre il fallimento di copertura completa sul bundle precedente e aggiungere regressioni di geometria, identità, quote e rilascio.
- [x] Verificare CPU/GPU e Standard/Full: 24 processi, 240 azioni, 1.553 ridisegni con copertura completa, 24 controlli 1:1 esatti; invalidazione, recupero grafico e XPC passati. Bundle aggiornato e [rapporto](reports/fit-coverage-macos.json).
- [ ] Estendere le prove a livelli RAW ridotti, pressione reale e assenza della riserva; la copertura completa del corpus piccolo non è una garanzia universale.

## Immagini grandi, pressione e RAW Mac — 15 settembre 2026

- [x] Verificare livelli residenti 12/24/45 MP, Standard/Full, CPU/GPU con fallback esplicito, freddo/riapertura: 24 processi e 240 azioni, 1:1 esatto.
- [x] Provare espulsione della riserva a 1536 MiB e rifiuto sicuro del decode a 512 MiB con crediti restituiti; distinguere pressione iniettata e memoria fisica campionata.
- [x] Riprodurre e correggere il crash LibRaw sullo stack XPC; 120 sviluppi su 30 D750 autorizzati passati, originali invariati.
- [x] Confrontare nove ritagli centrali registrati e ripetere 32 casi Bayer con verità nota; non attribuire differenze di ricetta alla sola qualità del demosaic.
- [x] Completare 40 azioni RAW, invalidazione 45 MP, cinque stadi del cambio motore UI e verifica XPC; 75 test Rust passati, bundle aggiornato. [Rapporto e limiti](reports/large-pressure-raw-macos.json).
- [ ] Ridurre/qualificare il picco memoria su 45 MP Full: la somma RSS supera 4 GiB e il footprint arriva poco oltre 4 GiB, pur con crediti entro quota. Pressione fisica OS, decode RAW ridotto/regionale e target colore calibrato restano aperti.

## Restyling desktop — 15 settembre 2026

- [x] Rivedere il README dopo la consegna: schermate aggiornate di viewer, griglia e preferenze, spiegazione dei selettori e istruzioni per il bundle separato; controllare immagini e collegamenti locali.
- [x] Completare la ripresa e la consegna: correggere Esc nei popup senza uscire dal viewer, verificare il bundle finale con UI/pixel/XPC, recuperare rapporto e screenshot mancanti e aprire l'app aggiornata.
- [x] Revisione richiesta dal titolare: allineare barra e Settings, mostrare motore applicato, spostare strumenti in alto e cartella/conteggi in basso; selettori espliciti Globale e Solo questa foto Standard/Piena; sette regressioni UI con clic reali egui, screenshot e verifiche del bundle in [rapporto](reports/toolbar-macos.json).
- [x] Definire composizioni per griglia, viewer e preferenze; [progetto](docs/progetto-restyling.md).
- [x] Centralizzare stile e gerarchia visiva; compattare barra, navigazione, miniature e strumenti del viewer.
- [x] Organizzare ispettore in sezioni richiudibili e preferenze in quattro schede con azioni fuori dall'area scorrevole.
- [x] Verificare bozze, comandi, italiano/inglese e layout alle dimensioni ridotte/200%; 12 screenshot preferenze e 8 di rendering, 11 regioni entro la soglia di 1 livello sRGB8.
- [x] Completare build release e copia separata `dist/TrueRenderer-restyle.app`, firma e XPC; aggiornare registro/README e sincronizzare le architetture. [Rapporto e limiti](reports/restyle-macos.json).

## Interfaccia bilingue — 14 settembre 2026

- [x] Tradurre controlli, preferenze, guida e messaggi applicativi; inglese predefinito, menu Language/Lingua con English e Italiano.
- [x] Salvare la lingua senza applicare altre preferenze in bozza o invalidare immagini, selezione e zoom; compatibilità con impostazioni precedenti.
- [x] Aggiornare README con lingua, comandi inglesi e limiti dei dialoghi/diagnostica nativi.
- [x] Verificare build, lint, cinque nuove regressioni e smoke grafico inglese/italiano; sincronizzare le architetture anche con Python 3.9. Suite desktop: 39 passati, 6 ignorati e un fallimento cache riprodotto sul commit base; [rapporto](reports/localization-macos.json).

## Revisioni Windows del 13–14 settembre

- [x] Revisionare README e Markdown dopo la pubblicazione: separare Windows/macOS e stato corrente/storico, indicare i documenti archiviabili, verificare i link locali e sincronizzare le architetture.
- [x] Verificare i percorsi vicini alle ultime correzioni con 18 nuovi casi sintetici per worker debug/release e confrontare gli hash della precedente suite.
- [x] Applicare la curva PNG con sola `gAMA=45455`, senza identificarla con sRGB; regressioni numeriche 8/16 bit, alpha, provenienza e precedenze, cache v5.
- [x] Distinguere orientamento TIFF assente da 1 e impedire che una IFD secondaria ruoti l'anteprima principale; regressioni per directory/EXIF e worker LPAC, cache v5.
- [x] Riesaminare il lavoro non committato e rieseguire fmt, Clippy, 86 test ordinari e 6 integrazioni; verificare LibRaw e inventario/notice.
- [x] Riprodurre i nuovi rilievi con file sintetici, broker reale e modulo Directory di produzione; documentare risultati e limiti.
- [x] Rifiutare esplicitamente la colorimetria TIFF non supportata nei tag 301/318/319/342/532; verificare rifiuti e pixel senza tag, invalidare le vecchie cache con `bitmap-tiff-color-v4`.
- [x] Rendere coerenti probe e decode nel fallback RAW su anteprima di diversa risoluzione, conservando la validazione IPC; regressione LPAC passata.
- [x] Correggere l'aggiornamento del timestamp degli hit cache Windows e verificare scadenza/LRU, descrittori/blocchi e preservazione degli hard link.
- [ ] Estendere la diagnostica PNG per cICP malformato e dichiarazioni sRGB/gAMA discordanti, distinta dai tre P2.
- [x] Recuperare la sessione, verificare i rilievi e completare build release, ricampionamento e riproduzione del blocco di compilazione Mac; registrare gli esiti senza modifiche applicative o commit.
- [x] Ripristinare il controllo esaustivo di `Owner` nel test di crash/recovery e compilare la prova minima con entrambe le varianti.
- [x] Completare la suite Mac: rilievo cache risolto, 106 test ordinari e 10 integrazioni passati il 15 settembre, fmt/Clippy e build debug/release senza warning Rust.
- [ ] Estendere la verifica fotografica nativa Mac dei motori; le prove del corpus e del bundle non qualificano la matrice Apple/LibRaw.
- [x] Correggere il percorso «Applica e salva» per conservare selezione/zoom al cambio motore; regressioni e smoke nativo sulla stessa azione completa del pulsante.
- [x] Gestire o rifiutare esplicitamente PNG `cICP` e TIFF con alpha associata; aggiungere regressioni e invalidare le cache interessate.
- [ ] Qualificare separatamente contesa del writer e persistenza del dettaglio: nello smoke serale una scrittura saltata per lock Windows 33 e nuovo decode al ritorno, senza interrompere la visualizzazione.
- [x] Inventariare tutto il lavoro non committato e rivedere anche port Windows, cache, bridge nativo, build, dipendenze e documenti.
- [x] Verificare debug/release, 82 test Rust, 6 prove IPC, 2 Python, ricampionamento e 156 sviluppi release D750/D40; originali invariati.
- [x] Provare GUI release e cambio motore; riprodurre separatamente i difetti, conservando log e laboratorio privati.
- [x] Correggere accesso residuo ai file del worker Windows e verificare LibRaw 0.22.2 con le correzioni applicabili; perimetro LPAC e limiti in ADR 0009.
- [x] Correggere colore PNG esterni, classificazione TIFF/RAW e accesso fuori limite nel parser preview; regressioni passate su Windows.
- [x] Correggere quota al cambio tipo di sorgente, controllo hard link e invalidazione Windows con mtime ripristinato; completare provenienza/migrazione impostazioni.
- [x] Incorporare il runtime C/C++ MSVC e controllare gli import release; generare inventario Windows, 348 notice e manifest nativo, preservando i byte upstream nei checkout Git.
- [x] Costruire una copia del nuovo bundle Mac/XPC: release separata del 15 settembre con corpus, UI e XPC verificati; la matrice fotografica estesa dei motori resta aperta.
- [ ] Verificare il pacchetto in un'installazione Windows pulita prima della distribuzione.
- [x] Accorpare audit/correzioni, piano Windows e nota LibRaw; ridurre la cronologia duplicata, aggiornare riferimenti e verificare la sincronizzazione.
- [ ] Qualificare notifiche di pressione memoria Windows, percorsi lunghi/manifest longPathAware e gestione ICC prima di dichiarare completa la piattaforma.

## Obiettivo e regole

Applicazione desktop Rust per sfogliare, selezionare e ispezionare immagini con una resa tracciabile. Target previsti per la prima versione: macOS arm64 e Windows x86-64, SDR; sono implementati e provati prototipi macOS arm64 e Windows x86-64, con perimetri e qualifiche ancora aperti. Le tappe del documento sono criteri di accettazione: una schermata funzionante non chiude un gate di colore, sicurezza o accessibilità.

Si sviluppa una verticale per volta. Ogni consegna contiene codice compilabile, prove riproducibili, limiti osservati e aggiornamento dell'architettura. Nessuna modifica agli originali; annotazioni separate dall'indice. Nessun servizio cloud, account o canone. Tutto il progetto, inclusa la toolchain locale, rimane nella cartella del progetto. Dati di sviluppo locali in `var/`; prima di una release spostarli nel percorso app-data nativo qualificato.

## Primo incremento — prototipo R0

- [x] Leggere requisiti, invarianti, architettura, UX, schema e roadmap; conservare il documento originale.
- [x] Registrare il nuovo nome e questo piano, con tracciamento progressivo nel documento richiesto.
- [x] Installare Rust localmente senza cambiare PATH o configurazione della shell.
- [x] Creare workspace con `tr-core`, `tr-app`, `tr-render`, `tr-store`, `tr-platform`, `tr-worker` e desktop.
- [x] Implementare tipi validati, stato deterministico, geometria e riferimento numerico sRGB/Rec.2020 fp32.
- [x] Implementare IPC bounded con processo decoder persistente, timeout, copia privata e rifiuto di risposte invalide.
- [x] Generare un corpus deterministico proprio e ammettere solo i suoi digest finché la sandbox OS non è qualificata.
- [x] Collegare finestra egui/wgpu, griglia virtualizzata, anteprima, zoom fisico 1:1, confronto a due e ispezione.
- [x] Prototipare annotazioni durevoli, ricerca e filtri; indice separato, revisioni, undo e backup SQLite.
- [x] Preparare avvio da doppio clic, bundle macOS interno e istruzioni riproducibili.
- [x] Eseguire test di dominio/colore/IPC/persistenza, build, lint e smoke test grafico; salvare i risultati reali.
- [x] Aggiornare lo stato finale e specificare il prossimo incremento senza dichiarare conclusa la v1.

**Verifica dell’incremento conclusa:** avvio del bundle dal Finder passato dopo il consenso macOS al Desktop; passata anche una copia con bundle e corpus indipendenti. Dettagli in `reports/VERIFICA.md`.

## Incremento successivo — prova XPC macOS

- [x] Creare un laboratorio separato con host e servizio XPC firmato ad hoc, con il solo entitlement App Sandbox.
- [x] Misurare accesso negato a file sintetici non concessi, scrittura, connessioni loopback e creazione di processi.
- [x] Provare il trasferimento di un descrittore in sola lettura e verificare che non permetta scrittura.
- [x] Osservare PID su due connessioni, crash/recovery e memoria del servizio senza dedurre garanzie non misurate.
- [x] Salvare risultati e decisione successiva. Non collegare il decoder esterno prima del gate completo.

Passati i controlli delle due varianti: sandbox base e sandbox con `RLIMIT_NPROC` hard/soft 0. File/rete negati, descrittore in sola lettura verificato, crash contenuto. Il limite aggiuntivo blocca i figli; due connessioni condividono un PID. Il gate sandbox completo rimane aperto. Codice, risultati e decisione in `experiments/macos-xpc/README.md`.

## Incremento 0.1.1 — integrazione del confine XPC

Completata la verticale interna: broker macOS, decoder riutilizzabile e due servizi nel bundle; percorso su pipe per i test del core e i target non ancora qualificati. Build 0.1.1 verificata all’epoca con prove native del decoder, controllo grafico e copia autonoma passati; ora conservata nello storico; versione corrente 0.1.5. Restano aperti i gate elencati sotto.

- [x] Portare il decoder controllato in due servizi XPC con protocollo senza percorsi e allowlist anche nel decoder.
- [x] Vincolare il broker al CDHash del servizio installato, verificare due PID reali e conservare la copia privata degli output.
- [x] Separare i due decoder dal writer SQLite; coda di 64 richieste, priorità, cancellazione e riciclo al cambio generazione.
- [x] Misurare terminazione di un decoder sospeso, recupero indipendente e supervisione memoria con fault injection limitata a 512 MiB.
- [x] Verificare che il servizio normale rifiuti il comando di fault injection.
- [x] Completare smoke grafico, avvio Finder e rilocazione della build 0.1.1.
- [x] Verificare il riciclo del decoder inattivo al cambio cartella, anche senza nuovi job; 26 test Rust e 6 prove IPC passati.
- [ ] Qualificare il firmatario di release e il requisito reciproco dei peer; il controllo attuale del servizio verifica soltanto l’identificatore dell’host.
- [ ] Qualificare l’API di terminazione per i minimi OS: attualmente libproc SPI con audit token, verificata su macOS 26.6.2.
- [ ] Estendere misure di revoca al cambio dominio, handle duplicati e overshoot sotto pressione; il campionamento RSS non è un tetto rigido.
- [ ] Estendere i test negativi a input ostili, descrittori residui, output concorrente e quota di risorse.
- [ ] Completare la qualifica della verticale Windows e le prove di display/accessibilità prima di chiudere R0.

Decisione e limiti in `docs/adr/0002-xpc-decoder-r0.md`; evidenze in `reports/VERIFICA.md`. Per completare il gate XPC sul Mac restano la suite avversaria del bootstrap/ciclo di vita e la qualifica estesa della memoria end-to-end; l’ordine dell’incremento locale corrente è indicato in apertura. Nella 0.1.1 i PNG esterni restavano esclusi; la modifica del perimetro nella 0.1.3 è registrata in ADR 0004. Il gate completo della sandbox rimane aperto.

## Incremento 0.1.2 — fedeltà del ricampionamento e documentazione

Concluso il sottoinsieme correttivo richiesto. Build 0.1.2 verificata all’epoca e conservata nello storico; i gate generali R0/R1 restano aperti.

- [x] Osservare la finestra precedente e riprodurre i problemi di Frequenze radiali in Adatta e griglia.
- [x] Usare una sorgente/piramide condivisa in griglia, viewer, inspector, filmstrip e confronto.
- [x] Filtrare in luce lineare alla dimensione fisica, mantenendo 1:1 allineato senza filtro; registrare la variante Lanczos e il trattamento alpha in ADR 0003.
- [x] Eliminare il secondo ridimensionamento delle miniature e limitare il calcolo alle regioni visibili con coda asincrona e cache bounded.
- [x] Superare quattro nuovi test Rust, 24 casi sinusoidali e confronto esatto LOD 0 del corpus invariato.
- [x] Acquisire otto schermate native e confrontare 14 regioni con il raster atteso: zero differenze di canale su questo Mac.
- [x] Superare fmt/clippy, 30 test Rust, 6 controlli IPC, suite XPC e verifica firma/hash del bundle 0.1.2.
- [x] Verificare avvio Finder e copia autonoma con database integri; conservare il bundle precedente.
- [x] Spiegare la differenza PLAN/Architettura e confrontare tutti i capitoli 1–24 con codice/prove, distinguendo mancante, parziale, non qualificato e post-v1.
- [x] Aggiornare entrambe le architetture senza cambiare il backup originale.

**Rimangono da qualificare (§10/§19.3):** alias 2D/Siemens, overshoot, grafo CPU/GPU e filtro diretto, transizioni LOD, EXIF 1–8, f64 della geometria completa, tile/cuciture, altri display/DPI e prestazioni. Il filtro CPU può mostrare brevemente lo sfondo durante il ricalcolo: «mai viewport vuoto» e tutti i p95 non sono ancora garantiti. Le 24 sinusoidi includono 11 casi di banda di transizione osservati senza soglia di accettazione. Le soglie valide riguardano 8 casi di stopband e 5 di passband.

## Incremento 0.1.3 — formati esterni e GitHub proprietario

Richiesta del titolare: repository **pubblico**, **licenza proprietaria**, README e apertura effettiva dei formati. ADR 0004 registra l'anticipo dei decoder Apple rispetto ai gate della proposta; Standard/Riferimento restano indisponibili.

- [x] Creare il repository pubblico `antoniodicoratoinfodev/TrueRenderer`.
- [x] Preparare LICENSE proprietaria, README, NOTICE, regole per contributi e segnalazioni.
- [x] Aggiungere Apri file, trascinamento file/cartella e `--open`.
- [x] Decodificare JPEG/PNG/TIFF/GIF/BMP/HEIC/WebP nel servizio macOS isolato.
- [x] Sviluppare RAW completo con CIRAWFilter, ricetta nominata e nessun fallback su JPEG incorporato.
- [x] Conservare 16 bit/alpha, applicare EXIF una volta, mostrare provenienza e SHA-256.
- [x] Definire quote 256 MiB/64 Mi pixel e cache, evitando la copia integrale del raster per la piramide.
- [x] Superare 19 fixture di formati, 6 controlli aggiuntivi, 32 test Rust e 6 controlli IPC; generatori riproducibili senza fotografie esterne.
- [x] Eseguire tre schermate native di griglia, DNG e JPEG 12 MP.
- [x] Verificare pacchetto finale, regressioni di campionamento, XPC, Finder e copia autonoma; installare 0.1.3.
- [x] Aggiornare entrambe le architetture e pubblicare il commit completo, escludendo dati personali e artefatti locali. Incremento applicativo `424d307` su `main`.

Restano: matrice reale fotocamere/sottotipi/ICC, RAW multipiattaforma con LibRaw, memoria globale e gate R0/R1/R3 completi. I test sul DNG sintetico non qualificano ogni RAW. HEIC/WebP sono un'anticipazione limitata della precedente lista post-v1.

## Incremento 0.1.4 — cache per cartella e impostazioni

Richiesta del titolare del 7 settembre: cache e temporanei accanto alle immagini, quote configurabili, README inglese. Pubblicare questo incremento prima di ulteriori ottimizzazioni delle prestazioni.

- [x] Creare `.truerenderer-cache/entries` e `tmp` nella cartella aperta, con proprietà riconoscibile e fallback in RAM per cartelle non scrivibili.
- [x] Persistenza lossless fp32 della piramide e provenienza; chiave da SHA-256 sorgente, pipeline/decoder e versione; invalidazione e rifiuto dei file corrotti.
- [x] Quote per cartella, temporanei prenotati, riserva di spazio libero, LRU/scadenza, pulizia sicura dei soli derivati.
- [x] Impostazioni persistenti e comandi di pulizia, stato cache e hit/miss visibili.
- [x] Test di precisione, corruzione, limiti, concorrenza, symlink, cancellazione e file originali invariati; misure a cache fredda/calda.
- [x] README in inglese, ADR e sincronizzazione dei due documenti; bundle e verifiche native.
- [x] Commit e push dell’incremento cache: `2dad0b2` pubblicato su `main`, prima delle ottimizzazioni successive.
- [x] Dopo il primo push: profilare, accelerare SHA-256 e riusare lo snapshot sul miss; 40 test e pacchetto finale verificati, JPEG/DNG caldi circa 7× più rapidi nelle prove locali.
- [x] Pubblicare il secondo incremento: `a901942` su `main`; registrati risultati e commit nei documenti.

## Progetto di incremento — anteprime, cache e prestazioni

Richiesta del titolare, 7 settembre 2026: progettare prima di implementare la navigazione con cache RAM/SSD, scelta fra Anteprima Standard e Piena, limiti di memoria configurabili e uso intenso di CPU/GPU durante il lavoro utile. Specifica dettagliata: [progetto di anteprime, cache e prestazioni](docs/progetto-anteprime-cache-prestazioni.md), revisione 2 pubblicata nel commit `a2db2ab`, ora revisione 5 integrata nell’appendice E dell’architettura.

**Stato: implementazione 0.1.5 disponibile; qualifica integrata aperta.** Baseline 0.1.4 conservata in `reports/preview-baseline-macos.json`; decisioni applicate e limiti in [ADR 0006](docs/adr/0006-anteprime-residenza-compute.md). Anteprima Standard/Piena rimane distinta dai badge di pipeline Standard/Riferimento. Nessun gate R0–R4 è chiuso da questo incremento.

- [x] A, modelli: qualità/richieste, migrazione compatibile, budget/lease, ammissione prima delle allocazioni, preferenze RAM/GPU/CPU e adattamento all'alimentazione.
- [x] A, baseline locale: conservare misure della build 0.1.4 senza confondere il caricamento dell'intera piramide con le nuove miniature autonome.
- [x] Revisione locale 9 settembre: 30 NEF Nikon D750 a risoluzione nativa, Standard/Piena fredde/calde e riapertura cache verificati con budget esplicito 3 GiB; 64 test Rust e suite nativa finale passati. Alla chiusura di quella prova il lavoro era locale; ora è incluso in `10123b5`. Ripresa in [docs/ripresa-codex.md](docs/ripresa-codex.md).
- [ ] A, qualifica RAW: baseline e target di throughput su corpus reale autorizzato 12/24/45 MP.
- [x] Continuazione 9 settembre: ridurre la durata dei buffer compressi, stimare il massimo delle fasi e contenere snapshot/decode pesanti. 67 test Rust e 30 NEF a 2 GiB, incluse due richieste simultanee, passati; footprint campionato massimo 2.100.284.608 byte in un bundle isolato dalle altre istanze.
- [x] Verificare e aggiornare il bundle della continuazione: cache, XPC, sorgenti/device, suite Finder/rilocazione/database/firma passati; quattro binari identici alla prova RAW. Conservare il pacchetto precedente e sincronizzare piano, registro e architetture.
- [ ] Budget RAW full-frame: estendere la qualifica a 12/24/45 MP, carichi misti, tutte le viste e pressione fisica; valutare decode ridotto/regionale per i carichi non ammessi. I 30 NEF di una fotocamera non chiudono il requisito generale.
- [x] B: livelli e frame autonomi, writer asincrono limitato in byte, I/O cache indipendente dai due decoder, consegna prima della persistenza.
- [x] C: Standard/Piena persistenti, override per foto/1:1, provenienza e completezza, intent e probe; fallback dichiarato di sviluppo completo temporaneo per Standard.
- [ ] C, ottimizzazione opzionale del backend: qualificare sviluppo RAW ridotto Apple; non attivato nella 0.1.5.
- [x] D: artefatti v2 lossless a blocchi bounded nella stessa quota/lock v1, letture autonome, verifica SHA sorgente, pubblicazione descrittore per ultimo, GC e minimo recuperabile per miniature; regressioni cache negative.
- [x] E, percorso disponibile: domanda visibile, promozione, cancellazione dei consumatori obsoleti, raggruppamento di uno sviluppo per richieste compatibili, prefetch dopo stabilità, pausa/preparazione esplicita; pool Rayon e NEON verificato bit-exact.
- [x] E, scheduler macOS: P0–P6/FIFO, promozioni durante lookup, prefetch direzionale con ritardo adattivo, notifiche pressione OS e precedenza locale ai lettori durante writer/GC; regressioni passate.
- [x] E, continuazione navigazione 9 settembre: annullare lookup/ammissioni non più richiesti, completare le chiamate native in corso senza riciclarle al cambio vista, riusare la stessa sorgente al cambio qualità e non consegnare consumatori abbandonati. Quattro regressioni aggiuntive; 71 test Rust e suite del codice passati.
- [x] E, confronto dello stadio renderer: 100 prove CPU scalare/parallela/GPU per ciascuna di due geometrie, grafi identici; CPU parallela bit-exact e beneficio misurato con intervallo bootstrap.
- [x] Consegna della continuazione navigazione: 30 NEF a 2 GiB nuovamente passati, tre prove native di recupero e suite completa del bundle installato; pacchetto precedente conservato. Evidenze in `reports/preview-navigation-continuation-macos.json`.
- [ ] E, qualifica integrata: beneficio CPU scalare/parallela evento→frame, pressione fisica e adattatori sugli altri OS.
- [x] F, percorso disponibile: compute WGSL separabile e display SDR, pipeline/input persistenti, texture diretta, controlli capability/quote, fallback CPU e lease fino al completamento GPU; controllo numerico prima dell'abilitazione.
- [x] F, Apple: riuso del contesto CPU provato; esperimento Metal distinto conforme sui casi sintetici. Metal nel decoder rimane disabilitato finché backend e beneficio non sono qualificati.
- [x] F, recupero applicativo: invalidare il presenter, ricreare una sola volta finestra/device, riverificare il compute e conservare servizio, salvataggi, undo e selezione. Arresto esplicito alla seconda perdita o se il backend finestra ha già propagato un panic.
- [ ] F, qualifica: reset fisici, OOM, matrice driver/display e beneficio integrato sulle navigazioni reali nei diversi profili.
- [x] §10, sorgenti residenti: monitor fuori UI dei file richiesti/residenti, invalidazione di RAM/presentazione e risultati tardivi su modifica/rimozione/ripristino, stato esplicito e annotazioni conservate. Token Unix più robusto; osservazione best-effort, non revisione coerente v1.
- [ ] G: completare qualifica di tutti i gate del §12, 1.000 RAW reali, scenari freddi/caldi/disabilitati/sotto pressione, p95/p99 evento→frame e misure dei driver. I report sintetici dichiarano il proprio perimetro e non sostituiscono questi gate.
- [x] Pubblicare l’incremento verificato richiesto dal titolare: commit `1b98b4f` su `origin/main`, 62 test Rust e suite nativa del bundle passati; documenti e licenza LibRaw aggiornati, requisiti residui espliciti.
- [x] Integrare l’intero progetto e ADR 0005–0006 nell’appendice E delle architetture della radice e di `docs/`; correggere percorsi e sincronizzazione, preservando il testo utente e il backup originale.
- [ ] Eliminare `docs/progetto-anteprime-cache-prestazioni.md` al completamento dell'intera richiesta, dopo aver trasferito risultati e decisioni nel registro. Il documento è conservato perché restano requisiti aperti.

Le caselle separano il codice verificato dalla qualifica ancora necessaria; non modificano i criteri del progetto per far risultare conclusa una fase parziale. Evidenze aggiornate in `reports/VERIFICA.md` e `docs/avanzamento.md`.

**Licenza LibRaw, verifica storica dell'8 settembre:** la nota e le fonti sono conservate in [ADR 0008](docs/adr/0008-libraw-licenza-e-collegamento.md) e §20.1 dell'architettura. La build di quella campagna non incorporava LibRaw; quella attuale collega LibRaw 0.22.2. Inventario, notice e obblighi dell'artefatto effettivamente distribuito restano da verificare prima del rilascio.

## Esperimento locale — motori RAW selezionabili (12 settembre 2026)

Richiesta del titolare: progettare ed eseguire la pipeline sperimentale, affiancandola ai motori esistenti su Windows/macOS. Incremento pubblicato in `67146e3`. Progetto: [motori RAW](docs/progetto-motori-raw.md). Anticipazione locale del demosaic prima post-v1, senza modifica dei gate.

- [x] Leggere AGENTS.md, piano, registro e architettura; definire contratto e verifiche.
- [x] Collegare selezione persistente, IPC, identità cache e invalidazione al cambio motore.
- [x] Implementare LibRaw AHD e TrueRenderer direzionale fp32 con calibrazione esplicita.
- [x] Verificare segnali sintetici, regressioni e RAW D750 su Windows; registrare i risultati: 32 casi analitici e 90 sviluppi completi, originali invariati. Superiorità generale non dimostrata.
- [x] Provare selettore e ritorno al motore precedente nella finestra; confrontare la superficie con CPU. Rimosso il secondo dithering egui: 558.144 pixel delle quattro regioni entro un livello sRGB8, dopo un primo esito negativo a due livelli.
- [x] Aggiornare stato, licenze locali e ripresa; sincronizzare le architetture preservando testo esterno ai blocchi e backup originale.
- [x] Revisione richiesta e D40: correggere scansione revocata dal cambio motore, aggiungere il modello esatto e verificare 66 sviluppi, UI/scansione e regressione D750. [Dettagli](docs/verifica-motori-d40.md).
- [ ] Qualificare build/bundle macOS e XPC, confronto Apple e fedeltà cromatica sul target.

## R0 — fattibilità (6–8 settimane nel documento, da ricalibrare)

Dipendenze: nessuna milestone precedente. Il prototipo corrente è una parte di R0.

1. Corpus autorizzato con manifest, hardware di riferimento Win/mac e protocollo di misura.
2. Finestra e device/coda condivisi, viewport opaco, shader, contratto sRGB della superficie.
3. Input PNG ICC, alpha, CMYK, precisione di confine e confronto CPU/GPU quantitativo.
4. Protocollo con framing, request ID, quote, handle e output del broker; fault injection e worker avversario.
5. XPC firmato/App Sandbox su macOS; LPAC senza capacità e Job Object su Windows; test negativi filesystem/rete e memoria. Il solo processo separato **non** soddisfa questa voce.
6. Snapshot/revisione sorgente sotto writer concorrente, nessuna promozione sul solo hash/stat.
7. Griglia 100k, IME, focus, DnD, VoiceOver/NVDA, scala 200%, monitor con profili e scale differenti, device loss.
8. ADR toolkit, minimi OS/GPU, licenze, canali; pacchetti interni sui due OS.
9. Almeno cinque interviste e raccolta autorizzata dei casi difficili. Richiede persone reali.

Uscita: tutti i gate di §4.4/§22.4 misurati. Mancando prove complete, egui e il colore restano provvisori. Le anteprime esterne macOS introdotte dalla 0.1.3 sono una deviazione esplicita di sviluppo (ADR 0004), non il superamento del gate.

## R1 — viewer SDR (10–16 settimane, dopo R0)

1. Bloccare backend nativi JPEG/PNG/TIFF, opzioni di compilazione, licenze e corpus per profilo.
2. Campioni nativi 8/16 bit, EXIF qualificato e orientamento una volta; rifiuto dei contenuti non supportati.
3. Little CMS isolato, ingressi RGB/GRAY/CMYK, ICC v2/v4, precedenza metadati colore, assegnazione esplicita.
4. Working linear Rec.2020 fp32, alpha premoltiplicata, viewport opaco, clipping soltanto dichiarato in uscita.
5. TileProvider, CPU reference, Lanczos3 a supporto adattato, pixel fisici 1:1 senza filtro, bordi e cuciture.
6. Renderer wgpu/WGSL con budget GPU/CPU, LUT adattive qualificate, fallback selettivo, recovery.
7. Istogramma/campionatore e provenance per stadio; badge Standard/Riferimento solo con criteri superati.
8. Libreria ICC e monitor, qualifica XMP Toolkit round-trip; nessun sidecar scritto prima del gate.
9. Viewer, loupe, confronto sincronizzato, tastiera e accessibilità provati sui due OS.

Uscita: suite §§19.2–19.3, demo controllata e tutti i formati dichiarati verificati. Non è ancora alpha su archivi reali.

## R2 — browser e dati durevoli (12–18 settimane, dopo R1)

1. Scansione incrementale, watcher, volumi, identità distinta da percorso/digest, file mancanti e ambiguità.
2. Schema completo separato index/library, migrazioni con backup, writer unico, proiezioni ricostruibili.
3. Rating, label, keyword gerarchiche, raccolte statiche/intelligenti, filtri e ricerca con indici/FTS.
4. Revisioni e journal transazionali, batch, undo come nuova revisione, recovery con errori disco/crash.
5. XMP a tre vie, no-clobber/coordinamento filesystem, conflitti visibili e riconciliazione esiti incerti.
6. Backup rotanti verificati, export portabile e restore; retention distinta dalla cache.
7. Griglia 100k misurata, code/priorità/cancellazione/backpressure, quote cache/memoria.
8. Solo dopo sandbox negativa e backup: pilot con 8–12 fotografi; almeno cinque utenti settimanali per quattro settimane.

Uscita: workflow completo senza assistenza e gate prodotto §2.0. Se debole, ridurre o fermare RAW/gigapixel.

## R3 — RAW e grandi immagini (16–24 settimane, dopo il pilot R2)

1. Pin LibRaw/opzioni, distinta e matrice reale fotocamere/CFA; nessun supporto RAW generico.
2. Ricetta nominata, preview incorporata separata dal render, WB/matrici e precisione dichiarati.
3. Tile TIFF/BigTIFF, livelli con provenienza, piramide persistente per necessità e invalidazione revisionata.
4. Preparazione sequenziale con progresso/cancellazione, spool coerente, quote, recovery e garbage collection.
5. Prova 4 Gpx nei profili qualificati, residenza GPU, prefetch e priorità dei tile visibili.
6. Misure cieche su RAW, CPU fallback e corpus colore/filtri/metadata sui due OS.

Uscita: matrice RAW esplicita, grandi immagini entro budget reali. Nessun demosaicing proprietario in v1.

## R4 — hardening e rilascio (12–18 settimane, dopo R3)

1. Fuzzing, sanitizers, fault injection, sandbox negativa completa, input ostili e recovery.
2. Benchmark p95 con almeno 100 prove, manifest hardware/corpus/cache e dispersione.
3. SBOM, notices e licenze sulle versioni effettivamente linkate; aggiornamenti dipendenze.
4. Installer Windows, bundle macOS, firma/notarizzazione, install/upgrade/rollback e smoke test puliti.
5. Test display reali, profili monitor, screen reader, IME, multimonitor e driver difettosi.
6. Manuale, privacy locale, matrice supporto, release candidate e bug triage; rilascio solo dopo gate.

La firma/distribuzione richiede credenziali e decisioni del titolare, non disponibili in questa sessione. Le prove Windows di sviluppo sono registrate; installazione pulita, firma e distribuzione restano da qualificare.

## Post-v1

Linux, Windows arm64, HDR/EDR, AVIF/JXL/EXR/PSD, Android, soft proof, conversione/export pixel, scrittura incorporata, confronto oltre due foto e demosaic proprio: ciascuno richiede ADR, prova di bisogno e gate specifico.

## Stime e aggiornamento

Il documento stima 56–84 settimane di milestone, circa 64–105 con riserva; sono ipotesi per una persona, non una data promessa. Prima revisione delle stime dopo le misure R0. Per ogni incremento aggiornare `docs/avanzamento.md`, le caselle di questo piano e `reports/`, poi eseguire `python3 scripts/sync-docs.py` per aggiornare entrambe le copie dell'architettura.
