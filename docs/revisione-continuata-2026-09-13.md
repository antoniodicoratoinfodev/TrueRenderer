# Revisione serale e correzioni — 13–14 settembre 2026

Documento unico di audit e correzione. I rilievi sotto sono la baseline negativa storica; le correzioni successive sono verificate su Windows e incluse in `67146e3`. La qualifica nativa Mac/XPC resta aperta. I rapporti JSON conservano hash ed esiti delle singole esecuzioni.

## Audit prima delle correzioni

Recuperata la sessione «Controlla il lavoro non committato», iniziata alle 20:05 locali con la richiesta «controlli tutto il lavoro fatto non ancora committato?». Si è interrotta per limite d'uso prima della risposta finale. Letti messaggi, comandi, risultati e riproduzioni; completati i controlli sospesi. Nessuna modifica applicativa, commit o staging durante l'audit.

Questo controllo segue le correzioni di [revisione-pre-main.md](revisione-pre-main.md): i quattro rilievi sotto riguardano casi ulteriori, non annullano le riproduzioni positive precedenti. Base Git `1279012b8cbcd15889ff060143b197d2523212ba`. Inventariati 522 file candidati prima di questi aggiornamenti documentali, incluse sorgenti native e notice. [Rapporto con hash ed evidenze](../reports/review-continuation-windows.json). Laboratorio privato recuperato in `var/review-current-20260913/`, log estratti e inventario della continuazione nella sottocartella `continuation/`.

## 1. P1 — Un pattern impedisce di compilare i test su macOS

Riferimento: `crates/tr-platform/src/lib.rs:928`, test `real_worker_is_recycled_after_crash`; enum `Owner` a riga 273.

Il refactor ha sostituito un `let ... else` con `let Owner::Pipe(worker, _) = ...`. Su Windows l'enum ha solo `Pipe`; su macOS contiene anche `Xpc`, quindi il pattern è refutabile. Il test è `#[ignore]`, ma viene comunque compilato da `cargo test` e da Clippy con `--all-targets`: la suite Mac si ferma prima di eseguire i test.

**Verifica:** il diff conferma la rimozione del ramo `else`; compilata la riproduzione minima con entrambe le varianti già preparata dalla sessione precedente (`src/mac-pattern-proof.rs` nel laboratorio). Rust restituisce `E0005`, variante `&mut Owner::Xpc(_)` non coperta. È una prova del difetto di compilazione, non una build nativa Mac o una verifica XPC.

**Correzione richiesta:** gestire esplicitamente la variante inattesa nel test del worker su pipe, quindi eseguire i controlli nativi Mac concordati. Non eliminare il test per fare passare la suite.

## 2. P2 — «Applica e salva» perde la selezione al cambio motore

Riferimenti: `apps/desktop/src/ui.rs:2146`, `:751`, `:968`; `crates/tr-app/src/lib.rs:51`.

Con una cartella già caricata, selezionare una foto diversa dalla prima, impostare uno zoom e cambiare motore dalle impostazioni. Il ramo del pulsante pone sempre `self.scanning = true` **prima** di `apply_settings()`. L'invalidazione del motore crede quindi che esista una scansione in corso e ne accoda un'altra. Quando arriva `Event::Scanned`, `replace_items()` azzera selezione, foto corrente e trasformazione, scegliendo la prima foto visibile.

**Verifica:** seguito l'intero percorso nel codice e recuperata la riproduzione che usa il reducer reale: foto corrente da `"2"` a `"0"` dopo il rescan. Il reset dello zoom è esplicito in `replace_items()`. Non è stata eseguita un'interazione nativa con il pulsante in questo audit.

Lo smoke `--raw-engine-smoke` chiama direttamente `apply_settings()` e parte dalla prima foto; non percorre l'azione completa del pulsante. Il suo esito positivo dimostra il cambio motore interno, ma non copre questa regressione del flusso utente.

**Correzione richiesta:** distinguere manutenzione della cache e scansione effettiva; preservare foto/selezione/trasformazione quando si riapplicano le impostazioni. Verificare il percorso del pulsante su una foto diversa dalla prima, anche durante una scansione realmente pendente.

## 3. P2 — I PNG con cICP vengono accettati come sRGB

Riferimento: `crates/tr-worker/src/lib.rs:140`, funzione `png_color_source()` e sua chiamata dal decoder bitmap esterno.

Il resolver controlla `iCCP`, `sRGB`, `cHRM` e `gAMA`, ma ignora `cICP`. PNG con primarie Display P3 o trasferimento lineare vengono accettati e convertiti con la curva/matrice sRGB; la provenienza dichiara «nessun chunk di colore», oppure sRGB se è presente anche quel chunk.

