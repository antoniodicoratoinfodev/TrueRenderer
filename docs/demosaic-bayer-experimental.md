# Esperimento Bayer a residui

## Esecuzione della baseline con tre righe

La costruzione direzionale di produzione può conservare soltanto tre righe di verde interpolate: R/B leggono il verde della riga corrente e delle due adiacenti. Ogni riga verde dipende esclusivamente dal mosaico acquisito, con supporto fino a due siti; non dipende dalle altre righe interpolate. Usare tre slot identificati dalla riga sensore modulo tre e riutilizzarli durante la scansione del crop. Ai bordi riflettere rispetto al sensore, senza ripetere il sito terminale; non riflettere sui limiti del crop. Precalcolare, per ogni colonna, i quattro indici riflessi a distanza −2/−1/+1/+2.

Conservare formula e ordine delle operazioni fp32, somma dei residui cromatici, WB precedente, matrice e orientamento successivi. Richiedere uguaglianza al bit con il sorgente precedente congelato, anche su valori estesi, zeri con segno, quattro fasi, dimensioni minime/dispari e crop sui bordi. Nessun cambio della ricetta numerica o promozione di una variante RI/MLRI deriva da questa riorganizzazione.

Lo scratch Bayer riserva `larghezza × (3 × sizeof(f32) + 4 × sizeof(usize)) + 1024` byte, escludendo mosaico e RGBA finale. Il piano del decoder considera separatamente il picco di unpack/calibrazione, quello del demosaic e quello dell'export, mantenendo la riserva metadati. La riduzione dei buffer non chiude il gate RSS. Misurare il kernel su fotogrammi autorizzati con trenta ripetizioni alternate dopo tre riscaldamenti per metodo; richiedere identità anche in ogni ripetizione. Parsing, codec e WB restano esclusi dal timer del kernel; la campagna applicativa/XPC qualifica separatamente l'integrazione.

## BR1/BR2: residui a verde fissato

