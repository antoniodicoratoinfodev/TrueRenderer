# Revisione del lavoro non committato prima di main

**Controllo successivo, 13 settembre sera:** la [revisione ripresa](revisione-continuata-2026-09-13.md) conferma quattro ulteriori difetti ancora aperti (test Mac, pulsante impostazioni, PNG `cICP`, alpha TIFF). Le risoluzioni sotto riguardano i casi dell'audit iniziale; non attestano l'assenza di questi nuovi difetti.

**Aggiornamento dopo le correzioni richieste:** gli otto rilievi sono corretti e verificati su Windows, con 89 test Rust e 156 sviluppi Nikon passati; [rapporto delle correzioni](../reports/pre-main-fixes-windows.json). Mac/XPC è rinviato dal titolare. La prima parte sotto conserva l'audit iniziale; la tabella finale indica le risoluzioni.

Data: 13 settembre 2026. Richiesta del titolare: verificare con cura tutto l'incremento locale, **senza commit né staging**. Base locale: `1279012b8cbcd15889ff060143b197d2523212ba` (`fix plan`); la baseline applicativa Mac documentata appartiene a `10123b5`.

**Esito dell'audit iniziale, prima delle correzioni: non promuovere ancora l'intero incremento a main.** La build e i casi Nikon provati funzionano; rimangono difetti riprodotti di isolamento, interpretazione dei bitmap e robustezza, oltre alla qualifica Mac mancante. Non serve dedurre da questi problemi che il demosaicer vada riscritto: diversi rilievi riguardano il port Windows precedente al selettore. Anche le modifiche nuove al selettore/classificazione vanno corrette.

Durante l'audit iniziale il codice applicativo e quello vendorizzato sono rimasti invariati. I rilievi e il [riepilogo senza fotografie](../reports/pre-main-review-windows.json) sotto conservano quella baseline negativa. **Il titolare ha poi richiesto le correzioni:** il seguito è registrato nella sezione finale e in `docs/avanzamento.md`; non attribuire i vecchi risultati ai binari corretti.

## Perimetro ed evidenze

Inventariati 153 file non committati prima degli aggiornamenti di questo audit: diff applicativo, nuovi file, bridge C++, build/packaging, port Windows, cache, protocollo, selettore, demosaic, strumenti e documenti. Letti i percorsi modificati e i contratti pertinenti; la dipendenza LibRaw è stata confrontata con la copia del crate di origine e con correzioni upstream, non sottoposta a revisione completa di ogni parser C++.

Gli hash di codice/binari, i comandi, i log e le riproduzioni sono conservati sotto `var/pre-main-review/`. Il laboratorio usa file sintetici propri per i test negativi. I NEF autorizzati sono aperti in sola lettura; GUI e cache usano una copia privata D40. Nessuna foto, database o credenziale fra i 153 file candidati rilevati da Git; nessun file aggiunto all'indice.

## Rilievi da correggere

### P1 — Il worker Windows può accedere a file non concessi

Riferimenti: `crates/tr-platform/src/isolation.rs:522`, `crates/tr-platform/src/lib.rs:182`, `crates/tr-platform/src/lib.rs:327`, `crates/tr-worker/src/lib.rs:526`.

`CreateRestrictedToken` disabilita privilegi e il gruppo amministratori, ma non imposta restricting SID, livello di integrità basso o AppContainer. Il Job Object limita risorse e processi; il broker concede comunque gli input esterni. La conferma del job nel worker non dimostra una policy di accesso ai file.

**Riprodotto:** un eseguibile di prova avviato con la stessa `Isolation::spawn` del prodotto risulta nel job e riesce a leggere e scrivere due sentinelle sintetiche in una directory sorella, mai concesse tramite handle. Non è stato sfruttato un decoder: la prova misura l'autorità residua del processo. La prova loopback restituisce errore Winsock 10106 (provider non inizializzato); non è evidenza di una regola OS che neghi la rete.