**Verifica:** fixture PNG RGB 1×1 con gli stessi campioni `[128,64,32]`: senza metadati, `cICP=[1,8,0,1]`, `cICP=[12,13,0,1]`, e `cICP` lineare insieme a `sRGB`. Il broker LPAC restituisce per tutti lo stesso pixel Rec.2020 lineare, circa `[0.15293947,0.06222374,0.02098652,1]`, con tutti e tre i motori Windows. Le codifiche dichiarate non sono equivalenti. Le 12 risposte sono nel rapporto privato recuperato.

La [PNG Third Edition, §11.3.2.6](https://www.w3.org/TR/2025/REC-png-3-20250624/#cICP-chunk) definisce primarie, trasferimento e range nel chunk e ne stabilisce la precedenza quando compreso dal decoder. Il contratto di TrueRenderer (§8.2) richiede inoltre di distinguere metadati assenti e codifiche non supportate, senza trasformare queste ultime in sRGB per assunzione.

**Correzione richiesta:** riconoscere `cICP` prima dei fallback, applicare soltanto le codifiche supportate e rifiutare esplicitamente le altre; rendere visibili i conflitti. Aggiornare l'identità del decoder/cache affinché i raster errati già persistiti non sopravvivano alla correzione.

## 4. P2 — I TIFF con alpha associata risultano troppo scuri

Riferimento: `crates/tr-worker/src/lib.rs:300`, conversione dei campioni in `decode_bitmap()`.

Il decoder tratta ogni RGBA come colore non associato e chiama `from_encoded_srgb()`, che applica la curva e poi moltiplica per alpha. Non risolve prima `ExtraSamples` del TIFF: quando RGB è già associato, il risultato subisce una trasformazione errata e un'ulteriore moltiplicazione.

**Verifica:** due TIFF sintetici RGB 1×1 descrivono lo stesso bianco semitrasparente: `ExtraSamples=1`, RGBA `[128,128,128,128]`, e `ExtraSamples=2`, RGBA `[255,255,255,128]`. Attraverso il worker LPAC il primo produce RGB circa `0.108353`, il secondo `0.501960`, con la stessa alpha `0.501961`. Esito identico nei tre motori Windows: sei risposte recuperate, nessuna fotografia coinvolta.

**Correzione richiesta:** risolvere la semantica alpha prima della trasformazione colore e premoltiplicare una sola volta nello spazio lineare; gestire alpha zero e casi ambigui. In alternativa rifiutare esplicitamente il caso non supportato. Anche questa correzione richiede invalidazione della cache bitmap.

## Controlli completati e limiti

- Sessione recuperata: **81 test Rust ordinari + 6 integrazioni** passati, build debug, fmt, Clippy `--all-targets`, sei controlli IPC, verifica LPAC e 18 risposte sulle fixture bitmap. Le due integrazioni con fotografie erano esplicitamente escluse. Gli 89 test e i 156 sviluppi Nikon del rapporto delle correzioni sono una campagna precedente, non ripetuta qui.
- Continuazione: **build release passata**, 24 segnali di ricampionamento e identità 1:1 passati; due regressioni Python passate; verificati 106 hash upstream LibRaw/79 unità compilate/tre ricette, 205 package e 348 notice Windows; riproduzione `E0005` confermata. `git diff --check` e sincronizzazione documentale verificati.
- LPAC: le sentinelle sono rifiutate in lettura/scrittura, il codice è leggibile ma non scrivibile, i figli sono negati. Winsock fallisce all'inizializzazione con 10107; non viene dichiarata una prova esaustiva di accesso alla rete. Resta il perimetro di [ADR 0009](adr/0009-isolamento-worker-windows.md).
- Il rapporto di ricampionamento si chiama ancora `resampling-macos.json` nel laboratorio, ma questa esecuzione è **Windows**. Non è una nuova misura Mac. Il sandbox del terminale aveva inizialmente impedito la creazione del profilo LPAC; la ripetizione autorizzata nell'ambiente Windows normale è passata.
- Gli hash correnti dei worker ricompilati sono registrati separatamente da quelli della precedente campagna Nikon. Non si attribuiscono automaticamente quei risultati fotografici ai nuovi eseguibili.
- La verifica della dipendenza LibRaw riguarda manifest, sorgenti compilate, bridge e ricette; non è un audit completo di ogni parser C++ upstream.
- Mac/XPC, installazione Windows pulita, colore misurato, display e prestazioni restano aperti. Il titolare ha rinviato le prove sul Mac. Nessun gate R0–R4 chiuso; nessuna disponibilità di Standard/Riferimento dichiarata.

L'incremento correttivo successivo è documentato in [correzioni della revisione serale](revisione-continuata-2026-09-13.md#correzioni). Durante questo audit erano stati preservati codice applicativo e vendorizzato, LICENSE, backup originale dell'architettura e testo esterno ai blocchi di avanzamento.

<a id="correzioni"></a>

## Correzioni ed esiti successivi

## Correzioni applicate

1. **Test worker su Mac:** il test di crash/recovery usa un `match` esaustivo su `Owner`, con rifiuto esplicito della variante XPC inattesa nel test delle pipe. Il test non è stato rimosso. La compilazione nativa Mac e la suite XPC rimangono rinviate dal titolare; la prova minima con entrambe le varianti non le sostituisce.
2. **Applica e salva:** manutenzione cache e scansione hanno stati distinti. Il completamento della manutenzione non azzera una scansione effettiva; salvare le preferenze non inventa un rescan. Pulsante e smoke nativo condividono `start_cache_action()`. Due regressioni del servizio/UI verificano foto diversa dalla prima, selezione multipla, zoom/centro, persistenza delle impostazioni, ritorno al primo motore, svuotamento cache e scansione realmente pendente.
3. **PNG cICP:** riconosciuto sRGB a range completo `[1,13,0,1]`, con provenienza dichiarata. Primarie/trasferimenti/range non supportati vengono rifiutati prima dei fallback sRGB; i conflitti rilevati sono espliciti. Le prove comprendono lineare, P3, PQ/HLG, range limitato e dichiarazioni concorrenti. Questo non implementa la conversione di tali spazi né Little CMS.
4. **TIFF alpha associata:** risolto `ExtraSamples` prima della trasformazione colore. RGB viene deassociato nel dominio dei campioni, convertito e premoltiplicato una sola volta in lineare; alpha zero produce RGBA zero. Alpha ambigua è rifiutata. Prove RGB a 8/16 bit, little/big endian, bianco equivalente e pixel colorati; gray+alpha resta esplicitamente non supportato dall'adattatore corrente.

La ricetta bitmap `bitmap-cicp-alpha-v3` entra nella provenienza e nel fingerprint della cache, escludendo i raster precedenti con interpretazione errata. Aggiunta dipendenza diretta dalla versione TIFF già usata da `image`; nessun sorgente LibRaw upstream modificato.

## Verifiche

- Recuperati e controllati i log delle correzioni: **86 test Rust ordinari**, build debug, formattazione e Clippy su tutti i target passati.
- Completate qui **6 integrazioni** con worker reale e **21 richieste sintetiche PNG/TIFF attraverso LPAC**, su tutti e tre i motori Windows. TIFF associato/non associato bit-exact, `cICP` supportato dichiarato e non supportato rifiutato, input sintetici invariati.
- Passati **6 controlli IPC**, **2 regressioni Python**, verifica dei **106 hash LibRaw / 79 unità compilate / 3 ricette**.
- Passati build release, controllo finale di formattazione e compilazione della prova minima del pattern Mac con entrambe le varianti. Quest'ultima verifica l'esaustività del codice estratto, non i target o il linkage Mac.
- **Prova nativa release passata:** due copie private dello stesso D40 autorizzato, seconda foto selezionata, zoom 1:1 e centro conservati in quattro stadi (bilineare, AHD, TrueRenderer, bilineare). Il cambio durante la scansione iniziale è incluso. Lo smoke esegue la stessa azione del pulsante, senza automatizzare un clic fisico. **577.296 pixel** nelle quattro regioni fotografiche GPU rispettano la soglia di **1 livello sRGB8** contro CPU; esclusi overlay, compositor e monitor. Sorgente e copie immutate, database privati integri.
- Passati **24 segnali di ricampionamento e identità 1:1**. Il file privato mantiene il nome storico `resampling-macos.json`, ma l'esecuzione è Windows.
- Nuova prova LPAC positiva: sentinella inaccessibile in lettura/scrittura, codice leggibile ma cartella non scrivibile, figli negati, token LPAC verificato. Winsock fallisce all'inizializzazione con 10107: nessuna nuova garanzia universale di isolamento o rete.
- Rigenerato l'inventario Windows con il Cargo.lock corrente: **205 package e 348 notice**, invariati nei contenuti delle licenze. LICENSE, sorgenti upstream, backup originale e testo delle architetture esterno ai blocchi gestiti preservati.

[Riepilogo con hash e provenienza delle prove](../reports/review-followup-fixes-windows.json).

**Limite cache osservato:** durante lo stadio TrueRenderer una persistenza è stata saltata per contesa del lock (errore Windows 33). La visualizzazione è proseguita; al ritorno al bilineare si osservano un hit e un nuovo decode del dettaglio. Non si dichiara quindi riuso completo senza decode né persistenza garantita per questa sequenza. La contesa del writer e i limiti di persistenza del dettaglio restano da qualificare separatamente; non sono una prova di perdita di originali o annotazioni.

Non ripetuta la precedente campagna di 156 sviluppi Nikon: quei risultati restano attribuiti ai rispettivi binari. Restano aperti nuovo bundle Mac/XPC, installazione Windows pulita, colore misurato, display, corpus esteso e prestazioni. Nessun gate R0–R4 chiuso; Standard/Riferimento restano indisponibili.
