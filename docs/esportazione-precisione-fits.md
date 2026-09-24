# Esportazione, precisione SDR e FITS

## ADR 0010 — tre percorsi distinti

Su richiesta del titolare, l'esportazione fotografica e una verticale FITS 2D
anticipano parte dello scope post-v1. Questo documento specifica il contratto;
implementazione, prove e limiti correnti risiedono soltanto in [STATO.md](../STATO.md).
Non cambia l'assurance Anteprima né chiude i gate R0–R4.

### Esportazione fotografica

Il comando Menu → Esporta fotografie opera sulla selezione, con destinazione
esplicita e massimo 1.000 elementi. Congela motore RAW e opzioni all'avvio.
Sviluppa la sorgente nativa nel decoder isolato; non salva miniature o screenshot.
Il lato lungo facoltativo riduce il render, senza ingrandirlo. Zero conserva le
dimensioni native. La qualità Standard/Piena della vista non modifica l'export.

«Verifica resa finale» mostra la proiezione sRGB16 del PNG/TIFF16 nativo prima
del ricampionamento del viewer. Trasformata, clamp e quantizzazione sono comuni
all'encoder; il working fp32 esteso rimane disponibile nella vista di sviluppo.
È una prova d'uscita del formato, distinta dalla calibrazione del monitor o
dalla fedeltà cromatica della fotocamera. Per JPEG, resize export, TIFF float32 e
DNG si mantengono i contratti specifici sotto: non sono simulati da questa prova.

