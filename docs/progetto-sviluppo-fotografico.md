# Sviluppo fotografico non distruttivo

Specifica della futura estensione richiesta dal titolare il 23 settembre 2026: regolare una fotografia con un insieme di strumenti paragonabile, per categorie d'uso, a Lightroom e Camera Raw. Piano di consegna, caselle operative e verifiche risiedono soltanto in [STATO.md](../STATO.md#piano-sviluppo-fotografico). I nomi, le scale e gli algoritmi seguenti sono scelte di TrueRenderer; non promettono equivalenza numerica con altri programmi.

## ADR 0011 — ricetta fotografica reversibile e condivisa

### Perimetro e rapporto con il viewer

L'architettura originale escludeva l'editing in §2.2 e rinviava profili creativi, Lensfun e soft proof. Questa estensione introduce il relativo progetto, separato dai gate del viewer R0–R4: la pianificazione non rende disponibili i controlli né chiude quei gate. All'attivazione delle rispettive fasi, questo contratto integra §§3.2, 6.4, 7.4–7.5 e 13 dell'[architettura](TrueRenderer-Architettura.md), senza riscriverne la baseline storica. Le correzioni e i look diventano operazioni esplicite nella ricetta, con versione e provenienza.

Il prodotto conserva gli originali in sola lettura. L'utente modifica una **versione della fotografia**, può sempre tornare allo sviluppo iniziale e ottiene la stessa ricetta in viewer, confronto, miniature ed export. Modifiche fotografiche, valutazioni/keyword, preferenze dell'app e qualità Standard/Piena rimangono quattro stati distinti. La qualità della vista non modifica i parametri fotografici.

Il flusso iniziale riguarda i RAW già ammessi dai rispettivi motori e JPEG/PNG/TIFF con colore supportato. Un formato non acquisisce supporto solo perché esiste il pannello Sviluppo. FITS conserva il suo percorso scientifico in sola lettura: WB, ottica e look fotografici non vengono applicati ai campioni scientifici o confusi con lo stretch di visualizzazione. HDR/EDR del display rimane una qualifica separata dalla capacità di conservare valori RGB oltre 1.

### Decisioni di progetto

1. Ricetta immutabile per revisione, salvata nella libreria durevole, con motore RAW e dipendenze risolti. Ogni nuova immagine parte da una ricetta identità rispetto allo sviluppo attuale; nessun Auto, look, denoise, sharpening o profilo ottico aggiunto implicitamente.
2. Un grafo comune ai render della vista e dell'esportazione. Riferimento numerico CPU fp32; accelerazione GPU solo per nodi qualificati. Questo riferimento interno non attribuisce il modo di assurance pubblico «Riferimento».
3. Regolazioni globali, locali e ottiche hanno stadi dichiarati. L'ordine non dipende dall'ordine dei clic; la prima versione non offre il trascinamento arbitrario dei moduli.
4. Working RGB Rec.2020/D65 esteso fp32, senza clamp generale a [0,1]. Operazioni tonali o percettive possono usare un dominio matematico dichiarato e ritornare al working lineare. Ricampionamento e compositing restano in luce lineare con alpha premoltiplicata. Questa è l'estensione esplicita dell'invariante §3.2 alle trasformazioni creative, non una gamma implicita nel renderer.
5. Gli strumenti che richiedono dati del sensore o calibrazione non vengono simulati con uno slider RGB presentato come equivalente. Il motore dichiara capacità, operazioni già eseguite e limiti.
6. Nessuna ricetta salva percorsi arbitrari da aprire nel worker: profili, maschere e altri asset passano dal broker come contenuti validati e identificati per hash. Rimangono XPC/App Sandbox su macOS e il confinamento Windows; nessun fallback pipe per esterni Mac.

## Riferimenti applicativi esaminati

Documentazione primaria consultata il 23 settembre 2026; confronto delle funzioni documentate, senza una prova comparativa dei rispettivi software. Le fonti spiegano i prodotti citati; le decisioni nelle sezioni successive sono la proposta TrueRenderer.

