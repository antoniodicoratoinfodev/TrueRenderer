# ADR 0005 — cache lossless e temporanei accanto alle immagini

Data: 7 settembre 2026. Stato: 0.1.4 verificata; primo incremento pubblicato (`2dad0b2`), ottimizzazioni successive qualificate e pubblicate (`a901942`).

## Scopo autorizzato

Il titolare richiede una cartella di cache/temporanei dentro ogni cartella aperta, impostazioni di limite e pulizia, README inglese e pubblicazione su GitHub prima di ulteriori ottimizzazioni. Questa richiesta anticipa esplicitamente la cache accanto alle foto che §11.4 collocava nel post-v1. Le fotografie rimangono in sola lettura; si scrive esclusivamente nella sottocartella derivata `.truerenderer-cache`. Annotazioni, backup e preferenze rimangono nel percorso dati dell'app.

## Formato e identità

`entries/<sha256>.tvc` conserva un header JSON bounded, tutti i livelli già calcolati della piramide RGBA fp32 little-endian, l'istogramma e un checksum SHA-256 dell'header e dei campioni. È lossless rispetto ai bit prodotti dal decoder/piramide corrente. Non introduce compressione JPEG, quantizzazione fp16 né un nuovo filtro. Il working space è Rec.2020 lineare con alpha premoltiplicata, come il percorso in RAM. Il formato è un prototipo full-frame, non il container tiled/gigapixel v1.

La chiave viene derivata da una tupla JSON versionata con digest completo della sorgente, versione app, versione del filtro, build OS, working space e ricetta RAW. Profilo incorporato e orientamento sono coperti dai byte sorgente; la ricetta corrente è fissa. L'apertura verifica nuovamente SHA-256 prima di riusare il disco. Non usa la sola stat come identità del contenuto. Il token stat dell'indice resta una osservazione best-effort sotto writer concorrente: non viene dichiarata una revisione sorgente coerente v1.

I file cache non sono considerati fidati: magic, header massimo 64 KiB, numero/dimensioni dei livelli, lunghezza esatta, campioni finiti, alpha, istogramma e checksum vengono verificati prima dell'uso. Un errore è un miss e attiva il decoder normale. Il worker su pipe continua ad ammettere solo il corpus; una cache non abilita gli esterni fuori dal bundle XPC. Un hit disco ha provenienza distinta `Cache disco · fp32 · Anteprima`; non abilita Standard/Riferimento. Il checksum rileva corruzioni, non autentica dati contro un attaccante che possa riscrivere interamente la cache.

## File, quote e pulizia

- `.truerenderer-cache/OWNER` identifica la directory eliminabile del progetto. Una directory omonima non riconosciuta viene lasciata intatta e la cache non è abilitata lì.
- `entries/` contiene solo artefatti conclusi; `tmp/` contiene scritture parziali con nome derivato dalla chiave. Sono esclusi da Git anche dentro il corpus.
- Directory e file vengono aperti senza seguire link. Su macOS si usano descrittori di directory e operazioni relative (`openat`, `unlinkat`, `renameatx_np` con `RENAME_EXCL`); la pulizia non percorre ricorsivamente percorsi arbitrari. File speciali e hard link vengono rifiutati.
- I lettori prendono un lock condiviso; scritture e pulizia prendono un lock esclusivo non bloccante. La cache occupata può essere saltata mantenendo l'immagine disponibile. Un reader attivo non può essere rimosso dalla manutenzione dell'app. Un filesystem che non offre le primitive richieste conserva il fallback in RAM.
- Prima di una scrittura si controlla la sua dimensione esatta, si puliscono temporanei abbandonati, si eliminano scaduti e meno usati, e si riserva nella quota il posto per il temporaneo. La pubblicazione avviene soltanto dopo scrittura completa/checksum e sincronizzazione del file; il rename esclude sovrascritture inattese. Un'interruzione lascia un temporaneo eliminabile, mai un file parziale ammesso come hit.
- Default: 4.096 MiB per cartella, 2.048 MiB massimi per artefatto temporaneo, 30 giorni senza utilizzo, 512 MiB liberi sul volume. Temporanei e cache conclusa condividono la quota per cartella. Il controllo dello spazio libero non prenota blocchi contro applicazioni esterne.
- I limiti sono per cartella, non una quota aggregata su tutte le cartelle già visitate. Le nuove impostazioni sono applicate alla cartella corrente e alle altre alla riapertura; non esiste uno scanner globale dei dischi.
- Disabilitare la cache evita nuovi hit/scritture e non elimina i dati esistenti; il comando di svuotamento elimina solo i derivati riconosciuti della cartella corrente. La RAM già pronta rimane disponibile. Preferenze salvate atomicamente in `var/settings.json`.

