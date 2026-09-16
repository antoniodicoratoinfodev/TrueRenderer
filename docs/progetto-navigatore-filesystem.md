# Navigatore di file e cartelle a sinistra — TrueRenderer

Data: 15 settembre 2026. Revisione: 2, con commutazione richiesta dal titolare.

**Stato: pianificazione, da implementare in un incremento successivo.** Questo documento risponde alla richiesta di un navigatore laterale per file, cartelle e directory, con un'interazione familiare a chi usa Adobe Bridge o l'esploratore di un IDE. Le scelte sotto sono la proposta operativa; non descrivono funzioni già disponibili e non autorizzano a dichiarare superati i gate R0–R4.

Il nome proposto nell'interfaccia è **Esplora** in italiano, **Explorer** in inglese. «Finder» descrive qui l'intento dell'utente; il pannello fa parte di TrueRenderer su entrambe le piattaforme previste.

**Requisito esplicito del titolare: il nuovo navigatore deve essere commutabile con il pannello sinistro già presente.** Lo stesso spazio ospita due schede, **Libreria / Library** (pannello attuale) ed **Esplora / Explorer** (nuovo). Il pannello attuale conserva corpus, selezione rapida e filtri; la scheda attiva si sceglie in qualsiasi momento. La commutazione riguarda soltanto l'interfaccia, senza cambiare la cartella o la fotografia in uso.

## 1. Risultato desiderato e confini

L'utente deve poter raggiungere una fotografia partendo dalle proprie cartelle, espandere la gerarchia a sinistra, passare fra cartelle vicine e ritrovare quelle usate spesso senza riaprire ogni volta il dialogo di sistema.

### 1.1 Prima versione completa dell'incremento

- Albero di cartelle **e file**, con caricamento dei figli soltanto quando richiesto.
- Radici aggiunte dall'utente, posizioni comuni e volumi montati, con stati di disponibilità espliciti.
- Apertura della cartella nella griglia esistente; selezione e apertura delle immagini nel viewer esistente.
- Preferiti persistenti, recenti, indietro/avanti, cartella superiore e percorso cliccabile.
- Pannello ridimensionabile e nascondibile, accessibile anche con finestra stretta.
- Selettore permanente Libreria / Esplora, con stato indipendente delle due schede e ripristino dell'ultima usata.
- Navigazione da tastiera, ricerca digitando nei nodi già caricati, filtro locale dei nomi e menu contestuali.
- Aggiornamento manuale, riconciliazione alla riattivazione e osservazione limitata dei rami attivi.
- Gestione di cartelle vuote, grandi directory, permessi, rimozioni, collegamenti e volumi offline.
- Italiano/inglese, percorsi nativi senza perdita d'identità e verifiche UI fino al 200%.

### 1.2 Funzioni successive, fuori da questo incremento

Rinomina, cancellazione, copia/spostamento e creazione di file/cartelle; scrittura di sidecar; terminale integrato; esecuzione o apertura automatica di file con altre applicazioni; indicizzazione ricorsiva dell'intero computer; ricerca globale su disco; nuove viste Elenco/Dettaglio centrali; raccolte statiche/intelligenti; espansione ricorsiva di tutti i discendenti; eject/mount di volumi e gestione di account cloud.

Il trascinamento della prima versione serve ad aprire una posizione o gestire un preferito. Non modifica la collocazione degli originali. La cache delle anteprime mantiene le scritture derivate già previste da ADR 0005; **la sola esplorazione dell'albero non crea cache nelle cartelle visitate**.

## 2. Base reale da cui partire

Riferimenti verificati mediante lettura del repository in questa sessione:

| Area | Stato attuale | Conseguenza per l'implementazione |
|---|---|---|
| [UI desktop](../apps/desktop/src/ui.rs), `sidebar` / `navigation_contents` | Pannello da 208 punti, intervallo 192–270; corpus, selezione e filtri. Scompare sotto la soglia di 1000 punti e parte dei comandi passa nel menu | Conservare il corpo attuale nella scheda Libreria e aggiungere Esplora nello stesso contenitore; rendere commutabile anche la variante compatta |
| `open_path` / `open_folder` | Apertura file/cartella, dialoghi, drop e startup; `open_path` usa anche `is_dir` e `canonicalize` nel frontend | Unificare le entrate in un comando asincrono; nessuna nuova operazione filesystem nel frame UI |
| `open_folder` | Incrementa la generazione, azzera immagini/cache UI, override, selezione e monitor; poi richiede la scansione | Non richiamarla all'espansione di un ramo. Separare cambio posizione, refresh e semplice selezione |
| [Servizio desktop](../apps/desktop/src/service.rs), `Request::Scan` | Scansione non ricorsiva fino a 100.000 immagini candidate; filtra le estensioni e pubblica un unico `Event::Scanned` finale | Serve un enumeratore distinto, capace di elencare tutti i tipi di voce e pubblicare risultati progressivi |
| Loop del servizio | La scansione e `Catalog::observe` avvengono nello stesso loop di Save/Undo/Backup e inoltro della domanda immagini | Spostare l'I/O di directory fuori dal writer e applicare il catalogo in piccoli batch cooperativi |
| Errori della scansione | Il fallimento arriva come `Event::Status(String)`, senza identità della richiesta | Introdurre eventi terminali tipizzati: completata, parziale, fallita, annullata; chiudere sempre lo stato di caricamento corretto |
| [Stato fotografico](../crates/tr-app/src/lib.rs) | `State::replace_items` azzera selezione, trasformazione e `pending`; `refilter` rimuove dalla selezione gli elementi esclusi | Aggiungere un percorso di riconciliazione; rendere i salvataggi pendenti indipendenti dalla cartella visualizzata |
| [Archivio](../crates/tr-store/src/lib.rs) | Writer unico; schema libreria v1; `observe` può creare un asset durevole, non si limita a elencare nomi | Non usare `observe` per espandere il filesystem. Le cartelle e i file non fotografici non diventano asset |
| [Monitor sorgenti](../apps/desktop/src/source_monitor.rs) | Polling dei soli file richiesti/residenti ogni 500 ms | Non è un watcher di cartelle. Aggiungere responsabilità distinta, conservando l'invalidazione delle immagini |
| [Modulo `ui/navigation.rs`](../apps/desktop/src/ui/navigation.rs) | Harness diagnostico comando→superficie del viewer | Non riutilizzare il nome per il nuovo esploratore; preservare il probe |
| [Impostazioni](../apps/desktop/src/cache/settings.rs) | `settings.json`, schema 2 e rifiuto dei campi sconosciuti; include cache, motori e lingua | Evitare di inserirvi implicitamente uno stato arbitrario dell'albero o di interferire con le bozze di Settings |
| [Piattaforma](../crates/tr-platform/src/lib.rs) | Filtro estensioni distinto dalla decodifica effettiva | La visibilità di un file non certifica che il formato o il modello RAW siano decodificabili |

La specifica generale già prevede albero pigro, preferiti, breadcrumb e accessibilità: [architettura](TrueRenderer-Architettura.md), §§5.4–5.6, 13.1–13.3, 14.1–14.6 e 16.5–16.6. Il [restyling corrente](progetto-restyling.md) mantiene strumenti in alto e cartella/conteggi in basso: questo progetto ne estende la navigazione senza ripristinare un'intestazione dentro l'area fotografica.

## 3. Composizione dell'interfaccia

Schema illustrativo, con nomi sintetici:

