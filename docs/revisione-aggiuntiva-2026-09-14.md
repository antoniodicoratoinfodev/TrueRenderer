# Revisione aggiuntiva dopo le correzioni — 14 settembre 2026

**Aggiornamento successivo:** i due P2 della baseline seguente sono stati corretti su richiesta del titolare. [Correzioni e verifiche Windows](correzioni-gamma-orientamento.md), [nuovo rapporto](../reports/gamma-orientation-fixes-windows.json). Le evidenze negative dell'audit restano conservate.

**Due ulteriori P2 riprodotti, ancora aperti.** Questa verifica non modifica il codice applicativo e non esegue staging o commit. Laboratorio privato `var/audit-extra-20260914/`; [rapporto con risultati e hash](../reports/additional-review-windows.json).

## P2 — PNG con sola gAMA trattato come sRGB

In `crates/tr-worker/src/lib.rs:180–192`, `gAMA=45455` viene accettato come «gAMA sRGB dichiarato»; il raster passa poi attraverso `from_encoded_srgb` alla riga 390. Il chunk indica invece una gamma approssimativamente 1/2,2 e non identifica la curva sRGB a tratti. Quando manca un segnale prioritario, occorre rispettare la gamma dichiarata. Riferimento: [PNG Third Edition, §§11.3.2.2, 12.1 e 13.13](https://www.w3.org/TR/png-3/#11gAMA).

Riproduzione: PNG RGB8 1×1, sola `gAMA=45455`, nessun `sRGB`, `cICP`, ICC o primarie. Per il grigio `[16,16,16]`, il valore lineare calcolato con la curva dichiarata è `(16/255)^(100000/45455) ≈ 0,00226309`; tutti e tre i motori Windows restituiscono circa **0,00518151**. Per `[128,128,128]`: atteso circa **0,21952305**, osservato **0,21586043**. Le primarie sRGB restano assunte e separate dalla curva; sul neutro D65 il passaggio a Rec.2020 conserva il valore entro l'approssimazione della matrice.

I controlli con chunk `sRGB` e gli stessi campioni danno esattamente gli stessi pixel dei PNG con sola gamma: il decoder sta usando la stessa curva per due dichiarazioni differenti. Confermato col worker debug e release attraverso LPAC.

Correzione richiesta: applicare la funzione di trasferimento dichiarata, distinguendo gamma e primarie, oppure rifiutare esplicitamente questo percorso finché non supportato; correggere la provenienza e invalidare i raster precedenti. Aggiungere regressioni sui toni scuri, sui mezzitoni e sulla precedenza di `sRGB`. Questo difetto numerico è distinto dalle note diagnostiche già aperte su cICP malformato e sRGB/gAMA discordanti.

## P2 — Una directory TIFF secondaria può ruotare l'anteprima principale

In `crates/tr-worker/src/container.rs:135`, `orientation == 1` viene usato come se significasse «non ancora letto». Ma 1 è anche un orientamento esplicito valido. Un tag in una directory successiva può quindi sovrascriverlo; alla riga 184 il valore globale viene attribuito all'anteprima già scelta, anche se appartiene a un'altra directory.

Riproduzione: DNG LinearRaw sintetico con orientamento primario esplicito 1 e JPEG principale **8×4**, più una seconda IFD completa contenente una miniatura grayscale 1×1. Le due varianti differiscono per **un solo byte**, il tag Orientation della seconda IFD: 1 oppure 6. Il RAW e il JPEG principale restano identici. Cambiare soltanto il tag della miniatura fa passare l'anteprima principale da **8×4 / NoTransforms** a **4×8 / Rotate90**, alterando anche i pixel. Confermato nel motore bilineare sia diretto sia attraverso worker LPAC debug/release; gli altri motori rifiutano questo CFA come previsto.

Correzione richiesta: distinguere orientamento assente da orientamento 1 e mantenere la relazione fra directory, immagine e anteprima scelta. Aggiungere una regressione con immagine non quadrata, orientamento primario 1 e orientamento diverso nella miniatura; verificare anche primario assente e JPEG con orientamento proprio. Aggiornare la compatibilità cache pertinente quando cambia il raster.

## Perimetro ed evidenze

- Riletti resolver PNG/TIFF, selezione dello stadio RAW, parser del contenitore, gestione delle dimensioni/orientamento, rinnovo e pubblicazione cache, LRU, writer e confine del mosaico. Non è un nuovo audit completo del codice upstream LibRaw o della piattaforma Mac.
- Sei fixture sintetiche, tre motori: **18 casi col worker debug e 18 col worker release**, ciascuno con probe/decode e confronto col decoder diretto. Sono riproduzioni, non 36 test di accettazione superati.
- Confermati input invariati, differenza di un solo byte fra i DNG, hash dei sorgenti e dei quattro binari identici al [rapporto delle ultime correzioni](../reports/general-review-fixes-windows.json). I 103 test, le build e le altre prove di quel rapporto non sono stati rieseguiti: restano attribuiti agli stessi sorgenti/binari verificati.
- Nessuna modifica applicativa; registro e piano aggiornati, architetture sincronizzate preservando il testo esterno ai blocchi gestiti. Report precedenti, LICENSE, vendor, backup originale e indice Git conservati.

Restano distinti i limiti già aperti: diagnostica PNG, conversione TIFF/ICC, contesa writer, Windows pulito, GUI/corpus esteso e qualifiche colore/prestazioni. Mac/XPC rimane rinviato dal titolare. Le ultime tre correzioni non sono annullate da questi rilievi; i due casi aggiuntivi richiedono interventi propri. Nessun gate R0–R4 chiuso.
