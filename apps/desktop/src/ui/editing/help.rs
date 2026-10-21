//! Photographic help shared by labels, headings and the tool catalogue.
use super::egui;
use crate::i18n::Language;

pub(super) struct ToolHelp {
    pub id: &'static str,
    pub it: &'static str,
    pub en: &'static str,
}
pub(super) const TOOLS: &[ToolHelp] = &[
    ToolHelp {
        id: "sample",
        it: "Seleziona i colori vicini a un campione della foto e ne corregge tonalità, saturazione e luminosità. Uniformità avvicina i colori selezionati al campione; la sfumatura rende graduale il confine.",
        en: "Selects colors close to a sample in the photo and adjusts their hue, saturation and brightness. Uniformity draws selected colors toward the sample; feathering softens the boundary.",
    },
    ToolHelp {
        id: "light",
        it: "Regola esposizione, temperatura, tinta e saturazione del livello. Usa la maschera per limitare l’effetto a una zona; temperatura e tinta agiscono sul colore già sviluppato.",
        en: "Adjusts layer exposure, warmth, tint and saturation. Use the mask to limit the effect to an area; warmth and tint act on already developed color.",
    },
    ToolHelp {
        id: "curves",
        it: "Associa i valori in ingresso a nuovi valori in uscita. La curva RGB modifica insieme i canali; le curve rosso, verde e blu permettono di correggere le dominanti. Clicca per aggiungere un punto e trascinalo per modellare la curva.",
        en: "Maps input values to new output values. The RGB curve adjusts all channels; individual red, green and blue curves can correct color casts. Click to add a point and drag it to shape the curve.",
    },
    ToolHelp {
        id: "levels",
        it: "Imposta nero, bianco e mezzitoni, insieme ai limiti di uscita. Analizza ingresso mostra l’istogramma reale; Auto e contagocce fissano valori modificabili. I canali separati possono cambiare il colore.",
        en: "Sets black, white and midtones, together with output limits. Analyze input shows the actual histogram; Auto and pickers set editable values. Separate channels can change color.",
    },
    ToolHelp {
        id: "mixer",
        it: "Modifica tonalità, saturazione e luminosità di otto famiglie di colore. Seleziona la famiglia da correggere; Bianco e nero converte il risultato in monocromia.",
        en: "Adjusts hue, saturation and brightness in eight color families. Select the family to edit; Black and white converts the result to monochrome.",
    },
    ToolHelp {
        id: "channels",
        it: "Costruisce ogni canale in uscita mescolando rosso, verde e blu in ingresso. I coefficienti e gli offset permettono trasformazioni cromatiche ampie; non sono il bilanciamento del bianco RAW.",
        en: "Builds each output channel by mixing the input red, green and blue channels. Coefficients and offsets allow broad color transformations; this is not RAW white balance.",
    },
    ToolHelp {
        id: "grading",
        it: "Aggiunge una dominante distinta a ombre, mezzitoni e luci, più una regolazione globale. L’angolo della ruota sceglie il colore, la distanza dal centro la cromia; bilanciamento e sovrapposizione regolano le transizioni.",
        en: "Adds separate color casts to shadows, midtones and highlights, plus a global adjustment. Wheel angle selects hue and distance from the center sets chroma; balance and overlap control the transitions.",
    },
    ToolHelp {
        id: "black_white",
        it: "Converte la foto in bianco e nero e regola quanto diventano chiari i colori originari. Per esempio, schiarire i blu può rendere più chiaro il cielo senza schiarire allo stesso modo i rossi.",
        en: "Converts the photo to black and white and adjusts how light the original colors become. For example, raising blues can lighten a blue sky without equally lightening reds.",
    },
    ToolHelp {
        id: "filter",
        it: "Applica una colorazione simile a un filtro fotografico. Scegli colore e saturazione, poi dosa l’effetto con Densità; Conserva luminanza limita il cambiamento di luminosità.",
        en: "Applies a tint similar to a photographic color filter. Choose hue and saturation, then set its strength with Density; Preserve luminance limits brightness changes.",
    },
    ToolHelp {
        id: "gradient_map",
        it: "Sostituisce i toni con i colori di una sfumatura: a sinistra le ombre, a destra le luci. Modifica colori e posizioni degli stop per creare viraggi o duotoni; Quantità dosa l’effetto.",
        en: "Maps tones to gradient colors: shadows on the left, highlights on the right. Edit stop colors and positions for toning or duotone effects; Amount sets the strength.",
    },
    ToolHelp {
        id: "selective_color",
        it: "Corregge una famiglia di colori con componenti ciano, magenta, giallo e nero. Relativo proporziona la correzione al segnale; Assoluto applica uno spostamento diretto. È una regolazione creativa RGB, non una conversione di stampa CMYK.",
        en: "Adjusts a color family using cyan, magenta, yellow and black components. Relative scales the correction with the signal; Absolute applies a direct shift. This is a creative RGB adjustment, not a print CMYK conversion.",
    },
    ToolHelp {
        id: "colorize",
        it: "Porta la foto verso una tonalità comune, anche nelle zone inizialmente grigie. Scegli tonalità e cromia nel campo colore; Quantità miscela il risultato e Luminosità regola la sua intensità luminosa.",
        en: "Moves the photo toward one common hue, including initially gray areas. Choose hue and chroma in the color field; Amount blends the result and Brightness adjusts its light level.",
    },
    ToolHelp {
        id: "tonal",
        it: "Regola la luce del livello: esposizione su tutto il segnale, luminosità sui mezzitoni e controlli distinti per ombre, luci, neri e bianchi. Il pivot stabilisce il riferimento del contrasto.",
        en: "Adjusts layer tone: exposure affects the whole signal, brightness targets midtones, and separate controls shape shadows, highlights, blacks and whites. The pivot sets the contrast reference.",
    },
    ToolHelp {
        id: "exposure_gamma",
        it: "Applica in ordine esposizione, offset e gamma. L’esposizione moltiplica il segnale, l’offset lo sposta e la gamma ne cambia la distribuzione; utile per regolazioni tecniche, con attenzione ai neri.",
        en: "Applies exposure, offset and gamma in that order. Exposure multiplies the signal, offset shifts it and gamma changes its distribution; useful for technical adjustments, with care around blacks.",
    },
    ToolHelp {
        id: "luminance_curve",
        it: "Modella la luminosità con punti sulla curva, conservando le differenze fra i canali. L’asse orizzontale è l’ingresso, quello verticale l’uscita; la diagonale lascia i toni invariati.",
        en: "Shapes brightness with curve points while preserving differences between channels. The horizontal axis is input and the vertical axis output; the diagonal leaves tones unchanged.",
    },
    ToolHelp {
        id: "parametric_curve",
        it: "Modella quattro zone tonali con cursori e tre confini regolabili. I confini stabiliscono dove agiscono ombre, toni scuri, toni chiari e luci; scegli se lavorare sulla luminanza o sui canali RGB.",
        en: "Shapes four tonal zones with sliders and three adjustable boundaries. Boundaries determine where shadows, darks, lights and highlights act; choose luminance or RGB channels.",
    },
];
struct ControlHelp {
    names: &'static str,
    it: &'static str,
    en: &'static str,
}
const CONTROLS: &[ControlHelp] = &[
    ControlHelp {
        names: "Grana creativa|Aggiungi grana",
        it: "Aggiunge rumore digitale per un effetto vintage, dopo lo sviluppo e il ritaglio. Disattivalo per confrontare la foto conservando le impostazioni. Valuta al 100% con Verifica resa finale.",
        en: "Adds digital noise for a vintage effect, after development and cropping. Turn it off to compare while keeping your settings. Assess at 100% with Verify final rendering.",
    },
    ControlHelp {
        names: "Distribuzione grana",
        it: "Uniforme genera valori con uguale probabilità entro un intervallo. Gaussiana concentra più valori vicino a zero, con picchi più marcati. La quantità nominale è comparabile fra le due distribuzioni.",
        en: "Uniform generates equally likely values within a range. Gaussian concentrates more values near zero, with stronger peaks. Nominal strength is comparable between distributions.",
    },
    ControlHelp {
        names: "Grana monocromatica",
        it: "Attiva: stessa variazione nei tre canali, per grana neutra. Disattiva: variazioni indipendenti, per puntini colorati. Non converte la foto in bianco e nero.",
        en: "On: the same variation in all three channels, for neutral grain. Off: independent variations, for colored speckles. Does not convert the photo to black and white.",
    },
    ControlHelp {
        names: "Quantità grana",
        it: "Regola l’ampiezza delle variazioni chiare e scure. Zero lascia la foto invariata; quantità alte possono produrre ombre e luci tagliate in uscita.",
        en: "Sets the amplitude of light and dark variations. Zero leaves the photo unchanged; high amounts can clip shadows and highlights at output.",
    },
    ControlHelp {
        names: "Dimensione grana (px)",
        it: "Regola la dimensione da 1 a 20 pixel dell’immagine finale a risoluzione nativa, indipendentemente dallo zoom. Grana più grande forma chiazze più morbide; ridurre l’export attenua la grana fine.",
        en: "Sets size from 1 to 20 native output pixels, independently of zoom. Larger grain forms softer patches; downsizing the export attenuates fine grain.",
    },
    ControlHelp {
        names: "Opacità grana",
        it: "Miscela l’effetto con la foto sviluppata: zero lo nasconde, 100% applica tutta la quantità scelta. Non modifica la trasparenza della fotografia.",
        en: "Blends the effect with the developed photo: zero hides it, 100% applies the full chosen amount. Does not change the photograph’s transparency.",
    },
    ControlHelp {
        names: "Nuova trama",
        it: "Genera una diversa disposizione dei granelli, salvata con la foto e annullabile. Gli altri controlli conservano la trama, senza sfarfallio.",
        en: "Generates a different grain pattern, saved with the photo and undoable. Other controls keep the pattern stable, without flicker.",
    },
    ControlHelp {
        names: "Ripristina grana",
        it: "Disattiva la grana e ripristina soltanto le sue impostazioni iniziali.",
        en: "Turns grain off and restores only its initial settings.",
    },
    ControlHelp {
        names: "Pila livelli",
        it: "I livelli si applicano dal basso verso l’alto. Seleziona una riga per modificarla; l’occhio ne alterna la visibilità e il menu contiene nome, riempimento e ordine.",
        en: "Layers are applied from bottom to top. Select a row to edit it; the eye toggles visibility, and the menu contains name, fill and ordering.",
    },
    ControlHelp {
        names: "Effetti del livello|Aggiungi regolazione",
        it: "Le regolazioni di questo livello condividono opacità e maschera. Aggiungi regolazione inserisce uno strumento nel livello selezionato.",
        en: "This layer’s adjustments share its opacity and mask. Add adjustment inserts a tool into the selected layer.",
    },
    ControlHelp {
        names: "Maschera|Crea maschera",
        it: "Sceglie dove agiscono gli effetti del livello. Senza maschera agiscono su tutta la foto; seleziona un componente per modificarne forma e sfumatura.",
        en: "Chooses where this layer’s effects apply. Without a mask they affect the whole photo; select a component to edit its shape and feathering.",
    },
    ControlHelp {
        names: "Sviluppo di base",
        it: "Apre le regolazioni globali della foto, applicate prima della pila dei livelli. L’originale rimane in sola lettura.",
        en: "Opens the photo’s global adjustments, applied before the layer stack. The original remains read-only.",
    },
    ControlHelp {
        names: "Regolazioni",
        it: "Corregge la foto intera con WB, luce, curve, colore e sezioni avanzate. Apri o richiudi un gruppo facendo clic sul suo titolo.",
        en: "Adjusts the whole photo with WB, light, curves, color and advanced sections. Click a group title to expand or collapse it.",
    },
    ControlHelp {
        names: "Livelli",
        it: "Organizza regolazioni sovrapposte. Seleziona un livello per modificarne strumenti, opacità e maschera; l’occhio ne attiva o disattiva l’effetto.",
        en: "Organizes stacked adjustments. Select a layer to edit its tools, opacity and mask; the eye toggles its effect.",
    },
    ControlHelp {
        names: "Strumenti",
        it: "Cerca una regolazione fotografica e applicala a un nuovo livello o a quello selezionato.",
        en: "Find a photographic adjustment and apply it to a new layer or the selected one.",
    },
    ControlHelp {
        names: "Proprietà del livello",
        it: "Rinomina il livello e dosa il suo contributo. Opacità miscela il risultato dopo la maschera; Riempimento dosa le regolazioni interne.",
        en: "Rename the layer and set its contribution. Opacity blends the result after masking; Fill controls its internal adjustments.",
    },
    ControlHelp {
        names: "Nome del livello",
        it: "Assegna un nome riconoscibile al livello, per esempio Cielo o Contrasto del soggetto.",
        en: "Give the layer a recognizable name, such as Sky or Subject contrast.",
    },
    ControlHelp {
        names: "Torna a Sviluppo",
        it: "Chiude il pannello dei look e torna alle regolazioni della foto.",
        en: "Closes the looks panel and returns to photo adjustments.",
    },
    ControlHelp {
        names: "Bordo sinistro|Bordo superiore|Bordo destro|Bordo inferiore",
        it: "Sposta questo bordo del ritaglio. Le coordinate vanno da 0 a 1 rispetto alle dimensioni della foto.",
        en: "Moves this crop edge. Coordinates run from 0 to 1 relative to the photo’s dimensions.",
    },
    ControlHelp {
        names: "Rapporto ritaglio",
        it: "Sceglie le proporzioni dell’inquadratura; Originale usa il rapporto della foto.",
        en: "Chooses the crop proportions; Original uses the photo’s aspect ratio.",
    },
    ControlHelp {
        names: "Grading ombre|Grading mezzitoni|Grading luci",
        it: "Aggiunge una dominante alla zona tonale indicata. Tonalità sceglie il colore e Intensità dosa l’effetto.",
        en: "Adds a color cast to the named tonal zone. Hue chooses the color and Strength sets the amount.",
    },
    ControlHelp {
        names: "Ritaglio sinistra (%)|Ritaglio alto (%)|Ritaglio destra (%)|Ritaglio basso (%)|Sinistra (%)|Alto (%)|Destra (%)|Basso (%)",
        it: "Sposta questo bordo del ritaglio, in percentuale delle dimensioni della foto.",
        en: "Moves this crop edge as a percentage of image dimensions.",
    },
    ControlHelp {
        names: "Inverti tavolozza",
        it: "Scambia la direzione della mappa: i colori delle ombre passano alle luci e viceversa.",
        en: "Reverses the map: shadow colors move to highlights and vice versa.",
    },
    ControlHelp {
        names: "Bianco e nero",
        it: "Converte il risultato in monocromia. Usa le famiglie di colore per regolare la luminosità dei soggetti originari.",
        en: "Converts the result to monochrome. Use color families to adjust the brightness of the original subjects.",
    },
    ControlHelp {
        names: "Inverti maschera",
        it: "Scambia le zone incluse ed escluse dalla maschera.",
        en: "Swaps included and excluded mask areas.",
    },
    ControlHelp {
        names: "Specchio orizzontale|Specchio verticale",
        it: "Riflette la foto lungo la direzione indicata.",
        en: "Mirrors the photo in the named direction.",
    },
    ControlHelp {
        names: "Traccia orizzonte",
        it: "Permette di indicare una linea sulla foto da usare per il raddrizzamento.",
        en: "Lets you mark a line on the photo to use for straightening.",
    },
    ControlHelp {
        names: "Attiva maschera",
        it: "Attiva o esclude le regolazioni di questa maschera senza eliminarla.",
        en: "Enables or bypasses this mask’s adjustments without deleting it.",
    },
    ControlHelp {
        names: "Disegna sulla foto",
        it: "Permette di tracciare la maschera direttamente sull’immagine.",
        en: "Lets you draw the mask directly on the image.",
    },
    ControlHelp {
        names: "WB automatico",
        it: "Attiva il contagocce. Scegli l’area in pixel originali e clicca una zona bianca o grigia neutra: il bilanciamento si applica automaticamente. Evita alte luci bruciate, ombre profonde e aree di colori diversi. Esc annulla; i valori restano modificabili.",
        en: "Activates the eyedropper. Choose an area in original pixels and click a neutral white or grey region: white balance is applied automatically. Avoid clipped highlights, deep shadows and mixed colors. Esc cancels; values remain editable.",
    },
    ControlHelp {
        names: "WB RAW come scattato",
        it: "Ripristina il bilanciamento del bianco iniziale di questa foto.",
        en: "Restores this photo’s initial white balance.",
    },
    ControlHelp {
        names: "Auto esposizione",
        it: "Propone una regolazione della luce basata sull’immagine; resta modificabile e annullabile.",
        en: "Proposes an image-based exposure adjustment; it remains editable and undoable.",
    },
    ControlHelp {
        names: "Auto RGB · grigio medio",
        it: "Stima una correzione del colore già sviluppato assumendo una scena mediamente neutra. Non cambia il WB RAW.",
        en: "Estimates a developed-color correction assuming an average neutral scene. It does not change RAW WB.",
    },
    ControlHelp {
        names: "Campiona nero",
        it: "Usa un campione della foto per impostare il punto nero del canale selezionato.",
        en: "Uses a photo sample to set the selected channel’s black point.",
    },
    ControlHelp {
        names: "Campiona grigio",
        it: "Usa un campione neutro per regolare i mezzitoni; in RGB allinea i canali per ridurre la dominante.",
        en: "Uses a neutral sample to adjust midtones; in RGB it aligns channels to reduce a color cast.",
    },
    ControlHelp {
        names: "Campiona bianco",
        it: "Usa un campione della foto per impostare il punto bianco del canale selezionato.",
        en: "Uses a photo sample to set the selected channel’s white point.",
    },
    ControlHelp {
        names: "Auto composito|Auto canali indipendenti",
        it: "Imposta nero e bianco dai percentili 1–99% dell’ingresso analizzato. I canali indipendenti possono cambiare il colore.",
        en: "Sets black and white from the analyzed input’s 1–99% percentiles. Independent channels may change color.",
    },
    ControlHelp {
        names: "Analizza ingresso",
        it: "Calcola le statistiche all’ingresso di questa regolazione, prima della maschera, per istogramma e automatismi dei livelli.",
        en: "Computes statistics at this adjustment’s input, before masking, for the histogram and automatic levels.",
    },
    ControlHelp {
        names: "Campiona sulla foto",
        it: "Preleva un colore all’ingresso del livello per definire l’intervallo interessato dalla regolazione.",
        en: "Samples a color at the layer input to define the adjustment’s selected range.",
    },
    ControlHelp {
        names: "Pennello",
        it: "Seleziona liberamente una zona dipingendo sulla foto.",
        en: "Selects an area freely by painting on the photo.",
    },
    ControlHelp {
        names: "Radiale",
        it: "Crea una selezione morbida attorno a un centro, utile per interventi localizzati.",
        en: "Creates a soft selection around a center for localized adjustments.",
    },
    ControlHelp {
        names: "Gradiente",
        it: "Crea una transizione lineare fra zona inclusa ed esclusa.",
        en: "Creates a linear transition between included and excluded areas.",
    },
    ControlHelp {
        names: "Aggiungi|Sottrai|Interseca",
        it: "Combina questa componente con la maschera: unione, sottrazione o sola area comune.",
        en: "Combines this component with the mask: union, subtraction or shared area only.",
    },
    ControlHelp {
        names: "Esposizione|Esposizione (EV)|Esposizione locale|Esposizione locale (EV)",
        it: "Schiarisce o scurisce moltiplicando la luce: +1 EV raddoppia il segnale, −1 EV lo dimezza. Non ricrea dettagli già persi per saturazione.",
        en: "Brightens or darkens by scaling light: +1 EV doubles the signal, −1 EV halves it. It cannot restore detail already lost to clipping.",
    },
    ControlHelp {
        names: "Luminosità",
        it: "Schiarisce o scurisce soprattutto i mezzitoni, con un effetto diverso dall’esposizione.",
        en: "Primarily brightens or darkens midtones, with a different effect from exposure.",
    },
    ControlHelp {
        names: "Contrasto",
        it: "Aumenta o riduce la separazione tra toni chiari e scuri attorno al riferimento tonale.",
        en: "Increases or reduces the separation between light and dark tones around the tonal reference.",
    },
    ControlHelp {
        names: "Ombre|Toni scuri",
        it: "Regola la parte scura della foto. Valori positivi la schiariscono, valori negativi la scuriscono.",
        en: "Adjusts the darker part of the photo. Positive values brighten it; negative values darken it.",
    },
    ControlHelp {
        names: "Alte luci|Luci|Toni chiari",
        it: "Regola la parte chiara della foto. Abbassare il valore attenua le luci, ma non recupera da solo dettagli RAW persi.",
        en: "Adjusts the brighter part of the photo. Lower values restrain highlights but do not by themselves recover lost RAW detail.",
    },
    ControlHelp {
        names: "Neri",
        it: "Regola i toni più scuri e la profondità delle ombre. Controlla il risultato per evitare neri troppo chiusi.",
        en: "Adjusts the darkest tones and shadow depth. Watch the result to avoid blocked shadows.",
    },
    ControlHelp {
        names: "Bianchi",
        it: "Regola i toni più chiari. Controlla il risultato per evitare di perdere differenze nelle zone luminose.",
        en: "Adjusts the lightest tones. Watch the result to avoid losing distinctions in bright areas.",
    },
    ControlHelp {
        names: "Temperatura",
        it: "Sposta il bilanciamento dal freddo al caldo. Nei motori a scala relativa, zero corrisponde a Come scattato; Apple usa Kelvin.",
        en: "Shifts white balance from cool to warm. In engines with a relative scale, zero means As shot; Apple uses kelvin.",
    },
    ControlHelp {
        names: "Tinta|Tinta RAW",
        it: "Corregge la dominante verde–magenta: verso sinistra più verde, verso destra più magenta.",
        en: "Corrects the green–magenta cast: left adds green, right adds magenta.",
    },
    ControlHelp {
        names: "Temperatura RGB|Temperatura locale",
        it: "Rende più freddo o più caldo il colore già sviluppato. Non cambia il bilanciamento del sensore RAW.",
        en: "Makes already developed color cooler or warmer. It does not change RAW sensor white balance.",
    },
    ControlHelp {
        names: "Tinta RGB",
        it: "Sposta il colore già sviluppato verso verde o magenta. È distinta dalla tinta del WB RAW.",
        en: "Shifts already developed color toward green or magenta. It is separate from RAW WB tint.",
    },
    ControlHelp {
        names: "Saturazione|Saturazione locale",
        it: "Aumenta o riduce l’intensità dei colori nell’ambito della regolazione selezionata.",
        en: "Increases or decreases color intensity within the selected adjustment.",
    },
    ControlHelp {
        names: "Vividezza",
        it: "Rafforza soprattutto i colori meno saturi, con un intervento più contenuto sui colori già intensi.",
        en: "Mainly strengthens less saturated colors, with less effect on colors that are already vivid.",
    },
    ControlHelp {
        names: "Proteggi toni caldi",
        it: "Riduce l’intervento della vividezza sui colori caldi. Non riconosce automaticamente la pelle.",
        en: "Reduces vibrance changes in warm colors. It does not automatically detect skin.",
    },
    ControlHelp {
        names: "Tonalità|Tonalità (°)",
        it: "Sceglie o sposta il colore lungo il cerchio cromatico. Negli strumenti a intervallo agisce sui colori selezionati.",
        en: "Selects or shifts hue around the color circle. In range-based tools it affects the selected colors.",
    },
    ControlHelp {
        names: "Cromia",
        it: "Regola la forza del colore scelto: al centro o a zero il colore è neutro.",
        en: "Sets the strength of the selected color: at the center or zero, it is neutral.",
    },
    ControlHelp {
        names: "Luminanza|Luminanza zona (EV)",
        it: "Schiarisce o scurisce i colori o la zona tonale interessati dallo strumento.",
        en: "Brightens or darkens the colors or tonal zone affected by the tool.",
    },
    ControlHelp {
        names: "Bilanciamento",
        it: "Sposta il peso del grading fra ombre e luci: negativo privilegia le ombre, positivo le luci.",
        en: "Shifts grading weight between shadows and highlights: negative favors shadows, positive favors highlights.",
    },
    ControlHelp {
        names: "Sovrapposizione",
        it: "Regola quanto si mescolano i viraggi delle zone tonali vicine.",
        en: "Controls how much the tints of neighboring tonal zones blend.",
    },
    ControlHelp {
        names: "Saturazione filtro",
        it: "Regola quanto è saturo il colore del filtro; la densità controlla separatamente quanto filtro applicare.",
        en: "Sets filter color saturation; density separately controls how much filter is applied.",
    },
    ControlHelp {
        names: "Densità filtro",
        it: "Dosa l’effetto del filtro: zero lo disattiva, valori alti rafforzano la colorazione.",
        en: "Sets filter strength: zero disables it, higher values strengthen the tint.",
    },
    ControlHelp {
        names: "Conserva luminanza",
        it: "Mantiene la luminanza durante la trasformazione cromatica, nei limiti del modello dello strumento.",
        en: "Preserves luminance during the color transformation within the tool’s model.",
    },
    ControlHelp {
        names: "Quantità B&N",
        it: "Miscela la foto a colori con la versione in bianco e nero. A 100 la conversione è completa.",
        en: "Blends the color photo with the black-and-white version. At 100 the conversion is complete.",
    },
    ControlHelp {
        names: "Quantità Colorizza|Quantità mappa gradiente",
        it: "Miscela l’effetto con il risultato precedente: zero non applica lo strumento, 100 applica tutto l’effetto.",
        en: "Blends the effect with the previous result: zero applies none, 100 applies the full effect.",
    },
    ControlHelp {
        names: "Luminosità Colorizza (EV)",
        it: "Schiarisce o scurisce il risultato della colorizzazione prima della miscela con la foto.",
        en: "Brightens or darkens the colorized result before blending it with the photo.",
    },
    ControlHelp {
        names: "Posizione stop|Posizione",
        it: "Sposta il colore lungo la scala dei toni: zero nelle ombre, uno nelle luci.",
        en: "Moves the color along the tonal scale: zero in shadows, one in highlights.",
    },
    ControlHelp {
        names: "Ciano (C)|Magenta (M)|Giallo (Y)|Nero (K)",
        it: "Aggiunge o sottrae questa componente nella famiglia selezionata. Il metodo Relativo o Assoluto determina la risposta.",
        en: "Adds or subtracts this component in the selected family. Relative or Absolute mode determines the response.",
    },
    ControlHelp {
        names: "Relativo",
        it: "Proporziona la correzione del colore selettivo all’intensità del segnale.",
        en: "Scales the selective color correction with signal intensity.",
    },
    ControlHelp {
        names: "Assoluto",
        it: "Applica uno spostamento diretto al segnale RGB della famiglia selezionata.",
        en: "Applies a direct shift to the selected family’s RGB signal.",
    },
    ControlHelp {
        names: "Pivot tonale",
        it: "Imposta il tono di riferimento attorno al quale agisce il contrasto.",
        en: "Sets the reference tone around which contrast operates.",
    },
    ControlHelp {
        names: "Offset",
        it: "Aggiunge o sottrae una quantità costante al segnale. Può sollevare o abbassare anche i neri.",
        en: "Adds or subtracts a constant amount from the signal. It can also lift or lower blacks.",
    },
    ControlHelp {
        names: "Gamma",
        it: "Modifica la distribuzione dei mezzitoni tra nero e bianco. Il valore neutro è 1.",
        en: "Changes midtone distribution between black and white. The neutral value is 1.",
    },
    ControlHelp {
        names: "Nero ingresso",
        it: "Imposta il valore di ingresso che viene mappato al nero di uscita.",
        en: "Sets the input value mapped to output black.",
    },
    ControlHelp {
        names: "Bianco ingresso",
        it: "Imposta il valore di ingresso che viene mappato al bianco di uscita.",
        en: "Sets the input value mapped to output white.",
    },
    ControlHelp {
        names: "Nero uscita",
        it: "Imposta il livello dei neri dopo la regolazione dei livelli. Alzarlo rende meno profondi i neri.",
        en: "Sets the black level after the levels adjustment. Raising it makes blacks less deep.",
    },
    ControlHelp {
        names: "Bianco uscita",
        it: "Imposta il livello dei bianchi dopo la regolazione dei livelli. Abbassarlo riduce la luminosità massima.",
        en: "Sets the white level after the levels adjustment. Lowering it reduces peak brightness.",
    },
    ControlHelp {
        names: "Ingresso|Ingresso (X)",
        it: "Sposta il punto selezionato lungo i toni originali della curva.",
        en: "Moves the selected point along the curve’s original input tones.",
    },
    ControlHelp {
        names: "Uscita|Uscita (Y)",
        it: "Imposta il tono risultante del punto selezionato nella curva.",
        en: "Sets the resulting output tone of the selected curve point.",
    },
    ControlHelp {
        names: "Mezzitoni curva",
        it: "Alza o abbassa la curva nel suo punto medio. La diagonale conserva i toni originali.",
        en: "Raises or lowers the curve at its midpoint. The diagonal preserves original tones.",
    },
    ControlHelp {
        names: "Confine ombre|Confine mezzitoni|Confine luci",
        it: "Sposta il confine fra due zone della curva parametrica. I confini mantengono il loro ordine.",
        en: "Moves the boundary between two parametric curve zones. Boundaries retain their order.",
    },
    ControlHelp {
        names: "Ampiezza tonalità (°)",
        it: "Allarga o restringe la gamma di tonalità selezionate attorno al campione.",
        en: "Widens or narrows the range of hues selected around the sample.",
    },
    ControlHelp {
        names: "Ampiezza cromia",
        it: "Allarga o restringe l’intervallo di intensità cromatica selezionato attorno al campione.",
        en: "Widens or narrows the selected chroma range around the sample.",
    },
    ControlHelp {
        names: "Ampiezza luminanza",
        it: "Allarga o restringe l’intervallo di luminosità selezionato attorno al campione.",
        en: "Widens or narrows the selected brightness range around the sample.",
    },
    ControlHelp {
        names: "Uniformità tonalità|Uniformità cromia|Uniformità luminanza",
        it: "Avvicina questa proprietà dei colori selezionati a quella del campione. Aumentala per ridurre le differenze all’interno dell’intervallo.",
        en: "Draws this property of selected colors toward the sample’s value. Increase it to reduce differences within the range.",
    },
    ControlHelp {
        names: "Sfumatura|Sfumatura maschera",
        it: "Rende più morbido il passaggio fra la zona selezionata e quella esclusa.",
        en: "Softens the transition between selected and excluded areas.",
    },
    ControlHelp {
        names: "Opacità",
        it: "Dosa quanto il risultato del livello si mescola con quello sottostante, dopo la maschera.",
        en: "Sets how much the layer result blends with the underlying result, after masking.",
    },
    ControlHelp {
        names: "Riempimento",
        it: "Dosa le regolazioni interne del livello prima della sua opacità finale.",
        en: "Sets the strength of the layer’s adjustments before final opacity.",
    },
    ControlHelp {
        names: "Copertura",
        it: "Imposta quanto la maschera lascia agire il livello: zero esclude, uno include completamente.",
        en: "Sets how much the mask lets the layer affect the image: zero excludes, one fully includes.",
    },
    ControlHelp {
        names: "Centro X|Centro Y",
        it: "Sposta il centro della maschera lungo l’asse indicato. Le coordinate da 0 a 1 sono relative all’intera foto.",
        en: "Moves the mask center along the named axis. Coordinates from 0 to 1 are relative to the whole photo.",
    },
    ControlHelp {
        names: "Mostra area|Mostra area maschera",
        it: "Mostra sulla foto la copertura della selezione per controllarne posizione e sfumatura.",
        en: "Shows selection coverage over the photo to check its position and feathering.",
    },
    ControlHelp {
        names: "Raggio maschera|Raggio pennello",
        it: "Regola l’estensione del pennello o della forma rispetto alla foto.",
        en: "Sets the extent of the brush or shape relative to the photo.",
    },
    ControlHelp {
        names: "Angolo gradiente",
        it: "Ruota la direzione del passaggio graduale della maschera.",
        en: "Rotates the direction of the mask’s gradual transition.",
    },
    ControlHelp {
        names: "Minimo|Massimo|Intervallo minimo|Intervallo massimo",
        it: "Imposta uno dei limiti dell’intervallo selezionato dalla maschera.",
        en: "Sets one of the limits of the range selected by the mask.",
    },
    ControlHelp {
        names: "Texture",
        it: "Aumenta o attenua i piccoli dettagli. Valuta il risultato a ingrandimento 100%.",
        en: "Enhances or softens fine detail. Evaluate the result at 100% zoom.",
    },
    ControlHelp {
        names: "Chiarezza",
        it: "Aumenta o attenua il contrasto locale su strutture più ampie della texture.",
        en: "Enhances or softens local contrast on structures larger than texture detail.",
    },
    ControlHelp {
        names: "Rimozione foschia",
        it: "Modifica contrasto e presenza per attenuare o aumentare l’aspetto velato della foto.",
        en: "Changes contrast and presence to reduce or increase a hazy appearance.",
    },
    ControlHelp {
        names: "Nitidezza",
        it: "Rafforza i bordi e i dettagli; valori elevati possono accentuare aloni e rumore.",
        en: "Strengthens edges and detail; high values can accentuate halos and noise.",
    },
    ControlHelp {
        names: "Raggio nativo (px)",
        it: "Regola la dimensione dei dettagli coinvolti dalla nitidezza, in pixel della foto originale.",
        en: "Sets the size of details affected by sharpening, in original-image pixels.",
    },
    ControlHelp {
        names: "Soglia rumore",
        it: "Aumenta la sensibilità della nuova riduzione rumore e limita l’accentuazione della grana con Texture, Chiarezza e Nitidezza. A zero resta la stima automatica; valori alti possono attenuare dettagli deboli.",
        en: "Increases the new noise reduction’s sensitivity and limits grain enhancement by Texture, Clarity and Sharpening. Zero keeps the automatic estimate; high values may soften weak detail.",
    },
    ControlHelp {
        names: "Rumore luminanza",
        it: "Riduce la grana adattando la stima del rumore alle diverse luminosità della foto, sia RAW sia JPEG, e proteggendo i dettagli più forti. Aumenta gradualmente; valori elevati possono attenuare le trame fini. Valuta al 100% con Verifica resa finale.",
        en: "Reduces grain by adapting the noise estimate to the photo’s brightness levels, for both RAW and JPEG, while protecting stronger detail. Increase gradually; high values can soften fine texture. Evaluate at 100% with Verify final rendering.",
    },
    ControlHelp {
        names: "Rumore cromatico",
        it: "Riduce puntini e macchie colorate su più scale, separatamente dalla grana di luminanza e senza desaturare globalmente la foto. Aumenta gradualmente e valuta bordi e trame colorate al 100%: valori elevati possono attenuarli.",
        en: "Reduces colored speckles and blotches at several scales, separately from luminance grain, without globally desaturating the photo. Increase gradually and assess colored edges and textures at 100%: high values can soften them.",
    },
    ControlHelp {
        names: "Aggiorna riduzione rumore",
        it: "Applica il nuovo filtro con le quantità già impostate. La revisione precedente rimane disponibile con Annulla; aprire una vecchia ricetta conserva la sua resa.",
        en: "Applies the new filter at the current amounts. The previous revision remains available through Undo; opening an old recipe preserves its rendering.",
    },
    ControlHelp {
        names: "Distorsione",
        it: "Corregge manualmente la curvatura a barilotto o a cuscinetto delle linee.",
        en: "Manually corrects barrel or pincushion curvature in straight lines.",
    },
    ControlHelp {
        names: "Distorsione a baffo",
        it: "Regola una componente di distorsione radiale più complessa, da combinare con Distorsione.",
        en: "Adjusts a more complex radial distortion component, to combine with Distortion.",
    },
    ControlHelp {
        names: "Vignettatura ottica",
        it: "Schiarisce o scurisce progressivamente i bordi rispetto al centro.",
        en: "Progressively brightens or darkens edges relative to the center.",
    },
    ControlHelp {
        names: "Defringe viola/verde",
        it: "Attenua frange viola e verdi vicino ai bordi contrastati. Controlla che non indebolisca colori reali.",
        en: "Reduces purple and green fringes near contrasty edges. Check that it does not weaken real colors.",
    },
    ControlHelp {
        names: "CA residua rosso (px)|CA residua blu (px)",
        it: "Ridimensiona radialmente questo canale sul colore già sviluppato per ridurre frange residue. Non riallinea il mosaico RAW.",
        en: "Radially scales this channel in developed color to reduce residual fringes. It does not realign the RAW mosaic.",
    },
    ControlHelp {
        names: "Rotazione (°)|Rotazione|Raddrizzamento (°)",
        it: "Ruota la foto per raddrizzare l’orizzonte o modificare la composizione.",
        en: "Rotates the photo to straighten the horizon or change the composition.",
    },
    ControlHelp {
        names: "Prospettiva verticale|Prospettiva orizzontale",
        it: "Corregge la convergenza delle linee nella direzione indicata. Può richiedere un ritaglio dei bordi.",
        en: "Corrects converging lines in the named direction. Edge cropping may be needed.",
    },
    ControlHelp {
        names: "Scala|Scala (%)|Scala geometria",
        it: "Ingrandisce o riduce la foto nella geometria corretta.",
        en: "Enlarges or reduces the photo within the corrected geometry.",
    },
    ControlHelp {
        names: "Proporzioni",
        it: "Modifica il rapporto fra larghezza e altezza nella trasformazione geometrica.",
        en: "Changes the width-to-height relationship in the geometric transform.",
    },
    ControlHelp {
        names: "Curva rosso|Curva verde|Curva blu",
        it: "Modifica i mezzitoni di questo canale per correggere o creare una dominante.",
        en: "Changes this channel’s midtones to correct or create a color cast.",
    },
    ControlHelp {
        names: "Intensità",
        it: "Dosa la forza dell’effetto nella zona selezionata.",
        en: "Sets effect strength in the selected zone.",
    },
];

