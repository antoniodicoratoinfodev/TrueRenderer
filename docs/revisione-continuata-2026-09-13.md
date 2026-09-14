# Revisione ripresa del lavoro non committato — 13 settembre 2026

**Stato dell'audit prima delle correzioni:** quattro difetti confermati. Su successiva richiesta del titolare sono stati corretti; [esiti del 14 settembre e limiti Mac](correzioni-revisione-serale.md). Le riproduzioni negative e i riferimenti sotto conservano la situazione osservata durante la revisione.

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

L'incremento correttivo successivo è documentato in [correzioni della revisione serale](correzioni-revisione-serale.md). Durante questo audit erano stati preservati codice applicativo e vendorizzato, LICENSE, backup originale dell'architettura e testo esterno ai blocchi di avanzamento.
