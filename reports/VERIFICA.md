# TrueRenderer — verifica del prototipo R0

Aggiornamento: 6 settembre 2026. Build interna 0.1.0.

## Risultati osservati

| Controllo | Esito | Evidenza |
|---|---|---|
| Workspace Rust, debug e release | Passato | `verification.log`, `build-macos.log` |
| rustfmt e Clippy con `-D warnings` | Passato | `scripts/verify.sh`, controllo ripetuto dopo la modifica dell'avvio bundle |
| Test Rust | 23 passati | 21 normali + 2 integrazioni eseguite esplicitamente dopo la build |
| IPC avversario sul binario worker | 6 controlli passati | `worker-protocol.json` |
| Timeout del processo | Passato | worker fermo interrotto entro il limite del test |
| Crash e riciclo del worker reale | Passato | decodifica, kill, errore contenuto, nuova decodifica |
| Libreria, revisioni e undo | Passato | conflitto di revisione, rollback, undo con incremento, servizio asincrono |
| Backup/restore | Passato | Online Backup API, controllo integrità e restore verificato in database separato |
| Eliminazione dell'indice | Passato | rating e UUID conservati dopo ricostruzione dell'indice |
| Native UI da binario | Passato | Metal/Apple M4, 12 immagini, griglia/anteprima/confronto, tre screenshot |
| Calcolo WGSL vs CPU | Passato per il solo stadio provato | 4.096 campioni, massimo errore assoluto 9,536743e-7; soglia 1e-4 |
| Bundle `.app`, plist, arm64 e firma ad hoc | Passato | `codesign --verify --deep --strict`, plist valido, Mach-O arm64 |
| Primo avvio del bundle via Finder/LaunchServices | Passato dopo il consenso macOS | `finder-macos.json`: 12 immagini, zero errori, Metal, 60 frame |
| Copia indipendente del runtime | Passato | `relocation-check.json`, `relocation-macos.json`: bundle/corpus copiati, database nella nuova cartella |
| Laboratorio XPC con App Sandbox | Controlli del laboratorio passati | `xpc-sandbox-macos.json`: file/rete negati, FD readonly, versione errata e crash contenuti |
| Laboratorio XPC con limite processi | Controlli del laboratorio passati | `xpc-sandbox-limited-macos.json`: `RLIMIT_NPROC` 0 blocca figli e non può essere rialzato |

La richiesta iniziale del Desktop è stata osservata nei log TCC (`AUTHREQ_PROMPTING` per `it.truerenderer.prototype`); il successivo avvio è terminato con esito positivo. Il progetto conserva corpus e libreria sul Desktop come richiesto. Il consenso non è stato dato automaticamente e le impostazioni privacy non sono state modificate. Il messaggio dell'app è dichiarato in `NSDesktopFolderUsageDescription`.

Fonti e spiegazione del comportamento: [documentazione Apple della chiave Desktop](https://developer.apple.com/documentation/bundleresources/information-property-list/nsdesktopfolderusagedescription), [controllo degli accessi ai file macOS](https://support.apple.com/guide/security/controlling-app-access-to-files-secddd1d86a6/web).

## Mac di prova

- macOS 26.6.2, build 25G83; architettura arm64.
- Adattatore wgpu: Apple M4, backend Metal; superficie Bgra8Unorm.
- Rust 1.98.1; egui/eframe 0.36.1; wgpu 30.0.1; SQLite bundled 3.53.2.
- Corpus sintetico generato nel repository: 12 PNG, 8/16 bit e trasparenza.

I tempi registrati nello smoke sono misure di una singola esecuzione e non sono benchmark p95.

## Cosa questi risultati non dimostrano

Il laboratorio prova restrizioni concrete di un servizio XPC separato; non qualifica ancora la sandbox del decoder dell’app, un tetto di memoria, la revoca degli handle o l’autenticazione di un firmatario di release. App Sandbox da sola consente il figlio `/usr/bin/true`; il limite aggiuntivo è stato provato soltanto sul Mac indicato. Due connessioni hanno condiviso lo stesso PID. Dettagli e limiti in `experiments/macos-xpc/README.md`.

Non sono prove dell'accessibilità con VoiceOver/NVDA, di Windows, del recupero da device loss, della gestione ICC o della fedeltà del display. Il test GPU non misura ΔE00, LUT, gamut o filtro Lanczos3. Non qualificano RAW, XMP, JPEG/TIFF nativi o gigapixel. La rilocazione riguarda i file necessari al runtime; non prova una ricompilazione della toolchain dopo spostamento. I render rimangono Anteprima e R0 resta aperto.

## Riproduzione

`scripts/verify.sh` esegue build/lint/test e protocollo. `scripts/verify.sh --gui` aggiunge lo smoke nativo. `scripts/build-macos.sh` crea e verifica la firma del bundle; il test LaunchServices è `open -W dist/TrueRenderer.app --args --smoke-test`. Il primo avvio del bundle può richiedere di consentire l'accesso al Desktop nella finestra di macOS.

Il laboratorio XPC usa `python3 scripts/build-xpc-probe.py` e `python3 scripts/test-xpc-probe.py`; ripetere entrambi con `--limited` per la seconda variante. Le suite del laboratorio sono separate dai 23 test Rust e dalle 6 prove IPC del decoder.
