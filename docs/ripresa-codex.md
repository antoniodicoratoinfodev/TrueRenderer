# TrueRenderer — ripresa e revisione del 9 settembre 2026

## Stato corrente verificato il 9 settembre 2026

Il lavoro recuperato, le correzioni della navigazione e i relativi report sono inclusi nel commit `10123b5`, presente sia su `main` locale sia sul remoto verificato in rete. Gli hash di codice, bundle e report coincidono con il [riepilogo delle ultime prove](../reports/preview-navigation-continuation-macos.json): 71 test Rust, 30 NEF entro 2 GiB e suite installata passati. La successiva revisione dei Markdown aggiorna soltanto la documentazione.

Le indicazioni «nessun nuovo commit/push» nelle sezioni storiche si riferiscono alla chiusura delle rispettive prove, prima della pubblicazione. Anche i campi Git nei JSON sono una fotografia del momento della misura e vanno conservati. Il prossimo lavoro è la misura integrata della navigazione; la qualifica generale su altri RAW/hardware rimane aperta.

## Richiesta recuperata

Rilette le sessioni locali «Applica modifiche anteprime cache» e «Consulta la documentazione cache», inclusi i messaggi originali. L'ultima richiesta applicativa è verificare il progetto di anteprime/cache rispetto al codice, correggerlo e applicarlo, integrare la specifica nel documento di architettura spostato nella radice del progetto e rivedere `PLAN.md`. L'ultimo messaggio della sessione precedente chiedeva di salvare il lavoro e i punti aperti per il cambio di abbonamento. Questa continuazione è autorizzata dal titolare.

La richiesta precedente su LibRaw e commit/push era stata consegnata con `1b98b4f` ed `e609310`; non era rimasta un'attività pendente della sessione interrotta. La revisione applicativa successiva si trovava invece nel working tree. Non confondere i report della versione pubblicata con quelli del codice locale.

## Stato recuperato e riscontri

- Invalidazione delle sorgenti richieste/residenti tramite monitor separato; token Unix con device/inode/ctime, epoca dei decoder e scarto dei risultati tardivi. Annotazioni conservate.
- Recupero grafico con una sola ricreazione della finestra/device; salvataggi, selezione e undo separati dal renderer. Seconda perdita: arresto controllato. Le prove usano `device.destroy`, non reset fisici del driver.
- NEF Nikon D750: riconoscimento RAW nel contenitore TIFF, sviluppo nativo 6016×4016, profondità sensore sconosciuta quando i metadati descrivono solo la miniatura. Fingerprint `raw-detection-v2`.
- Specifica anteprime e ADR 0005–0006 integrati nell'appendice E delle due architetture. Il sincronizzatore preserva il testo esterno ai blocchi gestiti e non ricrea il file storico sul Desktop.
- La sessione precedente registrava 64 test Rust e prove native positive; 30 NEF verificati con budget esplicito di 3 GiB. Il fallimento a 2 GiB era reale: richiesta 1603 MiB oltre a 460 MiB già prenotati. Non era una perdita di crediti.
- Il presente file era citato nel piano e nel registro ma non era stato creato: ripristinato durante questa revisione.

## Prima continuazione verificata (storico, prima della revisione navigazione)

1. Corrette le verifiche RAW, memoria e recupero nativo: un nuovo tentativo parte con esito non positivo; errori, timeout e risultati incompleti non lasciano come risultato corrente un precedente successo. I dettagli che possono contenere percorsi privati restano nei log locali. Il report memoria identifica i quattro binari effettivamente provati.
2. Anticipato il rilascio dei byte compressi nel broker dopo l'invio e nel worker prima della risposta raster. La stima di ammissione considera il massimo delle fasi successive: sviluppo nativo, trasferimento privato, costruzione della piramide e copie dei livelli richiesti. Restano l'allowance nativa di 32 byte/pixel + 128 MiB, la baseline di 384 MiB e i crediti separati degli snapshot. Dimensioni dispari e immagini a una sola riga/colonna sono conteggiate con geometria esatta.
3. Passati 67 test Rust, 2 regressioni Python dei report, 6 controlli IPC e 24 sinusoidi. La misura finale usa una copia univoca del bundle e passa a 2 GiB: 30 NEF, cache fredda/calda Standard/Piena, 30 riaperture senza decode, dettaglio nativo su tre file e due richieste fredde simultanee. Footprint massimo 2.100.284.608 byte, RSS 2.066.481.152 byte, massimo tre processi; originali invariati e crediti di lavoro azzerati. Campionamento incompleto: zero; massimo intervallo 355 ms, quindi non è una prova dei picchi più brevi.
4. La precedente misura estesa registrava 2.591.724.536 byte con cinque processi e resta negativa in `reports/preview-memory-overlap-failure-macos.json`. Il dettaglio successivo ha mostrato un'altra istanza host già aperta dallo stesso percorso e un suo decoder: quel totale non isola il carico. La copia univoca risolve la contaminazione includendo tutti i processi della prova. La modifica sperimentale al ritiro XPC è stata rimossa; il codice di terminazione rimane quello precedente.
5. Passate sul candidato le tre prove native di sorgente modificata e perdita del device, con database integri; passata l'integrazione dei due XPC e il rifiuto del comando di fault injection. Gli hash dei quattro binari coincidono con quelli della prova RAW/memoria. Il rifiuto a 512 MiB e i report negativi restano evidenze separate.
6. Bundle finale aggiornato in `dist/TrueRenderer.app`; precedente conservato in `var/package-history/TrueRenderer-0.1.5-before-continuation-20260909-142049.app`. Suite completa installata passata: Finder, formati/cache, screenshot/campionamento, impostazioni, XPC, copia autonoma, database e firma/hash. I quattro binari installati sono identici a quelli delle prove RAW e native. Piano, registro e architetture sincronizzati. Modifiche salvate localmente, nessun nuovo commit/push. I risultati correnti sono in `reports/VERIFICA.md`.

