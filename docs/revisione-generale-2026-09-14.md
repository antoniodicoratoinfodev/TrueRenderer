# Revisione generale del lavoro non committato — 14 settembre 2026

**Aggiornamento successivo:** i tre P2 descritti nella baseline qui sotto sono stati corretti su richiesta del titolare e verificati su Windows. Vedere [correzioni e limiti](correzioni-ricontrollo-generale.md) e [nuovo rapporto](../reports/general-review-fixes-windows.json). Le riproduzioni negative seguenti restano conservate come evidenza dell'audit.

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

Per match Mac, azione completa delle impostazioni, PNG `cICP` valido e alpha TIFF, restano valide le [prove delle quattro correzioni](correzioni-revisione-serale.md) e il [rapporto precedente con hash](../reports/review-followup-fixes-windows.json). Release, GUI con 577.296 pixel e 21 casi bitmap appartengono a quella campagna; non vengono presentati come nuove esecuzioni qui.

[Rapporto corrente, casi sintetici e hash](../reports/recheck-windows-2026-09-14.json). La baseline e i programmi di riproduzione rimangono nel laboratorio privato, escluso da Git.

Restano aperti il nuovo bundle Mac e i due XPC, rinviati dal titolare, la prova su Windows pulito, contesa/persistenza cache, qualifiche colore/display, corpus RAW esteso e prestazioni. Non ripetuta la campagna fotografica Nikon precedente. I tre P2 richiedono correzioni prima di considerare pronto il lavoro; nessuna chiusura dei gate R0–R4 o disponibilità Standard/Riferimento.
