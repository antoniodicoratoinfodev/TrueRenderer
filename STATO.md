# TrueRenderer — stato e piano di sviluppo

Aggiornato: 20 settembre 2026.

Fonte unica per stato corrente, prossime attività, caselle operative e registro degli incrementi. Sostituisce i precedenti piano, avanzamento e documento di ripresa.

- [Punto di ripresa](#punto-di-ripresa)
- [Piano operativo](#piano-operativo)
- [Matrice dei requisiti e budget](#matrice-dei-requisiti-e-budget)
- [Registro delle verifiche e degli incrementi](#registro-delle-verifiche-e-degli-incrementi)

Specifica: [architettura](docs/TrueRenderer-Architettura.md). Evidenze dettagliate: [rapporti](STATO.md#registro-delle-verifiche-e-degli-incrementi). Decisioni e progetti: [indice documentale](docs/README.md). Vincoli di lavoro: [AGENTS.md](AGENTS.md).

## Regola di aggiornamento

Per ogni incremento aggiornare qui implementato, verificato, aperto e mancante, con una breve nota e i collegamenti alle prove. Aggiornare le caselle pertinenti senza ripetere il resoconto in altre sezioni. I dettagli storici conservano il perimetro della loro campagna e non certificano i binari successivi. Modificare specifiche e ADR solo quando cambiano requisiti o decisioni; README solo quando cambiano informazioni utili all'utente. Aggiungere rapporti quando esistono nuove prove da documentare.

Le architetture rimandano a questo file e non ne incorporano il contenuto. `python3 scripts/sync-docs.py` aggiorna i rimandi e l'appendice della specifica anteprime: serve quando cambiano le sue fonti, non a ogni aggiornamento di stato. Il backup originale v1.2 resta immutato.

## Punto di ripresa

### Caricamento cartella e percentuale — 20 settembre

Implementati barra percentuale cliccabile nella riga superiore e popup con conteggi, errori/esclusioni, pausa/ripresa, annullamento e passaggio al background. Preferenza persistente in Anteprime e RAW: background predefinito oppure preparazione in primo piano con popup modale fino al termine. Scansione indeterminata prima del totale; contatore della cartella, non delle sole foto filtrate; annullamento non finge il 100%. Coda limitata a una richiesta aggiuntiva, riuso cache, attesa memoria esplicita, nessun riavvio su refresh identico. Restano invariati originali e vincoli decoder. Bundle `dist/TrueRenderer-loading.app`: cinque nuove regressioni, 20 test UI, suite completa 136 ordinari + 11 integrazioni, corpus nativo 12/12 e 30 copie D750 Piena/Apple in entrambe le modalità senza errori; Standard verificato sulla revisione precedente. Dodici catture impostazioni IT/EN/minima/200%, screenshot popup e due XPC passati. [Rapporto e limiti](reports/folder-loading-macos.json). Il 100% non certifica dettaglio nativo residente o persistenza SSD; nessun nuovo gate memoria/colore/rilascio, né ripetizione nativa Windows/altri motori in questo incremento.

- [x] Implementare barra, popup, preferenza e contatori verificabili.
- [x] Verificare migrazione impostazioni, clic, pausa, errori, annullamento e riletture.
- [x] Completare bundle finale, screenshot e verifica XPC; registrare evidenze.

### Errori anteprime D750 — verifica richiesta il 20 settembre

Prosecuzione dei requisiti RAW, memoria e richieste concorrenti del progetto anteprime (§12) e del progetto motori RAW. I 30 originali D750 passano 120 sviluppi singoli sui quattro motori e conservano gli SHA-256 iniziali. Il messaggio segnalato «Read error» è l'etichetta generica degli errori delle miniature: riprodotto un rifiuto di memoria a 2 GiB in sei dei 24 gruppi della baseline, con Apple, LibRaw bilineare e AHD, non una corruzione dei RAW. Il limite riguardava soltanto la coda pronta e lasciava trattenere fino a cinque snapshot tra i thread; ora due lease complessivi coprono anche lettura e decoder attivi/in attesa, mantenendo cancellazione e coalescenza. Riprodotto e corretto anche il caso 1:1 + filmstrip: il viewer completa prima i derivati visibili e riprende poi il dettaglio nativo, rilasciando gli antenati troppo grandi. Tooltip sulle miniature con il vero errore.

Bundle consegnabile: `dist/TrueRenderer-d750.app`. Passati 24 gruppi (30 foto × quattro motori × Standard/Piena) sia a cache fredda sulla revisione con lo stesso pool finale, sia riaprendo i derivati nel pacchetto definitivo; quest'ultimo passa anche cinque stadi nativi 1:1/filmstrip, cinque confronti pixel senza differenze, 40 transizioni foto/zoom/pan/qualità sui quattro motori e due XPC con recupero. La prova estesa aveva rilevato un ulteriore rifiuto nel cambio Piena→Standard dopo lo zoom: ora si rilasciano anche gli antenati obsoleti, proteggendo le identità condivise ancora richieste e i fotogrammi completati. Superati `scripts/verify.sh --gui` (131 test ordinari + 11 integrazioni, due test privati sostituiti dalla campagna isolata), due nuove regressioni, controlli protocollo/ricampionamento e 291 link documentali. [Rapporto, identità dei binari e limiti](reports/d750-preview-memory-macos.json).

**Limite aperto distinto dagli errori corretti:** nella navigazione nativa D750 il picco aggregato è 3.332.456.448 byte RSS / 3.646.361.728 byte footprint con 2 GiB configurati; tutti i quattro casi superano il budget fisico, pur restando entro i crediti di ammissione. Misura comprende i readback della prova; isolare il costo del test da quello produttivo e correggere gli extra rimane da fare. Copertura durante transizioni non nulla ma talvolta parziale, non continuità universale. Nessuna preferenza, fotografia originale o database dell'utente modificato; gli SHA-256 originali sono ricontrollati, fotografie e report dettagliati restano in `var/`. Nessun gate di memoria fisica, colore o rilascio chiuso.

- [x] Identificare i quattro motori Mac disponibili e verificare tutti i 30 originali senza modificarli.
- [x] Riprodurre l'errore delle anteprime, correggere la causa e aggiungere regressioni.
- [x] Verificare griglia/viewer su tutti i motori, bundle finale e XPC; registrare perimetro e limiti.

### Bundle e priorità generali

Il bundle Mac più recente è `dist/TrueRenderer-loading.app`, con le prove del caricamento indicate sopra; `dist/TrueRenderer-d750.app` conserva la precedente campagna sui quattro motori. Il precedente `dist/TrueRenderer-navigator-compact.app` conserva la campagna del 19 settembre: barra percorso compatta, schede Esplora/Libreria stabili, 14 regressioni UI, sei layout nativi, dodici catture preferenze e due servizi XPC. [Rapporto e limiti](reports/navigator-layout-macos.json). La campagna precedente del navigatore appartiene a `dist/TrueRenderer-navigator.app`, conservato separatamente: [rapporto](reports/filesystem-browser-macos.json). La campagna storica D750 e PNG 12/24/45 MP resta distinta dalle verifiche odierne: [rapporto fotografico](reports/large-pressure-raw-macos.json). Windows: ultima suite registrata di 99 test ordinari + 8 integrazioni, pubblicata in `67146e3`; GUI e Nikon non ripetuti per gli ultimi fix. [Rapporto](reports/gamma-orientation-fixes-windows.json).

**Priorità:** investigare il superamento della memoria sui 45 MP Full (somma RSS 4.816.601.088 byte e footprint 4.296.512.936 byte con 4 GiB configurati) e quello D750 a 2 GiB appena registrato sopra. Pressione fisica OS, decode RAW ridotto/regionale e qualifica fotografica estesa restano aperti. Nessun gate R0–R4 è chiuso; Standard/Piena sono qualità di anteprima, non assurance Standard/Riferimento.

Il prossimo incremento deve investigare il superamento memoria misurato su 45 MP Full e ampliare pressione OS, target colore e fotocamere. Fit, livelli residenti ridotti e transizioni sono verificati nei perimetri PNG 12/24/45 MP e D750 descritti sopra; decode RAW ridotto/regionale, presentazione effettiva e campagna statistica restano aperti. I gate che richiedono altri RAW, hardware o persone rimangono espliciti. Il seguente ordine riguarda invece la roadmap complessiva R0–R4.

1. L’incremento di ricampionamento 0.1.2 è chiuso per il sottoinsieme R0 provato; estendere in R1 orientamenti, isotropia, alias/overshoot, LOD e verifica fisica su più scale/display. ADR 0003 conserva le scelte e i criteri di revoca.
2. Completare i confini R0 sul Mac: bootstrap/handle/output avversari, memoria end-to-end, sorgenti sotto writer concorrente e recupero XPC. Decidere API/minimi OS e firma reciproca con evidenze.
3. Completare la qualifica della verticale Windows e i gate toolkit/display/accessibilità su hardware reale; affiancare interviste e corpus autorizzato. Estendere il corpus oltre D750 e D40 già autorizzati. Hardware, persone reali e credenziali di distribuzione non si possono sostituire con test simulati.
4. Superati i gate pertinenti, R1: codec bitmap qualificati, EXIF/ICC/monitor, renderer/tile di riferimento e viewer completo. Qualificare il percorso esterno anticipato nella 0.1.3; mantenere distinti Anteprima e modi di riferimento.
5. R2: catalogo completo, XMP sicuro, durabilità/restore e pilot. L’esito del pilot decide l’impegno successivo in R3 matrice RAW/LibRaw e gigapixel.
6. R4: hardening, misure p95, accessibilità/piattaforme, licenze, installer e rilascio. Nessuna milestone è chiusa solo perché l’app si avvia.

### Attività di ripresa per area

- Dopo la correzione Fit, estendere a RAW ridotti/pressione e individuare la presentazione effettiva prima della campagna p95/p99. Il readback attuale include il costo della cattura.
- RAW Mac: i 30 D750 sui quattro motori e nove ritagli centrali allineati sono verificati; servono altre fotocamere, target colore misurato, rumore/moire e ICC. Usare originali autorizzati in sola lettura e cataloghi separati.
- Windows: installazione pulita, contesa writer/cache e diagnostica PNG malformata o conflittuale.
- Qualifica: colore/display, confronto fra motori con ritagli allineati, corpus autorizzato esteso, pressione memoria e latenze evento→frame. Nessun gate R0–R4 chiuso; Standard/Piena restano qualità di anteprima, non assurance Standard/Riferimento.


## Riordino documentale — 19 settembre 2026

Ulteriore accorpamento richiesto: specifiche e decisioni riunite direttamente in `docs/`, senza `adr/`. Sette ADR incorporati per argomento: 0003/0005/0006 nel progetto anteprime, 0008 nel progetto RAW, 0002/0004/0009 in isolamento decoder e formati. Sei file in meno rispetto alla precedente struttura; testi e vincoli conservati con ancore numerate. Aggiornati rimandi, istruzioni e sincronizzatore; stato resta qui, report e notice nelle rispettive sedi tecniche. Nessuna modifica applicativa o nuova qualifica.

Verificato questo accorpamento: contenuto integrale dei sette ADR conservato salvo livelli dei titoli e percorsi dei link; 290 collegamenti locali validi e seconda sincronizzazione senza modifiche. Originale v1.2 e LICENSE invariati; negli avvisi di licenza aggiornato solo il rimando. Nelle architetture aggiornate le appendici generate e il solo link LibRaw esterno al blocco. Le vecchie sedi restano recuperabili da Git.

Su richiesta del titolare, aggiornamenti e lavori conclusi restano soltanto qui. Eliminati sei Markdown: ADR 0001 e 0007, progetto restyling, revisioni Windows, campagne RAW e registro separato delle verifiche. Sintesi sotto; testi completi recuperabili da Git, rapporti JSON e prove negative conservati. Gli ADR restanti descrivono contratti ancora attivi, non liste di attività concluse. Nessuna nuova prova applicativa in questo riordino; gate e limiti aperti invariati.

Verifica documentale: 287 collegamenti locali validi, sincronizzazione delle appendici idempotente, testo delle architetture fuori dai blocchi gestiti invariato; originale v1.2, LICENSE e NOTICE invariati. Nessun rapporto JSON storico, fotografia, catalogo o backup eliminato.

<a id="prototipo-e-port"></a>

### Prototipo e port conclusi

Realizzati il prototipo Rust/egui/wgpu con corpus controllato, catalogo SQLite e annotazioni durevoli, quindi la porta `Decoder` comune e il primo port Windows. Il worker singolo iniziale è superato dai due XPC Mac e dal confinamento LPAC Windows; backend e licenze restano negli ADR 0002/0004/0008/0009. Il contratto decoder ancora valido è conservato in ADR 0009; toolkit, ICC, porte WorkingSpace/TileProvider e qualifica multipiattaforma restano aperti.

<a id="restyling-concluso"></a>

### Restyling concluso

Applicati stile neutro, strumenti in alto, cartella/conteggi in basso, griglia fotografica, ispettore richiudibile e quattro schede preferenze con bozze separate dall'applicazione. Il viewer distingue qualità globale e override di sessione per foto; il cambio globale li azzera. Pixel fotografici non alterati dalla selezione, nomi lunghi con tooltip, identificatori stabili fra lingue e badge Anteprima conservati. Verificati layout IT/EN, finestra minima/200%, bozze e rendering nel [rapporto toolbar](reports/toolbar-macos.json); navigatore e barra compatta nel [rapporto aggiornato](reports/navigator-layout-macos.json). Riproduzione: test `ui::`, `--restyle-smoke` con corpus/catalogo separati e suite XPC; accessibilità e display non qualificati.

<a id="revisioni-windows-concluse"></a>

### Revisioni Windows concluse

Corretti isolamento LPAC/quote, LibRaw 0.22.2, parsing delle preview, classificazione TIFF/RAW, provenienza, hard link e invalidazione sorgenti; poi selezione/zoom nelle preferenze, PNG cICP/gamma, alpha/colorimetria TIFF, fallback RAW, timestamp cache e orientamento per IFD. Ultima campagna: 99 test ordinari + 8 integrazioni; non attribuita automaticamente ai binari successivi. Evidenze audit→correzione: [iniziale](reports/pre-main-review-windows.json) → [fix](reports/pre-main-fixes-windows.json), [serale](reports/review-continuation-windows.json) → [fix](reports/review-followup-fixes-windows.json), [generale](reports/recheck-windows-2026-09-14.json) → [fix](reports/general-review-fixes-windows.json), [gamma/IFD](reports/additional-review-windows.json) → [fix](reports/gamma-orientation-fixes-windows.json). Restano installazione pulita, persistenza sotto contesa (errore 33), diagnostica PNG malformata/conflittuale, conversioni TIFF/ICC e qualifica estesa.

<a id="campagne-raw-concluse"></a>

### Campagne RAW concluse

Esteso il motore proprio al modello esatto D40 e corretto il cambio motore durante scansione: 66/66 sviluppi su 22 NEF a ISO 200 e regressione D750, senza ampliare implicitamente la matrice camere ([rapporto Windows](reports/raw-engines-d40-windows.json)). Su Mac: 120 sviluppi D750 sui quattro motori dopo il fix heap LibRaw/XPC, nove confronti centrali, 240 azioni PNG 12/24/45 MP, 20 sotto pressione renderer e 40 RAW ([rapporto](reports/large-pressure-raw-macos.json)). Ritagli allineati solo per traslazione intera ±16 pixel: Apple/AHD non sono riferimenti della scena. Rimane il superamento memoria 45 MP Full indicato in apertura; mancano pressione OS, decode RAW ridotto, colore misurato e p95/p99. Riproduzione: `scripts/test-large-navigation.py`, `scripts/compare-raw-patches.py` e `--verify-raw-engines`, esclusivamente con corpus autorizzato e dati separati.

## Piano operativo

## Navigatore filesystem — implementazione del 19 settembre 2026

### Revisione estetica richiesta: explorer in stile IDE

- [x] Correggere l'allineamento delle schede: margine comune di 8 punti in Libreria/Esplora, conservando le larghezze indipendenti. Lo switch viene completato nello stesso frame prima della presentazione e riusa rami/righe caricati, senza ricostruzione o nuova richiesta posizioni a ogni ritorno; i rami compressi restano compressi.
- [x] Ridurre il padding verticale della barra percorso da 16 a 6 punti. Regressione con clic reali nelle due lingue: coordinate dei tab identiche attraverso 16 switch, larghezze distinte conservate, nessuna ricostruzione delle righe e altezza barra entro 42 punti.
- [x] Riesaminare i Markdown dopo gli accorpamenti precedenti: mantenere separati specifiche anteprime, motori RAW, navigatore e composizione desktop; ADR e rapporti descrivono decisioni/prove differenti. Anche il breve progetto restyling conserva regole di griglia/viewer/preferenze non proprie del navigatore. Nessun altro file eliminato in questa revisione; copie architettura, originale v1.2, istruzioni, laboratorio e notice mantengono il loro scopo.
- [x] Verificare il bundle con barra compatta: fmt, Clippy desktop, build release, 14 regressioni UI, sei layout nativi IT/EN/compatti/200%, dodici catture preferenze e integrazione XPC passati. Aggiornate le quattro catture pubbliche `reports/navigator-*.png` e aperto `dist/TrueRenderer-navigator-compact.app` con dati di prova separati. [Rapporto con hash e limiti](reports/navigator-layout-macos.json); nessuna qualifica statistica di latenza o chiusura dei gate.
- [x] Compattare righe e margini, aggiungere guide gerarchiche, chevron e icone vettoriali cartella/immagine/file; selezione blu a riga intera, scheda attiva sottolineata e controlli piatti.
- [x] Verificare Clippy, 13 regressioni UI e sei layout nativi IT/EN, compatti e 200%. Bundle separato `dist/TrueRenderer-navigator-ide.app`; [rapporto](reports/navigator-ide-macos.json).
- [x] Completare dodici catture preferenze e aggiornare le tre immagini del README, aggiungendo il rimando al layout compatto. Immagini `reports/navigator-*.png` su corpus sintetico; screenshot storici conservati. Verifica XPC sul nuovo bundle passata e app aggiornata aperta con dati di prova separati. Restano i limiti di qualifica già elencati sotto.

### Implementazione funzionale e razionalizzazione documentale

Richiesta corrente del titolare: implementare il [progetto Esplora](docs/progetto-navigatore-filesystem.md), quindi razionalizzare i Markdown conservando specifiche, decisioni ed evidenze. Il lavoro non chiude i gate R0–R4.

- [x] Collegare schede Libreria/Esplora, albero pigro, aperture asincrone, breadcrumb, cronologia, tastiera, filtro nomi e modalità delle voci.
- [x] Separare enumerazione e writer con due slot filesystem e code limitate; risultati progressivi e terminali identificati, scansione indipendente dalla generazione decoder, salvataggi conservati al cambio cartella.
- [x] Aggiungere preferiti transazionali, migrazione libreria 1→2 con backup verificato, export e sessione versionata distinta dalle impostazioni.
- [x] Completare regressioni della prima implementazione Mac: navigazioni rapide/fallite, cronologia, refresh, file mirato con filtri, cambio motore durante scansione, tastiera, preferiti/sessione, migrazione/backup/export e salvataggi pendenti.
- [x] Eseguire `scripts/verify.sh --gui`: fmt, Clippy workspace con warning negati, build debug, 128 test ordinari e 10 integrazioni esplicite passati; due test fotografici privati esclusi. Sei controlli IPC, due regressioni Python, 24 segnali di ricampionamento e prova della superficie con otto schermate passati.
- [x] Costruire la release e il bundle separato, verificare sei layout nativi IT/EN, Libreria/Esplora, griglia/viewer e 200%, ispezionare tutte le catture e ripetere XPC sul medesimo hash. Controllo automatico dell'albero dentro il viewport: almeno 64 punti nella finestra 550×360. [Rapporto](reports/filesystem-browser-macos.json).
- [x] Provare un listing di 100.001 file sintetici: arresto esplicito alla quota byte prima del limite numerico, batch limitati, ordine terminale, zero cache/catalogo e crediti restituiti dopo shutdown. Test `hundred_thousand_entries_stop_explicitly_at_memory_quota`, eseguito separatamente con `--ignored`; non è una campagna di latenza né una prova NAS.
- [x] Accorpare sei Markdown in due, conservando integralmente i testi salvo titoli/ancore/link: quattro revisioni in [revisioni Windows](STATO.md#revisioni-windows-concluse), D40 e grandi RAW in [campagne RAW](STATO.md#campagne-raw-concluse). Quattro file in meno, originali recuperabili da Git. Aggiornati indice, README, rimandi e appendici tramite sincronizzatore; script di controllo link/ancore passato. Originale v1.2, LICENSE, notice e vendor invariati.

Correzioni emerse nelle prove: glifi assenti dal font, nomi centrati nell'albero e pannello compatto senza spazio utile. Controlli ASCII, testo allineato a sinistra, finestra limitata al viewport e menu Preferiti/Recenti scorrevoli correggono i sei layout osservati. Primo tentativo della suite interrotto dall'assenza delle fixture Metal; generate con lo script del progetto, quindi suite completata. Il test TIFF nativo richiede l'esecuzione fuori dal sandbox aggiuntivo del terminale. Il link XPC continua a segnalare oggetti LibRaw compilati per macOS 27 rispetto al target 13: questa campagna su macOS 27/M4 non qualifica il minimo OS né una firma di rilascio.

I vecchi nomi di documenti negli inventari JSON storici rimangono tali, per non alterare l'evidenza delle campagne. Le copie delle architetture richieste dal flusso del titolare, l'originale immutabile, gli ADR, i progetti con requisiti unici e le attribuzioni non sono stati eliminati. La cronologia degli accorpamenti non viene più duplicata nell'indice documentale.

**Aperto/mancante rispetto alla specifica completa:** watcher nativi, identità persistente e stato offline qualificato dei volumi, limite di concorrenza per volume, adattatori alias/shortcut/cloud e ripristino completo delle espansioni discendenti. Il fallback usa polling ogni 15 secondi, due slot totali, al massimo 16 rami riletti e 64 MiB prenotati per navigazione/listing; link/pacchetti/placeholder riconosciuti non vengono attraversati automaticamente. Mancano prove native Windows, VoiceOver/NVDA, NAS con syscall bloccate, fault injection di crash/disco pieno nella migrazione e campagna statistica prestazioni/pressione. N0–N7 non sono dichiarati integralmente conclusi e i relativi gate restano aperti.

## Progetto di incremento — navigatore filesystem commutabile, 15 settembre 2026

Richiesta storica del 15 settembre: pianificare un navigatore di file/cartelle a sinistra, **commutabile con il pannello già presente**. Specifica: [progetto Esplora / Libreria](docs/progetto-navigatore-filesystem.md). La pianificazione era solo documentale; l'implementazione e il perimetro verificato del 19 settembre sono registrati sopra. Le caselle seguenti riguardano la qualifica completa, non la sola presenza delle funzioni.

- [x] Esaminare codice e architettura e documentare layout, switch Libreria/Esplora, interazioni, concorrenza, persistenza, fasi e matrice di accettazione; nessuna implementazione applicativa in questa sessione.
- [ ] N0: verificare contratti, baseline e spike nativi/accessibilità, includendo la conservazione del pannello attuale.
- [ ] N1–N2: modello dell'albero, enumerazione asincrona limitata, coordinatore di navigazione e scansione progressiva separata dai salvataggi.
- [ ] N3: pannelli Libreria/Esplora commutabili, file/cartelle, breadcrumb e cronologia, mouse/tastiera e layout IT/EN; switch senza reset di selezione, filtri, zoom o qualità.
- [ ] N4–N5: preferiti durevoli, sessione, migrazione/backup, refresh, watcher, volumi e casi filesystem.
- [ ] N6–N7: eseguire la matrice di accettazione, verificare bundle/XPC e piattaforme dichiarate, registrare risultati e limiti in questo documento.

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
- [x] Definire composizioni per griglia, viewer e preferenze; [progetto](STATO.md#restyling-concluso).
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

Si sviluppa una verticale per volta. Ogni consegna contiene codice compilabile, prove riproducibili, limiti osservati e aggiornamento di questo stato; l'architettura cambia quando cambiano requisiti o decisioni. Nessuna modifica agli originali; annotazioni separate dall'indice. Nessun servizio cloud, account o canone. Tutto il progetto, inclusa la toolchain locale, rimane nella cartella del progetto. Dati di sviluppo locali in `var/`; prima di una release spostarli nel percorso app-data nativo qualificato.

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

**Verifica dell’incremento conclusa:** avvio del bundle dal Finder passato dopo il consenso macOS al Desktop; passata anche una copia con bundle e corpus indipendenti. Dettagli in `STATO.md` (registro e collegamenti ai rapporti JSON).

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

Decisione e limiti in `docs/isolamento-decoder-e-formati.md#adr-0002`; evidenze in `STATO.md` (registro e collegamenti ai rapporti JSON). Per completare il gate XPC sul Mac restano la suite avversaria del bootstrap/ciclo di vita e la qualifica estesa della memoria end-to-end; l’ordine dell’incremento locale corrente è indicato in apertura. Nella 0.1.1 i PNG esterni restavano esclusi; la modifica del perimetro nella 0.1.3 è registrata in ADR 0004. Il gate completo della sandbox rimane aperto.

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

**Stato: implementazione 0.1.5 disponibile; qualifica integrata aperta.** Baseline 0.1.4 conservata in `reports/preview-baseline-macos.json`; decisioni applicate e limiti in [ADR 0006](docs/progetto-anteprime-cache-prestazioni.md#adr-0006). Anteprima Standard/Piena rimane distinta dai badge di pipeline Standard/Riferimento. Nessun gate R0–R4 è chiuso da questo incremento.

- [x] A, modelli: qualità/richieste, migrazione compatibile, budget/lease, ammissione prima delle allocazioni, preferenze RAM/GPU/CPU e adattamento all'alimentazione.
- [x] A, baseline locale: conservare misure della build 0.1.4 senza confondere il caricamento dell'intera piramide con le nuove miniature autonome.
- [x] Revisione locale 9 settembre: 30 NEF Nikon D750 a risoluzione nativa, Standard/Piena fredde/calde e riapertura cache verificati con budget esplicito 3 GiB; 64 test Rust e suite nativa finale passati. Alla chiusura di quella prova il lavoro era locale; ora è incluso in `10123b5`. Ripresa nella sezione [Punto di ripresa](#punto-di-ripresa).
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

Le caselle separano il codice verificato dalla qualifica ancora necessaria; non modificano i criteri del progetto per far risultare conclusa una fase parziale. Evidenze aggiornate in `STATO.md` (registro e collegamenti ai rapporti JSON) e `STATO.md`.

**Licenza LibRaw, verifica storica dell'8 settembre:** la nota e le fonti sono conservate in [ADR 0008](docs/progetto-motori-raw.md#adr-0008) e §20.1 dell'architettura. La build di quella campagna non incorporava LibRaw; quella attuale collega LibRaw 0.22.2. Inventario, notice e obblighi dell'artefatto effettivamente distribuito restano da verificare prima del rilascio.

## Esperimento locale — motori RAW selezionabili (12 settembre 2026)

Richiesta del titolare: progettare ed eseguire la pipeline sperimentale, affiancandola ai motori esistenti su Windows/macOS. Incremento pubblicato in `67146e3`. Progetto: [motori RAW](docs/progetto-motori-raw.md). Anticipazione locale del demosaic prima post-v1, senza modifica dei gate.

- [x] Leggere AGENTS.md, piano, registro e architettura; definire contratto e verifiche.
- [x] Collegare selezione persistente, IPC, identità cache e invalidazione al cambio motore.
- [x] Implementare LibRaw AHD e TrueRenderer direzionale fp32 con calibrazione esplicita.
- [x] Verificare segnali sintetici, regressioni e RAW D750 su Windows; registrare i risultati: 32 casi analitici e 90 sviluppi completi, originali invariati. Superiorità generale non dimostrata.
- [x] Provare selettore e ritorno al motore precedente nella finestra; confrontare la superficie con CPU. Rimosso il secondo dithering egui: 558.144 pixel delle quattro regioni entro un livello sRGB8, dopo un primo esito negativo a due livelli.
- [x] Aggiornare stato, licenze locali e ripresa; sincronizzare le architetture preservando testo esterno ai blocchi e backup originale.
- [x] Revisione richiesta e D40: correggere scansione revocata dal cambio motore, aggiungere il modello esatto e verificare 66 sviluppi, UI/scansione e regressione D750. [Dettagli](STATO.md#campagne-raw-concluse).
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

Il documento stima 56–84 settimane di milestone, circa 64–105 con riserva; sono ipotesi per una persona, non una data promessa. Prima revisione delle stime dopo le misure R0. Per ogni incremento aggiornare questo documento. Aggiungere rapporti per nuove campagne di verifica; sincronizzare le architetture soltanto se cambiano le fonti della specifica anteprime.

## Matrice dei requisiti e budget

### Confronto completo con la proposta v1.2

Stati: **parziale** = esiste codice utilizzabile ma non tutto il requisito; **da qualificare** = esistono prove limitate, insufficienti per la garanzia v1; **mancante** = la funzione richiesta non è implementata; **post-v1** = esclusa deliberatamente dalla prima versione. Questa matrice riguarda tutti i capitoli della proposta, senza attribuire una percentuale artificiale di completamento.

| Riferimento | Realizzato finora | Cosa manca rispetto al documento |
|---|---|---|
| §1 — prodotto, invarianti e decisioni | Nome TrueRenderer, nucleo Rust, separazione UI/motore, CPU lineare, dati durevoli separati, worker persistenti; ADR 0001–0009 | **Parziale.** Conferma toolkit sui due OS, TileProvider, Little CMS, requisito di release dei peer, canali, minimi OS/GPU, formato XMP e matrice RAW. Standard/Riferimento non disponibili; firma ad hoc non chiude le decisioni di rilascio. |
| §2.0 — bisogno e pilot | Corpus sintetico controllato, 30 NEF Nikon D750 e 22 D40 autorizzati usati nelle prove tecniche e workflow di selezione provabile localmente | **Parziale.** Almeno cinque interviste, ampliamento dei casi reali rappresentativi e pilot R2 con 8–12 fotografi; misura dell’uso settimanale per quattro settimane. Nessuna intervista o validazione commerciale è stata inventata. |
| §§2.1–2.4 — requisiti, formati, budget | UI reattiva con decodifica separata dai salvataggi; corpus più JPEG/PNG/TIFF/RAW su Windows LPAC e macOS XPC, con formati aggiuntivi nel bundle Mac storico | **Parziale/da qualificare.** Qualifica dei sottotipi bitmap, BigTIFF e fotocamere RAW; budget p95, griglia 100k, memoria globale e accessibilità. Il recupero XPC di circa 10 s **non soddisfa** il requisito < 1 s. Dettaglio budget sotto. |
| §3 — immagine reale e ispezione | Working Rec.2020 lineare fp32 premoltiplicato, RAW proprio senza clamp RGB intermedio, bilineare/AHD con raster intermedio intero 16 bit, badge Anteprima, informazioni decoder/digest, campionatore del livello residente dichiarato e istogramma composito sRGB | **Parziale.** Provenienza completa per ogni stadio/revisione, istogrammi e campioni selezionabili per stadio, ICC/monitor, clipping/gamut, cache promossa solo dopo verifica. Un test GPU aritmetico non certifica il display. |
| §4 — stack | Workspace Rust bloccato, egui/eframe e wgpu/WGSL, SQLite bundled, inventario dipendenze | **Da qualificare.** Gate egui di focus, IME, DnD, screen reader e 200% su Windows/macOS; qualifica dei backend adottati, Little CMS e XMP Toolkit; LibRaw scelta con qualifica RAW aperta. Gli spike non rendono definitivo tutto lo stack. |
| §§5.1–5.2, 5.6 — moduli e stato | Sei crate più desktop; reducer con comandi/effetti, identità UUID distinta da percorso/digest, revisioni e stato di salvataggio | **Parziale.** La mappa modulare completa non è realizzata: mancano provider tile, servizi ICC/metadata/RAW, cache tiled versionata v1 e journal completo degli effetti esterni. |
| §§5.3–5.5 — processi e apertura | Due servizi XPC separati, copia privata degli output, protocollo bounded, allowlist nel percorso pipe/corpus e decoder esterni in XPC macOS o LPAC/Job Object Windows, coda finita, generazioni, timeout/riciclo; SQLite in writer separato | **Parziale/da qualificare.** Bootstrap ostile, autorità residua e descrittori duplicati, mutazioni concorrenti della sorgente/spool coerente, quote end-to-end, peer di release e qualifica completa del confine Windows. Gli esterni sono ammessi come Anteprima nei confini di ADR 0004 e 0009. |
| §6 — pipeline bitmap | Corpus analitico invariato; ImageIO/ColorSync sul Mac storico; JPEG/PNG/TIFF portabili su Windows con regressioni gamma, colore dichiarato, alpha e orientamento | **Parziale.** Matrice completa di sottotipi/bit depth, JPEG CMYK/YCCK, BigTIFF, palette/gray/ICC, rifiuti ostili esaustivi e copertura multipiattaforma completa dell'alpha associata. Il percorso Apple non sostituisce la qualifica dei backend multipiattaforma previsti. |
| §7 — RAW | Baseline Apple CIRAWFilter; incremento pubblicato con selettore, LibRaw bilineare/AHD e TrueRenderer direzionale fp32, ricette e cache distinte; campagne D750/D40 sui tre motori Windows, Bayer sintetici e calibrazione/rifiuti verificati | **Parziale/sperimentale.** Nuovo bundle Mac, confronto Apple allineato, WB/matrici e ΔE00 misurati, matrice camere/CFA, rumore/artefatti e prestazioni da qualificare. Anticipato localmente il demosaic proprio di §7.3 su richiesta; nessun supporto universale o superiorità generale dichiarata. |
| §8 — colore end-to-end | Riferimento analitico sRGB ↔ Rec.2020, compute WGSL separabile/display SDR verificato contro CPU scalare e conversione esterna ColorSync; uscita sRGB8 dichiarata | **Mancante/da qualificare in R1.** Little CMS, ICC v2/v4 RGB/GRAY/CMYK, precedenza metadata, assegnazioni persistenti e bypass, libreria profili, monitor/intent/BPC, LUT adattive, ΔE00, display multipli. Soft proof §8.9 post-v1. |
| §9 — HDR/gain map/float | Nessuna implementazione | **Post-v1**, non requisito bloccante della v1 SDR. Serviranno contratti formato/display e nuove prove prima di abilitarli. |
| §10 — ricampionamento e geometria | Correzione 0.1.2: filtro lineare alla dimensione fisica, piramide CPU, 1:1 diretto, percorso condiviso griglia/inspector/viewer/confronto, filtro separato per alpha | **Parziale.** ADR 0003 registra il margine di banda aggiunto a Lanczos3. Mancano qualifica completa isotropia/zone plate, estensione del confronto GPU/CPU a tutti i grafi e transizioni LOD, geometria combinata con gli orientamenti, coordinate pan/zoom interamente f64, ispezione nearest esplicita agli ingrandimenti e comando 100% logico. Il limite di 8.388.608 pixel riguarda il raster di presentazione; le sorgenti ora arrivano a 67.108.864 pixel. |
| §11 — gigapixel/tile/cache | Piramide CPU per sorgenti entro 64 Mi pixel; cache lossless fp32 persistente per cartella, temporanei, quote, LRU/scadenza e impostazioni (0.1.4); livelli autonomi e cache v2 con writer asincrono (0.1.5) | **Mancante in R3.** TileProvider, TIFF nativo piramidale, provenienza SubIFD, cache tiled/spool, GC v1 e invalidazione per tile, generazione tiled cancellabile, prefetch/residenza GPU per tile, prova 4 Gpx. La piramide attuale in RAM non equivale al sistema gigapixel. |
| §12 — CPU/GPU e prestazioni | Livelli autonomi, budget globale di ammissione, pool Rayon/NEON, compute WGSL e texture dirette, fallback CPU; writer asincrono con precedenza ai lettori, I/O distinto, P0–P6/FIFO e prefetch direzionale/adattivo, pressione macOS; A/B scalare/parallelo/GPU del renderer | **Parziale.** Reset reali/OOM, blacklist/selezione GPU, LUT, telemetria completa, consumi, pressione fisica/altri OS e p95 evento→frame. Il frame della stessa revisione copre il raffinamento; la continuità va ancora qualificata su tutte le transizioni/display. |
| §§13.1–13.2 — dati e durabilità | `library.sqlite` separata da `index.sqlite`, writer unico, UUID/revisioni, conflitti, undo come revisione, backup consistente verificato, export JSON no-clobber; prove di ricostruzione indice | **Parziale.** Schema v1 completo, journal di intenti/esiti incerti, migrazioni successive, batch atomici e undo di batch, redo, retention/restore UI/export portabile v1 e recovery dopo power-fault/disco pieno. Le transazioni correnti sono per singola immagine. |
| §§13.3–13.6 — catalogo e metadata | Scansione cartella, rating/scarto/label/keyword piatte, ricerca e filtri locali, apertura file/cartella e metadati tecnici con provenienza decoder; monitor dei file richiesti/residenti con invalidazione e gestione di rimozione/ripristino | **Parziale; completamento in R2.** Watcher/scansione incrementale dell’intero catalogo, gestione dei volumi e riconciliazione delle rinomine/file mancanti, keyword gerarchiche, raccolte statiche/intelligenti, facce/FTS, EXIF normalizzato, XMP a tre vie/round-trip, conflitti e coordinamento filesystem. Nessuna scrittura su originali o sidecar oggi. |
| §§14.1–14.3 — browser desktop | Griglia virtualizzata con celle regolabili, preview laterale, metadati/annotazioni, ricerca, barra stato, selezione e filmstrip | **Parziale.** Breadcrumb/back-forward, albero filesystem e preferiti DnD, sezioni riordinabili, viste Dettaglio/Elenco con colonne, intero range celle 64–1024 e badge metadata/XMP, pannelli fotografici completi. |
| §14.4 — viewer e confronto | Adatta, 1:1 fisico, pan/zoom, schermo intero, due immagini con centro normalizzato/zoom condivisi e fit indipendente | **Parziale.** Loupe, tendina/dissolvenza con contratto geometrico, due campionatori e differenza del punto, carosello revisione, surround configurabile, filmstrip sui quattro lati. Confronto > 2 e registrazione automatica restano post-v1. |
| §§14.5–14.7 — comandi, layout, colore | Principali scorciatoie di selezione/valutazione/viewer e protezione dei campi testo; pannelli nascondibili | **Parziale.** Rimappatura tasti, spazi di lavoro salvati, overflow e comandi mancanti, finestra Colore, override ICC, avvisi gamut/clipping e controlli delle modalità qualificate. |
| §15 — Android | Nessun frontend mobile | **Post-v1.** Non conteggiato come funzione desktop mancante. |
| §16 — design e accessibilità | Tema scuro sobrio, gerarchia tipografica, focus visibile, feedback/errori e scorciatoie | **Da completare/qualificare.** Baseline VoiceOver/NVDA, sola tastiera su tutti i controlli, 200%, alto contrasto, IME e semantica accessibile delle immagini; tema/preferenze e tutti gli stati della specifica. |
| §17 — piattaforme | Baseline bundle arm64/macOS 26.6.2 con XPC; port Windows x86-64 pubblicato, provato sui D750/D40 | **Parziale.** Nuovo bundle con motori su Mac, minimi OS/GPU, display, installer/firma/notarizzazione da qualificare. Linux, Windows arm64 e Android non sono target v1. |
| §18 — sicurezza/robustezza | Allowlist non confinata, esterni XPC/App Sandbox sul Mac; worker Windows con LPAC senza capacità e Job Object; framing, copia privata, timeout e shutdown indipendente dai salvataggi | **Da qualificare.** Confine Windows completo, suite ostile completa/fuzz/sanitizer, policy OS release, output concorrente/handle residui, pressione ed errori filesystem, recovery e quarantena v1. Supervisione RSS ogni 25 ms ≠ tetto rigido di memoria. |
| §19 — test/corpus | 107 test Rust nell'ultima campagna Windows; baseline Mac con 71 test e prove IPC/XPC, diagnostica GPU, corpus numerico, smoke nativo con screenshot; campionamento, 19 fixture di formati esterni e 6 controlli aggiuntivi; 30 D750 e 22 D40 autorizzati nelle campagne distinte, prove native di invalidazione/recupero grafico | **Parziale.** Intere suite colore/ricampionamento, ampliamento del corpus fotografico autorizzato a camere/risoluzioni diverse e corpus ostile, orientamenti su altri formati/Siemens/tile, fault power/disk/device, benchmark ripetuti e CI su due OS. Uno screenshot a sua volta ridotto può introdurre moiré: servono anche confronti di pixel. |
| §20 — licenze e nome | Licenza proprietaria scelta dal titolare, repository pubblico, Cargo.lock, inventario Mac storico 212 package/330 notice, Windows 205 package/348 notice e manifest LibRaw 0.22.2 conservati, nome TrueRenderer | **Da qualificare prima di distribuire.** SBOM degli artefatti finali, obblighi delle librerie native effettive, codec/brevetti/canali e clearance del marchio **TrueRenderer**. Il cambio di nome nel progetto non costituisce una verifica commerciale del marchio. |
| §21 — build e distribuzione | Repository pubblico con README/licenza, toolchain locale, script, bundle interno firmato ad hoc, prove Finder/rilocazione e storico pacchetti | **Parziale.** CI macOS/Windows, build native e sanitizers di release, installer, firma di distribuzione/notarizzazione, install/upgrade/rollback, aggiornamenti e manuale/matrice pubblica. |
| §22 — roadmap | `STATO.md` contiene R0–R4, dipendenze e gate, registro per incremento | **R0 aperto.** Le UI/dati anticipano porzioni R1/R2 senza chiuderle. Le stime 56–84 settimane più riserva restano ipotesi della proposta, non date promesse né tempo già investito. R3/R4 ancora pianificati. |
| §23 — rischi | Registrati limiti osservati, confini del prototipo e decisioni ADR; esterni ammessi come Anteprima nei confini XPC macOS e LPAC Windows, con limiti espliciti | **Aperti.** Ampiezza dello scope, differenze display/driver/OS, input ostili, durabilità e prova del bisogno richiedono evidenze, non solo descrizioni. |
| §24 — appendici | Conservate matrice formati, glossario, fonti e revisione storica v1.2 | **Da aggiornare al rilascio.** La matrice richiesta non è una matrice di supporto attuale: il supporto corrente è descritto nel README e negli ADR 0004 e 0009, con copertura limitata alle prove elencate. I riferimenti tecnici sono requisiti/fonti, non prove di implementazione. |

### Budget della proposta: cosa è stato misurato e cosa no

- **Recupero worker < 1 s (§2.3): non soddisfatto.** La terminazione del decoder sospeso è rapida nelle prove, ma launchd può ritardare il nuovo servizio circa 10 s. Serve riprogettare/qualificare la strategia di riavvio oppure approvare una modifica esplicita del requisito; un timeout passato non basta.
- **Working set `min(2 GiB, 25% RAM)`: ammissione implementata, tetto effettivo da qualificare.** Budget condiviso, lease per campioni/scratch/upload e completamenti GPU, allowance base 384 MiB. I report di campionamento sommano RSS/footprint di host e XPC nei casi generati e sui 30 NEF autorizzati; l’ultima prova RAW riporta un picco di footprint di 2.018.970.672 byte (`reports/preview-navigation-memory-real-raw-macos.json`). Non contabilizzano separatamente tutti i driver né i picchi fra campioni. La riduzione dei limiti blocca nuove ammissioni mentre si liberano i lavori già in uso.
- **Pan/zoom < 16,7 ms e mai viewport vuoto: non ancora qualificato.** Il presenter conserva e riproietta il frame della stessa revisione durante il raffinamento; compute GPU usa texture dirette. Il confronto A/B dello stadio renderer esclude la superficie egui e non dimostra p95/p99 evento→frame o zero vuoti in tutte le sequenze.

- **Avvio, cache, griglia 100k/indice 200k, RAW, TIFF 2 Gpx e memoria a riposo:** i relativi p95/p99 e batch della tabella non sono stati misurati secondo le almeno 100 prove su due macchine. I tempi dei singoli smoke non li sostituiscono.
- **Errore GPU ΔE00/Little CMS:** non misurato. I confronti del filtro lineare e della codifica sRGB8 misurano grandezze differenti da ΔE00 e non qualificano ICC/display.
- **Durabilità crash/power-fault e sidecar:** revisioni, rollback e backup passano test funzionali, ma manca la suite fault-injected completa; nessun aggiornamento XMP è abilitato.
- **Isolamento e accessibilità:** prove macOS e Windows parziali; screen reader e scenari avversari completi restano necessari.


## Registro delle verifiche e degli incrementi

### Accorpamento documentale — 17 settembre 2026

Accorpati piano, avanzamento e ripresa in questo file; conservati caselle, gate, matrice, evidenze e limiti delle campagne. Aggiornati istruzioni e collegamenti; il sincronizzatore ora inserisce un rimando stabile allo stato invece del registro completo. Specifiche, ADR, rapporti e originale v1.2 conservati. Verifica documentale: collegamenti, conservazione dei blocchi esterni e idempotenza del sincronizzatore. Nessuna nuova prova applicativa; gate invariati.

### Quadro registrato il 16 settembre 2026

Incremento Windows/LPAC, LibRaw 0.22.2, motori selezionabili e correzioni pubblicati in `67146e3`. Ultima suite Windows: **99 test ordinari + 8 integrazioni**, build debug/release, 18 casi sintetici per worker e controlli IPC/Python/ricampionamento. [Audit e correzioni gamma/orientamento](STATO.md#revisioni-windows-concluse), [rapporto con hash](reports/gamma-orientation-fixes-windows.json). GUI e Nikon non ripetuti per questi ultimi fix.

La campagna fotografica Mac storica del 9 settembre comprende: 71 test Rust, 30 NEF nel carico a 2 GiB e bundle/XPC installato verificato. [Rapporto della baseline](reports/preview-navigation-continuation-macos.json). Il restyling del 15 settembre aggiunge una copia release separata e prove UI/corpus/XPC, senza attribuire a quel binario la campagna fotografica storica o una qualifica estesa dei nuovi motori RAW.



**Verificato:** conservati i confini effettivi di piattaforme, formati, motori RAW, isolamento, licenza proprietaria, dati durevoli e comandi di build. Screenshot e collegamenti locali controllati; il README non usa elenchi promozionali con trattini e separa la pagina di prodotto da piano, registro e rapporti.

**Aperto e mancante:** questo incremento è soltanto documentale e non modifica applicazione, bundle, database o originali. TrueRenderer resta dichiarato anteprima di sviluppo; colore end to end, ampiezza delle fotocamere, memoria sotto pressione, accessibilità, pacchetti di distribuzione e gate R0–R4 rimangono aperti nei documenti tecnici. Nessuna nuova prova applicativa o affermazione di accuratezza comparativa è stata aggiunta.

### Navigatore filesystem commutabile — pianificazione del 15 settembre 2026

**Documentato:** [progetto del navigatore](docs/progetto-navigatore-filesystem.md), richiesto dal titolare per un'implementazione successiva. Un unico spazio sinistro con schede **Libreria / Esplora**: il pannello attuale conserva corpus, selezione rapida e filtri; il nuovo aggiunge albero di file/cartelle, preferiti, recenti e navigazione. Lo switch conserva contesto fotografico e stato di ciascuna scheda. Specificati moduli, scansioni limitate indipendenti dal writer, identità delle richieste, salvataggi, migrazioni, casi filesystem, budget, fasi N0–N7 e matrice di accettazione.

**Verificato in questa sessione:** corrispondenza del piano con i punti d'ingresso del codice mediante lettura, collegamenti documentali e sincronizzazione. **Aperto e mancante:** intera implementazione e tutte le prove applicative/nativamente misurate del navigatore. Nessuna modifica a codice, bundle, database o originali; nessun gate R0–R4 chiuso e nessun cambiamento di priorità alle qualifiche già aperte.

### Immagini grandi, pressione e RAW Mac — verifiche del 15 settembre 2026

**Implementato:** harness opt-in su input esterni isolati, registrazione di livello residente/coordinate native/crediti, pressione della sola policy renderer e ritagli RAW lineari privati. La campagna iniziale ha scoperto un crash dei tre motori LibRaw nel probe XPC (`Thread stack size exceeded`): i quattro oggetti nativi del bridge sono ora sullo heap con proprietà RAII, senza cambiare ricette o isolamento.

**Verificato:** 24 processi e **240 azioni su PNG 12/24/45 MP**, Standard/Full e CPU/GPU con fallback dichiarati; copertura completa della traccia, 24 controlli 1:1 esatti. Le riaperture distinguono hit e nuovo decode dei Full non persistibili. Modifica, rimozione e ripristino di una sorgente da 45 MP passati con annotazione conservata. Due tracce di pressione renderer a 1536 MiB: riserva da uno a zero frame, copertura transitoria minima 3,14%, risultati finali corretti; picco footprint 1.594.870.688 byte, entro quota. A 512 MiB il RAW è rifiutato esplicitamente, originali invariati e zero crediti di lavoro dopo shutdown.

**RAW Mac:** 30 NEF D750 autorizzati × quattro motori, **120 sviluppi completi passati**, originali invariati. Altre 40 azioni nel viewer sui quattro motori con 1:1 esatto, cinque stadi del cambio motore nella UI e confronto pixel passati. Nove ritagli centrali dei primi tre NEF allineati; le differenze tonali includono ricette/esposizione/WB e non stabiliscono fedeltà assoluta. I 32 Bayer sintetici confermano 24 casi con MSE inferiore del direzionale, quattro rampe sostanzialmente equivalenti e quattro trame a crominanze indipendenti peggiori.

**Aperto:** nei quattro casi 45 MP Full, a 4 GiB configurati, la somma RSS supera quota: massimo **4.816.601.088 byte**, footprint **4.296.512.936 byte**. Crediti di ammissione entro quota; le somme possono contare pagine condivise più volte. Il superamento è conservato come evidenza negativa e il gate memoria fisica resta aperto. Non sono qualificati pressione fisica OS, decode RAW ridotto/regionale, driver, altre fotocamere, target colore/ΔE00/ICC o p95/p99. Standard riduce il livello residente dopo sviluppo completo; nessun gate R0–R4 chiuso.

Passati fmt/Clippy workspace, build release, **75 test Rust** (46 desktop, 24 worker, 5 renderer), regressione Python dell’allineamento, firma e XPC; nove test ignorati in questa corsa. Il test TIFF nativo passa fuori dal sandbox aggiuntivo del terminale. Aggiornato `dist/TrueRenderer-restyle.app`, precedente in `var/large-pressure-raw/before.app`. README e protocollo aggiornati; fotografie e ritagli restano privati. [Rapporto con hash](reports/large-pressure-raw-macos.json), [protocollo](STATO.md#campagne-raw-concluse).

### Prosecuzione — cache e verifica Mac, 15 settembre 2026

**Copertura Fit implementata e verificata:** il presenter conserva facoltativamente un frame più ampio della stessa vista, foto, digest, motore e dimensioni sorgente, usando quello con la maggiore sovrapposizione corretta durante il ricalcolo. Il caricamento Full da SSD può creare provider distinti ma compatibili: una prima implementazione troppo restrittiva è fallita su questo caso; la regressione ora lo include. Massimo due frame facoltativi entro un quarto dei budget globale/GPU e 64 frame totali; lease originali mantenuti fino al completamento. Pressione, quote ridotte, invalidazione, cambio compute e ammissione renderer/decoder possono scartare la riserva.

Release finale CPU/GPU e Standard/Full: **24 processi, 240 azioni, 1.553 ridisegni delle transizioni con copertura completa**, contro il minimo storico 40,5%. Pixel finali entro un livello sRGB8, **24 controlli 1:1 esatti**, zero fallback. Picco della riserva osservata: un frame, 38.263.752 byte di crediti globali e 12.754.584 GPU; sono contabilità di ammissione, non RSS. Passati fmt/Clippy workspace, build release, 46 test desktop e 5 renderer; sei integrazioni desktop non ripetute. Passate prove native di modifica sorgente, recupero grafico singolo e arresto alla seconda perdita, firma e XPC. Aggiornato `dist/TrueRenderer-restyle.app`, precedente in `var/fit-coverage/before.app`. [Rapporto e hash](reports/fit-coverage-macos.json).

La copertura misurata riguarda i draw della traccia sintetica, non tutti i refresh fisici. Se manca un frame ampio compatibile o viene espulso per quota, può restare parziale. RAW ridotti, pressione fisica, altri OS e p95/p99 restano aperti; nessun gate R0–R4 chiuso.

**Prima campagna delle transizioni (prima della correzione Fit):** il probe aggiunge zoom, pan, Fit, override Standard/Full e 1:1. Il presenter espone durante le sole catture diagnostiche la copertura dei comandi immagine: esatta, riproiettata o assente. Release CPU/GPU e Standard/Full: **24 processi, 240 azioni, 24 controlli 1:1 esatti**, pixel finali entro un livello sRGB8. Osservati **1.568 ridisegni** delle transizioni, di cui 1.007 riproiettati, senza draw completamente vuoti. Il ritorno dal ritaglio a Fit scende però al **40,5% di copertura** durante il ricalcolo: il resto attende contenuto corretto. L'incremento misura questo limite, senza modificare il comportamento di rendering né attribuire i draw a tutti i refresh del monitor. [Rapporto e campioni](reports/navigation-transitions-macos.json).

Passati fmt/Clippy workspace, build debug/release, 46 test desktop e 2 renderer; la nuova regressione distingue esatto, sovrapposizione parziale, assenza e lane di revisione diversa. Sei integrazioni desktop ignorate in questa corsa; XPC e firma verificati sul bundle diagnostico separato `var/navigation-transitions/TrueRenderer-release.app`. Il limite di copertura osservato ha motivato la correzione Fit descritta sopra. RAW ridotti, pressione, mouse/rotella OS, baseline e prestazioni statistiche restano da qualificare.

**Primo harness comando→superficie verificato:** aggiunto un probe opt-in al viewer di produzione e un orchestratore su corpus generato. Misura dal comando di selezione al raster esatto aggiunto a egui e alla ricezione del readback; il confronto numerico avviene dopo il timestamp finale. Configurazioni isolate, prefetch/ispettore/filmstrip esclusi e nessuna richiesta immagine prima del comando. CPU/GPU della stessa qualità, cache applicativa fredda, SSD dopo nuovo processo e ritorno RAM sono scenari distinti, confermati da contatori e residenza.

Release Standard e Full: tre ripetizioni per modalità, **24 processi e 72 selezioni** complessivi, tutti passati entro un livello sRGB8 con geometrie identiche fra CPU/GPU e nessun fallback. Passati fmt/Clippy workspace, build debug/release, 46 test desktop (6 integrazioni ignorate in questa corsa), controllo Python dei quantili/soppressione dei piccoli campioni e XPC del bundle diagnostico `var/navigation-probe/TrueRenderer-release.app`. [Rapporto e campioni](reports/navigation-surface-macos.json). I tempi includono readback e consegna dell'evento, con repaint del probe a 10 ms; non sono timestamp del monitor, p95/p99 o beneficio prestazionale qualificato. Restano scroll/zoom/cambio qualità, RAW/pressione, baseline e campagna statistica; l'app ordinaria non attiva il probe.

Riesaminati PLAN, ripresa, progetti anteprime/RAW/restyling e rispettivi gate dell'architettura. Corrette le indicazioni correnti che trattavano ancora il bundle Mac come rinviato: corpus, interfaccia e XPC sono stati verificati il 15 settembre; confronto fotografico Apple/LibRaw e qualifica colore restano aperti. Il prossimo incremento prestazionale resta l'harness evento→frame, dopo la verifica della base corrente.

Riprodotto il fallimento Unix della cache: la lettura consente hard link, ma veniva usata anche per autorizzare la rimozione di una voce invalida prima di riscriverla. La primitiva di rimozione ora riapre senza seguire link e rifiuta file con più collegamenti, sia per v1 sia per record v2 e temporanei. Letture validate e riuso v2 restano disponibili senza aggiornare timestamp condivisi. Due nuove regressioni e il test storico rafforzato verificano nomi, byte e mtime; 25 test cache passati. Allineata inoltre la compilazione del decoder bitmap portabile a Windows e test, conservando la copertura su Mac. Nessuna garanzia contro tutte le corse con processi esterni o qualifica Windows dedotta dai soli test Mac.

Verifica completa `scripts/verify.sh` passata fuori dal sandbox aggiuntivo del terminale: **106 test ordinari + 10 integrazioni**, fmt/Clippy workspace con warning negati, build debug, 2 test Python, 6 controlli IPC e 24 casi numerici con 1:1 esatto. Build release passata. Lo script ora esclude esplicitamente i due test fotografici privati quando manca `TR_RAW_SAMPLE`, evitando di contare come eseguite funzioni che ritornano subito. Generate 19 fixture locali; 17 confronti nativi CPU/esperimento Metal passati, senza abilitare Metal nel decode o qualificare prestazioni.

Aggiornato `dist/TrueRenderer-restyle.app`, copia precedente in `var/cache-integrity/before.app`. Firma ad hoc e XPC passati; cache nativa su tre sorgenti generate con 15 riusi bit-exact, zero nuovi decode a caldo, riapertura e pulizia passate. Otto schermate del viewer/griglia/confronto, 11 regioni entro un livello sRGB8, 77 canali differenti e 1:1 esatto. [Rapporto con hash](reports/cache-integrity-macos.json), evidenze locali in `var/cache-integrity/`. Restano warning del deployment target LibRaw, minimi OS, matrice fotografica e gate R0–R4; i fallimenti cache e warning Rust riportati nelle campagne storiche seguenti sono superati da questo incremento.

### Restyling desktop — 15 settembre 2026

README rivisto dopo la consegna: screenshot del viewer in apertura e schermate aggiornate di griglia e preferenze dalla stessa campagna del bundle finale; immagini del solo corpus generato. Riorganizzate le istruzioni su qualità globale/per foto, Settings, Esc, build e avvio della copia separata con il limite di una sola istanza per libreria. Collegamenti locali e immagini verificati; aggiornamento documentale, senza nuove prove applicative.

Ripresa della sessione completata: recuperate le evidenze della revisione della barra e corretto un conflitto di tastiera individuato nella prova diretta: Esc chiudeva il selettore ma riportava anche il viewer alla griglia. I popup ora hanno precedenza sulle scorciatoie fotografiche; regressione fallita prima della correzione e passata dopo, confermata anche nel bundle finale. Ripetuti fmt, Clippy desktop, build release workspace, sette test UI, suite desktop (42 passati, stesso fallimento cache, 6 ignorati), 12 schermate delle preferenze e 8 di rendering: 11 regioni entro 1 livello sRGB8, 77 canali differenti e 1:1 esatto. Firma e XPC passati sull'hash finale; completati rapporto e immagini referenziate dal README. Evidenze in `var/toolbar/reports/`, versione prima della correzione Esc recuperabile in `var/toolbar/before-escape-fix.app`. Copia aggiornata aperta sul viewer del corpus generato.

Seconda revisione richiesta dal titolare: Open e Settings coerenti e allineati, indicatore discreto del motore RAW applicato, strumenti viewer nella barra superiore e cartella/conteggi nella barra inferiore, senza duplicati centrali. Selettore globale distinto dal selettore Standard/Piena della foto corrente, con ritorno alla qualità globale e mantenimento della regola di azzeramento degli override al cambio globale. Riattivare Settings non scarta la bozza già aperta; il selettore globale non sovrascrive altre preferenze in bozza. Sette regressioni UI passate, inclusi clic su Open/Menu/Settings, layout bilingue a tre dimensioni e qualità bidirezionale; suite desktop a 42 passati, 1 fallimento cache già noto, 6 ignorati. Nuovi screenshot e controlli del bundle nel [rapporto della revisione](reports/toolbar-macos.json). Aggiornato lo stesso `dist/TrueRenderer-restyle.app`, con prima iterazione recuperabile in `var/restyle/first-iteration.app`; l'app storica `dist/TrueRenderer.app` resta invariata.

Implementati stile neutro centralizzato, barra Apri/viste/ricerca/menu, navigazione compatta, celle più leggere con nomi completi ed ellissi, ispettore a sezioni richiudibili e anteprima duplicata inizialmente chiusa nel viewer. Preferenze separate in Generale, Anteprime e RAW, Prestazioni, Cache e dati; corpo scorrevole e azioni di applicazione/chiusura fuori dallo scorrimento. Barra compatta e ispettore flottante nello spazio ridotto, senza sovrapporre l'ispettore alle preferenze; badge Anteprima permanente, stati vuoti con azioni, indicazione dell'override Piena per foto. Traduzioni e README aggiornati. [Composizioni e criteri](STATO.md#restyling-concluso).

Verificati fmt, Clippy desktop con warning negati, quattro regressioni UI (incluse bozze/selezione/zoom e quattro schede × due lingue × tre dimensioni), 4 test app/renderer, 6 integrazioni desktop con worker e 2 test Python. Suite desktop ordinaria: 39 passati, 6 ignorati, stesso fallimento cache Unix sugli hard link già riprodotto sul commit base nella campagna di localizzazione. Build release e firma ad hoc passate; prove native delle preferenze, campionamento e XPC registrate nel [rapporto con hash](reports/restyle-macos.json). Artefatti completi in `var/restyle/`; solo schermate del corpus generato pubblicabili nel README. Copia pronta in `dist/TrueRenderer-restyle.app`; bundle precedente, launcher, database e fotografie dell'utente non sostituiti.

Restano aperti il rilievo cache, warning worker Mac e warning di deployment target degli oggetti LibRaw (27.0 contro link 13.0), la matrice RAW Mac estesa, UI Windows, minimi OS, VoiceOver/NVDA e qualifica completa di accessibilità/display. Il test nativo 200% è un controllo del layout, non la chiusura di quei gate. Nessun gate R0–R4 chiuso, nessuna notarizzazione o garanzia di release attribuita al bundle.

### Interfaccia bilingue — incremento locale sul Mac

Implementati catalogo inglese/italiano del frontend, inglese predefinito anche per impostazioni precedenti senza lingua, menu Language/Lingua con applicazione immediata e persistenza. Il cambio salva soltanto la lingua e conserva selezione, zoom, generazione delle anteprime e preferenze ancora in bozza. Tradotti controlli, preferenze, guida, stato e descrizioni di provenienza; nomi file, percorsi, parole chiave e identificatori tecnici restano invariati. I dialoghi nativi seguono il sistema e dettagli diagnostici esterni possono mantenere la lingua originale. README aggiornato con istruzioni in entrambe le lingue.

Verificati build desktop/worker, fmt e Clippy desktop con warning negati, cinque nuove regressioni (default/migrazione, messaggi e percorsi, template, rendering bilingue, cambio lingua/persistenza/fallimento di scrittura). Suite desktop: 39 passati, 6 ignorati e un fallimento del test Unix sugli hard link della cache, riprodotto anche sul commit base `666f650` in una copia separata. Smoke grafico delle preferenze passato in inglese e italiano su corpus/dati temporanei; schermate controllate e conservate localmente in `var/localization/`, escluse da Git. [Rapporto e hash](reports/localization-macos.json). Corretto `sync-docs.py` per Python 3.9 del Mac, preservando LF e blocchi esterni. Restano aperti il rilievo cache, la prova UI Windows e la qualifica del nuovo bundle/XPC; il bundle esistente non è sostituito. Nessun gate R0–R4 chiuso.

### Implementato e verificato nelle campagne Windows

- LPAC senza capacità e Job Object; controlli filesystem e limiti descritti in [ADR 0009](docs/isolamento-decoder-e-formati.md#adr-0009).
- Bilineare, AHD e TrueRenderer fp32; campagne D750/D40, selezione durante scansione e cache per ricetta. [Progetto RAW](docs/progetto-motori-raw.md), [D40](STATO.md#campagne-raw-concluse).
- Correzioni iniziali di parsing, quote, bitmap, sorgenti, dipendenze e runtime: [audit prima di main](STATO.md#revisioni-windows-concluse).
- Selezione/zoom con Applica e salva, PNG cICP e alpha TIFF: [revisione serale e correzioni](STATO.md#revisioni-windows-concluse).
- Tag TIFF non supportati, fallback RAW coerente e timestamp/hard link cache: [ricontrollo e correzioni](STATO.md#revisioni-windows-concluse).
- Gamma PNG distinta da sRGB e orientamento per IFD: [ultima revisione](STATO.md#revisioni-windows-concluse); ricetta cache `bitmap-gamma-ifd-v5`.

I conteggi e gli hash di ogni campagna restano nei [rapporti di verifica](STATO.md#registro-delle-verifiche-e-degli-incrementi). Le evidenze negative sono conservate insieme alle correzioni, senza attribuire vecchie misure ai nuovi binari.
