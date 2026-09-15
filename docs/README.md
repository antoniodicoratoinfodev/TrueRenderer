# Documentazione di TrueRenderer

Revisione del 15 settembre 2026. Il codice Windows e le correzioni sono pubblicati in `67146e3`; il restyling locale Mac aggiunge prove UI/corpus/XPC su una copia separata, mentre la matrice estesa dei motori resta aperta. I conteggi nei resoconti storici si riferiscono alle rispettive esecuzioni.

## Documenti correnti

| Documento | Uso |
|---|---|
| [README del progetto](../README.md) | Funzioni, piattaforme, build e limiti |
| [PLAN](../PLAN.md) | Attività, dipendenze e gate aperti |
| [Avanzamento](avanzamento.md) | Registro modificabile di implementazione e verifiche |
| [Architettura](TrueRenderer-Architettura.md) | Specifica; distinguere proposta e stato implementato |
| [Ripresa del lavoro](ripresa-codex.md) | Punto di ripresa e vincoli delle sessioni |
| [Anteprime, cache e prestazioni](progetto-anteprime-cache-prestazioni.md) | Fonte della specifica integrata, con requisiti ancora da qualificare |
| [Motori RAW](progetto-motori-raw.md) | Contratto, ricette e limiti sperimentali |
| [Restyling desktop](progetto-restyling.md) | Composizioni, stile, preferenze e verifica del layout bilingue |
| [Verifiche](../reports/VERIFICA.md) | Cronologia delle campagne e rapporti con hash |

## Decisioni architetturali

Gli ADR vanno conservati: spiegano le decisioni nel loro contesto temporale.

- [0001 — prototipo controllato](adr/0001-prototipo-r0.md)
- [0002 — servizi XPC macOS](adr/0002-xpc-decoder-r0.md)
- [0003 — campionamento fisico](adr/0003-campionamento-fisico-r0.md)
- [0004 — formati esterni macOS e pubblicazione](adr/0004-formati-esterni-e-pubblicazione.md)
- [0005 — cache per cartella](adr/0005-cache-cartella.md)
- [0006 — anteprime, residenza e compute](adr/0006-anteprime-residenza-compute.md)
- [0007 — porta decoder e prima build Windows](adr/0007-porta-decoder-e-build-windows.md)
- [0008 — LibRaw e collegamento](adr/0008-libraw-licenza-e-collegamento.md)
- [0009 — confinamento Windows LPAC](adr/0009-isolamento-worker-windows.md), successivo al primo confine descritto nel piano Windows.

## Revisioni e prove storiche

Conservano problemi riprodotti, correzioni e limiti delle singole campagne. Per i difetti ancora aperti, leggere il registro e le note successive.

| Campagna | Correzioni / esito successivo |
|---|---|
| [Revisione prima di main](revisione-pre-main.md) | Correzioni nella seconda parte dello stesso documento |
| [Revisione serale del 13 settembre](revisione-continuata-2026-09-13.md) | [Correzioni della revisione serale](revisione-continuata-2026-09-13.md#correzioni) |
| [Ricontrollo generale del 14 settembre](revisione-generale-2026-09-14.md) | [Correzioni TIFF, RAW e cache](revisione-generale-2026-09-14.md#correzioni) |
| [Revisione aggiuntiva del 14 settembre](revisione-aggiuntiva-2026-09-14.md) | [Correzioni gamma e orientamento](revisione-aggiuntiva-2026-09-14.md#correzioni) |
| [Verifica dei motori sulla D40](verifica-motori-d40.md) | Supporto D40 e regressione D750 della campagna indicata |

## Accorpamenti eseguiti

| File eliminato | Contenuto conservato in |
|---|---|
| `piano-windows-raw.md` | ADR 0007 (storia e limiti del port), ADR 0005 (cache Windows), PLAN (attività aperte) |
| `licenza-libraw.md` | ADR 0008 (decisione e fonti della nota preliminare) |
| `correzioni-revisione-serale.md` | [Revisione serale, correzioni](revisione-continuata-2026-09-13.md#correzioni) |
| `correzioni-ricontrollo-generale.md` | [Revisione generale, correzioni](revisione-generale-2026-09-14.md#correzioni) |
| `correzioni-gamma-orientamento.md` | [Revisione aggiuntiva, correzioni](revisione-aggiuntiva-2026-09-14.md#correzioni) |

Eliminate dal registro e dal documento di ripresa le cronologie ripetute e le istruzioni di sessione superate. Le campagne restano nei rapporti di verifica; le riproduzioni negative e le correzioni sono ora nello stesso documento. PLAN conserva le caselle operative. I file originali delle versioni pubblicate restano recuperabili dalla storia Git.

## Copie e supporti da mantenere

- [TrueVision-Architettura nella radice](../TrueVision-Architettura.md): copia richiesta dal flusso del titolare e dal sincronizzatore; non eliminabile nel flusso attuale.
- [Originale v1.2](TrueVision-Architettura.originale-v1.2.md): backup immutabile, da non allineare al codice corrente.
- [AGENTS](../AGENTS.md): istruzioni operative. Aggiornare registro e fonti della specifica, poi eseguire `python3 scripts/sync-docs.py`, senza modificare manualmente le copie dei blocchi generati.
- [Laboratorio XPC](../experiments/macos-xpc/README.md): esperimento iniziale, distinto dal bundle corrente.
- [Fixture decoder](../crates/tr-worker/tests/fixtures/README.md): input sintetici delle regressioni.
- [Modello di segnalazione](../.github/ISSUE_TEMPLATE/bug_report.md): supporto alle issue GitHub.
- [NOTICE](../NOTICE.md), [LICENSE](../LICENSE), [README LibRaw](../third_party/libraw/README-TrueRenderer.md) e notice delle dipendenze: attribuzioni e informazioni da conservare.

Dopo gli accorpamenti restano 30 Markdown di progetto e 31 di supporto alle dipendenze. Controllati riferimenti e percorsi locali, sezioni accorpate e sincronizzazione. Licenze, rapporti JSON e backup originale sono conservati; i termini legali non sono stati riesaminati.
