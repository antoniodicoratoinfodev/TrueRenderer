//! Interface translations. Source keys stay separate from file names, annotations and IPC.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Language {
    #[default]
    #[serde(rename = "en")]
    English,
    #[serde(rename = "it")]
    Italian,
}

impl Language {
    pub fn text(self, source: &str) -> &str {
        if self == Self::Italian {
            return source;
        }
        match source {
            "Sviluppo" => "Develop",
            "Caricamento ricetta…" => "Loading edit recipe…",
            "Riprova salvataggio" => "Retry save",
            "Annulla sviluppo" => "Undo edit",
            "Ripeti sviluppo" => "Redo edit",
            "Prima/Dopo" => "Before/After",
            "Prima" => "Before",
            "Modificata" => "Edited",
            "In calcolo" => "Rendering",
            "Errore" => "Error",
            "Verifica resa finale" => "Verify final rendering",
            "Resa finale alla risoluzione nativa" => "Final rendering at native resolution",
            "Ricetta salvata nella libreria" => "Recipe saved in library",
            "Modifiche in corso" => "Editing in progress",
            "Luce" => "Light",
            "Esposizione" => "Exposure",
            "Luminosità" => "Brightness",
            "Contrasto" => "Contrast",
            "Alte luci" => "Highlights",
            "Ombre" => "Shadows",
            "Bianchi" => "Whites",
            "Neri" => "Blacks",
            "Curva tonale" => "Tone curve",
            "Mezzitoni curva" => "Curve midtones",
            "Colore del render · correzione RGB relativa" => {
                "Rendered color · relative RGB correction"
            }
            "Temperatura RGB" => "RGB warmth",
            "Tinta RGB" => "RGB tint",
            "Saturazione" => "Saturation",
            "La correzione RGB non cambia il WB RAW del decoder." => {
                "RGB correction does not change the decoder's RAW white balance."
            }
            "Azzera correzione RGB" => "Reset RGB correction",
            "Neutralizza campione RGB" => "Neutralize RGB sample",
            "Verifica la resa finale e campiona la vista modificata." => {
                "Verify final rendering and sample the edited view."
            }
            "Sviluppo originale" => "Original development",
            "Vista modificata provvisoria; export alla risoluzione nativa." => {
                "Provisional edited view; export at native resolution."
            }
            "Prima · sviluppo originale" => "Before · original development",
            "Calcolo regolazioni · originale provvisorio" => {
                "Calculating edits · original shown provisionally"
            }
            "Anteprima modificata provvisoria · export nativo" => {
                "Provisional edited preview · native export"
            }
            "Motore salvato per foto · clamp nei formati interi, nessun dither" => {
                "Saved engine per photo · integer formats clamp, no dither"
            }
            "Motore salvato nella ricetta della foto." => "Engine saved in this photo's recipe.",
            "Attendere il caricamento o salvataggio delle ricette selezionate." => {
                "Wait for selected recipes to load or save."
            }
            "DNG lineare: sviluppo tecnico senza regolazioni fotografiche; usare JPEG, PNG o TIFF per la resa modificata." => {
                "Linear DNG: technical development without photographic edits; use JPEG, PNG, or TIFF for the edited result."
            }
            "DNG RAW conserva il mosaico: le regolazioni fotografiche non vengono applicate." => {
                "RAW DNG preserves the mosaic; photographic edits are not applied."
            }
            "Per riaprire i DNG esportati scegliere LibRaw bilineare/AHD. Apple RAW e il motore mosaico TrueRenderer non sono compatibili con questi file." => {
                "To reopen exported DNG files, select LibRaw bilinear/AHD. Apple RAW and the TrueRenderer mosaic engine are not compatible with these files."
            }
            "FITS · dati scientifici" => "FITS · scientific data",
            "Prima immagine 2D idonea, sola lettura. Selezione di altri HDU, cubi, tabelle, compressione e WCS non disponibili." => {
                "First eligible 2D image, read-only. Other HDU selection, cubes, tables, compression and WCS are unavailable."
            }
            "Istogramma scalare del piano nativo completo, prima dello stretch; min/max nelle unità dichiarate." => {
                "Scalar histogram of the full native plane before stretch; min/max in declared units."
            }
            "Trasformata di vista (globale FITS)" => "View transform (global FITS)",
            "Nero/bianco nella scala min–max normalizzata; non modifica i campioni. Piano costante → grigio medio, invalidi → sfondo." => {
                "Black/white in the normalized min–max scale; does not change samples. Constant plane → middle gray, invalid → background."
            }
            "Nero" => "Black",
            "Bianco" => "White",
            "Reset · min/max lineare" => "Reset · linear min/max",
            "Proxy fp32 e copertura separata; media sui validi, poi stretch. Calcolo FITS su CPU anche in modalità GPU; presentazione 8/10/16 float comune. Non fotometria." => {
                "fp32 proxy and separate coverage; average valid values, then stretch. FITS processing uses CPU even in GPU mode; shared 8/10/16-float presentation. Not photometry."
            }
            "Campione nativo (lettura isolata su richiesta)" => {
                "Native sample (isolated read on request)"
            }
            "Leggi valore originale" => "Read original value",
            "Coordinate 0-based; asse 1 orizzontale, asse 2 verso il basso. Il campione proviene dai byte originali, non da mip/stretch. Valore fisico calcolato in f64 una sola volta; NaN/Inf/BLANK distinti." => {
                "0-based coordinates; axis 1 horizontal, axis 2 downward. Sample comes from original bytes, not mip/stretch. Physical value calculated once in f64; NaN/Inf/BLANK distinguished."
            }
            "Esporta fotografie…" => "Export photos…",
            "Esporta fotografie" => "Export photos",
            "Nuovi file: nessun originale o export esistente viene sostituito." => {
                "New files: originals and existing exports are never replaced."
            }
            "Qualità JPEG" => "JPEG quality",
            "sRGB 8 bit, compressione con perdita. Trasparenza composta su bianco." => {
                "8-bit sRGB, lossy compression. Transparency composited onto white."
            }
            "sRGB, alpha conservata. La compressione non cambia la qualità." => {
                "sRGB, alpha preserved. Compression does not change quality."
            }
            "Veloce" => "Fast",
            "Bilanciata" => "Balanced",
            "Compatta" => "Compact",
            "RGB Rec.2020 lineare già sviluppato, 16 bit interi. Non conserva negativi o valori oltre 1. Trasparenza non supportata." => {
                "Developed linear Rec.2020 RGB, 16-bit integer. Does not preserve negatives or values above 1. No transparency."
            }
            "Riapertura: scegliere LibRaw bilineare/AHD. Apple RAW e motore mosaico TrueRenderer non supportano questo DNG lineare." => {
                "To reopen, select LibRaw bilinear/AHD. Apple RAW and TrueRenderer mosaic engines do not support this linear DNG."
            }
            "sRGB ICC, 16 bit interi, alpha conservata; non compresso, clamp [0,1]." => {
                "sRGB ICC, 16-bit integer, alpha preserved; uncompressed, clamp [0,1]."
            }
            "Rec.2020 lineare ICC, RGBA float32 con alpha associata. Conserva negativi e valori oltre 1 del render; non compresso. Non è un RAW sensore né un export FITS." => {
                "Linear Rec.2020 ICC, float32 RGBA with associated alpha. Preserves negative and above-1 rendered values; uncompressed. Not sensor RAW or FITS export."
            }
            "Mosaico area attiva: Nikon D750/D40. Nessun demosaic, WB applicato o ridimensionamento." => {
                "Active-area mosaic: Nikon D750/D40. No demosaic, applied WB or resize."
            }
            "Conserva campioni, CFA, nero/bianco, WB e orientamento; matrice D65 LibRaw. Non include margini ottici, MakerNotes, EXIF/GPS o NEF compresso: conservare l'originale. Altre camere vengono rifiutate." => {
                "Preserves samples, CFA, black/white levels, WB and orientation; LibRaw D65 matrix. Excludes optical margins, MakerNotes, EXIF/GPS and compressed NEF: keep the original. Other cameras are rejected."
            }
            "Lato lungo (0 = originale)" => "Long edge (0 = original)",
            "Cartella destinazione…" => "Destination folder…",
            "Esporta selezione" => "Export selection",
            "Annulla export" => "Cancel export",
            "Annullato. I file già salvati restano nella destinazione." => {
                "Cancelled. Files already saved remain in the destination."
            }
            "TIFF · Rec.2020 lineare float32" => "TIFF · linear Rec.2020 float32",
            "DNG lineare · RGB sviluppato 16 bit" => "Linear DNG · developed 16-bit RGB",
            "DNG RAW · mosaico area attiva originale" => "RAW DNG · original active-area mosaic",
            "Precisione di presentazione SDR" => "SDR presentation precision",
            "SDR 8 bit · compatibile" => "SDR 8-bit · compatible",
            "SDR 10 bit · sperimentale" => "SDR 10-bit · experimental",
            "SDR 16 float · sperimentale" => "SDR 16-float · experimental",
            "Applicare e riavviare l'app. La coppia formato + sRGB deve essere supportata; altrimenti rimane SDR 8 bit. Non abilita HDR né certifica i bit del monitor." => {
                "Apply and restart the app. The format + sRGB pair must be supported; otherwise SDR 8-bit remains. Does not enable HDR or certify display bit depth."
            }
            "16 float aumenta memoria di texture/superficie; precisione non uniforme, diversa da 16 bit interi. Working e cache restano fp32, anche con calcolo CPU." => {
                "16-float uses more texture/surface memory; nonuniform precision differs from 16-bit integer. Working data and cache remain fp32, including CPU processing."
            }
            "Superficie effettiva e capacità" => "Effective surface and capabilities",
            "Caricamento cartella" => "Folder loading",
            "Conteggio file…" => "Counting files…",
            "Cartella" => "Folder",
            "Annullato" => "Cancelled",
            "Preparazione delle anteprime…" => "Preparing previews…",
            "Preparazione delle miniature, non del dettaglio 1:1." => {
                "Preparing thumbnails, not full 1:1 detail."
            }
            "Preparazione annullata" => "Preparation cancelled",
            "Lettura della cartella incompleta" => "Folder scan incomplete",
            "In attesa delle viste attive o di memoria disponibile." => {
                "Waiting for active views or available memory."
            }
            "Preparazione in pausa" => "Preparation paused",
            "Continua in background" => "Continue in background",
            "Completa in primo piano" => "Finish in foreground",
            "Riprendi" => "Resume",
            "Pausa" => "Pause",
            "All'apertura della cartella" => "When opening a folder",
            "Carica le anteprime in background" => "Load previews in background",
            "Prepara prima la cartella con popup" => "Prepare the folder first with a popup",
            "La scelta si applica dopo Applica e salva. Il popup può continuare in background." => {
                "Takes effect after Apply and save. The popup can continue in background."
            }
            "Esplora" => "Explorer",
            "Navigazione" => "Navigation",
            "Pannello" => "Panel",
            "Preferiti" => "Favorites",
            "Recenti" => "Recent",
            "Posizioni" => "Locations",
            "Indietro" => "Back",
            "Avanti" => "Forward",
            "Cartella superiore" => "Parent folder",
            "Inserisci un percorso" => "Enter a location",
            "Filtra nomi nei rami caricati" => "Filter names in loaded branches",
            "Tutti i file" => "All files",
            "Cartelle e immagini" => "Folders and images",
            "Solo cartelle" => "Folders only",
            "Mostra" => "Show",
            "Mostra nascosti" => "Show hidden items",
            "Svuota recenti" => "Clear recent locations",
            "Memorizza recenti" => "Remember recent locations",
            "Aggiungi posizione…" => "Add location…",
            "Foto corrente" => "Current photo",
            "Aggiungi cartella corrente" => "Add current folder",
            "Rinomina preferito" => "Rename favorite",
            "Sposta su" => "Move up",
            "Sposta giù" => "Move down",
            "Rimuovi preferito" => "Remove favorite",
            "Salva" => "Save",
            "Annulla" => "Cancel",
            "Espandi o comprimi" => "Expand or collapse",
            "Ramo non letto" => "Branch not loaded",
            "Apri destinazione" => "Open location",
            "Aggiungi ai preferiti" => "Add to favorites",
            "Rileggi questo ramo" => "Refresh this branch",
            "Copia percorso" => "Copy path",
            "Mostra nel sistema" => "Reveal in Finder / File Explorer",
            "Albero file e cartelle" => "Files and folders tree",
            "Filtri sospesi per apertura diretta" => "Filters suspended for direct opening",
            "Ripristina filtri" => "Restore filters",
            "Filtri fotografici attivi" => "Photo filters active",
            "Mostra filtri" => "Show filters",
            "Apertura della posizione…" => "Opening location…",
            "Rilettura della cartella…" => "Refreshing folder…",
            "Anteprima non disponibile per questo tipo di file" => {
                "Preview unavailable for this file type"
            }
            "Collegamento: usa Apri destinazione nel menu" => "Link: use Open location in the menu",
            "Coda filesystem occupata" => "Filesystem queue busy",
            "La foto richiesta non è disponibile nell'elenco" => {
                "Requested photo is unavailable in this listing"
            }
            "Globale" => "Global",
            "Qualità globale delle anteprime" => "Global preview quality",
            "Cambia tutte le foto e azzera le eccezioni di sessione." => {
                "Changes all photos and resets per-photo session overrides."
            }
            "Usa la qualità globale" => "Use global quality",
            "Solo la foto corrente · eccezione per questa sessione." => {
                "Current photo only · override for this session."
            }
            "Motore selezionato e applicato. Il calcolo CPU/GPU del viewer si configura in Prestazioni." => {
                "Selected and applied RAW engine. Configure viewer CPU/GPU processing in Performance."
            }
            "Questa scelta riguarda il ricampionamento del viewer. Il motore RAW si sceglie in Anteprime e RAW." => {
                "This controls viewer resampling. Choose the RAW engine in Previews and RAW."
            }
            "Apri cartella…" => "Open folder…",
            "Apri immagini della cartella; su macOS il bundle usa decoder isolati." => {
                "Open images from a folder; the macOS bundle uses isolated decoders."
            }
            "Apri file…" => "Open file…",
            "Rileggi cartella" => "Refresh folder",
            "Griglia" => "Grid",
            "Anteprima" => "Preview",
            "Confronto" => "Compare",
            "Guida e stato del prototipo" => "Prototype help and status",
            "Impostazioni" => "Settings",
            "Cerca nome o parola chiave" => "Search name or keyword",
            "LIBRERIA" => "LIBRARY",
            "Corpus di prova" => "Test corpus",
            "SELEZIONE" => "SELECTION",
            "Tutte le immagini" => "All images",
            "Da conservare" => "Keepers",
            "Cinque stelle" => "Five stars",
            "Scartate" => "Rejected",
            "FILTRI" => "FILTERS",
            "Valutazione minima" => "Minimum rating",
            "Tutte le etichette" => "All labels",
            "Azzera filtri" => "Reset filters",
            "DATI LOCALI" => "LOCAL DATA",
            "Annulla modifica" => "Undo edit",
            "Cmd/Ctrl + Z · crea una nuova revisione" => "Cmd/Ctrl + Z · creates a new revision",
            "Crea backup" => "Create backup",
            "Esporta annotazioni…" => "Export annotations…",
            "JSON con annotazioni e percorsi locali" => "JSON with annotations and local paths",
            "MOTORE" => "ENGINE",
            "SDR / sRGB di sistema" => "SDR / system sRGB",
            "Anteprima di sviluppo" => "Development preview",
            "Gli originali rimangono intatti. Le annotazioni sono nella libreria locale." => {
                "Originals remain untouched. Annotations are stored in the local library."
            }
            "ISPEZIONE" => "INSPECTOR",
            "Seleziona un'immagine" => "Select an image",
            "ANTEPRIMA" => "PREVIEW",
            "ANTEPRIMA NON DISPONIBILE" => "PREVIEW UNAVAILABLE",
            "Dimensioni" => "Dimensions",
            "Non decodificato" => "Not decoded",
            "Formato" => "Format",
            "Dimensione" => "Size",
            "VALUTAZIONE" => "RATING",
            "Scarta · X" => "Reject · X",
            "PAROLE CHIAVE" => "KEYWORDS",
            "paesaggio, studio, colore" => "landscape, studio, colour",
            "Salva parole chiave" => "Save keywords",
            "Salvataggio in corso…" => "Saving…",
            "Solo libreria · XMP non attivo" => "Library only · XMP disabled",
            "PROVENIENZA DEL RENDER" => "RENDER PROVENANCE",
            "Ingresso" => "Input",
            "Non determinato" => "Unknown",
            "Lavoro" => "Working space",
            "Rec.2020 lineare · fp32" => "Linear Rec.2020 · fp32",
            "Premoltiplicata in luce lineare" => "Premultiplied in linear light",
            "Uscita" => "Output",
            "sRGB 8 bit · clamp dichiarato" => "8-bit sRGB · explicit clamping",
            "Contratto da qualificare" => "Contract awaiting qualification",
            "Isolamento" => "Isolation",
            "Orientamento" => "Orientation",
            "Decodifica" => "Decoding",
            "Presentazione" => "Presentation",
            "Lineare · Lanczos3 / Mitchell" => "Linear · Lanczos3 / Mitchell",
            "Area / triangolare" => "Area / triangular",
            "Standard e Riferimento richiedono le prove R1." => {
                "Standard and Reference require R1 qualification."
            }
            "CAMPIONE DEL VIEWPORT" => "VIEWPORT SAMPLE",
            "Errore di lettura" => "Read error",
            "Anteprima non abilitata" => "Preview disabled",
            "Caricamento…" => "Loading…",
            "Scartata" => "Rejected",
            "Anteprima disponibile · dettagli del decoder in Ispezione" => {
                "Preview available · decoder details in Inspector"
            }
            "Decoder non disponibile o file oltre quota" => {
                "Decoder unavailable or file exceeds limits"
            }
            "Qualità anteprima" => "Preview quality",
            "Piena" => "Full",
            "Qualità piena per questa foto" => "Full quality for this photo",
            "Override di sessione · il cambio globale lo rimuove" => {
                "Session override · cleared by a global quality change"
            }
            "Adatta" => "Fit",
            "Un pixel sorgente per pixel fisico dello schermo" => {
                "One source pixel per physical screen pixel"
            }
            "Adatta alla finestra" => "Fit to window",
            "Seleziona due immagini con Cmd/Ctrl + clic nella griglia." => {
                "Select two images with Cmd/Ctrl + click in the grid."
            }
            "Preparazione dettaglio 1:1…" => "Preparing 1:1 detail…",
            "Raffinamento anteprima…" => "Refining preview…",
            "Preparazione dell'immagine…" => "Preparing image…",
            "Aprire il bundle macOS con decoder XPC.\nLimite: 256 MiB per file, 64 Mi pixel." => {
                "Open the macOS bundle with XPC decoders.\nLimits: 256 MiB per file, 64 Mi pixels."
            }
            "sRGB > lineare Rec.2020 > sRGB" => "sRGB > linear Rec.2020 > sRGB",
            "Salvataggio…" => "Saving…",
            "Miniature" => "Thumbnails",
            "TrueRenderer · guida e stato" => "TrueRenderer · help and status",
            "Un'immagine, una resa tracciabile." => "One image, a traceable rendering path.",
            "Disponibile: corpus PNG 8/16 bit, griglia, anteprima, confronto a due, zoom fisico 1:1, campione al puntatore, rating, etichette, parole chiave, ricerca, undo e backup locali." => {
                "Available: 8/16-bit PNG corpus, grid, preview, two-image comparison, physical 1:1 zoom, pointer sampling, ratings, labels, keywords, search, undo and local backups."
            }
            "Il motore RAW si sceglie nelle impostazioni: Apple sul Mac, LibRaw bilineare/AHD e TrueRenderer fp32 sperimentale. Il motore proprio supporta attualmente Nikon D750 e D40 Bayer; compatibilità e resa dipendono dal motore. Il bundle Mac usa servizi XPC, il port Windows un worker confinato sperimentale. Massimo 256 MiB e 64 Mi pixel; il normale worker non confinato accetta soltanto il corpus." => {
                "Choose the RAW engine in Settings: Apple on Mac, LibRaw bilinear/AHD and experimental TrueRenderer fp32. The custom engine currently supports Nikon D750 and D40 Bayer files; compatibility and rendering depend on the engine. The Mac bundle uses XPC services; Windows uses an experimental confined worker. Limits: 256 MiB and 64 Mi pixels; the unconfined worker accepts only the test corpus."
            }
            "Restano da qualificare: XPC/App Sandbox e Windows, ICC/Little CMS, presentazione sul monitor, filtri e CPU/GPU, accessibilità e prestazioni. JPEG/TIFF, RAW, XMP e gigapixel seguono la roadmap. Il badge rimane Anteprima." => {
                "Qualification remains open for XPC/App Sandbox and Windows, ICC/Little CMS, monitor presentation, filters and CPU/GPU, accessibility and performance. Format qualification, XMP and gigapixel support follow the roadmap. Assurance remains Preview."
            }
            "Valuta / scarta" => "Rate / reject",
            "Etichette rosso, giallo, verde, blu" => "Red, yellow, green, blue labels",
            "Griglia / anteprima / confronto" => "Grid / preview / compare",
            "Adatta o pixel fisici 1:1" => "Fit or physical 1:1 pixels",
            "Frecce / trascina / rotella" => "Arrows / drag / wheel",
            "Naviga / pan / zoom" => "Navigate / pan / zoom",
            "Ricerca / annulla modifica" => "Search / undo edit",
            "Pannello / miniature / schermo intero / griglia" => {
                "Inspector / thumbnails / fullscreen / grid"
            }
            "Su Windows usare Ctrl al posto di Cmd. Le scorciatoie non agiscono mentre scrivi in un campo." => {
                "On Windows use Ctrl instead of Cmd. Shortcuts are disabled while typing in a text field."
            }
            "Preferenze · Anteprime e prestazioni" => "Preferences · Previews and performance",
            "Le impostazioni valgono per tutte le cartelle; la quota disco si applica a ciascuna cartella separatamente." => {
                "Settings apply to all folders; the disk quota applies to each folder separately."
            }
            "Motore RAW" => "RAW engine",
            "Applica e salva aggiorna le immagini. Ogni motore conserva le proprie anteprime in cache." => {
                "Apply and save refreshes images. Each engine keeps its own cached previews."
            }
            "Sperimentale: Nikon D750 e D40 Bayer. Colore e superiorità rispetto agli altri motori ancora da qualificare. I RAW non supportati mostrano un errore." => {
                "Experimental: Nikon D750 and D40 Bayer. Colour accuracy and superiority over other engines remain unqualified. Unsupported RAW files show an error."
            }
            "Memoria automatica" => "Automatic memory",
            "Memoria richiesta (MiB)" => "Requested memory (MiB)",
            "Cache RAM automatica" => "Automatic RAM cache",
            "Cache RAM riutilizzabile (MiB; 0 = solo viste)" => {
                "Reusable RAM cache (MiB; 0 = views only)"
            }
            "Riduzione memoria in corso" => "Reducing memory use",
            "Il budget include stime dei decoder; non è un limite RSS imposto dal sistema." => {
                "The budget includes decoder estimates; it is not an OS-enforced RSS limit."
            }
            "Cache GPU (MiB; 0 = automatica)" => "GPU cache (MiB; 0 = automatic)",
            "Profilo prestazioni" => "Performance profile",
            "Prestazioni" => "Performance",
            "Bilanciato" => "Balanced",
            "Risparmio" => "Power saver",
            "Calcolo immagine" => "Image processing",
            "Automatico" => "Automatic",
            "GPU compatibile" => "Compatible GPU",
            "Questa scelta riguarda il ricampionamento del viewer. Lo sviluppo usa il motore RAW selezionato sopra." => {
                "This controls viewer resampling. RAW development uses the engine selected above."
            }
            "Riduci automaticamente il lavoro a batteria" => "Automatically reduce work on battery",
            "batteria" => "battery",
            "alimentazione esterna / non rilevata" => "external power / unknown",
            "normale" => "normal",
            "elevata · lavoro anticipato sospeso" => "high · prefetch paused",
            "critica · cache riutilizzabili in rilascio" => "critical · releasing reusable caches",
            "adattatore non disponibile" => "adapter unavailable",
            "Thread CPU (0 = automatici)" => "CPU threads (0 = automatic)",
            "Precaricamento" => "Prefetch",
            "Disattivato" => "Disabled",
            "Esteso" => "Extended",
            "Pausa preparazione in background" => "Pause background preparation",
            "Ricostruisci anteprime della cartella" => "Rebuild folder previews",
            "Annulla preparazione" => "Cancel preparation",
            "Abilita cache su disco nella cartella delle immagini" => {
                "Enable disk cache in the image folder"
            }
            "Quota per cartella (MiB)" => "Quota per folder (MiB)",
            "Temporanei (MiB)" => "Temporary files (MiB)",
            "Scadenza senza utilizzo (giorni)" => "Expire after inactivity (days)",
            "Spazio libero da riservare (MiB)" => "Free space to reserve (MiB)",
            "I temporanei rientrano nella quota disco. Le immagini troppo grandi per la cache restano visualizzabili in RAM. I file meno usati vengono rimossi per rispettare la quota." => {
                "Temporary files count towards the disk quota. Images too large for the cache remain viewable in RAM. Least recently used files are removed to meet the quota."
            }
            "Cartella corrente:" => "Current folder:",
            "entries contiene i render fp32 senza perdita; tmp contiene le scritture in corso. I temporanei abbandonati vengono rimossi alla successiva apertura. Originali, annotazioni e backup sono separati." => {
                "entries contains lossless fp32 renders; tmp contains writes in progress. Abandoned temporary files are removed on the next open. Originals, annotations and backups are separate."
            }
            "Applica e salva" => "Apply and save",
            "Svuota cache cartella" => "Clear folder cache",
            "Aggiornamento cache…" => "Updating cache…",
            "Immagini" => "Images",
            "Lettura dei file…" => "Reading files…",
            "Nessuna immagine da mostrare" => "No images to display",
            "Apri una cartella oppure azzera i filtri." => "Open a folder or reset the filters.",
            "Apri il corpus di prova" => "Open the test corpus",
            "Nessuna" => "None",
            "Rosso" => "Red",
            "Giallo" => "Yellow",
            "Verde" => "Green",
            "Blu" => "Blue",
            "Viola" => "Purple",
            "Anteprima standard" => "Standard preview",
            "Anteprima a qualità piena" => "Full-quality preview",
            "LibRaw bilineare (storico)" => "LibRaw bilinear (legacy)",
            "TrueRenderer fp32 (sperimentale)" => "TrueRenderer fp32 (experimental)",
            "Lingua" => "Language",
            "Avvio del motore…" => "Starting engine…",
            "Diagnostica GPU in corso…" => "Running GPU diagnostics…",
            "GPU non disponibile" => "GPU unavailable",
            "sconosciuta" => "unknown",
            "GPU ricreata e riverificata" => "GPU recreated and revalidated",
            "GPU ricreata · calcolo CPU, verifica compute non superata" => {
                "GPU recreated · CPU processing, compute check failed"
            }
            "Dispositivo grafico ripristinato · sessione e annotazioni conservate" => {
                "Graphics device restored · session and annotations preserved"
            }
            "Coda occupata: riprovare fra un momento" => "Queue busy: try again shortly",
            "Lettura della cartella…" => "Reading folder…",
            "Qualità aggiornata; override per foto rimossi" => {
                "Quality updated; per-photo overrides cleared"
            }
            "Sorgente modificata · aggiornamento anteprima…" => {
                "Source changed · updating preview…"
            }
            "Sorgente non disponibile · anteprima precedente rimossa" => {
                "Source unavailable · previous preview removed"
            }
            "Sorgenti cambiate: anteprime invalidate; annotazioni conservate" => {
                "Sources changed: previews invalidated; annotations preserved"
            }
            "Annotazioni salvate nella libreria · XMP non attivo in R0" => {
                "Annotations saved in the library · XMP disabled in R0"
            }
            "Impostazioni/cache aggiornate" => "Settings/cache updated",
            "Operazione cache interrotta" => "Cache operation interrupted",
            "Attendo il salvataggio delle annotazioni prima di chiudere…" => {
                "Waiting for annotations to save before closing…"
            }
            "Scansione sostituita" => "Scan superseded",
            "Limite R0: 100.000 file nella cartella" => "R0 limit: 100,000 files per folder",
            "Cartella pronta · Anteprima" => "Folder ready · Preview",
            "File esterni: aprire il bundle macOS con servizi XPC." => {
                "External files: open the macOS bundle with XPC services."
            }
            "Undo in conflitto con una modifica più recente" => "Undo conflicts with a newer edit",
            "Persistenza rinviata per letture prioritarie" => {
                "Disk write deferred for priority reads"
            }
            "Cache disco disattivata · solo RAM" => "Disk cache disabled · RAM only",
            "Cache della cartella svuotata" => "Folder cache cleared",
            "Cache della cartella pronta" => "Folder cache ready",
            "Cache scaduta" => "Cache expired",
            "Immagine riutilizzata dalla cache" => "Image reused from cache",
            "Immagine oltre quota cache/temporanei; mantenuta in RAM" => {
                "Image exceeds cache/temporary quota; retained in RAM"
            }
            "Spazio libero riservato: cache saltata" => "Free space reserved: cache skipped",
            "Cache annullata" => "Cache cancelled",
            "Cache della cartella aggiornata" => "Folder cache updated",
            "Cartella esistente non riconosciuta: nessuna modifica" => {
                "Existing folder not recognized: no changes made"
            }
            "Cartella cache non riconosciuta" => "Cache folder not recognized",
            "Quota cache non applicabile; scrittura saltata" => {
                "Cache quota cannot be applied; write skipped"
            }
            "Valutazione valida: scartato (-1), oppure 0–5" => {
                "Valid rating: rejected (-1), or 0–5"
            }
            "Massimo 64 parole chiave per immagine" => "Maximum 64 keywords per image",
            "Parola chiave non valida (massimo 128 byte)" => "Invalid keyword (maximum 128 bytes)",
            "Versione impostazioni non supportata" => "Unsupported settings version",
            "Impostazioni cache fuori intervallo" => "Cache settings out of range",
            "Impostazioni memoria/thread fuori intervallo" => "Memory/thread settings out of range",
            "Impostazioni troppo grandi" => "Settings file too large",
            "Motore RAW non disponibile su questa piattaforma: scegliere un motore nelle impostazioni" => {
                "RAW engine unavailable on this platform: choose an engine in Settings"
            }
            "Metadati · nessun raster" => "Metadata · no raster",
            "nessuno · probe" => "none · probe",
            "nessuno (campioni LOD 0)" => "none (LOD 0 samples)",
            "sRGB · corpus generato" => "sRGB · generated corpus",
            "Applicato da LibRaw una volta" => "Applied once by LibRaw",
            "Profilo ImageIO implicito/assunto; non assegnato dall'utente" => {
                "Implicit/assumed ImageIO profile; not assigned by the user"
            }
            "Formato non riconosciuto" => "Unrecognized format",
            "Formato non riconosciuto da ImageIO" => "Format not recognized by ImageIO",
            "Metadati/dimensioni fuori quota" => "Metadata/dimensions exceed limits",
            "Formato, numero di pagine/fotogrammi o dimensioni non supportati (max 64 Mi pixel, 32768 per lato)" => {
                "Unsupported format, page/frame count or dimensions (max 64 Mi pixels, 32768 per side)"
            }
            "RAW non supportato dal decoder Apple installato, oppure oltre quota" => {
                "RAW unsupported by the installed Apple decoder, or exceeds limits"
            }
            "ImageIO non riesce a decodificare il file" => "ImageIO cannot decode the file",
            "Il decoder non ha prodotto un'immagine completa valida" => {
                "Decoder did not produce a valid complete image"
            }
            "Backend Core Image non disponibile" => "Core Image backend unavailable",
            "TrueRenderer è già aperto con questa libreria" => {
                "TrueRenderer is already open with this library"
            }
            "Manca tr-worker accanto all'applicazione. Eseguire scripts/build-macos.sh." => {
                "tr-worker is missing beside the application. Run the platform build script."
            }
            "Apri" => "Open",
            "Vista" => "View",
            "Menu" => "Menu",
            "Libreria e filtri" => "Library and filters",
            "Libreria" => "Library",
            "Selezione" => "Selection",
            "Filtri" => "Filters",
            "Etichetta" => "Label",
            "Qualsiasi valutazione" => "Any rating",
            "Originali in sola lettura" => "Read-only originals",
            "Generale" => "General",
            "Anteprime e RAW" => "Previews and RAW",
            "Cache e dati" => "Cache and data",
            "Il tuo spazio di lavoro, le tue preferenze." => "Your workspace, your preferences.",
            "Manutenzione" => "Maintenance",
            "Svuota solo le anteprime ricostruibili della cartella corrente." => {
                "Clear only the rebuildable previews in the current folder."
            }
            "Ripristina modifiche" => "Revert changes",
            "Chiudi" => "Close",
            "Modifiche da applicare" => "Unapplied changes",
            "La lingua viene applicata e salvata subito." => {
                "Language changes are applied and saved immediately."
            }
            "Vista · questa sessione" => "View · this session",
            "Mostra ispettore" => "Show inspector",
            "Mostra miniature nel viewer" => "Show viewer filmstrip",
            "Dati locali" => "Local data",
            "Resa delle anteprime" => "Preview rendering",
            "Preparazione in background" => "Background preparation",
            "Elaborazione" => "Processing",
            "Memoria e GPU" => "Memory and GPU",
            "Archiviazione delle anteprime" => "Preview storage",
            "Utilizzo memoria" => "Memory usage",
            "Diagnostica GPU" => "GPU diagnostics",
            "Valutazione" => "Rating",
            "Parole chiave" => "Keywords",
            "Provenienza del render" => "Render provenance",
            "Anteprima immagine" => "Image preview",
            "Ispettore" => "Inspector",
            "Qualità" => "Quality",
            "Solo questa foto" => "This photo only",
            "Piena · questa foto" => "Full · this photo",
            "Dettagli" => "Details",
            "Trascina qui le immagini" => "Drop images here",
            "Apri una cartella per iniziare, oppure trascina file e cartelle nella finestra." => {
                "Open a folder to get started, or drop files and folders into this window."
            }
            _ => source,
        }
    }
    /// Translate only registered application messages, never arbitrary replacements
    /// inside paths, filenames, keywords or decoder identifiers.
    pub fn message(self, source: &str) -> std::borrow::Cow<'_, str> {
        let literal = self.text(source);
        if self == Self::Italian || literal != source {
            return std::borrow::Cow::Borrowed(literal);
        }
        for (pattern, translation) in MESSAGES {
            if let Some(message) = translate_message(source, pattern, translation) {
                return std::borrow::Cow::Owned(message);
            }
        }
        std::borrow::Cow::Borrowed(source)
    }
}

