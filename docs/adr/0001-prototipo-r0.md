# ADR 0001 — verticale R0 controllata

Data: 5 settembre 2026. Stato: accettata per il prototipo interno, revocabile ai gate R0/R1.

## Raccordo con la 0.1.5 — 9 settembre 2026

Questo ADR conserva contesto, quote e decisioni del 5 settembre 2026. Il confine macOS è stato esteso da [ADR 0002](0002-xpc-decoder-r0.md) e gli esterni ammessi come Anteprima da [ADR 0004](0004-formati-esterni-e-pubblicazione.md). Residenza, budget e compute correnti sono in [ADR 0006](0006-anteprime-residenza-compute.md); stato e prove aggiornate in [avanzamento](../avanzamento.md). I riferimenti al documento sul Desktop descrivono la collocazione iniziale: oggi la copia della radice è `TrueVision-Architettura.md` nella cartella del progetto.

Aggiornamento 0.1.1: le decisioni 4–5 descrivono l’incremento iniziale. Il bundle macOS ora
usa due decoder XPC e supervisione memoria, come registrato in `0002-xpc-decoder-r0.md`.

Aggiornamento 0.1.2: il campionamento della decisione 6 è sostituito dalla pipeline fisica descritta in `0003-campionamento-fisico-r0.md`; le decisioni originali sotto restano storiche.

## Contesto

La proposta v1.2 contiene un programma R0–R4 per macOS arm64/Windows x86-64. Questa sessione dispone di un Mac Apple Silicon, senza un target Windows reale, corpus fotografico autorizzato o prove di sandbox/display. Implementare una v1 dichiarandola già qualificata contraddirebbe i requisiti.

## Decisioni dell'incremento

1. Nome **TrueRenderer**, package `tr-*`; copia iniziale immutata conservata, avanzamento aggiornato nel documento sul Desktop.
2. Rust 1.98.1 locale; egui/eframe 0.36.1 provvisori e wgpu 30.0.1/Metal effettivi, con device/coda condivisi dalla diagnostica e UI. Nessuna decisione finale sul toolkit prima di VoiceOver/NVDA/IME/multimonitor.
3. Backend **PNG Rust** (`image` 0.25.10, `png` 0.18.1) solo per il corpus numerico proprio. È un backend da spike, non la distinta JPEG/PNG/TIFF nativa di R1. ICC incorporati rifiutati; sRGB noto dal corpus.
4. Worker persistente singolo, riciclato dopo 32 job o errore/timeout. Protocollo R0 versionato con header di 20 byte e controllo JSON ≤64 KiB, corpo sorgente ≤32 MiB e raster fp32 ≤8.388.608 pixel. Pipe con copie private al posto di handle/shared memory; deviazione deliberata e testabile per il primo spike. Nessuna UI/SQL nel worker. Il corpo è separato dal framing ma non implementa ancora descrittori/handle della v1.
5. Timeout assoluto 12 s, code finite, limite decoder dichiarato non rigido 256 MiB. **Nessuna sandbox OS e nessuna quota RSS/kernel dimostrata**. Il broker controlla un'allowlist di digest compilata ed elenca soltanto i file esterni senza decodificarli. Nessun `sandbox_init` o aggiramento del gate. Il passaggio a 2 isolati, XPC e AppContainer è lavoro R0 successivo.
6. sRGB analitico -> Rec.2020 lineare fp32, alpha premoltiplicata, riduzione area con footprint rettangolare e accumulo fp64, uscita opaca sRGB8. Nearest di presentazione; Lanczos3/LOD/ICC/grafi di Riferimento non implementati. La diagnostica WGSL confronta il solo stadio working/output su 4.096 campioni entro 1e-4; non abilita badge Standard/Riferimento.
7. SQLite bundled 3.53.2: supera la baseline minima WAL-reset del documento. Library FULL/fullfsync su macOS, indice NORMAL, schema non fidato disattivato e controllo integrità. Un writer thread; lock di istanza, revisioni/annotazioni/journal library nello stesso commit. Backup Online Backup API + verifica, export JSON no-clobber. Non è ancora il journal degli effetti XMP né l'export portabile v1.
8. La UI e le annotazioni sono prototipi del workflow, utili alle prove R0; non chiudono R1/R2 e non impongono di confermare egui. Nessun account, rete, scrittura originali o sidecar.
9. Tutti i file del progetto sul Desktop, dati R0 in `var/` come richiesto. Spostamento in app-data locale qualificato prima della distribuzione.

## Prove e revoca

Report reali in `reports/`, test riproducibili tramite `scripts/verify.sh`. Le prove di R0 non concluse sono nel piano. Se egui fallisce accessibilità/IME/display sui due OS si applica il fallback toolkit dell'architettura. Prima di ammettere file esterni si sostituisce il confine del worker con la primitiva OS qualificata e si superano i test negativi filesystem/rete/resource limits. Le API correnti sono state consultate anche nei sorgenti scaricati e bloccati da Cargo.lock.

## Fonti primarie consultate

- [Installazione Rust/rustup](https://rust-lang.github.io/rustup/installation/)
- [eframe](https://docs.rs/eframe/0.36.1/eframe/) e [renderer](https://docs.rs/eframe/0.36.1/eframe/enum.Renderer.html)
- [image 0.25.10](https://docs.rs/image/0.25.10/image/)
- [rusqlite 0.40.2](https://docs.rs/rusqlite/0.40.2/rusqlite/)
- [dialoghi rfd 0.17.2](https://docs.rs/rfd/0.17.2/rfd/)

Le fonti descrivono API; le prove locali, non questi collegamenti, sostengono i risultati del prototipo.