```text
┌────────────────────────────────────────────────────────────────────────────┐
│ TrueRenderer   Apri   Viste …                        Cerca foto   Settings │
│ [Pannello] [←] [→] [↑] Posizioni › Foto › 2026 › Viaggio              [⋯] │
├──────────────────────┬───────────────────────────────────┬─────────────────┤
│ Libreria | [Esplora] │                                   │ ISPETTORE       │
│ Filtra nomi…         │   Griglia / Viewer / Confronto     │                 │
│                      │                                   │                 │
│ ▾ PREFERITI      [+] │   Immagini della cartella attiva   │                 │
│   Foto               │                                   │                 │
│   Progetto esempio   │                                   │                 │
│ ▸ RECENTI            │                                   │                 │
│ ▾ POSIZIONI          │                                   │                 │
│   ▸ Cartella utente  │                                   │                 │
│   ▾ Foto             │                                   │                 │
│     ▾ 2026           │                                   │                 │
│       ▾ Viaggio      │                                   │                 │
│           001.nef    │                                   │                 │
│           note.txt   │                                   │                 │
│       ▸ Studio       │                                   │                 │
│   ▸ Disco esterno    │                                   │                 │
│                      │                                   │                 │
├──────────────────────┴───────────────────────────────────┴─────────────────┤
│ Viaggio · 24 immagini · 1 selezionata · filtri attivi …                     │
└────────────────────────────────────────────────────────────────────────────┘
```

### 3.1 Commutazione con il pannello attuale

Il selettore **Libreria | Esplora** è in testa al pannello, fuori dallo scorrimento, e rimane raggiungibile in entrambe le modalità. Due schede testuali, con stato attivo chiaro ed etichette accessibili; niente dipendenza da sole icone o da un'opzione nascosta nelle preferenze. Viene mostrato un solo corpo alla volta, senza aggiungere un secondo pannello affiancato.

| Scheda | Contenuto | Stato proprio da conservare |
|---|---|---|
| Libreria / Library | Il pannello esistente: Corpus di prova, Tutte le immagini, Da conservare, Cinque stelle, Scartate, filtri di valutazione/etichetta e azzeramento | Larghezza, scroll, focus e futuri stati di espansione dei suoi controlli |
| Esplora / Explorer | Preferiti, Recenti, Posizioni, albero file/cartelle e filtro nomi | Larghezza, scroll, nodo focalizzato, selezione del tree, espansioni, filtro nomi e modalità delle voci |

Cartella attiva, fotografie, selezione fotografica, filtri fotografici, qualità, motore, zoom e annotazioni sono **condivisi**, con un solo proprietario. Le schede sono due modi di accedere allo stesso contesto; non duplicare `tr_app::State`, catalogo o pipeline.

Regole obbligatorie:

1. Cambiare scheda non chiama `open_folder`, non aggiunge cronologia, non azzera filtri/override/zoom e non incrementa generazioni decoder. Una scansione della cartella attiva o un salvataggio già accettato proseguono.
2. I controlli e il comportamento del pannello esistente restano disponibili in Libreria. I filtri fotografici non vengono trasferiti dentro Esplora o duplicati con un secondo stato. Quando sono attivi restano indicati nella barra anche con Esplora visibile; il riepilogo offre «Mostra filtri» che seleziona Libreria.
3. Ogni scheda conserva il proprio scroll, larghezza e focus logico. La scheda nascosta non riceve tasti né genera widget con focus attivo. Tornando a Esplora si ritrovano i rami aperti; tornandovi dopo un cambio foto si sincronizza l'indicatore corrente senza cancellare l'esplorazione precedente.
4. Con focus sul selettore, frecce sinistra/destra cambiano scheda; Tab entra nel corpo attivo. Un comando dedicato «Vai a Esplora» seleziona Esplora e porta al tree; il semplice clic sulla scheda mantiene un focus prevedibile sul selettore.
5. Default in assenza di stato salvato: **Libreria**, per mantenere riconoscibile l'interfaccia corrente. Dopo una scelta si ripristina l'ultima scheda usata all'avvio. Un valore futuro/non valido ricade su Libreria senza alterare il contesto fotografico.
6. Se il pannello è nascosto, un comando esplicito Libreria/Esplora lo mostra nella scheda richiesta. Il comando Mostra/Nascondi pannello conserva invece l'ultima scheda. Il cambio automatico di dimensione della finestra non cambia scheda.

La barra di percorso è condivisa e resta disponibile in entrambe le schede. Aprire una cartella da Libreria (per esempio il corpus) aggiorna il contesto di Esplora; se Esplora è nascosto si registra la posizione da rivelare, senza caricare l'intero albero in background.

### 3.2 Organizzazione e misure iniziali

- Il pannello sinistro è inizialmente visibile dove c'è spazio, con la scheda Libreria. Esplora propone larghezza 264 punti logici, minima 200, massima 420; Libreria mantiene come base 208 punti e intervallo attuale 192–270. Memorizzare le larghezze per scheda e adattarle allo spazio centrale disponibile. Questi valori sono parametri da verificare, non pixel fisici.
- In Esplora, sezioni richiudibili Preferiti, Recenti e Posizioni. Recenti inizialmente chiusi; ordine fisso nel primo incremento. Il riordino di tutte le sezioni della specifica v1 resta successivo.
- Selettore delle schede e campo filtro di Esplora rimangono raggiungibili; ciascun corpo ha scorrimento proprio. Selezione rapida e filtri fotografici restano nel pannello Libreria.
- Righe alte almeno 24 punti a scala normale, adattate alle metriche del testo. Icona 16–20, rientro iniziale 16; nomi su una riga con ellissi, percorso completo accessibile da focus/tooltip e Copia percorso.
- Directory prima dei file; ordinamento naturale per nome, numeri compresi (`2` prima di `10`), con criterio stabile di spareggio sui nomi nativi. Ordinamento e ricerca non modificano l'identità dei percorsi.
- Icone sobrie per cartella aperta/chiusa, file immagine candidato, altro file, collegamento e volume. Nessuna miniatura nel tree nella prima versione; nessun decode per disegnare una riga.
- Cartella attiva evidenziata; focus da tastiera distinto; file corrente indicato quando il ramo è visibile. I tre stati possono riferirsi a righe differenti.
- Indicatore «contiene immagini» solo dopo aver osservato direttamente almeno un candidato. Non scandire preventivamente sottocartelle e non mostrare zero per un ramo inesplorato.

### 3.3 Spazio ridotto e viewer

La barra di navigazione occupa una riga dedicata sotto la barra principale e prima degli strumenti del viewer. Breadcrumb comprimibile al centro con antenati in overflow; nessuna terza copia del percorso esteso nel pannello fotografico. Il piè di finestra conserva nome cartella, conteggi e stato.

Quando le larghezze minime del pannello sinistro, centro e ispettore non entrano, il contenitore diventa un pannello temporaneo aperto dal pulsante sempre raggiungibile. Conserva lo stesso selettore Libreria | Esplora e la stessa scheda attiva; il passaggio resta possibile anche qui. Proposta: sotto 1000 punti iniziare dal pannello temporaneo, poi calibrare con i test del layout effettivo. Il vecchio menu compatto «Libreria e filtri» deve aprire Libreria nello stesso contenitore, senza renderizzare una seconda copia dei controlli. Un solo pannello temporaneo alla volta; Settings ha precedenza. Esc chiude il pannello e restituisce il focus al pulsante; il viewer non riceve lo stesso Esc.

Nel pannello temporaneo cambiare scheda o espandere un ramo non chiude il pannello. L'attivazione di file/cartella riuscita lo chiude. Il ridimensionamento, la chiusura o il ripristino della larghezza dell'altra scheda modifica il viewport: Adatta si ricalcola, 1:1 mantiene il rapporto fisico e il centro valido. Non deve invalidare sorgenti, qualità o annotazioni.

## 4. Contratto delle interazioni

### 4.1 Mouse, attivazione e file non fotografici

