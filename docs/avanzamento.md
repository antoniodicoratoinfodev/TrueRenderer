## 0. Sviluppo di TrueRenderer — piano e avanzamento

**Nome corrente: TrueRenderer.** TrueVision è il nome precedente del progetto. I riferimenti storici ai backup conservano il nome originale.

**Ultimo aggiornamento: 6 settembre 2026. Stato: TrueRenderer 0.1.1 installato sul Desktop; integrazione XPC, prove native, avvio Finder e copia autonoma passati. R0 complessivo rimane aperto.**

Progetto: `/Users/antonio/Desktop/TrueRenderer`. Piano completo: `PLAN.md` nella cartella del progetto. Backup immutato della proposta v1.2: `docs/TrueVision-Architettura.originale-v1.2.md`.

| Area | Stato effettivo | Prossima attività |
|---|---|---|
| Piano | Preparata roadmap R0–R4 con dipendenze e gate | Aggiornare dopo ogni incremento |
| Ambiente | Rust 1.98.1 locale, Cargo.lock e inventario di 206 package build/test macOS | Qualifica dipendenze e canali R0/R4 |
| Codice | 6 crate + desktop; due decoder XPC separati dal writer delle annotazioni, coda finita e cancellazione | Proseguire le prove e i confini mancanti di R0 |
| Verifica | 26 test Rust e 6 prove IPC passati; XPC, timeout, crescita memoria, Finder e copia autonoma verificati | Estendere suite avversaria e prove sui target mancanti |
| GPU | Apple M4/Metal: 4.096 campioni, errore massimo 9,54e-7 contro soglia 1e-4 | ICC/LUT, display e recovery non qualificati |
| Pacchetto | 0.1.1 release arm64 con due servizi XPC; firma, hash, Finder e copia indipendente verificati | Firma di distribuzione e target Windows in R4 |
| Sandbox macOS | Decoder integrato in due servizi, CDHash verificato dal broker, controllo Mach/audit token, soglia 384 MiB | Firma di release, compatibilità libproc SPI e gate avversari completi |
| R0 | Aperto | Sandbox, display, Windows, accessibilità e corpus reale |
| R1–R4 | Pianificate, non completate | Gate descritti nel piano e in §22 |

**Limite di questa fase.** È un prototipo di sviluppo, non una v1 qualificata. Standard/Riferimento, RAW, gigapixel e scrittura XMP non possono essere dichiarati disponibili senza i rispettivi test. Fino al gate sandbox i decoder vengono usati solo su corpus controllato, come stabilito dal documento.

**Registro attività**

- 06/09/2026, incremento 0.1.1: integrato il decoder Rust in due servizi XPC incorporati, senza percorsi nel protocollo e con allowlist verificata anche nel servizio. Due thread servono le anteprime mentre SQLite salva in un writer separato. Test con decoder bloccato: salvataggio annotazioni e shutdown passati.

- 06/09/2026, prove native 0.1.1: due PID distinti e pixel identici, un decoder sospeso terminato in 216 ms nel test da 200 ms sulla build finale, altro isolato operativo e riavvio passato. Crescita controllata: massimo osservato 408.731.648 byte, 6.078.464 byte oltre soglia (circa 5,8 MiB), recupero passato. È una traiettoria misurata, non un tetto rigido. Il comando di fault injection è rifiutato dal bundle normale.

- 06/09/2026, revisione finale 0.1.1: corretto un caso di revoca tardiva, riciclando anche il decoder inattivo al cambio cartella senza attendere nuovi job. Il nuovo test con worker reale passa; totale 26 test Rust. Ripulite etichette con frecce non disponibili nel font. Il pacchetto aggiornato ha superato Finder e copia autonoma: 12 immagini, due servizi XPC, zero errori e tre schermate controllate. Verificati firma, hash dei binari e integrità dei database della copia. Pacchetti precedenti conservati in `var/package-history/`; backup originale dell’architettura e 319 notice invariati nei byte.

- 05/09/2026: letti i requisiti principali, creata la cartella, conservato il documento originale, registrato il cambio nome e il piano operativo; installata la toolchain locale.

- 05/09/2026: creati workspace e Cargo.lock, 12 immagini numeriche PNG proprie (8/16 bit), pipeline fp32 con alpha, IPC su pipe e allowlist dei digest; aggiunti libreria/index separati, revisioni/undo/backup, UI egui/wgpu e shader diagnostico. Superati i 10 test iniziali del nucleo. Correzioni di integrazione API in corso.

- 06/09/2026: completata la prima verticale R0 e superati 23 test Rust, 6 controlli avversari IPC, lint e smoke nativo su Apple M4/Metal. Verificati errori di decoder, timeout, riavvio dopo crash, persistenza/restore del backup, undo e servizio asincrono. Salvate le tre schermate e l’inventario dipendenze; corretto il layout del confronto. Build release prodotta, firma del bundle in completamento dopo una collisione di nomi del launcher.

- 06/09/2026: completati bundle release, firma ad hoc verificata, rilevamento della cartella a partire dall’eseguibile e messaggio privacy Desktop. Il primo avvio via Finder è in attesa del consenso macOS (`AUTHREQ_PROMPTING` verificato nei log TCC); nessuna modifica automatica alle autorizzazioni. Dettagli in `reports/VERIFICA.md`.

- 06/09/2026: superato il primo avvio Finder dopo il consenso Desktop di macOS (12 immagini, nessun errore, Metal). Superata anche la copia autonoma di bundle e corpus: report e database creati nella nuova cartella. Verificati i byte del backup originale e di tutti i 319 notice delle dipendenze anche nel repository Git locale.

- 06/09/2026: completato `experiments/macos-xpc/` con due suite eseguite sul Mac. Il servizio con App Sandbox nega lettura/scrittura/creazione esterne e rete, accetta il FD readonly e nega la scrittura; versione sconosciuta e crash rimangono contenuti. Due connessioni condividono un PID. La sandbox base consente figli; la variante con `RLIMIT_NPROC` hard/soft 0 li blocca (`EAGAIN`) e non può rialzare il limite (`EPERM`). Misurata un’allocazione di 16 MiB, senza dichiarare un tetto memoria. Report e istruzioni conservati; il decoder del prototipo mantiene la allowlist.

**Limiti nuovi espliciti:** la firma ad hoc non qualifica un distributore; il servizio verifica l’identificatore dell’host, mentre il broker vincola il CDHash del decoder. La terminazione usa libproc SPI e audit token ottenuto dal task del kernel: provata su questo Mac, da qualificare per il rilascio. Nessun ripiego su segnali al solo PID. Dettagli in `docs/adr/0002-xpc-decoder-r0.md`.

**Prossimo incremento tecnico:** estendere i test avversari del bootstrap XPC, dell’output e degli handle residui; misurare memoria end-to-end e pressione, chiudere la scelta di firma/API e proseguire Windows x86-64, display e accessibilità. Solo dopo i gate pertinenti si possono ammettere archivi esterni e completare viewer SDR/ICC/JPEG/TIFF di R1. R2 aggiunge catalogo completo e XMP; R3 RAW/gigapixel; R4 hardening e rilascio.