const MESSAGES: &[(&str, &str)] = &[
    ("{} · applicato una volta", "{} · applied once"),
    ("{} · dichiarato dal file", "{} · declared by the file"),
    ("{} · sviluppo RAW", "{} · RAW development"),
    ("sRGB assunto · {}", "Assumed sRGB · {}"),
    ("{} · primarie sRGB assunte", "{} · assumed sRGB primaries"),
    ("Profilo ImageIO: {}", "ImageIO profile: {}"),
    (
        "RAW: WB/metadati Apple; baseline {} EV; boost/NR/sharpen off",
        "RAW: Apple WB/metadata; baseline {} EV; boost/NR/sharpen off",
    ),
    (
        "Persistenza anteprima saltata: {}",
        "Preview disk write skipped: {}",
    ),
    (
        "GPU ricreata · calcolo CPU: {}",
        "GPU recreated · CPU processing: {}",
    ),
    (
        "Salvataggio qualità fallito: {}",
        "Could not save quality: {}",
    ),
    (
        "Salvataggio lingua fallito: {}",
        "Could not save language: {}",
    ),
    (
        "GPU/CPU: {} campioni · errore max {} · filtro e uscita verificati",
        "GPU/CPU: {} samples · max error {} · filter and output verified",
    ),
    (
        "GPU/CPU: {} campioni · errore max {} · fuori soglia",
        "GPU/CPU: {} samples · max error {} · outside tolerance",
    ),
    (
        "Diagnostica GPU non disponibile: {}",
        "GPU diagnostics unavailable: {}",
    ),
    ("Modifica NON salvata: {}", "Edit NOT saved: {}"),
    ("Apertura libreria: {}", "Opening library: {}"),
    (
        "Cache non disponibile, uso RAM: {}",
        "Cache unavailable, using RAM: {}",
    ),
    (
        "Corpus pronto · {} file non leggibili",
        "Corpus ready · {} unreadable files",
    ),
    ("Scansione: {}", "Scan: {}"),
    ("Backup verificato: {}", "Backup verified: {}"),
    ("Backup non riuscito: {}", "Backup failed: {}"),
    ("Export salvato: {}", "Export saved: {}"),
    ("Export non riuscito: {}", "Export failed: {}"),
    (
        "Salvataggio impostazioni fallito: {}",
        "Could not save settings: {}",
    ),
];