| Azione | Effetto proposto |
|---|---|
| Clic sulla freccia di una cartella | Espande/comprime quel ramo; non cambia cartella centrale, selezione fotografica o generazione decoder |
| Clic sul nome di una cartella | Richiede l'apertura delle sue immagini nella griglia; mantiene separato lo stato di espansione |
| Doppio clic sulla cartella | Apre e rende espanso il ramo; deduplica il primo clic, senza due scansioni della stessa richiesta |
| Clic sul file immagine candidato | Seleziona la foto nella cartella pertinente; in griglia resta in griglia, nel viewer aggiorna il viewer; nel confronto torna alla griglia per evitare una coppia implicita |
| Doppio clic o Invio su immagine candidata | Apre il viewer; riusa selezione/catalogo se la cartella è già attiva |
| Clic su altro file | Seleziona soltanto la riga; indica «Anteprima non disponibile per questo tipo di file» e mantiene il contesto fotografico |
| Invio/doppio clic su altro file | Mostra azioni disponibili (percorso, mostra nel sistema); non esegue né avvia applicazioni |
| Clic destro | Menu della riga; non apre cartelle e non cambia la selezione fotografica di nascosto |
| Espansione automatica verso una foto | Espande soltanto la catena degli antenati necessaria, mai i fratelli o i discendenti interi |
| Clic sulla cartella già attiva | Porta il focus alla posizione; non equivale a Rileggi e non azzera il viewer |

Menu cartella: Apri, Aggiungi/Rimuovi dai preferiti, Rileggi questo ramo, Copia percorso, Mostra nel Finder/Esplora file. Menu file: Apri anteprima se candidato, Mostra nel pannello, Copia percorso, Mostra nel sistema. Menu preferito: Apri, Rinomina preferito, Sposta su/giù, Rimuovi. Rinominare un preferito modifica solo la sua etichetta nella libreria.

L'albero mostra inizialmente tutti i file ordinari, compresi quelli non supportati; menu con «Solo cartelle», «Cartelle e immagini», «Tutti i file». Le cartelle restano visibili in ogni modalità. La griglia rimane fotografica e non acquisisce celle per TXT, archivi o eseguibili.

### 4.2 Filtri e apertura diretta

Distinguere **Filtra nomi** nell'albero da **Cerca foto** e dai filtri fotografici. Il primo restringe soltanto i nodi già caricati, mantenendo gli antenati necessari: testo di aiuto «Nei rami caricati». Non è una ricerca ricorsiva sul disco; i rami non letti restano segnalati. Digitare con focus nel tree, fuori dal campo, effettua invece ricerca per prefisso nelle righe visibili, con reset dopo 700 ms e composizione IME rispettata.

Navigazione ordinaria verso una nuova cartella conserva i filtri fotografici correnti. La barra mostra filtri attivi e distingue «Cartella senza immagini» da «Nessuna immagine corrisponde ai filtri». Azione Azzera filtri disponibile nello stato vuoto.

L'apertura esplicita di una foto dal tree, dal dialogo, dal drop o da `--open` deve raggiungere quella foto anche se i filtri la escludono. Proposta: sospensione temporanea e dichiarata dei filtri per l'apertura mirata, con comando Ripristina filtri; non cancellare le preferenze di ricerca. Questa eccezione va modellata nel filtro dello stato fotografico, perché il `refilter` attuale eliminerebbe il target dalla selezione. Chiudere l'apertura mirata o navigare a un'altra cartella ripristina il contesto ordinario; la cronologia conserva il contesto precedente.

Un'immagine candidata rifiutata dal decoder resta selezionabile con errore esplicito. Un file senza estensione riconosciuta rimane visibile, ma non introduce un nuovo percorso di sniffing/decodifica nell'host.

### 4.3 Indietro, avanti, su e percorso

- Cronologia di sessione di massimo 100 posizioni, con indice corrente. Espandere, comprimere, filtrare il tree, selezionare un file della stessa cartella o fare refresh non aggiunge posizioni.
- Nuova navigazione riuscita dopo Indietro tronca il ramo Avanti. Attivazioni identiche consecutive si fondono. Un tentativo fallito prima dell'attivazione non entra nella cronologia.
- Ogni voce conserva posizione, filtri fotografici, foto corrente e selezione per ID, vista, ancora di scorrimento per ID e offset relativo. Conserva zoom/centro soltanto se possono essere ristabiliti per la stessa sorgente/revisione; non conserva raster o nuovi override di qualità.
- Indietro/Avanti rivalidano la posizione e recuperano soltanto gli ID ancora presenti. In caso di foto sparita, selezionano un vicino valido o nessuna foto. Niente associazioni per sola somiglianza del nome.
- Su apre il genitore effettivo; alla radice consentita offre di raggiungere una nuova posizione tramite il meccanismo di accesso esplicito. Alla radice del volume è disabilitato.
- Breadcrumb: segmenti cliccabili, percorso completo copiabile e modalità di inserimento percorso; risoluzione in background. Accetta percorsi nativi assoluti e relativi alla cartella attiva; nessuna espansione shell, variabile o esecuzione di comandi.
- Apri file/cartella, drop e startup passano dallo stesso coordinatore. La sincronizzazione griglia→tree evidenzia il file senza generare una navigazione tree→griglia di ritorno.
- «Segui la foto corrente» inizialmente attivo: espansione degli antenati con limite, nessun furto del focus. Scorrere il tree per esplorare un altro ramo sospende l'autoscroll fino al comando Mostra foto corrente.

### 4.4 Tastiera e accessibilità

| Contesto / tasti | Effetto |
|---|---|
| Tree, ↑ / ↓ | Focus alla riga precedente/successiva; non carica ogni fotografia attraversata |
| Tree, → | Espande cartella; se già espansa porta al primo figlio |
| Tree, ← | Comprime cartella; altrimenti porta al genitore |
| Tree, Home / End | Prima/ultima riga del modello visibile caricato |
| Tree, Page Up / Down | Sposta focus di circa una pagina, con scorrimento coerente |
| Tree, Invio | Attiva cartella o immagine; applica la policy del tipo di voce |
| Tree, Spazio | Seleziona la riga senza aprire il viewer o assegnare rating |
| Tree, Shift+F10 / tasto menu | Menu contestuale della riga focalizzata |
| Tab / Shift+Tab | Passaggio tra toolbar, selettore schede, corpo attivo (tree oppure controlli Libreria) e centro; nessuna trappola né focus nella scheda nascosta |
| Esc | Prima popup/IME/campo percorso, poi pannello temporaneo; una sola azione per evento |

Scorciatoie proposte, da verificare sui due OS: Cmd/Ctrl+B mostra/nasconde il pannello sinistro conservando la scheda attiva, Cmd/Ctrl+Shift+E attiva Esplora e porta il focus al tree, Cmd/Ctrl+L modifica il percorso; macOS Cmd+[ / Cmd+] e Windows Alt+← / Alt+→ per cronologia; Cmd/Ctrl+↑ per Su come previsto dalla specifica. Cmd/Ctrl+F resta Cerca foto. Libreria è raggiungibile dal selettore e dal menu Mostra pannello → Libreria. I menu mostrano le combinazioni effettivamente assegnate. Nessuna combinazione deve intercettare digitazione, selezione di testo o composizione IME.

Estendere l'instradamento dei comandi prima del ramo fotografico di `keyboard`: popup/dialogo → testo/IME → tree → immagini. I tasti 0–9, X, G, E, C digitati nel tree non devono assegnare rating o cambiare vista.

