# ADR 0004 — formati esterni macOS e repository pubblico proprietario

Data: 7 settembre 2026. Stato: implementato nella 0.1.3, qualifica limitata alle prove registrate.

## Richiesta e modifica dello scopo

Il titolare ha richiesto un repository GitHub pubblico, README, licenza **proprietaria** e apertura effettiva di JPEG, PNG, RAW, TIFF e altri formati. La 0.1.2 ammetteva soltanto i dodici PNG del corpus. Questo incremento anticipa una parte dei formati R1/R3 attraverso i decoder Apple, conservando la modalità **Anteprima**. È una modifica esplicita del perimetro di sviluppo rispetto all'attesa del gate R0 completo nella proposta originale; non chiude quel gate e non sostituisce la futura matrice RAW/ICC multipiattaforma.

## Confine di esecuzione

Solo il bundle macOS abilita i file esterni: due servizi XPC separati con App Sandbox e senza entitlement di rete o accesso generale ai file. Il broker legge gli originali in sola lettura, produce una copia privata, ne calcola SHA-256 e trasferisce soltanto i byte nel protocollo bounded. Nessun percorso viene inviato al decoder. Il worker su pipe continua a rifiutare tutto ciò che non appartiene al corpus compilato. Un errore XPC non attiva un decoder alternativo nel processo UI.

Il riconoscimento usa la firma del contenuto, oltre all'elenco di estensioni per la scansione. Un file rifiutato con risposta IPC valida lascia il servizio disponibile per il job successivo; errori di framing, crash, cancellazione e timeout comportano il riciclo. Le generazioni di cartella conservano la revoca del decoder inattivo. Il digest mostrato nell'ispezione appartiene ai byte decodificati; il token stat dell'indice resta una osservazione best-effort, non una revisione coerente garantita sotto writer concorrente.

Restano i limiti di ADR 0002: il broker verifica il CDHash del servizio, mentre il servizio verifica l'identificatore dell'host; non è il requisito reciproco di un firmatario di release. Firma ad hoc, libproc SPI/audit token, ritardo di launchd e suite avversaria incompleta mantengono aperto il gate di rilascio.

## Decoder e colore

- JPEG, PNG, TIFF, GIF, BMP, HEIC/HEIF e WebP: ImageIO crea il bitmap, Core Image/ColorSync lo converte in Rec.2020 lineare esteso fp32 con alpha premoltiplicata. L'orientamento EXIF viene applicato una volta. PNG e TIFF a 16 bit non transitano in un buffer sRGB8. Per file multipagina/animati viene mostrata solo la prima pagina o il primo fotogramma, dichiarandolo nella provenienza.
- RAW: `CIRAWFilter.outputImage`, scala 1 e draft disattivato, con dimensioni native controllate. Nessun fallback alla preview JPEG. Ricetta **TR-linear-v1**: WB e baseline exposure dai metadati/decoder Apple; esposizione aggiunta zero, boost/gamut mapping/lens correction disattivati, sharpening, contrast/detail, noise reduction, moiré e local tone mapping a zero dove supportati. Demosaicing e trasformazioni indispensabili restano quelli Apple; non si promette una ricetta identica a LibRaw o ad altri sviluppatori.
- Provenienza: decoder/versione OS o RAW, ricetta, profilo rilevato/assunto, profondità disponibile, orientamento, SHA-256 e stadi di presentazione. Profondità RAW non riportata = non dichiarata, mai inventata.
- Il corpus continua sul percorso analitico `image/png` precedente. Il sistema di campionamento di ADR 0003 resta condiviso da tutte le viste. Non si aggiunge un ricampionamento sRGB o della UI.

Il supporto RAW è condizionato al modello e al decoder installato: elencare NEF/CR2/CR3/ARW/RAF ecc. non certifica ogni variante. La prova positiva riguarda un DNG Bayer RGGB sintetico 1024×768 senza preview incorporata. Non è una qualifica di fotocamere reali. Little CMS, ICC v2/v4 e CMYK/YCCK completi, precedenza metadati e monitor restano lavoro R1; LibRaw/matrice CFA e gigapixel restano R3. HEIC/WebP anticipano un sottoinsieme che la proposta collocava dopo v1.

## Quote del prototipo

| Risorsa | Quota corrente |
|---|---|
| File sorgente | 256 MiB, controllata prima e durante la lettura |
| Raster sorgente orientato | 67.108.864 pixel, massimo 32.768 per lato nel decoder nativo |
| Pagine/fotogrammi | massimo 256 nel contenitore; si decodifica il primo |
| Render fisico di una vista | 8.388.608 pixel |
| Worker esterno | supervisione a 2 GiB ogni 25 ms; timeout assoluto 45 s |
| Worker corpus | supervisione a 384 MiB; timeout 12 s |
| Cache sorgenti/piramidi | massimo 64 voci e 1.536 MiB |
| Cache presentazione | massimo 64 voci e 128 MiB |

Il campionamento memoria non è un tetto rigido. Cache, job in corso, copie, staging e GPU non hanno ancora un budget globale coordinato. La piramide prende possesso del raster, evitando la precedente copia integrale. Non c'è una promessa di immagini gigapixel: sorgenti troppo grandi vengono rifiutate.

## Evidenze e riproduzione

`scripts/generate-format-fixtures.py` crea i file locali usando formule proprie, ImageIO e `cwebp` per la sola fixture WebP. Nessuna fotografia viene scaricata. `--verify-formats` passa 19 casi: otto famiglie di formato incluso DNG, PNG/TIFF 16 bit, EXIF 1–8 e JPEG 12 MP. Confronta dimensioni e quadranti con un riferimento sRGB analitico; per RAW verifica output completo non costante senza preview. Sei controlli aggiuntivi coprono tre input malformati con recupero sullo stesso PID, firma contro estensione, quota sorgente e osservazione obsoleta. La precisione 16 bit/alpha ha anche una prova Rust separata. Questi controlli non equivalgono a fuzzing o a una matrice fotografica completa.

`--formats-smoke` verifica griglia, DNG completo e crop 1:1 del JPEG 12 MP con tre screenshot. I report sono `reports/formats-macos.json` e `reports/formats-smoke-macos.json`. Le prove di ricampionamento del corpus e la suite XPC vengono ripetute sul pacchetto finale; esiti aggiornati in `reports/VERIFICA.md`.

## Repository e licenza

Repository: [antoniodicoratoinfodev/TrueRenderer](https://github.com/antoniodicoratoinfodev/TrueRenderer). Il file `LICENSE` riserva i diritti sul materiale originale ad Antonio Dicorato, preserva i diritti previsti dai termini GitHub e quelli delle dipendenze. La consultabilità pubblica non concede una licenza open source. Nessun database, backup, immagine personale o toolchain viene pubblicato. I report pubblici e i loro screenshot riguardano solo contenuti sintetici del progetto.

Fonti primarie: [Apple CIRAWFilter](https://developer.apple.com/documentation/coreimage/cirawfilter), [dimensioni native RAW](https://developer.apple.com/documentation/coreimage/cirawfilter/nativesize), [ImageIO](https://developer.apple.com/documentation/imageio), [termini GitHub, contenuti degli utenti](https://docs.github.com/en/site-policy/github-terms/github-terms-of-service#d-user-generated-content). Le API sono state controllate anche negli header dell'SDK locale; le fonti descrivono i contratti, i report misurano l'implementazione.