fn translate_message(source: &str, pattern: &str, translation: &str) -> Option<String> {
    let mut parts = pattern.split("{}");
    let mut remaining = source.strip_prefix(parts.next()?)?;
    let mut values = Vec::new();
    let separators: Vec<_> = parts.collect();
    for (index, separator) in separators.iter().enumerate() {
        if index + 1 == separators.len() {
            values.push(remaining.strip_suffix(separator)?);
            remaining = "";
        } else {
            let (value, rest) = remaining.split_once(separator)?;
            values.push(value);
            remaining = rest;
        }
    }
    if !remaining.is_empty() {
        return None;
    }
    let mut translated = translation.split("{}");
    let mut result = translated.next()?.to_owned();
    for (value, suffix) in values.into_iter().zip(translated) {
        result.push_str(value);
        result.push_str(suffix);
    }
    Some(result)
}

macro_rules! localized_format {
    ($lang:expr, $it:literal, $en:literal $(, $arg:expr)* $(,)?) => {
        match $lang {
            $crate::i18n::Language::Italian => format!($it $(, $arg)*),
            $crate::i18n::Language::English => format!($en $(, $arg)*),
        }
    };
}
pub(crate) use localized_format;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translated_messages_preserve_paths_and_unknown_diagnostics() {
        let path = "/foto/Scartate/è una foto.png";
        let source = format!("Backup verificato: {path}");
        assert_eq!(
            Language::English.message(&source),
            format!("Backup verified: {path}")
        );
        assert_eq!(Language::Italian.message(&source), source);
        assert_eq!(Language::English.message(path), path);
        assert_eq!(
            Language::English.message("vendor error 123"),
            "vendor error 123"
        );
        assert_eq!(
            Language::English
                .message("GPU/CPU: 42 campioni · errore max 1.00e-5 · filtro e uscita verificati"),
            "GPU/CPU: 42 samples · max error 1.00e-5 · filter and output verified"
        );
    }

    #[test]
    fn all_message_templates_preserve_every_interpolated_value() {
        for (source, translation) in MESSAGES {
            assert_eq!(
                source.matches("{}").count(),
                translation.matches("{}").count(),
                "{source}"
            );
            let sample = source.replace("{}", "value");
            assert_eq!(
                Language::English.message(&sample),
                translation.replace("{}", "value"),
                "{source}"
            );
        }
    }
}
