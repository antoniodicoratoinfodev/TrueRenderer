# Revisione e correzioni gamma PNG / orientamento RAW — 14 settembre 2026

Documento unico di audit e correzione. I rilievi sotto sono la baseline negativa storica; le correzioni successive sono verificate su Windows e incluse in `67146e3`. La qualifica nativa Mac/XPC resta aperta. I rapporti JSON conservano hash ed esiti delle singole esecuzioni.

## Audit prima delle correzioni

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

<a id="correzioni"></a>

## Correzioni ed esiti successivi

## Comportamento

- **PNG con sola gamma supportata:** `gAMA=45455` applica ora la curva di potenza con esponente `100000/45455`, prima della matrice verso Rec.2020 e della premoltiplicazione. Alpha rimane lineare. La provenienza distingue la curva dichiarata e applicata dalle sole primarie sRGB assunte, tramite `ColorSource::AssumedPrimaries`; non attribuisce fedeltà misurata all'assunzione. Il grigio 16/255 torna a circa **0,00226309**, invece di 0,00518151. `sRGB` e `cICP` prioritari mantengono la curva sRGB a tratti. Le altre gamma e le primarie dichiarate non supportate continuano a essere rifiutate: non è un'implementazione generale di gestione colore.
- **Orientamento delle anteprime RAW:** ogni IFD conserva il proprio orientamento. L'anteprima scelta usa il valore della propria directory, oppure quello della primaria quando manca; directory estranee non possono modificarlo. `Some(1)` significa esplicitamente nessuna rotazione, mentre `None` permette di usare EXIF del JPEG. Il caso dell'audit resta **8×4** anche quando la miniatura secondaria dichiara orientamento 6. Una directory effettivamente selezionata conserva invece il proprio orientamento, anche se diverso dalla primaria.
- **Compatibilità cache:** `bitmap-gamma-ifd-v5` entra nel fingerprint comune delle cache bitmap/anteprima RAW ed esclude i raster precedenti con interpretazione errata. Nessuna cancellazione di originali, cataloghi o backup.

## Verifiche

- **99 test ordinari + 8 integrazioni = 107 passati**, con toolchain locale offline; fmt e Clippy con warning negati passati.
- Tre nuove regressioni ordinarie: curva PNG a 8/16 bit e alpha, precedenza sRGB/cICP; IFD secondaria irrilevante e fallback EXIF JPEG con primario assente/1/6; orientamento della IFD selezionata con fallback alla primaria.
- Nuova integrazione col worker LPAC: le due varianti DNG restituiscono la stessa anteprima 8×4 e gli stessi pixel; sorgenti invariate. Le due integrazioni fotografiche dipendenti dal corpus privato non sono state rieseguite.
- Build debug/release e **18 casi sintetici per ciascun worker** sui sei file dell'audit, con controllo dei valori gamma e delle dimensioni/pixel RAW. Rifiuti degli altri motori per il DNG non Bayer confermati.
- **Sei controlli IPC, due regressioni Python, 24 segnali di ricampionamento e identità 1:1** passati. LibRaw invariato: 106 hash upstream, 79 unità compilate, tre ricette. Cargo.lock/inventario Windows e 348 notice coerenti, nessuna nuova dipendenza.
- Conservati report precedenti, sorgenti non pertinenti, LICENSE, vendor, backup originale e indice Git. Piano/registro aggiornati e architetture sincronizzate senza cambiare il testo esterno ai blocchi gestiti.

[Rapporto con hash ed esiti](../reports/gamma-orientation-fixes-windows.json). Il riferimento della curva gAMA rimane la [PNG Third Edition, §11.3.2.2 e gestione gamma](https://www.w3.org/TR/png-3/#11gAMA).

La suite XPC è stata invocata ma non può partire su Windows senza `/usr/bin/codesign`; nuovo bundle e verifica Mac restano rinviati dal titolare. GUI e campagna Nikon non ripetute. Restano diagnostica PNG malformata/conflittuale, TIFF/ICC non supportati, contesa writer, Windows pulito e qualifiche generali colore/display/prestazioni. Nessun gate R0–R4 chiuso; Standard/Riferimento indisponibili.
