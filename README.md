# TrueRenderer

Prototipo desktop **R0 · 0.1.2**, sviluppato dalla proposta `TrueVision-Architettura.md`. Il nome dell'app è ora **TrueRenderer**.

## Avvio sul Mac

Fare doppio clic su **`Avvia TrueRenderer.command`**, oppure aprire `dist/TrueRenderer.app`.
Al primo avvio macOS richiede l’accesso al Desktop: scegliere **Consenti** per usare corpus, annotazioni e backup nella cartella del progetto. Con queste build ad hoc la richiesta può ripetersi dopo una ricompilazione. È il consenso del sistema operativo, non un account dell’app.

Si apre il corpus di prova incluso. Il bundle deve rimanere dentro questa cartella di progetto, perché usa `corpus/` e `var/` della cartella superiore. Puoi spostare l'intera cartella `TrueRenderer`.

Il pacchetto è una build interna con firma ad hoc, non una release notarizzata. Non richiede un account né una connessione per funzionare.

## Cosa puoi già provare

- Griglia virtualizzata con miniature regolabili, selezione e ricerca per nome/parola chiave.
- Anteprima, zoom da 1% a 3200%, pan, pixel fisici **1:1** anche su Retina.
- Confronto di due immagini: centro normalizzato condiviso; zoom fisico comune quando impostato, adattamento indipendente in modalità Adatta.
- Ricampionamento lineare alla risoluzione fisica, condiviso da griglia, viewer, confronto e anteprima laterale; corretto il moiré da nearest/doppio ridimensionamento.
- Istogramma della sorgente composta in sRGB e campionatore sorgente LOD 0 nel viewport.
- Rating 0–5, scarto, etichette e parole chiave; filtri e salvataggio in SQLite.
- Undo di sessione come nuova revisione; backup SQLite verificato ed export JSON senza sovrascrivere file esistenti.
- 12 immagini numeriche PNG proprie, comprese alpha, bordi dispari e gradienti a 16 bit.
- Diagnostica GPU/CPU automatica e report in `reports/`.
- Due decoder isolati nel bundle macOS, con controllo di timeout/memoria e salvataggio delle annotazioni indipendente dalla decodifica.

Le frecce cambiano foto; `G`, `E`, `C` scelgono la vista. `0`–`5` valutano, `X` scarta, `6`–`9` assegnano etichette. `Z` alterna Adatta/1:1, `Cmd+1` seleziona il pixel fisico 1:1, rotella e trascinamento controllano zoom/pan. `Cmd+Z` annulla, `Cmd+F` cerca. `I` mostra/nasconde l'ispezione, `T` la striscia, `F` lo schermo intero, `Esc` torna alla griglia. Su Windows i sorgenti usano Ctrl al posto di Cmd; il target non è ancora qualificato.

## Confini di questa versione

**Le anteprime funzionano solo sui PNG del corpus incluso**, controllati tramite digest compilati nel broker e nel decoder. “Apri cartella” elenca anche file JPEG/PNG/TIFF esterni ma non li decodifica. Questo applica il gate del documento: l'isolamento con XPC/App Sandbox e AppContainer deve essere qualificato completamente prima degli archivi esterni. Non esiste un interruttore per aggirarlo.

Tutti i render restano **Anteprima**. Il decoder R0 è `image/png`, riusato dai due servizi XPC del bundle macOS; i binari di sviluppo fuori dal bundle conservano il worker su pipe. È ancora da sostituire con i backend nativi v1. Il working space è Rec.2020 lineare fp32 con alpha premoltiplicata. La 0.1.2 usa una piramide CPU condivisa: riduzione opaca Lanczos3 con margine di banda versionato, ingrandimento Mitchell, alpha area/triangolo; 1:1 allineato legge direttamente LOD 0. Il raster è calcolato per i pixel fisici della vista, senza un secondo ridimensionamento nella UI. Composizione opaca su grigio sRGB `#777777`, clamp/quantizzazione solo in uscita sRGB8. Durante il calcolo asincrono può apparire brevemente lo sfondo. Non si certificano display, ICC, gamut o calibrazione fisica con il test numerico GPU. Decisioni e limiti in `docs/adr/0003-campionamento-fisico-r0.md`.

Adatta può essere una riduzione o un ingrandimento, in base a finestra e Retina. Per esempio, nello smoke di questo Mac la sorgente 1200×800 occupa 1625×1083 pixel fisici in Adatta, 1200×800 a 1:1 e 444×296 al 37%. Quando si riduce, le frequenze non rappresentabili devono perdere contrasto: conservare ogni dettaglio produrrebbe moiré. I cerchi originali della zone plate rimangono; non devono comparire nuovi motivi spuri.

