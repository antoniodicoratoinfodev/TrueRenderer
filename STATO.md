# TrueRenderer — stato e piano di sviluppo

Aggiornato: 25 settembre 2026.

Fonte unica per stato corrente, prossime attività, caselle operative e registro degli incrementi. Sostituisce i precedenti piano, avanzamento e documento di ripresa.

- [Punto di ripresa](#punto-di-ripresa)
- [Piano operativo](#piano-operativo)
- [Sviluppo fotografico: regolazioni e correzioni ottiche](#piano-sviluppo-fotografico)
- [Matrice dei requisiti e budget](#matrice-dei-requisiti-e-budget)
- [Registro delle verifiche e degli incrementi](#registro-delle-verifiche-e-degli-incrementi)

Specifica: [architettura](docs/TrueRenderer-Architettura.md). Evidenze dettagliate: [rapporti](STATO.md#registro-delle-verifiche-e-degli-incrementi). Decisioni e progetti: [indice documentale](docs/README.md). Vincoli di lavoro: [AGENTS.md](AGENTS.md).

## Regola di aggiornamento

Per ogni incremento aggiornare qui implementato, verificato, aperto e mancante, con una breve nota e i collegamenti alle prove. Aggiornare le caselle pertinenti senza ripetere il resoconto in altre sezioni. I dettagli storici conservano il perimetro della loro campagna e non certificano i binari successivi. Modificare specifiche e ADR solo quando cambiano requisiti o decisioni; README solo quando cambiano informazioni utili all'utente. Aggiungere rapporti quando esistono nuove prove da documentare.

Le architetture rimandano a questo file e non ne incorporano il contenuto. `python3 scripts/sync-docs.py` aggiorna i rimandi e l'appendice della specifica anteprime: serve quando cambiano le sue fonti, non a ogni aggiornamento di stato. Il backup originale v1.2 resta immutato.

## Punto di ripresa

### Verifica viewer/export Windows — 25 settembre

Richiesta del titolare: riassumere le modifiche non committate e verificare app, immagini visualizzate ed export su tutti i motori disponibili. Usato un NEF D750 autorizzato dalla cartella Download; fotografie, copie, librerie ed export restano in `var/`, originali invariati.

- [x] Suite sul codice corrente: fmt, Clippy workspace senza warning, build, **190 test Rust ordinari + 9 aggiuntivi**, due test Python, otto controlli protocollo, 24 segnali e superficie nativa RTX 3060/Vulkan passati. Il wrapper PowerShell esterno ha restituito 1 classificando stderr nativo come errore; la suite `set -eu` ha raggiunto il proprio marcatore finale positivo e i rapporti sono passati. Evidenze in `var/verify-avGx1KC4/` e log separato; non attribuito quel codice al risultato dei test.
- [x] D750 su **Bilineare, AHD e TrueRenderer × neutra/modificata × PNG16/TIFF16**: 12 casi, **291.852.288 pixel / 1.167.409.152 canali RGBA**, zero differenze nei codici esportati. Riapertura isolata e anteprima d'uscita entro **1/255** a 1:1 e nei 24 fit 1400/600. Il render esteso resta distinto dalla prova d'uscita: nei fit modificati scarti fino a 117/255, coerenti con il diverso ordine di proiezione sRGB e filtraggio; non dichiarata parità dell'anteprima provvisoria. **19/19** controlli di esportazione/riapertura formati passati; questa campagna non include le fixture FITS.
- [x] Prova UI iniziale con calcolo CPU: superficie esatta sui tre motori, sviluppo salvato e griglia; export bilineare passato, AHD/TrueRenderer rifiutati perché le anteprime occupavano ancora gli slot. Corretto soltanto il probe affinché attenda richieste immagini e miniature modificate prima dell'export; nessuna modifica al renderer o all'encoder. Build, fmt, Clippy desktop su tutti i target e 75 test desktop passati dopo la correzione.
- [x] Ripetizione sul binario aggiornato con una copia della fotografia isolata e GPU forzata: tre motori, superficie 859×574 entro 1/255, sviluppo/griglia ed export PNG16 6032×4032 passati. Un tentativo sulla cartella di 30 RAW durante compilazioni concorrenti è scaduto a 180 s; ripetizione bilineare sulla cartella completa senza compilazioni passata, nessun rifiuto di memoria. Il timeout resta conservato, senza dedurne una causa certa o una qualifica prestazionale. Hash dell'originale e della copia invariati.
- [x] Corretto il taglio di «Prima/Dopo» nel pannello Sviluppo: le tre azioni vanno a capo quando la larghezza disponibile non basta. Build workspace e fmt passati; catture native 1440×940 controllate in italiano (pulsante intero sulla seconda riga) e inglese (tre pulsanti sulla stessa riga). Prova italiana di sviluppo, superficie d'uscita ed export passata. Nel controllo aggiuntivo inglese un tooltip sovrapposto al segnale radiale ha fatto fallire il confronto di `08-grid-large`: interferenza visibile nella cattura conservata, senza alterare le soglie. [Rapporto della correzione](reports/develop-buttons-layout-windows-2026-09-25.json); evidenze separate in `var/develop-buttons-2026-09-25/`. Nessuna modifica alla pipeline fotografica.

[Rapporto numerico con hash distinti prima/dopo la sola correzione del probe](reports/windows-viewer-export-parity-2026-09-25.json). Evidenze private in `var/parity-2026-09-25-t4sv70va/`, `var/parity-ui-isolated-2026-09-25/` e `var/parity-folder-final-2026-09-25/`; negativi iniziali conservati. Controllate visivamente griglia/viewer/confronto sintetici e catture della fotografia. Nessun nuovo test Apple/macOS/XPC, nessuna qualifica del monitor fisico o universale dei RAW/ICC; JPEG verificato per riapertura, non per identità dopo compressione. Nessun gate SF/R0–R4 chiuso.

### Correzione ICC Windows — 24 settembre

Seguito alla richiesta «sistema»: risolvere la riapertura JPEG/TIFF rimasta negativa nella campagna Windows precedente, conservando il contratto colore e il confinamento del decoder.

- [x] Riprodurre il rifiuto con un test fallente; applicare ICC RGB v2/v4 matrice/TRC tramite Little CMS 2.19 statico nel worker Windows, relativo senza BPC verso Rec.2020/D65 fp32. Versionare la ricetta bitmap Windows per distinguere le cache. Profilo dichiarato nell'ispettore; nessuna rimozione dei metadati o assunzione sRGB sostitutiva.
- [x] Distinguere ICC TIFF illeggibile da assente e controllare i segmenti JPEG APP2 completi/univoci prima di accettare il profilo. Limiti 4 MiB/1.024 tag; rifiuto esplicito di LUT, Gray/CMYK, DeviceLink, profili malformati e conflitti PNG. Alpha associata rimossa prima della trasformata e riapplicata nel working; valori estesi ammessi soltanto con TRC analitiche identità.
- [x] Regressioni worker: **51 test passati**, due integrazioni separate. Otto profili generati (quattro spazi × ICC v2/v4), **39.304 pixel** confrontati con lettura indipendente di matrici/curve e trasformata analitica entro 0,00025 assoluto; test JPEG/TIFF16/TIFF float, alpha zero/quasi zero/parziale, dominio esteso e metadati corrotti. Aggiornati README, contratto ADR 0009 e inventario Windows con licenza nativa Little CMS; storico conservato.
- [x] Suite finale `scripts/dev-windows.ps1 verify --gui`: **190 test Rust ordinari e 9 aggiuntivi**, due test Python, fmt, Clippy senza warning, build workspace, otto controlli protocollo e 24 segnali. Prova grafica RTX 3060/Vulkan e confronto superficie passati. LPAC/Job conserva i rifiuti di lettura/scrittura privata e secondo processo; Winsock 10107 resta soltanto la misura osservata, non una qualifica esaustiva della rete.
- [x] Campagna export/FITS sui binari finali: **20/20 controlli passati**, comprese le nove riaperture JPEG/TIFF prima negative. JPEG, TIFF16 e TIFF float riaperti anche nella UI con librerie distinte: sviluppo salvato, prova d'uscita confrontata col raster CPU, griglia ed export PNG16 completati; digest della sorgente selezionata verificato e invariato. [Rapporto numerico e hash dei binari](reports/windows-icc-verification-2026-09-24.json); dati sintetici isolati in `var/windows-icc-5rysclvq/`, suite in `var/verify-heqS927h/`. Build utilizzabile in `target/debug/truerenderer.exe` con `tr-worker.exe` accanto.

Restano fuori dal perimetro ICC LUT/Gray/CMYK, gestione monitor e qualifica colore universale, corpus RAW privato, NVDA, installatore firmato e nuova prova macOS/XPC. Nessun gate SF/R0–R4 chiuso; originali e dati durevoli esclusi dalle prove.

### Verifica e ambiente Windows — 24 settembre

Richiesta del titolare: verificare l'app su Windows e completare l'ambiente di sviluppo.

- [x] Installare Rust 1.98.1 MSVC, rustfmt e Clippy nelle cartelle locali ignorate `.tools/cargo` e `.tools/rustup`; scaricare le dipendenze di `Cargo.lock`. Completata l'installazione interrotta di Visual Studio Build Tools 2026 (18.10.12217.157), MSVC 14.51.36231 e Windows SDK 10.0.26100.0; nessun riavvio richiesto. Git Bash e Python 3.14 già presenti.
- [x] Build nativa debug del workspace e Clippy su tutti i target con warning negati. Corretto l'errore Windows dei test `Location` aggiungendo la verifica UTF-16 dei percorsi distinti con identica visualizzazione lossy.
- [x] Aggiungere `scripts/dev-windows.ps1`, che richiama `scripts/cargo-local.sh` tramite Git Bash con ambiente completo. Adattare `verify.sh` a Python funzionante (escludendo gli alias Store), suffisso `.exe` e percorsi nativi del worker; ogni campagna usa una nuova radice `var/verify-*`, senza sovrascrivere rapporti storici o libreria personale. Rapporti di campionamento/superficie identificati per OS.
- [x] Riprodurre due problemi intermittenti nella campagna grafica: barra flottante dell'ispettore sovrapposta al bordo dell'anteprima e coordinate registrate prima della stabilizzazione del layout iniziale. Riservato spazio alla barra; il probe attende geometria invariata per 250 ms, senza allargare la soglia di un livello su 255. Evidenze negative conservate nelle rispettive radici di verifica.
- [x] Suite finale `scripts/dev-windows.ps1 verify --gui`: **183 test ordinari e 9 prove aggiuntive**, fmt, Clippy, build, otto controlli protocollo e 24 segnali; codice di uscita zero. Superficie su **NVIDIA RTX 3060/Vulkan**, SDR Bgra8Unorm: suite e tre ripetizioni mirate senza differenze nei canali confrontati. Schermate sintetiche controllate visivamente. Sviluppo salvato, griglia, anteprima d'uscita 871×581 esatta rispetto al raster CPU ed export PNG16 1440×960 completati sul binario finale, sorgente invariata.
- [x] Probe LPAC/Job: lettura/scrittura della sentinella e scrittura accanto al codice negate, secondo processo negato, lettura codice consentita, Winsock non inizializzato (10107); non una qualifica esaustiva di rete. Campagna codec su tre motori: **11/20 controlli passati**, PNG8/16 e DNG lineare riaperti, due prove FITS passate; **9/20 negativi** su riapertura JPEG/TIFF16/TIFF float32 per ICC incorporato, deliberatamente rifiutato dal decoder portabile in attesa della qualifica colore R1. Nessuna rimozione del profilo o fallback silenzioso. Isolamento/codec provati prima delle sole correzioni finali UI/probe; hash distinti conservati. [Rapporto numerico, risultati negativi e identità dei binari](reports/windows-verification-2026-09-24.json).

Restano ICC Windows, corpus RAW privato (tre test esclusi), calibrazione/display fisico, NVDA, installatore firmato e qualifica completa delle piattaforme; nessun gate SF/R0–R4 chiuso e nessuna nuova prova macOS. Build disponibile in `target/debug/truerenderer.exe`, worker accanto; ambiente e dati delle prove ignorati da Git. Originali, dati durevoli, licenze e architettura originale immutati.

### Revisione di anteprima d'uscita e colore — 24 settembre

Richiesta del titolare: ricontrollare il lavoro precedente e migliorarlo dove necessario. Revisione di trasformata d'uscita, profili ICC, alpha, risultati asincroni, riserve memoria e strumenti di qualifica; le precedenti campagne restano legate ai loro binari.

- [x] Riprodurre con test fallente l'incoerenza del profilo TIFF float32: bianco e `chad` non coerenti col PCS D50. ColorSync già superava il confronto dei campioni; il difetto riguarda i metadati e l'interoperabilità. Corretto il profilo lineare dalle primarie/bianco Rec.2020, con la stessa trasformazione per coloranti e adattamento e descrizione esplicita; nessuna modifica ai campioni fp32. Controllati anche i valori delle primarie ricostruite dai tag.
- [x] Riprodurre e correggere crediti temporanei trattenuti dalle anteprime dopo il calcolo: nel caso sintetico 3.145.728 byte riservati per 1.398.256 residenti. Viewer, prova d'uscita e miniature restituiscono lo scratch dopo la piramide; i consumatori ancora attivi conservano i crediti residenti fino all'ultimo rilascio. Corretto anche il blocco della prova D750 a 2 GiB causato dalla precedente riserva di tre raster: ora l'ammissione conta i livelli trattenuti, lo scratch orizzontale conservativo e le tabelle del filtro; la sorgente neutra possiede già i propri crediti. Nessuna riduzione di precisione o risoluzione, nessuna nuova qualifica RSS.
- [x] Regressioni native generate su TIFF float32 con negativi/oltre uno e alpha, PNG/TIFF16 con alpha zero, quasi zero, parziale, quasi opaca e opaca. Passate prima/dopo: nessun falso difetto attribuito al decoder.
- [x] Rafforzare il probe SDR: tutti i pixel, rampe e colori alternati fuori gamut/trasparenti, 1:1 e fit non intero, formato richiesto effettivamente raggiunto e attesa del fotogramma corretto. Dodici casi CPU/GPU × SDR8/10/16F × scala passati anche sul bundle definitivo. Massimi errori normalizzati: SDR8 0,001961, SDR10 0,000962, SDR16F 0,000489. I sei casi precedenti cambiavano la finestra, senza ridurre la fotografia.
- [x] Riprodurre sulla D750 esterna un blocco sfuggito alle prove sintetiche: ricetta legata al token di scansione, risultato legato allo SHA-256, confronto diretto incompatibile. Viewer e miniature ora richiedono la corrispondenza attraverso la cache della sorgente corrente; l'export usa l'identità convalidata nello snapshot del broker. Nessuna riscrittura delle revisioni durevoli. Passate regressioni su identità errate, cache assente/invalida, sostituzione del file e rifiuto del trasporto pipe.
- [x] Suite definitiva `scripts/verify.sh --gui`: **182 test ordinari e 13 integrazioni**, fmt/Clippy/build, otto controlli protocollo, 24 segnali e superficie nativa. Tre prove private escluse dalla suite standard. Bundle `dist/TrueRenderer-output-proof-review-final.app`: due XPC, timeout/recupero e rifiuto fault injection; 26 controlli export/FITS con riapertura, compreso il nuovo profilo TIFF float.
- [x] D750 nell'app su **quattro motori**, ciascuno con libreria separata e 2 GiB configurati: ricetta salvata, prova d'uscita, griglia ed export PNG16 nativo completati. Confronto numerico superficie GPU/riferimento CPU entro **1/255**; Apple 1573×1050 con 45 canali differenti, gli altri 1571×1050 con 55/34/47 canali (bilineare/AHD/TrueRenderer). Export 6016×4016 o 6032×4032, header 16 bit RGBA e tutti i CRC PNG verificati; sorgente invariata. Un rifiuto di ammissione recuperato per caso, nessun errore finale.

Rapporto completo privato: `var/output-proof-review-2026-09-24/private-reports/output-proof-review-macos.json`. Fotografie, export e catture D750 restano privati in `var/output-proof-review-2026-09-24/`; i rapporti storici non sono sovrascritti. La nuova prova UI misura la superficie rispetto alla prova CPU e il completamento/struttura degli export: la campagna precedente di 16 confronti integrali PNG/TIFF conserva il proprio binario e non viene contata come ripetuta qui. Restano calibrazione/display fisico, corpus più ampio, limiti dell'osservazione esterna e compatibilità ICC universale; nessun gate SF/R0–R4 chiuso.

### Anteprima d'uscita e precisione colore — 24 settembre

Richiesta del titolare: correggere gli scarti misurati e conservare la massima precisione colore disponibile. «Verifica resa finale» costruisce ora una copia sRGB16 del render nativo **prima** della piramide: trasformata, clamp e quantizzazione comuni a encoder PNG/TIFF16 e viewer. Regolazioni e sorgente restano Rec.2020 fp32 esteso; nessun intermedio a 8 bit nella prova d'uscita. La modalità resta attiva durante le regolazioni, richiede qualità Piena, rifiuta una sorgente ridotta e distingue risultati/errori in volo per modalità; usa gli stessi crediti di memoria. Il ritorno al render esteso abilita l'ispezione working e il contagocce, escluso dai campioni già proiettati.

- [x] Regressioni su PNG/TIFF16 riaperti con lettore indipendente, clipping, alpha zero/parziale/quasi-opaca e tutti i mip; la quantizzazione dell'alpha determina il filtro della copia. Test UI su ricette neutre/modificate, sorgente nativa obbligatoria e cambio modalità durante un calcolo. Working originale conservato.
- [x] Suite `scripts/verify.sh --gui` ripetuta dopo la correzione ICC: 178 test ordinari, 11 integrazioni, fmt/Clippy/build, otto controlli protocollo, 24 segnali e superficie nativa. Tre test privati esclusi dalla suite standard; la D750 viene provata separatamente sotto.
- [x] D750 su quattro motori × neutra/modificata × PNG16/TIFF16: **16 casi, 388.493.312 pixel / 1.553.973.248 canali RGBA**, tutti i campioni esportati esatti; anteprima entro **1/255** a 1:1 e in tutti i 32 fit 1400/600, contro il massimo precedente di 123/255. Prima prova TIFF16: otto canali nei due casi bilineari a 2/255 a 1:1; risultato negativo conservato. Corretto il profilo sRGB di JPEG/TIFF16: coloranti e bianco PCS D50, `chad` D65→D50 al posto della matrice Bradford di base dei default; campioni codificati invariati. Regressione sui tag serializzati e ripetizione positiva di PNG e TIFF sul medesimo bundle finale. Sorgente invariata.
- [x] Prova UI con readback numerico dell'anteprima d'uscita: GPU 1683×1122, un solo canale differente di 1/255. Passate sei combinazioni SDR8/10/16F × CPU/GPU, 26 controlli sintetici export/FITS e due XPC con recupero/rifiuto fault injection sul bundle definitivo `dist/TrueRenderer-output-proof-icc.app`; schermata sintetica controllata visivamente. Dati isolati in `var/d750-output-proof-2026-09-24/`.

Rapporto completo privato: `var/output-proof-review-2026-09-24/private-reports/d750-output-proof-macos.json`. README e contratti aggiornati; originali, libreria personale, backup, licenze e architettura originale immutati.

La precisione numerica non certifica calibrazione del monitor, gamut fisico o fedeltà cromatica della fotocamera. Il percorso corrente resta SDR sRGB; TIFF float32 conserva lo sviluppo esteso e il suo profilo non cambia in questo incremento. La prova sRGB16 va attivata esplicitamente: anteprima rapida, griglia e render esteso conservano i rispettivi contratti. Parità JPEG/PNG8/resize/profili estesi e gate SF/R0–R4 restano distinti; il JPEG è qui verificato per esportabilità/riapertura, non per identità dopo compressione.

### Confronto D750 fra viewer ed export — 24 settembre

Richiesta del titolare: verificare la corrispondenza della stessa fotografia fra visualizzazione ed export su tutti i motori; autorizzato il confronto numerico al posto degli screenshot. Campagna su copia privata D750 già autorizzata, con hash iniziale verificato, libreria personale esclusa. Nuovo comando opt-in `--verify-photo-export-parity <sorgente> --root <radice-isolata>`: quattro motori, ricetta neutra e regolazioni esplicite, PNG16 nativo, confronto di ogni canale esportato e riapertura nel decoder isolato; resa CPU 1:1 e fit, anteprima rapida misurata separatamente. Fotografie ed export rimangono in `var/`.

- [x] Completare gli otto casi: **194.246.656 pixel / 776.986.624 canali RGBA** confrontati, zero differenze nei codici PNG16 nativi; riapertura attraverso ImageIO/ColorSync e resa CPU 1:1 entro un livello su 255 per tutti i motori e le due ricette. Sorgente invariata. Apple 6016×4016, gli altri 6032×4032: confronti sempre interni allo stesso motore.
- [x] Misurare anche fit a 1400 e 600 px: la soglia di un livello fallisce in sette casi su otto. Massimo scarto RGB su 255, neutro/modificato: Apple **66/123**, LibRaw bilineare **1/94**, LibRaw AHD **4/66**, TrueRenderer **62/117**. Scostamenti localizzati: al massimo 1,88% dei canali oltre un livello nel caso Apple modificato; la media bassa non chiude la parità. Il PNG taglia gamut/estremi prima del filtro, il viewer nativo filtra prima della trasformata d'uscita. Lettore PNG Rust indipendente e trasformata sRGB analitica riproducono invece il PNG riaperto entro un livello in tutti i 16 fit: verificata la causa distinta dall'import. Le anteprime rapide modificate differiscono ulteriormente perché applicano la ricetta dopo la riduzione; sono provvisorie.
- [x] Nuovo strumento di qualifica, fmt/Clippy tutti i target desktop, build debug/release e due XPC con timeout/recupero e rifiuto fault injection sul bundle misurato `dist/TrueRenderer-photo-parity.app`. Rapporto numerico privato: `var/output-proof-review-2026-09-24/private-reports/d750-photo-export-parity-macos.json`. Export e render privati in `var/d750-export-parity-2026-09-24/var/photo-export-parity-IUXyyd/`. Comando ausiliario `--verify-png-projection <cartella-prova>` riproduce la verifica indipendente; non richiede decoder RAW. Nessuna fotografia aggiunta ai file pubblici.
- [x] Implementare e qualificare un'anteprima d'uscita con trasformata/gamut/quantizzazione del formato prima della piramide, per confrontare export e viewer alla medesima scala: seguito nell'incremento «Anteprima d'uscita e precisione colore» sopra. Nel binario di questa prima campagna «Verifica resa finale» applicava la ricetta nativa senza simulare la proiezione prima del filtro; i risultati negativi restano storici.

Limiti: un solo NEF D750 e una ricetta modificata, PNG16 nativo; nessuna nuova misura GUI/GPU/compositore/display fisico, JPEG/TIFF/DNG o export ridimensionato. Il titolare ha preferito il confronto numerico agli screenshot. Nessuna parità universale né gate SF/R0–R4 dichiarati conclusi.

### Revisione degli ultimi otto commit — 23 settembre

Controllo del tratto `aa8898c^..65634ad`: sviluppo fotografico, salvataggio/cronologia, cache e selezione motore RAW, export/DNG/FITS, memoria e mip, presentazione SDR, caricamento cartelle, documentazione e screenshot. Revisione dei percorsi modificati e regressioni mirate; non una certificazione esaustiva di ogni combinazione o del display fisico.

- [x] Correggere cinque difetti: il 201º annullamento consecutivo poteva rendere il record della cronologia illeggibile (ora limite verificato prima della scrittura e bottone coerente); la cache singola impediva alle due foto modificate del confronto A/B di restare pronte (ora massimo due risultati indipendenti sotto gli stessi crediti); il viewer poteva usare un'anteprima di un motore diverso da quello congelato nella ricetta; viewer e miniature modificati perdevano il flag di opacità, scegliendo il filtro sbagliato; un risultato rapido ancora in volo poteva bloccare «Verifica resa finale» sul mip ridotto (ora il livello fa parte della validità del risultato e dell'errore).
- [x] Aggiungere quattro regressioni e ampliare quella della verifica nativa: 201 revisioni/200 undo con riapertura e redo, due foto A/B, provenienza del motore, confronto di tutti i mip opachi/trasparenti e passaggio rapido→nativo durante un calcolo. Cronologia, A/B e corsa della verifica finale riprodotti con test fallenti prima delle rispettive correzioni. Nessuna riparazione automatica di eventuali cronologie personali già danneggiate.
- [x] Passare `scripts/verify.sh --gui`: 175 test ordinari, 11 integrazioni, fmt/Clippy/build, otto controlli protocollo, 24 segnali e superficie nativa. Tre test su fotocamere private esclusi. Verificati 332 link locali e gli hash dei quattro screenshot pubblicati, mantenendo le loro evidenze storiche.
- [x] Sul nuovo `dist/TrueRenderer-audit-v1.app` passati due XPC con timeout/recupero e rifiuto fault injection, 26 controlli sintetici export/FITS, sei combinazioni SDR8/10/16F × CPU/GPU, Sviluppo/griglia, sei layout navigazione, dodici catture Preferenze e caricamento cartella 12/12 in primo piano/background. PNG16 modificato esatto rispetto allo sviluppo nativo in entrambi gli slot e sorgente invariata. Quattro nuove catture controllate visivamente; A/B e corsa della verifica finale coperti dai test di stato UI, senza nuova campagna di interazione A/B nativa. [Rapporto, hash del bundle e riproduzione](reports/recent-commits-audit-macos.json); log e dati isolati in `var/recent-commits-audit/`. Correzioni locali, senza commit/push in questa revisione.

Restano aperti i gate SF0–SF2/R0–R4 e le attività della verticale seguente. La parità export/vista misurata riguarda lo sviluppo nativo: l'export ridimensionato usa integrazione d'area e richiede una futura anteprima d'uscita e una qualifica propria. Il JSON della libreria conserva annotazioni/preferiti, non ricette e cronologia fotografica; queste sono comprese nel backup SQLite, mentre la portabilità delle ricette resta da implementare. Nessuna nuova campagna RAW privati, memoria fisica/p95, Windows o display fisico. Originali, libreria personale, backup durevoli, licenze e architettura originale immutati.

### Sviluppo fotografico — prima verticale avviata il 23 settembre

Il titolare ha autorizzato l'implementazione del [piano SF0–SF10](#piano-sviluppo-fotografico) descritto nella [specifica e ADR 0011](docs/progetto-sviluppo-fotografico.md). Questa consegna avvia SF0–SF2, senza dichiarare conclusa alcuna fase: i rispettivi gate completi e R0–R4 restano aperti.

- **Implementato:** ricetta fotografica schema/processo 1 con identità, esposizione, luminosità, contrasto, alte luci, ombre, bianchi, neri, punto intermedio della curva tonale, temperatura/tinta **relative RGB** e saturazione. Il primo contagocce neutralizza un pixel RGB nativo opaco e non tagliato, limita la correzione a ±100 e salva l'azione nella cronologia; non modifica il WB RAW. Pannello Sviluppo IT/EN, bozza durante il gesto, Prima/Dopo, verifica esplicita dalla risoluzione nativa, cronologia durevole undo/redo e migrazione `library.sqlite` 2→3 con backup verificato. Il motore RAW è congelato per foto alla prima revisione. L'export JPEG/PNG/TIFF applica la ricetta nativa congelata per foto, legata al digest della sorgente; DNG lineare/RAW conservano i rispettivi contratti tecnici. La preview rapida del viewer da mip ≤1024 px è dichiarata provvisoria; griglia, filmstrip e anteprima dell'ispettore applicano la ricetta su mip ≤512 px con cache asincrona a crediti di memoria, badge Prima/Modificata/In calcolo/Errore e istogramma della stessa anteprima modificata. Il calcolo nativo può rifiutare la richiesta sotto quota memoria.
- **Verificato:** ricette identità, alpha/EV/rampe/validazione, neutralizzazione RGB reversibile e rifiuto dei campioni invalidi, cronologia e ripristino da backup su libreria sintetica; due miniature indipendenti, invalidazione della bozza, istogramma della vista modificata e export PNG16 con alpha invariata. `scripts/verify.sh --gui` passato fuori dal sandbox del terminale: 171 test ordinari, 11 integrazioni, fmt/Clippy/build, otto controlli protocollo, 24 segnali e smoke nativo. Sul bundle `dist/TrueRenderer-develop-v7.app` passati due XPC, timeout/recupero, rifiuto fault injection e confronto **pixel per pixel esatto** fra sviluppo nativo ed export PNG16 con esposizione e correzione RGB in entrambi gli slot su una fixture sintetica, con sorgente invariata; [rapporto e identità](reports/photo-edit-rgb-picker-macos.json). Dal medesimo bundle passati gli smoke nativi Sviluppo/griglia, navigazione compatta e Preferenze, con le [quattro schermate del README](reports/readme-screenshots-macos.json). Le [verifiche v6](reports/photo-edit-preview-consistency-macos.json) e [v4](reports/photo-edit-increment-macos.json) restano storiche. Le tre prove su fotocamere private restano escluse dalla suite standard. I risultati non qualificano il display fisico o una resa fotografica equivalente ad altri software.
- **Aperto:** confronto numerico finale vista/export su corpus esteso e RAW, interazione nativa del contagocce e del pannello su finestra compatta/200%, media robusta 5×5/11×11 e incertezza dei campioni, memoria e p95 con RAW grandi, differenze fra motori, qualità fotografica dei controlli, gestione di una sorgente offline o sostituita durante una bozza. L'istogramma dell'ispettore misura il mip modificato, non l'export nativo. Nessun gate SF0–SF2 completato.
- **Mancante:** WB RAW qualificato e Auto, vividezza/profili/colore avanzato, ottica e prospettiva, Texture/Chiarezza/Foschia, nitidezza/rumore, maschere/ritocco, preset/versioni/batch, GPU dei nodi, Windows nativo e qualificazione SF10. Nessun file personale, `var/library.sqlite`, `var/backups`, originale v1.2 o licenze modificati in questa consegna.

<a id="incremento-corrente--memoria-delle-immagini-grandi"></a>

### Incremento precedente — memoria delle immagini grandi

Richiesta del titolare: proseguire dalle attività aperte. Priorità scelta: investigare il superamento fisico 45 MP/Piena e D750, senza modificare precisione, originali o limiti manuali. Baseline `d4e539b`, già pubblicata.

- [x] Riprodurre il caso sintetico 45 MP: la baseline interrompe il decoder per quota XPC; picco aggregato 4.298.577.848 byte di footprint con 4 GiB configurati. Traccia per processo aggiunta al probe; evidenza negativa in `var/large-navigation-2ktqz1xj/`.
- [x] Isolare le cause con probe nativo e `vmmap`: circa 1,2 GiB di regioni malloc grandi vuote ancora residenti, oltre alle risorse Core Image. Il solo `clearCaches` non risolve; riciclare XPC sopra 384 MiB riduce memoria ma introduce attese di circa dieci secondi e viene scartato. Prove in `var/native-memory-investigation/` e candidato scartato in `var/raw-performance-sxevtax1/`.
- [x] Implementare filtro CPU per fasce (circa 8 MiB di intermedio), mapping anonimi privati per allocazioni Rust macOS ≥4 MiB/allineamento ≤4096 e contesti Core Image per job da 12 MP. Il broker conserva il precedente riuso XPC. Passati 34 test core: bit dei livelli contro scalare completo, bordi/alpha, crescita/riduzione/zero/allineamento e fallimento allocazione con vecchi dati conservati. Contratto in ADR 0006.
- [x] Qualificare `dist/TrueRenderer-memory-final.app`: 12 casi nativi CPU/GPU, 120 azioni e 12 controlli 1:1 esatti. I quattro casi 45 MP/Piena restano entro 4 GiB (massimi RSS 3.529.080.832 / footprint 2.354.565.800 byte); gli otto D750 sui quattro motori entro 2 GiB (1.875.853.312 / 2.002.455.912 byte). Zero campioni incompleti; sorgenti di prova invariate. [Rapporto e riproduzione](reports/large-image-memory-macos.json).
- [x] Confrontare due copie D750 senza readback: RSS aggregato 2.579.742.720→1.595.686.912 byte, footprint 2.555.579.920→1.570.917.232; 48 hash delle code mip identici. Somma dei tempi dei 24 stadi 37,21→37,14 s: un passaggio sequenziale, senza controllo cache OS/carico né qualifica statistica.
- [x] Completare `scripts/verify.sh --gui`: 163 test ordinari + 11 integrazioni, fmt/Clippy/build, otto controlli protocollo, 24 segnali e superficie nativa. Passati 51 controlli export/FITS e due XPC con timeout/recupero e rifiuto fault injection sul bundle definitivo; identità uguali a quelle delle campagne memoria. Tre test automatici su fotocamere private saltati dalla suite standard; le prove RAW isolate sopra ne mantengono distinto il perimetro. Evidenze complete in `var/memory-qualification-roqj1kth/`, inventario dipendenze e appendici aggiornati.
- [x] Revisione dei file per GitHub e pulizia `dist` richiesta dal titolare: eliminati 11 bundle precedenti e `.DS_Store`, conservato soltanto `TrueRenderer-memory-final.app` con SHA-256 verificati. Sorgenti, fixture sintetiche, rapporti numerici/screenshot e licenze restano versionati; sette log grezzi e il dump `reports/bundle-stack.txt` restano locali e ignorati, insieme a build, dati privati, database, configurazioni locali, backup temporanei e credenziali. Nessuna modifica a `var/library.sqlite` o `var/backups`. I percorsi dei bundle precedenti nei registri/rapporti sono riferimenti storici e non indicano più copie presenti in `dist`.
- [x] Aggiornare le quattro schermate del README dal bundle Mac corrente: viewer, griglia, navigazione compatta e preferenze Prestazioni, tutte sul corpus sintetico con libreria separata. Tre smoke nativi riusciti; i PNG pubblicati conservano SHA-256 e identità del bundle nel [rapporto schermate](reports/readme-screenshots-macos.json). Le vecchie immagini `navigator-*` restano come evidenza della campagna storica.

Incremento completato nel perimetro misurato: PNG sintetico 45 MP e due copie D750 autorizzate. Restano da ampliare cartelle/fotocamere, pressione fisica OS, contabilità driver e Windows; nessun gate universale memoria fisica o R0–R4 chiuso. RSS e footprint sono somme campionate dei processi, possono contare pagine condivise più volte e perdere picchi brevi. Le campagne precedenti restano documentate separatamente; le prove del candidato scartato con riavvii XPC non certificano quello finale.

### Alta precisione e FITS — incremento pubblicato

Commit e push delle ottimizzazioni RAW completati prima dell'analisi: `c15a8b3` su `origin/main`, identità remota verificata. Il titolare ha approvato l'implementazione del [piano alta precisione/FITS](#piano-alta-precisione-fits) e l'esportazione fotografica, specificando **DNG lineare e DNG RAW come operazioni distinte**. Contratto aggiornato in [ADR 0010](docs/esportazione-precisione-fits.md).

Implementati export selezione da sviluppo nativo con motore congelato, destinazione no-clobber e annullamento: JPEG qualità 1–100, PNG 8/16 con tre compressioni lossless, TIFF16, TIFF float32 lineare esteso, DNG lineare16 e DNG mosaico separati. Il mosaico conserva l'area attiva Nikon D750/D40, non margini ottici, MakerNotes o il NEF originale. Entrambi i DNG si riaprono con LibRaw bilineare/AHD; Apple RAW li rifiuta e il motore mosaico TrueRenderer mantiene la propria allowlist. Limitazione esposta nella UI, nessuna sostituzione silenziosa del motore. Non sono copie archivistiche.

Presentazione SDR8 predefinita e SDR10/SDR16F opt-in al riavvio: percorso CPU/GPU senza intermedio fotografico a 8 bit, coppia formato+sRGB esplicita, fallback compatibile e diagnostica. FITS in sola lettura: prima immagine 2D primary/IMAGE, BITPIX 8/16/32/−32, BSCALE/BZERO/BUNIT/BLANK, NaN/Inf distinti, istogramma nativo e campionatore f64 dai byte originali; proxy con copertura e stretch lineare/asinh. Ricampionamento scientifico CPU dichiarato. UI dei nuovi controlli IT/EN; descrizioni tecniche e alcuni esiti del worker restano italiani.

- [x] Suite `scripts/verify.sh --gui`: 160 test ordinari, 11 integrazioni, Clippy, otto controlli protocollo, 24 segnali di ricampionamento e superficie nativa. Ulteriore prova privata D750: ogni campione attivo esportato identico, sorgente invariata. Test sintetici su alpha, oltre 256 livelli PNG16, float32 bit-exact, qualità/compressione, quote, collisioni/cancellazione, DNG/CFA/calibrazione e FITS ostili/scaling/HDU/invalidi.
- [x] Probe della presentazione nel bundle finale: sei combinazioni 8/10/16F × CPU/GPU con resize e alpha; rampa 1.024 campioni → 256/1.024/1.024 livelli. [Dati numerici](reports/presentation-precision-macos.json). Non misura pannello, collegamento video o HDR.
- [x] Due servizi XPC del bundle finale, recupero e rifiuto fault injection: [qualifica](reports/xpc-qualification-macos.json). Firma ad hoc e supervisione RSS non diventano garanzie di rilascio.
- [x] UI nativa su libreria separata: FITS 64×32 con un BLANK, asinh e lettura campione originale; export JPEG e PNG16 1600×900 dal gradiente sintetico, destinazione privata, esito visibile. Evidenze in `var/export-ui-tnzv4q/`; nessuna preferenza o libreria personale usata.
- [x] Consolidare il [rapporto export/FITS](reports/export-fits-macos.json): 51 controlli isolati (24 export del gradiente sui quattro motori + due slot FITS, 24 export D750 + un mosaico RAW), hash originali invariati e identità del bundle uguali al rapporto XPC. Riproduzione con `--verify-exports`, `--export-source` su copia autorizzata e `scripts/report-export-verification.py`. Collisione UI PNG verificata con suffisso `-1` e byte identici, file precedenti conservati. Licenze vendorizzate/inventario aggiornati, LICENSE e originale v1.2 immutati; link locali e diff controllati. Commit/push richiesti soltanto dopo queste verifiche.

Bundle: `dist/TrueRenderer-export-final.app`. Restano aperti: memoria fisica già oltre budget, contabilità esatta delle swapchain/driver, campagne prestazionali statistiche, Windows nativo, device-loss/multi-monitor ad alta precisione, dither e display fisico/HDR. FITS64, cubi, compressione, selettore HDU, WCS, fotometria ed export FITS esclusi. Nessun gate R0–R4 chiuso; P1–P3 e F0–F2 completi della proposta originaria non sono dichiarati conclusi.

### Velocità RAW, griglia e filmstrip — 20–21 settembre

Richiesta: ridurre il tempo alla comparsa delle immagini, soprattutto miniature della griglia e filmstrip del viewer, sui NEF D750 autorizzati in Download e sui quattro motori Mac. Piano: baseline release con copie private e cataloghi separati; misurare prima miniatura/completamento, cambio vista e riuso RAM/SSD; ottimizzare trasferimento fp32 e riuso dei livelli compatibili; verificare pixel, cancellazione/priorità, quote, GUI e XPC; consegnare un bundle separato. Ricette, originali, isolamento e distinzione Standard/Piena restano vincoli di accettazione.

- [x] Registrare baseline riproducibile dei 30 D750 sui quattro motori: 144 stadi griglia/viewer-filmstrip/ritorno, Standard/Piena, freddo/riaperto; originali invariati. Evidenze private in `var/raw-performance-9dc4j0hc/`.
- [x] Ottimizzare trasferimento fp32 in blocchi, mip canonico nel worker prima di IPC e riuso asincrono dei livelli RAM/SSD compatibili. Nove nuove regressioni su protocollo, grafo, quote, derivati e richieste Retina. Passato `scripts/verify.sh --gui` (145 ordinari + 11 integrazioni, Clippy, protocollo e superficie nativa). Il test TIFF preesistente non disponibile nel sandbox del terminale passa fuori da esso.
- [x] Confrontare tempi e pixel sul bundle finale: 144 stadi per binario e 1.440 confronti delle code mip fp32 delle miniature, zero differenze. Griglia fredda sui 30 D750: Apple Piena 55,77→51,20 s; LibRaw bilineare 31,77→22,47 s; AHD 37,59→29,16 s; TrueRenderer 32,85→25,08 s. Standard: riduzione 7,6–22,1%. Prima miniatura filmstrip: mediana per gruppo circa 18–24 ms nel probe; il dettaglio maggiore del viewer Piena continua a svilupparsi quando necessario. Riapertura griglia: 0,90–1,73 s per 30 foto, zero decode in sette casi su otto e uno nel caso bilineare Piena (persistenza opzionale, non garantita). Misure di disponibilità CPU, non del display; una campagna sequenziale senza controllo della cache OS/carico né qualifica p95. [Rapporto A/B](reports/raw-preview-performance-macos.json); candidato finale privato in `var/raw-performance-mvd_wzur/`, precedente candidato distinto in `var/raw-performance-45433wtg/`.
- [x] Completare prove RAW native nel bundle finale: cinque stadi motore/filmstrip/1:1 e cinque confronti superficie con errore nullo; caricamento dei 30 D750 in primo piano e background; 80 azioni su quattro motori × CPU/GPU, con pixel 1:1 esatti. Nei casi GPU restano fallback CPU dichiarati sotto quota e copertura talvolta parziale durante le transizioni, non continuità universale. Due XPC, timeout/recupero e rifiuto della fault injection passati, con identità dei binari uguali alla campagna prestazionale. Prove native in `var/raw-native-final-pdCgYF/` e `var/large-navigation-isqffzxg/`.
- [x] Preparare `dist/TrueRenderer-performance.app`, registrare tempi, identità, prove e limiti nel rapporto A/B; sincronizzare specifica e architetture, verificare 287 link e diff senza errori. Originale v1.2, LICENSE e NOTICE invariati; fotografie e cataloghi di prova restano privati. Conservata la modifica concorrente dell'utente al pubblico destinatario nel README. Windows non ripetuto in questo incremento.

Estensione emersa nella verifica nativa: la sequenza 1:1→griglia grande a 2 GiB rifiutava sei richieste. Il lato fisico veniva arrotondato alla potenza di due e poi raddoppiato: una cella da 536 px tratteneva il mip D750 da 1504 px invece del sufficiente 752 px (quattro volte i pixel). Introdotti bucket fisici da 128 px anche per il pannello laterale e lookup SSD coerente, con regressioni sui limiti, sulla richiesta effettiva della griglia Retina e sui pixel renderizzati rispetto al grafo completo. Prova negativa conservata in `var/raw-performance-45433wtg/reports/raw-grid-budget-before.json`; ripetizione corretta in `var/raw-native-final-pdCgYF/`: otto stadi/catture, 15,25 s, zero errori, griglia piccola e grande controllate visivamente. Non si riduce il margine di ammissione nativo né la precisione.

Limite memoria non chiuso: il probe headless finale, senza readback grafico, misura fino a 2.964.258.816 byte RSS / 2.599.128.784 byte footprint aggregati con 2 GiB configurati; la navigazione nativa con readback arriva a 3.017.752.576 / 3.453.620.640 byte. Originali invariati, zero campioni incompleti. La somma dei processi può contare pagine condivise più volte e non cattura ogni picco. Lo sviluppo RAW resta nativo completo: si riducono trasferimento e lavoro ripetuto, non si sostituisce la ricetta con JPEG incorporati o demosaic ridotto. Nessun gate R0–R4, memoria fisica, colore o rilascio chiuso.

### Caricamento cartella e percentuale — 20 settembre

Implementati barra percentuale cliccabile nella riga superiore e popup con conteggi, errori/esclusioni, pausa/ripresa, annullamento e passaggio al background. Preferenza persistente in Anteprime e RAW: background predefinito oppure preparazione in primo piano con popup modale fino al termine. Scansione indeterminata prima del totale; contatore della cartella, non delle sole foto filtrate; annullamento non finge il 100%. Coda limitata a una richiesta aggiuntiva, riuso cache, attesa memoria esplicita, nessun riavvio su refresh identico. Restano invariati originali e vincoli decoder. Bundle `dist/TrueRenderer-loading.app`: cinque nuove regressioni, 20 test UI, suite completa 136 ordinari + 11 integrazioni, corpus nativo 12/12 e 30 copie D750 Piena/Apple in entrambe le modalità senza errori; Standard verificato sulla revisione precedente. Dodici catture impostazioni IT/EN/minima/200%, screenshot popup e due XPC passati. [Rapporto e limiti](reports/folder-loading-macos.json). Il 100% non certifica dettaglio nativo residente o persistenza SSD; nessun nuovo gate memoria/colore/rilascio, né ripetizione nativa Windows/altri motori in questo incremento.

- [x] Implementare barra, popup, preferenza e contatori verificabili.
- [x] Verificare migrazione impostazioni, clic, pausa, errori, annullamento e riletture.
- [x] Completare bundle finale, screenshot e verifica XPC; registrare evidenze.

### Errori anteprime D750 — verifica richiesta il 20 settembre

Prosecuzione dei requisiti RAW, memoria e richieste concorrenti del progetto anteprime (§12) e del progetto motori RAW. I 30 originali D750 passano 120 sviluppi singoli sui quattro motori e conservano gli SHA-256 iniziali. Il messaggio segnalato «Read error» è l'etichetta generica degli errori delle miniature: riprodotto un rifiuto di memoria a 2 GiB in sei dei 24 gruppi della baseline, con Apple, LibRaw bilineare e AHD, non una corruzione dei RAW. Il limite riguardava soltanto la coda pronta e lasciava trattenere fino a cinque snapshot tra i thread; ora due lease complessivi coprono anche lettura e decoder attivi/in attesa, mantenendo cancellazione e coalescenza. Riprodotto e corretto anche il caso 1:1 + filmstrip: il viewer completa prima i derivati visibili e riprende poi il dettaglio nativo, rilasciando gli antenati troppo grandi. Tooltip sulle miniature con il vero errore.

Bundle consegnabile: `dist/TrueRenderer-d750.app`. Passati 24 gruppi (30 foto × quattro motori × Standard/Piena) sia a cache fredda sulla revisione con lo stesso pool finale, sia riaprendo i derivati nel pacchetto definitivo; quest'ultimo passa anche cinque stadi nativi 1:1/filmstrip, cinque confronti pixel senza differenze, 40 transizioni foto/zoom/pan/qualità sui quattro motori e due XPC con recupero. La prova estesa aveva rilevato un ulteriore rifiuto nel cambio Piena→Standard dopo lo zoom: ora si rilasciano anche gli antenati obsoleti, proteggendo le identità condivise ancora richieste e i fotogrammi completati. Superati `scripts/verify.sh --gui` (131 test ordinari + 11 integrazioni, due test privati sostituiti dalla campagna isolata), due nuove regressioni, controlli protocollo/ricampionamento e 291 link documentali. [Rapporto, identità dei binari e limiti](reports/d750-preview-memory-macos.json).

**Limite aperto distinto dagli errori corretti:** nella navigazione nativa D750 il picco aggregato è 3.332.456.448 byte RSS / 3.646.361.728 byte footprint con 2 GiB configurati; tutti i quattro casi superano il budget fisico, pur restando entro i crediti di ammissione. Misura comprende i readback della prova; isolare il costo del test da quello produttivo e correggere gli extra rimane da fare. Copertura durante transizioni non nulla ma talvolta parziale, non continuità universale. Nessuna preferenza, fotografia originale o database dell'utente modificato; gli SHA-256 originali sono ricontrollati, fotografie e report dettagliati restano in `var/`. Nessun gate di memoria fisica, colore o rilascio chiuso.

- [x] Identificare i quattro motori Mac disponibili e verificare tutti i 30 originali senza modificarli.
- [x] Riprodurre l'errore delle anteprime, correggere la causa e aggiungere regressioni.
- [x] Verificare griglia/viewer su tutti i motori, bundle finale e XPC; registrare perimetro e limiti.

### Bundle e priorità generali

Il bundle Mac dell'incremento memoria è `dist/TrueRenderer-memory-final.app`, qualificato nel [perimetro sopra](#incremento-corrente--memoria-delle-immagini-grandi). Il bundle di questa nuova verticale è indicato nel punto di ripresa. I percorsi seguenti identificano le campagne storiche dei bundle ora rimossi: `dist/TrueRenderer-export-final.app` per export/FITS e precisione SDR; `dist/TrueRenderer-performance.app` conserva le prove RAW/griglia/filmstrip indicate sopra; `dist/TrueRenderer-loading.app` e `dist/TrueRenderer-d750.app` conservano le precedenti campagne del caricamento e dei quattro motori. Il precedente `dist/TrueRenderer-navigator-compact.app` conserva la campagna del 19 settembre: barra percorso compatta, schede Esplora/Libreria stabili, 14 regressioni UI, sei layout nativi, dodici catture preferenze e due servizi XPC. [Rapporto e limiti](reports/navigator-layout-macos.json). La campagna precedente del navigatore appartiene a `dist/TrueRenderer-navigator.app`, documentata separatamente: [rapporto](reports/filesystem-browser-macos.json). La campagna storica D750 e PNG 12/24/45 MP resta distinta dalle verifiche odierne: [rapporto fotografico](reports/large-pressure-raw-macos.json). Windows: ultima suite registrata di 99 test ordinari + 8 integrazioni, pubblicata in `67146e3`; GUI e Nikon non ripetuti per gli ultimi fix. [Rapporto](reports/gamma-orientation-fixes-windows.json).

**Priorità:** ampliare la qualifica memoria a cartelle estese, altre fotocamere e pressione fisica OS dopo la correzione misurata sopra. Decode RAW ridotto/regionale e qualifica fotografica estesa restano aperti. Nessun gate R0–R4 è chiuso; Standard/Piena sono qualità di anteprima, non assurance Standard/Riferimento.

Il prossimo incremento deve ampliare pressione OS, target colore e fotocamere. Fit, livelli residenti ridotti e transizioni sono verificati nei perimetri PNG 12/24/45 MP e D750 descritti sopra; decode RAW ridotto/regionale, presentazione effettiva e campagna statistica restano aperti. I gate che richiedono altri RAW, hardware o persone rimangono espliciti. Il seguente ordine riguarda invece la roadmap complessiva R0–R4.

1. L’incremento di ricampionamento 0.1.2 è chiuso per il sottoinsieme R0 provato; estendere in R1 orientamenti, isotropia, alias/overshoot, LOD e verifica fisica su più scale/display. ADR 0003 conserva le scelte e i criteri di revoca.
2. Completare i confini R0 sul Mac: bootstrap/handle/output avversari, memoria end-to-end, sorgenti sotto writer concorrente e recupero XPC. Decidere API/minimi OS e firma reciproca con evidenze.
3. Completare la qualifica della verticale Windows e i gate toolkit/display/accessibilità su hardware reale; affiancare interviste e corpus autorizzato. Estendere il corpus oltre D750 e D40 già autorizzati. Hardware, persone reali e credenziali di distribuzione non si possono sostituire con test simulati.
4. Superati i gate pertinenti, R1: codec bitmap qualificati, EXIF/ICC/monitor, renderer/tile di riferimento e viewer completo. Qualificare il percorso esterno anticipato nella 0.1.3; mantenere distinti Anteprima e modi di riferimento.
5. R2: catalogo completo, XMP sicuro, durabilità/restore e pilot. L’esito del pilot decide l’impegno successivo in R3 matrice RAW/LibRaw e gigapixel.
6. R4: hardening, misure p95, accessibilità/piattaforme, licenze, installer e rilascio. Nessuna milestone è chiusa solo perché l’app si avvia.

### Attività di ripresa per area

- Dopo la correzione Fit, estendere a RAW ridotti/pressione e individuare la presentazione effettiva prima della campagna p95/p99. Il readback attuale include il costo della cattura.
- RAW Mac: i 30 D750 sui quattro motori e nove ritagli centrali allineati sono verificati; servono altre fotocamere, target colore misurato, rumore/moire e ICC. Usare originali autorizzati in sola lettura e cataloghi separati.
- Windows: installazione pulita, contesa writer/cache e diagnostica PNG malformata o conflittuale.
- Sviluppo fotografico futuro: partire da SF0 del [piano dedicato](#piano-sviluppo-fotografico), poi regolazioni globali e WB; ottica, filtri locali, maschere e automazioni seguono le dipendenze dichiarate. La progettazione non sostituisce le qualifiche del viewer già aperte.
- Qualifica: colore/display, confronto fra motori con ritagli allineati, corpus autorizzato esteso, pressione memoria e latenze evento→frame. Nessun gate R0–R4 chiuso; Standard/Piena restano qualità di anteprima, non assurance Standard/Riferimento.


## Riordino documentale — 19 settembre 2026

Ulteriore accorpamento richiesto: specifiche e decisioni riunite direttamente in `docs/`, senza `adr/`. Sette ADR incorporati per argomento: 0003/0005/0006 nel progetto anteprime, 0008 nel progetto RAW, 0002/0004/0009 in isolamento decoder e formati. Sei file in meno rispetto alla precedente struttura; testi e vincoli conservati con ancore numerate. Aggiornati rimandi, istruzioni e sincronizzatore; stato resta qui, report e notice nelle rispettive sedi tecniche. Nessuna modifica applicativa o nuova qualifica.

Verificato questo accorpamento: contenuto integrale dei sette ADR conservato salvo livelli dei titoli e percorsi dei link; 290 collegamenti locali validi e seconda sincronizzazione senza modifiche. Originale v1.2 e LICENSE invariati; negli avvisi di licenza aggiornato solo il rimando. Nelle architetture aggiornate le appendici generate e il solo link LibRaw esterno al blocco. Le vecchie sedi restano recuperabili da Git.

Su richiesta del titolare, aggiornamenti e lavori conclusi restano soltanto qui. Eliminati sei Markdown: ADR 0001 e 0007, progetto restyling, revisioni Windows, campagne RAW e registro separato delle verifiche. Sintesi sotto; testi completi recuperabili da Git, rapporti JSON e prove negative conservati. Gli ADR restanti descrivono contratti ancora attivi, non liste di attività concluse. Nessuna nuova prova applicativa in questo riordino; gate e limiti aperti invariati.

Verifica documentale: 287 collegamenti locali validi, sincronizzazione delle appendici idempotente, testo delle architetture fuori dai blocchi gestiti invariato; originale v1.2, LICENSE e NOTICE invariati. Nessun rapporto JSON storico, fotografia, catalogo o backup eliminato.

<a id="prototipo-e-port"></a>

### Prototipo e port conclusi

Realizzati il prototipo Rust/egui/wgpu con corpus controllato, catalogo SQLite e annotazioni durevoli, quindi la porta `Decoder` comune e il primo port Windows. Il worker singolo iniziale è superato dai due XPC Mac e dal confinamento LPAC Windows; backend e licenze restano negli ADR 0002/0004/0008/0009. Il contratto decoder ancora valido è conservato in ADR 0009; toolkit, ICC, porte WorkingSpace/TileProvider e qualifica multipiattaforma restano aperti.

<a id="restyling-concluso"></a>

### Restyling concluso

Applicati stile neutro, strumenti in alto, cartella/conteggi in basso, griglia fotografica, ispettore richiudibile e quattro schede preferenze con bozze separate dall'applicazione. Il viewer distingue qualità globale e override di sessione per foto; il cambio globale li azzera. Pixel fotografici non alterati dalla selezione, nomi lunghi con tooltip, identificatori stabili fra lingue e badge Anteprima conservati. Verificati layout IT/EN, finestra minima/200%, bozze e rendering nel [rapporto toolbar](reports/toolbar-macos.json); navigatore e barra compatta nel [rapporto aggiornato](reports/navigator-layout-macos.json). Riproduzione: test `ui::`, `--restyle-smoke` con corpus/catalogo separati e suite XPC; accessibilità e display non qualificati.

<a id="revisioni-windows-concluse"></a>

### Revisioni Windows concluse

Corretti isolamento LPAC/quote, LibRaw 0.22.2, parsing delle preview, classificazione TIFF/RAW, provenienza, hard link e invalidazione sorgenti; poi selezione/zoom nelle preferenze, PNG cICP/gamma, alpha/colorimetria TIFF, fallback RAW, timestamp cache e orientamento per IFD. Ultima campagna: 99 test ordinari + 8 integrazioni; non attribuita automaticamente ai binari successivi. Evidenze audit→correzione: [iniziale](reports/pre-main-review-windows.json) → [fix](reports/pre-main-fixes-windows.json), [serale](reports/review-continuation-windows.json) → [fix](reports/review-followup-fixes-windows.json), [generale](reports/recheck-windows-2026-09-14.json) → [fix](reports/general-review-fixes-windows.json), [gamma/IFD](reports/additional-review-windows.json) → [fix](reports/gamma-orientation-fixes-windows.json). Restano installazione pulita, persistenza sotto contesa (errore 33), diagnostica PNG malformata/conflittuale, conversioni TIFF/ICC e qualifica estesa.

<a id="campagne-raw-concluse"></a>

### Campagne RAW concluse

Esteso il motore proprio al modello esatto D40 e corretto il cambio motore durante scansione: 66/66 sviluppi su 22 NEF a ISO 200 e regressione D750, senza ampliare implicitamente la matrice camere ([rapporto Windows](reports/raw-engines-d40-windows.json)). Su Mac: 120 sviluppi D750 sui quattro motori dopo il fix heap LibRaw/XPC, nove confronti centrali, 240 azioni PNG 12/24/45 MP, 20 sotto pressione renderer e 40 RAW ([rapporto](reports/large-pressure-raw-macos.json)). Ritagli allineati solo per traslazione intera ±16 pixel: Apple/AHD non sono riferimenti della scena. Rimane il superamento memoria 45 MP Full indicato in apertura; mancano pressione OS, decode RAW ridotto, colore misurato e p95/p99. Riproduzione: `scripts/test-large-navigation.py`, `scripts/compare-raw-patches.py` e `--verify-raw-engines`, esclusivamente con corpus autorizzato e dati separati.

## Piano operativo

<a id="piano-sviluppo-fotografico"></a>

## Progetto di incremento — sviluppo fotografico, 23 settembre 2026

Richiesta: progettare tutte le principali regolazioni fotografiche, confrontare gli altri programmi e conservare qui il lavoro futuro. **Progettato:** insieme dei controlli, valori neutri e scale iniziali, interazione, grafo di calcolo, capacità dei motori RAW, correzioni ottiche, dati durevoli, prestazioni, esportazione e criteri di accettazione. Contratto completo e fonti primarie nella [specifica sviluppo fotografico / ADR 0011](docs/progetto-sviluppo-fotografico.md). Le scale proposte sono TrueRenderer, non una promessa di equivalenza con i numeri di Lightroom.

**Baseline della sessione di progettazione iniziale:** lettura dei contratti dell'architettura, del codice di colore/decoder/anteprime/export e della persistenza; consultazione della documentazione ufficiale dei prodotti; 327 collegamenti locali validi e diff senza errori di whitespace. Allora nessun controllo fotografico era stato implementato e nessuna campagna dell'editing era stata eseguita. Lo stato successivo è nel punto di ripresa sopra; le caselle SF0–SF10 indicano ancora i gate non conclusi.

### Controlli da consegnare

| Area | Contenuto del progetto |
|---|---|
| Luce | Esposizione EV, luminosità dei mezzitoni, contrasto/pivot, alte luci, ombre, bianchi, neri, recupero RAW distinto dalla compressione tonale, Auto reversibile. |
| WB e profili | Come scattato, temperatura/tinta, contagocce, preset e Auto; WB nativo quando qualificato, correzione relativa RGB distinta; profilo tecnico e look creativo separati. |
| Curve e colore | Curve a punti/parametriche, livelli, saturazione, vividezza, mixer HSL, colore selettivo, grading di ombre/mezzitoni/luci, bianco e nero, profili/LUT qualificati. |
| Presenza e dettaglio | Texture, chiarezza, rimozione foschia, nitidezza con raggio/dettaglio/mascheratura, rumore luminanza/cromatico, moiré; nitidezza d'uscita separata. |
| Ottica | Distorsione a barilotto/cuscinetto e a baffo, profili/metadati/manuale, CA laterale, defringe viola/verde, vignettatura ottica; stato delle correzioni già applicate e nessuna duplicazione automatica. |
| Geometria | Crop e rapporti, rotazione/specchio/raddrizzamento, prospettiva manuale/guidata/Auto, scala/offset e ritaglio all'area valida. |
| Maschere e ritocco | Pennello/gomma, gradienti, intervalli colore/luminanza, combinazioni, regolazioni locali, polvere, clone/correttivo e occhi rossi. Selezioni automatiche e modelli in estensione successiva. |
| Effetti e flusso | Vignetta creativa, grana, Prima/Dopo, cronologia/undo/redo, snapshot, versioni virtuali, preset e copia/batch selettivi. |
| Salvataggio ed export | Ricetta per foto nella libreria durevole, stesso grafo in vista/export, dipendenze versionate, backup/restore; DNG RAW e DNG lineare mantengono contratti distinti dalle immagini con editing applicato. |

### Ordine di realizzazione e gate

Una verticale per volta, con copia applicativa separata e catalogo di prova. L'ordine indica dipendenze di questo ampliamento; non chiude R0–R4 né attribuisce una data di rilascio. Prima consegna utilizzabile: SF0–SF2 con regolazioni globali reversibili e export coerente. Ottica e filtri successivi entrano solo con i rispettivi confronti numerici/fotografici.

- [ ] **SF0 — fondamenta:** fissare schema/processo, ricetta identità, capacità/provenienza per motore e grafo CPU comune a vista/export; introdurre revisioni, asset durevoli, migrazione/backup e undo minimo. Gate: identità rispetto alla baseline, round-trip della ricetta, crash/restore e nessuna perdita dopo pulizia cache; nessun risultato tardivo applicato a foto/revisione errata.
- [ ] **SF1 — luce e curve di base:** pannello Sviluppo IT/EN, esposizione/luminosità/contrasto, luci/ombre/bianchi/neri, curve e clipping; bozza durante il gesto, salvataggio alla conclusione, Prima/Dopo e export della revisione congelata. Gate: rampe/campioni/alpha, CPU a piena risoluzione contro export lossless e zero variazioni degli originali.
- [ ] **SF2 — WB e colore iniziale:** selezione come scattato/personalizzata/contagocce, adapter per WB nativo qualificato, correzione RGB relativa, saturazione/vividezza e Auto con parametri congelati. Gate: neutri e calibrazioni note, niente Kelvin inventati o clipping celato; capacità provate separatamente per Apple, bilineare, AHD e TrueRenderer.
- [ ] **SF3 — geometria e ottica manuale:** crop/rotazioni/prospettiva guidata, distorsione e vignettatura manuali, CA laterale nello stadio corretto e defringe distinto; metadati su correzioni già eseguite. Gate: griglie/flat-field/canali disallineati, area valida, coordinate delle selezioni e assenza di doppia correzione.
- [ ] **SF4 — profili ottici e automatismi:** decidere integrazione Lensfun/dati incorporati, versione/licenze/database, matching e profili manuali; quantità separate per distorsione/CA/vignettatura, aggiornamento volontario delle vecchie ricette. Gate: matrice corpo/ottica/focale/apertura, identificazioni ambigue e dati mancanti, riproduzione con vecchio profilo, licenze e parser confinato.
- [ ] **SF5 — presenza e dettaglio:** scegliere/versionare filtri Texture, Chiarezza, Foschia, denoise e nitidezza, con dimensioni native/halo e previsione finale. Gate: aloni/rumore/alias, tile contro frame completo, confronto 1:1 e fit contro export; misure memoria su D750 e 12/24/45 MP.
- [ ] **SF6 — colore avanzato ed effetti:** mixer HSL, campioni selettivi, grading, monocromia, curve RGB, profili creativi, grana/vignetta; DCP/ICC/LUT solo con il gate del rispettivo formato. Gate: dominio esteso, neutri/incarnati/saturi, nessun clamp implicito, seed stabile e dipendenze riproducibili.
- [ ] **SF7 — maschere e ritocco:** pennelli, gradienti, intervalli, combinazioni e regolazioni locali; clone/correttivo/polvere/occhi rossi. Gate: coordinate dopo trasformazioni, ordine e sovrapposizioni, nessuna cucitura/ciclo, durabilità delle pennellate e limiti di risorse.
- [ ] **SF8 — organizzazione e batch:** UI di snapshot/versioni virtuali, preset, copia selettiva, applicazione a più foto e default facoltativi per fotocamera; import/export ricetta. Gate: batch misto/cancellato, undo raggruppato, nessun profilo ottico copiato implicitamente fra obiettivi. XMP solo dopo round-trip, merge e scrittura sicura già previsti dall'architettura.
- [ ] **SF9 — accelerazione e destinazioni colore:** qualificare ogni nodo GPU contro CPU; pianificazione/riuso/invalidazione e misure p95; nitidezza d'uscita, ICC aggiuntivi/soft proof quando qualificati. Gate: tolleranze per nodo e output, driver/device-loss/fallback, budget fisici e distinzione output/display. Nessuna conversione del progetto in HDR/EDR implicita.
- [ ] **SF10 — qualifica della verticale:** suite completa, GUI Mac/Windows, XPC sul bundle, backup/restore, corpus ostile e fotografico autorizzato, ricette storiche, display/accessibilità ed export su tutti i motori pertinenti. Pubblicare una matrice precisa delle capacità e dei limiti; nessuna funzione marcata disponibile sulla base del solo pannello UI.

Modelli locali per selezione soggetto/cielo/persone, denoise avanzato e profondità sono una successiva estensione con asset/versione/licenza e fallback manuale. HDR merge, panorami, focus stacking, rimozione generativa, tethering e stampa completa richiedono progetti propri e non bloccano le regolazioni richieste. I dettagli tecnici e le soglie iniziali sono nella [matrice di accettazione](docs/progetto-sviluppo-fotografico.md#matrice-di-accettazione); gli esiti futuri si registrano soltanto qui.

<a id="piano-alta-precisione-fits"></a>

## Proposta originaria — uscita ad alta precisione e dati FITS, 21 settembre 2026

Il testo seguente conserva il contesto dell'analisi iniziale; autorizzazioni e disponibilità correnti sono nel punto di ripresa sopra. Le scelte di questo incremento, incluso il parser FITS ristretto al posto del candidato CFITSIO e l'export approvato, sono definite in ADR 0010. La proposta estesa resta distinta dal sottoinsieme consegnato.

Richiesta: valutare se serva superare l'uscita a 8 bit, studiare anche FITS e pianificare prima di implementare. Baseline applicativa: `c15a8b3`, già pubblicata. Letti architettura §§2.2/2.4, 6.3/6.4, 8.7/8.10 e 9, contratti anteprime e ADR formati; esaminati codice applicativo e dipendenze bloccate e consultate fonti primarie Apple, Microsoft e FITS/CFITSIO. Nessuna nuova prova del display fisico, build o qualifica di formato in questa analisi.

### Conclusione e confini della proposta

Non serve portare il **working** da 8 a 16 bit: è già Rec.2020 lineare esteso **fp32**. Serve valutare la rimozione della quantizzazione anticipata a 8 bit nel **presenter**. Il beneficio atteso riguarda sfumature e banding, non dettaglio, demosaic o velocità di comparsa dei RAW. Profondità, gamut e HDR sono tre proprietà diverse: aumentare i bit non amplia automaticamente gamut/dinamica e non recupera clipping già avvenuto nel decoder.

| Ambito | Situazione verificata nel codice | Proposta |
|---|---|---|
| Sviluppo RAW, working, mip e cache | RGBA fp32; negativi e RGB oltre 1 ammessi. LibRaw consegna già un raster lineare a 16 bit interi, con limiti della propria ricetta | Conservare precisione, ricette e cache; nessuna conversione generale a fp16 o uint16 |
| Presentazione CPU | `display_pixel()` → `Vec<u8>` → `egui::ColorImage` | Eliminare questo collo di bottiglia nel nuovo percorso ad alta precisione |
| Presentazione GPU | `display_pass` converte/clampa in una texture `Rgba8Unorm` | Mantenere float fino all'ultimo passaggio verso la superficie scelta |
| Finestra | Report nativo: `Bgra8Unorm`; egui sceglie preferenzialmente RGBA/BGRA8 | Negoziare coppia formato/spazio colore; SDR10 come candidato leggero, RGBA16F dove utile e verificato |
| FITS e dati scientifici | Nessun decoder FITS; raster generico senza unità/maschera e con rifiuto NaN/Inf; istogramma della vista sRGB8 | Percorso scientifico distinto, non una nuova estensione associata al decoder fotografico |
| Esportazione | Esiste export delle annotazioni, non un export fotografico dei pixel | TIFF/PNG16 solo con richiesta e progetto separati; non è necessario per migliorare il viewer |

Riferimenti applicativi: [colore e raster](crates/tr-core/src/color.rs), [presenter CPU](crates/tr-render/src/presenter.rs), [compute GPU](crates/tr-render/src/preview_compute.wgsl), [risorse GPU](crates/tr-render/src/resident_compute.rs), [finestra](apps/desktop/src/graphics.rs), [ricette LibRaw](native/libraw/tr_libraw.cpp), [ultimo smoke](reports/smoke-macos.json).

**16 bit interi non significa 16 bit float.** I primi offrono 65.536 codici uniformi nel dominio scelto; fp16 ha passo variabile ed è un formato adatto allo scambio grafico, non un sostituto senza perdita dei dati scientifici fp32/fp64. Esempio numerico riproducibile, senza dither: 4.096 grigi `sRGB = 0,45 + 0,10*i/4095`, arrotondati al quantizzatore, diventano 26 valori distinti a 8 bit, 104 a 10 bit, 705 convertendo prima in lineare e arrotondando a IEEE binary16, 4.096 in uint16. Calcolato con Python standard (`struct` formato `e` per binary16). Dimostra solo la quantizzazione su quella rampa, non quanti livelli vedrà il pannello né un miglioramento già ottenuto nell'app.

Il solo cambio della superficie non basta: se riceve pixel già ridotti a 8 bit, le sfumature sono già perse. Viceversa, FITS/float possono essere visualizzati correttamente anche su un display SDR8 attraverso una trasformata di vista esplicita; non dipendono da un monitor HDR. Un TIFF float eventualmente aperto da ImageIO non costituisce oggi supporto scientifico qualificato e il solo tipo float non prova che i valori rappresentino luce lineare.

### Vincoli reali dello stack

Versioni ispezionate: `eframe`/`egui-wgpu` 0.36.1 e `wgpu`/`wgpu-hal` 30.0.1, già in `Cargo.lock`.

- wgpu espone `SurfaceCapabilities::format_capabilities`, `SurfaceConfiguration::color_space` e `Surface::display_hdr_info()`. Il backend Metal configura `CAMetalLayer.colorspace`/EDR; DX12 configura `SetColorSpace1`. Non occorre presumere un aggiornamento di wgpu o scrivere subito un'integrazione Metal/DXGI parallela.
- egui-wgpu sceglie il target in `preferred_framebuffer_format`; `SurfaceConfig` espone soltanto present mode/latenza. Occorre una piccola integrazione versionata per selezione formato/spazio e accesso diagnostico alla superficie, da stimare nel primo esperimento. Non modificare `.tools/cargo/registry`; una eventuale patch vendorizzata conserva versione, diff e licenze. Sostituire tutto il toolkit non è la prima scelta.
- `Rgba16Float` con spazio `Auto` può risolversi in **scRGB lineare**: scriverci gli attuali valori gamma-sRGB cambierebbe la luminosità. La scelta deve essere esplicita. Metal pubblicizza anche fp16+sRGB; DX12 fp16 richiede il contratto extended-linear. Non estendere automaticamente una combinazione verificata su un OS all'altro.
- Lo shader egui corrente assume texture/colore UI codificati sRGB; la scelta del fragment dipende da `is_srgb()` del formato, non dal nuovo contratto colore. Foto e chrome devono arrivare alla superficie con la codifica corretta, incluso blending/alpha e bianco UI. Non basta selezionare il fragment per una superficie sRGB per qualificare tutta la composizione lineare.
- La cattura egui attuale accetta soltanto `Rgba8Unorm`/`Bgra8Unorm` e restituisce `ColorImage`. Per dimostrare più di 8 bit serve readback numerico nel formato nativo; un PNG8 non può essere la prova.
- Capability della superficie, headroom corrente e profondità fisica sono dati distinti. Su Metal i bit del collegamento/pannello non sono riportati da questa API; lasciare «non osservati». La ricognizione `system_profiler` di questa sessione identifica M4 ma non fornisce una qualifica del display.

Queste scelte rispettano il [contratto di presentazione esistente](docs/TrueRenderer-Architettura.md#870-contratto-di-presentazione) e la distinzione già prevista fra [uscita oltre 8 bit](docs/TrueRenderer-Architettura.md#8105-uscita-a-più-di-8-bit) e HDR post-v1. Riferimenti esterni: [Apple, spazi colore e formato Metal](https://developer.apple.com/documentation/metal/using-color-spaces-to-display-hdr-content), [Apple, capacità EDR corrente e potenziale](https://developer.apple.com/documentation/metal/determining-support-for-edr-values), [Microsoft, Advanced Color e scRGB](https://learn.microsoft.com/en-us/windows/win32/direct3darticles/high-dynamic-range). Le capacità dichiarate dalle API non provano la resa fisica.

### Percorso P — presentazione ad alta precisione

**P0 — esperimento controllato e decisione tecnica.**

1. Generare rampe, gradienti scuri, colori saturi e bordi con alpha in fp32; baseline SDR8 attuale, SDR8 con dither finale, SDR10 e fp16. Non cambiare contemporaneamente demosaic, curva di resa e profondità.
2. Aggiungere diagnostica di formato/spazio selezionati, coppie supportate, headroom e informazioni non disponibili. Provare configurazione, resize e ricreazione su Mac; Windows ha un gate nativo distinto. Nessuna attivazione automatica basata sul solo nome della GPU.
3. Prototipare l'adattamento minimo egui e il readback 10-bit/fp16. Provare prima `Rgb10a2Unorm` + sRGB dove supportato: mantiene 4 byte/pixel. Confrontarlo con `Rgba16Float` e relativo contratto. L'esperimento sceglie il percorso, non una preferenza cosmetica «16 bit».
4. Registrare errore numerico, livelli conservati, aspetto della UI, tempi e memoria. Se il percorso esteso non supera le verifiche, resta disattivato e si conserva il fallback dichiarato.

**P1 — separare campioni e uscita, su CPU e GPU.**

1. In `tr-core` introdurre un contratto di presentazione versionato: formato, primarie, transfer, dominio SDR/esteso, riferimento del bianco, policy di clipping e dither. Separare trasformata float e quantizzazione oggi unite in `display_pixel()`; conservare la funzione legacy per confronti e fallback.
2. In `tr-render` mantenere il risultato del ricampionamento fp32 fino al passaggio di uscita. CPU: non transitare in `ColorImage` per le foto ad alta precisione. GPU: usare il risultato float già disponibile, evitando il passaggio obbligatorio in `rgba8unorm`. Preferire un pass di presentazione dedicato/callback che scriva direttamente la superficie; un intermedio fp16 è una scelta da misurare, con errore dichiarato, non la nuova precisione del working.
3. Stessa via per griglia, filmstrip, ispettore, viewer e confronto; riguarda tutti e quattro i motori RAW, senza duplicare la logica nei decoder. Conservare campionamento fisico 1:1, clip, prioritizzazione e assenza di readback nel percorso interattivo.
4. Aggiungere versione/identità del contratto alla chiave del presenter, alle risposte asincrone e ai frame conservati. Al cambio di superficie/scalatura colore invalidare solo i derivati dipendenti dall'uscita, non sviluppi RAW, mip e cache SSD lineare.

**P2 — superficie, ripieghi, dither e memoria.**

1. Esporre «Automatica», «SDR 8 bit compatibile» e modalità estese solo se qualificate. Mostrare separatamente richiesta ed effettiva, formato, spazio, dither e motivo del fallback. Il calcolo CPU deve poter alimentare l'uscita estesa: CPU/GPU di calcolo e formato di presentazione non sono la stessa preferenza.
2. Percorso iniziale SDR: non cambiare il rendering delle alte luci o attivare tone mapping creativo. Un'uscita scRGB/fp16 richiede gestione coerente anche di UI e bianco SDR. P3/wide gamut ed EDR/HDR richiedono prove aggiuntive di colore/headroom; nessuna implicita promozione dal solo formato fp16.
3. Valutare un solo dither finale, prima della quantizzazione intera a 8/10 bit, con seed e coordinate fisiche definiti. Non riattivare indiscriminatamente il dither egui dopo una texture già quantizzata. Verificare bias, rumore, stabilità e cuciture; i campioni del working restano indipendenti dal dither e il confronto diagnostico può disabilitarlo esplicitamente.
4. Rimuovere le assunzioni `pixel * 4` dal budget del presenter. Contabilizzare texture, buffer float residenti, upload, swapchain, doppio/triplo buffering e risorse ritirate fino al completamento GPU. A 3840×2160 il solo payload RGBA8/RGBA16F/RGBA32F vale circa 31,6/63,3/126,6 MiB per buffer: fp16 raddoppia questo costo rispetto a RGBA8, non necessariamente l'intera RAM dell'app. Tre buffer fp16 aggiungono circa 94,9 MiB rispetto a tre RGBA8, senza contare overhead del driver.
5. Misurare separatamente il superamento memoria RAW già aperto; non compensarlo aumentando la quota dichiarata. Introdurre l'uscita estesa inizialmente opt-in; il default automatico richiede margine verificato e nessuna nuova attesa/errore sistematico delle miniature.

**P3 — accettazione e consegna separata.**

- Riferimento CPU float indipendente, errore pre-quantizzazione e risultato nel dominio della superficie; rampe monotone, livelli oltre 256 verificati nel readback, alpha, negativi/fuori gamut, estremi, colore UI e test contro doppia codifica. Per UNORM confrontare anche i codici interi con tolleranza motivata dal quantizzatore; per fp16 errori assoluti/relativi e ULP nel dominio dichiarato, non una soglia sRGB8 riciclata.
- Rami CPU/GPU, tutti i motori, griglia/filmstrip/viewer/confronto, 1:1/fit/pan/zoom, cambio qualità, resize/Retina, device loss, cambio display e fallback 8 bit. Baseline fotografica autorizzata D750 in copie private; cache e mip fp32 devono restare invariati a parità di ricetta. Non promettere pixel di presentazione identici quando cambia il quantizzatore o viene abilitato il dither.
- Readback numerico distinto dagli screenshot illustrativi e dalla valutazione sul monitor. Confrontare latenza e memoria alle stesse condizioni; ripetere i casi, riportare dispersione e usare almeno 100 prove indipendenti se si dichiara un p95. Nessuna attestazione fisica basata soltanto sul readback.
- Suite `scripts/verify.sh --gui`, bundle separato e controlli XPC del pacchetto. Registrare report, identità e limiti in questo stato; aggiornare specifica/ADR e sincronizzazione solo dopo l'approvazione del contratto. Gate R0–R4 e assurance restano distinti.

### Percorso F — FITS e campioni scientifici, indipendente da P

FITS è utile se il prodotto deve aprire acquisizioni/stack o immagini scientifiche; non serve per migliorare i NEF né per ottenere un'uscita a 16 bit. Il primo incremento proposto è un **viewer FITS in sola lettura**, non stacking, calibrazione, fotometria o esportazione scientifica. Richiede approvazione del nuovo perimetro, oggi assente dalla matrice dei formati.

**F0 — modello dei dati prima del codec.**

1. Separare un piano di campioni scientifici dal raster fotografico Rec.2020: tipo nativo, dimensioni/assi, unità dichiarate o ignote, scaling, maschera di validità, HDU e provenienza. Non assegnare sRGB, alpha o significato fotometrico per il solo fatto che il file contiene numeri. Anche un FITS può contenere dati già elaborati: non dichiararlo scene-linear senza evidenza.
2. Conservare tipo/campione originale e applicare lo scaling una sola volta. I dati nativi per campionatore/statistiche non si ricavano da una miniatura o invertendo lo stretch. I32 oltre 24 bit significativi e F64 non passano silenziosamente in fp32; calcolare normalizzazione/scaling in precisione adeguata e dichiarare le conversioni della sola anteprima.
3. NaN, infinito e valori mancanti richiedono classificazione/maschera e contatori; non allentare `LinearImage::new` accettando NaN in tutta la pipeline fotografica. Statistiche sui validi, stato esplicito per un piano senza campioni validi e gestione definita della copertura nel downsample. Nessuno zero sostitutivo viene presentato come dato originale.
4. Istogramma scientifico separato da quello attuale a 256 bin della vista; min/max/percentili con unità e dicitura completa/campionata. Campionatore: valore memorizzato, valore scalato, validità e valore di vista. Coordinate/ordine degli assi e convenzione di visualizzazione documentati; nessuna soluzione WCS implicita.

**F1 — decoder isolato e sottoinsieme esplicito.**

1. Candidato: CFITSIO con versione/hash bloccati e shim ristretto, costruito per entrambi gli OS. Prima di adottarlo verificare licenze/notices del pacchetto effettivo, dipendenze e opzioni di build. Non promettere supporto completo perché la libreria legge più varianti della UI.
2. Usare i byte della copia privata già concessa al worker; candidato `fits_open_memfile(..., READONLY, ..., mem_realloc = NULL)`. Niente URL, percorsi arbitrari, sintassi di filtri/espressioni, scritture o rete. Questa API evita la riallocazione del file in memoria, ma non limita da sola tutte le allocazioni interne: restano quote, controlli del broker e isolamento XPC/LPAC. Nessun fallback FITS nel processo UI o apertura esterna su pipe.
3. MVP: `.fits`, `.fit`, `.fts`, firma verificata, immagini 2D non compresse nel primario o nelle estensioni IMAGE; elenco HDU limitato e HDU visualizzato esplicito. Tipi iniziali proposti `BITPIX` 8/16/32/-32, inclusa convenzione unsigned16; scaling `BSCALE`/`BZERO`, `BUNIT` e `BLANK` validati. Primario vuoto: individuare e dichiarare la prima immagine 2D idonea, senza spacciare un'altra estensione per il primario. 64/-64 rifiutati con motivo finché il percorso nativo/fp64 non è qualificato.
4. Cubi, tabelle, compressione tiled/esterna, selezione di piani e composizione RGB sono incrementi successivi; un asse di lunghezza 3 non prova RGB. Niente debayer automatico né instradamento ai quattro motori fotografici. Mantengono invece valore i due motori di calcolo CPU/GPU della visualizzazione.
5. Limiti proposti: mantenere sorgente ≤256 MiB e piano ≤67.108.864 pixel, ulteriormente ridotti dall'ammissione memoria; massimo 256 HDU e 1 MiB di header per HDU, con tetto totale header esplicito da fissare sul corpus. La quota sorgente precede l'apertura; una ricognizione strutturale limitata nel worker deve validare header e dimensioni prima della decodifica pesante. Controllare prodotti, offset, allineamenti e lunghezze con aritmetica checked; leggere a blocchi senza espandere il file in quattro canali float solo per il probe. Richieste IPC tipizzate e risposte/versioni rivalidate; firma falsa, input troncato, timeout e recupero sono prove obbligatorie.

Lo standard definisce scaling, unità e campioni mancanti: [FITS 4.0, §§4.4/5/7](https://fits.gsfc.nasa.gov/standard40/fits_standard40aa-le.pdf). Il backend deve essere configurato e provato per non normalizzare implicitamente valori IEEE: [CFITSIO, valori speciali](https://heasarc.gsfc.nasa.gov/docs/software/fitsio/c/c_user/node27.html). Riferimenti per la valutazione dello shim: [API memory-file](https://heasarc.gsfc.nasa.gov/docs/software/fitsio/c/c_user/node65.html) e [condizioni della libreria](https://heasarc.gsfc.nasa.gov/docs/software/fitsio/c/c_user/node6.html).

**F2 — trasformata di vista e anteprime.**

1. Vista iniziale monocromatica; controlli nero/bianco e stretch lineare/asinh, reset e auto-stretch dichiarato, deterministico e stabile per HDU. Un FITS quasi nero nella vista lineare non è necessariamente decodificato male. Non riscrivere i campioni quando si muove un cursore.
2. Definire prima l'ordine fra riduzione dei campioni e stretch: non commutano. Candidato per il viewer scientifico: mip scalari sui validi, copertura separata, poi stretch di visualizzazione; confrontare con riferimento e dichiarare che non è una misura fotometrica. Non riusare alla cieca un filtro fotografico a pesi negativi sulle maschere. Min/max e misure restano riferiti al dato nativo.
3. Separare cache dei campioni/mip da cache della vista. Chiavi comprendono digest, HDU, piano, tipo/scaling, versione del filtro/maschera, stretch e contratto di presentazione solo negli artefatti che ne dipendono. Cambio stretch senza rileggere/decodificare tutto il FITS; griglia e viewer devono dichiarare la medesima interpretazione.
4. Estendere corpus con fixture sintetiche indipendenti: unsigned16 con offset, negativi, F32 piccoli/grandi, BLANK, NaN/Inf, piani costanti/tutti invalidi, HDU multipli, endian, dimensioni ostili e file troncati. Confrontare i campioni con un riferimento indipendente dal wrapper, oltre alla GUI e alle prove di isolamento sul bundle finale. File astronomici reali solo se forniti/autorizzati; mai pubblicati automaticamente.

### Esportazione e ordine raccomandato

Un eventuale export TIFF/PNG16 parte dal working o dalla trasformata di vista scelta, mai da screenshot/`to_display()`. Deve dichiarare profilo, curva, alpha, scala, clipping e dither; un uint16 non conserva da solo negativi, valori estesi o campioni scientifici. Per preservare questi ultimi servirebbe un formato float e un contratto ulteriore, non «salva a 16 bit». Nuovi file, nessuna sovrascrittura degli originali. Questa funzione resta fuori dal primo intervento e post-v1 finché non viene approvata.

Ordine proposto: **P0 → decisione su percorso e costi → P1/P2 → P3**. FITS può seguire come **F0 → F1 → F2**, senza attendere HDR o un monitor a più di 8 bit. Non accorpare entrambe le verticali in un unico cambiamento: hanno criteri di correttezza, licenze e rischi differenti. Per l'uso attuale D750, priorità al presenter e al contenimento memoria; per un uso astronomico effettivo, il percorso dati/stretch FITS può precedere l'attivazione della superficie estesa.

- [x] Committare e pubblicare prima dell'analisi le ottimizzazioni RAW completate.
- [x] Identificare i colli di precisione, verificare lo stack bloccato e preparare questa proposta. Verifica documentale: 297 collegamenti locali, zero errori; `git diff --check` passato. Solo `STATO.md` modificato dopo il commit; nessuna sincronizzazione delle specifiche necessaria.
- [x] Approvare il primo incremento P0 e il suo criterio di scelta; autorizzato con la richiesta di implementazione/export.
- [x] Eseguire il verticale P0 SDR senza dither e scegliere 10-bit/fp16 opt-in con fallback sulla base dei risultati; hardware fisico e prove ulteriori restano aperti.
- [ ] Implementare e verificare P1–P3, con gate memoria e Mac/Windows distinti.
- [x] Approvare il bisogno e il sottoinsieme FITS insieme al piano precedente.
- [x] Implementare e verificare il sottoinsieme FITS 2D di ADR 0010 con corpus e isolamento; non l'intero F0–F2 proposto.
- [x] Implementare export selezione, destinazione senza sovrascritture, qualità JPEG, PNG 8/16 e due DNG distinti; verificare campioni e metadati, annullamento del temporaneo e XPC. Interoperabilità DNG limitata a LibRaw, esplicitata sopra.
- [ ] Valutare separatamente wide gamut/HDR, FITS64/compressione/cubi e altri formati float.

## Navigatore filesystem — implementazione del 19 settembre 2026

### Revisione estetica richiesta: explorer in stile IDE

- [x] Correggere l'allineamento delle schede: margine comune di 8 punti in Libreria/Esplora, conservando le larghezze indipendenti. Lo switch viene completato nello stesso frame prima della presentazione e riusa rami/righe caricati, senza ricostruzione o nuova richiesta posizioni a ogni ritorno; i rami compressi restano compressi.
- [x] Ridurre il padding verticale della barra percorso da 16 a 6 punti. Regressione con clic reali nelle due lingue: coordinate dei tab identiche attraverso 16 switch, larghezze distinte conservate, nessuna ricostruzione delle righe e altezza barra entro 42 punti.
- [x] Riesaminare i Markdown dopo gli accorpamenti precedenti: mantenere separati specifiche anteprime, motori RAW, navigatore e composizione desktop; ADR e rapporti descrivono decisioni/prove differenti. Anche il breve progetto restyling conserva regole di griglia/viewer/preferenze non proprie del navigatore. Nessun altro file eliminato in questa revisione; copie architettura, originale v1.2, istruzioni, laboratorio e notice mantengono il loro scopo.
- [x] Verificare il bundle con barra compatta: fmt, Clippy desktop, build release, 14 regressioni UI, sei layout nativi IT/EN/compatti/200%, dodici catture preferenze e integrazione XPC passati. Aggiornate le quattro catture pubbliche `reports/navigator-*.png` e aperto `dist/TrueRenderer-navigator-compact.app` con dati di prova separati. [Rapporto con hash e limiti](reports/navigator-layout-macos.json); nessuna qualifica statistica di latenza o chiusura dei gate.
- [x] Compattare righe e margini, aggiungere guide gerarchiche, chevron e icone vettoriali cartella/immagine/file; selezione blu a riga intera, scheda attiva sottolineata e controlli piatti.
- [x] Verificare Clippy, 13 regressioni UI e sei layout nativi IT/EN, compatti e 200%. Bundle separato `dist/TrueRenderer-navigator-ide.app`; [rapporto](reports/navigator-ide-macos.json).
- [x] Completare dodici catture preferenze e aggiornare le tre immagini del README, aggiungendo il rimando al layout compatto. Immagini `reports/navigator-*.png` su corpus sintetico; screenshot storici conservati. Verifica XPC sul nuovo bundle passata e app aggiornata aperta con dati di prova separati. Restano i limiti di qualifica già elencati sotto.

### Implementazione funzionale e razionalizzazione documentale

Richiesta corrente del titolare: implementare il [progetto Esplora](docs/progetto-navigatore-filesystem.md), quindi razionalizzare i Markdown conservando specifiche, decisioni ed evidenze. Il lavoro non chiude i gate R0–R4.

- [x] Collegare schede Libreria/Esplora, albero pigro, aperture asincrone, breadcrumb, cronologia, tastiera, filtro nomi e modalità delle voci.
- [x] Separare enumerazione e writer con due slot filesystem e code limitate; risultati progressivi e terminali identificati, scansione indipendente dalla generazione decoder, salvataggi conservati al cambio cartella.
- [x] Aggiungere preferiti transazionali, migrazione libreria 1→2 con backup verificato, export e sessione versionata distinta dalle impostazioni.
- [x] Completare regressioni della prima implementazione Mac: navigazioni rapide/fallite, cronologia, refresh, file mirato con filtri, cambio motore durante scansione, tastiera, preferiti/sessione, migrazione/backup/export e salvataggi pendenti.
- [x] Eseguire `scripts/verify.sh --gui`: fmt, Clippy workspace con warning negati, build debug, 128 test ordinari e 10 integrazioni esplicite passati; due test fotografici privati esclusi. Sei controlli IPC, due regressioni Python, 24 segnali di ricampionamento e prova della superficie con otto schermate passati.
- [x] Costruire la release e il bundle separato, verificare sei layout nativi IT/EN, Libreria/Esplora, griglia/viewer e 200%, ispezionare tutte le catture e ripetere XPC sul medesimo hash. Controllo automatico dell'albero dentro il viewport: almeno 64 punti nella finestra 550×360. [Rapporto](reports/filesystem-browser-macos.json).
- [x] Provare un listing di 100.001 file sintetici: arresto esplicito alla quota byte prima del limite numerico, batch limitati, ordine terminale, zero cache/catalogo e crediti restituiti dopo shutdown. Test `hundred_thousand_entries_stop_explicitly_at_memory_quota`, eseguito separatamente con `--ignored`; non è una campagna di latenza né una prova NAS.
- [x] Accorpare sei Markdown in due, conservando integralmente i testi salvo titoli/ancore/link: quattro revisioni in [revisioni Windows](STATO.md#revisioni-windows-concluse), D40 e grandi RAW in [campagne RAW](STATO.md#campagne-raw-concluse). Quattro file in meno, originali recuperabili da Git. Aggiornati indice, README, rimandi e appendici tramite sincronizzatore; script di controllo link/ancore passato. Originale v1.2, LICENSE, notice e vendor invariati.

Correzioni emerse nelle prove: glifi assenti dal font, nomi centrati nell'albero e pannello compatto senza spazio utile. Controlli ASCII, testo allineato a sinistra, finestra limitata al viewport e menu Preferiti/Recenti scorrevoli correggono i sei layout osservati. Primo tentativo della suite interrotto dall'assenza delle fixture Metal; generate con lo script del progetto, quindi suite completata. Il test TIFF nativo richiede l'esecuzione fuori dal sandbox aggiuntivo del terminale. Il link XPC continua a segnalare oggetti LibRaw compilati per macOS 27 rispetto al target 13: questa campagna su macOS 27/M4 non qualifica il minimo OS né una firma di rilascio.

I vecchi nomi di documenti negli inventari JSON storici rimangono tali, per non alterare l'evidenza delle campagne. Le copie delle architetture richieste dal flusso del titolare, l'originale immutabile, gli ADR, i progetti con requisiti unici e le attribuzioni non sono stati eliminati. La cronologia degli accorpamenti non viene più duplicata nell'indice documentale.

**Aperto/mancante rispetto alla specifica completa:** watcher nativi, identità persistente e stato offline qualificato dei volumi, limite di concorrenza per volume, adattatori alias/shortcut/cloud e ripristino completo delle espansioni discendenti. Il fallback usa polling ogni 15 secondi, due slot totali, al massimo 16 rami riletti e 64 MiB prenotati per navigazione/listing; link/pacchetti/placeholder riconosciuti non vengono attraversati automaticamente. Mancano prove native Windows, VoiceOver/NVDA, NAS con syscall bloccate, fault injection di crash/disco pieno nella migrazione e campagna statistica prestazioni/pressione. N0–N7 non sono dichiarati integralmente conclusi e i relativi gate restano aperti.

## Progetto di incremento — navigatore filesystem commutabile, 15 settembre 2026

Richiesta storica del 15 settembre: pianificare un navigatore di file/cartelle a sinistra, **commutabile con il pannello già presente**. Specifica: [progetto Esplora / Libreria](docs/progetto-navigatore-filesystem.md). La pianificazione era solo documentale; l'implementazione e il perimetro verificato del 19 settembre sono registrati sopra. Le caselle seguenti riguardano la qualifica completa, non la sola presenza delle funzioni.

- [x] Esaminare codice e architettura e documentare layout, switch Libreria/Esplora, interazioni, concorrenza, persistenza, fasi e matrice di accettazione; nessuna implementazione applicativa in questa sessione.
- [ ] N0: verificare contratti, baseline e spike nativi/accessibilità, includendo la conservazione del pannello attuale.
- [ ] N1–N2: modello dell'albero, enumerazione asincrona limitata, coordinatore di navigazione e scansione progressiva separata dai salvataggi.
- [ ] N3: pannelli Libreria/Esplora commutabili, file/cartelle, breadcrumb e cronologia, mouse/tastiera e layout IT/EN; switch senza reset di selezione, filtri, zoom o qualità.
- [ ] N4–N5: preferiti durevoli, sessione, migrazione/backup, refresh, watcher, volumi e casi filesystem.
- [ ] N6–N7: eseguire la matrice di accettazione, verificare bundle/XPC e piattaforme dichiarate, registrare risultati e limiti in questo documento.

## Prosecuzione — cache e verifica Mac, 15 settembre 2026

- [x] Riesaminare i progetti attivi: il restyling e il bundle/corpus/XPC sono verificati; qualifica fotografica RAW, prestazioni integrate e gate R0–R4 restano aperti.
- [x] Correggere la rimozione di hard link nella cache v1/v2, mantenendo le letture valide e proteggendo anche link comparsi dopo una scansione; 25 test cache passati.
- [x] Completare suite workspace (106 ordinari + 10 integrazioni), build debug/release e prove native del nuovo bundle: cache, 8 schermate, 11 regioni e XPC passati; [hash e limiti](reports/cache-integrity-macos.json), due test fotografici privati esplicitamente saltati.
- [ ] Qualifica integrata evento→frame (§12 della specifica anteprime): primo probe comando→superficie implementato sotto; presentazione effettiva e campagna completa ancora aperte.

### Harness della superficie — 15 settembre 2026

- [x] Collegare un probe opt-in al viewer reale: comando, raster esatto aggiunto a egui, ricezione della superficie e confronto pixel successivo alla misura.
- [x] Preparare esecuzioni isolate CPU/GPU, cache applicativa fredda, SSD dopo riavvio e ritorno RAM; impedire caricamenti prima del primo comando.
- [x] Verificare release Standard/Full: 24 processi isolati, 72 selezioni e confronto pixel entro un livello sRGB8; CPU/GPU, cache fredda/SSD/RAM confermati. [Rapporto](reports/navigation-surface-macos.json). Il readback non è un timestamp del display; tre selezioni dipendenti non qualificano p95/p99.
- [ ] Completare scroll, transizioni di risoluzione RAW, pressione, baseline e campagna statistica con intervalli di confidenza prima di chiudere §12; zoom/pan e override sul corpus piccolo sono verificati sotto.

### Transizioni del viewer — 15 settembre 2026

- [x] Estendere il probe a zoom 300%, pan, Fit, override Standard/Full e 1:1 fisico usando gli stessi comandi del viewer.
- [x] Registrare copertura completa/parziale e riproiezione a ogni ridisegno della stessa sorgente; fallire se manca completamente il contenuto.
- [x] Verificare release Standard/Full e CPU/GPU: 24 processi, 240 azioni, 24 controlli 1:1 esatti, 1.568 ridisegni delle transizioni senza draw completamente vuoti; [rapporto](reports/navigation-transitions-macos.json). Copertura provvisoria minima 40,5%, non sempre completa.
- [x] Migliorare il ritorno a Fit conservando una rappresentazione con copertura più ampia della stessa sorgente/revisione entro quota; verificare invalidazione, rilascio memoria e continuità senza inventare pixel mancanti.

### Copertura del ritorno a Fit — 15 settembre 2026

- [x] Conservare facoltativamente un frame più ampio per vista, foto, digest, motore e dimensioni sorgente: massimo due complessivi, entro un quarto dei budget applicabile/GPU e 64 frame totali.
- [x] Abbandonare prima i frame facoltativi in caso di pressione, riduzione quota, ammissione renderer/decoder o invalidazione; mantenere i crediti fino al completamento previsto.
- [x] Riprodurre il fallimento di copertura completa sul bundle precedente e aggiungere regressioni di geometria, identità, quote e rilascio.
- [x] Verificare CPU/GPU e Standard/Full: 24 processi, 240 azioni, 1.553 ridisegni con copertura completa, 24 controlli 1:1 esatti; invalidazione, recupero grafico e XPC passati. Bundle aggiornato e [rapporto](reports/fit-coverage-macos.json).
- [ ] Estendere le prove a livelli RAW ridotti, pressione reale e assenza della riserva; la copertura completa del corpus piccolo non è una garanzia universale.

## Immagini grandi, pressione e RAW Mac — 15 settembre 2026

- [x] Verificare livelli residenti 12/24/45 MP, Standard/Full, CPU/GPU con fallback esplicito, freddo/riapertura: 24 processi e 240 azioni, 1:1 esatto.
- [x] Provare espulsione della riserva a 1536 MiB e rifiuto sicuro del decode a 512 MiB con crediti restituiti; distinguere pressione iniettata e memoria fisica campionata.
- [x] Riprodurre e correggere il crash LibRaw sullo stack XPC; 120 sviluppi su 30 D750 autorizzati passati, originali invariati.
- [x] Confrontare nove ritagli centrali registrati e ripetere 32 casi Bayer con verità nota; non attribuire differenze di ricetta alla sola qualità del demosaic.
- [x] Completare 40 azioni RAW, invalidazione 45 MP, cinque stadi del cambio motore UI e verifica XPC; 75 test Rust passati, bundle aggiornato. [Rapporto e limiti](reports/large-pressure-raw-macos.json).
- [x] Ridurre e qualificare il picco memoria su 45 MP Full nel perimetro della [campagna corrente](#incremento-corrente--memoria-delle-immagini-grandi).
- [ ] Ampliare a pressione fisica OS, decode RAW ridotto/regionale e target colore calibrato; il rispetto del budget nei casi misurati non chiude il gate universale.

## Restyling desktop — 15 settembre 2026

- [x] Rivedere il README dopo la consegna: schermate aggiornate di viewer, griglia e preferenze, spiegazione dei selettori e istruzioni per il bundle separato; controllare immagini e collegamenti locali.
- [x] Completare la ripresa e la consegna: correggere Esc nei popup senza uscire dal viewer, verificare il bundle finale con UI/pixel/XPC, recuperare rapporto e screenshot mancanti e aprire l'app aggiornata.
- [x] Revisione richiesta dal titolare: allineare barra e Settings, mostrare motore applicato, spostare strumenti in alto e cartella/conteggi in basso; selettori espliciti Globale e Solo questa foto Standard/Piena; sette regressioni UI con clic reali egui, screenshot e verifiche del bundle in [rapporto](reports/toolbar-macos.json).
- [x] Definire composizioni per griglia, viewer e preferenze; [progetto](STATO.md#restyling-concluso).
- [x] Centralizzare stile e gerarchia visiva; compattare barra, navigazione, miniature e strumenti del viewer.
- [x] Organizzare ispettore in sezioni richiudibili e preferenze in quattro schede con azioni fuori dall'area scorrevole.
- [x] Verificare bozze, comandi, italiano/inglese e layout alle dimensioni ridotte/200%; 12 screenshot preferenze e 8 di rendering, 11 regioni entro la soglia di 1 livello sRGB8.
- [x] Completare build release e copia separata `dist/TrueRenderer-restyle.app`, firma e XPC; aggiornare registro/README e sincronizzare le architetture. [Rapporto e limiti](reports/restyle-macos.json).

## Interfaccia bilingue — 14 settembre 2026

- [x] Tradurre controlli, preferenze, guida e messaggi applicativi; inglese predefinito, menu Language/Lingua con English e Italiano.
- [x] Salvare la lingua senza applicare altre preferenze in bozza o invalidare immagini, selezione e zoom; compatibilità con impostazioni precedenti.
- [x] Aggiornare README con lingua, comandi inglesi e limiti dei dialoghi/diagnostica nativi.
- [x] Verificare build, lint, cinque nuove regressioni e smoke grafico inglese/italiano; sincronizzare le architetture anche con Python 3.9. Suite desktop: 39 passati, 6 ignorati e un fallimento cache riprodotto sul commit base; [rapporto](reports/localization-macos.json).

## Revisioni Windows del 13–14 settembre

- [x] Revisionare README e Markdown dopo la pubblicazione: separare Windows/macOS e stato corrente/storico, indicare i documenti archiviabili, verificare i link locali e sincronizzare le architetture.
- [x] Verificare i percorsi vicini alle ultime correzioni con 18 nuovi casi sintetici per worker debug/release e confrontare gli hash della precedente suite.
- [x] Applicare la curva PNG con sola `gAMA=45455`, senza identificarla con sRGB; regressioni numeriche 8/16 bit, alpha, provenienza e precedenze, cache v5.
- [x] Distinguere orientamento TIFF assente da 1 e impedire che una IFD secondaria ruoti l'anteprima principale; regressioni per directory/EXIF e worker LPAC, cache v5.
- [x] Riesaminare il lavoro non committato e rieseguire fmt, Clippy, 86 test ordinari e 6 integrazioni; verificare LibRaw e inventario/notice.
- [x] Riprodurre i nuovi rilievi con file sintetici, broker reale e modulo Directory di produzione; documentare risultati e limiti.
- [x] Rifiutare esplicitamente la colorimetria TIFF non supportata nei tag 301/318/319/342/532; verificare rifiuti e pixel senza tag, invalidare le vecchie cache con `bitmap-tiff-color-v4`.
- [x] Rendere coerenti probe e decode nel fallback RAW su anteprima di diversa risoluzione, conservando la validazione IPC; regressione LPAC passata.
- [x] Correggere l'aggiornamento del timestamp degli hit cache Windows e verificare scadenza/LRU, descrittori/blocchi e preservazione degli hard link.
- [ ] Estendere la diagnostica PNG per cICP malformato e dichiarazioni sRGB/gAMA discordanti, distinta dai tre P2.
- [x] Recuperare la sessione, verificare i rilievi e completare build release, ricampionamento e riproduzione del blocco di compilazione Mac; registrare gli esiti senza modifiche applicative o commit.
- [x] Ripristinare il controllo esaustivo di `Owner` nel test di crash/recovery e compilare la prova minima con entrambe le varianti.
- [x] Completare la suite Mac: rilievo cache risolto, 106 test ordinari e 10 integrazioni passati il 15 settembre, fmt/Clippy e build debug/release senza warning Rust.
- [ ] Estendere la verifica fotografica nativa Mac dei motori; le prove del corpus e del bundle non qualificano la matrice Apple/LibRaw.
- [x] Correggere il percorso «Applica e salva» per conservare selezione/zoom al cambio motore; regressioni e smoke nativo sulla stessa azione completa del pulsante.
- [x] Gestire o rifiutare esplicitamente PNG `cICP` e TIFF con alpha associata; aggiungere regressioni e invalidare le cache interessate.
- [ ] Qualificare separatamente contesa del writer e persistenza del dettaglio: nello smoke serale una scrittura saltata per lock Windows 33 e nuovo decode al ritorno, senza interrompere la visualizzazione.
- [x] Inventariare tutto il lavoro non committato e rivedere anche port Windows, cache, bridge nativo, build, dipendenze e documenti.
- [x] Verificare debug/release, 82 test Rust, 6 prove IPC, 2 Python, ricampionamento e 156 sviluppi release D750/D40; originali invariati.
- [x] Provare GUI release e cambio motore; riprodurre separatamente i difetti, conservando log e laboratorio privati.
- [x] Correggere accesso residuo ai file del worker Windows e verificare LibRaw 0.22.2 con le correzioni applicabili; perimetro LPAC e limiti in ADR 0009.
- [x] Correggere colore PNG esterni, classificazione TIFF/RAW e accesso fuori limite nel parser preview; regressioni passate su Windows.
- [x] Correggere quota al cambio tipo di sorgente, controllo hard link e invalidazione Windows con mtime ripristinato; completare provenienza/migrazione impostazioni.
- [x] Incorporare il runtime C/C++ MSVC e controllare gli import release; generare inventario Windows, 348 notice e manifest nativo, preservando i byte upstream nei checkout Git.
- [x] Costruire una copia del nuovo bundle Mac/XPC: release separata del 15 settembre con corpus, UI e XPC verificati; la matrice fotografica estesa dei motori resta aperta.
- [ ] Verificare il pacchetto in un'installazione Windows pulita prima della distribuzione.
- [x] Accorpare audit/correzioni, piano Windows e nota LibRaw; ridurre la cronologia duplicata, aggiornare riferimenti e verificare la sincronizzazione.
- [ ] Qualificare notifiche di pressione memoria Windows, percorsi lunghi/manifest longPathAware e gestione ICC prima di dichiarare completa la piattaforma.

## Obiettivo e regole

Applicazione desktop Rust per sfogliare, selezionare e ispezionare immagini con una resa tracciabile. Target previsti per la prima versione: macOS arm64 e Windows x86-64, SDR; sono implementati e provati prototipi macOS arm64 e Windows x86-64, con perimetri e qualifiche ancora aperti. Le tappe del documento sono criteri di accettazione: una schermata funzionante non chiude un gate di colore, sicurezza o accessibilità.

Si sviluppa una verticale per volta. Ogni consegna contiene codice compilabile, prove riproducibili, limiti osservati e aggiornamento di questo stato; l'architettura cambia quando cambiano requisiti o decisioni. Nessuna modifica agli originali; annotazioni separate dall'indice. Nessun servizio cloud, account o canone. Tutto il progetto, inclusa la toolchain locale, rimane nella cartella del progetto. Dati di sviluppo locali in `var/`; prima di una release spostarli nel percorso app-data nativo qualificato.

## Primo incremento — prototipo R0

- [x] Leggere requisiti, invarianti, architettura, UX, schema e roadmap; conservare il documento originale.
- [x] Registrare il nuovo nome e questo piano, con tracciamento progressivo nel documento richiesto.
- [x] Installare Rust localmente senza cambiare PATH o configurazione della shell.
- [x] Creare workspace con `tr-core`, `tr-app`, `tr-render`, `tr-store`, `tr-platform`, `tr-worker` e desktop.
- [x] Implementare tipi validati, stato deterministico, geometria e riferimento numerico sRGB/Rec.2020 fp32.
- [x] Implementare IPC bounded con processo decoder persistente, timeout, copia privata e rifiuto di risposte invalide.
- [x] Generare un corpus deterministico proprio e ammettere solo i suoi digest finché la sandbox OS non è qualificata.
- [x] Collegare finestra egui/wgpu, griglia virtualizzata, anteprima, zoom fisico 1:1, confronto a due e ispezione.
- [x] Prototipare annotazioni durevoli, ricerca e filtri; indice separato, revisioni, undo e backup SQLite.
- [x] Preparare avvio da doppio clic, bundle macOS interno e istruzioni riproducibili.
- [x] Eseguire test di dominio/colore/IPC/persistenza, build, lint e smoke test grafico; salvare i risultati reali.
- [x] Aggiornare lo stato finale e specificare il prossimo incremento senza dichiarare conclusa la v1.

**Verifica dell’incremento conclusa:** avvio del bundle dal Finder passato dopo il consenso macOS al Desktop; passata anche una copia con bundle e corpus indipendenti. Dettagli in `STATO.md` (registro e collegamenti ai rapporti JSON).

## Incremento successivo — prova XPC macOS

- [x] Creare un laboratorio separato con host e servizio XPC firmato ad hoc, con il solo entitlement App Sandbox.
- [x] Misurare accesso negato a file sintetici non concessi, scrittura, connessioni loopback e creazione di processi.
- [x] Provare il trasferimento di un descrittore in sola lettura e verificare che non permetta scrittura.
- [x] Osservare PID su due connessioni, crash/recovery e memoria del servizio senza dedurre garanzie non misurate.
- [x] Salvare risultati e decisione successiva. Non collegare il decoder esterno prima del gate completo.

Passati i controlli delle due varianti: sandbox base e sandbox con `RLIMIT_NPROC` hard/soft 0. File/rete negati, descrittore in sola lettura verificato, crash contenuto. Il limite aggiuntivo blocca i figli; due connessioni condividono un PID. Il gate sandbox completo rimane aperto. Codice, risultati e decisione in `experiments/macos-xpc/README.md`.

## Incremento 0.1.1 — integrazione del confine XPC

Completata la verticale interna: broker macOS, decoder riutilizzabile e due servizi nel bundle; percorso su pipe per i test del core e i target non ancora qualificati. Build 0.1.1 verificata all’epoca con prove native del decoder, controllo grafico e copia autonoma passati; ora conservata nello storico; versione corrente 0.1.5. Restano aperti i gate elencati sotto.

- [x] Portare il decoder controllato in due servizi XPC con protocollo senza percorsi e allowlist anche nel decoder.
- [x] Vincolare il broker al CDHash del servizio installato, verificare due PID reali e conservare la copia privata degli output.
- [x] Separare i due decoder dal writer SQLite; coda di 64 richieste, priorità, cancellazione e riciclo al cambio generazione.
- [x] Misurare terminazione di un decoder sospeso, recupero indipendente e supervisione memoria con fault injection limitata a 512 MiB.
- [x] Verificare che il servizio normale rifiuti il comando di fault injection.
- [x] Completare smoke grafico, avvio Finder e rilocazione della build 0.1.1.
- [x] Verificare il riciclo del decoder inattivo al cambio cartella, anche senza nuovi job; 26 test Rust e 6 prove IPC passati.
- [ ] Qualificare il firmatario di release e il requisito reciproco dei peer; il controllo attuale del servizio verifica soltanto l’identificatore dell’host.
- [ ] Qualificare l’API di terminazione per i minimi OS: attualmente libproc SPI con audit token, verificata su macOS 26.6.2.
- [ ] Estendere misure di revoca al cambio dominio, handle duplicati e overshoot sotto pressione; il campionamento RSS non è un tetto rigido.
- [ ] Estendere i test negativi a input ostili, descrittori residui, output concorrente e quota di risorse.
- [ ] Completare la qualifica della verticale Windows e le prove di display/accessibilità prima di chiudere R0.

Decisione e limiti in `docs/isolamento-decoder-e-formati.md#adr-0002`; evidenze in `STATO.md` (registro e collegamenti ai rapporti JSON). Per completare il gate XPC sul Mac restano la suite avversaria del bootstrap/ciclo di vita e la qualifica estesa della memoria end-to-end; l’ordine dell’incremento locale corrente è indicato in apertura. Nella 0.1.1 i PNG esterni restavano esclusi; la modifica del perimetro nella 0.1.3 è registrata in ADR 0004. Il gate completo della sandbox rimane aperto.

## Incremento 0.1.2 — fedeltà del ricampionamento e documentazione

Concluso il sottoinsieme correttivo richiesto. Build 0.1.2 verificata all’epoca e conservata nello storico; i gate generali R0/R1 restano aperti.

- [x] Osservare la finestra precedente e riprodurre i problemi di Frequenze radiali in Adatta e griglia.
- [x] Usare una sorgente/piramide condivisa in griglia, viewer, inspector, filmstrip e confronto.
- [x] Filtrare in luce lineare alla dimensione fisica, mantenendo 1:1 allineato senza filtro; registrare la variante Lanczos e il trattamento alpha in ADR 0003.
- [x] Eliminare il secondo ridimensionamento delle miniature e limitare il calcolo alle regioni visibili con coda asincrona e cache bounded.
- [x] Superare quattro nuovi test Rust, 24 casi sinusoidali e confronto esatto LOD 0 del corpus invariato.
- [x] Acquisire otto schermate native e confrontare 14 regioni con il raster atteso: zero differenze di canale su questo Mac.
- [x] Superare fmt/clippy, 30 test Rust, 6 controlli IPC, suite XPC e verifica firma/hash del bundle 0.1.2.
- [x] Verificare avvio Finder e copia autonoma con database integri; conservare il bundle precedente.
- [x] Spiegare la differenza PLAN/Architettura e confrontare tutti i capitoli 1–24 con codice/prove, distinguendo mancante, parziale, non qualificato e post-v1.
- [x] Aggiornare entrambe le architetture senza cambiare il backup originale.

**Rimangono da qualificare (§10/§19.3):** alias 2D/Siemens, overshoot, grafo CPU/GPU e filtro diretto, transizioni LOD, EXIF 1–8, f64 della geometria completa, tile/cuciture, altri display/DPI e prestazioni. Il filtro CPU può mostrare brevemente lo sfondo durante il ricalcolo: «mai viewport vuoto» e tutti i p95 non sono ancora garantiti. Le 24 sinusoidi includono 11 casi di banda di transizione osservati senza soglia di accettazione. Le soglie valide riguardano 8 casi di stopband e 5 di passband.

## Incremento 0.1.3 — formati esterni e GitHub proprietario

Richiesta del titolare: repository **pubblico**, **licenza proprietaria**, README e apertura effettiva dei formati. ADR 0004 registra l'anticipo dei decoder Apple rispetto ai gate della proposta; Standard/Riferimento restano indisponibili.

- [x] Creare il repository pubblico `antoniodicoratoinfodev/TrueRenderer`.
- [x] Preparare LICENSE proprietaria, README, NOTICE, regole per contributi e segnalazioni.
- [x] Aggiungere Apri file, trascinamento file/cartella e `--open`.
- [x] Decodificare JPEG/PNG/TIFF/GIF/BMP/HEIC/WebP nel servizio macOS isolato.
- [x] Sviluppare RAW completo con CIRAWFilter, ricetta nominata e nessun fallback su JPEG incorporato.
- [x] Conservare 16 bit/alpha, applicare EXIF una volta, mostrare provenienza e SHA-256.
- [x] Definire quote 256 MiB/64 Mi pixel e cache, evitando la copia integrale del raster per la piramide.
- [x] Superare 19 fixture di formati, 6 controlli aggiuntivi, 32 test Rust e 6 controlli IPC; generatori riproducibili senza fotografie esterne.
- [x] Eseguire tre schermate native di griglia, DNG e JPEG 12 MP.
- [x] Verificare pacchetto finale, regressioni di campionamento, XPC, Finder e copia autonoma; installare 0.1.3.
- [x] Aggiornare entrambe le architetture e pubblicare il commit completo, escludendo dati personali e artefatti locali. Incremento applicativo `424d307` su `main`.

Restano: matrice reale fotocamere/sottotipi/ICC, RAW multipiattaforma con LibRaw, memoria globale e gate R0/R1/R3 completi. I test sul DNG sintetico non qualificano ogni RAW. HEIC/WebP sono un'anticipazione limitata della precedente lista post-v1.

## Incremento 0.1.4 — cache per cartella e impostazioni

Richiesta del titolare del 7 settembre: cache e temporanei accanto alle immagini, quote configurabili, README inglese. Pubblicare questo incremento prima di ulteriori ottimizzazioni delle prestazioni.

- [x] Creare `.truerenderer-cache/entries` e `tmp` nella cartella aperta, con proprietà riconoscibile e fallback in RAM per cartelle non scrivibili.
- [x] Persistenza lossless fp32 della piramide e provenienza; chiave da SHA-256 sorgente, pipeline/decoder e versione; invalidazione e rifiuto dei file corrotti.
- [x] Quote per cartella, temporanei prenotati, riserva di spazio libero, LRU/scadenza, pulizia sicura dei soli derivati.
- [x] Impostazioni persistenti e comandi di pulizia, stato cache e hit/miss visibili.
- [x] Test di precisione, corruzione, limiti, concorrenza, symlink, cancellazione e file originali invariati; misure a cache fredda/calda.
- [x] README in inglese, ADR e sincronizzazione dei due documenti; bundle e verifiche native.
- [x] Commit e push dell’incremento cache: `2dad0b2` pubblicato su `main`, prima delle ottimizzazioni successive.
- [x] Dopo il primo push: profilare, accelerare SHA-256 e riusare lo snapshot sul miss; 40 test e pacchetto finale verificati, JPEG/DNG caldi circa 7× più rapidi nelle prove locali.
- [x] Pubblicare il secondo incremento: `a901942` su `main`; registrati risultati e commit nei documenti.

## Progetto di incremento — anteprime, cache e prestazioni

Richiesta del titolare, 7 settembre 2026: progettare prima di implementare la navigazione con cache RAM/SSD, scelta fra Anteprima Standard e Piena, limiti di memoria configurabili e uso intenso di CPU/GPU durante il lavoro utile. Specifica dettagliata: [progetto di anteprime, cache e prestazioni](docs/progetto-anteprime-cache-prestazioni.md), revisione 2 pubblicata nel commit `a2db2ab`, ora revisione 5 integrata nell’appendice E dell’architettura.

**Stato: implementazione 0.1.5 disponibile; qualifica integrata aperta.** Baseline 0.1.4 conservata in `reports/preview-baseline-macos.json`; decisioni applicate e limiti in [ADR 0006](docs/progetto-anteprime-cache-prestazioni.md#adr-0006). Anteprima Standard/Piena rimane distinta dai badge di pipeline Standard/Riferimento. Nessun gate R0–R4 è chiuso da questo incremento.

- [x] A, modelli: qualità/richieste, migrazione compatibile, budget/lease, ammissione prima delle allocazioni, preferenze RAM/GPU/CPU e adattamento all'alimentazione.
- [x] A, baseline locale: conservare misure della build 0.1.4 senza confondere il caricamento dell'intera piramide con le nuove miniature autonome.
- [x] Revisione locale 9 settembre: 30 NEF Nikon D750 a risoluzione nativa, Standard/Piena fredde/calde e riapertura cache verificati con budget esplicito 3 GiB; 64 test Rust e suite nativa finale passati. Alla chiusura di quella prova il lavoro era locale; ora è incluso in `10123b5`. Ripresa nella sezione [Punto di ripresa](#punto-di-ripresa).
- [ ] A, qualifica RAW: baseline e target di throughput su corpus reale autorizzato 12/24/45 MP.
- [x] Continuazione 9 settembre: ridurre la durata dei buffer compressi, stimare il massimo delle fasi e contenere snapshot/decode pesanti. 67 test Rust e 30 NEF a 2 GiB, incluse due richieste simultanee, passati; footprint campionato massimo 2.100.284.608 byte in un bundle isolato dalle altre istanze.
- [x] Verificare e aggiornare il bundle della continuazione: cache, XPC, sorgenti/device, suite Finder/rilocazione/database/firma passati; quattro binari identici alla prova RAW. Conservare il pacchetto precedente e sincronizzare piano, registro e architetture.
- [ ] Budget RAW full-frame: estendere la qualifica a 12/24/45 MP, carichi misti, tutte le viste e pressione fisica; valutare decode ridotto/regionale per i carichi non ammessi. I 30 NEF di una fotocamera non chiudono il requisito generale.
- [x] B: livelli e frame autonomi, writer asincrono limitato in byte, I/O cache indipendente dai due decoder, consegna prima della persistenza.
- [x] C: Standard/Piena persistenti, override per foto/1:1, provenienza e completezza, intent e probe; fallback dichiarato di sviluppo completo temporaneo per Standard.
- [ ] C, ottimizzazione opzionale del backend: qualificare sviluppo RAW ridotto Apple; non attivato nella 0.1.5.
- [x] D: artefatti v2 lossless a blocchi bounded nella stessa quota/lock v1, letture autonome, verifica SHA sorgente, pubblicazione descrittore per ultimo, GC e minimo recuperabile per miniature; regressioni cache negative.
- [x] E, percorso disponibile: domanda visibile, promozione, cancellazione dei consumatori obsoleti, raggruppamento di uno sviluppo per richieste compatibili, prefetch dopo stabilità, pausa/preparazione esplicita; pool Rayon e NEON verificato bit-exact.
- [x] E, scheduler macOS: P0–P6/FIFO, promozioni durante lookup, prefetch direzionale con ritardo adattivo, notifiche pressione OS e precedenza locale ai lettori durante writer/GC; regressioni passate.
- [x] E, continuazione navigazione 9 settembre: annullare lookup/ammissioni non più richiesti, completare le chiamate native in corso senza riciclarle al cambio vista, riusare la stessa sorgente al cambio qualità e non consegnare consumatori abbandonati. Quattro regressioni aggiuntive; 71 test Rust e suite del codice passati.
- [x] E, confronto dello stadio renderer: 100 prove CPU scalare/parallela/GPU per ciascuna di due geometrie, grafi identici; CPU parallela bit-exact e beneficio misurato con intervallo bootstrap.
- [x] Consegna della continuazione navigazione: 30 NEF a 2 GiB nuovamente passati, tre prove native di recupero e suite completa del bundle installato; pacchetto precedente conservato. Evidenze in `reports/preview-navigation-continuation-macos.json`.
- [ ] E, qualifica integrata: beneficio CPU scalare/parallela evento→frame, pressione fisica e adattatori sugli altri OS.
- [x] F, percorso disponibile: compute WGSL separabile e display SDR, pipeline/input persistenti, texture diretta, controlli capability/quote, fallback CPU e lease fino al completamento GPU; controllo numerico prima dell'abilitazione.
- [x] F, Apple: riuso del contesto CPU provato; esperimento Metal distinto conforme sui casi sintetici. Metal nel decoder rimane disabilitato finché backend e beneficio non sono qualificati.
- [x] F, recupero applicativo: invalidare il presenter, ricreare una sola volta finestra/device, riverificare il compute e conservare servizio, salvataggi, undo e selezione. Arresto esplicito alla seconda perdita o se il backend finestra ha già propagato un panic.
- [ ] F, qualifica: reset fisici, OOM, matrice driver/display e beneficio integrato sulle navigazioni reali nei diversi profili.
- [x] §10, sorgenti residenti: monitor fuori UI dei file richiesti/residenti, invalidazione di RAM/presentazione e risultati tardivi su modifica/rimozione/ripristino, stato esplicito e annotazioni conservate. Token Unix più robusto; osservazione best-effort, non revisione coerente v1.
- [ ] G: completare qualifica di tutti i gate del §12, 1.000 RAW reali, scenari freddi/caldi/disabilitati/sotto pressione, p95/p99 evento→frame e misure dei driver. I report sintetici dichiarano il proprio perimetro e non sostituiscono questi gate.
- [x] Pubblicare l’incremento verificato richiesto dal titolare: commit `1b98b4f` su `origin/main`, 62 test Rust e suite nativa del bundle passati; documenti e licenza LibRaw aggiornati, requisiti residui espliciti.
- [x] Integrare l’intero progetto e ADR 0005–0006 nell’appendice E delle architetture della radice e di `docs/`; correggere percorsi e sincronizzazione, preservando il testo utente e il backup originale.
- [ ] Eliminare `docs/progetto-anteprime-cache-prestazioni.md` al completamento dell'intera richiesta, dopo aver trasferito risultati e decisioni nel registro. Il documento è conservato perché restano requisiti aperti.

Le caselle separano il codice verificato dalla qualifica ancora necessaria; non modificano i criteri del progetto per far risultare conclusa una fase parziale. Evidenze aggiornate in `STATO.md` (registro e collegamenti ai rapporti JSON) e `STATO.md`.

**Licenza LibRaw, verifica storica dell'8 settembre:** la nota e le fonti sono conservate in [ADR 0008](docs/progetto-motori-raw.md#adr-0008) e §20.1 dell'architettura. La build di quella campagna non incorporava LibRaw; quella attuale collega LibRaw 0.22.2. Inventario, notice e obblighi dell'artefatto effettivamente distribuito restano da verificare prima del rilascio.

## Esperimento locale — motori RAW selezionabili (12 settembre 2026)

Richiesta del titolare: progettare ed eseguire la pipeline sperimentale, affiancandola ai motori esistenti su Windows/macOS. Incremento pubblicato in `67146e3`. Progetto: [motori RAW](docs/progetto-motori-raw.md). Anticipazione locale del demosaic prima post-v1, senza modifica dei gate.

- [x] Leggere AGENTS.md, piano, registro e architettura; definire contratto e verifiche.
- [x] Collegare selezione persistente, IPC, identità cache e invalidazione al cambio motore.
- [x] Implementare LibRaw AHD e TrueRenderer direzionale fp32 con calibrazione esplicita.
- [x] Verificare segnali sintetici, regressioni e RAW D750 su Windows; registrare i risultati: 32 casi analitici e 90 sviluppi completi, originali invariati. Superiorità generale non dimostrata.
- [x] Provare selettore e ritorno al motore precedente nella finestra; confrontare la superficie con CPU. Rimosso il secondo dithering egui: 558.144 pixel delle quattro regioni entro un livello sRGB8, dopo un primo esito negativo a due livelli.
- [x] Aggiornare stato, licenze locali e ripresa; sincronizzare le architetture preservando testo esterno ai blocchi e backup originale.
- [x] Revisione richiesta e D40: correggere scansione revocata dal cambio motore, aggiungere il modello esatto e verificare 66 sviluppi, UI/scansione e regressione D750. [Dettagli](STATO.md#campagne-raw-concluse).
- [ ] Qualificare build/bundle macOS e XPC, confronto Apple e fedeltà cromatica sul target.

## R0 — fattibilità (6–8 settimane nel documento, da ricalibrare)

Dipendenze: nessuna milestone precedente. Il prototipo corrente è una parte di R0.

1. Corpus autorizzato con manifest, hardware di riferimento Win/mac e protocollo di misura.
2. Finestra e device/coda condivisi, viewport opaco, shader, contratto sRGB della superficie.
3. Input PNG ICC, alpha, CMYK, precisione di confine e confronto CPU/GPU quantitativo.
4. Protocollo con framing, request ID, quote, handle e output del broker; fault injection e worker avversario.
5. XPC firmato/App Sandbox su macOS; LPAC senza capacità e Job Object su Windows; test negativi filesystem/rete e memoria. Il solo processo separato **non** soddisfa questa voce.
6. Snapshot/revisione sorgente sotto writer concorrente, nessuna promozione sul solo hash/stat.
7. Griglia 100k, IME, focus, DnD, VoiceOver/NVDA, scala 200%, monitor con profili e scale differenti, device loss.
8. ADR toolkit, minimi OS/GPU, licenze, canali; pacchetti interni sui due OS.
9. Almeno cinque interviste e raccolta autorizzata dei casi difficili. Richiede persone reali.

Uscita: tutti i gate di §4.4/§22.4 misurati. Mancando prove complete, egui e il colore restano provvisori. Le anteprime esterne macOS introdotte dalla 0.1.3 sono una deviazione esplicita di sviluppo (ADR 0004), non il superamento del gate.

## R1 — viewer SDR (10–16 settimane, dopo R0)

1. Bloccare backend nativi JPEG/PNG/TIFF, opzioni di compilazione, licenze e corpus per profilo.
2. Campioni nativi 8/16 bit, EXIF qualificato e orientamento una volta; rifiuto dei contenuti non supportati.
3. Little CMS isolato, ingressi RGB/GRAY/CMYK, ICC v2/v4, precedenza metadati colore, assegnazione esplicita.
4. Working linear Rec.2020 fp32, alpha premoltiplicata, viewport opaco, clipping soltanto dichiarato in uscita.
5. TileProvider, CPU reference, Lanczos3 a supporto adattato, pixel fisici 1:1 senza filtro, bordi e cuciture.
6. Renderer wgpu/WGSL con budget GPU/CPU, LUT adattive qualificate, fallback selettivo, recovery.
7. Istogramma/campionatore e provenance per stadio; badge Standard/Riferimento solo con criteri superati.
8. Libreria ICC e monitor, qualifica XMP Toolkit round-trip; nessun sidecar scritto prima del gate.
9. Viewer, loupe, confronto sincronizzato, tastiera e accessibilità provati sui due OS.

Uscita: suite §§19.2–19.3, demo controllata e tutti i formati dichiarati verificati. Non è ancora alpha su archivi reali.

## R2 — browser e dati durevoli (12–18 settimane, dopo R1)

1. Scansione incrementale, watcher, volumi, identità distinta da percorso/digest, file mancanti e ambiguità.
2. Schema completo separato index/library, migrazioni con backup, writer unico, proiezioni ricostruibili.
3. Rating, label, keyword gerarchiche, raccolte statiche/intelligenti, filtri e ricerca con indici/FTS.
4. Revisioni e journal transazionali, batch, undo come nuova revisione, recovery con errori disco/crash.
5. XMP a tre vie, no-clobber/coordinamento filesystem, conflitti visibili e riconciliazione esiti incerti.
6. Backup rotanti verificati, export portabile e restore; retention distinta dalla cache.
7. Griglia 100k misurata, code/priorità/cancellazione/backpressure, quote cache/memoria.
8. Solo dopo sandbox negativa e backup: pilot con 8–12 fotografi; almeno cinque utenti settimanali per quattro settimane.

Uscita: workflow completo senza assistenza e gate prodotto §2.0. Se debole, ridurre o fermare RAW/gigapixel.

## R3 — RAW e grandi immagini (16–24 settimane, dopo il pilot R2)

1. Pin LibRaw/opzioni, distinta e matrice reale fotocamere/CFA; nessun supporto RAW generico.
2. Ricetta nominata, preview incorporata separata dal render, WB/matrici e precisione dichiarati.
3. Tile TIFF/BigTIFF, livelli con provenienza, piramide persistente per necessità e invalidazione revisionata.
4. Preparazione sequenziale con progresso/cancellazione, spool coerente, quote, recovery e garbage collection.
5. Prova 4 Gpx nei profili qualificati, residenza GPU, prefetch e priorità dei tile visibili.
6. Misure cieche su RAW, CPU fallback e corpus colore/filtri/metadata sui due OS.

Uscita: matrice RAW esplicita, grandi immagini entro budget reali. Nessun demosaicing proprietario in v1.

## R4 — hardening e rilascio (12–18 settimane, dopo R3)

1. Fuzzing, sanitizers, fault injection, sandbox negativa completa, input ostili e recovery.
2. Benchmark p95 con almeno 100 prove, manifest hardware/corpus/cache e dispersione.
3. SBOM, notices e licenze sulle versioni effettivamente linkate; aggiornamenti dipendenze.
4. Installer Windows, bundle macOS, firma/notarizzazione, install/upgrade/rollback e smoke test puliti.
5. Test display reali, profili monitor, screen reader, IME, multimonitor e driver difettosi.
6. Manuale, privacy locale, matrice supporto, release candidate e bug triage; rilascio solo dopo gate.

La firma/distribuzione richiede credenziali e decisioni del titolare, non disponibili in questa sessione. Le prove Windows di sviluppo sono registrate; installazione pulita, firma e distribuzione restano da qualificare.

## Post-v1

Linux, Windows arm64, HDR/EDR, AVIF/JXL/EXR/PSD, Android, soft proof, conversione/export pixel, scrittura incorporata, confronto oltre due foto e demosaic proprio: ciascuno richiede ADR, prova di bisogno e gate specifico.

L'ampliamento di sviluppo fotografico richiesto il 23 settembre ha ora un [piano dedicato SF0–SF10](#piano-sviluppo-fotografico) e ADR 0011. La lista storica sopra non ne costituisce un divieto: distingue lo scope originario del viewer dalle estensioni richieste e qualificate separatamente, come già per export/FITS.

## Stime e aggiornamento

Il documento stima 56–84 settimane di milestone, circa 64–105 con riserva; sono ipotesi per una persona, non una data promessa. Prima revisione delle stime dopo le misure R0. Per ogni incremento aggiornare questo documento. Aggiungere rapporti per nuove campagne di verifica; sincronizzare le architetture soltanto se cambiano le fonti della specifica anteprime.

## Matrice dei requisiti e budget

### Confronto completo con la proposta v1.2

Stati: **parziale** = esiste codice utilizzabile ma non tutto il requisito; **da qualificare** = esistono prove limitate, insufficienti per la garanzia v1; **mancante** = la funzione richiesta non è implementata; **post-v1** = esclusa deliberatamente dalla prima versione. Questa matrice riguarda tutti i capitoli della proposta, senza attribuire una percentuale artificiale di completamento.

| Riferimento | Realizzato finora | Cosa manca rispetto al documento |
|---|---|---|
| §1 — prodotto, invarianti e decisioni | Nome TrueRenderer, nucleo Rust, separazione UI/motore, CPU lineare, dati durevoli separati, worker persistenti; ADR 0001–0009 | **Parziale.** Conferma toolkit sui due OS, TileProvider, Little CMS, requisito di release dei peer, canali, minimi OS/GPU, formato XMP e matrice RAW. Standard/Riferimento non disponibili; firma ad hoc non chiude le decisioni di rilascio. |
| §2.0 — bisogno e pilot | Corpus sintetico controllato, 30 NEF Nikon D750 e 22 D40 autorizzati usati nelle prove tecniche e workflow di selezione provabile localmente | **Parziale.** Almeno cinque interviste, ampliamento dei casi reali rappresentativi e pilot R2 con 8–12 fotografi; misura dell’uso settimanale per quattro settimane. Nessuna intervista o validazione commerciale è stata inventata. |
| §§2.1–2.4 — requisiti, formati, budget | UI reattiva con decodifica separata dai salvataggi; corpus più JPEG/PNG/TIFF/RAW su Windows LPAC e macOS XPC, con formati aggiuntivi nel bundle Mac storico | **Parziale/da qualificare.** Qualifica dei sottotipi bitmap, BigTIFF e fotocamere RAW; budget p95, griglia 100k, memoria globale e accessibilità. Il recupero XPC di circa 10 s **non soddisfa** il requisito < 1 s. Dettaglio budget sotto. |
| §3 — immagine reale e ispezione | Working Rec.2020 lineare fp32 premoltiplicato, RAW proprio senza clamp RGB intermedio, bilineare/AHD con raster intermedio intero 16 bit, badge Anteprima, informazioni decoder/digest, campionatore del livello residente dichiarato e istogramma composito sRGB | **Parziale.** Provenienza completa per ogni stadio/revisione, istogrammi e campioni selezionabili per stadio, ICC/monitor, clipping/gamut, cache promossa solo dopo verifica. Un test GPU aritmetico non certifica il display. |
| §4 — stack | Workspace Rust bloccato, egui/eframe e wgpu/WGSL, SQLite bundled, inventario dipendenze | **Da qualificare.** Gate egui di focus, IME, DnD, screen reader e 200% su Windows/macOS; qualifica dei backend adottati, Little CMS e XMP Toolkit; LibRaw scelta con qualifica RAW aperta. Gli spike non rendono definitivo tutto lo stack. |
| §§5.1–5.2, 5.6 — moduli e stato | Sei crate più desktop; reducer con comandi/effetti, identità UUID distinta da percorso/digest, revisioni e stato di salvataggio | **Parziale.** La mappa modulare completa non è realizzata: mancano provider tile, servizi ICC/metadata/RAW, cache tiled versionata v1 e journal completo degli effetti esterni. |
| §§5.3–5.5 — processi e apertura | Due servizi XPC separati, copia privata degli output, protocollo bounded, allowlist nel percorso pipe/corpus e decoder esterni in XPC macOS o LPAC/Job Object Windows, coda finita, generazioni, timeout/riciclo; SQLite in writer separato | **Parziale/da qualificare.** Bootstrap ostile, autorità residua e descrittori duplicati, mutazioni concorrenti della sorgente/spool coerente, quote end-to-end, peer di release e qualifica completa del confine Windows. Gli esterni sono ammessi come Anteprima nei confini di ADR 0004 e 0009. |
| §6 — pipeline bitmap | Corpus analitico invariato; ImageIO/ColorSync sul Mac storico; JPEG/PNG/TIFF portabili su Windows con regressioni gamma, colore dichiarato, alpha e orientamento | **Parziale.** Matrice completa di sottotipi/bit depth, JPEG CMYK/YCCK, BigTIFF, palette/gray/ICC, rifiuti ostili esaustivi e copertura multipiattaforma completa dell'alpha associata. Il percorso Apple non sostituisce la qualifica dei backend multipiattaforma previsti. |
| §7 — RAW | Baseline Apple CIRAWFilter; incremento pubblicato con selettore, LibRaw bilineare/AHD e TrueRenderer direzionale fp32, ricette e cache distinte; campagne D750/D40 sui tre motori Windows, Bayer sintetici e calibrazione/rifiuti verificati | **Parziale/sperimentale.** Nuovo bundle Mac, confronto Apple allineato, WB/matrici e ΔE00 misurati, matrice camere/CFA, rumore/artefatti e prestazioni da qualificare. Anticipato localmente il demosaic proprio di §7.3 su richiesta; nessun supporto universale o superiorità generale dichiarata. |
| §8 — colore end-to-end | Riferimento analitico sRGB ↔ Rec.2020, compute WGSL separabile/display SDR verificato contro CPU scalare e conversione esterna ColorSync; uscita sRGB8 dichiarata | **Mancante/da qualificare in R1.** Little CMS, ICC v2/v4 RGB/GRAY/CMYK, precedenza metadata, assegnazioni persistenti e bypass, libreria profili, monitor/intent/BPC, LUT adattive, ΔE00, display multipli. Soft proof §8.9 post-v1. |
| §9 — HDR/gain map/float | Nessuna implementazione | **Post-v1**, non requisito bloccante della v1 SDR. Serviranno contratti formato/display e nuove prove prima di abilitarli. |
| §10 — ricampionamento e geometria | Correzione 0.1.2: filtro lineare alla dimensione fisica, piramide CPU, 1:1 diretto, percorso condiviso griglia/inspector/viewer/confronto, filtro separato per alpha | **Parziale.** ADR 0003 registra il margine di banda aggiunto a Lanczos3. Mancano qualifica completa isotropia/zone plate, estensione del confronto GPU/CPU a tutti i grafi e transizioni LOD, geometria combinata con gli orientamenti, coordinate pan/zoom interamente f64, ispezione nearest esplicita agli ingrandimenti e comando 100% logico. Il limite di 8.388.608 pixel riguarda il raster di presentazione; le sorgenti ora arrivano a 67.108.864 pixel. |
| §11 — gigapixel/tile/cache | Piramide CPU per sorgenti entro 64 Mi pixel; cache lossless fp32 persistente per cartella, temporanei, quote, LRU/scadenza e impostazioni (0.1.4); livelli autonomi e cache v2 con writer asincrono (0.1.5) | **Mancante in R3.** TileProvider, TIFF nativo piramidale, provenienza SubIFD, cache tiled/spool, GC v1 e invalidazione per tile, generazione tiled cancellabile, prefetch/residenza GPU per tile, prova 4 Gpx. La piramide attuale in RAM non equivale al sistema gigapixel. |
| §12 — CPU/GPU e prestazioni | Livelli autonomi, budget globale di ammissione, pool Rayon/NEON, compute WGSL e texture dirette, fallback CPU; writer asincrono con precedenza ai lettori, I/O distinto, P0–P6/FIFO e prefetch direzionale/adattivo, pressione macOS; A/B scalare/parallelo/GPU del renderer | **Parziale.** Reset reali/OOM, blacklist/selezione GPU, LUT, telemetria completa, consumi, pressione fisica/altri OS e p95 evento→frame. Il frame della stessa revisione copre il raffinamento; la continuità va ancora qualificata su tutte le transizioni/display. |
| §§13.1–13.2 — dati e durabilità | `library.sqlite` separata da `index.sqlite`, writer unico, UUID/revisioni, conflitti, undo come revisione, backup consistente verificato, export JSON no-clobber; prove di ricostruzione indice | **Parziale.** Schema v1 completo, journal di intenti/esiti incerti, migrazioni successive, batch atomici e undo di batch, redo, retention/restore UI/export portabile v1 e recovery dopo power-fault/disco pieno. Le transazioni correnti sono per singola immagine. |
| §§13.3–13.6 — catalogo e metadata | Scansione cartella, rating/scarto/label/keyword piatte, ricerca e filtri locali, apertura file/cartella e metadati tecnici con provenienza decoder; monitor dei file richiesti/residenti con invalidazione e gestione di rimozione/ripristino | **Parziale; completamento in R2.** Watcher/scansione incrementale dell’intero catalogo, gestione dei volumi e riconciliazione delle rinomine/file mancanti, keyword gerarchiche, raccolte statiche/intelligenti, facce/FTS, EXIF normalizzato, XMP a tre vie/round-trip, conflitti e coordinamento filesystem. Nessuna scrittura su originali o sidecar oggi. |
| §§14.1–14.3 — browser desktop | Griglia virtualizzata con celle regolabili, preview laterale, metadati/annotazioni, ricerca, barra stato, selezione e filmstrip | **Parziale.** Breadcrumb/back-forward, albero filesystem e preferiti DnD, sezioni riordinabili, viste Dettaglio/Elenco con colonne, intero range celle 64–1024 e badge metadata/XMP, pannelli fotografici completi. |
| §14.4 — viewer e confronto | Adatta, 1:1 fisico, pan/zoom, schermo intero, due immagini con centro normalizzato/zoom condivisi e fit indipendente | **Parziale.** Loupe, tendina/dissolvenza con contratto geometrico, due campionatori e differenza del punto, carosello revisione, surround configurabile, filmstrip sui quattro lati. Confronto > 2 e registrazione automatica restano post-v1. |
| §§14.5–14.7 — comandi, layout, colore | Principali scorciatoie di selezione/valutazione/viewer e protezione dei campi testo; pannelli nascondibili | **Parziale.** Rimappatura tasti, spazi di lavoro salvati, overflow e comandi mancanti, finestra Colore, override ICC, avvisi gamut/clipping e controlli delle modalità qualificate. |
| §15 — Android | Nessun frontend mobile | **Post-v1.** Non conteggiato come funzione desktop mancante. |
| §16 — design e accessibilità | Tema scuro sobrio, gerarchia tipografica, focus visibile, feedback/errori e scorciatoie | **Da completare/qualificare.** Baseline VoiceOver/NVDA, sola tastiera su tutti i controlli, 200%, alto contrasto, IME e semantica accessibile delle immagini; tema/preferenze e tutti gli stati della specifica. |
| §17 — piattaforme | Baseline bundle arm64/macOS 26.6.2 con XPC; port Windows x86-64 pubblicato, provato sui D750/D40 | **Parziale.** Nuovo bundle con motori su Mac, minimi OS/GPU, display, installer/firma/notarizzazione da qualificare. Linux, Windows arm64 e Android non sono target v1. |
| §18 — sicurezza/robustezza | Allowlist non confinata, esterni XPC/App Sandbox sul Mac; worker Windows con LPAC senza capacità e Job Object; framing, copia privata, timeout e shutdown indipendente dai salvataggi | **Da qualificare.** Confine Windows completo, suite ostile completa/fuzz/sanitizer, policy OS release, output concorrente/handle residui, pressione ed errori filesystem, recovery e quarantena v1. Supervisione RSS ogni 25 ms ≠ tetto rigido di memoria. |
| §19 — test/corpus | 107 test Rust nell'ultima campagna Windows; baseline Mac con 71 test e prove IPC/XPC, diagnostica GPU, corpus numerico, smoke nativo con screenshot; campionamento, 19 fixture di formati esterni e 6 controlli aggiuntivi; 30 D750 e 22 D40 autorizzati nelle campagne distinte, prove native di invalidazione/recupero grafico | **Parziale.** Intere suite colore/ricampionamento, ampliamento del corpus fotografico autorizzato a camere/risoluzioni diverse e corpus ostile, orientamenti su altri formati/Siemens/tile, fault power/disk/device, benchmark ripetuti e CI su due OS. Uno screenshot a sua volta ridotto può introdurre moiré: servono anche confronti di pixel. |
| §20 — licenze e nome | Licenza proprietaria scelta dal titolare, repository pubblico, Cargo.lock, inventario Mac storico 212 package/330 notice, Windows 205 package/348 notice e manifest LibRaw 0.22.2 conservati, nome TrueRenderer | **Da qualificare prima di distribuire.** SBOM degli artefatti finali, obblighi delle librerie native effettive, codec/brevetti/canali e clearance del marchio **TrueRenderer**. Il cambio di nome nel progetto non costituisce una verifica commerciale del marchio. |
| §21 — build e distribuzione | Repository pubblico con README/licenza, toolchain locale, script, bundle interno firmato ad hoc, prove Finder/rilocazione e storico pacchetti | **Parziale.** CI macOS/Windows, build native e sanitizers di release, installer, firma di distribuzione/notarizzazione, install/upgrade/rollback, aggiornamenti e manuale/matrice pubblica. |
| §22 — roadmap | `STATO.md` contiene R0–R4, dipendenze e gate, registro per incremento | **R0 aperto.** Le UI/dati anticipano porzioni R1/R2 senza chiuderle. Le stime 56–84 settimane più riserva restano ipotesi della proposta, non date promesse né tempo già investito. R3/R4 ancora pianificati. |
| §23 — rischi | Registrati limiti osservati, confini del prototipo e decisioni ADR; esterni ammessi come Anteprima nei confini XPC macOS e LPAC Windows, con limiti espliciti | **Aperti.** Ampiezza dello scope, differenze display/driver/OS, input ostili, durabilità e prova del bisogno richiedono evidenze, non solo descrizioni. |
| §24 — appendici | Conservate matrice formati, glossario, fonti e revisione storica v1.2 | **Da aggiornare al rilascio.** La matrice richiesta non è una matrice di supporto attuale: il supporto corrente è descritto nel README e negli ADR 0004 e 0009, con copertura limitata alle prove elencate. I riferimenti tecnici sono requisiti/fonti, non prove di implementazione. |

### Budget della proposta: cosa è stato misurato e cosa no

- **Recupero worker < 1 s (§2.3): non soddisfatto.** La terminazione del decoder sospeso è rapida nelle prove, ma launchd può ritardare il nuovo servizio circa 10 s. Serve riprogettare/qualificare la strategia di riavvio oppure approvare una modifica esplicita del requisito; un timeout passato non basta.
- **Working set `min(2 GiB, 25% RAM)`: ammissione implementata, tetto effettivo da qualificare.** Budget condiviso, lease per campioni/scratch/upload e completamenti GPU, allowance base 384 MiB. I report di campionamento sommano RSS/footprint di host e XPC nei casi generati e sui 30 NEF autorizzati; l’ultima prova RAW riporta un picco di footprint di 2.018.970.672 byte (`reports/preview-navigation-memory-real-raw-macos.json`). Non contabilizzano separatamente tutti i driver né i picchi fra campioni. La riduzione dei limiti blocca nuove ammissioni mentre si liberano i lavori già in uso.
- **Pan/zoom < 16,7 ms e mai viewport vuoto: non ancora qualificato.** Il presenter conserva e riproietta il frame della stessa revisione durante il raffinamento; compute GPU usa texture dirette. Il confronto A/B dello stadio renderer esclude la superficie egui e non dimostra p95/p99 evento→frame o zero vuoti in tutte le sequenze.

- **Avvio, cache, griglia 100k/indice 200k, RAW, TIFF 2 Gpx e memoria a riposo:** i relativi p95/p99 e batch della tabella non sono stati misurati secondo le almeno 100 prove su due macchine. I tempi dei singoli smoke non li sostituiscono.
- **Errore GPU ΔE00/Little CMS:** non misurato. I confronti del filtro lineare e della codifica sRGB8 misurano grandezze differenti da ΔE00 e non qualificano ICC/display.
- **Durabilità crash/power-fault e sidecar:** revisioni, rollback e backup passano test funzionali, ma manca la suite fault-injected completa; nessun aggiornamento XMP è abilitato.
- **Isolamento e accessibilità:** prove macOS e Windows parziali; screen reader e scenari avversari completi restano necessari.


## Registro delle verifiche e degli incrementi

### Accorpamento documentale — 17 settembre 2026

Accorpati piano, avanzamento e ripresa in questo file; conservati caselle, gate, matrice, evidenze e limiti delle campagne. Aggiornati istruzioni e collegamenti; il sincronizzatore ora inserisce un rimando stabile allo stato invece del registro completo. Specifiche, ADR, rapporti e originale v1.2 conservati. Verifica documentale: collegamenti, conservazione dei blocchi esterni e idempotenza del sincronizzatore. Nessuna nuova prova applicativa; gate invariati.

### Quadro registrato il 16 settembre 2026

Incremento Windows/LPAC, LibRaw 0.22.2, motori selezionabili e correzioni pubblicati in `67146e3`. Ultima suite Windows: **99 test ordinari + 8 integrazioni**, build debug/release, 18 casi sintetici per worker e controlli IPC/Python/ricampionamento. [Audit e correzioni gamma/orientamento](STATO.md#revisioni-windows-concluse), [rapporto con hash](reports/gamma-orientation-fixes-windows.json). GUI e Nikon non ripetuti per questi ultimi fix.

La campagna fotografica Mac storica del 9 settembre comprende: 71 test Rust, 30 NEF nel carico a 2 GiB e bundle/XPC installato verificato. [Rapporto della baseline](reports/preview-navigation-continuation-macos.json). Il restyling del 15 settembre aggiunge una copia release separata e prove UI/corpus/XPC, senza attribuire a quel binario la campagna fotografica storica o una qualifica estesa dei nuovi motori RAW.



**Verificato:** conservati i confini effettivi di piattaforme, formati, motori RAW, isolamento, licenza proprietaria, dati durevoli e comandi di build. Screenshot e collegamenti locali controllati; il README non usa elenchi promozionali con trattini e separa la pagina di prodotto da piano, registro e rapporti.

**Aperto e mancante:** questo incremento è soltanto documentale e non modifica applicazione, bundle, database o originali. TrueRenderer resta dichiarato anteprima di sviluppo; colore end to end, ampiezza delle fotocamere, memoria sotto pressione, accessibilità, pacchetti di distribuzione e gate R0–R4 rimangono aperti nei documenti tecnici. Nessuna nuova prova applicativa o affermazione di accuratezza comparativa è stata aggiunta.

### Navigatore filesystem commutabile — pianificazione del 15 settembre 2026

**Documentato:** [progetto del navigatore](docs/progetto-navigatore-filesystem.md), richiesto dal titolare per un'implementazione successiva. Un unico spazio sinistro con schede **Libreria / Esplora**: il pannello attuale conserva corpus, selezione rapida e filtri; il nuovo aggiunge albero di file/cartelle, preferiti, recenti e navigazione. Lo switch conserva contesto fotografico e stato di ciascuna scheda. Specificati moduli, scansioni limitate indipendenti dal writer, identità delle richieste, salvataggi, migrazioni, casi filesystem, budget, fasi N0–N7 e matrice di accettazione.

**Verificato in questa sessione:** corrispondenza del piano con i punti d'ingresso del codice mediante lettura, collegamenti documentali e sincronizzazione. **Aperto e mancante:** intera implementazione e tutte le prove applicative/nativamente misurate del navigatore. Nessuna modifica a codice, bundle, database o originali; nessun gate R0–R4 chiuso e nessun cambiamento di priorità alle qualifiche già aperte.

### Immagini grandi, pressione e RAW Mac — verifiche del 15 settembre 2026

**Implementato:** harness opt-in su input esterni isolati, registrazione di livello residente/coordinate native/crediti, pressione della sola policy renderer e ritagli RAW lineari privati. La campagna iniziale ha scoperto un crash dei tre motori LibRaw nel probe XPC (`Thread stack size exceeded`): i quattro oggetti nativi del bridge sono ora sullo heap con proprietà RAII, senza cambiare ricette o isolamento.

**Verificato:** 24 processi e **240 azioni su PNG 12/24/45 MP**, Standard/Full e CPU/GPU con fallback dichiarati; copertura completa della traccia, 24 controlli 1:1 esatti. Le riaperture distinguono hit e nuovo decode dei Full non persistibili. Modifica, rimozione e ripristino di una sorgente da 45 MP passati con annotazione conservata. Due tracce di pressione renderer a 1536 MiB: riserva da uno a zero frame, copertura transitoria minima 3,14%, risultati finali corretti; picco footprint 1.594.870.688 byte, entro quota. A 512 MiB il RAW è rifiutato esplicitamente, originali invariati e zero crediti di lavoro dopo shutdown.

**RAW Mac:** 30 NEF D750 autorizzati × quattro motori, **120 sviluppi completi passati**, originali invariati. Altre 40 azioni nel viewer sui quattro motori con 1:1 esatto, cinque stadi del cambio motore nella UI e confronto pixel passati. Nove ritagli centrali dei primi tre NEF allineati; le differenze tonali includono ricette/esposizione/WB e non stabiliscono fedeltà assoluta. I 32 Bayer sintetici confermano 24 casi con MSE inferiore del direzionale, quattro rampe sostanzialmente equivalenti e quattro trame a crominanze indipendenti peggiori.

**Aperto:** nei quattro casi 45 MP Full, a 4 GiB configurati, la somma RSS supera quota: massimo **4.816.601.088 byte**, footprint **4.296.512.936 byte**. Crediti di ammissione entro quota; le somme possono contare pagine condivise più volte. Il superamento è conservato come evidenza negativa e il gate memoria fisica resta aperto. Non sono qualificati pressione fisica OS, decode RAW ridotto/regionale, driver, altre fotocamere, target colore/ΔE00/ICC o p95/p99. Standard riduce il livello residente dopo sviluppo completo; nessun gate R0–R4 chiuso.

Passati fmt/Clippy workspace, build release, **75 test Rust** (46 desktop, 24 worker, 5 renderer), regressione Python dell’allineamento, firma e XPC; nove test ignorati in questa corsa. Il test TIFF nativo passa fuori dal sandbox aggiuntivo del terminale. Aggiornato `dist/TrueRenderer-restyle.app`, precedente in `var/large-pressure-raw/before.app`. README e protocollo aggiornati; fotografie e ritagli restano privati. [Rapporto con hash](reports/large-pressure-raw-macos.json), [protocollo](STATO.md#campagne-raw-concluse).

### Prosecuzione — cache e verifica Mac, 15 settembre 2026

**Copertura Fit implementata e verificata:** il presenter conserva facoltativamente un frame più ampio della stessa vista, foto, digest, motore e dimensioni sorgente, usando quello con la maggiore sovrapposizione corretta durante il ricalcolo. Il caricamento Full da SSD può creare provider distinti ma compatibili: una prima implementazione troppo restrittiva è fallita su questo caso; la regressione ora lo include. Massimo due frame facoltativi entro un quarto dei budget globale/GPU e 64 frame totali; lease originali mantenuti fino al completamento. Pressione, quote ridotte, invalidazione, cambio compute e ammissione renderer/decoder possono scartare la riserva.

Release finale CPU/GPU e Standard/Full: **24 processi, 240 azioni, 1.553 ridisegni delle transizioni con copertura completa**, contro il minimo storico 40,5%. Pixel finali entro un livello sRGB8, **24 controlli 1:1 esatti**, zero fallback. Picco della riserva osservata: un frame, 38.263.752 byte di crediti globali e 12.754.584 GPU; sono contabilità di ammissione, non RSS. Passati fmt/Clippy workspace, build release, 46 test desktop e 5 renderer; sei integrazioni desktop non ripetute. Passate prove native di modifica sorgente, recupero grafico singolo e arresto alla seconda perdita, firma e XPC. Aggiornato `dist/TrueRenderer-restyle.app`, precedente in `var/fit-coverage/before.app`. [Rapporto e hash](reports/fit-coverage-macos.json).

La copertura misurata riguarda i draw della traccia sintetica, non tutti i refresh fisici. Se manca un frame ampio compatibile o viene espulso per quota, può restare parziale. RAW ridotti, pressione fisica, altri OS e p95/p99 restano aperti; nessun gate R0–R4 chiuso.

**Prima campagna delle transizioni (prima della correzione Fit):** il probe aggiunge zoom, pan, Fit, override Standard/Full e 1:1. Il presenter espone durante le sole catture diagnostiche la copertura dei comandi immagine: esatta, riproiettata o assente. Release CPU/GPU e Standard/Full: **24 processi, 240 azioni, 24 controlli 1:1 esatti**, pixel finali entro un livello sRGB8. Osservati **1.568 ridisegni** delle transizioni, di cui 1.007 riproiettati, senza draw completamente vuoti. Il ritorno dal ritaglio a Fit scende però al **40,5% di copertura** durante il ricalcolo: il resto attende contenuto corretto. L'incremento misura questo limite, senza modificare il comportamento di rendering né attribuire i draw a tutti i refresh del monitor. [Rapporto e campioni](reports/navigation-transitions-macos.json).

Passati fmt/Clippy workspace, build debug/release, 46 test desktop e 2 renderer; la nuova regressione distingue esatto, sovrapposizione parziale, assenza e lane di revisione diversa. Sei integrazioni desktop ignorate in questa corsa; XPC e firma verificati sul bundle diagnostico separato `var/navigation-transitions/TrueRenderer-release.app`. Il limite di copertura osservato ha motivato la correzione Fit descritta sopra. RAW ridotti, pressione, mouse/rotella OS, baseline e prestazioni statistiche restano da qualificare.

**Primo harness comando→superficie verificato:** aggiunto un probe opt-in al viewer di produzione e un orchestratore su corpus generato. Misura dal comando di selezione al raster esatto aggiunto a egui e alla ricezione del readback; il confronto numerico avviene dopo il timestamp finale. Configurazioni isolate, prefetch/ispettore/filmstrip esclusi e nessuna richiesta immagine prima del comando. CPU/GPU della stessa qualità, cache applicativa fredda, SSD dopo nuovo processo e ritorno RAM sono scenari distinti, confermati da contatori e residenza.

Release Standard e Full: tre ripetizioni per modalità, **24 processi e 72 selezioni** complessivi, tutti passati entro un livello sRGB8 con geometrie identiche fra CPU/GPU e nessun fallback. Passati fmt/Clippy workspace, build debug/release, 46 test desktop (6 integrazioni ignorate in questa corsa), controllo Python dei quantili/soppressione dei piccoli campioni e XPC del bundle diagnostico `var/navigation-probe/TrueRenderer-release.app`. [Rapporto e campioni](reports/navigation-surface-macos.json). I tempi includono readback e consegna dell'evento, con repaint del probe a 10 ms; non sono timestamp del monitor, p95/p99 o beneficio prestazionale qualificato. Restano scroll/zoom/cambio qualità, RAW/pressione, baseline e campagna statistica; l'app ordinaria non attiva il probe.

Riesaminati PLAN, ripresa, progetti anteprime/RAW/restyling e rispettivi gate dell'architettura. Corrette le indicazioni correnti che trattavano ancora il bundle Mac come rinviato: corpus, interfaccia e XPC sono stati verificati il 15 settembre; confronto fotografico Apple/LibRaw e qualifica colore restano aperti. Il prossimo incremento prestazionale resta l'harness evento→frame, dopo la verifica della base corrente.

Riprodotto il fallimento Unix della cache: la lettura consente hard link, ma veniva usata anche per autorizzare la rimozione di una voce invalida prima di riscriverla. La primitiva di rimozione ora riapre senza seguire link e rifiuta file con più collegamenti, sia per v1 sia per record v2 e temporanei. Letture validate e riuso v2 restano disponibili senza aggiornare timestamp condivisi. Due nuove regressioni e il test storico rafforzato verificano nomi, byte e mtime; 25 test cache passati. Allineata inoltre la compilazione del decoder bitmap portabile a Windows e test, conservando la copertura su Mac. Nessuna garanzia contro tutte le corse con processi esterni o qualifica Windows dedotta dai soli test Mac.

Verifica completa `scripts/verify.sh` passata fuori dal sandbox aggiuntivo del terminale: **106 test ordinari + 10 integrazioni**, fmt/Clippy workspace con warning negati, build debug, 2 test Python, 6 controlli IPC e 24 casi numerici con 1:1 esatto. Build release passata. Lo script ora esclude esplicitamente i due test fotografici privati quando manca `TR_RAW_SAMPLE`, evitando di contare come eseguite funzioni che ritornano subito. Generate 19 fixture locali; 17 confronti nativi CPU/esperimento Metal passati, senza abilitare Metal nel decode o qualificare prestazioni.

Aggiornato `dist/TrueRenderer-restyle.app`, copia precedente in `var/cache-integrity/before.app`. Firma ad hoc e XPC passati; cache nativa su tre sorgenti generate con 15 riusi bit-exact, zero nuovi decode a caldo, riapertura e pulizia passate. Otto schermate del viewer/griglia/confronto, 11 regioni entro un livello sRGB8, 77 canali differenti e 1:1 esatto. [Rapporto con hash](reports/cache-integrity-macos.json), evidenze locali in `var/cache-integrity/`. Restano warning del deployment target LibRaw, minimi OS, matrice fotografica e gate R0–R4; i fallimenti cache e warning Rust riportati nelle campagne storiche seguenti sono superati da questo incremento.

### Restyling desktop — 15 settembre 2026

README rivisto dopo la consegna: screenshot del viewer in apertura e schermate aggiornate di griglia e preferenze dalla stessa campagna del bundle finale; immagini del solo corpus generato. Riorganizzate le istruzioni su qualità globale/per foto, Settings, Esc, build e avvio della copia separata con il limite di una sola istanza per libreria. Collegamenti locali e immagini verificati; aggiornamento documentale, senza nuove prove applicative.

Ripresa della sessione completata: recuperate le evidenze della revisione della barra e corretto un conflitto di tastiera individuato nella prova diretta: Esc chiudeva il selettore ma riportava anche il viewer alla griglia. I popup ora hanno precedenza sulle scorciatoie fotografiche; regressione fallita prima della correzione e passata dopo, confermata anche nel bundle finale. Ripetuti fmt, Clippy desktop, build release workspace, sette test UI, suite desktop (42 passati, stesso fallimento cache, 6 ignorati), 12 schermate delle preferenze e 8 di rendering: 11 regioni entro 1 livello sRGB8, 77 canali differenti e 1:1 esatto. Firma e XPC passati sull'hash finale; completati rapporto e immagini referenziate dal README. Evidenze in `var/toolbar/reports/`, versione prima della correzione Esc recuperabile in `var/toolbar/before-escape-fix.app`. Copia aggiornata aperta sul viewer del corpus generato.

Seconda revisione richiesta dal titolare: Open e Settings coerenti e allineati, indicatore discreto del motore RAW applicato, strumenti viewer nella barra superiore e cartella/conteggi nella barra inferiore, senza duplicati centrali. Selettore globale distinto dal selettore Standard/Piena della foto corrente, con ritorno alla qualità globale e mantenimento della regola di azzeramento degli override al cambio globale. Riattivare Settings non scarta la bozza già aperta; il selettore globale non sovrascrive altre preferenze in bozza. Sette regressioni UI passate, inclusi clic su Open/Menu/Settings, layout bilingue a tre dimensioni e qualità bidirezionale; suite desktop a 42 passati, 1 fallimento cache già noto, 6 ignorati. Nuovi screenshot e controlli del bundle nel [rapporto della revisione](reports/toolbar-macos.json). Aggiornato lo stesso `dist/TrueRenderer-restyle.app`, con prima iterazione recuperabile in `var/restyle/first-iteration.app`; l'app storica `dist/TrueRenderer.app` resta invariata.

Implementati stile neutro centralizzato, barra Apri/viste/ricerca/menu, navigazione compatta, celle più leggere con nomi completi ed ellissi, ispettore a sezioni richiudibili e anteprima duplicata inizialmente chiusa nel viewer. Preferenze separate in Generale, Anteprime e RAW, Prestazioni, Cache e dati; corpo scorrevole e azioni di applicazione/chiusura fuori dallo scorrimento. Barra compatta e ispettore flottante nello spazio ridotto, senza sovrapporre l'ispettore alle preferenze; badge Anteprima permanente, stati vuoti con azioni, indicazione dell'override Piena per foto. Traduzioni e README aggiornati. [Composizioni e criteri](STATO.md#restyling-concluso).

Verificati fmt, Clippy desktop con warning negati, quattro regressioni UI (incluse bozze/selezione/zoom e quattro schede × due lingue × tre dimensioni), 4 test app/renderer, 6 integrazioni desktop con worker e 2 test Python. Suite desktop ordinaria: 39 passati, 6 ignorati, stesso fallimento cache Unix sugli hard link già riprodotto sul commit base nella campagna di localizzazione. Build release e firma ad hoc passate; prove native delle preferenze, campionamento e XPC registrate nel [rapporto con hash](reports/restyle-macos.json). Artefatti completi in `var/restyle/`; solo schermate del corpus generato pubblicabili nel README. Copia pronta in `dist/TrueRenderer-restyle.app`; bundle precedente, launcher, database e fotografie dell'utente non sostituiti.

Restano aperti il rilievo cache, warning worker Mac e warning di deployment target degli oggetti LibRaw (27.0 contro link 13.0), la matrice RAW Mac estesa, UI Windows, minimi OS, VoiceOver/NVDA e qualifica completa di accessibilità/display. Il test nativo 200% è un controllo del layout, non la chiusura di quei gate. Nessun gate R0–R4 chiuso, nessuna notarizzazione o garanzia di release attribuita al bundle.

### Interfaccia bilingue — incremento locale sul Mac

Implementati catalogo inglese/italiano del frontend, inglese predefinito anche per impostazioni precedenti senza lingua, menu Language/Lingua con applicazione immediata e persistenza. Il cambio salva soltanto la lingua e conserva selezione, zoom, generazione delle anteprime e preferenze ancora in bozza. Tradotti controlli, preferenze, guida, stato e descrizioni di provenienza; nomi file, percorsi, parole chiave e identificatori tecnici restano invariati. I dialoghi nativi seguono il sistema e dettagli diagnostici esterni possono mantenere la lingua originale. README aggiornato con istruzioni in entrambe le lingue.

Verificati build desktop/worker, fmt e Clippy desktop con warning negati, cinque nuove regressioni (default/migrazione, messaggi e percorsi, template, rendering bilingue, cambio lingua/persistenza/fallimento di scrittura). Suite desktop: 39 passati, 6 ignorati e un fallimento del test Unix sugli hard link della cache, riprodotto anche sul commit base `666f650` in una copia separata. Smoke grafico delle preferenze passato in inglese e italiano su corpus/dati temporanei; schermate controllate e conservate localmente in `var/localization/`, escluse da Git. [Rapporto e hash](reports/localization-macos.json). Corretto `sync-docs.py` per Python 3.9 del Mac, preservando LF e blocchi esterni. Restano aperti il rilievo cache, la prova UI Windows e la qualifica del nuovo bundle/XPC; il bundle esistente non è sostituito. Nessun gate R0–R4 chiuso.

### Implementato e verificato nelle campagne Windows

- LPAC senza capacità e Job Object; controlli filesystem e limiti descritti in [ADR 0009](docs/isolamento-decoder-e-formati.md#adr-0009).
- Bilineare, AHD e TrueRenderer fp32; campagne D750/D40, selezione durante scansione e cache per ricetta. [Progetto RAW](docs/progetto-motori-raw.md), [D40](STATO.md#campagne-raw-concluse).
- Correzioni iniziali di parsing, quote, bitmap, sorgenti, dipendenze e runtime: [audit prima di main](STATO.md#revisioni-windows-concluse).
- Selezione/zoom con Applica e salva, PNG cICP e alpha TIFF: [revisione serale e correzioni](STATO.md#revisioni-windows-concluse).
- Tag TIFF non supportati, fallback RAW coerente e timestamp/hard link cache: [ricontrollo e correzioni](STATO.md#revisioni-windows-concluse).
- Gamma PNG distinta da sRGB e orientamento per IFD: [ultima revisione](STATO.md#revisioni-windows-concluse); ricetta cache `bitmap-gamma-ifd-v5`.

I conteggi e gli hash di ogni campagna restano nei [rapporti di verifica](STATO.md#registro-delle-verifiche-e-degli-incrementi). Le evidenze negative sono conservate insieme alle correzioni, senza attribuire vecchie misure ai nuovi binari.
