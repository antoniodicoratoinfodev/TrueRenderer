# Piano futuro per l'ia generativa nell'app

**Possiamo progettare queste funzioni per una versione commerciale di TrueRenderer, mantenendo l’app proprietaria, l’elaborazione locale e l’assenza di abbonamenti obbligatori.** La scelta dei componenti deve partire da due requisiti insieme: qualità tecnica e diritto di distribuirli ai clienti.

La proposta è **pochi modelli selezionati e verificati, integrati in strumenti fotografici nostri, con pacchetti installabili e funzionamento offline.** Le indicazioni sulle licenze qui sotto sono una selezione preliminare basata sulle fonti ufficiali; prima della vendita il pacchetto effettivo dovrà essere verificato da un professionista esperto di licenze software e IA.

**La distinzione fondamentale riguarda quattro diritti diversi:**

- usare e distribuire il **codice** del motore IA;
- usare, modificare e distribuire i **pesi del modello**, cioè i file che contengono ciò che ha appreso;
- distribuire **encoder, tokenizer, librerie e altri componenti** necessari;
- consentire ai clienti l’utilizzo dei **risultati**, anche professionale e commerciale.

Una licenza permissiva del codice non dimostra automaticamente che tutti i pesi scaricabili abbiano le stesse condizioni. E il permesso di vendere immagini generate non equivale al permesso di incorporare il modello in un’app venduta.

