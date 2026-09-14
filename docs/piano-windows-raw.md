# Piano — aprire i RAW su Windows

**Correzioni successive all'audit verificate:** il confine corrente usa LPAC senza capacità e Job Object, descritto in [ADR 0009](adr/0009-isolamento-worker-windows.md); LibRaw è aggiornata a 0.22.2. PNG/TIFF, bounds preview, quota, hard link e osservazione sorgente sono corretti; passati 89 test Rust, 156 sviluppi release e GUI, con [esito nel registro](avanzamento.md). Il titolare ha rinviato la prova nativa Mac. Le misure e le aperture sotto descrivono le versioni precedenti.

**Audit del 13 settembre 2026:** questo piano conserva lo storico del port. La successiva [revisione prima di main](revisione-pre-main.md) ha dimostrato che token ristretto/Job Object consentono ancora accesso a file non concessi; il test negativo filesystem previsto al §4 non è soddisfatto. Non interpretare «isolamento eseguito» come qualifica del gate esterno. Sono inoltre aperti difetti di colore PNG, classificazione TIFF, robustezza preview, quota e cache; il nuovo bundle Mac non è verificato. Le prove positive Nikon restano funzionali e locali.

Data: 10 settembre 2026. Stato: fasi 1, 2 e 3 eseguite, più la cache su disco. Nessun gate R0–R4 viene chiuso da questo documento: aprire un file non è qualificarlo.

Obiettivo di riferimento: i 30 NEF Nikon D750 già verificati su macOS devono aprirsi su Windows x86-64 con lo standard di fedeltà del progetto, cioè colore e dettaglio riferiti al file originale e mai a un'assunzione taciuta.

## 1. Stato misurato su Windows

Misure eseguite il 10 settembre 2026 su questa macchina, non dedotte.

| Elemento | Esito |
|---|---|
| Compilazione del workspace | passa dopo la correzione di `sha2` di [ADR 0007](adr/0007-porta-decoder-e-build-windows.md) |
| Avvio, finestra e disegno | passa: 117 fotogrammi, 12 immagini, zero errori di presentazione |
| Adattatore grafico | NVIDIA GeForce RTX 3060 su Vulkan |
| Confronto GPU/CPU | errore massimo 6,10e-5 |
| Verifica ricampionamento | passa: 24 segnali, corrispondenza esatta a 1:1 |
| Suite Rust di `tr-core`, `tr-app`, `tr-platform`, `tr-render`, `tr-store`, `tr-worker` | verdi |
| Verifica cache su disco | fallisce: `cfg(not(unix))` |
| Verifica RAW reali | rifiutata: richiede il bundle XPC sandboxed |

Il motore quindi rende correttamente su Windows. Non rende i file dell'utente.

## 2. Le barriere, misurate

**Nessun decoder esterno.** `external_decoder` restituisce `None` fuori da macOS. Anche concedendo la fiducia, non c'è niente da invocare.

**Fiducia esterna mai concessa.** `serve` gira con `external = false`, quindi il cancello del corpus rifiuta qualunque sorgente il cui digest non sia nel manifest compilato. Su macOS l'unico punto che concede `Trust::External` è l'ingresso XPC. Su Windows non esiste un equivalente, e PLAN.md §158 è esplicito: «Il solo processo separato **non** soddisfa questa voce».

**Cache su disco assente, non bloccante.** `Directory::open` è chiusa da `cfg(not(unix))` perché usa descrittori di directory con `O_DIRECTORY` e `O_NOFOLLOW`. L'applicazione degrada: lo smoke test è passato con la cache non disponibile. Sei test di `apps/desktop` restano rossi, ma la visualizzazione non dipende da questo.

**Segnale di pressione memoria assente.** `memory_pressure` restituisce `None` fuori da macOS. Il budget di ammissione resta valido, perché è un sistema di crediti applicativo e indipendente dalla piattaforma; manca la notifica del sistema che oggi riduce le ammissioni speculative sotto pressione. È una perdita della risposta adattiva, non del limite.

Il carico è dimensionabile. Un D750 produce un'area immagine di 6016 per 4016 pixel, cioè 24,16 Mpixel; a fp32 RGBA sono circa 369 MiB per fotogramma pieno. La prova macOS usava un budget di ammissione di 2 GiB per 30 file, inclusi due accessi simultanei.

## 3. Contenuto reale dei file

Ispezione diretta di `nikon_d750_01.nef`, utile perché decide la fase 2.