## Ultima continuazione applicativa — navigazione

Preservato il lavoro della sessione precedente; copia dei file interessati, diff iniziale e report in `var/continuation-navigation-20260909-143737`. Corretta una lacuna dello scheduler: un lookup già attivo o un decode in attesa poteva proseguire anche dopo il cambio foto. Ora i passaggi interrompibili controllano la domanda; le chiamate native già in corso terminano senza riciclare XPC, con i crediti mantenuti fino al completamento. La stessa foto con nuova qualità riusa lo sviluppo, mentre i consumatori abbandonati vengono rimossi e rimangono riprovabili.

Passati 71 test Rust (61 ordinari + 10 integrazioni), 2 regressioni Python, fmt/Clippy, 6 prove IPC e 24 sinusoidi. Quattro nuovi test verificano cancellazione prima dell’ammissione, lookup bloccato sui crediti, cambio foto durante probe e cambio qualità con riuso. La prima esecuzione nel sandbox del terminale non aveva accesso a Core Image; la suite nella sessione macOS nativa è passata. Log `var/continuation-navigation-20260909-143737/verify-native.log`.

La nuova verifica su 30 NEF a 2 GiB è passata: 60 coppie Standard/Piena fredde/calde bit-exact, 30 riaperture senza decode, dettaglio su tre file e due richieste simultanee; originali invariati e zero crediti residui. Footprint aggregato massimo 2.018.970.672 byte, RSS 1.965.047.808 byte; 7.042 campioni completi, intervallo massimo 54,63 ms. Questo è un risultato campionato sul carico dichiarato, non un A/B, un p95 o una qualifica dei driver. Report `preview-navigation-memory-real-raw-macos.json` e `preview-real-raw-macos.json`.

**Consegna finale di questa sessione:** aggiornato `dist/TrueRenderer.app`; precedente conservato in `var/package-history/TrueRenderer-0.1.5-before-navigation-20260909-144830.app`. Suite completa installata passata: XPC, Finder, formati/cache, 14 regioni di screenshot entro la soglia di un livello sRGB8, impostazioni, copia autonoma, integrità database e firma/hash. I quattro binari installati coincidono con quelli delle prove RAW/memoria e di recupero. [Riepilogo delle verifiche](../reports/preview-navigation-continuation-macos.json), log in `var/continuation-navigation-20260909-143737`. Piano, registro e architetture sincronizzati; lavoro inizialmente salvato localmente e successivamente pubblicato nel commit `10123b5`. Il prossimo incremento rimane la misura integrata della navigazione evento→frame sui dati disponibili, mantenendo distinti i gate che richiedono altro corpus/hardware.

## Attività ancora aperte nel progetto complessivo

- Memoria e concorrenza su altri RAW, 12/24/45 MP, carichi misti, tutte le viste e pressione fisica; budget di ammissione distinto dal limite RSS del kernel.
- Corpus autorizzato di 1.000 RAW reali e misure p95/p99 evento→frame su hardware definito. I 1.000 DNG sintetici da 512×384 e i 30 NEF da una fotocamera non lo sostituiscono.
- A/B integrato CPU/GPU, energia/termica, reset fisici/OOM, driver e display diversi. Sviluppo RAW ridotto/regionale e Metal del decoder non qualificati.
- Gate R0: bootstrap XPC ostile, handle/output concorrenti, riavvio <1 s, firma reciproca di release e API/minimi OS; verticale Windows e toolkit/accessibilità/display reali.
- R1–R4 e funzionalità mancanti: elenco e dipendenze in `PLAN.md` e nella matrice del registro. Le qualità Anteprima Standard/Piena non abilitano Standard/Riferimento.
- Il titolare aveva chiesto di eliminare `docs/progetto-anteprime-cache-prestazioni.md` **quando tutto fosse finito**. Rimane la fonte della specifica integrata finché i requisiti aperti non sono qualificati; non cancellarlo solo perché il codice di una fase è presente.

## Audit dei Markdown successivo alla pubblicazione

Il 9 settembre sono stati controllati 19 documenti di progetto. Corretti README (stato funzionale, RAW e fixture), piano/spec (prossima attività e stato delle fasi), riferimenti al commit pubblicato e raccordi storici degli ADR. Allineate entrambe le architetture tramite il sincronizzatore. Link locali e hash di codice/bundle/report verificati; l’audit riguarda la documentazione e non aggiunge misure o chiude gate applicativi. I dati di prova nei JSON restano quelli delle esecuzioni originali. Nel secondo controllo sono state corrette anche le voci della matrice di avanzamento su corpus RAW autorizzato, monitor dei file, cache v2, test e memoria, distinguendo quanto già provato dalla qualifica generale.

## Come proseguire senza perdere lavoro

Usare `scripts/cargo-local.sh`; controlli completi in `scripts/verify.sh`. Dopo modifiche a broker/worker/bundle eseguire `scripts/test-xpc-integration.py`, poi le suite native pertinenti. Non ripetere suite già valide senza nuove modifiche o dubbi concreti.

`docs/avanzamento.md` è il registro modificabile; aggiornare le caselle di `PLAN.md` e sincronizzare con `python3 scripts/sync-docs.py`. Preservare il backup v1.2 e il testo utente fuori dai blocchi gestiti. Originali, `var/library.sqlite` e `var/backups` sono dati da conservare; le verifiche fotografiche usano copie private. Non pubblicare fotografie, percorsi privati, cache, database, backup o toolchain.