pub(super) fn tool_text(lang: Language, id: &str) -> Option<&'static str> {
    TOOLS.iter().find(|t| t.id == id).map(|t| {
        if lang == Language::Italian {
            t.it
        } else {
            t.en
        }
    })
}

pub(super) fn scope<R>(
    ui: &mut egui::Ui,
    id: &'static str,
    body: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let key = egui::Id::new("photographic-help-context");
    let previous = ui.data(|d| d.get_temp::<&'static str>(key));
    ui.data_mut(|d| d.insert_temp(key, id));
    let result = body(ui);
    ui.data_mut(|d| {
        if let Some(previous) = previous {
            d.insert_temp(key, previous);
        } else {
            d.remove::<&'static str>(key);
        }
    });
    result
}

pub(super) fn description(ui: &egui::Ui, lang: Language, label: &str) -> Option<&'static str> {
    let is = |key| label == key || label == lang.text(key);
    let context =
        ui.data(|d| d.get_temp::<&'static str>(egui::Id::new("photographic-help-context")));
    let choose = |it, en| if lang == Language::Italian { it } else { en };
    if context == Some("light") && (is("Temperatura") || is("Tinta")) {
        return Some(choose(
            "Corregge il colore già sviluppato del livello: Temperatura sposta da freddo a caldo, Tinta da verde a magenta. La maschera limita l’effetto; il WB RAW resta separato.",
            "Adjusts the layer’s developed color: Temperature moves from cool to warm, Tint from green to magenta. The mask limits the effect; RAW WB remains separate.",
        ));
    }
    if context == Some("grading")
        && ["Ombre", "Mezzitoni", "Luci", "Globale"]
            .iter()
            .any(|n| is(n))
    {
        return Some(choose(
            "Seleziona la zona da colorare. Globale agisce sull’intera foto; ogni zona conserva le proprie regolazioni.",
            "Selects the zone to tint. Global affects the whole photo; each zone retains its own adjustments.",
        ));
    }
    if context == Some("selective_color")
        && [
            "Rossi", "Gialli", "Verdi", "Ciani", "Blu", "Magenta", "Bianchi", "Neutri", "Neri",
        ]
        .iter()
        .any(|n| is(n))
    {
        return Some(choose(
            "Seleziona la famiglia da correggere con i cursori C/M/Y/K. Il punto accanto al nome indica una famiglia già modificata.",
            "Selects the family to adjust with the C/M/Y/K sliders. A dot beside the name marks an edited family.",
        ));
    }
    if context == Some("mask")
        && ["Luminanza", "Tonalità", "Colore a campioni"]
            .iter()
            .any(|n| is(n))
    {
        return Some(choose(
            "Seleziona le zone della foto comprese nell’intervallo di luminosità o colore. I limiti e la sfumatura determinano la copertura.",
            "Selects areas within a brightness or color range. The limits and feathering determine coverage.",
        ));
    }
    if context == Some("parametric_curve") && ["Luminanza", "RGB"].iter().any(|n| is(n)) {
        return Some(choose(
            "Sceglie come applicare la curva: Luminanza conserva le differenze fra i canali, RGB applica la trasformazione separatamente a ciascun canale.",
            "Chooses how to apply the curve: Luminance preserves channel differences, while RGB transforms each channel separately.",
        ));
    }
    if is("RGB") {
        return Some(choose(
            "Seleziona la regolazione composita dei tre canali rosso, verde e blu.",
            "Selects the composite adjustment of red, green and blue channels.",
        ));
    }
    if [
        "Rosso",
        "Verde",
        "Blu",
        "Arancio",
        "Giallo",
        "Acquamarina",
        "Viola",
        "Magenta",
    ]
    .iter()
    .any(|n| is(n))
    {
        return Some(match context {
            Some("black_white") => choose(
                "Schiarisce o scurisce in bianco e nero le zone che avevano questo colore nella foto.",
                "Lightens or darkens areas that originally had this color in the black-and-white result.",
            ),
            Some("gradient_map") => choose(
                "Imposta questa componente del colore dello stop selezionato nella sfumatura.",
                "Sets this color component of the selected gradient stop.",
            ),
            Some("sample") => choose(
                "Imposta questa componente del colore usato come riferimento per selezionare l’intervallo.",
                "Sets this component of the reference color used to select the range.",
            ),
            _ => choose(
                "Seleziona questa famiglia o questo canale per le regolazioni successive.",
                "Selects this color family or channel for the following adjustments.",
            ),
        });
    }
    if ["R", "G", "B"].iter().any(|n| is(n)) {
        return Some(choose(
            "Regola il contributo di questo canale al canale in uscita indicato dalla riga.",
            "Sets this channel’s contribution to the output channel named by the row.",
        ));
    }
    if let Some(t) = tr_core::editing::layers::TOOLS.iter().find(|t| is(t.name)) {
        return tool_text(lang, t.id);
    }
    if let Some(c) = CONTROLS.iter().find(|c| c.names.split('|').any(is)) {
        return Some(choose(c.it, c.en));
    }
    let (it, en) = if is("Luce") {
        (
            "Regola luminosità, contrasto e distribuzione dei toni della foto.",
            "Adjusts brightness, contrast and tonal distribution in the photo.",
        )
    } else if is("Bilanciamento del bianco RAW") {
        (
            "Corregge le dominanti nello sviluppo RAW. Come scattato ripristina il bilanciamento iniziale della fotocamera.",
            "Corrects color casts during RAW development. As shot restores the camera’s initial white balance.",
        )
    } else if is("Colore RGB") {
        (
            "Regola il colore dell’immagine già sviluppata: temperatura, tinta e intensità dei colori.",
            "Adjusts already developed image color: warmth, tint and color intensity.",
        )
    } else if is("Curva tonale") {
        (
            "Associa i toni in ingresso a nuovi toni in uscita. La diagonale non cambia la foto; sollevare un punto schiarisce i toni corrispondenti, abbassarlo li scurisce.",
            "Maps input tones to new output tones. The diagonal leaves the photo unchanged; raising a point brightens the corresponding tones and lowering it darkens them.",
        )
    } else if is("Curva a punti · luminanza lineare") {
        return tool_text(lang, "luminance_curve");
    } else if is("Curva a punti · RGB lineare") {
        return tool_text(lang, "curves");
    } else if is("Ottica manuale") {
        (
            "Corregge manualmente distorsione, vignettatura e frange di colore residue. Considera le correzioni già applicate dal motore.",
            "Manually corrects distortion, vignetting and residual color fringes. Account for corrections already applied by the engine.",
        )
    } else if is("Presenza e dettaglio") {
        (
            "Regola texture, contrasto locale, nitidezza e rumore. Valuta i dettagli al 100% con il render finale.",
            "Adjusts texture, local contrast, sharpening and noise. Evaluate detail at 100% with the final render.",
        )
    } else if is("Ritaglio e geometria") || is("Ritaglio…") {
        (
            "Modifica in modo reversibile inquadratura, proporzioni, rotazione e prospettiva.",
            "Reversibly adjusts framing, proportions, rotation and perspective.",
        )
    } else if is("Colore avanzato") {
        return tool_text(lang, "mixer");
    } else if is("Maschere locali") || is("Maschera del livello") {
        (
            "Limita le regolazioni ad aree della foto. Combina forme, pennellate o intervalli di colore e luminanza.",
            "Limits adjustments to areas of the photo. Combine shapes, brush strokes or color and luminance ranges.",
        )
    } else if is("Intervallo del campione") || is("Uniformità verso il campione") {
        return tool_text(lang, "sample");
    } else if is("Intervallo luminanza") {
        return tool_text(lang, "gradient_map");
    } else if is("Transizioni tonali") {
        return tool_text(
            lang,
            if context == Some("tonal") {
                "tonal"
            } else {
                "grading"
            },
        );
    } else if is("Confini delle zone") {
        return tool_text(lang, "parametric_curve");
    } else {
        return None;
    };
    Some(choose(it, en))
}

pub(super) fn attach(
    ui: &egui::Ui,
    response: egui::Response,
    lang: Language,
    label: &str,
) -> egui::Response {
    if let Some(text) = description(ui, lang, label) {
        response.on_hover_text(text).on_disabled_hover_text(text)
    } else {
        response
    }
}
pub(super) fn label(ui: &mut egui::Ui, lang: Language, label: &str) -> egui::Response {
    let response = ui.label(lang.text(label));
    attach(ui, response, lang, label)
}
pub(super) struct CollapsingHeader<'a> {
    inner: egui::CollapsingHeader,
    lang: Language,
    label: &'a str,
}
impl<'a> CollapsingHeader<'a> {
    pub fn new(lang: Language, label: &'a str) -> Self {
        Self {
            inner: egui::CollapsingHeader::new(lang.text(label)),
            lang,
            label,
        }
    }
    pub fn id_salt(mut self, salt: impl std::hash::Hash + std::fmt::Debug) -> Self {
        self.inner = self.inner.id_salt(salt);
        self
    }
    pub fn show<R>(
        self,
        ui: &mut egui::Ui,
        body: impl FnOnce(&mut egui::Ui) -> R,
    ) -> egui::collapsing_header::CollapsingResponse<R> {
        let mut result = self.inner.show(ui, body);
        result.header_response = attach(ui, result.header_response, self.lang, self.label);
        result
    }
}

