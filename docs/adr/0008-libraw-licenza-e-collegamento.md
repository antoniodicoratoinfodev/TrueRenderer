# ADR 0008 — LibRaw: licenza, collegamento e binding

Data: 10 settembre 2026. Stato: decisione presa e integrazione eseguita su Windows; qualifica R3 aperta.

**Aggiornamento del 13 settembre:** l'audit ha richiesto l'aggiornamento a LibRaw **0.22.2**, dal tag ufficiale, conservando CDDL-1.0 e gli avvisi originali. Il manifest `third_party/libraw/manifest-truerenderer.json` registra archivio, hash dei 106 file upstream e le 79 unità compilate; `scripts/verify-libraw.py` verifica copia, opzioni e ricette. Attivato anche `LIBRAW_CALLOC_RAWSTORE`, come raccomandato upstream; nessun sorgente upstream modificato o pack GPL aggiunto. I paragrafi su 0.21.1 sotto descrivono la prima integrazione storica. Le nuove prove Windows e il Mac rinviato sono registrati nell'avanzamento.

## Decisione

Tre scelte, prese insieme perché si vincolano a vicenda.

1. **Licenza: CDDL-1.0**, fra le due che LibRaw offre.
2. **Collegamento: statico**, dentro l'eseguibile.
3. **Binding: uno shim C stretto compilato da `build.rs`**, non bindgen.

Il criterio era il prodotto commerciale proprietario con la distribuzione più semplice possibile. Le tre scelte discendono da quel criterio, non da preferenze di stile.

## Perché CDDL e non LGPL

La nota preliminare dell'8 settembre è stata assorbita in questo ADR: la candidatura iniziale CDDL è diventata la decisione qui registrata.

La CDDL è un copyleft **per file**. Gli obblighi riguardano i file coperti: renderne disponibile il sorgente, conservare le attribuzioni, identificare le proprie modifiche, non limitare nell'EULA i diritti sul sorgente coperto. Il §3.6 permette esplicitamente di combinare il software coperto con altro codice sotto termini diversi, compreso il proprietario, e di distribuire l'opera risultante. Non c'è alcun obbligo di rilink.

La LGPL 2.1 chiede invece che il destinatario possa sostituire la libreria con una propria versione modificata. Con collegamento **statico** il §6 lo soddisfa in un modo solo: distribuire i propri file oggetto, così che l'utente possa rilinkare. Per un prodotto proprietario è inaccettabile. Con collegamento **dinamico** l'obbligo si soddisfa spedendo la DLL separata e permettendone la sostituzione, ma resta un obbligo da progettare e da rispettare in ogni pacchetto.

Quindi: la LGPL rende costoso lo statico e vincolante il dinamico. La CDDL non fa né l'uno né l'altro.

## Perché statico

Discende dalla scelta sopra: una volta rimosso l'obbligo di rilink, non c'è più ragione di pagare il prezzo del dinamico.

Un solo eseguibile significa nessuna DLL da installare, nessun disallineamento di versione fra applicazione e libreria, nessun percorso di caricamento da governare, e un solo file da firmare. Vale sia per MSIX sia per un installer tradizionale, cioè per entrambe le opzioni che §17.1 lascia aperte.

## Obblighi che restano, e sono pochi

- Rendere disponibile ai destinatari il sorgente dei file coperti, con le eventuali modifiche, e indicare come ottenerlo. Il repository è già pubblico: vendorizzare LibRaw lì dentro soddisfa l'obbligo come effetto collaterale della build.
- Conservare licenza e attribuzioni, e identificare le proprie modifiche ai file LibRaw. La correzione di build descritta sotto tocca il wrapper, non i sorgenti LibRaw, che restano intatti.
- Non limitare nell'EULA i diritti CDDL sul sorgente coperto.
- Registrare versione e opzioni nel manifest, come §7 già richiede per la ricetta.

## Trappola verificata: i demosaic pack GPL

LibRaw distribuisce a parte dei pacchetti di demosaicing sotto GPL2 e GPL3, fra cui AMaZE, AFD, VCD e LMMSE. Compilarne uno dentro un prodotto proprietario ne comprometterebbe la licenza.

Verificato sulla copia vendorizzata di LibRaw 0.21.1: contiene le sole `LICENSE.CDDL` e `LICENSE.LGPL`, i file di demosaicing compilati sono i sei del nucleo, e nessun sorgente compilato contiene il testo della GNU General Public License. Il controllo va rifatto a ogni cambio di versione e appartiene al gate di R3, non all'occhio di chi aggiorna la dipendenza.

## Perché uno shim C e non bindgen

Il crate `libraw_rs_vendor` genera i binding con bindgen, che a build time richiede libclang. Aggiungere LLVM alle macchine di build e alla CI, solo per rigenerare a ogni compilazione una superficie che non cambia, è attrito che si paga per sempre.

Lo shim inverte il rapporto. Un file C che espone quattro funzioni: apri da buffer, sviluppa con la ricetta, consegna il raster, chiudi. La ricetta `TR-linear-v1` di §7, con `output_bps`, `output_color`, gamma lineare, `no_auto_bright`, `highlight`, `user_qual` e bilanciamento, vive lì dentro in un punto solo, leggibile, versionato e diffabile. La build richiede allora il solo MSVC.

