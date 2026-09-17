# Motori RAW selezionabili — progetto sperimentale

**Stato corrente, 15 settembre 2026:** incremento Windows pubblicato in `67146e3`, comprese le correzioni successive; 107 test Rust nella campagna [gamma/orientamento](revisione-aggiuntiva-2026-09-14.md#correzioni). Il bundle Mac con i motori collegati è stato costruito e verificato per UI/corpus/XPC nel [restyling](../reports/toolbar-macos.json). La campagna Mac D750 è ora verificata sotto; restano altre fotocamere, confronto fotografico esteso e qualifica colore. Le misure del 12–13 settembre restano attribuite alle rispettive campagne.

Progetto avviato il 12 settembre 2026 su richiesta del titolare: motore proprio affiancato agli esistenti, selezionabile nelle impostazioni Windows/macOS. Anticipa un esperimento prima post-v1 (§7.3); non cambia i gate o la promessa della v1.

## Campagna Mac del 15 settembre

Verificati 30 NEF D750 sui quattro motori nel bundle XPC: **120 sviluppi completi passati**, hash degli originali invariati. La prima prova ha scoperto un esaurimento dello stack nel probe LibRaw; gli oggetti del bridge sono ora sullo heap con gestione automatica della durata. Nove confronti centrali dei primi tre NEF risultano registrati, con geometrie Apple 6016×4016 e LibRaw/TrueRenderer 6032×4032 (o orientate). Le differenze tonali misurate includono esposizione, WB e scelte della ricetta; non sono un punteggio di fedeltà del motore.

Ripetuti 32 Bayer sintetici: il metodo direzionale riduce MSE in 24 casi, quattro rampe sostanzialmente equivalenti e quattro trame a crominanze indipendenti peggiori. Restano target fotografato con riferimento noto, ΔE00/ICC, altre camere e corpus ampio di rumore/moire. [Protocollo e limiti](verifica-grandi-raw-macos.md). I paragrafi seguenti descrivono le campagne storiche quando indicano Mac come rinviato.

## Contratto e progetto

**Correzioni del 13 settembre:** i rilievi della [revisione prima di main](revisione-pre-main.md) sono stati corretti e verificati su Windows: anche i TIFF ordinari con miniatura vengono decodificati dall'immagine principale. Passati 89 test Rust, build debug/release, 156 sviluppi D750/D40 e cambio motore nella GUI; [rapporto](../reports/pre-main-fixes-windows.json). Il contratto resta limitato al perimetro provato. Il titolare ha rinviato Mac/XPC; il codice resta sperimentale e la promozione integrale richiede quella verifica e i restanti controlli di distribuzione.

- Default conservato: Apple CIRAWFilter su macOS, LibRaw bilineare su Windows.
- Motori espliciti: Apple (solo macOS), LibRaw bilineare storico, LibRaw AHD, TrueRenderer direzionale fp32 sperimentale.
- Il motore viaggia nella richiesta IPC e nell'identità delle anteprime. Probe e sviluppo ricevono lo stesso valore. La cache distingue motore, ricetta/versione e piattaforma; le vecchie voci non vengono attribuite al nuovo motore.
- Applica e salva rende effettiva la scelta senza riavvio: invalida la generazione, RAM/presentazione e richieste precedenti, conservando selezione, annotazioni e originali. I risultati tardivi non sono consegnati come nuova ricetta.
- La scelta riguarda il RAW; il percorso bitmap della piattaforma resta disponibile. Una ricetta sperimentale non supportata dà un errore esplicito, senza sostituzione con un altro motore o JPEG.

## Motore sperimentale

LibRaw **0.22.2**, aggiornata dopo l'audit del 13 settembre dalla precedente 0.21.1, identifica e decomprime il mosaico. Il contratto reale comprende Nikon D750 e D40 Bayer a tre colori; DNG Bayer sintetici TrueRenderer servono alla verifica. La D40 è stata aggiunta nella [revisione successiva](verifica-motori-d40.md): 22 NEF, 66 sviluppi completi passati sui tre motori, originali invariati. D40X, DNG convertiti, X-Trans, sRAW, pixel shift e altre fotocamere non sono ammessi implicitamente. Tutte le ricette includono la versione LibRaw; cambia anche l'impronta della cache legacy Windows, per evitare riuso di pixel della vecchia dipendenza.

Implementazione propria in Rust: interpolazione del verde secondo gradienti orizzontali/verticali con correzione della seconda differenza del colore campionato; interpolazione delle differenze R−G e B−G. Bordi riflessi con parità CFA conservata. Nessun sharpening, denoise, recupero ricostruttivo delle alte luci o curva creativa. Non si dichiara un algoritmo nuovo nella letteratura.

Il confine nativo consegna mosaico, CFA, livelli di nero/bianco, WB as-shot e matrice camera→sRGB lineare di LibRaw. Il nuovo percorso normalizza e sviluppa in fp32, converte subito in Rec.2020 senza raster RGB intero intermedio e senza clamp RGB. WB mancante o calibrazione non rappresentabile sono rifiutati. La calibrazione resta quella interpretata da LibRaw, non un profilo misurato da TrueRenderer; eventuali tagli già avvenuti nella decompressione non sono recuperabili.

LibRaw AHD è un termine di confronto indipendente: ricetta 16 bit Rec.2020, gamma lineare, WB obbligatorio, highlight unclip, auto-bright e auto-adjust maximum disattivati. Il suo confine intero resta dichiarato: non equivale a float senza limiti.

## Verifiche richieste

- Bayer sintetici con verità RGB nota: campi uniformi, rampe, bordi, trame, tutte le quattro fasi CFA, bordi/rotazioni e valori negativi/sopra uno.
- Errore numerico e artefatti rispetto al bilineare; risultati favorevoli solo sul sottoinsieme misurato. AHD e Apple non sono verità assolute del RAW.
- Regressioni di preferenze, IPC, cache e cambio motore, incluso lavoro in corso.
- Build/fmt/Clippy/test Windows e prova sui RAW autorizzati se disponibili; rendicontazione distinta per foto a piena risoluzione e segnali sintetici.
- macOS: build reale e suite XPC sul bundle obbligatorie prima della qualifica. Da questa macchina Windows non sono sostituibili con una compilazione simulata.
- Restano aperti: ΔE00 su target fotografato con illuminante noto, confronto Apple sugli stessi file, rumore/moire su corpus ampio, display ICC, prestazioni e memoria su due OS. Nessun badge Standard/Riferimento o gate R0–R4 chiuso.

## Stato

- [x] Istruzioni e architettura lette; progetto e confini definiti.
- [x] Selettore persistente, protocollo e cache collegati.
- [x] Estrazione nativa e demosaicing fp32 implementati.
- [x] AHD collegato alle configurazioni di build Windows e macOS; prove fotografiche Windows, build/corpus/XPC Mac verificati il 15 settembre.
- [x] Verifiche numeriche e 90 sviluppi completi Windows; report riproducibile.
- [x] Revisione successiva, supporto D40 verificato (66 sviluppi), correzione del cambio motore durante scansione e regressione D750.
- [x] Verifica funzionale macOS/XPC sui 30 D750, transizioni RAW e cambio motore UI; nove confronti centrali registrati, senza verità colore assoluta.
- [ ] Qualifica colore/display, altre camere e confronto esteso di rumore/moire.

Stato effettivo, prove e prossima attività sono aggiornati soltanto in `STATO.md` durante l'esecuzione.

## Risultati locali del 12 settembre

30 NEF D750 × 3 motori Windows, tutti sviluppati nel worker confinato a 6032×4032 (o equivalente orientato): **90/90 passati, SHA-256 degli originali invariati**. Il percorso Apple storico riportava 6016×4016: occorre registrare/allineare i ritagli prima di confrontare pixel e dettagli. Non è un semplice cambio di demosaicing a parità di ogni altro parametro: anche normalizzazione, WB e gestione delle alte luci differiscono tra le ricette.

| Motore | Mediana sviluppo completo, debug | Intervallo RGB lineare osservato |
|---|---:|---:|
| LibRaw bilineare | 2,67 s | 0…1 |
| LibRaw AHD | 4,61 s | 0…1 |
| TrueRenderer fp32 | 6,10 s | −0,188…2,298 |

Sono tempi sequenziali di sviluppo/trasferimento, con cache OS non svuotata e altri controlli svolti sulla macchina; non misure evento→frame, release, p95 o benchmark A/B del solo demosaic. I valori fuori [0,1] sono conservati nel buffer di lavoro; l'uscita SDR può ancora tagliarli. Non equivalgono a ricostruzione delle alte luci sature nel sensore.

Il confronto analitico proprio copre otto scene × quattro fasi Bayer: 24 casi hanno MSE minore del bilineare matematico; quattro rampe sono sostanzialmente equivalenti (errore numerico); quattro trame a crominanze indipendenti sono **peggiori** (MSE medio 0,003209 contro 0,0009054). Il metodo sfrutta la correlazione tra canali e può introdurre falsi colori dove manca. È un candidato sperimentale utile, non una superiorità universale né una misura di fedeltà rispetto alla scena fotografata.

Test nativi DNG verificano nero/bianco/WB, valori sotto zero e sopra uno, orientamenti e rifiuto di WB/modelli non supportati senza cambio di motore. I test Rust coprono inoltre fasi CFA, campioni noti, bordi pari/dispari, preferenze precedenti, serializzazione IPC, separazione della cache e consumatori di motori diversi.

Report dettagliati e ritagli fotografici restano in `var/raw-engine-evaluation-1789220057/`; confronto sintetico in `var/demosaic-comparison-windows.json`; prova finestra in `var/raw-engine-ui.json`. La prova GUI usa una copia privata, con catalogo separato: selezione invariata e livelli Adatta riusati da disco al ritorno al bilineare senza nuovi sviluppi. La persistenza di un raster completo è verificata con fixture piccole: il writer preesistente ha un'ammissione di 64 MiB e non promette la cache del raster fp32 completo da 24 MP. Non confondere un ritorno al motore precedente con un cache hit a piena risoluzione.

82 test Rust (75 ordinari + 7 integrazioni esplicite), fmt/Clippy/build, due regressioni Python, sei controlli IPC e 24 segnali di ricampionamento passati su Windows. L'harness grafico acquisisce una sola schermata per motore, dopo la presentazione della sorgente corrente, e azzera l'esito all'avvio. Il confronto con CPU ha rilevato un secondo dithering egui, disattivato nell'applicazione perché l'immagine è già sRGB8: massimo errore della superficie passato da due a un livello su 255, senza allentare la soglia. Quattro regioni, 558.144 pixel verificati; esclusi finestra impostazioni, compositor e monitor. Riferimenti CPU, screenshot e prima prova negativa restano privati. Il nuovo comportamento della superficie richiede regressione anche sul Mac.

## Riproduzione e prosecuzione

Da shell di sviluppo usare `scripts/cargo-local.sh` per fmt, Clippy, build e test. Su Windows serve MSVC; LibRaw è compilato dai sorgenti vendorizzati. `target/debug/truerenderer.exe --verify-raw-engines <cartella> [--raw-limit N]` controlla gli originali in sola lettura e salva report privati; `cargo-local.sh run -p tr-core --example compare-demosaic --locked --offline -- var/demosaic-comparison-windows.json` riproduce i segnali sintetici. `--raw-engines-smoke --open <copia-privata> --data <catalogo-di-prova>` prova il cambio nella UI senza utilizzare il catalogo personale; dopo la chiusura, `--verify-raw-engine-screenshots` confronta i pixel visibili con CPU e scrive `var/raw-engine-ui-pixels.json`.

Su Mac: build nativa con Apple e LibRaw, creazione del bundle con `scripts/package-macos.py`, quindi `scripts/test-xpc-integration.py --bundle dist/TrueRenderer.app` e le stesse prove tramite il binario del bundle. Il collegamento dei due servizi include adesso libc++; nessun nuovo bundle Mac è stato costruito qui. Il tentativo della suite XPC su Windows non può procedere senza `/usr/bin/codesign` e non conta come verifica passata.

Prima di promuovere il motore: confronto allineato con AHD/Apple, esposizione/WB controllati e target con riferimento misurato; poi trame a colori indipendenti, moiré e rumore reale, altre fotocamere e profili. I principi di interpolazione direzionale del verde e delle differenze di colore sono noti in letteratura ([descrizione Hamilton–Adams in IPOL](https://www.ipol.im/pub/art/2011/bcms-ssdd/article.pdf)); il codice Rust è un'implementazione propria. La calibrazione usa i campi documentati da [LibRaw](https://www.libraw.org/docs/API-datastruct-eng.html).
