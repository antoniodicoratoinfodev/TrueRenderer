# Contratto osservabile del codec Fuji X-Trans, revisione 1

Specifica di ricerca associata a [ADR 0012 §14](progetto-truerenderer-experimental.md#14-implementazione-propria-e-miglioramento-misurabile). Non è una specifica Fujifilm, né prova di origine indipendente. Il contratto descrive fatti ricavati dal port CDDL E8, la cui [provenienza resta dichiarata](../third_party/fuji-codec/manifest.json), e sottoposti a esperimenti. La conoscenza di quel codice non viene cancellata dalla separazione dei file. Una futura implementazione può superare il confronto numerico senza avere ancora superato la revisione di provenienza.

## Dominio e fonti

| Identificatore | Materiale | Cosa può dimostrare |
|---|---|---|
| F1 | [Manuale X-T5](https://fujifilm-dsc.com/en/manual/x-t5/menu_shooting/image_quality_setting/) | Esistenza di modalità compresse reversibili e irreversibili; non l'algoritmo binario |
| F2 | [Contenitore RAF, libopenraw](https://libopenraw.freedesktop.org/formats/raf/) | Struttura del contenitore; non i predittori lossy |
| F3 | Port E8 e sorgente LibRaw 0.22.2 fissato dal manifest | Origine delle regole algoritmiche sotto indicate; fonte derivata esplicita |
| E1 | RAW X-T5 lossy autorizzato, hash nel manifest privato della campagna | Compatibilità su quel file; una sola combinazione camera/modalità |
| E2 | DNG prodotto da Adobe e campioni stage 1 del SDK | Controllo indipendente dei codici nell'area conservata dal convertitore |
| E3 | Fixture generate da un emettitore di bit del banco, con soluzioni analitiche | Controlli dei rami esercitati; non nuovi campioni fotografici né fonte primaria del formato |

Le regole numeriche seguenti hanno fonte F3. Il confronto E1/E2 le verifica congiuntamente nel solo dominio esercitato: non ne prova singolarmente la generalità. Lossless fotografico, altre profondità, Bayer Fuji, altri modelli e quantizzazione fuori dai casi esercitati richiedono acquisizioni. Il campione E1 non autorizza ad aggiungere tali capacità alla GUI.

## Involucro del payload

Interi multibyte big endian; bit del residuo dal più significativo. Header di 16 byte: `IS`, un byte reversibile (1) / lossy (0), tipo sensore 16, precisione 14, altezza u16, larghezza imbottita u16, larghezza reale u16, larghezza striscia u16 = 768, numero strisce u8, numero gruppi u16 = altezza / 6. Il contratto limitato ammette larghezza >=768 multipla di 24, altezza >=6 multipla di 6, al massimo 16 strisce e il limite di pixel del nucleo. La larghezza imbottita è `ceil(larghezza/768)*768`.

Seguono lunghezze u32 delle strisce; la tabella occupa un multiplo di 16 byte. In lossy segue una tabella byte dei quantizzatori, ordinata per striscia e poi gruppo; ogni colonna di gruppi è arrotondata a 16 byte. Il dato di padding non è un quantizzatore attivo. Le strisce seguono contigue, con somma delle lunghezze esattamente uguale al payload residuo; nessun lettore può oltrepassare la propria striscia. La coda non consumata ammessa dalla baseline è al massimo 16 byte, arrotondando in eccesso i bit residui. Questa tolleranza osservata non implica che qualunque padding sia prescritto dal produttore.

Controesempi obbligatori: header/tag discordante, dimensione e conteggio fuori limite, overflow, intervallo troncato, lunghezza zero, coda eccessiva e prefisso senza terminatore. Rifiuto esplicito prima di allocare immagini non ammesse.

## Geometria delle componenti

Una striscia rappresenta 768 colonne sensore con 512 posizioni per riga di componente. Ogni gruppo di sei righe sensore produce tre righe R, sei G e tre B. Ogni componente conserva due righe precedenti, inizialmente zero. Colonne virtuali -1 e 512 della riga corrente copiano rispettivamente le colonne 0 e 511 della riga precedente. Prima di un gruppo le nuove righe interne sono zero; prima di ogni coppia di righe i bordi delle righe non ancora prodotte riflettono lo stato disponibile. La dipendenza temporale dei bordi fa parte del contratto.

Ordine delle coppie (indice della riga nel gruppo, partendo da zero):

| Fase | Prima | Seconda | Pari codificati della prima | Pari codificati della seconda |
|---|---|---|---|---|
| 0 | R0 | G0 | nessuno | tutti |
| 1 | G1 | B0 | tutti | nessuno |
| 2 | R1 | G2 | posizione % 4 = 2 | nessuno |
| 3 | G3 | B1 | tutti | posizione % 4 = 0 |
| 4 | R2 | G4 | posizione % 4 = 0 | tutti |
| 5 | G5 | B2 | nessuno | posizione % 4 = 2 |

Per ciascuna posizione pari, prima componente poi seconda. Dopo aver prodotto i primi cinque pari (0,2,4,6,8), produrre la prima coppia di dispari (1); poi alternare una coppia pari e una dispari fino a esaurire i pari, infine i dispari restanti. Tutti i dispari sono codificati. Contesti statistici 0,1,2 per i pari secondo fase % 3, 3,4,5 per i dispari. Una posizione non codificata usa solo il predittore.

Proiezione: alla colonna sensore x corrisponde la posizione componente `4*floor(x/6)+[0,1,1,2,3,3][x%6]`. La riga di R/B è floor(y/2), quella di G è y. La selezione di componente segue il CFA canonico in `cfa.rs`, senza traslare il pattern completo prima del codec. La striscia finale si decodifica intera ma si copiano soltanto le colonne reali. Controesempio E8: duplicare la posizione della colonna sensore 4 altera il verde e fallisce E2.

## Predizione e aritmetica

Per la posizione corrente, B è sopra, C sopra-sinistra, D sopra-destra, F due righe sopra, A sinistra e H destra nella riga corrente. Interi non negativi a 14 bit; divisioni qui indicate sono intere verso il basso.

Pari: se `|C-B|` supera strettamente sia `|F-B|` sia `|D-B|`, P=(F+D+2B)/4; se `|D-B|` supera strettamente gli altri due, P=(F+C+2B)/4; altrimenti P=(D+C+2B)/4. Gradienti U=B-F, V=C-B.

Dispari: quando B è un estremo stretto rispetto a C e D, P=(A+H+2B)/4; altrimenti P=(A+H)/2. Gradienti U=B-C, V=C-A. Pareggi e ordine di calcolo devono essere conservati. Il valore del predittore e dei campioni sintetizzati resta nel dominio sensore.

## Quantizzazione, contesti e residui

Massimo M=16383. Per quantizzatore q: passo S=2q+1, cardinalità T=floor((M+2q)/S)+1, larghezza escape L=ceil(log2(T)). Bucket firmato di gradiente v: zero se |v|<=q, altrimenti magnitudine 1 sotto 3q+18, 2 sotto 5q+67, 3 sotto 7q+276, 4 altrimenti; segno di v.

Il modello principale usa q del gruppo (0 in reversibile), coefficiente 9 e si reinizializza solo al cambio di q. Tre modelli secondari usano q=0,1,2 e coefficiente 3; persistono per tutta la striscia. Scegliere il primo indice j in 1,2,3 per cui q principale>=j e |U|+|V|<=j+4; altrimenti il principale. Indice firmato K=coefficiente*bucket(U)+bucket(V). Ciascun modello ha sei contesti e 41 statistiche, selezionate da |K|. Stato iniziale (somma,conteggio)=(max(2,floor((T+32)/64)),1).

Leggere z zeri seguiti da 1; rifiutare z>56. Se z<55-L, k è il minimo intero non negativo con conteggio*2^k>=somma (k<=14), poi codice=z*2^k+leggi(k). Altrimenti codice=1+leggi(L). Rifiutare codice>=T. Delta=codice/2 per codice pari, -(codice+1)/2 per dispari. Aggiornare somma+=|delta|, dimezzare somma e conteggio quando il conteggio precedente era 64, poi incrementare conteggio.

Valore=P+delta*S se K>=0, P-delta*S altrimenti. Se valore<-q aggiungere T*S; se valore>M+q sottrarre T*S; infine saturare a [0,M]. Questa saturazione appartiene al codec irreversibile, non al rendering float. Controesempi: soglie di escape, z=56/57, valori appena oltre cardinalità, delta negativo, cambio q con reset del solo principale, wrap a entrambi gli estremi. Non usare un confronto visivo per convalidarli.

## Acquisizioni e decisioni di qualifica

Per il codec servono coppie di scatti X-T5 lossless/lossy/non compressi con modalità, bit depth, firmware, ISO e impostazioni annotate; includere nero, saturazione, rumore ad alto ISO, trame fini, bordi e una scena cromatica. Servono inoltre margini sensore leggibili da un riferimento distinto e almeno un'altra camera prima di estendere il modello. Le coppie non sono identiche pixel per pixel: il rumore fra scatti non è un errore del codec. Il confronto esatto deve decodificare lo stesso file.

Per profili propri servono target con misure spettrali/colorimetriche utilizzabili e illuminanti misurati (almeno due pertinenti), esposizioni senza clipping, ripetizioni, dark frame e campi uniformi. Tenere sessioni/patch di verifica separate dal fit; annotare ottica e correzioni. Le fotografie generiche in Downloads e i JPEG sviluppati da altri motori non sostituiscono questi dati. Diritti delle immagini e dei dati di misura vanno registrati prima di inserirli in un corpus pubblico.

Esiti e limiti effettivamente verificati vengono registrati soltanto in STATO.md e nei rapporti della campagna. Questa specifica non promuove modalità o provenienza.

## Esecuzione del laboratorio

Compilare con `scripts/cargo-local.sh build -p tr-raw-lab --release --locked --offline`. Il comando `target/release/tr-raw-lab PAYLOAD WIDTH HEIGHT NEW_OUTPUT_U16LE` legge un payload già estratto e convalidato, produce codici little endian e stampa hash, dimensioni e memoria dei buffer. Rifiuta un output già esistente. Non legge automaticamente cartelle fotografie, non è il decoder della GUI e non importa il port E8. Usare una directory privata sotto `var/`; non aggiungere payload fotografici al repository. Per la verifica di indipendenza compilare con un `CARGO_TARGET_DIR` nuovo e controllare artefatti, depfile, simboli e librerie dinamiche del solo eseguibile.

Il confronto di ricerca si esegue con `scripts/cargo-local.sh test -p tr-raw --lib --release --locked --offline codec_research_campaign -- --ignored --nocapture`. L'ambiente `TR_FUJI_LAB_MANIFEST` deve indicare un JSON privato con `source`, `source_sha256`, `reference_u16le`, `reference_sha256`, `reference_width`, `reference_height`, `output`. Il riferimento è stage 1 ottenuto con il SDK Adobe dal DNG originale, convertito senza trasformazioni a u16 little endian; conservare versione/hash del SDK, diagnostica, DNG originale e hash del TIFF. Il banco non include parsing e hash nel tempo di unpack; i due riscaldamenti e le 30 ripetizioni alternate sono prescritti dal protocollo. Conservare il rapporto di ogni candidato prima di usare un nuovo output.

`photographic_refactor_research_campaign` usa lo stesso manifest per confrontare la separazione camera RGB con il sorgente E8 congelato, su tutti i pixel con due WB. `quality_research_campaign` usa `TR_QUALITY_LAB_ROOT` e `TR_QUALITY_SPLIT=development` oppure `validation`; i candidati successivi devono rispettare la regola dell'holdout. La suite ordinaria esclude esplicitamente questi tre esperimenti manuali; continua a eseguire fixture, limiti dei residui, mutazioni del payload ed equivalenza sintetica del refactor.