| SubIFD | Contenuto |
|---|---|
| 0 | anteprima JPEG 6016 x 4016, circa 1,9 MiB |
| 1 | mosaico RAW 6032 x 4032, 14 bit, compressione Nikon 34713 |
| 2 | anteprima JPEG 1620 x 1080 |

IFD0 dichiara 160 per 120, cioè la miniatura: è il caso dei NEF in contenitore TIFF che [ADR 0004](adr/0004-formati-esterni-e-pubblicazione.md) dice di aver corretto nel riconoscimento. L'anteprima grande è a piena risoluzione, il che rende la fase 2 sensata invece che un ripiego a bassa qualità.

## 4. Fase 1 — isolamento del worker su Windows

È la fase che sblocca tutto, perché senza di essa `Trust::External` non può essere concesso senza contraddire un requisito scritto.

Meccanismi candidati elencati nell'architettura §5.5: AppContainer o LPAC dove praticabile, token ristretto, Job Object con limiti, con test obbligatorio del passaggio degli handle.

Ordine consigliato: **token ristretto più Job Object prima di AppContainer**. È uno dei candidati nominati, costa una frazione, e dà limiti veri subito. La sostituzione riguarda solo il ramo `else` di `Broker::spawn` in `tr-platform`, che oggi avvia il figlio con `Command::new` più `env_clear`.

Un dettaglio che vale la pena notare: un limite di memoria su Job Object è imposto dal kernel. Il README dichiara che oggi «Worker memory is supervised rather than hard-capped». Su Windows questa fase darebbe quindi una garanzia che macOS non ha, e compenserebbe in parte il segnale di pressione mancante.

Stato finale della fase: il worker gira con token ristretto in un Job Object con tetto di memoria e di processi; un test negativo dimostra che non può aprire un file fuori dagli handle concessi; solo allora un ingresso Windows equivalente a `tr_worker_serve_fds` concede `Trust::External`. Restano fuori AppContainer e LPAC, da valutare dopo una misura.

### 4.1 Incremento 1a — Job Object: eseguito

Data: 10 settembre 2026. `crates/tr-platform/src/isolation.rs`.

`Isolation` è un valore RAII che possiede un job anonimo, quindi non apribile per nome da nient'altro sulla macchina. I limiti richiesti al kernel sono il tetto di memoria committed per processo e per job, un solo processo attivo, terminazione alla chiusura del job e terminazione su eccezione non gestita. Il tetto è lo stesso che il broker già applicava contabilmente, 384 MiB per il corpus e fino a 2 GiB per gli esterni, ora chiesto anche al sistema.

L'innesto sta nel ramo a pipe di `Broker::spawn`. Il job viene creato prima del figlio, così un fallimento non avvia nulla; se l'adozione fallisce il figlio viene terminato e l'avvio annullato, perché un worker non confinato non deve restare vivo. Fuori da Windows il tipo esiste e non concede niente: `kernel_enforced` risponde con la verità e `Broker::transport` distingue «Processo su pipe in Job Object» da «Processo su pipe», così l'etichetta di isolamento nel pannello non promette più di quello che c'è.

Due prove, entrambe verdi su questa macchina. La prima richiede che il confinamento sia dichiarato solo dove il kernel lo applica. La seconda dimostra che i limiti sono del kernel e non contabili: un secondo processo viene rifiutato perché il job limita i processi attivi, e chiudere il job termina quello confinato. Le verifiche end-to-end restano verdi con il worker confinato: ricampionamento con 24 segnali e corrispondenza esatta a 1:1, smoke con 152 fotogrammi, 12 immagini e zero errori.

Il confine noto e dichiarato è una finestra di corsa: il figlio viene creato e poi adottato, quindi esiste un intervallo in cui gira senza limiti. È stretto, perché il worker si blocca sulla prima lettura dalla pipe, ma è reale. Si chiude creando il processo direttamente dentro il job, cosa che arriva con l'incremento 1b insieme al token ristretto.

`windows-sys` 0.61.2 diventa dipendenza diretta di `tr-platform`, solo su target Windows. Era già nel grafo tramite wgpu e winit alla stessa versione, quindi la supply chain non cambia.

### 4.2 Incremento 1b — creazione confinata e token ristretto: eseguito

Data: 10 settembre 2026.

Il worker non viene più avviato con `std::process::Command` su Windows. `Isolation::spawn` lo crea con `CreateProcessAsUser`, e le tre proprietà che contano arrivano nella stessa chiamata.

