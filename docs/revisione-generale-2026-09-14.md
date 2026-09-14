# Revisione e correzioni TIFF, fallback RAW e cache — 14 settembre 2026

Documento unico di audit e correzione. I rilievi sotto sono la baseline negativa storica; le correzioni successive sono verificate su Windows e incluse in `67146e3`. La qualifica nativa Mac/XPC resta aperta. I rapporti JSON conservano hash ed esiti delle singole esecuzioni.

## Audit prima delle correzioni

**Le quattro correzioni precedenti sono confermate nei casi verificati su Windows; la revisione generale ha riprodotto altri tre difetti P2, ancora aperti.** Nessuna modifica applicativa, staging o commit in questa revisione. Aggiornati soltanto registro, piano, documenti e rapporto; laboratorio sintetico privato in `var/recheck-20260914/`.

## Rilievi da correggere

### P2 — Colorimetria TIFF dichiarata ignorata

In `crates/tr-worker/src/lib.rs:285–292`, il resolver tratta i TIFF senza ICC come sRGB assunto, anche quando dichiarano una colorimetria completa. La nuova gestione di `ExtraSamples` risolve l'alpha, ma non legge `TransferFunction`, `WhitePoint` e `PrimaryChromaticities`.

Riproduzione: `linear-srgb.tif`, RGB 8 bit, pixel `[128,128,128]`, tag 301 con tre tabelle lineari `v*257`, bianco D65 e primarie Rec.709 esplicite nei tag 318/319. Tutti e tre i motori Windows, attraverso il broker LPAC, producono RGB lineare circa **0,2158604** e dichiarano «sRGB assunto · nessun profilo incorporato». Il grigio lineare atteso è circa **0,5019608**: il risultato viene scurito applicando indebitamente la curva sRGB. La definizione delle tabelle e dei tag è nella [specifica TIFF 6.0, §20, pp. 83–86](https://www.itu.int/itudoc/itu-t/com16/tiff-fx/docs/tiff6.pdf); il requisito locale è §8.2 dell'architettura.

Correzione richiesta: risolvere questi tag prima della conversione, oppure rifiutare esplicitamente la colorimetria dichiarata non supportata. Aggiungere una regressione sul valore numerico e aggiornare l'identità cache quando cambia l'interpretazione. Il caso P3 con primarie dichiarate è anch'esso accettato come assunto, ma il suo pixel neutro non dimostra da solo un errore di gamut.

### P2 — Il fallback RAW su anteprima fallisce al confine del worker

In `crates/tr-worker/src/lib.rs:407–412`, `PortablePreview::probe()` restituisce le dimensioni RAW appena LibRaw riconosce il contenitore. Se lo sviluppo viene poi rifiutato, `decode()` alle righe 429–441 restituisce l'anteprima incorporata; il controllo alle righe 757–760 rifiuta dimensioni sorgente diverse dal probe.

Riproduzione con DNG LinearRaw sintetico **32×24**, non Bayer, contenente JPEG **8×8**: il decoder diretto riesce a restituire `RAW · anteprima` con la ragione del rifiuto dello sviluppo; lo stesso file attraverso il broker, col motore bilineare, fallisce con **«Dimensioni diverse dal probe»**. Il fallback dichiarato non è quindi utilizzabile quando il RAW è riconosciuto dal probe e l'anteprima ha un'altra risoluzione. Il rifiuto esplicito degli altri due motori non è conteggiato come difetto.

Correzione richiesta: rendere coerente lo stadio scelto da probe e decode, preservando provenienza, dimensioni e controlli IPC. Non rimuovere semplicemente la validazione delle risposte. Aggiungere una regressione attraverso il worker reale.

### P2 — Gli hit della cache non aggiornano la data di utilizzo su Windows

In `apps/desktop/src/cache/directory.rs:256`, `open_file(..., false, false, false)` apre il file in lettura senza diritto di scrittura degli attributi. Gli hit in `cache/artifact.rs:198,221,290` e `cache/mod.rs:261` tentano `File::set_times(...)`, ignorandone l'errore. Scadenza e ordinamento della pulizia dipendono proprio da `modified()`.

Riproduzione isolata compilando il modulo `Directory` di produzione: file sintetico con mtime di due giorni fa, apertura identica a quella degli hit e tentativo di aggiornamento. **Errore Windows 5**, timestamp invariato. Controllo positivo con handle scrivibile: timestamp aggiornato. Una voce usata di recente può quindi essere eliminata come vecchia/scaduta, con ulteriori decodifiche. Nessuna prova di perdita di originali o annotazioni.

Correzione richiesta: consentire l'aggiornamento degli attributi mantenendo i controlli su link/reparse point, o registrare l'ultimo utilizzo separatamente; verificare che l'hit cambi realmente il criterio di scadenza/LRU. La prova copre l'handle e l'operazione usati dal codice, non una sessione UI né un'attesa reale di trenta giorni. È distinta dalla contesa writer con errore 33 già registrata nello smoke precedente.

## Verifiche e perimetro

- Rieseguiti con `scripts/cargo-local.sh`, offline: **fmt, Clippy con warning negati, 86 test Rust ordinari e 6 integrazioni**. Tutti passati. Le due integrazioni fotografiche ignorate non sono state rieseguite in questo controllo.
- **36 casi attraverso il broker** su 12 file sintetici e tre motori, con probe/decode e confronto col decoder diretto. Sono osservazioni comprendenti errori attesi e difetti riprodotti, non 36 test di accettazione passati. Fixture invariate dopo la lettura.
- PNG `cICP` sRGB valido dichiarato; P3 rifiutato esplicitamente, matrice/range invalidi rifiutati. Il chunk `cICP` corto viene scartato e appare come assente; `sRGB` con `gAMA` discordante viene accettato senza diagnostica del conflitto. Questi ultimi sono limiti diagnostici rispetto al §8.2, separati dai tre P2. La precedenza di `sRGB` su `gAMA` non dimostra da sola un errore dei pixel ([PNG Third Edition](https://www.w3.org/TR/png-3/)).
- Riletti i diff del port Windows, LPAC/Job Object, snapshot e identità, decoder e demosaic, bridge C++, build/bundle, preferenze/UI, code di decode e cache, script e documentazione. Controllati i nuovi file pertinenti; il codice upstream LibRaw non è oggetto di un audit completo riga per riga.
- Verificati **106 hash upstream LibRaw, 79 unità compilate, tre ricette**, Cargo.lock dell'inventario, **205 package e 348 notice**. `git diff --check` passato. LICENSE, backup originale, sorgenti applicativi, vendor, report preesistenti e indice Git preservati rispetto alla baseline del controllo; testo delle architetture esterno ai blocchi gestiti invariato.
- Corrette alcune accentate e separatori diventati `?` nei paragrafi documentali inseriti durante il recupero precedente; sincronizzate le architetture col registro.

Per match Mac, azione completa delle impostazioni, PNG `cICP` valido e alpha TIFF, restano valide le [prove delle quattro correzioni](revisione-continuata-2026-09-13.md#correzioni) e il [rapporto precedente con hash](../reports/review-followup-fixes-windows.json). Release, GUI con 577.296 pixel e 21 casi bitmap appartengono a quella campagna; non vengono presentati come nuove esecuzioni qui.

[Rapporto corrente, casi sintetici e hash](../reports/recheck-windows-2026-09-14.json). La baseline e i programmi di riproduzione rimangono nel laboratorio privato, escluso da Git.

Restano aperti il nuovo bundle Mac e i due XPC, rinviati dal titolare, la prova su Windows pulito, contesa/persistenza cache, qualifiche colore/display, corpus RAW esteso e prestazioni. Non ripetuta la campagna fotografica Nikon precedente. I tre P2 richiedono correzioni prima di considerare pronto il lavoro; nessuna chiusura dei gate R0–R4 o disponibilità Standard/Riferimento.

<a id="correzioni"></a>

## Correzioni ed esiti successivi

## Comportamento corretto

1. **TIFF con colorimetria dichiarata:** il percorso portabile controlla `TransferFunction`, `WhitePoint`, `PrimaryChromaticities`, `TransferRange` e `ReferenceBlackWhite`, oltre a ICC e alpha. I tag non ancora applicabili producono un rifiuto esplicito prima dell'assunzione sRGB, anche per descrizioni parziali. Il TIFF lineare dell'audit non viene più visualizzato troppo scuro. Questa correzione non implementa la conversione dei tag TIFF: tali immagini restano non supportate da questo adattatore. Un TIFF senza dichiarazioni conserva l'assunzione sRGB visibile e i pixel attesi. La ricetta `bitmap-tiff-color-v4` invalida il riuso dei raster bitmap precedenti.
2. **Fallback RAW coerente:** il probe LibRaw applica anche il limite della ricetta sui CFA non Bayer. Il motore bilineare sceglie quindi già nel probe l'anteprima incorporata quando lo sviluppo è escluso dai metadati, con le sue dimensioni e la ragione del rifiuto. Decode ripete la stessa scelta. Il DNG sintetico 32×24 con JPEG 8×8 ora attraversa il broker come `RAW · anteprima`, anche con riduzione a 4×4, mantenendo dimensioni sorgente 8×8. I controlli IPC restano attivi. Un errore successivo durante unpack/process di un RAW ammesso dal probe rimane un errore esplicito, senza sostituire lo stadio dopo il probe; non si esegue uno sviluppo completo per leggere i metadati. AHD e motore proprio conservano il rifiuto dei CFA non qualificati.
3. **Timestamp degli hit cache:** `Directory::touch` aggiorna il file già validato. Su Windows usa `ReOpenFile` per ottenere sullo stesso oggetto i soli diritti di lettura/scrittura degli attributi; i normali handle di lettura non acquisiscono il diritto di modificare i pixel. Il conteggio dei link impedisce l'aggiornamento dei timestamp di hard link anche su Unix. Tutti i percorsi di hit, legacy, descrittori e blocchi, usano la stessa operazione. Scadenza e LRU vedono ora l'ultimo utilizzo. La lettura resta utilizzabile se il filesystem o i permessi impediscono il rinnovo: la persistenza continua a essere facoltativa.

Il requisito Windows per modificare i tempi e la riapertura dello stesso oggetto sono documentati da Microsoft: [SetFileTime](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-setfiletime), [ReOpenFile](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-reopenfile). Il rinnovo non riapre il percorso, che potrebbe essere stato sostituito dopo la lettura.

## Verifiche concluse

- **96 test Rust ordinari + 7 integrazioni = 103 passati**, con toolchain locale e dipendenze offline. Cinque nuove regressioni ordinarie, una nuova integrazione LPAC e cinque test cache preesistenti abilitati anche su Windows. Il test specifico dei symlink Unix rimane condizionale.
- Regressioni TIFF: rifiuto della descrizione completa e parziale in probe/decode, controllo numerico del TIFF privo di dichiarazioni; le precedenti regressioni alpha 8/16 bit e PNG restano passate.
- Regressione RAW sia diretta sia col worker reale: provenienza, dimensioni native dell'anteprima, riduzione, rifiuto degli altri motori e input invariato.
- Regressioni cache: un hit cambia realmente l'ordine di espulsione e impedisce la scadenza da inattività; descrittore e tutti i blocchi aggiornati; un handle di lettura non può sovrascrivere i pixel; un hard link non riceve modifiche al timestamp o ai contenuti. I tempi dei file vengono impostati artificialmente, senza attese di giorni.
- Rieseguiti **36 casi sintetici col worker debug e 36 col worker release**, sugli stessi dodici input dell'audit: tutti gli esiti previsti delle correzioni confermati, fixture immutate. Questi sono casi osservati con accettazioni e rifiuti attesi, distinti dai 103 test Rust.
- Passati **build debug/release, fmt, Clippy con warning negati, 6 controlli IPC, 2 regressioni Python, 24 segnali di ricampionamento e identità 1:1**. La prova di ricampionamento richiede il worker LPAC: il primo tentativo nella sandbox è stato negato alla creazione del profilo; l'esecuzione autorizzata fuori dalla sandbox è passata.
- LibRaw invariato: **106 hash upstream, 79 unità compilate, tre ricette**. Cargo.lock e inventario Windows coerenti, **205 package e 348 notice** verificati. Nessuna nuova dipendenza. Le quattro fixture in `crates/tr-worker/tests/fixtures/` sono sintetiche, senza fotografie dell'utente.
- Conservati LICENSE, backup originale, vendor, report preesistenti, modifiche non pertinenti e indice Git. Piano e registro aggiornati; testo delle architetture esterno ai blocchi gestiti preservato dalla sincronizzazione.

[Rapporto con casi, test e hash dei binari](../reports/general-review-fixes-windows.json).

## Limiti ancora aperti

La suite XPC è stata invocata sul percorso del bundle, ma non può partire su questo Windows perché manca `/usr/bin/codesign`. Build nativa Mac, nuovo bundle e i due XPC restano rinviati dal titolare. Non ripetute GUI e campagna fotografica Nikon; i relativi report precedenti non qualificano i nuovi binari. Restano Windows pulito, contesa writer con errore 33, diagnostica PNG malformata/conflittuale già segnalata, conversione TIFF/ICC, corpus esteso, display e prestazioni. Nessun gate R0–R4 chiuso; Standard/Riferimento restano indisponibili.
