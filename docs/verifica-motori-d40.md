# Revisione dei motori RAW e Nikon D40

12 settembre 2026. Richiesta del titolare: controllare il lavoro precedente e verificare i NEF D40 nella cartella autorizzata. Tutto locale, **nessun commit**; originali in sola lettura.

## Revisione del codice

Esaminati selezione/preferenze, identità IPC e cache, coalescenza delle richieste, invalidazione della UI, estrazione nativa e calibrazione, interpolazione Bayer e orientamento. Le ricette rimangono distinte e il percorso proprio non viene sostituito da AHD o JPEG quando rifiuta un RAW. Questo controllo non equivale a un audit completo di sicurezza o colore.

**Problema trovato:** `invalidate_raw_engine()` revocava la generazione anche di una scansione in corso, ma non la riaccodava. La risposta diventava obsoleta e la finestra poteva attendere indefinitamente. La correzione riaccoda la stessa cartella nella nuova generazione, mantenendo l'eventuale selezione del file richiesta all'apertura. La prova nativa `--raw-engines-smoke --raw-engine-scan-change` cambia motore prima che la UI possa consumare la risposta della scansione.

## D40: prova della versione precedente

22 NEF autorizzati, verificati tutti con ciascuno dei tre motori Windows:

- LibRaw bilineare: 22/22 sviluppi completi.
- LibRaw AHD: 22/22 sviluppi completi.
- TrueRenderer fp32: 22/22 rifiuti espliciti, perché l'allowlist ammetteva soltanto D750 e DNG sintetici propri.

Originali invariati. Report privato negativo conservato in `var/raw-engine-evaluation-1789239163/report.json`. Un rifiuto del motore sperimentale non era un errore di decompressione dei NEF: i due percorsi LibRaw riconoscevano Nikon D40 e restituivano 3039×2014 o equivalente orientato.

## Estensione verificata su Windows

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
