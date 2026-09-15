# TrueRenderer — ripresa del lavoro

Aggiornato il 15 settembre 2026. Usare [PLAN](../PLAN.md) e [avanzamento](avanzamento.md) per lo stato corrente; i rapporti identificano i binari effettivamente provati.

## Stato verificato

Ultima suite Windows: **99 test ordinari + 8 integrazioni**, debug/release, gamma PNG, orientamento RAW, IPC e ricampionamento. [Audit e correzioni](revisione-aggiuntiva-2026-09-14.md#correzioni), [rapporto con hash](../reports/gamma-orientation-fixes-windows.json). GUI e campagna Nikon non ripetuti per questi ultimi fix.

Il bundle macOS del restyling è `dist/TrueRenderer-restyle.app`: barra, qualità globale/per foto, quattro schede Settings, layout bilingue, pixel del corpus e due XPC verificati il 15 settembre. [Rapporto](../reports/toolbar-macos.json). La campagna fotografica ampia resta quella del 9 settembre, con 71 test e 30 NEF a 2 GiB nel carico dichiarato: [baseline](../reports/preview-navigation-continuation-macos.json). Non attribuire queste misure alla nuova configurazione Apple/LibRaw.

Ultimo incremento locale: copertura Fit corretta anche con provider Full distinti dalla cache, entro quota. **240 azioni, 1.553 ridisegni a copertura completa, 24 controlli 1:1 esatti**; invalidazione/recupero grafico/XPC passati. Copia aggiornata `dist/TrueRenderer-restyle.app`, backup `var/fit-coverage/before.app`. [Rapporto finale](../reports/fit-coverage-macos.json). Copertura senza riserva e livelli RAW ridotti ancora da qualificare.

Ultima consegna: **120 sviluppi D750 sui quattro motori** dopo la correzione dello stack LibRaw/XPC; 240 azioni PNG 12/24/45 MP, 20 con pressione renderer e 40 RAW passate. Verificati invalidazione 45 MP, cambio motore UI, pixel e XPC; 75 test Rust. Bundle aggiornato nello stesso percorso, backup `var/large-pressure-raw/before.app`. [Rapporto](../reports/large-pressure-raw-macos.json), [protocollo](verifica-grandi-raw-macos.md).

**Priorità aperta:** su 45 MP Full la somma RSS arriva a 4.816.601.088 byte e il footprint a 4.296.512.936, con 4 GiB configurati. Il gate memoria fisica è fallito nel perimetro dichiarato; non sostituire questa evidenza con le sole quote di ammissione. Pressione renderer iniettata e rifiuto a 512 MiB sono verificati, pressione OS reale e driver no. Il decode RAW resta full-frame; i livelli residenti ridotti non sono decode regionale.

## Prossime attività

- Base cache verificata: rilievo hard link corretto, suite Mac a 106 test ordinari + 10 integrazioni, fmt/Clippy e build debug/release passati. [Rapporto](../reports/cache-integrity-macos.json). Il rinvio Mac delle sessioni Windows è superato dalle prove del 15 settembre, non è un vincolo ancora attivo.
- Primo harness comando→superficie completato: Standard/Full, CPU/GPU e cache fredda/SSD/RAM verificati su 24 processi e 72 selezioni. [Rapporto](../reports/navigation-surface-macos.json), [protocollo](progetto-anteprime-cache-prestazioni.md#121-protocollo-delle-misure). Bundle diagnostico in `var/navigation-probe/TrueRenderer-release.app`; questo rapporto identifica il precedente bundle diagnostico.
- Transizioni completate sul corpus piccolo: 240 azioni CPU/GPU, 1:1 esatto, nessun draw completamente vuoto nei 1.568 ridisegni osservati; copertura parziale minima 40,5%. [Rapporto](../reports/navigation-transitions-macos.json), bundle diagnostico `var/navigation-transitions/TrueRenderer-release.app`.
- Dopo la correzione Fit, estendere a RAW ridotti/pressione e individuare la presentazione effettiva prima della campagna p95/p99. Il readback attuale include il costo della cattura.
- RAW Mac: i 30 D750 sui quattro motori e nove ritagli centrali allineati sono verificati; servono altre fotocamere, target colore misurato, rumore/moire e ICC. Usare originali autorizzati in sola lettura e cataloghi separati.
- Windows: installazione pulita, contesa writer/cache e diagnostica PNG malformata o conflittuale.
- Qualifica: colore/display, confronto fra motori con ritagli allineati, corpus autorizzato esteso, pressione memoria e latenze evento→frame. Nessun gate R0–R4 chiuso; Standard/Piena restano qualità di anteprima, non assurance Standard/Riferimento.

Il dettaglio operativo e le caselle sono in [PLAN](../PLAN.md); la matrice requisiti/implementazione è in [avanzamento](avanzamento.md). La cronologia tecnica resta nei [rapporti di verifica](../reports/VERIFICA.md) e nelle revisioni elencate nell'[indice](README.md).

## Vincoli di lavoro

Usare `scripts/cargo-local.sh`. Eseguire verifiche pertinenti alle modifiche; dopo cambi a broker/worker/bundle aggiungere la prova nativa XPC. Non ripetere suite valide senza nuove modifiche o dubbi concreti.

Aggiornare piano e registro, poi eseguire `python3 scripts/sync-docs.py`. Conservare il backup v1.2 e il testo utente delle architetture. La specifica anteprime resta una fonte attiva finché i suoi gate non sono qualificati.

Originali in sola lettura. `var/library.sqlite` e `var/backups` sono dati durevoli; non ripulire `var/` come se fosse tutta cache. Non pubblicare fotografie, database, backup, credenziali o toolchain.
