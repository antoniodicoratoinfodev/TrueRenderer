# Correzioni della revisione serale — 14 settembre 2026

Ripresa la sessione del 13 settembre dopo la richiesta esplicita di correggere i quattro rilievi della [revisione continuata](revisione-continuata-2026-09-13.md). Le modifiche applicative e cinque nuove regressioni erano già salvate; l'interruzione era avvenuta prima delle integrazioni e della prova nativa. Nessun commit o staging. Laboratorio privato: `var/review-followup-fixes/`, con baseline, script e log; le evidenze negative precedenti restano conservate.

## Correzioni applicate

1. **Test worker su Mac:** il test di crash/recovery usa un `match` esaustivo su `Owner`, con rifiuto esplicito della variante XPC inattesa nel test delle pipe. Il test non è stato rimosso. La compilazione nativa Mac e la suite XPC rimangono rinviate dal titolare; la prova minima con entrambe le varianti non le sostituisce.
2. **Applica e salva:** manutenzione cache e scansione hanno stati distinti. Il completamento della manutenzione non azzera una scansione effettiva; salvare le preferenze non inventa un rescan. Pulsante e smoke nativo condividono `start_cache_action()`. Due regressioni del servizio/UI verificano foto diversa dalla prima, selezione multipla, zoom/centro, persistenza delle impostazioni, ritorno al primo motore, svuotamento cache e scansione realmente pendente.
3. **PNG cICP:** riconosciuto sRGB a range completo `[1,13,0,1]`, con provenienza dichiarata. Primarie/trasferimenti/range non supportati vengono rifiutati prima dei fallback sRGB; i conflitti rilevati sono espliciti. Le prove comprendono lineare, P3, PQ/HLG, range limitato e dichiarazioni concorrenti. Questo non implementa la conversione di tali spazi né Little CMS.
4. **TIFF alpha associata:** risolto `ExtraSamples` prima della trasformazione colore. RGB viene deassociato nel dominio dei campioni, convertito e premoltiplicato una sola volta in lineare; alpha zero produce RGBA zero. Alpha ambigua è rifiutata. Prove RGB a 8/16 bit, little/big endian, bianco equivalente e pixel colorati; gray+alpha resta esplicitamente non supportato dall'adattatore corrente.

La ricetta bitmap `bitmap-cicp-alpha-v3` entra nella provenienza e nel fingerprint della cache, escludendo i raster precedenti con interpretazione errata. Aggiunta dipendenza diretta dalla versione TIFF già usata da `image`; nessun sorgente LibRaw upstream modificato.

## Verifiche

- Recuperati e controllati i log delle correzioni: **86 test Rust ordinari**, build debug, formattazione e Clippy su tutti i target passati.
- Completate qui **6 integrazioni** con worker reale e **21 richieste sintetiche PNG/TIFF attraverso LPAC**, su tutti e tre i motori Windows. TIFF associato/non associato bit-exact, `cICP` supportato dichiarato e non supportato rifiutato, input sintetici invariati.
- Passati **6 controlli IPC**, **2 regressioni Python**, verifica dei **106 hash LibRaw / 79 unità compilate / 3 ricette**.
- Passati build release, controllo finale di formattazione e compilazione della prova minima del pattern Mac con entrambe le varianti. Quest'ultima verifica l'esaustività del codice estratto, non i target o il linkage Mac.
- **Prova nativa release passata:** due copie private dello stesso D40 autorizzato, seconda foto selezionata, zoom 1:1 e centro conservati in quattro stadi (bilineare, AHD, TrueRenderer, bilineare). Il cambio durante la scansione iniziale è incluso. Lo smoke esegue la stessa azione del pulsante, senza automatizzare un clic fisico. **577.296 pixel** nelle quattro regioni fotografiche GPU rispettano la soglia di **1 livello sRGB8** contro CPU; esclusi overlay, compositor e monitor. Sorgente e copie immutate, database privati integri.
- Passati **24 segnali di ricampionamento e identità 1:1**. Il file privato mantiene il nome storico `resampling-macos.json`, ma l'esecuzione è Windows.
- Nuova prova LPAC positiva: sentinella inaccessibile in lettura/scrittura, codice leggibile ma cartella non scrivibile, figli negati, token LPAC verificato. Winsock fallisce all'inizializzazione con 10107: nessuna nuova garanzia universale di isolamento o rete.
- Rigenerato l'inventario Windows con il Cargo.lock corrente: **205 package e 348 notice**, invariati nei contenuti delle licenze. LICENSE, sorgenti upstream, backup originale e testo delle architetture esterno ai blocchi gestiti preservati.

[Riepilogo con hash e provenienza delle prove](../reports/review-followup-fixes-windows.json).

**Limite cache osservato:** durante lo stadio TrueRenderer una persistenza è stata saltata per contesa del lock (errore Windows 33). La visualizzazione è proseguita; al ritorno al bilineare si osservano un hit e un nuovo decode del dettaglio. Non si dichiara quindi riuso completo senza decode né persistenza garantita per questa sequenza. La contesa del writer e i limiti di persistenza del dettaglio restano da qualificare separatamente; non sono una prova di perdita di originali o annotazioni.

Non ripetuta la precedente campagna di 156 sviluppi Nikon: quei risultati restano attribuiti ai rispettivi binari. Restano aperti nuovo bundle Mac/XPC, installazione Windows pulita, colore misurato, display, corpus esteso e prestazioni. Nessun gate R0–R4 chiuso; Standard/Riferimento restano indisponibili.
