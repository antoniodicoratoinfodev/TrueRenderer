# Motori RAW selezionabili — progetto sperimentale

**Stato corrente, 15 settembre 2026:** incremento Windows pubblicato in `67146e3`, comprese le correzioni successive; 107 test Rust nella campagna [gamma/orientamento](../STATO.md#revisioni-windows-concluse). Il bundle Mac con i motori collegati è stato costruito e verificato per UI/corpus/XPC nel [restyling](../reports/toolbar-macos.json). La campagna Mac D750 è ora verificata sotto; restano altre fotocamere, confronto fotografico esteso e qualifica colore. Le misure del 12–13 settembre restano attribuite alle rispettive campagne.

Progetto avviato il 12 settembre 2026 su richiesta del titolare: motore proprio affiancato agli esistenti, selezionabile nelle impostazioni Windows/macOS. Anticipa un esperimento prima post-v1 (§7.3); non cambia i gate o la promessa della v1.

## Campagna Mac del 15 settembre

Verificati 30 NEF D750 sui quattro motori nel bundle XPC: **120 sviluppi completi passati**, hash degli originali invariati. La prima prova ha scoperto un esaurimento dello stack nel probe LibRaw; gli oggetti del bridge sono ora sullo heap con gestione automatica della durata. Nove confronti centrali dei primi tre NEF risultano registrati, con geometrie Apple 6016×4016 e LibRaw/TrueRenderer 6032×4032 (o orientate). Le differenze tonali misurate includono esposizione, WB e scelte della ricetta; non sono un punteggio di fedeltà del motore.

Ripetuti 32 Bayer sintetici: il metodo direzionale riduce MSE in 24 casi, quattro rampe sostanzialmente equivalenti e quattro trame a crominanze indipendenti peggiori. Restano target fotografato con riferimento noto, ΔE00/ICC, altre camere e corpus ampio di rumore/moire. [Protocollo e limiti](../STATO.md#campagne-raw-concluse). I paragrafi seguenti descrivono le campagne storiche quando indicano Mac come rinviato.

## Contratto e progetto

**Correzioni del 13 settembre:** i rilievi della [revisione prima di main](../STATO.md#revisioni-windows-concluse) sono stati corretti e verificati su Windows: anche i TIFF ordinari con miniatura vengono decodificati dall'immagine principale. Passati 89 test Rust, build debug/release, 156 sviluppi D750/D40 e cambio motore nella GUI; [rapporto](../reports/pre-main-fixes-windows.json). Il contratto resta limitato al perimetro provato. Il titolare ha rinviato Mac/XPC; il codice resta sperimentale e la promozione integrale richiede quella verifica e i restanti controlli di distribuzione.

- Default conservato: Apple CIRAWFilter su macOS, LibRaw bilineare su Windows.
- Motori espliciti: Apple (solo macOS), LibRaw bilineare storico, LibRaw AHD, TrueRenderer direzionale fp32 sperimentale.
- Il motore viaggia nella richiesta IPC e nell'identità delle anteprime. Probe e sviluppo ricevono lo stesso valore. La cache distingue motore, ricetta/versione e piattaforma; le vecchie voci non vengono attribuite al nuovo motore.
- Applica e salva rende effettiva la scelta senza riavvio: invalida la generazione, RAM/presentazione e richieste precedenti, conservando selezione, annotazioni e originali. I risultati tardivi non sono consegnati come nuova ricetta.
- La scelta riguarda il RAW; il percorso bitmap della piattaforma resta disponibile. Una ricetta sperimentale non supportata dà un errore esplicito, senza sostituzione con un altro motore o JPEG.

## Motore sperimentale

LibRaw **0.22.2**, aggiornata dopo l'audit del 13 settembre dalla precedente 0.21.1, identifica e decomprime il mosaico. Il contratto reale comprende Nikon D750 e D40 Bayer a tre colori; DNG Bayer sintetici TrueRenderer servono alla verifica. La D40 è stata aggiunta nella [revisione successiva](../STATO.md#campagne-raw-concluse): 22 NEF, 66 sviluppi completi passati sui tre motori, originali invariati. D40X, DNG convertiti, X-Trans, sRAW, pixel shift e altre fotocamere non sono ammessi implicitamente. Tutte le ricette includono la versione LibRaw; cambia anche l'impronta della cache legacy Windows, per evitare riuso di pixel della vecchia dipendenza.

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

<a id="adr-0008"></a>

## ADR 0008 — LibRaw: licenza, collegamento e binding

Data: 10 settembre 2026. Stato: decisione presa e integrazione eseguita su Windows; qualifica R3 aperta.

**Aggiornamento del 13 settembre:** l'audit ha richiesto l'aggiornamento a LibRaw **0.22.2**, dal tag ufficiale, conservando CDDL-1.0 e gli avvisi originali. Il manifest `third_party/libraw/manifest-truerenderer.json` registra archivio, hash dei 106 file upstream e le 79 unità compilate; `scripts/verify-libraw.py` verifica copia, opzioni e ricette. Attivato anche `LIBRAW_CALLOC_RAWSTORE`, come raccomandato upstream; nessun sorgente upstream modificato o pack GPL aggiunto. I paragrafi su 0.21.1 sotto descrivono la prima integrazione storica. Le nuove prove Windows e il Mac rinviato sono registrati nell'avanzamento.

### Decisione

Tre scelte, prese insieme perché si vincolano a vicenda.

1. **Licenza: CDDL-1.0**, fra le due che LibRaw offre.
2. **Collegamento: statico**, dentro l'eseguibile.
3. **Binding: uno shim C stretto compilato da `build.rs`**, non bindgen.

Il criterio era il prodotto commerciale proprietario con la distribuzione più semplice possibile. Le tre scelte discendono da quel criterio, non da preferenze di stile.

### Perché CDDL e non LGPL

La nota preliminare dell'8 settembre è stata assorbita in questo ADR: la candidatura iniziale CDDL è diventata la decisione qui registrata.

La CDDL è un copyleft **per file**. Gli obblighi riguardano i file coperti: renderne disponibile il sorgente, conservare le attribuzioni, identificare le proprie modifiche, non limitare nell'EULA i diritti sul sorgente coperto. Il §3.6 permette esplicitamente di combinare il software coperto con altro codice sotto termini diversi, compreso il proprietario, e di distribuire l'opera risultante. Non c'è alcun obbligo di rilink.

La LGPL 2.1 chiede invece che il destinatario possa sostituire la libreria con una propria versione modificata. Con collegamento **statico** il §6 lo soddisfa in un modo solo: distribuire i propri file oggetto, così che l'utente possa rilinkare. Per un prodotto proprietario è inaccettabile. Con collegamento **dinamico** l'obbligo si soddisfa spedendo la DLL separata e permettendone la sostituzione, ma resta un obbligo da progettare e da rispettare in ogni pacchetto.

Quindi: la LGPL rende costoso lo statico e vincolante il dinamico. La CDDL non fa né l'uno né l'altro.

### Perché statico

Discende dalla scelta sopra: una volta rimosso l'obbligo di rilink, non c'è più ragione di pagare il prezzo del dinamico.

Un solo eseguibile significa nessuna DLL da installare, nessun disallineamento di versione fra applicazione e libreria, nessun percorso di caricamento da governare, e un solo file da firmare. Vale sia per MSIX sia per un installer tradizionale, cioè per entrambe le opzioni che §17.1 lascia aperte.

### Obblighi che restano, e sono pochi

- Rendere disponibile ai destinatari il sorgente dei file coperti, con le eventuali modifiche, e indicare come ottenerlo. Il repository è già pubblico: vendorizzare LibRaw lì dentro soddisfa l'obbligo come effetto collaterale della build.
- Conservare licenza e attribuzioni, e identificare le proprie modifiche ai file LibRaw. La correzione di build descritta sotto tocca il wrapper, non i sorgenti LibRaw, che restano intatti.
- Non limitare nell'EULA i diritti CDDL sul sorgente coperto.
- Registrare versione e opzioni nel manifest, come §7 già richiede per la ricetta.

### Trappola verificata: i demosaic pack GPL

LibRaw distribuisce a parte dei pacchetti di demosaicing sotto GPL2 e GPL3, fra cui AMaZE, AFD, VCD e LMMSE. Compilarne uno dentro un prodotto proprietario ne comprometterebbe la licenza.

Verificato sulla copia vendorizzata di LibRaw 0.21.1: contiene le sole `LICENSE.CDDL` e `LICENSE.LGPL`, i file di demosaicing compilati sono i sei del nucleo, e nessun sorgente compilato contiene il testo della GNU General Public License. Il controllo va rifatto a ogni cambio di versione e appartiene al gate di R3, non all'occhio di chi aggiorna la dipendenza.

### Perché uno shim C e non bindgen

Il crate `libraw_rs_vendor` genera i binding con bindgen, che a build time richiede libclang. Aggiungere LLVM alle macchine di build e alla CI, solo per rigenerare a ogni compilazione una superficie che non cambia, è attrito che si paga per sempre.

Lo shim inverte il rapporto. Un file C che espone quattro funzioni: apri da buffer, sviluppa con la ricetta, consegna il raster, chiudi. La ricetta `TR-linear-v1` di §7, con `output_bps`, `output_color`, gamma lineare, `no_auto_bright`, `highlight`, `user_qual` e bilanciamento, vive lì dentro in un punto solo, leggibile, versionato e diffabile. La build richiede allora il solo MSVC.

C'è anche una simmetria che il progetto ha già: il decoder macOS è esattamente questo, un file Objective-C con un header C stretto, compilato da `build.rs`. Il percorso Windows diventa il suo gemello invece di un meccanismo diverso.

Lo shim va inoltre costruito sulle sorgenti `LibRaw_datastream` come chiede §5.5, così la libreria legge il buffer concesso e non riapre mai un nome di file.

### Correzioni di build accertate

Provate su questa macchina, con MSVC 14.51 e LibRaw 0.21.1 vendorizzato.

1. Il build script del crate passa `-Wno-deprecated-declarations` e `-pthread` con `flag`, che MSVC rifiuta con `D8021`. Vanno passati con `flag_if_supported`.
2. Gli header dichiarano i simboli con `__declspec(dllimport)` quando `LIBRAW_NODLL` non è definita, quindi ogni definizione confligge con la propria dichiarazione, `C4273`. Una build statica deve definire `LIBRAW_NODLL`.

Con entrambe applicate i sorgenti C++ compilano. La build si ferma poi su libclang, che è esattamente il motivo della scelta dello shim.

### Prima integrazione del 10 settembre (storico)

LibRaw 0.21.1 vive in `third_party/libraw`, non modificato, 75 unità di traduzione compilate su 78 presenti: le tre escluse sono file inclusi da altri, non compilati per sé. Lo shim è in `native/libraw`, il binding in `crates/tr-worker/src/libraw.rs`, e `crates/tr-worker/build.rs` compila il percorso Apple su macOS e questo su Windows.

Una terza correzione è emersa in integrazione, oltre alle due previste. Le dimensioni finali dell'immagine si ottengono solo chiamando `adjust_sizes_info_only`: lette subito dopo l'identificazione danno i valori prima della rotazione, e il chiamante dimensionerebbe il buffer per un'immagine di forma diversa.

Il nome della ricetta sta in `tr_core::decoder::RECIPE`, letto sia dalla provenienza sia dalla chiave di cache, e un test verifica che coincida con `TR_LIBRAW_RECIPE` nell'header dello shim. Prima la chiave di cache conteneva `Apple-TR-linear-v1` cablato, quindi su Windows avrebbe dichiarato la ricetta Apple mentre i pixel venivano da LibRaw.

Verifica sui 30 NEF Nikon D750 autorizzati: trenta contenitori, trenta sviluppati dal mosaico, nessuna ricaduta sull'anteprima. Il test controlla anche che stadio dichiarato e affermazione sul colore siano coerenti, cioè che lo sviluppo dichiari il proprio spazio e l'anteprima dichiari di assumerlo.

### Conseguenze

L'integrazione richiede il solo MSVC, già presente. Nessuna installazione di LLVM, nessun vcpkg, nessuna dipendenza di infrastruttura oltre a quella che serve già a compilare il workspace.

Restano fuori da questo ADR: la ricetta misurata contro il gate di §7, la matrice camere e il badge `RENDER LIBRAW` di R3, e il parere formale di conformità prima del primo pacchetto distribuito. La scelta di licenza qui registrata è la premessa di quel parere, non il suo sostituto.

Non si dichiara alcun confronto di fedeltà con il percorso Apple: ADR 0004 dice già che una ricetta identica non è promessa, e la tolleranza colorimetrica fra decoder diversi va definita, non presa in prestito dalla soglia ΔE00 del percorso GPU.

### Estensione locale del 12 settembre 2026

Nella sessione del 12 settembre, prima della pubblicazione del 14: selettore di motore, AHD e demosaicing TrueRenderer fp32 affiancati al bilineare. Il nome della ricetta effettiva è ora `RawEngine::recipe()`, serializzato nelle richieste e separato nella cache; `RECIPE` rimane per la compatibilità storica. Il colore RAW è etichettato come sviluppo con ricetta, distinto dal profilo dichiarato da un file bitmap.

Lo shim offre anche estrazione del mosaico e calibrazione per D750/DNG sintetici, con gestione delle eccezioni al confine C. L'AHD impiega il nucleo già vendorizzato; nessun file upstream o licenza è stato modificato. MSVC 14.51 richiede qui un archivio creato in una sola invocazione con response file: il merge progressivo di `cc` falliva con LNK1114. La build macOS aggiunge LibRaw ai componenti Apple e libc++ ai due XPC; resta da costruire e verificare su Mac. Risultati, ricette, limiti e istruzioni in [progetto motori RAW](progetto-motori-raw.md).

### Fonti e stato della nota preliminare assorbita

La nota dell'8 settembre precedeva l'integrazione: quella baseline usava CIRAWFilter e non incorporava LibRaw. La scelta successiva è CDDL-1.0, con LibRaw 0.22.2 pubblicata in `67146e3`; versione e opzioni sono fissate nel manifest. Il nuovo bundle macOS resta da verificare.

Fonti conservate dalla nota: [presentazione ufficiale LibRaw](https://www.libraw.org/about), [testo CDDL incluso](../third_party/libraw/LICENSE.CDDL) e [alternativa LGPL inclusa](../third_party/libraw/LICENSE.LGPL). Questi testi e le attribuzioni upstream rimangono nel repository. L'accorpamento documentale non è un nuovo parere legale né una qualifica di un pacchetto commerciale; il controllo di distribuzione resta quello indicato nelle conseguenze e in [NOTICE](../NOTICE.md).