| Riferimento | Indicazione utile al progetto |
|---|---|
| [Lightroom desktop: strumenti di modifica](https://helpx.adobe.com/lightroom/desktop/edit-photos/edit-photos.html) | Organizzazione in luce, colore, effetti, dettaglio e ottica; regolazioni selettive del colore, versioni e copia delle impostazioni. Texture e chiarezza sono controlli distinti. |
| [Camera Raw: colore e tono](https://helpx.adobe.com/camera-raw/desktop/using/make-color-tonal-adjustments-camera.html) | WB Kelvin sui RAW e relativo sui file già sviluppati; esposizione in stop, curve e controlli per intervalli tonali. Il controllo Adobe chiamato Brightness è descritto per processi precedenti, non come componente universale del pannello moderno. |
| [Capture One: luminosità](https://support.captureone.com/hc/en-us/articles/360002609938-Adjusting-brightness-in-the-Exposure-tool) | Un comando separato per agire principalmente sui mezzitoni: motivazione per mantenere Luminosità oltre a Esposizione, come richiesto. |
| [Lightroom: maschere](https://helpx.adobe.com/lightroom/desktop/edit-photos/masking.html) | Pennelli, gradienti e selezioni per intervalli, con aggiunta, sottrazione, intersezione e inversione; le selezioni automatiche sono una famiglia ulteriore. |
| [Lightroom: ritaglio e geometria](https://helpx.adobe.com/lightroom/desktop/edit-photos/crop-rotate-geometry.html) | Ritaglio, raddrizzamento, prospettiva guidata, trasformazioni manuali e vincolo all'area valida. |
| [darktable 5.6: correzioni obiettivo](https://docs.darktable.org/usermanual/5.6/en/module-reference/processing-modules/lens-correction/) | Correzioni da metadati o Lensfun, parametri ottici e indicazione delle correzioni realmente applicate. La correzione cromatica ripetuta può produrre sovracorrezione. |
| [darktable 5.6: ordine della pipeline](https://docs.darktable.org/usermanual/5.6/en/darkroom/pixelpipe/the-pixelpipe-and-module-order/) | Le anteprime per regioni o risoluzioni ridotte possono divergere dall'export, soprattutto con filtri locali: serve una verifica finale con la stessa catena. |
| [Lensfun: progetto e licenze](https://github.com/lensfun/lensfun) | Candidato per distorsione, aberrazione laterale e vignettatura; librerie LGPL-3.0, applicazioni GPL-3.0 e database CC BY-SA 3.0 hanno regimi distinti. Versione, integrazione e redistribuzione richiedono una decisione dedicata. |

## Esperienza d'uso

Nel viewer il pannello destro contiene le schede **Sviluppo / Informazioni**. Sviluppo espone istogramma, ricetta di partenza e sezioni richiudibili; il pannello Esplora/Libreria e la filmstrip conservano la navigazione. Una barra di strumenti dà accesso a ritaglio, maschere e ritocco. In finestra compatta il pannello diventa richiamabile senza coprire permanentemente il soggetto; preferenze generali e regolazioni fotografiche non condividono la stessa finestra.

Ordine dei pannelli: **Profilo e WB → Luce → Curve → Colore → Presenza → Dettaglio → Ottica → Geometria → Effetti**. Maschere, Ritocco, Preset e Versioni sono spazi richiamabili. Questo ordine favorisce l'uso, mentre l'ordine di calcolo è quello del grafo descritto sotto. Vista essenziale con controlli principali e vista completa con opzioni avanzate, ricerca dei controlli e preferiti.

- Ogni slider ha nome, valore editabile, unità e reset; doppio clic sul nome ripristina il valore iniziale della ricetta. Frecce e modifica fine da tastiera, focus visibile e descrizioni IT/EN. I limiti dipendenti da unità e formato sono validati anche fuori dalla UI.
- Ogni sezione ha attivazione, bypass temporaneo, reset e indicazione di modifiche. Bypass di consultazione non aggiunge voci alla cronologia; disattivare una sezione salvandone lo stato è invece una modifica.
- **Prima/Dopo** confronta, a scelta, sviluppo iniziale, stato all'ingresso o snapshot scelto, con zoom/pan collegati. Per RAW «prima» significa sviluppo iniziale del motore, non JPEG della fotocamera; quest'ultimo, se disponibile, è una vista distinta. Per confrontare il solo colore si può mantenere la geometria corrente su entrambi i lati, dichiarandolo.
- **Annulla/Ripeti**, snapshot nominati, versioni virtuali, reset totale e ripristino selettivo dei pannelli. Una versione virtuale condivide la sorgente e conserva una ricetta indipendente.
- Istogramma e avvisi distinguono sorgente disponibile, working modificato e uscita selezionata. Un canale del sensore saturo è diverso da un valore ritagliato dall'export. Gli istogrammi da proxy sono indicati come approssimati; il campionatore conserva stadio e coordinate.
- L'utente vede «Calcolo…», «Anteprima provvisoria», «Salvataggio…» o «Salvato» soltanto quando pertinenti. Informazioni su hash, algoritmo e driver stanno nella diagnostica della ricetta, non accanto a ogni slider.
- Cambiare foto conclude il gesto e richiede la persistenza della revisione. Se il salvataggio fallisce, la bozza resta recuperabile con Riprova/Esporta ricetta/Annulla modifiche; nessun «Salvato» anticipato né scarto silenzioso.

## Catalogo delle regolazioni

Le scale sono iniziali e specifiche di TrueRenderer. Prima di congelare la versione del processo vanno provate sul corpus; cambiarne il significato richiederà una nuova versione, non la reinterpretazione dei vecchi numeri. Zero è neutro salvo selettori, parametri geometrici identità e valori esplicitamente indicati. Il reset torna alla baseline salvata della foto, compreso un eventuale preset iniziale scelto dall'utente; il comando «Sviluppo originale» elimina anche quel preset.

### Profilo, bilanciamento del bianco e calibrazione

| Controllo | Contratto proposto |
|---|---|
| Motore RAW | Motore disponibile per piattaforma e sorgente, salvato nella versione. Una foto già modificata non cambia resa al cambiare della preferenza globale. Cambio motore sulla foto con anteprima e nuova revisione/versione. |
| Profilo tecnico | Calibrazione fotocamera→working identificata e versionata. Il profilo tecnico non è un look creativo e non è il profilo del monitor. DCP/ICC fotocamera entrano soltanto con parser, matching e corpus qualificati. |
| Profilo creativo | Neutro predefinito; look originali colore/monocromia e LUT importabili con dominio, trasformata e diritti noti. Intensità 0–200%, 100% nominale. Il risultato non modifica numericamente gli altri slider. |
| WB | Come scattato, automatico esplicito, contagocce e personalizzato; preset luce diurna, ombra, nuvoloso, tungsteno, fluorescente e flash solo dove interpretabili. La scelta salva parametri risolti, non un Auto da ricalcolare a ogni apertura. |
| Temperatura RAW | 2.000–50.000 K, corsa non lineare e inserimento numerico; valore fisico solo se l'adattatore dispone di calibrazione e trasformazione qualificata. «Come scattato» resta valido anche quando non si può stimare un Kelvin attendibile. |
| Tinta RAW | Scala UI −150…+150 lungo verde↔magenta; conversione interna versionata verso il modello di neutralità/illuminante. Non è un'unità fisica universale né una scala intercambiabile con Adobe. |
| Temperatura/tinta RGB | −100…+100 relativi al render per JPEG/TIFF/PNG, WB locale e RAW senza controllo nativo qualificato. Etichetta «Correzione colore del render», senza Kelvin fittizi. |
| Contagocce | Area 1×1, 5×5 o 11×11 a coordinate native; media robusta, esclusione alpha nullo e campioni saturi/non validi. Mostra impossibilità o incertezza quando mancano campioni utili. |
| Calibrazione avanzata | Matrici/profili e neutralità tecnica separati da variazioni creative di tonalità/saturazione delle primarie. Import di profili controllato, confronto su target; nessun cursore nascosto per cambiare nero/bianco del sensore. |

WB nativo cambia lo sviluppo nel decoder: moltiplicatori e stadio dipendono dal motore. Un adattamento cromatico applicato a RGB già sviluppato non può annullare clipping o ricostruire la risposta spettrale della fotocamera. L'interfaccia distingue queste capacità, coerentemente con la [distinzione RAW/RGB documentata da Camera Raw](https://helpx.adobe.com/camera-raw/desktop/using/make-color-tonal-adjustments-camera.html). Una tinta locale resta una correzione del render e non forza un nuovo demosaic.

**Primo contagocce del processo 1.** «Neutralizza campione RGB» usa l'ultimo pixel opaco campionato nella vista modificata a risoluzione nativa, in coordinate Rec.2020 lineari, e modifica solo temperatura/tinta relative del render. Richiede saturazione della ricetta pari a zero e componenti RGB deassociate nell'intervallo `[0,02; 0,95)`; un campione scuro, trasparente, saturo, fuori scala o una correzione oltre ±100 viene rifiutato senza cambiare la ricetta. Per il campione `r,g,b` già reso aggiorna `temperatura ← temperatura − 100 ln(r/b)/0,36` e `tinta ← tinta + 100 ln(g/√(rb))/0,21`, invertendo i guadagni cromatici del processo 1. L'azione entra nella cronologia reversibile. È un'operazione su un singolo pixel, non una stima di Kelvin né un WB RAW: media robusta 5×5/11×11, analisi di incertezza e adattatori WB nativi restano gate di SF2.

### Luce e curve

| Controllo | Scala iniziale | Effetto e vincoli |
|---|---|---|
| Esposizione | −10…+10 EV; passo fine 0,05 | Moltiplica RGB lineare per `2^EV`, preservando alpha e segno. Non cambia i metadati di scatto. Non ricostruisce canali già tagliati nel sensore o dal decoder. |
| Luminosità | −100…+100 | Curva monotona concentrata sui mezzitoni, con ancoraggi alle estremità e raccordi morbidi. Non equivale a moltiplicare tutta l'immagine né ad aggiungere una costante RGB. |
| Contrasto | −100…+100 | Espansione/compressione intorno a un pivot; default 0,18 di luminanza lineare. Pivot regolabile nelle opzioni avanzate; scala e risposta versionate. |
| Alte luci / Ombre | −100…+100 ciascuno | Rimodellamento progressivo dei rispettivi intervalli, con protezione dei bordi e della cromia. La soglia non segue zoom o viewport. |
| Bianchi / Neri | −100…+100 ciascuno | Regolano gli estremi tonali dello sviluppo. Anteprima del clipping d'uscita; separati dai livelli fisici nero/bianco usati per leggere il mosaico. |
| Recupero RAW | Spento / metodo supportato; intensità 0–100 | Nodo nativo con capacità dichiarata. Distingue conservazione di headroom e ricostruzione di canali mancanti; quest'ultima è un'interpretazione e va segnalata. Nessuna promessa su saturazione di tutti i canali. |
| Compressione dinamica | Spenta / Fotografica; intensità e punto di bianco | Mappa esplicita delle alte luci verso la destinazione SDR, con controllo del contrasto e della cromia. Nessun tone mapping applicato soltanto dal viewer e assente nell'export. |
| Auto luce | Azione reversibile | Analizza il frame con regola e campionamento fissi; propone esposizione/toni e mostra i valori. WB, geometria e look cambiano solo con le rispettive azioni Auto. |
| Curva parametrica | Ombre, scuri, chiari, alte luci; tre separatori | Quattro intervalli con raccordi continui e risposta monotona per il tono base. Separatore ordinato, nessuna divisione per intervalli vuoti. |
| Curva a punti | Luminanza, RGB composita, R/G/B | Aggiunta/rimozione punti, coordinate numeriche, strumento mirato e reset per canale. Interpolazione monotona per segmenti senza overshoot; dominio percettivo dichiarato ed estensione continua fuori dal range mostrato. Curve creative invertenti ammesse solo come scelta esplicita. |
| Livelli | Nero, mezzo, bianco; uscite separate | Vista avanzata della trasformazione tonale; istogramma per canale. Le regolazioni non sovrascrivono i livelli RAW e non si sommano a una seconda funzione identica nascosta. |

La prima implementazione tonale usa funzioni puntuali deterministiche per esposizione, luminosità, contrasto e raccordi di ombre/luci. Le varianti adattive che dipendono dal contesto aggiungono un nodo esplicito, analisi globale congelata e halo dichiarato. Per evitare divergenze cromatiche, la curva di luminanza ricostruisce il colore con una regola documentata vicino allo zero; divisioni per luminanze piccole, valori negativi e oltre 1 hanno fixture specifiche. Le curve RGB restano una scelta creativa che può cambiare anche la cromia.

**Processo fotografico 1.** La prima versione eseguibile usa coordinate Rec.2020 lineari. Dopo la deassociazione dell'alpha applica esposizione `2^EV` e guadagni RGB relativi `exp(0,18t+0,07m)`, `exp(-0,14m)`, `exp(-0,18t+0,07m)`, con `t=temperatura/100` e `m=tinta/100`: sono controlli del render, non kelvin o WB nativo. Sulla luminanza `Y=0,2627R+0,6780G+0,0593B`, per `0<Y≤1`, luminosità, ombre, luci, neri e bianchi applicano in quest'ordine `Y ← Y + (a/200)Y(1-Y)w`, con pesi rispettivi `1`, `(1-Y)^2`, `Y^2`, `(1-Y)^4`, `Y^4`. Il contrasto usa `Y ← 0,18·(Y/0,18)^exp(c/100)` per `Y>0`; i valori negativi e presso zero non sono divisi per luminanza. La curva iniziale è una spezzata monotona sulla luminanza **lineare** fra gli ancoraggi (0,0) e (1,1), continua per identità fuori dal dominio. Si ricostruisce RGB moltiplicando per `Y_nuovo/Y_vecchio` quando `Y_vecchio>10⁻⁸`, quindi si applica saturazione `Y+(RGB-Y)(1+s/100)`. Alpha resta identica; zero di tutti i controlli evita ogni riarrotondamento. Cambiare queste formule richiede una nuova versione di processo e una migrazione esplicita, non una reinterpretazione delle ricette salvate.

La vista rapida del processo 1 può applicare le funzioni al primo mip residente di lato massimo 1024: è marcata **provvisoria** perché una funzione tonale non commuta con la riduzione. Griglia, filmstrip e anteprima dell'ispettore usano un mip fino a 512 px; l'istogramma dell'ispettore misura quella anteprima modificata e ne dichiara il livello, non pretende di rappresentare l'export nativo. Il calcolo delle miniature è asincrono e limitato da crediti di memoria e numero di risultati residenti; durante il calcolo mostra esplicitamente la sorgente e un risultato fallito non viene rilanciato per ogni fotogramma. «Verifica resa finale» richiede il raster nativo e costruisce un'anteprima d'uscita **sRGB16 PNG/TIFF a dimensioni native**: dopo la ricetta, su una copia privata, applica la stessa trasformata, clamp e quantizzazione dell'encoder, ricostruisce il working lineare premoltiplicato e soltanto allora costruisce i mip. Il render esteso e la ricetta non vengono tagliati. Il percorso non usa un intermedio a 8 bit; l'alpha quantizzata determina la policy di filtro della copia. La modalità resta attiva durante le regolazioni; una sorgente ridotta o un risultato di un'altra modalità non possono soddisfarla. Può rifiutare il calcolo se non ci sono crediti di memoria. «Torna al render esteso fp32» ripristina ispezione working e contagocce: i campioni già proiettati in uscita non alimentano la neutralizzazione RGB. La prova d'uscita non simula JPEG, resize export, DNG o TIFF float32, né certifica calibrazione/display o fedeltà rispetto alla scena. JPEG, PNG e TIFF applicano la ricetta allo sviluppo nativo prima del resize; DNG lineare e DNG RAW conservano lo sviluppo tecnico e il mosaico rispettivamente, senza incorporare le regolazioni creative.

### Colore

| Gruppo | Controlli e comportamento |
|---|---|
| Colore globale | Saturazione e Vividezza −100…+100. Saturazione agisce sulla cromia; vividezza pesa di più i colori poco saturi e offre protezione incarnato esplicita, senza garantire riconoscimento universale della pelle. |
| Mixer HSL | Otto fasce iniziali: rosso, arancio, giallo, verde, acquamarina, blu, viola e magenta; tonalità, saturazione e luminanza −100…+100. Pesi continui tra fasce, gestione circolare della tonalità e strumento mirato sulla foto. |
| Colore selettivo | Campioni con centro di tonalità/cromia/luminanza, ampiezza e sfumatura; variazioni H/S/L e sovrapposizione della zona interessata. Fino a 16 campioni nella prima versione; ordine stabile in caso di sovrapposizione. |
| Color grading | Ruote globale, ombre, mezzitoni e alte luci; tonalità 0–360°, intensità 0–100, luminanza −100…+100, bilanciamento −100…+100 e fusione 0–100. Pesi delle zone ricavati da uno stadio fisso. |
| Bianco e nero | Conversione reversibile con mixer degli otto colori, contagocce mirato e viraggio tramite grading. I dati a colori rimangono nella sorgente e nella ricetta, senza riscrittura in scala di grigi. |
| Gamut | Visualizzazione delle escursioni e compressione opzionale verso la destinazione, separata da saturazione. Nessun clamp nascosto nel mixer per far sparire componenti negative. |

Il dominio percettivo scelto per il colore deve definire conversione e ritorno per RGB estesi, neutri, neri e valori negativi. Un algoritmo che richiede solo RGB positivi non riceve un clamp generalizzato come scorciatoia: va qualificata una trasformazione d'estensione o dichiarata la capacità limitata del nodo. Questo punto è un gate tecnico prima dell'attivazione dei controlli avanzati.

### Presenza, dettaglio ed effetti

| Controllo | Scala iniziale | Semantica |
|---|---|---|
| Texture | −100…+100 | Attenuazione/enfasi di frequenze intermedie: trama di tessuti, foglie, pelle. Decomposizione multiscala, protezione del rumore e dei bordi forti; DC costante preservata. |
| Chiarezza | −100…+100 | Contrasto locale a scala più ampia, soprattutto nei mezzitoni; protezione degli estremi e limitazione degli aloni. Non è il controllo di nitidezza. |
| Rimozione foschia | −100…+100 | Positivo riduce il velo stimato, negativo aggiunge foschia; stima di luce atmosferica/transmissione limitata e protezione da rumore/dominanti. Intensità zero dà identità; stima insufficiente segnalata, non venduta come ricostruzione della scena. |
| Nitidezza di sviluppo | Quantità 0–150; raggio 0,3–3 px nativi | Raggio, dettaglio 0–100, mascheratura 0–100 e soglia rumore. Vista maschera e confronto 1:1; nessuna nitidezza implicita nel ricampionamento. |
| Rumore luminanza | Quantità, dettaglio, contrasto 0–100 | Filtro multiscala con protezione dei bordi e compromesso dettaglio/rumore visibile. Scala riferita alla sorgente; nessuna garanzia di recupero del dettaglio rimosso. |
| Rumore cromatico | Quantità, dettaglio, uniformità 0–100 | Riduce macchie cromatiche preservando bordi e neutri. Indipendente dal rumore di luminanza; non scambiato per rimozione delle aberrazioni ottiche. |
| Moiré / falsi colori | Intensità 0–100; preferenza locale | Trattamento mirato con maschera consigliata e controllo del dettaglio. Non corregge ogni aliasing del sensore. |
| Pixel difettosi | Mappa tecnica o rilevamento qualificato | Dipende da dati/capacità RAW; separato da polvere e timbro. Si conserva la provenienza del rilevamento. |
| Vignettatura creativa | Quantità −100…+100; centro, raggio, rotondità, sfumatura, protezione luci | Effetto riferito al ritaglio finale, distinto dalla compensazione fisica della caduta di luce dell'obiettivo. |
| Grana | Quantità/dimensione/irregolarità 0–100; monocromatica o cromatica | Effetto deterministico, con seed salvato e coordinate della fotografia; nessuno sfarfallio al pan e nessuna cucitura tra tile. |
| Nitidezza d'uscita | Spenta / schermo / stampa; intensità | Nodo dell'export dopo il resize, distinto dalla nitidezza di sviluppo. Un'anteprima d'uscita dedicata permette di controllarlo alla dimensione scelta. |

Le prime implementazioni di Texture e Chiarezza partono da un comune banco di filtri a scale diverse con protezione dei bordi, ma hanno pesi e risposte indipendenti. La foschia richiede uno spike separato prima di scegliere e versionare l'algoritmo. Nessun parametro viene presentato come copia di un algoritmo proprietario. Denoise precede l'enfasi dei dettagli; ogni stadio dichiara raggio massimo e dipendenza dalla risoluzione. Una foto uniforme, una rampa e un bordo sintetico devono distinguere i tre strumenti di presenza.

### Ottica: distorsione, vignettatura e aberrazioni

Il pannello mostra corpo, obiettivo, focale reale in mm, apertura e distanza di fuoco se disponibili, con origine dei dati e possibilità di correzione manuale della **ricetta**, senza cambiare EXIF originali. Identificatore numerico e nome commerciale non bastano da soli: duplicati di LensID, adattatori, moltiplicatori, crop del sensore e modalità RAW/JPEG possono cambiare l'associazione.

| Controllo | Progetto |
|---|---|
| Sorgente di correzione | Nessuna aggiunta / metadati incorporati qualificati / profilo selezionato / manuale. «Automatico» è un'azione che risolve la scelta e la salva, non una ricerca ripetuta contro un database mutevole. |
| Distorsione da profilo | Attivazione e intensità 0–200%, 100% nominale. Modello e intervallo di calibrazione dichiarati; interpolazione solo dove il profilo la permette. |
| Distorsione manuale | Barilotto/cuscinetto −100…+100; modello radiale avanzato per distorsione a baffo, centro ottico e parametri tangenziali solo nel pannello avanzato. Limiti sulla mappa per evitare pieghe e trasformazioni singolari. |
| Aberrazione cromatica laterale | Attivazione da profilo o stima separata, con regolazione fine R/G e B/G; modello di spostamento/scalatura dei canali nel dominio qualificato. Sui RAW si privilegia il mosaico/RGB fotocamera prima della matrice colore. Proposta manuale fino a ±10 px al bordo, normalizzata alle dimensioni native e validata sulla mappa. |
| Frange viola e verdi | Quantità 0–100 per famiglia, intervallo cromatico, soglia sui bordi, contagocce e maschera di protezione. Soppressione mirata dei residui; non sostituisce la riallineatura geometrica dei canali. |
| Aberrazione longitudinale | Defringe può attenuare il colore delle frange davanti/dietro il piano a fuoco, ma non ripristina la messa a fuoco dei canali. Deconvoluzione ottica solo in un'estensione con PSF e dati qualificati. |
| Vignettatura ottica | Attivazione/intensità 0–200% da profilo; compensazione manuale fino a +4 EV ai bordi, con centro, raggio e raccordo. Guadagno massimo limitato e rumore risultante verificato. |
| Proiezione | Rettolineare nella prima versione. Fisheye→rettolineare e altre proiezioni richiedono modello/campo visivo compatibili e gate successivo. Non si corregge un fisheye con coefficienti rettolineari arbitrari. |
| Area valida | Mostra bordi mancanti e ritaglio automatico facoltativo. Trasparenza dove disponibile, oppure fondo scelto e dichiarato all'export; nessun riempimento generativo implicito. |

**Stato delle correzioni già applicate.** Il decoder deve consegnare, separatamente per distorsione, CA e vignettatura: `non applicata`, `applicata rimovibile`, `applicata obbligatoria` o `non determinabile`, più stadio/profilo/crop. Una capacità dichiarata dall'API non prova da sola che tutti i modelli si comportino allo stesso modo. Se una correzione è già eseguita, non si aggiunge automaticamente la stessa. Se è obbligatoria, la UI non offre un falso originale ottico. Se lo stato è ignoto, niente correzione automatica cumulativa; il manuale resta una scelta esplicita con confronto.

Il percorso Apple corrente richiede `lensCorrectionEnabled=NO`, mentre le ricette LibRaw fissano altri parametri. Questo è un punto di partenza da verificare per fotocamera, non la prova che ogni risultato sia privo di qualunque trasformazione ottica. I [limiti RAW esistenti](progetto-motori-raw.md) e gli opcode obbligatori di §7.5 continuano a valere.

**Profili e associazione.** Metadati incorporati qualificati hanno priorità solo quando applicabilità e stato del decoder sono noti; in alternativa si propone il profilo esatto, poi una scelta manuale. Mancanza/ambiguità e focali/aperture fuori calibrazione devono restare visibili. Una distanza di fuoco mancante non viene trasformata in un valore misurato. Conservare hash e copia lecita della versione del profilo usata con la ricetta; un aggiornamento del database non cambia automaticamente vecchie foto. L'utente può confrontare e aggiornare una versione volontariamente.

**Dominio della CA laterale.** Riallineare i canali dopo che una matrice li ha mescolati nello spazio Rec.2020 non equivale in generale a correggere i canali del sensore. Il backend deve applicare la correzione nello stadio nativo appropriato oppure consegnare un intermedio calibrato idoneo. Se non lo consente, quella capacità RAW resta non disponibile; una correzione residua sul render è esplicitamente distinta e qualificata sul suo dominio, come per immagini già sviluppate. Non si inverte arbitrariamente la pipeline Apple né si tratta il defringe come sostituto geometrico.

**Lensfun.** Candidato preferito da valutare, senza aggiungerlo automaticamente alla distinta. Confrontare uso della libreria con un adattatore ai dati/formule documentate; fissare versione, calibrazioni, licenze, attribuzioni, distribuzione e comportamento su entrambi gli OS prima della scelta. Una riscrittura della libreria non elimina i diritti sul database. Profili Adobe LCP o del costruttore non sono copiati dalle installazioni concorrenti; eventuale import successivo richiede formato documentato, diritti e validazione. Fonti e obblighi distinti nel [repository Lensfun](https://github.com/lensfun/lensfun).

### Ritaglio e prospettiva

Ritaglio libero, rapporto originale, 1:1, 3:2, 4:3, 5:4, 16:9 e personalizzato; inversione del rapporto, rotazioni di 90°, specchio H/V e raddrizzamento ±45° con due punti. Coordinate normalizzate persistenti e dimensioni finali visibili. Orientamento EXIF è già normalizzato una volta; gli interventi dell'utente sono trasformazioni ulteriori nominate.

Prospettiva verticale/orizzontale, rotazione, proporzioni, scala e offset X/Y; guida con due o quattro linee e proposta automatica reversibile. La prospettiva usa una trasformazione proiettiva, distinta dalla curvatura radiale dell'obiettivo. Homografie singolari, regioni non invertibili e canvas oltre quota sono rifiutati prima dell'allocazione. Scale UI normalizzate −100…+100 non sostituiscono i coefficienti risolti salvati.

Compensazione ottica e trasformazioni geometriche vengono composte in una sola mappa di ricampionamento dove compatibili; una CA nativa può richiedere un passaggio precedente distinto, che non va spostato dopo la matrice colore per risparmiare un filtro. Le mappe per canale conservano una copertura comune conservativa. Il ritaglio viene valutato sulla geometria risultante. Maschere e ritocchi seguono la fotografia quando cambiano orientamento, crop e prospettiva.

### Maschere e ritocco

Maschere iniziali: pennello/gomma, gradiente lineare, radiale/ellittico, intervallo di luminanza e intervallo di colore. Pennello con diametro, durezza/sfumatura, flusso, densità e pressione opzionale della penna; nessun requisito di tavoletta per usarlo. Ogni maschera ha nome, visibilità, intensità, duplicazione e sovrapposizione selezionabile. Combinazioni aggiungi, sottrai, interseca e inverti sono salvate come espressioni limitate, prive di cicli.

Le regolazioni locali comprendono esposizione, luminosità, contrasto, luci/ombre, bianchi/neri, temperatura/tinta relativa, saturazione/colore, texture/chiarezza/foschia, rumore, nitidezza, moiré e defringe secondo capacità del nodo. Il WB nativo, il demosaic e il profilo ottico non vengono rieseguiti per ogni pennellata. Una localizzazione agisce nello stadio del proprio operatore; non è un secondo pannello arbitrario applicato dopo l'immagine finale.

Le maschere geometriche vivono in coordinate della sorgente orientata, prima del crop; il puntatore viene riportato indietro dalla geometria della vista. Quelle per colore/luminanza vengono calcolate su uno stadio fisso dopo WB e prima dei look, oppure su uno snapshot esplicitamente scelto, evitando dipendenze circolari. La combinazione di coperture è definita: aggiunta `1-(1-a)(1-b)`, sottrazione `a(1-b)`, intersezione `ab`, inversione `1-a`; feather e opacità hanno un ordine versionato. Le regolazioni locali si compongono nello stesso ordine stabile per stadio; dove non commutano viene conservato anche l'ordine dei gruppi.

Ritocco tradizionale: rimozione polvere, timbro clone, correttivo con adattamento locale e occhi rossi. Sorgente/destinazione, raggio, sfumatura e opacità sono editabili; coordinate e ordine dei tratti sono salvati, con riproduzione deterministica. Le sorgenti dei tratti leggono uno snapshot definito precedente al ritocco oppure i tratti precedenti, dichiarandolo; nessuna dipendenza circolare. Confronto 1:1 e vista per evidenziare le macchie.

Selezione soggetto/cielo/persone, maschere di profondità e denoise basato su modelli sono estensioni successive. Richiedono modello/versione/licenza, inferenza locale entro quota, fallback manuale e maschera/derivato persistente per riprodurre il risultato. La rimozione generativa, espansione e sfocatura basata su profondità sono funzionalità distinte: non necessarie per consegnare le regolazioni fotografiche richieste, non introdotte come pulsanti attivi senza implementazione.

### Preset, copie e sincronizzazione di più foto

Preset con nome, descrizione, anteprima, versione e selezione dei gruppi da includere; niente percorsi sorgente o informazioni private incorporati per default. Intensità continua soltanto per parametri interpolabili: motore, profilo tecnico e scelta di algoritmo sono discreti. Import/export di ricette TrueRenderer con schema validato e limiti di dimensione; un preset Adobe o un sidecar `crs:` non garantisce lo stesso rendering.

Copia/incolla selettivo di luce, WB, colore, dettaglio, ottica, geometria e maschere. Default del batch: non copiare crop, pennelli, ritocchi, motore o profilo ottico da una foto a un'altra senza selezione esplicita. «Stessi valori» e «Ricalcola Auto per ogni foto» sono due azioni distinte. L'associazione obiettivo si ricalcola per foto, senza applicare il profilo della prima a corpi/ottiche diversi. Anteprima del numero di elementi compatibili, esiti per foto, cancellazione e annullamento del batch per gruppo di modifiche.

Default per fotocamera/ISO e preset all'apertura sono facoltativi e si risolvono solo alla creazione di una nuova versione. Foto già modificate, riapertura della libreria e aggiornamenti dell'app non ricevono nuove impostazioni automaticamente.

## Grafo di elaborazione e domini

Il progetto usa nodi tipizzati, ciascuno con versione, parametri, dominio colore, halo, risorse e regola di invalidazione. Il contratto del singolo decoder prevale su un ordine RAW generico, specialmente per DNG e correzioni obbligatorie. La catena comune dopo il decoder è invece fissata dalla versione del processo.

```text
Sorgente immutabile + ricetta + dipendenze per hash
  → lettura/probe e sviluppo RAW nel worker isolato
      [WB, CA nativa, calibrazione e recupero supportati; operazioni già applicate]
  → verifica del broker → working Rec.2020 fp32
  → correzioni radiometriche ottiche e denoise di base qualificati
  → mappa composta distorsione / geometria + area di crop
      [CA residua sul render solo nel suo percorso esplicito e qualificato]
  → defringe e ritocco
  → correzione cromatica relativa RGB, esposizione e tono
      [globali e contributi locali nel relativo stadio]
  → foschia / texture / chiarezza / nitidezza di sviluppo
  → mixer colore / monocromia / grading / look creativo
  → vignettatura creativa e grana
  → immagine modificata canonica a risoluzione nativa
      ├─ mip canonici modificati → vista → trasformata display
      └─ resize export → nitidezza d'uscita → trasformata output → codifica
```

Il mapping tonale fotografico esplicito appartiene allo stadio Tono; la curva/look di un profilo creativo è un nodo separato dopo il colore. Le dipendenze RAW, per esempio un denoise sul mosaico o una vignettatura imposta da opcode, rimangono nel decoder nello stadio richiesto e non vengono ripetute nella catena comune. Un filtro che richiede un ordine diverso ottiene un nuovo processo, non un riordino silenzioso.

**Alpha e precisione.** Nodi puntuali non lineari operano su colore straight con deassociazione protetta; alpha non subisce EV, curve, ICC o WB. Nodi spaziali e blending usano copertura e colore premoltiplicato in lineare; filtri guidati da luminanza/logaritmi dichiarano la trasformazione della guida e ricostruiscono un risultato lineare. A copertura zero si usa RGB zero nel working. Le maschere di regolazione modulano l'effetto e non modificano implicitamente la trasparenza. Nessun intermedio fotografico uint8, fp16 solo dopo verifica separata, NaN/Inf respinti con errore identificabile.

**Scala e regioni.** Texture, nitidezza, denoise e ritocco fanno riferimento a pixel della sorgente oppure a frazioni della sua diagonale secondo il nodo; il livello della preview non cambia la dimensione estetica dell'effetto. Con trasformazioni geometriche il raggio usa la mappa locale; le implementazioni regionali devono leggere gli halo necessari e conservare gli stessi bordi del calcolo completo. Se il decoder non sviluppa regioni, l'ottimizzazione dei nodi successivi non viene chiamata «decode RAW regionale».

**Operazioni globali.** Auto, istogrammi usati dagli algoritmi, stima foschia e maschere parametriche devono essere deterministici rispetto alla fotografia e alla ricetta, non alla porzione di schermo o all'ordine dei tile. Le analisi lavorano su un campionamento canonico versionato con peso della copertura; risultati e chiavi vengono congelati. Un filtro o una curva non commutano in generale con una riduzione: applicarli al mip già ridotto è soltanto una preview approssimata finché non se ne dimostra l'equivalenza.

## Contratti di dati e persistenza

Struttura concettuale; nomi e schema definitivo da fissare nel primo incremento senza sovraccaricare il campo `annotation` esistente:

La prima migrazione durevole usa `photo_edit_head(asset_id, source_digest, generation, cursor, next_revision, redo)` e `photo_edit_revision(asset_id, revision, parent_revision, source_digest, recipe, created_at)` nella versione 3 di `library.sqlite`. Una revisione iniziale identità viene materializzata alla prima modifica; le revisioni successive sono immutabili. Il cursore e la pila redo, limitata a 200 voci, cambiano con controllo della generazione attesa; una nuova modifica dopo undo crea un ramo nuovo senza riscrivere la storia precedente. Il digest lega ogni ricetta alla sorgente osservata. Questo schema non contiene ancora blob, maschere, preset o versioni virtuali: i loro asset durevoli richiederanno le migrazioni successive descritte sotto.

Nel percorso esterno attuale, il campo storico `source_digest` può contenere il token `unverified:` della scansione, distinto dallo SHA-256 dei byte. Il viewer risolve questo legame soltanto attraverso un risultato del decoder accettato nella generazione corrente, con token, asset e immagine in cache corrispondenti; una modifica della sorgente invalida la generazione e la cache. L'export confronta la ricetta con l'identità effettivamente convalidata dal broker nello snapshot privato. Non basta la presenza del prefisso e non si promuove una stat a hash: il broker verifica token prima/dopo, calcola SHA-256 e conserva entrambi i riferimenti con i byte immutabili. Le revisioni esistenti non vengono riscritte. Restano i limiti del contratto di osservazione esterna, senza garanzia universale contro mutazioni concorrenti non rilevabili dai metadati.

```text
PhotoVersion
  asset_id, version_id, parent_version_id, head_revision
EditRevision
  version_id, revision_id, parent_revision, source_digest
  schema_version, process_version, raw_recipe, parameters
  profile_hashes, lens_resolution, masks, retouch, analysis_results
  dependency_manifest, created_at, change_group_id
ResolvedRawRecipe
  engine, decoder_recipe_version, decoder_identity
  wb_mode, resolved_native_wb, input_profile, native_options
  applied_corrections, source_geometry
DependencyAsset
  content_hash, type, schema_version, byte_count, provenance, license
```

`library.sqlite` conserva versioni, revisioni, preset, riferimenti alle maschere e journal; blob grandi in uno store durevole dedicato, per esempio `var/library-assets/`, incluso in backup/restore. `index.sqlite`, cache mip e render rimangono ricostruibili. Un'eliminazione della cache non deve perdere una pennellata, una ricetta o un profilo necessario a riprodurla. La raccolta dei blob non referenziati è un'operazione durevole separata, successiva a backup e conteggio dei riferimenti di storia/snapshot; non fa parte di «svuota cache».

Pubblicazione dei blob prima della transazione che li referenzia; file scritti atomicamente con digest verificato. Revisioni aggiunte e puntatore corrente aggiornato nella stessa transazione con controllo della revisione attesa. Crash tra blob e commit lascia al più un orfano recuperabile, non un riferimento a byte mai completati. Salvataggi su writer separato dai decoder; migrazione con backup consistente, verifica e prova di ripristino.

Un trascinamento produce una bozza in memoria e un'unica voce logica alla fine del gesto; input da tastiera ravvicinati si raggruppano entro una finestra documentata, inizialmente 500 ms. Il cambiamento può essere mostrato prima del commit ma resta marcato come pendente. Undo/redo agisce sulle revisioni e mantiene il ramo di ripetizione fino a una nuova modifica; snapshot e versioni conservano i riferimenti necessari. La chiusura aspetta il salvataggio entro timeout oppure espone l'esito, senza eliminare una bozza recuperabile.

Validazione proposta: ricetta strutturata ≤1 MiB esclusi blob; fino a 64 gruppi maschera, 256 componenti per gruppo, 100.000 punti di pennello complessivi per versione e 2.000 tratti di ritocco. Curve fino a 32 punti per canale, 16 campioni colore; dimensioni dei raster maschera entro i limiti immagine e i crediti effettivamente disponibili. Limiti iniziali da verificare con usi reali; errore prima dell'allocazione, nessuna troncatura. Parametri non finiti, enumerazioni sconosciute, cicli, path traversal e versioni future non eseguibili vengono rifiutati conservando il documento originale come dato non interpretato.

Le dipendenze salvate includono versioni di nodo e profilo, hash, risoluzione della selezione automatica e identità del decoder. Il decoder Apple dipende dall'OS: registrarlo non permette di reinstallarne magicamente una vecchia versione. Se una dipendenza non è più disponibile, la foto resta consultabile con le anteprime disponibili e stato esplicito; rieseguire con un processo nuovo crea una revisione dopo confronto, senza chiamarla riproduzione identica.

Una sorgente offline mantiene ricette e storia. Se allo stesso percorso compaiono byte diversi, il digest impedisce di applicarvi silenziosamente la vecchia revisione: rilocalizzare la stessa sorgente e trasferire modifiche a una sorgente nuova sono operazioni distinte. Anche il cambio motore può cambiare area attiva e dimensioni, come nei render D750 Apple e LibRaw: crop/maschere/ritocchi si trasferiscono solo con una mappa documentata delle geometrie; altrimenti si conserva la versione precedente e si richiede un riallineamento esplicito nella nuova. Coordinate normalizzate da sole non dimostrano corrispondenza del soggetto.

XMP futuro: mantenere rating/keyword e namespace altrui, import/export di un namespace TrueRenderer versionato e relativo manifest di dipendenze. Nessuna scrittura in `crs:` per fingere interoperabilità Adobe. Il sidecar è una replica della libreria, non l'unico archivio; merge a tre vie e gate round-trip/scrittura sicura dell'architettura restano obbligatori. Il trasferimento di una ricetta tra computer deve includere le dipendenze redistribuibili e segnalare quelle mancanti.

## Motori, richieste asincrone e cache

`RawEngine::recipe()` oggi identifica una ricetta fissa; il progetto aggiunge una descrizione risolta per richiesta, senza modificarne in posto il significato storico. All'inizio le ricette neutre mantengono byte e chiavi compatibili con il percorso esistente oppure usano un namespace nuovo esplicitamente migrato. Nessun parametro fotografico vive come globale mutabile del worker.

La tabella delle capacità distingue per file/motore: WB nativo modificabile, recupero RAW, profilo tecnico selezionabile, headroom/precisione, correzioni ottiche applicate, geometria e crop. Apple, LibRaw bilineare, AHD e TrueRenderer non devono avere artificialmente la stessa tabella. Esporre «non disponibile con questo motore» o una correzione RGB distinta; nessun cambio motore per far funzionare uno slider senza mostrarlo. Le regolazioni comuni a valle possono operare sul loro RGB lineare, ma non recuperano clipping o quantizzazione dei passaggi precedenti.

Chiave concettuale di un derivato:

```text
digest sorgente + identità/crop dello sviluppo RAW + hash ricetta risolta
+ versione processo/nodo + hash dipendenze e analisi
+ stadio + ROI/halo + scala/livello + dominio colore/alpha + qualità effettiva
```

Trasformata display e swapchain restano in una cache di presentazione distinta. La ricetta usa serializzazione canonica, valori finiti e normalizzazione dei valori equivalenti; ordinare le mappe non deve riordinare nodi o maschere semanticamente ordinati. Draft, revisioni confermate, ricette identità e artefatti storici non condividono chiavi ambigue.

Spostare Esposizione invalida il nodo e i suoi discendenti, senza rifare unpack/demosaic se il risultato precedente è ancora disponibile. WB nativo o profilo fotocamera invalidano lo sviluppo a monte; crop/ottica invalidano geometria, coordinate e nodi dipendenti. A memoria insufficiente si ricalcola o si attende; non si conserva obbligatoriamente un fotogramma completo per ogni stadio. Le revisioni transitorie non alimentano una cache SSD illimitata.

Richieste con `asset_id`, `version_id`, revisione/bozza, generazione di navigazione e generazione del dispositivo. Il lavoro obsoleto viene cancellato e non può aggiornare foto, istogramma o esportazione più recenti. Si possono mantenere pixel dell'ultima revisione con etichetta provvisoria, ma non comporre tile di revisioni differenti come immagine completa.

## Prestazioni e parità con l'esportazione

Durante il gesto usare il working/mip disponibile per una risposta provvisoria; al rilascio convergere al grafo canonico. Il comando **Verifica resa finale** calcola la stessa catena dell'export alla risoluzione richiesta, poi riduce per la vista. Un'immagine completa può richiedere sviluppo nativo: la bassa risoluzione della finestra non autorizza a chiamare definitiva un'approssimazione dei filtri non lineari.

Obiettivi iniziali da misurare su hardware dichiarato: feedback UI entro 50 ms; prima risposta fotografica a caldo p95 ≤100 ms per controlli puntuali, ≤200 ms per filtri locali su viewport fisico fino a 2560×1440. I tempi a freddo e la convergenza nativa RAW vengono misurati separatamente, per motore e dimensione; nessun limite prestazionale dichiarato superato per deduzione dai tempi dei vecchi viewer. Servono almeno 100 interazioni per distribuzione p95, cache/carico documentati e timestamp comando→frame; il readback resta una misura distinta dal display fisico.

Un budget condiviso ammette decode, working, intermedi, maschere, halo, analisi, texture, readback ed export. Una D750 da 6032×4032 richiede circa 371 MiB per il solo RGBA fp32: la catena non può trattenere una copia completa per slider. Lavorare per fasce/tile e riusare scratch; il massimo dei raggi e le mappe geometriche sono inclusi nella stima. Export e prima foto visibile hanno priorità dichiarate, lavori in background cancellabili; nessun aumento automatico dei budget dell'utente. RSS/footprint aggregati si misurano separatamente dai crediti.

All'avvio dell'export congelare **per foto** la revisione salvata, il motore, le dipendenze, la geometria e le opzioni di uscita. Un successivo gesto nel viewer non cambia un job già avviato. Risoluzione nativa o resize esplicito, mai cattura schermo o miniatura. Pubblicazione no-clobber, cancellazione e limiti di [ADR 0010](esportazione-precisione-fits.md) restano il contratto di base.

| Uscita | Rapporto con le modifiche |
|---|---|
| JPEG, PNG, TIFF16 | Ricetta fotografica completa, crop, resize e nitidezza d'uscita eventuale; trasformata/tag coerenti. Prima destinazione sRGB; Adobe RGB/Display P3/profili ICC e soft proof solo dopo gate colore dedicato. |
| TIFF float32 | Può contenere il working **modificato** oppure lo sviluppo neutro scelto esplicitamente; etichetta e manifest distinguono i due. RGB lineare esteso come codifica non significa più proporzionalità alla scena dopo curve/look. Si conservano i bit dello stadio scelto, non quelli di un altro ramo. |
| DNG lineare | Conserva il significato di RGB sviluppato del contratto corrente. Prima fase: esporta sviluppo tecnico, senza applicare silenziosamente editing creativo; UI spiega la differenza e propone TIFF/PNG/JPEG per la resa modificata. Un DNG lineare con editing incorporato richiede un'estensione normativa e prove di round-trip proprie. |
| DNG RAW | Conserva il mosaico e la calibrazione previsti da ADR 0010; WB creativo, crop, curve, ottica e maschere non vengono cotti nei campioni. Esportazione eventuale della ricetta come file separato esplicito. |

La parità riguarda lo stesso stadio e la stessa destinazione: per PNG/TIFF lossless confrontare dopo riapertura/tag; per JPEG considerare la compressione. Per un export con resize/sharpen dedicati si confronta la relativa anteprima d'uscita, non la vista fotografica a una scala diversa. Dither d'uscita, se introdotto, ha seed/versione e viene eseguito una volta soltanto, con contratto comune a viewer/export.

## Ripartizione nei moduli esistenti

| Area | Responsabilità da introdurre |
|---|---|
| `crates/tr-core` | Tipi e validazione `EditRecipe`, versione processo, nodi CPU, domini/alpha, modelli geometrici e maschere; estensioni dei contratti decoder/protocollo/export. Nuovi moduli dedicati, senza accumulare tutta la catena in `color.rs`. |
| `crates/tr-worker` e adattatori nativi | Parametri RAW risolti, capacità e provenienza ottica, lettura profili non fidati, esecuzione del grafo per export e worker di calcolo confinato. Nessun accesso libero a profili per path. |
| `crates/tr-store` | Migrazioni della libreria, revisioni, versioni virtuali, preset, dipendenze durevoli, backup/restore e scritture concorrenti. Conservare separazione dall'indice e dalle annotazioni. |
| `crates/tr-render` | Valutazione a tile/ROI, residenza e invalidazione per nodo, kernel GPU qualificati, preview provvisoria/finale e stessa trasformata d'uscita dichiarata. |
| `apps/desktop` | Stato bozze/gesture/salvataggio, pannello Sviluppo, strumenti, cronologia, batch e integrazione `photo_export`; generazioni e i18n/accessibilità. |
| Cache e strumenti di verifica | Namespace ricetta/stadio, invalidazione, monitor memoria, harness di editing e confronto numerico vista/export. Rapporto con build, hash, ricette e perimetro. |

## Matrice di accettazione

Questi sono requisiti di verifica futura, non risultati ottenuti. Le soglie iniziali si congelano con la prima versione del processo e si riportano per algoritmo, formato, motore e piattaforma; il confronto fra decoder diversi non usa una soglia CPU/GPU come misura di fedeltà fotografica.

| Area | Prova e criterio di uscita |
|---|---|
| Identità | Tutti i nuovi nodi disattivati conservano bit del working e pixel del percorso baseline nella medesima build. Ricette precedenti, profili e motore globale non cambiano foto già salvate. |
| Esposizione e toni | Campioni fp32 compresi fra negativi e alte luci: +1 EV raddoppia e −1 dimezza entro arrotondamento; alpha invariato. Rampe senza inversioni indesiderate, continuità agli ancoraggi, nessun NaN/Inf o clamp non dichiarato. |
| WB e colore | Neutri sintetici, illuminanti/calibrazioni noti e target fotografici con riferimento misurato; errore di neutralità e colore riportato. Kelvin RAW e correzione RGB distinti; campioni saturi non usati come grigio. Incarnati/saturi verificati visivamente con condizioni registrate. |
| Dettaglio | Impulsi, bordi, tessuti/Siemens, rampe e rumore noto a più scale; quantità zero identità, DC conservata, aloni/alias/rumore quantificati. Texture/chiarezza/sharpening distinguibili; nessuno sharpening inatteso a fit. |
| Ottica | Griglie sintetiche con coefficienti noti: residuo geometrico RMS ≤0,2 px nativi, residuo massimo dichiarato; canali sintetici TCA riallineati entro 0,2 px. Flat-field: uniformità residua ≤1% nel dominio valido e non saturo. Verifica fotografica separata per ottica/focale/apertura e copertura del profilo. |
| Profili e geometria | Nessuna doppia correzione; matching ambiguo non automatico; profilo mancante/versione mutata non sostituiti tacitamente. Baffo, crop, rotazioni, proiezioni invalide, alpha ai bordi e mappe per canale; nessuna area fuori sorgente presentata come campione valido. |
| Tile e maschere | Full frame contro tile con halo, bordi immagine, alpha nullo, maschere sovrapposte e trasformazioni. Coordinate del pennello/ritocco stabili dopo crop, rotazione e cambio scala; nessuna cucitura o dipendenza dall'ordine delle richieste. |
| CPU/GPU | Confronto sul working: obiettivo `abs(a-b) ≤ 1e-5 + 1e-4*abs(a)` per canale finito sul corpus e per nodo; differenze di ramo discrete non ammesse. Output SDR8 entro un codice con dither disattivato. Controlli aggiuntivi su gradiente 16 bit e valori estesi; soglie non raggiunte mantengono CPU o aprono un difetto, non un fallback silenzioso alla vecchia ricetta. |
| Vista/export | Medesima revisione, ROI e output; lossless entro il quantizzatore previsto e hash uguali dove garantiti; JPEG con confronto separato della perdita. Tutti i quattro motori Mac e i tre Windows, Standard/Piena, 1:1/fit e export durante modifica/cambio foto. |
| Durabilità | Crash prima/dopo commit e pubblicazione blob, disco pieno, originali spostati/offline, ricostruzione indice/cache, undo/redo dopo riavvio, backup+restore su copia e schema futuro. Nessuna perdita delle revisioni confermate, nessuna falsa conferma di salvataggio. |
| Sicurezza | Ricette, profili, maschere e IPC malformati, dimensioni/halo ostili, path traversal, dipendenze mancanti, timeout e annullamento. Gli originali conservano hash; nessun accesso esterno al confine previsto. |
| Prestazioni | D750/D40 autorizzati, immagini sintetiche 12/24/45 MP e corpus esteso; gesto continuo, pan/zoom, undo, batch ed export concorrenti con memoria a 2/4 GiB secondo ammissione. RSS/footprint, frame, tempi a caldo/freddo, rifiuti e recupero registrati. |
| UI e piattaforme | IT/EN, layout compatto e 200%, sola tastiera, VoiceOver/NVDA, focus/popup/Esc, preferenze globali e foto indipendenti. Test nativi Mac e Windows prima di attribuire supporto a entrambi. |

Al cambiare di worker/broker/bundle, le consegne includono `scripts/verify.sh --gui` e `scripts/test-xpc-integration.py` sul pacchetto effettivo, oltre alle prove mirate. Corpus fotografico e artefatti privati rimangono in `var/` ignorata; nei rapporti pubblici solo dati autorizzati e fixture sintetiche. Le prove di prodotto aggiornano STATO.md, senza trasformare questo documento in un secondo registro.

## Estensioni e decisioni da chiudere prima dei rispettivi incrementi

Sono punti di progetto circoscritti: algoritmo e dominio esatto delle curve/colore esteso; parametri WB qualificabili per ciascun backend; formato e integrazione del database ottico; filtri multiscala/foschia; comportamento e conservazione della cronologia; distribuzione di profili e modelli; ICC/soft proof e sharpening di stampa. Ogni spike deve consegnare una scelta versionata e un riferimento numerico prima di attivare il controllo, con fallback CPU/manuale dove previsto.

L'estensione avanzata può aggiungere selezioni automatiche locali, denoise su mosaico basato su modelli, profili fotocamera personalizzati, correzione di diffrazione con PSF nota, sfocatura di profondità e rimozione di riflessi. Per ciascuna servono dati, licenza, determinismo/asset persistente, capacità del backend e una prova di qualità dedicata. Fusione HDR, panorama, focus stacking, riempimento generativo, tethering e stampa completa sono progetti separati; non diventano dipendenze del pannello Sviluppo fotografico.
