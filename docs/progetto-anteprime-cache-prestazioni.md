# TrueRenderer — progetto di anteprime, cache e prestazioni

> Revisione 5, allineamento documentale del 9 settembre 2026 al codice e ai report inclusi in `10123b5`: specifica integrata nell'appendice E delle due architetture del progetto. La 0.1.5 contiene già livelli autonomi, qualità Standard/Piena, budget, cache v2, writer asincrono, scheduler/prefetch e compute CPU/GPU. La revisione aggiunge osservazione delle sorgenti residenti, recupero grafico, correzione dell'ammissione RAW, cancellazione dei lavori abbandonati in navigazione e verifiche resistenti a risultati obsoleti; mantiene separati implementazione, prove locali e qualifica ancora aperta.

**Stato: specifica applicata in parte e verificata per incrementi; qualifica integrata aperta.** Standard/Piena sono qualità dell'Anteprima, non i badge di pipeline Standard/Riferimento. R0–R4 restano aperti. Gli originali esterni richiedono il bundle macOS XPC/App Sandbox secondo ADR 0004 oppure il worker Windows LPAC secondo ADR 0009; il percorso su pipe mantiene il corpus controllato. Dopo restyling e correzione cache, il primo probe comando→superficie del 15 settembre verifica selezioni e ritorni CPU/GPU, Standard/Full, con cache distinte (§12.1). Presentazione effettiva e qualifica evento→frame completa restano aperte.