Il profilo sRGB incorporato in JPEG/TIFF16 usa coloranti PCS D50, bianco D50 e
tag `chad` D65→D50 coerenti con la [descrizione ICC di sRGB, §B.2](https://registry.color.org/rgb-registry/files/sRGB.pdf).
La curva sRGB resta analitica. I default del generatore ICC non costituiscono da
soli il contratto: la regressione verifica i tag serializzati e la campagna nativa
verifica il file riaperto. Questa correzione non cambia i campioni RGB codificati.

Per il TIFF float32, primarie e bianco D65 provengono dalla
[definizione Rec.2020](https://registry.color.org/rgb-registry/bt2020), con TRC
lineare. Coloranti e `chad` usano la stessa trasformazione Bradford D65→PCS D50;
il bianco del profilo segue il [contratto ICC D50](https://www.color.org/whyd50/).
Il profilo è denominato «TrueRenderer Linear Rec.2020 D65». Il colorante rosso
adattato contiene Z negativo: si conserva la colorimetria, senza ritaglio dei tag.
Come indicato da ICC per Rec.2020, questo richiede una verifica dei lettori:
non implica interoperabilità universale dei CMM. I campioni TIFF restano fp32
lineari estesi, indipendentemente dai limiti della trasformata del lettore.

| Operazione | Dati e colore | Alpha e perdita |
|---|---|---|
| JPEG | sRGB ICC, 8 bit, qualità 1–100 | con perdita; bianco composto in lineare |
| PNG 8/16 | sRGB, interi 8/16 bit; compressione veloce/bilanciata/compatta | alpha straight; compressione lossless |
| TIFF 16 | sRGB ICC, interi 16 bit, non compresso | alpha straight |
| TIFF float32 | Rec.2020 lineare ICC, RGBA fp32, non compresso | alpha associata; conserva i bit del working, negativi e valori oltre 1 |
| DNG lineare | RGB Rec.2020 D65 sviluppato, interi 16 bit, LinearRaw | niente mosaico; rifiuta trasparenza |
| DNG RAW | campioni u16 del mosaico dell'area attiva, CFA e calibrazione | niente demosaic, resize, WB applicato o gamma |

Tutti i formati interi sviluppati ritagliano i valori fuori dal dominio ammesso;
il risultato riporta il numero di canali ritagliati. Non si applica dither.
Compressione PNG e qualità JPEG non sono controlli intercambiabili. TIFF float32
è una copia del **render** working, non dei campioni del sensore: non recupera
clipping, gamma o precisione già persi dal decoder. Non è un export FITS.

Il DNG RAW è limitato a Nikon D750/D40 Bayer e fixture sintetica autorizzata.
Conserva campioni attivi, fase CFA, nero/bianco, neutral WB, orientamento e matrice
D65 della versione LibRaw bloccata. Non include margini ottici, MakerNotes,
metadata privati EXIF/GPS o il flusso NEF originale compresso. **Non sostituisce
l'originale come archivio.** La calibrazione della ricetta e l'interoperabilità
dei lettori restano proprietà da verificare, non conseguenze del suffisso DNG.
Il DNG lineare descrive invece colori già sviluppati, non una camera fittizia
con dati mosaico. Writer indipendente dalla specifica Adobe, senza Adobe SDK.

Per riaprire i DNG di questo writer nell'app si seleziona LibRaw
bilineare oppure AHD: sul DNG lineare riconoscono RGB già sviluppato e non eseguono
demosaic, sul DNG RAW sviluppano il mosaico. Apple RAW e il motore mosaico
TrueRenderer non supportano questi output; nessuna sostituzione silenziosa del motore richiesto. La restrizione
non viene aggirata falsificando modello o matrice della fotocamera.

Codifica e parsing avvengono nei due worker già isolati, con snapshot immutabile,
quote, timeout e cancellazione. Nessun decoder nell'interfaccia e nessun fallback
pipe per file esterni macOS. Output massimo 512 MiB; l'ammissione considera anche
sviluppo completo e buffer di codifica, quindi può rifiutare file grandi.
Il broker pubblica un temporaneo nella destinazione con `persist_noclobber` e
suffisso in caso di collisione. Non sostituisce file esistenti o originali.
Annullamento elimina soltanto il temporaneo posseduto; gli export completati
restano. Non promette durabilità directory contro ogni crash del filesystem.

### Presentazione SDR oltre 8 bit

Il working resta Rec.2020 lineare esteso fp32. Le preferenze di presentazione
sono 8 bit compatibile (default), SDR10 e SDR16 float, applicate al riavvio.
La scelta riguarda il viewer comune ai motori RAW, non la qualità dello sviluppo.
CPU e GPU evitano la quantizzazione intermedia a 8 bit nel percorso ad alta
precisione; il quad fotografico è opaco dopo composizione nel working.

Si negozia **la coppia formato/spazio colore**: RGB10A2 o RGBA16F soltanto se
il backend dichiara sRGB per quel formato, altrimenti fallback 8 bit visibile
nella diagnostica. I valori sono sRGB SDR clamp [0,1], con tag esplicito sRGB,
mai Auto/scRGB. Non è HDR/EDR, wide gamut o color management app-managed.
Su Windows non si usa RGBA16F/scRGB con valori sRGB codificati. Il patch locale
egui-wgpu mantiene questo contratto e le licenze upstream.

Richiesto, effettivo e profondità del pannello sono distinti. Il readback nativo
verifica codici prima del compositore, non il collegamento video o la luce emessa.
FP16 non equivale a 65.536 livelli uniformi; l'uscita intera 16 bit appartiene
all'esportazione. Texture e buffer fp16 costano di più; le quote non certificano
un tetto fisico RSS né includono una misura esatta di ogni allocazione swapchain.
Non si alzano implicitamente i budget per far riuscire la modalità.

### FITS scientifico in sola lettura

Parser Rust limitato, nello stesso worker isolato; nessuna nuova libreria CFITSIO
né supporto a URL, espressioni o file ausiliari. Riconoscimento dalla firma SIMPLE,
non dalla sola estensione `.fits`, `.fit` o `.fts`.

- Primary HDU 2D oppure primary vuoto seguito da IMAGE 2D: prima immagine idonea,
  senza selettore HDU; i successivi HDU non sono interpretati.
- BITPIX 8, 16, 32, −32; byte order FITS, BSCALE/BZERO applicati una volta in f64,
  BUNIT, BLANK intero distinto da NaN, ±Inf e overflow della scala.
- Niente BITPIX 64/−64, cubi, tabelle, compressione, gruppi, WCS o fotometria.
- Sorgente entro la quota comune; header ≤1 MiB ciascuno, ≤4 MiB totali,
  ≤256 HDU, dimensioni e prodotti verificati, piano entro 67.108.864 pixel.

I campioni scientifici non sono RGB. Statistiche e istogramma riguardano l'intero
piano nelle unità fisiche, ignorando i campioni invalidi. Il proxy fp32 normalizzato
conserva numeratore e copertura; le riduzioni usano pesi positivi e lo stretch
lineare/asinh si applica dopo la riduzione, soltanto alla vista. Invalidi sullo
sfondo, piano costante a grigio medio. Non si fa istogramma scientifico su sRGB8.
Il percorso di ricampionamento FITS è CPU anche con preferenza GPU, dichiarato
nell'ispettore; la presentazione successiva usa il contratto SDR comune.

Il campionatore su richiesta rilegge i byte dello snapshot originale isolato:
coordinate 0-based, x orizzontale e y verso il basso, valore memorizzato e fisico
separati. Non ricostruisce campioni dal proxy e conserva interi oltre 2²⁴ in f64.
Non conserva il payload NaN nell'IPC, ma ne dichiara la classificazione.
Lo stretch è globale di sessione per FITS e non modifica dati o metadata.
L'export fotografico di FITS è rifiutato per evitare conversioni scientifiche
implicite. Un futuro export FITS richiede un contratto separato per dati e header.

### Fonti primarie

- [DNG Adobe](https://helpx.adobe.com/camera-raw/desktop/dng-and-file-formats/digital-negative.html)
- [FITS 4.0](https://fits.gsfc.nasa.gov/standard40/fits_standard40aa-le.pdf)
- [Apple: spazi colore e HDR](https://developer.apple.com/documentation/metal/using-color-spaces-to-display-hdr-content)
- [Microsoft: HDR e scRGB](https://learn.microsoft.com/en-us/windows/win32/direct3darticles/high-dynamic-range)
