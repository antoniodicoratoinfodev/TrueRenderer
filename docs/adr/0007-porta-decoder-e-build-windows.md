# ADR 0007 — porta decoder, contratto di colore in ingresso e build Windows

**Seguito dell'audit:** [ADR 0009](0009-isolamento-worker-windows.md) descrive il nuovo avvio LPAC Windows; stato delle correzioni bitmap e verifiche in [avanzamento](../avanzamento.md). Questo ADR conserva le decisioni e il perimetro del refactor iniziale.

**Nota del 13 settembre 2026:** questo ADR registra il refactor iniziale con gate esterno invariato. Il successivo port locale ha introdotto `serve_confined` e concesso gli esterni Windows; questo testo non costituisce l'autorizzazione o la qualifica di quell'estensione. L'[audit prima di main](../revisione-pre-main.md) ha riprodotto accesso filesystem residuo e controlli colore saltati dal decoder esterno: le proprietà descritte sotto non valgono automaticamente per tutti gli ingressi aggiunti dopo il refactor.

Data: 10 settembre 2026. Stato: refactor applicato; nessuna piattaforma nuova qualificata.

## Decisione

Introdurre in `tr-core` una porta decoder esplicita, rendere tipato il contratto di colore in ingresso e far passare da lì entrambe le implementazioni esistenti. Correggere separatamente la dipendenza che impediva la compilazione su Windows. Nessun formato nuovo viene ammesso, nessun backend codec viene scelto e il perimetro di sicurezza resta identico.

Il documento di architettura descrive `tr-core` come sede delle porte, con Little CMS e LibRaw come implementazioni esterne. Nel codice quelle porte non esistevano: il percorso di richiesta conteneva due rami `#[cfg(target_os)]`, uno per i metadati e uno per il raster, e ogni piattaforma nuova avrebbe aggiunto un terzo e un quarto ramo nella stessa funzione. La porta rende quel costo costante.

## Porta

`tr_core::decoder::Decoder` dichiara `name`, `probe` e `decode`. Le implementazioni restituiscono lo stesso `RasterInfo` di provenienza e gli stessi campioni lineari Rec.2020 premoltiplicati, quindi il chiamante non le distingue per contratto. `name` è un'identità stabile, adatta a provenienza, report e chiavi di cache.

`tr_core::decoder::Trust` separa `Controlled` da `External`. La fiducia è una proprietà del chiamante, non del decoder: resta il trasporto a stabilire quale delle due vale, come prima. `Trust::Controlled` designa le sorgenti il cui digest è nel manifest compilato; `Trust::External` designa byte arbitrari e appartiene soltanto a un ospite sandboxed.

Due implementazioni in `tr-worker`. `CorpusPng` è portabile, presente in ogni build, e non accetta sorgenti arbitrarie. `Apple`, compilato solo su macOS, delega a ImageIO, ColorSync e CIRAWFilter senza cambiare la ricetta `TR-linear-v1` di ADR 0004. `external_decoder` è l'unico punto del percorso di decodifica che nomina un sistema operativo; `None` è uno stato previsto e significa che quella build serve solo il corpus controllato. Una porta per una piattaforma nuova si pubblica lì.

## Contratto di colore in ingresso

Ogni decoder converte nello stesso spazio di lavoro lineare Rec.2020, quindi i campioni da soli non dicono se la conversione ha onorato il file o l'ha indovinato. È quella differenza a decidere se il render può dirsi fedele all'originale, perciò `ColorSource` è un tipo e non una riga di prosa. `Declared` significa che il file dichiara il proprio spazio e questa build lo ha applicato; `Assumed` che il file non dichiara nulla e sRGB è stato assunto. Non esiste una variante per "dichiarato ma non applicato": un file che dichiara uno spazio che questa build non sa applicare viene rifiutato, perché sostituirne un altro cambierebbe il significato dei suoi pixel. È la regola di §8.2, finora scritta e non implementata.

La porta impone la distinzione nelle firme: `probe` e `decode` restituiscono anche `ColorSource`, e il trasporto scrive `input_color` da `provenance()` invece che dalla prosa del singolo decoder. Un'assunzione resta quindi visibile come assunzione nel pannello di provenienza, con la stessa formulazione per ogni piattaforma. Il valore non viaggia ancora tipato sul protocollo: portarlo su `RasterInfo` cambierebbe la serializzazione e il fingerprint della cache, ed è il passo successivo, non questo.

Il decoder del corpus ora legge davvero i chunk di colore del PNG. Prima rifiutava un ICC incorporato ed era cieco a tutto il resto: un PNG con `cHRM` che dichiara primarie Display P3 sarebbe stato convertito come sRGB, in silenzio e con colori sbagliati. Questa è la ragione per cui il contratto viene prima di qualunque decoder nuovo. Un decoder portabile scritto come quello attuale avrebbe portato quel difetto su ogni JPEG aperto su Windows.

