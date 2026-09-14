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