#[cfg(test)]
pub(super) fn audit_control(ui: &egui::Ui, lang: Language, label: &str) {
    if description(ui, lang, label).is_none() {
        ui.data_mut(|d| {
            d.get_temp_mut_or_default::<Vec<String>>(egui::Id::new("missing-control-help"))
                .push(label.to_owned())
        });
    }
}

pub(super) fn checkbox(
    ui: &mut egui::Ui,
    lang: Language,
    label: &str,
    value: &mut bool,
) -> egui::Response {
    let response = ui.checkbox(value, lang.text(label));
    attach(ui, response, lang, label)
}
pub(super) fn button(ui: &mut egui::Ui, lang: Language, label: &str) -> egui::Response {
    let response = ui.add(egui::Button::new(lang.text(label)).truncate());
    attach(ui, response, lang, label)
}

pub(super) fn button_enabled(
    ui: &mut egui::Ui,
    lang: Language,
    label: &str,
    enabled: bool,
) -> egui::Response {
    let response = ui.add_enabled(enabled, egui::Button::new(lang.text(label)).truncate());
    attach(ui, response, lang, label)
}

pub(super) fn selectable_value<T: PartialEq>(
    ui: &mut egui::Ui,
    lang: Language,
    label: &str,
    current: &mut T,
    value: T,
) -> egui::Response {
    let response = ui.selectable_value(current, value, lang.text(label));
    attach(ui, response, lang, label)
}

pub(super) fn selectable_label(
    ui: &mut egui::Ui,
    lang: Language,
    label: &str,
    selected: bool,
) -> egui::Response {
    let response = ui.selectable_label(selected, lang.text(label));
    attach(ui, response, lang, label)
}
