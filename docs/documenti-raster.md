# Documenti raster TrueRenderer — contratto iniziale

## Flusso di lavoro e pila dei livelli

La vista fotografica e il progetto usano le schede **Sviluppo**, **Livelli**, **Tools**
e **Export**. Non è necessario entrare in uno «Spazio documento» separato. Alla
richiesta confermata di composizione dalla foto, il pool acquisisce il
raster nativo con motore e WB della ricetta corrente; conserva le regolazioni RGB
come parametri del livello. La tela conserva le dimensioni native della foto: il
ritaglio resta una regolazione del livello e non ridimensiona il documento. Un caricamento
fallito lascia intatto il progetto precedente. Il documento occupa il viewer e
il pannello laterale ordinari, senza header, footer o layout separati. Griglia,
filmstrip, navigatore e selezione restano comuni a foto e progetti.

Nuovo documento crea con nome un file `.truerenderer` nella cartella corrente,
senza sovrascrivere nomi esistenti, e lo seleziona. Apri file, trascinamento e
apertura OS seguono lo stesso percorso delle foto: selezionano il file nella sua
cartella, senza copiarlo implicitamente. Salva con nome seleziona il nuovo file;
esportare una copia aggiorna l'inventario senza cambiare il lavoro corrente.
Le miniature native sono prodotte da un solo lettore in background con quote,
checksum e cache limitata, mai inviate ai decoder fotografici.

Selezionare un'altra foto conserva il progetto e la cronologia in memoria;
selezionarlo nuovamente o Apri → Riprendi progetto in corso lo riattiva nel viewer.
Un altro progetto richiede Salva/Scarta/Annulla se ci sono modifiche pendenti;
annullare riporta al lavoro precedente. I file non validi non sostituiscono il
lavoro corrente. Le successive modifiche al catalogo fotografico non sostituiscono
il progetto già acquisito.

La foto occupa il **livello 0**, in fondo alla pila, che non si elimina né si
riordina sopra gli altri. Nei documenti nuovi il livello 0 è lo sfondo iniziale.
Si possono comunque modificarne visibilità, opacità, pixel, regolazioni e
trasformazione. I nuovi livelli vuoti, le immagini inserite e le copie del visibile
si aggiungono in cima; la copia di un livello o della sua selezione si colloca
immediatamente sopra la sorgente. I numeri indicano la posizione corrente nella
pila. Le regolazioni fotografiche e le loro maschere appartengono al raster
selezionato e rimangono accessibili nel suo pannello.

**Sposta** è accessibile in Sviluppo, Livelli e Tools; modifica il contenuto del
livello selezionato rispetto alla tela, con trascinamento o coordinate, scala e
rotazione numeriche. Tools raccoglie selezioni, pennello, cancellazione e ritocco.
**Selezione → nuovo livello** copia i pixel e la ricetta su un raster indipendente,
trasferendo la copertura della selezione della tela in una maschera locale,
moltiplicata per l'eventuale maschera già presente. Deseleziona al termine per
permettere lo spostamento indipendente; il livello sorgente resta intatto. Il gesto
di spostamento o ritocco costituisce una voce undo; cambiare scheda termina il gesto
corrente e disattiva gli strumenti interattivi nascosti.

Export distingue **Salva progetto**, **Salva con nome** ed **Esporta copia del
progetto** dall'immagine finita. PNG16 e PNG8 offrono sRGB, Adobe RGB (1998), Display P3 o Rec.2020 lineare
con alpha; JPEG usa lo stesso profilo RGB scelto con ICC e qualità selezionabile, componendo la trasparenza
su bianco in luce lineare. Il lato lungo opzionale riduce soltanto la copia di
uscita; il progetto conserva tutti i pixel. L'export immagine non sovrascrive file
esistenti. Dimensioni, profilo e DPI/PPI scelti in Export appartengono solo alla
copia esportata. I valori del documento si scelgono alla creazione di un nuovo
file vuoto; avviando il lavoro da una foto si conservano quelli della sorgente.

La risoluzione fisica viene letta nel decoder isolato dai metadati primari EXIF
(TIFF/RAW basati su TIFF e JPEG), JFIF o PNG pHYs. I due assi rimangono distinti;
centimetri vengono convertiti in pollici e l'orientamento scambia gli assi quando
necessario. Metadati assenti, non validi, fuori quota o senza unità fisica restano
assenti: non vengono sostituiti con 72/300 PPI né con i valori di una miniatura.
PNG dichiara pHYs solo quando noto; JPEG usa EXIF razionale e JFIF intero.
Le altre famiglie RAW non hanno ancora un lettore di densità specifico.
Nessuna dipendenza da percorsi locali, backend grafico o piattaforma è aggiunta.

