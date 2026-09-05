## 0. Sviluppo di TrueRenderer — piano e avanzamento

**Nome corrente: TrueRenderer.** TrueVision è il nome precedente del progetto. I riferimenti storici ai backup conservano il nome originale.

**Ultimo aggiornamento: 6 settembre 2026. Stato: prototipo R0 funzionante; bundle pronto, primo avvio Finder in attesa del consenso macOS al Desktop.**

Progetto: `/Users/antonio/Desktop/TrueRenderer`. Piano completo: `PLAN.md` nella cartella del progetto. Backup immutato della proposta v1.2: `docs/TrueVision-Architettura.originale-v1.2.md`.

| Area | Stato effettivo | Prossima attività |
|---|---|---|
| Piano | Preparata roadmap R0–R4 con dipendenze e gate | Aggiornare dopo ogni incremento |
| Ambiente | Rust 1.98.1 locale, Cargo.lock e inventario di 206 package build/test macOS | Qualifica dipendenze e canali R0/R4 |
| Codice | 6 crate + desktop compilabili; griglia, viewer, confronto, annotazioni, backup/undo e corpus PNG | Proseguire le prove e i confini mancanti di R0 |
| Verifica | 23 test Rust e 6 prove IPC passati; fmt/clippy senza avvisi; smoke nativo Metal e 3 screenshot | Verifica finale del bundle macOS |
| GPU | Apple M4/Metal: 4.096 campioni, errore massimo 9,54e-7 contro soglia 1e-4 | ICC/LUT, display e recovery non qualificati |
| Pacchetto | Bundle release arm64 pronto e firma ad hoc verificata; avvio diretto testato | Primo avvio Finder attende il consenso Desktop di macOS |
| R0 | Aperto | Sandbox, display, Windows, accessibilità e corpus reale |
| R1–R4 | Pianificate, non completate | Gate descritti nel piano e in §22 |

**Limite di questa fase.** È un prototipo di sviluppo, non una v1 qualificata. Standard/Riferimento, RAW, gigapixel e scrittura XMP non possono essere dichiarati disponibili senza i rispettivi test. Fino al gate sandbox i decoder vengono usati solo su corpus controllato, come stabilito dal documento.

**Registro attività**

- 05/09/2026: letti i requisiti principali, creata la cartella, conservato il documento originale, registrato il cambio nome e il piano operativo; installata la toolchain locale.

- 05/09/2026: creati workspace e Cargo.lock, 12 immagini numeriche PNG proprie (8/16 bit), pipeline fp32 con alpha, IPC su pipe e allowlist dei digest; aggiunti libreria/index separati, revisioni/undo/backup, UI egui/wgpu e shader diagnostico. Superati i 10 test iniziali del nucleo. Correzioni di integrazione API in corso.

- 06/09/2026: completata la prima verticale R0 e superati 23 test Rust, 6 controlli avversari IPC, lint e smoke nativo su Apple M4/Metal. Verificati errori di decoder, timeout, riavvio dopo crash, persistenza/restore del backup, undo e servizio asincrono. Salvate le tre schermate e l’inventario dipendenze; corretto il layout del confronto. Build release prodotta, firma del bundle in completamento dopo una collisione di nomi del launcher.

- 06/09/2026: completati bundle release, firma ad hoc verificata, rilevamento della cartella a partire dall’eseguibile e messaggio privacy Desktop. Il primo avvio via Finder è in attesa del consenso macOS (`AUTHREQ_PROMPTING` verificato nei log TCC); nessuna modifica automatica alle autorizzazioni. Dettagli in `reports/VERIFICA.md`.

**Prossimo incremento tecnico:** qualifica della sandbox macOS XPC con test negativi, prova Windows x86-64, contratto display e accessibilità. Solo dopo questi gate si possono ammettere archivi esterni e completare viewer SDR/ICC/JPEG/TIFF di R1. R2 aggiunge catalogo completo e XMP; R3 RAW/gigapixel; R4 hardening e rilascio.
