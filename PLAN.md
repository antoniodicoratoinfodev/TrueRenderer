# TrueRenderer — piano di sviluppo

Aggiornato: 6 settembre 2026. Fonte: `docs/TrueRenderer-Architettura.md`.
Il nome corrente è **TrueRenderer**; TrueVision è il nome precedente.

Questo file è il piano operativo: ordina attività, dipendenze e caselle completate. `../TrueVision-Architettura.md` è la specifica tecnica e di prodotto, con criteri di accettazione e registro aggiornato. Il confronto completo requisiti/implementazione si trova nel §0 dell’architettura; la fonte del registro è `docs/avanzamento.md`.

## Obiettivo e regole

Applicazione desktop Rust per sfogliare, selezionare e ispezionare immagini con una resa tracciabile. Prima versione supportata: macOS arm64 e Windows x86-64, SDR. Le tappe del documento sono criteri di accettazione: una schermata funzionante non chiude un gate di colore, sicurezza o accessibilità.

Si sviluppa una verticale per volta. Ogni consegna contiene codice compilabile, prove riproducibili, limiti osservati e aggiornamento dell'architettura. Nessuna modifica agli originali; annotazioni separate dall'indice. Nessun servizio cloud, account o canone. Tutto il progetto, inclusa la toolchain locale, rimane in questa cartella sul Desktop. Dati di sviluppo locali in `var/`; prima di una release spostarli nel percorso app-data nativo qualificato.

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

Completata la verticale interna: broker macOS, decoder riutilizzabile e due servizi nel bundle; percorso su pipe per i test del core e i target non ancora qualificati. Build 0.1.1 verificata all’epoca con prove native del decoder, controllo grafico e copia autonoma passati; ora conservata nello storico e sostituita dalla 0.1.2. Restano aperti i gate elencati sotto.

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
- [ ] Affiancare la verticale Windows reale e le prove di display/accessibilità prima di chiudere R0.

Decisione e limiti in `docs/adr/0002-xpc-decoder-r0.md`; evidenze in `reports/VERIFICA.md`. Il prossimo lavoro sul Mac è la suite avversaria del bootstrap e del ciclo di vita, seguita dal budget memoria end-to-end. I PNG esterni restano esclusi dal decoder; l’integrazione non chiude il gate completo della sandbox.

## Incremento 0.1.2 — fedeltà del ricampionamento e documentazione

Concluso il sottoinsieme correttivo richiesto. Build 0.1.2 installata e verificata; i gate generali R0/R1 restano aperti.

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

## R0 — fattibilità (6–8 settimane nel documento, da ricalibrare)

Dipendenze: nessuna milestone precedente. Il prototipo corrente è una parte di R0.

1. Corpus autorizzato con manifest, hardware di riferimento Win/mac e protocollo di misura.
2. Finestra e device/coda condivisi, viewport opaco, shader, contratto sRGB della superficie.
3. Input PNG ICC, alpha, CMYK, precisione di confine e confronto CPU/GPU quantitativo.
4. Protocollo con framing, request ID, quote, handle e output del broker; fault injection e worker avversario.
5. XPC firmato/App Sandbox su macOS; AppContainer/token ristretto e Job Object su Windows; test negativi filesystem/rete e memoria. Il solo processo separato **non** soddisfa questa voce.
6. Snapshot/revisione sorgente sotto writer concorrente, nessuna promozione sul solo hash/stat.
7. Griglia 100k, IME, focus, DnD, VoiceOver/NVDA, scala 200%, monitor con profili e scale differenti, device loss.
8. ADR toolkit, minimi OS/GPU, licenze, canali; pacchetti interni sui due OS.
9. Almeno cinque interviste e raccolta autorizzata dei casi difficili. Richiede persone reali.

Uscita: tutti i gate di §4.4/§22.4 misurati. Mancando prove, egui e il colore restano provvisori e gli archivi esterni non entrano nel decoder del prototipo.

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

La firma/distribuzione richiede credenziali e decisioni del titolare, non disponibili in questa sessione. Le prove Windows richiedono un target Windows reale. Non sono segnate come eseguite.

## Post-v1

Linux, Windows arm64, HDR/EDR, HEIF/AVIF/JXL/EXR/PSD, Android, soft proof, conversione/export pixel, scrittura incorporata, confronto oltre due foto e demosaic proprio: ciascuno richiede ADR, prova di bisogno e gate specifico.

## Stime e aggiornamento

Il documento stima 56–84 settimane di milestone, circa 64–105 con riserva; sono ipotesi per una persona, non una data promessa. Prima revisione delle stime dopo le misure R0. Per ogni incremento aggiornare `docs/avanzamento.md`, le caselle di questo piano e `reports/`, poi eseguire `python3 scripts/sync-docs.py` per aggiornare entrambe le copie dell'architettura.