## Foto originali e ritocco reversibile

Aprire Sviluppo, Livelli, Tools o Export non crea un documento. La foto resta il
livello 0; i 16 strumenti fotografici, le loro regolazioni e maschere continuano a
usare la ricetta nel catalogo. Pennello, gomma, clona, ripara, ripara rapido e
spostamento lavorano anch'essi sulla foto originale, senza cambiare estensione.
Il file originale rimane in sola lettura. Non viene creato un asset raster laterale.

La ricetta schema v3 aggiunge `retouch` opzionale, versione 1: tratti ordinati,
attivazione globale e per tratto, raggio relativo alla larghezza nativa, durezza,
opacità, colore sRGB, offset del donatore e selezione vettoriale congelata nel
tratto; traslazione normalizzata, scala e rotazione della foto. Le coordinate
appartengono alla sorgente orientata. Undo/redo, chiusura/riapertura e salvataggio
nel catalogo conservano i parametri; i tratti si possono disattivare, modificare o
eliminare singolarmente. I limiti sono 128 tratti, 1024 punti complessivi fra
tracciati e selezioni, 16 componenti per selezione e 48 KiB per l'intera ricetta.
I limiti di lunghezza e interpolazione rifiutano gesti eccessivi esplicitamente.
Le vecchie ricette restano valide e non vengono migrate alla sola apertura.

Il replay opera su una copia privata del RAW sviluppato con il motore/WB corrente:
prima ritocchi, poi regolazioni fotografiche e livelli, quindi trasformazione foto
e geometria/grana esistenti. Il donatore è congelato all'inizio di ciascun tratto
nel replay, non nel file. Cambiare motore o WB ricalcola quindi anche il ritocco.
La tela mantiene i pixel nativi; il ritaglio fotografico continua a definire la
regione visibile. L'export definitivo esegue il replay nativo nel decoder isolato;
le anteprime usano la risoluzione residente e la stessa semantica normalizzata.
I percorsi GPU che non rappresentano i nuovi operatori rinviano al replay CPU;
il valutatore regionale ricostruisce il frame residente completo. Non si promette
la latenza degli strumenti puntuali GPU sui ritocchi né indipendenza dal numero
di tratti. Restano le limitazioni preesistenti dell'analisi automatica con strumenti
avanzati; Auto non analizza una ricetta con ritocchi attivi.

Le selezioni libera/poligonale/rettangolare usano sostituzione, unione probabilistica,
intersezione e sottrazione. I punti in corso sono visibili sulla tela; Invio/doppio
clic conclude il poligono, Esc annulla. **Selezione → regolazione** crea una maschera
poligonale in un livello fotografico, ancora sul RAW. La selezione di sessione è
transitoria; quella applicata a un tratto o a una regolazione viene serializzata.
Spostamento e maschere si escludono a vicenda come strumenti interattivi; Spazio
conserva il pan del viewer.

Solo copia immagine, livello raster vuoto, immagine esterna e copia di una selezione
aprono la conferma **Passare all’editing raster?**. Essa indica il nuovo
nome, la conservazione dell'originale e l'incorporazione dello sviluppo sensore.
Confermando si prepara e salva in background un file con nome libero nella cartella
della foto; nessun file esistente è sovrascritto. Si seleziona il nuovo progetto
soltanto dopo il completamento. Errori e annullamento conservano il lavoro precedente.
La copia della selezione riprende il risultato visibile con copertura applicata dopo
il replay, così i ritocchi non ripristinano pixel fuori dalla selezione; il livello
0 conserva la ricetta originale. Le copie dell'intera foto conservano i parametri.

L'export fotografico mantiene JPEG, PNG8/16, TIFF16/float32, DNG lineare e DNG RAW.
Nei formati renderizzati dimensioni e densità ereditano quelle della sorgente;
lato lungo e PPI personalizzati appartengono solo alla copia esportata. La densità
ignota rimane assente, gli assi vengono scambiati dopo rotazioni di un quarto di
giro. Il DNG RAW conserva il mosaico e non incorpora gli effetti del rendering.

## Modalità, foto di partenza e colore di destinazione

La conferma di conversione consiglia di completare prima lo sviluppo fotografico
(WB sensore, esposizione e regolazioni), poi passare al raster. I comandi che
creano contenuto indipendente sono raccolti sotto **Raster**; il ritocco e le
selezioni fotografiche sono esplicitamente indicati come reversibili senza
rasterizzazione. La copia della selezione porta la dicitura Raster sul pulsante.