La tabella §1 descrive esplicitamente la **baseline storica 0.1.4**, commit `0c3ee2cc3224e35f377497eb5105d04b68a48f63`, comprendente `2dad0b2` e `a901942`. Le prescrizioni successive sono il contratto di progetto; non ogni requisito o numero obiettivo è già qualificato. Stato corrente in [stato e piano](../STATO.md), [ADR 0006](progetto-anteprime-cache-prestazioni.md#adr-0006) e [verifiche](../STATO.md#registro-delle-verifiche-e-degli-incrementi).

La fonte unica è questo file, comprese le sezioni ADR 0003, 0005 e 0006; `scripts/sync-docs.py` ne integra l'intero contenuto nell'architettura della radice e in quella sotto `docs/`, con un rimando stabile a STATO.md. Sincronizzare solo quando cambiano queste fonti della specifica. Il backup originale v1.2 rimane immutato.

### Esito della revisione tecnica

| Tema | Esito e limite |
|---|---|
| RAM/SSD | Corretto separare pixel residenti e catalogo. `ImageLevels` stacca i livelli piccoli; il writer limitato in byte conserva gli stessi lease dei dati trattenuti. Una miniatura non richiede una piramide completa residente. |
| Qualità | Standard e Piena sono distinte da assurance e completezza. Standard usa attualmente sviluppo nativo completo temporaneo e riduzione; nessuna promessa di demosaic ridotto più veloce. |
| Cache v1/v2 | Lossless RGBA32F, checksum e quota comune per cartella corretti. I record v2 non sono tile sorgente né un codec compresso; non qualificano gigapixel. ADR 0005 documenta la storia v1, ADR 0006 il percorso interattivo v2. |
| Budget | Rilascio anticipato dei buffer compressi, massimo delle fasi, mip esatti, massimo due snapshot pronti e serializzazione dei decode pesanti. I 30 NEF D750 passano a 2 GiB, incluse due richieste simultanee; ultimo footprint campionato 2.018.970.672 byte (`preview-navigation-memory-real-raw-macos.json`). Default e baseline 384 MiB invariati. La precedente necessità di 3 GiB è conservata nei report storici; altre camere, carichi misti, pressione e driver restano da qualificare. Nessun tetto kernel è garantito. |
| Prefetch/CPU | P0–P6, ritardo 100–500 ms, I/O distinto e Rayon/NEON implementati. Lookup/ammissioni abbandonati sono cancellati ai confini interrompibili; le chiamate native iniziate terminano e il cambio qualità della stessa sorgente può riusare lo sviluppo. Il ritardo usa consegna CPU recente; non misura la latenza evento→frame. AVX2, controllo termico e alcuni adattamenti restano da implementare/qualificare. |
| GPU | Viewer compute e presentazione diretta disponibili, piramide CPU e decoder Apple richiesto software. Un fallimento compute con device sano permette CPU; perdere il device di presentazione richiede la ricreazione della finestra. |
| Sorgenti cambiate | Monitor fuori UI e writer SQLite, ogni 500 ms sui file richiesti/residenti; invalidazione delle copie interessate e dei risultati tardivi. Su Unix il token comprende device/inode/ctime oltre a size/mtime; resta best-effort, con SHA completo al nuovo caricamento. |
| Riconoscimento RAW | Corretto un bug trovato sui NEF Nikon D750: ImageIO identificava TIFF/miniatura. Probe e decode cercano un decoder RAW effettivo per i contenitori TIFF; il fingerprint cache cambia. Il report esige provenienza RAW e dimensioni native. |
| Prove | Corpus sintetico e A/B del solo renderer già presenti; 71 test Rust e suite installata passati; prove native su sorgenti cambiate, recupero GPU, cancellazione della navigazione e 30 RAW reali autorizzati. Report e limiti effettivi in `STATO.md` (registro e collegamenti ai rapporti JSON); nessuna equivalenza con 1.000 RAW o p95 evento→frame. |

## 1. Decisione e confronto con il codice attuale

Evolvere la cache lossless per cartella già implementata, aggiungendo miniature e anteprime autonome, quote RAM/GPU coordinate, caricamento anticipato guidato dalla navigazione e calcolo parallelo CPU/GPU. Riutilizzare impostazioni, snapshot, checksum, primitive di accesso e test esistenti. La distanza dalla foto visibile influenza la priorità e la permanenza in RAM; non provoca automaticamente una scrittura dell'intera immagine su SSD.

| Area | Comportamento 0.1.4 verificato nel codice | Cambiamento progettato |
|---|---|---|
| Catalogo | Elenco della cartella in RAM con percorsi, identificatori e annotazioni; i 1.000 RAW non vengono tutti sviluppati all'apertura | Conservare questa separazione fra catalogo e pixel |
| Miniature | `ensure_image` richiede `edge: 0` anche per la griglia; sorgente completa e piramide condivisa | Renderizzare da un artefatto autonomo, senza obbligare la sorgente completa a restare residente |
| RAM delle immagini | Massimo 64 elementi oppure 1.536 MiB; eliminazione per uso recente, dopo il completamento | Quote in byte, prenotazione prima dei lavori, protezione delle rappresentazioni visibili |
| Presentazione | Cache nominale di 64 texture / 128 MiB; dipendenza dalla piramide della sorgente | Texture utilizzabili anche dopo l'espulsione della sorgente CPU; contabilizzazione dei buffer in volo |
| SSD | `.truerenderer-cache/entries` conserva l'intera piramide fp32 non compressa e l'istogramma; hit verificato senza nuovo decode | Conservare anche artefatti piccoli indipendenti; evitare di leggere tutta la piramide per una miniatura |
| Preferenze | Già disponibili abilitazione, quota per cartella, temporaneo massimo, scadenza, riserva libera e pulizia; RAM/GPU ancora fisse | Estendere le preferenze esistenti con qualità, RAM/GPU e prestazioni, preservando i valori salvati |
| Hash e sorgente | SHA-256 accelerato dove supportato; `SourceSnapshot` privato riusato sul miss | Conservare queste ottimizzazioni e ammettere il costo dello snapshot nel budget prima della lettura |
| Scrittura cache | Avviene nel thread di decode prima di consegnare l'immagine | Consegnare prima il risultato e persistere in una coda separata con limiti in byte |
| Scheduler | Due decoder, priorità urgente/miniature, coda finita; cancellazione legata alla generazione della cartella | Priorità per vista, revisione del viewport, deduplicazione, promozione dei lavori e cancellazione del lavoro superato |
| CPU | Preparazione della piramide nei thread di decode; ricampionamento del presenter su un solo thread | Pool CPU comune, blocchi paralleli e SIMD verificato |
| GPU | wgpu presenta il raster; il kernel diagnostico non è il renderer completo | Ricampionamento e trasformate del viewer in compute, con confronto contro CPU |
| Decoder Apple | `CIContext` creato nel render con `kCIContextUseSoftwareRenderer: YES`; RAW a scala nativa | Riutilizzo del contesto e percorso Metal nel worker, da verificare; decode ridotto esplicito per la qualità standard |

Mappa dei moduli (i link aprono il codice corrente; per ricostruire la baseline usare il commit `0c3ee2cc3224e35f377497eb5105d04b68a48f63`): [stato UI e cache](../apps/desktop/src/ui.rs), [cache e impostazioni](../apps/desktop/src/cache/mod.rs), [primitive filesystem](../apps/desktop/src/cache/directory.rs), [snapshot e broker](../crates/tr-platform/src/lib.rs), [servizio catalogo](../apps/desktop/src/service.rs), [pool decoder](../apps/desktop/src/decode_pool.rs), [piramide](../crates/tr-core/src/resample.rs), [presenter](../crates/tr-render/src/presenter.rs), [decoder nativo Rust](../crates/tr-worker/src/native.rs), [decoder Apple](../native/macos/image_decoder.m).

Una sorgente 6000×4000 RGBA32F occupa circa 366 MiB; la piramide attuale completa circa 488 MiB. Il limite di 1.536 MiB contiene circa tre di queste immagini, esclusi worker e temporanei. È un calcolo delle dimensioni, non una misura RSS. L'obiettivo è evitare che ogni miniatura richieda di trattenere quei 488 MiB.

Nella 0.1.4 il ritorno su una foto espulsa può già evitare sviluppo e costruzione della piramide grazie all'SSD; rimane il costo di lettura/verifica dell'intera piramide. I [report pubblicati](../reports/cache-performance-comparison.json) misurano cinque riusi per fixture con cache OS non svuotata: sono evidenze limitate del percorso attuale, non p95 su 1.000 RAW né prestazioni del sistema qui progettato.

## 2. Contratto delle due qualità

### 2.1 Nomi e significato

Il selettore utente si chiama **Qualità anteprima** e offre **Standard** e **Piena**. Nel viewer le indicazioni complete sono `Anteprima standard` e `Anteprima a qualità piena`. Non mostrare mai il solo badge `STANDARD` per la scelta veloce: quel badge identifica già una futura pipeline qualificata nell'architettura.

Tre proprietà separate nel modello:

- `PreviewQuality`: `Standard` oppure `Full`, scelta dall'utente.
- `Assurance`: classe di verifica della pipeline; nell'incremento resta `Preview` per entrambe le scelte.
- `Completeness`: contenuto provvisorio, in raffinamento oppure completo rispetto alla richiesta corrente.

| Proprietà | Anteprima standard | Anteprima a qualità piena |
|---|---|---|
| Scopo | Sfogliare rapidamente e contenere il costo delle rappresentazioni | Ispezionare il dettaglio disponibile e conservare la resa del percorso completo |
| Risoluzione del viewer | Derivato con lato lungo iniziale massimo di 2.048 pixel; usare meno pixel se la vista ne richiede meno | Risoluzione necessaria ai pixel fisici del viewport; LOD 0 per 1:1 |
| Griglia e filmstrip | Derivati piccoli commisurati alla cella fisica | Livelli prodotti dal grafo completo di ADR 0003, sufficienti alla cella fisica; nessun obbligo di tenere l'intero RAW in RAM |
| RAW | Sviluppo ridotto esplicito quando supportato e verificato; altrimenti sviluppo completo temporaneo seguito dalla riduzione | Sviluppo alla risoluzione nativa, ricetta nominata corrente; successiva selezione dei livelli necessari |
| Colore e precisione | Derivati a risoluzione ridotta in Rec.2020 lineare RGBA32F, alpha premoltiplicata, senza clamp RGB intermedio | Stesso spazio e formato RGBA32F, cache lossless prima dell'uscita; nessuna nuova quantizzazione intermedia o compressione lossy |
| Filtri | Filtraggio lineare corretto anche sui derivati; il minor dettaglio non autorizza nearest a scala arbitraria | Stesso grafo, versione del filtro, alpha e coordinate di ADR 0003; CPU o GPU verificata per quel grafo |
| Garanzie | Anteprima con dettaglio limitato | Anteprima del percorso completo; non certifica ICC, monitor, ogni fotocamera o la modalità Riferimento |

**Qualità piena riguarda la rappresentazione richiesta, non l'obbligo di materializzare ogni pixel e ogni livello di tutte le foto.** In una cella piccola basta un livello corretto della piramide; a 1:1 occorre il dettaglio sorgente della regione mostrata.

Il valore 2.048 è una scelta iniziale da verificare su display diversi. Se il viewport richiede più dettaglio, Standard resta limitata e lo segnala. Non cambiare automaticamente qualità globale per riempire un monitor più grande.

La differenza iniziale fra le qualità riguarda risoluzione e percorso di sviluppo dichiarato. Standard non applica implicitamente un gamut sRGB più stretto, clipping anticipato, sharpening o una diversa resa tonale. Un futuro formato di cache sRGB8/lossy sarebbe un compromesso distinto, da esporre e verificare; non è il default di questo incremento. Il decode RAW ridotto può comunque differire dal Full: misurare e dichiarare tali differenze, senza promettere equivalenza del demosaicing.

### 2.2 Comportamento dei comandi

1. Il selettore è disponibile nella barra del viewer/confronto e nelle preferenze. La scelta globale si applica anche a griglia, filmstrip e preview laterale, ed è persistente.
2. Nuove installazioni: Standard come valore iniziale orientato alla navigazione. Migrazione da 0.1.3/0.1.4: Piena, per conservare la resa precedente. Rilevare l'installazione esistente prima di creare nuovi file di configurazione/catalogo e conservare le impostazioni della cache già salvate.
3. Dal restyling del 15 settembre, il selettore `Solo questa foto: Standard/Piena` crea un override di sessione per l'asset in entrambe le direzioni; `Usa la qualità globale` lo rimuove. Il selettore `Globale: Standard/Piena` è distinto: il cambio della preferenza globale elimina gli override, come indicato nella spiegazione del controllo. `1:1` continua a richiedere Piena per la foto interessata. Nel confronto il selettore per foto riguarda quella corrente; la qualità effettiva di ciascun riquadro va letta insieme alla sua provenienza.
4. `1:1` e il campionamento dei pixel sorgente richiedono Piena per la foto interessata. Mentre arriva il dettaglio, mostrare `Preparazione dettaglio 1:1`; una preview ingrandita non diventa un vero 1:1. Lo zoom libero in Standard può ingrandire il derivato, segnalando il limite di dettaglio.
5. Il passaggio a Piena conserva l'anteprima già valida della stessa foto/revisione mentre raffina. Il ritorno a Standard cancella i lavori Full privi di altri consumatori, senza interrompere risorse ancora utilizzate dalla GPU.
6. Un risultato Standard non soddisfa mai una richiesta Full. Può soltanto coprire temporaneamente la stessa regione, dichiarando il raffinamento.
7. Istogramma e campionatore indicano lo stadio analizzato. Un istogramma del derivato non viene presentato come istogramma sorgente; il dettaglio tecnico può essere recuperato separatamente dai pixel pesanti.

### 2.3 RAW e fedeltà

La prima implementazione mantiene la scelta di ADR 0004: **nessun ripiego silenzioso sul JPEG incorporato nel RAW**. Standard significa inizialmente sviluppo ridotto o derivato dello sviluppo TrueRenderer. Scala, decoder, ricetta e dimensioni effettive sono registrati nella provenienza.

L'estrazione della preview della fotocamera potrà essere un'opzione distinta, `Usa anteprima della fotocamera`, con indicazione dell'origine e prove per formato/orientamento. Non è necessaria per consegnare le due qualità di questo progetto e non è un prerequisito nascosto del miglioramento promesso. I tempi di estrazione di JPEG incorporati non vengono usati come benchmark dello sviluppo RAW.

## 3. Preferenze e controllo delle risorse

Il restyling del 15 settembre organizza questi controlli in **Settings → Previews and RAW**, **Performance** e **Cache and data** (tradotti in italiano). Le impostazioni sono caricate una volta, fuori dal percorso di disegno; la migrazione verso uno schema versionato nell'app-data resta un requisito di rilascio, mentre lo sviluppo usa il percorso dati locale. Conservare `enabled`, `disk_mib`, `temporary_mib`, `unused_days` e `free_mib` già presenti. La configurazione non appartiene alla cache eliminabile.

| Controllo utente | Valore iniziale proposto | Semantica |
|---|---|---|
| Qualità anteprima | Standard nuova installazione; Piena per migrazione | Preferenza persistente descritta in §2 |
| Memoria complessiva | Candidato automatico iniziale: `min(2 GiB, 25% RAM fisica)` | Budget complessivo app, worker, staging e memoria GPU attribuibile; nessun doppio conteggio della stessa allocazione unificata |
| Limite manuale memoria | 512 MiB fino al 75% della RAM fisica, compatibilmente con i minimi del motore | L'interfaccia espone il valore richiesto e quello effettivamente disponibile; i valori iniziali sono da qualificare |
| Cache riutilizzabile in RAM | Automatico, fino al 40% del budget complessivo quando c'è spazio | Sottolimite, non memoria aggiuntiva. `0` disabilita la conservazione opportunistica; i buffer della vista corrente restano memoria di lavoro |
| Cache su disco, già disponibile | Conservare il valore salvato; default attuale 4.096 MiB per cartella | Tutti gli artefatti e temporanei, vecchi e nuovi, condividono la quota della cartella; nessuna nuova quota aggiuntiva implicita |
| Temporaneo massimo, già disponibile | Conservare il valore salvato; default attuale 2.048 MiB per artefatto | Limite del singolo artefatto in preparazione, oltre alla prenotazione aggregata nella quota della cartella |
| Scadenza, già disponibile | Conservare il valore salvato; default attuale 30 giorni senza uso | Mantenere pulizia per scadenza e LRU con lettori attivi protetti |
| Spazio libero da preservare, già disponibile | Conservare il valore salvato; default attuale 512 MiB sul volume | Riserva distinta dalla quota cache; se non disponibile, sospendere nuove scritture |
| Limite cache disco personalizzato | Valore in GiB, oppure cache disabilitata | Disabilitare interrompe nuovi accessi persistenti; i dati preesistenti restano eliminabili con il comando dedicato |
| Limite GPU, avanzato | Automatico entro budget complessivo e budget del dispositivo | Sottolimite in MiB; su GPU discreta include VRAM attribuibile, su memoria unificata evita duplicazioni |
| Profilo prestazioni | Prestazioni con alimentazione esterna; riduzione automatica a batteria | Prestazioni, Bilanciato, Risparmio; indipendenti dalla qualità visiva |
| Calcolo immagine | Automatico | Automatico, GPU compatibile, CPU; la scelta comprende il compute disponibile nel decoder. La presentazione continua a usare la GPU anche scegliendo CPU |
| Thread CPU, avanzato | Automatico | Limite superiore del pool applicativo, con indicazione dei limiti sui decoder di sistema |
| Precaricamento | Automatico | Disattivato, Automatico, Esteso; sempre subordinato alle quote e alla latenza interattiva |
| Manutenzione | Comandi espliciti | `Svuota cache immagini`, `Ricostruisci anteprime della cartella`, `Pausa preparazione` |

Mostrare occupazione, quota e spazio prenotato separatamente. La riduzione di una quota entra in vigore subito per le nuove ammissioni; i lavori non interrompibili finiscono o vengono gestiti dal broker, e i buffer GPU si liberano dopo completamento. Fino ad allora indicare `Riduzione memoria in corso`, senza fingere un limite RSS istantaneo.

Le impostazioni hanno schema, validazione numerica, migrazione e scrittura temporanea con sostituzione controllata. Un file danneggiato viene conservato per diagnosi e sostituito da default documentati; un fallimento di salvataggio resta visibile. Aumentare la memoria non modifica le quote di sicurezza su file/pixel né abilita un decoder fuori sandbox.

La quota disco resta **per cartella**, come richiesto in ADR 0005; le preferenze globali si applicano alle cartelle visitate, non equivalgono a un tetto di spazio su tutti i volumi. Un eventuale limite aggregato richiede un registro dei percorsi gestiti e una politica per volumi offline e altre istanze: è un'estensione separata, non una garanzia di questa fase. Non cercare e cancellare cache su dischi non aperti dall'utente.

`Svuota cache immagini` opera soltanto nella directory cache gestita. Originali, `library.sqlite`, annotazioni, configurazione e backup durevoli non sono coinvolti. La ricostruzione di tutta la cartella è un lavoro esplicito con progresso/cancellazione; l'apertura normale non avvia 1.000 sviluppi Full.

## 4. Rappresentazioni autonome e flusso dei dati

```mermaid
flowchart LR
    V[Vista e qualità richiesta] --> S[Scheduler e budget]
    S --> G[Cache GPU]
    S --> R[Cache RAM]
    S --> C[Lettura e verifica cache]
    S --> W[Decoder isolato]
    O[Originale in sola lettura] --> B[Broker e snapshot verificato]
    B --> W
    B --> C
    D[Cache SSD per cartella] --> C
    C --> R
    W --> P[Livelli o regioni utili]
    P --> R
    P --> Q[Coda persistenza limitata]
    Q --> D
    R --> G
    G --> F[Presentazione progressiva]
```

Il ramo cache ha una coda I/O indipendente dai decode RAW. Nel formato fp32 non compresso si estende il lettore Rust controllato già previsto da ADR 0005; eventuali codec di decompressione esterni richiedono il confine isolato e l'ammissione descritti in §6. Il broker autorizza accessi e snapshot: i worker non ricevono percorsi generali né scrivono liberamente nella cache. Il diagramma descrive il sistema finale proposto.

| Rappresentazione | Contenuto e durata |
|---|---|
| `AssetDescriptor` | Identità, revisione osservata e metadati leggeri; indipendente dai pixel |
| `ThumbnailArtifact` | Miniatura o livello sufficiente alle celle fisiche; condiviso da griglia e pannelli compatibili |
| `PreviewArtifact` | Derivato Standard oppure livelli Full sufficienti a una preview; non contiene obbligatoriamente LOD 0 |
| `SourceBlock` / `MipLevel` | Campioni lineari indipendenti con coordinate e provenienza; liberabili separatamente |
| `DisplayFrame` | Risultato della vista corrente, con texture e trasformata; disegnabile senza possedere la piramide completa |
| `FullFrameLease` | Sorgente completa temporanea per backend che richiedono lo sviluppo intero; costo prenotato prima del decode |

Refactoring essenziale: `Pyramid { levels: Vec<LinearImage> }` non può restare l'unica unità di residenza. Separare descrizione geometrica dell'immagine, elenco dei livelli e possesso dei blocchi. Un riferimento a una miniatura non deve trattenere indirettamente tutto il raster attraverso un `Arc<Pyramid>`. Anche `Pyramid::from_levels()` della 0.1.4 valida una catena completa fino a 1×1: un insieme parziale richiede tipi e invarianti propri, senza aggirare quelli esistenti.

Una porta `ImageProvider`/`TileProvider` espone le rappresentazioni disponibili e le richieste mancanti. Il primo adattatore può sviluppare l'intera sorgente entro quota e produrre livelli indipendenti. Questo non equivale a implementare decode regionale RAW, streaming o gigapixel: le capability del provider devono dichiararlo.

In Piena si producono soltanto i livelli finali necessari e gli intermedi richiesti dal medesimo grafo di filtri. Gli intermedi transitano con una durata limitata; generarli e scartarli non deve cambiare coefficienti, origine dei centri o gestione dei bordi rispetto al riferimento.

Una miniatura Full persistente conserva il livello lineare necessario e la trasformata dalle coordinate sorgente. La texture finale alla dimensione fisica esatta può rimanere in RAM/GPU. Non reintrodurre una miniatura fissa da 320 pixel ulteriormente ingrandita dalla UI come risultato Full.

## 5. Budget unico, ammissione e liberazione

### 5.1 Contabilità

Il limite delle singole mappe viene sostituito da un `MemoryBudget` condiviso. Ogni allocazione applicativa ha dimensione verificata, identificatore e proprietario; le copie fisicamente distinte sono conteggiate separatamente. Spostare un buffer dal job alla cache trasferisce la prenotazione, senza sommarlo due volte.

```text
B = budget complessivo effettivo
M = memoria di base stimata app + worker, esclusi i blocchi attribuiti a C
C = allocazioni immagine CPU/GPU vive, inclusi lavori e risultati in volo
J = byte futuri ancora da allocare, prenotati per i lavori ammessi
H = margine per runtime, allocatori, driver e stime incerte

ammetti(job) soltanto se M + C + J + picco_incrementale(job) + H <= B
```

Quando un job alloca un buffer, trasferire atomicamente il relativo credito da `J` a `C`; una copia aggiuntiva consuma credito aggiuntivo. Un buffer completato resta in `C` anche nella coda risultati o nella cache, fino al rilascio effettivo. Non sommare il picco intero del job ai buffer che ne occupano già una parte. `M` e il margine si calibrano senza ricontare questi blocchi.

RSS/footprint/VRAM osservati sono un controllo esterno del modello, non un'altra somma da aggiungere integralmente a `C + J`. Dove la condivisione non è dimostrabile, mantenere una stima prudente e indicarne l'incertezza. La memoria GPU di Core Image nel worker appartiene anch'essa al budget.

Il picco incrementale include lettura privata del file, copie IPC realmente concorrenti, decoder nativo, raster, passaggi del filtro, codifica/decompressione, upload/readback e risultati non ancora consumati. Code limitate per numero di messaggi non bastano a limitare questi byte.

Le risorse dei decoder non sono tutte contabilizzabili con precisione prima dell'avvio. Per backend/classi di input non qualificati si usano stime conservative e supervisione; si dichiara il limite come budget di ammissione e obiettivo misurato, non come tetto kernel. Un input entro 64 Mi pixel può comunque richiedere più del default di 2 GiB nel percorso full-frame.

Prima di fissare quel default, la fase A deve misurare i picchi del corpus 12/24/45 MP anche in Piena. Una migrazione che rende inapribili file prima utilizzabili non supera il gate: ridurre copie/intermedi o ricalibrare esplicitamente il default automatico entro la frazione di RAM dichiarata. Non aumentare di nascosto un limite manuale. Il numero di file nel catalogo non giustifica moltiplicare il budget per 1.000.

### 5.2 Politica di residenza

1. Proteggere i blocchi minimi che coprono le viste attive e i risultati in uso. La protezione riguarda miniature/livelli/tile, non tutti i raster completi delle foto visibili.
2. Riservare spazio per il lavoro che sblocca il primo contenuto utile; espellere le cache opportunistiche prima di rifiutarlo.
3. Conservare un piccolo insieme recente e le rappresentazioni vicine entro quota. Separare miniatura, preview e dettaglio pesante per evitare che un RAW espella tutte le miniature.
4. Usare LRU segmentata con ingresso provvisorio: una lunga scansione attraversata una sola volta non deve eliminare immediatamente tutte le foto visitate ripetutamente.
5. Espellere prima elementi non protetti, lontani e poco riutilizzati; a parità considerare costo di ricostruzione e presenza su SSD. Non basare tutto sul numero di foto.
6. Gli `Arc` dei lavori attivi e i buffer in volo mantengono la loro prenotazione anche dopo la rimozione dalla mappa. Il rilascio effettivo restituisce credito al budget.
7. In pressione memoria/termica sospendere prefetch e costruzione di sfondo, ridurre concorrenza ed espellere dati opportunistici. Un timer di permanenza non impedisce la liberazione urgente.

Se il prossimo lavoro Full non entra: serializzare, ridurre intermedi o usare un provider regionale/streaming realmente disponibile. Se continua a non entrare, conservare il contenuto provvisorio con stato esplicito e mostrare memoria richiesta/limite. Nessun passaggio silenzioso a una qualità inferiore dichiarata completa, nessun superamento del limite scelto per aumentare l'utilizzo CPU.

## 6. Cache su SSD e correttezza del riuso

### 6.1 Evolvere il formato e la directory esistenti

Conservare `.truerenderer-cache` dentro la cartella aperta, come autorizzato in ADR 0005, e le primitive di riconoscimento/accesso già implementate. Una directory omonima priva del corretto `OWNER` rimane intatta e non viene adottata. La cartella non scrivibile produce un percorso RAM/decoder con stato visibile; non cambiare automaticamente posizione o quota.

```text
<cartella foto>/
  <originali in sola lettura>
  .truerenderer-cache/
    OWNER                  riconoscimento della cache gestita
    cache.lock             protocollo di esclusione condiviso
    entries/<sha256>.tvc    artefatti v1 e nuovi record tipizzati v2
    tmp/<sha256>.part       temporanei entro la medesima quota
```

La versione del formato è distinta dal marker di proprietà della directory. Per il primo incremento mantenere nomi gestiti, estensioni e protocollo di lock della 0.1.4: il vecchio GC conta i nuovi `.tvc` nella stessa quota, anche se non ne interpreta i pixel. Le chiavi v2 hanno namespace distinto. Non introdurre una seconda gerarchia invisibile alla contabilità esistente.

| Tipo | Formato iniziale | Regola |
|---|---|---|
| Miniatura Standard o Full | Header limitato/versionato e RGBA32F little-endian lossless | Qualità, grafo e geometria registrati; nessun clamp RGB intermedio |
| Preview o dettaglio più grande | Stessi campioni, suddivisibili in record indipendenti | Descrittore limitato con dimensioni/trasformata e riferimenti ai blocchi necessari |
| Piramide v1 esistente | Lettore attuale completo e verificato | Riuso opportunistico entro budget; nessuna conversione in massa all'apertura |
| Frame di presentazione | Texture/raster della vista, transitorio in RAM/GPU | Non diventa la sorgente persistente; dipende da display e geometria |

Un obiettivo iniziale di circa 4 MiB di payload per record limita il lavoro sotto lock. Una preview grande può usare blocchi di righe o tile e un descrittore `.tvc`, pubblicato per ultimo. Il blocco di archiviazione non cambia il grafo di filtraggio: assemblare il livello necessario, oppure caricare anche gli aloni richiesti dal filtro. Non implica decode RAW regionale. Limiti su record, riferimenti, dimensioni e output totale impediscono catene arbitrarie di descrittori. Un blocco mancante rende incompleto il derivato, senza obbligare a scartare le altre miniature valide.

Il primo formato resta non compresso e riusa SHA-256 e i controlli esistenti. Zstd lossless è una successiva ottimizzazione da misurare per classe di artefatto: oggi è una dipendenza assente, da qualificare e bloccare se il beneficio supera codifica, decompressione, copie e isolamento. Nessun fp16, sRGB8 o lossy implicito. Non serializzare `to_display()` come dato lineare: oggi include conversione/clamp sRGB8 e composizione con il surround.

Le dimensioni rendono concreta la quota: 512×342 RGBA32F sono circa 2,67 MiB, quindi 1.000 miniature simili circa 2,61 GiB; 2.048×1.366 sono circa 42,69 MiB, quindi 1.000 preview circa 41,69 GiB, prima di header e blocchi aggiuntivi. Il default di 4 GiB può conservare molte miniature e un insieme selettivo di preview; non promette tutte le preview 2K di 1.000 RAW. Scegliere la dimensione realmente necessaria alle celle e misurare la compressione senza presumere un rapporto favorevole.

### 6.2 Identità e invalidazione senza indebolire la verifica

Conservare la codifica canonica tipizzata e SHA-256. Estendere la chiave attuale con tipo/versione dell'artefatto, qualità, stadio, formato, dimensioni/livello/regione, trasformata dei centri, orientamento applicato e policy alpha. Mantenere digest sorgente, backend, build OS, spazio di lavoro, ricetta RAW e versione del filtro che già influenzano il riuso. Sostituire in futuro la versione globale dell'app con revisioni semantiche della pipeline solo dopo test che dimostrino l'invalidazione di ogni modifica ai pixel; inizialmente la chiave resta conservativa.

Il digest sorgente resta lo SHA-256 dell'intero contenuto, con l'accelerazione e il riuso di `SourceSnapshot` già implementati. Un token filesystem può localizzare una candidata o invalidare presto un risultato, ma non autorizza un hit disco basato solo su percorso, dimensione e timestamp. Alla riapertura verificare la sorgente prima di accettare l'artefatto, come in ADR 0005; includere il costo di lettura/hash nella latenza e la vita dello snapshot nel budget. Non trattenere gli snapshot di tutti i RAW della cartella. Entro una revisione attiva, richieste compatibili condividono lo snapshot privato già acquisito; hash, hit e miss usano quella stessa identità.

Uno snapshot privato lega hash e decode agli stessi byte; non prova da solo che un file modificato durante la lettura rappresenti uno stato atomico dell'originale. Mantenere i controlli prima/dopo lettura e il contratto di revoca/revisione del broker. Checksum dell'artefatto significa rilevazione di corruzione, non autenticazione contro chi riscrive anche il checksum: Standard e Piena restano Anteprima.

Tema, viewport e profilo monitor non entrano nella cache persistente lineare; entrano nelle chiavi di presentazione quando cambiano i pixel. Annotazioni e ordinamento non invalidano gli artefatti. Non dedurre compatibilità Full da una preview Standard. Il riuso v1 è consentito solo per una chiave/pipeline riconosciuta e dopo la validazione completa attuale, seguita eventualmente dall'estrazione dei derivati richiesti; se non entra nel budget o non è compatibile, è un miss. Le vecchie entry rimangono soggette alle quote/scadenza esistenti, senza migrazione obbligatoria di tutta la cartella.

### 6.3 Pubblicazione asincrona, quote e concorrenza

1. Consegnare il risultato utile alla vista prima della persistenza. La coda cache ha limiti di descrittori, byte trattenuti e spazio disco prenotato; un job opzionale non trattiene `Arc<Pyramid>` solo per salvare una miniatura. Sotto pressione rinunciare alla scrittura e liberare i dati.
2. Riservare input/scratch/output RAM prima di preparare un record. Preparazione e possibile codifica avvengono fuori dal lock cartella; tenere in RAM solo output limitato e contabilizzato. Nel protocollo iniziale, creare e scrivere il `.part` soltanto mentre si detiene il lock esclusivo esistente: un vecchio GC elimina i `.part` che trova liberi.
3. Sotto lock verificare proprietà, quota, temporanei concorrenti, spazio libero e cancellazione, poi scrivere, validare lunghezze/checksum, sincronizzare e pubblicare con rename esclusivo già adottato. La prenotazione disco è ricontrollata in questo punto fra processi; una stima locale non prenota lo spazio contro altre istanze.
4. Rilasciare il lock fra record e dare precedenza alle letture della vista. Non mantenere il lock per codificare tutta una preview o per scrivere una piramide completa. Misurare la durata dei lock e ridurre i blocchi se il writer peggiora P0; nessuna promessa di durata massima del filesystem sotto carico.
5. Pubblicare il descrittore completo dopo i suoi blocchi. Un crash può lasciare blocchi orfani eliminabili o riferimenti mancanti: il lettore accetta solo l'insieme verificato, con limiti prima delle allocazioni. Il primo incremento non richiede un nuovo database SQLite; l'indice in RAM è ricostruibile dai record gestiti.
6. Conservare lock condivisi/lease per la lettura e i blocchi ancora in uso; il GC non cancella file sotto lettura. Dopo la copia privata validata in RAM, la cancellazione del file cache non invalida i campioni. Quota, statistiche e manutenzione comprendono entry v1/v2, descrittori, orfani e temporanei.

Prevedere il costo dell'intero derivato prima di iniziarne i blocchi: il limite del temporaneo massimo si applica al lavoro logico, e suddividerlo non permette di aggirarlo. Proteggere localmente i blocchi in preparazione dalla propria LRU e verificare sotto lock che siano ancora presenti prima di pubblicare il descrittore. Un'altra istanza può averli espulsi fra due record: annullare o ritentare una sola volta entro quota, senza scritture infinite né pubblicazioni dichiarate complete senza verifica.

Non accorciare ulteriormente i lock lasciando temporanei non protetti: richiederebbe prenotazioni persistenti e un protocollo nuovo compatibile fra processi. Cambiare solo `OWNER` non revoca un'istanza vecchia già in esecuzione. Un simile cambiamento richiede migrazione coordinata e prove specifiche; non è necessario per consegnare gli artefatti piccoli e il writer asincrono.

Mantenere la quota unica per cartella, con LRU/scadenza. Distinguere le classi e proteggere una quota minima iniziale del 20% per miniature, prestando lo spazio libero alle altre classi; è una soglia minima recuperabile, non un tetto del 20% alle miniature. Il resto segue domanda e riuso. Il dettaglio pesante si persiste selettivamente. Una vecchia istanza può eliminare record nuovi secondo la sua LRU: ciò deve comportare un miss sicuro, senza promettere la stessa politica di residenza fra versioni.

Quando la foto si allontana, una copia SSD valida evita nuove scritture. Se manca, persistere eventualmente i derivati piccoli a bassa priorità; l'espulsione urgente non attende codifica o I/O. Disco pieno, quota insufficiente o cache disabilitata lasciano disponibile il percorso RAM/decoder entro budget. Il writer annotazioni rimane separato e durevole. La pulizia resta confinata alla radice riconosciuta, senza seguire link/reparse point o coinvolgere originali, libreria e backup.

### 6.4 Letture, isolamento e contesa

Estendere il lettore Rust non compresso della 0.1.4 con gli stessi controlli su magic, header limitato, aritmetica delle dimensioni, lunghezza esatta, campioni finiti/alpha e checksum. Copiare e validare i byte che verranno effettivamente usati prima dell'upload GPU; non validare un file e poi usare un mapping modificabile senza nuova protezione. Il pool I/O/validazione host ha quota e priorità proprie e non occupa i due decoder RAW.

Distinguere `Hit`, `Missing`, `Invalid`, `Busy` e `Disabled`. Un lock occupato genera un retry limitato con priorità P0 e sospensione delle nuove scritture locali, senza spin né un nuovo sviluppo RAW a ogni tentativo. Dopo un'attesa massima dichiarata, un eventuale fallback decode è singolo, deduplicato e ammesso entro budget; registrare la contesa separatamente dai miss reali. Il limite del writer di un'altra istanza rimane osservabile.

Se si introduce un codec compresso esterno, la decompressione passa dal worker isolato con output massimo prenotato e uno slot breve riservato secondo §7. La cache non è un'autorizzazione a decodificare nuovi originali: il gate corpus rimane prima del riuso nel percorso pipe e dentro il decoder. Nessun flag generico `trusted_cache`. Il backend persistente macOS esistente è il punto di partenza; Windows richiede primitive/isolamento OS qualificati, senza dichiararlo già operativo.

## 7. Scheduler, vicinanza e cancellazione

### 7.1 Descrivere la domanda delle viste

La UI pubblica un `ViewDemand` compatto quando cambiano selezione, area visibile, ordinamento, filtro, zoom, DPI o qualità. Contiene asset/revisione, ruolo della vista, regione sorgente in f64, dimensioni fisiche, qualità e scadenza. `state.visible` identifica l'elenco filtrato: non confonderlo con le sole celle effettivamente sullo schermo.

Tre generazioni distinte:

- `domain_generation`: cambio cartella/dominio; conserva la revoca di autorità del broker e il riciclo dei servizi previsti oggi.
- `view_generation`: domanda visuale corrente; rende obsoleti i consumatori senza riciclare un servizio XPC a ogni scroll.
- `settings_generation`: qualità/contratto display/impostazioni che cambiano la richiesta; impedisce l'arrivo tardivo di un risultato incompatibile.

La chiave del lavoro identifica l'artefatto, non il singolo consumatore. Richieste equivalenti da griglia e filmstrip condividono decode e blocchi quando compatibili. Un gruppo per asset, stesso snapshot, ricetta e dominio coordina le richieste: un Full già in corso può produrre anche i derivati Standard ammessi dal contratto, mentre un decode Standard ridotto non soddisfa Full. Condividere prima le dipendenze utili; dimensioni o regioni diverse richiedono una verifica geometrica, non una deduplica per solo asset. Se una miniatura già in coda diventa la foto aperta, promuovere il job e le sue dipendenze; la deduplicazione non deve lasciarlo bloccato in priorità bassa. La 0.1.5 promuove le richieste già pendenti anche durante il lookup; mantenere questa proprietà nelle evoluzioni.

### 7.2 Priorità

| Priorità | Lavoro |
|---|---|
| P0 | Primo contenuto della foto aperta, confronto e miniature della griglia attiva |
| P1 | Raffinamento delle regioni visibili alla qualità richiesta |
| P2 | Regioni appena oltre il viewport, nella direzione del pan |
| P3 | Miniature effettivamente visibili nei pannelli secondari |
| P4 | Righe adiacenti nella griglia |
| P5 | Preview della foto precedente/successiva e breve anticipo nella sequenza |
| P6 | Preparazione esplicita della cartella, persistenza cache, indicizzazione e statistiche differibili |

I salvataggi durevoli mantengono il writer e la coda separati; non sono job P6 scartabili. L'aging vale fra lavori compatibili, senza permettere al prefetch di trattenere le risorse necessarie al primo contenuto.

Code limitate sia per descrittori sia per memoria incrementale prenotata. Prima implementazione: massimo 64 descrittori decode come oggi e due servizi XPC; risultati completi e upload hanno anche un tetto in byte. All'arrivo di P0, una coda piena deve poter rimuovere lavori speculativi. Nessuna coda può trattenere 64 sorgenti full-frame già allocate.

Separare lettura/hash snapshot, lettura/verifica cache, decode RAW, compute e scrittura. Le letture di artefatti pronti non aspettano il completamento di un demosaicing. Limitare anche i task I/O e gli snapshot in byte, leggere/hashare per blocchi cancellabili e conservare capacità per la domanda visibile invece di saturare il volume col prefetch.

Durante navigazione interattiva, ammettere al massimo **un decode lungo speculativo**; il secondo servizio dà precedenza alla domanda visibile. Due decode lunghi effettivamente necessari al visibile possono procedere entro budget se le misure di latenza lo consentono: la cache fp32 ha già letture indipendenti nell'host. Se una preview Standard richiede in realtà sviluppo completo, appartiene alla classe lunga. Una nuova richiesta P0 Full può ancora attendere chiamate native non interrompibili: dichiarare e misurare quell'attesa, riducendo la concorrenza quando peggiora la navigazione, senza promettere prelazione.

L'eventuale codec cache isolato cambia questa scelta: durante navigazione con artefatti compressi riservare uno dei due servizi ai lavori brevi e non assegnargli RAW lunghi. Il costo di questa riserva, comprese le copie XPC, rientra nell'A/B prima di abilitare la compressione. Non introdurre Zstd solo per ridurre i byte se peggiora il tempo al visibile.

La preparazione esplicita della cartella può usare entrambi i decoder quando non c'è domanda interattiva, sempre entro budget. Al ritorno dell'utente sospendere nuove ammissioni di sfondo; i due lavori già non interrompibili possono ritardare il primo decode. Anche un batch deve lasciare indipendenti letture cache e presentazione. Non sacrificare il tempo al primo contenuto per mostrare due worker occupati.

### 7.3 Prefetch e isteresi

- Griglia: partire da una schermata adiacente nella direzione di scorrimento e mezza nella direzione opposta, limitate dal budget. Estendere solo se la misura dimostra utilità.
- Viewer: preparare una preview della precedente e della successiva; in navigazione stabile, fino a due ulteriori preview nella direzione prevalente. Non avviare demosaicing Full speculativi se impediscono un P0.
- Pan/zoom: richiedere regioni e livelli adiacenti realmente utilizzabili dal provider; nessuna finta promessa di decode regionale di un RAW full-frame.
- Anticipo temporale iniziale: `clamp(latenza_p95_recente + margine, 100 ms, 500 ms)`. Convertire velocità e latenza in distanza nell'ordine corrente, con tetto di byte. I valori sono parametri da tarare.
- Conservare una fascia recente per circa 1 secondo quando c'è spazio. Il bordo di espulsione è più ampio di quello di precaricamento; piccoli movimenti avanti/indietro non devono provocare continui caricamenti.
- Salti grandi, filtri e cambi di direzione invalidano il prefetch ormai inutile. Non attraversare tutte le foto intermedie fra due selezioni lontane.

Un job senza consumatori può completare un artefatto quasi pronto solo se il costo residuo è piccolo e non blocca un lavoro prioritario. In ogni altro caso si cancella ai confini di blocco. Le chiamate native non cancellabili e i dispatch GPU già inviati non vengono dichiarati terminati prima del loro completamento. Il riciclo XPC resta una misura per timeout/sicurezza: con il ritardo di launchd osservato, usarlo a ogni cambio foto peggiorerebbe la navigazione.

## 8. CPU e GPU: utilizzo elevato durante lavoro utile

### 8.1 Criterio di prestazione e profili

In profilo **Prestazioni** il motore deve sfruttare i core disponibili, vettorizzazione, GPU e sovrapposizione I/O/calcolo per svuotare il lavoro utile rapidamente. Non viene accettata una pipeline che resta seriale per scelta accidentale mentre esistono blocchi indipendenti e risorse disponibili.

Il criterio di successo è tempo alla prima immagine, latenza di interazione e immagini elaborate al secondo entro memoria e qualità. L'utilizzo CPU/GPU è una misura diagnostica: una cache hit può essere velocissima usando poco calcolo; un RAW può essere limitato da memoria o decoder. Non impostare un obiettivo artificiale del 100% simultaneo di entrambe le unità, che potrebbe aumentare contesa e durata del lavoro.

| Profilo | Regola iniziale |
|---|---|
| Prestazioni | Pool applicativo fino a `max(1, parallelismo_disponibile - 1)` thread; durante un batch esplicito senza interazione, possibile uso di tutti i thread se throughput migliora |
| Bilanciato | Pool iniziale circa metà del parallelismo disponibile; prefetch corto, aumento guidato dalle misure |
| Risparmio | Uno o pochi lavori concorrenti, priorità al visibile, riduzione del prefetch e sospensione della costruzione di sfondo |

Questi limiti riguardano il pool controllato dall'app. ImageIO/Core Image e futuri codec possono creare thread propri: il controllore considera anche il loro carico ed evita di moltiplicare pool pieni per ogni immagine. Una preferenza sui thread non è un tetto di utilizzo imposto dal sistema operativo.

A immagine ferma e senza lavori pendenti non avviare nuovi calcoli o submit periodici. Non precalcolare 1.000 RAW soltanto per tenere occupato il processore. Il comando esplicito di preparazione della cartella, invece, deve procedere a throughput elevato con progresso e pausa.

### 8.2 Parallelismo CPU

1. Separare orchestrazione, I/O, decode, calcolo e scrittura durevole. Il thread UI non esegue letture, decompressioni, filtri o attese GPU sincrone.
2. Introdurre un unico pool di calcolo a work stealing, con Rayon 1.11.0 già presente nel lockfile della 0.1.5. Nessun pool completo per ogni miniatura.
3. Parallelizzare le righe/blocchi indipendenti del filtro. Ogni task produce pixel disgiunti e ha una quota esplicita di scratch; gestire aloni e intermedi dei passaggi senza buffer full-frame superflui.
4. Precalcolare e riusare i coefficienti compatibili per geometria/versione del filtro; limitare anche la cache dei coefficienti.
5. Conservare un'implementazione scalare di confronto e aggiungere NEON su macOS arm64, AVX2 con rilevamento runtime su Windows x86-64. Nessun requisito AVX2 globale sul binario e nessun cambio dei risultati oltre la tolleranza autorizzata.
6. Istogrammi: accumuli per blocco e riduzione finale, evitando un lock per pixel. Il calcolo sorgente completo è richiesto quando serve, non per ogni apparizione di una miniatura.
7. Adattare granularità e concorrenza al picco di memoria e ai costi misurati. Il parallelismo non cambia il grafo dei livelli o l'ordine semantico colore/alpha.

### 8.3 Compute GPU del renderer

È una parte richiesta del progetto, non un semplice mantenimento della presentazione wgpu esistente. Il lockfile attuale contiene **wgpu 30.0.1**, attraverso eframe 0.36.1; progettare contro quella versione senza aggiornamenti impliciti.

- Condividere il `Device` e la `Queue` di eframe per il renderer. Pipeline WGSL per riduzione multistadio, ricampionamento del viewport, operazioni colore disponibili e composizione; CPU semanticamente equivalente per ogni stadio.
- Nella baseline 0.1.4 il viewport era calcolato su CPU e caricato sulla GPU. Nel percorso 0.1.5 caricare blocchi riutilizzabili, calcolare e mantenere il risultato sulla GPU fino alla presentazione, evitando un readback a ogni cambio viewport. Readback soltanto quando serve per persistenza, ispezione o verifica, asincrono e prenotato; preferire per la cache i campioni CPU già disponibili quando compatibili.
- Piena mantiene RGBA32F per il working; usare `textureLoad` e accumuli espliciti quando servono. Non presumere filtraggio hardware di RGBA32Float né storage sulla swapchain. Se una capability manca, scegliere un percorso supportato o CPU.
- Prima variante di workgroup 8×8, poi poche alternative come 16×16 solo dopo verifica dei limiti e confronto. Fusione di passaggi solo con identico dominio e semantica: non sostituire una catena di filtri con una diversa per ridurre i dispatch.
- Ring di staging riutilizzabile e upload raggruppati. Dimensioni, allineamenti e byte in volo entrano nel budget; nessun readback o `poll(Wait)` bloccante nel disegno UI.
- Suddividere il lavoro di sfondo in submit corti; target iniziale di circa 2 ms per lotto stimato prima di rivalutare P0. Non promettere la prelazione di un dispatch già inviato. La dimensione dei lotti si adatta alla latenza osservata.
- Le callback di completamento rilasciano lease/crediti e inviano eventi brevi. Un segnale logico di cancellazione non autorizza a riutilizzare un buffer ancora in uso. La semantica di completamento e polling va rispettata secondo [Queue wgpu 30.0.1](https://docs.rs/wgpu/30.0.1/wgpu/struct.Queue.html).
- Con timestamp query supportate misurare upload/compute separati. Se non disponibili, indicare tempi indiretti e misurare comunque il risultato end-to-end.

Il percorso GPU Full si abilita per gli stadi/dispositivi che superano il confronto con CPU e le prove fisiche della vista. Una diagnostica aritmetica di 4.096 campioni non basta a qualificarlo. Se il compute fallisce si passa al calcolo CPU mantenendo il device di presentazione quando sano; un device perso richiede il recovery completo della UI previsto dall'architettura.

### 8.4 Decoder macOS: verificare e accelerare il render nativo

Nella baseline 0.1.4 il render nativo creava un `CIContext` per ogni chiamata e richiedeva il renderer software tramite `kCIContextUseSoftwareRenderer: YES`. La 0.1.5 riusa il contesto per servizio XPC; l’esperimento Metal è separato. Qualificare un contesto su dispositivo Metal disponibile, mantenendo precisione fp32, ricetta, orientamento e comportamento alpha. Core Image gestisce stato e cache interni: il riuso va bilanciato con memoria residente e riciclo del dominio. Riferimenti API: [CIContext](https://developer.apple.com/documentation/coreimage/cicontext), [useSoftwareRenderer](https://developer.apple.com/documentation/coreimage/cicontextoption/usesoftwarerenderer).

La documentazione di `useSoftwareRenderer` precisa che l’opzione non ha effetto sulle piattaforme senza OpenCL: il flag nel codice non misura da solo il backend effettivo. Cambiare una singola opzione non prova che ogni stadio del RAW sia eseguito in GPU. Misurare CPU, tempi del decoder e attività GPU, mantenendo il percorso software come confronto. Se il renderer Metal non è disponibile nel servizio isolato, usare il percorso software all'interno dello stesso isolamento, senza estendere accessi filesystem/rete.

Core Image nel worker e wgpu nell'host hanno contesti e code di processi distinti. Non assumere una `MTLCommandQueue` condivisa attraverso XPC né zero copie grazie alla memoria unificata. Il broker coordina ammissione e numero di lavori GPU dei worker per contenere la contesa con il viewport; il costo delle copie IPC rimane misurato.

Questo incremento conserva **due servizi XPC**. Usare fino a quattro decoder richiederebbe una verticale separata su bundle, identità dei servizi, quote e prove: aumentare i thread host non crea automaticamente quattro servizi indipendenti.

### 8.5 Adattamento dinamico

Ogni finestra di misura aggiorna attese P0, throughput, byte trasferiti, occupazione delle code e pressione memoria. Aumentare concorrenza/prefetch a piccoli passi quando migliora il throughput senza deteriorare la latenza; ridurli quando peggiora il tempo al visibile. Applicare isteresi per evitare oscillazioni.

Su Windows il budget GPU fornito da DXGI è un segnale dinamico da considerare, non la VRAM nominale utilizzabile tutta dall'app. [DXGI_QUERY_VIDEO_MEMORY_INFO](https://learn.microsoft.com/en-us/windows/win32/api/dxgi1_4/ns-dxgi1_4-dxgi_query_video_memory_info). Su macOS affiancare footprint dei servizi e segnali documentati di pressione. Quando un dato non è disponibile, riportarlo come tale e usare una strategia prudente.

Cambio adattatore solo tramite riavvio controllato del motore, non durante un pan. Batteria e segnali termici documentati possono ridurre lavoro speculativo; la qualità scelta dall'utente resta una proprietà separata e non viene silenziosamente abbassata.

## 9. Decoder e protocollo: richieste esplicite

L'attuale parametro `edge` non basta. Nel decoder nativo il ridimensionamento avviene dopo la produzione del raster completo: inviare `edge = 2048` senza cambiare quel percorso non elimina il picco full-frame e non realizza questa ottimizzazione.

Modello di richiesta proposto, da tradurre in tipi Rust e protocollo versionato:

```text
DecodeIntent:
  Probe                  metadati/capability sufficienti al preflight, con limiti propri
  StandardPreview        dimensioni massime e qualità standard esplicita
  FullSource             campioni nativi per sviluppo completo
  RegionOrLevel          soltanto quando la capability del backend è reale
  ReadCacheArtifact      solo per codec cache da isolare; tipo/versione e output massimo

Request:
  request_id, asset_id, source_revision, intent, quality,
  domain_generation, expected_dimensions, maximum_output_bytes,
  recipe_revision

HostJob (non serializzato):
  cancellation_token, consumer_set, budget_lease, snapshot_lease
```

Il token di cancellazione e i lease sono oggetti host, non puntatori inviati su IPC. Un eventuale messaggio `Cancel(request_id)` richiede supporto negoziato; in sua assenza il broker scarta il risultato obsoleto e attende il completamento reale prima di restituire i crediti.

Il probe esegue i parser non fidati nel worker e non è un'operazione gratuita o senza quota. Riservare il suo input/stato prima dell'avvio. Per il decode Standard valutare [CIRAWFilter.scaleFactor](https://developer.apple.com/documentation/coreimage/cirawfilter/scalefactor), mantenendo una ricetta ridotta identificabile; verificare se riduce effettivamente lavoro e memoria sul backend. Draft, sharpening, WB e tone mapping non cambiano implicitamente con il profilo Prestazioni.

La risposta dichiara dimensioni sorgente e raster, scala, origine/trasformata della regione, precisione, ricetta, decoder e qualità effettiva. Il broker verifica tutto contro la richiesta, incluso output massimo e campioni finiti. Un raster ridotto non viene accettato come risposta FullSource; nessuna promozione per il solo nome del comando.

Host e servizi negoziano una versione/capability coerente. Un bundle vecchio non interpreta i nuovi messaggi; l'incompatibilità ha errore esplicito e non attiva un decoder nell'host. La policy di sicurezza dell'artefatto cache è distinta dal suo formato e non viene sostituita dal flag `Full`/`Standard`.

Per backend full-frame, la prima ottimizzazione libera rapidamente sorgente/intermedi dopo aver prodotto i derivati richiesti. Un vero picco bounded per righe/tile richiede supporto del backend o spool qualificato e viene consegnato come passo successivo; non lo si deduce dalla granularità della cache.

## 10. Scenari e stati di errore da rendere espliciti

| Scenario | Risultato richiesto |
|---|---|
| Cartella con 1.000 RAW mai vista | Elenco e celle visibili subito schedulabili; sviluppo limitato alla domanda; nessuna materializzazione iniziale di tutte le sorgenti |
| Seconda visita alla stessa griglia | Miniature da RAM/SSD senza risviluppare RAW se gli artefatti corrispondenti sono validi |
| Salto dalla foto 10 alla 900 | P0 sulla 900 e prefetch del nuovo intorno; cancellazione dei lavori speculativi precedenti |
| Oscillazione fra due foto | Riutilizzo delle rappresentazioni recenti; nessuna scrittura SSD a ogni passaggio |
| Piena richiesta con preview Standard presente | Preview corretta della stessa foto durante il raffinamento; completamento soltanto con dettaglio Full |
| Cache Full sufficiente ad Adatta, poi zoom 1:1 | Riutilizzo dei livelli bassi come copertura; caricamento di dettaglio mancante, senza fingere che la cache sia completa |
| Miniatura ancora visibile, sorgente CPU espulsa | La miniatura continua a essere disegnata e non genera da sola una nuova richiesta FullSource |
| Limite memoria troppo basso | Riduzione di concorrenza e cache; se il decode non entra, messaggio con limite/costo stimato, senza retry infinito |
| Cache disabilitata o disco pieno | Uso di RAM e decoder entro quota; nessuna perdita di annotazioni e nessun blocco della UI per il writer cache |
| Sorgente modificata o rimossa | Invalidazione della revisione; nessun hit disco dichiarato valido senza lo snapshot verificato. Stato di file cambiato/non disponibile; eventuale frame già in RAM è indicato come precedente |
| Payload cache corrotto | Scarto della voce, ricostruzione quando possibile e massimo un tentativo automatico; conteggio diagnostico |
| Cache occupata da writer/GC | Retry limitato in coda I/O, precedenza al visibile e nessuna tempesta di decode; contesa distinta dal miss |
| Risultato tardivo di un'altra foto/qualità | Può entrare in una cache compatibile se utile; non sostituisce il frame corrente |
| Perdita GPU | Invalidazione delle risorse GPU e una ricreazione controllata; stato utente separato dal renderer |

La regola di continuità vale dopo il primo contenuto valido e durante il raffinamento della stessa immagine/revisione. Al passaggio a una foto mai pronta può esserci un placeholder dichiarato; non mantenere la vecchia foto facendo credere che sia la nuova selezione.

## 11. Mappa delle modifiche e dipendenze

La tabella distingue i moduli effettivamente presenti nella 0.1.5 dalle estensioni ancora proposte. Non serve creare moduli duplicati per far coincidere i nomi con quelli del progetto iniziale.

| Area | File/moduli interessati | Responsabilità |
|---|---|---|
| Dominio immagine | `tr-core`: `preview.rs`, `provider.rs`, `resample.rs`, `protocol.rs` | Qualità, artefatti, geometria, capability e protocollo |
| Stato e scheduling | `tr-app`: `scheduler.rs`, `lib.rs`; budget in `tr-core/src/budget.rs` | Domanda delle viste, dipendenze, quote/lease e generazioni; tipi indipendenti dalla UI |
| Cache e impostazioni | `apps/desktop/src/cache/`: `mod.rs`, `directory.rs`, `artifact.rs`, `writer.rs`, `settings.rs` | Riusare backend e controlli 0.1.4, aggiungere artefatti v2, writer separato, statistiche e migrazione preferenze. Nessun secondo backend o nuovo database obbligatorio |
| Renderer | `tr-render`: `presenter.rs`, `preview_compute.rs`, `resident_compute.rs`, shader WGSL; CPU in `tr-core/src/compute.rs` | Frame autonomi, livelli residenti, CPU/GPU e completamenti |
| Piattaforma | `tr-platform`: broker, `xpc.rs`, `resources.rs`; adattatori non macOS ancora da realizzare | Ammissione prima delle copie, pressione RAM/VRAM, capability e verifiche |
| Decoder | `tr-worker`, `native/macos/image_decoder.m/.h` | Intent espliciti, contesto riutilizzabile, decode ridotto/Full e codec cache isolati |
| Desktop | `apps/desktop/src/`: `ui.rs`, `service.rs`, `decode_pool.rs`, `main.rs`, `source_monitor.rs`, `graphics.rs`, `wake.rs` | Preferenze, selettore qualità, pubblicazione domanda, collegamento dei servizi |
| Verifica | `verify_previews.rs`, `scripts/test-preview-*.py`, probe `ui/navigation.rs` e `scripts/test-navigation-surface.py`; harness evento→frame completo ancora aperto | Benchmark riproducibili e regressioni del contratto visuale |

Rayon e l'eventuale Zstd richiedono scelta di versione, build riproducibile e aggiornamento dei notice delle dipendenze effettive. SHA-256 con accelerazione e snapshot resta quanto già implementato; non introdurre BLAKE3 in parallelo senza una necessità misurata. Non servono nuove dipendenze per approvare questo progetto documentale. Non modificare la licenza proprietaria o pubblicare cache, foto, database e toolchain.

Gli smoke correnti verificano stadi, risultati richiesti e screenshot; `cache.len()` rimane una statistica e un limite di residenza, non una condizione di completamento dell’intera cartella. Mantenere condizioni sul risultato richiesto e sugli eventi completati. Con residenza corretta è normale non tenere tutte le sorgenti in RAM: un test non deve obbligare l'app a farlo.

## 12. Misure e criteri di accettazione

### 12.1 Protocollo delle misure

**Primo probe della superficie, 15 settembre 2026.** `--navigation-smoke` esegue tre selezioni deterministiche (segnale radiale, paesaggio, ritorno al segnale) nel viewer reale; `scripts/test-navigation-surface.py` crea copie del corpus e cataloghi separati, confrontando CPU/GPU della medesima qualità. La prima selezione parte senza precedenti richieste immagine; la scansione del catalogo e la qualifica GPU sono già completate. Prefetch, filmstrip e ispettore sono disabilitati. Una corsa popola la cache; una nuova istanza ne verifica il riuso dal disco. Il terzo comando della seconda istanza misura il ritorno RAM. Contatori e residenza confermano lo scenario, senza dedurlo dal nome del test.

Il primo timestamp precede `Command::Select`; il secondo registra l'inserimento del raster esatto richiesto nel frame egui, prima del submit. Il terzo registra la ricezione del readback della superficie. Solo dopo si confrontano tutti i pixel della regione con CPU, entro un livello sRGB8; frame precedenti, riproiettati o con qualità inferiore non terminano la prova. Il readback include copia GPU, mapping e consegna dell'evento; il probe richiede repaint a 10 ms e può alterare lo scheduling. **Non misura il timestamp del compositor/monitor e non chiude i target evento→frame.** Cache OS non svuotata; SSD indica la cache applicativa riaperta, senza affermare una lettura fisica dal disco.

La corsa predefinita ha tre ripetizioni indipendenti per modalità/stato e riporta min/mediana/max. Lo script sopprime p95 sotto 100 processi indipendenti per scenario e p99 sotto 1.000; anche oltre queste soglie le stime sono descrittive, senza intervalli di confidenza o gate di accelerazione. Il corpus piccolo PNG, la geometria Fit e questo primo confine software non sostituiscono la campagna completa seguente: restano scroll/pan/zoom, cambi qualità, RAW, pressione, baseline storica e presentazione effettiva.

**Estensione delle transizioni.** L'opzione `--transitions` aggiunge zoom 300%, pan a centro deterministico, ritorno Fit, override Standard, override Full, 1:1 fisico e ritorno Standard/Fit. Sono azioni sintetiche sullo stato del viewer e sugli stessi helper di qualità/zoom, non eventi OS di mouse o rotella. Per ogni ridisegno della stessa sorgente si registra la frazione dell'area immagine coperta da draw esatti o riproiettati; una copertura zero fa fallire la traccia. La riproiezione può coprire solo la parte in comune con il vecchio ritaglio, senza inventare pixel delle zone mancanti. La correttezza finale è verificata sul readback, con tolleranza zero a 1:1. Queste osservazioni non attestano tutti i refresh del monitor né copertura sempre completa; il corpus sotto 2048 pixel non prova ancora transizioni di risoluzione RAW ridotta.

**Riserva di copertura.** Il presenter conserva facoltativamente un frame più ampio della stessa foto, digest, motore, dimensioni sorgente e vista, entro due slot e un quarto dei budget globale/GPU, con rilascio prioritario in caso di pressione o necessità di ammissione. I lease rimangono contabilizzati fino al completamento. Il runner `--transitions --require-full-coverage` richiede copertura geometrica completa a ogni ridisegno osservato della stessa sorgente; questo controllo non misura tutti i refresh fisici né garantisce copertura quando la riserva manca o viene espulsa. Contratto in ADR 0006.


Creare un harness di navigazione con sequenza deterministica di scroll, selezioni, cambio qualità, zoom e ritorni. La baseline primaria è **0.1.4 al commit indicato in apertura**, comprensiva di cache e snapshot accelerati; 0.1.3 è un confronto storico opzionale. Confrontare nuova CPU e GPU sulla stessa macchina, corpus, superficie e quota.

Misurare Piena contro il percorso completo 0.1.4. Per Standard riportare sia il confronto d'esperienza con la baseline sia l'A/B della stessa pipeline Standard con cache/parallelismo attivi e disattivi, mantenendo identici sorgente, ricetta ridotta, dimensioni e precisione. Separare il vantaggio della minore risoluzione da quello dell'architettura. I report cache già pubblicati sono un punto di partenza, non sostituiscono questo harness.

Corpus: sintetico deterministico per CI; raccolta autorizzata di RAW reali per modelli e dimensioni qualificati, senza pubblicare fotografie o percorsi personali. Includere 1.000 file distinti per il caso catalogo, subset da 12/24/45 MP, file privi di preview incorporata, alpha, 16 bit, dimensioni dispari e input rifiutati. Non usare copie identiche di un solo RAW per gonfiare il beneficio della deduplicazione.

Distinguere almeno: primo accesso senza cache applicativa; SSD popolato dopo riavvio; RAM/GPU calde; cache disabilitata; pressione memoria. Dichiarare lo stato della cache OS. Usare manifest con CPU, GPU, RAM, SSD/volume, OS/driver, build, decoder, qualità, DPI e alimentazione; almeno 100 prove indipendenti per p95, campioni sufficienti per p99, dispersione/intervalli di confidenza. Windows prova gli esterni solo dopo il suo isolamento reale; prima, riportare i limiti del corpus ammesso.

### 12.2 Gate funzionali e di memoria

Le caselle di §12 riguardano il requisito completo nel perimetro concordato, inclusi scenari e piattaforme richiesti. Una casella aperta può avere già codice e prove parziali: il dettaglio implementato/verificato è in §13 e in `STATO.md`.

- [ ] Una miniatura resta utilizzabile dopo l'espulsione della sorgente/piramide completa; la vista non ne richiede un nuovo decode senza bisogno di più dettaglio.
- [ ] Entrambe le qualità, il cambio rapido, il confronto e l'override 1:1 rispettano il contratto di §2; nessun badge Standard/Riferimento abilitato dal selettore.
- [ ] Qualità, quote e profilo persistono al riavvio; migrazione da 0.1.3/0.1.4 conserva Piena e le preferenze disco esistenti; configurazione corrotta e salvataggio fallito sono gestiti.
- [ ] Lo scheduler promuove i job già pendenti quando diventano visibili; coda piena di prefetch non impedisce l'ammissione di P0. Letture cache indipendenti dai RAW lunghi, retry `Busy` limitati e deduplicazione Standard/Full verificati.
- [ ] Nessuna ammissione supera i crediti disponibili; ogni allocazione e lease ha un solo conteggio; cancellazione e completamenti non perdono credito.
- [ ] Il picco osservato su ciascun scenario resta nel budget concordato con il margine dichiarato; eventuali scostamenti sono failure, non corretti contando meno processi o omettendo la GPU.
- [ ] Se il budget scelto non permette l'operazione, il risultato è un limite esplicito, non un superamento silenzioso o una qualità degradata dichiarata Full.
- [ ] Quote disco per cartella includono entry v1/v2, descrittori, orfani e temporanei; resize, più istanze e GC durante letture attive non corrompono il contenuto. Nessuna quota duplicata dalla migrazione.
- [ ] Crash prima/dopo pubblicazione, disco pieno, payload manomessi, file mancanti e revisione cambiata producono miss/errori contenuti. La verifica sorgente non viene ridotta al solo stat. Il writer asincrono non ritarda la consegna del risultato né trattiene memoria senza quota.
- [ ] La pulizia della cache conserva byte e integrità di originali, `library.sqlite`, configurazione e backup su copie di prova.

### 12.3 Gate di qualità e GPU

- [ ] Cache lineare Standard e Full: round-trip dei campioni fp32 bit-per-bit, incluso RGB negativo/oltre 1 e alpha; nessuna ricompressione lossy nascosta. Standard conserva spazio/precisione, distinguendo gli effetti del decode ridotto.
- [ ] Nuovo percorso CPU Full: stesso raster della baseline per lo stesso grafo e le stesse coordinate; mantenere esattezza LOD 0 a 1:1.
- [ ] GPU e SIMD: confrontare tutti gli stadi con CPU scalare, bordi/cuciture, orientamenti, alpha, livelli e transizioni. Target numerico iniziale da ratificare sul corpus: `abs(gpu - cpu) <= 1e-5 + 1e-4 * abs(cpu)` per canale lineare finito; nessuna tolleranza consente un campionamento geometrico errato a 1:1.
- [ ] Conservare almeno i controlli attuali di sinusoidi e screenshot; target di presentazione non oltre un livello per canale sRGB8 nelle regioni qualificate. Espandere Siemens, alias 2D, overshoot e display/DPI secondo §19.3.
- [ ] I target numerici sopra sono gate di questa ottimizzazione, non una qualifica ICC/ΔE00 o della modalità Riferimento. Se falliscono, lo stadio resta CPU; non allargare le soglie dopo aver visto il risultato.
- [ ] Decoder Apple Metal: verificare resa, precisione, orientamento, quote, riuso del contesto e isolamento. Differenze backend effettive producono distinta revisione di pipeline e report.
- [ ] GPU non compatibile, device loss, query temporali indisponibili e fallback CPU non causano risultati obsoleti o perdita di stato durevole.

### 12.4 Obiettivi di prestazione

Sono criteri da misurare, non promesse di prestazioni già raggiunte.

| Metrica | Obiettivo/criterio | Condizione |
|---|---|---|
| Prima miniatura da cache | p95 < 75 ms | Cartella 1.000 elementi; cache richiesta disponibile; stato OS dichiarato |
| Ritorno alla preview da cache pronta | Target iniziale p95 < 100 ms | Artefatto sufficiente alla qualità/geometria; tutte le letture, verifica sorgente e upload necessari sono inclusi |
| Scroll/pan/zoom con rappresentazioni residenti | p95 < 16,7 ms; p99 < 33 ms | Profilo 60 Hz e stesso contenuto/qualità; cache miss misurati a parte |
| Continuità del raffinamento | Zero frame vuoti dopo il primo contenuto valido della stessa revisione | Cambi scala e passaggio Standard → Piena |
| Ritorno su miniature persistite | Zero nuovi sviluppi RAW per gli artefatti già validi e sufficienti | Trace di ritorno con cache SSD dimensionata per il dataset richiesto |
| Uso delle risorse a riposo | Zero nuovi lavori immagine/submit dovuti a polling inutile | Nessuna interazione, animazione o costruzione di sfondo pendente |
| Sviluppo RAW a freddo | Limite iniziale di regressione Piena: +5% sul p95 di latenza; throughput non inferiore al 95% della baseline | Medesima ricetta, dimensioni e stato cache; limiti e significatività fissati prima del confronto |
| Accelerazione GPU/parallelismo | Miglioramento end-to-end oltre la variabilità della misura; P0 entro il limite di regressione +5% e i target interattivi | A/B sulla stessa qualità; scegliere CPU per gli stadi dove risulta più veloce |
| Preparazione di 1.000 anteprime | Throughput, tempo totale, picco memoria e scritture pubblicati nel report | Lavoro esplicito; distinguere sviluppo ridotto, completo ed eventuale estrazione incorporata |

La latenza parte dall'evento utente e termina al primo frame corretto presentato, non dal completamento dell'hash o dal momento in cui il job esce dalla coda. Separare gli stati RAM/GPU caldi e SSD dopo riavvio, ma includere sempre attesa, lettura/hash sorgente, verifica artefatto, conversione e upload quando necessari. I target 75/100 ms richiedono hardware/corpus e dimensioni degli artefatti dichiarati: una lettura del RAW originale può dominarli. Se falliscono, riportare il limite; non omettere l'hash o mostrare una candidata non verificata per farli passare.

Per i confronti stabilire prima la procedura statistica e gli intervalli di confidenza; se l'incertezza supera la soglia del 5%, il risultato è inconcludente e servono campioni aggiuntivi. Le soglie sono iniziali da ratificare con la baseline, non da cambiare dopo aver osservato il candidato.

Il requisito storico `< 30 s per 1.000 miniature RAW` riguarda l'estrazione di anteprime incorporate. Non attribuirlo allo sviluppo completo o ridotto di questo incremento. Stabilire un target di throughput del corpus RAW dopo la baseline della prima fase, prima di valutare l'accelerazione, e registrarlo senza abbassarlo a posteriori per far passare il gate.

Registrare anche hit/miss per livello, decodifiche duplicate evitate, lavori cancellati, byte scritti per foto, prefetch usato/sprecato, latenza della coda, utilizzo CPU/GPU dove disponibile, throttling e consumo della sessione. Le percentuali di utilizzo non sostituiscono le latenze.

I nuovi nomi report sono proposti: `reports/preview-cache-<os>.json`, `reports/preview-navigation-<os>.json`, `reports/preview-memory-<os>.json` e `reports/preview-quality-<os>.json`. Non creare report di esito positivo prima di eseguire le prove.

## 13. Sequenza di consegna

Le fasi sono incrementi verificabili; i primi miglioramenti RAM/CPU non devono attendere il completamento del renderer GPU. La richiesta complessiva sulle prestazioni comprende comunque la fase GPU, oppure un esito misurato che ne documenti i limiti per uno specifico backend.

| Fase | Lavoro | Uscita richiesta |
|---|---|---|
| A — Baseline e modelli | Baseline 0.1.4, contatori, qualità/revisione, migrazione impostazioni, budget/lease e ammissione prioritaria minima | Misure iniziali e target RAW fissati; test ammissioni; nessuna duplicazione della cache già consegnata |
| B — Residenza e consegna | Separare miniature/preview/sorgente e `Arc<Pyramid>`, preservare frame, consegnare prima di scrivere, code I/O/decode separate | Espulsione senza decode delle miniature; writer con byte limitati; letture cache non accodate ai RAW lunghi |
| C — Scelta qualità e decoder | Selettore Standard/Piena, override 1:1, intent di decode, sviluppo ridotto verificato, provenienza | Due qualità reali; nessun finto Full ottenuto ingrandendo la Standard |
| D — Artefatti persistenti | Estendere cache 0.1.4 con record/descrittori v2, chiavi, quote comuni, letture piccole, compatibilità/recupero | Riavvio/ritorno senza RAW quando sufficiente; niente lettura obbligatoria della piramide completa; test negativi passati |
| E — Scheduler e CPU | Domanda visibile, promozione/deduplica, prefetch con isteresi, pool parallelo, SIMD | Trace navigazione e pressione entro budget; beneficio misurato rispetto alla baseline |
| F — GPU e decoder Apple | Compute WGSL, staging/residenza, contesti Core Image persistenti/Metal, adattamento | Confronti CPU/GPU e prove native/XPC; miglioramento per i percorsi abilitati |
| G — Qualifica integrata | Tutti i profili/qualità, 1.000 RAW, cache fredda/calda, limiti bassi, driver/display | Report riproducibili, limiti dichiarati, documentazione utente e pacchetto verificato |

Checklist della proposta aggiornata durante l’implementazione; le fasi parziali restano aperte:

- [x] A, parte locale: baseline 0.1.4 e budget/priorità minimi verificati.
- [ ] A, qualifica RAW: baseline e target di throughput per corpus reale 12/24/45 MP.
- [x] B: artefatti residenti autonomi e consegna prima della persistenza verificati.
- [x] C: due qualità e migrazione preferenze consegnate con prove; Standard usa il fallback esplicito di sviluppo completo temporaneo.
- [x] D: nuovi artefatti nella cache per cartella, senza duplicare quote o indebolire controlli.
- [x] E, parte locale: scheduler/priorità/cancellazione e CPU parallela/NEON verificati; confronto del solo stadio renderer eseguito.
- [ ] E, qualifica integrata: beneficio evento→frame, pressione fisica e adattatori degli altri OS.
- [x] F, parte locale: compute WGSL, contesto CPU Apple riutilizzato e recupero grafico limitato verificati.
- [ ] F, qualifica: beneficio integrato, reset fisici/OOM, driver/display; Metal nel decoder resta un esperimento disabilitato.
- [ ] G: qualifica integrata e report di tutti i gate §12.

Il nucleo di budget della fase A è prerequisito alle allocazioni delle altre fasi; la fase B può continuare a usare temporaneamente il decoder Full esistente, serializzato entro quota. Le prove XPC accompagnano ogni modifica a broker/decoder/bundle, senza rimandarle tutte alla fase G.

Durante l'implementazione usare `scripts/cargo-local.sh` per Rust e i controlli pertinenti di `scripts/verify.sh`; `--gui` aggiunge la prova nativa. Sul bundle macOS modificato eseguire anche `scripts/test-xpc-integration.py`, verifiche formati/campionamento/pacchetto e smoke di navigazione. Il backend Windows esterno richiede prima isolamento OS reale; non aggirare la allowlist su pipe per ottenere benchmark.

Durante i futuri incrementi applicativi aggiornare soltanto [stato e piano](../STATO.md) per il tracciamento operativo. Le caselle di questo documento esprimono i criteri di accettazione della specifica: modificarle quando viene qualificato il relativo requisito o ne cambia il contratto, non per ogni prova parziale. In quel caso sincronizzare l'appendice E con `python3 scripts/sync-docs.py`. Conservare immutata [l'architettura originale v1.2](TrueVision-Architettura.originale-v1.2.md).

## 14. Decisioni iniziali e questioni da risolvere con misure

**Decisioni di questo progetto:** evolvere la cache per cartella 0.1.4 preservando preferenze e verifica sorgente; due qualità distinte dalla classe di verifica, entrambe con campioni lineari fp32; nessuno swap integrale automatico; quote in byte e ammissione globale; miniature indipendenti e persistenza asincrona; letture cache separate dai decode lunghi; prefetch limitato; CPU parallela e compute GPU verificato; due servizi XPC; nessun ripiego automatico sul JPEG del RAW.

**Da misurare prima di fissare la configurazione di rilascio:** risoluzione Standard ottimale per i display, costo dei decode ridotti Apple, disponibilità GPU nel servizio sandbox, picco delle copie/decoder, numero di thread utile, blocchi SIMD/GPU, soglie di prefetch, compressione Zstd e pareggio CPU/GPU. Questi punti non bloccano la progettazione, ma non possono essere dichiarati risolti da questo documento.

**Fuori dal primo incremento:** demosaicing proprietario/GPU LibRaw, RAW regionale non offerto dal backend, gigapixel qualificato, texture sparse, quota disco globale su tutti i volumi, nuovo protocollo di lock incompatibile con la 0.1.4, più di due servizi XPC e promozione dei badge Standard/Riferimento. Le astrazioni devono permettere le estensioni previste senza far dipendere la navigazione di 1.000 foto dal completamento di R3.

## 15. Tracciabilità dei requisiti del titolare

| Richiesta | Sezioni e verifica |
|---|---|
| RAM per le immagini utili e recupero delle lontane da SSD | §§4–7; espulsione senza risviluppo delle miniature, cache persistente e prefetch |
| Scelta fra anteprima full qualità e standard | §§2–3, 9; selettore persistente, override 1:1, provenienza e gate di qualità |
| Limiti cache scelti dall'utente | §§3, 5–6; quote RAM/SSD/GPU, riduzione dinamica, cache disabilitabile e manutenzione |
| CPU/GPU molto utilizzate e app performante | §§7–9, 12; profilo Prestazioni, parallelismo/SIMD, compute WGSL verificato, riuso del contesto Apple; Metal nel decoder sperimentale e disabilitato, beneficio integrato ancora da misurare |
| Confronto concreto con 1.000 RAW | §§1, 10, 12; baseline, scenari freddi/caldi e misure per la stessa qualità |
| Progetto salvato prima dell'implementazione | Questo documento; checklist autonoma §13 e gate §12 ancora aperti |

Le fonti API collegate descrivono capacità e vincoli delle piattaforme; formule, valori iniziali e sequenza degli incrementi sono scelte progettuali di TrueRenderer. Il riferimento normativo rimane [l'architettura corrente](TrueRenderer-Architettura.md), in particolare §§1.2, 2.3, 5.3–5.4, 7.6, 10–12 e 19, con le decisioni di [ADR 0003](progetto-anteprime-cache-prestazioni.md#adr-0003), [ADR 0004](isolamento-decoder-e-formati.md#adr-0004) e [ADR 0005](progetto-anteprime-cache-prestazioni.md#adr-0005), che aggiorna la scelta della cache accanto agli originali.

<a id="adr-0003"></a>

## ADR 0003 — campionamento alla risoluzione fisica

Data: 6 settembre 2026. Versione app: 0.1.2. Decisione per il prototipo Anteprima; promozione Standard/Riferimento subordinata a §§10 e 19.3.

### Raccordo con la 0.1.5 — 9 settembre 2026

Le descrizioni di sorgente/piramide condivisa, quote fisse e attesa sullo sfondo documentano la 0.1.2. [ADR 0006](progetto-anteprime-cache-prestazioni.md#adr-0006) mantiene il grafo del filtro ma usa livelli autonomi, budget con lease, compute GPU e riuso/riproiezione del frame durante il raffinamento. Istogramma e campionatore indicano ora il livello residente; 1:1 richiede il dettaglio nativo Full. La suite corrente confronta anche CPU/GPU e le fixture EXIF, entro il proprio perimetro; qualifica completa di transizioni, display e prestazioni ancora aperta. Risultati aggiornati in [VERIFICA](../STATO.md#registro-delle-verifiche-e-degli-incrementi).

### Problema osservato

Il corpus `04_Frequenze_radiali.png` mostrava falsi dettagli e moiré in Adatta e nella griglia della 0.1.1. Il viewer applicava nearest alla texture sorgente a scala arbitraria; le miniature passavano da una riduzione area a 320 pixel e poi da un ulteriore ridimensionamento nearest, anche su Retina. La miniatura laterale poteva inoltre cambiare sorgente quando arrivava la decodifica intera. Screenshot della vecchia finestra acquisito prima della modifica; nessun cambiamento al generatore o ai PNG del corpus per nascondere il problema.

### Decisione

- Tutte le viste condividono la stessa sorgente completa e la stessa piramide CPU lineare Rec.2020 fp32 premoltiplicata. Le priorità di decodifica restano separate dal parametro di riduzione; la UI richiede LOD 0 anche per le miniature del corpus.
- Le dimensioni e le due coordinate del rettangolo vengono arrotondate alla griglia dei pixel fisici. Il motore produce esattamente quel raster: la texture finale è presentata texel-per-pixel, senza un secondo ridimensionamento. Nearest in questo ultimo trasferimento non seleziona o scarta campioni a una scala diversa.
- A 1:1 allineato, anche con crop/pan intero, si copiano i campioni LOD 0 senza filtro. In ingrandimento opaco si usa Mitchell-Netravali B=C=1/3. Non si attiva nearest automaticamente oltre 200%.
- Per riduzioni opache si usa Lanczos3 con supporto adattato e piramide a passi non superiori a 2×. I livelli hanno dimensioni `ceil(n/2)`; origine al bordo pixel e centri a `n+0.5`, estensione costante ai bordi con pesi rinormalizzati. Il livello è il primo con rapporto residuo ≤2 su entrambi gli assi; i rapporti usano le dimensioni effettive, comprese quelle dispari. Il caso esatto 2× è equivalente al livello precomputato successivo. Warp anisotropi generici non sono qualificati.
- **Variante esplicita rispetto al Lanczos3 iniziale della proposta:** il supporto usa `scale_eff = s * (1 + 0.1 * clamp(s-1, 0, 1))`, con kernel `sinc(x/scale_eff) * sinc(x/(3*scale_eff))`. Il margine di banda cresce continuamente da zero a scala unitaria al 10% nella decimazione 2×; non modifica le dimensioni dell'immagine. È parte della versione `cpu-pyramid-lanczos3-guard10-mitchell-alpha-area-triangle-v1`, non un cambiamento silenzioso del filtro v1 qualificato.
- Motivazione quantitativa: il Lanczos3 senza margine lasciava RMS 0,025793 nel caso sinusoidale 0,317 cicli/pixel, 768→320, oltre la soglia prefissata 0,02. La versione scelta passa i casi senza abbassare la soglia. Il filtro composto può altrimenti piegare alias già nel primo livello della piramide. Il margine attenua anche parte della banda di transizione: il compromesso è dichiarato e viene misurata separatamente la conservazione del contrasto nella banda passante.
- Per immagini con trasparenza: area in riduzione e triangolo in ingrandimento, stessi pesi non negativi per RGB premoltiplicato e alpha. Non si applicano lobi negativi ad alpha e non si nasconde colore trasparente con un clamp incoerente. Le unità di memoria non vengono riscritte in gamma prima del filtro.
- Un thread di presentazione prepara i raster fuori dalla UI. Coda e richieste coalescenti limitate a 64; risultati obsoleti ignorati, identificatori sorgente monotoni per evitare riuso degli indirizzi; cache texture 64 voci/128 MiB e cache sorgenti/piramidi 64 voci/384 MiB. Durante un ricalcolo si attende il raster corretto, mostrando temporaneamente lo sfondo. Non è ancora il raffinamento progressivo v1 né una quota di memoria globale.
- In zoom elevato si calcola solo la regione visibile, evitando raster enormi fuori viewport. Restano la quota R0 di 8.388.608 pixel e un massimo di 16.384 per asse di presentazione. Le coordinate di filtro sono f64; lo stato pan/zoom egui è ancora f32 e non è qualificato per gigapixel.
- Il campionatore è etichettato **Sorgente LOD 0**: i suoi valori non pretendono di essere quelli del pixel filtrato del viewport. L'istogramma è della sorgente composta in sRGB, non del riquadro ridimensionato.

### Prove e limiti

Quattro test Rust aggiunti: lettura esatta e crop con RGB negativo/oltre 1, costanti e dimensioni 1×N/N×1/dispari, alternanza bianco-nero che produce circa 188 sRGB in riduzione, alpha nascosto e rifiuto quote/NaN. Le prove CLI confrontano 24 sinusoidi con riferimenti analitici: RMS ≤0,02 oltre 1,5× Nyquist di uscita; rapporto di contrasto 0,95–1,05 sotto 0,25× Nyquist. La banda intermedia è osservata senza dichiararla superata come gate. Il corpus radiale è verificato bit-per-bit a 1:1 e salvato a cinque scale con un modello del vecchio Adatta nearest per confronto.

Lo smoke nativo esteso produce otto schermate. Il confronto dei pixel delle schermate con il raster CPU controlla griglia a tre dimensioni, preview laterale, filmstrip e viewer 1:1/Adatta/37%, con soglia di un livello per canale a 8 bit. Il metadato registra rettangolo fisico, origine, passo e dimensione di ogni regione radiale interamente visibile; non si confrontano aree intenzionalmente tagliate dal clip. `reports/sampling-presentation-macos.json` contiene i risultati effettivi, non dedotti dal solo valore Retina 2×.

Questi controlli non esauriscono §19.3. Restano alias 2D/Siemens/isotropia, overshoot ai bordi, confronto piramide/direct e CPU/GPU, transizioni temporali, orientamento EXIF, tile/cuciture, altri display/DPI e costo p95. La variante Lanczos deve essere rivista o sostituita con un'alternativa (ad esempio Kaiser con parametri fissati) se la suite estesa non rispetta le soglie. Non si clampa RGB nel working per nascondere overshoot; il solo clipping è in uscita sRGB8, come nel prototipo precedente. Nessun badge Standard/Riferimento viene abilitato.

Una riduzione non può conservare tutte le frequenze della sorgente: il filtro deve attenuare quelle non rappresentabili dai pixel disponibili. I cerchi centrali della zone plate fanno parte dell'immagine originale; l'obiettivo è evitare nuovi motivi spuri, non eliminare i cerchi o produrre ovunque grigio uniforme. Gli screenshot ingranditi/ridotti da un altro programma possono introdurre propri artefatti. Il confronto numerico riguarda la superficie dell'app: compositore, profilo monitor e pannello fisico restano prove separate.

### Riproduzione

```sh
./scripts/verify.sh
./scripts/build-macos.sh
./dist/TrueRenderer.app/Contents/MacOS/TrueRenderer --verify-resampling
open -n -W dist/TrueRenderer.app --args --sampling-smoke
./dist/TrueRenderer.app/Contents/MacOS/TrueRenderer --verify-sampling-screenshots
```

Le prove del bundle richiedono la sessione nativa macOS; la libreria dello smoke è separata in `var/smoke`. Risultati in `reports/resampling-macos.json`, `reports/sampling-presentation-macos.json` e `reports/smoke-macos.json`.

Fonti primarie per la teoria del campionamento: [PBRT, Sampling Theory](https://www.pbr-book.org/4ed/Sampling_and_Reconstruction/Sampling_Theory) e [Image Reconstruction](https://www.pbr-book.org/4ed/Sampling_and_Reconstruction/Image_Reconstruction). I coefficienti, la scelta del margine e le soglie sopra sono decisioni del progetto, non garanzie attribuite al libro.

<a id="adr-0005"></a>

## ADR 0005 — cache lossless e temporanei accanto alle immagini

Data: 7 settembre 2026. Documento storico del formato v1; aggiornamento di raccordo 8 settembre 2026. Stato: 0.1.4 verificata; primo incremento pubblicato (`2dad0b2`), ottimizzazioni successive qualificate e pubblicate (`a901942`).

### Raccordo con l'implementazione corrente

Questo ADR conserva decisioni e misure della 0.1.4. Nella 0.1.5 il percorso interattivo usa artefatti autonomi v2, lettura separata e writer asincrono: [ADR 0006](progetto-anteprime-cache-prestazioni.md#adr-0006) estende formato, residenza, budget, concorrenza e compute. V1/v2 condividono directory riconosciuta, lock e quota per cartella. Il vecchio percorso full-frame è ancora usato dal relativo harness di regressione. Le frasi seguenti sul decode con scrittura sincrona si riferiscono alla baseline 0.1.4, non alla consegna interattiva attuale.

Il formato v1 `.tvc`, il formato di record v2 e il futuro container tiled/gigapixel sono tre contratti distinti. I checksum rilevano corruzioni, senza autenticare dati riscritti da un attaccante. Token di filesystem e qualità Piena non promuovono una cache ad assurance Standard/Riferimento.

#### Raccordo hard link — 15 settembre 2026

Il lettore corrente consente file regolari con più hard link per tollerare la condivisione temporanea dei client di sincronizzazione; i record restano soggetti a tutte le verifiche di contenuto. Questo non autorizza scritture, aggiornamenti di timestamp o rimozioni. Corretto il percorso che usava la sola leggibilità per sostituire una voce invalida: la rimozione ricontrolla ora tipo e numero di link sul file aperto, sia per v1 sia per record v2 e temporanei. Una destinazione condivisa invalida conserva entrambi i nomi e fa saltare la scrittura; una valida può ancora essere riusata in sola lettura.

La verifica Mac copre byte/mtime, nomi conservati, descrittore e blocchi v2, link aggiunti dopo la scansione e ritorno alla raccolta ordinaria dopo la loro rimozione esterna. Non è una garanzia atomica contro ogni sostituzione o creazione di link da processi non cooperanti tra controllo e unlink. File protetti possono restare sul disco oltre la quota eliminabile; non si attribuisce la qualifica Windows ai soli test Mac. Le frasi storiche seguenti sul rifiuto degli hard link vanno lette insieme a questo contratto corrente.

### Scopo autorizzato

Il titolare richiede una cartella di cache/temporanei dentro ogni cartella aperta, impostazioni di limite e pulizia, README inglese e pubblicazione su GitHub prima di ulteriori ottimizzazioni. Questa richiesta anticipa esplicitamente la cache accanto alle foto che §11.4 collocava nel post-v1. Le fotografie rimangono in sola lettura; si scrive esclusivamente nella sottocartella derivata `.truerenderer-cache`. Annotazioni, backup e preferenze rimangono nel percorso dati dell'app.

### Formato e identità

`entries/<sha256>.tvc` conserva un header JSON bounded, tutti i livelli già calcolati della piramide RGBA fp32 little-endian, l'istogramma e un checksum SHA-256 dell'header e dei campioni. È lossless rispetto ai bit prodotti dal decoder/piramide corrente. Non introduce compressione JPEG, quantizzazione fp16 né un nuovo filtro. Il working space è Rec.2020 lineare con alpha premoltiplicata, come il percorso in RAM. Il formato è un prototipo full-frame, non il container tiled/gigapixel v1.

La chiave viene derivata da una tupla JSON versionata con digest completo della sorgente, versione app, versione del filtro, build OS, working space e ricetta RAW. Profilo incorporato e orientamento sono coperti dai byte sorgente; la ricetta corrente è fissa. L'apertura verifica nuovamente SHA-256 prima di riusare il disco. Non usa la sola stat come identità del contenuto. Il token stat dell'indice resta una osservazione best-effort sotto writer concorrente: non viene dichiarata una revisione sorgente coerente v1.

I file cache non sono considerati fidati: magic, header massimo 64 KiB, numero/dimensioni dei livelli, lunghezza esatta, campioni finiti, alpha, istogramma e checksum vengono verificati prima dell'uso. Un errore è un miss e attiva il decoder normale. Il worker su pipe continua ad ammettere solo il corpus; una cache non abilita gli esterni fuori dal bundle XPC. Un hit disco ha provenienza distinta `Cache disco · fp32 · Anteprima`; non abilita Standard/Riferimento. Il checksum rileva corruzioni, non autentica dati contro un attaccante che possa riscrivere interamente la cache.

### File, quote e pulizia

- `.truerenderer-cache/OWNER` identifica la directory eliminabile del progetto. Una directory omonima non riconosciuta viene lasciata intatta e la cache non è abilitata lì.
- `entries/` contiene solo artefatti conclusi; `tmp/` contiene scritture parziali con nome derivato dalla chiave. Sono esclusi da Git anche dentro il corpus.
- Directory e file vengono aperti senza seguire link. Su macOS si usano descrittori di directory e operazioni relative (`openat`, `unlinkat`, `renameatx_np` con `RENAME_EXCL`); la pulizia non percorre ricorsivamente percorsi arbitrari. File speciali e hard link vengono rifiutati.
- I lettori prendono un lock condiviso; scritture e pulizia prendono un lock esclusivo non bloccante. La cache occupata può essere saltata mantenendo l'immagine disponibile. Un reader attivo non può essere rimosso dalla manutenzione dell'app. Un filesystem che non offre le primitive richieste conserva il fallback in RAM.
- Prima di una scrittura si controlla la sua dimensione esatta, si puliscono temporanei abbandonati, si eliminano scaduti e meno usati, e si riserva nella quota il posto per il temporaneo. La pubblicazione avviene soltanto dopo scrittura completa/checksum e sincronizzazione del file; il rename esclude sovrascritture inattese. Un'interruzione lascia un temporaneo eliminabile, mai un file parziale ammesso come hit.
- Default: 4.096 MiB per cartella, 2.048 MiB massimi per artefatto temporaneo, 30 giorni senza utilizzo, 512 MiB liberi sul volume. Temporanei e cache conclusa condividono la quota per cartella. Il controllo dello spazio libero non prenota blocchi contro applicazioni esterne.
- I limiti sono per cartella, non una quota aggregata su tutte le cartelle già visitate. Le nuove impostazioni sono applicate alla cartella corrente e alle altre alla riapertura; non esiste uno scanner globale dei dischi.
- Disabilitare la cache evita nuovi hit/scritture e non elimina i dati esistenti; il comando di svuotamento elimina solo i derivati riconosciuti della cartella corrente. La RAM già pronta rimane disponibile. Preferenze salvate atomicamente in `var/settings.json`.

Errori di quota, lettura/scrittura, readonly, lock o filesystem non bloccano il salvataggio delle annotazioni: la manutenzione manuale gira fuori dal writer SQLite e dalla UI. La manutenzione automatica all'apertura gira in background. Il backend persistente è verificato su macOS; Windows è ancora da implementare/qualificare.

### Prestazioni e limiti

L'hit evita decoder e ricostruzione della piramide/istogramma, ma deve leggere e validare i campioni. Il fp32 non compresso può essere molto più grande della fotografia compressa. Il prototipo mantiene i limiti sorgente (64 Mi pixel), RAM/cache di ADR 0004 e renderer fisico di ADR 0003; non chiude il budget globale o «mai viewport vuoto».

Il primo incremento è stato pubblicato con commit `2dad0b2` prima di queste ottimizzazioni. Il campionamento CPU di una finestra del benchmark (3 s richiesti, intervallo 1 ms) mostra SHA-256 in cima allo stack per 1.737 dei 2.153 campioni del thread di verifica. Non è una percentuale dell'uso complessivo dell'app.

Abilitata la feature `asm` di `sha2 0.10.9`: su aarch64 seleziona le istruzioni SHA-256 dopo controllo delle capacità CPU e mantiene il fallback software. La nuova dipendenza opzionale `sha2-asm 0.6.4` è bloccata in Cargo.lock e la sua licenza MIT è conservata nell'inventario. Il vantaggio riguarda gli hash della cache e delle sorgenti, senza cambiare algoritmo, chiavi, checksum, fp32 o filtri.

Il broker ora prepara un `SourceSnapshot` con byte/digest privati e non modificabili dai chiamanti: lookup e decodifica usano la stessa copia, eliminando la seconda lettura/hash sul miss. Le verifiche di digest/token e ammissibilità sono centralizzate nel broker; il percorso decoder ricontrolla la policy del proprio trasporto anche per uno snapshot proveniente da un altro broker. Questo riuso non risolve la revisione coerente sotto writer concorrente prevista per la v1.

Sulle stesse fixture e sullo stesso Mac, la mediana di cinque riusi scende da 0,9631 a 0,1374 s per JPEG 12 MP (7,0×), da 0,06928 a 0,00972 s per DNG (7,1×), da 0,000304 a 0,000119 s per PNG piccolo (2,6×). I caricamenti a cache applicativa fredda, scrittura inclusa, sono rispettivamente 0,6223 / 0,1727 / 0,01024 s. Il cache OS non è svuotato; non si attribuisce una quota separata di guadagno alle due modifiche. Dati prima/dopo e limiti in `reports/cache-performance-comparison.json`.

La scrittura resta nel thread di decodifica prima della consegna dell'immagine. Scrittura differita con coda bounded, cache compressa/tiled, scheduler globale e memorie sotto pressione restano ulteriori passi.

### Verifiche

Cinque test Rust mirati: round-trip bit esatti inclusi valori RGB negativi/>1 e alpha, invalidazione per contenuto/pipeline, corruzione/troncamento, cancellazione senza pubblicazione parziale, LRU/scadenza e temporanei abbandonati, quote/spazio, impostazioni persistenti, directory non riconosciute, symlink/hardlink e lock dei reader. Tre test aggiuntivi coprono vettori SHA-256 noti anche a confini di blocco, rifiuto di uno snapshot esterno su pipe e decodifica dei byte catturati dopo una modifica del file originale. Totale workspace: 40 test passati. Restano fuzzing, power-fault reale, filesystem remoti e altri OS.

`--verify-cache` usa solo tre immagini generate del progetto: JPEG 12 MP, DNG Bayer e PNG 16 bit. Una corsa con cache applicativa fredda e cinque calde; confronta ogni bit di ogni livello e l'istogramma, verifica l'assenza di nuovi job decoder nei riusi e gli originali invariati. Il cache OS non viene svuotato: non è un benchmark p95. `--settings-smoke` acquisisce il pannello delle impostazioni. Esiti effettivi in `reports/cache-macos.json`, `reports/cache-settings-macos.json` e `STATO.md` (registro e collegamenti ai rapporti JSON).

### Raccordo Windows — 14 settembre 2026

Accorpato dal primo piano Windows: la directory cache è mantenuta aperta senza condivisione di cancellazione; l'ultimo componente viene controllato per reparse point, i nomi escludono anche `:`, gli hard link sono rifiutati e la pubblicazione non sovrascrive destinazioni inattese. Gli antenati sopra la radice della cache non sono fissati come con le operazioni Unix relative al descrittore: il limite resta esplicito.

I payload consentono condivisione di cancellazione per mantenere utilizzabile un handle già aperto durante la raccolta. La contesa Windows `ERROR_LOCK_VIOLATION` viene normalizzata come condizione ritentabile. Identità del file e ChangeTime contribuiscono all'osservazione delle sorgenti; gli hit cache aggiornano il timestamp tramite handle con soli diritti attributi, proteggendo gli hard link.

Riproduzioni e correzioni sono nella [revisione cache/TIFF/RAW](../STATO.md#revisioni-windows-concluse). Una persistenza saltata sotto contesa resta documentata nella [revisione serale](../STATO.md#revisioni-windows-concluse): non è qualificato il riuso completo per ogni sequenza.

<a id="adr-0006"></a>

## ADR 0006 — anteprime autonome, ammissione e compute

Data: 8 settembre 2026. Stato: implementazione 0.1.5; qualifica integrata del progetto ancora aperta.

### Decisione

Estendere ADR 0003–0005 con livelli lineari autonomi. La qualità dell'anteprima (`Standard`/`Full`) resta separata dall'assurance `Preview`; nessun badge della futura pipeline Standard/Riferimento viene abilitato. Nuove configurazioni partono da Standard; la migrazione riconosce impostazioni o libreria preesistenti prima di crearle e conserva Full e i limiti disco precedenti. Un file impostazioni danneggiato viene conservato e il recupero è segnalato.

Standard conserva un derivato fino a 2048 pixel di lato. Full conserva il sottoinsieme del grafo sufficiente alla vista fisica; 1:1 richiede LOD 0 e un override per foto. Il provider dichiara sviluppo full-frame e assenza di decode regionale/ridotto RAW qualificato. Lo sviluppo completo temporaneo seguito dal grafo di riduzione è il fallback previsto dal progetto; nessun JPEG incorporato viene sostituito allo sviluppo.

`ImageLevels` possiede una coda di livelli senza riferimento alla sorgente più grande. Richieste compatibili contemporanee della stessa revisione condividono lo sviluppo; i derivati piccoli vengono staccati e le allocazioni uguali condivise. Coefficienti, coordinate f64, ordine degli accumuli e policy alpha restano quelli di ADR 0003. Rayon 1.11.0 fornisce un pool applicativo comune; il percorso arm64 usa NEON f64 senza FMA. Il limite sui thread applicativi non limita internamente i codec di sistema.

### Memoria e code

Un budget condiviso ammette snapshot, letture, raster, scratch e presentazione prima delle allocazioni pesanti. Include un'allowance iniziale di 384 MiB per app, contesti persistenti e infrastruttura device, oltre agli incrementi dei job. È una stima da qualificare e non un tetto imposto dal kernel. Gli snapshot hanno anche un sottolimite di un terzo del budget; un writer opzionale trattiene al massimo 64 MiB di livelli e prenota scratch separatamente. Gli Arc dei campioni possiedono il lease; spostare il risultato fra code non duplica i crediti globali.

La UI pubblica la domanda effettivamente disegnata; le classi P0–P6 mantengono ordine FIFO e promozioni anche durante un lookup. Una coda piena può espellere lavori secondari per quelli visibili. Hash/cache e due decoder sono code separate. Un solo decoder può occuparsi di lavoro speculativo lungo; il secondo dà precedenza al visibile. Cambiare cartella revoca il dominio, cambiare vista elimina consumatori obsoleti senza riciclare XPC a ogni scroll. Il backend nativo già in corso può dover terminare. Il prefetch segue la direzione di navigazione e considera solo i vicini della vista, anche dopo un salto lontano. Attende una stabilità fra 100 e 500 ms stimata dalle recenti latenze di consegna degli artefatti (400 ms iniziali); questa euristica non è una misura evento→frame. Pausa e ricostruzione esplicita non bloccano le annotazioni.

Su macOS una dispatch source osserva MEMORYPRESSURE. In Warn/Critical si sospendono le nuove ammissioni speculative e le scritture opzionali, si riducono a due i thread applicativi effettivi e si espellono RAM/GPU riutilizzabili. Preferenze e crediti dei buffer in uso rimangono validi. Le prove coprono la policy con pressione iniettata e la disponibilità dell'adattatore nativo; non equivalgono a una prova di pressione fisica dell'intera macchina. Sugli altri OS l'adattatore non è ancora implementato.

La riduzione dei limiti si applica alle nuove ammissioni, mentre i buffer già utilizzati dalla GPU mantengono i crediti fino al completamento. La UI distingue una riduzione in corso e segnala un'operazione che non entra in memoria. Il sistema non promette un picco RSS istantaneo uguale al valore del controllo.

### Persistenza

Stessa directory `.truerenderer-cache`, marker, lock, nomi gestiti e quota di ADR 0005. I record v2 hanno SHA-256, lunghezze esatte e payload fp32 lossless massimo di 4 MiB. Il descrittore viene pubblicato dopo i blocchi. Scritture e temporanei restano protetti dal lock esclusivo; la preparazione dei record è esterna al lock, acquisito per gruppi limitati a circa 4 MiB. Quote e presenza dei blocchi precedenti sono ricontrollate a ogni acquisizione; non si riusa una scansione dopo aver rilasciato il lock. Writer e GC cedono precedenza ai lettori locali in attesa con attesa limitata e cancellabile. Una seconda istanza può espellere un blocco prima della pubblicazione: il writer abbandona l'operazione opzionale, senza dichiararla completa.

Il lettore verifica prima lo snapshot sorgente completo e poi i byte copiati dei record. `Busy`, `Missing`, `Invalid`, `Disabled` e limite memoria restano distinti; la contesa ha retry limitato a 200 ms. La scansione delle quote legge i metadati rispetto al descrittore della directory senza seguire link, evitando aperture dei payload; la pubblicazione aggiorna la scansione già protetta dallo stesso lock, senza ripeterla per le sole statistiche. V1 viene riusata solo con fingerprint esatto e prenotazione per la lettura completa. Non è prevista una conversione massiva delle cache di altre versioni. Il GC comprende v1/v2, orfani e temporanei e protegge il minimo recuperabile per miniature; il vecchio GC mantiene comunque sicurezza e quota, non la nuova preferenza di residenza.

### Compute e decoder

Il viewer usa coefficienti canonici CPU, due pass compute separabili e conversione/composizione SDR in WGSL. Pipeline e input compatibili sono riutilizzati; la texture finale viene registrata direttamente nel renderer egui, senza readback per frame. Input e scratch fuori capability o quota producono fallback CPU registrato. Automatico usa la GPU verificata per richieste sufficientemente grandi; il confronto numerico del renderer avviene prima dell'abilitazione. I report di performance distinguono questa elaborazione dal tempo evento-utente→presentazione.

Core Image riutilizza il contesto richiesto software con intermedi disabilitati. È disponibile un esperimento separato con contesto richiesto Metal, ma non è abilitato per il decode produttivo: i casi sintetici conformi non provano backend effettivo, beneficio end-to-end e matrice reale delle fotocamere. La presentazione può usare compute GPU mentre il decode resta CPU. Il riciclo e l'isolamento XPC continuano a seguire ADR 0002/0004.

La preparazione e l'encoding compute avvengono sul worker, ma i submit alla coda condivisa avvengono esclusivamente sul thread UI. Un submit concorrente a `Surface::configure` causava un errore di validazione wgpu riprodotto durante gli avvii nativi. La correzione serializza anche il controllo iniziale GPU prima del loop della finestra. La cancellazione prima del submit libera subito il lavoro mai inviato; dopo il submit i lease rimangono fino alla callback di completamento. Lo shutdown attende la fine dell'encoder e il completamento GPU con timeout. La suite lifecycle ripete avvii e transizioni senza ritentare i fallimenti.

### Revisione applicativa dell'8–9 settembre 2026

**Invalidazione delle sorgenti residenti.** Un monitor separato dalla UI e dal writer SQLite osserva ogni 500 ms soltanto file richiesti e residenti. Su Unix il token comprende device/inode, size, mtime e ctime con nanosecondi; cancellazioni e file non disponibili hanno uno stato distinto. Una modifica invalida gli artefatti della foto e le relative presentazioni, incrementa l'epoca dei decode e scarta i risultati precedenti; il ripristino riabilita il caricamento. Annotazioni, selezione e override non vengono azzerati. Si rimuove la vecchia immagine con indicazione esplicita; non si mostra un frame della revisione precedente come corrente. La nuova lettura esegue sempre snapshot/SHA nel broker. Il polling rimane best-effort, non un watcher completo del catalogo né una revisione coerente sotto writer arbitrari; una syscall su volume remoto può ritardare l'osservazione.

**Recupero grafico.** Un device perso interrompe il disegno e invalida il presenter; si tenta al massimo una nuova finestra/device e si ripete il confronto numerico prima del compute. Il servizio catalogo, i salvataggi accettati, l'undo, la selezione e gli artefatti CPU rimangono vivi. Un destinatario di repaint condiviso viene collegato al nuovo contesto egui, senza polling periodico della UI. La seconda perdita chiude con diagnostica nativa e completa i salvataggi accettati. Se egui/winit ha già propagato un panic dal disegno, il delegate macOS non è riutilizzabile: quel caso chiude conservando i dati durevoli anziché tentare una ricreazione su uno stato incoerente. Surface loss resta gestita da eframe; reset fisici, OOM e altri driver/display richiedono qualifica separata.

**RAW contenuti in TIFF.** La verifica con NEF Nikon D750 ha rilevato che ImageIO riconosceva `public.tiff` e restituiva IFD0 da 160×120, sebbene il file contenesse il RAW nativo. Anche `CIRAWFilter` non nullo accetta bitmap: non basta per dichiarare sviluppo RAW. Probe e decode ora condividono la ricerca fra gli identificatori RAW registrati da ImageIO per i contenitori TIFF e accettano il percorso RAW soltanto con `decoderVersion` disponibile. La profondità sensore viene dichiarata sconosciuta quando l’unico valore disponibile descrive la miniatura. Dimensioni native, quote e ricetta vengono poi verificate come prima; i bitmap ordinari conservano il loro percorso. Il fingerprint cache include `raw-detection-v2`, così i derivati della vecchia classificazione non vengono riutilizzati. Non vengono passati percorsi al worker né ampliati i suoi permessi. Gli altri modelli RAW restano subordinati alla matrice qualificata.

**Budget sui RAW reali, prima della continuazione.** Lo sviluppo nativo full-frame prenotava `64 × pixel + 128 MiB`, oltre a `2 × byte sorgente + 1 MiB` per snapshot e allowance base 384 MiB. Alcuni NEF da circa 24 MP non entravano quindi nel default di 2 GiB anche senza perdite di crediti; il rifiuto era esplicito e precedente al decode. La prima prova completa usava un budget manuale di test di 3 GiB, senza cambiare le preferenze dell’utente. I report storici `preview-*-before-phases-*.json` conservano questo esito; la correzione e la misura corrente sono descritte sotto.

### Continuazione della revisione: durata dei buffer e report

Il broker rilascia il proprio riferimento ai byte compressi appena terminato l'invio; il probe mantiene solo lo snapshot condiviso necessario al successivo decode. Nel worker `native::decode_backend` chiude già l'handle nativo prima di restituire il raster; `serve_with_policy` ora libera anche i byte compressi prima di trasferire il raster. Non cambia la ricetta, il formato fp32, il protocollo o la copia privata del broker.

La nuova ammissione considera il massimo fra fasi successive. La fase nativa conserva una stima di scratch di `32 × pixel + 128 MiB`, oltre al raster nativo `16 × pixel`. La fase host conta una piramide completa, l'intermedio orizzontale del filtro, il livello di uscita e tutte le code mip distinte che i consumatori possono possedere; per prudenza conta anche il raster del worker fino alla fine, poiché l'ultima lettura non sincronizza il suo distruttore. Ogni livello usa dimensioni arrotondate per eccesso: non si applica la scorciatoia 4/3 alle immagini con una sola riga/colonna. Si conserva il margine di 128 MiB anche nella fase host. Snapshot e baseline di 384 MiB restano separati. Per 6016×4016 domina `48 × pixel + 128 MiB`; le geometrie sottili possono richiedere di più. Sono stime di ammissione da confrontare con i processi reali, non un nuovo tetto kernel.

I decode che richiedono più della metà del budget di lavoro disponibile dopo la baseline vengono serializzati prima del probe; il secondo attende con cancellazione e limite di attesa, senza consumare l'allowance di parsing né ricevere il precedente errore di contesa dopo due secondi. I job piccoli rilasciano il permesso dopo il probe e possono procedere in parallelo. La coda hash/cache rimane indipendente. Al massimo due snapshot attendono nei pronti al decode: eventuali eccedenze di priorità inferiore vengono differite alla UI e liberate, mantenendo separato il limite di 64 descrittori. Ciò impedisce a un accumulo di sorgenti future di occupare tutto lo spazio necessario al prossimo decode. L'ordine dei lavori resta quello delle priorità esistenti; un job nativo già attivo non viene preemptato solo per uno scroll.

I report RAW, memoria e regressioni native ora partono da `passed: false`; una prova interrotta resta incompleta e un errore non lascia un vecchio successo come risultato corrente. I fallimenti del carico invalidano anche la prova memoria, pur quando il picco misurato è basso. Il report memoria include exit code, timeout, campioni mancanti, dettaglio dei processi al picco e hash dei quattro binari provati. Ogni misura avvia una copia univoca del bundle: tutte le sue istanze XPC, incluse quelle in riciclo, sono conteggiate, mentre altre app già aperte da un diverso percorso non appartengono al carico. Questo corregge la contaminazione da un’altra istanza rilevata nella prova estesa; non elimina processi del carico per rientrare nella soglia. I percorsi fotografici e i dettagli delle eccezioni rimangono nei log locali.

La prima prova isolata dopo la correzione per fasi passava a 2 GiB con 30 NEF Nikon D750: 60 coppie di miniature fredde/calde Standard/Piena bit-exact, 30 riaperture senza decode, tre esempi di dettaglio nativo e due richieste fredde simultanee. Footprint aggregato massimo 2.100.284.608 byte, RSS 2.066.481.152 byte, massimo tre processi; zero campioni incompleti, originali invariati e crediti di lavoro azzerati allo shutdown. Il massimo intervallo di campionamento è 355 ms: la misura non esclude picchi più brevi. Un limite di test di 512 MiB produce invece un rifiuto sicuro e report negativi. Riferimenti e identità dei binari in `STATO.md` (registro e collegamenti ai rapporti JSON).

### Continuazione navigazione del 9 settembre

La revoca del dominio rimane distinta dall’abbandono di una vista. Snapshot/hash/cache e attesa dei crediti controllano anche la domanda corrente; un lavoro annullato non acquisisce nuova memoria neppure quando sarebbe disponibile. Prima e dopo le chiamate native si rivaluta se esistono ancora consumatori della sorgente; la cancellazione dovuta alla vista non termina il processo durante la chiamata. Rimangono attivi shutdown/revoca del dominio e gli arresti per timeout, errore di protocollo o quota memoria del broker. Il cambio qualità/edge della stessa sorgente mantiene il lavoro full-frame condivisibile. Questo criterio andrà ristretto alle capability effettive prima di introdurre un decoder Standard ridotto.

La cancellazione osservata resta valida fino alla chiusura del tentativo: un ritorno rapido alla foto riceve una notifica di rinvio e può riprovare, senza un errore permanente di memoria. Prima della consegna/persistenza si verifica che i consumatori siano ancora richiesti; anche la richiesta originaria rimossa viene liberata dal registro pending. Le regressioni coprono attesa memoria, probe nativo in corso, cambio foto e cambio qualità, processo ancora vivo e crediti restituiti. Non misurano p95/p99 né la prelazione di chiamate native.

La suite della continuazione navigazione registra 71 test Rust e le verifiche native del bundle passate. L’ultima misura isolata sui 30 NEF a 2 GiB ha footprint massimo 2.018.970.672 byte e RSS 1.965.047.808 byte, 7.042 campioni completi e intervallo massimo 54,63 ms; originali invariati, zero decode alla riapertura e zero crediti residui. Questa prova è successiva a quella da 2.100.284.608 byte descritta sopra; non è un A/B. Codice e report sono inclusi in `10123b5`. [Riepilogo e identità dei binari](../reports/preview-navigation-continuation-macos.json).

### Copertura provvisoria del viewer — 15 settembre 2026

Il presenter può conservare un frame più ampio per vista, foto, digest, motore e dimensioni sorgente, scegliendo durante il raffinamento quello con maggiore sovrapposizione geometrica. Il frame facoltativo mantiene i propri lease, senza nuove prenotazioni: massimo due complessivi, entro un quarto dei budget globale e GPU e nel limite di 64 frame. Non viene promosso a risultato esatto; il raster richiesto deve comunque essere calcolato.

Invalidazione, cambio compute, pressione, riduzione quota e necessità di ammissione renderer/decoder possono eliminare questa riserva. I crediti ritirati restano contabilizzati fino al completamento previsto dalla coda. Se manca una rappresentazione ampia compatibile o la quota la espelle, la copertura può restare parziale; non si inventano pixel e non si garantisce continuità universale o un tetto RSS.

### Prova grandi immagini e riserva sotto pressione — 15 settembre 2026

Le transizioni su PNG 12/24/45 MP verificano livelli residenti Standard ridotti, recupero del livello zero e 1:1 esatto. Il decode iniziale resta completo. I livelli oltre capability GPU ricadono sulla CPU dichiarandolo; il writer può non conservare i Full troppo grandi. Con pressione renderer iniettata la riserva viene espulsa e la copertura può diventare parziale, ma la traccia termina con raster corretti.

La quota di ammissione non è ancora un limite fisico qualificato: su 45 MP Full, con 4 GiB configurati, la somma RSS arriva a 4.816.601.088 byte e il footprint a 4.296.512.936 byte. I crediti rimangono entro quota; la somma RSS può contare pagine condivise più volte. Il superamento resta aperto, senza sottrarre processi o attribuire i picchi ai soli driver. A 1536 MiB le due tracce di pressione restano entro la quota campionata; a 512 MiB il decode RAW viene rifiutato esplicitamente e restituisce i crediti. [Protocollo](../STATO.md#campagne-raw-concluse).

### Qualifica aperta

I report sotto `reports/preview-*` descrivono prove realmente eseguite, con soglie e scope. Restano il corpus autorizzato di 1000 RAW reali e sottoinsiemi 12/24/45 MP, la latenza evento→frame con p95/p99 e confronti indipendenti, la contabilità dei driver oltre l'attribuzione ai processi, pressione fisica/reset dei driver e la matrice di display/driver e altri target. Il corpus di 1000 Bayer distinti generati serve al catalogo e alle code e non sostituisce questi casi. R0–R4 restano aperti.