Questo non soddisfa il requisito dell'architettura «nessun accesso oltre handle/buffer concessi» né il test negativo previsto in `docs/piano-windows-raw.md` §4. Prima di abilitare genericamente gli esterni su main occorrono una policy OS effettiva e prove negative filesystem/rete/handle/figli; nel frattempo mantenere esplicito e separato il perimetro dell'esperimento. L'ADR 0007 descrive il refactor iniziale con gate invariato e **non** costituisce l'ADR di autorizzazione della successiva estensione Windows.

Riferimento tecnico: [Microsoft, CreateRestrictedToken](https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-createrestrictedtoken). Evidenza locale: `findings.json`, campi `in_production_job` e `isolation`.

### P1 — La copia LibRaw omette correzioni upstream per input malformati

Riferimenti: `third_party/libraw/libraw/libraw_version.h:23`, `third_party/libraw/src/metadata/tiff.cpp:1035`, `third_party/libraw/src/decoders/load_mfbacks.cpp:485`, `crates/tr-worker/build.rs:162`.

La versione incorporata è 0.21.1. Confermata l'identità byte per byte di 99 file con `libraw_rs_vendor` 1.0.0 locale, escluso il README del progetto. Questo conferma la provenienza locale dichiarata, non la sicurezza della versione.

L'upstream ha successivamente corretto letture fuori limite nel parser TIFF Fuji e nelle correzioni PhaseOne. Nel file locale è ancora presente `rafdata[fi - 15]` senza il controllo aggiunto upstream; mancano anche i successivi controlli delle tabelle PhaseOne. Questi sorgenti entrano nella build. La lista di modelli del demosaicer proprio viene controllata **dopo** l'identificazione LibRaw e non evita l'esposizione al parser.

