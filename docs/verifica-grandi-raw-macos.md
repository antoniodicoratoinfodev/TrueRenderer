# Immagini grandi, pressione e motori RAW — macOS

Campagna locale del 15 settembre 2026, su richiesta del titolare. Esiti e hash nel [rapporto finale](../reports/large-pressure-raw-macos.json). Nessun gate R0–R4 chiuso.

## Esito

Passate 240 azioni PNG 12/24/45 MP, 20 con pressione renderer e 40 RAW; 120 sviluppi D750, nove confronti registrati, invalidazione 45 MP, cambio motore UI/pixel e XPC. Suite: 75 test Rust, nove ignorati in questa corsa; regressione Python dell’allineamento passata.

**Il gate memoria fisica non è passato:** nei quattro casi 45 MP Full con 4 GiB configurati, massimo RSS sommato 4.816.601.088 byte e footprint 4.296.512.936 byte. Crediti di ammissione entro quota. A 1536 MiB, dopo l’espulsione della riserva, le due tracce restano entro quota campionata e recuperano pixel esatti; la copertura provvisoria scende al 3,14%. A 512 MiB il rifiuto RAW restituisce tutti i crediti di lavoro. Nessun limite fisico universale dedotto da questi controlli.

## Protocollo

- PNG analitici generati da 4000×3000, 6000×4000 e 8256×5504 pixel. Due immagini per scenario, quota esplicita 4096 MiB, Standard/Full e CPU/GPU, processi distinti freddo/riapertura.
- La traccia verifica selezione, ritorno RAM, zoom, pan, Fit, cambio qualità e 1:1; registra dimensioni native, livello residente, crediti, copertura e pixel della superficie. Standard deve esercitare un livello ridotto; 1:1 deve ottenere livello zero e pixel esatti.
- I livelli ridotti sono rappresentazioni residenti ricavate dallo sviluppo completo: questa campagna non abilita né qualifica un decode RAW ridotto/regionale.
- La GPU può usare il fallback CPU dichiarato quando il livello supera le capability. Una riapertura può richiedere nuovo decode se l’artefatto Full supera il limite del writer. Entrambi gli esiti sono registrati, senza chiamarli esecuzione interamente GPU o hit SSD.
- Un bundle univoco separa host e XPC dalle istanze dell’utente. Campionamento RSS e physical footprint tramite libproc circa ogni 25 ms; somma fra processi, con possibilità di doppio conteggio di pagine condivise. Non misura separatamente tutti i driver o i picchi fra campioni.
- Pressione della sola policy renderer iniettata durante pan/Fit, su 12 MP e quota 1536 MiB: la riserva deve esistere prima ed essere espulsa dopo. Il ricalcolo deve concludersi con pixel corretti. Non è pressione fisica OS.
- Rifiuto di decode RAW a 512 MiB: errore esplicito, originali invariati e zero crediti di lavoro dopo shutdown. Non si considera passato il carico negativo; è passata la verifica del suo rifiuto sicuro.
- Trenta NEF D750 autorizzati, sola lettura, quattro motori: Apple, LibRaw bilineare, LibRaw AHD e TrueRenderer fp32. Le prime tre sorgenti producono ritagli centrali lineari privati per confronti registrati.

## Rilievo XPC e correzione

La prima campagna fotografica ha riprodotto l’esaurimento dello stack del thread dispatch nel probe LibRaw (`Thread stack size exceeded`, `tr_libraw_probe`). L’oggetto LibRaw locale superava lo stack disponibile. I quattro ingressi del bridge ora possiedono l’oggetto sullo heap tramite `std::unique_ptr`, con distruzione automatica e le gestioni delle eccezioni già presenti. Nessuna modifica alla ricetta dei pixel o agli entitlement. Un primo NEF passa su tutti i motori dopo la correzione; la campagna estesa identifica separatamente il binario finale.

## Confronto fotografico

Le geometrie Apple/LibRaw possono differire nell’area attiva. Il confronto cerca una traslazione intera entro ±16 pixel tra ritagli centrali, usando i gradienti della luminanza lineare; una correlazione debole o al bordo della ricerca non qualifica l’allineamento. Nessuna compensazione di scala, distorsione o spostamento subpixel.

Le metriche RGB lineari descrivono differenze fra pipeline, comprese esposizione, WB, clipping e area attiva. Apple e AHD non sono verità della scena. Mancano un target misurato con illuminante noto, qualifica ΔE00/ICC, altre fotocamere e una campagna estesa di rumore/moire. I 32 Bayer sintetici con verità nota costituiscono una prova distinta dai NEF.

## Riproduzione

Usare `scripts/cargo-local.sh` per build e test e un bundle macOS con XPC:

```sh
python3 scripts/test-large-navigation.py --bundle <bundle.app>
python3 scripts/test-large-navigation.py --bundle <bundle.app> --sizes 12 --qualities Standard --memory-mib 1536 --pressure
python3 scripts/compare-raw-patches.py --self-test
python3 scripts/compare-raw-patches.py <cartella-privata-dei-ritagli>
```

`--raw-folder <cartella-autorizzata> --engines Apple,LibRawBilinear,LibRawAhd,TrueRenderer --qualities Standard --modes Cpu` estende la traccia a copie private dei primi due RAW. `--verify-raw-engines` sul binario del bundle verifica invece tutti i NEF/DNG della cartella. Percorsi, fotografie, ritagli e cataloghi restano sotto `var/`; i rapporti pubblicabili riportano dati aggregati e hash.
