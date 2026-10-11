# Banco X-Trans e candidato direzionale DR1

Contratto di ricerca di [ADR 0012 §14](progetto-truerenderer-experimental.md#14-implementazione-propria-e-miglioramento-misurabile). Metriche e soglie sono fissate nel [protocollo v1](../crates/tr-raw/tests/fixtures/research-v1.json). Il modulo camera RGB della baseline mantiene ordine aritmetico, vicini, pesi e siti misurati di E8; il confronto bit per bit precede gli esperimenti. La matrice colore e l'orientamento si applicano dopo la ricostruzione e non partecipano alla misura analitica.

## Formulazione propria DR1

Per ogni sito R/B, considerare quattro direzioni (orizzontale, verticale, due diagonali). Cercare il verde più vicino in ciascun verso fino a distanza 3 lungo la direzione, e il campione dello stesso colore del centro fino a distanza 6. Usare soltanto coppie interamente presenti nell'immagine; nessuna riflessione che cambi fase CFA. Indicando con a,b le distanze sinistra/destra, l'interpolante al centro è `(b*L+a*R)/(a+b)`.

Per una direzione con entrambe le coppie, Gd è l'interpolante dei verdi più `(C0−interpolante_C)*aG*bG/(aC*bC)`. La correzione compensa una curvatura locale comune, senza supporre distanze identiche. La confidenza è l'inverso del quadrato di `0.002+|GR−GL|/(aG+bG)+|C0−interpolante_C|/(aC*bC)`. Il verde ricostruito è la media dei Gd pesata dalle confidenze. Se nessuna coppia è disponibile, usare il verde della baseline. Il valore 0.002 è una regolarizzazione fissa nella scala lineare del protocollo, non un modello di rumore misurato della camera.

Il residuo R−G/B−G si interpola sui vicini della baseline (offset [-3,3], distanza quadratica <=10), con peso `1/d²/(1+64*|G0−Gvicino|)`, usando il nuovo verde. Il canale misurato al centro viene sempre conservato. Nessuna saturazione, denoise o curva di tono. La formulazione è del progetto; nessun codice RI/ARI/Markesteijn viene importato e non si attribuisce al candidato l'efficacia di quegli algoritmi.

## Scene e criteri

Scene generate a risoluzione 192×144, con tutte le 36 traslazioni del CFA, scala ed esposizione fisse e bordi misurati separatamente. Le trame sono sinusoidi lineari in direzioni e frequenze fissate dal protocollo; quelle cromatiche hanno canali con ampiezze e fasi distinte. Bordi obliqui includono transizioni neutre e cromatiche. Campi uniformi includono negativi, valori sopra uno e rumore deterministico a media zero. Il rumore è applicato ai soli campioni acquisiti; la verità di riferimento resta il segnale prima del rumore. Questi segnali matematici non rappresentano l'intera statistica delle fotografie.

Misurare RMSE dei canali mancanti, RGB al bordo, luminanza camera (media aritmetica dei tre canali), differenze finite della luminanza e, soltanto sulle trame acromatiche, RMS di R−G/B−G. Conservare somme dei quadrati e conteggi per categoria/fase, senza mediare rapporti di casi con denominatore nullo. Siti misurati confrontati come bit float rispetto al mosaico (anche in presenza di rumore); tutti gli output devono essere finiti. Soglia assoluta 1e-6 per rapporti instabili vicino a zero.

La verifica usa frequenze, angoli e seed distinti dallo sviluppo. Nessuna modifica di DR1 dopo aver visto la verifica; un candidato successivo necessita di nuovo holdout. Riportare anche la fase peggiore. Per proporre la selezione nel prodotto occorrono tutti i gate sintetici e una verifica fotografica indipendente: un numero migliore su sinusoidi, un'immagine più nitida o la somiglianza a un motore concorrente non bastano. Gli esiti della campagna appartengono al rapporto e a STATO.md.