Errori di quota, lettura/scrittura, readonly, lock o filesystem non bloccano il salvataggio delle annotazioni: la manutenzione manuale gira fuori dal writer SQLite e dalla UI. La manutenzione automatica all'apertura gira in background. Il backend persistente è verificato su macOS; Windows è ancora da implementare/qualificare.

## Prestazioni e limiti

L'hit evita decoder e ricostruzione della piramide/istogramma, ma deve leggere e validare i campioni. Il fp32 non compresso può essere molto più grande della fotografia compressa. Il prototipo mantiene i limiti sorgente (64 Mi pixel), RAM/cache di ADR 0004 e renderer fisico di ADR 0003; non chiude il budget globale o «mai viewport vuoto».

Il primo incremento è stato pubblicato con commit `2dad0b2` prima di queste ottimizzazioni. Il campionamento CPU di una finestra del benchmark (3 s richiesti, intervallo 1 ms) mostra SHA-256 in cima allo stack per 1.737 dei 2.153 campioni del thread di verifica. Non è una percentuale dell'uso complessivo dell'app.

Abilitata la feature `asm` di `sha2 0.10.9`: su aarch64 seleziona le istruzioni SHA-256 dopo controllo delle capacità CPU e mantiene il fallback software. La nuova dipendenza opzionale `sha2-asm 0.6.4` è bloccata in Cargo.lock e la sua licenza MIT è conservata nell'inventario. Il vantaggio riguarda gli hash della cache e delle sorgenti, senza cambiare algoritmo, chiavi, checksum, fp32 o filtri.

Il broker ora prepara un `SourceSnapshot` con byte/digest privati e non modificabili dai chiamanti: lookup e decodifica usano la stessa copia, eliminando la seconda lettura/hash sul miss. Le verifiche di digest/token e ammissibilità sono centralizzate nel broker; il percorso decoder ricontrolla la policy del proprio trasporto anche per uno snapshot proveniente da un altro broker. Questo riuso non risolve la revisione coerente sotto writer concorrente prevista per la v1.

Sulle stesse fixture e sullo stesso Mac, la mediana di cinque riusi scende da 0,9631 a 0,1374 s per JPEG 12 MP (7,0×), da 0,06928 a 0,00972 s per DNG (7,1×), da 0,000304 a 0,000119 s per PNG piccolo (2,6×). I caricamenti a cache applicativa fredda, scrittura inclusa, sono rispettivamente 0,6223 / 0,1727 / 0,01024 s. Il cache OS non è svuotato; non si attribuisce una quota separata di guadagno alle due modifiche. Dati prima/dopo e limiti in `reports/cache-performance-comparison.json`.

La scrittura resta nel thread di decodifica prima della consegna dell'immagine. Scrittura differita con coda bounded, cache compressa/tiled, scheduler globale e memorie sotto pressione restano ulteriori passi.

## Verifiche

Cinque test Rust mirati: round-trip bit esatti inclusi valori RGB negativi/>1 e alpha, invalidazione per contenuto/pipeline, corruzione/troncamento, cancellazione senza pubblicazione parziale, LRU/scadenza e temporanei abbandonati, quote/spazio, impostazioni persistenti, directory non riconosciute, symlink/hardlink e lock dei reader. Tre test aggiuntivi coprono vettori SHA-256 noti anche a confini di blocco, rifiuto di uno snapshot esterno su pipe e decodifica dei byte catturati dopo una modifica del file originale. Totale workspace: 40 test passati. Restano fuzzing, power-fault reale, filesystem remoti e altri OS.

`--verify-cache` usa solo tre immagini generate del progetto: JPEG 12 MP, DNG Bayer e PNG 16 bit. Una corsa con cache applicativa fredda e cinque calde; confronta ogni bit di ogni livello e l'istogramma, verifica l'assenza di nuovi job decoder nei riusi e gli originali invariati. Il cache OS non viene svuotato: non è un benchmark p95. `--settings-smoke` acquisisce il pannello delle impostazioni. Esiti effettivi in `reports/cache-macos.json`, `reports/cache-settings-macos.json` e `reports/VERIFICA.md`.