**TrueRenderer può restare proprietario.** Componenti sotto MIT, BSD e Apache 2.0 possono generalmente essere integrati in prodotti commerciali a codice chiuso, rispettando le condizioni applicabili: testi di licenza, attribuzioni, avvisi e, per Apache, indicazione delle modifiche e conservazione degli eventuali `NOTICE`. Apache 2.0 comprende anche una concessione brevettuale delimitata, ma non concede i marchi né garantisce l’assenza di diritti di terzi. [Licenza Apache 2.0](https://www.apache.org/licenses/LICENSE-2.0).

Conserveremmo quindi i diritti sul nostro codice, sull’interfaccia e sull’integrazione; i componenti esterni manterrebbero le proprie licenze. La nostra licenza utente dovrebbe rispettare questa distinzione.

Non escluderei una dipendenza perché “open source”: **open source e commerciale sono compatibili**. Escluderei invece dalla configurazione scelta le condizioni incompatibili con il nostro prodotto. Le licenze copyleft richiedono una valutazione specifica; collocare un componente in un processo separato non è una soluzione legale automatica.

**Le funzioni da costruire:**

| Strumento | Comportamento proposto |
|---|---|
| Riduzione rumore IA | Ridurre il rumore conservando trama, colore e dettaglio |
| Nitidezza e recupero del mosso | Migliorare la leggibilità con intensità controllabile |
| Ingrandimento IA | Aumentare la risoluzione, inizialmente 2× |
| Rimozione intelligente | Selezionare un oggetto e ricostruire lo sfondo |
| Pennello generativo | Dipingere un’area e descrivere cosa inserire o sostituire |
| Espansione generativa | Allargare la tela e ricostruire i bordi mancanti |
| Generazione di immagini | Creare immagini o grafiche raster su nuovi documenti/livelli |

Separerei nell’interfaccia **miglioramento fotografico e generazione creativa**. Anche un modello di restauro può introdurre dettagli inesatti; non venderei quindi la nitidezza IA come recupero garantito di informazioni realmente presenti nella scena.

Il timbro clone tradizionale continuerebbe a servire per copiare campioni esistenti. Il pennello generativo aggiungerebbe il flusso “seleziono, descrivo, confronto le varianti, applico”.

**La scelta iniziale dei modelli in ottica commerciale:**

| Candidato | Valutazione per TrueRenderer commerciale |
|---|---|
| **FLUX.2 klein 4B** | Candidato prioritario per generazione ed editing: la versione specifica è dichiarata Apache 2.0. Da verificare l’intero pacchetto e la qualità dei ritocchi locali. |
| **FLUX.2 klein 9B e FLUX.2 dev** | Esclusi dal pacchetto standard proposto: hanno licenze non commerciali. Un eventuale accordo commerciale sarebbe una scelta separata. |
| **Real-ESRGAN** | Candidato per ingrandimento: il codice è BSD-3-Clause. Prima di distribuirlo, documentare anche i diritti dello specifico checkpoint e delle dipendenze incluse. |
| **NAFNet** | Candidato tecnico per denoise/deblur, ma non ancora approvato per il prodotto: la licenza del codice non basta a chiudere la verifica sui pesi. |
| **SDXL Inpainting** | Non lo metterei nella prima distribuzione senza chiarimenti: la scheda indica Open RAIL++ e contiene anche una dichiarazione di destinazione alla ricerca. Va risolta l’interpretazione applicabile al checkpoint. |

Le differenze fra le varianti FLUX sono riportate nel [repository ufficiale](https://github.com/black-forest-labs/flux2). Per gli altri candidati: [licenza Real-ESRGAN](https://github.com/xinntao/Real-ESRGAN/blob/master/LICENSE), [licenza NAFNet](https://github.com/megvii-research/NAFNet/blob/main/LICENSE), [richiesta di chiarimento sui pesi NAFNet](https://github.com/megvii-research/NAFNet/issues/194), [scheda SDXL Inpainting](https://huggingface.co/diffusers/stable-diffusion-xl-1.0-inpainting-0.1).

Questo non significa che Real-ESRGAN o NAFNet siano necessariamente vietati commercialmente: significa che **non li dichiarerei distribuibili sulla sola base della licenza del repository**.

Se un candidato valido avesse diritti poco chiari, le alternative sarebbero ottenere una conferma scritta dal titolare, scegliere un modello con condizioni esplicite oppure addestrare un modello su materiale utilizzabile a tale scopo. Non partirei dall’addestramento di un grande generatore nostro: richiederebbe risorse molto maggiori. Un modello fotografico specializzato potrebbe invece diventare un investimento successivo.

**Per Mac, NVIDIA e AMD manterrei un’unica esperienza utente e motori interni diversi.**

- **Apple Silicon:** Core ML come primo candidato per restauro e modelli convertibili; Metal/MLX per la generazione quando più conveniente. Core ML può utilizzare CPU, GPU e Neural Engine, ma la distribuzione effettiva del lavoro dipende dal modello. [Apple Core ML](https://developer.apple.com/documentation/CoreML).
- **Windows NVIDIA:** ONNX Runtime con CUDA per i modelli supportati; eventuali ottimizzazioni TensorRT dopo le misure.
- **Windows AMD:** Windows ML/ONNX per il restauro e un percorso Vulkan da qualificare per la generazione. La compatibilità va dichiarata per scheda, driver e modello: il provider AMD MIGraphX di Windows ML, per esempio, attualmente non supporta gli scenari GenAI. [Provider Windows ML](https://learn.microsoft.com/en-us/windows/ai/new-windows-ml/supported-execution-providers?tabs=winml2).

ONNX Runtime, MLX e `stable-diffusion.cpp` hanno licenze MIT nei rispettivi repository; restano da verificare dipendenze e componenti concretamente distribuiti. Le librerie NVIDIA hanno invece proprie condizioni e un perimetro di redistribuzione: la licenza del nostro motore non le sostituisce. [ONNX Runtime](https://github.com/microsoft/onnxruntime/blob/main/LICENSE), [MLX](https://github.com/ml-explore/mlx/blob/main/LICENSE), [stable-diffusion.cpp](https://github.com/leejet/stable-diffusion.cpp/blob/master/LICENSE), [licenza CUDA](https://docs.nvidia.com/cuda/eula/index.html).

**Non farei dipendere questi strumenti da DLSS, FSR o MetalFX.** Sono tecnologie nate per il rendering e non risolvono il riempimento guidato da testo o il restauro fotografico. Sfrutteremmo comunque GPU e acceleratori disponibili, tramite motori adatti ai modelli scelti. [NVIDIA DLSS](https://developer.nvidia.com/rtx/dlss), [AMD FSR](https://gpuopen.com/fidelityfx-super-resolution-3/), [Apple MetalFX](https://developer.apple.com/documentation/MetalFX).

**Il MacBook Air M4 deve essere un dispositivo di riferimento fin dall’inizio.** Imposterei un obiettivo iniziale su 16 GB di memoria, da verificare con modelli compatti, una generazione alla volta e lavorazione per regioni. Le macchine più potenti potrebbero usare risoluzioni e modelli maggiori.

Non pubblicherei requisiti minimi o tempi promessi prima dei benchmark. Per commercializzare l’app dobbiamo misurare anche sessioni prolungate, pressione della memoria, consumi e reattività dell’interfaccia.

Nell’architettura di TrueRenderer aggiungerei:

- un **gestore dei modelli**, con versione, provenienza, licenza, hash e dimensione;
- un **processo IA dedicato**, con avanzamento, annullamento e recupero dagli errori;
- una gestione comune dei budget di memoria con viewer e sviluppo;
- **risultati salvati come livelli raster**, con maschere, opacità e undo;
- un contratto esplicito di conversione fra lo spazio colore del documento e quello previsto dal modello.

Il progetto deve conservare i pixel generati, oltre a prompt e parametri. Deve riaprirsi anche se il modello è stato rimosso: il solo seed non garantisce risultati identici su hardware o versioni differenti.

Per l’ingrandimento bisogna inoltre estendere il percorso delle grandi immagini: una foto da 24 MP ingrandita 2× per lato diventa da 96 MP, oltre il limite corrente di circa 67 MP. Serve lavorazione ed export per blocchi, con memoria controllata.

**Commercialmente proporrei una licenza perpetua della versione acquistata**, con eventuali aggiornamenti maggiori a pagamento e pacchetti IA scaricabili separatamente. È una proposta di prodotto, da definire nei termini di vendita.

Dopo l’installazione dei pacchetti:

- elaborazione completamente locale;
- nessun credito per immagine;
- nessuna connessione periodica necessaria per usare le funzioni acquistate;
- possibilità di installare i pacchetti anche da file locale.

Resterebbero i nostri costi di sviluppo, distribuzione, firma, hosting dei download e assistenza. L’obiettivo realistico è **assenza di costi variabili obbligatori di inferenza e di royalty sui componenti selezionati**, non assenza di costi aziendali.

Far scaricare un modello all’utente invece di includerlo nell’installer non rende automaticamente lecita un’integrazione commerciale altrimenti vietata.

**Sui diritti delle immagini generate dobbiamo formulare una promessa precisa.** Sceglierei componenti che consentano l’uso commerciale previsto e stabilirei che TrueRenderer non rivendica diritti sulle fotografie o sui risultati dei clienti. Non prometterei invece esclusività, registrabilità del copyright o assenza assoluta di diritti di terzi.

Sono aspetti distinti: per esempio, negli Stati Uniti il Copyright Office distingue i contributi creativi umani dai contenuti interamente generati e considera insufficienti i soli prompt per stabilire la paternità dell’output. Le regole vanno valutate nei mercati di vendita. [Rapporto ufficiale sul copyright degli output](https://www.copyright.gov/ai/Copyright-and-Artificial-Intelligence-Part-2-Copyrightability-Report.pdf).

Per il mercato europeo inserirei inoltre la trasparenza dei contenuti generativi nella progettazione dell’export. L’articolo 50 dell’AI Act prevede obblighi di marcatura leggibile dalle macchine, con eccezioni per determinate funzioni di editing assistivo; non possiamo presumere che denoise e sostituzione generativa di oggetti abbiano lo stesso trattamento. Il funzionamento offline, da solo, non risolve questa valutazione. [AI Act, articolo 50](https://eur-lex.europa.eu/legal-content/EN/ALL/?uri=CELEX%3A32024R1689).

**Ordine proposto:**

1. Definire i requisiti commerciali: app proprietaria, uso professionale dei risultati, offline e nessuna royalty obbligatoria per immagine.
2. Preparare un inventario dei componenti candidati, separando codice, pesi e dipendenze, con condizioni e questioni aperte.
3. Completare livelli raster e salvataggio dei documenti.
4. Provare un modello di restauro e un generatore su M4, NVIDIA e AMD.
5. Selezionare soltanto i pacchetti che superano sia la verifica dei diritti sia quella di qualità e prestazioni.
6. Consegnare progressivamente restauro, pennello generativo, espansione e nuovi documenti.

**La scelta raccomandata è vendere il valore di TrueRenderer — strumenti, qualità fotografica, integrazione e affidabilità — usando una selezione controllata di tecnologie esterne.** Il primo investimento dovrebbe essere una verifica documentata dei diritti insieme a un prototipo piccolo: ci permetterebbe di costruire il prodotto commerciale attorno a componenti che possiamo realmente distribuire e mantenere.