Semantica accessibile da verificare sul toolkit installato: albero, elemento, livello, espanso/compresso, selezionato, occupato, non disponibile; etichette per tutte le icone. La virtualizzazione mantiene il focus sul nodo logico e materializza la riga quando necessario, senza creare nodi fantasma per tutto il disco. VoiceOver e NVDA richiedono prove native; lo screenshot al 200% non le sostituisce.

## 5. Posizioni, percorsi e disponibilità

### 5.1 Radici e volumi

Posizioni propone le directory comuni disponibili attraverso l'adattatore OS, le cartelle aperte esplicitamente e i volumi montati rilevati. Visualizzare una radice non ne enumera automaticamente i discendenti. Il primo accesso è un'azione dell'utente; apertura di file autorizza l'esplorazione della sua cartella nel perimetro già previsto dal prodotto.

Il catalogo delle posizioni è distinto dai preferiti: una posizione comune può scomparire senza cancellare dati dell'utente; un preferito scelto dall'utente resta presente come non disponibile. Rilevare i mount point è distinto da esplorare la rete: niente ricerca automatica di server né montaggio/connessione automatica.

Lo stato persistito non è un permesso OS. Cartelle protette possono richiedere il dialogo nativo o l'intervento dell'utente nelle impostazioni del sistema; errori locali con azione Apri cartella… e Riprova. Non richiedere accesso completo al disco come prerequisito generale del pannello e non modificare gli entitlement dei decoder.

### 5.2 Identità e nomi

- Usare `PathBuf` / `OsString` e una chiave nativa separata dal testo mostrato. Persistenza lossless: byte Unix o unità UTF-16 Windows con tag piattaforma e versione.
- Non identificare nodi, preferiti o foto con stringhe lower-case o `to_string_lossy`; nomi simili e normalizzazioni Unicode possono collidere.
- Separare percorso richiesto, percorso risolto e identità osservata del volume/file. La canonicalizzazione è I/O fallibile, da eseguire fuori dalla UI e mai su tutti i nodi per frame.
- `NodeId` è l'identità della riga nell'albero della sessione; `LocationKey` identifica la posizione; l'ID fotografico rimane quello del catalogo. Due rami possono mostrare lo stesso target senza fondere nodi e annotazioni.
- File ID/volume ID aiutano a rilevare sostituzioni, collegamenti e cicli; non dimostrano da soli continuità dell'asset. La riconciliazione durevole generale di rinomine resta in R2.
- I percorsi Windows UNC, drive root, reparse point e lunghi richiedono fixture/native test; non costruire percorsi con concatenazioni di stringhe o tagli sul carattere `/`.

### 5.3 Policy dei casi speciali

| Caso | Regola |
|---|---|
| Nascosti | Esclusi per default; «Mostra nascosti» vale per tree e scansione fotografica della posizione, con refresh coerente; non confonderli con tipi non supportati |
| `.truerenderer-cache` | Esclusa sempre dal tree fotografico e dalla scansione; mai scendere nei derivati come se fossero sorgenti |
| Symlink / junction / reparse point | Visibili come collegamenti; non attraversati automaticamente. Attivazione esplicita risolve e verifica destinazione/perimetro; se fuori radice richiede l'apertura esplicita della nuova posizione |
| Ciclo / profondità estrema | Identità degli antenati visitati + limite proposto 128 livelli; messaggio e possibilità di aprire direttamente una sottocartella come nuova radice |
| Collegamento rotto | Riga presente con stato leggibile; nessuna ricorsione o retry continuo |
| Alias macOS / shortcut Windows | Non interpretarli come normali cartelle senza un resolver nativo qualificato. Nella prima verticale possono restare file con azione Mostra nel sistema; lo stato di supporto va dichiarato |
| Pacchetto macOS | Trattarlo come elemento opaco nella prima versione; nessun attraversamento automatico o parsing delle risorse interne |
| Cloud placeholder | Nessuna lettura dei byte o anteprima al semplice expand. Se il provider non garantisce un'osservazione senza download, segnalare il limite e rinviare l'apertura a un'azione esplicita |
| File speciale, socket, FIFO, device | Non aprire come immagine, non leggerne il contenuto; mostrare eventualmente il tipo nel tree |
| Permesso negato / errore I/O | Stato del solo nodo, distinguendo «non leggibile» da vuoto; i fratelli restano utilizzabili |
| Directory incompleta / quota raggiunta | Mostrare «Elenco parziale» e motivo; non trasformare gli elementi non osservati in cancellazioni |
| Volume scollegato | Conservare radice, preferiti e annotazioni; cancellare richieste/artefatti non più validi e indicare offline |
| Ritorno del volume | Riconciliare mediante identità qualificata e riaprire la posizione. Lo stesso nome del disco o lettera non basta per ricollegare automaticamente un preferito |

Il rilevamento cloud/pacchetti è uno spike esplicito per piattaforma. Non promettere assenza universale di I/O del provider: misurare il comportamento. Non attivare lookup di icone/miniature della shell che possa avviare parser o scaricamenti impliciti.

## 6. Architettura proposta

### 6.1 Moduli e responsabilità

I percorsi nuovi elencati sotto sono **proposti**, non presenti nella consegna di pianificazione.

| Modulo | Responsabilità |
|---|---|
| `crates/tr-app/src/browser.rs` — nuovo | Modello deterministico, comandi, focus, espansioni, cronologia, selezione di posizione, accettazione/scarto delle risposte |
| `apps/desktop/src/filesystem_browser.rs` — nuovo | Servizio di enumerazione in background, code limitate, cancellazione, cache metadati di directory; nessun parser immagine o accesso diretto al DB |
| `apps/desktop/src/ui/left_panel.rs` — nuovo | Unico contenitore sinistro, selettore Libreria/Esplora, layout normale/temporaneo, geometria e focus delle due schede; ospita il contenuto Libreria già esistente |
| `apps/desktop/src/ui/explorer.rs` — nuovo | Tree virtualizzato, sezioni, menu, focus e layout; produce comandi, non fa I/O |
| `apps/desktop/src/ui/location_bar.rs` — nuovo | Indietro/Avanti/Su, breadcrumb, immissione percorso e overflow |
| `apps/desktop/src/browser_session.rs` — nuovo | Persistenza versionata di scheda attiva, geometria per scheda, recenti e stato recuperabile |
| `crates/tr-platform/src/filesystem.rs` — nuovo | Posizioni comuni, volumi, classificazione nativa, identità, osservazione directory e reveal nel sistema |
| `apps/desktop/src/service.rs` — esistente | Writer unico, registrazione delle sole immagini della cartella attiva in batch, commit preferiti, coordinamento con decode esistente |
| `crates/tr-store` — esistente | Migrazione e API preferiti, backup/export, osservazioni delle immagini senza ridefinire l'identità degli asset |
| `ui.rs`, `i18n.rs`, `ui/style.rs`, `main.rs` | Collegamento comandi/aperture, testi IT/EN, stile e ciclo di vita |

Riutilizzare egui/eframe e il sistema di repaint corrente. Nessuna nuova dipendenza per un widget albero prima dello spike; le eventuali dipendenze native richiedono motivazione, pin e inventario/notice aggiornati.

### 6.2 Stato minimo

```text
LeftPanelState
  mode: Library | Explorer
  visible / presentation: Docked | Temporary
  width_by_mode / scroll_by_mode / last_focus_by_mode

BrowserState
  roots / favorites / recent_locations
  nodes: NodeId -> Node
  expanded_nodes / focused_node / selected_node
  active_location / pending_navigation
  history + cursor
  visible_rows + revision
  name_filter / entry_filter / show_hidden
  follow_current_photo

Node
  id / parent / native_location / display_name / kind
  child_state: Unloaded | Loading | Ready | Partial | Failed | Offline
  child_ids / observed_identity / request_id / listing_epoch

PendingNavigation
  navigation_id / target / origin / desired_photo / desired_view
  phase: Resolving | Validating | Activated | Scanning
  restore_context / cancellation
```

