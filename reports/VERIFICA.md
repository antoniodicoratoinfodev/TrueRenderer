# TrueRenderer — verifica del prototipo R0

Aggiornamento: 6 settembre 2026. Build interna **0.1.1**, installata in `dist/TrueRenderer.app`.

## Risultati osservati

| Controllo | Esito | Evidenza |
|---|---|---|
| Workspace Rust, debug e release | Passato | `verification.log`, `build-macos.log` |
| rustfmt e Clippy con `-D warnings` | Passato | `verification.log`, incluse le ultime modifiche UI |
| Test Rust | 26 passati | 23 normali + 3 integrazioni eseguite esplicitamente dopo la build |
| IPC avversario sul binario worker | 6 controlli passati | `worker-protocol.json` |
| Timeout del processo | Passato | worker fermo interrotto entro il limite del test |
| Crash e riciclo del worker reale | Passato | decodifica, kill, errore contenuto, nuova decodifica |
| Gate nel decoder | Passato | input fuori allowlist rifiutato prima del codec; il successivo job del corpus passa |
| Salvataggio durante decoder bloccato | Passato | test del servizio: commit delle annotazioni e shutdown entro i limiti |
| Cambio dominio con decoder inattivo | Passato | worker reale riciclato senza inviare un nuovo job |
| Libreria, revisioni e undo | Passato | conflitto di revisione, rollback, undo con incremento, servizio asincrono |
| Backup/restore | Passato | Online Backup API, controllo integrità e restore verificato in database separato |
| Eliminazione dell'indice | Passato | rating e UUID conservati dopo ricostruzione dell'indice |
| Native UI nel bundle 0.1.1 | Passato | Metal/Apple M4, 12 immagini, griglia/anteprima/confronto, tre screenshot controllati |
| Calcolo WGSL vs CPU | Passato per il solo stadio provato | 4.096 campioni, massimo errore assoluto 9,536743e-7; soglia 1e-4 |
| Bundle `.app`, servizi XPC, plist, arm64 e firma ad hoc | Passato | `package-macos.json`: firma strict, entitlements dei servizi, hash dei quattro binari |
| Avvio 0.1.1 via Finder/LaunchServices | Passato dopo il consenso macOS | `finder-macos.json`: 12 immagini, zero errori, Metal, due PID XPC |
| Copia indipendente del runtime 0.1.1 | Passato | `relocation-check.json`, `relocation-macos.json`: bundle/corpus copiati, XPC e database integri nella nuova cartella |
| Laboratorio XPC con App Sandbox | Controlli del laboratorio passati | `xpc-sandbox-macos.json`: file/rete negati, FD readonly, versione errata e crash contenuti |
| Laboratorio XPC con limite processi | Controlli del laboratorio passati | `xpc-sandbox-limited-macos.json`: `RLIMIT_NPROC` 0 blocca figli e non può essere rialzato |
| Controllo del processo tramite task/audit token | Prova SPI locale passata | `xpc-control-macos.json`; `task_terminate` non è utilizzabile per questo processo BSD |
| Decoder XPC integrati | Passato | `xpc-integration-macos.json`: due processi, pixel identici, 12 immagini, timeout e recupero indipendente |
| Memoria in un servizio di fault injection separato | Passato per la traiettoria provata | `xpc-memory-growth-macos.json`: soglia superata, servizio terminato e sostituito |
| Esclusione della fault injection dal bundle normale | Passato | `xpc-qualification-macos.json`: comando rifiutato, hash confrontati con il pacchetto installato |

La richiesta del Desktop è stata osservata nei log TCC (`AUTHREQ_PROMPTING` per `it.truerenderer.prototype`), anche dopo il cambiamento del codice firmato ad hoc; i successivi avvii sono terminati con esito positivo. Il progetto conserva corpus e libreria sul Desktop come richiesto. Il consenso non è stato dato automaticamente e le impostazioni privacy non sono state modificate. Il messaggio dell'app è dichiarato in `NSDesktopFolderUsageDescription`.

