# Restyling desktop — 15 settembre 2026

Proposta approvata dal titolare: gerarchia fotografica, controlli compatti, superfici neutre e preferenze per argomento. Implementazione sul frontend egui esistente. Stato e verifiche nel registro di avanzamento.

## Composizioni di riferimento

Griglia: barra Apri / viste / motore RAW applicato / ricerca / Settings / Menu, con pulsanti allineati; navigazione e filtri a sinistra, sole immagini al centro, cartella e conteggi nella barra inferiore; anteprima, istogramma e sezioni richiudibili a destra. Nome e valutazione sotto ogni fotografia, contorno di selezione neutro, nessuna alterazione dei pixel per indicare selezione.

Viewer: stessa barra principale, con seconda riga superiore per Adatta / 1:1 / zoom / Globale: Standard/Piena / Solo questa foto: Standard/Piena, divisa in due righe alle larghezze ridotte; nessuna intestazione o barra strumenti nel pannello centrale. Immagine e filmstrip; ispettore con anteprima duplicata inizialmente chiusa e istogramma disponibile. Le eccezioni per foto sono bidirezionali e di sessione, eliminabili con Usa la qualità globale; un cambio globale le azzera. L'indicatore RAW riflette il motore applicato, non una bozza delle preferenze; il calcolo CPU/GPU del viewer resta distinto in Prestazioni.

Preferenze: titolo e quattro schede (Generale, Anteprime e RAW, Prestazioni, Cache e dati), corpo scorrevole indipendente, piè di finestra con stato delle modifiche, Ripristina modifiche, Chiudi, Applica e salva. La lingua mantiene la propria applicazione immediata; nessun cambio scheda applica le altre preferenze in bozza.

## Regole

- Scala di spaziatura 4/8/12/16/24; titoli 20, testo 13, metadati 12 punti logici. Palette neutra, focus distinto e leggibile.
- Stile condiviso e preferenze estratti in moduli del frontend; nessuna nuova dipendenza grafica.
- Nomi lunghi con ellissi e testo completo nel tooltip; identificatori UI stabili al cambio lingua.
- Controlli secondari in menu, statistiche e spiegazioni tecniche espandibili; stato Preview e catena colore sempre rintracciabili.
- Confronti grafici in inglese/italiano, larghezza minima e scala 200%; test di bozze, azioni e geometria delle finestre. Prove di campionamento distinte dalla valutazione estetica.
- Le nuove icone SVG complete, la qualifica VoiceOver/NVDA e gli altri gate v1 rimangono attività esplicite della roadmap; questo incremento usa i controlli egui e simboli già disponibili.

## Verifica riproducibile

`scripts/cargo-local.sh test --offline -p truerenderer ui::settings_regressions` verifica preferenze in bozza, applicazione completa, selezione/zoom, persistenza della lingua e layout delle quattro schede in entrambe le lingue a 1440×940, 1100×720 e 550×360 punti. L'ultimo spazio logico rappresenta la finestra minima con UI al 200%; il test attende l'assestamento del layout egui e controlla anche il clipping dei pulsanti finali.

Il flag diagnostico `--restyle-smoke --root <root-sintetica> --data <dati-temporanei>` acquisisce dodici schermate native: quattro schede per lingua, finestra minima nelle due lingue e scala UI 200% nelle due lingue. Cambia lingua e dimensioni soltanto nella sessione di prova; usare dati separati perché la lingua viene salvata dal percorso reale. La root deve contenere una copia del corpus e `reports/`. `--sampling-smoke` e `--verify-sampling-screenshots` controllano separatamente griglia, viewer e confronto contro i raster attesi.

La revisione della barra estende la suite a sette test UI: include clic su Open/Menu/Settings e la precedenza della tastiera dei popup. Esc deve chiudere il menu senza cambiare la vista fotografica. Rapporto della consegna e hash in `reports/toolbar-macos.json`; schermate e risultati completi in `var/toolbar/reports/`.

Per verificare una copia separata del bundle senza sovrascrivere i rapporti storici, `scripts/test-xpc-integration.py --bundle dist/TrueRenderer-restyle.app --report-root <root-sintetica>` conserva anche gli esiti XPC nella root di prova. Il bundle precedente e i database dell'utente non vengono sostituiti.
