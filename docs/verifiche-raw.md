# Campagne RAW e immagini grandi

Raccolta delle campagne storiche, con risultati positivi e negativi e limiti riferiti ai rispettivi binari. Per lo stato corrente leggere [STATO.md](../STATO.md); i rinvii «ancora aperto» nei testi sotto descrivono la data della campagna, non la situazione attuale.

I nomi originali riportati negli inventari JSON rimangono riferimenti storici; le sezioni seguenti ne conservano il contenuto.

<a id="nikon-d40"></a>

## Revisione dei motori RAW e Nikon D40

12 settembre 2026. Richiesta del titolare: controllare il lavoro precedente e verificare i NEF D40 nella cartella autorizzata. Tutto locale, **nessun commit**; originali in sola lettura.

### Revisione del codice

Esaminati selezione/preferenze, identità IPC e cache, coalescenza delle richieste, invalidazione della UI, estrazione nativa e calibrazione, interpolazione Bayer e orientamento. Le ricette rimangono distinte e il percorso proprio non viene sostituito da AHD o JPEG quando rifiuta un RAW. Questo controllo non equivale a un audit completo di sicurezza o colore.

**Problema trovato:** `invalidate_raw_engine()` revocava la generazione anche di una scansione in corso, ma non la riaccodava. La risposta diventava obsoleta e la finestra poteva attendere indefinitamente. La correzione riaccoda la stessa cartella nella nuova generazione, mantenendo l'eventuale selezione del file richiesta all'apertura. La prova nativa `--raw-engines-smoke --raw-engine-scan-change` cambia motore prima che la UI possa consumare la risposta della scansione.

### D40: prova della versione precedente

22 NEF autorizzati, verificati tutti con ciascuno dei tre motori Windows:

- LibRaw bilineare: 22/22 sviluppi completi.
- LibRaw AHD: 22/22 sviluppi completi.
- TrueRenderer fp32: 22/22 rifiuti espliciti, perché l'allowlist ammetteva soltanto D750 e DNG sintetici propri.

Originali invariati. Report privato negativo conservato in `var/raw-engine-evaluation-1789239163/report.json`. Un rifiuto del motore sperimentale non era un errore di decompressione dei NEF: i due percorsi LibRaw riconoscevano Nikon D40 e restituivano 3039×2014 o equivalente orientato.

### Estensione verificata su Windows

Aggiunto soltanto il modello esatto Nikon D40 NEF alla stessa estrazione Bayer fp32. D40X, altri modelli e DNG convertiti non entrano implicitamente. Restano obbligatori CFA Bayer a tre colori, WB as-shot valido, calibrazione nero/bianco e matrice rappresentabili, geometria coerente prima/dopo unpack e orientamento supportato. Il demosaic e le ricette dei modelli già supportati non cambiano: non è necessario attribuire loro nuovi pixel o invalidare cache identiche.

LibRaw 0.21.1 contiene una matrice specifica D40 (`src/tables/colordata.cpp`), interpreta la curva NEF nel decoder Nikon e applica il crop specifico nel riconoscimento. Il nostro codice usa i suoi dati dopo unpack, senza inventare WB, livelli o profili. Dimensioni dispari sono coperte dalle regressioni CFA/bordi del core.

- [x] Baseline completa sui 22 NEF e revisione del codice.
- [x] Correzione della scansione ed estensione controllata D40 implementate.
- [x] Nuovi sviluppi completi D40, regressioni D750 e test del codice.
- [x] Prova nativa della scansione/cambio motore e pixel del viewer D40.
- [x] Documenti finali sincronizzati.

**D40:** 22 NEF × 3 motori = **66/66 sviluppi completi passati**, originali invariati. Dimensioni 3039×2014 e 2014×3039 secondo l'orientamento. Tutti i file del corpus dichiarano ISO 200; non è una matrice ISO completa. L'estrazione propria legge nero 0 e bianco 4095, con WB as-shot variabile e matrice LibRaw D40. Report privato: `var/raw-engine-evaluation-1789239590/report.json`.