Le regole applicate: `iCCP` viene rifiutato, perché Little CMS resta da qualificare; un chunk `sRGB` è una dichiarazione che la pipeline onora, e come prescrive la specifica prevale su `gAMA` e `cHRM` accanto ad esso; `cHRM` da solo viene rifiutato; `gAMA` da solo fissa la curva e non dice nulla sulle primarie, quindi al valore sRGB lascia lo spazio assunto e a qualunque altro valore viene rifiutato. Vengono letti i campi grezzi dei chunk e non gli accessori risolti del crate, che sostituiscono i valori sRGB e nasconderebbero quello che il file dichiara davvero.

`png` 0.18.1 diventa una dipendenza diretta. Era già nel grafo tramite `image`, alla stessa versione bloccata, quindi non cambia nulla nella supply chain; serve perché `image` espone il profilo ICC e il gamma ma non `cHRM` né il chunk `sRGB`, e senza quelli la cecità sopra non è chiudibile.

L'adattatore Apple classifica a partire dalla stringa che il livello nativo costruisce. È un ponte dichiarato: la distinzione appartiene a `TRImageInfo` come flag, accanto al testo, e va spostata lì quando il percorso macOS viene ricompilato.

## Perimetro invariato

`serve` continua a girare con `external = false` e `tr_worker_serve_fds`, ingresso XPC, resta l'unico punto che concede `external = true`. Il cancello del corpus di `CorpusPolicy` è invariato e viene valutato prima della selezione. La porta non allarga nulla: rende solo esplicito ciò che il trasporto già decideva.

Due test coprono l'invariante. Il primo verifica che la porta controllata esista sempre, che quella esterna coincida con `external_decoder` e che le due siano distinte; su una build senza porta esterna richiede che l'errore dichiari il limite di piattaforma invece di ricadere silenziosamente sul decoder del corpus. Il secondo verifica che una sorgente fuori corpus non raggiunga alcun decoder sul trasporto portabile, qualunque piattaforma fornisca la porta esterna.

Tre test coprono il contratto di colore. I dodici PNG del corpus dichiarano `sRGB` nei propri chunk, quindi la lettura è verificabile contro file reali e non contro fixture costruite per l'occasione: il primo test richiede che quella dichiarazione venga letta e non assunta. Il secondo costruisce un PNG con primarie Display P3 e uno con una curva lineare e richiede che entrambi vengano rifiutati nominando il chunk responsabile. Il terzo richiede che un file senza informazione di colore renda comunque, dichiarando però l'assunzione come tale.

## Dipendenza sha2

`sha2-asm` distribuisce assembly in sintassi GNU e MSVC non lo compila, quindi l'intero workspace non compilava su Windows prima di raggiungere il codice del progetto. La feature `asm` resta attiva su ogni target tranne MSVC, dichiarata in `tr-core` con `cfg(not(target_env = "msvc"))` e unificata sul grafo. Su macOS il comportamento non cambia. Su Windows si usa l'implementazione Rust, più lenta: SHA-256 non è marginale, viene calcolato su ogni sorgente per verificare la cache, quindi la differenza va misurata quando la piattaforma sarà reale.

## Stato su Windows

Il workspace compila e le suite di `tr-core`, `tr-app`, `tr-platform`, `tr-render`, `tr-store` e `tr-worker` passano. Restano fuori sette test di `apps/desktop`, sei della cache su disco e uno del monitor sorgente: la cache persistente è chiusa da `cfg(not(unix))` perché usa descrittori di directory con `O_DIRECTORY` e `O_NOFOLLOW`. Erano già rossi prima di questo refactor ed è stato verificato isolando la sola correzione della dipendenza.

Restano quindi tre lavori distinti, nessuno affrontato qui: un decoder esterno che non sia quello Apple, una sandbox equivalente a XPC secondo §17.1 e la tabella dei meccanismi candidati, e una cache su disco che non dipenda da descrittori di directory Unix. Finché la sandbox manca, `Trust::External` non va concesso su Windows: aprire file arbitrari fuori da un isolamento reale contraddirebbe il motivo per cui il cancello del corpus esiste.

## Conseguenze

Il costo di una piattaforma nuova nel percorso di decodifica diventa un'implementazione di `Decoder` più una riga in `external_decoder`, invece di rami condizionali sparsi. Ogni decoder futuro è obbligato dalle firme a dichiarare sotto quale contratto di colore ha lavorato, quindi la cecità che era possibile prima non è più esprimibile.

La scelta definitiva del backend codec, che la sezione 4 lascia aperta, non è anticipata: LibRaw, libjpeg-turbo, libpng, libtiff e Little CMS restano candidati. `WorkingSpace` e `TileProvider`, altre due porte che il documento dà per esistenti, restano da estrarre.

Il rifiuto è oggi più severo dell'obiettivo. Un JPEG Display P3 e un PNG con `cHRM` sono file legittimi e comuni, e questa build li rifiuta invece di mostrarli male: è la scelta corretta finché non c'è una trasformata, ma non è uno stato di arrivo. Ciò che lo scioglie è Little CMS come trasformata di ingresso, con la verifica ΔE00 che §8 elenca fra le misure ancora mancanti. È il primo lavoro che allarga davvero i formati leggibili senza rinunciare alla fedeltà, su entrambe le piattaforme.