Le righe sono una proiezione piatta dei soli rami espansi, con profondità e ID stabili; si ricostruisce quando cambiano struttura/filtro, non a ogni frame. Lo scroll disegna solo il tratto visibile più un piccolo margine. Collassare un antenato del focus riporta il focus all'antenato; l'espulsione dalla cache non deve lasciare riferimenti a nodi inesistenti.

`LeftPanelState` governa soltanto la presentazione; `BrowserState` vive anche quando Libreria è visibile. Usare un ID stabile per il contenitore e namespace UI distinti per i due corpi, indipendenti dalla lingua. Non ricreare il modello dell'albero a ogni switch e non renderizzare entrambe le schede invisibilmente. Le operazioni superflue del tree nascosto possono essere sospese, ma la scansione fotografica attiva e i salvataggi restano indipendenti.

### 6.3 Comandi ed eventi

Contratto concettuale da tradurre in enum Rust:

```text
Commands:
  SetLeftPanelMode(Library | Explorer), ToggleLeftPanel, FocusExplorer
  Expand(NodeId), Collapse(NodeId), Focus(NodeId), Activate(NodeId, mode)
  Navigate(Location, origin, desired_photo), Back, Forward, Up
  Refresh(scope), RevealCurrentPhoto, SetEntryFilter, SetNameFilter
  AddFavorite, RenameFavorite, ReorderFavorite, RemoveFavorite

Directory requests:
  ResolveLocation, ListChildren, CancelListing, ObserveLocations

Directory events:
  LocationResolved / LocationFailed
  ListingBatch(node_id, request_id, listing_epoch, sequence, entries)
  ListingFinished(..., completeness, counts)
  ListingFailed(..., typed_error, partial_count)
  ListingCancelled(...)
  LocationInvalidated(location_key, observation_epoch, reason)

Catalog scan events:
  ScanStarted / ScanBatch / ScanFinished / ScanFailed / ScanCancelled
  (sempre con navigation_id, scan_id, location_key e sequenza)
```

Gli errori portano codice traducibile, contesto di posizione e diagnostica separata; non un generico Status senza richiesta. Limiti, permessi e offline non sono la stessa classe di errore. Ogni richiesta ammessa ha un esito terminale o una revoca registrata; nessuno spinner dipende dall'arrivo di una stringa libera.

### 6.4 Tre cicli di vita distinti

1. **Listing del ramo:** richiesta/epoca per nodo. Espandere A non invalida B e non ricicla decoder. Comprimere un ramo revoca il lavoro superfluo; una risposta di un'epoca precedente non lo riapre.
2. **Navigazione e scansione della posizione attiva:** `navigation_id` e `scan_id`. Cambi rapidi A→B→C fanno applicare alla griglia soltanto C; un errore tardivo di A non copre lo stato di C. Un refresh mantiene la posizione e cambia solo `scan_id`.
3. **Immagini:** generazione/revisione delle richieste del percorso esistente. Cambiare motore o invalidare una sorgente non deve perdere il target di una navigazione in corso. Il cambio effettivo di cartella revoca la domanda immagine precedente una sola volta, inclusa la revoca del dominio prevista dal broker.

La UI controlla l'intera identità del risultato prima di applicarlo. Risultati obsoleti liberano i propri buffer/crediti; eventuale riuso richiede compatibilità verificata. I salvataggi di annotazioni e preferiti hanno ID di operazione indipendenti: **non sono cancellabili con la navigazione**.

### 6.5 Sequenza di apertura di una cartella

1. Un comando registra la destinazione richiesta; il breadcrumb attivo continua a indicare la cartella realmente mostrata, con «Apertura di …» separato.
2. Il servizio risolve il percorso e verifica accesso, tipo e perimetro in background. Se fallisce qui, vecchia vista e cronologia restano utilizzabili.
3. Dopo una prima apertura valida della directory, il coordinatore attiva la nuova posizione: salva il contesto di ritorno, aggiorna cronologia, annulla la domanda della cartella precedente e mostra il caricamento della nuova. Nessuna vecchia foto etichettata con il percorso nuovo.
4. Listing progressivo dei figli diretti. Le immagini candidate passano in batch al writer; la griglia si popola via `ScanBatch` e richiede soltanto le anteprime necessarie alla vista. L'albero non deve dipendere dai tempi di `Catalog::observe`.
5. L'eventuale target file viene selezionato appena registrato. Non auto-selezionare a ogni batch e non rubare una selezione fatta dall'utente durante il caricamento.
6. Al termine, ordinamento e conteggi finali; chiusura dello stato occupato. Un errore dopo l'attivazione lascia quella posizione con lista parziale/errore e Riprova/Indietro; non la dichiara completata e non ripristina silenziosamente la vecchia cartella.

I batch arrivano in ordine di enumerazione e possono precedere il sort finale. Durante il caricamento indicare «Ordine in aggiornamento»; l'ordinamento naturale definitivo viene pubblicato come revisione atomica del modello preservando ancora di scroll e selezione per ID. Una ricerca che richiede una lista completa espone il limite finché l'enumerazione non termina. Non chiamare `read_dir().skip(n)` una paginazione stabile: il cursore appartiene alla singola sessione di enumerazione.

### 6.6 Refresh e modifiche esterne

Rileggi non usa più il percorso distruttivo del cambio cartella. Conserva gli elementi noti come «da verificare», selezione, trasformazione compatibile e stato delle annotazioni. Riconcilia i risultati: inserimenti, modifiche, assenti confermati solo da una scansione completa riuscita. Errori, cancellazione o superamento quota non autorizzano rimozioni dal catalogo.

I watcher osservano cartella attiva e rami espansi entro quota; non l'intero disco. Registrare l'osservazione prima della scansione, accumulare invalidazioni durante la lettura e riconciliare di nuovo se necessario. Eventi accorpati/overflow richiedono rilettura, non applicazione cieca di singole rinomine. Riattivazione della finestra e Refresh funzionano anche senza watcher; fallback di polling soltanto sulle posizioni attive, con frequenza misurata e sospensione a finestra inattiva.

Il monitor delle sorgenti resta responsabile dell'invalidazione dei pixel già richiesti/residenti. Coordinare i segnali per evitare due invalidazioni dello stesso cambiamento. Una foto rimossa o sostituita non deve rimanere presentata come corrente; le annotazioni non si cancellano. Nessun tentativo in questo incremento di trasferire automaticamente annotazioni a una foto rinominata o simile.

### 6.7 Concorrenza, salvataggi e arresto

- Enumerazione separata dal writer, dal monitor sorgenti e dai decoder. Massimo iniziale due operazioni filesystem concorrenti complessive, una per volume dove identificabile.
- Priorità: risoluzione/apertura richiesta dall'utente, cartella attiva, espansione esplicita, riconciliazione di sfondo. Si deduplicano richieste uguali; l'ultima navigazione sostituisce quelle non ancora partite.
- Code limitate in elementi **e byte**; contropressione sui batch. La UI drena con un budget temporale per frame e richiede repaint se rimane lavoro.
- Il writer applica osservazioni catalogo in batch brevi, cedendo a Save/Undo e alla domanda visibile tra un batch e il successivo. Nessun mutex del catalogo rimane acquisito durante `read_dir`, stat, canonicalizzazione o attese di rete.
- Trasferire il registro delle mutazioni pendenti a un proprietario di sessione durevole rispetto ai cambi vista. `replace_items` non può far dimenticare un Save accettato, il suo errore, l'undo o l'attesa in chiusura. I commit di foto non visibili aggiornano comunque lo stato confermato.
- La cancellazione filesystem è cooperativa fra operazioni. Una syscall bloccata su NAS non viene interrotta da un flag o da un timeout UI. Segnalare attesa prolungata, usare l'altro slot se disponibile e non creare thread sostitutivi senza limite.
- Se entrambi gli slot sono bloccati, l'I/O dell'esploratore resta in attesa con stato esplicito; UI, salvataggi già accettati e immagini residenti rimangono operativi. Lo spike deve misurare questo limite, senza promettere nuove navigazioni immediate su ogni NAS.
- Shutdown: revoca listing/watch, chiusura dei canali e rilascio ordinato; i worker di solo listing non possiedono DB né dati da salvare. Non attendere indefinitamente il join di una syscall; la chiusura deve comunque completare i salvataggi accettati secondo il contratto esistente. Se l'adattatore OS non consente questo isolamento, fermare quel gate e progettare una soluzione dedicata, senza alterare tacitamente i due servizi XPC decoder.