| Motore | Mediana sviluppo, debug | Intervallo RGB osservato |
|---|---:|---:|
| LibRaw bilineare | 0,706 s | 0…1 |
| LibRaw AHD | 1,197 s | 0…1 |
| TrueRenderer fp32 | 1,561 s | −0,172…2,095 |

Tempi sequenziali senza cache applicativa, con cache OS non svuotata e altri controlli sulla macchina. Non sono tempi evento→frame o benchmark del solo demosaic. La conservazione dei valori fuori [0,1] riguarda il buffer di lavoro, non una promessa di recupero delle alte luci o della loro visualizzazione SDR.

**D750:** una sorgente di regressione × 3 motori, tutti passati; nove ritagli 1:1 identici byte per byte alla prova precedente, così come metadati e intervalli completi. Report privati `var/raw-engine-evaluation-1789239713/` e `var/d40-check/d750-regression.json`. Non si dichiara un confronto bit per bit di tutti i float dell'immagine.

**Finestra:** una copia privata D40 con orientamento verticale, catalogo separato, cambio motore prima del consumo della scansione e quattro stadi bilineare/AHD/proprio/ritorno al bilineare. Scansione conclusa, selezione conservata; al ritorno due cache hit e nessun nuovo sviluppo. Confrontati **260.796 pixel** visibili: differenza massima **zero** rispetto al riferimento CPU, soglia prestabilita un livello sRGB8. Questa geometria utilizza il ricampionamento CPU; non è una nuova qualifica GPU/monitor. Screenshot, riferimenti e report conservati in `var/d40-check/ui/`.

**Codice:** fmt, Clippy con warning negati, build, 75 test Rust ordinari e sette integrazioni esplicite passati; queste ultime eseguite anche con `TR_RAW_SAMPLE` sulla cartella D40. Passate due regressioni Python e sei controlli IPC. La guida nelle impostazioni nomina D750 e D40; il messaggio di fine scansione non attribuisce più a macOS le cartelle aperte su Windows.

[Riepilogo verificabile senza fotografie](../reports/raw-engines-d40-windows.json). Il precedente report `raw-engines-windows.json` rimane la fotografia della consegna D750, con i suoi hash. Nessun commit, staging o pubblicazione.

Restano da qualificare colore misurato, altre condizioni ISO/illuminazione/firmware, matrice camere generale e nuovo bundle macOS/XPC. La suite XPC è stata nuovamente invocata su Windows e non può procedere senza codesign; log in `var/d40-xpc-attempt.log`. Un'immagine visualizzabile non prova che il motore sia superiore ad Apple/AHD o fedele a un target colorimetrico.

*Fonte storica: `docs/verifica-motori-d40.md`.*

<a id="grandi-raw-macos"></a>

## Immagini grandi, pressione e motori RAW — macOS

Campagna locale del 15 settembre 2026, su richiesta del titolare. Esiti e hash nel [rapporto finale](../reports/large-pressure-raw-macos.json). Nessun gate R0–R4 chiuso.

### Esito

Passate 240 azioni PNG 12/24/45 MP, 20 con pressione renderer e 40 RAW; 120 sviluppi D750, nove confronti registrati, invalidazione 45 MP, cambio motore UI/pixel e XPC. Suite: 75 test Rust, nove ignorati in questa corsa; regressione Python dell’allineamento passata.

**Il gate memoria fisica non è passato:** nei quattro casi 45 MP Full con 4 GiB configurati, massimo RSS sommato 4.816.601.088 byte e footprint 4.296.512.936 byte. Crediti di ammissione entro quota. A 1536 MiB, dopo l’espulsione della riserva, le due tracce restano entro quota campionata e recuperano pixel esatti; la copertura provvisoria scende al 3,14%. A 512 MiB il rifiuto RAW restituisce tutti i crediti di lavoro. Nessun limite fisico universale dedotto da questi controlli.

### Protocollo