BR1/BR2 sono sonde proprie dello stadio R/B a verde fissato, ispirate ai modelli locali RI e MLRI analizzati da [Jin et al., IPOL 2021](https://www.ipol.im/pub/art/2021/358/). Non sono implementazioni complete degli algoritmi pubblicati: conservano il verde direzionale della baseline, senza il loro stadio di ricostruzione direzionale del verde o media dei coefficienti. Nessun codice IPOL è importato.

Per ciascun pixel e canale R/B, sui siti acquisiti della finestra 7×7 stimare `C=aG+b` in float64, con `b=media(C)−a*media(G)`. BR1 usa covarianza/varianza con regolarizzazione 1e-6; BR2 minimizza il residuo del Laplaciano a croce di passo 2, usando `a=media(LG*LC)/(media(LG²)+1e-6)` sui centri con vicini validi. Se mancano centri, usare BR1. Valutare la previsione al centro; interpolare bilinearmente il residuo acquisito `C−previsione` dai siti dello stesso canale e sommarlo alla previsione mancante. Ai bordi includere solo i vicini esistenti e rinormalizzare. Conservare siti acquisiti, verde e valori estesi. Nessun denoise o recupero delle alte luci implicito.

Il protocollo Bayer v1 usa le famiglie analitiche del banco X-Trans, quattro fasi Bayer e nuove frequenze/angoli/seed di verifica. Metriche e gate sono identici: falso colore acromatico almeno −20%, altre categorie/dettaglio non oltre +5% per categoria e fase, siti acquisiti esatti e nessun nonfinito. Pubblicare entrambi i candidati, anche quando respinti. Lo stadio verde invariato limita ciò che queste sonde possono migliorare. Non cambiare il demosaic Nikon sulla sola base di questo esperimento; serve anche verifica fotografica indipendente e costo del prodotto. Rumore e alte luci richiedono acquisizioni ripetute, dark/flat e riferimenti non saturi prima di qualificare una ricostruzione fisica.

Errata del manifest v1 congelato: il campo descrittivo singolare `candidate: "DR6"` e la frase «all 36 phases» sono residui del modello X-Trans. Per Bayer valgono l'elenco `candidates` con BR1/BR2 e `quality.phases: 4`, coerenti con l'esecuzione del banco. I byte originali restano conservati per verificarne gli hash; tali campi descrittivi non selezionano il demosaic né il numero di fasi.

## BR3/BR4: verde e crominanza

Varianti RAW con entrambi gli stadi RI/MLRI e media dei coefficienti locali, secondo la struttura matematica discussa da Jin et al. (§5 e appendice A). Codice proprio, senza importazione del software IPOL; non si rivendica equivalenza numerica alla loro implementazione. Scelte esplicite: float64 intermedio, epsilon 1e-6 in scala lineare, nessun clamp e bordo riflesso senza ripetizione del pixel terminale, che conserva la parità CFA. I siti acquisiti vengono riassegnati esattamente.

Separare R, verde sulla riga R, verde sulla riga B e B. Per ciascun asse costruire la guida bilineare a distanza uno; ogni piano viene predetto dalla componente adiacente sull'asse, con fit sui soli siti originali della componente bersaglio. BR3 usa covarianza/varianza con raggio cinque lungo l'asse. BR4 usa prodotti dei Laplaciani a passo due lungo l'asse in una finestra di raggio tre per entrambi gli assi. L'intercetta conserva le medie mascherate. Mediare entrambi i coefficienti sulla medesima finestra prima della previsione; interpolare i residui dei siti acquisiti a distanza uno.

Le differenze G−R/G−B risultanti alimentano una fusione su quattro direzioni. Gradiente centrato a distanza uno, smoothing gaussiano separabile raggio quattro e sigma uno; peso inverso del gradiente al quadrato più epsilon, valutato a distanza due per BR3 e uno per BR4. Ogni stima usa pesi dal centro verso l'esterno [0,26; 0,24; 0,21; 0,17; 0,12]. Sommare la differenza al campione R/B per ricostruire G. Infine stimare R/B con G come guida, raggio cinque su entrambi gli assi, stessa scelta RI/MLRI, Laplaciano a croce con passo due per BR4 e correzione bilineare dei residui.

Il protocollo Bayer v2 conserva sviluppo, categorie e soglie del v1, ma fissa nuovi parametri di verifica prima delle misure. Gli stadi completi non implicano qualifica della qualità: occorrono i gate sintetici, verifica fotografica separata e limiti di memoria/prestazioni prima dell'integrazione. Il laboratorio usa buffer a fotogramma intero e non costituisce il piano memoria del prodotto.

## BR5: intervento subordinato ai campioni acquisiti

BR5 miscela il BR3 congelato con la baseline usando soltanto il mosaico per stimare la confidenza. Per ogni cella 2×2 sovrapposta, raccogliere R, media dei due G e B, prodotti incrociati e quadrato della differenza dei verdi. Sommare i momenti nella finestra 9×9 troncata ai bordi. Richiedere almeno 32 celle e varianza di ogni canale superiore a 1e-10 per cella. Il minimo delle tre correlazioni Pearson **con segno** alimenta una transizione smoothstep da 0,70 a 0,95; correlazioni negative non autorizzano l'intervento.

Moltiplicare per la confidenza direzionale: tensore dei gradienti centrati a passo due, quindi dello stesso colore CFA, sulla medesima finestra; almeno 16 gradienti validi, energia sopra 1e-10 per osservazione e smoothstep della coerenza da 0,55 a 0,85. Un terzo fattore decresce da uno a zero quando il rapporto fra energia di disaccordo dei due verdi e quattro volte la loro varianza passa da 0,05 a 0,25. Peso finale nel dominio [0,1]; nessun clamp dei valori RGB. Peso zero conserva esattamente i canali mancanti della baseline; siti acquisiti sempre ripristinati bit per bit. Saltare BR3 quando tutti i pesi sono zero.

Non è una garanzia teorica di assenza di aliasing: le osservazioni dei canali non sono co-localizzate. Il protocollo Bayer v3 mantiene sviluppo e soglie, con nuovi parametri di verifica distinti da v1/v2. Nessun uso della verità sintetica per calcolare il peso; la promozione richiede comunque tutti i gate.


## `dir2` — superfici coerenti ai bordi obliqui o curvi

La base direzionale a tre righe resta invariata. Soltanto vicino a una variazione della stessa fase a distanza due, oltre il 10% della scala con pavimento 0,05, provare il modello a due superfici 13×13 definito in [DR7](demosaic-xtrans-experimental.md#dr7--bordi-confermati-dai-campioni). Per Bayer si escludono le maschere costanti lungo un asse: la costruzione direzionale continua a trattare i gradini orizzontali/verticali, compresi quelli con posizione non identificabile dopo clipping. Non applicare il fit in presenza di clipping privo di supporto entro la finestra. La sostituzione riguarda esclusivamente i canali mancanti e precede la matrice colore; valori misurati, negativi e headroom restano conservati.

Le matrici del fit e la maschera sono locali allo stack; la riserva Bayer diventa `larghezza × (3*sizeof(f32) + sizeof([usize;4]) + 52*6*sizeof(f32)) + sizeof(RangeRows) + 8 KiB`, oltre alla mappa HL 6×6. Non c'è un raster verde o RGB supplementare a fotogramma intero. Le sonde storiche BR1–BR5 continuano a usare il riferimento `dir1`, congelato nei test, e non ereditano la nuova base per cambiare retroattivamente i risultati.