## 7. Persistenza e migrazioni

### 7.1 Separare dati scelti dall'utente e stato ricostruibile

| Dato | Destinazione proposta | Regola |
|---|---|---|
| Preferiti: etichetta, ordine, posizione nativa, identità volume osservata | Nuova tabella di `library.sqlite` | Durevoli; writer unico, commit confermato, inclusi in backup/export; non espellibili come cache |
| Scheda attiva, visibilità, larghezza/scroll per scheda, sezioni, filtri del tree, segui foto | `<data>/browser-state.json`, schema dedicato | Default Libreria; scrittura atomica dopo breve debounce; non applica bozze di Settings. Focus logico conservato solo in sessione |
| Recenti | Stesso stato di navigazione, massimo 20 posizioni | Comando Svuota recenti e opzione per non memorizzarli; nessuna cancellazione di preferiti/originali |
| Espansioni e posizione precedente | Stesso file, massimo 128 espansioni salvate | Sono suggerimenti da rivalidare; nessuna scansione massiva all'avvio |
| Cronologia, selezioni e zoom di ritorno | Solo sessione, massimo 100 voci | Nessuna persistenza di raster o riavvio automatico di decode per la cronologia |
| Listing e conteggi | RAM limitata; eventuale indice futuro separato | Ricostruibili; scadenza/invalidazione, mai fonte delle annotazioni |

Proposta di tabella preferiti: ID stabile, etichetta facoltativa, chiave percorso nativa con formato/versione/piattaforma, suggerimento di volume qualificato, ordine e revisione. L'etichetta predefinita deriva dal nome corrente della posizione; il dato scelto esplicitamente dall'utente viene preservato. Limitare inizialmente a 200 preferiti e 256 caratteri per l'etichetta; segnalare il limite prima del commit.

L'import/export conserva i locator di altra piattaforma come irrisolti: non converte automaticamente `C:\Foto` in una posizione macOS. Una nuova associazione del preferito non riassegna gli asset fotografici. Non deduplicare target ambigui sulla sola canonicalizzazione o sul nome del volume.

### 7.2 Migrazione v1 → versione successiva

La prima implementazione dei preferiti richiede una migrazione esplicita della libreria, con backup SQLite consistente **prima** delle modifiche. All'avvio attuale `Catalog::open` rifiuta versioni maggiori di 1: implementare la migrazione nella sequenza corretta, non incrementare soltanto `user_version` nello SQL.

Verifiche: libreria vuota, copia v1 popolata, backup e ripristino, interruzione prima/dopo commit, disco pieno, schema futuro sconosciuto, export con preferiti. Una build precedente deve rifiutare il nuovo schema con messaggio chiaro e senza scritture. Rollback tramite copia di prova/backup verificato, evitando di perdere annotazioni create dopo la migrazione; non promettere downgrade automatico.

`browser-state.json` non amplia `settings.json` con campi che la build corrente rifiuterebbe. Conservare file corrotti o di versione futura prima di qualsiasi recupero, con limiti di dimensione (proposta 2 MiB), numero di elementi e lunghezze. Errori di persistenza del layout non impediscono di sfogliare; errori dei preferiti non devono essere presentati come salvataggi riusciti.

La radice dati è quella già risolta da `--data`/applicazione. Non introdurre hardcoded directory nella home. Il passaggio generale a app-data nativo qualificato resta nel piano di rilascio. `var/library.sqlite`, `var/backups`, percorsi privati e recenti non devono finire in Git o nei report pubblici.

## 8. Isolamento, cache e rendering

L'enumeratore usa solo nomi e metadati filesystem minimi. I byte delle immagini entrano nel broker/decode esistente; nessun EXIF, RAW, thumbnail della shell o preview di file generico viene interpretato nel widget.

Sul Mac gli esterni restano ammessi soltanto nel bundle con i **due servizi XPC/App Sandbox**, secondo [ADR 0002](adr/0002-xpc-decoder-r0.md) e [ADR 0004](adr/0004-formati-esterni-e-pubblicazione.md). Il binario Mac fuori bundle mantiene l'allowlist e non ottiene nuove capacità grazie al pannello. Un errore XPC non produce fallback non isolato.

Il repository descrive inoltre il percorso Windows LPAC verificato in [ADR 0009](adr/0009-isolamento-worker-windows.md): conservarne i controlli di confinamento effettivi, senza dedurre autorizzazioni dal semplice uso delle pipe o dalla presenza di un'estensione. Il navigatore non estende alcuna matrice di formati o sicurezza.

La cache delle directory del tree contiene metadati di navigazione, distinta dalla cache fp32 delle immagini. Solo una cartella realmente attivata entra nel normale ciclo di manutenzione/anteprime. Thumbnail, viewer, filmstrip e confronto continuano a condividere pipeline, quote, revisioni e provenienza; il tree non richiede anteprime né alimenta prefetch per rami espansi.

Gli originali restano in sola lettura. Eventuali comandi Mostra nel sistema usano un adattatore con argomenti strutturati e percorsi nativi, senza interpolazione shell. Nessun nome file viene trattato come comando.

## 9. Budget e prestazioni da qualificare

Valori iniziali **proposti**, da registrare prima delle misure e ricalibrare con evidenze:

| Risorsa / misura | Budget o obiettivo iniziale |
|---|---|
| Concorrenza filesystem | 2 operazioni totali; 1 per volume ove identificabile |
| Richieste pendenti | 64, deduplicate; nuova navigazione sostituisce quella superata |
| Batch listing | Massimo 256 voci e 256 KiB; chiude il batch prima se raggiunge uno dei limiti |
| Batch in transito | Massimo 8 e 2 MiB complessivi |
| Modello, stringhe, righe e listing in RAM | Massimo iniziale 64 MiB complessivi, inclusi buffer/sort e messaggi; parte del budget applicativo, non quota aggiuntiva gratuita |
| Directory attiva | Fino a 100.000 voci complessive entro quota byte; il vecchio limite conta solo immagini. Oltre quota: elenco parziale esplicito, nessuna promessa di completezza |
| Radici/rami residenti | Espulsione LRU dei rami comprimibili/inattivi; focus, cartella attiva e antenati necessari protetti entro quota |
| Watcher di directory | Fino a 128; priorità alla cartella attiva e rami visibili; altri riletti all'accesso |
| Applicazione eventi nella UI | Target ≤2 ms per frame; lavoro residuo differito, nessun sort di 100k nomi nel frame |
| Segnale visivo dell'azione | Entro il frame successivo, target ≤100 ms su macchina di prova libera |
| Primo batch locale | Target p95 ≤250 ms per directory sintetica di 10.000 voci su SSD qualificato; distinguere listing da registrazione catalogo e decode |
| Scorrimento tree residente | Target frame p95 ≤16,7 ms a 60 Hz, misurato con tree e viewer attivi |

