# Documenti raster TrueRenderer — contratto iniziale

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
l'uscita PNG16: sRGB oppure Rec.2020 lineare; il calcolo interno resta lineare.
Il PNG dichiara profilo/primarie e PPI; i valori fuori gamut/intervallo vengono
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
Un tratto concluso è una voce undo. I ritocchi persistono come pixel del livello,
ancora modificabili con gli strumenti raster; i parametri dei tratti conclusi e
la cronologia della sessione non vengono serializzati. Per conservare una base
intatta si lavora su una copia indipendente del livello.

## Contenitore v1

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

1. Otto byte `54 52 44 4f 43 00 01 00`.
2. Lunghezza del manifest, `u32` little-endian, massimo 1 MiB.
3. Manifest JSON UTF-8: `document`, presenza della selezione e presenza delle
   maschere per livello. I vettori raster non sono codificati in JSON.
4. Selezione se presente: `width × height` campioni fp32 little-endian.
5. Per ogni livello: RGBA fp32 little-endian, poi l'eventuale maschera fp32.
6. SHA-256 dei byte precedenti, 32 byte. Nessuna coda ulteriore ammessa.

Versioni o campi sconosciuti vengono rifiutati; non si scartano proprietà in silenzio.
Dimensioni, conteggi, quote e presenza delle maschere vengono controllati prima di
allocare gli asset. Dopo la lettura si verificano checksum, finitezza, alpha,
trasformazioni e ricette. `scripts/generate-document-fixture.py` è un produttore
indipendente basato sulla sola libreria standard Python.

## Memoria, salvataggio e recupero

Il contratto iniziale ammette 16 megapixel per tela/asset, 64 livelli e 256 MiB
complessivi di pixel e maschere. La cronologia è limitata a 32 revisioni/256 MiB.
Lo spazio documento prenota conservativamente 3 GiB nel budget condiviso per
asset, storia, snapshot del gesto, rendering, scratch e I/O. È ammissione applicativa,
non una garanzia RSS del sistema operativo. La chiusura del documento rilascia la
prenotazione; tornare alle fotografie conserva il documento e la sua memoria.
L'importazione prenota separatamente 3 GiB per worker isolato, snapshot e raster
in arrivo; se l'ammissione fallisce l'immagine non viene decodificata.
La presentazione iniziale è Adatta con anteprima massima di 2048 px; salvataggio e
export usano tutti i pixel. Tiling e valutazione regionale del documento richiedono
un incremento distinto prima di estendere le quote.

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