Il pannello del documento presenta **Editing raster** sopra le quattro schede.
Luce, curva tonale e colore condividono gli stessi controlli, reset, interruttori,
slider e aiuti delle foto; anche i controlli avanzati e i 16 strumenti di
regolazione sono comuni. Il WB sensore appartiene alla foto originale; nel raster
resta il bilanciamento RGB sul livello selezionato.
Il contagocce RGB campiona una zona 3 × 3 dopo le regolazioni correnti; ripetere
il campionamento sulla stessa zona neutra non accumula il bilanciamento. Le
limitazioni del contagocce fotografico (strumenti avanzati, saturazione e curve)
restano applicabili.

**Torna alla foto…** apre una conferma esplicita: si scartano solo le
modifiche raster non salvate e la cronologia in memoria. Il file di progetto già
salvato non viene rimosso né modificato; originali e ricette del catalogo restano
invariati. Il comando riapre la foto nel pannello Sviluppo. Una sorgente assente
non permette lo scarto; lo stesso vale per un’estensione non fotografica.
L'associazione alla sorgente vive nella sessione e
sopravvive ai salvataggi; un progetto riaperto senza associazione propone
**Scegli originale…**, poi la stessa conferma. Non si inseriscono percorsi
privati nel contenitore portabile.

La pila delle foto mostra **0 · Foto di partenza** in fondo, sotto le regolazioni,
con occhio di visibilità. `retouch.base_hidden`, facoltativo e falso per le vecchie
ricette, conserva lo stato con undo/redo e persistenza. Nascondere la base rende
trasparente la foto sviluppata, compresi i ritocchi applicati ad essa; le regolazioni
non generano pixel indipendenti. Il toggle è indipendente dall'attivazione dei
ritocchi. Nei documenti raster il livello 0 mantiene il proprio interruttore di
visibilità e i livelli indipendenti sopra restano visibili.

**Profilo colore di destinazione** offre sRGB, Adobe RGB (1998), Display P3 e
Rec.2020 lineare in Sviluppo e Export. Nelle foto la scelta appartiene alle opzioni
di export della sessione; nei documenti è salvata come destinazione del progetto,
con override separato per l'export. La conversione dalla foto eredita la destinazione
selezionata. Non cambia primarie o precisione dei calcoli
interni, profilo del monitor o anteprima sRGB16 preesistente. JPEG, PNG8/16 e TIFF16
fotografici convertono davvero i campioni nel profilo scelto; i documenti fanno
lo stesso per JPEG e PNG. sRGB PNG usa il chunk sRGB; le altre destinazioni PNG
usano iCCP. JPEG/TIFF incorporano ICC; JPEG non dichiara sRGB in EXIF per le altre
destinazioni. TIFF float32 e DNG mantengono i propri contratti espliciti.

