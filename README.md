# TrueRenderer

Prototipo desktop **R0 · 0.1.0**, sviluppato dalla proposta `TrueVision-Architettura.md`. Il nome dell'app è ora **TrueRenderer**.

## Avvio sul Mac

Fare doppio clic su **`Avvia TrueRenderer.command`**, oppure aprire `dist/TrueRenderer.app`.
Al primo avvio macOS richiede l’accesso al Desktop: scegliere **Consenti** per usare corpus, annotazioni e backup nella cartella del progetto. È il consenso del sistema operativo, non un account dell’app.

Si apre il corpus di prova incluso. Il bundle deve rimanere dentro questa cartella di progetto, perché usa `corpus/` e `var/` della cartella superiore. Puoi spostare l'intera cartella `TrueRenderer`.

Il pacchetto è una build interna con firma ad hoc, non una release notarizzata. Non richiede un account né una connessione per funzionare.

## Cosa puoi già provare

- Griglia virtualizzata con miniature regolabili, selezione e ricerca per nome/parola chiave.
- Anteprima, zoom da 1% a 3200%, pan, pixel fisici **1:1** anche su Retina.
- Confronto di due immagini: centro normalizzato condiviso; zoom fisico comune quando impostato, adattamento indipendente in modalità Adatta.
- Istogramma dell'uscita sRGB composita e campionatore nel viewport.
- Rating 0–5, scarto, etichette e parole chiave; filtri e salvataggio in SQLite.
- Undo di sessione come nuova revisione; backup SQLite verificato ed export JSON senza sovrascrivere file esistenti.
- 12 immagini numeriche PNG proprie, comprese alpha, bordi dispari e gradienti a 16 bit.
- Diagnostica GPU/CPU automatica e report in `reports/`.

Le frecce cambiano foto; `G`, `E`, `C` scelgono la vista. `0`–`5` valutano, `X` scarta, `6`–`9` assegnano etichette. `Z` alterna Adatta/1:1, `Cmd+1` seleziona il pixel fisico 1:1, rotella e trascinamento controllano zoom/pan. `Cmd+Z` annulla, `Cmd+F` cerca. `I` mostra/nasconde l'ispezione, `T` la striscia, `F` lo schermo intero, `Esc` torna alla griglia. Su Windows i sorgenti usano Ctrl al posto di Cmd; il target non è ancora qualificato.

## Confini di questa versione

**Le anteprime funzionano solo sui PNG del corpus incluso**, controllati tramite digest compilati nel broker. “Apri cartella” elenca anche file JPEG/PNG/TIFF esterni ma non li decodifica. Questo applica il gate del documento: l'isolamento con XPC/App Sandbox e AppContainer deve essere verificato prima degli archivi esterni. Non esiste un interruttore per aggirarlo.

Tutti i render restano **Anteprima**. Il decoder R0 è `image/png` in un processo separato, non il backend nativo v1. Il working space è Rec.2020 lineare fp32 con alpha premoltiplicata; riduzione area CPU per le miniature, composizione opaca su grigio sRGB `#777777`, clamp/quantizzazione a sRGB 8 bit in uscita. La presentazione usa nearest, anche in Adatta: il filtro Lanczos3 qualificato e il ricampionamento esatto alla dimensione del viewport sono ancora da completare. Non si certificano display, ICC, gamut o calibrazione fisica con il test numerico GPU.

Sono ancora da implementare/qualificare: sandbox OS, profili ICC/Little CMS e monitor, backend nativi JPEG/TIFF e CMYK, EXIF/XMP completi, RAW/LibRaw, tile/gigapixel, raccolte, watcher, journal degli effetti esterni, migrazioni successive, export/restore portabile v1, accessibilità completa, Windows reale, installer e notarizzazione. La UI anticipa parti del workflow come prototipo: R1/R2 non sono conclusi. Le modifiche multi-selezione sono transazioni per immagine e l'undo opera sull'ultima singola modifica; batch atomici sono R2.

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
./scripts/verify.sh --gui        # richiede una sessione desktop macOS
./scripts/build-macos.sh         # ricrea il bundle release interno
python3 scripts/sync-docs.py     # aggiorna entrambe le architetture
```

Rust è bloccato in `rust-toolchain.toml`; tutte le versioni transitive e i checksum sono in `Cargo.lock`. Su una macchina nuova serve Rust oppure una toolchain locale equivalente e il download iniziale delle dipendenze (togliere `--offline` al primo fetch). Nel sandbox di un terminale l'avvio Cocoa/Metal può essere bloccato: eseguire il test grafico nella normale sessione desktop.

Il codice è organizzato in sei crate di servizio e un'app: `tr-core` (tipi/colore/IPC), `tr-app` (reducer), `tr-store` (SQLite), `tr-platform` (broker/I/O), `tr-worker` (decoder), `tr-render` (presentazione/diagnostica) e `apps/desktop` (composizione/UX). Le fonti primarie consultate e le deviazioni R0 sono in `docs/adr/0001-prototipo-r0.md`.

La prova nativa dal Finder e la copia indipendente di bundle/corpus sono passate il 6 settembre 2026. È disponibile anche un laboratorio XPC separato: ha verificato filesystem/rete negati, trasferimento di descrittori in sola lettura, recupero dopo crash e un limite aggiuntivo per impedire processi figli. Risultati e riproduzione in `experiments/macos-xpc/README.md`. Questa prova non è ancora integrata con il decoder e non chiude il gate della sandbox dell’app.