Un nome singolo che eccede il limite del messaggio richiede errore di voce esplicito, non troncamento della chiave nativa. Al raggiungimento della quota: liberare rami inattivi, poi fermare l'enumerazione con stato parziale se ancora necessario. Preferiti e libreria non sono risorse espellibili. Non tentare di aggirare il limite accumulando pagine nascoste senza contabilità.

Misurare almeno: input→risposta UI, richiesta→primo batch tree, richiesta→primo batch fotografico, scansione completa, attesa/commit annotazioni, numero di directory visitate, byte/nodi/handle/thread residenti, decode avviati, durata cancellazione cooperativa e tempo di chiusura. Separare I/O bloccato da attività CPU.

Fixture: 0/1/10.000/100.000 e oltre 100.000 voci, cartelle miste, profondità fino/oltre limite, nomi lunghi, Unicode e non UTF-8 ove supportato. Prove su memoria bassa e configurazione ordinaria, SSD/cache calda/fredda, disco rimovibile e rete lenta se disponibili. Per i p95 almeno 100 campioni indipendenti con condizioni/hardware/versione dichiarati; nessuna qualifica NAS o Windows dedotta da un mock Mac. Misurare A/B della build precedente per escludere regressioni di viewer e salvataggi.

## 10. Fasi di implementazione e criteri di uscita

Le sigle N0–N7 sono fasi di **questo incremento**, distinte dai gate R0–R4. In questa consegna sono tutte da eseguire. Implementare in ordine; ogni fase lascia codice compilabile e aggiornamento documentale.

| Fase | Lavoro concreto | Dipendenze | Uscita verificabile |
|---|---|---|---|
| N0 — contratti e spike | Fixture sintetiche; baseline; prototipo di switch Libreria/Esplora e tree/focus virtualizzato; prove filesystem bloccato, volumi, placeholder e percorsi nativi; definire esatta tabella comandi | Lettura di questo documento e stato del repository al momento dell'avvio | Scelte OS/toolkit misurate, pannello attuale conservato, limiti annotati, criteri congelati prima di sviluppare |
| N1 — modello ed enumeratore | `BrowserState`, riduzione comandi, ID/epoche, cache limitata, listing pigro con batch/errori/cancellazione | N0 | Espansione A/B indipendente, no I/O UI, limiti di byte/handle e nessuna ricorsione implicita |
| N2 — navigazione e catalogo | Coordinatore unico di apertura; separazione I/O/writer; scansione progressiva; refresh; registro Save indipendente; apertura mirata e filtri | N1 | A→B→C corretto, errori terminali, selezione stabile durante batch, Save/Undo/chiusura conservati |
| N3 — pannelli commutabili | Contenitore Libreria/Esplora, tree, barra percorso/cronologia, menu, keyboard routing, IT/EN, layout compatto e focus, sincronizzazione con griglia/viewer | N2 | Switch senza reset del contesto e controlli Libreria conservati; workflow cartella→file→viewer→Indietro completo con mouse e tastiera; pixel del viewer invariati |
| N4 — preferiti e sessione | Migrazione libreria, API commit, export/backup, recenti e geometria versionati; DnD dei preferiti con alternativa da tastiera | N2–N3 | Riavvio e recovery verificati su copie temporanee; nessuna interferenza con Settings o annotazioni |
| N5 — aggiornamento e piattaforme | Watcher/reconciliation, mount/unmount, accesso negato, link, hidden, pacchetti e fallback documentati | N1–N4 | Nessuna rimozione da lista parziale; offline/ritorno corretti; nessun download implicito nelle configurazioni qualificate |
| N6 — qualifica integrata | Carico 100k, memoria bassa, viewer, XPC, shutdown, screenshot IT/EN e 200%, VoiceOver/NVDA e prove native Windows | N1–N5 | Matrice seguente con prove reali; mancanza hardware resta «non verificato», non «passato» |
| N7 — consegna | Documentare uso e limiti, bundle separato e rollback, rapporti con hash, PLAN/registro/sync | N6 | Artefatto identificato e verificabile, checklist aggiornata, nessuna qualifica di rilascio implicita |

**Taglio minimo dimostrabile:** N0–N3 permette già di navigare tra file/cartelle e commutare con il pannello Libreria attuale; lo switch è obbligatorio anche per questa verticale. Consegnarla eventualmente come parziale con le mancanze nominate. La richiesta completa comprende N4–N7: preferiti, refresh robusto e verifica non sono sostituibili da un albero statico.

Stima orientativa per una persona che conosce il codice: N0 1–2 giorni, N1 2–4, N2 3–5, N3 2–4, N4 2–3, N5 3–5, N6–N7 3–5; totale **16–28 giornate**, più eventuali blocchi nativi/accessibilità e 20–30% di riserva. È una stima di pianificazione, non un tempo misurato o una data promessa; rivederla dopo N0/N2. Nessun nuovo motore di rendering rientra nella stima.

## 11. Matrice di accettazione

Tutti i casi sotto sono **da implementare/eseguire**, non risultati della presente sessione.