**Dentro il job dalla prima istruzione.** `PROC_THREAD_ATTRIBUTE_JOB_LIST` colloca il processo nel job alla creazione. La finestra di corsa dichiarata in 1a è chiusa: non esiste più un intervallo in cui il worker gira senza limiti. Un processo creato così non può nemmeno uscire dal job.

**Solo le sue tre pipe.** `PROC_THREAD_ATTRIBUTE_HANDLE_LIST` enumera esattamente gli handle ereditabili. È più forte dell'omissione: prima l'ereditarietà era limitata dal fatto che nessun altro handle era marcato, ora è il kernel a rifiutare qualunque cosa non sia in lista. Le pipe sono create in coppia e l'estremità che resta al broker viene esplicitamente resa non ereditabile, così il worker non riceve mai un handle al lato che non gli spetta. Lo standard error va su `NUL`.

**Token ristretto.** `CreateRestrictedToken` con `DISABLE_MAX_PRIVILEGE`, che lascia cadere ogni privilegio, e il gruppo Administrators locale marcato deny-only. Essendo una restrizione del token del chiamante, `CreateProcessAsUser` lo accetta senza i privilegi che quella chiamata normalmente richiede. L'ambiente è vuoto come prima.

Restano fuori di proposito i restricting SID, il livello di integrità basso e AppContainer. Cambiano ciò che il worker può caricare e ognuno richiede la propria misura prima di essere aggiunto; dichiararli senza averli provati sarebbe peggio che non averli.

Le prove. `Isolation::confines` chiede al kernel, con `IsProcessInJob`, se il processo è dentro quel job, invece di dedurlo dall'aver chiamato la funzione giusta. Il test verifica che un worker appena creato risulti confinato e che un job diverso non lo rivendichi. I due test che avviano il worker reale, decodifica di byte catturati senza riaprire la sorgente e riciclo dopo un crash, passano con il worker sotto token ristretto. Le verifiche end-to-end restano verdi: ricampionamento con corrispondenza esatta a 1:1, smoke con 141 fotogrammi e zero errori.

Un dettaglio costato un errore, utile a chi tocca questo codice: `UpdateProcThreadAttribute` conserva i valori per puntatore, non per copia. Un array locale passato lì muore prima di `CreateProcess` e il risultato è un `ERROR_INVALID_HANDLE` che non indica affatto un handle sbagliato. Job e lista di handle vivono ora nella struttura che possiede la lista.

### 4.3 Da qui a `Trust::External`

L'isolamento c'è, quindi la barriera tecnica non è più il motivo per cui la fiducia esterna non viene concessa. Resta la decisione del §8.2: su macOS quella fiducia sta dietro un servizio XPC firmato con App Sandbox, e token ristretto più Job Object è più debole. Va scritto in un ADR e accettato esplicitamente, non dedotto dal fatto che il codice ora esista.

## 5. Fase 2 — prima resa visibile, senza LibRaw

L'architettura §7 prescrive il **rendering a due stadi**: alla selezione si mostra l'anteprima incorporata se valida, e quando la selezione resta stabile parte il render baseline. La fase 2 realizza il primo stadio soltanto.

Va conciliata con un divieto altrettanto esplicito. ADR 0004 dice «Nessun fallback alla preview JPEG» e il README dice che un'anteprima incorporata non viene mai sostituita silenziosamente ai dati RAW. La conciliazione sta nell'avverbio: l'anteprima è ammessa come stadio dichiarato, mai come sostituzione taciuta. La provenienza deve dire che si tratta dell'anteprima incorporata e non dello sviluppo del RAW, e nessuna affermazione di fedeltà al sensore va fatta.

Qui il contratto di colore di ADR 0007 lavora già: l'anteprima è un JPEG con il proprio spazio, e `ColorSource` obbliga a dichiarare se quello spazio è stato letto o assunto.

Il decoder JPEG del crate `image` è già in dipendenza. Serve l'estrazione dell'anteprima dal contenitore TIFF, che è lettura di IFD e non decodifica del mosaico.

Stato finale della fase: i 30 file compaiono su Windows a piena risoluzione, orientati correttamente, con provenienza che dichiara l'anteprima incorporata. Non è sviluppo RAW e il documento non deve lasciar credere il contrario.

## 6. Fase 3 — LibRaw, lo sviluppo RAW vero

È la fase che il documento assegna a LibRaw come baseline v1, con demosaicing proprio rimandato al post-v1.