Il bundle richiede XPC senza ripiego automatico. App Sandbox, limiti del processo e supervisione a 384 MiB contengono i decoder nelle prove eseguite su macOS 26.6.2 arm64. Il campionamento ogni 25 ms può superare la soglia: non è un tetto rigido. Dopo un crash il riavvio del servizio può impiegare circa 10 s. Firma di distribuzione e compatibilità dell’API libproc SPI usata per la terminazione restano gate aperti. Decisione e prove in `docs/adr/0002-xpc-decoder-r0.md`.

Sono ancora da implementare/qualificare: gate completi della sandbox OS, profili ICC/Little CMS e monitor, backend nativi JPEG/TIFF e CMYK, EXIF/XMP completi, RAW/LibRaw, tile/gigapixel, raccolte, watcher, journal degli effetti esterni, migrazioni successive, export/restore portabile v1, accessibilità completa, Windows reale, installer e notarizzazione. La UI anticipa parti del workflow come prototipo: R1/R2 non sono conclusi. Le modifiche multi-selezione sono transazioni per immagine e l'undo opera sull'ultima singola modifica; batch atomici sono R2.

Identità R0: UUID per asset distinto da percorso e digest; copie uguali non vengono unite, sostituzioni di contenuto nello stesso percorso creano un nuovo asset. Riconciliazione di rinomine, volumi e file mancanti resta R2. I file esterni elencati hanno identità osservata best-effort da stat, esplicitamente non verificata.

## Dove sono i dati

| Percorso | Contenuto |
|---|---|
| `PLAN.md` | Piano completo R0–R4, dipendenze, gate e stato dell'incremento |
| `docs/TrueRenderer-Architettura.md` | Architettura con registro dello sviluppo |
| `../TrueVision-Architettura.md` | Documento richiesto sul Desktop, aggiornato nel blocco di avanzamento |
| `docs/TrueVision-Architettura.originale-v1.2.md` | Copia intatta del documento iniziale |
| `var/library.sqlite` | Annotazioni durevoli e revisioni: **non è una cache** |
| `var/index.sqlite` | Indice ricostruibile |
| `var/backups/` | Backup consistenti verificati; in R0 sono conservati tutti |
| `var/smoke/` | Libreria separata dei test grafici |
| `reports/` | Risultati verifiche, screenshot e inventario dipendenze |
| `.tools/` | Toolchain Rust e cache Cargo locali |

I backup sullo stesso disco non coprono la perdita del disco. Il percorso dati sotto il progetto è una scelta del prototipo richiesto sul Desktop; la release userà app-data locale qualificato, fuori dalle cartelle sincronizzate. Gli originali non vengono mai scritti.

## Sviluppo e verifica

```sh
./scripts/cargo-local.sh build --workspace --locked --offline
./scripts/verify.sh
./scripts/verify.sh --gui        # otto screenshot e confronto pixel, sessione macOS
./scripts/build-macos.sh         # ricrea il bundle release interno
python3 scripts/test-xpc-integration.py  # prova i servizi XPC della build release
./dist/TrueRenderer.app/Contents/MacOS/TrueRenderer --verify-resampling
open -n -W dist/TrueRenderer.app --args --sampling-smoke
./dist/TrueRenderer.app/Contents/MacOS/TrueRenderer --verify-sampling-screenshots
python3 scripts/sync-docs.py     # aggiorna entrambe le architetture
```

Rust è bloccato in `rust-toolchain.toml`; tutte le versioni transitive e i checksum sono in `Cargo.lock`. Su una macchina nuova serve Rust oppure una toolchain locale equivalente e il download iniziale delle dipendenze (togliere `--offline` al primo fetch). Nel sandbox di un terminale l'avvio Cocoa/Metal può essere bloccato: eseguire il test grafico nella normale sessione desktop.

Il codice è organizzato in sei crate di servizio e un'app: `tr-core` (tipi/colore/IPC), `tr-app` (reducer), `tr-store` (SQLite), `tr-platform` (broker/I/O), `tr-worker` (decoder), `tr-render` (presentazione/diagnostica) e `apps/desktop` (composizione/UX). Le fonti primarie consultate e le deviazioni R0 sono in `docs/adr/0001-prototipo-r0.md`.

Le prove dell’app e del pacchetto sono registrate in `reports/VERIFICA.md`. Il laboratorio XPC separato ha verificato filesystem/rete negati, trasferimento di descrittori in sola lettura, recupero dopo crash e un limite aggiuntivo per impedire processi figli; istruzioni in `experiments/macos-xpc/README.md`. L’incremento 0.1.1 ha integrato il decoder e aggiunto prove di due processi distinti, timeout, recupero e crescita di memoria. Il gate completo della sandbox resta aperto.

Build 0.1.2 verificata: 30 test Rust, 6 prove IPC, 24 casi sinusoidali, 8 screenshot e 14 regioni con pixel identici al raster CPU; suite XPC, Finder e copia autonoma passati sul Mac di sviluppo. Il §0 dell’architettura contiene ora la matrice completa di ciò che esiste e manca rispetto ai capitoli 1–24. Il backup originale dell’architettura rimane intatto; le build precedenti sono conservate in `var/package-history/`.