Fonti e spiegazione del comportamento: [documentazione Apple della chiave Desktop](https://developer.apple.com/documentation/bundleresources/information-property-list/nsdesktopfolderusagedescription), [controllo degli accessi ai file macOS](https://support.apple.com/guide/security/controlling-app-access-to-files-secddd1d86a6/web).

## Mac di prova

- macOS 26.6.2, build 25G83; architettura arm64.
- Adattatore wgpu: Apple M4, backend Metal; superficie Bgra8Unorm.
- Rust 1.98.1; egui/eframe 0.36.1; wgpu 30.0.1; SQLite bundled 3.53.2.
- Corpus sintetico generato nel repository: 12 PNG, 8/16 bit e trasparenza.

I tempi registrati nello smoke sono misure di una singola esecuzione e non sono benchmark p95.

## Misure XPC 0.1.1

La build release ha usato due PID distinti. Un decoder sospeso è stato fermato in **216 ms**
nel test con timeout da 200 ms; l’altro ha continuato a decodificare. Il riavvio è riuscito
con un nuovo processo. Il giro completo dura circa 10,93 s e include il ritardo di launchd.

Il servizio avversario separato alloca 512 MiB a passi nel singolo comando di prova e ignora
gli errori di disconnessione XPC. Il broker campiona ogni 25 ms, con soglia 384 MiB:
picco osservato **408.731.648 byte**, superamento **6.078.464 byte**, circa **5,8 MiB**.
La terminazione è avvenuta 1.099 ms dopo l’inizio della crescita; questo tempo non misura
la latenza a partire dall’attraversamento della soglia. Il decoder successivo ha funzionato.
La fault injection è compilata soltanto nel bundle di prova sotto `var/`; il servizio normale
rifiuta quel comando. Una traiettoria non stabilisce il massimo overshoot possibile.

Il broker verifica il CDHash del servizio installato e usa il task del kernel per la misura;
la terminazione impiega un audit token e libproc SPI, senza fallback a un segnale al solo PID.
Le impronte dei binari sottoposti alla suite XPC coincidono con quelle del pacchetto installato.
Lettura aggiuntiva dei database rilocati: file chiusi senza WAL, controllo di integrità readonly
con SQLite di sistema 3.51.0; l’app e i test di persistenza usano sempre la versione bundled 3.53.2.

## Cosa questi risultati non dimostrano

Il decoder è ora integrato con XPC, ma il gate completo della sandbox resta aperto. La firma
ad hoc non autentica un distributore e il servizio verifica soltanto l’identificatore dell’host;
il requisito reciproco del firmatario di release rimane da qualificare. La terminazione usa una
SPI, provata su questo Mac: non è una promessa di supporto per tutte le versioni macOS.
Mancano corpus avversario esteso, handle residui/duplicati, quote end-to-end, bootstrap ostile
e pressione del sistema. La supervisione memoria non è un tetto rigido imposto dal kernel.
App Sandbox da sola consente il figlio `/usr/bin/true`; il limite aggiuntivo è stato provato
soltanto sul Mac indicato. I due PID dell’app derivano da due servizi distinti; due connessioni
allo stesso servizio nel laboratorio condividevano un PID. Dettagli in `docs/adr/0002-xpc-decoder-r0.md`.

Non sono prove dell'accessibilità con VoiceOver/NVDA, di Windows, del recupero da device loss, della gestione ICC o della fedeltà del display. Il test GPU non misura ΔE00, LUT, gamut o filtro Lanczos3. Non qualificano RAW, XMP, JPEG/TIFF nativi o gigapixel. La rilocazione riguarda i file necessari al runtime; non prova una ricompilazione della toolchain dopo spostamento. I render rimangono Anteprima e R0 resta aperto.

## Riproduzione

`scripts/verify.sh` esegue build/lint/test e protocollo. `scripts/verify.sh --gui` aggiunge lo
smoke del binario fuori bundle, che usa pipe. `scripts/build-macos.sh` crea il bundle con XPC;
`python3 scripts/test-xpc-integration.py` ne verifica decoder, timeout e rifiuto della fault injection.
Il test LaunchServices è `open -n -W dist/TrueRenderer.app --args --smoke-test`.
L’avvio del bundle può richiedere di consentire l’accesso al Desktop nella finestra di macOS.
`python3 scripts/verify-package-macos.py` controlla firma, entitlements e hash dopo che i report
Finder e rilocazione della versione corrente sono stati salvati. Le copie per la rilocazione
devono contenere un proprio `corpus/`, il bundle in `dist/` e nessun symlink verso gli originali.

Il laboratorio XPC usa `python3 scripts/build-xpc-probe.py` e `python3 scripts/test-xpc-probe.py`;
ripetere entrambi con `--limited` per la seconda variante. La prova di crescita si riproduce con
i comandi in ADR 0002. Le suite XPC sono separate dai 26 test Rust e dalle 6 prove IPC del worker.