Contratto v1 già scritto in §7: configurazione immutabile e versionata; il primo contratto da provare è l'output LibRaw lineare a 16 bit in uno spazio RGB **nominato**, con camera-space come alternativa solo se il primo fallisce un requisito misurato. La ricetta fissa almeno gamma lineare, `output_bps`, `output_color`, bilanciamento del bianco e fallback, `no_auto_bright`, `highlight`, `user_qual`, scala ed esposizione, e la gestione dell'orientamento. Auto-bright, gamma di visualizzazione e curve creative sono disattivati. Se l'API non consente di verificare il confine, quel modello non riceve il badge `RENDER LIBRAW`.

I binding devono usare `LibRaw_datastream` costruito sull'handle concesso, così la libreria non riapre un nome di file. È un requisito di sicurezza, non di stile, e si combina con la fase 1.

### 6.1 Eseguita

Data: 10 settembre 2026. La decisione di licenza è in [ADR 0008](adr/0008-libraw-licenza-e-collegamento.md): CDDL-1.0, collegamento statico, shim C invece di bindgen. LibRaw 0.21.1 è vendorizzato non modificato, la ricetta `TR-libraw-linear-v1` vive in `native/libraw/tr_libraw.cpp`, e la build richiede il solo MSVC.

I 30 NEF Nikon D750 vengono sviluppati dal mosaico, trenta su trenta, senza ricadute sull'anteprima. Windows e macOS ora sviluppano entrambi il sensore; restano decoder diversi e quindi rese diverse, come §7 di questo piano già avvertiva.

L'anteprima incorporata della fase 2 non è stata buttata: resta il ripiego dichiarato per una fotocamera che LibRaw non sviluppa, con il motivo del rifiuto scritto nella provenienza.

### 6.2 Il contesto originale, conservato

Questa fase era **bloccata da una decisione formale di licenza**, non da lavoro tecnico. `docs/licenza-libraw.md` dice che TrueRenderer 0.1.5 non include né collega LibRaw, e che prima di includerlo vanno bloccate versione e opzioni, verificati i termini dei componenti realmente compilati, scelta formalmente l'opzione fra CDDL 1.0 e LGPL 2.1 e preparati sorgenti e notice.

## 7. Confronto con macOS: cosa si può misurare e cosa no

Serve dirlo prima di scrivere i test, perché è il punto in cui è facile promettere troppo.

LibRaw e CIRAWFilter **non** produrranno lo stesso risultato. ADR 0004 lo dice già: «non si promette una ricetta identica a LibRaw o ad altri sviluppatori». La resa macOS non è quindi un riferimento bit-per-bit per quella Windows.

Restano confrontabili senza ambiguità: dimensioni native, orientamento applicato una volta, assenza di clipping nello spazio di lavoro, profondità dichiarata e mai inventata, e la stabilità della ricetta fra esecuzioni.

Non è confrontabile per identità il colore. Va definita una tolleranza colorimetrica fra decoder, dichiarata come tale. La soglia ΔE00 di §14, media sotto 0,5 e massimo sotto 1,0, riguarda il percorso GPU contro la stessa trasformata Little CMS: è una misura interna a una pipeline e non va presa in prestito per un confronto fra due sviluppatori RAW diversi.

## 8. Decisioni del titolare

Tre, e nessuna è tecnica.

1. **Licenza LibRaw**: CDDL 1.0 oppure LGPL 2.1. Blocca la fase 3 e va presa prima di scrivere codice che linka la libreria.
2. **Livello di isolamento accettabile su Windows** per concedere `Trust::External`. Su macOS è un servizio XPC firmato con App Sandbox; token ristretto più Job Object è più debole. Se si accetta per una build di sviluppo, va scritto in un ADR e non lasciato implicito.
3. **Ammissibilità dell'anteprima dichiarata** della fase 2, dato quanto è netto il divieto di ADR 0004. La lettura proposta qui è che il divieto colpisca la sostituzione taciuta e non lo stadio dichiarato, ma la scelta resta del titolare.

## 8.1 Cache su disco su Windows: eseguita

Data: 10 settembre 2026. Era elencata come fuori piano; è stata fatta perché era la causa unica dei sette test rossi, dei tre avvisi del compilatore e dell'esclusione dei test di `decode_pool` dalla piattaforma.