C'è anche una simmetria che il progetto ha già: il decoder macOS è esattamente questo, un file Objective-C con un header C stretto, compilato da `build.rs`. Il percorso Windows diventa il suo gemello invece di un meccanismo diverso.

Lo shim va inoltre costruito sulle sorgenti `LibRaw_datastream` come chiede §5.5, così la libreria legge il buffer concesso e non riapre mai un nome di file.

## Correzioni di build accertate

Provate su questa macchina, con MSVC 14.51 e LibRaw 0.21.1 vendorizzato.

1. Il build script del crate passa `-Wno-deprecated-declarations` e `-pthread` con `flag`, che MSVC rifiuta con `D8021`. Vanno passati con `flag_if_supported`.
2. Gli header dichiarano i simboli con `__declspec(dllimport)` quando `LIBRAW_NODLL` non è definita, quindi ogni definizione confligge con la propria dichiarazione, `C4273`. Una build statica deve definire `LIBRAW_NODLL`.

Con entrambe applicate i sorgenti C++ compilano. La build si ferma poi su libclang, che è esattamente il motivo della scelta dello shim.

## Prima integrazione del 10 settembre (storico)

LibRaw 0.21.1 vive in `third_party/libraw`, non modificato, 75 unità di traduzione compilate su 78 presenti: le tre escluse sono file inclusi da altri, non compilati per sé. Lo shim è in `native/libraw`, il binding in `crates/tr-worker/src/libraw.rs`, e `crates/tr-worker/build.rs` compila il percorso Apple su macOS e questo su Windows.

Una terza correzione è emersa in integrazione, oltre alle due previste. Le dimensioni finali dell'immagine si ottengono solo chiamando `adjust_sizes_info_only`: lette subito dopo l'identificazione danno i valori prima della rotazione, e il chiamante dimensionerebbe il buffer per un'immagine di forma diversa.

Il nome della ricetta sta in `tr_core::decoder::RECIPE`, letto sia dalla provenienza sia dalla chiave di cache, e un test verifica che coincida con `TR_LIBRAW_RECIPE` nell'header dello shim. Prima la chiave di cache conteneva `Apple-TR-linear-v1` cablato, quindi su Windows avrebbe dichiarato la ricetta Apple mentre i pixel venivano da LibRaw.

Verifica sui 30 NEF Nikon D750 autorizzati: trenta contenitori, trenta sviluppati dal mosaico, nessuna ricaduta sull'anteprima. Il test controlla anche che stadio dichiarato e affermazione sul colore siano coerenti, cioè che lo sviluppo dichiari il proprio spazio e l'anteprima dichiari di assumerlo.

## Conseguenze

L'integrazione richiede il solo MSVC, già presente. Nessuna installazione di LLVM, nessun vcpkg, nessuna dipendenza di infrastruttura oltre a quella che serve già a compilare il workspace.

Restano fuori da questo ADR: la ricetta misurata contro il gate di §7, la matrice camere e il badge `RENDER LIBRAW` di R3, e il parere formale di conformità prima del primo pacchetto distribuito. La scelta di licenza qui registrata è la premessa di quel parere, non il suo sostituto.

Non si dichiara alcun confronto di fedeltà con il percorso Apple: ADR 0004 dice già che una ricetta identica non è promessa, e la tolleranza colorimetrica fra decoder diversi va definita, non presa in prestito dalla soglia ΔE00 del percorso GPU.

## Estensione locale del 12 settembre 2026

Nella sessione del 12 settembre, prima della pubblicazione del 14: selettore di motore, AHD e demosaicing TrueRenderer fp32 affiancati al bilineare. Il nome della ricetta effettiva è ora `RawEngine::recipe()`, serializzato nelle richieste e separato nella cache; `RECIPE` rimane per la compatibilità storica. Il colore RAW è etichettato come sviluppo con ricetta, distinto dal profilo dichiarato da un file bitmap.

Lo shim offre anche estrazione del mosaico e calibrazione per D750/DNG sintetici, con gestione delle eccezioni al confine C. L'AHD impiega il nucleo già vendorizzato; nessun file upstream o licenza è stato modificato. MSVC 14.51 richiede qui un archivio creato in una sola invocazione con response file: il merge progressivo di `cc` falliva con LNK1114. La build macOS aggiunge LibRaw ai componenti Apple e libc++ ai due XPC; resta da costruire e verificare su Mac. Risultati, ricette, limiti e istruzioni in [progetto motori RAW](../progetto-motori-raw.md).

## Fonti e stato della nota preliminare assorbita

La nota dell'8 settembre precedeva l'integrazione: quella baseline usava CIRAWFilter e non incorporava LibRaw. La scelta successiva è CDDL-1.0, con LibRaw 0.22.2 pubblicata in `67146e3`; versione e opzioni sono fissate nel manifest. Il nuovo bundle macOS resta da verificare.

Fonti conservate dalla nota: [presentazione ufficiale LibRaw](https://www.libraw.org/about), [testo CDDL incluso](../../third_party/libraw/LICENSE.CDDL) e [alternativa LGPL inclusa](../../third_party/libraw/LICENSE.LGPL). Questi testi e le attribuzioni upstream rimangono nel repository. L'accorpamento documentale non è un nuovo parere legale né una qualifica di un pacchetto commerciale; il controllo di distribuzione resta quello indicato nelle conseguenze e in [NOTICE](../../NOTICE.md).
