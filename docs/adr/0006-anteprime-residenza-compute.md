# ADR 0006 — anteprime autonome, ammissione e compute

Data: 8 settembre 2026. Stato: implementazione 0.1.5; qualifica integrata del progetto ancora aperta.

## Decisione

Estendere ADR 0003–0005 con livelli lineari autonomi. La qualità dell'anteprima (`Standard`/`Full`) resta separata dall'assurance `Preview`; nessun badge della futura pipeline Standard/Riferimento viene abilitato. Nuove configurazioni partono da Standard; la migrazione riconosce impostazioni o libreria preesistenti prima di crearle e conserva Full e i limiti disco precedenti. Un file impostazioni danneggiato viene conservato e il recupero è segnalato.

Standard conserva un derivato fino a 2048 pixel di lato. Full conserva il sottoinsieme del grafo sufficiente alla vista fisica; 1:1 richiede LOD 0 e un override per foto. Il provider dichiara sviluppo full-frame e assenza di decode regionale/ridotto RAW qualificato. Lo sviluppo completo temporaneo seguito dal grafo di riduzione è il fallback previsto dal progetto; nessun JPEG incorporato viene sostituito allo sviluppo.

`ImageLevels` possiede una coda di livelli senza riferimento alla sorgente più grande. Richieste compatibili contemporanee della stessa revisione condividono lo sviluppo; i derivati piccoli vengono staccati e le allocazioni uguali condivise. Coefficienti, coordinate f64, ordine degli accumuli e policy alpha restano quelli di ADR 0003. Rayon 1.11.0 fornisce un pool applicativo comune; il percorso arm64 usa NEON f64 senza FMA. Il limite sui thread applicativi non limita internamente i codec di sistema.

## Memoria e code

Un budget condiviso ammette snapshot, letture, raster, scratch e presentazione prima delle allocazioni pesanti. Include un'allowance iniziale di 384 MiB per app, contesti persistenti e infrastruttura device, oltre agli incrementi dei job. È una stima da qualificare e non un tetto imposto dal kernel. Gli snapshot hanno anche un sottolimite di un terzo del budget; un writer opzionale trattiene al massimo 64 MiB di livelli e prenota scratch separatamente. Gli Arc dei campioni possiedono il lease; spostare il risultato fra code non duplica i crediti globali.

La UI pubblica la domanda effettivamente disegnata; le classi P0–P6 mantengono ordine FIFO e promozioni anche durante un lookup. Una coda piena può espellere lavori secondari per quelli visibili. Hash/cache e due decoder sono code separate. Un solo decoder può occuparsi di lavoro speculativo lungo; il secondo dà precedenza al visibile. Cambiare cartella revoca il dominio, cambiare vista elimina consumatori obsoleti senza riciclare XPC a ogni scroll. Il backend nativo già in corso può dover terminare. Il prefetch segue la direzione di navigazione e considera solo i vicini della vista, anche dopo un salto lontano. Attende una stabilità fra 100 e 500 ms stimata dalle recenti latenze di consegna degli artefatti (400 ms iniziali); questa euristica non è una misura evento→frame. Pausa e ricostruzione esplicita non bloccano le annotazioni.

Su macOS una dispatch source osserva MEMORYPRESSURE. In Warn/Critical si sospendono le nuove ammissioni speculative e le scritture opzionali, si riducono a due i thread applicativi effettivi e si espellono RAM/GPU riutilizzabili. Preferenze e crediti dei buffer in uso rimangono validi. Le prove coprono la policy con pressione iniettata e la disponibilità dell'adattatore nativo; non equivalgono a una prova di pressione fisica dell'intera macchina. Sugli altri OS l'adattatore non è ancora implementato.

La riduzione dei limiti si applica alle nuove ammissioni, mentre i buffer già utilizzati dalla GPU mantengono i crediti fino al completamento. La UI distingue una riduzione in corso e segnala un'operazione che non entra in memoria. Il sistema non promette un picco RSS istantaneo uguale al valore del controllo.

## Persistenza

Stessa directory `.truerenderer-cache`, marker, lock, nomi gestiti e quota di ADR 0005. I record v2 hanno SHA-256, lunghezze esatte e payload fp32 lossless massimo di 4 MiB. Il descrittore viene pubblicato dopo i blocchi. Scritture e temporanei restano protetti dal lock esclusivo; la preparazione dei record è esterna al lock, acquisito per gruppi limitati a circa 4 MiB. Quote e presenza dei blocchi precedenti sono ricontrollate a ogni acquisizione; non si riusa una scansione dopo aver rilasciato il lock. Writer e GC cedono precedenza ai lettori locali in attesa con attesa limitata e cancellabile. Una seconda istanza può espellere un blocco prima della pubblicazione: il writer abbandona l'operazione opzionale, senza dichiararla completa.

Il lettore verifica prima lo snapshot sorgente completo e poi i byte copiati dei record. `Busy`, `Missing`, `Invalid`, `Disabled` e limite memoria restano distinti; la contesa ha retry limitato a 200 ms. La scansione delle quote legge i metadati rispetto al descrittore della directory senza seguire link, evitando aperture dei payload; la pubblicazione aggiorna la scansione già protetta dallo stesso lock, senza ripeterla per le sole statistiche. V1 viene riusata solo con fingerprint esatto e prenotazione per la lettura completa. Non è prevista una conversione massiva delle cache di altre versioni. Il GC comprende v1/v2, orfani e temporanei e protegge il minimo recuperabile per miniature; il vecchio GC mantiene comunque sicurezza e quota, non la nuova preferenza di residenza.

## Compute e decoder

Il viewer usa coefficienti canonici CPU, due pass compute separabili e conversione/composizione SDR in WGSL. Pipeline e input compatibili sono riutilizzati; la texture finale viene registrata direttamente nel renderer egui, senza readback per frame. Input e scratch fuori capability o quota producono fallback CPU registrato. Automatico usa la GPU verificata per richieste sufficientemente grandi; il confronto numerico del renderer avviene prima dell'abilitazione. I report di performance distinguono questa elaborazione dal tempo evento-utente→presentazione.

Core Image riutilizza il contesto richiesto software con intermedi disabilitati. È disponibile un esperimento separato con contesto richiesto Metal, ma non è abilitato per il decode produttivo: i casi sintetici conformi non provano backend effettivo, beneficio end-to-end e matrice reale delle fotocamere. La presentazione può usare compute GPU mentre il decode resta CPU. Il riciclo e l'isolamento XPC continuano a seguire ADR 0002/0004.

La preparazione e l'encoding compute avvengono sul worker, ma i submit alla coda condivisa avvengono esclusivamente sul thread UI. Un submit concorrente a `Surface::configure` causava un errore di validazione wgpu riprodotto durante gli avvii nativi. La correzione serializza anche il controllo iniziale GPU prima del loop della finestra. La cancellazione prima del submit libera subito il lavoro mai inviato; dopo il submit i lease rimangono fino alla callback di completamento. Lo shutdown attende la fine dell'encoder e il completamento GPU con timeout. La suite lifecycle ripete avvii e transizioni senza ritentare i fallimenti.

## Qualifica aperta

I report sotto `reports/preview-*` descrivono prove realmente eseguite, con soglie e scope. Restano il corpus autorizzato di 1000 RAW reali e sottoinsiemi 12/24/45 MP, la latenza evento→frame con p95/p99 e confronti indipendenti, la contabilità dei driver oltre l'attribuzione ai processi, pressione/device loss e la matrice di display/driver e altri target. Il corpus di 1000 Bayer distinti generati serve al catalogo e alle code e non sostituisce questi casi. R0–R4 restano aperti.