Adobe RGB usa primarie D65 e gamma 563/256 della
[specifica Adobe RGB (1998)](https://www.adobe.com/digitalimag/pdfs/AdobeRGB1998.pdf).
I profili sono generati tramite moxcms già incluso, con coloranti PCS D50 e
adattamento coerenti con le matrici dei campioni e allineamento ICC a quattro byte; nessun asset ICC Adobe è copiato.
Display P3 usa le primarie P3 con D65 e TRC sRGB. Restano applicabili licenza e
avvisi delle dipendenze esistenti. CMYK è indicato come non disponibile: richiede
un profilo ICC di stampa e un percorso RGB→CMYK dedicato, non una semplice voce
RGB o la regolazione creativa «Colore selettivo».

## Ambito e rappresentazione

Il documento usa lo stesso modello e la stessa serializzazione su Windows e macOS.
Le immagini sono asset incorporati in Rec.2020/D65 lineare fp32, con alpha
premoltiplicata. Una copia possiede un vettore di pixel indipendente. Il documento
contiene tela, PPI, unità, profilo di uscita, selezione e pila dal basso verso l'alto.
Ogni livello conserva nome, pixel, visibilità, blocco, opacità, fusione, maschera,
traslazione, scala, rotazione e ricetta fotografica. La maschera segue il contenuto
prima della geometria fotografica; la trasformazione del livello colloca il risultato
sulla tela. Normal, Multiply e Screen si calcolano in luce lineare. Il campionamento
bilineare considera trasparenti i punti esterni al livello.

La conversione da unità fisiche usa PPI e arrotonda al pixel intero più vicino.
Cambiare orientamento scambia larghezza e altezza. Il profilo selezionato definisce
la destinazione RGB di PNG8/16 e JPEG; il calcolo interno resta Rec.2020 lineare fp32.
Il PNG dichiara profilo/primarie e, quando noti, PPI; i valori fuori gamut/intervallo vengono
limitati nell'export intero, senza modificare gli asset fp32 del documento.

L'importazione acquisisce il raster attraverso la coda del pool e il broker isolato
esistente, riutilizzando uno dei due servizi XPC nel bundle macOS. Non apre una
terza connessione a un servizio già occupato. Crediti di snapshot, ammissione del
decoder e prenotazione del documento restano vivi fino al trasferimento. Copiare
una foto dal viewer richiede il raster nativo, mai una miniatura, e conserva la
ricetta RGB. Lo sviluppo sensore è incorporato nei pixel: il contenitore iniziale
non incorpora il RAW originale e non offre la modifica successiva del decoder/WB
sensore. Gli originali restano in sola lettura. Non sono previsti asset collegati
nel contratto iniziale; nessun percorso locale è necessario alla riapertura.

## Editing e conservazione dei parametri

Gli interruttori fotografici parcheggiano i parametri nella ricetta e sostituiscono
soltanto i campi eseguibili con valori neutri. La UI mostra i valori conservati,
disabilitando il corpo del pannello. La riattivazione li ripristina. I renderer CPU,
GPU, regionali e l'export continuano a interpretare gli stessi campi eseguibili.
Gli effetti nei livelli hanno un deposito separato per indice, validato contro tipo
e neutralità dell'operatore attivo. Undo/redo e persistenza conservano entrambi.
Le vecchie ricette senza questi campi mantengono serializzazione e comportamento.

Le selezioni libera, poligonale e rettangolare sono coperture della tela, con
sostituzione, massimo per aggiunta, prodotto per intersezione e prodotto per il
complemento nella sottrazione. La sfumatura usa la distanza dal bordo del poligono.
La gomma porta RGB premoltiplicato e alpha verso zero, rispettando la selezione.
La conversione della selezione in maschera riproietta la copertura nello spazio
originale del livello, anche se questo è trasformato. Il ritocco usa la mappa
diretta/inversa della geometria fotografica e della trasformazione sulla tela;
per prospettiva e distorsione controlla tutto il dominio sorgente, con un costo
superiore al ritocco su livelli senza deformazioni. I pannelli fotografici del
documento gestiscono curve, maschere disegnate, overlay, orizzonte, campionamento
colore e analisi dell'ingresso della regolazione.

Fra le estensioni facoltative sono incluse pittura, selezione rettangolare, scala
e rotazione numeriche. Riempimento contiguo, sfumatura raster, selezione cromatica
e maniglie di trasformazione libera non fanno parte del contratto iniziale.

Clona campiona il livello selezionato da una copia congelata all'inizio del tratto.
Ripara aggiunge alla texture campionata una correzione RGB stimata dal bordo;
Ripara rapido cerca una patch nei dintorni confrontando i campioni del bordo,
poi applica la medesima correzione. Non sono algoritmi generativi. Un bordo privo
di campioni opachi validi produce un errore esplicito. Raggio, durezza e intensità
determinano la copertura; i gesti sono interpolati per evitare lacune.
Un tratto raster concluso nel progetto è una voce undo. Questi ritocchi persistono come pixel del livello,
ancora modificabili con gli strumenti raster; i parametri dei tratti conclusi e
la cronologia della sessione non vengono serializzati. Per conservare una base
intatta si lavora su una copia indipendente del livello.

## Contenitore v2 e lettura v1

Estensione di lavoro: **`.truerenderer`**; `.true` non è un alias. Si preferisce il
nome lungo e riconoscibile, coerente con la [guida Microsoft sulle estensioni](https://learn.microsoft.com/en-us/windows/win32/shell/how-to-choose-a-file-type-extension).
Non si rivendica esclusività globale e non si dichiara registrato un tipo MIME IANA.
Il tipo macOS dichiarato dal bundle è `it.truerenderer.document`, conforme a
`public.data`; il ProgID Windows è `TrueRenderer.Document`.
L'apertura dal Finder usa [kAEOpenDocuments](https://developer.apple.com/documentation/coreservices/kaeopendocuments)
e un handler di [NSAppleEventManager](https://developer.apple.com/documentation/foundation/nsappleeventmanager/seteventhandler(_:andselector:foreventclass:andeventid:)),
senza sostituire il delegate della finestra. Su Windows le aperture successive
sono inoltrate all'istanza proprietaria del catalogo mediante una coda locale.

Ordine dei byte:

1. Otto byte `54 52 44 4f 43 00 02 00` per v2; `54 52 44 4f 43 00 01 00` per v1.
2. Lunghezza del manifest, `u32` little-endian, massimo 1 MiB.
3. Manifest JSON UTF-8: `document`, presenza della selezione e presenza delle
   maschere per livello. I vettori raster non sono codificati in JSON.
4. Selezione se presente: `width × height` campioni fp32 little-endian.
5. Per ogni livello: RGBA fp32 little-endian, poi l'eventuale maschera fp32.
6. SHA-256 dei byte precedenti, 32 byte. Nessuna coda ulteriore ammessa.

I nuovi documenti usano v2: `resolution` contiene `x` e `y` in PPI oppure è
assente. Il campo storico `ppi` coincide con X quando noto; quando ignoto il suo
valore tecnico non descrive una densità fisica e non viene esportato. I file v1
restano apribili: il loro `ppi` vale per entrambi gli assi. Un v1 aperto viene
conservato come v1; una vecchia applicazione che legge solo v1 non può aprire v2.
La versione del manifest deve coincidere con quella nell'intestazione.

Versioni o campi sconosciuti vengono rifiutati; non si scartano proprietà in silenzio.
Dimensioni, conteggi, quote e presenza delle maschere vengono controllati prima di
allocare gli asset. Dopo la lettura si verificano checksum, finitezza, alpha,
trasformazioni e ricette. `scripts/generate-document-fixture.py` è un produttore
indipendente basato sulla sola libreria standard Python.

## Memoria, salvataggio e recupero

La tela e ciascun asset ammettono fino a 67.108.864 pixel (64 Mi pixel), come il
decoder fotografico, con 64 livelli e 2 GiB complessivi di pixel e maschere.
Una sorgente oltre quota viene rifiutata esplicitamente, mai ridotta per entrare.
L'ammissione preventiva nel budget condiviso può limitare ulteriormente il lavoro
su macchine con meno memoria. Non si promettono progetti arbitrariamente grandi.
La cronologia conserva al massimo 32 revisioni e un totale pari al maggiore fra
256 MiB e il peso dell'ultima transazione: una foto grande ammette almeno una
revisione. La cronologia di sessione non viene salvata nel file.

L'inviluppo conservativo del documento comprende asset correnti, transazione,
undo/redo, composito nativo e mip, scratch di rendering e presentazione. Parte da
3 GiB e cresce prima delle allocazioni in funzione di pixel e asset. È ammissione
applicativa, non una garanzia RSS. Il documento residente conserva i crediti anche
durante la navigazione in altre foto; sostituirlo rilascia i crediti non più utili
quando non ci sono importazioni condivise. Un gesto non ammesso non modifica pixel.
Importazione e lettore delle miniature hanno proprie prenotazioni; gli snapshot e
i temporanei del decoder sono contabilizzati separatamente e possono richiedere
più dei 3 GiB iniziali. La lettura ricontrolla il manifest contro i crediti ammessi
prima di allocare, anche se il file è cambiato dopo l'ispezione iniziale.

Il viewer campiona la regione visibile dal composito nativo e dai suoi mip: a 1:1
usa i pixel originali, senza il precedente tetto di 2048 px. La miniatura del
progetto residente rimane un derivato fino a 2048 px; quella in background arriva
a 512 px. Il normale limite del viewport è 8 Mi pixel. Pan con Spazio e zoom
operano nello stesso viewer fotografico. Il composito viene ancora valutato per
intero dopo una modifica: tiling e valutazione regionale degli edit restano un
incremento distinto, senza una garanzia di fluidità per grandi progetti.

Salva scrive un temporaneo nella stessa directory, esegue flush e sync, conserva
il documento precedente in un backup dal nome univoco `*.previous-*.truerenderer`
e pubblica il nuovo file mediante sostituzione atomica. I backup sono durevoli;
non vengono sovrascritti né rimossi come cache. Il salvataggio non sostituisce un
file esistente che non sia un documento valido. Un'interruzione prima della
pubblicazione conserva il documento precedente; il backup si apre con Apri.
Non si promette autosalvataggio dei gesti mai salvati. La chiusura chiede come
gestire le modifiche pendenti. Export documento produce una copia senza cambiare
il percorso corrente né dichiarare salvato il documento di lavoro.

Il trasferimento fra sistemi richiede confronti strutturali e della resa oltre
alla semplice lettura del file. I criteri trasversali restano in architettura §17.0.1.
Stato, prove eseguite e lavoro aperto sono registrati soltanto in [STATO.md](../STATO.md#livelli-raster-ritocco-formato-progetto).
