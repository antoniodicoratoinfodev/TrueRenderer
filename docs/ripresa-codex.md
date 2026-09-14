# TrueRenderer — ripresa del lavoro

Aggiornato il 14 settembre 2026. Codice Windows/RAW e correzioni pubblicati in `67146e3`; la successiva revisione e l'accorpamento dei Markdown sono modifiche locali.

## Stato verificato

Ultima suite Windows: **99 test ordinari + 8 integrazioni**, debug/release, gamma PNG, orientamento RAW, IPC e ricampionamento. [Audit e correzioni](revisione-aggiuntiva-2026-09-14.md#correzioni), [rapporto con hash](../reports/gamma-orientation-fixes-windows.json). GUI e campagna Nikon non ripetuti per questi ultimi fix.

Il bundle macOS verificato resta quello del 9 settembre, con 71 test e 30 NEF a 2 GiB nel carico dichiarato. [Rapporto della baseline](../reports/preview-navigation-continuation-macos.json). Non attribuire queste misure alla nuova configurazione Apple/LibRaw.

## Prossime attività

- Mac/XPC: rinviato esplicitamente dal titolare. Su Mac costruire il nuovo bundle, provare entrambi gli XPC, selettore/cambio durante scansione, TIFF/PNG, cache e superficie. Usare `scripts/verify.sh --gui` e `scripts/test-xpc-integration.py` sul pacchetto nuovo.
- Windows: installazione pulita, contesa writer/cache e diagnostica PNG malformata o conflittuale.
- Qualifica: colore/display, confronto fra motori con ritagli allineati, corpus autorizzato esteso, pressione memoria e latenze evento→frame. Nessun gate R0–R4 chiuso; Standard/Piena restano qualità di anteprima, non assurance Standard/Riferimento.

Il dettaglio operativo e le caselle sono in [PLAN](../PLAN.md); la matrice requisiti/implementazione è in [avanzamento](avanzamento.md). La cronologia tecnica resta nei [rapporti di verifica](../reports/VERIFICA.md) e nelle revisioni elencate nell'[indice](README.md).

## Vincoli di lavoro

Usare `scripts/cargo-local.sh`. Eseguire verifiche pertinenti alle modifiche; dopo cambi a broker/worker/bundle aggiungere la prova nativa XPC. Non ripetere suite valide senza nuove modifiche o dubbi concreti.

Aggiornare piano e registro, poi eseguire `python3 scripts/sync-docs.py`. Conservare il backup v1.2 e il testo utente delle architetture. La specifica anteprime resta una fonte attiva finché i suoi gate non sono qualificati.

Originali in sola lettura. `var/library.sqlite` e `var/backups` sono dati durevoli; non ripulire `var/` come se fosse tutta cache. Non pubblicare fotografie, database, backup, credenziali o toolchain.