Fonti primarie: [correzione 66fe663](https://github.com/LibRaw/LibRaw/commit/66fe663), [rilascio 0.21.4](https://github.com/LibRaw/LibRaw/releases/tag/0.21.4). Nessun exploit è stato eseguito e non si attribuisce automaticamente ogni advisory alla configurazione locale. Prima di main: scegliere una versione con le correzioni applicabili o backport verificati, riesaminare licenze/opzioni, versionare le ricette/cache e ripetere le regressioni. La 0.21.4 citata documenta correzioni mancanti, non è una raccomandazione di versione aggiornata al 2026.

### P1 — I PNG esterni saltano il contratto di colore

Riferimenti: `crates/tr-worker/src/lib.rs:191`, `crates/tr-worker/src/lib.rs:235`, `crates/tr-worker/src/lib.rs:304`.

`png_color_source` controlla gAMA/cHRM/sRGB nel decoder del corpus. `portable_decode`, usato per gli esterni Windows, verifica solo ICC e applica sempre la conversione da sRGB codificato. I test sul corpus rimangono verdi mentre l'ingresso effettivo dell'app viola il contratto.

**Riprodotto:** PNG RGB 1×1, valore 128, gAMA=1.0. Il canale lineare dovrebbe conservare circa 0,50196; il decoder restituisce circa **0,21586**, etichettando lo spazio come non dichiarato. L'assenza di primarie non autorizza a ignorare una curva esplicitamente dichiarata. Anche il ramo cHRM viene saltato.

Correzione richiesta: condividere il controllo del colore fra gli ingressi; applicare una trasformata supportata o rifiutare precisamente il metadato non supportato. Aggiungere regressioni attraverso il decoder esterno, non solo chiamando il validatore del corpus. Evidenza: `findings.json`, `png_gamma`.

### P1 — Un TIFF con miniatura viene scambiato per RAW

Riferimenti: `crates/tr-worker/src/lib.rs:205`, `crates/tr-worker/src/lib.rs:465`.

La presenza di un JPEG incorporato viene usata come prova che il file sia RAW. Anche un TIFF ordinario può contenerlo.

**Riprodotto attraverso il broker reale:** TIFF RGB 16 bit 16×8 con una miniatura JPEG 8×4 nella seconda directory. Con il bilineare viene restituito solo il JPEG 8×4 e il formato diventa `RAW · anteprima`; AHD e TrueRenderer rifiutano il TIFF come RAW non supportato. La scelta del motore RAW quindi rompe anche bitmap validi, oltre a perdere dettaglio nel percorso storico.

Separare classificazione del formato, selezione della pagina/immagine primaria e disponibilità di una preview. Aggiungere TIFF con/senza miniatura alle regressioni di tutti i motori. Il ramo Mac condivide `EngineDecoder::is_raw`; il relativo comportamento va provato sul Mac. Per i RAW non riconosciuti da LibRaw va inoltre impedito che il ramo `bitmap()` usi Apple RAW come sostituzione implicita della selezione esplicita: rischio individuato dal flusso, non riprodotto qui su Mac. Evidenza: `ordinary_tiff_through_broker`.

### P2 — Il parser di preview va in panic su un payload di un byte

Riferimento: `crates/tr-worker/src/container.rs:154`.

Il controllo ammette `length > 0`, ma il successivo confronto legge sempre due byte. Un TIFF sintetico di 39 byte con offset sull'ultimo byte e lunghezza 1 supera il primo controllo e produce `range end index 40 out of range for slice of length 39`. La lettura dovrebbe restituire un errore bounded. Nel worker Windows il panic interrompe il processo; non è stata osservata corruzione di memoria Rust o del broker.

Usare un accesso checked alla firma con lunghezza almeno 2 e testare le lunghezze 0/1/2 al bordo del buffer. Evidenza: `one_byte_preview_panics` e log del laboratorio.

### P2 — La quota del job dipende dal primo tipo di sorgente

Riferimenti: `crates/tr-platform/src/lib.rs:448`, `crates/tr-platform/src/lib.rs:634`.

Il job nasce a 384 MiB per il corpus o 2 GiB per gli esterni. Al cambio di tipo la variabile `active_external` cambia, ma un processo già attivo conserva il limite del job precedente.

**Riprodotto con lo stesso broker:** decodifica corpus, poi un NEF D750 a edge 64: `failed to fill whole buffer`. Dopo `recycle()`, lo stesso NEF viene sviluppato correttamente. L'originale conserva il digest. Il decode ridotto richiede comunque lo sviluppo completo. È un problema nei carichi misti o nei riusi del broker fra tipi di sorgente, non un fallimento generalizzato delle cartelle Nikon già provate.

Aggiornare il limite in modo verificabile o ricreare il worker al cambio del dominio di risorse; provare entrambi i versi. Evidenza: `quota-transition.json`.

### P2 — La deroga per hard link confonde creazione richiesta e creazione esclusiva

Riferimento: `apps/desktop/src/cache/directory.rs:218`.

`links_allowed(write, create)` autorizza un file con più link quando `create=true`, anche con `exclusive=false`: l'apertura può aver trovato un file già esistente. Il percorso `cache.lock` usa proprio questa combinazione. La modifica riguarda anche Unix.

**Riprodotto:** il laboratorio apre una sentinella con hard link come file cache esistente e riesce a modificarla attraverso l'API `Directory`. Il prodotto oggi usa quell'handle per un lock: questa prova non dimostra che la normale manutenzione abbia sovrascritto una fotografia. Dimostra che la primitiva non garantisce quanto dichiara il suo controllo.

La deroga deve dipendere da una creazione effettivamente esclusiva, oppure deve restare il controllo dei link sull'handle aperto. Servono casi reali di filesystem, oltre al test corrente che verifica soltanto il predicato. Evidenza: `existing_hardlink_writable_with_create` e `hardlink_canary_changed`.

### P2 — Modifiche in-place con mtime ripristinato non invalidano la sorgente Windows

Riferimento: `crates/tr-platform/src/lib.rs:135`; consumatore: `apps/desktop/src/source_monitor.rs:22`.

Il token Windows combina identità, dimensione, last-write time e creation time. Una modifica della stessa lunghezza con mtime ripristinato lascia invariati tutti questi campi. **Riprodotto** su una sentinella di sei byte: contenuto diverso, stesso token. Il monitor non segnala la modifica e un'immagine già residente può restare obsoleta finché non interviene un altro motivo di caricamento. La verifica SHA degli snapshot su disco non corregge automaticamente questo caso residente.

Qualificare un segnale Windows di modifica più forte o una rivalidazione del contenuto e mantenere esplicito il limite best-effort. Evidenza: `same_length_restored_mtime_token_unchanged`.

## Ulteriori limiti e incongruenze

- `portable_decode` assegna `native_bits=8` anche ai TIFF 16 bit: riprodotto; i campioni passano in float, ma la provenienza è sbagliata. Leggere la profondità dal decoder (`lib.rs:261`).
- `Settings::validate` non controlla la disponibilità del motore: impostazioni Mac con `Apple` vengono accettate su Windows e gli esterni vengono poi rifiutati dal worker. Da gestire come migrazione esplicita e segnalata, senza reinterpretazione silenziosa dei RAW.
- `Isolation::spawn` ricostruisce il percorso eseguibile passando da `binary.display()` (`isolation.rs:666`): il testo può perdere UTF-16 non Unicode. Usare `OsStr::encode_wide` senza conversione lossy. Individuato staticamente, non qualificato con un'installazione in quel percorso.
- La build release Windows importa `MSVCP140.dll`, `VCRUNTIME140.dll` e `VCRUNTIME140_1.dll` (verifica `dumpbin /dependents`). Lo static link di LibRaw non rende autonomo il runtime C++; un pacchetto richiede una scelta e una prova su Windows pulito. Non esiste ancora tale qualifica.
- L'inventario dipendenze pubblicato riguarda la baseline Mac; LibRaw vendorizzato non è un package di `Cargo.lock`. Il candidato necessita di inventario delle sorgenti/opzioni native e runtime, oltre al grafo Cargo. Le licenze vendorizzate e `LICENSE` sono conservate; questo audit non è un parere legale di distribuzione.
- I precedenti testi che citano ADR 0007 come autorizzazione del worker esterno o descrivono il rifiuto dei PNG dichiarati come proprietà di tutti gli ingressi non corrispondono al codice corrente. Le annotazioni di questo audit prevalgono sulle descrizioni storiche del port.

## Verifiche positive e loro limiti

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

## Ordine consigliato prima della promozione

1. Correggere il confine Windows e qualificare la versione nativa LibRaw; mantenere l'esperimento separato fino ai test negativi.
2. Correggere PNG/TIFF e parser preview; trasformare i casi del laboratorio in regressioni dell'ingresso esterno e del broker.
3. Correggere transizione di quota, controllo hard link, osservazione delle sorgenti e provenienza/migrazione impostazioni.
4. Costruire e provare il nuovo bundle Mac, riesaminare packaging/runtime e aggiornare gli inventari del candidato.
5. Ripetere i controlli pertinenti dopo le correzioni; promuovere solo il perimetro effettivamente verificato. Il confronto fotografico/colore rimane un gate separato per eventuali promesse di qualità.

Il laboratorio locale si esegue con `scripts/cargo-local.sh run --manifest-path var/pre-main-review/harness/Cargo.toml --target-dir target --offline`; le modalità `-- --quota` e `-- --screenshot-coverage` riproducono i controlli aggiuntivi. Su Windows va invocato tramite MSYS bash, fuori dalla sandbox del terminale per le prove del token. Log, immagini e percorsi personali restano in `var/` e non vanno pubblicati.

## Correzioni richieste dopo l'audit — 13 settembre

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
