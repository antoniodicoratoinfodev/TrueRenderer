# ADR 0003 — campionamento alla risoluzione fisica

Data: 6 settembre 2026. Versione app: 0.1.2. Decisione per il prototipo Anteprima; promozione Standard/Riferimento subordinata a §§10 e 19.3.

## Raccordo con la 0.1.5 — 9 settembre 2026

Le descrizioni di sorgente/piramide condivisa, quote fisse e attesa sullo sfondo documentano la 0.1.2. [ADR 0006](0006-anteprime-residenza-compute.md) mantiene il grafo del filtro ma usa livelli autonomi, budget con lease, compute GPU e riuso/riproiezione del frame durante il raffinamento. Istogramma e campionatore indicano ora il livello residente; 1:1 richiede il dettaglio nativo Full. La suite corrente confronta anche CPU/GPU e le fixture EXIF, entro il proprio perimetro; qualifica completa di transizioni, display e prestazioni ancora aperta. Risultati aggiornati in [VERIFICA](../../reports/VERIFICA.md).

## Problema osservato

Il corpus `04_Frequenze_radiali.png` mostrava falsi dettagli e moiré in Adatta e nella griglia della 0.1.1. Il viewer applicava nearest alla texture sorgente a scala arbitraria; le miniature passavano da una riduzione area a 320 pixel e poi da un ulteriore ridimensionamento nearest, anche su Retina. La miniatura laterale poteva inoltre cambiare sorgente quando arrivava la decodifica intera. Screenshot della vecchia finestra acquisito prima della modifica; nessun cambiamento al generatore o ai PNG del corpus per nascondere il problema.

## Decisione

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

## Prove e limiti

Quattro test Rust aggiunti: lettura esatta e crop con RGB negativo/oltre 1, costanti e dimensioni 1×N/N×1/dispari, alternanza bianco-nero che produce circa 188 sRGB in riduzione, alpha nascosto e rifiuto quote/NaN. Le prove CLI confrontano 24 sinusoidi con riferimenti analitici: RMS ≤0,02 oltre 1,5× Nyquist di uscita; rapporto di contrasto 0,95–1,05 sotto 0,25× Nyquist. La banda intermedia è osservata senza dichiararla superata come gate. Il corpus radiale è verificato bit-per-bit a 1:1 e salvato a cinque scale con un modello del vecchio Adatta nearest per confronto.

Lo smoke nativo esteso produce otto schermate. Il confronto dei pixel delle schermate con il raster CPU controlla griglia a tre dimensioni, preview laterale, filmstrip e viewer 1:1/Adatta/37%, con soglia di un livello per canale a 8 bit. Il metadato registra rettangolo fisico, origine, passo e dimensione di ogni regione radiale interamente visibile; non si confrontano aree intenzionalmente tagliate dal clip. `reports/sampling-presentation-macos.json` contiene i risultati effettivi, non dedotti dal solo valore Retina 2×.

Questi controlli non esauriscono §19.3. Restano alias 2D/Siemens/isotropia, overshoot ai bordi, confronto piramide/direct e CPU/GPU, transizioni temporali, orientamento EXIF, tile/cuciture, altri display/DPI e costo p95. La variante Lanczos deve essere rivista o sostituita con un'alternativa (ad esempio Kaiser con parametri fissati) se la suite estesa non rispetta le soglie. Non si clampa RGB nel working per nascondere overshoot; il solo clipping è in uscita sRGB8, come nel prototipo precedente. Nessun badge Standard/Riferimento viene abilitato.

Una riduzione non può conservare tutte le frequenze della sorgente: il filtro deve attenuare quelle non rappresentabili dai pixel disponibili. I cerchi centrali della zone plate fanno parte dell'immagine originale; l'obiettivo è evitare nuovi motivi spuri, non eliminare i cerchi o produrre ovunque grigio uniforme. Gli screenshot ingranditi/ridotti da un altro programma possono introdurre propri artefatti. Il confronto numerico riguarda la superficie dell'app: compositore, profilo monitor e pannello fisico restano prove separate.

## Riproduzione

```sh
./scripts/verify.sh
./scripts/build-macos.sh
./dist/TrueRenderer.app/Contents/MacOS/TrueRenderer --verify-resampling
open -n -W dist/TrueRenderer.app --args --sampling-smoke
./dist/TrueRenderer.app/Contents/MacOS/TrueRenderer --verify-sampling-screenshots
```

Le prove del bundle richiedono la sessione nativa macOS; la libreria dello smoke è separata in `var/smoke`. Risultati in `reports/resampling-macos.json`, `reports/sampling-presentation-macos.json` e `reports/smoke-macos.json`.

Fonti primarie per la teoria del campionamento: [PBRT, Sampling Theory](https://www.pbr-book.org/4ed/Sampling_and_Reconstruction/Sampling_Theory) e [Image Reconstruction](https://www.pbr-book.org/4ed/Sampling_and_Reconstruction/Image_Reconstruction). I coefficienti, la scelta del margine e le soglie sopra sono decisioni del progetto, non garanzie attribuite al libro.