- PNG analitici generati da 4000×3000, 6000×4000 e 8256×5504 pixel. Due immagini per scenario, quota esplicita 4096 MiB, Standard/Full e CPU/GPU, processi distinti freddo/riapertura.
- La traccia verifica selezione, ritorno RAM, zoom, pan, Fit, cambio qualità e 1:1; registra dimensioni native, livello residente, crediti, copertura e pixel della superficie. Standard deve esercitare un livello ridotto; 1:1 deve ottenere livello zero e pixel esatti.
- I livelli ridotti sono rappresentazioni residenti ricavate dallo sviluppo completo: questa campagna non abilita né qualifica un decode RAW ridotto/regionale.
- La GPU può usare il fallback CPU dichiarato quando il livello supera le capability. Una riapertura può richiedere nuovo decode se l’artefatto Full supera il limite del writer. Entrambi gli esiti sono registrati, senza chiamarli esecuzione interamente GPU o hit SSD.
- Un bundle univoco separa host e XPC dalle istanze dell’utente. Campionamento RSS e physical footprint tramite libproc circa ogni 25 ms; somma fra processi, con possibilità di doppio conteggio di pagine condivise. Non misura separatamente tutti i driver o i picchi fra campioni.
- Pressione della sola policy renderer iniettata durante pan/Fit, su 12 MP e quota 1536 MiB: la riserva deve esistere prima ed essere espulsa dopo. Il ricalcolo deve concludersi con pixel corretti. Non è pressione fisica OS.
- Rifiuto di decode RAW a 512 MiB: errore esplicito, originali invariati e zero crediti di lavoro dopo shutdown. Non si considera passato il carico negativo; è passata la verifica del suo rifiuto sicuro.
- Trenta NEF D750 autorizzati, sola lettura, quattro motori: Apple, LibRaw bilineare, LibRaw AHD e TrueRenderer fp32. Le prime tre sorgenti producono ritagli centrali lineari privati per confronti registrati.

### Rilievo XPC e correzione

La prima campagna fotografica ha riprodotto l’esaurimento dello stack del thread dispatch nel probe LibRaw (`Thread stack size exceeded`, `tr_libraw_probe`). L’oggetto LibRaw locale superava lo stack disponibile. I quattro ingressi del bridge ora possiedono l’oggetto sullo heap tramite `std::unique_ptr`, con distruzione automatica e le gestioni delle eccezioni già presenti. Nessuna modifica alla ricetta dei pixel o agli entitlement. Un primo NEF passa su tutti i motori dopo la correzione; la campagna estesa identifica separatamente il binario finale.

### Confronto fotografico

Le geometrie Apple/LibRaw possono differire nell’area attiva. Il confronto cerca una traslazione intera entro ±16 pixel tra ritagli centrali, usando i gradienti della luminanza lineare; una correlazione debole o al bordo della ricerca non qualifica l’allineamento. Nessuna compensazione di scala, distorsione o spostamento subpixel.

Le metriche RGB lineari descrivono differenze fra pipeline, comprese esposizione, WB, clipping e area attiva. Apple e AHD non sono verità della scena. Mancano un target misurato con illuminante noto, qualifica ΔE00/ICC, altre fotocamere e una campagna estesa di rumore/moire. I 32 Bayer sintetici con verità nota costituiscono una prova distinta dai NEF.

### Riproduzione

Usare `scripts/cargo-local.sh` per build e test e un bundle macOS con XPC:

```sh
python3 scripts/test-large-navigation.py --bundle <bundle.app>
python3 scripts/test-large-navigation.py --bundle <bundle.app> --sizes 12 --qualities Standard --memory-mib 1536 --pressure
python3 scripts/compare-raw-patches.py --self-test
python3 scripts/compare-raw-patches.py <cartella-privata-dei-ritagli>
```

`--raw-folder <cartella-autorizzata> --engines Apple,LibRawBilinear,LibRawAhd,TrueRenderer --qualities Standard --modes Cpu` estende la traccia a copie private dei primi due RAW. `--verify-raw-engines` sul binario del bundle verifica invece tutti i NEF/DNG della cartella. Percorsi, fotografie, ritagli e cataloghi restano sotto `var/`; i rapporti pubblicabili riportano dati aggregati e hash.

*Fonte storica: `docs/verifica-grandi-raw-macos.md`.*