| ID | Scenario | Esito richiesto |
|---|---|---|
| F01 | Avvio e aggiunta di una radice | Nessuna scansione ricorsiva; root raggiungibile e caricamento esplicito |
| F02 | Espandi/comprimi ramo con una foto nel viewer | Stessa foto, qualità, zoom e generazione decode; zero nuovi decode causati dal tree |
| F03 | Cartella con sottocartelle, immagini e TXT | Tutte le voci previste visibili, ordine stabile; soltanto immagini candidate nella griglia |
| F04 | Cartella vuota / solo TXT / filtri escludenti | Tre stati comprensibili; azioni per navigare o azzerare filtri |
| F05 | Singolo/doppio clic, tastiera e apertura mirata | Una sola navigazione; target corretto anche con filtri fotografici attivi |
| F06 | Indietro/Avanti/Su, duplicati e nuova diramazione | Cronologia corretta, foto/scroll recuperati per ID, nessun decode di vecchie posizioni |
| F07 | Dialogo/drop/CLI e selezione griglia→tree | Stesso coordinatore; nessun ciclo di feedback né furto del focus |
| F08 | Menu e trascinamento | Pin/ordine dei preferiti soltanto; nessuna rinomina/spostamento/cancellazione degli originali |
| F09 | Libreria→Esplora→Libreria in griglia/viewer/confronto | Stessi foto, selezione, filtri, zoom, qualità/override e generazione; nessuna nuova navigazione o scansione fotografica causata dallo switch |
| F10 | Rami aperti e scroll diversi, poi switch ripetuti | Stato proprio di ciascun pannello recuperato; tutti i controlli del pannello attuale ancora funzionanti |
| F11 | Cambia foto/cartella con Libreria visibile, poi apri Esplora | Esplora riflette la posizione condivisa senza riaprire la cartella o espandere inutilmente altri rami |
| F12 | Switch durante scansione e Save, poi chiusura/riavvio | Nessuna operazione attiva persa; ultima scheda ripristinata, default Libreria se assente |
| C01 | A→B→C con risposte/errori invertiti | Solo C modifica la griglia e il suo stato occupato |
| C02 | Espandi A, comprimi, riespandi con vecchio risultato in arrivo | Vecchia epoca scartata; B e viewer indipendenti |
| C03 | Refresh e cambio motore/qualità durante scansione | Target conservato; nessun risultato vecchio applicato e nessuno spinner permanente |
| C04 | Scansione 100k con Save/Undo già accettati | Writer disponibile tra batch; esiti delle annotazioni sempre ricevuti e durevoli |
| C05 | Selezione fatta mentre arrivano batch e sort finale | Selezione/ancora non saltano sulla prima riga; ordine finale deterministico |
| C06 | Errore parziale, cancellazione, quota ed overflow watcher | Nessuna falsa cancellazione; stato incompleto e rilettura disponibili |
| C07 | Chiusura durante listing lento e salvataggio | Nessun join infinito dell'esploratore; i salvataggi accettati conservano il proprio contratto |
| P01 | Preferiti salvati, riordinati, rinominati e riavvio | Commit confermato e dati identici; operazioni non toccano directory reali |
| P02 | Migrazione v1, crash, disco pieno e schema futuro | Backup verificato, nessuna libreria sovrascritta o falsa conferma |
| P03 | JSON sessione corrotto/grande/futuro e impostazioni in bozza | Recupero controllato e originale conservato; Settings non applicate né perse |
| P04 | Esporta/ripristina libreria con preferiti | Annotazioni e preferiti conservati; percorsi non risolti dichiarati |
| S01 | Permission denied, rimozione, ripristino e volume diverso con stesso nome | Errori locali, offline distinto da cancellato, nessuna riassociazione ambigua |
| S02 | Symlink, junction, ciclo, pacchetto, placeholder, FIFO | Policy rispettata; niente ricorsione/download/lettura del contenuto impliciti |
| S03 | Nomi non UTF-8/UTF-16 nativi, Unicode equivalente, case e percorsi lunghi | Nessuna collisione da display/casefold; round-trip e limiti dichiarati |
| S04 | Mac fuori bundle, guasto XPC e confine Windows | Visibilità distinta da decodificabilità; esterni mai decodificati con fallback non confinato |
| S05 | Espansione di cartelle fotografiche mai attivate | Zero hash/parser/decode/catalog.observe e nessuna nuova `.truerenderer-cache` |
| U01 | Popup, IME, tree e viewer con gli stessi tasti | Un solo destinatario; nessun rating accidentale; Esc non chiude anche il viewer |
| U02 | IT/EN a 1440×940, 1100×720 e 550×360 punti effettivi | Testi/azioni raggiungibili, overflow e pannello temporaneo corretti |
| U03 | Ridimensionamento pannello, Adatta e 1:1 fisico | Centro/scala validi, confronto pixel col riferimento del presenter |
| U04 | VoiceOver/NVDA, sola tastiera, focus fuori viewport | Gerarchia e stati annunciati, focus stabile, menu e riordino accessibili |
| U05 | Switch nel pannello temporaneo e a 200%, popup e campo testo focalizzati | Selettore sempre raggiungibile; una sola scheda riceve input, nessun focus nascosto o doppio Esc |
| L01 | 100k/oltre quota, 128 livelli, 200 preferiti, watcher saturi | Memoria/code/handle limitati, degradazione esplicita e nessun accumulo nascosto |
| L02 | Rete bloccata, due slot occupati, viewer e Save | UI e dati durevoli operativi; attesa dell'I/O dichiarata, niente crescita dei thread |
| L03 | Navigazione/scroll con viewer CPU/GPU e memoria ridotta | Obiettivi misurati separatamente; nessun peggioramento nascosto di pipeline e budget |

### 11.1 Come verificare l'implementazione futura

Unit test del reducer con eventi riordinati e clock controllato; enumeratore su fixture sintetiche e provider finto per errori/ritardi; integrazioni su filesystem reale per identità, link e permessi; catalogo con copie temporanee per migrazioni e annotazioni. Le prove simulate non qualificano cancellazione di syscall, volumi, cloud o screen reader.

Test egui con eventi reali di clic/tasti sul widget, oltre ai test di stato; screenshot nativi per layout e confronto pixel del viewer. DnD, dialoghi, rimozione del disco, screen reader e scala effettiva richiedono prova nell'app nativa.

Usare sempre `scripts/cargo-local.sh`. Per l'incremento completo eseguire i controlli pertinenti e `scripts/verify.sh`, con `--gui` per la prova nativa. Dopo modifiche a broker/decoder/bundle è obbligatorio `scripts/test-xpc-integration.py` sul pacchetto; includerlo comunque nella verifica del bundle consegnato, dato che cambia il ciclo di navigazione. Leggere gli argomenti correnti degli script prima dell'esecuzione.

Proposte di nuovi strumenti, da creare soltanto durante l'implementazione: generatore di fixture filesystem, smoke Esplora e rapporto `reports/filesystem-browser-macos.json` / `reports/filesystem-browser-windows.json`. I rapporti registrano hash dei binari, commit, OS/hardware, corpus sintetico, condizioni di cache, campioni, errori e casi saltati. Fotografie, percorsi personali, database e dump completi restano in aree locali escluse dalla pubblicazione.

## 12. Decisioni iniziali e rischi da risolvere

| Decisione proposta | Motivo / rischio da verificare |
|---|---|
| Un pannello sinistro con schede Libreria / Esplora, ultimo stato ricordato | Requisito esplicito del titolare; conserva il pannello corrente e consente lo switch in qualsiasi layout |
| File e cartelle nello stesso albero; tutti i file per default | Soddisfa l'esplorazione in stile IDE; modalità solo cartelle/immagini per ridurre la densità |
| Freccia espande, nome attiva; tastiera prima sposta focus | Evita scansioni e decode involontari durante l'esplorazione |
| Listing e catalogo separati; apertura fotografica resta integrata | Il loop attuale può ritardare salvataggi/domanda durante una scansione lunga |
| Preferiti nella libreria, sessione in file separato | Preferiti sono dati scelti dall'utente; richiede migrazione/backup reali |
| Refresh progressivo con riconciliazione per ID | Il reset attuale perde contesto; risultati parziali non provano assenze |
| Limiti espliciti invece di espansione globale | Percorsi lenti, nomi lunghi e molti nodi possono esaurire RAM e thread |
| Widget egui esistente con modello virtualizzato dedicato | Riduce dipendenze; semantica accessibile da validare nello spike, non presunta |
| Supporto Mac verificato prima della consegna locale, Windows verificato sul proprio host | I percorsi Windows/LPAC esistono, ma UI, filesystem e accessibilità non si qualificano per analogia |

Scelte aperte dopo N0: adattatori nativi esatti e minimi OS per volumi/watch/placeholder; gestione qualificata di alias/shortcut; eventuali limiti rilevati del toolkit per albero virtualizzato; valori finali di larghezza e budget sulla baseline. Ogni deviazione va motivata in questo documento e nel registro; modifiche ai contratti di sicurezza/durabilità richiedono una decisione architetturale esplicita.

## 13. Istruzioni per riprendere più avanti

1. Rileggere `AGENTS.md`, `PLAN.md`, `docs/avanzamento.md`, questo documento e le sezioni pertinenti dell'architettura. Verificare le differenze di codice intervenute dal 15 settembre 2026.
2. Confermare nel codice la mappa di §2 e registrare la baseline della build effettiva. Non trasformare le osservazioni di questa pianificazione in test già superati.
3. Avviare N0 su corpus/dati sintetici isolati, poi seguire N1→N7; conservare gli originali e il bundle precedente.
4. Usare le caselle in `PLAN.md` come tracciamento operativo e la matrice di §11 per gli esiti dettagliati. Segnare separatamente implementato, verificato, aperto e mancante.
5. Aggiornare `docs/avanzamento.md` ed eseguire `python3 scripts/sync-docs.py`; il registro e il link al progetto vengono riportati nelle due architetture. Il testo completo del navigatore resta in questo documento fino a una futura integrazione esplicita.

**Consegna di questa sessione:** specifica e piano documentale. Nessuna modifica al codice applicativo, ai database, agli originali o al bundle; nessuna nuova prova applicativa attribuita al navigatore.