Unix fissa la cartella con un descrittore e lavora con le chiamate `*at`, così un antenato rinominato a metà operazione non può dirottare una scrittura. Windows non ha un equivalente documentato senza `NtCreateFile`. La sostituzione tiene aperto l'handle della cartella **senza condivisione di cancellazione**: finché quel valore vive, nessuno può rinominare o eliminare la cartella cache, e ogni figlio si raggiunge per percorso a partire da lì. Gli antenati sopra la radice della cache non sono fissati: è l'unica garanzia che questa piattaforma non riproduce, ed è scritta invece che sottintesa.

Le proprietà che si trasferiscono: rifiuto di un punto di reparse sull'ultimo componente, tramite `FILE_FLAG_OPEN_REPARSE_POINT` più controllo dell'attributo; rifiuto degli hard link, con il conteggio letto da `GetFileInformationByHandle` perché `std` lo espone solo su nightly; rifiuto dei file speciali; pubblicazione senza sovrascrivere, con `MoveFileEx` privo di `MOVEFILE_REPLACE_EXISTING`; validazione dei nomi estesa a `:`, che su Windows indirizzerebbe un flusso alternativo.

Sui payload la condivisione include la cancellazione. È deliberato: consente di pubblicare o raccogliere una voce mentre un lettore la tiene aperta, e Windows mantiene il file vivo per quell'handle fino all'ultima chiusura. È il comportamento di unlink-while-open che il design della cache già assume.

Due difetti trovati per strada, entrambi invisibili finché la cache non ha funzionato.

Il primo: `fs2` segnala la contesa del lock con l'errore nativo della piattaforma. Unix restituisce `EWOULDBLOCK`, che `std` classifica; Windows restituisce `ERROR_LOCK_VIOLATION`, che `std` lascia non categorizzato. Il chiamante che confrontava con `WouldBlock` leggeva quindi una cartella occupata come un errore definitivo invece che come una condizione ritentabile. La normalizzazione ora avviene in un punto solo.

Il secondo è più serio e riguarda la fedeltà. Il token di osservazione su Windows conteneva solo lunghezza e data di modifica. Un file sostituito con uno di pari dimensione e data ripristinata non veniva rilevato, quindi l'applicazione avrebbe mostrato una resa vecchia di un file cambiato. Ora il token include l'identità del file, numero di serie del volume e indice, che sono la controparte Windows di dispositivo e inode. Non essendo leggibili da `Metadata`, la firma di `observation_token` prende anche il percorso e apre brevemente un handle; un percorso non apribile degrada a dimensione e data invece di fallire, perché resta un'osservazione e non una garanzia.

Prove: le suite dell'intero workspace sono verdi, 17 test su `apps/desktop` contro i 5 di prima. La cache è stata esercitata davvero, non solo nei test: una prima esecuzione scrive 88 voci per 14 MB accanto al corpus, una seconda non ne riscrive nessuna. Restano Unix tre test che pilotano il worker attraverso uno script `/bin/sh`, ora marcati singolarmente invece di escludere l'intero modulo. `--verify-cache` resta non eseguibile qui, ma per mancanza delle fixture generate su macOS e del decoder esterno, non per la cache.

## 9. Cosa non è in questo piano

Little CMS e la trasformata ICC di ingresso, che è ciò che scioglie il rifiuto oggi applicato ai file con `cHRM` o profilo incorporato. Qualifica della matrice camere e badge `RENDER LIBRAW`, collocati a inizio R3. AppContainer e LPAC. Percorsi wide-character e manifest `longPathAware` di §17.1, necessari prima di dichiarare la piattaforma e non prima di aprire un file. Installer, firma e canale di distribuzione.

## 10. Estensione richiesta il 12 settembre — motori a scelta

Il port sopra è conservato come base locale, non committata. Aggiunti selettore persistente, LibRaw AHD e TrueRenderer direzionale fp32 sperimentale, con cache/provenienza distinte e scelta applicata senza perdere selezione/annotazioni. Il bilineare storico resta il default Windows. Tutti e tre sviluppano i 30 NEF D750 autorizzati: 90 prove complete passate, originali invariati. Il nuovo fp32 conserva headroom e valori negativi ma non è universalmente migliore: perde sul sottoinsieme sintetico a crominanze indipendenti. Il motore proprio rifiuta altre camere e WB non valido.

La configurazione di build include gli stessi motori anche sul Mac, oltre ad Apple; nuovo bundle e XPC ancora da verificare nativamente. Stato, limiti del colore/display, prove e riproduzione in [progetto motori RAW](progetto-motori-raw.md). Nessun cambiamento allo stato aperto dei gate o agli obblighi di distribuzione.
