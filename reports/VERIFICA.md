# TrueRenderer — verifica del prototipo R0

Aggiornamento: 7 settembre 2026. Build interna **0.1.4**, installata in `dist/TrueRenderer.app`.

## Ottimizzazioni dopo il primo push — pacchetto finale 0.1.4

Il commit cache `59bf9d7` è stato pubblicato prima di profilare e modificare ulteriormente le prestazioni. Attivati SHA-256 hardware con rilevamento CPU/fallback software e riuso dello snapshot privato fra cache lookup e decode. Sono passati **40 test Rust** (35 ordinari + 5 integrazioni esplicite), fmt/Clippy, 6 controlli IPC, 19 fixture e 6 controlli aggiuntivi dei formati, 24 sinusoidi, XPC, Finder, copia autonoma e firma/hash del pacchetto. Tutte le 14 regioni di screenshot continuano a dare zero differenze di canale. Evidenze aggiornate: `verification.log`, `installed-verification.log`, `package-macos.json` e report nativi. Il pannello `13-cache-settings.png` è stato controllato visivamente.

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
