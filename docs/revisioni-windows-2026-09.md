# Revisioni Windows — settembre 2026

Raccolta delle campagne storiche, con risultati positivi e negativi e limiti riferiti ai rispettivi binari. Per lo stato corrente leggere [STATO.md](../STATO.md); i rinvii «ancora aperto» nei testi sotto descrivono la data della campagna, non la situazione attuale.

I nomi originali riportati negli inventari JSON rimangono riferimenti storici; le sezioni seguenti ne conservano il contenuto.

<a id="pre-main"></a>

## Revisione del lavoro non committato prima di main

**Controllo successivo, 13 settembre sera:** la [revisione ripresa](revisioni-windows-2026-09.md#serale) conferma quattro ulteriori difetti ancora aperti (test Mac, pulsante impostazioni, PNG `cICP`, alpha TIFF). Le risoluzioni sotto riguardano i casi dell'audit iniziale; non attestano l'assenza di questi nuovi difetti.

**Aggiornamento dopo le correzioni richieste:** gli otto rilievi sono corretti e verificati su Windows, con 89 test Rust e 156 sviluppi Nikon passati; [rapporto delle correzioni](../reports/pre-main-fixes-windows.json). Mac/XPC è rinviato dal titolare. La prima parte sotto conserva l'audit iniziale; la tabella finale indica le risoluzioni.

Data: 13 settembre 2026. Richiesta del titolare: verificare con cura tutto l'incremento locale, **senza commit né staging**. Base locale: `1279012b8cbcd15889ff060143b197d2523212ba` (`fix plan`); la baseline applicativa Mac documentata appartiene a `10123b5`.

**Esito dell'audit iniziale, prima delle correzioni: non promuovere ancora l'intero incremento a main.** La build e i casi Nikon provati funzionano; rimangono difetti riprodotti di isolamento, interpretazione dei bitmap e robustezza, oltre alla qualifica Mac mancante. Non serve dedurre da questi problemi che il demosaicer vada riscritto: diversi rilievi riguardano il port Windows precedente al selettore. Anche le modifiche nuove al selettore/classificazione vanno corrette.

Durante l'audit iniziale il codice applicativo e quello vendorizzato sono rimasti invariati. I rilievi e il [riepilogo senza fotografie](../reports/pre-main-review-windows.json) sotto conservano quella baseline negativa. **Il titolare ha poi richiesto le correzioni:** il seguito è registrato nella sezione finale e in `STATO.md`; non attribuire i vecchi risultati ai binari corretti.

### Perimetro ed evidenze

Inventariati 153 file non committati prima degli aggiornamenti di questo audit: diff applicativo, nuovi file, bridge C++, build/packaging, port Windows, cache, protocollo, selettore, demosaic, strumenti e documenti. Letti i percorsi modificati e i contratti pertinenti; la dipendenza LibRaw è stata confrontata con la copia del crate di origine e con correzioni upstream, non sottoposta a revisione completa di ogni parser C++.

Gli hash di codice/binari, i comandi, i log e le riproduzioni sono conservati sotto `var/pre-main-review/`. Il laboratorio usa file sintetici propri per i test negativi. I NEF autorizzati sono aperti in sola lettura; GUI e cache usano una copia privata D40. Nessuna foto, database o credenziale fra i 153 file candidati rilevati da Git; nessun file aggiunto all'indice.

### Rilievi da correggere

#### P1 — Il worker Windows può accedere a file non concessi

Riferimenti: `crates/tr-platform/src/isolation.rs:522`, `crates/tr-platform/src/lib.rs:182`, `crates/tr-platform/src/lib.rs:327`, `crates/tr-worker/src/lib.rs:526`.

`CreateRestrictedToken` disabilita privilegi e il gruppo amministratori, ma non imposta restricting SID, livello di integrità basso o AppContainer. Il Job Object limita risorse e processi; il broker concede comunque gli input esterni. La conferma del job nel worker non dimostra una policy di accesso ai file.

**Riprodotto:** un eseguibile di prova avviato con la stessa `Isolation::spawn` del prodotto risulta nel job e riesce a leggere e scrivere due sentinelle sintetiche in una directory sorella, mai concesse tramite handle. Non è stato sfruttato un decoder: la prova misura l'autorità residua del processo. La prova loopback restituisce errore Winsock 10106 (provider non inizializzato); non è evidenza di una regola OS che neghi la rete.

Questo non soddisfa il requisito dell'architettura «nessun accesso oltre handle/buffer concessi» né il test negativo previsto in `docs/adr/0007-porta-decoder-e-build-windows.md` §4. Prima di abilitare genericamente gli esterni su main occorrono una policy OS effettiva e prove negative filesystem/rete/handle/figli; nel frattempo mantenere esplicito e separato il perimetro dell'esperimento. L'ADR 0007 descrive il refactor iniziale con gate invariato e **non** costituisce l'ADR di autorizzazione della successiva estensione Windows.

Riferimento tecnico: [Microsoft, CreateRestrictedToken](https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-createrestrictedtoken). Evidenza locale: `findings.json`, campi `in_production_job` e `isolation`.

#### P1 — La copia LibRaw omette correzioni upstream per input malformati

Riferimenti: `third_party/libraw/libraw/libraw_version.h:23`, `third_party/libraw/src/metadata/tiff.cpp:1035`, `third_party/libraw/src/decoders/load_mfbacks.cpp:485`, `crates/tr-worker/build.rs:162`.

La versione incorporata è 0.21.1. Confermata l'identità byte per byte di 99 file con `libraw_rs_vendor` 1.0.0 locale, escluso il README del progetto. Questo conferma la provenienza locale dichiarata, non la sicurezza della versione.

L'upstream ha successivamente corretto letture fuori limite nel parser TIFF Fuji e nelle correzioni PhaseOne. Nel file locale è ancora presente `rafdata[fi - 15]` senza il controllo aggiunto upstream; mancano anche i successivi controlli delle tabelle PhaseOne. Questi sorgenti entrano nella build. La lista di modelli del demosaicer proprio viene controllata **dopo** l'identificazione LibRaw e non evita l'esposizione al parser.

Fonti primarie: [correzione 66fe663](https://github.com/LibRaw/LibRaw/commit/66fe663), [rilascio 0.21.4](https://github.com/LibRaw/LibRaw/releases/tag/0.21.4). Nessun exploit è stato eseguito e non si attribuisce automaticamente ogni advisory alla configurazione locale. Prima di main: scegliere una versione con le correzioni applicabili o backport verificati, riesaminare licenze/opzioni, versionare le ricette/cache e ripetere le regressioni. La 0.21.4 citata documenta correzioni mancanti, non è una raccomandazione di versione aggiornata al 2026.

#### P1 — I PNG esterni saltano il contratto di colore

Riferimenti: `crates/tr-worker/src/lib.rs:191`, `crates/tr-worker/src/lib.rs:235`, `crates/tr-worker/src/lib.rs:304`.

`png_color_source` controlla gAMA/cHRM/sRGB nel decoder del corpus. `portable_decode`, usato per gli esterni Windows, verifica solo ICC e applica sempre la conversione da sRGB codificato. I test sul corpus rimangono verdi mentre l'ingresso effettivo dell'app viola il contratto.

**Riprodotto:** PNG RGB 1×1, valore 128, gAMA=1.0. Il canale lineare dovrebbe conservare circa 0,50196; il decoder restituisce circa **0,21586**, etichettando lo spazio come non dichiarato. L'assenza di primarie non autorizza a ignorare una curva esplicitamente dichiarata. Anche il ramo cHRM viene saltato.

Correzione richiesta: condividere il controllo del colore fra gli ingressi; applicare una trasformata supportata o rifiutare precisamente il metadato non supportato. Aggiungere regressioni attraverso il decoder esterno, non solo chiamando il validatore del corpus. Evidenza: `findings.json`, `png_gamma`.

#### P1 — Un TIFF con miniatura viene scambiato per RAW

Riferimenti: `crates/tr-worker/src/lib.rs:205`, `crates/tr-worker/src/lib.rs:465`.

La presenza di un JPEG incorporato viene usata come prova che il file sia RAW. Anche un TIFF ordinario può contenerlo.

**Riprodotto attraverso il broker reale:** TIFF RGB 16 bit 16×8 con una miniatura JPEG 8×4 nella seconda directory. Con il bilineare viene restituito solo il JPEG 8×4 e il formato diventa `RAW · anteprima`; AHD e TrueRenderer rifiutano il TIFF come RAW non supportato. La scelta del motore RAW quindi rompe anche bitmap validi, oltre a perdere dettaglio nel percorso storico.

Separare classificazione del formato, selezione della pagina/immagine primaria e disponibilità di una preview. Aggiungere TIFF con/senza miniatura alle regressioni di tutti i motori. Il ramo Mac condivide `EngineDecoder::is_raw`; il relativo comportamento va provato sul Mac. Per i RAW non riconosciuti da LibRaw va inoltre impedito che il ramo `bitmap()` usi Apple RAW come sostituzione implicita della selezione esplicita: rischio individuato dal flusso, non riprodotto qui su Mac. Evidenza: `ordinary_tiff_through_broker`.

#### P2 — Il parser di preview va in panic su un payload di un byte

Riferimento: `crates/tr-worker/src/container.rs:154`.

Il controllo ammette `length > 0`, ma il successivo confronto legge sempre due byte. Un TIFF sintetico di 39 byte con offset sull'ultimo byte e lunghezza 1 supera il primo controllo e produce `range end index 40 out of range for slice of length 39`. La lettura dovrebbe restituire un errore bounded. Nel worker Windows il panic interrompe il processo; non è stata osservata corruzione di memoria Rust o del broker.

Usare un accesso checked alla firma con lunghezza almeno 2 e testare le lunghezze 0/1/2 al bordo del buffer. Evidenza: `one_byte_preview_panics` e log del laboratorio.

#### P2 — La quota del job dipende dal primo tipo di sorgente

Riferimenti: `crates/tr-platform/src/lib.rs:448`, `crates/tr-platform/src/lib.rs:634`.

Il job nasce a 384 MiB per il corpus o 2 GiB per gli esterni. Al cambio di tipo la variabile `active_external` cambia, ma un processo già attivo conserva il limite del job precedente.

**Riprodotto con lo stesso broker:** decodifica corpus, poi un NEF D750 a edge 64: `failed to fill whole buffer`. Dopo `recycle()`, lo stesso NEF viene sviluppato correttamente. L'originale conserva il digest. Il decode ridotto richiede comunque lo sviluppo completo. È un problema nei carichi misti o nei riusi del broker fra tipi di sorgente, non un fallimento generalizzato delle cartelle Nikon già provate.

Aggiornare il limite in modo verificabile o ricreare il worker al cambio del dominio di risorse; provare entrambi i versi. Evidenza: `quota-transition.json`.

#### P2 — La deroga per hard link confonde creazione richiesta e creazione esclusiva

Riferimento: `apps/desktop/src/cache/directory.rs:218`.

`links_allowed(write, create)` autorizza un file con più link quando `create=true`, anche con `exclusive=false`: l'apertura può aver trovato un file già esistente. Il percorso `cache.lock` usa proprio questa combinazione. La modifica riguarda anche Unix.

**Riprodotto:** il laboratorio apre una sentinella con hard link come file cache esistente e riesce a modificarla attraverso l'API `Directory`. Il prodotto oggi usa quell'handle per un lock: questa prova non dimostra che la normale manutenzione abbia sovrascritto una fotografia. Dimostra che la primitiva non garantisce quanto dichiara il suo controllo.

La deroga deve dipendere da una creazione effettivamente esclusiva, oppure deve restare il controllo dei link sull'handle aperto. Servono casi reali di filesystem, oltre al test corrente che verifica soltanto il predicato. Evidenza: `existing_hardlink_writable_with_create` e `hardlink_canary_changed`.

#### P2 — Modifiche in-place con mtime ripristinato non invalidano la sorgente Windows

Riferimento: `crates/tr-platform/src/lib.rs:135`; consumatore: `apps/desktop/src/source_monitor.rs:22`.

Il token Windows combina identità, dimensione, last-write time e creation time. Una modifica della stessa lunghezza con mtime ripristinato lascia invariati tutti questi campi. **Riprodotto** su una sentinella di sei byte: contenuto diverso, stesso token. Il monitor non segnala la modifica e un'immagine già residente può restare obsoleta finché non interviene un altro motivo di caricamento. La verifica SHA degli snapshot su disco non corregge automaticamente questo caso residente.

Qualificare un segnale Windows di modifica più forte o una rivalidazione del contenuto e mantenere esplicito il limite best-effort. Evidenza: `same_length_restored_mtime_token_unchanged`.

### Ulteriori limiti e incongruenze

- `portable_decode` assegna `native_bits=8` anche ai TIFF 16 bit: riprodotto; i campioni passano in float, ma la provenienza è sbagliata. Leggere la profondità dal decoder (`lib.rs:261`).
- `Settings::validate` non controlla la disponibilità del motore: impostazioni Mac con `Apple` vengono accettate su Windows e gli esterni vengono poi rifiutati dal worker. Da gestire come migrazione esplicita e segnalata, senza reinterpretazione silenziosa dei RAW.
- `Isolation::spawn` ricostruisce il percorso eseguibile passando da `binary.display()` (`isolation.rs:666`): il testo può perdere UTF-16 non Unicode. Usare `OsStr::encode_wide` senza conversione lossy. Individuato staticamente, non qualificato con un'installazione in quel percorso.
- La build release Windows importa `MSVCP140.dll`, `VCRUNTIME140.dll` e `VCRUNTIME140_1.dll` (verifica `dumpbin /dependents`). Lo static link di LibRaw non rende autonomo il runtime C++; un pacchetto richiede una scelta e una prova su Windows pulito. Non esiste ancora tale qualifica.
- L'inventario dipendenze pubblicato riguarda la baseline Mac; LibRaw vendorizzato non è un package di `Cargo.lock`. Il candidato necessita di inventario delle sorgenti/opzioni native e runtime, oltre al grafo Cargo. Le licenze vendorizzate e `LICENSE` sono conservate; questo audit non è un parere legale di distribuzione.
- I precedenti testi che citano ADR 0007 come autorizzazione del worker esterno o descrivono il rifiuto dei PNG dichiarati come proprietà di tutti gli ingressi non corrispondono al codice corrente. Le annotazioni di questo audit prevalgono sulle descrizioni storiche del port.

### Verifiche positive e loro limiti

| Verifica | Esito |
|---|---|
| Formattazione, Clippy `--all-targets -D warnings`, build debug | Passati, offline e lockfile bloccato |
| Build release dell'intero workspace | Passata; hash dei due eseguibili nel report |
| Suite Rust Windows | 75 ordinari + 7 integrazioni esplicite = 82 passati |
| Protocollo worker / validazione report Python | 6 + 2 passati |
| Ricampionamento release | 24 casi e controllo 1:1 passati nel test root privato |
| Release D750 | 30 NEF × 3 motori = 90 sviluppi completi passati |
| Release D40 | 22 NEF × 3 motori = 66 sviluppi completi passati |
| Originali | Digest invariati nelle due campagne e nella prova di cambio quota |
| GUI release D40 | Quattro stadi, cambio durante scansione, selezione conservata, ritorno al bilineare senza nuovo decode |
| Superficie GUI | 260.796 pixel confrontati, errore canale massimo 0; ulteriore regione fotografica centrale di 180.056 pixel, errore 0. Controllato che le regioni contengano migliaia di colori, non soltanto sfondo |
| LibRaw vendorizzato | 99 file identici alla copia del crate di origine locale; correzioni upstream mancanti come sopra |
| Nuovo Mac/XPC | Non verificato: il tentativo si ferma sull'assenza di `/usr/bin/codesign` su Windows |

I primi due tentativi non conclusi sono conservati nei log: una variabile del laboratorio indicava un NEF invece della directory richiesta da `TR_RAW_SAMPLE`; una prova release indicava il test root prima della sua creazione. Corretta la preparazione, i controlli sono stati completati senza modificare il prodotto. Non sono fallimenti del decoder nascosti dal conteggio finale.

Il nuovo Mac cambia linkage LibRaw/libc++, worker e impostazioni; il dithering del viewer cambia su entrambi gli OS. Le prove Mac della baseline non qualificano questi binari. Occorrono build e bundle reali, i due servizi XPC, firma/relocation, ingressi bitmap, cambio motore e confronto di superficie. I test Unix esclusi da Windows richiedono anch'essi esecuzione sul target pertinente.

Le 156 elaborazioni Nikon attestano compatibilità funzionale su due fotocamere, non accuratezza cromatica misurata, superiorità su Apple/AHD, rumore/moire, p95 o memoria globale. Le campagne release si sono svolte con altre verifiche concorrenti e cache OS non svuotata: i tempi non costituiscono un benchmark comparativo. R0–R4 e Standard/Riferimento restano aperti.

### Ordine consigliato prima della promozione

1. Correggere il confine Windows e qualificare la versione nativa LibRaw; mantenere l'esperimento separato fino ai test negativi.
2. Correggere PNG/TIFF e parser preview; trasformare i casi del laboratorio in regressioni dell'ingresso esterno e del broker.
3. Correggere transizione di quota, controllo hard link, osservazione delle sorgenti e provenienza/migrazione impostazioni.
4. Costruire e provare il nuovo bundle Mac, riesaminare packaging/runtime e aggiornare gli inventari del candidato.
5. Ripetere i controlli pertinenti dopo le correzioni; promuovere solo il perimetro effettivamente verificato. Il confronto fotografico/colore rimane un gate separato per eventuali promesse di qualità.

Il laboratorio locale si esegue con `scripts/cargo-local.sh run --manifest-path var/pre-main-review/harness/Cargo.toml --target-dir target --offline`; le modalità `-- --quota` e `-- --screenshot-coverage` riproducono i controlli aggiuntivi. Su Windows va invocato tramite MSYS bash, fuori dalla sandbox del terminale per le prove del token. Log, immagini e percorsi personali restano in `var/` e non vanno pubblicati.

### Correzioni richieste dopo l'audit — 13 settembre

| Rilievo | Modifica | Verifica della correzione |
|---|---|---|
| Accesso residuo ai file | LPAC senza capacità, copia del codice in sola lettura/esecuzione, token e Job interrogati dal worker | Sentinella assoluta: lettura/scrittura negate con errore 5; pipe/codice funzionanti; dettagli e limiti in ADR 0009 |
| LibRaw obsoleta | 0.22.2 dal tag ufficiale, calloc del raw store, versione in tutte le ricette e nell'impronta legacy Windows | 106 hash upstream e 79 unità compilate verificati; regressioni native passate |
| PNG esterni | Stesso controllo sRGB/gAMA/cHRM del percorso corpus | Tutti e tre i motori Windows rifiutano gAMA non supportata e cHRM non applicabile; accettano sRGB dichiarato |
| TIFF con miniatura | La miniatura non identifica un RAW; serve riconoscimento LibRaw o dichiarazione CFA/LinearRaw/DNG | TIFF sintetico 16×8 a 16 bit con JPEG 8×4: i tre motori restituiscono la primaria, profondità corretta |
| Preview corta a EOF | Lunghezza minima 2 e accesso ai byte controllato | Payload di 0, 1 e 2 byte non causano panic |
| Quota congelata | Riciclo al passaggio corpus/esterno e viceversa | Tre lavori sullo stesso broker: tre processi, quote 384 MiB/2 GiB/384 MiB e decodifiche riuscite |
| Hard link | Deroga solo per creazione esclusiva | `cache.lock` esistente con hard link rifiutato in scrittura, target invariato, lettura e nuova creazione consentite |
| Invalidazione Windows | ChangeTime oltre a volume/file-id/mtime/dimensione | Riscrittura della stessa lunghezza e ripristino mtime cambiano il token |

Ulteriori correzioni: profondità effettiva dei bitmap a 16 bit; preferenza Apple trasferita su Windows migrata con avviso, backup originale e conservazione delle altre impostazioni; percorso eseguibile UTF-16 esplicito; runtime C/C++ statico MSVC; inventario Windows e notice separati dalla baseline Mac. Il percorso Mac dei motori espliciti riconosce il flag RAW del probe Apple per impedire sostituzioni implicite; la prova nativa è rinviata dal titolare.

Passati 81 test ordinari e 8 integrazioni Rust, fmt/Clippy, build debug/release, regressione LPAC, 6 prove IPC, 2 Python, 24 segnali e 1:1 esatto. **156/156 sviluppi release Nikon passati**, originali invariati anche rispetto all'audit; GUI D40 con quattro stadi e riuso cache passata, 260.796 pixel fotografici a errore zero rispetto alla CPU. [Risultati e hash](../reports/pre-main-fixes-windows.json). Le prove Mac/XPC (rinviate dal titolare) e l'installazione su Windows pulito restano da eseguire, così come i gate di fedeltà cromatica. Nessun commit o staging.

Il vecchio harness filesystem usava sentinelle relative al CWD: il nuovo worker ha una propria cartella del codice, quindi quel vecchio caso non dimostra un divieto di accesso. Usare la nuova regressione con sentinelle assolute, conservando il vecchio rapporto soltanto come baseline dell'audit.

*Fonte storica: `docs/revisione-pre-main.md`.*

<a id="serale"></a>

## Revisione serale e correzioni — 13–14 settembre 2026

Documento unico di audit e correzione. I rilievi sotto sono la baseline negativa storica; le correzioni successive sono verificate su Windows e incluse in `67146e3`. La qualifica nativa Mac/XPC resta aperta. I rapporti JSON conservano hash ed esiti delle singole esecuzioni.

### Audit prima delle correzioni

Recuperata la sessione «Controlla il lavoro non committato», iniziata alle 20:05 locali con la richiesta «controlli tutto il lavoro fatto non ancora committato?». Si è interrotta per limite d'uso prima della risposta finale. Letti messaggi, comandi, risultati e riproduzioni; completati i controlli sospesi. Nessuna modifica applicativa, commit o staging durante l'audit.

Questo controllo segue le correzioni di [revisione-pre-main.md](revisioni-windows-2026-09.md#pre-main): i quattro rilievi sotto riguardano casi ulteriori, non annullano le riproduzioni positive precedenti. Base Git `1279012b8cbcd15889ff060143b197d2523212ba`. Inventariati 522 file candidati prima di questi aggiornamenti documentali, incluse sorgenti native e notice. [Rapporto con hash ed evidenze](../reports/review-continuation-windows.json). Laboratorio privato recuperato in `var/review-current-20260913/`, log estratti e inventario della continuazione nella sottocartella `continuation/`.

### 1. P1 — Un pattern impedisce di compilare i test su macOS

Riferimento: `crates/tr-platform/src/lib.rs:928`, test `real_worker_is_recycled_after_crash`; enum `Owner` a riga 273.

Il refactor ha sostituito un `let ... else` con `let Owner::Pipe(worker, _) = ...`. Su Windows l'enum ha solo `Pipe`; su macOS contiene anche `Xpc`, quindi il pattern è refutabile. Il test è `#[ignore]`, ma viene comunque compilato da `cargo test` e da Clippy con `--all-targets`: la suite Mac si ferma prima di eseguire i test.

**Verifica:** il diff conferma la rimozione del ramo `else`; compilata la riproduzione minima con entrambe le varianti già preparata dalla sessione precedente (`src/mac-pattern-proof.rs` nel laboratorio). Rust restituisce `E0005`, variante `&mut Owner::Xpc(_)` non coperta. È una prova del difetto di compilazione, non una build nativa Mac o una verifica XPC.

**Correzione richiesta:** gestire esplicitamente la variante inattesa nel test del worker su pipe, quindi eseguire i controlli nativi Mac concordati. Non eliminare il test per fare passare la suite.

### 2. P2 — «Applica e salva» perde la selezione al cambio motore

Riferimenti: `apps/desktop/src/ui.rs:2146`, `:751`, `:968`; `crates/tr-app/src/lib.rs:51`.

Con una cartella già caricata, selezionare una foto diversa dalla prima, impostare uno zoom e cambiare motore dalle impostazioni. Il ramo del pulsante pone sempre `self.scanning = true` **prima** di `apply_settings()`. L'invalidazione del motore crede quindi che esista una scansione in corso e ne accoda un'altra. Quando arriva `Event::Scanned`, `replace_items()` azzera selezione, foto corrente e trasformazione, scegliendo la prima foto visibile.

**Verifica:** seguito l'intero percorso nel codice e recuperata la riproduzione che usa il reducer reale: foto corrente da `"2"` a `"0"` dopo il rescan. Il reset dello zoom è esplicito in `replace_items()`. Non è stata eseguita un'interazione nativa con il pulsante in questo audit.

Lo smoke `--raw-engine-smoke` chiama direttamente `apply_settings()` e parte dalla prima foto; non percorre l'azione completa del pulsante. Il suo esito positivo dimostra il cambio motore interno, ma non copre questa regressione del flusso utente.

**Correzione richiesta:** distinguere manutenzione della cache e scansione effettiva; preservare foto/selezione/trasformazione quando si riapplicano le impostazioni. Verificare il percorso del pulsante su una foto diversa dalla prima, anche durante una scansione realmente pendente.

### 3. P2 — I PNG con cICP vengono accettati come sRGB

Riferimento: `crates/tr-worker/src/lib.rs:140`, funzione `png_color_source()` e sua chiamata dal decoder bitmap esterno.

Il resolver controlla `iCCP`, `sRGB`, `cHRM` e `gAMA`, ma ignora `cICP`. PNG con primarie Display P3 o trasferimento lineare vengono accettati e convertiti con la curva/matrice sRGB; la provenienza dichiara «nessun chunk di colore», oppure sRGB se è presente anche quel chunk.

**Verifica:** fixture PNG RGB 1×1 con gli stessi campioni `[128,64,32]`: senza metadati, `cICP=[1,8,0,1]`, `cICP=[12,13,0,1]`, e `cICP` lineare insieme a `sRGB`. Il broker LPAC restituisce per tutti lo stesso pixel Rec.2020 lineare, circa `[0.15293947,0.06222374,0.02098652,1]`, con tutti e tre i motori Windows. Le codifiche dichiarate non sono equivalenti. Le 12 risposte sono nel rapporto privato recuperato.

La [PNG Third Edition, §11.3.2.6](https://www.w3.org/TR/2025/REC-png-3-20250624/#cICP-chunk) definisce primarie, trasferimento e range nel chunk e ne stabilisce la precedenza quando compreso dal decoder. Il contratto di TrueRenderer (§8.2) richiede inoltre di distinguere metadati assenti e codifiche non supportate, senza trasformare queste ultime in sRGB per assunzione.

**Correzione richiesta:** riconoscere `cICP` prima dei fallback, applicare soltanto le codifiche supportate e rifiutare esplicitamente le altre; rendere visibili i conflitti. Aggiornare l'identità del decoder/cache affinché i raster errati già persistiti non sopravvivano alla correzione.

### 4. P2 — I TIFF con alpha associata risultano troppo scuri

Riferimento: `crates/tr-worker/src/lib.rs:300`, conversione dei campioni in `decode_bitmap()`.

Il decoder tratta ogni RGBA come colore non associato e chiama `from_encoded_srgb()`, che applica la curva e poi moltiplica per alpha. Non risolve prima `ExtraSamples` del TIFF: quando RGB è già associato, il risultato subisce una trasformazione errata e un'ulteriore moltiplicazione.

**Verifica:** due TIFF sintetici RGB 1×1 descrivono lo stesso bianco semitrasparente: `ExtraSamples=1`, RGBA `[128,128,128,128]`, e `ExtraSamples=2`, RGBA `[255,255,255,128]`. Attraverso il worker LPAC il primo produce RGB circa `0.108353`, il secondo `0.501960`, con la stessa alpha `0.501961`. Esito identico nei tre motori Windows: sei risposte recuperate, nessuna fotografia coinvolta.

**Correzione richiesta:** risolvere la semantica alpha prima della trasformazione colore e premoltiplicare una sola volta nello spazio lineare; gestire alpha zero e casi ambigui. In alternativa rifiutare esplicitamente il caso non supportato. Anche questa correzione richiede invalidazione della cache bitmap.

### Controlli completati e limiti

- Sessione recuperata: **81 test Rust ordinari + 6 integrazioni** passati, build debug, fmt, Clippy `--all-targets`, sei controlli IPC, verifica LPAC e 18 risposte sulle fixture bitmap. Le due integrazioni con fotografie erano esplicitamente escluse. Gli 89 test e i 156 sviluppi Nikon del rapporto delle correzioni sono una campagna precedente, non ripetuta qui.
- Continuazione: **build release passata**, 24 segnali di ricampionamento e identità 1:1 passati; due regressioni Python passate; verificati 106 hash upstream LibRaw/79 unità compilate/tre ricette, 205 package e 348 notice Windows; riproduzione `E0005` confermata. `git diff --check` e sincronizzazione documentale verificati.
- LPAC: le sentinelle sono rifiutate in lettura/scrittura, il codice è leggibile ma non scrivibile, i figli sono negati. Winsock fallisce all'inizializzazione con 10107; non viene dichiarata una prova esaustiva di accesso alla rete. Resta il perimetro di [ADR 0009](adr/0009-isolamento-worker-windows.md).
- Il rapporto di ricampionamento si chiama ancora `resampling-macos.json` nel laboratorio, ma questa esecuzione è **Windows**. Non è una nuova misura Mac. Il sandbox del terminale aveva inizialmente impedito la creazione del profilo LPAC; la ripetizione autorizzata nell'ambiente Windows normale è passata.
- Gli hash correnti dei worker ricompilati sono registrati separatamente da quelli della precedente campagna Nikon. Non si attribuiscono automaticamente quei risultati fotografici ai nuovi eseguibili.
- La verifica della dipendenza LibRaw riguarda manifest, sorgenti compilate, bridge e ricette; non è un audit completo di ogni parser C++ upstream.
- Mac/XPC, installazione Windows pulita, colore misurato, display e prestazioni restano aperti. Il titolare ha rinviato le prove sul Mac. Nessun gate R0–R4 chiuso; nessuna disponibilità di Standard/Riferimento dichiarata.

L'incremento correttivo successivo è documentato in [correzioni della revisione serale](revisioni-windows-2026-09.md#serale-correzioni). Durante questo audit erano stati preservati codice applicativo e vendorizzato, LICENSE, backup originale dell'architettura e testo esterno ai blocchi di avanzamento.

<a id="serale-correzioni"></a>

### Correzioni ed esiti successivi

### Correzioni applicate

1. **Test worker su Mac:** il test di crash/recovery usa un `match` esaustivo su `Owner`, con rifiuto esplicito della variante XPC inattesa nel test delle pipe. Il test non è stato rimosso. La compilazione nativa Mac e la suite XPC rimangono rinviate dal titolare; la prova minima con entrambe le varianti non le sostituisce.
2. **Applica e salva:** manutenzione cache e scansione hanno stati distinti. Il completamento della manutenzione non azzera una scansione effettiva; salvare le preferenze non inventa un rescan. Pulsante e smoke nativo condividono `start_cache_action()`. Due regressioni del servizio/UI verificano foto diversa dalla prima, selezione multipla, zoom/centro, persistenza delle impostazioni, ritorno al primo motore, svuotamento cache e scansione realmente pendente.
3. **PNG cICP:** riconosciuto sRGB a range completo `[1,13,0,1]`, con provenienza dichiarata. Primarie/trasferimenti/range non supportati vengono rifiutati prima dei fallback sRGB; i conflitti rilevati sono espliciti. Le prove comprendono lineare, P3, PQ/HLG, range limitato e dichiarazioni concorrenti. Questo non implementa la conversione di tali spazi né Little CMS.
4. **TIFF alpha associata:** risolto `ExtraSamples` prima della trasformazione colore. RGB viene deassociato nel dominio dei campioni, convertito e premoltiplicato una sola volta in lineare; alpha zero produce RGBA zero. Alpha ambigua è rifiutata. Prove RGB a 8/16 bit, little/big endian, bianco equivalente e pixel colorati; gray+alpha resta esplicitamente non supportato dall'adattatore corrente.

La ricetta bitmap `bitmap-cicp-alpha-v3` entra nella provenienza e nel fingerprint della cache, escludendo i raster precedenti con interpretazione errata. Aggiunta dipendenza diretta dalla versione TIFF già usata da `image`; nessun sorgente LibRaw upstream modificato.

### Verifiche

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

*Fonte storica: `docs/revisione-continuata-2026-09-13.md`.*

<a id="generale"></a>

## Revisione e correzioni TIFF, fallback RAW e cache — 14 settembre 2026

Documento unico di audit e correzione. I rilievi sotto sono la baseline negativa storica; le correzioni successive sono verificate su Windows e incluse in `67146e3`. La qualifica nativa Mac/XPC resta aperta. I rapporti JSON conservano hash ed esiti delle singole esecuzioni.

### Audit prima delle correzioni

**Le quattro correzioni precedenti sono confermate nei casi verificati su Windows; la revisione generale ha riprodotto altri tre difetti P2, ancora aperti.** Nessuna modifica applicativa, staging o commit in questa revisione. Aggiornati soltanto registro, piano, documenti e rapporto; laboratorio sintetico privato in `var/recheck-20260914/`.

### Rilievi da correggere

#### P2 — Colorimetria TIFF dichiarata ignorata

In `crates/tr-worker/src/lib.rs:285–292`, il resolver tratta i TIFF senza ICC come sRGB assunto, anche quando dichiarano una colorimetria completa. La nuova gestione di `ExtraSamples` risolve l'alpha, ma non legge `TransferFunction`, `WhitePoint` e `PrimaryChromaticities`.

Riproduzione: `linear-srgb.tif`, RGB 8 bit, pixel `[128,128,128]`, tag 301 con tre tabelle lineari `v*257`, bianco D65 e primarie Rec.709 esplicite nei tag 318/319. Tutti e tre i motori Windows, attraverso il broker LPAC, producono RGB lineare circa **0,2158604** e dichiarano «sRGB assunto · nessun profilo incorporato». Il grigio lineare atteso è circa **0,5019608**: il risultato viene scurito applicando indebitamente la curva sRGB. La definizione delle tabelle e dei tag è nella [specifica TIFF 6.0, §20, pp. 83–86](https://www.itu.int/itudoc/itu-t/com16/tiff-fx/docs/tiff6.pdf); il requisito locale è §8.2 dell'architettura.

Correzione richiesta: risolvere questi tag prima della conversione, oppure rifiutare esplicitamente la colorimetria dichiarata non supportata. Aggiungere una regressione sul valore numerico e aggiornare l'identità cache quando cambia l'interpretazione. Il caso P3 con primarie dichiarate è anch'esso accettato come assunto, ma il suo pixel neutro non dimostra da solo un errore di gamut.

#### P2 — Il fallback RAW su anteprima fallisce al confine del worker

In `crates/tr-worker/src/lib.rs:407–412`, `PortablePreview::probe()` restituisce le dimensioni RAW appena LibRaw riconosce il contenitore. Se lo sviluppo viene poi rifiutato, `decode()` alle righe 429–441 restituisce l'anteprima incorporata; il controllo alle righe 757–760 rifiuta dimensioni sorgente diverse dal probe.

Riproduzione con DNG LinearRaw sintetico **32×24**, non Bayer, contenente JPEG **8×8**: il decoder diretto riesce a restituire `RAW · anteprima` con la ragione del rifiuto dello sviluppo; lo stesso file attraverso il broker, col motore bilineare, fallisce con **«Dimensioni diverse dal probe»**. Il fallback dichiarato non è quindi utilizzabile quando il RAW è riconosciuto dal probe e l'anteprima ha un'altra risoluzione. Il rifiuto esplicito degli altri due motori non è conteggiato come difetto.

Correzione richiesta: rendere coerente lo stadio scelto da probe e decode, preservando provenienza, dimensioni e controlli IPC. Non rimuovere semplicemente la validazione delle risposte. Aggiungere una regressione attraverso il worker reale.

#### P2 — Gli hit della cache non aggiornano la data di utilizzo su Windows

In `apps/desktop/src/cache/directory.rs:256`, `open_file(..., false, false, false)` apre il file in lettura senza diritto di scrittura degli attributi. Gli hit in `cache/artifact.rs:198,221,290` e `cache/mod.rs:261` tentano `File::set_times(...)`, ignorandone l'errore. Scadenza e ordinamento della pulizia dipendono proprio da `modified()`.

Riproduzione isolata compilando il modulo `Directory` di produzione: file sintetico con mtime di due giorni fa, apertura identica a quella degli hit e tentativo di aggiornamento. **Errore Windows 5**, timestamp invariato. Controllo positivo con handle scrivibile: timestamp aggiornato. Una voce usata di recente può quindi essere eliminata come vecchia/scaduta, con ulteriori decodifiche. Nessuna prova di perdita di originali o annotazioni.

Correzione richiesta: consentire l'aggiornamento degli attributi mantenendo i controlli su link/reparse point, o registrare l'ultimo utilizzo separatamente; verificare che l'hit cambi realmente il criterio di scadenza/LRU. La prova copre l'handle e l'operazione usati dal codice, non una sessione UI né un'attesa reale di trenta giorni. È distinta dalla contesa writer con errore 33 già registrata nello smoke precedente.

### Verifiche e perimetro

- Rieseguiti con `scripts/cargo-local.sh`, offline: **fmt, Clippy con warning negati, 86 test Rust ordinari e 6 integrazioni**. Tutti passati. Le due integrazioni fotografiche ignorate non sono state rieseguite in questo controllo.
- **36 casi attraverso il broker** su 12 file sintetici e tre motori, con probe/decode e confronto col decoder diretto. Sono osservazioni comprendenti errori attesi e difetti riprodotti, non 36 test di accettazione passati. Fixture invariate dopo la lettura.
- PNG `cICP` sRGB valido dichiarato; P3 rifiutato esplicitamente, matrice/range invalidi rifiutati. Il chunk `cICP` corto viene scartato e appare come assente; `sRGB` con `gAMA` discordante viene accettato senza diagnostica del conflitto. Questi ultimi sono limiti diagnostici rispetto al §8.2, separati dai tre P2. La precedenza di `sRGB` su `gAMA` non dimostra da sola un errore dei pixel ([PNG Third Edition](https://www.w3.org/TR/png-3/)).
- Riletti i diff del port Windows, LPAC/Job Object, snapshot e identità, decoder e demosaic, bridge C++, build/bundle, preferenze/UI, code di decode e cache, script e documentazione. Controllati i nuovi file pertinenti; il codice upstream LibRaw non è oggetto di un audit completo riga per riga.
- Verificati **106 hash upstream LibRaw, 79 unità compilate, tre ricette**, Cargo.lock dell'inventario, **205 package e 348 notice**. `git diff --check` passato. LICENSE, backup originale, sorgenti applicativi, vendor, report preesistenti e indice Git preservati rispetto alla baseline del controllo; testo delle architetture esterno ai blocchi gestiti invariato.
- Corrette alcune accentate e separatori diventati `?` nei paragrafi documentali inseriti durante il recupero precedente; sincronizzate le architetture col registro.

Per match Mac, azione completa delle impostazioni, PNG `cICP` valido e alpha TIFF, restano valide le [prove delle quattro correzioni](revisioni-windows-2026-09.md#serale-correzioni) e il [rapporto precedente con hash](../reports/review-followup-fixes-windows.json). Release, GUI con 577.296 pixel e 21 casi bitmap appartengono a quella campagna; non vengono presentati come nuove esecuzioni qui.

[Rapporto corrente, casi sintetici e hash](../reports/recheck-windows-2026-09-14.json). La baseline e i programmi di riproduzione rimangono nel laboratorio privato, escluso da Git.

Restano aperti il nuovo bundle Mac e i due XPC, rinviati dal titolare, la prova su Windows pulito, contesa/persistenza cache, qualifiche colore/display, corpus RAW esteso e prestazioni. Non ripetuta la campagna fotografica Nikon precedente. I tre P2 richiedono correzioni prima di considerare pronto il lavoro; nessuna chiusura dei gate R0–R4 o disponibilità Standard/Riferimento.

<a id="generale-correzioni"></a>

### Correzioni ed esiti successivi

### Comportamento corretto

1. **TIFF con colorimetria dichiarata:** il percorso portabile controlla `TransferFunction`, `WhitePoint`, `PrimaryChromaticities`, `TransferRange` e `ReferenceBlackWhite`, oltre a ICC e alpha. I tag non ancora applicabili producono un rifiuto esplicito prima dell'assunzione sRGB, anche per descrizioni parziali. Il TIFF lineare dell'audit non viene più visualizzato troppo scuro. Questa correzione non implementa la conversione dei tag TIFF: tali immagini restano non supportate da questo adattatore. Un TIFF senza dichiarazioni conserva l'assunzione sRGB visibile e i pixel attesi. La ricetta `bitmap-tiff-color-v4` invalida il riuso dei raster bitmap precedenti.
2. **Fallback RAW coerente:** il probe LibRaw applica anche il limite della ricetta sui CFA non Bayer. Il motore bilineare sceglie quindi già nel probe l'anteprima incorporata quando lo sviluppo è escluso dai metadati, con le sue dimensioni e la ragione del rifiuto. Decode ripete la stessa scelta. Il DNG sintetico 32×24 con JPEG 8×8 ora attraversa il broker come `RAW · anteprima`, anche con riduzione a 4×4, mantenendo dimensioni sorgente 8×8. I controlli IPC restano attivi. Un errore successivo durante unpack/process di un RAW ammesso dal probe rimane un errore esplicito, senza sostituire lo stadio dopo il probe; non si esegue uno sviluppo completo per leggere i metadati. AHD e motore proprio conservano il rifiuto dei CFA non qualificati.
3. **Timestamp degli hit cache:** `Directory::touch` aggiorna il file già validato. Su Windows usa `ReOpenFile` per ottenere sullo stesso oggetto i soli diritti di lettura/scrittura degli attributi; i normali handle di lettura non acquisiscono il diritto di modificare i pixel. Il conteggio dei link impedisce l'aggiornamento dei timestamp di hard link anche su Unix. Tutti i percorsi di hit, legacy, descrittori e blocchi, usano la stessa operazione. Scadenza e LRU vedono ora l'ultimo utilizzo. La lettura resta utilizzabile se il filesystem o i permessi impediscono il rinnovo: la persistenza continua a essere facoltativa.

Il requisito Windows per modificare i tempi e la riapertura dello stesso oggetto sono documentati da Microsoft: [SetFileTime](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-setfiletime), [ReOpenFile](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-reopenfile). Il rinnovo non riapre il percorso, che potrebbe essere stato sostituito dopo la lettura.

### Verifiche concluse

- **96 test Rust ordinari + 7 integrazioni = 103 passati**, con toolchain locale e dipendenze offline. Cinque nuove regressioni ordinarie, una nuova integrazione LPAC e cinque test cache preesistenti abilitati anche su Windows. Il test specifico dei symlink Unix rimane condizionale.
- Regressioni TIFF: rifiuto della descrizione completa e parziale in probe/decode, controllo numerico del TIFF privo di dichiarazioni; le precedenti regressioni alpha 8/16 bit e PNG restano passate.
- Regressione RAW sia diretta sia col worker reale: provenienza, dimensioni native dell'anteprima, riduzione, rifiuto degli altri motori e input invariato.
- Regressioni cache: un hit cambia realmente l'ordine di espulsione e impedisce la scadenza da inattività; descrittore e tutti i blocchi aggiornati; un handle di lettura non può sovrascrivere i pixel; un hard link non riceve modifiche al timestamp o ai contenuti. I tempi dei file vengono impostati artificialmente, senza attese di giorni.
- Rieseguiti **36 casi sintetici col worker debug e 36 col worker release**, sugli stessi dodici input dell'audit: tutti gli esiti previsti delle correzioni confermati, fixture immutate. Questi sono casi osservati con accettazioni e rifiuti attesi, distinti dai 103 test Rust.
- Passati **build debug/release, fmt, Clippy con warning negati, 6 controlli IPC, 2 regressioni Python, 24 segnali di ricampionamento e identità 1:1**. La prova di ricampionamento richiede il worker LPAC: il primo tentativo nella sandbox è stato negato alla creazione del profilo; l'esecuzione autorizzata fuori dalla sandbox è passata.
- LibRaw invariato: **106 hash upstream, 79 unità compilate, tre ricette**. Cargo.lock e inventario Windows coerenti, **205 package e 348 notice** verificati. Nessuna nuova dipendenza. Le quattro fixture in `crates/tr-worker/tests/fixtures/` sono sintetiche, senza fotografie dell'utente.
- Conservati LICENSE, backup originale, vendor, report preesistenti, modifiche non pertinenti e indice Git. Piano e registro aggiornati; testo delle architetture esterno ai blocchi gestiti preservato dalla sincronizzazione.

[Rapporto con casi, test e hash dei binari](../reports/general-review-fixes-windows.json).

### Limiti ancora aperti

La suite XPC è stata invocata sul percorso del bundle, ma non può partire su questo Windows perché manca `/usr/bin/codesign`. Build nativa Mac, nuovo bundle e i due XPC restano rinviati dal titolare. Non ripetute GUI e campagna fotografica Nikon; i relativi report precedenti non qualificano i nuovi binari. Restano Windows pulito, contesa writer con errore 33, diagnostica PNG malformata/conflittuale già segnalata, conversione TIFF/ICC, corpus esteso, display e prestazioni. Nessun gate R0–R4 chiuso; Standard/Riferimento restano indisponibili.

*Fonte storica: `docs/revisione-generale-2026-09-14.md`.*

<a id="gamma-orientamento"></a>

## Revisione e correzioni gamma PNG / orientamento RAW — 14 settembre 2026

Documento unico di audit e correzione. I rilievi sotto sono la baseline negativa storica; le correzioni successive sono verificate su Windows e incluse in `67146e3`. La qualifica nativa Mac/XPC resta aperta. I rapporti JSON conservano hash ed esiti delle singole esecuzioni.

### Audit prima delle correzioni

**Due ulteriori P2 riprodotti, ancora aperti.** Questa verifica non modifica il codice applicativo e non esegue staging o commit. Laboratorio privato `var/audit-extra-20260914/`; [rapporto con risultati e hash](../reports/additional-review-windows.json).

### P2 — PNG con sola gAMA trattato come sRGB

In `crates/tr-worker/src/lib.rs:180–192`, `gAMA=45455` viene accettato come «gAMA sRGB dichiarato»; il raster passa poi attraverso `from_encoded_srgb` alla riga 390. Il chunk indica invece una gamma approssimativamente 1/2,2 e non identifica la curva sRGB a tratti. Quando manca un segnale prioritario, occorre rispettare la gamma dichiarata. Riferimento: [PNG Third Edition, §§11.3.2.2, 12.1 e 13.13](https://www.w3.org/TR/png-3/#11gAMA).

Riproduzione: PNG RGB8 1×1, sola `gAMA=45455`, nessun `sRGB`, `cICP`, ICC o primarie. Per il grigio `[16,16,16]`, il valore lineare calcolato con la curva dichiarata è `(16/255)^(100000/45455) ≈ 0,00226309`; tutti e tre i motori Windows restituiscono circa **0,00518151**. Per `[128,128,128]`: atteso circa **0,21952305**, osservato **0,21586043**. Le primarie sRGB restano assunte e separate dalla curva; sul neutro D65 il passaggio a Rec.2020 conserva il valore entro l'approssimazione della matrice.

I controlli con chunk `sRGB` e gli stessi campioni danno esattamente gli stessi pixel dei PNG con sola gamma: il decoder sta usando la stessa curva per due dichiarazioni differenti. Confermato col worker debug e release attraverso LPAC.

Correzione richiesta: applicare la funzione di trasferimento dichiarata, distinguendo gamma e primarie, oppure rifiutare esplicitamente questo percorso finché non supportato; correggere la provenienza e invalidare i raster precedenti. Aggiungere regressioni sui toni scuri, sui mezzitoni e sulla precedenza di `sRGB`. Questo difetto numerico è distinto dalle note diagnostiche già aperte su cICP malformato e sRGB/gAMA discordanti.

### P2 — Una directory TIFF secondaria può ruotare l'anteprima principale

In `crates/tr-worker/src/container.rs:135`, `orientation == 1` viene usato come se significasse «non ancora letto». Ma 1 è anche un orientamento esplicito valido. Un tag in una directory successiva può quindi sovrascriverlo; alla riga 184 il valore globale viene attribuito all'anteprima già scelta, anche se appartiene a un'altra directory.

Riproduzione: DNG LinearRaw sintetico con orientamento primario esplicito 1 e JPEG principale **8×4**, più una seconda IFD completa contenente una miniatura grayscale 1×1. Le due varianti differiscono per **un solo byte**, il tag Orientation della seconda IFD: 1 oppure 6. Il RAW e il JPEG principale restano identici. Cambiare soltanto il tag della miniatura fa passare l'anteprima principale da **8×4 / NoTransforms** a **4×8 / Rotate90**, alterando anche i pixel. Confermato nel motore bilineare sia diretto sia attraverso worker LPAC debug/release; gli altri motori rifiutano questo CFA come previsto.

Correzione richiesta: distinguere orientamento assente da orientamento 1 e mantenere la relazione fra directory, immagine e anteprima scelta. Aggiungere una regressione con immagine non quadrata, orientamento primario 1 e orientamento diverso nella miniatura; verificare anche primario assente e JPEG con orientamento proprio. Aggiornare la compatibilità cache pertinente quando cambia il raster.

### Perimetro ed evidenze

- Riletti resolver PNG/TIFF, selezione dello stadio RAW, parser del contenitore, gestione delle dimensioni/orientamento, rinnovo e pubblicazione cache, LRU, writer e confine del mosaico. Non è un nuovo audit completo del codice upstream LibRaw o della piattaforma Mac.
- Sei fixture sintetiche, tre motori: **18 casi col worker debug e 18 col worker release**, ciascuno con probe/decode e confronto col decoder diretto. Sono riproduzioni, non 36 test di accettazione superati.
- Confermati input invariati, differenza di un solo byte fra i DNG, hash dei sorgenti e dei quattro binari identici al [rapporto delle ultime correzioni](../reports/general-review-fixes-windows.json). I 103 test, le build e le altre prove di quel rapporto non sono stati rieseguiti: restano attribuiti agli stessi sorgenti/binari verificati.
- Nessuna modifica applicativa; registro e piano aggiornati, architetture sincronizzate preservando il testo esterno ai blocchi gestiti. Report precedenti, LICENSE, vendor, backup originale e indice Git conservati.

Restano distinti i limiti già aperti: diagnostica PNG, conversione TIFF/ICC, contesa writer, Windows pulito, GUI/corpus esteso e qualifiche colore/prestazioni. Mac/XPC rimane rinviato dal titolare. Le ultime tre correzioni non sono annullate da questi rilievi; i due casi aggiuntivi richiedono interventi propri. Nessun gate R0–R4 chiuso.

<a id="gamma-orientamento-correzioni"></a>

### Correzioni ed esiti successivi

### Comportamento

- **PNG con sola gamma supportata:** `gAMA=45455` applica ora la curva di potenza con esponente `100000/45455`, prima della matrice verso Rec.2020 e della premoltiplicazione. Alpha rimane lineare. La provenienza distingue la curva dichiarata e applicata dalle sole primarie sRGB assunte, tramite `ColorSource::AssumedPrimaries`; non attribuisce fedeltà misurata all'assunzione. Il grigio 16/255 torna a circa **0,00226309**, invece di 0,00518151. `sRGB` e `cICP` prioritari mantengono la curva sRGB a tratti. Le altre gamma e le primarie dichiarate non supportate continuano a essere rifiutate: non è un'implementazione generale di gestione colore.
- **Orientamento delle anteprime RAW:** ogni IFD conserva il proprio orientamento. L'anteprima scelta usa il valore della propria directory, oppure quello della primaria quando manca; directory estranee non possono modificarlo. `Some(1)` significa esplicitamente nessuna rotazione, mentre `None` permette di usare EXIF del JPEG. Il caso dell'audit resta **8×4** anche quando la miniatura secondaria dichiara orientamento 6. Una directory effettivamente selezionata conserva invece il proprio orientamento, anche se diverso dalla primaria.
- **Compatibilità cache:** `bitmap-gamma-ifd-v5` entra nel fingerprint comune delle cache bitmap/anteprima RAW ed esclude i raster precedenti con interpretazione errata. Nessuna cancellazione di originali, cataloghi o backup.

### Verifiche

- **99 test ordinari + 8 integrazioni = 107 passati**, con toolchain locale offline; fmt e Clippy con warning negati passati.
- Tre nuove regressioni ordinarie: curva PNG a 8/16 bit e alpha, precedenza sRGB/cICP; IFD secondaria irrilevante e fallback EXIF JPEG con primario assente/1/6; orientamento della IFD selezionata con fallback alla primaria.
- Nuova integrazione col worker LPAC: le due varianti DNG restituiscono la stessa anteprima 8×4 e gli stessi pixel; sorgenti invariate. Le due integrazioni fotografiche dipendenti dal corpus privato non sono state rieseguite.
- Build debug/release e **18 casi sintetici per ciascun worker** sui sei file dell'audit, con controllo dei valori gamma e delle dimensioni/pixel RAW. Rifiuti degli altri motori per il DNG non Bayer confermati.
- **Sei controlli IPC, due regressioni Python, 24 segnali di ricampionamento e identità 1:1** passati. LibRaw invariato: 106 hash upstream, 79 unità compilate, tre ricette. Cargo.lock/inventario Windows e 348 notice coerenti, nessuna nuova dipendenza.
- Conservati report precedenti, sorgenti non pertinenti, LICENSE, vendor, backup originale e indice Git. Piano/registro aggiornati e architetture sincronizzate senza cambiare il testo esterno ai blocchi gestiti.

[Rapporto con hash ed esiti](../reports/gamma-orientation-fixes-windows.json). Il riferimento della curva gAMA rimane la [PNG Third Edition, §11.3.2.2 e gestione gamma](https://www.w3.org/TR/png-3/#11gAMA).

La suite XPC è stata invocata ma non può partire su Windows senza `/usr/bin/codesign`; nuovo bundle e verifica Mac restano rinviati dal titolare. GUI e campagna Nikon non ripetute. Restano diagnostica PNG malformata/conflittuale, TIFF/ICC non supportati, contesa writer, Windows pulito e qualifiche generali colore/display/prestazioni. Nessun gate R0–R4 chiuso; Standard/Riferimento indisponibili.

*Fonte storica: `docs/revisione-aggiuntiva-2026-09-14.md`.*
