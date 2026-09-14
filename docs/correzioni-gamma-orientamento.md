# Correzioni gamma PNG e orientamento RAW — 14 settembre 2026

**Corretti i due P2 della [revisione aggiuntiva](revisione-aggiuntiva-2026-09-14.md), su richiesta del titolare.** Nessun commit/staging; laboratorio privato `var/fix-extra-20260914/`. Le evidenze negative precedenti sono conservate.

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
